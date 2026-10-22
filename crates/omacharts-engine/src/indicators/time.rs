//! Session times: stretches of each day, shaded behind the bars.
//!
//! One indicator holding a list of ranges. Two of them are the instrument's
//! own day, as its exchange keeps it (see [`crate::exchanges`]): the extended
//! hours and the regular session. The rest are wall-clock hours — London's
//! session, the Asian one, a lunch hour — in the chart's time zone or one of
//! their own.
//!
//! Each range is shaded beneath everything on the chart, across the price plot
//! and every strip. Unless somebody picks a colour, the shading darkens the
//! chart's ground rather than tinting it: the extended hours set back from the
//! session people trade, so the eye finds the session without being shown
//! anything.
//!
//! What it produces is counted in bars rather than in pixels or seconds,
//! because that is how the chart lays time out — a weekend takes no room —
//! and so the renderer places it with the same arithmetic as the candles.

use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::bars::{Bar, Timeframe};
use crate::exchanges::{instant, registry, wall, Part, Schedule};
use crate::symbols::Instrument;
use crate::theme::ColorChoice;

/// Which stretch of each day a range covers.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Hours {
    /// Everything outside the regular session, by the instrument's exchange:
    /// pre-market, post-market and overnight.
    Eth,
    /// The regular session, by the instrument's exchange.
    Rth,
    /// The same wall-clock hours every day, in minutes past midnight. An end
    /// at or before the start runs past midnight, the way the Asian session
    /// does in New York. `zone` is an IANA name; unset keeps the chart's own
    /// clock, which is the one the hours were typed in.
    Clock {
        from: u32,
        to: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        zone: Option<String>,
    },
}

/// One range: its name, its hours and how it is shaded.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TimeRange {
    pub name: String,
    pub hours: Hours,
    /// Unset darkens the chart's ground; see [`shade`].
    #[serde(default)]
    pub color: Option<ColorChoice>,
    /// How solid the shading is.
    pub alpha: f64,
}

/// How solid a range starts: enough to tell the stretch apart, nowhere near
/// enough for the candles inside it to look dimmed. It is a background, and a
/// background the eye notices first has stopped being one.
pub const DEFAULT_ALPHA: f64 = 0.09;

/// How solid ETH starts. Its lift is towards the theme's text, which is a
/// long way from the ground, so the same few levels of difference take a
/// third of the strength a darkening does.
pub const ETH_ALPHA: f64 = 0.03;

/// The paint a range with no colour of its own gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ground {
    /// Lifted towards the theme's text: the extended hours, a shade off the
    /// ground the regular session sits on. Lighter on a dark theme and greyer
    /// on a light one — either way, further from the session people trade.
    Lifted,
    /// Darkened: anything else, set back from what is around it.
    Darkened,
}

/// What a range is painted with: its own colour at its own strength, or with
/// none chosen, the ground lifted or darkened.
///
/// A darkening is half as strong on a light theme, where the same black reads
/// twice as dark.
pub fn shade<'a>(
    color: Option<&ColorChoice>,
    alpha: f64,
    ground: Ground,
    theme: &'a crate::theme::Theme,
) -> (std::borrow::Cow<'a, str>, f64) {
    use std::borrow::Cow;
    match (color, ground) {
        (Some(choice), _) => (Cow::Owned(choice.resolve(theme)), alpha),
        (None, Ground::Lifted) => (Cow::Borrowed(theme.ui.text.as_str()), alpha),
        (None, Ground::Darkened) => {
            let strength = match theme.mode {
                crate::theme::Mode::Dark => 1.0,
                crate::theme::Mode::Light => 0.5,
            };
            (Cow::Borrowed("#000000"), alpha * strength)
        }
    }
}

impl Hours {
    /// How a range over these hours is painted when nobody chose a colour.
    pub fn ground(&self) -> Ground {
        match self {
            Hours::Eth => Ground::Lifted,
            _ => Ground::Darkened,
        }
    }
}

/// The least and the most a shading can be: below the one it is not there at
/// all, and past the other it is no longer a background.
pub const MIN_ALPHA: f64 = 0.02;
pub const MAX_ALPHA: f64 = 0.6;

