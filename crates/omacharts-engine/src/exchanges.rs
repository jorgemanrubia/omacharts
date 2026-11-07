//! Exchanges, and the shape of their trading day.
//!
//! Everything a chart does with time beyond drawing bars in order — keeping
//! regular hours, shading the overnight, putting a line where a day begins,
//! knowing whether another bar is coming — needs the same three facts about
//! where an instrument trades: the clock it keeps, where one trading day ends
//! and the next begins, and which parts of that day are the regular session.
//!
//! Those facts are data, not code. They live in three tables shipped in the
//! binary — `exchanges.tsv`, `trading_hours.tsv` and `contracts.tsv` — read
//! once, the way the symbol inventory is, so a row can be corrected without
//! touching anything that reads it.
//!
//! There is no holiday calendar. A public holiday reads as an ordinary
//! weekday: its sessions are shaded and its separator drawn over whatever few
//! bars it has, which is wrong in a way that is easy to see and costs nothing.

use std::sync::OnceLock;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::symbols::{Instrument, InstrumentKind};

const EXCHANGES: &str = include_str!("exchanges.tsv");
const HOURS: &str = include_str!("trading_hours.tsv");
const CONTRACTS: &str = include_str!("contracts.tsv");

/// Minutes in a day.
const DAY: u32 = 1_440;

/// A part of the trading day.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    /// The futures session that runs through the night before the open.
    Overnight,
    /// The pre-market.
    Pre,
    /// The session regular-hours charts keep: RTH.
    Regular,
    /// The post-market.
    Post,
    /// A market with no regular session to tell apart from the rest: FX and
    /// crypto are trading or they are not.
    Open,
}

impl Part {
    pub fn key(self) -> &'static str {
        match self {
            Part::Overnight => "overnight",
            Part::Pre => "pre",
            Part::Regular => "regular",
            Part::Post => "post",
            Part::Open => "open",
        }
    }

    pub fn from_key(key: &str) -> Option<Part> {
        [Part::Overnight, Part::Pre, Part::Regular, Part::Post, Part::Open]
            .into_iter()
            .find(|part| part.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            Part::Overnight => "Overnight",
            Part::Pre => "Pre-market",
            Part::Regular => "Regular hours",
            Part::Post => "Post-market",
            Part::Open => "Open",
        }
    }

    /// Extended hours: anything a regular-hours chart leaves out.
    pub fn is_extended(self) -> bool {
        matches!(self, Part::Overnight | Part::Pre | Part::Post)
    }
}

/// One stretch of the day, as minutes past the trading day's base midnight.
///
/// The base is the calendar day the trading day starts on — the evening
/// before, for Globex — so a segment running past midnight is simply one
/// whose end is past 1440, and comparing a moment against it is arithmetic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Segment {
    pub part: Part,
    pub start: u32,
    pub end: u32,
}

/// One shape of trading day.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hours {
    pub id: String,
    /// Indexed from Monday: which days a trading day is named for.
    days: [bool; 7],
    /// Where the day begins, in minutes past midnight.
    day_starts: u32,
    /// Whether it begins the evening before the date it is named for.
    evening_before: bool,
    pub segments: Vec<Segment>,
}

impl Hours {
    /// Is a trading day named for this date one this market keeps?
    pub fn trades_on(&self, day: Weekday) -> bool {
        self.days[day.num_days_from_monday() as usize]
    }

