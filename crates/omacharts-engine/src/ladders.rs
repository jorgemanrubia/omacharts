//! Volume at price: what traded at each price, on which side, minute by minute.
//!
//! The model itself — ladders, minutes, days, what a chart holds and how it
//! is stored — is `omacharts-ticks`, re-exported here whole. What stays is
//! what needs the rest of the engine: [`Days`], which cuts trading days the
//! way the chart's bars do, from the exchange calendar, and footprints
//! folded along the chart's [`Grid`].

use std::cell::Cell;

use chrono::NaiveDate;

pub use omacharts_ticks::ladders::*;

use crate::bars::{bucket_of, Grid, Timeframe};
use crate::exchanges::{registry, Schedule};
use crate::symbols::{Instrument, InstrumentKind};

/// Where trading days begin, the way the chart's bars already cut them.
///
/// The exchange's own calendar when its hours are known — a CME equity-index
/// day is named for the date it ends on and starts at 17:00 Chicago the
/// evening before — and otherwise midnight UTC moved by the instrument's
/// `session_origin`, as [`Grid`] does. FX and crypto are cut at the origin,
/// as there.
///
/// A day is numbered as its **trading date in days since 1970-01-01**: a
/// date rather than the instant the session starts, so a day keeps its name
/// across daylight saving and reads back as a date in the store.
#[derive(Clone, Debug)]
pub struct Days {
    schedule: Option<Schedule<'static>>,
    origin: i64,
    /// The last day asked about: where it starts and ends, and its number.
    last: Cell<Option<(i64, i64, i64)>>,
}

const SECONDS_PER_DAY: i64 = 86_400;

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or_default()
}

impl Days {
    pub fn new(instrument: &Instrument) -> Days {
        let schedule = match instrument.kind {
            InstrumentKind::Fx | InstrumentKind::Crypto => None,
            _ => registry().for_instrument(instrument),
        };
        Days { schedule, origin: instrument.session_origin, last: Cell::new(None) }
    }

    /// Days cut every 24 hours at `origin` seconds past midnight UTC.
    pub fn fixed(origin: i64) -> Days {
        Days { schedule: None, origin, last: Cell::new(None) }
    }

    /// The trading day `ts` belongs to.
    pub fn of(&self, ts: i64) -> i64 {
        if let Some((start, end, day)) = self.last.get()
            && (start..end).contains(&ts)
        {
            return day;
        }
        let found = match self.schedule.and_then(|s| s.trading_date(ts).map(|date| (s, date))) {
            Some((schedule, date)) => {
                let end = date.succ_opt().map_or(i64::MAX, |next| schedule.day_start(next));
                (schedule.day_start(date), end, (date - epoch()).num_days())
            }
            None => {
                let start = (ts - self.origin).div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY + self.origin;
                // Named, like an exchange's, for the date its middle falls on:
                // a session opening at 22:00 UTC is the next day's.
                let day = (start + SECONDS_PER_DAY / 2).div_euclid(SECONDS_PER_DAY);
                (start, start + SECONDS_PER_DAY, day)
            }
        };
        self.last.set(Some(found));
        found.2
    }

    /// Where trading day `day` begins, unix seconds.
    pub fn start(&self, day: i64) -> i64 {
        match self.schedule {
            Some(schedule) => schedule.day_start(epoch() + chrono::Duration::days(day)),
            None => {
                // The one start of the day whose middle falls on `day`.
                let noon = day * SECONDS_PER_DAY + SECONDS_PER_DAY / 2;
                (noon - 1 - self.origin).div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY + self.origin
            }
        }
    }
}


impl TradingDays for Days {
    #[inline]
    fn of(&self, ts: i64) -> i64 {
        Days::of(self, ts)
    }
}

/// Footprints along the chart's grid.
pub trait GridBars {
    /// The minutes opening in `from..to` folded into bars of `timeframe`
    /// along `grid`, each with its ladder: footprints. One allocation per
    /// bar, none per minute.
    fn bars(&self, from: i64, to: i64, timeframe: Timeframe, grid: &Grid) -> Vec<LadderBar>;
}

