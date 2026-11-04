//! Dated futures contracts: the months a root lists, which of them is the
//! front month, and when the next takes over.
//!
//! A futures root, `ES`, is a continuous series: whatever contract is
//! trading now, and the one after it from the day the market moves over. A
//! dated contract, `ESH7`, is one of those: March 2027, and nothing else,
//! until it expires. Both are the same kind of instrument, and the symbol
//! tells them apart — a root, then a month code and a year, in the
//! exchange's own spelling, which is what every feed and broker starts
//! from.
//!
//! Which contract is the front month is a feed's to say, and most do. When
//! one cannot — Rithmic Test, for one, answers "no data" — the rule here
//! says instead: the months the root lists, when each stops trading and
//! when traders move to the next, in `contracts.tsv` for the roots that
//! matter and a plain default for the rest. The rule also lists the
//! contracts of a root for a feed that lists none, so that search can show
//! them without asking anybody.
//!
//! Business days here are weekdays that are not one of the US exchanges'
//! holidays, the ten every year keeps. A one-off closure moves a date a
//! day once in a long while, and a feed that knows the real date says so.

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{Datelike, Days, NaiveDate, Weekday};

use crate::exchanges::registry;

/// The exchanges' month codes, January to December.
pub const CODES: [char; 12] = ['F', 'G', 'H', 'J', 'K', 'M', 'N', 'Q', 'U', 'V', 'X', 'Z'];

/// How many contracts of a root are listed when nobody lists them: two
/// years of a quarterly cycle, eight months of a monthly one.
const LISTED: usize = 8;

/// When no rule says otherwise, the front month moves to the next contract
/// this many days before the one trading now expires.
pub const ROLL_DAYS: u32 = 8;

/// A delivery month: `H7` is March 2027.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Month {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
}

impl Month {
    pub fn new(year: i32, month: u32) -> Option<Month> {
        (1..=12).contains(&month).then_some(Month { year, month })
    }

    /// The month a code names, in `year`.
    pub fn of(code: char, year: i32) -> Option<Month> {
        let at = CODES.iter().position(|c| *c == code.to_ascii_uppercase())?;
        Some(Month { year, month: at as u32 + 1 })
    }

    pub fn code(self) -> char {
        CODES[self.month as usize - 1]
    }

    /// `H7`: the code and the year's last digit, which is how the exchange
    /// and every feed spell it.
    pub fn short(self) -> String {
        format!("{}{}", self.code(), self.year.rem_euclid(10))
    }

    /// `Mar 2027`.
    pub fn label(self) -> String {
        format!("{} {}", NAMES[self.month as usize - 1], self.year)
    }

    pub fn first_day(self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, 1).expect("a month between 1 and 12")
    }

    /// `months` later; earlier when negative.
    pub fn plus(self, months: i32) -> Month {
        let index = self.year * 12 + self.month as i32 - 1 + months;
        Month { year: index.div_euclid(12), month: index.rem_euclid(12) as u32 + 1 }
    }

    /// The year a one-digit year code means, seen from `today`: the one
    /// ending in that digit from last year to eight years on. Last year,
    /// because a contract that expired lately is still spoken of, in a
    /// position held past its last day; and no further back, because a
    /// contract nine years out is listed on some roots and one nine years
    /// gone is not.
    fn year_of(digit: u32, today: NaiveDate) -> i32 {
        let from = today.year() - 1;
        (from..from + 10).find(|year| year.rem_euclid(10) == digit as i32).unwrap_or(from)
    }
}

const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A dated contract's symbol in the exchange's spelling: `ESH7`.
pub fn symbol(root: &str, month: Month) -> String {
    format!("{root}{}", month.short())
}

/// The root and month a contract's symbol names, as somebody types it:
/// `ESH7`, `ESH27`, `ESH2027`, `ES H7`, `es h27`. `None` for anything else,
/// a root on its own included. `today` says which decade a one-digit year
/// is in.
pub fn parse(text: &str, today: NaiveDate) -> Option<(String, Month)> {
    let text = text.trim().to_ascii_uppercase();
    let (root, rest) = match text.split_once(char::is_whitespace) {
        Some((root, rest)) => (root.trim(), rest.trim()),
        None => {
            // The root is everything before the last letter.
            let digits = text.trim_end_matches(|c: char| c.is_ascii_digit());
            let letter = digits.len().checked_sub(1)?;
            text.split_at_checked(letter)?
        }
    };
    if root.is_empty() || !root.chars().all(|c| c.is_ascii_alphanumeric()) || rest.chars().any(char::is_whitespace) {
        return None;
    }
    // A root ending in a digit, 6E, read without a space as a code with no
    // year: not a contract.
    let mut rest = rest.chars();
    let code = rest.next()?;
    let year = rest.as_str();
    if !CODES.contains(&code) || year.is_empty() || !year.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let year = match year.len() {
        1 => Month::year_of(year.parse().ok()?, today),
        2 => 2000 + year.parse::<i32>().ok()?,
        4 => year.parse().ok()?,
        _ => return None,
    };
    Some((root.to_string(), Month::of(code, year)?))
}