    /// Does this day have a regular session to keep apart from the rest?
    pub fn has_regular(&self) -> bool {
        self.segments.iter().any(|s| s.part == Part::Regular)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Exchange {
    /// What everything else calls it: `CME`, `XETRA`.
    pub code: String,
    pub name: String,
    /// The ISO 10383 market identifier, when it has one.
    pub mic: Option<String>,
    pub zone: Tz,
    hours: usize,
    suffixes: Vec<String>,
    aliases: Vec<String>,
}

impl Exchange {
    /// Does this name, as a listing or a person spells it, mean this exchange?
    fn answers_to(&self, name: &str) -> bool {
        self.code.eq_ignore_ascii_case(name)
            || self.name.eq_ignore_ascii_case(name)
            || self.mic.as_deref().is_some_and(|mic| mic.eq_ignore_ascii_case(name))
            || self.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(name))
    }
}

/// Every exchange, every shape of day, and which futures trade where.
#[derive(Debug)]
pub struct Registry {
    exchanges: Vec<Exchange>,
    hours: Vec<Hours>,
    contracts: Vec<Contract>,
}

/// A futures root: the exchange it trades on, the hours it keeps there, and
/// the size of one tick of its price and what a point of it is worth when
/// the table knows them.
#[derive(Debug)]
struct Contract {
    root: String,
    exchange: usize,
    hours: usize,
    tick: Option<f64>,
    point: Option<f64>,
    currency: Option<String>,
    quoted: Option<String>,
    /// The month codes it lists, `HMUZ`, when the table says.
    months: Option<String>,
    /// When a contract of it stops trading, and when the one after it
    /// becomes the front month, in the words [`crate::contracts`] reads.
    expiry: Option<String>,
    roll: Option<String>,
}

/// The tables, read once.
pub fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Registry::parse(EXCHANGES, HOURS, CONTRACTS).registry)
}

/// What reading the tables produced, and every row it had to leave out.
struct Parsed {
    registry: Registry,
    #[cfg_attr(not(test), allow(dead_code))]
    rejected: Vec<String>,
}

fn rows(text: &str) -> impl Iterator<Item = Vec<&str>> {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(|line| line.split('\t').collect())
}

fn dash(field: &str) -> Option<&str> {
    match field.trim() {
        "" | "-" => None,
        text => Some(text),
    }
}

