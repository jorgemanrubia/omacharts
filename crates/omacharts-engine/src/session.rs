//! Regular and extended trading hours.
//!
//! A free feed hands back every bar it has, including the thin overnight ones.
//! Those matter sometimes and ruin a chart the rest of the time: a handful of
//! trades at 3am stretch the price scale and leave the session everyone
//! actually traded squashed into a corner.
//!
//! So the chart can ask for regular hours only. Each market keeps its own
//! clock: New York's cash session is what "RTH" means for most of what this
//! app charts — US equities, the index futures that track them, and the US
//! indexes themselves — and Taipei's is the TWSE's and the TPEx's.
//! Instruments that keep hours this module does not know, or no hours at all,
//! are left alone, because filtering them would only throw data away.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use chrono::{Datelike, NaiveDate, TimeZone, Timelike, Weekday};
use chrono_tz::America::New_York;
use chrono_tz::Asia::Taipei;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::bars::Bar;
use crate::symbols::{Instrument, InstrumentKind};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Session {
    /// Everything the provider sends, overnight included.
    #[default]
    Extended,
    /// The cash session only.
    Regular,
}

impl Session {
    pub const ALL: [Session; 2] = [Session::Extended, Session::Regular];

    pub fn label(self) -> &'static str {
        match self {
            Session::Extended => "Extended hours",
            Session::Regular => "Regular hours only",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Session::Extended => "extended",
            Session::Regular => "regular",
        }
    }

    pub fn from_key(key: &str) -> Option<Session> {
        Session::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// One cash market's day, in its own wall-clock time.
///
/// Two windows rather than one, and deliberately, because they answer
/// different questions. The bells say which bars belong on a regular-hours
/// chart; `trading` says whether another bar is still coming at all — and
/// somebody with a chart open at eight in the morning is watching it
/// precisely because the pre-market is moving.
struct Market {
    zone: Tz,
    /// The opening and closing bell, in minutes past local midnight.
    bells: (u32, u32),
    /// From the pre-market's start to the post-market's end.
    trading: (u32, u32),
    /// Whether the bar stamped at the closing bell is the close itself, which
    /// a regular-hours chart then has to keep.
    close_prints: bool,
    /// The exchange's own list of the days it shuts, or shuts early, as the
    /// text of a file: see [`Calendar::parse`].
    holidays: &'static str,
    calendar: OnceLock<Calendar>,
}

/// One trading day's hours, once the calendar has had its say.
#[derive(Clone, Copy)]
struct Day {
    bells: (u32, u32),
    trading: (u32, u32),
    close_prints: bool,
}

impl Day {
    /// The bars a regular-hours chart keeps: the bells, open inclusive and
    /// close exclusive, and the closing print where the close prints at the
    /// bell.
    fn regular(&self) -> (u32, u32) {
        (self.bells.0, self.bells.1 + u32::from(self.close_prints))
    }

    /// Where `minutes` past midnight falls in this day.
    fn phase(&self, minutes: u32) -> Phase {
        let ((open, close), (pre, post)) = (self.bells, self.trading);
        match minutes {
            m if (open..close).contains(&m) => Phase::Open,
            m if (pre..open).contains(&m) => Phase::PreMarket,
            m if (close..post).contains(&m) => Phase::PostMarket,
            _ => Phase::Closed,
        }
    }
}

impl Market {
    /// The local date and minutes past midnight, where this market keeps them.
    ///
    /// Timezone-aware rather than a fixed offset, because a session keeps its
    /// local hours across daylight saving while its UTC offset moves.
    fn local(&self, ts: i64) -> Option<(NaiveDate, u32)> {
        let local = self.zone.timestamp_opt(ts, 0).single()?;
        Some((local.date_naive(), local.hour() * 60 + local.minute()))
    }

    fn calendar(&self) -> &Calendar {
        self.calendar.get_or_init(|| Calendar::parse(self.holidays))
    }

    /// How `date` trades, or `None` for a weekend or a holiday.
    ///
    /// An early close brings the post-market forward with it, keeping its
    /// usual length: New York's runs to 17:00 after a 13:00 bell.
    fn day(&self, date: NaiveDate) -> Option<Day> {
        let calendar = self.calendar();
        if !weekday(date.weekday()) || calendar.closed.contains(&date) {
            return None;
        }
        let close = calendar.early.get(&date).copied().unwrap_or(self.bells.1);
        let early = self.bells.1 - close.min(self.bells.1);
        Some(Day {
            bells: (self.bells.0, close),
            trading: (self.trading.0, self.trading.1 - early),
            close_prints: self.close_prints,
        })
    }

    /// The day and the minute `ts` falls on, if the market trades that day.
    fn at(&self, ts: i64) -> Option<(Day, u32)> {
        let (date, minutes) = self.local(ts)?;
        Some((self.day(date)?, minutes))
    }
}

fn weekday(day: Weekday) -> bool {
    !matches!(day, Weekday::Sat | Weekday::Sun)
}

/// The days an exchange has said it will not trade, and the days it closes
/// early with the time of that day's closing bell.
///
/// Read from a file the exchange's own list was copied into, once per run. A
/// year nobody has written down has nothing in it, which is where this was
/// before there were files: every weekday reads as an ordinary trading day.
struct Calendar {
    closed: HashSet<NaiveDate>,
    early: HashMap<NaiveDate, u32>,
}

impl Calendar {
    /// One date per line, `#` for comments; `@HH:MM` after the date makes it
    /// an early close at that time rather than a day off.
    fn parse(text: &str) -> Calendar {
        let mut calendar = Calendar { closed: HashSet::new(), early: HashMap::new() };
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            let mut words = line.split_whitespace();
            let Some(date) = words.next().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            else {
                continue;
            };
            let early = words
                .next()
                .and_then(|w| w.strip_prefix('@'))
                .and_then(|t| chrono::NaiveTime::parse_from_str(t, "%H:%M").ok());
            match early {
                Some(bell) => {
                    calendar.early.insert(date, bell.hour() * 60 + bell.minute());
                }
                None => {
                    calendar.closed.insert(date);
                }
            }
        }
        calendar
    }
}

