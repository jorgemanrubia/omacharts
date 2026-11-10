//! Cumulative volume delta: the running sum of what buyers took at the ask
//! less what sellers took at the bid, started again at each reset period.
//!
//! **Two ways of knowing it.** Where the chart's source has volume at price
//! (Sierra Chart's `.scid`, Rithmic), each bar's delta is the ask volume
//! less the bid volume of the minutes it holds, read from the same minute
//! ladders a volume profile's delta is. Where it has none, or for a bar from
//! before the ladders begin, it is estimated from the bar itself, the way
//! TradingView estimates it without a lower timeframe: the whole volume of
//! a bar that closed above its open counts as bought, below as sold, and a
//! bar that closed where it opened takes the side of its close against the
//! close before it, or none if that did not move either. The estimate is
//! taken from the chart's own bars, not a finer series: a second series
//! threaded through every chart that shows one is a second fetch, a second
//! cache and a second stream, for an estimate.
//!
//! **Each bar a candle.** A bar's open is the running delta it started
//! from and its close the running delta it ended on. Read from ladders, its
//! high and low are the furthest the running delta went minute by minute in
//! between, so a bar of five minutes or more has wicks and a one-minute bar
//! is a body alone: a minute is the grain the ladders are kept in. A bar of
//! a day or longer is read from whole days' totals and is a body alone too.
//! An estimated bar is all one way, and is a body alone as well.
//!
//! **Live.** A chart computes its indicators again on every tick. This one
//! keeps what it worked out last ([`Memo`]) and starts again only from the
//! first bar that changed — on a live chart the last one or two — unless
//! what was held of the past changed underneath it.

use std::cell::RefCell;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::periods::Reset;
use crate::bars::{Bar, Timeframe};
use crate::ladders::{Days, Held};

/// How each bar's running delta is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// A candle from the running delta it opened at to the one it closed
    /// at, with the furthest it went either way between.
    #[default]
    Candles,
    /// The running delta at each bar's close, joined up.
    Line,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Candles, Style::Line];

    pub fn label(self) -> &'static str {
        match self {
            Style::Candles => "Candles",
            Style::Line => "Line",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Style::Candles => "candles",
            Style::Line => "line",
        }
    }

    pub fn from_key(key: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|style| style.key() == key)
    }
}

/// What the Reset every row says for a delta that never starts again.
pub const NEVER: &str = "Never";

/// One bar of the running delta.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct DeltaBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Estimated from the bar's direction rather than read from what
    /// traded at the bid and the ask.
    pub estimated: bool,
    /// The first bar of its reset period, which starts from nought.
    pub starts: bool,
}

/// Where a chart's volume at price comes from, for a delta read from it.
pub struct Source<'a> {
    pub held: &'a Held,
    /// How the instrument's trading days are cut, for bars a day or longer.
    pub days: &'a Days,
}

/// What a computation leaves for the next one on the same chart.
#[derive(Clone, Debug, Default)]
pub struct Memo {
    key: Option<Key>,
    bars: Vec<Bar>,
    out: Vec<DeltaBar>,
}

/// Everything besides the bars that the bars' deltas depend on.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Key {
    reset: Option<Reset>,
    timeframe: Timeframe,
    session_origin: i64,
    /// The held ladders' generation, or `None` for a chart with none.
    generation: Option<u64>,
}

/// Each chart's memos, by the indicator's id.
#[derive(Debug, Default)]
pub struct Memos(RefCell<HashMap<u32, Memo>>);

impl Memos {
    pub fn new() -> Memos {
        Memos::default()
    }

    /// Run `work` with the memo kept for indicator `id`.
    pub fn with<T>(&self, id: u32, work: impl FnOnce(&mut Memo) -> T) -> T {
        let mut memos = self.0.borrow_mut();
        work(memos.entry(id).or_default())
    }

    /// Forget every memo but those of `ids`: an indicator taken off.
    pub fn keep(&self, ids: &[u32]) {
        self.0.borrow_mut().retain(|id, _| ids.contains(id));
    }
}