fn list(field: &str) -> Vec<String> {
    dash(field)
        .map(|text| text.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default()
}

/// A tick size: a decimal, or a fraction such as `1/64`, above zero.
fn tick(text: &str) -> Option<f64> {
    let value = match text.split_once('/') {
        Some((top, bottom)) => top.trim().parse::<f64>().ok()? / bottom.trim().parse::<f64>().ok()?,
        None => text.parse::<f64>().ok()?,
    };
    (value.is_finite() && value > 0.0).then_some(value)
}

/// `HH:MM`, in minutes past midnight.
fn clock(text: &str) -> Option<u32> {
    let (hours, minutes) = text.split_once(':')?;
    let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

fn weekdays(text: &str) -> Option<[bool; 7]> {
    match text {
        "mon-fri" => Some([true, true, true, true, true, false, false]),
        "mon-sun" => Some([true; 7]),
        _ => None,
    }
}

fn hours_row(fields: &[&str]) -> Option<Hours> {
    let [id, days, day_starts, segments, ..] = fields else { return None };
    let day_starts = clock(day_starts)?;
    // A time after noon is the evening before: no trading day starts in the
    // afternoon of the date it is named for. A morning one would leave the
    // hours before it belonging to no day, so it is refused rather than read
    // wrongly.
    let evening_before = day_starts >= 12 * 60;
    if day_starts != 0 && !evening_before {
        return None;
    }
    let place = |minutes: u32| match evening_before && minutes < day_starts {
        true => minutes + DAY,
        false => minutes,
    };
    let mut parsed = Vec::new();
    for segment in segments.split_whitespace() {
        let (part, span) = segment.split_once('=')?;
        let (from, to) = span.split_once('-')?;
        let start = place(clock(from)?);
        let mut end = place(clock(to)?);
        if end <= start {
            end += DAY;
        }
        parsed.push(Segment { part: Part::from_key(part)?, start, end });
    }
    if parsed.is_empty() {
        return None;
    }
    Some(Hours { id: id.to_string(), days: weekdays(days)?, day_starts, evening_before, segments: parsed })
}

impl Registry {
    fn parse(exchanges: &str, hours: &str, contracts: &str) -> Parsed {
        let mut rejected = Vec::new();
        let mut parsed_hours: Vec<Hours> = Vec::new();
        for fields in rows(hours) {
            match hours_row(&fields) {
                Some(row) => parsed_hours.push(row),
                None => rejected.push(fields.join("\t")),
            }
        }
        let mut parsed_exchanges: Vec<Exchange> = Vec::new();
        for fields in rows(exchanges) {
            let row = (|| {
                let [code, name, mic, zone, hours, suffixes, aliases, ..] = fields.as_slice() else {
                    return None;
                };
                Some(Exchange {
                    code: code.to_string(),
                    name: name.to_string(),
                    mic: dash(mic).map(str::to_string),
                    zone: zone.parse().ok()?,
                    hours: parsed_hours.iter().position(|h| h.id == *hours)?,
                    suffixes: list(suffixes),
                    aliases: list(aliases),
                })
            })();
            match row {
                Some(row) => parsed_exchanges.push(row),
                None => rejected.push(fields.join("\t")),
            }
        }
        let mut parsed_contracts = Vec::new();
        for fields in rows(contracts) {
            let row = (|| {
                let [root, exchange, hours, rest @ ..] = fields.as_slice() else { return None };
                let exchange = parsed_exchanges.iter().position(|e| e.code == *exchange)?;
                let hours = parsed_hours.iter().position(|h| h.id == *hours)?;
                // A point is read as a tick is: a number above zero.
                let positive = |at: usize| match rest.get(at).and_then(|field| dash(field)) {
                    Some(text) => tick(text).map(Some),
                    None => Some(None),
                };
                let text = |at: usize| rest.get(at).and_then(|field| dash(field)).map(str::to_string);
                Some(Contract {
                    root: root.to_string(),
                    exchange,
                    hours,
                    tick: positive(0)?,
                    point: positive(1)?,
                    currency: text(2),
                    quoted: text(3),
                    months: text(4),
                    expiry: text(5),
                    roll: text(6),
                })
            })();
            match row {
                Some(row) => parsed_contracts.push(row),
                None => rejected.push(fields.join("\t")),
            }
        }
        Parsed {
            registry: Registry {
                exchanges: parsed_exchanges,
                hours: parsed_hours,
                contracts: parsed_contracts,
            },
            rejected,
        }
    }

    /// The exchange this name means: its code, its name, its MIC, or one of
    /// the other names listings use for it.
    pub fn exchange(&self, name: &str) -> Option<&Exchange> {
        self.exchanges.iter().find(|e| e.answers_to(name))
    }

    /// The code of the exchange `contracts.tsv` puts a futures root on, or
    /// `None` for a root it does not list. Unlike [`Registry::for_instrument`],
    /// which takes an unlisted root to be on Globex so that it has hours,
    /// this guesses nothing: a feed deciding whether it serves a root must
    /// not serve one on the strength of a default.
    pub fn contract_exchange(&self, root: &str) -> Option<&str> {
        let contract = self.contract(root)?;
        self.exchanges.get(contract.exchange).map(|exchange| exchange.code.as_str())
    }

    /// The size of one tick of a futures root's price, as `contracts.tsv`
    /// gives it: every price it trades at is a whole number of these. `None`
    /// for a root it does not list, or lists without one.
    pub fn contract_tick(&self, root: &str) -> Option<f64> {
        self.contract(root)?.tick
    }

    /// What one whole point of a futures root's price is worth on one
    /// contract, in the currency it trades in, as `contracts.tsv` gives it:
    /// 50 for ES. `None` for a root it does not list, or lists without one.
    pub fn contract_point(&self, root: &str) -> Option<f64> {
        self.contract(root)?.point
    }

    /// The currency a point of a futures root is worth in, as
    /// `contracts.tsv` gives it: "USD" for 6E as for ES.
    pub fn contract_currency(&self, root: &str) -> Option<&str> {
        self.contract(root)?.currency.as_deref()
    }

    /// What a futures root's price is a number of, as `contracts.tsv` says:
    /// "cents per bushel" for ZC.
    pub fn contract_quoted(&self, root: &str) -> Option<&str> {
        self.contract(root)?.quoted.as_deref()
    }

    /// A futures root's month cycle, and when a contract of it expires and
    /// when the next becomes the front month, as `contracts.tsv` writes
    /// them: `None` for a root it does not list, or lists without them.
    pub fn contract_months(&self, root: &str) -> Option<(&str, &str, &str)> {
        let contract = self.contract(root)?;
        Some((contract.months.as_deref()?, contract.expiry.as_deref()?, contract.roll.as_deref()?))
    }

    /// Every futures root `contracts.tsv` lists.
    pub fn contract_roots(&self) -> impl Iterator<Item = &str> {
        self.contracts.iter().map(|contract| contract.root.as_str())
    }

    fn contract(&self, root: &str) -> Option<&Contract> {
        self.contracts.iter().find(|contract| contract.root == root)
    }

    fn by_code(&self, code: &str) -> Option<usize> {
        self.exchanges.iter().position(|e| e.code == code)
    }

    fn schedule(&self, exchange: usize, hours: Option<usize>) -> Option<Schedule<'_>> {
        let exchange = self.exchanges.get(exchange)?;
        Some(Schedule { exchange, hours: self.hours.get(hours.unwrap_or(exchange.hours))? })
    }

    /// Where this instrument trades, and the day it keeps there.
    ///
    /// Worked out in the order the evidence is good in: a futures root by the
    /// contract table; a listing by its suffix; then by the exchange the
    /// listing named; then FX and crypto as the markets they are; and last a
    /// guess from the currency for anything left — an index never carries a
    /// suffix, and one quoted in dollars or untagged is on New York's hours.
    /// `None` when none of that says, because hours this does not know are
    /// hours it must not rule on.
    pub fn for_instrument(&self, instrument: &Instrument) -> Option<Schedule<'_>> {
        match instrument.kind {
            InstrumentKind::FutureRoot => {
                return match self.contract(&instrument.symbol) {
                    Some(contract) => self.schedule(contract.exchange, Some(contract.hours)),
                    None => self.schedule(self.by_code("CME")?, None),
                };
            }
            InstrumentKind::Fx => return self.schedule(self.by_code("FX")?, None),
            InstrumentKind::Crypto => return self.schedule(self.by_code("CRYPTO")?, None),
            _ => {}
        }
        if let Some(suffix) = instrument.suffix.as_deref() {
            let found = self
                .exchanges
                .iter()
                .position(|e| e.suffixes.iter().any(|s| s.eq_ignore_ascii_case(suffix)));
            return found.and_then(|at| self.schedule(at, None));
        }
        if let Some(named) = instrument.exchange.as_deref()
            && let Some(at) = self.exchanges.iter().position(|e| e.answers_to(named))
        {
            return self.schedule(at, None);
        }
        match instrument.currency.as_deref() {
            // Unknown currency is treated as dollars: it is what a ticker
            // typed into the search field with nothing behind it turns out
            // to be.
            None | Some("USD") => self.schedule(self.by_code("NYSE")?, None),
            Some("TWD") => self.schedule(self.by_code("TWSE")?, None),
            _ => None,
        }
    }
}

