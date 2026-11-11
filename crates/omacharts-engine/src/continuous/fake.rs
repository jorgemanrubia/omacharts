//! A feed of ES's months for the tests of [`super`] and the app's: daily bars
//! on weekdays and hourly bars around the clock of five months, each across
//! the whole range as a folder of contract files has them, a later month ten
//! points above the one before it, so every roll is a gap of ten.

use std::sync::Mutex;

use chrono::{Datelike, NaiveDate};

use super::{midnight, Adjustment, Continuity};
use crate::bars::{Bar, Timeframe, Unit};
use crate::contracts::{self, Month};
use crate::ladders::{to_tick, Minute, Side};
use crate::provider::{Capability, LadderSink, Ladders, Provider, ProviderError, Sink, Stream, Subscription, Update};
use crate::symbols::{Instrument, InstrumentKind};

/// The months of ES the fake has, oldest first; each later month trades ten
/// points above the one before it, so every roll is a gap of ten.
pub const MONTHS: [(char, i32); 5] = [('Z', 2025), ('H', 2026), ('M', 2026), ('U', 2026), ('Z', 2026)];
pub const PREMIUM: f64 = 10.0;

pub fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

/// From when every month's history starts, as a back month, and to when.
pub fn first() -> i64 {
    midnight(day("2025-06-02"))
}
pub fn last() -> i64 {
    midnight(day("2026-11-06"))
}

/// A month's price at `ts`: the market, a point a day, and the month's
/// premium over the oldest.
pub fn price(index: usize, ts: i64) -> f64 {
    5_000.0 + ((ts - first()) / 3_600) as f64 / 24.0 + PREMIUM * index as f64
}

pub fn month(index: usize) -> Month {
    let (code, year) = MONTHS[index];
    Month::of(code, year).unwrap()
}

pub fn index_of(month: Month) -> Option<usize> {
    MONTHS.iter().position(|(code, year)| Month::of(*code, *year) == Some(month))
}

/// The feed: the months it holds, what it declares, and what it was asked.
pub struct Months {
    /// `months` unless a test needs what is known of its rolls to itself.
    pub id: &'static str,
    pub held: Vec<usize>,
    /// Months with no history before they became front, by index: their
    /// roll into them has nothing to measure.
    pub late: Vec<usize>,
    pub native: &'static [Adjustment],
    pub per_contract: bool,
    pub ladders: bool,
    pub asked: Mutex<Vec<(String, Timeframe, Option<i64>)>>,
    /// The contract it charts the root as: `ESZ6` unless a test rolls it.
    pub front: Mutex<String>,
    /// Months it holds and cannot read for now, by index.
    pub unreadable: Mutex<Vec<usize>>,
    /// What its stream and its live ladders were asked to follow.
    pub followed: Mutex<Vec<String>>,
    /// Every span asked for by time, with how many bars it held: what was
    /// read.
    pub between: Mutex<Vec<(String, Timeframe, i64, i64, usize)>>,
}

impl Months {
    pub fn all() -> Months {
        Months::without(&[])
    }

    pub fn without(missing: &[usize]) -> Months {
        Months {
            id: "months",
            held: (0..MONTHS.len()).filter(|index| !missing.contains(index)).collect(),
            late: Vec::new(),
            native: &[],
            per_contract: true,
            ladders: true,
            asked: Mutex::new(Vec::new()),
            front: Mutex::new("ESZ6".into()),
            unreadable: Mutex::new(Vec::new()),
            followed: Mutex::new(Vec::new()),
            between: Mutex::new(Vec::new()),
        }
    }

    pub fn held(&self, symbol: &str) -> Option<usize> {
        let (root, month) = contracts::parse(symbol, day("2026-11-10"))?;
        (root == "ES").then_some(())?;
        index_of(month).filter(|index| self.held.contains(index))
    }

    /// Forget what is known only for as long as the app runs, as a restart
    /// does: which series were drawn short, and the gaps.
    pub fn restart() {
        *super::DEGRADED.lock().unwrap() = None;
        *super::GAPS.lock().unwrap() = None;
    }

    /// Chart the root as `contract` from now on: the rule rolled.
    pub fn roll_to(&self, contract: &str) {
        *self.front.lock().unwrap() = contract.to_string();
    }

    pub fn asked_for(&self, symbol: &str) -> bool {
        self.asked.lock().unwrap().iter().any(|(asked, _, _)| asked == symbol)
    }
}

