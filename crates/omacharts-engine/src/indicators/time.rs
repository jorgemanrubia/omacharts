//! Indicators of time rather than of price.
//!
//! Three of them, built on one thing: the trading day the instrument's
//! exchange keeps (see [`crate::exchanges`]).
//!
//! * **Sessions** shades the extended hours and draws a line where each
//!   day, week or month begins — by the exchange's day, so the ES's Tuesday
//!   starts at five on Monday afternoon in Chicago, not at midnight.
//! * **Time window** marks one stretch of each day: London's session, the
//!   Asian one, the first half hour of the regular session. It can shade it,
//!   draw a line at its start, and carry its high and low forward.
//! * **Session levels** carries one part of each day's prices into the
//!   next: the prior regular session's high, low and close; the overnight's
//!   high and low.
//!
//! What they produce is [`Time`]: stretches of bars to shade, bars to put a
//! line in front of, and levels to draw across a run of bars. All of it is
//! counted in bars rather than in pixels or seconds, because that is how the
//! chart lays time out — a weekend takes no room — and so the renderer places
//! it with the same arithmetic it places the candles with.

use chrono::{Datelike, NaiveDate, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::bars::{Bar, Timeframe};
use crate::exchanges::{instant, registry, wall, Part, Schedule};
use crate::symbols::Instrument;
use crate::theme::ColorChoice;

/// Where a separator goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Separators {
    None,
    /// Each trading day.
    #[default]
    Day,
    Week,
    Month,
}

impl Separators {
    pub const ALL: [Separators; 4] =
        [Separators::None, Separators::Day, Separators::Week, Separators::Month];

    pub fn key(self) -> &'static str {
        match self {
            Separators::None => "none",
            Separators::Day => "day",
            Separators::Week => "week",
            Separators::Month => "month",
        }
    }

    pub fn from_key(key: &str) -> Option<Separators> {
        Separators::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Separators::None => "None",
            Separators::Day => "Day",
            Separators::Week => "Week",
            Separators::Month => "Month",
        }
    }

    /// What to draw at this timeframe: these separators, or the next coarser
    /// ones when these would fall every few bars.
    ///
    /// The same promotion a VWAP's reset period gets, and for the same
    /// reason: a line between every daily bar is not a separator, it is a
    /// grid. Too coarse for even a month's, and nothing is drawn.
    pub fn effective_for(self, timeframe: Timeframe) -> Separators {
        /// Fewest bars a separated stretch should hold.
        const MIN_BARS: i64 = 10;
        let ladder = [Separators::Day, Separators::Week, Separators::Month];
        let Some(from) = ladder.iter().position(|s| *s == self) else { return Separators::None };
        ladder[from..]
            .iter()
            .copied()
            .find(|s| s.seconds() >= MIN_BARS * timeframe.seconds())
            .unwrap_or(Separators::None)
    }

    /// Which stretch a date falls in; a separator goes where this changes.
    fn period(self, date: NaiveDate) -> Option<i64> {
        match self {
            Separators::None => None,
            Separators::Day => Some(date.num_days_from_ce() as i64),
            Separators::Week => {
                let monday = date.num_days_from_ce() as i64 - date.weekday().num_days_from_monday() as i64;
                Some(monday)
            }
            Separators::Month => Some(date.year() as i64 * 12 + date.month0() as i64),
        }
    }

    /// Roughly how long the stretch is, to tell whether a bar is shorter.
    fn seconds(self) -> i64 {
        match self {
            Separators::None => i64::MAX,
            Separators::Day => 86_400,
            Separators::Week => 7 * 86_400,
            Separators::Month => 28 * 86_400,
        }
    }
}

/// Which stretch of each day a time window covers.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Window {
    /// The same wall-clock hours every day: London's 08:00 to 16:30. Minutes
    /// past midnight; an end at or before the start runs past midnight, the
    /// way the Asian session does.
    Clock { from: u32, to: u32 },
    /// A stretch counted from where a part of the instrument's own day
    /// begins: the first thirty minutes of the regular session is the opening
    /// range, wherever and whenever that session opens.
    Session { part: Part, offset: u32, minutes: u32 },
}