/// The US cash market. The bar stamped at 16:00 belongs to the post-market,
/// not to this session; the pre- and post-market run 04:00 to 20:00.
///
/// Its holidays are the NYSE's, kept by hand from the three years it publishes.
static NEW_YORK: Market = Market {
    zone: New_York,
    bells: (9 * 60 + 30, 16 * 60),
    trading: (4 * 60, 20 * 60),
    close_prints: false,
    holidays: include_str!("holidays_ny.txt"),
    calendar: OnceLock::new(),
};

/// The Taiwan Stock Exchange and the Taipei Exchange, which keep one clock.
///
/// Continuous trading runs 09:00 to 13:25 and the closing auction prints at
/// 13:30 — a bar stamped 13:30 *is* the close, unlike New York's 16:00 bar,
/// so a regular-hours chart keeps it. The trading window is wider at both
/// ends: orders are taken from 08:30, and the fixed-price after-hours session
/// runs 14:00 to 14:30. Taiwan has no daylight saving, so this is 01:00 to
/// 05:30 UTC all year.
///
/// Its holidays are the TWSE's Holiday Schedule, which
/// `tools/build_taiwan_holidays.py` merges into the file a year at a time.
static TAIPEI: Market = Market {
    zone: Taipei,
    bells: (9 * 60, 13 * 60 + 30),
    trading: (8 * 60 + 30, 14 * 60 + 30),
    close_prints: true,
    holidays: include_str!("holidays_tw.txt"),
    calendar: OnceLock::new(),
};

/// Every cash market this module keeps the hours of.
static EXCHANGES: [&Market; 2] = [&NEW_YORK, &TAIPEI];

/// Minutes past midnight, New York time, of the hour the futures session
/// breaks for each day, which is also where its week starts and ends.
const BREAK_START: u32 = 17 * 60;
const BREAK_END: u32 = 18 * 60;

/// The cash market whose clock this instrument keeps, when it is one this
/// module knows.
///
/// FX and crypto have no cash session. A listing says where it trades with
/// its suffix — `.TW` for the TWSE, `.TWO` for the TPEx, and any other is
/// abroad on hours this module does not know. An index never carries one, so
/// it says so with its currency instead: the DAX's session is 09:00-17:30 in
/// Frankfurt and the Nikkei's has closed before New York opens. Holding those
/// to 09:30-16:00 in New York keeps two hours of the DAX's day and none at
/// all of the Nikkei's.
fn market(instrument: &Instrument) -> Option<&'static Market> {
    if !matches!(
        instrument.kind,
        InstrumentKind::Equity | InstrumentKind::Etf | InstrumentKind::Index | InstrumentKind::FutureRoot
    ) {
        return None;
    }
    match (instrument.suffix.as_deref(), instrument.currency.as_deref()) {
        (Some("TW" | "TWO"), _) | (None, Some("TWD")) => Some(&TAIPEI),
        // Unknown currency is treated as dollars: it is what a ticker typed
        // into the search field with nothing behind it turns out to be.
        (None, None | Some("USD")) => Some(&NEW_YORK),
        _ => None,
    }
}

/// Does restricting to regular hours mean anything for this instrument?
pub fn has_regular_hours(instrument: &Instrument) -> bool {
    market(instrument).is_some()
}

/// Keep only the bars inside the cash session.
///
/// A no-op for daily and coarser bars — one bar already is a session — and for
/// instruments with no cash session to speak of.
pub fn filter(bars: &[Bar], session: Session, instrument: &Instrument, intraday: bool) -> Vec<Bar> {
    if session == Session::Extended || !intraday {
        return bars.to_vec();
    }
    let Some(market) = market(instrument) else {
        return bars.to_vec();
    };
    // The bars come in order, so the calendar is asked once a day, not once
    // a bar: a year of minute bars is a quarter of a million of them.
    let mut today: Option<(NaiveDate, Option<Day>)> = None;
    bars.iter()
        .copied()
        .filter(|bar| {
            let Some((date, minutes)) = market.local(bar.ts) else { return false };
            if today.is_none_or(|(seen, _)| seen != date) {
                today = Some((date, market.day(date)));
            }
            today.and_then(|(_, day)| day).is_some_and(|day| {
                let (start, end) = day.regular();
                (start..end).contains(&minutes)
            })
        })
        .collect()
}