impl GridBars for Series {
    fn bars(&self, from: i64, to: i64, timeframe: Timeframe, grid: &Grid) -> Vec<LadderBar> {
        self.bars_by(from, to, |ts| bucket_of(ts, timeframe, grid))
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;
    use omacharts_studies::footprint::point_of_control;

    /// A small deterministic generator, so a failing case can be run again.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    const TICK: f64 = 0.25;

    /// A minute of random prints around `center`.
    fn minute(rng: &mut Rng, ts: i64, center: i32) -> Minute {
        let mut m = Minute::opening(ts, to_price(center, TICK));
        for _ in 0..1 + rng.below(40) {
            let tick = center + rng.below(21) as i32 - 10;
            let side = match rng.below(10) {
                0 => Side::Unknown,
                1..=4 => Side::Sell,
                _ => Side::Buy,
            };
            m.add_trade(tick, to_price(tick, TICK), 1 + rng.below(20), side, 1);
        }
        m
    }

    /// `n` days of minutes, with holes, cut by `days`.
    fn series(rng: &mut Rng, days: &Days, start: i64, n: i64) -> Series {
        let mut series = Series::new(TICK);
        let mut center = 30_000;
        for i in 0..n * 1440 {
            if rng.below(5) == 0 {
                continue;
            }
            center += rng.below(5) as i32 - 2;
            series.insert(days, minute(rng, start + i * 60, center));
        }
        series
    }

    fn trimmed(ladder: &Ladder) -> Ladder {
        let mut ladder = ladder.clone();
        ladder.trim();
        ladder
    }

    fn brute(series: &Series, from: i64, to: i64) -> Ladder {
        let mut sum = Ladder::new();
        for m in series.minutes().filter(|m| (from..to).contains(&m.ts)) {
            sum.add(m);
        }
        sum
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).single().unwrap().timestamp()
    }

    fn day_number(y: i32, mo: u32, d: u32) -> i64 {
        (NaiveDate::from_ymd_opt(y, mo, d).unwrap() - epoch()).num_days()
    }

    fn es() -> Instrument {
        Instrument {
            symbol: "ES".into(),
            name: "E-mini S&P 500".into(),
            kind: InstrumentKind::FutureRoot,
            suffix: None,
            currency: Some("USD".into()),
            tier: 0,
            session_origin: 79_200,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
            formula: None,
        }
    }

    #[test]
    fn prices_round_to_the_nearest_tick() {
        assert_eq!(to_tick(7625.25, 0.25), 30501);
        assert_eq!(to_tick(7625.2499999, 0.25), 30501);
        assert_eq!(to_tick(7625.2500001, 0.25), 30501);
        assert_eq!(to_tick(1.3, 0.1), 13, "1.3 / 0.1 is 12.999…");
        assert_eq!(to_tick(-0.25, 0.25), -1);
        assert_eq!(to_price(30501, 0.25), 7625.25);
    }

    #[test]
    fn a_cme_day_starts_the_evening_before_and_is_named_for_the_next() {
        let days = Days::new(&es());
        // 2024-03-12 is in US daylight time: 17:00 in Chicago is 22:00 UTC.
        assert_eq!(days.of(at(2024, 3, 12, 22, 0)), day_number(2024, 3, 13));
        assert_eq!(days.of(at(2024, 3, 12, 21, 59)), day_number(2024, 3, 12));
        assert_eq!(days.of(at(2024, 3, 13, 14, 30)), day_number(2024, 3, 13));
        assert_eq!(days.start(day_number(2024, 3, 13)), at(2024, 3, 12, 22, 0));
        // And in winter, an hour later in UTC.
        assert_eq!(days.start(day_number(2024, 1, 10)), at(2024, 1, 9, 23, 0));
    }

    #[test]
    fn a_fixed_origin_names_days_the_same_way() {
        let days = Days::fixed(79_200);
        assert_eq!(days.of(at(2024, 3, 12, 22, 0)), day_number(2024, 3, 13));
        assert_eq!(days.of(at(2024, 3, 12, 21, 59)), day_number(2024, 3, 12));
        assert_eq!(days.start(day_number(2024, 3, 13)), at(2024, 3, 12, 22, 0));
        let midnight = Days::fixed(0);
        assert_eq!(midnight.of(at(2024, 3, 12, 23, 59)), day_number(2024, 3, 12));
        assert_eq!(midnight.start(day_number(2024, 3, 12)), at(2024, 3, 12, 0, 0));
    }