/// An exchange and the day it keeps for one instrument.
#[derive(Clone, Copy, Debug)]
pub struct Schedule<'a> {
    pub exchange: &'a Exchange,
    pub hours: &'a Hours,
}

/// One trading day, laid out in real instants.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TradingDay {
    /// The date it is named for.
    pub date: NaiveDate,
    /// Where it begins and the next one does, as unix seconds.
    pub start: i64,
    pub end: i64,
    /// Its segments, in order, as unix seconds.
    pub segments: Vec<(Part, i64, i64)>,
}

/// The instant a wall clock in `zone` reads `minutes` past midnight on `date`.
///
/// Minutes may run past a day. A wall-clock time the spring change skips is
/// taken an hour on, which is when the clock next reads anything at all; one
/// the autumn change repeats is the first of the two.
pub fn instant(zone: Tz, date: NaiveDate, minutes: u32) -> i64 {
    let at = date.and_hms_opt(0, 0, 0).unwrap_or_default() + Duration::minutes(minutes as i64);
    resolve(zone, at)
}

fn resolve(zone: Tz, at: NaiveDateTime) -> i64 {
    zone.from_local_datetime(&at)
        .earliest()
        .or_else(|| zone.from_local_datetime(&(at + Duration::hours(1))).earliest())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| at.and_utc().timestamp())
}

/// What a wall clock in `zone` reads at `ts`: its date and minutes past midnight.
pub fn wall(zone: Tz, ts: i64) -> Option<(NaiveDate, u32)> {
    let local = zone.timestamp_opt(ts, 0).single()?;
    Some((local.date_naive(), local.hour() * 60 + local.minute()))
}