/// A date rule, as `contracts.tsv` writes one: a day in or around the
/// contract month, moved by some business or calendar days.
#[derive(Clone, Debug, PartialEq)]
pub struct When {
    anchor: Anchor,
    /// Business days back, then calendar days back; forward when negative.
    business: i32,
    calendar: i32,
    /// The anchor itself, when it is no business day, is the business day
    /// before it: what `0b` says.
    adjust: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Anchor {
    /// The third Friday of the contract month.
    ThirdFriday,
    /// The third Wednesday of the contract month.
    ThirdWednesday,
    /// The last business day of the contract month.
    MonthEnd,
    /// The last business day of the month before, or the one before that.
    PriorMonthEnd(u32),
    /// The 25th of the month before, or the business day before it when
    /// the 25th is not one.
    Prior25th,
    /// The 15th of the contract month.
    Fifteenth,
    /// The first day of the contract month, business day or not: what the
    /// energy and Treasury rules count back from.
    MonthStart,
    /// The first Friday of the contract month.
    FirstFriday,
    /// The `n`th business day of the contract month.
    BusinessDay(u32),
    /// The contract's expiry, by the root's own rule.
    Expiry,
}

impl When {
    /// `3rd-fri`, `3rd-fri-8d`, `prior-eom-1b`, `expiry-3b`: an anchor
    /// and any number of steps back, `Nb` business days strictly before
    /// it or `Nd` calendar days. `+Nb` is a step forward, and `0b` the
    /// anchor itself or the business day before it.
    pub fn parse(text: &str) -> Option<When> {
        let text = text.trim().to_ascii_lowercase();
        let (anchor, rest) = [
            ("3rd-fri", Anchor::ThirdFriday),
            ("3rd-wed", Anchor::ThirdWednesday),
            ("1st-fri", Anchor::FirstFriday),
            ("prior2-eom", Anchor::PriorMonthEnd(2)),
            ("prior-eom", Anchor::PriorMonthEnd(1)),
            ("prior-25th", Anchor::Prior25th),
            ("eom", Anchor::MonthEnd),
            ("15th", Anchor::Fifteenth),
            ("bom", Anchor::MonthStart),
            ("6th-bday", Anchor::BusinessDay(6)),
            ("10th-bday", Anchor::BusinessDay(10)),
            ("expiry", Anchor::Expiry),
        ]
        .into_iter()
        .find_map(|(word, anchor)| text.strip_prefix(word).map(|rest| (anchor, rest)))?;
        let mut when = When { anchor, business: 0, calendar: 0, adjust: false };
        for step in rest.split('-').filter(|step| !step.is_empty()) {
            let (sign, step) = match step.strip_prefix('+') {
                Some(forward) => (-1, forward),
                None => (1, step),
            };
            let (count, unit) = step.split_at_checked(step.len().checked_sub(1)?)?;
            let count: i32 = count.parse().ok()?;
            match unit {
                "b" if count == 0 => when.adjust = true,
                "b" => when.business += sign * count,
                "d" => when.calendar += sign * count,
                _ => return None,
            }
        }
        Some(when)
    }

