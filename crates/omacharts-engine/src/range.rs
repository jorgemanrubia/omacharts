//! How much of the past a chart shows, named the way people say it: a
//! year back from today, or everything since the first of January.
//!
//! A range is a stretch of the calendar, not a count of bars. A year of a
//! stock is about 250 daily bars and a year of bitcoin is 365, so a chart
//! told "1Y" has to count its own bars each time it is given a series —
//! which is why it is kept as a range and not turned into a zoom once.
//!
//! "Today" is the day of the last bar, not the clock's. A series that
//! stopped trading last spring still has a year before its end worth
//! looking at, and on a Sunday the week to date is the week that just
//! traded.

use chrono::{Datelike, Days, Duration, Months, NaiveDate, NaiveDateTime, NaiveTime, Weekday};

use crate::bars::Timeframe;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    /// So many days, weeks, months or years back from today, today included.
    Back(u32, Span),
    /// From the first day of the current week, month, quarter or year, or
    /// of so many before it: `ToDate(2, Year)` in 2026 runs from 1 January
    /// 2025. One is the period to date.
    ToDate(u32, Period),
    /// The whole series.
    All,
    /// One calendar year, January to December.
    Year(i32),
    /// One calendar month: the year, then the month from 1.
    Month(i32, u32),
    /// One calendar month named without a year, `june` or `jan`: the
    /// latest one, this month's when it is that month now.
    LatestMonth(u32),
    /// One day: a date, `mon`, `10/12`, `yesterday`, `3 weeks ago`. Always
    /// one of the [`End`]s that is a day.
    Day(End),
    /// From one day on, for as long as the chart showed when it was asked:
    /// a [`Range::Day`] typed on a chart, which starts the chart at the
    /// beginning of that day and keeps the length of time on screen. The
    /// day as typed, then that length in seconds, from the first moment on
    /// screen to the last as the time axis reads them, empty room included,
    /// then how many seconds before the day the window starts. That is none
    /// unless the length would have run past now, when the window ends at
    /// now instead and starts as much earlier: the day falls inside it
    /// rather than at its left edge, and the length is still the one kept.
    Lasting(End, i64, i64),
    /// From one end to the other, written the way pandas slices dates:
    /// `2025:` from 1 January 2025 to today, `2024:2025-06` the two whole,
    /// `:2020` from the first bar, `-2y:-1y` the year before last. Both ends
    /// are included, so `2024:2025` holds all of 2025. A slice with neither
    /// end, `:`, is [`Range::All`].
    Slice(Option<End>, Option<End>),
}

/// One end of a [`Range::Slice`]: a day, a month, a year, or so long ago.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    Day(NaiveDate),
    Month(i32, u32),
    Year(i32),
    /// So many days, weeks, months or years before today: `-6m`.
    Ago(u32, Span),
    /// The latest of a weekday, today counting, or so many before it:
    /// `mon` is this week's Monday, today on a Monday, and `2mon` the one
    /// before that.
    Weekday(u32, Weekday),
    /// A month and a day with no year: the latest not after today, so this
    /// year's unless it is still to come.
    MonthDay(u32, u32),
    Today,
    Yesterday,
    /// So many days, weeks, months or years before today, to the day:
    /// `3 weeks ago` on 1 November is 11 October. Not [`End::Ago`], which
    /// is where the last three weeks begin, a day later.
    Before(u32, Span),
    /// The latest of a weekday before today: `last fri`, which on a Friday
    /// is a week ago.
    Last(Weekday),
    /// So long after today: `tomorrow`, `in 2 days`. Read only so as to be
    /// refused as a day still to come, with the same words as `2030`.
    After(u32, Span),
    /// The next of a weekday after today, `next fri`; refused the same way.
    Next(Weekday),
    /// A month by name with no year, `jan`: the latest, this one included.
    LatestMonth(u32),
}

impl End {
    /// `2025`, `2025-06`, `2025-06-30`, `-6m`, or any other day
    /// [`one_day`] reads.
    pub fn parse(text: &str) -> Option<End> {
        let text = text.trim();
        if let Some(ago) = text.strip_prefix('-') {
            if let Some((count, span)) = count_and_unit(ago).filter(|_| !ago.trim().is_empty()) {
                return Some(End::Ago(count, span));
            }
            // `-2mon` is `2mon`: a weekday already counts back.
            return one_day(ago).filter(|end| matches!(end, End::Weekday(..)));
        }
        if let Some(day) = one_day(text) {
            return Some(day);
        }
        match calendar(text).or_else(|| named_month(text))? {
            Range::Year(year) => Some(End::Year(year)),
            Range::Month(year, month) => Some(End::Month(year, month)),
            Range::LatestMonth(month) => Some(End::LatestMonth(month)),
            _ => None,
        }
    }

    /// What reads back as the same end.
    pub fn key(self) -> String {
        match self {
            End::Day(day) => day.format("%Y-%m-%d").to_string(),
            End::Month(year, month) => format!("{year}-{month:02}"),
            End::Year(year) => year.to_string(),
            End::Ago(count, span) => format!("-{}", Range::Back(count, span).key()),
            End::Weekday(1, weekday) => weekday.to_string().to_lowercase(),
            End::Weekday(count, weekday) => format!("{count}{}", weekday.to_string().to_lowercase()),
            End::MonthDay(month, day) => format!("{month:02}-{day:02}"),
            End::Today => "today".into(),
            End::Yesterday => "yesterday".into(),
            End::Before(count, span) => format!("{count}{} ago", span.letter()),
            End::Last(weekday) => format!("last {}", weekday.to_string().to_lowercase()),
            End::After(1, Span::Day) => "tomorrow".into(),
            End::After(count, span) => format!("in {count}{}", span.letter()),
            End::Next(weekday) => format!("next {}", weekday.to_string().to_lowercase()),
            End::LatestMonth(month) => month_name(month).to_lowercase(),
        }
    }

    /// The first day it holds. `-1y` starts where `1y` does, so `-1y:` and
    /// `1y` are the same stretch.
    pub fn first(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            End::Day(day) => Some(day),
            End::Month(year, month) => NaiveDate::from_ymd_opt(year, month, 1),
            End::Year(year) => NaiveDate::from_ymd_opt(year, 1, 1),
            End::Ago(count, span) => Range::Back(count, span).start(today),
            End::Weekday(count, weekday) => {
                let since = (7 + today.weekday().num_days_from_monday() - weekday.num_days_from_monday()) % 7;
                today.checked_sub_days(Days::new(u64::from(since) + 7 * u64::from(count.max(1) - 1)))
            }
            End::MonthDay(month, day) => (0..8)
                .filter_map(|back| NaiveDate::from_ymd_opt(today.year() - back, month, day))
                .find(|date| *date <= today),
            End::Today => Some(today),
            End::Yesterday => today.pred_opt(),
            End::Before(count, span) => match span {
                Span::Day => today.checked_sub_days(Days::new(u64::from(count))),
                Span::Week => today.checked_sub_days(Days::new(7 * u64::from(count))),
                Span::Month => today.checked_sub_months(Months::new(count)),
                Span::Year => today.checked_sub_months(Months::new(12 * count)),
            },
            End::After(count, span) => match span {
                Span::Day => today.checked_add_days(Days::new(u64::from(count))),
                Span::Week => today.checked_add_days(Days::new(7 * u64::from(count))),
                Span::Month => today.checked_add_months(Months::new(count)),
                Span::Year => today.checked_add_months(Months::new(12 * count)),
            },
            End::Last(weekday) => {
                let since = (7 + today.weekday().num_days_from_monday() - weekday.num_days_from_monday()) % 7;
                today.checked_sub_days(Days::new(u64::from(if since == 0 { 7 } else { since })))
            }
            End::Next(weekday) => {
                let until = (7 + weekday.num_days_from_monday() - today.weekday().num_days_from_monday()) % 7;
                today.checked_add_days(Days::new(u64::from(if until == 0 { 7 } else { until })))
            }
            End::LatestMonth(month) => {
                let (year, month) = latest_month(month, today);
                NaiveDate::from_ymd_opt(year, month, 1)
            }
        }
    }

    /// The last day it holds: the day before a `-6m` slice would start.
    pub fn last(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            End::Day(day) => Some(day),
            End::Month(year, month) => {
                NaiveDate::from_ymd_opt(year, month, 1)?.checked_add_months(Months::new(1))?.pred_opt()
            }
            End::LatestMonth(month) => {
                let (year, month) = latest_month(month, today);
                End::Month(year, month).last(today)
            }
            End::Year(year) => NaiveDate::from_ymd_opt(year, 12, 31),
            End::Ago(..) => self.first(today)?.pred_opt(),
            End::Weekday(..)
            | End::MonthDay(..)
            | End::Today
            | End::Yesterday
            | End::Before(..)
            | End::Last(..)
            | End::After(..)
            | End::Next(..) => self.first(today),
        }
    }

    fn words(self) -> String {
        match self {
            End::Day(day) => day.format("%-d %b %Y").to_string(),
            End::Month(year, month) => match NaiveDate::from_ymd_opt(year, month, 1) {
                Some(first) => first.format("%B %Y").to_string(),
                None => self.key(),
            },
            End::Year(year) => year.to_string(),
            End::Ago(count, span) | End::Before(count, span) => format!("{} ago", span.counted(count)),
            End::After(1, Span::Day) => "tomorrow".into(),
            End::After(count, span) => format!("in {}", span.counted(count)),
            End::Last(weekday) => format!("last {}", day_name(weekday)),
            End::Next(weekday) => format!("next {}", day_name(weekday)),
            End::LatestMonth(month) => month_name(month),
            End::Weekday(count, weekday) => {
                let name = day_name(weekday);
                match count {
                    1 => name,
                    2 => format!("{name} a week before"),
                    _ => format!("{name} {} weeks before", count - 1),
                }
            }
            End::MonthDay(month, day) => match NaiveDate::from_ymd_opt(2000, month, day) {
                Some(date) => date.format("%-d %b").to_string(),
                None => self.key(),
            },
            End::Today => "today".into(),
            End::Yesterday => "yesterday".into(),
        }
    }
}

/// One day, however it is written: `2026-03-12` or `2026/3/12`; `10/12`, a
/// month and a day with no year; `12 mar`, `mar 12 2025`; a weekday, `mon`
/// or `2fri`; `today` or `yesterday`. Numbers run year, month, day, the way
/// the rest of the grammar does, so `10/12` is 12 October.
fn one_day(text: &str) -> Option<End> {
    let lower = text.trim().to_ascii_lowercase();
    // `on 22 oct`, `on the 22 oct`: said the way a day is, and the same day.
    let lower = match lower.strip_prefix("on the ").or_else(|| lower.strip_prefix("on ")) {
        Some(rest) => rest.trim_start().to_string(),
        None => lower,
    };
    match lower.as_str() {
        "today" => return Some(End::Today),
        "yesterday" => return Some(End::Yesterday),
        "tomorrow" => return Some(End::After(1, Span::Day)),
        _ => {}
    }
    if let Some(rest) = lower.strip_suffix("ago") {
        let (count, span) = count_and_unit(rest).filter(|_| !rest.trim().is_empty())?;
        return Some(End::Before(count, span));
    }
    if let Some(rest) = lower.strip_suffix("from now").or_else(|| lower.strip_prefix("in ")) {
        let (count, span) = count_and_unit(rest).filter(|_| !rest.trim().is_empty())?;
        return Some(End::After(count, span));
    }
    if let Some(rest) = lower.strip_prefix("last ") {
        return weekday(rest.trim()).map(End::Last);
    }
    if let Some(rest) = lower.strip_prefix("next ") {
        return weekday(rest.trim()).map(End::Next);
    }
    let split = lower.find(|c: char| !c.is_ascii_digit()).unwrap_or(lower.len());
    let (digits, word) = lower.split_at(split);
    if let Some(weekday) = weekday(word) {
        let count = match digits {
            "" => 1,
            digits => digits.parse().ok().filter(|&n| n > 0 && n <= 100)?,
        };
        return Some(End::Weekday(count, weekday));
    }
    // Numbers with one separator between them, the same each time.
    for separator in ['-', '/'] {
        let parts: Vec<&str> = lower.split(separator).collect();
        let numbers = parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
        if parts.len() < 2 || !numbers {
            continue;
        }
        return match parts[..] {
            [year, month, day] if year.len() == 4 && month.len() <= 2 && day.len() <= 2 => {
                let date = NaiveDate::from_ymd_opt(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?)?;
                (1800..=2200).contains(&date.year()).then_some(End::Day(date))
            }
            [month, day] if month.len() <= 2 && day.len() <= 2 => month_day(month.parse().ok()?, day.parse().ok()?),
            _ => None,
        };
    }
    // A month by name, a day, and maybe a year, in any order and with or
    // without spaces between: `12 mar`, `12mar`, `mar 12 2025`.
    let mut tokens: Vec<String> = Vec::new();
    for c in lower.chars() {
        match c {
            ' ' | ',' => tokens.push(String::new()),
            _ => match tokens.last_mut() {
                Some(last) if last.chars().last().is_some_and(|l| l.is_ascii_digit() == c.is_ascii_digit()) => last.push(c),
                _ => tokens.push(c.to_string()),
            },
        }
    }
    tokens.retain(|token| !token.is_empty());
    let (mut month, mut day, mut year) = (None, None, None);
    for token in &tokens {
        match token.parse::<u32>() {
            Ok(n) if token.len() == 4 && year.is_none() => year = Some(n as i32),
            Ok(n) if token.len() <= 2 && day.is_none() => day = Some(n),
            Ok(_) => return None,
            Err(_) if month.is_none() => month = Some(month_named(token)?),
            Err(_) => return None,
        }
    }
    match (month?, day?, year) {
        (month, day, Some(year)) => {
            let date = NaiveDate::from_ymd_opt(year, month, day)?;
            (1800..=2200).contains(&year).then_some(End::Day(date))
        }
        (month, day, None) => month_day(month, day),
    }
}