/// The windows people reach for, ready made.
///
/// Each keeps the clock of the market it is named after, so London opens at
/// eight in London through the weeks its clocks and New York's disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Preset {
    pub name: &'static str,
    pub window: PresetWindow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresetWindow {
    Clock { from: u32, to: u32, zone: &'static str },
    Session { part: Part, offset: u32, minutes: u32 },
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Opening range",
        window: PresetWindow::Session { part: Part::Regular, offset: 0, minutes: 30 },
    },
    Preset {
        name: "Initial balance",
        window: PresetWindow::Session { part: Part::Regular, offset: 0, minutes: 60 },
    },
    Preset {
        name: "Asia",
        window: PresetWindow::Clock { from: 9 * 60, to: 15 * 60, zone: "Asia/Tokyo" },
    },
    Preset {
        name: "London",
        window: PresetWindow::Clock { from: 8 * 60, to: 16 * 60 + 30, zone: "Europe/London" },
    },
    Preset {
        name: "New York",
        window: PresetWindow::Clock { from: 9 * 60 + 30, to: 16 * 60, zone: "America/New_York" },
    },
];

impl Preset {
    /// The window and the zone it is stored with.
    pub fn parts(&self) -> (Window, Option<String>) {
        match self.window {
            PresetWindow::Clock { from, to, zone } => {
                (Window::Clock { from, to }, Some(zone.to_string()))
            }
            PresetWindow::Session { part, offset, minutes } => {
                (Window::Session { part, offset, minutes }, None)
            }
        }
    }

    /// The preset these settings are, if they are one: shown as chosen while
    /// they match, and as nothing in particular once they have been changed.
    pub fn matching(window: &Window, zone: Option<&str>) -> Option<&'static Preset> {
        PRESETS.iter().find(|preset| {
            let (w, z) = preset.parts();
            w == *window && (matches!(w, Window::Session { .. }) || z.as_deref() == zone)
        })
    }
}

/// Which part of each day session levels are taken from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LevelSource {
    /// The regular session: the prior day's high, low and close.
    #[default]
    Regular,
    /// The extended hours before the regular session: the overnight on
    /// futures, the pre-market on a share.
    Extended,
    /// The whole trading day.
    Day,
}

impl LevelSource {
    pub const ALL: [LevelSource; 3] = [LevelSource::Regular, LevelSource::Extended, LevelSource::Day];

    pub fn key(self) -> &'static str {
        match self {
            LevelSource::Regular => "regular",
            LevelSource::Extended => "extended",
            LevelSource::Day => "day",
        }
    }

    pub fn from_key(key: &str) -> Option<LevelSource> {
        LevelSource::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            LevelSource::Regular => "Regular session",
            LevelSource::Extended => "Overnight and pre-market",
            LevelSource::Day => "Whole day",
        }
    }

    /// What the line's label starts with.
    fn prefix(self) -> &'static str {
        match self {
            LevelSource::Regular => "RTH",
            LevelSource::Extended => "ON",
            LevelSource::Day => "Day",
        }
    }
}

/// One price a period is summed up by.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Open,
    High,
    Low,
    Close,
    /// Halfway between the high and the low.
    Mid,
}

impl Level {
    pub const ALL: [Level; 5] = [Level::High, Level::Low, Level::Close, Level::Open, Level::Mid];

    pub fn key(self) -> &'static str {
        match self {
            Level::Open => "open",
            Level::High => "high",
            Level::Low => "low",
            Level::Close => "close",
            Level::Mid => "mid",
        }
    }

    pub fn from_key(key: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Level::Open => "Open",
            Level::High => "High",
            Level::Low => "Low",
            Level::Close => "Close",
            Level::Mid => "Midpoint",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Level::Open => "O",
            Level::High => "H",
            Level::Low => "L",
            Level::Close => "C",
            Level::Mid => "Mid",
        }
    }

    fn of(self, bars: &[Bar]) -> Option<f64> {
        let (first, last) = (bars.first()?, bars.last()?);
        let high = bars.iter().map(|b| b.high).fold(f64::MIN, f64::max);
        let low = bars.iter().map(|b| b.low).fold(f64::MAX, f64::min);
        Some(match self {
            Level::Open => first.open,
            Level::High => high,
            Level::Low => low,
            Level::Close => last.close,
            Level::Mid => (high + low) / 2.0,
        })
    }
}

