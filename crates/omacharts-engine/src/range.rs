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

use chrono::{Datelike, Days, Months, NaiveDate, Weekday};

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
    /// One day: a date, `mon`, `10/12`, `yesterday`. Always one of the
    /// [`End`]s that is a day.
    Day(End),
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
}

impl End {
    /// `2025`, `2025-06`, `2025-06-30`, `-6m`, or any other day
    /// [`one_day`] reads.
    pub fn parse(text: &str) -> Option<End> {
        let text = text.trim();
        if let Some(ago) = text.strip_prefix('-') {
            let split = ago.find(|c: char| !c.is_ascii_digit())?;
            let (digits, unit) = ago.split_at(split);
            let count: u32 = match digits {
                "" => 1,
                digits => digits.parse().ok().filter(|&n| n > 0 && n <= 100)?,
            };
            let span = match unit.to_ascii_lowercase().as_str() {
                "d" => Span::Day,
                "w" => Span::Week,
                "m" => Span::Month,
                "y" => Span::Year,
                // `-2mon` is `2mon`: a weekday already counts back.
                _ => return one_day(ago).filter(|end| matches!(end, End::Weekday(..))),
            };
            return Some(End::Ago(count, span));
        }
        if let Some(day) = one_day(text) {
            return Some(day);
        }
        match calendar(text)? {
            Range::Year(year) => Some(End::Year(year)),
            Range::Month(year, month) => Some(End::Month(year, month)),
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
        }
    }

    /// The last day it holds: the day before a `-6m` slice would start.
    pub fn last(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            End::Day(day) => Some(day),
            End::Month(year, month) => {
                NaiveDate::from_ymd_opt(year, month, 1)?.checked_add_months(Months::new(1))?.pred_opt()
            }
            End::Year(year) => NaiveDate::from_ymd_opt(year, 12, 31),
            End::Ago(..) => self.first(today)?.pred_opt(),
            End::Weekday(..) | End::MonthDay(..) | End::Today | End::Yesterday => self.first(today),
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
            End::Ago(count, span) => {
                let unit = match span {
                    Span::Day => "day",
                    Span::Week => "week",
                    Span::Month => "month",
                    Span::Year => "year",
                };
                match count {
                    1 => format!("a {unit} ago"),
                    _ => format!("{count} {unit}s ago"),
                }
            }
            End::Weekday(count, weekday) => {
                let name = NaiveDate::from_isoywd_opt(2026, 1, weekday).map_or(String::new(), |d| d.format("%A").to_string());
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
    match lower.as_str() {
        "today" => return Some(End::Today),
        "yesterday" => return Some(End::Yesterday),
        _ => {}
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
            Range::Day(day) => day.key(),
            Range::Slice(from, to) => {
                format!("{}:{}", from.map(End::key).unwrap_or_default(), to.map(End::key).unwrap_or_default())
            }
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
            Range::All => "whole series".into(),
            Range::Year(year) => format!("all of {year}"),
            Range::Month(year, month) => match NaiveDate::from_ymd_opt(year, month, 1) {
                Some(first) => format!("all of {}", first.format("%B %Y")),
                None => format!("{year}-{month:02}"),
            },
            Range::Day(day) => day.words(),
            Range::Slice(from, to) => match (from, to) {
                (Some(from), None) => format!("from {} to today", from.words()),
                (None, Some(to)) => format!("up to {}", to.words()),
                (Some(from), Some(to)) => format!("{} to {}", from.words(), to.words()),
                (None, None) => "whole series".into(),
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
        let split = text.find(|c: char| !c.is_ascii_digit())?;
        let (digits, unit) = text.split_at(split);
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
        let (count, span) = match unit.to_ascii_lowercase().as_str() {
            "d" | "day" | "days" => (count, Span::Day),
            "w" | "wk" | "week" | "weeks" => (count, Span::Week),
            "m" | "mo" | "month" | "months" => (count, Span::Month),
            "q" => (count * 3, Span::Month),
            "y" | "yr" | "year" | "years" => (count, Span::Year),
            _ => return None,
        };
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
            Range::Day(day) => day.first(today),
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
            Range::Day(day) => day.last(today),
            _ => None,
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
        // A day: it alone, then from it to today.
        if let Some(Range::Day(day)) = Range::parse(text) {
            return vec![Range::Day(day), Range::Slice(Some(day), None)];
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
        for bad in ["2/30", "13/1", "mar", "0mon", "12 mar 3", "1/2/3", "2025-3/12"] {
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
}