    #[test]
    fn prints_land_on_their_side_and_unknown_ones_at_their_price_on_neither() {
        let mut m = Minute::opening(0, 100.0);
        m.add_trade(400, 100.0, 5, Side::Buy, 1);
        m.add_trade(398, 99.5, 3, Side::Sell, 1);
        m.add_trade(402, 100.5, 2, Side::Unknown, 1);
        assert_eq!(m.volume, 10);
        assert_eq!((m.high, m.low, m.close), (100.5, 99.5, 100.5));
        assert_eq!(m.ladder.low_tick, 398);
        assert_eq!(m.ladder.levels.len(), 5, "the unknown print takes its price");
        assert_eq!(m.ladder.at(400).at_ask, 5);
        assert_eq!(m.ladder.at(398).at_bid, 3);
        let unknown = m.ladder.at(402);
        assert_eq!((unknown.at_bid, unknown.at_ask, unknown.unknown, unknown.volume()), (0, 0, 2, 2));
        let s = m.ladder.sides;
        assert_eq!((s.at_bid, s.at_ask, s.unknown, s.volume(), s.delta()), (3, 5, 2, 10, 2));
        assert_eq!((s.bid_trades, s.ask_trades, s.unknown_trades), (1, 1, 1));
        assert_eq!(point_of_control(m.ladder.low_tick, &m.ladder.levels, 1, Level::volume), Some(400));
        let prices: Vec<_> = m.ladder.prices(0.25).collect();
        assert_eq!(prices, vec![(99.5, 3, 0), (99.75, 0, 0), (100.0, 0, 5), (100.25, 0, 0), (100.5, 0, 0)]);
    }

    /// A feed that never says who took liquidity still has a volume at
    /// every price it traded, and no delta at all.
    #[test]
    fn trades_with_no_aggressor_have_volume_at_price_and_no_delta() {
        let days = Days::fixed(0);
        let mut s = Series::new(TICK);
        for i in 0..10 {
            let mut m = Minute::opening(i * 60, to_price(400, TICK));
            m.add_trade(400, to_price(400, TICK), 4, Side::Unknown, 1);
            m.add_trade(401 + i as i32 % 2, to_price(401, TICK), 1, Side::Unknown, 1);
            s.insert(&days, m);
        }
        let ladder = s.volume_at_price(0, 600);
        assert_eq!((ladder.low_tick, ladder.levels.len()), (400, 3));
        let volumes: Vec<u64> = ladder.levels.iter().map(Level::volume).collect();
        assert_eq!(volumes, vec![40, 5, 5]);
        assert_eq!(point_of_control(ladder.low_tick, &ladder.levels, 1, Level::volume), Some(400));
        assert_eq!((ladder.total(), ladder.delta()), (50, 0));
        assert_eq!((ladder.sides.at_bid, ladder.sides.at_ask, ladder.sides.unknown), (0, 0, 50));
        for bar in s.bars_by(0, 600, |ts| ts.div_euclid(300) * 300) {
            assert_eq!((bar.volume, bar.ladder.delta()), (25, 0));
            assert_eq!(bar.ladder.levels.iter().map(Level::volume).sum::<u64>(), 25);
        }
        // Taken back out, a day's total drops the prices it leaves empty.
        let mut day = s.days[0].clone();
        day.remove(0, 600);
        assert!(day.total.levels.is_empty());
    }

    #[test]
    fn inserting_and_replacing_keeps_the_day_total_right() {
        let mut rng = Rng(7);
        let mut day = Day::new(1);
        for i in 0..200 {
            day.insert(minute(&mut rng, i * 60, 1000 + (i as i32 % 30)));
        }
        // Replace every third, insert some out of order, and take a stretch out.
        for i in (0..200).step_by(3) {
            day.insert(minute(&mut rng, i * 60, 900));
        }
        day.insert(minute(&mut rng, 30, 1200));
        day.remove(50 * 60, 60 * 60);
        assert!(day.minutes.windows(2).all(|w| w[0].ts < w[1].ts));
        let mut summed = day.clone();
        summed.resum();
        assert_eq!(trimmed(&day.total), trimmed(&summed.total));
        assert_eq!(day.total.sides, summed.total.sides);
    }