/// A count and a unit, read the same wherever the grammar takes one: a
/// span (`3d`), so long ago (`3 days ago`), an end of a slice (`-3 d:`).
/// The count is digits, or `a` or `an` for one, or nothing for one; then
/// any spaces; then the unit in any case and any of its spellings:
///
/// - `d`, `dy`, `day`, `days`
/// - `w`, `wk`, `wks`, `week`, `weeks`
/// - `m`, `mo`, `mos`, `mth`, `mths`, `month`, `months`
/// - `q`, `qtr`, `qtrs`, `quarter`, `quarters`, three months each
/// - `y`, `yr`, `yrs`, `year`, `years`
///
/// `m` is a month, as it is everywhere in this grammar. A capital alone,
/// `Y`, `M`, `Q`, `W`, is a period to date, which [`Range::parse`] reads
/// before this.
fn count_and_unit(text: &str) -> Option<(u32, Span)> {
    let lower = text.trim().to_ascii_lowercase();
    let (count, unit) = match lower.strip_prefix("an ").or_else(|| lower.strip_prefix("a ")) {
        Some(unit) => (1, unit.trim()),
        None => {
            let split = lower.find(|c: char| !c.is_ascii_digit()).unwrap_or(lower.len());
            let (digits, unit) = lower.split_at(split);
            let count = match digits {
                "" => 1,
                digits => digits.parse().ok().filter(|&n| n > 0 && n <= 100)?,
            };
            (count, unit.trim())
        }
    };
    let (times, span) = match unit {
        "d" | "dy" | "day" | "days" => (1, Span::Day),
        "w" | "wk" | "wks" | "week" | "weeks" => (1, Span::Week),
        "m" | "mo" | "mos" | "mth" | "mths" | "month" | "months" => (1, Span::Month),
        "q" | "qtr" | "qtrs" | "quarter" | "quarters" => (3, Span::Month),
        "y" | "yr" | "yrs" | "year" | "years" => (1, Span::Year),
        _ => return None,
    };
    Some((count * times, span))
}

/// `Monday`, for saying a weekday.
fn day_name(weekday: Weekday) -> String {
    NaiveDate::from_isoywd_opt(2026, 1, weekday).map_or(String::new(), |d| d.format("%A").to_string())
}

/// A month and a day that some year has: 29 February, not 30.
fn month_day(month: u32, day: u32) -> Option<End> {
    NaiveDate::from_ymd_opt(2000, month, day).map(|_| End::MonthDay(month, day))
}

/// `mon`, `mond`, `tues`, `thursday`: the start of the name, three letters
/// at least, so `m` stays a month; `weds` too.
fn weekday(word: &str) -> Option<Weekday> {
    if word == "weds" {
        return Some(Weekday::Wed);
    }
    if word.len() < 3 {
        return None;
    }
    [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun]
        .into_iter()
        .find(|day| NaiveDate::from_isoywd_opt(2026, 1, *day).is_some_and(|d| d.format("%A").to_string().to_lowercase().starts_with(word)))
}

/// A month by name, `june`, `jan`, `Sept`, alone for the latest one or
/// with a year, `june 2025`, `2025 june`, for that one. A name and a day,
/// `oct 22`, is a day, which [`one_day`] reads first.
fn named_month(text: &str) -> Option<Range> {
    let lower = text.trim().to_ascii_lowercase();
    let words: Vec<&str> = lower.split([' ', ',']).filter(|word| !word.is_empty()).collect();
    match words[..] {
        [name] => Some(Range::LatestMonth(month_named(name)?)),
        [name, year] | [year, name] if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) => {
            let year: i32 = year.parse().ok().filter(|y| (1800..=2200).contains(y))?;
            Some(Range::Month(year, month_named(name)?))
        }
        _ => None,
    }
}

/// The latest of `month` as of `today`, this month included: its year and
/// the month.
fn latest_month(month: u32, today: NaiveDate) -> (i32, u32) {
    match month <= today.month() {
        true => (today.year(), month),
        false => (today.year() - 1, month),
    }
}

/// `January`, for saying a month.
fn month_name(month: u32) -> String {
    NaiveDate::from_ymd_opt(2000, month, 1).map_or(String::new(), |d| d.format("%B").to_string())
}