/// A bar's volume with the side its direction gives it: bought when it
/// closed above its open, sold below, and for one that closed where it
/// opened, the side of its close against `previous`, or nought.
pub fn estimate(bar: &Bar, previous: Option<f64>) -> f64 {
    let volume = if bar.volume.is_finite() { bar.volume.max(0.0) } else { 0.0 };
    let sign = |a: f64, b: f64| if a > b { 1.0 } else if a < b { -1.0 } else { 0.0 };
    let side = match sign(bar.close, bar.open) {
        0.0 => previous.map_or(0.0, |previous| sign(bar.close, previous)),
        side => side,
    };
    side * volume
}

/// The running delta of `bars`, starting again at each `reset` period
/// (never, for `None`), read from `source` where it can be and estimated
/// where it cannot. `memo`, when given, is what the last computation on this
/// chart left, and is left for the next.
pub fn compute(
    bars: &[Bar],
    reset: Option<Reset>,
    session_origin: i64,
    timeframe: Timeframe,
    source: Option<&Source>,
    memo: Option<&mut Memo>,
) -> Vec<DeltaBar> {
    let key = Key { reset, timeframe, session_origin, generation: source.map(|s| s.held.generation) };
    let mut scratch = Memo::default();
    let memo = memo.unwrap_or(&mut scratch);
    let start = match memo.key {
        Some(kept) if kept == key => {
            let same = memo.bars.iter().zip(bars).take_while(|(a, b)| a == b).count();
            // Ladders move under the newest bars without the bars moving:
            // the forming minute, and the minute that sealed into the bar
            // before at a boundary.
            let live = if source.is_some() { memo.out.len().saturating_sub(2) } else { memo.out.len() };
            same.min(live).min(memo.out.len())
        }
        _ => 0,
    };
    memo.key = Some(key);
    memo.bars.truncate(start);
    memo.bars.extend_from_slice(&bars[start..]);
    memo.out.truncate(start);

    let period = |ts: i64| reset.map_or(0, |reset| reset.bucket(ts, session_origin));
    let mut running = start.checked_sub(1).map_or(0.0, |at| memo.out[at].close);
    for at in start..bars.len() {
        let bar = &bars[at];
        let previous = at.checked_sub(1).map(|before| &bars[before]);
        let starts = previous.is_none_or(|before| period(before.ts) != period(bar.ts));
        if starts {
            running = 0.0;
        }
        let open = running;
        let read = source.and_then(|source| read(bar, open, timeframe, source));
        let delta = match read {
            Some(delta) => delta,
            None => {
                let close = open + estimate(bar, previous.map(|p| p.close));
                DeltaBar { open, high: open.max(close), low: open.min(close), close, estimated: true, starts }
            }
        };
        running = delta.close;
        memo.out.push(DeltaBar { starts, ..delta });
    }
    memo.out.clone()
}