    #[test]
    fn a_range_is_the_brute_force_sum_of_its_minutes() {
        let mut rng = Rng(42);
        let days = Days::new(&es());
        let start = at(2024, 3, 10, 20, 0);
        let series = series(&mut rng, &days, start, 6);
        assert!(series.days.len() >= 6);
        let end = start + 6 * 86_400;
        let mut cases = vec![(start - 999, end + 999), (start, start), (end, start)];
        for _ in 0..200 {
            let a = start + rng.below((end - start) as u64) as i64;
            let b = a + rng.below(3 * 86_400) as i64;
            cases.push((a, b));
        }
        for (from, to) in cases {
            let fast = series.volume_at_price(from, to);
            let slow = brute(&series, from, to);
            assert_eq!(trimmed(&fast), trimmed(&slow), "{from}..{to}");
            assert_eq!(fast.sides, slow.sides);
            assert_eq!(series.side_totals(from, to), slow.sides);
        }
    }

    #[test]
    fn replacing_minutes_across_a_series_keeps_ranges_exact() {
        let mut rng = Rng(9);
        let days = Days::fixed(0);
        let mut s = series(&mut rng, &days, 0, 3);
        for _ in 0..500 {
            let ts = rng.below(3 * 1440) as i64 * 60;
            s.insert(&days, minute(&mut rng, ts, 29_000));
        }
        s.remove(86_000, 90_000);
        for (from, to) in [(0, 3 * 86_400), (40_000, 200_000)] {
            assert_eq!(trimmed(&s.volume_at_price(from, to)), trimmed(&brute(&s, from, to)));
        }
    }

    #[test]
    fn footprints_are_the_sum_of_their_minutes() {
        let mut rng = Rng(3);
        let days = Days::fixed(0);
        let s = series(&mut rng, &days, 0, 2);
        let bars = s.bars_by(3_600, 100_000, |ts| ts.div_euclid(300) * 300);
        assert!(!bars.is_empty());
        for bar in &bars {
            let (from, to) = (bar.ts.max(3_600), (bar.ts + 300).min(100_000));
            let slow = brute(&s, from, to);
            assert_eq!(trimmed(&bar.ladder), trimmed(&slow));
            assert_eq!(bar.volume, slow.sides.volume());
            let mins: Vec<_> = s.minutes().filter(|m| (from..to).contains(&m.ts)).collect();
            assert_eq!(bar.open, mins[0].open);
            assert_eq!(bar.close, mins[mins.len() - 1].close);
        }
    }

    #[test]
    fn the_forming_minute_seals_when_the_next_one_trades() {
        const N: i64 = 1_000_000_000;
        let mut f = Forming::new(TICK);
        f.connect(10 * N); // part way through minute 0
        assert!(f.add_trade(15, 100.0, 2, Side::Buy).is_none());
        assert!(f.add_trade(20, 99.75, 1, Side::Sell).is_none());
        assert!(f.add_trade(59, 100.25, 3, Side::Buy).is_none());
        assert_eq!(f.covered(), None, "minute 0 was joined late");
        let sealed = f.add_trade(61, 100.0, 4, Side::Unknown).unwrap();
        assert_eq!((sealed.ts, sealed.open, sealed.high, sealed.low, sealed.close), (0, 100.0, 100.25, 99.75, 100.25));
        assert_eq!(sealed.volume, 6);
        assert_eq!(sealed.ladder.sides.at_ask, 5);
        assert_eq!(f.current().map(|m| m.ts), Some(60));
        // Quiet minutes 2 and 3 are complete; the print in minute 4 seals 1.
        assert!(f.add_trade(250, 100.0, 1, Side::Buy).is_some());
        assert_eq!(f.covered(), Some((60, 240)));
        assert_eq!(f.seal_by(299), None);
        assert_eq!(f.seal_by(300).map(|m| m.ts), Some(240));
        // A disconnect hands back what was forming and covers nothing more.
        f.add_trade(301, 100.0, 1, Side::Buy);
        assert_eq!(f.disconnect().map(|m| m.ts), Some(300));
        assert!(!f.is_connected());
        assert_eq!(f.covered(), None);
    }

    #[test]
    fn a_day_round_trips_through_its_encoding() {
        let mut rng = Rng(11);
        let days = Days::fixed(0);
        let s = series(&mut rng, &days, 0, 1);
        let day = &s.days[0];
        let bytes = encode_day(day, TICK);
        assert_eq!(bytes[0], ENCODING_VERSION);
        assert_eq!(&decode_day(&bytes, TICK).unwrap(), day);
        let total = encode_ladder(&day.total);
        assert_eq!(decode_day_with_total(&bytes, &total, TICK).unwrap(), *day);
        let m = &day.minutes[3];
        assert_eq!(&decode_minute(&encode_minute(m, TICK), TICK).unwrap(), m);
    }