pub fn series(index: usize, timeframe: Timeframe, since: Option<i64>) -> Vec<Bar> {
    let step = match timeframe.unit {
        Unit::Day => 86_400,
        _ => timeframe.seconds(),
    };
    (first()..=last())
        .step_by(step as usize)
        .filter(|ts| timeframe.unit != Unit::Day || chrono::DateTime::from_timestamp(*ts, 0).unwrap().weekday().number_from_monday() <= 5)
        .filter(|ts| since.is_none_or(|since| *ts >= since))
        .map(|ts| {
            let close = price(index, ts);
            Bar { ts, open: close, high: close + 1.0, low: close - 1.0, close, volume: 100.0 }
        })
        .collect()
}

impl Provider for Months {
    fn id(&self) -> &'static str {
        self.id
    }
    fn label(&self) -> &'static str {
        "Months"
    }
    fn delay_minutes(&self, _: InstrumentKind) -> u32 {
        0
    }
    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        (instrument.kind == InstrumentKind::FutureRoot).then(|| instrument.symbol.clone())
    }
    fn charted_contract(&self, instrument: &Instrument) -> Option<String> {
        match instrument.is_contract() {
            true => Some(instrument.symbol.clone()),
            false => Some(self.front.lock().unwrap().clone()),
        }
    }
    fn capabilities(&self) -> &'static [Capability] {
        const CAPABILITIES: &[Capability] = &[
            Capability { timeframe: Timeframe::hours(1), history_days: None, max_request_days: None },
            Capability { timeframe: Timeframe::days(1), history_days: None, max_request_days: None },
        ];
        CAPABILITIES
    }
    fn continuity(&self) -> Continuity {
        Continuity { native: self.native, per_contract: self.per_contract }
    }
    fn adjusted(&self, symbol: &str, adjustment: Adjustment) -> String {
        format!("{symbol}@{}", adjustment.key())
    }
    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        self.asked.lock().unwrap().push((symbol.to_string(), timeframe, since));
        let index = self.held(symbol).ok_or(ProviderError::NotFound)?;
        if self.unreadable.lock().unwrap().contains(&index) {
            return Err(ProviderError::Network("busy".into()));
        }
        let mut bars = series(index, timeframe, since);
        if self.late.contains(&index) {
            let rule = contracts::rule_of("ES");
            let starts = rule.roll(super::previous(&rule, month(index))).unwrap();
            bars.retain(|bar| bar.ts >= midnight(starts));
        }
        Ok(bars)
    }
    fn bars_between(&self, symbol: &str, timeframe: Timeframe, since: i64, until: i64) -> Result<Vec<Bar>, ProviderError> {
        let mut bars = self.bars(symbol, timeframe, Some(since))?;
        bars.retain(|bar| bar.ts < until);
        self.between.lock().unwrap().push((symbol.to_string(), timeframe, since, until, bars.len()));
        Ok(bars)
    }
    /// Every month's history starts at [`first`], as a back month.
    fn earliest(&self, symbol: &str) -> Option<i64> {
        self.held(symbol).map(|_| first())
    }
    fn stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }
    fn ladders(&self) -> Option<&dyn Ladders> {
        self.ladders.then_some(self as &dyn Ladders)
    }
}

/// A snapshot of the month asked for, then one bar of it, at once.
impl Stream for Months {
    fn subscribe(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>, sink: Sink) -> Result<Box<dyn Subscription>, ProviderError> {
        self.followed.lock().unwrap().push(symbol.to_string());
        let bars = self.bars(symbol, timeframe, since)?;
        let latest = *bars.last().unwrap();
        sink(Update::Snapshot(bars));
        sink(Update::Bar(Bar { close: latest.close + 0.25, ..latest }));
        struct Done;
        impl Subscription for Done {}
        Ok(Box::new(Done))
    }
}

/// A minute an hour, all at its price, a lot at the ask.
impl Ladders for Months {
    fn serves(&self, symbol: &str) -> bool {
        self.held(symbol).is_some()
    }
    fn history_days(&self) -> u32 {
        400
    }
    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError> {
        let index = self.held(symbol).ok_or(ProviderError::NotFound)?;
        Ok(series(index, Timeframe::hours(1), Some(from))
            .into_iter()
            .filter(|bar| bar.ts < to)
            .map(|bar| {
                let mut minute = Minute::opening(bar.ts, bar.close);
                minute.add_trade(to_tick(bar.close, tick_size), bar.close, 1, Side::Buy, 1);
                minute
            })
            .collect())
    }
    /// Noted, and nothing ever heard.
    fn watch(&self, symbol: &str, _: f64, _: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        self.followed.lock().unwrap().push(format!("ladders {symbol}"));
        struct Quiet;
        impl Subscription for Quiet {}
        Ok(Box::new(Quiet))
    }
}