/// A run of bars to shade, first and last inclusive.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub from: usize,
    pub to: usize,
}

/// A line in front of a bar, with what it marks.
#[derive(Clone, PartialEq, Debug)]
pub struct Marker {
    pub bar: usize,
    pub label: Option<String>,
}

/// A price held across a run of bars, first and last inclusive.
#[derive(Clone, PartialEq, Debug)]
pub struct LevelRun {
    pub from: usize,
    pub to: usize,
    pub price: f64,
    pub label: String,
}

/// What an indicator of time produces.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Time {
    pub spans: Vec<Span>,
    /// The shading's colour, unset for the indicator's own.
    pub fill: Option<ColorChoice>,
    pub fill_alpha: f64,
    pub markers: Vec<Marker>,
    pub levels: Vec<LevelRun>,
}

/// The fewest of the chart's height a shading is worth: below this it is not
/// there at all.
pub const MIN_SHADE_ALPHA: f64 = 0.02;
pub const MAX_SHADE_ALPHA: f64 = 0.6;

/// The extended hours shaded, and a line where each day begins.
pub fn sessions(
    bars: &[Bar],
    timeframe: Timeframe,
    schedule: Option<Schedule>,
    shade: bool,
    separators: Separators,
) -> Time {
    let mut time = Time::default();
    if bars.is_empty() {
        return time;
    }
    let separators = separators.effective_for(timeframe);
    // The date a bar belongs to: the exchange's trading day for an intraday
    // bar, and the calendar date the bar is stamped with for anything longer,
    // because a daily bar already is a day.
    let date = |bar: &Bar| match (timeframe.is_intraday(), schedule) {
        (true, Some(schedule)) => schedule.trading_date(bar.ts),
        _ => chrono::DateTime::from_timestamp(bar.ts, 0).map(|dt| dt.date_naive()),
    };
    let mut previous = None;
    for (at, bar) in bars.iter().enumerate() {
        let period = date(bar).and_then(|d| separators.period(d));
        if previous.is_some() && period.is_some() && period != previous {
            time.markers.push(Marker { bar: at, label: None });
        }
        if period.is_some() {
            previous = period;
        }
    }
    if shade
        && timeframe.is_intraday()
        && let Some(schedule) = schedule.filter(|s| s.has_regular())
    {
        // Anything outside the regular session is shaded, a bar between
        // sessions included: a print at a time the exchange is closed is
        // still not regular hours.
        let extended = |bar: &Bar| schedule.part_at(bar.ts) != Some(Part::Regular);
        time.spans = runs(bars, extended);
    }
    time
}

/// The runs of consecutive bars that pass `test`.
fn runs(bars: &[Bar], test: impl Fn(&Bar) -> bool) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut open: Option<usize> = None;
    for (at, bar) in bars.iter().enumerate() {
        match (test(bar), open) {
            (true, None) => open = Some(at),
            (false, Some(from)) => {
                spans.push(Span { from, to: at - 1 });
                open = None;
            }
            _ => {}
        }
    }
    if let Some(from) = open {
        spans.push(Span { from, to: bars.len() - 1 });
    }
    spans
}

/// The bars inside `start..end`, as a range of indexes.
fn inside(bars: &[Bar], start: i64, end: i64) -> std::ops::Range<usize> {
    let from = bars.partition_point(|b| b.ts < start);
    let to = bars.partition_point(|b| b.ts < end);
    from..to.max(from)
}