    /// Volume with no side at a price round-trips, and costs a day with
    /// none of it nothing: its blob is laid out as version 1's was.
    #[test]
    fn volume_with_no_side_round_trips_and_is_written_only_when_there_is_some() {
        let mut m = Minute::opening(60, 100.0);
        m.add_trade(400, 100.0, 3, Side::Buy, 1);
        m.add_trade(401, 100.25, 2, Side::Unknown, 1);
        m.ladder.sides.unknown += 4; // reported with no price
        m.volume += 4;
        let day = Day { day: 0, total: m.ladder.clone(), minutes: vec![m.clone()] };
        let bytes = encode_day(&day, TICK);
        assert_eq!(bytes[1] & FLAG_UNKNOWN, FLAG_UNKNOWN);
        assert_eq!(decode_day(&bytes, TICK).unwrap(), day);
        assert_eq!(decode_ladder(&encode_ladder(&day.total)).unwrap(), day.total);
        assert_eq!(decode_minute(&encode_minute(&m, TICK), TICK).unwrap(), m);

        let mut sided = Minute::opening(60, 100.0);
        sided.add_trade(400, 100.0, 3, Side::Buy, 1);
        assert_eq!(encode_minute(&sided, TICK)[1] & FLAG_UNKNOWN, 0);
    }

    /// Ladders stored before a price kept its volume with no side are read
    /// as they were written: that volume in the totals, at no price.
    #[test]
    fn version_1_blobs_still_decode() {
        // One minute at 60 s: low tick 400, OHLC all on it, one level of
        // 3 bid and 5 ask with a trade count each side, and 2 contracts
        // in one print with no side.
        let v1 = [1, FLAG_COUNTS, 120, 0xa0, 0x06, 0, 0, 0, 0, 1, 2, 1, 3, 5, 1, 2];
        let m = decode_minute(&v1, TICK).unwrap();
        assert_eq!((m.ts, m.open, m.close, m.volume), (60, 100.0, 100.0, 10));
        assert_eq!(m.ladder.levels, vec![Level { at_bid: 3, at_ask: 5, unknown: 0, bid_trades: 1, ask_trades: 2 }]);
        let s = m.ladder.sides;
        assert_eq!((s.at_bid, s.at_ask, s.unknown, s.unknown_trades, s.delta()), (3, 5, 2, 1, 2));

        // A whole day and a total in version 1 are version 2's bytes with
        // no per-price unknown, under the old version byte.
        let mut rng = Rng(5);
        let mut day = series(&mut rng, &Days::fixed(0), 0, 1).days.remove(0);
        for minute in &mut day.minutes {
            for level in &mut minute.ladder.levels {
                level.unknown = 0;
            }
        }
        day.resum();
        let mut bytes = encode_day(&day, TICK);
        let mut total = encode_ladder(&day.total);
        assert_eq!((bytes[1] & FLAG_UNKNOWN, total[1] & FLAG_UNKNOWN), (0, 0));
        bytes[0] = 1;
        total[0] = 1;
        assert_eq!(decode_day_with_total(&bytes, &total, TICK).unwrap(), day);
        assert_eq!(decode_day(&[9, 0], TICK), Err(DecodeError::Version(9)));
    }

    #[test]
    fn side_trade_counts_survive_without_per_price_counts() {
        let mut m = Minute::opening(60, 10.0);
        m.add_trade(40, 10.0, 7, Side::Buy, 0);
        m.ladder.sides.ask_trades = 3; // a provider that sent totals alone
        let day = Day { day: 0, total: m.ladder.clone(), minutes: vec![m] };
        let back = decode_day(&encode_day(&day, TICK), TICK).unwrap();
        assert_eq!(back, day);
        assert_eq!(back.minutes[0].ladder.sides.ask_trades, 3);

        // And dropped on request, the side totals still stand.
        let mut rng = Rng(5);
        let s = series(&mut rng, &Days::fixed(0), 0, 1);
        let with = encode_day(&s.days[0], TICK);
        let without = encode_day_as(&s.days[0], TICK, false);
        assert!(without.len() < with.len());
        let back = decode_day(&without, TICK).unwrap();
        assert_eq!(back.total.sides, s.days[0].total.sides);
    }

