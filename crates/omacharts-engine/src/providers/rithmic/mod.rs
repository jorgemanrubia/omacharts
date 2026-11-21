//! Charts from a Rithmic login: CME futures, continuous, live.
//!
//! The protocol is [`rithmic`]'s, and so is the one session every chart
//! shares. This is the thin layer that names what Rithmic serves, asks it for
//! bars, and folds its trades into the bar that is forming.
//!
//! A root, never a contract. `ES` is `ES` here and in the cache key
//! (`rithmic:ES`), and which contract that means today — `ESZ6` — is asked
//! when bars are fetched, on the provider's thread. A roll is then a series
//! whose newest bars disagree with the cached tail of the old contract,
//! which the loader already treats as history rewritten.
//!
//! **Continuous, as traded.** Rithmic's history plant answers a root, `ES`,
//! with its own continuous series: each contract's bars while it was the
//! front month, joined at Rithmic's own roll and never adjusted, daily back
//! to 2011. That is what a root's history is read from
//! ([`Continuity::native`](crate::continuous::Continuity::native), as
//! traded, like thinkorswim's and Yahoo's), while the live trades are the
//! front month's. A month that has expired cannot be read on its own:
//! Rithmic refuses it with "reference data not available", so there is
//! nothing to stitch either. A dated contract may be asked for in either
//! spelling, `ESZ26` or Rithmic's `ESZ6` ([`spelled`]).
//!
//! **A decade apart.** Rithmic spells a contract's year with one digit, so
//! `ESZ6` is December 2026 and also December 2016, and its history plant
//! answers a long request with both. A contract's bars are kept only from
//! nine years before its month ([`first_possible`]), which no contract of
//! the same name ten years older reaches.

pub mod ladders;
pub mod session;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::bars::{Bar, Timeframe, Unit};
use crate::exchanges::registry;
use crate::specs::root_of;
use crate::provider::{
    Capability, FetchFailure, Ladders, Provider, ProviderError, Sink, Stream, Subscription, Update,
};
use crate::symbols::{Instrument, InstrumentKind};

/// The exchanges of the CME group, which is what a Rithmic market data
/// login is for. A root `contracts.tsv` puts anywhere else, or does not
/// list, is not ours.
const CME_GROUP: [&str; 4] = ["CME", "CBOT", "NYMEX", "COMEX"];

/// How far back a first fetch asks, in days: a month of one-minute bars,
/// some forty thousand of them, and a quarter of anything else intraday,
/// because a first chart is not worth more of the plant's quota than that.
/// The plant itself keeps years of intraday history of a root, and a chart
/// scrolled back to its first bar reads the same span again before it
/// ([`Provider::reaches_back`]), so this is where a chart starts, not how
/// far it can go.
const MINUTE_DAYS: u32 = 30;
const INTRADAY_DAYS: u32 = 90;
/// Daily bars go back ten years at first, and further on a scroll back.
const DAILY_DAYS: i64 = 3_650;

/// Every resolution as far back as the plant has it: none is capped.
const CAPABILITIES: &[Capability] = &[
    Capability { timeframe: Timeframe::minutes(1), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(2), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(5), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(15), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(30), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::minutes(90), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::hours(1), history_days: None, max_request_days: None },
    Capability { timeframe: Timeframe::days(1), history_days: None, max_request_days: None },
];

/// How Rithmic is offered.
pub const LISTED: crate::providers::Listed = crate::providers::Listed {
    id: "rithmic",
    label: "Rithmic",
    serves: "CME futures, continuous, from your Rithmic login",
    covers: "CME futures, front month",
    build: |feed| Box::new(Rithmic::of(feed)),
    setup: Some(&session::SIGNING),
    experimental: false,
    gate: None,
    login: None,
};

/// What a chart says when Rithmic has nobody signed in.
pub const NOT_SIGNED_IN: &str = "Rithmic is not signed in";

/// One login's feed, or the entry's, which has none. Nothing in it: the
/// session belongs to the first fetch.
pub struct Rithmic {
    feed: &'static crate::providers::Listed,
}

impl Default for Rithmic {
    fn default() -> Rithmic {
        Rithmic::new()
    }
}

impl Rithmic {
    /// The entry in the list of feeds, with no login of its own.
    pub fn new() -> Rithmic {
        Rithmic { feed: &LISTED }
    }

    /// The feed `feed`, a login's or the entry's.
    pub fn of(feed: &'static crate::providers::Listed) -> Rithmic {
        Rithmic { feed }
    }

    /// The login this feed signs in with: empty for the entry.
    fn login(&self) -> &'static str {
        self.feed.login.unwrap_or_default()
    }
}

/// This instrument in Rithmic's spelling, if Rithmic serves it: a futures
/// root on a CME group exchange, as itself. Pure and offline, because it is
/// asked on the main thread and to build cache keys.
///
/// Once a session has said which exchanges the login is entitled to, a root
/// on any other is not served either, and goes to the next source: Rithmic
/// Test's login sees CME and CBOT, so GC, CL and NG chart from Yahoo there.
pub fn rithmic_symbol(login: &str, instrument: &Instrument) -> Option<String> {
    symbol_under(instrument, |exchange| session::entitled(login, exchange))
}

/// [`rithmic_symbol`] with `entitled` saying which exchanges may be charted.
fn symbol_under(instrument: &Instrument, entitled: impl Fn(&str) -> bool) -> Option<String> {
    if let Some(explicit) = instrument.override_for("rithmic") {
        return Some(explicit.to_string());
    }
    if instrument.kind != InstrumentKind::FutureRoot {
        return None;
    }
    let exchange = registry().contract_exchange(instrument.root())?;
    (CME_GROUP.contains(&exchange) && entitled(exchange)).then(|| instrument.symbol.clone())
}

/// The exchange Rithmic is asked on for a root, or a dated contract of
/// one, and an order on it goes to. A root only an override names is taken
/// to be on CME.
pub fn exchange_of(symbol: &str) -> &'static str {
    let listed = registry().contract_exchange(root_of(symbol));
    CME_GROUP.into_iter().find(|exchange| Some(*exchange) == listed).unwrap_or("CME")
}

/// Is this a dated contract, `ESH7`, rather than a root, `ES`? Rithmic
/// spells both the same way, and a dated contract is asked for as itself
/// where a root is asked for as its front month.
fn is_dated(symbol: &str) -> bool {
    root_of(symbol) != symbol
}

/// A dated contract as Rithmic spells it, with a one-digit year: `ESZ6`,
/// from the stitcher's `ESZ26` or from `ESZ6` itself. A root, or anything
/// that is not a contract, is itself.
pub(crate) fn spelled(symbol: &str) -> String {
    match crate::contracts::parse(symbol, crate::contracts::today()) {
        Some((root, month)) if is_dated(symbol) => format!("{root}{}{}", month.code(), month.year.rem_euclid(10)),
        _ => symbol.to_string(),
    }
}

