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
    setup: Some(&session::SETUP),
    experimental: false,
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
pub fn rithmic_symbol(instrument: &Instrument) -> Option<String> {
    if let Some(explicit) = instrument.override_for("rithmic") {
        return Some(explicit.to_string());
    }
    if instrument.kind != InstrumentKind::FutureRoot {
        return None;
    }
    let exchange = registry().contract_exchange(&instrument.symbol)?;
    CME_GROUP.contains(&exchange).then(|| instrument.symbol.clone())
}

/// The exchange Rithmic is asked on for a root. A root only an override
/// names is taken to be on CME.
fn exchange_of(root: &str) -> &'static str {
    let listed = registry().contract_exchange(root);
    CME_GROUP.into_iter().find(|exchange| Some(*exchange) == listed).unwrap_or("CME")
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

/// Bars of `root`'s front month, from `from` on.
///
/// Asks once with the contract it remembers, and again with a fresh answer
/// if that one has nothing: a contract that has rolled off is the likely
/// reason, and the next front month is the cure.
fn front_month_bars(
    client: &rithmic::Client,
    root: &str,
    timeframe: Timeframe,
    from: i64,
) -> Result<Vec<Bar>, ProviderError> {
    let period = period_of(timeframe)?;
    let exchange = exchange_of(root);
    let to = now() + timeframe.seconds();
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
        let instrument = Instrument { symbol: root.to_string(), kind: InstrumentKind::FutureRoot, ..placeholder() };
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
/// Fed a snapshot first, which holds only bars that have closed: the plant
/// sends nothing for the minute in progress. A trade in a bar the snapshot
/// already holds is one it counted, and is left out. The first bar built
/// after a snapshot may have missed trades from before the subscription
/// began, so once it has closed it is asked for again, whole, and replaces
/// what was folded.
struct Fold {
    timeframe: Timeframe,
    root: String,
    /// The newest bar the snapshot held. Trades at or before it are in it.
    floor: Option<i64>,
    forming: Option<Bar>,
    /// Bars that may be missing trades, by start: asked for again once they
    /// have closed.
    partial: Vec<i64>,
    /// After a snapshot, the next new bar is partial.
    fresh: bool,
}

impl Fold {
    fn new(timeframe: Timeframe, root: &str) -> Fold {
        Fold { timeframe, root: root.to_string(), floor: None, forming: None, partial: Vec::new(), fresh: true }
    }

    /// Start again from a snapshot.
    fn snapshot(&mut self, bars: &[Bar]) {
        self.floor = bars.last().map(|bar| bar.ts);
        self.forming = None;
        self.fresh = true;
    }

    /// Fold one trade in. The bar it changed, if it changed one.
    fn trade(&mut self, trade: &rithmic::Trade) -> Option<Bar> {
        let ts = trade.at_nanos.div_euclid(1_000_000_000);
        let start = bucket(ts, self.timeframe, &self.root);
        if self.floor.is_some_and(|floor| start <= floor) {
            return None;
        }
        let volume = trade.size as f64;
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

/// What the subscription's thread is told.
enum Event {
    Live(rithmic::Live),
    Stop,
}

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
    let _watch = client.watch(&root, exchange_of(&root), move |live| {
        let _ = sender.send(Event::Live(live));
    });
    let mut load = |from: i64| front_month_bars(&client, &root, timeframe, from);
    serve(&events, Fold::new(timeframe, &root), since, &mut load, &say, &stop);
}

/// Turn what the session hears into updates: a snapshot each time it says
/// it is watching — which it says again after every reconnect, and after
/// trades were missed — the forming bar on each trade, and a loss as a
/// loss. `load` fetches bars from a time on; `since` is what the chart
/// already holds.
fn serve(
    events: &mpsc::Receiver<Event>,
    mut fold: Fold,
    since: Option<i64>,
    load: &mut dyn FnMut(i64) -> Result<Vec<Bar>, ProviderError>,
    say: &dyn Fn(Update),
    stop: &AtomicBool,
) {
    let timeframe = fold.timeframe;
    // What the chart holds already, and then what this has sent it.
    let mut newest = since;
    for event in events {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match event {
            Event::Stop => return,
            Event::Live(rithmic::Live::Watching { .. }) => {
                // From a few bars back, so a gap left by a drop is filled.
                let from = match newest {
                    Some(newest) => newest - 5 * timeframe.seconds(),
                    None => now() - depth(timeframe),
                };
                match load(from) {
                    Ok(bars) => {
                        fold.snapshot(&bars);
                        if let Some(last) = bars.last() {
                            newest = Some(newest.map_or(last.ts, |held| held.max(last.ts)));
                        }
                        say(Update::Snapshot(bars));
                    }
                    Err(error) => say(Update::Lost(FetchFailure::from(&error))),
                }
            }
            Event::Live(rithmic::Live::Trade(trade)) => {
                if let Some(bar) = fold.trade(&trade) {
                    newest = Some(newest.map_or(bar.ts, |held| held.max(bar.ts)));
                    say(Update::Bar(bar));
                }
                for start in fold.due(now()) {
                    let whole = load(start).ok().and_then(|bars| bars.into_iter().find(|bar| bar.ts == start));
                    if let Some(whole) = whole {
                        say(Update::Bar(whole));
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

    /// Trades fold into the minute they print in; one in a minute the
    /// snapshot already holds is already counted.
    #[test]
    fn trades_fold_into_the_bar_that_is_forming() {
        let minute = Timeframe::minutes(1);
        let mut fold = Fold::new(minute, "ES");
        let held = Bar { ts: 600, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        fold.snapshot(&[held]);
        assert_eq!(fold.trade(&trade(630, 5.0, 2)), None, "the snapshot's own minute");

        let first = fold.trade(&trade(661, 10.0, 2)).unwrap();
        assert_eq!((first.ts, first.open, first.volume), (660, 10.0, 2.0));
        let second = fold.trade(&trade(700, 12.0, 1)).unwrap();
        assert_eq!((second.ts, second.high, second.close, second.volume), (660, 12.0, 12.0, 3.0));
        let low = fold.trade(&trade(719, 9.0, 1)).unwrap();
        assert_eq!((low.low, low.close), (9.0, 9.0));

        let next = fold.trade(&trade(720, 11.0, 4)).unwrap();
        assert_eq!((next.ts, next.open, next.volume), (720, 11.0, 4.0));
        assert_eq!(fold.trade(&trade(700, 50.0, 1)), None, "a trade for a closed bar");

        // The first bar after the snapshot may have missed trades, and is
        // asked for whole once the plant has it; the next one is whole.
        assert!(fold.due(725).is_empty());
        assert_eq!(fold.due(720 + 30), vec![660]);
        assert!(fold.due(10_000).is_empty());
    }

    /// A drop is a loss, and the session's coming back is a fresh snapshot
    /// from a few bars before the newest the chart was sent, which fills the
    /// gap the drop left.
    #[test]
    fn a_drop_is_a_loss_and_coming_back_is_a_fresh_snapshot() {
        let minute = Timeframe::minutes(1);
        let bar = |ts: i64| Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        let (send, events) = mpsc::channel();
        let watching = || Event::Live(rithmic::Live::Watching { contract: "ESZ6".into() });
        let at = 1_800_000_000 / 60 * 60;
        send.send(watching()).unwrap();
        send.send(Event::Live(rithmic::Live::Trade(trade(at + 61, 2.0, 3)))).unwrap();
        send.send(Event::Live(rithmic::Live::Lost(rithmic::Error::Connection("connection closed".into()))))
            .unwrap();
        send.send(watching()).unwrap();
        send.send(Event::Stop).unwrap();
        send.send(watching()).unwrap();

        let asked = std::cell::RefCell::new(Vec::new());
        let mut load = |from: i64| {
            asked.borrow_mut().push(from);
            Ok(vec![bar(at)])
        };
        let heard = std::cell::RefCell::new(Vec::new());
        let say = |update: Update| heard.borrow_mut().push(update);
        serve(&events, Fold::new(minute, "ES"), Some(at - 600), &mut load, &say, &AtomicBool::new(false));

        let heard = heard.into_inner();
        assert!(matches!(&heard[0], Update::Snapshot(bars) if bars.len() == 1), "{heard:?}");
        assert!(matches!(&heard[1], Update::Bar(bar) if bar.ts == at + 60 && bar.volume == 3.0), "{heard:?}");
        assert_eq!(heard[2], Update::Lost(FetchFailure::Unreachable));
        assert!(matches!(&heard[3], Update::Snapshot(_)), "{heard:?}");
        assert_eq!(heard.len(), 4, "nothing after the stop: {heard:?}");
        // The first snapshot goes back from what the chart held, the second
        // from the bar it was last sent.
        assert_eq!(asked.into_inner(), vec![at - 600 - 300, at + 60 - 300]);
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
