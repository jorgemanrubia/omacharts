//! Charts from a Rithmic login: CME futures, front month, live.
//!
//! The protocol is [`rithmic`]'s, and so is the one session every chart
//! shares. This is the thin layer that names what Rithmic serves, asks it for
//! bars, and folds its trades into the bar that is forming.
//!
//! A root, never a contract. `ES` is `ES` here and in the cache key
//! (`rithmic:ES`), and which contract that means today — `ESZ6` — is asked
//! when bars are fetched, on the provider's thread. A roll is then a series
//! whose newest bars disagree with the cached tail of the old contract,
//! which the loader already treats as history rewritten. Front month only,
//! not stitched: a long daily chart starts where the contract does.

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

/// How far back a first fetch asks, in days, and what the capabilities
/// claim. Rithmic keeps about ninety days of intraday history for a
/// contract; one-minute bars ask for a month of that, some forty thousand
/// bars, because a first chart is not worth the rest of the plant's quota.
const MINUTE_DAYS: u32 = 30;
const INTRADAY_DAYS: u32 = 90;
/// Daily bars go back as far as the front month does.
const DAILY_DAYS: i64 = 3_650;

const CAPABILITIES: &[Capability] = &[
    Capability { timeframe: Timeframe::minutes(1), history_days: Some(MINUTE_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(2), history_days: Some(INTRADAY_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(5), history_days: Some(INTRADAY_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(15), history_days: Some(INTRADAY_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(30), history_days: Some(INTRADAY_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(90), history_days: Some(INTRADAY_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::hours(1), history_days: Some(INTRADAY_DAYS), max_request_days: None },
    Capability { timeframe: Timeframe::days(1), history_days: None, max_request_days: None },
];

/// How Rithmic is offered.
pub const LISTED: crate::providers::Listed = crate::providers::Listed {
    id: "rithmic",
    label: "Rithmic",
    serves: "CME futures, front month, from your Rithmic login",
    covers: "CME futures, front month",
    build: || Box::new(Rithmic::new()),
    setup: Some(&session::Signing),
    experimental: false,
    gate: None,
};

/// What a chart says when Rithmic has nobody signed in.
pub const NOT_SIGNED_IN: &str = "Rithmic is not signed in";

pub struct Rithmic;

impl Default for Rithmic {
    fn default() -> Rithmic {
        Rithmic
    }
}

impl Rithmic {
    /// A value with nothing in it: the session belongs to the first fetch.
    pub fn new() -> Rithmic {
        Rithmic
    }
}

/// This instrument in Rithmic's spelling, if Rithmic serves it: a futures
/// root on a CME group exchange, as itself. Pure and offline, because it is
/// asked on the main thread and to build cache keys.
///
/// Once a session has said which exchanges the login is entitled to, a root
/// on any other is not served either, and goes to the next source: Rithmic
/// Test's login sees CME and CBOT, so GC, CL and NG chart from Yahoo there.
pub fn rithmic_symbol(instrument: &Instrument) -> Option<String> {
    symbol_under(instrument, session::entitled)
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

fn period_of(timeframe: Timeframe) -> Result<rithmic::Period, ProviderError> {
    match (timeframe.unit, timeframe.count) {
        (Unit::Minute, count @ (1 | 2 | 5 | 15 | 30 | 90)) => Ok(rithmic::Period::Minutes(count)),
        (Unit::Hour, 1) => Ok(rithmic::Period::Minutes(60)),
        (Unit::Day, 1) => Ok(rithmic::Period::Days(1)),
        _ => Err(ProviderError::Unsupported(format!("{} is folded, not fetched", timeframe.label()))),
    }
}

/// How far back a first fetch at `timeframe` goes, in seconds.
fn depth(timeframe: Timeframe) -> i64 {
    match timeframe.unit {
        Unit::Minute if timeframe.count == 1 => i64::from(MINUTE_DAYS) * 86_400,
        Unit::Minute | Unit::Hour => i64::from(INTRADAY_DAYS) * 86_400,
        _ => DAILY_DAYS * 86_400,
    }
}

/// Rithmic's words, as a provider error. A refusal of the login is
/// remembered for the settings to show.
fn map_err(error: rithmic::Error) -> ProviderError {
    use rithmic::Error;

    match error {
        Error::NoLogin(why) => ProviderError::NeedsSetup(format!("{NOT_SIGNED_IN}: {why}")),
        Error::Refused(why) => {
            session::refused(&why);
            ProviderError::NeedsSetup(format!("Rithmic refused the sign-in: {why}"))
        }
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

/// Bars of `root`'s front month, or of the dated contract `root` names,
/// from `from` on.
///
/// For a root, asks once with the contract it remembers, and again with a
/// fresh answer if that one has nothing: a contract that has rolled off is
/// the likely reason, and the next front month is the cure.
fn front_month_bars(
    client: &rithmic::Client,
    root: &str,
    timeframe: Timeframe,
    from: i64,
) -> Result<Vec<Bar>, ProviderError> {
    let period = period_of(timeframe)?;
    let exchange = exchange_of(root);
    let to = now() + timeframe.seconds();
    if is_dated(root) {
        let bars = client.bars(root, exchange, period, from, to).map_err(map_err)?;
        return Ok(bars.iter().map(bar_of).filter(|bar| bar.ts >= from).collect());
    }
    let contract = client.front_month(root, exchange).map_err(map_err)?;
    let first = client.bars(&contract, exchange, period, from, to);
    let bars = match first {
        Ok(bars) if !bars.is_empty() => bars,
        stale => {
            client.forget_front_month(root, exchange);
            let fresh = client.front_month(root, exchange).map_err(map_err)?;
            if fresh == contract {
                stale.map_err(map_err)?
            } else {
                client.bars(&fresh, exchange, period, from, to).map_err(map_err)?
            }
        }
    };
    Ok(bars.iter().map(bar_of).filter(|bar| bar.ts >= from).collect())
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
/// subscription began: it is asked for again the same way.
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
    /// Bars that may be missing trades, by start: asked for again once they
    /// have closed.
    partial: Vec<i64>,
    /// The next new bar is partial.
    fresh: bool,
    /// A snapshot has arrived: until one has, there is nothing to fold into.
    ready: bool,
    /// Snapshots asked for and not yet answered.
    loading: usize,
    /// Trades heard while a snapshot was being fetched. The snapshot may be
    /// older than the newest of them, so they are folded onto it again.
    heard: Vec<rithmic::Trade>,
}

impl Fold {
    fn new(timeframe: Timeframe, root: &str) -> Fold {
        Fold {
            timeframe,
            root: root.to_string(),
            floor: None,
            forming: None,
            counted: None,
            partial: Vec::new(),
            fresh: true,
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

    /// Start again from a snapshot the plant answered at `answered`, in
    /// nanoseconds. The forming bar, if the trades heard while it was
    /// fetched moved it past what the snapshot says.
    fn snapshot(&mut self, bars: &[Bar], answered: i64) -> Option<Bar> {
        self.loading = self.loading.saturating_sub(1);
        self.ready = true;
        self.forming = None;
        self.counted = None;
        let now = bucket(answered.div_euclid(1_000_000_000), self.timeframe, &self.root);
        match bars.last() {
            Some(last) if last.ts == now => {
                self.floor = Some(last.ts);
                self.forming = Some(*last);
                self.counted = Some((last.ts, answered));
                if !self.partial.contains(&last.ts) {
                    self.partial.push(last.ts);
                }
                self.fresh = false;
            }
            last => {
                self.floor = last.map(|bar| bar.ts + 1);
                self.fresh = true;
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
            }
            // A late trade for a bar already behind us: the bar it belongs to
            // is asked for whole once it has closed, so it is dropped here.
            Some(bar) if start < bar.ts => return None,
            _ => {
                if std::mem::take(&mut self.fresh) {
                    self.partial.push(start);
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

    /// Partial bars that closed long enough ago for the history plant to
    /// have them whole, taken off the list.
    fn due(&mut self, now: i64) -> Vec<i64> {
        // The plant has a closed minute a few seconds after it closes.
        const SETTLED: i64 = 30;
        let length = self.timeframe.seconds();
        let (due, waiting): (Vec<i64>, Vec<i64>) =
            self.partial.iter().partition(|start| **start + length + SETTLED <= now);
        self.partial = waiting;
        due
    }
}

/// What the subscription's thread is told: what the session hears, and what
/// its history thread fetched.
enum Event {
    Live(rithmic::Live),
    /// A snapshot, and when the plant answered, in nanoseconds.
    Loaded { bars: Result<Vec<Bar>, ProviderError>, at: i64 },
    /// A partial bar, asked for again once it closed, as the plant has it.
    Whole(Bar),
    Stop,
}

/// What the history thread is asked to fetch.
enum Job {
    /// A snapshot, from this time on.
    Snapshot(i64),
    /// The bar starting then, whole.
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

impl Subscription for Live {}

impl Drop for Live {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.events.send(Event::Stop);
    }
}

fn failure(error: rithmic::Error) -> FetchFailure {
    FetchFailure::from(&map_err(error))
}

/// One subscription's life, on a thread of its own: watch the front month,
/// and hand what the session hears to [`serve`].
fn stream(
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
    let client = match session::client() {
        Ok(client) => client,
        Err(error) => return say(Update::Lost(failure(error))),
    };
    let heard = sender.clone();
    let hear = move |live| {
        let _ = heard.send(Event::Live(live));
    };
    let _watch = match is_dated(&root) {
        true => client.watch_contract(&root, exchange_of(&root), hear),
        false => client.watch(&root, exchange_of(&root), hear),
    };
    let load: Load = {
        let root = root.clone();
        Box::new(move |from: i64| front_month_bars(&client, &root, timeframe, from))
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
                let bars = load(from);
                Event::Loaded { bars, at: clock() }
            }
            Job::Whole(start) => {
                match load(start).ok().and_then(|bars| bars.into_iter().find(|bar| bar.ts == start)) {
                    Some(whole) => Event::Whole(whole),
                    None => continue,
                }
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
            Event::Loaded { bars: Ok(bars), at } => {
                served = true;
                let moved = fold.snapshot(&bars, at);
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
            Event::Whole(whole) => say(Update::Bar(whole)),
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
            Event::Live(rithmic::Live::Lost(error)) => say(Update::Lost(failure(error))),
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
        std::thread::Builder::new()
            .name("omacharts rithmic stream".into())
            .spawn(move || stream(root, timeframe, since, sink, stopping, inbox, sender))
            .map_err(|error| ProviderError::Network(error.to_string()))?;
        Ok(Box::new(Live { stop, events }))
    }
}

impl Provider for Rithmic {
    fn id(&self) -> &'static str {
        "rithmic"
    }

    fn label(&self) -> &'static str {
        "Rithmic"
    }

    /// The login's own feed, as it prints.
    fn delay_minutes(&self, _kind: InstrumentKind) -> u32 {
        0
    }

    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        rithmic_symbol(instrument)
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        period_of(timeframe)?;
        if !session::here() {
            return Err(ProviderError::NeedsSetup(
                "Rithmic charts are served by the open window, which holds the one session".into(),
            ));
        }
        let client = session::client().map_err(map_err)?;
        let from = since.unwrap_or_else(|| now() - depth(timeframe));
        front_month_bars(&client, symbol, timeframe, from)
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
        let client = session::client().map_err(map_err)?;
        let listed = client.listed(root, exchange_of(root)).map_err(map_err)?;
        Ok(listed
            .into_iter()
            .filter_map(|(symbol, (year, month, day))| Some((symbol, chrono::NaiveDate::from_ymd_opt(year, month, day)?)))
            .collect())
    }

    /// The window is done with Rithmic: the session logs out of every
    /// plant. A chart that asks again later signs in again.
    fn retire(&self) {
        rithmic::shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instrument(kind: InstrumentKind, symbol: &str) -> Instrument {
        Instrument { symbol: symbol.into(), kind, ..placeholder() }
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
        assert!(session::entitled("NYMEX") || rithmic::entitled_exchanges().is_some(), "unknown means entitled");
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
        assert_eq!(fold.snapshot(&snapshot, 690 * NANOS), None);

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
        fold.snapshot(&[ohlc(600, 1.0, 1.0, 1.0, 1.0, 1.0)], 665 * NANOS);
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
        let moved = fold.snapshot(&[ohlc(660, 3.0, 4.0, 2.0, 3.0, 10.0)], 675 * NANOS).unwrap();
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
                let fold = Fold::new(Timeframe::minutes(1), "ES");
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

    /// The test that matters most about errors: a login refused, or
    /// missing, is something to sign in again about rather than a network
    /// failure to sit out; and none of them can carry a password.
    #[test]
    fn a_refusal_is_a_sign_in_in_rithmics_words() {
        let mapped = map_err(rithmic::Error::Refused("permission denied".into()));
        assert!(matches!(&mapped, ProviderError::NeedsSetup(why) if why.contains("permission denied")), "{mapped}");
        assert_eq!(FetchFailure::from(&mapped), FetchFailure::NeedsSignIn);
        let missing = map_err(rithmic::Error::NoLogin("no Rithmic user ID is saved".into()));
        assert_eq!(FetchFailure::from(&missing), FetchFailure::NeedsSignIn);
        assert!(matches!(map_err(rithmic::Error::Timeout), ProviderError::Network(_)));
        assert!(matches!(map_err(rithmic::Error::NotFound("x".into())), ProviderError::NotFound));
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
        let provider = map_err(error);
        texts.push(format!("{provider} {provider:?}"));

        for error in [
            rithmic::Error::Refused("permission denied".into()),
            rithmic::Error::Connection("connection failed".into()),
            rithmic::Error::Rejected("no data".into()),
            rithmic::Error::NoLogin("the keyring holds no Rithmic password for Rithmic Test".into()),
        ] {
            let mapped = map_err(error.clone());
            texts.push(format!("{error} {error:?} {mapped} {mapped:?} {:?}", FetchFailure::from(&mapped)));
        }
        let _ = std::fs::remove_dir_all(&dir);

        for text in texts {
            assert!(!text.contains(PLANTED), "{text}");
        }
    }
}
