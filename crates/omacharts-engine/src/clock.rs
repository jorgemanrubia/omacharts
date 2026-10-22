//! Times of day, as people type them and as charts write them.
//!
//! Two preferences decide how a chart talks about time, and both live here so
//! the axis, the crosshair, the editors and the command line cannot come to
//! disagree: which clock it reads (the time zone) and how it writes what it
//! reads (twelve hours or twenty-four).

use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::exchanges::registry;
use crate::symbols::Instrument;

/// Twelve hours or twenty-four.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum TimeFormat {
    #[default]
    #[serde(rename = "24h")]
    H24,
    #[serde(rename = "12h")]
    H12,
}

impl TimeFormat {
    pub const ALL: [TimeFormat; 2] = [TimeFormat::H24, TimeFormat::H12];

    pub fn key(self) -> &'static str {
        match self {
            TimeFormat::H24 => "24h",
            TimeFormat::H12 => "12h",
        }
    }

    pub fn from_key(key: &str) -> Option<TimeFormat> {
        TimeFormat::ALL.into_iter().find(|f| f.key() == key.trim().to_ascii_lowercase())
    }

    pub fn label(self) -> &'static str {
        match self {
            TimeFormat::H24 => "24-hour (15:30)",
            TimeFormat::H12 => "12-hour (3:30 PM)",
        }
    }

    /// The `strftime` pattern for an hour and its minutes.
    pub fn pattern(self) -> &'static str {
        match self {
            TimeFormat::H24 => "%H:%M",
            TimeFormat::H12 => "%-I:%M %p",
        }
    }
}

/// Minutes past midnight, written the way this format writes them.
pub fn format_minutes(minutes: u32, format: TimeFormat) -> String {
    let (hours, minutes) = ((minutes / 60) % 24, minutes % 60);
    match format {
        TimeFormat::H24 => format!("{hours:02}:{minutes:02}"),
        TimeFormat::H12 => {
            let suffix = if hours < 12 { "AM" } else { "PM" };
            let twelve = match hours % 12 {
                0 => 12,
                h => h,
            };
            format!("{twelve}:{minutes:02} {suffix}")
        }
    }
}

/// A time of day as somebody typed it, in minutes past midnight.
///
/// Generous on purpose, because a field that refuses `9am` is a field people
/// stop typing into: `9`, `09`, `930`, `0930`, `093000`, `9:30`, `9.30`,
/// `09:30:00`, `9am`, `9:30pm`, `9:30 p.m.`, `21h30` and `15:30` all mean what
/// they look like. Seconds are accepted and dropped, since nothing here keeps
/// anything finer than a minute. `None` for anything that is not a time —
/// `25:00`, `13pm`, `noonish`.
pub fn parse_time(text: &str) -> Option<u32> {
    let compact: String =
        text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    let (body, meridiem) = split_meridiem(&compact);
    let parts: Vec<&str> = body.split([':', '.', 'h']).collect();
    let (hours, minutes) = match parts.as_slice() {
        [digits] if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) => {
            match digits.len() {
                1 | 2 => (digits.parse().ok()?, 0),
                3 => (digits[..1].parse().ok()?, digits[1..].parse().ok()?),
                4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
                5 => (digits[..1].parse().ok()?, digits[1..3].parse().ok()?),
                6 => (digits[..2].parse().ok()?, digits[2..4].parse().ok()?),
                _ => return None,
            }
        }
        // `21h` is the hour alone.
        [hours, ""] if body.ends_with('h') => (number(hours)?, 0),
        [hours, minutes] => (number(hours)?, number(minutes)?),
        [hours, minutes, seconds] => {
            number(seconds).filter(|s| *s < 60)?;
            (number(hours)?, number(minutes)?)
        }
        _ => return None,
    };
    if minutes >= 60 {
        return None;
    }
    let hours = match meridiem {
        None if hours < 24 => hours,
        Some(pm) if (1..=12).contains(&hours) => (hours % 12) + if pm { 12 } else { 0 },
        _ => return None,
    };
    Some(hours * 60 + minutes)
}

/// The text before an `am` or a `pm`, however it was spelled, and which of
/// the two it was.
fn split_meridiem(text: &str) -> (&str, Option<bool>) {
    for (suffix, pm) in [
        ("a.m.", false),
        ("p.m.", true),
        ("a.m", false),
        ("p.m", true),
        ("am", false),
        ("pm", true),
        ("a", false),
        ("p", true),
    ] {
        if let Some(body) = text.strip_suffix(suffix) {
            return (body, Some(pm));
        }
    }
    (text, None)
}