impl Schedule<'_> {
    pub fn zone(&self) -> Tz {
        self.exchange.zone
    }

    /// Does this instrument have a regular session to keep apart from the rest?
    pub fn has_regular(&self) -> bool {
        self.hours.has_regular()
    }

    /// The calendar day a trading day named for `date` starts on.
    fn base(&self, date: NaiveDate) -> NaiveDate {
        match self.hours.evening_before {
            true => date.pred_opt().unwrap_or(date),
            false => date,
        }
    }

    /// The trading day `ts` falls in, named by its date, and how far into
    /// its base day that moment is, in minutes.
    ///
    /// One timezone conversion and arithmetic, because this runs once per bar
    /// when a chart is filtered to regular hours.
    fn place(&self, ts: i64) -> Option<(NaiveDate, u32)> {
        let (date, minutes) = wall(self.zone(), ts)?;
        let hours = self.hours;
        match hours.evening_before && minutes >= hours.day_starts {
            true => Some((date.succ_opt()?, minutes)),
            false if hours.evening_before => Some((date, minutes + DAY)),
            false => Some((date, minutes)),
        }
    }

    /// The trading day `ts` belongs to, whether or not anything trades in it.
    pub fn trading_date(&self, ts: i64) -> Option<NaiveDate> {
        self.place(ts).map(|(date, _)| date)
    }

    /// Which part of a trading day `ts` is in, if any: `None` between
    /// sessions, at the weekend, and in the break.
    pub fn part_at(&self, ts: i64) -> Option<Part> {
        let (date, minutes) = self.place(ts)?;
        if !self.hours.trades_on(date.weekday()) {
            return None;
        }
        self.hours
            .segments
            .iter()
            .find(|s| (s.start..s.end).contains(&minutes))
            .map(|s| s.part)
    }

    /// Is `ts` inside the regular session?
    pub fn is_regular(&self, ts: i64) -> bool {
        self.part_at(ts) == Some(Part::Regular)
    }

    /// Is any part of a trading day under way at `ts`?
    pub fn is_open(&self, ts: i64) -> bool {
        self.part_at(ts).is_some()
    }

    /// The trading day named for `date`, laid out, when it is one this market
    /// keeps.
    pub fn day(&self, date: NaiveDate) -> Option<TradingDay> {
        if !self.hours.trades_on(date.weekday()) {
            return None;
        }
        let zone = self.zone();
        let base = self.base(date);
        let start = instant(zone, base, self.hours.day_starts);
        let end = instant(zone, base, self.hours.day_starts + DAY);
        let segments = self
            .hours
            .segments
            .iter()
            .map(|s| (s.part, instant(zone, base, s.start), instant(zone, base, s.end)))
            .collect();
        Some(TradingDay { date, start, end, segments })
    }

    /// Where the trading day named for `date` begins, as a unix second,
    /// whether or not this market keeps it.
    pub fn day_start(&self, date: NaiveDate) -> i64 {
        instant(self.zone(), self.base(date), self.hours.day_starts)
    }

    /// Where the bars of the trading day named for `date` start, as a unix
    /// second, whether or not this market keeps it: the regular open when
    /// `regular` and there is one, and otherwise the first part of the day
    /// that trades — 04:00 for a US listing, 17:00 for Globex.
    ///
    /// A pre-market of under an hour is an opening auction, Taipei's, Hong
    /// Kong's or Mumbai's, which matches orders and prints no bars of its
    /// own, so the day's bars start at the open after it.
    pub fn bars_start(&self, date: NaiveDate, regular: bool) -> i64 {
        let segments = &self.hours.segments;
        let open = segments.iter().filter(|s| s.part == Part::Regular).map(|s| s.start).min();
        let trading = segments
            .iter()
            .filter(|s| !(s.part == Part::Pre && s.end - s.start < 60))
            .map(|s| s.start)
            .min();
        let minutes = match regular {
            true => open.or(trading),
            false => trading,
        };
        instant(self.zone(), self.base(date), minutes.unwrap_or(self.hours.day_starts))
    }

    /// Every trading day that overlaps `from..=to`, oldest first.
    pub fn days(&self, from: i64, to: i64) -> Vec<TradingDay> {
        let (Some(first), Some(last)) = (self.trading_date(from), self.trading_date(to)) else {
            return Vec::new();
        };
        first
            .iter_days()
            .take_while(|date| *date <= last)
            .filter_map(|date| self.day(date))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use chrono_tz::America::{Chicago, New_York};
    use chrono_tz::Asia::Tokyo;

    use super::*;

    fn instrument(kind: InstrumentKind, symbol: &str, suffix: Option<&str>) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: symbol.into(),
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

    fn at(zone: Tz, (y, m, d): (i32, u32, u32), (h, min): (u32, u32)) -> i64 {
        zone.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap().timestamp()
    }

    /// The tables are written by hand, so every row has to be one this reads.
    /// A typo in one would otherwise quietly leave an exchange on no hours.
    #[test]
    fn every_row_in_the_tables_is_read() {
        let parsed = Registry::parse(EXCHANGES, HOURS, CONTRACTS);
        assert!(parsed.rejected.is_empty(), "rows nobody could read: {:#?}", parsed.rejected);
        assert!(parsed.registry.exchanges.len() >= 20);
        assert!(parsed.registry.contracts.len() >= 40);
    }

    #[test]
    fn a_day_that_starts_in_the_morning_is_refused() {
        let parsed = Registry::parse("", "odd\tmon-fri\t06:00\tregular=09:00-17:00\n", "");
        assert_eq!(parsed.rejected.len(), 1);
    }

    #[test]
    fn codes_are_unique() {
        let codes: Vec<&str> = registry().exchanges.iter().map(|e| e.code.as_str()).collect();
        let mut sorted = codes.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
    }

    /// Every futures root the inventory ships is on an exchange of its own
    /// rather than falling through to the default.
    #[test]
    fn every_shipped_futures_root_has_its_exchange() {
        for root in crate::symbols::seed().iter().filter(|i| i.kind == InstrumentKind::FutureRoot) {
            assert!(
                registry().contract(&root.symbol).is_some(),
                "{} has no row in contracts.tsv",
                root.symbol
            );
        }
    }

    /// A tick is the contract's, read from the table: decimals and
    /// fractions both, and every root listed has one.
    #[test]
    fn every_root_has_its_tick() {
        let r = registry();
        for (root, tick) in [
            ("ES", 0.25),
            ("MES", 0.25),
            ("NQ", 0.25),
            ("MNQ", 0.25),
            ("YM", 1.0),
            ("RTY", 0.1),
            ("CL", 0.01),
            ("NG", 0.001),
            ("GC", 0.1),
            ("SI", 0.005),
            ("ZN", 1.0 / 64.0),
            ("ZB", 1.0 / 32.0),
            ("ZT", 1.0 / 256.0),
            ("6E", 0.00005),
            ("6J", 0.000_000_5),
            ("ZC", 0.25),
        ] {
            assert_eq!(r.contract_tick(root), Some(tick), "{root}");
        }
        for contract in &r.contracts {
            assert!(contract.tick.is_some(), "{} has no tick in contracts.tsv", contract.root);
        }
        assert_eq!(r.contract_tick("NOPE"), None);
        assert_eq!(instrument(InstrumentKind::FutureRoot, "ES", None).tick_size(), Some(0.25));
        assert_eq!(instrument(InstrumentKind::Equity, "ES", None).tick_size(), None, "a stock has no one tick");
    }

    /// What a point is worth, so that a tick of it comes to what the
    /// exchange says a tick is worth, and every root listed has one.
    #[test]
    fn every_root_has_its_point_value() {
        let r = registry();
        for (root, tick_value) in [
            ("ES", 12.5),
            ("MES", 1.25),
            ("NQ", 5.0),
            ("YM", 5.0),
            ("CL", 10.0),
            ("NG", 10.0),
            ("GC", 10.0),
            ("ZN", 15.625),
            ("6E", 6.25),
            ("ZC", 12.5),
            ("KC", 18.75),
        ] {
            let value = r.contract_point(root).unwrap() * r.contract_tick(root).unwrap();
            assert!((value - tick_value).abs() < 1e-9, "{root}: a tick is {value}");
        }
        for contract in &r.contracts {
            assert!(contract.point.is_some(), "{} has no point in contracts.tsv", contract.root);
        }
        assert_eq!(r.contract_point("NOPE"), None);
    }

    #[test]
    fn a_tick_is_a_decimal_or_a_fraction_above_zero() {
        assert_eq!(tick("0.25"), Some(0.25));
        assert_eq!(tick("1/64"), Some(0.015625));
        for refused in ["0", "-1", "1/0", "x", "nan"] {
            assert_eq!(tick(refused), None, "{refused}");
        }
        let parsed = Registry::parse(EXCHANGES, HOURS, "ES\tCME\tcme_equity_index\tquarter\nNQ\tCME\tcme_equity_index\t-\n");
        assert_eq!(parsed.rejected.len(), 1, "a tick that does not read is a row refused");
        assert_eq!(parsed.registry.contract_tick("NQ"), None, "and one left as - is not known");
    }

    #[test]
    fn an_exchange_is_found_by_any_name_it_goes_by() {
        let r = registry();
        assert_eq!(r.exchange("NYSE Arca").map(|e| e.code.as_str()), Some("ARCA"));
        assert_eq!(r.exchange("xnys").map(|e| e.code.as_str()), Some("NYSE"));
        assert_eq!(r.exchange("TPEx").map(|e| e.code.as_str()), Some("TPEX"));
        assert!(r.exchange("Nowhere").is_none());
    }

    #[test]
    fn instruments_land_on_their_exchanges() {
        let r = registry();
        let code = |i: &Instrument| r.for_instrument(i).map(|s| s.exchange.code.clone());
        assert_eq!(code(&instrument(InstrumentKind::FutureRoot, "ES", None)).as_deref(), Some("CME"));
        assert_eq!(code(&instrument(InstrumentKind::FutureRoot, "CL", None)).as_deref(), Some("NYMEX"));
        assert_eq!(
            code(&instrument(InstrumentKind::FutureRoot, "XYZ", None)).as_deref(),
            Some("CME"),
            "an unknown root is on Globex"
        );
        assert_eq!(code(&instrument(InstrumentKind::Equity, "SAP", Some("DE"))).as_deref(), Some("XETRA"));
        assert_eq!(code(&instrument(InstrumentKind::Equity, "2330", Some("TW"))).as_deref(), Some("TWSE"));
        assert_eq!(code(&instrument(InstrumentKind::Equity, "AAPL", None)).as_deref(), Some("NYSE"));
        let listed = Instrument {
            exchange: Some("NASDAQ".into()),
            ..instrument(InstrumentKind::Equity, "AAPL", None)
        };
        assert_eq!(code(&listed).as_deref(), Some("NASDAQ"));
        assert_eq!(code(&instrument(InstrumentKind::Fx, "EURUSD", None)).as_deref(), Some("FX"));
        assert_eq!(code(&instrument(InstrumentKind::Crypto, "BTC", None)).as_deref(), Some("CRYPTO"));
        // A suffix nobody has a row for is not guessed at.
        assert_eq!(code(&instrument(InstrumentKind::Equity, "X", Some("ZZ"))), None);
        let nikkei = Instrument {
            currency: Some("JPY".into()),
            ..instrument(InstrumentKind::Index, "N225", None)
        };
        assert_eq!(code(&nikkei), None, "an index abroad needs its exchange named");
        let nikkei = Instrument { exchange: Some("TSE".into()), ..nikkei };
        assert_eq!(code(&nikkei).as_deref(), Some("TSE"));
    }

    fn es() -> Schedule<'static> {
        registry().for_instrument(&instrument(InstrumentKind::FutureRoot, "ES", None)).unwrap()
    }

    /// Globex's Monday begins on Sunday evening, which is the whole reason a
    /// trading day is not a calendar day.
    #[test]
    fn a_globex_day_starts_the_evening_before() {
        let monday = NaiveDate::from_ymd_opt(2024, 3, 18).unwrap();
        let day = es().day(monday).unwrap();
        assert_eq!(day.start, at(Chicago, (2024, 3, 17), (17, 0)), "Sunday at five in Chicago");
        assert_eq!(day.end, at(Chicago, (2024, 3, 18), (17, 0)));
        assert_eq!(
            day.segments,
            vec![
                (Part::Overnight, at(Chicago, (2024, 3, 17), (17, 0)), at(Chicago, (2024, 3, 18), (8, 30))),
                (Part::Regular, at(Chicago, (2024, 3, 18), (8, 30)), at(Chicago, (2024, 3, 18), (15, 0))),
                (Part::Post, at(Chicago, (2024, 3, 18), (15, 0)), at(Chicago, (2024, 3, 18), (16, 0))),
            ]
        );
        assert_eq!(es().trading_date(at(Chicago, (2024, 3, 17), (20, 0))), Some(monday));
        assert_eq!(es().part_at(at(Chicago, (2024, 3, 17), (20, 0))), Some(Part::Overnight));
    }

    #[test]
    fn the_break_and_the_weekend_are_no_part_of_any_day() {
        assert_eq!(es().part_at(at(Chicago, (2024, 3, 18), (16, 30))), None, "the daily break");
        assert_eq!(es().part_at(at(Chicago, (2024, 3, 16), (12, 0))), None, "Saturday");
        assert_eq!(es().part_at(at(Chicago, (2024, 3, 17), (16, 0))), None, "Sunday before the open");
        assert!(es().day(NaiveDate::from_ymd_opt(2024, 3, 16).unwrap()).is_none());
    }

    /// The equity index RTH is the cash session, which is the same instant
    /// whether it is written in Chicago or in New York.
    #[test]
    fn the_equity_index_regular_session_is_the_cash_session() {
        assert!(!es().is_regular(at(New_York, (2024, 3, 13), (9, 29))));
        assert!(es().is_regular(at(New_York, (2024, 3, 13), (9, 30))));
        assert!(es().is_regular(at(New_York, (2024, 3, 13), (15, 59))));
        assert!(!es().is_regular(at(New_York, (2024, 3, 13), (16, 0))));
    }

    #[test]
    fn days_cover_a_range_and_skip_the_weekend() {
        let from = at(Chicago, (2024, 3, 14), (12, 0));
        let to = at(Chicago, (2024, 3, 19), (12, 0));
        let dates: Vec<String> = es().days(from, to).iter().map(|d| d.date.to_string()).collect();
        assert_eq!(dates, ["2024-03-14", "2024-03-15", "2024-03-18", "2024-03-19"]);
    }

    /// Across a clock change the sessions keep their wall-clock times, so the
    /// open moves an hour against UTC rather than staying put.
    #[test]
    fn the_open_keeps_its_wall_clock_time_across_the_clock_change() {
        // US clocks went forward on Sunday 2024-03-10.
        let friday = es().day(NaiveDate::from_ymd_opt(2024, 3, 8).unwrap()).unwrap();
        let monday = es().day(NaiveDate::from_ymd_opt(2024, 3, 11).unwrap()).unwrap();
        assert_eq!(monday.segments[1].1, at(Chicago, (2024, 3, 11), (8, 30)));
        assert_eq!(monday.segments[1].1 - friday.segments[1].1, 3 * 86_400 - 3600);
    }

    #[test]
    fn tokyo_breaks_for_lunch() {
        let tokyo = registry().for_instrument(&instrument(InstrumentKind::Equity, "7203", Some("T"))).unwrap();
        assert!(tokyo.is_regular(at(Tokyo, (2024, 3, 13), (11, 0))));
        assert_eq!(tokyo.part_at(at(Tokyo, (2024, 3, 13), (12, 0))), None);
        assert!(tokyo.is_regular(at(Tokyo, (2024, 3, 13), (13, 0))));
    }

    #[test]
    fn fx_runs_from_sunday_evening_to_friday_afternoon() {
        let fx = registry().for_instrument(&instrument(InstrumentKind::Fx, "EURUSD", None)).unwrap();
        assert!(!fx.has_regular());
        assert_eq!(fx.part_at(at(New_York, (2024, 3, 17), (18, 0))), Some(Part::Open), "Sunday evening");
        assert_eq!(fx.part_at(at(New_York, (2024, 3, 13), (3, 0))), Some(Part::Open));
        assert_eq!(fx.part_at(at(New_York, (2024, 3, 16), (12, 0))), None, "Saturday");
        assert_eq!(
            fx.trading_date(at(New_York, (2024, 3, 13), (17, 0))),
            NaiveDate::from_ymd_opt(2024, 3, 14),
            "five o'clock in New York is tomorrow"
        );
    }

    #[test]
    fn crypto_never_stops_and_its_day_is_utc() {
        let btc = registry().for_instrument(&instrument(InstrumentKind::Crypto, "BTC", None)).unwrap();
        let saturday = chrono::Utc.with_ymd_and_hms(2024, 3, 16, 0, 0, 0).unwrap().timestamp();
        assert_eq!(btc.part_at(saturday), Some(Part::Open));
        let day = btc.day(NaiveDate::from_ymd_opt(2024, 3, 16).unwrap()).unwrap();
        assert_eq!(day.start, saturday);
        assert_eq!(day.end - day.start, 86_400);
    }
}