    /// The date this names for a contract of `month`, with `expiry` what the
    /// root's own rule says that is.
    fn date(&self, month: Month, expiry: Option<NaiveDate>) -> Option<NaiveDate> {
        let anchored = match self.anchor {
            Anchor::ThirdFriday => nth_weekday(month, Weekday::Fri, 3),
            Anchor::ThirdWednesday => nth_weekday(month, Weekday::Wed, 3),
            Anchor::FirstFriday => nth_weekday(month, Weekday::Fri, 1),
            Anchor::MonthEnd => last_business_day(month),
            Anchor::PriorMonthEnd(back) => last_business_day(month.plus(-(back as i32))),
            Anchor::Prior25th => {
                let prior = month.plus(-1);
                business_days_back(NaiveDate::from_ymd_opt(prior.year, prior.month, 25)?, 0)
            }
            Anchor::Fifteenth => NaiveDate::from_ymd_opt(month.year, month.month, 15)?,
            Anchor::MonthStart => month.first_day(),
            Anchor::BusinessDay(n) => business_days_forward(month.first_day().pred_opt()?, n),
            Anchor::Expiry => expiry?,
        };
        let anchored = match self.adjust {
            true => business_days_back(anchored, 0),
            false => anchored,
        };
        let stepped = match self.business.signum() {
            1 => business_days_back(anchored, self.business as u32),
            -1 => business_days_forward(anchored, (-self.business) as u32),
            _ => anchored,
        };
        match self.calendar.signum() {
            1 => stepped.checked_sub_days(Days::new(self.calendar as u64)),
            -1 => stepped.checked_add_days(Days::new((-self.calendar) as u64)),
            _ => Some(stepped),
        }
    }
}

/// When a month's `n`th `weekday` falls.
fn nth_weekday(month: Month, weekday: Weekday, n: u32) -> NaiveDate {
    let first = month.first_day();
    let offset = (weekday.num_days_from_monday() + 7 - first.weekday().num_days_from_monday()) % 7;
    first + Days::new(u64::from(offset + 7 * (n - 1)))
}

/// A weekday the US exchanges are open: not one of the ten holidays CME
/// and ICE Futures U.S. close on every year.
pub fn is_business_day(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && !holidays(date.year()).contains(&date)
}

/// The exchange holidays of `year`, as they are observed: a fixed date on a
/// Saturday is kept the Friday before, on a Sunday the Monday after, except
/// New Year's, which is not moved into the year before.
fn holidays(year: i32) -> Vec<NaiveDate> {
    let fixed = |month: u32, day: u32, observed: bool| {
        let date = NaiveDate::from_ymd_opt(year, month, day).expect("a real date");
        match (date.weekday(), observed) {
            (Weekday::Sat, true) => date.pred_opt(),
            (Weekday::Sun, _) => date.succ_opt(),
            _ => Some(date),
        }
    };
    let nth_monday = |month: u32, n: u32| nth_weekday(Month { year, month }, Weekday::Mon, n);
    let last_monday = |month: u32| {
        let last = last_day_of(Month { year, month });
        last - Days::new(u64::from(last.weekday().num_days_from_monday()))
    };
    [
        fixed(1, 1, false),
        Some(nth_monday(1, 3)),
        Some(nth_monday(2, 3)),
        easter(year).checked_sub_days(Days::new(2)),
        Some(last_monday(5)),
        fixed(6, 19, true),
        fixed(7, 4, true),
        Some(nth_monday(9, 1)),
        Some(nth_weekday(Month { year, month: 11 }, Weekday::Thu, 4)),
        fixed(12, 25, true),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Easter Sunday of `year`, by the Gregorian computus.
fn easter(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32).expect("Easter falls in March or April")
}

fn last_day_of(month: Month) -> NaiveDate {
    month.plus(1).first_day().pred_opt().expect("a day before the first of a month")
}

/// The last business day of `month`.
fn last_business_day(month: Month) -> NaiveDate {
    business_days_back(last_day_of(month), 0)
}

/// The `count`th business day strictly before `date`; `date` itself, or
/// the business day before it, for a count of 0.
fn business_days_back(date: NaiveDate, count: u32) -> NaiveDate {
    let mut date = date;
    if count == 0 {
        while !is_business_day(date) {
            date = date.pred_opt().expect("a day before");
        }
        return date;
    }
    for _ in 0..count {
        date = date.pred_opt().expect("a day before");
        while !is_business_day(date) {
            date = date.pred_opt().expect("a day before");
        }
    }
    date
}

/// The `count`th business day strictly after `date`; `date` itself, or the
/// business day after it, for a count of 0.
fn business_days_forward(date: NaiveDate, count: u32) -> NaiveDate {
    let mut date = date;
    if count == 0 {
        while !is_business_day(date) {
            date = date.succ_opt().expect("a day after");
        }
        return date;
    }
    for _ in 0..count {
        date = date.succ_opt().expect("a day after");
        while !is_business_day(date) {
            date = date.succ_opt().expect("a day after");
        }
    }
    date
}

/// A root's month cycle, when a contract of it expires, and when the front
/// month moves on to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// The month codes it lists, in calendar order.
    pub months: Vec<char>,
    /// The last day a contract trades.
    pub expiry: When,
    /// The first day the contract after it is the front month: the day
    /// before first notice day where a contract is delivered, a few days
    /// before the last trade where it is settled in cash.
    pub roll: When,
}

impl Rule {
    /// The three columns of `contracts.tsv`, read.
    pub fn parse(months: &str, expiry: &str, roll: &str) -> Option<Rule> {
        let months: Vec<char> = match months.trim().to_ascii_uppercase().as_str() {
            "ALL" => CODES.to_vec(),
            codes => codes.chars().collect(),
        };
        if months.is_empty() || !months.iter().all(|code| CODES.contains(code)) {
            return None;
        }
        let expiry = When::parse(expiry)?;
        if expiry.anchor == Anchor::Expiry {
            return None;
        }
        Some(Rule { months, expiry, roll: When::parse(roll)? })
    }