fn number(text: &str) -> Option<u32> {
    (!text.is_empty() && text.len() <= 2 && text.chars().all(|c| c.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Which clock charts read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChartZone {
    /// One zone for every chart.
    Fixed(Tz),
    /// Each chart in the clock of the exchange its instrument trades on.
    Exchange,
}

impl Default for ChartZone {
    /// New York, where most of what gets charted here is priced.
    fn default() -> ChartZone {
        ChartZone::Fixed(chrono_tz::America::New_York)
    }
}

impl ChartZone {
    /// What the setting stores: an IANA name, or `exchange`.
    pub fn key(self) -> String {
        match self {
            ChartZone::Fixed(zone) => zone.name().to_string(),
            ChartZone::Exchange => "exchange".to_string(),
        }
    }

    pub fn from_key(key: &str) -> Option<ChartZone> {
        let key = key.trim();
        if key.eq_ignore_ascii_case("exchange") {
            return Some(ChartZone::Exchange);
        }
        key.parse::<Tz>().ok().map(ChartZone::Fixed)
    }

    /// The zone a chart of this instrument reads: the exchange's own, when
    /// that is what was asked for and the exchange is known, and New York
    /// when it is not.
    pub fn for_instrument(self, instrument: Option<&Instrument>) -> Tz {
        match self {
            ChartZone::Fixed(zone) => zone,
            ChartZone::Exchange => instrument
                .and_then(|i| registry().for_instrument(i))
                .map(|s| s.zone())
                .unwrap_or(chrono_tz::America::New_York),
        }
    }

    pub fn label(self) -> String {
        match self {
            ChartZone::Exchange => "Each chart's exchange".to_string(),
            ChartZone::Fixed(zone) => zone_label(zone),
        }
    }
}

/// The zones offered in a list, in the order the markets open across a day.
/// Anything else can still be set by its IANA name.
pub const ZONES: &[&str] = &[
    "Pacific/Auckland",
    "Australia/Sydney",
    "Asia/Tokyo",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Hong_Kong",
    "Asia/Taipei",
    "Asia/Singapore",
    "Asia/Kolkata",
    "Asia/Dubai",
    "Europe/Moscow",
    "Europe/Athens",
    "Europe/Berlin",
    "Europe/Paris",
    "Europe/Madrid",
    "Europe/Zurich",
    "Europe/London",
    "UTC",
    "America/Sao_Paulo",
    "America/New_York",
    "America/Toronto",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
];

/// `America/New_York` as a person says it: "New York".
pub fn zone_label(zone: Tz) -> String {
    let name = zone.name();
    let city = name.rsplit('/').next().unwrap_or(name);
    city.replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_way_of_writing_a_time_is_read() {
        let cases = [
            ("9", 9 * 60),
            ("09", 9 * 60),
            ("930", 9 * 60 + 30),
            ("0930", 9 * 60 + 30),
            ("090000", 9 * 60),
            ("93000", 9 * 60 + 30),
            ("9:30", 9 * 60 + 30),
            ("9.30", 9 * 60 + 30),
            ("09:30:00", 9 * 60 + 30),
            ("15:30", 15 * 60 + 30),
            ("1530", 15 * 60 + 30),
            ("21h30", 21 * 60 + 30),
            ("21h", 21 * 60),
            ("9am", 9 * 60),
            ("9 am", 9 * 60),
            ("9AM", 9 * 60),
            ("9:30pm", 21 * 60 + 30),
            ("9:30 p.m.", 21 * 60 + 30),
            ("930p", 21 * 60 + 30),
            ("12am", 0),
            ("12pm", 12 * 60),
            ("12:15 am", 15),
            ("0:00", 0),
            ("23:59", 23 * 60 + 59),
            ("  8:00  ", 8 * 60),
        ];
        for (text, minutes) in cases {
            assert_eq!(parse_time(text), Some(minutes), "{text:?}");
        }
    }

    #[test]
    fn what_is_not_a_time_is_refused() {
        for text in ["", "25:00", "24:00", "9:60", "13pm", "0am", "noonish", "1234567", "9:3:0:0", "am", "9:30:61"] {
            assert_eq!(parse_time(text), None, "{text:?}");
        }
    }

    #[test]
    fn times_are_written_in_either_format() {
        assert_eq!(format_minutes(9 * 60 + 30, TimeFormat::H24), "09:30");
        assert_eq!(format_minutes(21 * 60 + 5, TimeFormat::H24), "21:05");
        assert_eq!(format_minutes(9 * 60 + 30, TimeFormat::H12), "9:30 AM");
        assert_eq!(format_minutes(21 * 60 + 5, TimeFormat::H12), "9:05 PM");
        assert_eq!(format_minutes(0, TimeFormat::H12), "12:00 AM");
        assert_eq!(format_minutes(12 * 60, TimeFormat::H12), "12:00 PM");
    }

    /// Whatever is written can be read back as the same time.
    #[test]
    fn what_is_written_reads_back() {
        for format in TimeFormat::ALL {
            for minutes in (0..1440).step_by(7) {
                assert_eq!(parse_time(&format_minutes(minutes, format)), Some(minutes));
            }
        }
    }

    #[test]
    fn keys_round_trip() {
        for format in TimeFormat::ALL {
            assert_eq!(TimeFormat::from_key(format.key()), Some(format));
        }
        assert_eq!(ChartZone::from_key("exchange"), Some(ChartZone::Exchange));
        assert_eq!(ChartZone::from_key("Europe/London").map(ChartZone::key).as_deref(), Some("Europe/London"));
        assert_eq!(ChartZone::from_key("Mars/Olympus"), None);
        assert_eq!(ChartZone::default().key(), "America/New_York");
        for name in ZONES {
            assert!(ChartZone::from_key(name).is_some(), "{name}");
        }
    }
}