/// One bar's running delta read from what traded at the bid and the ask,
/// from `open` on, or `None` when the ladders cannot tell it whole.
fn read(bar: &Bar, open: f64, timeframe: Timeframe, source: &Source) -> Option<DeltaBar> {
    let (mut high, mut low, mut close) = (open, open, open);
    if timeframe.is_intraday() {
        let whole = source.held.minute_deltas(bar.ts, bar.ts + timeframe.seconds(), |_, delta| {
            close += delta as f64;
            high = high.max(close);
            low = low.min(close);
        });
        if !whole {
            return None;
        }
    } else {
        let end = timeframe.shift(bar.ts, 1);
        let delta = source.held.days_delta(bar.ts.div_euclid(86_400), end.div_euclid(86_400), source.days)?;
        close += delta as f64;
        high = high.max(close);
        low = low.min(close);
    }
    Some(DeltaBar { open, high, low, close, estimated: false, starts: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ladders::{to_price, Minute, Series, Side};

    const TICK: f64 = 0.25;
    /// 2026-10-12 00:00 UTC.
    const MIDNIGHT: i64 = 1_791_849_600;

    fn bar(ts: i64, open: f64, close: f64, volume: f64) -> Bar {
        Bar { ts, open, high: open.max(close), low: open.min(close), close, volume }
    }

    fn closes(out: &[DeltaBar]) -> Vec<f64> {
        out.iter().map(|b| b.close).collect()
    }

    /// A rising bar's volume is bought, a falling one's sold, and a bar that
    /// closed where it opened takes the side its close moved from the close
    /// before it, or none.
    #[test]
    fn an_estimate_takes_its_side_from_the_bars_direction() {
        assert_eq!(estimate(&bar(0, 10.0, 11.0, 50.0), None), 50.0);
        assert_eq!(estimate(&bar(0, 11.0, 10.0, 50.0), None), -50.0);
        assert_eq!(estimate(&bar(0, 10.0, 10.0, 50.0), Some(9.0)), 50.0, "a doji above the close before");
        assert_eq!(estimate(&bar(0, 10.0, 10.0, 50.0), Some(11.0)), -50.0, "below it");
        assert_eq!(estimate(&bar(0, 10.0, 10.0, 50.0), Some(10.0)), 0.0, "unchanged either way");
        assert_eq!(estimate(&bar(0, 10.0, 10.0, 50.0), None), 0.0, "nothing before it");
        assert_eq!(estimate(&bar(0, 10.0, 11.0, f64::NAN), None), 0.0);
    }

    /// Estimated bars add up, start from nought at each period, and run on
    /// for ever with no reset.
    #[test]
    fn the_running_delta_starts_again_at_each_period() {
        let day = 86_400;
        let bars = vec![
            bar(MIDNIGHT, 10.0, 11.0, 5.0),
            bar(MIDNIGHT + 60, 11.0, 10.0, 2.0),
            bar(MIDNIGHT + 120, 10.0, 10.0, 4.0),
            bar(MIDNIGHT + day, 10.0, 11.5, 3.0),
            bar(MIDNIGHT + day + 60, 12.0, 12.0, 1.0),
        ];
        let tf = Timeframe::minutes(1);
        let session = compute(&bars, Some(Reset::Session), 0, tf, None, None);
        assert_eq!(closes(&session), vec![5.0, 3.0, 3.0, 3.0, 4.0]);
        assert_eq!(session.iter().map(|b| b.starts).collect::<Vec<_>>(), vec![true, false, false, true, false]);
        assert_eq!(session[3].open, 0.0, "a new session opens at nought");
        assert!(session.iter().all(|b| b.estimated));
        let never = compute(&bars, None, 0, tf, None, None);
        assert_eq!(closes(&never), vec![5.0, 3.0, 3.0, 6.0, 7.0]);
        // A doji with the close before it below: bought.
        assert_eq!(never[4].close - never[4].open, 1.0);
        for b in &never {
            assert_eq!((b.high, b.low), (b.open.max(b.close), b.open.min(b.close)), "an estimate is a body");
        }
    }

    /// A day of minutes, each with its own buying and selling, as a feed
    /// with volume at price sends them.
    fn minutes(deltas: &[(u64, u64)]) -> Series {
        let days = Days::fixed(0);
        let mut series = Series::new(TICK);
        for (i, (bought, sold)) in deltas.iter().enumerate() {
            let ts = MIDNIGHT + 60 * i as i64;
            let mut minute = Minute::opening(ts, to_price(400, TICK));
            if *bought > 0 {
                minute.add_trade(401, to_price(401, TICK), *bought, Side::Buy, 1);
            }
            if *sold > 0 {
                minute.add_trade(399, to_price(399, TICK), *sold, Side::Sell, 1);
            }
            minute.add_trade(400, to_price(400, TICK), 7, Side::Unknown, 1);
            series.insert(&days, minute);
        }
        series
    }

    /// Read from ladders, a bar's delta is its minutes' ask volume less
    /// their bid volume, with no part for a print of no known side, and its
    /// high and low the furthest the running delta went minute by minute.
    #[test]
    fn a_bar_read_from_ladders_has_the_running_deltas_extremes_inside_it() {
        let series = minutes(&[(10, 0), (0, 25), (30, 0), (0, 5), (4, 4), (0, 0)]);
        let held = Held::of(&series);
        let days = Days::fixed(0);
        let source = Source { held: &held, days: &days };
        let tf = Timeframe::minutes(3);
        let bars = vec![bar(MIDNIGHT, 100.0, 90.0, 1.0), bar(MIDNIGHT + 180, 90.0, 80.0, 1.0)];
        let out = compute(&bars, Some(Reset::Session), 0, tf, Some(&source), None);
        // +10, -25, +30: 10, -15, 15.
        assert_eq!(out[0], DeltaBar { open: 0.0, high: 15.0, low: -15.0, close: 15.0, estimated: false, starts: true });
        // -5, 0, 0 from 15.
        assert_eq!(out[1], DeltaBar { open: 15.0, high: 15.0, low: 10.0, close: 10.0, estimated: false, starts: false });
        // Each one-minute bar is a body: the minute is the grain.
        let ones: Vec<Bar> = (0..6).map(|i| bar(MIDNIGHT + 60 * i, 1.0, 1.0, 1.0)).collect();
        let out = compute(&ones, None, 0, Timeframe::minutes(1), Some(&source), None);
        assert_eq!(closes(&out), vec![10.0, -15.0, 15.0, 10.0, 10.0, 10.0]);
        assert!(out.iter().all(|b| !b.estimated && b.high == b.open.max(b.close)));
    }

    /// Bought is up and sold is down, whatever the bars did: a falling bar
    /// that buyers took is a rising delta.
    #[test]
    fn the_sign_is_the_aggressors_and_not_the_bars() {
        let series = minutes(&[(40, 1)]);
        let held = Held::of(&series);
        let days = Days::fixed(0);
        let source = Source { held: &held, days: &days };
        let falling = [bar(MIDNIGHT, 100.0, 90.0, 41.0)];
        let out = compute(&falling, None, 0, Timeframe::minutes(1), Some(&source), None);
        assert_eq!(out[0].close, 39.0);
        assert_eq!(compute(&falling, None, 0, Timeframe::minutes(1), None, None)[0].close, -41.0, "the estimate");
    }

    /// A bar from before the ladders begin is estimated, the ones after are
    /// read, and the estimate carries into the period it shares with them.
    #[test]
    fn a_bar_before_the_ladders_begin_is_estimated_and_the_rest_read() {
        let series = minutes(&[(10, 0), (0, 3)]);
        let held = Held::of(&series);
        let days = Days::fixed(0);
        let source = Source { held: &held, days: &days };
        let bars = vec![
            bar(MIDNIGHT - 60, 10.0, 9.0, 6.0),
            bar(MIDNIGHT, 9.0, 8.0, 17.0),
            bar(MIDNIGHT + 60, 8.0, 9.0, 10.0),
        ];
        let out = compute(&bars, None, 0, Timeframe::minutes(1), Some(&source), None);
        assert_eq!(out.iter().map(|b| b.estimated).collect::<Vec<_>>(), vec![true, false, false]);
        assert_eq!(closes(&out), vec![-6.0, 4.0, 1.0]);
    }

    /// A day held by its total alone is asked for its flow, estimated until
    /// the flow is in, and read once it is.
    #[test]
    fn a_day_held_by_its_total_is_estimated_until_its_flow_is_read() {
        let series = minutes(&[(10, 0), (0, 3)]);
        let whole = Held::of(&series);
        let day = whole.days[0].clone();
        let mut held = Held::new(TICK);
        held.days.push(crate::ladders::HeldDay::total(day.day.day, day.first_ts, day.last_ts, day.day.total.clone()));
        held.floor = MIDNIGHT;
        let days = Days::fixed(0);
        let bars = vec![bar(MIDNIGHT, 9.0, 8.0, 17.0), bar(MIDNIGHT + 60, 8.0, 9.0, 10.0)];
        let tf = Timeframe::minutes(1);
        let mut memo = Memo::default();
        let out = compute(&bars, None, 0, tf, Some(&Source { held: &held, days: &days }), Some(&mut memo));
        assert!(out.iter().all(|b| b.estimated), "{out:?}");
        assert_eq!(held.take_wanted_flows(), vec![day.day.day]);

        assert!(held.put_flow(day.day.day, day.day.flow()));
        let out = compute(&bars, None, 0, tf, Some(&Source { held: &held, days: &days }), Some(&mut memo));
        assert_eq!(closes(&out), vec![10.0, 7.0], "read, not left as the estimate the memo held");
        assert!(out.iter().all(|b| !b.estimated));
    }

    /// Kept from the last computation, a live tick works out only the
    /// newest bars again, and comes to what working out every bar would.
    #[test]
    fn a_live_tick_works_out_only_the_newest_bars_again() {
        let mut bars: Vec<Bar> = (0..500)
            .map(|i| {
                let open = 100.0 + (i % 7) as f64;
                bar(MIDNIGHT + 60 * i, open, open + ((i % 3) as f64 - 1.0), 10.0 + i as f64)
            })
            .collect();
        let tf = Timeframe::minutes(1);
        let mut memo = Memo::default();
        let first = compute(&bars, Some(Reset::Session), 0, tf, None, Some(&mut memo));
        assert_eq!(first, compute(&bars, Some(Reset::Session), 0, tf, None, None));

        // What was kept is not worked out again: the memo's bars are spoiled
        // where nothing changed, and the answer keeps what it had there.
        memo.out[10].close = 12345.0;
        let last = bars.len() - 1;
        bars[last].close += 3.0;
        bars.push(bar(MIDNIGHT + 60 * 500, 101.0, 99.0, 8.0));
        let again = compute(&bars, Some(Reset::Session), 0, tf, None, Some(&mut memo));
        assert_eq!(again[10].close, 12345.0, "the bars before the tick were not worked out again");
        memo.out[10] = first[10];
        let again = compute(&bars, Some(Reset::Session), 0, tf, None, Some(&mut memo));
        assert_eq!(again, compute(&bars, Some(Reset::Session), 0, tf, None, None));
        assert_eq!(again.len(), 501);

        // A different reset is worked out from the start.
        let never = compute(&bars, None, 0, tf, None, Some(&mut memo));
        assert_eq!(never, compute(&bars, None, 0, tf, None, None));
    }

    /// On a chart with ladders the forming minute moves the newest bar
    /// without the bar moving, and a tick picks it up; a minute sealed live
    /// does the same.
    #[test]
    fn a_live_minute_moves_the_newest_bar_on_a_chart_with_ladders() {
        let series = minutes(&[(10, 0), (0, 3)]);
        let mut held = Held::of(&series);
        let days = Days::fixed(0);
        let tf = Timeframe::minutes(1);
        let mut bars = vec![bar(MIDNIGHT, 9.0, 8.0, 17.0), bar(MIDNIGHT + 60, 8.0, 9.0, 10.0)];
        let mut memo = Memo::default();
        let out = compute(&bars, None, 0, tf, Some(&Source { held: &held, days: &days }), Some(&mut memo));
        assert_eq!(closes(&out), vec![10.0, 7.0]);

        let mut forming = Minute::opening(MIDNIGHT + 120, to_price(400, TICK));
        forming.add_trade(401, to_price(401, TICK), 6, Side::Buy, 1);
        held.forming = Some(forming.clone());
        bars.push(bar(MIDNIGHT + 120, 9.0, 9.0, 6.0));
        let out = compute(&bars, None, 0, tf, Some(&Source { held: &held, days: &days }), Some(&mut memo));
        assert_eq!(closes(&out), vec![10.0, 7.0, 13.0]);

        forming.add_trade(399, to_price(399, TICK), 20, Side::Sell, 1);
        held.forming = Some(forming.clone());
        let out = compute(&bars, None, 0, tf, Some(&Source { held: &held, days: &days }), Some(&mut memo));
        assert_eq!(closes(&out), vec![10.0, 7.0, -7.0], "the bar did not move, the delta did");

        let generation = held.generation;
        held.seal(held.days[0].day.day, forming);
        assert_eq!(held.generation, generation, "a live minute is not a change to the past");
        let out = compute(&bars, None, 0, tf, Some(&Source { held: &held, days: &days }), Some(&mut memo));
        assert_eq!(closes(&out), vec![10.0, 7.0, -7.0]);
    }

    /// Bars a day long are read from whole days' totals.
    #[test]
    fn a_daily_bar_is_read_from_its_days_total() {
        let series = minutes(&[(10, 0), (0, 3), (5, 1)]);
        let held = Held::of(&series);
        let days = Days::fixed(0);
        let day = held.days[0].day.day;
        let mut whole = held.clone();
        whole.floor_day = day;
        let bars = vec![bar(day * 86_400, 1.0, 0.0, 99.0)];
        let out = compute(&bars, None, 0, Timeframe::days(1), Some(&Source { held: &whole, days: &days }), None);
        assert_eq!(out[0].close, 11.0);
        assert!(!out[0].estimated);
    }
}