    #[test]
    fn a_damaged_blob_is_refused_not_misread() {
        let mut rng = Rng(13);
        let s = series(&mut rng, &Days::fixed(0), 0, 1);
        let bytes = encode_day(&s.days[0], TICK);
        assert_eq!(decode_day(&bytes[..bytes.len() / 2], TICK), Err(DecodeError::Truncated));
        let mut future = bytes.clone();
        future[0] = 99;
        assert_eq!(decode_day(&future, TICK), Err(DecodeError::Version(99)));
        assert!(decode_day(&[], TICK).is_err());
        assert!(decode_ladder(&[1, 0, 0, 0xff, 0xff, 0xff, 0xff, 0x0f]).is_err());
    }

    /// The days of `series` by their totals alone, as a chart first holds
    /// them, trusted from its first minute.
    fn by_totals(series: &Series) -> Held {
        let days = series
            .days
            .iter()
            .map(|d| HeldDay::total(d.day, d.first_ts().unwrap(), d.last_ts().unwrap(), d.total.clone()))
            .collect::<Vec<_>>();
        let floor = days.first().map_or(i64::MAX, |d| d.first_ts);
        Held { tick_size: series.tick_size, days, floor, floor_day: i64::MIN, ..Held::default() }
    }

    /// A chart holds day totals and loads a day's minutes only when a span
    /// cuts it; what it answers is what summing every minute answers, and a
    /// span it cannot answer whole it does not answer at all.
    #[test]
    fn a_chart_answers_from_day_totals_and_loads_only_the_days_it_cuts() {
        let instrument = es();
        let days = Days::new(&instrument);
        let mut rng = Rng(0x5eed_1ad);
        let start = at(2026, 10, 12, 0, 0);
        let series = series(&mut rng, &days, start, 5);
        let mut held = by_totals(&series);
        let floor = held.floor;

        // Whole days: their totals, no minutes needed.
        let (whole_from, whole_to) = (days.start(series.days[1].day), days.start(series.days[3].day));
        assert!(held.cut(whole_from, whole_to).is_empty());
        assert_eq!(trimmed(&held.between(whole_from, whole_to).unwrap()), trimmed(&brute(&series, whole_from, whole_to)));

        // A span that cuts two days asks for exactly those two, and is
        // answered once they are in.
        let (from, to) = (whole_from + 3 * 3_600 + 60 * 7, whole_to + 5 * 3_600);
        assert_eq!(held.between(from, to), None);
        let cut = held.cut(from, to);
        assert_eq!(cut, vec![series.days[1].day, series.days[3].day]);
        for number in cut {
            held.load(series.days.iter().find(|d| d.day == number).unwrap().clone());
        }
        assert_eq!(trimmed(&held.between(from, to).unwrap()), trimmed(&brute(&series, from, to)));
        assert_eq!(held.days.iter().filter(|d| d.whole).count(), 2, "only the days it cut");

        // Before the floor nothing is known whole.
        assert_eq!(held.between(floor - 60, floor + 600), None);

        // Live: a sealed minute joins its day, and the forming one counts
        // until it is sealed, once.
        let last = series.days.last().unwrap();
        let next = last.last_ts().unwrap() + 60;
        let mut forming = Minute::opening(next, 7_500.0);
        forming.add_trade(30_000, 7_500.0, 5, Side::Buy, 1);
        held.forming = Some(forming.clone());
        let everything = |held: &Held| held.between(floor, next + 60).unwrap().sides.volume();
        let before = brute(&series, floor, next).sides.volume();
        assert_eq!(everything(&held), before + 5);
        held.seal(days.of(next), forming);
        assert_eq!(held.forming, None);
        assert_eq!(everything(&held), before + 5);
    }