    /// A root nobody wrote a rule for: every month, trading until the end
    /// of the month before it, and handed over [`ROLL_DAYS`] before that.
    pub fn fallback() -> Rule {
        Rule {
            months: CODES.to_vec(),
            expiry: When { anchor: Anchor::PriorMonthEnd(1), business: 0, calendar: 0, adjust: false },
            roll: When { anchor: Anchor::Expiry, business: 0, calendar: ROLL_DAYS as i32, adjust: false },
        }
    }

    /// The last day a contract of `month` trades.
    pub fn expiry(&self, month: Month) -> Option<NaiveDate> {
        self.expiry.date(month, None)
    }

    /// The first day the contract after `month`'s is the front month.
    pub fn roll(&self, month: Month) -> Option<NaiveDate> {
        self.roll.date(month, self.expiry(month))
    }

    /// The roll date for a contract whose expiry a feed gave: the rule's
    /// own when it is anchored to the month, and counted back from the
    /// feed's expiry when it is anchored to the expiry.
    fn roll_from(&self, month: Month, expiry: NaiveDate) -> Option<NaiveDate> {
        self.roll.date(month, Some(expiry))
    }

    /// The contract months listed from `today` on, `count` of them, by the
    /// cycle: the first is the one trading now or the one just expired, so
    /// a rule never lists fewer than it says.
    pub fn months_from(&self, today: NaiveDate, count: usize) -> Vec<Month> {
        let mut listed = Vec::with_capacity(count);
        let mut month = Month { year: today.year(), month: today.month() }.plus(-1);
        while listed.len() < count {
            if self.months.contains(&month.code()) && self.expiry(month).is_some_and(|expiry| expiry >= today) {
                listed.push(month);
            }
            month = month.plus(1);
        }
        listed
    }
}

/// One contract of a root, as search lists it and the chart names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    /// `ESH7`.
    pub symbol: String,
    pub month: Month,
    /// Its last day of trading.
    pub expires: NaiveDate,
    /// The first day the next contract is the front month instead of it.
    pub rolls: NaiveDate,
    /// The contract trading now: the one every chart of the root shows.
    pub front: bool,
}

/// The rule for `root`: the table's, or the fallback.
pub fn rule_of(root: &str) -> Rule {
    registry()
        .contract_months(root)
        .and_then(|(months, expiry, roll)| Rule::parse(months, expiry, roll))
        .unwrap_or_else(Rule::fallback)
}

/// What a feed lists for a root: each contract's symbol and expiry, and
/// which it calls the front month, when it says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Said {
    pub listed: Vec<(String, NaiveDate)>,
    pub front: Option<String>,
}

static SAID: Mutex<Option<HashMap<String, Said>>> = Mutex::new(None);

fn said(root: &str) -> Option<Said> {
    SAID.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(root).cloned()
}

/// A feed lists these contracts of `root`, each with the day it stops
/// trading. Kept over the rule's own listing, which is a computation, where
/// this is the exchange's word. The front month the feed named before is
/// kept.
pub fn supply(root: &str, listed: Vec<(String, NaiveDate)>) {
    let mut all = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let entry = all.get_or_insert_with(HashMap::new).entry(root.to_string()).or_default();
    entry.listed = listed;
}

/// A feed says `contract` is the front month of `root` now: what it said
/// is what every listing marks, until it says otherwise.
pub fn supply_front(root: &str, contract: &str) {
    let mut all = SAID.lock().unwrap_or_else(|e| e.into_inner());
    let entry = all.get_or_insert_with(HashMap::new).entry(root.to_string()).or_default();
    entry.front = Some(contract.to_string());
}

/// Forget what feeds said about `root`.
pub fn forget(root: &str) {
    if let Some(all) = SAID.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        all.remove(root);
    }
}

/// The contracts of `root` from `today` on, soonest first: what a feed
/// listed, or what the rule computes. The front month is the one a feed
/// named, or the rule's: the first whose roll date is still to come.
pub fn listing(root: &str, today: NaiveDate) -> Vec<Listing> {
    let rule = rule_of(root);
    let said = said(root).unwrap_or_default();
    let mut listed: Vec<Listing> = match said.listed.is_empty() {
        true => rule
            .months_from(today, LISTED)
            .into_iter()
            .filter_map(|month| {
                Some(Listing {
                    symbol: symbol(root, month),
                    month,
                    expires: rule.expiry(month)?,
                    rolls: rule.roll(month)?,
                    front: false,
                })
            })
            .collect(),
        false => said
            .listed
            .iter()
            .filter_map(|(symbol, expires)| {
                let (_, month) = parse(symbol, today)?;
                let rolls = rule.roll_from(month, *expires)?;
                Some(Listing { symbol: symbol.clone(), month, expires: *expires, rolls, front: false })
            })
            .filter(|listing| listing.expires >= today)
            .collect(),
    };
    listed.sort_by_key(|listing| (listing.expires, listing.month));
    listed.dedup_by(|a, b| a.symbol == b.symbol);
    let front = match said.front.as_deref().and_then(|front| listed.iter().position(|l| l.symbol == front)) {
        Some(at) => Some(at),
        None => listed.iter().position(|listing| listing.rolls > today),
    };
    if let Some(at) = front {
        listed[at].front = true;
    }
    listed
}