/// The ranges people reach for, ready made.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    Eth,
    Rth,
    Asia,
    London,
    NewYork,
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 6] =
        [Preset::Eth, Preset::Rth, Preset::Asia, Preset::London, Preset::NewYork, Preset::Custom];

    pub fn key(self) -> &'static str {
        match self {
            Preset::Eth => "eth",
            Preset::Rth => "rth",
            Preset::Asia => "asia",
            Preset::London => "london",
            Preset::NewYork => "new_york",
            Preset::Custom => "custom",
        }
    }

    pub fn from_key(key: &str) -> Option<Preset> {
        Preset::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Whether this preset is already among `ranges`, where a second would
    /// only shade the same bars twice. True only for ETH and RTH, which are
    /// the same stretch on every chart; two London ranges can differ in
    /// everything but their name.
    pub fn is_among(self, ranges: &[TimeRange]) -> bool {
        let hours = match self {
            Preset::Eth => Hours::Eth,
            Preset::Rth => Hours::Rth,
            _ => return false,
        };
        ranges.iter().any(|range| range.hours == hours)
    }

    /// What the menu offering it says.
    pub fn label(self) -> &'static str {
        match self {
            Preset::Eth => "ETH, extended hours",
            Preset::Rth => "RTH, regular hours",
            Preset::Asia => "Asia",
            Preset::London => "London",
            Preset::NewYork => "New York",
            Preset::Custom => "Custom",
        }
    }

    /// The range it makes. Each market named after a city keeps that city's
    /// clock, so London opens at eight in London through the weeks its clocks
    /// and New York's disagree.
    pub fn range(self) -> TimeRange {
        let clock = |from: u32, to: u32, zone: Option<&str>| Hours::Clock {
            from,
            to,
            zone: zone.map(str::to_string),
        };
        let (name, hours) = match self {
            Preset::Eth => ("ETH", Hours::Eth),
            Preset::Rth => ("RTH", Hours::Rth),
            Preset::Asia => ("Asia", clock(9 * 60, 15 * 60, Some("Asia/Tokyo"))),
            Preset::London => ("London", clock(8 * 60, 16 * 60 + 30, Some("Europe/London"))),
            Preset::NewYork => ("New York", clock(9 * 60 + 30, 16 * 60, Some("America/New_York"))),
            Preset::Custom => ("Custom", clock(12 * 60, 13 * 60, None)),
        };
        let alpha = match hours {
            Hours::Eth => ETH_ALPHA,
            _ => DEFAULT_ALPHA,
        };
        TimeRange { name: name.to_string(), hours, color: None, alpha }
    }
}

/// A run of bars to shade, first and last inclusive, and how.
#[derive(Clone, PartialEq, Debug)]
pub struct Span {
    pub from: usize,
    pub to: usize,
    /// Unset paints the ground; see [`shade`].
    pub color: Option<ColorChoice>,
    pub alpha: f64,
    pub ground: Ground,
}

/// What an indicator of time produces.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Time {
    pub spans: Vec<Span>,
}

/// Every range shaded across these bars.
///
/// Nothing on a daily or longer chart: a bar that long has no inside to
/// shade. ETH and RTH need the instrument's exchange, and a market with no
/// regular session — FX, crypto — has neither to tell apart.
pub fn session_times(
    bars: &[Bar],
    timeframe: Timeframe,
    ranges: &[TimeRange],
    zone: Tz,
    schedule: Option<Schedule>,
) -> Time {
    let mut time = Time::default();
    if !timeframe.is_intraday() || bars.is_empty() {
        return time;
    }
    for range in ranges {
        let alpha = range.alpha.clamp(MIN_ALPHA, MAX_ALPHA);
        let found: Vec<std::ops::Range<usize>> = match &range.hours {
            Hours::Eth | Hours::Rth => {
                let Some(schedule) = schedule.filter(|s| s.has_regular()) else { continue };
                let regular = matches!(range.hours, Hours::Rth);
                // Outside the regular session is extended, a bar between
                // sessions included: a print while the exchange is shut is
                // still not regular hours.
                runs(bars, |bar| (schedule.part_at(bar.ts) == Some(Part::Regular)) == regular)
            }
            Hours::Clock { from, to, zone: own } => {
                let zone = own.as_deref().and_then(|z| z.parse::<Tz>().ok()).unwrap_or(zone);
                occurrences(bars, *from, *to, zone)
                    .into_iter()
                    .map(|(start, end)| inside(bars, start, end))
                    .filter(|run| !run.is_empty())
                    .collect()
            }
        };
        time.spans.extend(found.into_iter().map(|run| Span {
            from: run.start,
            to: run.end - 1,
            color: range.color.clone(),
            alpha,
            ground: range.hours.ground(),
        }));
    }
    time
}