    /// A span that cuts days held by their totals asks for the pieces it
    /// cuts, not the days; with them in it answers what summing every
    /// minute answers, and holds no day's minutes. A day that comes whole,
    /// or takes a minute while held by its total, lets its pieces go.
    #[test]
    fn a_span_that_cuts_days_is_answered_from_their_pieces() {
        let instrument = es();
        let days = Days::new(&instrument);
        let mut rng = Rng(0x0b1e_c7ed);
        let series = series(&mut rng, &days, at(2026, 10, 12, 0, 0), 5);
        let mut held = by_totals(&series);
        let (from, to) = (days.start(series.days[1].day) + 3 * 3_600 + 420, days.start(series.days[3].day) + 5 * 3_600);

        assert_eq!(held.between(from, to), None);
        let wanted = held.take_wanted();
        assert_eq!(wanted.iter().map(|p| p.day).collect::<Vec<_>>(), vec![series.days[3].day, series.days[1].day]);
        assert!(held.take_wanted().is_empty(), "taken");
        for piece in &wanted {
            let day = series.days.iter().find(|d| d.day == piece.day).unwrap();
            assert!(held.put_piece(*piece, day.piece(piece.from, piece.to)));
        }
        assert_eq!(trimmed(&held.between(from, to).unwrap()), trimmed(&brute(&series, from, to)));
        assert!(held.days.iter().all(|d| !d.whole), "no day's minutes");
        assert_eq!(held.take_used().len(), 2, "both drawn from");
        assert!(held.take_wanted().is_empty());

        // A day come whole answers from its minutes; its piece goes.
        held.load(series.days[1].clone());
        assert_eq!(held.pieces.len(), 1);
        assert!(!held.put_piece(wanted[1], Ladder::new()), "not for a whole day");
        assert_eq!(trimmed(&held.between(from, to).unwrap()), trimmed(&brute(&series, from, to)));

        // A minute sealed into a day held by its total, after the span: the
        // piece stands as it was.
        let last = series.days[3].last_ts().unwrap();
        held.seal(series.days[3].day, Minute::opening(last + 60, 7_500.0));
        assert_eq!(held.pieces.len(), 1);
        assert_eq!(trimmed(&held.between(from, to).unwrap()), trimmed(&brute(&series, from, to)));
    }

    /// A period running on past the newest minute, as one covering today
    /// does, keeps the one piece of today it read while minutes seal onto
    /// the day: each goes into the piece, so the period is answered every
    /// minute, never left waiting for today to be read again, and always
    /// as summing every minute answers it.
    #[test]
    fn a_period_past_the_newest_minute_keeps_its_piece_as_minutes_seal() {
        let instrument = es();
        let days = Days::new(&instrument);
        let mut rng = Rng(0x70_da7);
        let mut series = series(&mut rng, &days, at(2026, 10, 12, 0, 0), 3);
        let mut held = by_totals(&series);
        let today = series.days.last().unwrap().clone();
        let (first, last) = (today.first_ts().unwrap(), today.last_ts().unwrap());
        let from = first + (last - first) / 2 / 60 * 60;
        let to = last + 3_600;
        assert!(first < from && from < last);

        assert_eq!(held.between(from, to), None);
        let wanted = held.take_wanted();
        assert_eq!(wanted, vec![Piece { day: today.day, from, to: i64::MAX }], "open past the day's last minute");
        assert!(held.put_piece(wanted[0], today.piece(wanted[0].from, wanted[0].to)));

        let mut ts = today.last_ts().unwrap();
        for k in 0..5i32 {
            ts += 60;
            let mut minute = Minute::opening(ts, 7_500.0);
            minute.add_trade(30_000 + k, 7_500.0 + f64::from(k) * 0.25, 3 + k as u64, Side::Sell, 1);
            series.insert_minute(today.day, minute.clone());
            held.seal(today.day, minute);
            let answer = held.between(from, to).expect("answered with no read of the day");
            assert_eq!(trimmed(&answer), trimmed(&brute(&series, from, to)));
            assert!(held.take_wanted().is_empty(), "nothing asked again");
        }
        assert_eq!(held.pieces.len(), 1, "one piece, however far the minutes run");
    }

    /// Spans counted in days, for daily bars: the totals of the days named,
    /// and nothing before the first day held from its start.
    #[test]
    fn whole_days_are_their_totals() {
        let instrument = es();
        let days = Days::new(&instrument);
        let mut rng = Rng(77);
        let series = series(&mut rng, &days, at(2026, 10, 12, 0, 0), 4);
        let mut held = by_totals(&series);
        let (a, b) = (series.days[1].day, series.days[3].day);
        let expected = brute(&series, days.start(a), days.start(b));
        assert_eq!(trimmed(&held.days_between(a, b, &days).unwrap()), trimmed(&expected));
        held.floor_day = a + 1;
        assert_eq!(held.days_between(a, b, &days), None);
    }
}