/// Each occurrence of a window across these bars, as instants.
fn occurrences(
    bars: &[Bar],
    window: &Window,
    zone: Tz,
    schedule: Option<Schedule>,
    weekdays: bool,
) -> Vec<(i64, i64)> {
    let (Some(first), Some(last)) = (bars.first(), bars.last()) else { return Vec::new() };
    match window {
        Window::Clock { from, to } => {
            let (Some((start, _)), Some((end, _))) = (wall(zone, first.ts), wall(zone, last.ts)) else {
                return Vec::new();
            };
            // From the day before, so a window that started yesterday evening
            // and runs into the first bar is counted.
            let start = start.pred_opt().unwrap_or(start);
            let length = if to > from { to - from } else { to + 1_440 - from };
            start
                .iter_days()
                .take_while(|date| *date <= end)
                .filter_map(|date| {
                    let begins = instant(zone, date, *from);
                    let ends = instant(zone, date, from + length);
                    // Judged by the day it ends on, so the Asian session that
                    // opens on Sunday evening is Monday's and the one that
                    // would open on Friday evening is nobody's.
                    let ending = wall(zone, ends - 1).map(|(d, _)| d.weekday())?;
                    let weekend = matches!(ending, Weekday::Sat | Weekday::Sun);
                    (!(weekdays && weekend)).then_some((begins, ends))
                })
                .collect()
        }
        Window::Session { part, offset, minutes } => {
            let Some(schedule) = schedule else { return Vec::new() };
            schedule
                .days(first.ts, last.ts)
                .into_iter()
                .filter_map(|day| {
                    let (_, begins, ends) = *day.segments.iter().find(|(p, ..)| p == part)?;
                    let start = begins + *offset as i64 * 60;
                    let stop = (start + *minutes as i64 * 60).min(ends);
                    (start < stop).then_some((start, stop))
                })
                .collect()
        }
    }
}

/// One window's occurrences: shaded, marked, and with its range carried on.
#[allow(clippy::too_many_arguments)]
pub fn window(
    bars: &[Bar],
    timeframe: Timeframe,
    name: &str,
    spec: &Window,
    zone: Tz,
    schedule: Option<Schedule>,
    weekdays: bool,
    shade: bool,
    mark: bool,
    range: bool,
) -> Time {
    let mut time = Time::default();
    // A window is a stretch of a day; a bar a day or longer has no inside.
    if !timeframe.is_intraday() {
        return time;
    }
    let found: Vec<std::ops::Range<usize>> = occurrences(bars, spec, zone, schedule, weekdays)
        .into_iter()
        .map(|(start, end)| inside(bars, start, end))
        .filter(|range| !range.is_empty())
        .collect();
    let label = (!name.trim().is_empty()).then(|| name.trim().to_string());
    for (at, bars_in) in found.iter().enumerate() {
        if shade {
            time.spans.push(Span { from: bars_in.start, to: bars_in.end - 1 });
        }
        if mark {
            time.markers.push(Marker { bar: bars_in.start, label: label.clone() });
        }
        if range {
            // Held from the window's start until the next one begins, so the
            // range is on screen while price trades around it.
            let until = found.get(at + 1).map(|next| next.start - 1).unwrap_or(bars.len() - 1);
            let slice = &bars[bars_in.clone()];
            for level in [Level::High, Level::Low] {
                let Some(price) = level.of(slice) else { continue };
                let label = match &label {
                    Some(name) => format!("{name} {}", level.short()),
                    None => level.short().to_string(),
                };
                time.levels.push(LevelRun { from: bars_in.start, to: until, price, label });
            }
        }
    }
    time
}