/// `mar`, `sept`, `november`: three letters of the name at least.
fn month_named(word: &str) -> Option<u32> {
    if word.len() < 3 {
        return None;
    }
    (1..=12u32).find(|&month| {
        NaiveDate::from_ymd_opt(2000, month, 1).is_some_and(|d| d.format("%B").to_string().to_lowercase().starts_with(word))
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Span {
    Day,
    Week,
    Month,
    Year,
}

impl Span {
    /// `d`, `w`, `m`, `y`: what a count is written with.
    fn letter(self) -> char {
        match self {
            Span::Day => 'd',
            Span::Week => 'w',
            Span::Month => 'm',
            Span::Year => 'y',
        }
    }

    /// `a week`, `3 weeks`.
    fn counted(self, count: u32) -> String {
        let unit = match self {
            Span::Day => "day",
            Span::Week => "week",
            Span::Month => "month",
            Span::Year => "year",
        };
        match count {
            1 => format!("a {unit}"),
            _ => format!("{count} {unit}s"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Period {
    /// From Monday.
    Week,
    Month,
    Quarter,
    Year,
}

impl Range {
    /// "6m", "1y", "YTD", "2YTD", "All": what is shown, and what reads
    /// back as the same range when typed.
    pub fn key(self) -> String {
        match self {
            Range::Back(count, span) => {
                let unit = match span {
                    Span::Day => 'd',
                    Span::Week => 'w',
                    Span::Month => 'm',
                    Span::Year => 'y',
                };
                format!("{count}{unit}")
            }
            Range::ToDate(count, period) => {
                let word = match period {
                    Period::Week => "WTD",
                    Period::Month => "MTD",
                    Period::Quarter => "QTD",
                    Period::Year => "YTD",
                };
                match count {
                    0 | 1 => word.to_string(),
                    _ => format!("{count}{word}"),
                }
            }
            Range::All => "All".into(),
            Range::Year(year) => year.to_string(),
            Range::Month(year, month) => format!("{year}-{month:02}"),
            Range::LatestMonth(month) => month_name(month).to_lowercase(),
            Range::Day(day) | Range::Lasting(day, ..) => day.key(),
            Range::Slice(from, to) => {
                format!("{}:{}", from.map(End::key).unwrap_or_default(), to.map(End::key).unwrap_or_default())
            }
        }
    }

    /// [`Range::key`] with a day of `year` written without it, `10-12`
    /// for `2026-10-12`: short enough for the corner of a chart, which a
    /// day pinned to its date by [`Range::pinned`] would otherwise outgrow.
    pub fn short_key(self, year: i32) -> String {
        let end = |end: End| match end {
            End::Day(day) if day.year() == year => day.format("%m-%d").to_string(),
            // A month of this year by its name, `jan`, which reads back.
            End::Month(this, month) if this == year => month_name(month).to_lowercase()[..3].to_string(),
            end => end.key(),
        };
        match self {
            Range::Month(this, month) => end(End::Month(this, month)),
            Range::Day(day) | Range::Lasting(day, ..) => end(day),
            Range::Slice(from, to) => format!("{}:{}", from.map(end).unwrap_or_default(), to.map(end).unwrap_or_default()),
            range => range.key(),
        }
    }

    /// Spelled out, for saying what was typed.
    pub fn description(self) -> String {
        match self {
            Range::Back(count, span) => {
                let unit = match span {
                    Span::Day => "day",
                    Span::Week => "week",
                    Span::Month => "month",
                    Span::Year => "year",
                };
                match count {
                    1 => format!("last {unit}"),
                    _ => format!("last {count} {unit}s"),
                }
            }
            Range::ToDate(count, period) => {
                let unit = match period {
                    Period::Week => "week",
                    Period::Month => "month",
                    Period::Quarter => "quarter",
                    Period::Year => "year",
                };
                match count {
                    0 | 1 => format!("{unit} to date"),
                    2 => format!("this {unit} and last"),
                    _ => format!("this {unit} and {} before", count - 1),
                }
            }
            Range::All => "whole history".into(),
            Range::Year(year) => format!("all of {year}"),
            Range::Month(year, month) => match NaiveDate::from_ymd_opt(year, month, 1) {
                Some(first) => format!("all of {}", first.format("%B %Y")),
                None => format!("{year}-{month:02}"),
            },
            Range::LatestMonth(month) => format!("all of the latest {}", month_name(month)),
            Range::Day(day) => day.words(),
            Range::Lasting(_, seconds, early) if early > 0 => match seconds / 86_400 {
                0 => "up to now".into(),
                1 => "the day up to now".into(),
                days => format!("the {days} days up to now"),
            },
            Range::Lasting(day, seconds, _) => match seconds / 86_400 {
                0 => day.words(),
                1 => format!("{} and the day after", day.words()),
                days => format!("{} and {days} days on", day.words()),
            },
            Range::Slice(from, to) => match (from, to) {
                (Some(from), None) => format!("from {} to today", from.words()),
                (None, Some(to)) => format!("up to {}", to.words()),
                (Some(from), Some(to)) => format!("{} to {}", from.words(), to.words()),
                (None, None) => "whole history".into(),
            },
        }
    }

    /// Read a range: a count and a unit, vim's way, and no count is one.
    ///
    /// The unit's case says which sort. In small letters it rolls back from
    /// today — `6m` six months, `3y` three years, `2w`, `10d`, and `q` three
    /// months. In capitals it counts whole periods to date — `Y` the year to
    /// date, `2Y` that and the year before, so from 1 January last year —
    /// and `W`, `M`, `Q` the same. `ytd`, `mtd`, `qtd` and `wtd` are the
    /// to-date ones in any case, `2ytd` too, and `all` is everything. "m" is
    /// a month here, never a minute: nobody asks a chart for the last three
    /// minutes this way.
    ///
    /// Four digits are a year, `2025`, and a year and a month one month,
    /// `2026-09` — the two that end before today. A day is that day alone:
    /// `2026-03-12`, `10/12` in the latest year it has passed in, `12 mar`,
    /// `mon` for this week's Monday and `2mon` for the one before, `today`,
    /// `yesterday`.
    ///
    /// A colon slices, as pandas slices dates: `2025:`, `2024:2025-06`,
    /// `:2020`, `-2y:-1y`, and `:` alone is everything. Two dots do the same,
    /// the way Ruby writes a range: `2025..`, `2024..2025-06`. Both ends are
    /// in either way. See [`Range::Slice`].
    pub fn parse(text: &str) -> Option<Range> {
        let text = &dots_to_colon(text);
        let text = text.trim();
        let lower = text.to_ascii_lowercase();
        if matches!(lower.as_str(), "all" | "max") {
            return Some(Range::All);
        }
        if let Some((from, to)) = text.split_once(':') {
            let end = |text: &str| match text.trim() {
                "" => Some(None),
                text => End::parse(text).map(Some),
            };
            return match (end(from)?, end(to)?) {
                (None, None) => Some(Range::All),
                (from, to) => Some(Range::Slice(from, to)),
            };
        }
        // A dash in front says "back", which a count or a weekday says
        // already, so it changes nothing: `-2w` is `2w`, `-2mon` is `2mon`.
        if let Some(rest) = text.strip_prefix('-') {
            return Range::parse(rest).filter(|range| !rest.starts_with('-') && dash_takes(*range));
        }
        if let Some(calendar) = calendar(text) {
            return Some(calendar);
        }
        if let Some(day) = one_day(text) {
            return Some(Range::Day(day));
        }
        if let Some(month) = named_month(text) {
            return Some(month);
        }
        let split = text.find(|c: char| !c.is_ascii_digit())?;
        let (digits, unit) = text.split_at(split);
        let unit = unit.trim();
        let count: u32 = match digits {
            "" => 1,
            digits => digits.parse().ok().filter(|&n| n > 0 && n <= 100)?,
        };
        let to_date = |period| Some(Range::ToDate(count, period));
        match unit.to_ascii_lowercase().as_str() {
            "wtd" => return to_date(Period::Week),
            "mtd" => return to_date(Period::Month),
            "qtd" => return to_date(Period::Quarter),
            "ytd" => return to_date(Period::Year),
            _ => {}
        }
        match unit {
            "W" => return to_date(Period::Week),
            "M" => return to_date(Period::Month),
            "Q" => return to_date(Period::Quarter),
            "Y" => return to_date(Period::Year),
            _ => {}
        }
        let (count, span) = count_and_unit(text)?;
        Some(Range::Back(count, span))
    }

    /// The first day the range holds, when today is `today`; `None` for the
    /// whole series.
    pub fn start(self, today: NaiveDate) -> Option<NaiveDate> {
        let back = |months: u32| today.checked_sub_months(Months::new(months)).and_then(|d| d.succ_opt());
        match self {
            Range::All => None,
            Range::Slice(from, _) => from?.first(today),
            Range::Year(year) => NaiveDate::from_ymd_opt(year, 1, 1),
            Range::Month(year, month) => NaiveDate::from_ymd_opt(year, month, 1),
            Range::LatestMonth(month) => End::LatestMonth(month).first(today),
            Range::Day(day) => day.first(today),
            Range::Lasting(..) => self.lasting_span(today).map(|(from, _)| from.date()),
            Range::Back(count, Span::Day) => today.checked_sub_days(Days::new(u64::from(count) - 1)),
            Range::Back(count, Span::Week) => today.checked_sub_days(Days::new(7 * u64::from(count) - 1)),
            Range::Back(count, Span::Month) => back(count),
            Range::Back(count, Span::Year) => back(12 * count),
            Range::ToDate(count, period) => {
                let before = count.max(1) - 1;
                match period {
                    Period::Week => today.checked_sub_days(Days::new(
                        u64::from(today.weekday().num_days_from_monday()) + 7 * u64::from(before),
                    )),
                    Period::Month => today.with_day(1)?.checked_sub_months(Months::new(before)),
                    Period::Quarter => NaiveDate::from_ymd_opt(today.year(), (today.month0() / 3) * 3 + 1, 1)?
                        .checked_sub_months(Months::new(3 * before)),
                    Period::Year => NaiveDate::from_ymd_opt(today.year() - before as i32, 1, 1),
                }
            }
        }
    }

    /// The last day the range holds, for the ranges that stop before the
    /// live edge; `None` for those that run to it.
    pub fn end(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Range::Slice(_, to) => to?.last(today),
            Range::Year(year) => NaiveDate::from_ymd_opt(year, 12, 31),
            Range::Month(year, month) => NaiveDate::from_ymd_opt(year, month, 1)?
                .checked_add_months(Months::new(1))?
                .pred_opt(),
            Range::LatestMonth(month) => End::LatestMonth(month).last(today),
            Range::Day(day) => day.last(today),
            Range::Lasting(..) => {
                let (from, seconds) = self.lasting_span(today)?;
                Some((from + Duration::seconds(seconds)).date())
            }
            _ => None,
        }
    }

    /// A day on its own as a chart shows it: from the start of that day for
    /// `seconds`, the length of time the chart shows now, so it ends that
    /// long after, unless that is past `now`. Then it ends at `now` and
    /// starts as much earlier, the same length: `today` on a chart six weeks
    /// wide is the six weeks up to now, not six weeks of room for bars still
    /// to come. Anything else is as it was: a range says how long it is.
    ///
    /// `now` is on the chart's clock, the one its window is measured on, and
    /// `today` the calendar's, the one the day is pinned to afterwards. They
    /// differ in the evening on a chart that keeps New York time, and a day
    /// named from the wrong one is a day out from the date it is pinned to.
    pub fn lasting(self, seconds: i64, today: NaiveDate, now: NaiveDateTime) -> Range {
        match self {
            Range::Day(day) => {
                let seconds = seconds.max(0);
                let from = day.first(today).map(|first| first.and_time(NaiveTime::MIN));
                // How far past now it would run from its day.
                let past = from.map_or(0, |from| (from + Duration::seconds(seconds) - now).num_seconds());
                Range::Lasting(day, seconds, past.max(0))
            }
            range => range,
        }
    }

    /// The moment a [`Range::Lasting`] starts and how long it lasts.
    fn lasting_span(self, today: NaiveDate) -> Option<(NaiveDateTime, i64)> {
        let Range::Lasting(day, seconds, early) = self else { return None };
        Some((day.first(today)?.and_time(NaiveTime::MIN) - Duration::seconds(early), seconds))
    }

    /// Where a chart lays a [`Range::Lasting`]: the moment it starts and how
    /// long it lasts. One moved earlier to end at now ends at `edge`
    /// instead when that is later: the chart's live edge, its newest bar
    /// and the room it keeps past it, so the window sits where a chart left
    /// at its live edge does and follows bars as they come in. Never later
    /// than the day, though: a short window on a chart whose room is long,
    /// a year on a weekly chart with ten weeks past its last bar, would
    /// otherwise start after the day it was asked for.
    pub fn lasting_on(self, today: NaiveDate, edge: NaiveDateTime) -> Option<(NaiveDateTime, i64)> {
        let (from, seconds) = self.lasting_span(today)?;
        match self {
            Range::Lasting(_, _, early) if early > 0 => {
                let day = from + Duration::seconds(early);
                Some((from.max(edge - Duration::seconds(seconds)).min(day), seconds))
            }
            _ => Some((from, seconds)),
        }
    }

    /// The range with every end that names a day on the calendar, `oct 31`,
    /// `3 weeks ago`, `fri`, `yesterday`, made that date as of `today`.
    ///
    /// A chart reckons today from its last bar, which is right for a span:
    /// the week to date on a Sunday is the week that just traded. A day is
    /// different. On a Sunday, `3 weeks ago` is three weeks before that
    /// Sunday, as the card said it would be, and `oct 31` is yesterday's
    /// Saturday rather than a year back because the last bar was Friday.
    pub fn pinned(self, today: NaiveDate) -> Range {
        let pin = |end: End| match end {
            End::Weekday(..) | End::MonthDay(..) | End::Today | End::Yesterday | End::Before(..) | End::Last(..) => {
                end.first(today).map_or(end, End::Day)
            }
            End::LatestMonth(month) => {
                let (year, month) = latest_month(month, today);
                End::Month(year, month)
            }
            end => end,
        };
        match self {
            Range::LatestMonth(month) => {
                let (year, month) = latest_month(month, today);
                Range::Month(year, month)
            }
            Range::Day(day) => Range::Day(pin(day)),
            Range::Lasting(day, seconds, early) => Range::Lasting(pin(day), seconds, early),
            Range::Slice(from, to) => Range::Slice(from.map(pin), to.map(pin)),
            range => range,
        }
    }

    /// What the range lands on, in days, for saying before it is shown:
    /// `22 Oct – 1 Nov`, `since 11 Oct`, `12 Mar`. The year only when it is
    /// not this one. A day on a chart names its whole window; a slice open
    /// at the end, or reaching today, runs to today and says since. One
    /// that needs no days to say is its description.
    pub fn spelled(self, today: NaiveDate) -> String {
        let day = |day: NaiveDate| match day.year() == today.year() {
            true => day.format("%-d %b").to_string(),
            false => day.format("%-d %b %Y").to_string(),
        };
        let (Some(first), last) = (self.start(today), self.end(today)) else { return self.description() };
        match self {
            Range::Day(_) => day(first),
            Range::Month(..) | Range::LatestMonth(_) => first.format("%B %Y").to_string(),
            Range::Lasting(..) => match last {
                Some(last) if last > first => format!("{} – {}", day(first), day(last)),
                _ => day(first),
            },
            Range::Slice(Some(_), _) => match last {
                Some(last) if last <= first => day(first),
                Some(last) if last < today => format!("{} – {}", day(first), day(last)),
                _ => format!("since {}", day(first)),
            },
            _ => self.description(),
        }
    }

    /// Whether the range holds a single day up to today. `2030` and
    /// `2025:2024` read as ranges but hold none, and showing one leaves the
    /// chart on no bars at a resolution picked for nothing.
    pub fn holds_days(self, today: NaiveDate) -> bool {
        let last = self.end(today).map_or(today, |end| end.min(today));
        self.start(today).is_none_or(|start| start <= last)
    }

    /// Calendar days the range covers up to today, today included; `None`
    /// for all of it.
    pub fn days(self, today: NaiveDate) -> Option<i64> {
        let last = self.end(today).map_or(today, |end| end.min(today));
        self.start(today).map(|start| (last - start).num_days().max(0) + 1)
    }

    /// Calendar days from the range's first day to today: how far back a
    /// feed has to reach to show it.
    pub fn reach(self, today: NaiveDate) -> Option<i64> {
        self.start(today).map(|start| (today - start).num_days().max(0) + 1)
    }

    /// What `text` could mean, for a picker to list, the likeliest first.
    ///
    /// Nothing typed lists the common ones, with last month and this year
    /// by name high enough to be seen without scrolling. A range typed is that range, then its twin in the other
    /// case — `2Y` and `2y` are easy to mix up — then the same count in the
    /// other units. A bare count offers each unit; the start of a year
    /// offers that year and its months, latest first, from two digits. Nothing that reads
    /// as a range is nothing.
    pub fn suggest(text: &str, today: NaiveDate) -> Vec<Range> {
        let text = text.trim();
        let this_year = today.year();
        let last_month = today.with_day(1).and_then(|d| d.checked_sub_months(Months::new(1)));
        if text.is_empty() {
            let mut out = vec![
                Range::ToDate(1, Period::Year),
                Range::Back(1, Span::Year),
                Range::Back(6, Span::Month),
                Range::Back(3, Span::Month),
                Range::ToDate(1, Period::Month),
                Range::Back(1, Span::Month),
            ];
            if let Some(month) = last_month {
                out.push(Range::Month(month.year(), month.month()));
            }
            out.extend([
                Range::Year(this_year),
                Range::Slice(Some(End::Year(this_year - 1)), None),
                Range::ToDate(1, Period::Quarter),
                Range::ToDate(2, Period::Year),
                Range::Back(5, Span::Year),
                Range::All,
            ]);
            return out;
        }
        let mut out: Vec<Range> = Vec::new();
        let mut add = |range: Range| {
            if !out.contains(&range) {
                out.push(range);
            }
        };
        // A slice is what it says; `-6m` on its own is the start of one.
        if text.contains(':') || text.contains("..") {
            return Range::parse(text).into_iter().collect();
        }
        if let Some(rest) = text.strip_prefix('-') {
            return match rest.is_empty() || rest.starts_with('-') {
                true => Vec::new(),
                false => Range::suggest(rest, today).into_iter().filter(|range| dash_takes(*range)).collect(),
            };
        }
        // A day: it alone, then from it to today. A month by name the same.
        match Range::parse(text) {
            Some(Range::Day(day)) => return vec![Range::Day(day), Range::Slice(Some(day), None)],
            Some(Range::LatestMonth(month)) => {
                return vec![Range::LatestMonth(month), Range::Slice(Some(End::LatestMonth(month)), None)];
            }
            Some(Range::Month(year, month)) if text.contains(|c: char| c.is_ascii_alphabetic()) => {
                return vec![Range::Month(year, month), Range::Slice(Some(End::Month(year, month)), None)];
            }
            _ => {}
        }
        let digits = text.chars().all(|c| c.is_ascii_digit());
        // The start of a year, or a year and the start of a month: that
        // year's months up to today, latest first.
        if let Some((year, month)) = text.split_once(['-', '/'])
            && year.len() == 4
            && let Ok(year) = year.parse::<i32>()
            && month.chars().all(|c| c.is_ascii_digit())
        {
            let last = if year == this_year { today.month() } else { 12 };
            for m in (1..=last).rev() {
                let key = format!("{m:02}");
                if month.is_empty() || key.starts_with(month) || m.to_string() == month {
                    add(Range::Month(year, m));
                }
            }
            // One month left: it, then from it to today.
            let only = match out[..] {
                [Range::Month(year, month)] => Some(Range::Slice(Some(End::Month(year, month)), None)),
                _ => None,
            };
            out.extend(only);
            return out;
        }
        let parsed = Range::parse(text);
        if parsed.is_none() && !digits {
            return out;
        }
        if let Some(range) = parsed {
            add(range);
            if let Range::Year(year) = range {
                add(Range::Slice(Some(End::Year(year)), None));
            }
        }
        if digits && text.len() >= 2 && this_year.to_string().starts_with(text) {
            add(Range::Year(this_year));
            add(Range::Slice(Some(End::Year(this_year)), None));
            if let Some(month) = last_month {
                add(Range::Month(month.year(), month.month()));
            }
        }
        // The same letters in the other case.
        let lead: String = text.chars().take_while(char::is_ascii_digit).collect();
        let unit = &text[lead.len()..];
        let swapped: String = unit
            .chars()
            .map(|c| match c.is_ascii_uppercase() {
                true => c.to_ascii_lowercase(),
                false => c.to_ascii_uppercase(),
            })
            .collect();
        if let Some(twin) = Range::parse(&format!("{lead}{swapped}")) {
            add(twin);
        }
        let Some(n) = (match lead.as_str() {
            "" => Some(1),
            lead => lead.parse::<u32>().ok().filter(|&n| n > 0 && n <= 100),
        }) else {
            return out;
        };
        let rolling = [
            Range::Back(n, Span::Month),
            Range::Back(n, Span::Year),
            Range::Back(n, Span::Week),
            Range::Back(n, Span::Day),
        ];
        let whole = [
            Range::ToDate(n, Period::Year),
            Range::ToDate(n, Period::Quarter),
            Range::ToDate(n, Period::Month),
            Range::ToDate(n, Period::Week),
        ];
        let (first, then) = match parsed {
            Some(Range::ToDate(..)) => (whole, rolling),
            _ => (rolling, whole),
        };
        for range in first.into_iter().chain(then) {
            add(range);
        }
        out.truncate(8);
        out
    }
}

/// What a dash in front leaves as it is: the ranges that count back from
/// today. Not a year or a date, where `-2025` would only be a typo.
fn dash_takes(range: Range) -> bool {
    matches!(range, Range::Back(..) | Range::ToDate(..) | Range::Day(End::Weekday(..)))
}

/// `..` read as `:`, once: `2025..2026` is `2025:2026`.
fn dots_to_colon(text: &str) -> std::borrow::Cow<'_, str> {
    match text.contains("..") {
        true => text.replacen("..", ":", 1).into(),
        false => text.into(),
    }
}

/// `2025` or `2026-09`: a year, or a month of one.
fn calendar(text: &str) -> Option<Range> {
    let (year, month) = match text.split_once(['-', '/']) {
        Some((year, month)) => (year, Some(month)),
        None => (text, None),
    };
    if year.len() != 4 || !year.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year: i32 = year.parse().ok().filter(|y| (1800..=2200).contains(y))?;
    match month {
        None => Some(Range::Year(year)),
        Some(month) if (1..=2).contains(&month.len()) && month.chars().all(|c| c.is_ascii_digit()) => {
            let month: u32 = month.parse().ok().filter(|m| (1..=12).contains(m))?;
            Some(Range::Month(year, month))
        }
        Some(_) => None,
    }
}

/// What a chart knows about the resolutions it could move to, for
/// [`resolution_for`].
pub struct Fit<'a> {
    /// The resolutions offered on the strip, in any order: a range moves
    /// between those, and off them to one of [`COMMON`] only when none of
    /// them fits.
    pub ladder: &'a [Timeframe],
    /// The fewest and the most bars a chart will spread across its width.
    pub bars: (usize, usize),
    /// The share of a calendar week that brings bars, intraday and daily;
    /// see [`crate::session::week_share`].
    pub share: (f64, f64),
    /// Whether the feed has the symbol at this resolution at all.
    pub offers: &'a dyn Fn(Timeframe) -> bool,
    /// How many days back the feed goes at this resolution, if it stops.
    pub reach: &'a dyn Fn(Timeframe) -> Option<u32>,
}

/// Roughly how many bars of `timeframe` the range holds; `None` for all.
///
/// A market that shuts at weekends trades on its weekdays, counted as they
/// fall rather than as five sevenths of every day: a single Tuesday of a
/// stock is a whole trading day of bars, not five sevenths of one.
pub fn bars_in(range: Range, today: NaiveDate, timeframe: Timeframe, share: (f64, f64)) -> Option<f64> {
    let days = range.days(today)? as f64;
    let first = range.start(today)?;
    let last = range.end(today).map_or(today, |end| end.min(today));
    let open = match share.1 < 0.99 {
        true => weekdays(first, last) as f64,
        false => days,
    };
    // How much of a day it trades, on a day it trades.
    let session = share.0 / share.1.max(f64::EPSILON);
    let bars = if timeframe.is_intraday() {
        open * 86_400.0 * session / timeframe.seconds() as f64
    } else if timeframe.seconds() < Timeframe::weeks(1).seconds() {
        open / (timeframe.seconds() as f64 / 86_400.0)
    } else {
        // A week, a month, a year: one bar each, trading days or not.
        days * 86_400.0 / timeframe.seconds() as f64
    };
    Some(bars.max(1.0))
}

/// Monday to Friday from `first` to `last`, both counted.
fn weekdays(first: NaiveDate, last: NaiveDate) -> i64 {
    let span = (last - first).num_days() + 1;
    if span <= 0 {
        return 0;
    }
    let start = i64::from(first.weekday().num_days_from_monday());
    let rest = (0..span % 7).filter(|d| (start + d) % 7 < 5).count() as i64;
    span / 7 * 5 + rest
}

/// Where a [`Range::Lasting`] lands on `bars`, each read on the chart's
/// clock by `at`: the first bar at or after `from`, the columns from there to
/// the moment `seconds` after it, and how many of those columns are empty
/// room past the last bar, one to each `step` seconds as the time axis counts
/// them there. The start stays where it is however near the last bar it is,
/// and the columns are whatever that stretch of time takes, so the bars widen
/// or narrow to show exactly it.
pub fn lasting_window<T>(
    bars: &[T],
    at: impl Fn(&T) -> Option<NaiveDateTime>,
    from: NaiveDateTime,
    seconds: i64,
    step: i64,
) -> (usize, usize, usize) {
    let until = from + Duration::seconds(seconds.max(0));
    let first = bars.partition_point(|bar| at(bar).is_some_and(|t| t < from));
    let end = bars.partition_point(|bar| at(bar).is_some_and(|t| t <= until));
    if end < bars.len() {
        return (first, end.saturating_sub(first).max(1), 0);
    }
    let room = match bars.last().and_then(&at) {
        Some(last) if step > 0 => ((until - last).num_seconds().max(0) / step) as usize,
        _ => 0,
    };
    (first, bars.len().saturating_sub(first) + room, room)
}

/// The fewest and the most bars a range aims to land on when the resolution
/// is picked for it: enough candles to read a shape, few enough that each is
/// still a candle rather than a column.
pub const AIM_BARS: (usize, usize) = (50, 400);

/// The resolution to show `range` at: `current` when it fits, or else the
/// best fit on the ladder, or else the best of [`COMMON`].
///
/// It fits when the feed reaches back far enough at that resolution and the
/// range is between the fewest and the most bars asked for. Too many, or out
/// of reach, moves coarser — a year of five-minute bars is a year of hourly
/// ones. Too few moves finer — a month of weekly bars is four candles, a
/// month of daily ones is a chart. Nothing fitting takes whichever the feed
/// reaches with the bars closest to fitting, and nothing reaching keeps
/// `current`, which then shows what it can.
pub fn resolution_for(range: Range, today: NaiveDate, current: Timeframe, fit: &Fit) -> Timeframe {
    let Some(days) = range.days(today) else {
        return current;
    };
    let (fewest, most) = (fit.bars.0 as f64, fit.bars.1 as f64);
    let count = |tf: Timeframe| bars_in(range, today, tf, fit.share).unwrap_or(0.0).max(1.0);
    let back = range.reach(today).unwrap_or(days);
    let reaches = |tf: Timeframe| (fit.reach)(tf).is_none_or(|limit| i64::from(limit) >= back);
    let usable = |tf: Timeframe| (fit.offers)(tf) && reaches(tf);
    // How far outside the band, as a ratio, so twice too many and half too
    // few are as far off: none inside it.
    let miss = |tf: Timeframe| {
        let n = count(tf);
        match () {
            _ if n < fewest => (fewest / n).ln(),
            _ if n > most => (n / most).ln(),
            _ => 0.0,
        }
    };
    // The chart's own, while it fits: a range should not change a
    // resolution that already shows it well.
    if usable(current) && miss(current) == 0.0 {
        return current;
    }
    // Otherwise the best fit, wherever it is: closest to fitting, then the
    // bars nearest the middle of the band, then fewest steps along the
    // ladder from `current`. Nearest-first took a year to date from 5m to
    // 4h, four hundred cramped candles, when 1D is two hundred readable
    // ones.
    let middle = (fewest * most).sqrt();
    let best = |offered: &[Timeframe]| {
        let mut ladder: Vec<Timeframe> = offered.to_vec();
        ladder.push(current);
        ladder.sort_by_key(|tf| tf.seconds());
        ladder.dedup();
        let at = |tf: Timeframe| ladder.iter().position(|t| *t == tf).unwrap_or(0);
        let here = at(current);
        let rank = |tf: Timeframe| (miss(tf), (count(tf) / middle).ln().abs(), at(tf).abs_diff(here));
        ladder.iter().copied().filter(|&tf| usable(tf)).min_by(|&a, &b| {
            let (a, b) = (rank(a), rank(b));
            a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2))
        })
    };
    // The strip first. Only when nothing on it fits does a common
    // resolution off it come in: a day on a strip that starts at an hour
    // is a handful of candles, and five minutes is a chart.
    match best(fit.ladder) {
        Some(tf) if miss(tf) == 0.0 => tf,
        on_strip => {
            let wider: Vec<Timeframe> = fit.ladder.iter().copied().chain(COMMON).collect();
            match best(&wider) {
                Some(tf) if miss(tf) == 0.0 => tf,
                _ => on_strip.unwrap_or(current),
            }
        }
    }
}

/// The resolutions a range may move to when none on the strip fits it, and
/// that the Go to card offers beside the strip's.
pub const COMMON: [Timeframe; 10] = [
    Timeframe::minutes(1),
    Timeframe::minutes(5),
    Timeframe::minutes(10),
    Timeframe::minutes(15),
    Timeframe::minutes(30),
    Timeframe::hours(1),
    Timeframe::hours(4),
    Timeframe::days(1),
    Timeframe::weeks(1),
    Timeframe::months(1),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn every_preset_reads_back_as_itself() {
        for range in Range::suggest("", day(2026, 10, 20)) {
            assert_eq!(Range::parse(&range.key()), Some(range), "{}", range.key());
        }
        assert_eq!(Range::parse("ytd"), Some(Range::ToDate(1, Period::Year)));
        assert_eq!(Range::parse("3m"), Some(Range::Back(3, Span::Month)), "a month, not a minute");
        assert_eq!(Range::parse("5d"), Some(Range::Back(5, Span::Day)));
        assert_eq!(Range::parse("0y"), None);
        assert_eq!(Range::parse("1h"), None);
        assert_eq!(Range::parse("2025"), Some(Range::Year(2025)));
        assert_eq!(Range::parse("2026-09"), Some(Range::Month(2026, 9)));
        assert_eq!(Range::parse("2026-9"), Some(Range::Month(2026, 9)));
        assert_eq!(Range::parse("2026-13"), None);
        assert_eq!(Range::parse("202"), None, "three digits are a count with no unit");
        for range in [Range::Year(2025), Range::Month(2026, 9)] {
            assert_eq!(Range::parse(&range.key()), Some(range), "{}", range.key());
        }
    }

    /// Small letters roll back, capitals count whole periods to date, and
    /// no count is one.
    #[test]
    fn the_case_of_the_unit_says_which_sort() {
        assert_eq!(Range::parse("y"), Some(Range::Back(1, Span::Year)));
        assert_eq!(Range::parse("Y"), Some(Range::ToDate(1, Period::Year)));
        assert_eq!(Range::parse("2Y"), Some(Range::ToDate(2, Period::Year)));
        assert_eq!(Range::parse("2ytd"), Some(Range::ToDate(2, Period::Year)));
        assert_eq!(Range::parse("3M"), Some(Range::ToDate(3, Period::Month)));
        assert_eq!(Range::parse("2q"), Some(Range::Back(6, Span::Month)));
        assert_eq!(Range::parse("Q"), Some(Range::ToDate(1, Period::Quarter)));
        assert_eq!(Range::parse("w"), Some(Range::Back(1, Span::Week)));
        assert_eq!(Range::parse("ALL"), Some(Range::All));
    }

    #[test]
    fn to_date_starts_on_the_first() {
        let today = day(2026, 10, 19);
        assert_eq!(Range::ToDate(1, Period::Year).start(today), Some(day(2026, 1, 1)));
        assert_eq!(Range::ToDate(1, Period::Quarter).start(today), Some(day(2026, 10, 1)));
        assert_eq!(Range::ToDate(1, Period::Quarter).start(day(2026, 6, 30)), Some(day(2026, 4, 1)));
        assert_eq!(Range::ToDate(1, Period::Month).start(today), Some(day(2026, 10, 1)));
        // 19 October 2026 is a Monday, so the week to date is that day.
        assert_eq!(Range::ToDate(1, Period::Week).start(today), Some(today));
        assert_eq!(Range::ToDate(1, Period::Week).start(day(2026, 10, 25)), Some(today), "Sunday");
        // A count reaches back whole periods.
        assert_eq!(Range::ToDate(2, Period::Year).start(today), Some(day(2025, 1, 1)));
        assert_eq!(Range::ToDate(3, Period::Month).start(today), Some(day(2026, 8, 1)));
        assert_eq!(Range::ToDate(2, Period::Quarter).start(today), Some(day(2026, 7, 1)));
        assert_eq!(Range::ToDate(5, Period::Quarter).start(today), Some(day(2025, 10, 1)));
        assert_eq!(Range::ToDate(2, Period::Week).start(today), Some(day(2026, 10, 12)));
        // On the first, the month to date is that one day.
        assert_eq!(Range::ToDate(1, Period::Month).start(day(2026, 11, 1)), Some(day(2026, 11, 1)));
    }

    #[test]
    fn rolling_ranges_hold_exactly_their_span() {
        let today = day(2026, 10, 19);
        assert_eq!(Range::Back(1, Span::Year).start(today), Some(day(2025, 10, 20)));
        assert_eq!(Range::Back(1, Span::Month).start(today), Some(day(2026, 9, 20)));
        assert_eq!(Range::Back(1, Span::Week).start(today), Some(day(2026, 10, 13)));
        assert_eq!(Range::Back(5, Span::Day).start(today), Some(day(2026, 10, 15)));
        assert_eq!(Range::Back(1, Span::Year).days(today), Some(365));
        assert_eq!(Range::All.start(today), None);
    }

    /// A month back from the 31st lands on the last day the shorter month
    /// has, and a year back from a leap day on the 28th.
    #[test]
    fn month_ends_and_leap_days_do_not_overflow() {
        assert_eq!(Range::Back(1, Span::Month).start(day(2026, 3, 31)), Some(day(2026, 3, 1)));
        assert_eq!(Range::Back(1, Span::Year).start(day(2028, 2, 29)), Some(day(2027, 3, 1)));
    }

    fn fit<'a>(ladder: &'a [Timeframe], share: (f64, f64), reach: &'a dyn Fn(Timeframe) -> Option<u32>) -> Fit<'a> {
        Fit { ladder, bars: (12, 3000), share, offers: &|_| true, reach }
    }

    const STRIP: [Timeframe; 6] = Timeframe::PRESETS;
    /// A US stock on regular hours.
    const STOCK: (f64, f64) = (6.5 / 24.0 * 5.0 / 7.0, 5.0 / 7.0);
    const CRYPTO: (f64, f64) = (1.0, 1.0);

    fn yahoo(tf: Timeframe) -> Option<u32> {
        match tf.native() {
            tf if tf.is_intraday() && tf.seconds() < 3_600 => Some(60),
            tf if tf == Timeframe::hours(1) => Some(730),
            _ => None,
        }
    }

    #[test]
    fn a_range_that_fits_keeps_the_resolution() {
        let today = day(2026, 10, 19);
        let fit = fit(&STRIP, STOCK, &yahoo);
        let ytd = Range::ToDate(1, Period::Year);
        assert_eq!(resolution_for(ytd, today, Timeframe::days(1), &fit), Timeframe::days(1));
        assert_eq!(resolution_for(ytd, today, Timeframe::weeks(1), &fit), Timeframe::weeks(1));
        let month = Range::Back(1, Span::Month);
        assert_eq!(resolution_for(month, today, Timeframe::days(1), &fit), Timeframe::days(1));
        assert_eq!(resolution_for(Range::All, today, Timeframe::minutes(5), &fit), Timeframe::minutes(5));
    }

    #[test]
    fn a_range_out_of_reach_or_too_long_moves_coarser() {
        let today = day(2026, 10, 19);
        let year = Range::Back(1, Span::Year);
        // Five-minute bars go back sixty days. An hour reaches two years, but
        // a year of days is the readier chart.
        let stock = fit(&STRIP, STOCK, &yahoo);
        assert_eq!(resolution_for(year, today, Timeframe::minutes(5), &stock), Timeframe::days(1));
        // A year of bitcoin by the hour is 8,760 bars, by four hours 2,190,
        // by the day 365.
        let crypto = fit(&STRIP, CRYPTO, &yahoo);
        assert_eq!(resolution_for(year, today, Timeframe::minutes(5), &crypto), Timeframe::days(1));
        // Five years of days is 1,300 bars; of weeks, 260.
        let five = Range::Back(5, Span::Year);
        assert_eq!(resolution_for(five, today, Timeframe::minutes(15), &stock), Timeframe::weeks(1));
    }

    /// From far off, the best fit rather than the nearest: a year to date
    /// from 5m is 1D, not 4h's four hundred cramped candles.
    #[test]
    fn from_far_off_the_best_fit_wins() {
        let today = day(2026, 10, 29);
        let stock = Fit { bars: AIM_BARS, ..fit(&STRIP, STOCK, &yahoo) };
        let ytd = Range::ToDate(1, Period::Year);
        assert_eq!(resolution_for(ytd, today, Timeframe::minutes(5), &stock), Timeframe::days(1));
        assert_eq!(resolution_for(ytd, today, Timeframe::days(1), &stock), Timeframe::days(1));
        assert_eq!(weekdays(day(2026, 10, 26), day(2026, 11, 1)), 5, "Monday to Sunday");
        assert_eq!(weekdays(day(2026, 10, 31), day(2026, 11, 1)), 0, "a weekend");
    }

    #[test]
    fn a_range_too_short_moves_finer() {
        let today = day(2026, 10, 19);
        let stock = fit(&STRIP, STOCK, &yahoo);
        // A month of weekly bars is four candles; of daily bars 21, of
        // hourly bars about 140, the middle of the band.
        let month = Range::Back(1, Span::Month);
        assert_eq!(resolution_for(month, today, Timeframe::weeks(1), &stock), Timeframe::hours(1));
        // A week of daily bars is five, of hourly ones thirty-odd, of
        // fifteen-minute ones 130.
        let week = Range::Back(1, Span::Week);
        assert_eq!(resolution_for(week, today, Timeframe::days(1), &stock), Timeframe::minutes(15));
    }

    /// Nothing on the strip fits: the nearest common resolution that does,
    /// and with none, the chart stays put.
    #[test]
    fn nothing_on_the_strip_that_fits_looks_off_it() {
        let today = day(2026, 10, 19);
        let only = [Timeframe::minutes(5)];
        let stock = fit(&only, STOCK, &yahoo);
        let year = Range::Back(1, Span::Year);
        assert_eq!(resolution_for(year, today, Timeframe::minutes(5), &stock), Timeframe::days(1));
        let nothing = Fit { offers: &|tf| tf == Timeframe::minutes(5), ..fit(&only, STOCK, &yahoo) };
        assert_eq!(resolution_for(year, today, Timeframe::minutes(5), &nothing), Timeframe::minutes(5));
    }

    /// A day on a strip that starts at an hour: an off-strip resolution
    /// that fits beats an hour's six candles, while the feed reaches it.
    #[test]
    fn a_short_range_off_the_strip_takes_a_common_resolution() {
        let today = day(2026, 10, 29);
        let strip = [Timeframe::hours(1), Timeframe::hours(4), Timeframe::days(1), Timeframe::weeks(1)];
        let stock = Fit { bars: AIM_BARS, ..fit(&strip, STOCK, &yahoo) };
        let yesterday = Range::Day(End::Yesterday);
        assert_eq!(resolution_for(yesterday, today, Timeframe::hours(1), &stock), Timeframe::minutes(5));
        // Past the sixty days Yahoo keeps minutes for, the hour stays.
        let april = Range::Day(End::Day(day(2026, 4, 27)));
        assert_eq!(resolution_for(april, today, Timeframe::hours(1), &stock), Timeframe::hours(1));
        // A strip that fits keeps its own.
        assert_eq!(resolution_for(Range::Back(1, Span::Year), today, Timeframe::days(1), &stock), Timeframe::days(1));
    }

    /// Picked by the window, a range aims for 50 to 400 bars, and stays as
    /// close to the chart's resolution as that allows.
    #[test]
    fn the_window_aims_for_a_readable_count_of_bars() {
        let today = day(2026, 10, 19);
        let stock = Fit { bars: AIM_BARS, ..fit(&STRIP, STOCK, &yahoo) };
        let at = |range, current| resolution_for(range, today, current, &stock);
        // A year of days is about 260 bars: it stays.
        assert_eq!(at(Range::Back(1, Span::Year), Timeframe::days(1)), Timeframe::days(1));
        // Five years of days is 1,300: weeks are 260.
        assert_eq!(at(Range::Back(5, Span::Year), Timeframe::days(1)), Timeframe::weeks(1));
        // A month of days is 21; of four hours, 35; of hours, 140.
        assert_eq!(at(Range::Back(1, Span::Month), Timeframe::days(1)), Timeframe::hours(1));
        // From further off, it stops at the first that fits.
        assert_eq!(at(Range::Back(1, Span::Year), Timeframe::minutes(5)), Timeframe::days(1));
        // Twenty years of weeks is 1,040, and nothing coarser is on the strip:
        // months, off it, are 240.
        assert_eq!(at(Range::Back(20, Span::Year), Timeframe::days(1)), Timeframe::months(1));
    }

    #[test]
    fn a_year_or_a_month_stops_where_it_ends() {
        let today = day(2026, 10, 19);
        let september = Range::Month(2026, 9);
        assert_eq!(september.start(today), Some(day(2026, 9, 1)));
        assert_eq!(september.end(today), Some(day(2026, 9, 30)));
        assert_eq!(september.days(today), Some(30));
        assert_eq!(september.reach(today), Some(49));
        assert_eq!(Range::Month(2024, 2).end(today), Some(day(2024, 2, 29)));
        assert_eq!(Range::Year(2025).days(today), Some(365));
        // This year so far, not the months still to come.
        assert_eq!(Range::Year(2026).days(today), Some(292));
        assert_eq!(Range::Year(2026).key(), "2026");
        assert_eq!(september.key(), "2026-09");
        assert_eq!(september.description(), "all of September 2026");
        assert_eq!(Range::ToDate(1, Period::Year).end(today), None);
    }

    /// Five-minute bars reach sixty days back, so last month at 5m fits on
    /// count but not on reach: two months ago is the hour's.
    #[test]
    fn a_month_long_ago_is_out_of_reach_by_how_far_back_it_is() {
        let today = day(2026, 10, 19);
        let stock = fit(&STRIP, STOCK, &yahoo);
        let august = Range::Month(2026, 8);
        assert_ne!(resolution_for(august, today, Timeframe::minutes(5), &stock), Timeframe::minutes(5));
    }

    #[test]
    fn the_picker_suggests_what_the_text_could_mean() {
        let today = day(2026, 10, 19);
        let keys = |text: &str| Range::suggest(text, today).iter().map(|r| r.key()).collect::<Vec<_>>();
        let empty = keys("");
        assert_eq!(empty.first().map(String::as_str), Some("YTD"));
        assert!(empty.contains(&"2026-09".to_string()) && empty.contains(&"2026".to_string()));
        assert_eq!(empty.last().map(String::as_str), Some("All"));
        assert_eq!(keys("2Y")[..2], ["2YTD", "2y"]);
        assert_eq!(keys("6m")[..2], ["6m", "6MTD"]);
        assert_eq!(keys("2")[..2], ["2m", "2y"]);
        assert_eq!(keys("20")[..4], ["2026", "2026:", "2026-09", "20m"]);
        assert_eq!(keys("2026"), ["2026", "2026:", "2026-09"]);
        assert_eq!(keys("2025"), ["2025", "2025:"]);
        assert_eq!(keys("2025-09"), ["2025-09", "2025-09:"]);
        assert_eq!(keys("-6m"), keys("6m"), "a dash in front changes nothing");
        assert_eq!(keys("2024:2025"), ["2024:2025"]);
        assert!(empty.contains(&"2025:".to_string()));
        assert_eq!(keys("2026-")[..3], ["2026-10", "2026-09", "2026-08"]);
        assert_eq!(keys("2025-1"), ["2025-12", "2025-11", "2025-10", "2025-01"]);
        assert!(keys("xyz").is_empty());
    }

    #[test]
    fn a_colon_slices_the_calendar_both_ends_included() {
        let today = day(2026, 10, 20);
        let slice = |text: &str| {
            let range = Range::parse(text).unwrap_or_else(|| panic!("{text} should read"));
            (range.start(today), range.end(today), range.key())
        };
        assert_eq!(Range::parse(":"), Some(Range::All));
        assert_eq!(slice("2025:"), (Some(day(2025, 1, 1)), None, "2025:".into()));
        assert_eq!(slice("2024:2025-06"), (Some(day(2024, 1, 1)), Some(day(2025, 6, 30)), "2024:2025-06".into()));
        assert_eq!(slice("2024:2025").1, Some(day(2025, 12, 31)), "the end is held whole, pandas-style");
        assert_eq!(slice(":2020"), (None, Some(day(2020, 12, 31)), ":2020".into()));
        assert_eq!(slice("2026-03-21:"), (Some(day(2026, 3, 21)), None, "2026-03-21:".into()));
        assert_eq!(Range::parse("2026/03/21:2026/09/30"), Range::parse("2026-03-21:2026-09-30"), "a slash dates as a dash does");
        assert_eq!(Range::parse("2024:2025/06"), Range::parse("2024:2025-06"));
        assert_eq!(slice("-2y:-1y"), (Some(day(2024, 10, 21)), Some(day(2025, 10, 20)), "-2y:-1y".into()));
        assert_eq!(Range::parse("-1y:").and_then(|r| r.start(today)), Range::Back(1, Span::Year).start(today));
        assert_eq!(Range::parse("2025:").map(|r| r.description()), Some("from 2025 to today".into()));
        // Ruby's two dots are the same slice.
        for (dots, colon) in [("2025..", "2025:"), ("..2020", ":2020"), ("2025/03..2025/06", "2025/03:2025/06"), ("-6m..-1m", "-6m:-1m"), ("..", ":")] {
            assert_eq!(Range::parse(dots), Range::parse(colon), "{dots}");
            assert!(Range::parse(dots).is_some(), "{dots}");
        }
        assert_eq!(Range::parse("2025...2026"), None, "three dots are not a range");
        assert_eq!(Range::parse("-2y:-1y").map(|r| r.description()), Some("2 years ago to a year ago".into()));
        for bad in ["2025:x", "6m:", "2025-13:", "2025-02-30:", "2025/02/30:", "2025/03-21:", "1:2"] {
            assert_eq!(Range::parse(bad), None, "{bad}");
        }
    }

    /// A day on its own is that day, however it is written, and reads back
    /// as itself.
    #[test]
    fn a_day_is_that_day_alone() {
        // A Thursday.
        let today = day(2026, 10, 29);
        let on = |text: &str| {
            let range = Range::parse(text).unwrap_or_else(|| panic!("{text} should read"));
            assert_eq!(Range::parse(&range.key()), Some(range), "{text} reads back as {}", range.key());
            assert_eq!(range.start(today), range.end(today), "{text} is one day");
            range.start(today)
        };
        assert_eq!(on("2026-03-12"), Some(day(2026, 3, 12)));
        assert_eq!(on("2026/3/12"), Some(day(2026, 3, 12)));
        assert_eq!(on("10/12"), Some(day(2026, 10, 12)), "no year is this year");
        assert_eq!(on("3-5"), Some(day(2026, 3, 5)));
        assert_eq!(on("12/05"), Some(day(2025, 12, 5)), "or last year's, when this year's is still to come");
        assert_eq!(on("2/29"), Some(day(2024, 2, 29)), "the latest year that has it");
        assert_eq!(on("12 mar"), Some(day(2026, 3, 12)));
        assert_eq!(on("Mar 12"), Some(day(2026, 3, 12)));
        assert_eq!(on("12mar2025"), Some(day(2025, 3, 12)));
        assert_eq!(on("sept 1, 2025"), Some(day(2025, 9, 1)));
        assert_eq!(on("mon"), Some(day(2026, 10, 26)));
        assert_eq!(on("thu"), Some(today), "today counts");
        assert_eq!(on("fri"), Some(day(2026, 10, 23)));
        assert_eq!(on("2mon"), Some(day(2026, 10, 19)));
        assert_eq!(on("3Tues"), Some(day(2026, 10, 13)));
        for spelled in ["mond", "monda", "monday", "Monday ", "MON"] {
            assert_eq!(on(spelled), Some(day(2026, 10, 26)), "{spelled}");
        }
        assert_eq!(on("weds"), Some(day(2026, 10, 28)));
        assert_eq!(on("today"), Some(today));
        assert_eq!(on("yesterday"), Some(day(2026, 10, 28)));
        assert_eq!(Range::parse("mon").map(|r| r.description()), Some("Monday".into()));
        assert_eq!(Range::parse("2fri").map(|r| r.description()), Some("Friday a week before".into()));
        // The rest of the grammar is as it was.
        assert_eq!(Range::parse("2m"), Some(Range::Back(2, Span::Month)));
        assert_eq!(Range::parse("1mo"), Some(Range::Back(1, Span::Month)));
        assert_eq!(Range::parse("2026-03"), Some(Range::Month(2026, 3)));
        // A month by name alone is a span, all of the latest March.
        assert_eq!(Range::parse("mar"), Some(Range::LatestMonth(3)));
        for bad in ["2/30", "13/1", "0mon", "12 mar 3", "1/2/3", "2025-3/12"] {
            assert_eq!(Range::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_dash_in_front_changes_nothing() {
        for (dashed, plain) in [("-2w", "2w"), ("-6m", "6m"), ("-Y", "Y"), ("-2ytd", "2ytd"), ("-2mon", "2mon"), ("-fri", "fri")] {
            assert_eq!(Range::parse(dashed), Range::parse(plain), "{dashed}");
            assert!(Range::parse(dashed).is_some(), "{dashed}");
        }
        for bad in ["-2025", "--2w", "-", "-10/12"] {
            assert_eq!(Range::parse(bad), None, "{bad}");
        }
        let today = day(2026, 10, 29);
        assert_eq!(Range::parse("-2mon:-1fri").and_then(|r| r.start(today)), Some(day(2026, 10, 19)));
        assert_eq!(Range::suggest("-2w", today).first(), Range::parse("2w").as_ref());
    }

    #[test]
    fn a_day_ends_a_slice() {
        let today = day(2026, 10, 29);
        // Last week, on a Thursday: the Friday that is latest is last week's.
        let range = Range::parse("2mon:fri").unwrap_or(Range::All);
        assert_eq!((range.start(today), range.end(today)), (Some(day(2026, 10, 19)), Some(day(2026, 10, 23))));
        assert_eq!(Range::parse("mon:").and_then(|r| r.start(today)), Some(day(2026, 10, 26)));
        assert_eq!(Range::parse("mon:").map(|r| r.description()), Some("from Monday to today".into()));
        assert_eq!(Range::parse("10/1..10/15").and_then(|r| r.end(today)), Some(day(2026, 10, 15)));
        assert_eq!(Range::suggest("mon", today), vec![Range::parse("mon").unwrap_or(Range::All), Range::parse("mon:").unwrap_or(Range::All)]);
    }

    /// A range that reads but lands on no day before today is no range to
    /// show: backwards, or wholly in the future.
    #[test]
    fn a_range_with_no_days_up_to_today_holds_none() {
        let today = day(2026, 10, 20);
        let holds = |text: &str| Range::parse(text).is_some_and(|range| range.holds_days(today));
        for good in ["ytd", "1d", "2026-10", "2026", "2024:2025", "-6m:-1m", ":2020", "all", "2026-10-20:"] {
            assert!(holds(good), "{good}");
        }
        for empty in ["2030", "2026-11", "2025:2024", "-1m:-6m", "2026-10-21:"] {
            assert!(!holds(empty), "{empty}");
        }
    }

    /// A day said the way people say one: so long ago, last Friday, a
    /// month by name either side of the day, `on` in front. Each is one day
    /// and reads back as itself.
    #[test]
    fn a_day_can_be_said_in_words() {
        // A Sunday.
        let today = day(2026, 11, 1);
        let on = |text: &str| {
            let range = Range::parse(text).unwrap_or_else(|| panic!("{text} should read"));
            assert!(matches!(range, Range::Day(_)), "{text} is a day, not {range:?}");
            assert_eq!(Range::parse(&range.key()), Some(range), "{text} reads back as {}", range.key());
            range.start(today)
        };
        assert_eq!(on("3 weeks ago"), Some(day(2026, 10, 11)), "to the day");
        assert_eq!(on("3w ago"), Some(day(2026, 10, 11)));
        assert_eq!(on("3wks ago"), Some(day(2026, 10, 11)));
        assert_eq!(on("a week ago"), Some(day(2026, 10, 25)));
        assert_eq!(on("1 day ago"), Some(day(2026, 10, 31)));
        assert_eq!(on("10 days ago"), Some(day(2026, 10, 22)));
        assert_eq!(on("2 months ago"), Some(day(2026, 9, 1)));
        assert_eq!(on("1y ago"), Some(day(2025, 11, 1)));
        assert_eq!(on("3D AGO"), Some(day(2026, 10, 29)), "in any case");
        assert_eq!(on("last fri"), Some(day(2026, 10, 30)));
        assert_eq!(on("last sunday"), Some(day(2026, 10, 25)), "last is before today, never today");
        assert_eq!(on("sun"), Some(today), "where a weekday alone is today");
        assert_eq!(on("oct 22"), Some(day(2026, 10, 22)));
        assert_eq!(on("22 oct"), Some(day(2026, 10, 22)));
        assert_eq!(on("october 22"), Some(day(2026, 10, 22)));
        assert_eq!(on("22 October 2025"), Some(day(2025, 10, 22)));
        assert_eq!(on("10/22"), Some(day(2026, 10, 22)), "month first, as the year comes first");
        assert_eq!(on("2026/10/22"), Some(day(2026, 10, 22)));
        assert_eq!(on("on oct 22"), Some(day(2026, 10, 22)));
        assert_eq!(on("on the 22 oct"), Some(day(2026, 10, 22)));
        assert_eq!(on("on friday"), Some(day(2026, 10, 30)));
        assert_eq!(on("nov 2"), Some(day(2025, 11, 2)), "no year is the latest that has passed");
        assert_eq!(Range::parse("3 weeks ago").map(|r| r.description()), Some("3 weeks ago".into()));
        assert_eq!(Range::parse("last fri").map(|r| r.description()), Some("last Friday".into()));
        // Ranges say how long they are, and stay ranges.
        assert_eq!(Range::parse("3w"), Some(Range::Back(3, Span::Week)));
        assert_eq!(Range::parse("ytd"), Some(Range::ToDate(1, Period::Year)));
        for bad in ["ago", "3 ago", "3 fortnights ago", "0 days ago", "last", "last oct", "on", "on the", "-3 weeks ago", "22nd oct"] {
            assert_eq!(Range::parse(bad), None, "{bad}");
        }
    }

    /// A day with no year is the latest one that has passed, which across
    /// New Year is last year's.
    #[test]
    fn a_day_with_no_year_looks_back_across_new_year() {
        let new_year = day(2027, 1, 1);
        let start = |text: &str| Range::parse(text).and_then(|r| r.start(new_year));
        assert_eq!(start("dec 30"), Some(day(2026, 12, 30)));
        assert_eq!(start("12/31"), Some(day(2026, 12, 31)));
        assert_eq!(start("jan 1"), Some(new_year), "today has passed");
        assert_eq!(start("jan 2"), Some(day(2026, 1, 2)), "tomorrow's is a year back");
        assert_eq!(start("2 weeks ago"), Some(day(2026, 12, 18)));
        assert_eq!(start("last thu"), Some(day(2026, 12, 31)), "1 January 2027 is a Friday");
    }

    /// Days still to come read, so that they can be refused as holding no
    /// day up to today, the same as `2030`, rather than meaning something
    /// else.
    #[test]
    fn a_day_still_to_come_holds_none() {
        let today = day(2026, 11, 1);
        for ahead in ["tomorrow", "in 2 days", "2 days from now", "in a week", "next fri", "tomorrow:"] {
            let range = Range::parse(ahead).unwrap_or_else(|| panic!("{ahead} should read"));
            assert!(!range.holds_days(today), "{ahead}");
        }
        assert_eq!(Range::parse("next fri").and_then(|r| r.start(today)), Some(day(2026, 11, 6)));
        assert_eq!(Range::parse("tomorrow").map(|r| r.key()), Some("tomorrow".into()));
    }

    /// Either end of a slice is any day, in words as in numbers.
    #[test]
    fn a_day_in_words_ends_a_slice() {
        let today = day(2026, 11, 1);
        let ends = |text: &str| {
            let range = Range::parse(text).unwrap_or_else(|| panic!("{text} should read"));
            assert_eq!(Range::parse(&range.key()), Some(range), "{text} reads back as {}", range.key());
            (range.start(today), range.end(today))
        };
        assert_eq!(ends("3 weeks ago:"), (Some(day(2026, 10, 11)), None), "to today");
        assert_eq!(ends(":oct 22"), (None, Some(day(2026, 10, 22))));
        assert_eq!(ends("oct 1:oct 22"), (Some(day(2026, 10, 1)), Some(day(2026, 10, 22))));
        assert_eq!(ends("6 months ago:1 month ago"), (Some(day(2026, 5, 1)), Some(day(2026, 10, 1))));
        assert_eq!(ends("last fri..yesterday"), (Some(day(2026, 10, 30)), Some(day(2026, 10, 31))));
        assert_eq!(ends("on oct 1..on oct 22"), ends("oct 1:oct 22"));
        // `-3w:` is where the last three weeks begin, a day after 3 weeks ago.
        assert_eq!(ends("-3w:").0, Some(day(2026, 10, 12)));
        assert_eq!(Range::parse("3 weeks ago:").map(|r| r.spelled(today)), Some("since 11 Oct".into()));
        assert_eq!(Range::parse("oct 11:oct 21").map(|r| r.spelled(today)), Some("11 Oct – 21 Oct".into()));
    }

    /// Noon on `date`, the moment the tests below call now.
    fn noon(date: NaiveDate) -> NaiveDateTime {
        date.and_time(NaiveTime::MIN) + Duration::hours(12)
    }

    /// A date typed on a chart lasts as long as the chart shows: with ten
    /// days on screen, every way of naming a day is ten days that hold it,
    /// from the day on, or up to now when that would run past it.
    #[test]
    fn a_date_on_a_chart_lasts_as_long_as_the_view() {
        let today = day(2026, 11, 1);
        let now = noon(today);
        let ten = 10 * 86_400;
        for text in [
            "2 days ago", "3 days ago", "5 days ago", "10/23", "yesterday", "last fri", "fri", "1 week ago",
            "on the 28 oct", "2026/10/22", "2026/10/12", "today",
        ] {
            let range = Range::parse(text).map(|r| r.lasting(ten, today, now)).unwrap_or(Range::All);
            let (start, end) = (range.start(today), range.end(today));
            let typed = Range::parse(text).and_then(|r| r.start(today));
            assert!(matches!(range, Range::Lasting(..)), "{text} is a date");
            assert_eq!(end.zip(start).map(|(end, start)| (end - start).num_days()), Some(10), "{text}");
            assert!(start <= typed && typed <= end && end <= Some(today), "{text} holds its day, up to today");
            assert!(range.holds_days(today), "{text}");
            assert!(!range.spelled(today).starts_with("since"), "{text} names its window");
        }
        let week = Range::parse("1 week ago").map(|r| r.lasting(ten, today, now));
        assert_eq!(week.map(|r| r.spelled(today)), Some("22 Oct – 1 Nov".into()));
        assert_eq!(week.map(|r| r.key()), Some("1w ago".into()), "it reads back as the day");
        let back = Range::parse("2026/10/12").map(|r| r.lasting(ten, today, now));
        assert_eq!(back.map(|r| r.spelled(today)), Some("12 Oct – 22 Oct".into()), "far enough back, from the day");
        // Less than a day on screen is the day itself.
        assert_eq!(Range::parse("oct 22").map(|r| r.lasting(6 * 3_600, today, now).spelled(today)), Some("22 Oct".into()));
        // A custom range open at the end still runs to today.
        assert_eq!(Range::parse("1 week ago:").map(|r| r.lasting(ten, today, now).spelled(today)), Some("since 25 Oct".into()));
        // Spans and ranges keep their own length.
        assert_eq!(Range::parse("2025").map(|r| r.lasting(ten, today, now)), Some(Range::Year(2025)));
        assert_eq!(Range::parse("1w").map(|r| r.lasting(ten, today, now)), Some(Range::Back(1, Span::Week)));
        assert_eq!(Range::parse("oct 1:").map(|r| r.lasting(ten, today, now)), Range::parse("oct 1:"));
    }

    /// A date near now on a chart that shows a long while would run past
    /// now from its day on, so it ends at now and starts as much earlier:
    /// the length kept, the day inside it. `today` on six weeks is the six
    /// weeks up to now, and `10 days ago` on eight years the eight years.
    #[test]
    fn a_date_near_now_ends_at_now_and_keeps_the_length() {
        let today = day(2026, 11, 2);
        let now = noon(today);
        let six_weeks = 42 * 86_400;
        let eight_years = (today - day(2018, 11, 2)).num_days() * 86_400;
        for (text, seconds, spelled, said) in [
            ("today", six_weeks, "21 Sep – 2 Nov", "the 42 days up to now"),
            ("10 days ago", eight_years, "2 Nov 2018 – 2 Nov", "the 2922 days up to now"),
        ] {
            let range = Range::parse(text).map(|r| r.lasting(seconds, today, now)).unwrap_or(Range::All);
            let ends = |newest| {
                let (from, seconds) = range.lasting_on(today, newest)?;
                Some((from + Duration::seconds(seconds), seconds))
            };
            assert_eq!(ends(now - Duration::hours(3)), Some((now, seconds)), "{text} ends at now, as long as it was");
            assert_eq!(range.end(today), Some(today), "{text}");
            assert_eq!(range.spelled(today), spelled, "{text}");
            assert_eq!(range.description(), said, "{text}");
            assert_eq!(Some(range.key()), Range::parse(text).map(|r| r.key()), "{text} reads back as the day");
            // A bar that comes in later moves the end on with it.
            let later = now + Duration::hours(4);
            assert_eq!(ends(later), Some((later, seconds)), "{text} stays at the live edge");
        }
        // Far enough back for its length to end before now, a day starts the
        // window, and the window does not follow the bars.
        let back = Range::parse("2026/09/01").map(|r| r.lasting(six_weeks, today, now));
        assert_eq!(back.map(|r| r.spelled(today)), Some("1 Sep – 13 Oct".into()));
        assert_eq!(
            back.and_then(|r| r.lasting_on(today, now)),
            Some((day(2026, 9, 1).and_time(NaiveTime::MIN), six_weeks))
        );
        // A live edge further on than the day has to spare stops at the day:
        // a year on a weekly chart, whose ten bars of room are ten weeks.
        let close = Range::parse("2025/12/02").map(|r| r.lasting(365 * 86_400, today, now));
        assert_eq!(
            close.and_then(|r| r.lasting_on(today, now + Duration::weeks(10))).map(|(from, _)| from),
            Some(day(2025, 12, 2).and_time(NaiveTime::MIN)),
            "the day stays on screen"
        );
        // Late in the evening on the chart's clock it is already tomorrow on
        // the calendar's: a day named relative to today ends where the same
        // date typed out does, at now.
        let evening = day(2026, 11, 1).and_time(NaiveTime::MIN) + Duration::hours(18);
        let spelled = |text| Range::parse(text).map(|r| r.lasting(eight_years, today, evening).pinned(today).spelled(today));
        assert_eq!(spelled("10 days ago"), spelled("2026/10/23"));
        assert_eq!(spelled("10 days ago").map(|s| s.ends_with("1 Nov")), Some(true), "it ends at now, on the chart's clock");
    }

    /// A day named on the calendar is pinned to its date before a chart,
    /// which reckons today from its last bar, is given it; a span is not.
    #[test]
    fn a_day_is_pinned_to_the_calendar_and_a_span_is_not() {
        let sunday = day(2026, 11, 1);
        let pinned = |text: &str| Range::parse(text).map(|r| r.pinned(sunday));
        assert_eq!(pinned("3 weeks ago"), Some(Range::Day(End::Day(day(2026, 10, 11)))));
        assert_eq!(pinned("oct 31"), Some(Range::Day(End::Day(day(2026, 10, 31)))));
        assert_eq!(
            pinned("3 weeks ago:"),
            Some(Range::Slice(Some(End::Day(day(2026, 10, 11))), None))
        );
        assert_eq!(pinned("fri:yesterday"), Range::parse("2026-10-30:2026-10-31"));
        for span in ["3w", "ytd", "2025", "2025-03", "-6m:-1m", "2025:", "all"] {
            assert_eq!(pinned(span), Range::parse(span), "{span}");
        }
        assert_eq!(pinned("oct 2:oct 12").map(|r| r.short_key(2026)), Some("10-02:10-12".into()));
        assert_eq!(pinned("2025/10/22").map(|r| r.short_key(2026)), Some("2025-10-22".into()));
        assert_eq!(pinned("ytd").map(|r| r.short_key(2026)), Some("YTD".into()));
        assert_eq!(pinned("jan:mar").map(|r| r.short_key(2026)), Some("jan:mar".into()));
        assert_eq!(pinned("june").map(|r| r.short_key(2026)), Some("jun".into()));
        assert_eq!(pinned("june 2025").map(|r| r.short_key(2026)), Some("2025-06".into()));
        let lasting = Range::parse("1 week ago").map(|r| r.lasting(86_400, sunday, noon(sunday)).pinned(sunday));
        assert_eq!(lasting, Some(Range::Lasting(End::Day(day(2026, 10, 25)), 86_400, 0)));
        let today = Range::parse("today").map(|r| r.lasting(86_400, sunday, noon(sunday)).pinned(sunday));
        assert_eq!(today, Some(Range::Lasting(End::Day(sunday), 86_400, 12 * 3_600)), "pinned, it keeps its window");
    }

    fn utc(ts: i64) -> Option<NaiveDateTime> {
        chrono::DateTime::from_timestamp(ts, 0).map(|t| t.naive_utc())
    }

    fn at(date: NaiveDate, hour: i64) -> i64 {
        date.and_hms_opt(0, 0, 0).map_or(0, |t| t.and_utc().timestamp()) + hour * 3_600
    }

    /// A stock's weekdays from 21 September to Friday 30 October, a bar
    /// every `step` seconds from 14:00 to 20:59.
    fn sessions(step: i64) -> Vec<i64> {
        (0..40)
            .map(|d| day(2026, 9, 21) + Days::new(d))
            .filter(|d| d.weekday().num_days_from_monday() < 5)
            .flat_map(|d| (0..7 * 3_600 / step).map(move |i| at(d, 14) + i * step))
            .collect()
    }

    /// The first and last moment on screen, the window as
    /// [`lasting_window`] gives it, and what the card calls it.
    type Shown = (i64, i64, (usize, usize, usize), String);

    /// A date as the window shows it: made a [`Range::Lasting`] of
    /// `seconds` at noon on the calendar's today, as the card and `chart
    /// range` do, pinned to its date, and laid over `bars` from the chart's
    /// own today, its last bar's, as the chart does. The first and last
    /// moment its columns stand for, as the time axis reads them, and what
    /// the card and the command call it.
    fn shown(text: &str, bars: &[i64], seconds: i64, step: i64) -> Option<Shown> {
        let today = day(2026, 11, 1);
        let range = Range::parse(text)?.lasting(seconds, today, noon(today)).pinned(today);
        let newest = bars.last().and_then(|&ts| utc(ts))?;
        let (from, seconds) = range.lasting_on(newest.date(), newest)?;
        let window = lasting_window(bars, |&ts| utc(ts), from, seconds, step);
        let (first, columns, _) = window;
        let moment = |column: usize| match bars.get(column) {
            Some(&ts) => ts,
            None => bars[bars.len() - 1] + (column + 1 - bars.len()) as i64 * step,
        };
        let left = moment(first.min(bars.len() - 1));
        let right = moment((first + columns).max(1) - 1).max(left);
        Some((left, right, window, range.spelled(today)))
    }

    /// Whatever the date and the resolution, the chart shows as long as it
    /// showed before, from the start of that day or, when that would run
    /// past now, up to now: its columns begin where that stretch does and
    /// end at the last moment inside it, bars or the empty room past the
    /// last bar alike, and the day is inside it.
    #[test]
    fn a_date_shows_exactly_its_stretch_at_any_resolution() {
        for (step, length) in [(900, 2 * 86_400), (3_600, 10 * 86_400), (86_400, 30 * 86_400)] {
            let bars = sessions(step.min(3_600));
            let bars: Vec<i64> = match step {
                86_400 => bars.iter().filter(|&&ts| (ts - 14 * 3_600).rem_euclid(86_400) == 0).copied().collect(),
                _ => bars,
            };
            for (text, date) in [
                ("2 days ago", day(2026, 10, 30)),
                ("3 days ago", day(2026, 10, 29)),
                ("5 days ago", day(2026, 10, 27)),
                ("10/23", day(2026, 10, 23)),
                ("last fri", day(2026, 10, 30)),
                ("2 weeks ago", day(2026, 10, 18)),
            ] {
                let Some((left, right, (first, columns, room), _)) = shown(text, &bars, length, step) else {
                    panic!("{text} at {step}s");
                };
                let from = at(date, 0).min(at(day(2026, 11, 1), 12) - length);
                let until = from + length;
                assert!((from..=until).contains(&at(date, 0)), "{text} at {step}s holds its day");
                assert!(left >= from, "{text} at {step}s starts where its stretch does");
                assert!(first == 0 || bars[first - 1] < from, "{text} at {step}s starts at the stretch's first bar");
                assert!(right <= until, "{text} at {step}s ends inside its stretch");
                let next = match room {
                    0 => bars.get(first + columns).copied(),
                    _ => Some(right + step),
                };
                assert!(next.is_none_or(|next| next > until), "{text} at {step}s ends {length}s after its day began");
                assert_eq!(room > 0, until >= bars[bars.len() - 1] + step, "{text} at {step}s: room only past the last bar");
            }
        }
    }

    /// A Saturday: the stretch starts on the Saturday, which has no bars,
    /// so the first column is Monday's first bar, and it ends where the
    /// length of time says, over the next weekend too.
    #[test]
    fn a_date_on_a_weekend_keeps_its_stretch() {
        let bars = sessions(3_600);
        let (left, right, _, spelled) = shown("10/24", &bars, 7 * 86_400, 3_600).unwrap_or_default();
        assert_eq!(left, at(day(2026, 10, 26), 14), "Monday's first bar");
        assert_eq!(right, at(day(2026, 10, 31), 0), "four columns of room past Friday's last bar, to Saturday 31 October");
        assert_eq!(spelled, "24 Oct – 31 Oct");
    }

    /// Wholly past the last bar, all of the stretch is empty room, counted
    /// at the resolution's step.
    #[test]
    fn a_date_past_the_last_bar_is_all_room() {
        let bars = sessions(3_600);
        let saturday = day(2026, 10, 31).and_time(NaiveTime::MIN);
        let (first, columns, room) = lasting_window(&bars, |&ts| utc(ts), saturday, 86_400, 3_600);
        assert_eq!((first, columns, room), (bars.len(), 28, 28), "20:00 Friday to midnight Saturday is 28 hours");
    }

    /// Every way of saying a day goes through the same window as the date
    /// it names: `2 days ago` on Sunday 1 November is Friday 30 October,
    /// the last bar's day, and shows exactly what `10/30` shows; so for
    /// weeks back, yesterday, last Friday and a weekday alone.
    #[test]
    fn a_relative_day_shows_exactly_what_its_date_does() {
        let bars = sessions(900);
        let length = 2 * 86_400;
        for (relative, fixed) in [
            ("2 days ago", "10/30"),
            ("3 days ago", "10/29"),
            ("5 days ago", "10/27"),
            ("2d ago", "2026/10/30"),
            ("1 week ago", "10/25"),
            ("a week ago", "oct 25"),
            ("2 weeks ago", "18 oct"),
            ("yesterday", "10/31"),
            ("last fri", "10/30"),
            ("fri", "10/30"),
            ("on the 28 oct", "10/28"),
        ] {
            let a = shown(relative, &bars, length, 900);
            assert!(a.is_some(), "{relative} reads as a day");
            assert_eq!(a, shown(fixed, &bars, length, 900), "{relative} and {fixed}");
            let (_, _, _, spelled) = a.unwrap_or_default();
            assert!(spelled.contains(" – "), "{relative} names its whole stretch: {spelled}");
        }
    }

    /// A count and a unit read the same everywhere one is taken: as a span,
    /// so long ago, and as either end of a custom range, with `:` or `..`
    /// and with or without spaces around it, in the From and To boxes too.
    #[test]
    fn a_count_and_a_unit_read_the_same_in_every_place() {
        let units: [(&[&str], u32, Span); 5] = [
            (&["d", " d", "dy", "day", " days", " Days", "DAYS"], 3, Span::Day),
            (&["w", " w", "wk", "wks", " week", " weeks", " Weeks"], 3, Span::Week),
            (&["m", " m", "mo", "mos", "mth", " month", " months", " Months"], 3, Span::Month),
            (&["q", " q", "qtr", "qtrs", " quarter", " quarters"], 9, Span::Month),
            (&["y", " y", "yr", "yrs", " year", " years", " YEARS"], 3, Span::Year),
        ];
        for (spellings, count, span) in units {
            for unit in spellings {
                let said = format!("3{unit}");
                assert_eq!(Range::parse(&said), Some(Range::Back(count, span)), "{said} as a span");
                assert_eq!(Range::parse(&format!("-{said}")), Some(Range::Back(count, span)), "-{said}");
                let ago = End::Before(count, span);
                assert_eq!(Range::parse(&format!("{said} ago")), Some(Range::Day(ago)), "{said} ago");
                assert_eq!(End::parse(&format!("{said} ago")), Some(ago), "{said} ago in From or To");
                let back = End::Ago(count, span);
                assert_eq!(End::parse(&format!("-{said}")), Some(back), "-{said} in From or To");
                for (text, range) in [
                    (format!("-{said}:"), Range::Slice(Some(back), None)),
                    (format!("-{said} .."), Range::Slice(Some(back), None)),
                    (format!(":-{said}"), Range::Slice(None, Some(back))),
                    (format!(".. -{said}"), Range::Slice(None, Some(back))),
                    (format!("{said} ago:"), Range::Slice(Some(ago), None)),
                    (format!("{said} ago ..{said} ago"), Range::Slice(Some(ago), Some(ago))),
                    (format!(" : {said} ago"), Range::Slice(None, Some(ago))),
                ] {
                    assert_eq!(Range::parse(&text), Some(range), "{text}");
                }
            }
        }
        // With a word for the unit, as with a letter.
        for (words, letters) in [("3 days", "3d"), ("2 weeks", "2w"), ("6 months", "6m"), ("1 year", "1y"), ("2 quarters", "2q")] {
            assert_eq!(Range::parse(words), Range::parse(letters), "{words}");
            assert!(Range::parse(words).is_some(), "{words}");
        }
        // A capital alone is still a period to date.
        assert_eq!(Range::parse("3M"), Some(Range::ToDate(3, Period::Month)));
        assert_eq!(Range::parse("3 Y"), Some(Range::ToDate(3, Period::Year)));
        assert_eq!(Range::parse("3 days ago").map(|r| matches!(r, Range::Day(_))), Some(true));
    }

    /// Every way of naming a day works alone and at either end of a custom
    /// range, with `:` or `..`, spaced or not, and in From and To.
    #[test]
    fn a_day_reads_the_same_in_every_place() {
        for text in [
            "oct 22", "22 oct", "october 22", "22 october 2025", "10/22", "2026/10/22", "2026-10-22", "3 weeks ago",
            "3w ago", "a week ago", "yesterday", "today", "last fri", "fri", "2mon", "on oct 22", "on the 22 oct",
        ] {
            let end = End::parse(text).unwrap_or_else(|| panic!("{text} in From or To"));
            assert_eq!(Range::parse(text), Some(Range::Day(end)), "{text} alone");
            for (slice, range) in [
                (format!("{text}:"), Range::Slice(Some(end), None)),
                (format!("{text} :"), Range::Slice(Some(end), None)),
                (format!("{text}.."), Range::Slice(Some(end), None)),
                (format!("{text} .. "), Range::Slice(Some(end), None)),
                (format!(":{text}"), Range::Slice(None, Some(end))),
                (format!(".. {text}"), Range::Slice(None, Some(end))),
                (format!("{text} : {text}"), Range::Slice(Some(end), Some(end))),
                (format!("{text}..{text}"), Range::Slice(Some(end), Some(end))),
            ] {
                assert_eq!(Range::parse(&slice), Some(range), "{slice}");
            }
        }
    }

    /// A month by name, full or short and in any case, is all of the
    /// latest one: alone, with a year, and at either end of a custom range,
    /// in From and To too.
    #[test]
    fn a_month_by_name_is_all_of_the_latest_one_everywhere() {
        let today = day(2026, 11, 1);
        let months = [
            ("january", 1), ("jan", 1), ("JAN", 1), ("february", 2), ("feb", 2), ("march", 3), ("mar", 3),
            ("april", 4), ("apr", 4), ("may", 5), ("June", 6), ("jun", 6), ("july", 7), ("jul", 7),
            ("august", 8), ("aug", 8), ("september", 9), ("sep", 9), ("Sept", 9), ("october", 10), ("oct", 10),
            ("november", 11), ("nov", 11), ("december", 12), ("dec", 12),
        ];
        for (name, month) in months {
            let year = if month <= 11 { 2026 } else { 2025 };
            let first = day(year, month, 1);
            let last = first.checked_add_months(Months::new(1)).and_then(|d| d.pred_opt());
            let range = Range::parse(name);
            assert_eq!(range, Some(Range::LatestMonth(month)), "{name}");
            assert_eq!(range.and_then(|r| r.start(today)), Some(first), "{name} starts on the 1st");
            assert_eq!(range.and_then(|r| r.end(today)), last, "{name} ends on its last day");
            assert_eq!(range.map(|r| r.pinned(today)), Some(Range::Month(year, month)), "{name} pinned");
            assert_eq!(range.map(|r| r.spelled(today)), Some(first.format("%B %Y").to_string()), "{name} spelled");
            assert_eq!(range.map(|r| Range::parse(&r.key())), Some(range), "{name} reads back");
            let end = End::LatestMonth(month);
            assert_eq!(End::parse(name), Some(end), "{name} in From or To");
            for (text, range) in [
                (format!("{name} 2025"), Range::Month(2025, month)),
                (format!("2025 {name}"), Range::Month(2025, month)),
                (format!("{name}:"), Range::Slice(Some(end), None)),
                (format!("{name} .."), Range::Slice(Some(end), None)),
                (format!(":{name}"), Range::Slice(None, Some(end))),
                (format!(".. {name}"), Range::Slice(None, Some(end))),
                (format!("{name} : {name}"), Range::Slice(Some(end), Some(end))),
                (format!("{name} 2025:"), Range::Slice(Some(End::Month(2025, month)), None)),
            ] {
                assert_eq!(Range::parse(&text), Some(range), "{text}");
            }
        }
        // A range of months by name, the latest of each.
        let quarter = Range::parse("jan:mar");
        assert_eq!(quarter.and_then(|r| r.start(today)), Some(day(2026, 1, 1)));
        assert_eq!(quarter.and_then(|r| r.end(today)), Some(day(2026, 3, 31)));
        assert_eq!(Range::parse("june:").and_then(|r| r.start(today)), Some(day(2026, 6, 1)));
        // What already meant something else still does.
        assert!(matches!(Range::parse("mon"), Some(Range::Day(End::Weekday(1, Weekday::Mon)))));
        assert!(matches!(Range::parse("sat"), Some(Range::Day(End::Weekday(1, Weekday::Sat)))));
        assert_eq!(Range::parse("oct 22"), Some(Range::Day(End::MonthDay(10, 22))));
        assert_eq!(Range::parse("22 oct"), Some(Range::Day(End::MonthDay(10, 22))));
        assert_eq!(Range::parse("3 mar"), Some(Range::Day(End::MonthDay(3, 3))));
        assert_eq!(Range::parse("m"), Some(Range::Back(1, Span::Month)));
        assert_eq!(Range::parse("2026-09"), Some(Range::Month(2026, 9)));
        assert_eq!(Range::parse("ma"), None);
    }

    /// This month by name is all of it, to its last day; January typed in
    /// January is this one, and in December last year's.
    #[test]
    fn a_month_by_name_this_month_and_across_new_year() {
        let november = Range::parse("november");
        assert_eq!(november.and_then(|r| r.end(day(2026, 11, 1))), Some(day(2026, 11, 30)));
        assert!(november.is_some_and(|r| r.holds_days(day(2026, 11, 1))));
        for (today, text, year) in [
            (day(2027, 1, 15), "january", 2027),
            (day(2027, 1, 15), "december", 2026),
            (day(2026, 12, 20), "january", 2026),
            (day(2026, 12, 20), "december", 2026),
            (day(2027, 1, 1), "jan", 2027),
        ] {
            let start = Range::parse(text).and_then(|r| r.start(today));
            assert_eq!(start.map(|d| d.year()), Some(year), "{text} on {today}");
        }
    }

    /// A month the last bar falls in shows all of itself: the bars from its
    /// first, then empty room to the end of its last day.
    #[test]
    fn this_month_runs_to_its_end_past_the_last_bar() {
        let bars = sessions(3_600);
        let start = day(2026, 10, 1);
        let end = day(2026, 10, 31);
        let seconds = (end - start).num_days() * 86_400 + 86_399;
        let (first, columns, room) = lasting_window(&bars, |&ts| utc(ts), start.and_time(NaiveTime::MIN), seconds, 3_600);
        assert_eq!(bars[first], at(start, 14), "the 1st's first bar");
        assert_eq!(room, 27, "20:00 Friday 30 October to the end of Saturday 31 October");
        assert_eq!(columns, bars.len() - first + room);
        // A month that is over is its own bars, edge to edge.
        let september = day(2026, 9, 1).and_time(NaiveTime::MIN);
        let (first, columns, room) = lasting_window(&bars, |&ts| utc(ts), september, 29 * 86_400 + 86_399, 3_600);
        assert_eq!((utc(bars[first]).map(|t| t.date()), room), (Some(day(2026, 9, 21)), 0));
        assert_eq!(utc(bars[first + columns - 1]).map(|t| t.date()), Some(day(2026, 9, 30)));
    }
}