/// Could another bar still arrive for this instrument at `ts`?
///
/// The question a chart left open has to answer before fetching itself again.
/// Asking a provider for bars that cannot exist spends a rate limit on a
/// guaranteed empty reply, and a watchlist of forty symbols asking it all
/// weekend is how an address gets throttled for the Monday open.
///
/// Generous at every edge on purpose. Being wrong in this direction costs one
/// request that comes back with nothing; being wrong in the other leaves a
/// chart quietly stale while the thing it charts is moving, which is the only
/// failure here anybody would notice. The exchanges' holiday calendars stop
/// a holiday from being asked about, for the years they have been written
/// down; past those a holiday reads as an ordinary weekday and costs a
/// handful of empty replies — the same trade, taken knowingly.
pub fn is_trading(instrument: &Instrument, ts: i64) -> bool {
    match Hours::of(instrument) {
        Some(Hours::Cash(market) | Hours::Bells(market)) => {
            market.at(ts).is_some_and(|(day, minutes)| day.phase(minutes) != Phase::Closed)
        }
        Some(Hours::Futures) => Hours::Futures.phase(ts) == Phase::Open,
        // Two very different groups come out the same way here. FX and
        // crypto are always worth asking: crypto never closes, and FX's
        // weekend is short enough that a few empty replies cost less than
        // being late for the Sunday open. A foreign listing, or an index
        // priced somewhere else, keeps hours this module knows nothing
        // about — and guessing would mean refusing to refresh a Madrid
        // listing right through Madrid's own session, which is worse than
        // not asking.
        Some(Hours::Fx | Hours::Always) | None => true,
    }
}

/// The futures week: Sunday evening through to Friday afternoon, broken for an
/// hour at the end of each session.
///
/// Worth answering separately because this is the one thing on a chart that
/// trades while everybody is asleep, and the small hours are exactly when
/// somebody has an index future open. Holding it to the cash market's day
/// would switch refreshing off for the half of its week that people watch it
/// for.
fn futures_are_trading(weekday: Weekday, minutes: u32) -> bool {
    in_week(weekday, minutes, BREAK_END) && !(BREAK_START..BREAK_END).contains(&minutes)
}

/// Inside a week that opens on Sunday at `sunday_open` and closes on Friday
/// at the futures' break, New York time — the futures week, and the FX week,
/// which opens an hour earlier and does not stop each day.
fn in_week(weekday: Weekday, minutes: u32, sunday_open: u32) -> bool {
    match weekday {
        Weekday::Sat => false,
        Weekday::Sun => minutes >= sunday_open,
        Weekday::Fri => minutes < BREAK_START,
        _ => true,
    }
}

/// Where a market is in its day, the way the dot beside a chart's symbol
/// says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Open,
    PreMarket,
    PostMarket,
    Closed,
}

impl Phase {
    /// As a heading: "Market open".
    pub fn label(self) -> &'static str {
        match self {
            Phase::Open => "Market open",
            Phase::PreMarket => "Pre-market",
            Phase::PostMarket => "Post-market",
            Phase::Closed => "Market closed",
        }
    }

    /// As a word, for a sentence that has already said "market", for a
    /// machine, and for a style class.
    pub fn key(self) -> &'static str {
        match self {
            Phase::Open => "open",
            Phase::PreMarket => "pre-market",
            Phase::PostMarket => "post-market",
            Phase::Closed => "closed",
        }
    }
}

/// An instrument's market right now, and when that next changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MarketStatus {
    pub phase: Phase,
    /// What comes next and when, or `None` for something that never closes.
    pub next: Option<(Phase, i64)>,
    /// The clock the market keeps, which is the one its times are told in.
    pub zone: Tz,
}