/// The front month of `root` on `today`, by [`listing`].
pub fn front(root: &str, today: NaiveDate) -> Option<Listing> {
    listing(root, today).into_iter().find(|listing| listing.front)
}

/// Which of the contracts a feed lists for `root` is its front month on
/// `today`, by the root's rule: what a feed asks when it cannot say itself.
/// Each is a contract's symbol and the day it stops trading. `None` when
/// every one listed has rolled off.
pub fn choose(root: &str, listed: &[(String, NaiveDate)], today: NaiveDate) -> Option<String> {
    let rule = rule_of(root);
    let mut candidates: Vec<(NaiveDate, NaiveDate, &str)> = listed
        .iter()
        .filter_map(|(symbol, expires)| {
            let (_, month) = parse(symbol, today)?;
            Some((*expires, rule.roll_from(month, *expires)?, symbol.as_str()))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().find(|(_, rolls, _)| *rolls > today).map(|(_, _, symbol)| symbol.to_string())
}

/// Today, as the exchanges count it: in Chicago.
pub fn today() -> NaiveDate {
    chrono::Utc::now().with_timezone(&chrono_tz::America::Chicago).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    const TODAY: &str = "2026-11-04";

    #[test]
    fn every_way_of_writing_a_contract_reads_the_same() {
        let today = day(TODAY);
        let march = Month::new(2027, 3).unwrap();
        for text in ["ESH7", "ESH27", "ESH2027", "ES H7", "ES H27", "es h7", "esh7", " ESH7 ", "ES  H2027"] {
            assert_eq!(parse(text, today), Some(("ES".to_string(), march)), "{text:?}");
        }
        assert_eq!(parse("6EH7", today), Some(("6E".to_string(), march)));
        assert_eq!(parse("MNQZ6", today), Some(("MNQ".to_string(), Month::new(2026, 12).unwrap())));
        assert_eq!(parse("CLF30", today), Some(("CL".to_string(), Month::new(2030, 1).unwrap())));
        for text in ["ES", "6E", "ESH", "ES H", "AAPL", "ESA7", "ES H7 X", "H7", "", "ES-H7", "ESH277"] {
            assert_eq!(parse(text, today), None, "{text:?}");
        }
        assert_eq!(symbol("ES", march), "ESH7");
        assert_eq!(march.label(), "Mar 2027");
        assert_eq!(march.short(), "H7");
    }

    /// One digit of a year is read from last year to eight years on: the
    /// contract that just expired is still what it was, and nine years out
    /// is nothing anybody charts.
    #[test]
    fn a_one_digit_year_is_the_coming_decade_less_one() {
        let today = day(TODAY);
        assert_eq!(parse("ESZ5", today).unwrap().1.year, 2025);
        assert_eq!(parse("ESZ6", today).unwrap().1.year, 2026);
        assert_eq!(parse("ESZ4", today).unwrap().1.year, 2034);
        assert_eq!(parse("ESZ5", day("2027-01-05")).unwrap().1.year, 2035);
        assert_eq!(parse("ESZ6", day("2027-01-05")).unwrap().1.year, 2026);
    }

    #[test]
    fn a_rule_is_read_from_the_table_and_a_bad_one_is_not() {
        let rule = Rule::parse("HMUZ", "3rd-fri", "expiry-8d").unwrap();
        assert_eq!(rule.months, vec!['H', 'M', 'U', 'Z']);
        assert_eq!(Rule::parse("all", "prior-eom", "expiry-3b").unwrap().months.len(), 12);
        assert!(Rule::parse("HMUZ", "expiry", "expiry-8d").is_none(), "an expiry anchored to itself");
        assert!(Rule::parse("HMUZ", "3rd-fri", "expiry-8x").is_none());
        assert!(Rule::parse("HMAZ", "3rd-fri", "expiry-8d").is_none(), "A is no month");
        assert!(Rule::parse("", "3rd-fri", "expiry-8d").is_none());
        assert!(Rule::parse("HMUZ", "someday", "expiry-8d").is_none());
        let when = When::parse("prior-eom-1b-2d").unwrap();
        assert_eq!((when.anchor, when.business, when.calendar), (Anchor::PriorMonthEnd(1), 1, 2));
        let when = When::parse("bom+2b").unwrap();
        assert_eq!((when.anchor, when.business), (Anchor::MonthStart, -2));
    }

    #[test]
    fn business_days_skip_the_weekend_and_the_holidays() {
        // 2026-11-06 is a Friday.
        assert_eq!(business_days_back(day("2026-11-08"), 0), day("2026-11-06"), "a Sunday is the Friday before");
        assert_eq!(business_days_back(day("2026-11-09"), 1), day("2026-11-06"), "Monday less one is Friday");
        assert_eq!(business_days_back(day("2026-11-06"), 3), day("2026-11-03"));
        assert_eq!(business_days_back(day("2026-11-27"), 1), day("2026-11-25"), "Thanksgiving is no business day");
        assert_eq!(business_days_back(day("2027-01-01"), 3), day("2026-12-29"), "counted from New Year's Day itself");
        assert_eq!(business_days_forward(day("2026-11-07"), 0), day("2026-11-09"));
        assert_eq!(business_days_forward(day("2026-11-06"), 1), day("2026-11-09"));
        assert_eq!(last_business_day(Month::new(2026, 11).unwrap()), day("2026-11-30"));
        assert_eq!(last_business_day(Month::new(2027, 1).unwrap()), day("2027-01-29"), "the 31st is a Sunday");
        assert_eq!(nth_weekday(Month::new(2026, 12).unwrap(), Weekday::Fri, 3), day("2026-12-18"));
        assert_eq!(nth_weekday(Month::new(2027, 3).unwrap(), Weekday::Fri, 3), day("2027-03-19"));
        assert_eq!(nth_weekday(Month::new(2027, 3).unwrap(), Weekday::Wed, 3), day("2027-03-17"));
        for closed in [
            "2026-01-01", "2026-01-19", "2026-02-16", "2026-04-03", "2026-05-25", "2026-06-19", "2026-07-03",
            "2026-09-07", "2026-11-26", "2026-12-25", "2027-01-01", "2027-03-26", "2027-06-18", "2027-07-05",
        ] {
            assert!(!is_business_day(day(closed)), "{closed} is a holiday");
        }
        assert!(is_business_day(day("2027-07-02")));
        assert_eq!(easter(2026), day("2026-04-05"));
        assert_eq!(easter(2027), day("2027-03-28"));
    }

    /// Each family's rule against the exchanges' own calendars for the
    /// coming contracts (CME Group's calendar pages and ICE Futures U.S.'s
    /// expiry tabs, read 2026-11-04): the last trade, and the first notice
    /// day for what is delivered, which the roll is the business day before.
    #[test]
    fn the_rules_give_the_exchanges_own_dates() {
        let today = day(TODAY);
        let expires = |contract: &str, date: &str| {
            let (root, month) = parse(contract, today).unwrap();
            assert_eq!(rule_of(&root).expiry(month), Some(day(date)), "{contract} expires");
        };
        let rolls = |contract: &str, date: &str| {
            let (root, month) = parse(contract, today).unwrap();
            assert_eq!(rule_of(&root).roll(month), Some(day(date)), "{contract} rolls");
        };
        // Equity index: the third Friday, or the business day before when
        // that is a holiday (Juneteenth 2027); rolled on the Thursday eight
        // days before, where the volume goes over.
        for root in ["ES", "MES", "NQ", "MNQ", "RTY", "M2K", "YM", "MYM"] {
            expires(&format!("{root}Z6"), "2026-12-18");
            expires(&format!("{root}H7"), "2027-03-19");
            expires(&format!("{root}M7"), "2027-06-17");
            rolls(&format!("{root}Z6"), "2026-12-10");
            rolls(&format!("{root}H7"), "2027-03-11");
            rolls(&format!("{root}M7"), "2027-06-10");
        }
        // Treasuries: ZN and ZB stop seven business days before the month's
        // last, ZF and ZT on it (Good Friday moves ZNH7); rolled the
        // business day before first position day, two business days before
        // the delivery month.
        expires("ZNZ6", "2026-12-21");
        expires("ZNH7", "2027-03-19");
        expires("ZNM7", "2027-06-21");
        expires("ZBH7", "2027-03-19");
        expires("ZTH7", "2027-03-31");
        rolls("ZNZ6", "2026-11-25");
        rolls("ZNH7", "2027-02-24");
        rolls("ZNM7", "2027-05-26");
        // Energy: CL three business days before the 25th of the month
        // before, or before the business day before it (Christmas 2026);
        // MCL a business day earlier; NG three business days before the
        // first; RB and HO the last business day of the month before; BZ
        // the last of the second month before.
        expires("CLZ6", "2026-11-20");
        expires("CLF7", "2026-12-21");
        expires("CLG7", "2027-01-20");
        expires("CLH7", "2027-02-22");
        expires("MCLZ6", "2026-11-19");
        expires("MCLF7", "2026-12-18");
        expires("MCLG7", "2027-01-19");
        expires("NGZ6", "2026-11-25");
        expires("NGF7", "2026-12-29");
        expires("NGG7", "2027-01-27");
        expires("RBF7", "2026-12-31");
        expires("HOF7", "2026-12-31");
        expires("BZF7", "2026-11-30");
        rolls("CLF7", "2026-12-16");
        // Grains: the business day before the 15th; first notice the last
        // business day of the month before, and the roll the day before that.
        expires("ZCZ6", "2026-12-14");
        expires("ZCH7", "2027-03-12");
        expires("ZCK7", "2027-05-14");
        expires("ZSF7", "2027-01-14");
        expires("ZLF7", "2027-01-14");
        expires("ZSH7", "2027-03-12");
        expires("ZWH7", "2027-03-12");
        rolls("ZCZ6", "2026-11-27");
        rolls("ZCH7", "2027-02-25");
        rolls("ZSF7", "2026-12-30");
        // Livestock: cattle to the month's last business day, with first
        // notice the business day after the first Friday; hogs to the tenth
        // business day.
        expires("LEZ6", "2026-12-31");
        expires("LEG7", "2027-02-26");
        expires("LEJ7", "2027-04-30");
        rolls("LEZ6", "2026-12-04");
        rolls("LEG7", "2027-02-05");
        rolls("LEJ7", "2027-04-02");
        expires("HEZ6", "2026-12-14");
        expires("HEG7", "2027-02-12");
        expires("HEJ7", "2027-04-14");
        rolls("HEZ6", "2026-12-09");
        // Metals: the third last business day; first notice the last
        // business day of the month before.
        expires("GCZ6", "2026-12-29");
        expires("GCG7", "2027-02-24");
        expires("GCJ7", "2027-04-28");
        expires("MGCG7", "2027-02-24");
        expires("SIH7", "2027-03-29");
        expires("HGH7", "2027-03-29");
        expires("PAH7", "2027-03-29");
        expires("PLF7", "2027-01-27");
        rolls("GCZ6", "2026-11-27");
        rolls("GCG7", "2027-01-28");
        rolls("GCJ7", "2027-03-30");
        rolls("SIH7", "2027-02-25");
        // Softs, on ICE Futures U.S.: coffee and cocoa stop the business day
        // before last notice day; cotton seventeen business days from the
        // month's end counting the last; sugar on the last business day of
        // the month before. First notice is counted back from the delivery
        // month: seven business days for coffee, ten from its sixth business
        // day for cocoa, five for cotton.
        expires("KCZ6", "2026-12-18");
        expires("KCH7", "2027-03-18");
        expires("KCK7", "2027-05-18");
        rolls("KCZ6", "2026-11-18");
        rolls("KCH7", "2027-02-17");
        rolls("KCK7", "2027-04-21");
        expires("SBH7", "2027-02-26");
        expires("SBK7", "2027-04-30");
        rolls("SBH7", "2027-02-25");
        expires("CCH7", "2027-03-15");
        expires("CCK7", "2027-05-13");
        rolls("CCH7", "2027-02-19");
        rolls("CCK7", "2027-04-23");
        expires("CTH7", "2027-03-08");
        expires("CTK7", "2027-05-06");
        rolls("CTH7", "2027-02-19");
        rolls("CTK7", "2027-04-23");
        // FX: two business days before the third Wednesday, one for the
        // Canadian dollar; rolled two business days before that.
        expires("6EZ6", "2026-12-14");
        expires("6EH7", "2027-03-15");
        expires("6EM7", "2027-06-14");
        expires("6JZ6", "2026-12-14");
        expires("6CZ6", "2026-12-15");
        expires("6CH7", "2027-03-16");
        rolls("6EZ6", "2026-12-10");
        rolls("6EH7", "2027-03-11");
    }

    /// The front month on a given day, family by family: the one just
    /// before and just after a roll.
    #[test]
    fn the_front_month_moves_on_the_roll_date() {
        for root in ["ES", "ZN", "CL", "GC"] {
            forget(root);
        }
        let front_on = |root: &str, date: &str| front(root, day(date)).map(|l| l.symbol).unwrap();
        assert_eq!(front_on("ES", "2026-12-09"), "ESZ6");
        assert_eq!(front_on("ES", "2026-12-10"), "ESH7");
        assert_eq!(front_on("ES", "2027-03-10"), "ESH7");
        assert_eq!(front_on("ES", "2027-03-11"), "ESM7");
        assert_eq!(front_on("ZN", "2026-11-24"), "ZNZ6");
        assert_eq!(front_on("ZN", "2026-11-25"), "ZNH7");
        assert_eq!(front_on("CL", "2026-12-15"), "CLF7");
        assert_eq!(front_on("CL", "2026-12-16"), "CLG7");
        assert_eq!(front_on("GC", "2026-11-26"), "GCZ6");
        assert_eq!(front_on("GC", "2026-11-27"), "GCG7");
        assert_eq!(front_on("GC", "2027-01-28"), "GCJ7");
        let listed = listing("ES", day("2026-11-04"));
        assert_eq!(listed.iter().map(|l| l.symbol.as_str()).take(4).collect::<Vec<_>>(), ["ESZ6", "ESH7", "ESM7", "ESU7"]);
        assert_eq!(listed.iter().filter(|l| l.front).count(), 1);
    }

    /// Every root in the table has a rule the table can read.
    #[test]
    fn every_listed_root_has_a_rule_that_reads() {
        for root in registry().contract_roots() {
            let (months, expiry, roll) = registry().contract_months(root).unwrap_or_else(|| panic!("{root} has no months"));
            let rule = Rule::parse(months, expiry, roll).unwrap_or_else(|| panic!("{root}: {months} {expiry} {roll}"));
            let month = rule.months_from(day(TODAY), 1)[0];
            let (expires, rolls) = (rule.expiry(month).unwrap(), rule.roll(month).unwrap());
            assert!(rolls <= expires, "{root}: rolls {rolls} after it expires {expires}");
            assert!(expires - rolls < chrono::Duration::days(45), "{root}: rolls {rolls}, expires {expires}");
        }
    }

    /// The fallback, for a root nobody wrote down: monthly, to the end of
    /// the month before, and over to the next eight days before that.
    #[test]
    fn an_unknown_root_gets_the_fallback_rule() {
        let rule = rule_of("ZZQ");
        assert_eq!(rule, Rule::fallback());
        let jan = Month::new(2027, 1).unwrap();
        assert_eq!(rule.expiry(jan), Some(day("2026-12-31")));
        assert_eq!(rule.roll(jan), Some(day("2026-12-23")));
        let listed = listing("ZZQ", day(TODAY));
        assert_eq!(listed.len(), LISTED);
        assert_eq!(listed[0].symbol, "ZZQZ6");
        assert!(listed[0].front, "{listed:?}");
        assert_eq!(listed[1].symbol, "ZZQF7");
    }

    /// A feed that cannot say which contract is the front month hands over
    /// what it lists, and the rule chooses by the root's own roll: the one
    /// nearest to expiring that has not rolled off yet.
    #[test]
    fn a_feed_that_cannot_say_is_answered_from_its_listing() {
        let listed = vec![
            ("ESH7".to_string(), day("2027-03-19")),
            ("ESZ6".to_string(), day("2026-12-18")),
            ("ESM7".to_string(), day("2027-06-18")),
        ];
        assert_eq!(choose("ES", &listed, day("2026-11-04")).as_deref(), Some("ESZ6"));
        assert_eq!(choose("ES", &listed, day("2026-12-09")).as_deref(), Some("ESZ6"), "the day before the roll");
        assert_eq!(choose("ES", &listed, day("2026-12-10")).as_deref(), Some("ESH7"), "the roll date");
        assert_eq!(choose("ES", &listed, day("2027-06-20")), None, "all rolled off");
        // A root with no rule: the nearest, rolled ROLL_DAYS before its expiry.
        let listed = vec![("ZZQF7".to_string(), day("2026-12-31")), ("ZZQG7".to_string(), day("2027-01-29"))];
        assert_eq!(choose("ZZQ", &listed, day("2026-12-22")).as_deref(), Some("ZZQF7"));
        assert_eq!(choose("ZZQ", &listed, day("2026-12-23")).as_deref(), Some("ZZQG7"));
    }

    /// What a feed lists is what is shown, with the rule saying only when
    /// each rolls; and the front month it names wins over the rule's.
    #[test]
    fn a_feed_listing_is_kept_over_the_rule_and_its_front_month_over_the_rules() {
        let today = day(TODAY);
        forget("NQ");
        let by_rule = listing("NQ", today);
        assert_eq!(by_rule.len(), LISTED);
        assert_eq!(by_rule[0].symbol, "NQZ6");
        assert!(by_rule[0].front);
        supply("NQ", vec![("NQH7".into(), day("2027-03-19")), ("NQZ6".into(), day("2026-12-18"))]);
        let said = listing("NQ", today);
        assert_eq!(said.iter().map(|l| l.symbol.as_str()).collect::<Vec<_>>(), vec!["NQZ6", "NQH7"]);
        assert_eq!(said[0].rolls, day("2026-12-10"));
        assert!(said[0].front && !said[1].front);
        supply_front("NQ", "NQH7");
        let named = listing("NQ", today);
        assert!(!named[0].front && named[1].front, "{named:?}");
        assert_eq!(front("NQ", today).unwrap().symbol, "NQH7");
        forget("NQ");
        assert!(front("NQ", today).unwrap().symbol == "NQZ6");
    }
}