/// The runs of consecutive bars that pass `test`.
fn runs(bars: &[Bar], test: impl Fn(&Bar) -> bool) -> Vec<std::ops::Range<usize>> {
    let mut found = Vec::new();
    let mut open: Option<usize> = None;
    for (at, bar) in bars.iter().enumerate() {
        match (test(bar), open) {
            (true, None) => open = Some(at),
            (false, Some(from)) => {
                found.push(from..at);
                open = None;
            }
            _ => {}
        }
    }
    if let Some(from) = open {
        found.push(from..bars.len());
    }
    found
}

/// The bars inside `start..end`, as a range of indexes.
fn inside(bars: &[Bar], start: i64, end: i64) -> std::ops::Range<usize> {
    let from = bars.partition_point(|b| b.ts < start);
    let to = bars.partition_point(|b| b.ts < end);
    from..to.max(from)
}

/// Each occurrence of a wall-clock range across these bars, as instants.
///
/// One a day, weekends included: a day with no bars in it shades nothing, so
/// a market that is shut at the weekend draws none there, and one that never
/// shuts draws them all.
fn occurrences(bars: &[Bar], from: u32, to: u32, zone: Tz) -> Vec<(i64, i64)> {
    let (Some(first), Some(last)) = (bars.first(), bars.last()) else { return Vec::new() };
    let (Some((start, _)), Some((end, _))) = (wall(zone, first.ts), wall(zone, last.ts)) else {
        return Vec::new();
    };
    // From the day before, so a range that started yesterday evening and runs
    // into the first bar is counted.
    let start: NaiveDate = start.pred_opt().unwrap_or(start);
    let length = if to > from { to - from } else { to + 1_440 - from };
    start
        .iter_days()
        .take_while(|date| *date <= end)
        .map(|date| (instant(zone, date, from), instant(zone, date, from + length)))
        .collect()
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
    /// 16:00, through the daily breaks as Globex has them.
    fn half_hourly_es() -> Vec<Bar> {
        let start = Chicago.with_ymd_and_hms(2024, 3, 17, 17, 0, 0).unwrap().timestamp();
        let end = Chicago.with_ymd_and_hms(2024, 3, 20, 16, 0, 0).unwrap().timestamp();
        (start..end)
            .step_by(1800)
            .filter(|ts| es().unwrap().is_open(*ts))
            .map(|ts| Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 })
            .collect()
    }

    fn shaded(time: &Time) -> usize {
        time.spans.iter().map(|s| s.to - s.from + 1).sum()
    }

    fn half_hour() -> Timeframe {
        Timeframe::minutes(30)
    }

    #[test]
    fn eth_is_everything_the_regular_session_is_not() {
        let bars = half_hourly_es();
        let eth = session_times(&bars, half_hour(), &[Preset::Eth.range()], New_York, es());
        let rth = session_times(&bars, half_hour(), &[Preset::Rth.range()], New_York, es());
        for span in &eth.spans {
            for bar in &bars[span.from..=span.to] {
                assert!(!es().unwrap().is_regular(bar.ts));
            }
        }
        assert_eq!(shaded(&eth) + shaded(&rth), bars.len(), "every bar is one or the other");
        assert_eq!(rth.spans.len(), 3, "one regular session a day");
        // Monday's overnight; each post-market joined to the next overnight
        // across the break, since no bar sits between them; and Wednesday's
        // post-market, which ends the data.
        assert_eq!(eth.spans.len(), 4);
        assert!(eth.spans.iter().all(|s| s.color.is_none() && s.alpha == ETH_ALPHA));
        assert!(eth.spans.iter().all(|s| s.ground == Ground::Lifted));
        assert!(rth.spans.iter().all(|s| s.ground == Ground::Darkened));
    }

    #[test]
    fn london_keeps_londons_clock() {
        let bars = half_hourly_es();
        let time = session_times(&bars, half_hour(), &[Preset::London.range()], New_York, es());
        assert_eq!(time.spans.len(), 3, "Monday to Wednesday");
        for span in &time.spans {
            let opens = London.timestamp_opt(bars[span.from].ts, 0).unwrap();
            assert_eq!(opens.format("%H:%M").to_string(), "08:00");
            // Mid-March, London is still on GMT and New York already on
            // daylight time: four hours apart, not five.
            let there = New_York.timestamp_opt(bars[span.from].ts, 0).unwrap();
            assert_eq!(there.format("%H:%M").to_string(), "04:00");
        }
    }

    /// A range typed with no zone of its own keeps the chart's clock.
    #[test]
    fn a_custom_range_keeps_the_charts_clock() {
        let bars = half_hourly_es();
        let lunch = TimeRange {
            name: "Lunch".into(),
            hours: Hours::Clock { from: 12 * 60, to: 13 * 60, zone: None },
            color: Some(ColorChoice::swatch("Blue")),
            alpha: 0.2,
        };
        let time = session_times(&bars, half_hour(), &[lunch], New_York, es());
        assert_eq!(time.spans.len(), 3);
        for span in &time.spans {
            assert_eq!(span.to - span.from + 1, 2, "two half hours");
            let at = New_York.timestamp_opt(bars[span.from].ts, 0).unwrap();
            assert_eq!(at.format("%H:%M").to_string(), "12:00");
            assert_eq!(span.color, Some(ColorChoice::swatch("Blue")));
        }
    }

    /// The Asian session in New York time runs past midnight.
    #[test]
    fn a_range_past_midnight_is_one_stretch() {
        let bars = half_hourly_es();
        let asia = TimeRange {
            name: "Asia".into(),
            hours: Hours::Clock { from: 19 * 60, to: 3 * 60, zone: None },
            color: None,
            alpha: DEFAULT_ALPHA,
        };
        let time = session_times(&bars, half_hour(), &[asia], New_York, es());
        let starts: Vec<String> = time
            .spans
            .iter()
            .map(|s| New_York.timestamp_opt(bars[s.from].ts, 0).unwrap().format("%a %H:%M").to_string())
            .collect();
        assert_eq!(starts, ["Sun 19:00", "Mon 19:00", "Tue 19:00"]);
        assert!(time.spans.iter().all(|s| s.to - s.from + 1 == 16), "eight hours each");
    }

    #[test]
    fn nothing_is_shaded_on_a_daily_chart_and_eth_needs_an_exchange() {
        let bars = half_hourly_es();
        let ranges = [Preset::Eth.range(), Preset::London.range()];
        assert!(session_times(&bars, Timeframe::days(1), &ranges, New_York, es()).spans.is_empty());
        let without = session_times(&bars, half_hour(), &ranges, New_York, None);
        assert_eq!(without.spans.len(), 3, "London still shades; ETH has nothing to go on");
    }

    #[test]
    fn a_market_that_never_shuts_has_no_extended_hours() {
        let btc = schedule_for(Some(&instrument(InstrumentKind::Crypto, "BTC")));
        let bars = half_hourly_es();
        let time = session_times(&bars, half_hour(), &[Preset::Eth.range()], New_York, btc);
        assert!(time.spans.is_empty());
    }

    #[test]
    fn eth_and_rth_are_offered_once() {
        let ranges = [Preset::Eth.range(), Preset::London.range()];
        assert!(Preset::Eth.is_among(&ranges));
        assert!(!Preset::Rth.is_among(&ranges));
        assert!(!Preset::London.is_among(&ranges), "a second London can be another London");
    }

    /// The regular session is the ground; the extended hours sit a shade off
    /// it — lighter on a dark theme — and a range set back is darker.
    #[test]
    fn unset_ranges_lift_eth_and_darken_the_rest() {
        let theme = crate::theme::builtin_themes().into_iter().find(|t| t.mode == crate::theme::Mode::Dark).unwrap();
        let read = |(colour, alpha): (std::borrow::Cow<str>, f64)| (colour.into_owned(), alpha);
        assert_eq!(read(shade(None, 0.03, Ground::Lifted, &theme)), (theme.ui.text.clone(), 0.03));
        assert_eq!(read(shade(None, 0.09, Ground::Darkened, &theme)), ("#000000".to_string(), 0.09));
        let blue = ColorChoice::Fixed { hex: "#0000ff".into() };
        assert_eq!(read(shade(Some(&blue), 0.2, Ground::Lifted, &theme)), ("#0000ff".to_string(), 0.2));
    }

    #[test]
    fn preset_keys_round_trip() {
        for preset in Preset::ALL {
            assert_eq!(Preset::from_key(preset.key()), Some(preset));
        }
    }
}