/// How an instrument's week is laid out, for the status rather than for
/// filtering bars.
///
/// Wider than [`market`] on purpose. Filtering has to leave FX and crypto
/// alone because neither has a cash session to keep; a status has something
/// true to say about both — FX shuts for the weekend, crypto never does.
#[derive(Clone, Copy)]
enum Hours {
    /// Shares and funds: a pre-market, the bell, a post-market.
    Cash(&'static Market),
    /// An index is computed from the cash session and nothing else, so it is
    /// open between the bells and closed either side of them.
    Bells(&'static Market),
    Futures,
    Fx,
    Always,
}

impl Hours {
    fn of(instrument: &Instrument) -> Option<Hours> {
        Some(match instrument.kind {
            InstrumentKind::Crypto => Hours::Always,
            InstrumentKind::Fx => Hours::Fx,
            InstrumentKind::FutureRoot => {
                // Only to say it is a US future; its week is not the cash day.
                market(instrument)?;
                Hours::Futures
            }
            InstrumentKind::Index => Hours::Bells(market(instrument)?),
            InstrumentKind::Equity | InstrumentKind::Etf => Hours::Cash(market(instrument)?),
        })
    }

    fn zone(self) -> Tz {
        match self {
            Hours::Cash(market) | Hours::Bells(market) => market.zone,
            Hours::Futures | Hours::Fx => New_York,
            Hours::Always => chrono_tz::UTC,
        }
    }

    fn phase(self, ts: i64) -> Phase {
        let open = |yes: bool| if yes { Phase::Open } else { Phase::Closed };
        let new_york = || NEW_YORK.local(ts).map(|(date, minutes)| (date.weekday(), minutes));
        match self {
            Hours::Always => Phase::Open,
            Hours::Futures => open(new_york().is_some_and(|(day, m)| futures_are_trading(day, m))),
            // Sydney's Monday morning to New York's Friday close.
            Hours::Fx => open(new_york().is_some_and(|(day, m)| in_week(day, m, BREAK_START))),
            Hours::Bells(market) => open(Hours::Cash(market).phase(ts) == Phase::Open),
            Hours::Cash(market) => market.at(ts).map_or(Phase::Closed, |(day, m)| day.phase(m)),
        }
    }

    /// Every local time on `date` at which the phase can change, earliest
    /// first. Asked a day at a time because an early close moves one.
    ///
    /// Midnight is never one: every market here is already closed when its
    /// weekday turns over into a weekend, a holiday, or out of one.
    fn edges(self, date: NaiveDate) -> Vec<u32> {
        match self {
            Hours::Always => Vec::new(),
            Hours::Futures => vec![BREAK_START, BREAK_END],
            Hours::Fx => vec![BREAK_START],
            Hours::Cash(market) | Hours::Bells(market) => market
                .day(date)
                .map(|day| vec![day.trading.0, day.bells.0, day.bells.1, day.trading.1])
                .unwrap_or_default(),
        }
    }

    /// The first edge after `now` at which the phase differs from the one
    /// it has now.
    ///
    /// Walked edge by edge rather than minute by minute. Three weeks is the
    /// horizon because Lunar New Year shuts Taipei for over a week, and a
    /// weekend either side of it makes eleven days.
    fn next(self, now: i64) -> Option<(Phase, i64)> {
        let zone = self.zone();
        let current = self.phase(now);
        let today = zone.timestamp_opt(now, 0).single()?.date_naive();
        (0..21)
            .filter_map(|days| today.checked_add_days(chrono::Days::new(days)))
            .flat_map(|day| {
                let edges = self.edges(day);
                edges.into_iter().filter_map(move |m| day.and_hms_opt(m / 60, m % 60, 0))
            })
            // `earliest` because a fall-back hour happens twice, and an edge
            // in a spring-forward gap does not happen at all, which is what
            // dropping it says.
            .filter_map(|local| zone.from_local_datetime(&local).earliest())
            .map(|at| at.timestamp())
            .filter(|at| *at > now)
            .map(|at| (self.phase(at), at))
            .find(|(phase, _)| *phase != current)
    }
}

/// Where this instrument's market is at `now`, or `None` for a listing whose
/// hours this module does not know.
///
/// Holidays and early closes are the exchanges' own, for the years their
/// calendars have been written down; futures and FX keep no holidays here.
pub fn status(instrument: &Instrument, now: i64) -> Option<MarketStatus> {
    let hours = Hours::of(instrument)?;
    Some(MarketStatus { phase: hours.phase(now), next: hours.next(now), zone: hours.zone() })
}

/// Every exchange whose day this module knows, and where that day is at
/// `now` — what the clock's tooltip lists.
pub fn exchanges(now: i64) -> Vec<(Tz, Phase)> {
    EXCHANGES.iter().map(|market| (market.zone, Hours::Cash(market).phase(now)))
        .collect()
}

impl MarketStatus {
    /// What the next change is, in words: "closes in 2h 26m".
    ///
    /// Named for what happens rather than for the phase it lands in, because
    /// "post-market in 2h" makes the reader work out that the market closes.
    /// Said beside the phase's own name, so the post-market "closes" rather
    /// than repeating that it is the post-market which is ending.
    pub fn countdown(&self, now: i64) -> String {
        let Some((next, at)) = self.next else {
            return "trades around the clock".into();
        };
        let verb = match (self.phase, next) {
            (Phase::PreMarket, Phase::Open) | (Phase::Closed, Phase::Open) => "opens",
            (Phase::Closed, _) => "pre-market starts",
            _ => "closes",
        };
        format!("{verb} in {}", span(at - now))
    }

    /// The wall-clock time of the next change, where the market keeps it:
    /// "16:00 New York", with the day when it is not today.
    pub fn next_local(&self, now: i64) -> Option<String> {
        let (_, at) = self.next?;
        let at = self.zone.timestamp_opt(at, 0).single()?;
        let today = self.zone.timestamp_opt(now, 0).single()?.date_naive();
        let days = (at.date_naive() - today).num_days();
        let when = match days {
            0 => at.format("%H:%M"),
            1..=6 => at.format("%a %H:%M"),
            _ => at.format("%a %-d %b %H:%M"),
        };
        Some(format!("{when} {}", city(self.zone)))
    }
}

/// "America/New_York" as people say it.
pub fn city(zone: Tz) -> String {
    zone.name().rsplit('/').next().unwrap_or(zone.name()).replace('_', " ")
}

/// A stretch of time to the precision anybody waiting on it cares about.
fn span(seconds: i64) -> String {
    let minutes = (seconds.max(0) + 59) / 60;
    match minutes {
        0 | 1 => "a minute".into(),
        m if m < 60 => format!("{m}m"),
        m if m < 24 * 60 => match m % 60 {
            0 => format!("{}h", m / 60),
            rest => format!("{}h {rest}m", m / 60),
        },
        m => match (m / 60) % 24 {
            0 => format!("{}d", m / (24 * 60)),
            hours => format!("{}d {hours}h", m / (24 * 60)),
        },
    }
}

#[cfg(test)]
mod tests {
    use chrono_tz::Asia::Tokyo;

    use super::*;

    /// Is this instant inside the New York cash session on a weekday?
    fn in_regular_hours(ts: i64) -> bool {
        NEW_YORK.at(ts).is_some_and(|(day, minutes)| {
            let (start, end) = day.regular();
            (start..end).contains(&minutes)
        })
    }

    fn instrument(kind: InstrumentKind, suffix: Option<&str>) -> Instrument {
        Instrument {
            symbol: "X".into(),
            name: "X".into(),
            kind,
            suffix: suffix.map(str::to_string),
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    /// The same instrument, quoted somewhere in particular.
    fn priced_in(kind: InstrumentKind, currency: &str) -> Instrument {
        Instrument { currency: Some(currency.into()), ..instrument(kind, None) }
    }

    fn bar(ts: i64) -> Bar {
        Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 }
    }

    /// 2024-03-13 was a Wednesday, in US daylight time (UTC-4).
    fn wednesday_at(hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 3, 13, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    /// 2024-01-10, a Wednesday in standard time (UTC-5).
    fn winter_wednesday_at(hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 1, 10, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    #[test]
    fn the_cash_session_runs_from_the_open_to_the_close() {
        assert!(!in_regular_hours(wednesday_at(9, 29)), "before the bell");
        assert!(in_regular_hours(wednesday_at(9, 30)), "the open");
        assert!(in_regular_hours(wednesday_at(12, 0)));
        assert!(in_regular_hours(wednesday_at(15, 59)));
        assert!(!in_regular_hours(wednesday_at(16, 0)), "the close belongs to the next session");
        assert!(!in_regular_hours(wednesday_at(3, 0)), "overnight");
    }

    #[test]
    fn the_session_keeps_its_local_hours_across_daylight_saving() {
        // Same wall-clock times, different UTC offsets.
        assert!(in_regular_hours(winter_wednesday_at(9, 30)));
        assert!(!in_regular_hours(winter_wednesday_at(9, 29)));
        assert!(in_regular_hours(winter_wednesday_at(15, 59)));
        // And the two dates really are on different offsets.
        assert_ne!(
            wednesday_at(9, 30) % 86_400,
            winter_wednesday_at(9, 30) % 86_400,
            "the fixture dates should straddle the change"
        );
    }

    #[test]
    fn weekends_are_not_the_cash_session() {
        let saturday = New_York.with_ymd_and_hms(2024, 3, 16, 12, 0, 0).single().unwrap();
        assert!(!in_regular_hours(saturday.timestamp()));
    }

    #[test]
    fn filtering_keeps_only_the_session() {
        let bars = vec![
            bar(wednesday_at(4, 0)),
            bar(wednesday_at(9, 30)),
            bar(wednesday_at(12, 0)),
            bar(wednesday_at(18, 0)),
        ];
        let stock = instrument(InstrumentKind::Equity, None);
        let kept = filter(&bars, Session::Regular, &stock, true);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].ts, wednesday_at(9, 30));
    }

    #[test]
    fn extended_hours_keeps_everything() {
        let bars: Vec<Bar> = (0..24).map(|h| bar(wednesday_at(h, 0))).collect();
        let stock = instrument(InstrumentKind::Equity, None);
        assert_eq!(filter(&bars, Session::Extended, &stock, true).len(), bars.len());
    }

    #[test]
    fn daily_bars_are_never_filtered() {
        // One daily bar already is a session; dropping it for being stamped
        // outside 09:30 would empty the chart.
        let bars = vec![bar(wednesday_at(0, 0)), bar(wednesday_at(0, 0) + 86_400)];
        let stock = instrument(InstrumentKind::Equity, None);
        assert_eq!(filter(&bars, Session::Regular, &stock, false).len(), 2);
    }

    #[test]
    fn instruments_without_a_cash_session_are_left_alone() {
        let bars: Vec<Bar> = (0..24).map(|h| bar(wednesday_at(h, 0))).collect();
        for kind in [InstrumentKind::Fx, InstrumentKind::Crypto] {
            let it = instrument(kind, None);
            assert!(!has_regular_hours(&it));
            assert_eq!(filter(&bars, Session::Regular, &it, true).len(), bars.len());
        }
        // Nor does a foreign listing, whose hours are not New York's.
        let madrid = instrument(InstrumentKind::Equity, Some("MC"));
        assert!(!has_regular_hours(&madrid));
        assert_eq!(filter(&bars, Session::Regular, &madrid, true).len(), bars.len());
    }

    #[test]
    fn an_index_quoted_abroad_keeps_its_own_hours() {
        // The Nikkei's whole session is overnight in New York, so holding it
        // to the cash session there would leave nothing on the chart at all.
        let nikkei = priced_in(InstrumentKind::Index, "JPY");
        assert!(!has_regular_hours(&nikkei));

        let bars: Vec<Bar> = (9..15)
            .map(|h| bar(Tokyo.with_ymd_and_hms(2024, 3, 13, h, 0, 0).single().unwrap().timestamp()))
            .collect();
        assert!(!bars.iter().any(|b| in_regular_hours(b.ts)), "the fixture should be overnight");
        assert_eq!(filter(&bars, Session::Regular, &nikkei, true).len(), bars.len());

        // The DAX fares differently and no better: Frankfurt's 09:00-17:30 and
        // New York's 09:30-16:00 overlap for about two hours, so filtering
        // keeps a quarter of the day and calls it the session.
        let dax = priced_in(InstrumentKind::Index, "EUR");
        assert!(!has_regular_hours(&dax));
    }

    #[test]
    fn a_dollar_index_still_has_a_cash_session() {
        // The S&P really is on New York hours, and so is a ticker typed into
        // the search field with no listing behind it to say otherwise.
        assert!(has_regular_hours(&priced_in(InstrumentKind::Index, "USD")));
        assert!(has_regular_hours(&instrument(InstrumentKind::Equity, None)));

        let bars = vec![bar(wednesday_at(4, 0)), bar(wednesday_at(12, 0))];
        let spx = priced_in(InstrumentKind::Index, "USD");
        assert_eq!(filter(&bars, Session::Regular, &spx, true).len(), 1);
    }

    /// A moment in New York, for the days of the week the futures session
    /// cares about. March 2024 runs Friday the 15th, Saturday the 16th,
    /// Sunday the 17th, Monday the 18th.
    fn ny(day: u32, hour: u32, minute: u32) -> i64 {
        New_York
            .with_ymd_and_hms(2024, 3, day, hour, minute, 0)
            .single()
            .unwrap()
            .timestamp()
    }

    /// The window a chart refetches inside is the extended one, not the cash
    /// session: somebody with a chart open before the bell is watching the
    /// pre-market, and telling them nothing is coming would be wrong.
    #[test]
    fn a_share_is_trading_through_the_pre_and_post_market() {
        let stock = instrument(InstrumentKind::Equity, None);
        assert!(!is_trading(&stock, ny(13, 3, 59)), "before the pre-market");
        assert!(is_trading(&stock, ny(13, 4, 0)), "the pre-market opens");
        assert!(is_trading(&stock, ny(13, 9, 0)), "an hour before the bell");
        assert!(is_trading(&stock, ny(13, 12, 0)), "the cash session");
        assert!(is_trading(&stock, ny(13, 18, 0)), "after the bell, still printing");
        assert!(!is_trading(&stock, ny(13, 20, 0)), "the post-market closes");
        assert!(!is_trading(&stock, ny(13, 2, 0)), "the middle of the night");
    }

    /// The case that saves the most requests by far: a chart left open over a
    /// weekend has two days in which nothing it could ask for exists.
    #[test]
    fn nothing_listed_is_trading_at_the_weekend() {
        let stock = instrument(InstrumentKind::Equity, None);
        assert!(!is_trading(&stock, ny(16, 12, 0)), "Saturday");
        assert!(!is_trading(&stock, ny(17, 12, 0)), "Sunday");
    }

    /// Futures keep their own week, and it is most of one. Holding them to the
    /// cash market's hours would switch refreshing off exactly when somebody
    /// watches an index future: overnight.
    #[test]
    fn a_future_trades_overnight_and_almost_all_week() {
        let future = instrument(InstrumentKind::FutureRoot, None);
        assert!(is_trading(&future, ny(13, 3, 0)), "three in the morning, and printing");
        assert!(is_trading(&future, ny(13, 12, 0)), "the middle of the day");
        assert!(is_trading(&future, ny(13, 23, 0)), "and late at night");

        // The hour each session breaks for.
        assert!(!is_trading(&future, ny(13, 17, 30)), "the daily break");
        assert!(is_trading(&future, ny(13, 18, 0)), "the next session opens");

        // The two ends of the week.
        assert!(is_trading(&future, ny(15, 16, 0)), "Friday afternoon");
        assert!(!is_trading(&future, ny(15, 17, 30)), "Friday's close is the week's");
        assert!(!is_trading(&future, ny(16, 12, 0)), "Saturday, like everything else");
        assert!(!is_trading(&future, ny(17, 12, 0)), "Sunday lunchtime, still shut");
        assert!(is_trading(&future, ny(17, 18, 0)), "Sunday evening, and the week restarts");
    }

    /// A market with no close has no moment at which asking is pointless.
    #[test]
    fn what_never_closes_is_always_trading() {
        for kind in [InstrumentKind::Crypto, InstrumentKind::Fx] {
            let it = instrument(kind, None);
            assert!(is_trading(&it, ny(17, 3, 0)), "{kind:?} at 3am on a Sunday");
            assert!(is_trading(&it, ny(13, 12, 0)), "{kind:?} midweek");
        }
    }

    /// Hours this module does not know are hours it must not rule on. A
    /// Madrid listing's session is the middle of New York's night, so reading
    /// "closed" off New York's clock would leave it stale all day.
    #[test]
    fn a_listing_whose_hours_we_do_not_know_is_treated_as_trading() {
        let madrid = instrument(InstrumentKind::Equity, Some("MC"));
        assert!(is_trading(&madrid, ny(13, 4, 0)), "ten in the morning in Madrid");
        assert!(is_trading(&madrid, ny(13, 22, 0)), "and at a time we cannot rule out");

        let nikkei = priced_in(InstrumentKind::Index, "JPY");
        assert!(is_trading(&nikkei, ny(13, 2, 0)));
    }

    /// The cash session and the window a chart refetches in are not the same
    /// window, and nothing should quietly collapse them into one.
    #[test]
    fn trading_is_a_wider_window_than_the_cash_session() {
        let stock = instrument(InstrumentKind::Equity, None);
        let before_the_bell = ny(13, 8, 0);
        assert!(!in_regular_hours(before_the_bell), "not a regular-hours bar");
        assert!(is_trading(&stock, before_the_bell), "but another one is coming");
    }

    /// A moment in Taipei. October 2026 runs Friday the 2nd, Saturday the
    /// 3rd, Sunday the 4th, Monday the 5th.
    /// A moment in Taipei in October 2026, which opens on a Thursday.
    fn taipei(day: u32, hour: u32, minute: u32) -> i64 {
        taipei_on(10, day, hour, minute)
    }

    fn taipei_on(month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        Taipei.with_ymd_and_hms(2026, month, day, hour, minute, 0).single().unwrap().timestamp()
    }

    #[test]
    fn a_taiwan_listing_keeps_taipei_hours() {
        for suffix in ["TW", "TWO"] {
            let tsmc = Instrument {
                currency: Some("TWD".into()),
                ..instrument(InstrumentKind::Equity, Some(suffix))
            };
            assert!(has_regular_hours(&tsmc), ".{suffix} has a cash session");
            let bars: Vec<Bar> = [(8, 59), (9, 0), (11, 0), (13, 25), (13, 30), (13, 31), (14, 0)]
                .iter()
                .map(|(h, m)| bar(taipei(5, *h, *m)))
                .collect();
            let kept: Vec<i64> =
                filter(&bars, Session::Regular, &tsmc, true).iter().map(|b| b.ts).collect();
            assert_eq!(
                kept,
                vec![taipei(5, 9, 0), taipei(5, 11, 0), taipei(5, 13, 25), taipei(5, 13, 30)],
                "09:00 through the 13:30 closing auction, and nothing either side"
            );
        }
    }

    /// The Taipei session is the middle of New York's night. Reading it off
    /// New York's clock, the way every suffixed listing used to be treated,
    /// would either call it shut all day or never call it shut at all.
    #[test]
    fn a_taiwan_listing_is_trading_only_through_its_own_day() {
        let tsmc = instrument(InstrumentKind::Equity, Some("TW"));
        assert!(!is_trading(&tsmc, taipei(5, 8, 29)), "before orders are taken");
        assert!(is_trading(&tsmc, taipei(5, 8, 30)), "the pre-open");
        assert!(is_trading(&tsmc, taipei(5, 10, 0)), "the session");
        assert!(is_trading(&tsmc, taipei(5, 14, 15)), "the after-hours fixed-price session");
        assert!(!is_trading(&tsmc, taipei(5, 14, 30)), "everything has closed");
        assert!(!is_trading(&tsmc, taipei(5, 22, 30)), "the US open is the Taipei night");
        assert!(!is_trading(&tsmc, taipei(3, 10, 0)), "Saturday");
        assert!(!is_trading(&tsmc, taipei(4, 10, 0)), "Sunday");
        assert!(is_trading(&tsmc, taipei(2, 10, 0)), "Friday");
    }

    #[test]
    fn the_taiex_keeps_taipei_hours_too() {
        // An index never carries a suffix, so its currency is what says where
        // it is priced.
        let taiex = priced_in(InstrumentKind::Index, "TWD");
        assert!(has_regular_hours(&taiex));
        assert!(is_trading(&taiex, taipei(5, 10, 0)));
        assert!(!is_trading(&taiex, taipei(5, 22, 0)));
        let tokyo = instrument(InstrumentKind::Equity, Some("T"));
        assert!(is_trading(&tokyo, taipei(5, 22, 0)), "Tokyo is not Taipei, and is not ruled on");
    }

    fn phase_of(it: &Instrument, ts: i64) -> Phase {
        status(it, ts).expect("a market this module knows").phase
    }

    #[test]
    fn a_share_moves_through_pre_open_post_and_closed() {
        let stock = instrument(InstrumentKind::Equity, None);
        assert_eq!(phase_of(&stock, ny(13, 3, 59)), Phase::Closed);
        assert_eq!(phase_of(&stock, ny(13, 4, 0)), Phase::PreMarket);
        assert_eq!(phase_of(&stock, ny(13, 9, 30)), Phase::Open);
        assert_eq!(phase_of(&stock, ny(13, 16, 0)), Phase::PostMarket);
        assert_eq!(phase_of(&stock, ny(13, 20, 0)), Phase::Closed);
    }

    #[test]
    fn the_next_change_is_the_next_bell() {
        let stock = instrument(InstrumentKind::Equity, None);
        let now = ny(13, 13, 34);
        let open = status(&stock, now).unwrap();
        assert_eq!(open.next, Some((Phase::PostMarket, ny(13, 16, 0))));
        assert_eq!(open.countdown(now), "closes in 2h 26m");
        assert_eq!(open.next_local(now).as_deref(), Some("16:00 New York"));
    }

    #[test]
    fn a_friday_evening_waits_for_monday_morning() {
        let stock = instrument(InstrumentKind::Equity, None);
        let now = ny(15, 21, 0);
        let closed = status(&stock, now).unwrap();
        assert_eq!(closed.next, Some((Phase::PreMarket, ny(18, 4, 0))));
        assert_eq!(closed.countdown(now), "pre-market starts in 2d 7h");
        assert_eq!(closed.next_local(now).as_deref(), Some("Mon 04:00 New York"));
    }

    #[test]
    fn an_index_is_only_open_between_the_bells() {
        let spx = instrument(InstrumentKind::Index, None);
        assert_eq!(phase_of(&spx, ny(13, 8, 0)), Phase::Closed);
        assert_eq!(phase_of(&spx, ny(13, 10, 0)), Phase::Open);
        let now = ny(13, 8, 0);
        assert_eq!(status(&spx, now).unwrap().countdown(now), "opens in 1h 30m");
    }

    #[test]
    fn futures_and_fx_shut_only_for_the_weekend_and_the_break() {
        let es = instrument(InstrumentKind::FutureRoot, None);
        assert_eq!(phase_of(&es, ny(13, 3, 0)), Phase::Open, "overnight");
        assert_eq!(phase_of(&es, ny(13, 17, 30)), Phase::Closed, "the daily break");
        assert_eq!(status(&es, ny(13, 17, 30)).unwrap().next, Some((Phase::Open, ny(13, 18, 0))));
        let fx = instrument(InstrumentKind::Fx, None);
        assert_eq!(phase_of(&fx, ny(13, 17, 30)), Phase::Open, "FX has no break");
        assert_eq!(phase_of(&fx, ny(16, 12, 0)), Phase::Closed, "Saturday");
        assert_eq!(status(&fx, ny(16, 12, 0)).unwrap().next, Some((Phase::Open, ny(17, 17, 0))));
    }

    #[test]
    fn crypto_never_closes_and_a_foreign_listing_is_not_guessed_at() {
        let btc = status(&instrument(InstrumentKind::Crypto, None), ny(16, 3, 0)).unwrap();
        assert_eq!(btc.phase, Phase::Open);
        assert_eq!(btc.next, None);
        assert_eq!(btc.countdown(0), "trades around the clock");
        assert!(status(&instrument(InstrumentKind::Equity, Some("MC")), ny(13, 12, 0)).is_none());
    }

    #[test]
    fn taipei_closes_at_the_bell_not_at_the_auction_window() {
        let tsmc = instrument(InstrumentKind::Equity, Some("TW"));
        // Wednesday 7 October 2026, an ordinary day.
        let at = |h, m| taipei(7, h, m);
        assert_eq!(phase_of(&tsmc, at(8, 45)), Phase::PreMarket);
        assert_eq!(phase_of(&tsmc, at(13, 29)), Phase::Open);
        assert_eq!(phase_of(&tsmc, at(13, 30)), Phase::PostMarket);
        assert_eq!(phase_of(&tsmc, at(14, 30)), Phase::Closed);
    }

    #[test]
    fn a_taipei_holiday_is_closed_all_day_and_not_traded() {
        // Friday 9 October 2026, the day made up for National Day.
        let tsmc = instrument(InstrumentKind::Equity, Some("TW"));
        assert_eq!(phase_of(&tsmc, taipei(9, 10, 0)), Phase::Closed);
        assert!(!is_trading(&tsmc, taipei(9, 10, 0)), "nothing to fetch on a holiday");
        assert_eq!(phase_of(&tsmc, taipei(8, 10, 0)), Phase::Open, "the day before trades");
        let taiex = priced_in(InstrumentKind::Index, "TWD");
        assert_eq!(phase_of(&taiex, taipei(9, 10, 0)), Phase::Closed);
    }

    #[test]
    fn the_countdown_steps_over_a_long_weekend() {
        let tsmc = instrument(InstrumentKind::Equity, Some("TW"));
        let thursday = taipei(8, 20, 0);
        let closed = status(&tsmc, thursday).unwrap();
        assert_eq!(closed.next, Some((Phase::PreMarket, taipei(12, 8, 30))));
        assert_eq!(closed.next_local(thursday).as_deref(), Some("Mon 08:30 Taipei"));
    }

    #[test]
    fn lunar_new_year_is_still_inside_the_horizon() {
        // Settlement-only days, the holiday proper and a made-up day: the
        // market shuts from Thursday 12 February to Monday the 23rd.
        let tsmc = instrument(InstrumentKind::Equity, Some("TW"));
        let eve = taipei_on(2, 11, 15, 0);
        let closed = status(&tsmc, eve).unwrap();
        assert_eq!(closed.next, Some((Phase::PreMarket, taipei_on(2, 23, 8, 30))));
        assert_eq!(closed.next_local(eve).as_deref(), Some("Mon 23 Feb 08:30 Taipei"));
    }

    /// A line the parser skips is a holiday that silently is not one.
    #[test]
    fn every_line_of_both_calendars_is_read() {
        for market in EXCHANGES {
            let lines = market.holidays.lines().filter(|l| !l.starts_with('#')).count();
            let calendar = market.calendar();
            assert_eq!(calendar.closed.len() + calendar.early.len(), lines);
        }
    }

    fn ny_2026(month: u32, day: u32, hour: u32, minute: u32) -> i64 {
        New_York.with_ymd_and_hms(2026, month, day, hour, minute, 0).single().unwrap().timestamp()
    }

    #[test]
    fn thanksgiving_is_shut_and_the_day_after_closes_at_one() {
        let stock = instrument(InstrumentKind::Equity, None);
        assert_eq!(phase_of(&stock, ny_2026(11, 26, 12, 0)), Phase::Closed, "Thanksgiving");
        assert!(!is_trading(&stock, ny_2026(11, 26, 12, 0)));
        assert_eq!(phase_of(&stock, ny_2026(11, 27, 12, 59)), Phase::Open);
        assert_eq!(phase_of(&stock, ny_2026(11, 27, 13, 0)), Phase::PostMarket, "the early bell");
        assert_eq!(phase_of(&stock, ny_2026(11, 27, 17, 0)), Phase::Closed, "a short post-market too");
        assert!(!is_trading(&stock, ny_2026(11, 27, 18, 0)));
        let morning = ny_2026(11, 27, 10, 0);
        let open = status(&stock, morning).unwrap();
        assert_eq!(open.next, Some((Phase::PostMarket, ny_2026(11, 27, 13, 0))));
        assert_eq!(open.countdown(morning), "closes in 3h");
        // A regular-hours chart keeps the short session's bars and no more.
        let bars = vec![bar(ny_2026(11, 27, 12, 30)), bar(ny_2026(11, 27, 14, 0))];
        assert_eq!(filter(&bars, Session::Regular, &stock, true).len(), 1);
    }

    #[test]
    fn the_evening_before_a_holiday_waits_for_the_day_after() {
        // Good Friday 2026: Thursday evening's next session is Monday's.
        let stock = instrument(InstrumentKind::Equity, None);
        let thursday = ny_2026(4, 2, 21, 0);
        let closed = status(&stock, thursday).unwrap();
        assert_eq!(closed.next, Some((Phase::PreMarket, ny_2026(4, 6, 4, 0))));
    }

    #[test]
    fn a_countdown_across_the_clocks_changing_counts_real_time() {
        // 10 March 2024 the clocks went forward at 02:00. From Friday's
        // close to Monday's pre-market is an hour shorter than it looks.
        let stock = instrument(InstrumentKind::Equity, None);
        let friday = New_York.with_ymd_and_hms(2024, 3, 8, 20, 0, 0).single().unwrap().timestamp();
        let monday = New_York.with_ymd_and_hms(2024, 3, 11, 4, 0, 0).single().unwrap().timestamp();
        assert_eq!(status(&stock, friday).unwrap().next, Some((Phase::PreMarket, monday)));
        assert_eq!(monday - friday, (2 * 24 + 8 - 1) * 3600);
    }

    #[test]
    fn keys_round_trip() {
        for session in Session::ALL {
            assert_eq!(Session::from_key(session.key()), Some(session));
        }
        assert_eq!(Session::from_key("nonsense"), None);
    }
}