/// The earliest a bar can be of the dated contract `symbol`, in either
/// spelling: nine years before its month. Its namesake ten years older,
/// which Rithmic spells the same, has stopped trading by then. `None` for
/// a root.
fn first_possible(symbol: &str) -> Option<i64> {
    let (_, month) = crate::contracts::parse(symbol, crate::contracts::today()).filter(|_| is_dated(symbol))?;
    let month = crate::contracts::Month::new(month.year - 9, month.month)?;
    Some(month.first_day().and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

fn period_of(timeframe: Timeframe) -> Result<rithmic::Period, ProviderError> {
    match (timeframe.unit, timeframe.count) {
        (Unit::Minute, count @ (1 | 2 | 5 | 15 | 30 | 90)) => Ok(rithmic::Period::Minutes(count)),
        (Unit::Hour, 1) => Ok(rithmic::Period::Minutes(60)),
        (Unit::Day, 1) => Ok(rithmic::Period::Days(1)),
        _ => Err(ProviderError::Unsupported(format!("{} is folded, not fetched", timeframe.label()))),
    }
}

/// How far back a first fetch at `timeframe` goes, in seconds, and how
/// far before its first bar a chart scrolled back reads at a time.
fn depth(timeframe: Timeframe) -> i64 {
    match timeframe.unit {
        Unit::Minute if timeframe.count == 1 => i64::from(MINUTE_DAYS) * 86_400,
        Unit::Minute | Unit::Hour => i64::from(INTRADAY_DAYS) * 86_400,
        _ => DAILY_DAYS * 86_400,
    }
}

/// Rithmic's words, as a provider error. A refusal of the login `login` is
/// remembered for its row in the settings to show.
fn map_err(login: &str, error: rithmic::Error) -> ProviderError {
    if let rithmic::Error::Refused(why) = &error {
        session::refused(login, why);
    }
    describe(error)
}

/// Rithmic's words, as a provider error, and nothing remembered.
fn describe(error: rithmic::Error) -> ProviderError {
    use rithmic::Error;

    match error {
        Error::NoLogin(why) => ProviderError::NeedsSetup(format!("{NOT_SIGNED_IN}: {why}")),
        Error::Refused(why) => ProviderError::NeedsSetup(format!("Rithmic refused the sign-in: {why}")),
        Error::NotFound(_) => ProviderError::NotFound,
        Error::Rejected(why) if names_a_symbol(&why) => ProviderError::NotFound,
        Error::Rejected(why) => ProviderError::Network(format!("Rithmic refused the request: {why}")),
        Error::Connection(why) => ProviderError::Network(why),
        Error::Timeout => ProviderError::Network("Rithmic did not answer in time".into()),
        Error::Closed => ProviderError::Network("the Rithmic session was signed out".into()),
    }
}

fn names_a_symbol(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("symbol") || lower.contains("instrument") || lower.contains("not found")
}

fn bar_of(bar: &rithmic::Bar) -> Bar {
    Bar { ts: bar.start, open: bar.open, high: bar.high, low: bar.low, close: bar.close, volume: bar.volume as f64 }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Bars of `root`'s continuous series, or of the dated contract `root`
/// names, from `from` and starting before `to`, or up to now with no `to`
/// and the bar forming now with them ([`with_forming`]).
fn front_month_bars(
    login: &str,
    client: &rithmic::Client,
    root: &str,
    timeframe: Timeframe,
    from: i64,
    to: Option<i64>,
) -> Result<Vec<Bar>, ProviderError> {
    let bars = plant_bars(login, client, root, timeframe, from, to)?;
    if to.is_some() {
        return Ok(bars);
    }
    let minutes = |from: i64| plant_bars(login, client, root, Timeframe::minutes(1), from, None);
    Ok(with_forming(bars, timeframe, root, now(), minutes))
}

/// `bars` up to `now`, and the bar forming at `now` after them, folded from
/// the one-minute bars `minutes` fetches from a time on.
///
/// The history plant gives finished bars only. Without this the bar forming
/// now would be the trades' alone, and they are only heard from when a
/// chart subscribes: today's daily bar, opened on a chart in the afternoon,
/// would be the one price the market last traded at, with none of the
/// session's range behind it, and an hour's bar the same for its first
/// minutes. Its finished minutes say what it has done so far, from the
/// first: a daily bar from the evening its trading day opens, extended
/// hours and all, as the plant's own daily bars are. The trades then move
/// it from there, and once it has closed it is asked for whole.
///
/// A one-minute chart has no finer bars to fold, and a plant that does give
/// the bar forming now is taken at its word. When the minutes cannot be had
/// the bars are as they came, and the trades start the bar as before.
fn with_forming(
    mut bars: Vec<Bar>,
    timeframe: Timeframe,
    root: &str,
    now: i64,
    minutes: impl FnOnce(i64) -> Result<Vec<Bar>, ProviderError>,
) -> Vec<Bar> {
    if timeframe == Timeframe::minutes(1) {
        return bars;
    }
    let forming = bucket(now, timeframe, root);
    if bars.last().is_some_and(|bar| bar.ts >= forming) {
        return bars;
    }
    // A daily bar is stamped with its trading date, and CME's trading day
    // opens the evening before it.
    let from = match timeframe.unit {
        Unit::Day => forming - 86_400,
        _ => forming,
    };
    let Ok(minutes) = minutes(from) else {
        return bars;
    };
    let mut inside = minutes.into_iter().filter(|minute| minute.ts >= from && bucket(minute.ts, timeframe, root) == forming);
    let Some(first) = inside.next() else {
        return bars;
    };
    let bar = inside.fold(Bar { ts: forming, ..first }, |bar, minute| Bar {
        high: bar.high.max(minute.high),
        low: bar.low.min(minute.low),
        close: minute.close,
        volume: bar.volume + minute.volume,
        ..bar
    });
    bars.push(bar);
    bars
}

/// The plant's bars of `root`'s continuous series, or of the dated
/// contract `root` names, from `from` and starting before `to`, or up to
/// now with no `to`: finished bars only.
///
/// A root is asked for as itself, which Rithmic answers with its continuous
/// series. Where it has none for the root, its front month stands in: asked
/// once with the contract it remembers, and again with a fresh answer if
/// that one has nothing, since a contract that has rolled off is the likely
/// reason and the next front month is the cure. A span that ends before
/// now is older history, where having nothing is the answer rather than a
/// sign of a roll, so the front month is not asked again for it.
fn plant_bars(
    login: &str,
    client: &rithmic::Client,
    root: &str,
    timeframe: Timeframe,
    from: i64,
    to: Option<i64>,
) -> Result<Vec<Bar>, ProviderError> {
    let period = period_of(timeframe)?;
    let exchange = exchange_of(root);
    let (from, ask_to, until) = span(from, to, now(), timeframe);
    // Its own bars, and none of a namesake a decade older.
    let own = |contract: &str, bars: Vec<rithmic::Bar>| -> Vec<Bar> {
        let from = from.max(first_possible(contract).unwrap_or(i64::MIN));
        bars.iter().map(bar_of).filter(|bar| bar.ts >= from && bar.ts < until).collect()
    };
    if is_dated(root) {
        let contract = spelled(root);
        let bars = client.bars(&contract, exchange, period, from, ask_to).map_err(|error| map_err(login, error))?;
        return Ok(own(&contract, bars));
    }
    match client.bars(root, exchange, period, from, ask_to) {
        Ok(bars) if !bars.is_empty() => return Ok(own(root, bars)),
        Ok(none) if to.is_some() => return Ok(own(root, none)),
        _ => {}
    }
    let contract = client.front_month(root, exchange).map_err(|error| map_err(login, error))?;
    let first = client.bars(&contract, exchange, period, from, ask_to);
    let (contract, bars) = match first {
        Ok(bars) if !bars.is_empty() || to.is_some() => (contract, bars),
        stale => {
            client.forget_front_month(root, exchange);
            let fresh = client.front_month(root, exchange).map_err(|error| map_err(login, error))?;
            if fresh == contract {
                (contract, stale.map_err(|error| map_err(login, error))?)
            } else {
                let bars = client.bars(&fresh, exchange, period, from, ask_to).map_err(|error| map_err(login, error))?;
                (fresh, bars)
            }
        }
    };
    Ok(own(&contract, bars))
}

/// What the plant is asked for, given bars wanted from `from` and starting
/// before `to`, or up to `now` with no `to`: the first bar's start, the end
/// of the request, and the start no kept bar reaches. Up to now the
/// request runs a bar past it, for the one forming; a span that ends
/// earlier asks for nothing after it, so the bars a chart already holds
/// are never read again.
fn span(from: i64, to: Option<i64>, now: i64, timeframe: Timeframe) -> (i64, i64, i64) {
    match to {
        Some(to) => (from, to.max(from), to),
        None => (from, now + timeframe.seconds(), i64::MAX),
    }
}

/// Where a trade at `ts` lands at `timeframe`: the start of its minute or
/// hour, or midnight UTC of its trading date, which is how daily bars from
/// the history plant are stamped.
fn bucket(ts: i64, timeframe: Timeframe, root: &str) -> i64 {
    if timeframe.unit == Unit::Day {
        let instrument = Instrument { symbol: root_of(root).to_string(), kind: InstrumentKind::FutureRoot, ..placeholder() };
        let date = registry()
            .for_instrument(&instrument)
            .and_then(|schedule| schedule.trading_date(ts))
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .map(|midnight| midnight.and_utc().timestamp());
        return date.unwrap_or(ts.div_euclid(86_400) * 86_400);
    }
    let step = timeframe.seconds().max(60);
    ts.div_euclid(step) * step
}

fn placeholder() -> Instrument {
    Instrument {
        symbol: String::new(),
        name: String::new(),
        kind: InstrumentKind::FutureRoot,
        suffix: None,
        currency: None,
        tier: 0,
        session_origin: 0,
        overrides: Vec::new(),
        exchange: None,
        popularity: 0,
        local_name: None,
        formula: None,
    }
}

/// The bar a series is forming, from the trades that arrive.
///
/// Fed a snapshot first. The history plant's newest bar is usually the one
/// still forming, as far as the plant had got when it answered, and that
/// bar is taken as the forming one: every trade after it moves its close,
/// high and low. A trade the plant may already have counted — one matched
/// before it answered — moves the price and adds no size. Once that bar has
/// closed it is asked for again, whole, and replaces what was folded, which
/// settles any size counted twice or missed.
///
/// A snapshot that ends on a bar already closed leaves the next bar to the
/// trades, and that first bar may have missed some from before the
/// subscription began: it is asked for again the same way. Before it
/// closes, once the plant has the minute its first trade was in, the
/// minutes it has so far are asked for too and put under the trades
/// ([`Fold::mend`]): its open, and the high and low it made before
/// anybody was listening, are the market's rather than the first trade's.
///
/// A snapshot can also end short of the bar before the one forming: the
/// plant has a bar a few seconds after it closes, and a subscription that
/// starts in those seconds — a switch of chartbook is one — is answered
/// without it. Nothing else would ever fill that slot, so the first one
/// missing is asked for once it is due, with everything after it, and so
/// is a bar the plant folded as forming that closed before it answered.
///
/// The plant is not always that quick: at an open, with everybody asking,
/// it can be a minute behind. An answer without the bar asked for, and
/// with nothing after it either, is the plant still behind rather than a
/// slot nobody traded in, and the bar is asked for again, each time twice
/// as long after ([`Owed`]); the forming bar's minutes too. One that holds
/// a later bar says the slot is empty, and it is not asked again. After
/// [`PATIENCE`] misses a bar is left as it is until the next
/// [`Fold::reconcile`], which a chart coming back on screen asks for.
///
/// Only a trade for a bar older than the one forming is dropped. That bar is
/// the snapshot's, or is asked for whole.
struct Fold {
    timeframe: Timeframe,
    root: String,
    /// Trades in bars that start before this are left out.
    floor: Option<i64>,
    forming: Option<Bar>,
    /// The snapshot's own forming bar, and when the plant answered, in
    /// nanoseconds: a trade in it matched no later than that may be in it.
    counted: Option<(i64, i64)>,
    /// Bars that may be missing trades, or missing altogether: asked for
    /// again, with every bar after them, once they have closed.
    partial: Vec<Owed>,
    /// Those asked for and not yet answered.
    asked: Vec<Owed>,
    /// The oldest bar asked for [`PATIENCE`] times and never had: the next
    /// reconcile reaches back to it.
    doubt: Option<i64>,
    /// When the plant was last asked to reconcile, in seconds.
    reconciled: Option<i64>,
    /// The next new bar is partial.
    fresh: bool,
    /// The forming bar the trades started part-way through, waiting for
    /// its finished minutes.
    seed: Option<Seed>,
    /// A snapshot has arrived: until one has, there is nothing to fold into.
    ready: bool,
    /// Snapshots asked for and not yet answered.
    loading: usize,
    /// Trades heard while a snapshot was being fetched. The snapshot may be
    /// older than the newest of them, so they are folded onto it again.
    heard: Vec<rithmic::Trade>,
}

/// A forming bar started by the first trade heard, which the minutes
/// before that trade belong in too.
#[derive(Clone, Copy, Debug)]
struct Seed {
    /// The bar's start.
    start: i64,
    /// The end of the minute the first trade was in, in seconds: the
    /// minutes the plant has by then hold every trade before it.
    after: i64,
    /// The size of the trades from `after` on, which those minutes do not.
    volume: f64,
    /// When to ask for its minutes, in seconds.
    due: i64,
    /// Asked for, and not yet answered.
    asked: bool,
    /// Answers so far without its minutes.
    missed: u32,
}

/// A bar to be asked for whole, with every bar after it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Owed {
    start: i64,
    /// When to ask, in seconds.
    due: i64,
    /// Answers so far that did not have it.
    missed: u32,
}

/// The plant has a closed minute a few seconds after it closes.
const SETTLED: i64 = 30;

/// How many answers without a bar are taken for the plant being behind.
/// Asked after [`SETTLED`] and then twice as long each time, the last ask
/// is a quarter of an hour after the bar closed.
const PATIENCE: u32 = 5;

/// How long after an answer at `asked`, in seconds, that missed a bar for
/// the `missed`th time, to ask for it again.
fn again(asked: i64, missed: u32) -> i64 {
    asked + (SETTLED << missed.min(PATIENCE))
}

/// How many bars before the newest a reconcile asks for, as the first
/// snapshot overlaps what the chart holds.
const RECONCILED: i64 = 5;

impl Fold {
    fn new(timeframe: Timeframe, root: &str) -> Fold {
        Fold {
            timeframe,
            root: root.to_string(),
            floor: None,
            forming: None,
            counted: None,
            partial: Vec::new(),
            asked: Vec::new(),
            doubt: None,
            reconciled: None,
            fresh: true,
            seed: None,
            ready: false,
            loading: 0,
            heard: Vec::new(),
        }
    }

    /// A snapshot has been asked for.
    fn asking(&mut self) {
        self.loading += 1;
    }

    /// The snapshot asked for could not be had.
    fn failed(&mut self) {
        self.loading = self.loading.saturating_sub(1);
        if self.loading == 0 {
            self.heard.clear();
        }
    }

    fn bucket(&self, nanos: i64) -> i64 {
        bucket(nanos.div_euclid(1_000_000_000), self.timeframe, &self.root)
    }

    /// Start again from a snapshot asked for at `asked` and answered at
    /// `answered`, both in nanoseconds. The forming bar, if the trades
    /// heard while it was fetched moved it past what the snapshot says.
    fn snapshot(&mut self, bars: &[Bar], asked: i64, answered: i64) -> Option<Bar> {
        self.loading = self.loading.saturating_sub(1);
        self.ready = true;
        self.forming = None;
        self.counted = None;
        self.seed = None;
        let now = self.bucket(answered);
        match bars.last() {
            Some(last) if last.ts == now => {
                self.floor = Some(last.ts);
                self.forming = Some(*last);
                self.counted = Some((last.ts, answered));
                self.owe(last.ts);
                self.fresh = false;
            }
            last => {
                self.floor = last.map(|bar| bar.ts + 1);
                self.fresh = true;
                if let Some(last) = last {
                    // Forming when the plant was asked, and closed by the
                    // time it answered: what it had of the bar then.
                    if last.ts == self.bucket(asked) {
                        self.owe(last.ts);
                    }
                    // The plant had not got as far as the bar before the
                    // one forming: the first slot it left out, and the
                    // rest with it. Intraday only, where every slot is
                    // the same length and an empty one asked for costs
                    // one answer with nothing in it.
                    let next = last.ts + self.timeframe.seconds();
                    if self.timeframe.is_intraday() && next < now {
                        self.owe(next);
                    }
                }
            }
        }
        let heard = if self.loading == 0 { std::mem::take(&mut self.heard) } else { self.heard.clone() };
        let mut moved = None;
        for trade in &heard {
            if let Some(bar) = self.fold(trade) {
                moved = Some(bar);
            }
        }
        moved
    }

    /// The bar starting at `start` is to be asked for whole once closed.
    fn owe(&mut self, start: i64) {
        if !self.partial.iter().chain(&self.asked).any(|owed| owed.start == start) {
            let due = start + self.timeframe.seconds() + SETTLED;
            self.partial.push(Owed { start, due, missed: 0 });
        }
    }

    /// One trade, as it arrives. The bar it changed, if it changed one.
    fn trade(&mut self, trade: &rithmic::Trade) -> Option<Bar> {
        if self.loading > 0 {
            self.heard.push(*trade);
        }
        if !self.ready {
            return None;
        }
        self.fold(trade)
    }

    fn fold(&mut self, trade: &rithmic::Trade) -> Option<Bar> {
        let ts = trade.at_nanos.div_euclid(1_000_000_000);
        let start = bucket(ts, self.timeframe, &self.root);
        if self.floor.is_some_and(|floor| start < floor) {
            return None;
        }
        let counted = self.counted.is_some_and(|(bar, until)| bar == start && trade.at_nanos <= until);
        let volume = if counted { 0.0 } else { trade.size as f64 };
        match self.forming.as_mut() {
            Some(bar) if bar.ts == start => {
                bar.high = bar.high.max(trade.price);
                bar.low = bar.low.min(trade.price);
                bar.close = trade.price;
                bar.volume += volume;
                if let Some(seed) = self.seed.as_mut().filter(|seed| seed.start == start && ts >= seed.after) {
                    seed.volume += volume;
                }
            }
            // A late trade for a bar already behind us: the bar it belongs to
            // is asked for whole once it has closed, so it is dropped here.
            Some(bar) if start < bar.ts => return None,
            _ => {
                self.seed = None;
                if std::mem::take(&mut self.fresh) {
                    self.owe(start);
                    // A minute is the finest the plant has: a one-minute
                    // bar has nothing to be seeded from.
                    if self.timeframe.seconds() > 60 {
                        let after = (ts.div_euclid(60) + 1) * 60;
                        let due = after + SETTLED;
                        self.seed = Some(Seed { start, after, volume: 0.0, due, asked: false, missed: 0 });
                    }
                }
                self.forming = Some(Bar {
                    ts: start,
                    open: trade.price,
                    high: trade.price,
                    low: trade.price,
                    close: trade.price,
                    volume,
                });
            }
        }
        self.forming
    }

    /// What to ask the plant for at `now`, by start: partial bars that
    /// closed long enough ago for the plant to have them whole, taken off
    /// the list, and the forming bar's minutes once it has the first.
    fn due(&mut self, now: i64) -> Vec<i64> {
        let (owed, waiting): (Vec<Owed>, Vec<Owed>) = self.partial.iter().partition(|owed| owed.due <= now);
        self.partial = waiting;
        let mut due: Vec<i64> = owed.iter().map(|owed| owed.start).collect();
        self.asked.extend(owed);
        // A bar that has closed is asked for whole, not for its minutes.
        if self.seed.is_some_and(|seed| seed.start + self.timeframe.seconds() <= now) {
            self.seed = None;
        }
        if let Some(seed) = self.seed.as_mut().filter(|seed| !seed.asked && seed.due <= now) {
            seed.asked = true;
            if !due.contains(&seed.start) {
                due.push(seed.start);
            }
        }
        due
    }

    /// What the plant answered, asked at `asked` in nanoseconds, for bars
    /// from `start` on: the bars it changed, oldest first.
    ///
    /// A bar that had closed when the plant was asked is whole, and is
    /// taken as it is. The one forming then is its finished minutes, and
    /// only the bar the trades started part-way through is put on them:
    /// their open, the trades' close — the market's last word — and the
    /// high and low of both. The size is the minutes' and that of the
    /// trades after them; the bar is asked for whole once closed, which
    /// settles it.
    ///
    /// A bar asked for that the answer does not have, with nothing after
    /// it either, is the plant behind: it is asked for again later, and so
    /// are the forming bar's minutes when they were not there.
    fn mend(&mut self, start: i64, bars: &[Bar], asked: i64) -> Vec<Bar> {
        let open = self.bucket(asked);
        let seconds = asked.div_euclid(1_000_000_000);
        if let Some(at) = self.asked.iter().position(|owed| owed.start == start) {
            let owed = self.asked.remove(at);
            if !bars.iter().any(|bar| bar.ts >= start) {
                let missed = owed.missed + 1;
                match missed < PATIENCE {
                    true => self.partial.push(Owed { start, due: again(seconds, missed), missed }),
                    false => self.doubt = Some(self.doubt.map_or(start, |doubt| doubt.min(start))),
                }
            }
        }
        let mut changed = Vec::new();
        for bar in bars {
            if bar.ts < open {
                if let Some(forming) = self.forming.as_mut().filter(|forming| forming.ts == bar.ts) {
                    *forming = *bar;
                }
                // Whole now, so its minutes are not wanted any more.
                if self.seed.is_some_and(|seed| seed.start == bar.ts) {
                    self.seed = None;
                }
                changed.push(*bar);
                continue;
            }
            let Some(seed) = self.seed.filter(|seed| seed.asked && seed.start == bar.ts) else { continue };
            let Some(forming) = self.forming.as_mut().filter(|forming| forming.ts == bar.ts) else { continue };
            *forming = Bar {
                ts: bar.ts,
                open: bar.open,
                high: bar.high.max(forming.high),
                low: bar.low.min(forming.low),
                close: forming.close,
                volume: bar.volume + seed.volume,
            };
            self.seed = None;
            changed.push(*forming);
        }
        // Still waiting for its minutes: the plant had not got to them.
        if let Some(seed) = self.seed.as_mut().filter(|seed| seed.asked && seed.start == start) {
            seed.asked = false;
            seed.missed += 1;
            seed.due = again(seconds, seed.missed);
            if seed.missed >= PATIENCE {
                self.seed = None;
            }
        }
        changed
    }

    /// Where to ask the plant from at `now`, in seconds, to bring the last
    /// bars up to date, when it is worth asking: the few before `newest`,
    /// the newest bar the chart was sent, and back to any bar given up on.
    ///
    /// Not before a snapshot, which is the same answer and is on its way,
    /// and not again within [`SETTLED`] of the last: a chartbook flicked
    /// through and back asks once.
    fn reconcile(&mut self, now: i64, newest: Option<i64>) -> Option<i64> {
        if !self.ready || self.loading > 0 || self.reconciled.is_some_and(|at| now - at < SETTLED) {
            return None;
        }
        let newest = newest?;
        self.reconciled = Some(now);
        let from = newest - RECONCILED * self.timeframe.seconds();
        Some(self.doubt.take().map_or(from, |doubt| doubt.min(from)))
    }
}

/// What the subscription's thread is told: what the session hears, and what
/// its history thread fetched.
enum Event {
    Live(rithmic::Live),
    /// A snapshot, and when the plant was asked and answered, in
    /// nanoseconds.
    Loaded { bars: Result<Vec<Bar>, ProviderError>, asked: i64, at: i64 },
    /// A partial bar asked for again once it closed, and the bars after it,
    /// as the plant has them, and when it was asked, in nanoseconds:
    /// nothing, when the plant had nothing or could not be asked.
    Whole { start: i64, bars: Vec<Bar>, asked: i64 },
    /// A chart showing the series came back on screen.
    Reconcile,
    Stop,
}

/// What the history thread is asked to fetch.
enum Job {
    /// A snapshot, from this time on.
    Snapshot(i64),
    /// The bar starting then, whole, and every bar after it.
    Whole(i64),
}

/// Fetches bars from a time on.
type Load = Box<dyn FnMut(i64) -> Result<Vec<Bar>, ProviderError> + Send>;

/// The time now, in nanoseconds: the wall clock, or a test's.
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// An open subscription. Dropping it tells its thread to stop, without
/// waiting for it.
struct Live {
    stop: Arc<AtomicBool>,
    events: mpsc::Sender<Event>,
}

impl Subscription for Live {
    fn reconcile(&self) {
        let _ = self.events.send(Event::Reconcile);
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.events.send(Event::Stop);
    }
}

fn failure(login: &str, error: rithmic::Error) -> FetchFailure {
    FetchFailure::from(&map_err(login, error))
}

/// One subscription's life, on a thread of its own: watch the front month,
/// and hand what the session hears to [`serve`].
#[allow(clippy::too_many_arguments)]
fn stream(
    login: &'static str,
    root: String,
    timeframe: Timeframe,
    since: Option<i64>,
    sink: Sink,
    stop: Arc<AtomicBool>,
    events: mpsc::Receiver<Event>,
    sender: mpsc::Sender<Event>,
) {
    let say = |update: Update| {
        if !stop.load(Ordering::Acquire) {
            sink(update);
        }
    };
    let client = match session::client(login) {
        Ok(client) => client,
        Err(error) => return say(Update::Lost(failure(login, error))),
    };
    let heard = sender.clone();
    let hear = move |live| {
        if let rithmic::Live::Lost(rithmic::Error::Refused(why)) = &live {
            session::refused(login, why);
        }
        let _ = heard.send(Event::Live(live));
    };
    let _watch = match is_dated(&root) {
        true => client.watch_contract(&spelled(&root), exchange_of(&root), hear),
        false => client.watch(&root, exchange_of(&root), hear),
    };
    let load: Load = {
        let root = root.clone();
        Box::new(move |from: i64| front_month_bars(login, &client, &root, timeframe, from, None))
    };
    let clock: Clock = Arc::new(|| chrono::Utc::now().timestamp_nanos_opt().unwrap_or(i64::MAX));
    serve(&events, sender, Fold::new(timeframe, root_of(&root)), since, load, clock, &say, &stop);
}

/// Fetch what [`serve`] asks for, in the order it asks, and hand it back as
/// events: on a thread of its own, so that a trade never waits behind a
/// request to the history plant.
fn fetch(jobs: mpsc::Receiver<Job>, mut load: Load, clock: Clock, back: mpsc::Sender<Event>) {
    for job in jobs {
        let event = match job {
            Job::Snapshot(from) => {
                let asked = clock();
                let bars = load(from);
                Event::Loaded { bars, asked, at: clock() }
            }
            // Sent back even with nothing in it: that is how the fold
            // knows to ask again.
            Job::Whole(start) => {
                let asked = clock();
                let bars: Vec<Bar> = load(start).unwrap_or_default().into_iter().filter(|bar| bar.ts >= start).collect();
                Event::Whole { start, bars, asked }
            }
        };
        if back.send(event).is_err() {
            return;
        }
    }
}

/// Turn what the session hears into updates: a snapshot each time it says
/// it is watching — which it says again after every reconnect, and after
/// trades were missed — the forming bar on each trade, and a loss as a
/// loss. `load` fetches bars from a time on, on a thread of its own whose
/// answers come back through `back`, so a trade is never behind a fetch;
/// `since` is what the chart already holds.
#[allow(clippy::too_many_arguments)]
fn serve(
    events: &mpsc::Receiver<Event>,
    back: mpsc::Sender<Event>,
    mut fold: Fold,
    since: Option<i64>,
    load: Load,
    clock: Clock,
    say: &dyn Fn(Update),
    stop: &AtomicBool,
) {
    let timeframe = fold.timeframe;
    let (jobs, queue) = mpsc::channel();
    let fetching = clock.clone();
    let spawned = std::thread::Builder::new()
        .name("omacharts rithmic history".into())
        .spawn(move || fetch(queue, load, fetching, back));
    if let Err(error) = spawned {
        return say(Update::Lost(FetchFailure::from(&ProviderError::Network(error.to_string()))));
    }
    // What the chart holds already, and then what this has sent it.
    let mut newest = since;
    // A snapshot has been sent, so the chart holds every bar up to `newest`.
    let mut served = false;
    let sent =|bar: &Bar, newest: &mut Option<i64>| {
        *newest = Some(newest.map_or(bar.ts, |held| held.max(bar.ts)));
    };
    for event in events {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match event {
            Event::Stop => return,
            Event::Live(rithmic::Live::Watching { .. }) => {
                // The first from a few bars before what the chart holds, as
                // the loader overlaps. Watching again, after a drop or
                // missed trades, is from the newest bar sent, the one the
                // drop cut: every bar before it was whole, and the gap after
                // it is all that is asked of the plant's quota.
                let from = match newest {
                    Some(newest) if served => newest,
                    Some(newest) => newest - 5 * timeframe.seconds(),
                    None => clock().div_euclid(1_000_000_000) - depth(timeframe),
                };
                fold.asking();
                if jobs.send(Job::Snapshot(from)).is_err() {
                    return;
                }
            }
            Event::Loaded { bars: Ok(bars), asked, at } => {
                served = true;
                let moved = fold.snapshot(&bars, asked, at);
                if let Some(last) = bars.last() {
                    sent(last, &mut newest);
                }
                say(Update::Snapshot(bars));
                if let Some(bar) = moved {
                    sent(&bar, &mut newest);
                    say(Update::Bar(bar));
                }
            }
            Event::Loaded { bars: Err(error), .. } => {
                fold.failed();
                say(Update::Lost(FetchFailure::from(&error)));
            }
            Event::Whole { start, bars, asked } => {
                for bar in fold.mend(start, &bars, asked) {
                    sent(&bar, &mut newest);
                    say(Update::Bar(bar));
                }
            }
            Event::Reconcile => {
                if let Some(from) = fold.reconcile(clock().div_euclid(1_000_000_000), newest)
                    && jobs.send(Job::Whole(from)).is_err()
                {
                    return;
                }
            }
            // The trade a subscription opens with is folded as any other:
            // a finished bar the plant gave already has it and leaves it
            // out, and the bar forming now, which the plant does not give,
            // starts from it rather than from whenever the market next
            // trades, so the last price is the market's at once.
            Event::Live(rithmic::Live::Trade(trade) | rithmic::Live::Last(trade)) => {
                if let Some(bar) = fold.trade(&trade) {
                    sent(&bar, &mut newest);
                    say(Update::Bar(bar));
                }
                for start in fold.due(clock().div_euclid(1_000_000_000)) {
                    if jobs.send(Job::Whole(start)).is_err() {
                        return;
                    }
                }
            }
            Event::Live(rithmic::Live::Lost(error)) => say(Update::Lost(FetchFailure::from(&describe(error)))),
        }
    }
}

/// The whole of what this feed does to stream: name the root, hand the
/// session a callback, and fold what comes back. Sharing one subscription
/// between charts, coalescing ticks to frames, writing behind to the cache
/// and deciding when to try again after a loss all happen above this line.
impl Stream for Rithmic {
    fn subscribe(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
        sink: Sink,
    ) -> Result<Box<dyn Subscription>, ProviderError> {
        period_of(timeframe)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (events, inbox) = mpsc::channel();
        let root = symbol.to_string();
        let sender = events.clone();
        let stopping = stop.clone();
        let login = self.login();
        std::thread::Builder::new()
            .name("omacharts rithmic stream".into())
            .spawn(move || stream(login, root, timeframe, since, sink, stopping, inbox, sender))
            .map_err(|error| ProviderError::Network(error.to_string()))?;
        Ok(Box::new(Live { stop, events }))
    }
}

impl Provider for Rithmic {
    fn id(&self) -> &'static str {
        self.feed.id
    }

    fn label(&self) -> &'static str {
        self.feed.label
    }

    /// The login's own feed, as it prints.
    fn delay_minutes(&self, _kind: InstrumentKind) -> u32 {
        0
    }

    fn native_symbol(&self, instrument: &Instrument) -> Option<String> {
        rithmic_symbol(self.login(), instrument)
    }

    /// A root as its front month, which is what [`front_month_bars`]
    /// draws: the one Rithmic said, which reaches [`crate::contracts`]
    /// before the chart's bars do.
    fn charted_contract(&self, instrument: &Instrument) -> Option<String> {
        let symbol = self.symbol_for(instrument)?;
        match is_dated(&symbol) {
            true => Some(symbol),
            false => crate::contracts::front(&symbol, crate::contracts::today()).map(|front| front.symbol),
        }
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    /// Rithmic's own continuous series of a root, as traded: see the
    /// module docs.
    fn continuity(&self) -> crate::continuous::Continuity {
        crate::continuous::Continuity::native(&[crate::continuous::Adjustment::AsTraded])
    }

    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        period_of(timeframe)?;
        if !session::here() {
            return Err(ProviderError::NeedsSetup(
                "Rithmic charts are served by the open window, which holds the one session".into(),
            ));
        }
        let login = self.login();
        let client = session::client(login).map_err(|error| map_err(login, error))?;
        let from = since.unwrap_or_else(|| now() - depth(timeframe));
        front_month_bars(login, &client, symbol, timeframe, from, None)
    }

    /// Only the span asked for, never what comes after it: the chart holds
    /// that already, and the plant's quota is spent on what it does not.
    fn bars_between(&self, symbol: &str, timeframe: Timeframe, since: i64, until: i64) -> Result<Vec<Bar>, ProviderError> {
        period_of(timeframe)?;
        if !session::here() {
            return Err(ProviderError::NeedsSetup(
                "Rithmic charts are served by the open window, which holds the one session".into(),
            ));
        }
        let login = self.login();
        let client = session::client(login).map_err(|error| map_err(login, error))?;
        front_month_bars(login, &client, symbol, timeframe, since, Some(until))
    }

    /// The plant keeps years of every resolution it serves, so a chart
    /// scrolled back reads as far again as its first load did, each time
    /// it reaches its first bar: a month of minutes, a quarter of anything
    /// else intraday, ten years of days.
    fn reaches_back(&self, _symbol: &str, timeframe: Timeframe) -> Option<i64> {
        period_of(timeframe).ok().map(|_| depth(timeframe))
    }

    fn stream(&self) -> Option<&dyn Stream> {
        Some(self)
    }

    /// Minute ladders, history and live: see [`ladders`].
    fn ladders(&self) -> Option<&dyn Ladders> {
        Some(self)
    }

    /// The contracts of `root` Rithmic's symbol search lists, each with
    /// the day it expires: the exchange's own listing, where the rule's is
    /// a computation.
    fn contracts(&self, root: &str) -> Result<Vec<(String, chrono::NaiveDate)>, ProviderError> {
        if !session::here() {
            return Err(ProviderError::NeedsSetup("Rithmic lists contracts for the open window, which holds the one session".into()));
        }
        let login = self.login();
        let client = session::client(login).map_err(|error| map_err(login, error))?;
        let listed = client.listed(root, exchange_of(root)).map_err(|error| map_err(login, error))?;
        Ok(listed
            .into_iter()
            .filter_map(|(symbol, (year, month, day))| Some((symbol, chrono::NaiveDate::from_ymd_opt(year, month, day)?)))
            .collect())
    }

    /// The window is done with Rithmic: the session logs out of every
    /// plant. A chart that asks again later signs in again.
    fn retire(&self) {
        rithmic::close_data(self.login());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    fn instrument(kind: InstrumentKind, symbol: &str) -> Instrument {
        Instrument { symbol: symbol.into(), kind, ..placeholder() }
    }

    /// The stitcher's two-digit year is Rithmic's one; a root, or Rithmic's
    /// own spelling, is itself.
    #[test]
    fn a_month_is_asked_of_rithmic_as_rithmic_spells_it() {
        assert_eq!(spelled("ESZ26"), "ESZ6");
        assert_eq!(spelled("MNQH27"), "MNQH7");
        assert_eq!(spelled("ESZ6"), "ESZ6");
        assert_eq!(spelled("ES"), "ES");
        assert_eq!(spelled("6EZ26"), "6EZ6");
    }

    /// December 2026's bars start no earlier than nine years before it, so
    /// December 2016's, which Rithmic spells the same, are left out; a
    /// root has no such bound.
    #[test]
    fn a_contract_keeps_none_of_its_namesake_a_decade_older() {
        let first = first_possible("ESZ26").expect("a contract");
        assert_eq!(first, first_possible("ESZ6").unwrap(), "either spelling");
        let dec_2016 = chrono::NaiveDate::from_ymd_opt(2016, 12, 15).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        let dec_2017 = chrono::NaiveDate::from_ymd_opt(2017, 12, 1).unwrap().and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        assert!(dec_2016 < first && first <= dec_2017);
        assert_eq!(first_possible("ES"), None);
    }

    /// Scrolled back, the plant is asked for the span before the chart's
    /// first bar and nothing after it, which the chart holds already; a
    /// first load or a tail runs to now and the bar forming past it.
    #[test]
    fn an_older_span_asks_for_nothing_the_chart_holds() {
        let hour = Timeframe::hours(1);
        let (now, first) = (2_000_000_000, 1_990_000_000);
        assert_eq!(span(first - 90 * 86_400, Some(first), now, hour), (first - 90 * 86_400, first, first));
        assert_eq!(span(first, None, now, hour), (first, now + 3_600, i64::MAX));
        assert_eq!(span(first, Some(first - 1), now, hour).1, first, "never a request that ends before it starts");
    }

    /// Every resolution Rithmic serves scrolls back, by as much again as
    /// its first load took, and every one folded from them with it; none
    /// is capped, since the plant keeps years of each.
    #[test]
    fn every_resolution_scrolls_back_and_none_is_capped() {
        let rithmic = Rithmic::new();
        for capability in rithmic.capabilities() {
            assert_eq!(capability.history_days, None, "{:?}", capability.timeframe);
            let reach = rithmic.reaches_back("ES", capability.timeframe);
            assert_eq!(reach, Some(depth(capability.timeframe)), "{:?}", capability.timeframe);
        }
        for label in ["3m", "10m", "2h", "4h", "1W", "1M", "3D"] {
            let folded = Timeframe::parse(label).expect("a resolution");
            assert!(rithmic.reaches_back("ES", folded.native()).is_some(), "{label}, folded from {:?}", folded.native());
        }
        assert_eq!(rithmic.reaches_back("ESZ6", Timeframe::minutes(1)), Some(30 * 86_400), "a dated contract too");
    }

    /// A root is Rithmic's own continuous series, as traded: an expired
    /// month cannot be read on its own, so there is nothing to stitch.
    #[test]
    fn a_root_is_rithmics_own_continuous_series() {
        assert_eq!(crate::continuous::source(&Rithmic::new()), crate::continuous::Source::Native);
        assert_eq!(Rithmic::new().continuity().native, &[crate::continuous::Adjustment::AsTraded]);
    }

    #[test]
    fn cme_futures_are_served_and_nothing_else_is() {
        let rithmic = Rithmic::new();
        for root in ["ES", "NQ", "CL", "GC", "ZN", "MES", "6E"] {
            assert_eq!(
                rithmic.symbol_for(&instrument(InstrumentKind::FutureRoot, root)).as_deref(),
                Some(root),
                "{root}"
            );
        }
        for (kind, symbol) in [
            (InstrumentKind::Equity, "AAPL"),
            (InstrumentKind::Index, "GSPC"),
            (InstrumentKind::Fx, "EURUSD"),
            (InstrumentKind::Crypto, "BTC"),
            (InstrumentKind::Etf, "SPY"),
            // On ICE, not CME, and so not this login's to chart.
            (InstrumentKind::FutureRoot, "KC"),
            // A root nothing lists is not guessed at.
            (InstrumentKind::FutureRoot, "ZZZ"),
        ] {
            assert_eq!(rithmic.symbol_for(&instrument(kind, symbol)), None, "{symbol}");
        }
    }

    /// What the session says the login may see decides what Rithmic serves:
    /// with CME and CBOT alone, as on Rithmic Test, the metals and energy go
    /// to the next source, and before any session has said, all of it is
    /// Rithmic's as before.
    #[test]
    fn a_root_on_an_exchange_the_login_is_not_entitled_to_is_not_served() {
        let test_login = |exchange: &str| matches!(exchange, "CME" | "CBOT");
        for root in ["ES", "NQ", "ZN", "ZB", "6E"] {
            assert!(symbol_under(&instrument(InstrumentKind::FutureRoot, root), test_login).is_some(), "{root}");
        }
        for root in ["GC", "SI", "CL", "NG"] {
            assert_eq!(symbol_under(&instrument(InstrumentKind::FutureRoot, root), test_login), None, "{root}");
            assert!(symbol_under(&instrument(InstrumentKind::FutureRoot, root), |_| true).is_some(), "{root}");
        }
        assert!(session::entitled("a login no session has spoken for", "NYMEX"), "unknown means entitled");
    }

    #[test]
    fn an_explicit_override_wins() {
        let mut es = instrument(InstrumentKind::FutureRoot, "ES");
        es.overrides.push(("rithmic".into(), "MES".into()));
        assert_eq!(Rithmic::new().symbol_for(&es).as_deref(), Some("MES"));
        assert_eq!(exchange_of("CL"), "NYMEX");
        assert_eq!(exchange_of("GC"), "COMEX");
        assert_eq!(exchange_of("ZN"), "CBOT");
    }

    /// A root is charted as its front month, a dated contract as itself, so
    /// a position held in that month on any service is marked to the chart.
    #[test]
    fn a_root_is_charted_as_its_front_month() {
        let rithmic = Rithmic::new();
        let front = crate::contracts::front("ES", crate::contracts::today()).map(|front| front.symbol);
        assert!(front.is_some());
        assert_eq!(rithmic.charted_contract(&instrument(InstrumentKind::FutureRoot, "ES")), front);
        let dated = instrument(InstrumentKind::FutureRoot, "MESZ6");
        assert_eq!(rithmic.charted_contract(&dated).as_deref(), Some("MESZ6"));
    }

    #[test]
    fn timeframes_it_does_not_fetch_are_refused_before_asking() {
        assert!(matches!(period_of(Timeframe::hours(4)), Err(ProviderError::Unsupported(_))));
        assert!(matches!(period_of(Timeframe::weeks(1)), Err(ProviderError::Unsupported(_))));
        assert_eq!(period_of(Timeframe::hours(1)).ok(), Some(rithmic::Period::Minutes(60)));
        for capability in CAPABILITIES {
            assert!(period_of(capability.timeframe).is_ok(), "{:?}", capability.timeframe);
        }
    }

    fn trade(seconds: i64, price: f64, size: u64) -> rithmic::Trade {
        rithmic::Trade { price, size, aggressor: None, at_nanos: seconds * 1_000_000_000 + 250 }
    }

    fn ohlc(ts: i64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Bar {
        Bar { ts, open, high, low, close, volume }
    }

    const NANOS: i64 = 1_000_000_000;

    /// The snapshot's newest bar is the minute still forming, and every
    /// trade after it moves it: a trade the plant may have counted already
    /// moves the price and adds no size, a later one adds its size too. Only
    /// a trade for an older bar is dropped, and the forming minute is asked
    /// for whole once it has closed.
    #[test]
    fn the_snapshots_forming_minute_moves_with_every_trade() {
        let minute = Timeframe::minutes(1);
        let mut fold = Fold::new(minute, "ES");
        let snapshot = [ohlc(600, 1.0, 1.0, 1.0, 1.0, 1.0), ohlc(660, 3.0, 4.0, 2.0, 3.0, 10.0)];
        assert_eq!(fold.snapshot(&snapshot, 690 * NANOS, 690 * NANOS), None);

        assert_eq!(fold.trade(&trade(630, 50.0, 1)), None, "a trade for a closed bar");
        let counted = fold.trade(&trade(689, 5.0, 2)).unwrap();
        assert_eq!((counted.ts, counted.open, counted.high, counted.close, counted.volume), (660, 3.0, 5.0, 5.0, 10.0));
        let low = fold.trade(&trade(700, 1.5, 3)).unwrap();
        assert_eq!((low.high, low.low, low.close, low.volume), (5.0, 1.5, 1.5, 13.0));

        let next = fold.trade(&trade(720, 2.0, 4)).unwrap();
        assert_eq!((next.ts, next.open, next.volume), (720, 2.0, 4.0));
        assert_eq!(fold.trade(&trade(715, 50.0, 1)), None, "a late trade for the minute just closed");

        // The forming minute is replaced whole once closed; the next one was
        // heard from its first trade and is whole already.
        assert!(fold.due(720 + 29).is_empty());
        assert_eq!(fold.due(720 + 30), vec![660]);
        assert!(fold.due(10_000).is_empty());
    }

    /// A snapshot that ends on a closed minute leaves the next one to the
    /// trades, and that one may have missed some: asked for whole once it
    /// has closed.
    #[test]
    fn after_a_snapshot_of_closed_bars_the_next_bar_is_built_and_asked_for_again() {
        let minute = Timeframe::minutes(1);
        let mut fold = Fold::new(minute, "ES");
        fold.snapshot(&[ohlc(600, 1.0, 1.0, 1.0, 1.0, 1.0)], 665 * NANOS, 665 * NANOS);
        assert_eq!(fold.trade(&trade(630, 5.0, 2)), None, "the snapshot's own minute");

        let first = fold.trade(&trade(666, 10.0, 2)).unwrap();
        assert_eq!((first.ts, first.open, first.volume), (660, 10.0, 2.0));
        let second = fold.trade(&trade(700, 12.0, 1)).unwrap();
        assert_eq!((second.high, second.close, second.volume), (12.0, 12.0, 3.0));
        fold.trade(&trade(720, 11.0, 4)).unwrap();
        assert_eq!(fold.due(720 + 30), vec![660]);
    }

    /// Nothing is folded before the first snapshot, which holds those trades;
    /// one heard while it was fetched and matched after the plant answered
    /// is put on top of it.
    #[test]
    fn trades_heard_while_the_first_snapshot_is_fetched_are_put_on_top_of_it() {
        let mut fold = Fold::new(Timeframe::minutes(1), "ES");
        fold.asking();
        assert_eq!(fold.trade(&trade(670, 2.0, 1)), None);
        assert_eq!(fold.trade(&trade(680, 9.0, 5)), None);
        let moved = fold.snapshot(&[ohlc(660, 3.0, 4.0, 2.0, 3.0, 10.0)], 675 * NANOS, 675 * NANOS).unwrap();
        assert_eq!((moved.high, moved.low, moved.close, moved.volume), (9.0, 2.0, 9.0, 15.0));
    }

    /// `serve` on a thread of its own, with a clock the test sets and a fake
    /// history plant.
    struct Served {
        send: mpsc::Sender<Event>,
        heard: mpsc::Receiver<Update>,
        clock: Arc<std::sync::atomic::AtomicI64>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Served {
        fn start(
            since: Option<i64>,
            now: i64,
            load: impl FnMut(i64) -> Result<Vec<Bar>, ProviderError> + Send + 'static,
        ) -> Served {
            Served::start_at(Timeframe::minutes(1), since, now, load)
        }

        fn start_at(
            timeframe: Timeframe,
            since: Option<i64>,
            now: i64,
            load: impl FnMut(i64) -> Result<Vec<Bar>, ProviderError> + Send + 'static,
        ) -> Served {
            let (send, events) = mpsc::channel();
            let (tell, heard) = mpsc::channel();
            let clock = Arc::new(std::sync::atomic::AtomicI64::new(now * NANOS));
            let reading = clock.clone();
            let back = send.clone();
            let thread = std::thread::spawn(move || {
                let say = move |update: Update| {
                    let _ = tell.send(update);
                };
                let clock: Clock = Arc::new(move || reading.load(Ordering::SeqCst));
                let fold = Fold::new(timeframe, "ES");
                serve(&events, back, fold, since, Box::new(load), clock, &say, &AtomicBool::new(false));
            });
            Served { send, heard, clock, thread: Some(thread) }
        }

        fn at(&self, seconds: i64) {
            self.clock.store(seconds * NANOS, Ordering::SeqCst);
        }

        fn watching(&self) {
            let _ = self.send.send(Event::Live(rithmic::Live::Watching { contract: "ESZ6".into() }));
        }

        fn trade(&self, seconds: i64, price: f64, size: u64) {
            self.send.send(Event::Live(rithmic::Live::Trade(trade(seconds, price, size)))).unwrap();
        }

        fn reconcile(&self) {
            self.send.send(Event::Reconcile).unwrap();
        }

        fn next(&self) -> Update {
            self.heard.recv_timeout(std::time::Duration::from_secs(10)).expect("an update")
        }

        fn bar(&self) -> Bar {
            match self.next() {
                Update::Bar(bar) => bar,
                other => panic!("expected a bar, heard {other:?}"),
            }
        }

        fn snapshot(&self) -> Vec<Bar> {
            match self.next() {
                Update::Snapshot(bars) => bars,
                other => panic!("expected a snapshot, heard {other:?}"),
            }
        }
    }

    impl Drop for Served {
        fn drop(&mut self) {
            let _ = self.send.send(Event::Stop);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// A minute boundary well in the past, so nothing here races the wall
    /// clock.
    const AT: i64 = 1_700_000_040;

    /// The whole point: a trade in the minute the snapshot was forming is on
    /// the chart at once, and so is every trade of a burst.
    #[test]
    fn every_trade_in_the_snapshots_forming_minute_is_delivered_at_once() {
        let served = Served::start(Some(AT - 600), AT + 20, |_| Ok(vec![ohlc(AT, 10.0, 11.0, 9.0, 10.5, 100.0)]));
        served.watching();
        assert_eq!(served.snapshot().len(), 1);

        served.trade(AT + 25, 12.0, 1);
        let bar = served.bar();
        assert_eq!((bar.ts, bar.open, bar.high, bar.low, bar.close), (AT, 10.0, 12.0, 9.0, 12.0));
        served.trade(AT + 26, 8.0, 1);
        let low = served.bar();
        assert_eq!((low.low, low.close), (8.0, 8.0));

        for i in 0..200 {
            served.trade(AT + 30, 9.0 + f64::from(i) * 0.25, 1);
        }
        for i in 0..200 {
            assert_eq!(served.bar().close, 9.0 + f64::from(i) * 0.25, "trade {i}");
        }
    }

    /// The plant gives finished bars only: the trade a subscription opens
    /// with starts the bar forming now, so the chart's last price is the
    /// market's at once on a market that seldom trades, and is left out
    /// when it is in a bar the plant gave.
    #[test]
    fn the_trade_a_watch_opens_with_starts_the_forming_bar() {
        let last = |seconds, price| Event::Live(rithmic::Live::Last(trade(seconds, price, 1)));
        let served = Served::start(Some(AT - 600), AT + 20, |_| Ok(vec![ohlc(AT - 60, 10.0, 11.0, 9.0, 10.5, 100.0)]));
        served.watching();
        served.send.send(last(AT + 10, 10.25)).unwrap();
        assert_eq!(served.snapshot().len(), 1);
        let forming = served.bar();
        assert_eq!((forming.ts, forming.close, forming.volume), (AT, 10.25, 1.0));

        let finished = Served::start(Some(AT - 600), AT + 20, |_| Ok(vec![ohlc(AT - 60, 10.0, 11.0, 9.0, 10.5, 100.0)]));
        finished.watching();
        finished.send.send(last(AT - 30, 10.5)).unwrap();
        assert_eq!(finished.snapshot().len(), 1);
        finished.trade(AT + 25, 12.0, 1);
        let next = finished.bar();
        assert_eq!((next.ts, next.open, next.volume), (AT, 12.0, 1.0), "the finished bar's trade is not counted again");
    }

    /// Asking the plant for a closed minute whole takes as long as it takes,
    /// and the trades go on meanwhile.
    #[test]
    fn a_refetch_of_a_closed_bar_does_not_hold_back_a_trade() {
        let (release, gate) = mpsc::channel::<()>();
        let mut calls = 0;
        let served = Served::start(Some(AT - 600), AT + 20, move |from| {
            calls += 1;
            if calls == 1 {
                return Ok(vec![ohlc(AT, 10.0, 11.0, 9.0, 10.5, 100.0)]);
            }
            // The history plant, slow to answer.
            gate.recv().unwrap();
            Ok(vec![ohlc(from, 10.0, 13.0, 8.0, 12.0, 250.0)])
        });
        served.watching();
        served.snapshot();

        // The next minute, long enough after the snapshot's for it to be due.
        served.at(AT + 60 + 31);
        served.trade(AT + 61, 12.0, 1);
        assert_eq!(served.bar().ts, AT + 60);
        // The refetch is blocked; the trade after it is not.
        served.trade(AT + 62, 12.5, 1);
        assert_eq!(served.bar().close, 12.5);
        served.trade(AT + 63, 12.75, 1);
        assert_eq!(served.bar().close, 12.75);

        release.send(()).unwrap();
        let whole = served.bar();
        assert_eq!((whole.ts, whole.volume), (AT, 250.0));
    }

    /// The session saying it is watching again in the middle of a minute
    /// reloads the snapshot without freezing the bar: trades go on moving it
    /// while the reload is out, and the reloaded snapshot is moved on to the
    /// newest of them.
    #[test]
    fn watching_again_mid_minute_keeps_the_forming_bar_live() {
        let (release, gate) = mpsc::channel::<()>();
        let mut calls = 0;
        let served = Served::start(Some(AT - 600), AT + 5, move |_| {
            calls += 1;
            if calls > 1 {
                gate.recv().unwrap();
            }
            // The plant as of a moment ago, behind the trades after it.
            Ok(vec![ohlc(AT, 10.0, 11.0, 9.0, 10.5, 100.0)])
        });
        served.watching();
        served.snapshot();
        served.at(AT + 20);
        served.trade(AT + 20, 11.5, 1);
        assert_eq!(served.bar().close, 11.5);

        for again in 0..3 {
            served.watching();
            served.trade(AT + 30 + again, 12.0 + f64::from(again as i32), 2);
            assert_eq!(served.bar().close, 12.0 + f64::from(again as i32), "during reload {again}");
            served.at(AT + 30 + again);
            release.send(()).unwrap();
            let reloaded = served.snapshot();
            assert_eq!(reloaded.last().unwrap().close, 10.5);
            let moved = served.bar();
            let newest = 12.0 + f64::from(again as i32);
            assert_eq!((moved.ts, moved.close, moved.high), (AT, newest, newest));
            // And the next trade moves it again, as any other would.
            served.trade(AT + 40 + again, 9.5, 1);
            let after = served.bar();
            assert_eq!((after.ts, after.close), (AT, 9.5));
        }
    }

    /// A drop is a loss, and the session's coming back is a fresh snapshot
    /// from the newest bar the chart was sent, the one the drop cut, which
    /// fills the gap the drop left and asks for nothing the chart has whole.
    #[test]
    fn a_drop_is_a_loss_and_coming_back_is_a_fresh_snapshot() {
        let (asked, froms) = mpsc::channel();
        let served = Served::start(Some(AT - 600), AT + 30, move |from| {
            asked.send(from).unwrap();
            // The plant has every minute from `from` to the reconnect.
            Ok((0..4).map(|i| ohlc(from.max(AT) + 60 * i, 1.0, 1.0, 1.0, 1.0, 10.0)).collect())
        });
        served.watching();
        assert_eq!(served.snapshot().len(), 4);
        served.at(AT + 241);
        served.trade(AT + 241, 2.0, 3);
        let bar = served.bar();
        assert_eq!((bar.ts, bar.volume), (AT + 240, 3.0));
        let dropped = rithmic::Error::Connection("connection closed".into());
        served.send.send(Event::Live(rithmic::Live::Lost(dropped))).unwrap();
        assert_eq!(served.next(), Update::Lost(FetchFailure::Unreachable));
        // Back three minutes later.
        served.at(AT + 420);
        served.watching();
        let refill = served.snapshot();
        assert_eq!(refill.iter().map(|bar| bar.ts).collect::<Vec<_>>(), vec![AT + 240, AT + 300, AT + 360, AT + 420]);
        assert_eq!(refill[0].volume, 10.0, "the cut bar whole, as the plant has it");
        // The first snapshot goes back from what the chart held, the second
        // from the bar the drop cut, once.
        let froms: Vec<i64> = froms.try_iter().collect();
        assert_eq!(froms, vec![AT - 600 - 300, AT + 240]);
        served.send.send(Event::Stop).unwrap();
        served.watching();
        assert!(served.heard.recv_timeout(std::time::Duration::from_millis(200)).is_err(), "nothing after the stop");
    }

    /// A daily bar is the trading day's, and CME's day starts the evening
    /// before: a trade at 23:00 UTC on a Monday is Tuesday's.
    #[test]
    fn a_daily_bar_belongs_to_its_trading_date() {
        // 2026-11-02 (a Monday) 23:00 UTC is 17:00 in Chicago.
        let evening = 1_793_660_400;
        let tuesday = 1_793_664_000;
        assert_eq!(bucket(evening, Timeframe::days(1), "ES"), tuesday);
        assert_eq!(bucket(tuesday + 3_600, Timeframe::days(1), "ES"), tuesday);
    }

    /// Tuesday 2026-11-03, ES's trading date: its session opened at 17:00
    /// in Chicago on the Monday, 23:00 UTC, and it is now 15:00 UTC, 09:00
    /// in Chicago, into the regular session.
    const TUESDAY: i64 = 1_793_664_000;
    const TUESDAY_OPENS: i64 = TUESDAY - 3_600;
    const NOW: i64 = TUESDAY + 15 * 3_600;

    /// What the plant has of Tuesday so far at 15:00 UTC: finished minutes
    /// from the Monday afternoon, which are Monday's, through the overnight
    /// and into the regular session. The overnight makes the low, the
    /// regular session the high.
    fn tuesdays_minutes() -> Vec<Bar> {
        vec![
            ohlc(TUESDAY_OPENS - 7_200, 6_900.0, 6_910.0, 6_890.0, 6_905.0, 50.0),
            ohlc(TUESDAY_OPENS, 6_800.0, 6_802.0, 6_799.0, 6_801.0, 10.0),
            ohlc(TUESDAY_OPENS + 3 * 3_600, 6_801.0, 6_801.0, 6_750.0, 6_760.0, 20.0),
            ohlc(TUESDAY + 14 * 3_600 + 1_800, 6_760.0, 6_840.0, 6_760.0, 6_830.0, 30.0),
            ohlc(NOW - 60, 6_830.0, 6_831.0, 6_820.0, 6_825.0, 40.0),
        ]
    }

    /// The days the plant gives: Monday's, finished, and not Tuesday's.
    fn finished_days() -> Vec<Bar> {
        vec![
            ohlc(TUESDAY - 2 * 86_400, 6_850.0, 6_880.0, 6_840.0, 6_870.0, 900.0),
            ohlc(TUESDAY - 86_400, 6_870.0, 6_920.0, 6_860.0, 6_905.0, 1_000.0),
        ]
    }

    /// The bug: the plant gives finished days only, and today's bar was the
    /// trade a watch opened with, one price with no range. Today's bar is
    /// folded from its finished minutes, from the evening its trading day
    /// opened, the overnight with them, and Monday's afternoon left out.
    #[test]
    fn todays_daily_bar_is_the_whole_session_so_far() {
        let day = Timeframe::days(1);
        let asked = std::cell::Cell::new(None);
        let bars = with_forming(finished_days(), day, "ES", NOW, |from| {
            asked.set(Some(from));
            Ok(tuesdays_minutes())
        });
        assert!(asked.get().is_some_and(|from| from <= TUESDAY_OPENS), "asked from {:?}", asked.get());
        assert_eq!(bars.len(), 3);
        assert_eq!(&bars[..2], &finished_days()[..]);
        let today = bars[2];
        assert_eq!(today.ts, TUESDAY);
        assert_eq!((today.open, today.high, today.low, today.close), (6_800.0, 6_840.0, 6_750.0, 6_825.0));
        assert_eq!(today.volume, 100.0);
    }

    /// On regular hours a daily bar is still the trading day's, as every
    /// day before it is: the session filter leaves daily bars whole, so
    /// today's keeps the overnight's open and low, the same as on extended
    /// hours.
    #[test]
    fn todays_daily_bar_is_the_same_on_extended_and_regular_hours() {
        let bars = with_forming(finished_days(), Timeframe::days(1), "ES", NOW, |_| Ok(tuesdays_minutes()));
        let es = instrument(InstrumentKind::FutureRoot, "ES");
        for session in crate::Session::ALL {
            let drawn = crate::session::filter(&bars, session, &es, false);
            let today = drawn.last().copied().expect("today's bar");
            assert_eq!((today.ts, today.open, today.high, today.low), (TUESDAY, 6_800.0, 6_840.0, 6_750.0), "{session:?}");
        }
    }

    /// An hour, the bars a four-hour chart folds from, starts the same way:
    /// its finished minutes, and none from the hour before.
    #[test]
    fn the_forming_hour_is_its_finished_minutes() {
        let hour = Timeframe::hours(1);
        let now = TUESDAY + 14 * 3_600 + 2_000;
        let finished = vec![ohlc(TUESDAY + 13 * 3_600, 6_770.0, 6_775.0, 6_765.0, 6_760.0, 5.0)];
        let minutes = vec![
            ohlc(TUESDAY + 13 * 3_600 + 3_540, 6_760.0, 6_790.0, 6_700.0, 6_760.0, 9.0),
            ohlc(TUESDAY + 14 * 3_600, 6_760.0, 6_765.0, 6_755.0, 6_762.0, 3.0),
            ohlc(TUESDAY + 14 * 3_600 + 1_800, 6_762.0, 6_840.0, 6_760.0, 6_830.0, 30.0),
        ];
        let bars = with_forming(finished.clone(), hour, "ES", now, |from| {
            assert_eq!(from, TUESDAY + 14 * 3_600);
            Ok(minutes)
        });
        let forming = bars[1];
        assert_eq!((forming.ts, forming.open, forming.high, forming.low, forming.close), (TUESDAY + 14 * 3_600, 6_760.0, 6_840.0, 6_755.0, 6_830.0));
        assert_eq!(forming.volume, 33.0);
    }

    /// The plant's own forming bar is taken as it is, a one-minute chart
    /// has nothing finer, and minutes that cannot be had leave the bars as
    /// they came: none of them asks for minutes it will not use.
    #[test]
    fn nothing_is_folded_where_there_is_nothing_to_fold() {
        let day = Timeframe::days(1);
        let unasked = |_| -> Result<Vec<Bar>, ProviderError> { panic!("minutes asked for") };
        let mut given = finished_days();
        given.push(ohlc(TUESDAY, 6_800.0, 6_850.0, 6_700.0, 6_820.0, 99.0));
        assert_eq!(with_forming(given.clone(), day, "ES", NOW, unasked), given);
        let minute = Timeframe::minutes(1);
        assert_eq!(with_forming(tuesdays_minutes(), minute, "ES", NOW, unasked), tuesdays_minutes());

        let down = with_forming(finished_days(), day, "ES", NOW, |_| Err(ProviderError::Network("down".into())));
        assert_eq!(down, finished_days());
        let quiet = with_forming(finished_days(), day, "ES", NOW, |_| Ok(Vec::new()));
        assert_eq!(quiet, finished_days());
    }

    /// End to end on a daily chart, the way it went wrong after a switch of
    /// chartbook: a fresh watch, a snapshot of finished days, and the trade
    /// the watch opens with. Today's bar keeps the session's open, high and
    /// low, and the trade only moves its close.
    #[test]
    fn a_daily_chart_watched_mid_session_draws_today_whole() {
        let day = Timeframe::days(1);
        let load = |_| Ok(with_forming(finished_days(), Timeframe::days(1), "ES", NOW, |_| Ok(tuesdays_minutes())));
        let served = Served::start_at(day, Some(TUESDAY - 86_400), NOW + 5, load);
        served.watching();
        let snapshot = served.snapshot();
        assert_eq!(snapshot.last().map(|bar| bar.ts), Some(TUESDAY));
        served.send.send(Event::Live(rithmic::Live::Last(trade(NOW + 10, 6_822.0, 1)))).unwrap();
        let today = served.bar();
        assert_eq!((today.ts, today.open, today.high, today.low, today.close), (TUESDAY, 6_800.0, 6_840.0, 6_750.0, 6_822.0));
    }

    /// The test that matters most about errors: a login refused, or
    /// missing, is something to sign in again about rather than a network
    /// failure to sit out; and none of them can carry a password.
    /// The history plant as it is at `now`: the bars of `minutes` it has
    /// finished and settled, `lag` seconds after each closes, at
    /// `timeframe` from `from` on, and the bar forming at `now` folded from
    /// the minutes it has of it, as [`front_month_bars`] asks.
    fn plant(minutes: &[Bar], timeframe: Timeframe, lag: i64, now: i64, from: i64) -> Vec<Bar> {
        let settled = |length: i64| move |bar: &Bar| bar.ts + length + lag <= now;
        let finished: Vec<Bar> = minutes.iter().copied().filter(settled(60)).collect();
        let mut bars: Vec<Bar> = Vec::new();
        for minute in &finished {
            let start = bucket(minute.ts, timeframe, "ES");
            match bars.last_mut() {
                Some(bar) if bar.ts == start => {
                    bar.high = bar.high.max(minute.high);
                    bar.low = bar.low.min(minute.low);
                    bar.close = minute.close;
                    bar.volume += minute.volume;
                }
                _ => bars.push(Bar { ts: start, ..*minute }),
            }
        }
        bars.retain(|bar| bar.ts >= from && settled(timeframe.seconds())(bar));
        with_forming(bars, timeframe, "ES", now, |from| Ok(finished.iter().copied().filter(|bar| bar.ts >= from).collect()))
    }

    /// Every update a served subscription sends, put together as the
    /// window's series does, until `done` says it has enough.
    fn drawn(served: &Served, series: &mut Vec<Bar>, done: impl Fn(&[Bar]) -> bool) {
        while !done(series) {
            match served.next() {
                Update::Snapshot(bars) => *series = bars,
                Update::Bar(bar) => {
                    crate::stream::upsert(series, bar);
                }
                Update::Lost(why) => panic!("lost: {why:?}"),
            }
        }
    }

    /// The bug, as a switch of chartbook met it on two-minute bars: the
    /// subscription opened a few seconds into a bar, before the plant had
    /// settled the one that just closed. The snapshot ended a bar short,
    /// the bar forming was started by the first trade heard, and nothing
    /// ever asked for the bar in between: the chart's last bar sat far from
    /// the close before it, with none of what its interval had done before
    /// the switch. The missing bar is asked for once the plant has it, and
    /// the forming bar is put on its finished minutes once there is one:
    /// no slot is missing, and the last bar opens where its interval did.
    #[test]
    fn a_chart_switched_to_early_in_a_bar_misses_no_bar_and_none_of_the_forming_one() {
        let two = Timeframe::minutes(2);
        // A two-minute boundary, and the chart's cache ten minutes behind it.
        let bar = AT;
        let since = bar - 600;
        // A market climbing a point a minute, and then the forming bar's
        // first minute: its open, its high and its low all made before the
        // switch, and the minute after it.
        let mut minutes: Vec<Bar> = (0..20)
            .map(|i| {
                let price = 100.0 + f64::from(i);
                ohlc(bar - 1_200 + 60 * i64::from(i), price, price + 0.75, price - 0.25, price + 0.5, 10.0)
            })
            .collect();
        minutes.push(ohlc(bar, 120.0, 126.0, 119.0, 125.0, 10.0));
        minutes.push(ohlc(bar + 60, 125.0, 127.0, 124.5, 127.0, 7.0));
        let truth = plant(&minutes, two, 0, bar + 120, 0);

        let clock = Arc::new(std::sync::atomic::AtomicI64::new(0));
        let reading = clock.clone();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let asking = asked.clone();
        let fed = minutes.clone();
        let served = Served::start_at(two, Some(since), bar + 10, move |from| {
            asking.lock().unwrap().push(from);
            // The plant settles a bar twenty seconds after it closes.
            Ok(plant(&fed, two, 20, reading.load(Ordering::SeqCst), from))
        });
        let at = |seconds: i64| {
            served.at(seconds);
            clock.store(seconds, Ordering::SeqCst);
        };
        at(bar + 10);
        served.watching();
        let mut series = Vec::new();
        drawn(&served, &mut series, |series| !series.is_empty());
        assert_eq!(series.last().map(|bar| bar.ts), Some(bar - 240), "the plant had not settled the bar before");

        // The first trade heard starts the forming bar, part-way through.
        served.trade(bar + 15, 124.0, 1);
        drawn(&served, &mut series, |series| series.last().is_some_and(|last| last.ts == bar));
        assert_eq!(series.last().map(|last| last.open), Some(124.0), "as far as the trades go");

        // Once the plant has it, the bar left out is asked for, and drawn.
        at(bar + 31);
        served.trade(bar + 31, 125.0, 1);
        drawn(&served, &mut series, |series| series.iter().any(|drawn| drawn.ts == bar - 120));

        // Once the plant has the minute the first trade was in, the forming
        // bar is put on its minutes: its open, high and low are the
        // interval's, its close the last trade's.
        at(bar + 91);
        served.trade(bar + 91, 127.0, 2);
        drawn(&served, &mut series, |series| series.last().is_some_and(|last| last.open == 120.0));

        let slots: Vec<i64> = series.iter().map(|drawn| drawn.ts).filter(|ts| *ts >= since).collect();
        assert_eq!(slots, (0..=5).map(|i| since + 120 * i).collect::<Vec<_>>(), "no slot missing");
        for drawn in series.iter().filter(|drawn| drawn.ts < bar) {
            assert_eq!(Some(drawn), truth.iter().find(|whole| whole.ts == drawn.ts), "{}", drawn.ts);
        }
        let last = *series.last().unwrap();
        assert_eq!((last.ts, last.open, last.high, last.low, last.close), (bar, 120.0, 127.0, 119.0, 127.0));
        assert_eq!(last.volume, 12.0, "the first minute's, and the trades after it");
        // Two asks besides the snapshot, each once.
        assert_eq!(asked.lock().unwrap().len(), 3, "{:?}", asked.lock().unwrap());
    }

    /// The same switch half a bar later: the plant has the bar before, and
    /// the forming bar's first minute, and the snapshot draws it from them,
    /// with nothing more to ask until it closes.
    #[test]
    fn a_chart_switched_to_late_in_a_bar_draws_it_from_its_minutes() {
        let two = Timeframe::minutes(2);
        let bar = AT;
        let minutes = vec![
            ohlc(bar - 120, 100.0, 101.0, 99.0, 100.5, 10.0),
            ohlc(bar - 60, 100.5, 102.0, 100.0, 101.0, 10.0),
            ohlc(bar, 110.0, 115.0, 109.0, 114.0, 10.0),
        ];
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counting = calls.clone();
        let served = Served::start_at(two, Some(bar - 120), bar + 85, move |from| {
            counting.fetch_add(1, Ordering::SeqCst);
            Ok(plant(&minutes, two, 20, bar + 85, from))
        });
        served.watching();
        let mut series = Vec::new();
        drawn(&served, &mut series, |series| !series.is_empty());
        served.trade(bar + 86, 113.0, 1);
        drawn(&served, &mut series, |series| series.last().is_some_and(|last| last.close == 113.0));
        let last = *series.last().unwrap();
        assert_eq!((last.ts, last.open, last.high, last.low), (bar, 110.0, 115.0, 109.0));
        assert_eq!(series.iter().map(|drawn| drawn.ts).collect::<Vec<_>>(), vec![bar - 120, bar]);
        served.at(bar + 100);
        served.trade(bar + 100, 112.0, 1);
        assert_eq!(served.bar().close, 112.0);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "nothing asked before the bar closes");
    }

    /// A bar the plant folded as forming when it was asked, and which had
    /// closed by the time it answered, is only what it had then: it is
    /// asked for whole, as the snapshot's forming bar would be.
    #[test]
    fn a_forming_bar_closed_before_the_plant_answered_is_asked_for_whole() {
        let mut fold = Fold::new(Timeframe::minutes(2), "ES");
        let snapshot = [ohlc(AT - 120, 1.0, 1.0, 1.0, 1.0, 1.0), ohlc(AT, 1.0, 2.0, 1.0, 2.0, 5.0)];
        fold.snapshot(&snapshot, (AT + 110) * NANOS, (AT + 125) * NANOS);
        assert!(fold.trade(&trade(AT + 126, 3.0, 1)).is_some());
        assert!(fold.due(AT + 120 + 29).is_empty());
        assert_eq!(fold.due(AT + 120 + 30), vec![AT]);
    }

    /// A one-minute bar has nothing finer to be seeded from: the trades
    /// start it, it is asked for whole once closed, and nothing is asked
    /// before then.
    #[test]
    fn a_minute_is_not_seeded() {
        let mut fold = Fold::new(Timeframe::minutes(1), "ES");
        fold.snapshot(&[ohlc(AT - 60, 1.0, 1.0, 1.0, 1.0, 1.0)], (AT + 5) * NANOS, (AT + 5) * NANOS);
        fold.trade(&trade(AT + 10, 2.0, 1)).unwrap();
        assert!(fold.due(AT + 59).is_empty());
        assert_eq!(fold.due(AT + 90), vec![AT]);
    }

    /// A snapshot that ends at the close before a halt, answered as the
    /// market reopens, asks for the hour in between at once rather than a
    /// bar at a time. An answer with nothing in it may be the plant still
    /// behind, so it is asked again a minute on; one that holds the first
    /// bar after the halt says the hour was empty, and that is the last.
    #[test]
    fn a_halt_is_asked_for_until_the_plant_is_past_it() {
        let mut fold = Fold::new(Timeframe::minutes(2), "ES");
        let reopens = AT + 3_720;
        fold.snapshot(&[ohlc(AT - 120, 1.0, 1.0, 1.0, 1.0, 1.0)], reopens * NANOS, (reopens + 2) * NANOS);
        fold.trade(&trade(reopens + 3, 2.0, 1)).unwrap();
        assert_eq!(fold.due(reopens + 4), vec![AT]);
        assert_eq!(fold.mend(AT, &[], (reopens + 4) * NANOS), vec![]);
        assert!(fold.due(reopens + 63).is_empty());
        assert_eq!(fold.due(reopens + 64), vec![AT], "asked again");
        // The bar forming at the reopen, as far as its minutes go: the
        // trades' to draw, but the plant is past the halt.
        let first = ohlc(reopens, 2.0, 2.0, 2.0, 2.0, 1.0);
        assert_eq!(fold.mend(AT, &[first], (reopens + 64) * NANOS), vec![]);
        assert!((0..2_000).all(|later| fold.due(reopens + 64 + later).iter().all(|start| *start != AT)), "and not again");
    }

    /// A bar the plant never has is asked for [`PATIENCE`] times, each
    /// twice as long after the last, and then left; the next reconcile
    /// reaches back to it, and asks once however often it is asked.
    #[test]
    fn a_bar_the_plant_never_has_is_left_to_the_next_reconcile() {
        let two = Timeframe::minutes(2);
        let mut fold = Fold::new(two, "ES");
        fold.snapshot(&[ohlc(AT - 120, 1.0, 1.0, 1.0, 1.0, 1.0)], (AT + 5) * NANOS, (AT + 5) * NANOS);
        fold.trade(&trade(AT + 6, 2.0, 1)).unwrap();
        // The seed's minutes, and then the bar whole, never there.
        let mut asks = Vec::new();
        for now in AT + 6..AT + 3_000 {
            for start in fold.due(now) {
                asks.push((start, now));
                fold.mend(start, &[], now * NANOS);
            }
        }
        let seeds: Vec<i64> = asks.iter().filter(|(_, at)| *at < AT + 120).map(|(_, at)| *at - AT).collect();
        assert_eq!(seeds, vec![90], "the minutes, while the bar formed");
        let wholes: Vec<i64> = asks.iter().filter(|(_, at)| *at >= AT + 120).map(|(_, at)| *at - AT).collect();
        assert_eq!(wholes, vec![150, 210, 330, 570, 1_050], "the bar whole, five times");
        assert!(asks.iter().all(|(start, _)| *start == AT));

        assert_eq!(fold.reconcile(AT + 3_000, Some(AT + 2_880)), Some(AT), "back to it");
        assert_eq!(fold.reconcile(AT + 3_010, Some(AT + 2_880)), None, "once");
        assert_eq!(fold.reconcile(AT + 3_030, Some(AT + 2_880)), Some(AT + 2_880 - 600), "and then the last few");
    }

    /// Nothing is asked to reconcile before a snapshot, which is the same
    /// answer and on its way.
    #[test]
    fn a_reconcile_before_the_snapshot_asks_nothing() {
        let mut fold = Fold::new(Timeframe::minutes(2), "ES");
        assert_eq!(fold.reconcile(AT, Some(AT - 600)), None);
        fold.asking();
        fold.snapshot(&[ohlc(AT - 120, 1.0, 1.0, 1.0, 1.0, 1.0)], AT * NANOS, AT * NANOS);
        fold.asking();
        assert_eq!(fold.reconcile(AT, Some(AT - 120)), None, "another on its way");
    }

    /// The bug, as a chartbook switched back to met it at an open: the
    /// subscription opened early in a two-minute bar, and the plant, busy,
    /// settled each bar over a minute after it closed rather than the half
    /// minute the fold allowed. The bar left out of the snapshot, the
    /// forming bar's minutes and that bar whole were each asked for once,
    /// came back without what was asked, and were never asked again: the
    /// chart kept a missing slot and a stub that opened at the first trade
    /// heard. Each is asked again until the plant has it, and every bar
    /// ends up as the plant has it.
    #[test]
    fn a_chart_switched_back_to_while_the_plant_is_slow_ends_up_with_every_bar_whole() {
        let two = Timeframe::minutes(2);
        let bar = AT;
        let since = bar - 600;
        let mut minutes: Vec<Bar> = (0..20)
            .map(|i| {
                let price = 100.0 + f64::from(i);
                ohlc(bar - 1_200 + 60 * i64::from(i), price, price + 0.75, price - 0.25, price + 0.5, 10.0)
            })
            .collect();
        // The forming bar, and the one after it, minute by minute; the
        // trades below are these minutes' prints from the switch on.
        minutes.push(ohlc(bar, 120.0, 126.0, 119.0, 125.0, 10.0));
        minutes.push(ohlc(bar + 60, 125.0, 127.0, 124.5, 127.0, 7.0));
        minutes.push(ohlc(bar + 120, 127.0, 128.0, 126.5, 127.5, 7.0));
        minutes.push(ohlc(bar + 180, 127.5, 128.0, 127.0, 128.0, 7.0));
        let prints: Vec<(i64, f64)> = vec![
            (bar + 15, 125.0),
            (bar + 50, 125.0),
            (bar + 61, 125.0),
            (bar + 90, 127.0),
            (bar + 110, 124.5),
            (bar + 119, 127.0),
            (bar + 121, 127.0),
            (bar + 150, 128.0),
            (bar + 170, 126.5),
            (bar + 179, 127.5),
            (bar + 181, 127.5),
            (bar + 200, 128.0),
            (bar + 220, 127.0),
            (bar + 239, 128.0),
        ];
        let truth = plant(&minutes, two, 0, bar + 240, 0);

        let clock = Arc::new(std::sync::atomic::AtomicI64::new(0));
        let reading = clock.clone();
        let asked = Arc::new(Mutex::new(Vec::new()));
        let asking = asked.clone();
        let fed = minutes.clone();
        let served = Served::start_at(two, Some(since), bar + 10, move |from| {
            asking.lock().unwrap().push(from);
            // Seventy-five seconds behind every close.
            Ok(plant(&fed, two, 75, reading.load(Ordering::SeqCst), from))
        });
        let at = |seconds: i64| {
            served.at(seconds);
            clock.store(seconds, Ordering::SeqCst);
        };
        at(bar + 10);
        served.watching();
        let mut series = Vec::new();
        drawn(&served, &mut series, |series| !series.is_empty());

        // The trades, and then the market quiet but for a print every ten
        // seconds at the last price, which is when the fold asks.
        let mut feed: Vec<(i64, f64)> = prints.clone();
        feed.extend((bar + 240..bar + 1_200).step_by(10).map(|t| (t, 128.0)));
        for (t, price) in feed {
            at(t);
            served.trade(t, price, 1);
            while let Ok(update) = served.heard.recv_timeout(std::time::Duration::from_millis(20)) {
                match update {
                    Update::Snapshot(bars) => series = bars,
                    Update::Bar(bar) => {
                        crate::stream::upsert(&mut series, bar);
                    }
                    Update::Lost(why) => panic!("lost: {why:?}"),
                }
            }
        }

        let slots: Vec<i64> = series.iter().map(|drawn| drawn.ts).filter(|ts| *ts >= since && *ts <= bar + 120).collect();
        assert_eq!(slots, (0..=6).map(|i| since + 120 * i).collect::<Vec<_>>(), "no slot missing");
        for drawn in series.iter().filter(|drawn| drawn.ts < bar + 120) {
            assert_eq!(Some(drawn), truth.iter().find(|whole| whole.ts == drawn.ts), "{}", drawn.ts - bar);
        }
        // The bar after, the trades' from its first print: as the plant
        // has it, but for the size of prints this test makes up.
        let next = series.iter().find(|drawn| drawn.ts == bar + 120).unwrap();
        let whole = truth.iter().find(|whole| whole.ts == bar + 120).unwrap();
        assert_eq!((next.open, next.high, next.low, next.close), (whole.open, whole.high, whole.low, whole.close));
        let asks = asked.lock().unwrap().len();
        assert!(asks <= 10, "a handful of asks, not one a trade: {asks}");
    }

    /// A chart that comes back on screen asks the plant for the last few
    /// bars of a series streamed all along, and a bar the trades drew
    /// short is put right: once, however often it is asked within moments.
    ///
    /// Every ask is waited for as it is made, on the history thread, rather
    /// than inferred from what the chart was sent: an earlier ask answered
    /// late reads a later clock, and can bring the whole bar itself.
    #[test]
    fn a_reconcile_puts_the_last_bars_right() {
        let two = Timeframe::minutes(2);
        let bar = AT;
        let whole = ohlc(bar - 120, 100.0, 104.0, 98.0, 101.0, 50.0);
        let plant_bars = [ohlc(bar - 240, 99.0, 100.0, 98.0, 100.0, 40.0), whole];
        let (asking, asks) = mpsc::channel();
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(bar - 200));
        let reading = clock.clone();
        let served = Served::start_at(two, Some(bar - 240), bar - 200, move |from| {
            // Each bar fifteen seconds after it closes; the forming one as
            // far as it has got. Told once the clock is read, so an ask
            // seen is one already answered as of that moment.
            let now = reading.load(Ordering::SeqCst);
            let _ = asking.send(from);
            Ok(plant_bars.iter().copied().filter(|bar| bar.ts >= from && bar.ts <= now && (bar.ts + 135 <= now || bar.ts + 120 > now)).collect())
        });
        let asked = || asks.recv_timeout(std::time::Duration::from_secs(10)).expect("an ask");
        let at = |seconds: i64| {
            served.at(seconds);
            clock.store(seconds, Ordering::SeqCst);
        };
        served.watching();
        let mut series = Vec::new();
        drawn(&served, &mut series, |series| !series.is_empty());
        assert_eq!(asked(), bar - 240 - 5 * 120, "the first snapshot");
        // The trades heard of the bar after the snapshot's, short of it:
        // some of its prints never arrived.
        at(bar - 115);
        served.trade(bar - 115, 100.5, 1);
        at(bar + 5);
        served.trade(bar + 5, 101.5, 1);
        drawn(&served, &mut series, |series| series.last().is_some_and(|last| last.ts == bar));
        let short = *series.iter().find(|drawn| drawn.ts == bar - 120).unwrap();
        assert_ne!(short, whole);
        // The bar the snapshot had forming, asked for whole now it has
        // closed: before the plant has the one after it.
        assert_eq!(asked(), bar - 240, "the bar the snapshot had forming");

        at(bar + 20);
        served.reconcile();
        served.reconcile();
        assert_eq!(asked(), bar - RECONCILED * 120, "the last few bars");
        drawn(&served, &mut series, |series| series.contains(&whole));
        // A trade after both, to be sure the second asked nothing.
        served.trade(bar + 21, 101.0, 1);
        drawn(&served, &mut series, |series| series.last().is_some_and(|last| last.close == 101.0));
        assert_eq!(series.last().map(|last| (last.ts, last.open)), Some((bar, 101.5)), "the forming bar left to the trades");
        // Asks are made in order, so the next one tells: a bar later, and
        // long enough after the first to count again, a reconcile asks from
        // the new bar. Had the second of the pair asked, this would hear it.
        at(bar + 125);
        served.trade(bar + 125, 101.25, 1);
        drawn(&served, &mut series, |series| series.last().is_some_and(|last| last.ts == bar + 120));
        served.reconcile();
        assert_eq!(asked(), bar + 120 - RECONCILED * 120, "asked once, however often it is asked within moments");
    }

    #[test]
    fn a_refusal_is_a_sign_in_in_rithmics_words() {
        let mapped = map_err("acme-mapped", rithmic::Error::Refused("permission denied".into()));
        assert!(matches!(&mapped, ProviderError::NeedsSetup(why) if why.contains("permission denied")), "{mapped}");
        assert_eq!(FetchFailure::from(&mapped), FetchFailure::NeedsSignIn);
        let missing = describe(rithmic::Error::NoLogin("no Rithmic user ID is saved".into()));
        assert_eq!(FetchFailure::from(&missing), FetchFailure::NeedsSignIn);
        assert!(matches!(describe(rithmic::Error::Timeout), ProviderError::Network(_)));
        assert!(matches!(describe(rithmic::Error::NotFound("x".into())), ProviderError::NotFound));
        // Remembered for that login's row alone.
        assert_eq!(session::refusal("acme-mapped").as_deref(), Some("permission denied"));
        assert_eq!(session::refusal("demo-unrefused"), None);
    }

    /// Nothing a provider says about a login holds its password: not the
    /// login's `Debug`, not any error built on the way to signing in, not
    /// the settings line.
    #[test]
    fn no_error_or_debug_string_holds_a_planted_password() {
        const PLANTED: &str = "PLANTED-hunter2-PASSWORD";
        let dir = std::env::temp_dir().join(format!("omacharts-rithmic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("login");
        let credentials = rithmic::Credentials {
            user: "trader".into(),
            password: PLANTED.into(),
            system: rithmic::TEST_SYSTEM.into(),
            url: rithmic::TEST_URL.into(),
        };
        let mut texts = vec![format!("{credentials:?}")];

        std::fs::write(&file, format!("trader\n{PLANTED}\n")).unwrap();
        let login = rithmic::Login::from_file(&file, rithmic::TEST_SYSTEM, rithmic::TEST_URL).unwrap();
        texts.push(format!("{login:?}"));

        // A file with the password where the user should be.
        std::fs::write(&file, format!("{PLANTED}\n")).unwrap();
        let error = rithmic::Login::from_file(&file, rithmic::TEST_SYSTEM, rithmic::TEST_URL).unwrap_err();
        texts.push(format!("{error} {error:?}"));
        let provider = describe(error);
        texts.push(format!("{provider} {provider:?}"));

        for error in [
            rithmic::Error::Refused("permission denied".into()),
            rithmic::Error::Connection("connection failed".into()),
            rithmic::Error::Rejected("no data".into()),
            rithmic::Error::NoLogin("the keyring holds no Rithmic password for Rithmic Test".into()),
        ] {
            let mapped = describe(error.clone());
            texts.push(format!("{error} {error:?} {mapped} {mapped:?} {:?}", FetchFailure::from(&mapped)));
        }
        let _ = std::fs::remove_dir_all(&dir);

        for text in texts {
            assert!(!text.contains(PLANTED), "{text}");
        }
    }
}