/// Each day's part summed up, and carried across the day after.
pub fn levels(
    bars: &[Bar],
    timeframe: Timeframe,
    schedule: Option<Schedule>,
    source: LevelSource,
    wanted: &[Level],
) -> Time {
    let mut time = Time::default();
    let Some(schedule) = schedule else { return time };
    if !timeframe.is_intraday() || bars.is_empty() || wanted.is_empty() {
        return time;
    }
    // A market with no regular session has only the whole day to offer.
    let source = match schedule.has_regular() {
        true => source,
        false => LevelSource::Day,
    };
    // The bars of each day's part, oldest day first.
    let mut periods: Vec<std::ops::Range<usize>> = Vec::new();
    for day in schedule.days(bars[0].ts, bars[bars.len() - 1].ts) {
        let regular_open = day.segments.iter().find(|(p, ..)| *p == Part::Regular).map(|(_, s, _)| *s);
        let picked: Vec<(i64, i64)> = match source {
            LevelSource::Day => vec![(day.start, day.end)],
            LevelSource::Regular => day
                .segments
                .iter()
                .filter(|(p, ..)| *p == Part::Regular)
                .map(|(_, s, e)| (*s, *e))
                .collect(),
            LevelSource::Extended => match regular_open {
                Some(open) => vec![(day.start, open)],
                None => Vec::new(),
            },
        };
        let (Some(start), Some(end)) = (picked.iter().map(|p| p.0).min(), picked.iter().map(|p| p.1).max())
        else {
            continue;
        };
        // Only the bars inside the picked segments, so Tokyo's lunch hour or
        // a stray print in the break is no part of its regular session.
        let range = inside(bars, start, end);
        let first = range.clone().find(|at| picked.iter().any(|(s, e)| (*s..*e).contains(&bars[*at].ts)));
        let last = range.clone().rev().find(|at| picked.iter().any(|(s, e)| (*s..*e).contains(&bars[*at].ts)));
        if let (Some(first), Some(last)) = (first, last) {
            periods.push(first..last + 1);
        }
    }
    for (at, period) in periods.iter().enumerate() {
        let from = period.end;
        if from >= bars.len() {
            break;
        }
        let to = periods.get(at + 1).map(|next| next.end - 1).unwrap_or(bars.len() - 1);
        let picked: Vec<&Bar> = bars[period.clone()]
            .iter()
            .filter(|bar| match source {
                LevelSource::Regular => schedule.is_regular(bar.ts),
                _ => true,
            })
            .collect();
        let picked: Vec<Bar> = picked.into_iter().copied().collect();
        for level in wanted {
            let Some(price) = level.of(&picked) else { continue };
            time.levels.push(LevelRun {
                from,
                to,
                price,
                label: format!("{} {}", source.prefix(), level.short()),
            });
        }
    }
    time
}

/// The schedule for this instrument, if its exchange is known.
pub fn schedule_for(instrument: Option<&Instrument>) -> Option<Schedule<'static>> {
    instrument.and_then(|i| registry().for_instrument(i))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use chrono_tz::America::{Chicago, New_York};
    use chrono_tz::Europe::London;

    use super::*;
    use crate::symbols::InstrumentKind;

    fn instrument(kind: InstrumentKind, symbol: &str) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: symbol.into(),
            kind,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    fn es() -> Option<Schedule<'static>> {
        schedule_for(Some(&instrument(InstrumentKind::FutureRoot, "ES")))
    }

    /// Half-hour ES bars from Sunday 2024-03-17 17:00 Chicago to Wednesday
    /// 16:00, through the daily breaks as Globex has them. Prices rise bar on
    /// bar, so a level says which bar it came from.
    fn half_hourly_es() -> Vec<Bar> {
        let start = Chicago.with_ymd_and_hms(2024, 3, 17, 17, 0, 0).unwrap().timestamp();
        let end = Chicago.with_ymd_and_hms(2024, 3, 20, 16, 0, 0).unwrap().timestamp();
        (start..end)
            .step_by(1800)
            .filter(|ts| es().unwrap().is_open(*ts))
            .enumerate()
            .map(|(at, ts)| Bar {
                ts,
                open: at as f64,
                high: at as f64 + 0.5,
                low: at as f64 - 0.5,
                close: at as f64 + 0.25,
                volume: 1.0,
            })
            .collect()
    }

    #[test]
    fn day_separators_fall_where_the_globex_day_starts() {
        let bars = half_hourly_es();
        let time = sessions(&bars, Timeframe::minutes(30), es(), false, Separators::Day);
        let at: Vec<String> = time
            .markers
            .iter()
            .map(|m| Chicago.timestamp_opt(bars[m.bar].ts, 0).unwrap().format("%a %H:%M").to_string())
            .collect();
        assert_eq!(at, ["Mon 17:00", "Tue 17:00"], "Tuesday and Wednesday begin the evening before");
    }

    #[test]
    fn extended_hours_are_shaded_and_the_cash_session_is_not() {
        let bars = half_hourly_es();
        let time = sessions(&bars, Timeframe::minutes(30), es(), true, Separators::None);
        assert!(time.markers.is_empty());
        for span in &time.spans {
            for bar in &bars[span.from..=span.to] {
                assert!(!es().unwrap().is_regular(bar.ts));
            }
        }
        let shaded: usize = time.spans.iter().map(|s| s.to - s.from + 1).sum();
        let regular = bars.iter().filter(|b| es().unwrap().is_regular(b.ts)).count();
        assert_eq!(shaded + regular, bars.len(), "every bar is one or the other");
        // Monday's overnight; each post-market joined to the next overnight
        // across the break, since no bar sits between them; and Wednesday's
        // post-market, which ends the data.
        assert_eq!(time.spans.len(), 4);
    }

    #[test]
    fn a_daily_chart_has_its_separators_promoted_to_months() {
        let bars: Vec<Bar> = (0..40)
            .map(|d| {
                let ts = chrono::Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap().timestamp() + d * 86_400;
                Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 }
            })
            .collect();
        let daily = Timeframe::days(1);
        // A line between every daily bar, or every fifth, is a grid: both are
        // promoted to a month's.
        assert_eq!(Separators::Day.effective_for(daily), Separators::Month);
        assert_eq!(Separators::Week.effective_for(daily), Separators::Month);
        assert_eq!(Separators::Day.effective_for(Timeframe::hours(1)), Separators::Day);
        assert_eq!(Separators::Week.effective_for(Timeframe::hours(4)), Separators::Week);
        assert_eq!(Separators::Day.effective_for(Timeframe::weeks(1)), Separators::None);
        assert_eq!(sessions(&bars, daily, es(), true, Separators::Day).markers.len(), 1, "1 April");
        assert!(sessions(&bars, daily, es(), true, Separators::Day).spans.is_empty());
        assert_eq!(sessions(&bars, daily, es(), false, Separators::Month).markers.len(), 1);
    }

    #[test]
    fn a_london_window_follows_londons_clock() {
        let bars = half_hourly_es();
        let time = window(
            &bars,
            Timeframe::minutes(30),
            "London",
            &Window::Clock { from: 8 * 60, to: 16 * 60 + 30 },
            London,
            es(),
            true,
            true,
            true,
            false,
        );
        // Monday to Wednesday, each opening at eight in London — which in
        // the weeks the two countries' clocks disagree is three in New York,
        // not four.
        assert_eq!(time.markers.len(), 3);
        for marker in &time.markers {
            let at = London.timestamp_opt(bars[marker.bar].ts, 0).unwrap();
            assert_eq!(at.format("%H:%M").to_string(), "08:00");
            assert_eq!(marker.label.as_deref(), Some("London"));
            let ny = New_York.timestamp_opt(bars[marker.bar].ts, 0).unwrap();
            assert_eq!(ny.format("%H:%M").to_string(), "04:00", "London is still on GMT in mid-March");
        }
        assert_eq!(time.spans.len(), 3);
    }

    /// The Asian session runs past midnight and opens on Sunday evening for
    /// Monday.
    #[test]
    fn a_window_past_midnight_belongs_to_the_day_it_ends_on() {
        let bars = half_hourly_es();
        let time = window(
            &bars,
            Timeframe::minutes(30),
            "Asia",
            &Window::Clock { from: 19 * 60, to: 3 * 60 },
            New_York,
            es(),
            true,
            true,
            true,
            false,
        );
        let starts: Vec<String> = time
            .markers
            .iter()
            .map(|m| New_York.timestamp_opt(bars[m.bar].ts, 0).unwrap().format("%a %H:%M").to_string())
            .collect();
        assert_eq!(starts, ["Sun 19:00", "Mon 19:00", "Tue 19:00"]);
    }

    #[test]
    fn the_opening_range_is_counted_from_the_regular_open() {
        let five = Timeframe::minutes(5);
        let open = Chicago.with_ymd_and_hms(2024, 3, 18, 8, 30, 0).unwrap().timestamp();
        let bars: Vec<Bar> = (-6..20)
            .map(|n| {
                let price: f64 = if n == 3 { 110.0 } else if n == 4 { 90.0 } else { 100.0 };
                Bar { ts: open + n * 300, open: 100.0, high: price.max(100.0), low: price.min(100.0), close: 100.0, volume: 1.0 }
            })
            .collect();
        let time = window(
            &bars,
            five,
            "OR",
            &Window::Session { part: Part::Regular, offset: 0, minutes: 30 },
            New_York,
            es(),
            true,
            true,
            true,
            true,
        );
        assert_eq!(time.spans, vec![Span { from: 6, to: 11 }], "six five-minute bars from the open");
        let high = time.levels.iter().find(|l| l.label == "OR H").unwrap();
        let low = time.levels.iter().find(|l| l.label == "OR L").unwrap();
        assert_eq!((high.price, low.price), (110.0, 90.0));
        assert_eq!((high.from, high.to), (6, bars.len() - 1), "held from the open to the last bar");
    }

    #[test]
    fn prior_regular_session_levels_are_carried_across_the_next_day() {
        let bars = half_hourly_es();
        let time = levels(&bars, Timeframe::minutes(30), es(), LevelSource::Regular, &[Level::High, Level::Low, Level::Close]);
        // Three days' worth: Wednesday's are carried into its own post-market,
        // which is the only stretch after it the data has.
        assert_eq!(time.levels.len(), 9);
        let monday_high = &time.levels[0];
        assert_eq!(monday_high.label, "RTH H");
        let regular: Vec<usize> = (0..bars.len()).filter(|at| es().unwrap().is_regular(bars[*at].ts)).collect();
        // Monday's regular session is the first run of regular bars.
        let monday_last = regular.iter().take_while(|at| **at < monday_high.from).max().copied().unwrap();
        assert_eq!(monday_high.from, monday_last + 1, "starts the bar after Monday's close");
        assert_eq!(monday_high.price, bars[monday_last].high, "the hours rise, so the last is the highest");
        let tuesday_last = regular.iter().copied().filter(|at| *at < time.levels[3].from).max().unwrap();
        assert_eq!(monday_high.to, tuesday_last, "held until Tuesday's session closes");
    }

    #[test]
    fn the_overnight_is_what_comes_before_the_open() {
        let bars = half_hourly_es();
        let time = levels(&bars, Timeframe::minutes(30), es(), LevelSource::Extended, &[Level::High, Level::Low]);
        let first = &time.levels[0];
        assert_eq!(first.label, "ON H");
        let at = Chicago.timestamp_opt(bars[first.from].ts, 0).unwrap();
        assert_eq!(at.format("%a %H:%M").to_string(), "Mon 08:30", "drawn from the regular open");
    }

    #[test]
    fn nothing_is_drawn_without_an_exchange_or_on_a_daily_chart() {
        let bars = half_hourly_es();
        assert!(levels(&bars, Timeframe::minutes(30), None, LevelSource::Regular, &[Level::High]).levels.is_empty());
        assert!(levels(&bars, Timeframe::days(1), es(), LevelSource::Regular, &[Level::High]).levels.is_empty());
    }

    #[test]
    fn keys_round_trip() {
        for s in Separators::ALL {
            assert_eq!(Separators::from_key(s.key()), Some(s));
        }
        for s in LevelSource::ALL {
            assert_eq!(LevelSource::from_key(s.key()), Some(s));
        }
        for l in Level::ALL {
            assert_eq!(Level::from_key(l.key()), Some(l));
        }
    }
}
