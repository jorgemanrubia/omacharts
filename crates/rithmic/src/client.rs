//! The data half of a login's connection: the ticker plant for live prices
//! and reference data, and the history plant for bars.
//!
//! A [`Client`] is a handle on it, lent by [`Connection::data`]. Its calls
//! block the caller, never the connection: each is a request handed to the
//! runtime and waited for, with a deadline. The plants themselves belong to
//! the connection's supervisor, which signs in, notices a drop, reconnects
//! and subscribes again to whatever is still watched.
//!
//! [`Connection::data`]: crate::Connection::data

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::request_search_symbols::{InstrumentType, Pattern};
use rithmic_rs::rti::{FrontMonthContractUpdate, LastTrade, OrderPriceLimits, ResponseReferenceData};
use rithmic_rs::{
    ConnectStrategy, RithmicConfig, RithmicEnv, RithmicError, RithmicHistoryPlant,
    RithmicHistoryPlantHandle, RithmicResponse, RithmicTickerPlant, RithmicTickerPlantHandle,
    TimeBarType, VolumeProfileMinuteBarsRequest,
};
use tokio::sync::{broadcast, mpsc, watch};

use crate::connection::{Command, Inner, Phase};
use crate::{Bar, Error, LadderMinute, Live, Period, Side, Trade, App, Credentials};

/// How long a call waits for the data half to be signed in before it gives
/// up and says why it is not.
const PATIENCE: Duration = Duration::from_secs(20);

/// How long one ordinary request may take.
const REQUEST: Duration = Duration::from_secs(15);

/// How long a replay of bars may take. The history plant answers a large
/// window in pieces, and the whole of it can take a while; past this it is
/// not coming.
const REPLAY: Duration = Duration::from_secs(120);

/// How long a front month is believed before it is asked for again. A roll
/// happens once a quarter; asking once an hour costs nothing.
const FRONT_FRESH: Duration = Duration::from_secs(3_600);

/// When Rithmic will not say which contract is the front month and nobody
/// above has said how to choose, the nearest to expire is, until this many
/// days before it expires. Traders roll the quarterly index contracts about
/// eight days early.
const ROLL_DAYS: i64 = 8;

pub(crate) type Sink = Arc<dyn Fn(Live) + Send + Sync>;

/// When a contract expires: its year, month and day.
pub type Expiry = (i32, u32, u32);

/// The plants' handles, for requests from any thread. A handle's methods
/// take `&self`, so one is shared rather than one made per call.
pub(crate) struct Handles {
    ticker: RithmicTickerPlantHandle,
    history: RithmicHistoryPlantHandle,
}

/// Where the data half stands, under the generation it is open as.
pub(crate) type Told = (u64, State);

#[derive(Clone)]
pub(crate) enum State {
    /// Signing in for the first time.
    Connecting,
    /// Signed in. No handles only in a test, whose plants are fakes.
    Up(Option<Arc<Handles>>),
    /// Lost, and reconnecting by itself.
    Down(Error),
    /// Will not try again by itself: refused, nothing to sign in with, or
    /// signed out.
    Stopped(Error),
}

/// Front months already asked for, by root and exchange.
#[derive(Default)]
pub(crate) struct Fronts(Mutex<HashMap<(String, String), (String, Instant)>>);

impl Fronts {
    pub(crate) fn get(&self, root: &str, exchange: &str) -> Option<String> {
        let fronts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let (contract, at) = fronts.get(&(root.to_string(), exchange.to_string()))?;
        (at.elapsed() < FRONT_FRESH).then(|| contract.clone())
    }

    /// Remembered here, and in [`FOUND`] for the trading half; and said to
    /// whoever listens, when it is news.
    fn put(&self, root: &str, exchange: &str, contract: &str) {
        let key = (root.to_string(), exchange.to_string());
        self.0.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (contract.to_string(), Instant::now()));
        found(root, exchange, contract);
    }

    /// The roots asked about on `exchange` and still remembered.
    fn roots(&self, exchange: &str) -> Vec<String> {
        let fronts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        fronts.keys().filter(|(_, on)| on == exchange).map(|(root, _)| root.clone()).collect()
    }

    fn forget(&self, root: &str, exchange: &str) {
        let key = (root.to_string(), exchange.to_string());
        self.0.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
        if let Some(found) = FOUND.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            found.remove(&key);
        }
    }

    fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Every front month found in this process, by root and exchange, and when:
/// kept when the data half that asked closes or drops, because an order on
/// a chart fed from elsewhere goes to it all the same.
static FOUND: Mutex<Option<Found>> = Mutex::new(None);
type Found = HashMap<(String, String), (String, Instant)>;

/// The front month last found for `root` on `exchange` by any connection,
/// and whether it is fresh enough not to ask again.
pub(crate) fn found_front_month(root: &str, exchange: &str) -> Option<(String, bool)> {
    let found = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    let (contract, at) = found.as_ref()?.get(&(root.to_string(), exchange.to_string()))?;
    Some((contract.clone(), at.elapsed() < FRONT_FRESH))
}

/// `contract` is `root`'s front month on `exchange`, found just now: asked,
/// chosen from a listing, or pushed by Rithmic. Kept in [`FOUND`], fresh,
/// and told to every [`on_front_month`] listener when it was not already
/// what was known.
fn found(root: &str, exchange: &str, contract: &str) {
    let news = {
        let mut found = FOUND.lock().unwrap_or_else(|e| e.into_inner());
        let key = (root.to_string(), exchange.to_string());
        let was = found.get_or_insert_with(HashMap::new).insert(key, (contract.to_string(), Instant::now()));
        was.is_none_or(|(known, _)| known != contract)
    };
    if !news {
        return;
    }
    let listeners = FRONT_MONTH_LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for listener in listeners {
        listener(root, exchange, contract);
    }
}

type FrontMonthListener = Arc<dyn Fn(&str, &str, &str) + Send + Sync>;
static FRONT_MONTH_LISTENERS: Mutex<Vec<FrontMonthListener>> = Mutex::new(Vec::new());

/// Call `listener` with a root, its exchange and the contract that is its
/// front month, whenever that becomes known or changes in this process:
/// asked of Rithmic, chosen from its listing, or pushed by it on a roll.
/// On whichever thread finds it. Every listener is kept and told.
pub fn on_front_month(listener: impl Fn(&str, &str, &str) + Send + Sync + 'static) {
    FRONT_MONTH_LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::new(listener));
}

type Chooser = Arc<dyn Fn(&str, &str, &[(String, Expiry)]) -> Option<String> + Send + Sync>;
static CHOOSER: Mutex<Option<Chooser>> = Mutex::new(None);

/// When Rithmic will not say which contract is a root's front month, hand
/// its listing of the root's contracts, each with its expiry, to `chooser`,
/// and take the contract it names. Rithmic Test says "no data" for every
/// root. A chooser that returns `None` leaves the choice to this crate: the
/// nearest to expire, until eight days before it does. One chooser per
/// process; a second replaces the first.
pub fn choose_front_month_with(
    chooser: impl Fn(&str, &str, &[(String, Expiry)]) -> Option<String> + Send + Sync + 'static,
) {
    *CHOOSER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(chooser));
}

/// The front month among `listed`, by the chooser installed above, or
/// failing that the nearest to expire.
fn chosen(root: &str, exchange: &str, listed: &[(String, Expiry)]) -> Result<String, Error> {
    let chooser = CHOOSER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    chooser
        .and_then(|choose| choose(root, exchange, listed))
        .or_else(|| nearest(listed, now_seconds()))
        .ok_or_else(|| Error::NotFound(format!("contract of {root} on {exchange} trading now")))
}

/// The exchange's price limits for the day on a contract: the lowest and
/// highest price it takes an order at, as the data half last heard them
/// for a contract it watches. Kept when the data half closes, as the day's
/// limits do not change with it.
pub fn price_limits(contract: &str, exchange: &str) -> (Option<f64>, Option<f64>) {
    let limits = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    limits.as_ref().and_then(|limits| limits.get(&(contract.to_string(), exchange.to_string())).copied()).unwrap_or_default()
}

static LIMITS: Mutex<Option<Limits>> = Mutex::new(None);
type Limits = HashMap<(String, String), (Option<f64>, Option<f64>)>;

/// Price limits pushed for a contract: what is present replaces what was,
/// what is cleared is no longer known.
pub(crate) fn heard_limits(heard: &OrderPriceLimits) {
    let (Some(symbol), Some(exchange)) = (heard.symbol.clone(), heard.exchange.clone()) else { return };
    const HIGH: u32 = 1;
    const LOW: u32 = 2;
    let present = heard.presence_bits.unwrap_or(0);
    let cleared = heard.clear_bits.unwrap_or(0);
    let mut limits = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    let (low, high) = limits.get_or_insert_with(HashMap::new).entry((symbol, exchange)).or_default();
    let pick = |bit: u32, now: Option<f64>, was: &mut Option<f64>| {
        if cleared & bit != 0 {
            *was = None;
        } else if (present & bit != 0 || present == 0) && now.is_some_and(|price| price > 0.0) {
            *was = now;
        }
    };
    pick(HIGH, heard.high_price_limit, high);
    pick(LOW, heard.low_price_limit, low);
}

/// What Rithmic says a contract is: the step its price moves in, what a
/// point of it is worth, and in which currency.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reference {
    pub tick: Option<f64>,
    pub point: Option<f64>,
    pub currency: Option<String>,
}

impl Reference {
    fn of(data: &ResponseReferenceData) -> Reference {
        let positive = |value: Option<f64>| value.filter(|value| value.is_finite() && *value > 0.0);
        Reference {
            tick: positive(data.min_qprice_change),
            point: positive(data.single_point_value),
            currency: data.currency.clone().filter(|currency| !currency.is_empty()),
        }
    }
}

type ReferenceListener = Arc<dyn Fn(&str, &str, &Reference) + Send + Sync>;
static REFERENCES: Mutex<Option<HashMap<(String, String), Reference>>> = Mutex::new(None);
static REFERENCE_LISTENER: Mutex<Option<ReferenceListener>> = Mutex::new(None);

/// Call `listener` with a contract, its exchange and what Rithmic says it
/// is, the first time the data half hears it. One listener per process.
pub fn on_reference(listener: impl Fn(&str, &str, &Reference) + Send + Sync + 'static) {
    *REFERENCE_LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(listener));
}

/// Has the contract's reference data been heard in this process? It is
/// asked once: a contract's specification does not change.
fn referenced(contract: &str, exchange: &str) -> bool {
    let all = REFERENCES.lock().unwrap_or_else(|e| e.into_inner());
    all.as_ref().is_some_and(|all| all.contains_key(&(contract.to_string(), exchange.to_string())))
}

pub(crate) fn heard_reference(contract: &str, exchange: &str, reference: Reference) {
    REFERENCES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert((contract.to_string(), exchange.to_string()), reference.clone());
    let listener = REFERENCE_LISTENER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(listener) = listener {
        listener(contract, exchange, &reference);
    }
}

/// The exchanges the signed-in login has level 1 market data for, once the
/// data half has asked: `None` before the first sign-in, and after a
/// shutdown.
pub fn entitled_exchanges() -> Option<Vec<String>> {
    ENTITLED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Call `listener` whenever [`entitled_exchanges`] changes, on the
/// connection's thread. One listener per process; a second replaces the
/// first.
pub fn on_entitlements(listener: impl Fn() + Send + Sync + 'static) {
    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(listener));
}

static ENTITLED: Mutex<Option<Vec<String>>> = Mutex::new(None);
type Listener = Arc<dyn Fn() + Send + Sync>;
static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);

pub(crate) fn forget_entitlements() {
    *ENTITLED.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn entitle(exchanges: Vec<String>) {
    let changed = {
        let mut held = ENTITLED.lock().unwrap_or_else(|e| e.into_inner());
        let changed = held.as_ref() != Some(&exchanges);
        *held = Some(exchanges);
        changed
    };
    let listener = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if changed && let Some(listener) = listener {
        listener();
    }
}

/// The exchanges a permissions reply grants level 1 market data on, sorted.
fn exchanges_of(responses: &[RithmicResponse]) -> Vec<String> {
    let mut exchanges: Vec<String> = responses
        .iter()
        .filter_map(|response| match &response.message {
            RithmicMessage::ResponseListExchangePermissions(found) => {
                let level_1 = found.level_1_market_data.as_deref()?;
                granted(level_1).then(|| found.exchange.clone()).flatten()
            }
            _ => None,
        })
        .collect();
    exchanges.sort();
    exchanges.dedup();
    exchanges
}

/// Does an entitlement status say yes? Rithmic spells it as a word.
fn granted(status: &str) -> bool {
    let status = status.trim().to_ascii_lowercase();
    matches!(status.as_str(), "enabled" | "enable" | "true" | "yes" | "1" | "granted" | "allowed")
}

/// A login's market data. Cheap to clone; every clone is the same data
/// half, until it is closed: a client from before a close stays closed.
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
    pub(crate) generation: u64,
}

impl Client {
    pub(crate) fn of(inner: Arc<Inner>, generation: u64) -> Client {
        Client { inner, generation }
    }

    fn open(&self) -> bool {
        self.inner.data_open(self.generation)
    }

    /// Wait for the data half to be signed in, or say why it is not.
    fn up(&self) -> Result<Arc<Handles>, Error> {
        if !self.open() {
            return Err(Error::Closed);
        }
        let generation = self.generation;
        let mut states = self.inner.data.clone();
        let state = self.inner.handle.block_on(async move {
            let settled = tokio::time::timeout(
                PATIENCE,
                states.wait_for(|(told, state)| {
                    *told == generation && matches!(state, State::Up(_) | State::Stopped(_))
                }),
            )
            .await
            .map(|waited| waited.map(|told| told.1.clone()));
            match settled {
                Ok(Ok(state)) => Ok(state),
                Ok(Err(_)) => Err(Error::Closed),
                Err(_) => Ok(match &*states.borrow() {
                    (told, state) if *told == generation => state.clone(),
                    _ => State::Connecting,
                }),
            }
        })?;
        // Closed while this waited.
        if !self.open() {
            return Err(Error::Closed);
        }
        match state {
            State::Up(Some(handles)) => Ok(handles),
            State::Up(None) => Err(Error::Closed),
            State::Stopped(error) | State::Down(error) => Err(error),
            State::Connecting => Err(Error::Timeout),
        }
    }

    /// Sign in now, if the data half is not already, and wait for it.
    pub fn sign_in(&self) -> Result<(), Error> {
        self.up().map(|_| ())
    }

    /// The contract trading now for a futures root, such as `ESZ6` for `ES`
    /// on `CME`. Asked once and remembered for an hour, or until
    /// [`Client::forget_front_month`].
    pub fn front_month(&self, root: &str, exchange: &str) -> Result<String, Error> {
        if let Some(contract) = self.inner.fronts.get(root, exchange) {
            return Ok(contract);
        }
        let handles = self.up()?;
        let contract = self.inner.handle.block_on(front_month(&handles.ticker, root, exchange))?;
        self.inner.fronts.put(root, exchange, &contract);
        Ok(contract)
    }

    /// Ask again next time: a request for the remembered contract failed,
    /// which is what a roll looks like from here.
    pub fn forget_front_month(&self, root: &str, exchange: &str) {
        self.inner.fronts.forget(root, exchange);
    }

    /// The contracts listed for a futures root on `exchange`, each with the
    /// day it expires, in no particular order: what a chooser installed with
    /// [`choose_front_month_with`] is handed.
    pub fn listed(&self, root: &str, exchange: &str) -> Result<Vec<(String, Expiry)>, Error> {
        let handles = self.up()?;
        self.inner.handle.block_on(listed(&handles.ticker, root, exchange))
    }

    /// Bars of `contract` from `from` to `to`, in unix seconds, oldest first.
    ///
    /// The whole window in one request however long it is: the history
    /// plant caps a reply at 10,000 bars unless asked not to, and this asks.
    /// Daily bars are asked for by date, which is how the plant takes them.
    /// Seconds are `i32` on the wire, so a time past 2038 is clamped.
    pub fn bars(&self, contract: &str, exchange: &str, period: Period, from: i64, to: i64) -> Result<Vec<Bar>, Error> {
        let handles = self.up()?;
        let (kind, count, start, end) = match period {
            Period::Minutes(count) => (TimeBarType::MinuteBar, count, wire_seconds(from), wire_seconds(to)),
            Period::Days(count) => (TimeBarType::DailyBar, count, date_number(from), date_number(to)),
        };
        let count = i32::try_from(count.max(1)).unwrap_or(1);
        let replay = handles.history.load_time_bars_all(
            contract.to_string(),
            exchange.to_string(),
            kind,
            count,
            start,
            end,
        );
        let responses = self.inner.handle.block_on(async {
            match tokio::time::timeout(REPLAY, replay).await {
                Ok(result) => result.map_err(error_of),
                Err(_) => Err(Error::Timeout),
            }
        })?;
        bars_of(&responses, period)
    }

    /// Minute ladders of `contract` for minutes opening in `from..to`, unix
    /// seconds, oldest first: the history plant's volume-profile minute
    /// bars, bid and ask volume and trade counts at each price.
    ///
    /// One request, however long the window: the plant continues a reply it
    /// cut short by itself. Rithmic keeps a data quota, so a caller asks for
    /// a span once and keeps it.
    pub fn ladders(&self, contract: &str, exchange: &str, from: i64, to: i64) -> Result<Vec<LadderMinute>, Error> {
        let handles = self.up()?;
        let request = VolumeProfileMinuteBarsRequest::new()
            .symbol(contract)
            .exchange(exchange)
            .bar_type_period(1)
            .start_time_sec(wire_seconds(from))
            .end_time_sec(wire_seconds(to))
            .resume_bars(true)
            .build()
            .map_err(error_of)?;
        let replay = handles.history.load_volume_profile_minute_bars(request);
        let responses = self.inner.handle.block_on(async {
            match tokio::time::timeout(REPLAY, replay).await {
                Ok(result) => result.map_err(error_of),
                Err(_) => Err(Error::Timeout),
            }
        })?;
        let minutes = ladders_of(&responses)?;
        Ok(minutes.into_iter().filter(|m| (from..to).contains(&m.start)).collect())
    }

    /// Cut the connection the way a dropped network does, from underneath:
    /// every plant of the login is aborted, and the supervisor finds out as
    /// it would about a real drop, reports it and reconnects. For proving
    /// that a chart survives one; nothing in the app calls it.
    #[doc(hidden)]
    pub fn sever(&self) {
        let _ = self.inner.commands.send(Command::Sever);
    }

    /// Watch `root`'s front month on `exchange`: every trade on it is handed
    /// to `sink`, on the connection's thread, until the [`Watch`] is dropped.
    ///
    /// Returns at once. The first thing `sink` hears is [`Live::Watching`]
    /// once the contract is subscribed, or [`Live::Lost`] saying why it could
    /// not be. A watch outlives a dropped connection: it hears `Lost`, and
    /// `Watching` again once the data half is back. And it outlives a roll:
    /// when Rithmic says the front month is now another contract, the watch
    /// moves to it and hears `Watching` with the new one.
    pub fn watch(&self, root: &str, exchange: &str, sink: impl Fn(Live) + Send + Sync + 'static) -> Watch {
        self.watch_subject(Subject::Front { root: root.to_string() }, exchange, Arc::new(sink))
    }

    /// Watch `contract` itself on `exchange`, whether or not it is anything's
    /// front month: nothing is asked, and a roll leaves it where it is. As
    /// [`Client::watch`] otherwise, `Watching` first.
    pub fn watch_contract(&self, contract: &str, exchange: &str, sink: impl Fn(Live) + Send + Sync + 'static) -> Watch {
        self.watch_subject(Subject::Contract { contract: contract.to_string() }, exchange, Arc::new(sink))
    }

    fn watch_subject(&self, subject: Subject, exchange: &str, sink: Sink) -> Watch {
        let id = self.inner.next_watch.fetch_add(1, Ordering::Relaxed);
        if !self.open() {
            sink(Live::Lost(Error::Closed));
        } else {
            let _ = self.inner.commands.send(Command::Watch {
                generation: self.generation,
                id,
                subject,
                exchange: exchange.to_string(),
                sink,
            });
        }
        Watch { id, commands: self.inner.commands.clone() }
    }
}

/// A watch on a contract's trades. Dropping it unsubscribes, and never
/// blocks.
pub struct Watch {
    id: u64,
    commands: mpsc::UnboundedSender<Command>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Unwatch { id: self.id });
    }
}

/// What a watch is on: a root's front month, whichever contract that is at
/// the time, or one contract for good.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Subject {
    Front { root: String },
    Contract { contract: String },
}

pub(crate) struct Watched {
    pub(crate) subject: Subject,
    pub(crate) exchange: String,
    pub(crate) sink: Sink,
    /// The contract it is subscribed to, once it is.
    pub(crate) contract: Option<String>,
    /// Told of a loss it has not yet been told is over.
    pub(crate) told_lost: bool,
}

impl Watched {
    pub(crate) fn new(subject: Subject, exchange: String, sink: Sink, told_lost: bool) -> Watched {
        Watched { subject, exchange, sink, contract: None, told_lost }
    }

    /// Is this a watch on `root`'s front month on `exchange`?
    fn follows(&self, root: &str, exchange: &str) -> bool {
        self.exchange == exchange && matches!(&self.subject, Subject::Front { root: own } if own == root)
    }
}

/// The data plants of a signed-in data half: what the supervisor hears
/// from them, what a watch asks of them, and what is closed with them.
pub(crate) struct DataPlants {
    pub(crate) ticker_updates: broadcast::Receiver<RithmicResponse>,
    pub(crate) history_updates: broadcast::Receiver<RithmicResponse>,
    watcher: Watcher,
    link: DataLink,
}

enum DataLink {
    Rithmic {
        ticker: RithmicTickerPlant,
        history: RithmicHistoryPlant,
        ticker_handle: RithmicTickerPlantHandle,
        history_handle: RithmicHistoryPlantHandle,
        handles: Arc<Handles>,
        /// Who signed in, for asking what they may see.
        user: String,
    },
    /// A test's, holding its channels open. Never made outside one.
    #[cfg_attr(not(test), allow(dead_code))]
    Fake(#[allow(dead_code)] Vec<broadcast::Sender<RithmicResponse>>),
}

impl DataPlants {
    #[cfg(test)]
    pub(crate) fn fake(
        ticker_updates: broadcast::Receiver<RithmicResponse>,
        plant: tests::Fake,
    ) -> DataPlants {
        let (history, history_updates) = broadcast::channel(16);
        DataPlants { ticker_updates, history_updates, watcher: Watcher::Fake(plant), link: DataLink::Fake(vec![history]) }
    }

    fn handles(&self) -> Option<Arc<Handles>> {
        match &self.link {
            DataLink::Rithmic { handles, .. } => Some(handles.clone()),
            DataLink::Fake(_) => None,
        }
    }

    /// Cut the plants from underneath, as a network failure would.
    pub(crate) fn abort(&self) {
        if let DataLink::Rithmic { ticker_handle, history_handle, .. } = &self.link {
            ticker_handle.abort();
            history_handle.abort();
        }
    }

    /// Sign out, or with `graceful` false only cut the plants, and wait for
    /// them to stop: dropping a plant does not stop it, and the old session
    /// would stay open next to the new one.
    pub(crate) async fn close(self, graceful: bool) {
        let DataLink::Rithmic { ticker, history, ticker_handle, history_handle, .. } = self.link else { return };
        if graceful {
            let _ = within(Duration::from_secs(2), ticker_handle.disconnect()).await;
            let _ = within(Duration::from_secs(2), history_handle.disconnect()).await;
        }
        ticker_handle.abort();
        history_handle.abort();
        let _ = tokio::time::timeout(Duration::from_secs(3), ticker.await_shutdown()).await;
        let _ = tokio::time::timeout(Duration::from_secs(3), history.await_shutdown()).await;
    }
}

/// The plant a watch asks: Rithmic's ticker plant, or a test's.
#[derive(Clone)]
pub(crate) enum Watcher {
    Rithmic(Arc<Handles>),
    #[cfg(test)]
    Fake(tests::Fake),
}

impl Plant for Watcher {
    async fn front_month(&self, root: String, exchange: String) -> Result<String, Error> {
        match self {
            Watcher::Rithmic(handles) => handles.front_month(root, exchange).await,
            #[cfg(test)]
            Watcher::Fake(fake) => fake.front_month(root, exchange).await,
        }
    }

    async fn subscribe(&self, contract: String, exchange: String) -> Result<(), Error> {
        match self {
            Watcher::Rithmic(handles) => handles.subscribe(contract, exchange).await,
            #[cfg(test)]
            Watcher::Fake(fake) => fake.subscribe(contract, exchange).await,
        }
    }

    async fn unsubscribe(&self, contract: String, exchange: String) {
        match self {
            Watcher::Rithmic(handles) => handles.unsubscribe(contract, exchange).await,
            #[cfg(test)]
            Watcher::Fake(fake) => fake.unsubscribe(contract, exchange).await,
        }
    }
}

/// What the plants are connected with: the login, and the app it is for.
pub(crate) fn config(app: App, credentials: &Credentials) -> Result<RithmicConfig, Error> {
    RithmicConfig::builder(RithmicEnv::Test)
        .url(&credentials.url)
        // Only read by a strategy that alternates, and required regardless.
        .beta_url(&credentials.url)
        .user(&credentials.user)
        .password(&credentials.password)
        .system_name(&credentials.system)
        .app_name(app.name)
        .app_version(app.version)
        .connect_attempt_timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| Error::NoLogin("the Rithmic login is incomplete".into()))
}

/// Sign in to the ticker and history plants.
pub(crate) async fn connect(app: App, credentials: &Credentials) -> Result<DataPlants, Error> {
    let config = config(app, credentials)?;
    // Kept to ask which exchanges it is entitled to; never logged.
    let user = credentials.user.clone();

    let ticker = within(REQUEST, RithmicTickerPlant::connect(&config, ConnectStrategy::Simple)).await?;
    let ticker_updates = ticker.get_handle();
    if let Err(error) = signed_in(within(REQUEST, ticker_updates.login()).await) {
        ticker_updates.abort();
        return Err(error);
    }
    let history = match within(REQUEST, RithmicHistoryPlant::connect(&config, ConnectStrategy::Simple)).await {
        Ok(history) => history,
        Err(error) => {
            let _ = within(REQUEST, ticker_updates.disconnect()).await;
            return Err(error);
        }
    };
    let history_updates = history.get_handle();
    if let Err(error) = signed_in(within(REQUEST, history_updates.login()).await) {
        history_updates.abort();
        let _ = within(REQUEST, ticker_updates.disconnect()).await;
        return Err(error);
    }
    let handles = Arc::new(Handles { ticker: ticker.get_handle(), history: history.get_handle() });
    // Heard by the supervisor, and the handles kept to sign out with.
    let (mut ticker_handle, mut history_handle) = (ticker_updates, history_updates);
    let ticker_updates = std::mem::replace(&mut ticker_handle.subscription_receiver, broadcast::channel(1).1);
    let history_updates = std::mem::replace(&mut history_handle.subscription_receiver, broadcast::channel(1).1);
    Ok(DataPlants {
        ticker_updates,
        history_updates,
        watcher: Watcher::Rithmic(handles.clone()),
        link: DataLink::Rithmic { ticker, history, ticker_handle, history_handle, handles, user },
    })
}

/// What a watch asks of the ticker plant, apart from the trades it then
/// hears: which contract is a root's front month, and subscribing to one.
/// The plant itself, or a test's.
pub(crate) trait Plant: Clone + Send + Sync + 'static {
    fn front_month(&self, root: String, exchange: String) -> impl Future<Output = Result<String, Error>> + Send;
    fn subscribe(&self, contract: String, exchange: String) -> impl Future<Output = Result<(), Error>> + Send;
    fn unsubscribe(&self, contract: String, exchange: String) -> impl Future<Output = ()> + Send;
}

impl Plant for Arc<Handles> {
    async fn front_month(&self, root: String, exchange: String) -> Result<String, Error> {
        front_month(&self.ticker, &root, &exchange).await
    }

    async fn subscribe(&self, contract: String, exchange: String) -> Result<(), Error> {
        let response = within(REQUEST, self.ticker.subscribe(&contract, &exchange)).await?;
        if let Some(error) = &response.error {
            return Err(rejected(error));
        }
        // The day's price limits too, which say where an order would be
        // refused: nothing is lost without them.
        let _ = within(REQUEST, self.ticker.subscribe_order_price_limits(&contract, &exchange)).await;
        // And what the contract is, once: its tick and what a point is worth.
        if !referenced(&contract, &exchange)
            && let Ok(asked) = within(REQUEST, self.ticker.get_reference_data(&contract, &exchange)).await
            && let RithmicMessage::ResponseReferenceData(data) = &asked.message
        {
            heard_reference(&contract, &exchange, Reference::of(data));
        }
        Ok(())
    }

    async fn unsubscribe(&self, contract: String, exchange: String) {
        let _ = within(REQUEST, self.ticker.unsubscribe_order_price_limits(&contract, &exchange)).await;
        let _ = within(REQUEST, self.ticker.unsubscribe(&contract, &exchange)).await;
    }
}

/// A request to the plant that watches were waiting on, answered.
pub(crate) enum Settled {
    /// A root's front month, or why there is none.
    Resolved { root: String, exchange: String, outcome: Result<String, Error> },
    /// A contract subscribed to, or why it could not be.
    Subscribed { contract: String, exchange: String, outcome: Result<(), Error> },
    /// A contract nothing watches any more, unsubscribed.
    Unsubscribed { contract: String, exchange: String },
}

/// The subscriptions of one signed-in data half, and the requests for them
/// still out.
///
/// A request is a round trip to Chicago, and the loop that asks is the one
/// that hands every trade on. So none is waited for there: each runs as a
/// task of its own and its answer comes back as a [`Settled`], while the
/// trades of every contract already watched go on arriving. Dropped with
/// its plants, so an answer meant for ones that are gone is never heard.
pub(crate) struct Subscriptions<P> {
    plant: P,
    /// How many watches each subscribed contract has, by contract and
    /// exchange.
    subscribed: HashMap<(String, String), usize>,
    /// Roots whose front month is being asked, and the watches waiting on
    /// each.
    resolving: HashMap<(String, String), Vec<u64>>,
    /// Contracts being subscribed, or waiting for an unsubscribe of theirs
    /// to finish first, and the watches waiting on each.
    subscribing: HashMap<(String, String), Vec<u64>>,
    /// Contracts being unsubscribed. A subscribe to one waits for it, or
    /// the two could reach the plant the wrong way round.
    unsubscribing: HashSet<(String, String)>,
    answers: mpsc::UnboundedSender<Settled>,
    pub(crate) settled: mpsc::UnboundedReceiver<Settled>,
}

impl<P: Plant> Subscriptions<P> {
    pub(crate) fn new(plant: P) -> Subscriptions<P> {
        let (answers, settled) = mpsc::unbounded_channel();
        Subscriptions {
            plant,
            subscribed: HashMap::new(),
            resolving: HashMap::new(),
            subscribing: HashMap::new(),
            unsubscribing: HashSet::new(),
            answers,
            settled,
        }
    }

    fn resolve(&self, root: &str, exchange: &str) {
        let (plant, answers) = (self.plant.clone(), self.answers.clone());
        let (root, exchange) = (root.to_string(), exchange.to_string());
        tokio::spawn(async move {
            let outcome = plant.front_month(root.clone(), exchange.clone()).await;
            let _ = answers.send(Settled::Resolved { root, exchange, outcome });
        });
    }

    fn subscribe(&self, contract: &str, exchange: &str) {
        let (plant, answers) = (self.plant.clone(), self.answers.clone());
        let (contract, exchange) = (contract.to_string(), exchange.to_string());
        tokio::spawn(async move {
            let outcome = plant.subscribe(contract.clone(), exchange.clone()).await;
            let _ = answers.send(Settled::Subscribed { contract, exchange, outcome });
        });
    }

    fn unsubscribe(&mut self, key: (String, String)) {
        if !self.unsubscribing.insert(key.clone()) {
            return;
        }
        let (plant, answers) = (self.plant.clone(), self.answers.clone());
        tokio::spawn(async move {
            let (contract, exchange) = key;
            plant.unsubscribe(contract.clone(), exchange.clone()).await;
            let _ = answers.send(Settled::Unsubscribed { contract, exchange });
        });
    }
}

/// The data half, as the connection's supervisor keeps it.
pub(crate) struct DataHalf {
    pub(crate) phase: Phase,
    pub(crate) watches: Watches,
    pub(crate) live: Option<(DataPlants, Subscriptions<Watcher>)>,
}

impl DataHalf {
    pub(crate) fn new(state: watch::Sender<Told>, fronts: Arc<Fronts>) -> DataHalf {
        DataHalf { phase: Phase::Off, watches: Watches::new(state, fronts), live: None }
    }

    /// Signed in: say so, ask which exchanges the login may see, and
    /// subscribe again everything still watched, each hearing
    /// [`Live::Watching`] once it is, which is its cue to load a snapshot.
    pub(crate) fn up(&mut self, plants: DataPlants) {
        let mut subscriptions = Subscriptions::new(plants.watcher.clone());
        self.watches.set(State::Up(plants.handles()));
        // A failure to answer leaves what was known, and charts as if all
        // were entitled.
        if let DataLink::Rithmic { handles, user, .. } = &plants.link {
            let (handles, user) = (handles.clone(), user.clone());
            tokio::spawn(async move {
                if let Ok(responses) = within(REQUEST, handles.ticker.list_exchange_permissions(&user)).await {
                    entitle(exchanges_of(&responses));
                }
            });
        }
        let ids: Vec<u64> = self.watches.watched.keys().copied().collect();
        for id in ids {
            self.watches.begin(id, &mut subscriptions);
        }
        self.live = Some((plants, subscriptions));
    }

    /// Sign out, and tell everything watching it is over.
    pub(crate) async fn close(&mut self) {
        if let Some((plants, _)) = self.live.take() {
            plants.close(true).await;
        }
        self.phase = Phase::Off;
        self.watches.closed();
    }
}

/// Everything watched on the data half, and what it has been told.
pub(crate) struct Watches {
    state: watch::Sender<Told>,
    /// The generation the half is open as. A watch asked for by a client
    /// of another is closed, and what a client waits for is told it under
    /// its own, so a client opening the half again never takes the close
    /// before it for an answer.
    pub(crate) generation: u64,
    fronts: Arc<Fronts>,
    pub(crate) watched: HashMap<u64, Watched>,
}

impl Watches {
    pub(crate) fn new(state: watch::Sender<Told>, fronts: Arc<Fronts>) -> Watches {
        Watches { state, generation: 0, fronts, watched: HashMap::new() }
    }

    /// Where the half stands now, as its clients are told.
    pub(crate) fn set(&self, state: State) {
        let _ = self.state.send((self.generation, state));
    }

    pub(crate) fn now(&self) -> State {
        self.state.borrow().1.clone()
    }

    /// Subscribe watch `id` to its contract, or its root's front month, and
    /// tell it so once it is. Returns at once: what the plant has to be
    /// asked is asked by a task, and its answer [`Watches::settle`]d when it
    /// comes.
    pub(crate) fn begin<P: Plant>(&mut self, id: u64, subscriptions: &mut Subscriptions<P>) {
        let Some((subject, exchange)) = self.watched.get(&id).map(|w| (w.subject.clone(), w.exchange.clone())) else {
            return;
        };
        let root = match subject {
            Subject::Contract { contract } => return self.attach(id, contract, subscriptions),
            Subject::Front { root } => root,
        };
        if let Some(contract) = self.fronts.get(&root, &exchange) {
            return self.attach(id, contract, subscriptions);
        }
        let key = (root, exchange);
        match subscriptions.resolving.get_mut(&key) {
            Some(waiting) => waiting.push(id),
            None => {
                subscriptions.resolve(&key.0, &key.1);
                subscriptions.resolving.insert(key, vec![id]);
            }
        }
    }

    /// Watch `id` is on `contract`: watching at once if that is subscribed
    /// already, and otherwise once it is.
    fn attach<P: Plant>(&mut self, id: u64, contract: String, subscriptions: &mut Subscriptions<P>) {
        let Some(exchange) = self.watched.get(&id).map(|w| w.exchange.clone()) else { return };
        let key = (contract, exchange);
        if subscriptions.subscribed.contains_key(&key) {
            return self.subscribed(id, &key, subscriptions);
        }
        match subscriptions.subscribing.get_mut(&key) {
            Some(waiting) => waiting.push(id),
            None => {
                if !subscriptions.unsubscribing.contains(&key) {
                    subscriptions.subscribe(&key.0, &key.1);
                }
                subscriptions.subscribing.insert(key, vec![id]);
            }
        }
    }

    /// Watch `id` is on `key`, which is subscribed: tell it so.
    fn subscribed<P: Plant>(&mut self, id: u64, key: &(String, String), subscriptions: &mut Subscriptions<P>) {
        let Some(watched) = self.watched.get_mut(&id) else { return };
        *subscriptions.subscribed.entry(key.clone()).or_insert(0) += 1;
        watched.contract = Some(key.0.clone());
        watched.told_lost = false;
        self.tell(id, Live::Watching { contract: key.0.clone() });
    }

    /// The plant answered a request that watches were waiting on.
    pub(crate) fn settle<P: Plant>(&mut self, settled: Settled, subscriptions: &mut Subscriptions<P>) {
        match settled {
            Settled::Resolved { root, exchange, outcome } => {
                // Nobody asking any more: Rithmic pushed the answer while
                // this was out, and the push is the later word.
                let Some(waiting) = subscriptions.resolving.remove(&(root.clone(), exchange.clone())) else { return };
                match outcome {
                    Ok(contract) => {
                        self.fronts.put(&root, &exchange, &contract);
                        for id in waiting {
                            self.attach(id, contract.clone(), subscriptions);
                        }
                    }
                    Err(error) => {
                        for id in waiting {
                            self.tell(id, Live::Lost(error.clone()));
                        }
                    }
                }
            }
            Settled::Subscribed { contract, exchange, outcome } => {
                let key = (contract, exchange);
                let waiting: Vec<u64> = subscriptions
                    .subscribing
                    .remove(&key)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|id| self.watched.contains_key(id))
                    .collect();
                match outcome {
                    // Everything that wanted it went while it was asked for.
                    Ok(()) if waiting.is_empty() => subscriptions.unsubscribe(key),
                    Ok(()) => {
                        for id in waiting {
                            self.subscribed(id, &key, subscriptions);
                        }
                    }
                    Err(error) => {
                        for id in waiting {
                            self.tell(id, Live::Lost(error.clone()));
                        }
                    }
                }
            }
            Settled::Unsubscribed { contract, exchange } => {
                let key = (contract, exchange);
                subscriptions.unsubscribing.remove(&key);
                // Watched again while it was being let go.
                if subscriptions.subscribing.contains_key(&key) {
                    subscriptions.subscribe(&key.0, &key.1);
                }
            }
        }
    }

    /// Forget watch `id`, and unsubscribe its contract if nothing else
    /// watches it. Returns at once.
    pub(crate) fn end<P: Plant>(&mut self, id: u64, subscriptions: &mut Subscriptions<P>) {
        let Some(watched) = self.watched.remove(&id) else { return };
        // Still waiting for its contract: the answer finds it gone.
        if let Some(contract) = watched.contract {
            release((contract, watched.exchange), subscriptions);
        }
    }

    /// Rithmic says `root`'s front month on `exchange` is `contract` now,
    /// pushed or answered: remember it, and move every watch on the root's
    /// front month over, each hearing [`Live::Watching`] with the new
    /// contract once it is subscribed. The old contract is let go when
    /// nothing watches it any more; a watch on that very contract keeps it.
    pub(crate) fn rolled<P: Plant>(
        &mut self,
        root: &str,
        exchange: &str,
        contract: &str,
        subscriptions: &mut Subscriptions<P>,
    ) {
        self.fronts.put(root, exchange, contract);
        let key = (root.to_string(), exchange.to_string());
        // Still asking: this is the answer.
        let mut moving = subscriptions.resolving.remove(&key).unwrap_or_default();
        for (id, watched) in &mut self.watched {
            if !watched.follows(root, exchange) || watched.contract.as_deref() == Some(contract) {
                continue;
            }
            match watched.contract.take() {
                Some(old) => {
                    release((old, exchange.to_string()), subscriptions);
                    moving.push(*id);
                }
                // Being subscribed to the old one, whose subscribe then
                // answers to nobody and lets it go; or lost on it, and the
                // new one is another chance.
                None => {
                    for (subscribing, waiting) in &mut subscriptions.subscribing {
                        if subscribing.1 == exchange {
                            waiting.retain(|waiting| waiting != id);
                        }
                    }
                    moving.push(*id);
                }
            }
        }
        for id in moving {
            self.attach(id, contract.to_string(), subscriptions);
        }
    }

    /// Every root whose front month is watched on `exchange`, or was asked
    /// about: the roots a pushed roll can be of.
    fn roots(&self, exchange: &str) -> Vec<String> {
        let mut roots: Vec<String> = self
            .watched
            .values()
            .filter_map(|watched| match &watched.subject {
                Subject::Front { root } if watched.exchange == exchange => Some(root.clone()),
                _ => None,
            })
            .chain(self.fronts.roots(exchange))
            .collect();
        roots.sort();
        roots.dedup();
        roots
    }

    /// Rithmic pushed that a front month changed. Which root's it says in
    /// `symbol`, when that is a root asked about here; otherwise the root is
    /// the longest asked about that the contract spells.
    pub(crate) fn heard_roll<P: Plant>(&mut self, update: &FrontMonthContractUpdate, subscriptions: &mut Subscriptions<P>) {
        let Some(exchange) = update.exchange.as_deref().or(update.trading_exchange.as_deref()) else { return };
        let Some((root, contract)) = roll_of(update, &self.roots(exchange)) else { return };
        self.rolled(&root, exchange, &contract, subscriptions);
    }

    fn tell(&self, id: u64, live: Live) {
        if let Some(watched) = self.watched.get(&id) {
            (watched.sink)(live);
        }
    }

    pub(crate) fn trade(&self, trade: &LastTrade) {
        let Some(found) = trade_of(trade) else { return };
        let (Some(symbol), Some(exchange)) = (trade.symbol.as_deref(), trade.exchange.as_deref()) else {
            return;
        };
        let live = match trade.is_snapshot {
            Some(true) => Live::Last(found),
            _ => Live::Trade(found),
        };
        self.deliver(symbol, exchange, live);
    }

    /// A trade on `symbol`, to everything watching it.
    fn deliver(&self, symbol: &str, exchange: &str, live: Live) {
        for watched in self.watched.values() {
            if watched.contract.as_deref() == Some(symbol) && watched.exchange == exchange {
                (watched.sink)(live.clone());
            }
        }
    }

    pub(crate) fn start_over(&self) {
        for watched in self.watched.values() {
            if let Some(contract) = &watched.contract {
                (watched.sink)(Live::Watching { contract: contract.clone() });
            }
        }
    }

    /// The plants are gone: nothing is subscribed, and a front month is
    /// asked for again.
    fn unsubscribed(&mut self) {
        for watched in self.watched.values_mut() {
            watched.contract = None;
        }
        self.fronts.clear();
    }

    /// The data half is down and will be tried again: say so, once, to
    /// everything watching.
    pub(crate) fn lose(&mut self, error: Error) {
        self.unsubscribed();
        self.set(State::Down(error.clone()));
        for watched in self.watched.values_mut() {
            if !watched.told_lost {
                watched.told_lost = true;
                (watched.sink)(Live::Lost(error.clone()));
            }
        }
    }

    /// Rithmic refused the login, or there is none, or it signed this one
    /// out: say so to everything watching now, and try nothing more until
    /// the half is closed and opened again.
    pub(crate) fn refuse(&mut self, error: Error) {
        self.unsubscribed();
        self.set(State::Stopped(error.clone()));
        for watched in self.watched.values() {
            (watched.sink)(Live::Lost(error.clone()));
        }
    }

    /// Signed out: everything watching hears so, and is forgotten.
    fn closed(&mut self) {
        self.unsubscribed();
        self.set(State::Stopped(Error::Closed));
        for watched in self.watched.values() {
            (watched.sink)(Live::Lost(Error::Closed));
        }
        self.watched.clear();
    }
}

/// One watch fewer on a subscribed contract, and unsubscribed when it was
/// the last.
fn release<P: Plant>(key: (String, String), subscriptions: &mut Subscriptions<P>) {
    let Some(count) = subscriptions.subscribed.get_mut(&key) else { return };
    *count -= 1;
    if *count == 0 {
        subscriptions.subscribed.remove(&key);
        subscriptions.unsubscribe(key);
    }
}

/// Which of `roots` a pushed roll is of, and the contract it names. The
/// update carries what the request did in `symbol` and the contract in
/// `trading_symbol`, as the answer to the request does; but Rithmic's
/// own documentation is thin on it, so each of its names is read as the
/// root when it is one asked about, and as the contract when it spells a
/// contract of one, the longest root first.
fn roll_of(update: &FrontMonthContractUpdate, roots: &[String]) -> Option<(String, String)> {
    let names = [update.trading_symbol.as_deref(), update.symbol_name.as_deref(), update.symbol.as_deref()];
    let named_root = names.into_iter().flatten().find(|name| roots.iter().any(|root| root == name));
    let mut by_length: Vec<&String> = roots.iter().collect();
    by_length.sort_by_key(|root| std::cmp::Reverse(root.len()));
    let contract_of = |root: &str| names.into_iter().flatten().find(|name| is_contract(root, name)).map(str::to_string);
    match named_root {
        Some(root) => contract_of(root).map(|contract| (root.to_string(), contract)),
        None => by_length.into_iter().find_map(|root| contract_of(root).map(|contract| (root.clone(), contract))),
    }
}

pub(crate) async fn within<T>(
    patience: Duration,
    future: impl std::future::Future<Output = Result<T, RithmicError>>,
) -> Result<T, Error> {
    match tokio::time::timeout(patience, future).await {
        Ok(result) => result.map_err(error_of),
        Err(_) => Err(Error::Timeout),
    }
}

/// A login's outcome: a refusal is the login's fault and final, a broken
/// connection is worth another go.
pub(crate) fn signed_in(outcome: Result<RithmicResponse, Error>) -> Result<(), Error> {
    match outcome {
        Ok(_) => Ok(()),
        Err(Error::Rejected(why)) => Err(Error::Refused(why)),
        Err(error) => Err(error),
    }
}

pub(crate) fn error_of(error: RithmicError) -> Error {
    if error.is_connection_issue() {
        return Error::Connection(error.to_string());
    }
    match &error {
        RithmicError::RequestRejected(why) => Error::Rejected(
            why.message.clone().filter(|message| !message.is_empty()).unwrap_or_else(|| why.to_string()),
        ),
        _ => Error::Rejected(error.to_string()),
    }
}

fn rejected(error: &RithmicError) -> Error {
    error_of(error.clone())
}

/// The connection is gone, if this update says so.
pub(crate) fn dropped(update: &RithmicResponse) -> Option<Error> {
    match &update.message {
        // A heartbeat the server turned down is about the heartbeat; the
        // connection is fine.
        RithmicMessage::HeartbeatTimeout if matches!(update.error, Some(RithmicError::RequestRejected(_))) => None,
        RithmicMessage::ConnectionError | RithmicMessage::HeartbeatTimeout | RithmicMessage::ForcedLogout(_) => {
            Some(Error::Connection(match &update.error {
                Some(error) => error.to_string(),
                None => "the connection dropped".into(),
            }))
        }
        _ => None,
    }
}

fn trade_of(trade: &LastTrade) -> Option<Trade> {
    let price = trade.trade_price.filter(|price| price.is_finite() && *price > 0.0)?;
    let size = u64::try_from(trade.trade_size?).ok().filter(|size| *size > 0)?;
    let seconds = i64::from(trade.source_ssboe.or(trade.ssboe)?);
    let nanos = trade
        .source_nsecs
        .map(i64::from)
        .or_else(|| trade.source_usecs.or(trade.usecs).map(|micros| i64::from(micros) * 1_000))
        .unwrap_or(0);
    Some(Trade {
        price,
        size,
        aggressor: match trade.aggressor {
            Some(1) => Some(Side::Buy),
            Some(2) => Some(Side::Sell),
            _ => None,
        },
        at_nanos: seconds * 1_000_000_000 + nanos,
    })
}

/// The front month of `root`: what Rithmic says it is, or, when it will not
/// say, the one [`chosen`] from the contracts it lists. Rithmic Test
/// answers the question with "no data" for every root, and lists the
/// contracts. Asked with updates: a roll then arrives on the ticker plant
/// as a [`FrontMonthContractUpdate`], which the supervisor hears.
async fn front_month(ticker: &RithmicTickerPlantHandle, root: &str, exchange: &str) -> Result<String, Error> {
    let asked = within(REQUEST, ticker.get_front_month_contract(root, exchange, true)).await?;
    if let RithmicMessage::ResponseFrontMonthContract(front) = &asked.message
        && let Some(contract) = front
            .trading_symbol
            .as_deref()
            .or(front.symbol.as_deref())
            .filter(|symbol| is_contract(root, symbol))
    {
        return Ok(contract.to_string());
    }
    let listed = listed(ticker, root, exchange).await?;
    chosen(root, exchange, &listed)
}

/// The contracts of `root` listed on `exchange`, each with the day it
/// expires. Whatever is listed that is not a contract of the root, such as
/// a spread, is left out.
async fn listed(ticker: &RithmicTickerPlantHandle, root: &str, exchange: &str) -> Result<Vec<(String, Expiry)>, Error> {
    let listed = within(
        REQUEST,
        ticker.search_symbols(root, Some(exchange), Some(root), Some(InstrumentType::Future), Some(Pattern::Equals)),
    )
    .await?;
    Ok(listed
        .iter()
        .filter_map(|response| match &response.message {
            RithmicMessage::ResponseSearchSymbols(found) => {
                let symbol = found.symbol.as_deref().filter(|symbol| is_contract(root, symbol))?;
                let expires = found.expiration_date.as_deref()?.parse::<i32>().ok()?;
                Some((symbol.to_string(), expiry(expires)?))
            }
            _ => None,
        })
        .collect())
}

/// The contract expiring soonest, but not within [`ROLL_DAYS`] of `now`.
fn nearest(listed: &[(String, Expiry)], now: i64) -> Option<String> {
    listed
        .iter()
        .map(|(symbol, expires)| (symbol, expiry_start(*expires)))
        .filter(|(_, expires)| *expires > now + ROLL_DAYS * 86_400)
        .min_by_key(|(_, expires)| *expires)
        .map(|(symbol, _)| symbol.clone())
}

/// `ESZ6` or `CLF30` for `ES` and `CL`: the root, a month code and a year.
fn is_contract(root: &str, symbol: &str) -> bool {
    let Some(rest) = symbol.strip_prefix(root) else { return false };
    let mut chars = rest.chars();
    matches!(chars.next(), Some('F' | 'G' | 'H' | 'J' | 'K' | 'M' | 'N' | 'Q' | 'U' | 'V' | 'X' | 'Z'))
        && {
            let year: &str = chars.as_str();
            (1..=2).contains(&year.len()) && year.chars().all(|c| c.is_ascii_digit())
        }
}

fn bars_of(responses: &[RithmicResponse], period: Period) -> Result<Vec<Bar>, Error> {
    let mut bars: Vec<Bar> = Vec::with_capacity(responses.len());
    for response in responses {
        let RithmicMessage::ResponseTimeBarReplay(bar) = &response.message else { continue };
        let (Some(marker), Some(open), Some(high), Some(low), Some(close)) =
            (bar.marker, bar.open_price, bar.high_price, bar.low_price, bar.close_price)
        else {
            continue;
        };
        // Minute bars are marked with the time they close; daily ones with
        // their trading date.
        let start = match period {
            Period::Minutes(count) => i64::from(marker) - 60 * i64::from(count.max(1)),
            Period::Days(_) => match date_start(marker) {
                Some(start) => start,
                None => continue,
            },
        };
        bars.push(Bar { start, open, high, low, close, volume: bar.volume.unwrap_or(0) });
    }
    // A reply with nothing in it and a refusal on its end marker is a
    // refusal; one cut short after some bars is still those bars.
    if bars.is_empty()
        && let Some(error) = responses.last().and_then(|end| end.error.as_ref())
    {
        return Err(rejected(error));
    }
    bars.sort_by_key(|bar| bar.start);
    bars.dedup_by_key(|bar| bar.start);
    Ok(bars)
}

/// The minutes of a volume-profile replay, oldest first. Marked, as minute
/// bars are, with the time they close.
fn ladders_of(responses: &[RithmicResponse]) -> Result<Vec<LadderMinute>, Error> {
    let mut minutes: Vec<LadderMinute> = Vec::with_capacity(responses.len());
    for response in responses {
        let RithmicMessage::ResponseVolumeProfileMinuteBars(bar) = &response.message else { continue };
        let (Some(marker), Some(open), Some(high), Some(low), Some(close)) =
            (bar.marker, bar.open_price, bar.high_price, bar.low_price, bar.close_price)
        else {
            continue;
        };
        let n = bar.profile_price.len();
        let wide = |values: &[i32]| -> Vec<u64> {
            (0..n).map(|i| values.get(i).map_or(0, |v| u64::try_from(*v).unwrap_or(0))).collect()
        };
        let count = |values: &[i32]| -> Vec<u32> {
            (0..n).map(|i| values.get(i).map_or(0, |v| u32::try_from(*v).unwrap_or(0))).collect()
        };
        minutes.push(LadderMinute {
            start: i64::from(marker) - 60,
            open,
            high,
            low,
            close,
            volume: bar.volume.unwrap_or(0),
            prices: bar.profile_price.clone(),
            bid: wide(&bar.profile_bid_volume),
            ask: wide(&bar.profile_ask_volume),
            unknown: wide(&bar.profile_no_aggressor_volume),
            bid_trades: count(&bar.profile_bid_aggressor_trades),
            ask_trades: count(&bar.profile_ask_aggressor_trades),
            unknown_trades: count(&bar.profile_no_aggressor_trades),
        });
    }
    if minutes.is_empty()
        && let Some(error) = responses.last().and_then(|end| end.error.as_ref())
    {
        return Err(rejected(error));
    }
    minutes.sort_by_key(|m| m.start);
    minutes.dedup_by_key(|m| m.start);
    Ok(minutes)
}

fn now_seconds() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_secs() as i64).unwrap_or(0)
}

fn wire_seconds(ts: i64) -> i32 {
    i32::try_from(ts.max(0)).unwrap_or(i32::MAX)
}

/// Unix seconds as the `YYYYMMDD` the history plant takes daily bars by.
fn date_number(ts: i64) -> i32 {
    let (year, month, day) = civil(ts.div_euclid(86_400));
    i32::try_from(year * 10_000 + month * 100 + day).unwrap_or(i32::MAX)
}

/// A `YYYYMMDD` date as a year, month and day, when it is one.
fn expiry(date: i32) -> Option<Expiry> {
    let (year, month, day) = (date / 10_000, (date / 100 % 100) as u32, (date % 100) as u32);
    ((1..=12).contains(&month) && (1..=31).contains(&day)).then_some((year, month, day))
}

/// Midnight UTC of the day a contract expires.
fn expiry_start((year, month, day): Expiry) -> i64 {
    days_from_civil(i64::from(year), i64::from(month), i64::from(day)) * 86_400
}

/// Midnight UTC of a `YYYYMMDD` date.
fn date_start(date: i32) -> Option<i64> {
    let (year, month, day) = (i64::from(date / 10_000), i64::from(date / 100 % 100), i64::from(date % 100));
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400)
}

/// A day count since 1970 as year, month and day.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Rithmic is told the caller's name and version, not this crate's.
    #[test]
    fn the_plants_are_told_the_app_the_caller_names() {
        let app = App { name: "abcd:omacharts", version: "9.8.7" };
        let credentials = Credentials {
            user: "someone".into(),
            password: "secret".into(),
            system: crate::TEST_SYSTEM.into(),
            url: crate::TEST_URL.into(),
        };
        let config = config(app, &credentials).expect("a config");
        assert_eq!(config.app_name, "abcd:omacharts");
        assert_eq!(config.app_version, "9.8.7");
        assert_ne!(config.app_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn dates_go_both_ways() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_760), (2026, 11, 3));
        assert_eq!(date_start(20261103), Some(20_760 * 86_400));
        assert_eq!(date_number(20_760 * 86_400 + 3_600), 20261103);
        assert_eq!(date_start(20261340), None);
    }

    /// Rithmic Test answers "enabled" for each of CME, CBOT, NYMEX and
    /// COMEX; anything else is a no.
    #[test]
    fn an_entitlement_is_granted_only_when_it_says_so() {
        assert!(granted("enabled"));
        assert!(granted(" Enabled "));
        assert!(!granted("disabled"));
        assert!(!granted(""));
        assert!(!granted("pending"));
    }

    #[test]
    fn a_contract_is_a_root_a_month_and_a_year() {
        assert!(is_contract("ES", "ESZ6"));
        assert!(is_contract("CL", "CLF30"));
        assert!(!is_contract("GC", "GC"));
        assert!(!is_contract("ES", "ESZ"));
        assert!(!is_contract("ES", "ESA6"));
        assert!(!is_contract("ES", "ESZ6 C5000"));
        assert!(!is_contract("M", "MESZ6"));
    }

    /// Reference data says a contract's tick, point value and currency, a
    /// zero or an empty field being nothing said, and is passed on once heard.
    #[test]
    fn reference_data_is_read_and_passed_on() {
        let mut data = ResponseReferenceData::default();
        data.min_qprice_change = Some(0.25);
        data.single_point_value = Some(50.0);
        data.currency = Some("USD".into());
        let said = Reference::of(&data);
        assert_eq!(said, Reference { tick: Some(0.25), point: Some(50.0), currency: Some("USD".into()) });
        data.single_point_value = Some(0.0);
        data.currency = Some(String::new());
        assert_eq!(Reference::of(&data), Reference { tick: Some(0.25), point: None, currency: None });

        let heard = Arc::new(Mutex::new(Vec::new()));
        let into = heard.clone();
        on_reference(move |contract, exchange, said| {
            into.lock().unwrap().push((contract.to_string(), exchange.to_string(), said.clone()));
        });
        assert!(!referenced("REFZ6", "CME"));
        heard_reference("REFZ6", "CME", said.clone());
        assert!(referenced("REFZ6", "CME"), "asked once");
        assert!(!referenced("REFZ6", "CBOT"));
        assert!(heard.lock().unwrap().contains(&("REFZ6".to_string(), "CME".to_string(), said)));
    }

    /// A contract's price limits as pushed: a snapshot sets both, an update
    /// replaces what it carries, and a cleared limit is no longer known.
    #[test]
    fn price_limits_follow_what_is_pushed() {
        let pushed = |high: Option<f64>, low: Option<f64>, present: u32, cleared: u32| {
            let mut heard = OrderPriceLimits::default();
            heard.symbol = Some("LIMZ6".into());
            heard.exchange = Some("CME".into());
            heard.high_price_limit = high;
            heard.low_price_limit = low;
            heard.presence_bits = Some(present);
            heard.clear_bits = Some(cleared);
            heard_limits(&heard);
        };
        assert_eq!(price_limits("LIMZ6", "CME"), (None, None), "nothing heard");
        pushed(Some(8_189.0), Some(7_117.0), 3, 0);
        assert_eq!(price_limits("LIMZ6", "CME"), (Some(7_117.0), Some(8_189.0)));
        pushed(Some(8_200.0), None, 1, 0);
        assert_eq!(price_limits("LIMZ6", "CME"), (Some(7_117.0), Some(8_200.0)), "the low kept");
        pushed(None, None, 0, 2);
        assert_eq!(price_limits("LIMZ6", "CME"), (None, Some(8_200.0)), "the low cleared");
        assert_eq!(price_limits("LIMZ6", "CBOT"), (None, None), "another exchange's is its own");
    }

    /// The nearest to expiring trades now, until the roll a week before it
    /// expires, and then the next one does.
    #[test]
    fn the_front_month_rolls_a_week_before_it_expires() {
        let day = 86_400;
        // Days 100, 10 and 190 of 1970.
        let listed = vec![
            ("ESH7".to_string(), (1970, 4, 11)),
            ("ESZ6".to_string(), (1970, 1, 11)),
            ("ESM7".to_string(), (1970, 7, 10)),
        ];
        assert_eq!(expiry_start((1970, 7, 10)), 190 * day);
        assert_eq!(nearest(&listed, 0).as_deref(), Some("ESZ6"));
        assert_eq!(nearest(&listed, 3 * day).as_deref(), Some("ESH7"));
        assert_eq!(nearest(&listed, 200 * day), None);
        assert_eq!(expiry(20261218), Some((2026, 12, 18)));
        assert_eq!(expiry(20261340), None);
    }

    /// A pushed roll names the root as the request did and the contract as
    /// its trading symbol; read either way round, and against the roots
    /// asked about, the longest first, when it names only the contract.
    #[test]
    fn a_pushed_roll_is_read_against_the_roots_asked_about() {
        let roots = vec!["ES".to_string(), "MES".to_string()];
        let update = |symbol: &str, trading: Option<&str>| {
            let mut update = FrontMonthContractUpdate::default();
            update.symbol = Some(symbol.into());
            update.trading_symbol = trading.map(str::to_string);
            update
        };
        let roll = |symbol, trading| roll_of(&update(symbol, trading), &roots);
        assert_eq!(roll("ES", Some("ESH7")), Some(("ES".into(), "ESH7".into())));
        assert_eq!(roll("ESH7", Some("ES")), Some(("ES".into(), "ESH7".into())), "either way round");
        assert_eq!(roll("MESH7", None), Some(("MES".into(), "MESH7".into())), "the longest root it spells");
        assert_eq!(roll("ESH7", None), Some(("ES".into(), "ESH7".into())));
        assert_eq!(roll("ES", None), None, "no contract named");
        assert_eq!(roll("NQ", Some("NQH7")), None, "not a root asked about");
        assert_eq!(roll("ES", Some("ESH7 C5000")), None, "not a contract");
    }

    /// A roll pushed for a root moves every watch on its front month to the
    /// new contract, which each hears, and lets the old contract go once
    /// nothing watches it: a watch on that very contract stays on it, and
    /// a watch on another root's front month is untouched.
    #[tokio::test]
    async fn a_pushed_roll_moves_the_watches_on_a_front_month_and_not_those_on_a_contract() {
        let fake = Fake::default();
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();

        watch_root(&mut supervisor, &mut subscriptions, 1, "RLA", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        watch_root(&mut supervisor, &mut subscriptions, 2, "RLA", &heard);
        watch_contract(&mut supervisor, &mut subscriptions, 3, "RLAZ6", &heard);
        watch_root(&mut supervisor, &mut subscriptions, 4, "RLB", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.subscribes.lock().unwrap().as_slice(), &["RLAZ6", "RLBZ6"]);
        assert_eq!(heard.lock().unwrap().len(), 4, "all four are watching");
        heard.lock().unwrap().clear();

        supervisor.rolled("RLA", "CME", "RLAH7", &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.subscribes.lock().unwrap().as_slice(), &["RLAZ6", "RLBZ6", "RLAH7"], "subscribed once");
        assert!(fake.unsubscribes.lock().unwrap().is_empty(), "RLAZ6 is still watched by itself");
        let mut moved: Vec<(u64, Live)> = heard.lock().unwrap().clone();
        moved.sort_by_key(|(id, _)| *id);
        let watching = |contract: &str| Live::Watching { contract: contract.into() };
        assert_eq!(moved, vec![(1, watching("RLAH7")), (2, watching("RLAH7"))]);
        assert_eq!(supervisor.fronts.get("RLA", "CME").as_deref(), Some("RLAH7"));
        assert_eq!(found_front_month("RLA", "CME"), Some(("RLAH7".into(), true)), "fresh for the trading half");

        // A trade on each goes to whoever is on it now.
        heard.lock().unwrap().clear();
        supervisor.deliver("RLAZ6", "CME", Live::Trade(print(1)));
        supervisor.deliver("RLAH7", "CME", Live::Trade(print(2)));
        let mut trades: Vec<(u64, i64)> =
            heard.lock().unwrap().iter().map(|(id, live)| (*id, match live { Live::Trade(t) => t.at_nanos, _ => 0 })).collect();
        trades.sort();
        assert_eq!(trades, vec![(1, 2_000_000_000), (2, 2_000_000_000), (3, 1_000_000_000)]);

        // The contract watch goes: nothing is on RLAZ6 any more.
        supervisor.end(3, &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.unsubscribes.lock().unwrap().as_slice(), &["RLAZ6"]);

        // And a roll nothing else holds lets the old contract go at once.
        supervisor.rolled("RLA", "CME", "RLAM7", &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.subscribes.lock().unwrap().as_slice(), &["RLAZ6", "RLBZ6", "RLAH7", "RLAM7"]);
        assert_eq!(fake.unsubscribes.lock().unwrap().as_slice(), &["RLAZ6", "RLAH7"]);
        supervisor.deliver("RLBZ6", "CME", Live::Trade(print(3)));
        let nq = heard.lock().unwrap().iter().filter(|(id, _)| *id == 4).count();
        assert_eq!(nq, 1, "RLB heard its trade and nothing of RLA's rolls");
    }

    /// A roll pushed while the old contract is still being subscribed moves
    /// the watch before it is told anything, and the old subscribe, answered
    /// to nobody, is let go.
    #[tokio::test]
    async fn a_roll_during_a_subscribe_moves_the_watch_before_it_hears() {
        let fake = Fake::default();
        fake.slow.lock().unwrap().insert("RSZ6".into());
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();
        watch_root(&mut supervisor, &mut subscriptions, 1, "RS", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert!(subscriptions.subscribing.contains_key(&("RSZ6".into(), "CME".into())), "the subscribe is out");

        supervisor.rolled("RS", "CME", "RSH7", &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(heard.lock().unwrap().as_slice(), &[(1, Live::Watching { contract: "RSH7".into() })]);
        fake.gate.add_permits(1);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.unsubscribes.lock().unwrap().as_slice(), &["RSZ6"], "let go once it answered");
        assert_eq!(heard.lock().unwrap().len(), 1, "never told of the old one");
    }

    /// Whoever listens for front months hears one found, and the roll that
    /// replaces it, and nothing when it is found again unchanged.
    #[tokio::test]
    async fn listeners_hear_a_front_month_found_and_rolled() {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let into = heard.clone();
        on_front_month(move |root, exchange, contract| {
            if root == "LSN" {
                into.lock().unwrap().push((exchange.to_string(), contract.to_string()));
            }
        });
        let fake = Fake::default();
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let sinks: Heard = Arc::default();
        watch_root(&mut supervisor, &mut subscriptions, 1, "LSN", &sinks);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(heard.lock().unwrap().as_slice(), &[("CME".to_string(), "LSNZ6".to_string())]);

        supervisor.rolled("LSN", "CME", "LSNH7", &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        supervisor.rolled("LSN", "CME", "LSNH7", &mut subscriptions);
        let told: Vec<String> = heard.lock().unwrap().iter().map(|(_, contract)| contract.clone()).collect();
        assert_eq!(told, vec!["LSNZ6", "LSNH7"], "the roll once");
    }

    /// When the plant will not say which contract is the front month, the
    /// chooser installed above names it from the listing; one that leaves
    /// the choice open gets the nearest to expire.
    #[tokio::test]
    async fn the_chooser_names_the_front_month_when_the_plant_will_not() {
        let listing = vec![("CHZ6".to_string(), (2099, 12, 18)), ("CHH7".to_string(), (2100, 3, 19))];
        let handed = Arc::new(Mutex::new(Vec::new()));
        let into = handed.clone();
        choose_front_month_with(move |root, exchange, listed| {
            into.lock().unwrap().push((root.to_string(), exchange.to_string(), listed.to_vec()));
            (root == "CH").then(|| "CHH7".to_string())
        });
        let fake = Fake::default();
        fake.unknown.lock().unwrap().insert("CH".into(), listing.clone());
        fake.unknown.lock().unwrap().insert("NC".into(), listing.iter().map(|(s, e)| (s.replace("CH", "NC"), *e)).collect());
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();

        watch_root(&mut supervisor, &mut subscriptions, 1, "CH", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(heard.lock().unwrap().as_slice(), &[(1, Live::Watching { contract: "CHH7".into() })]);
        assert_eq!(handed.lock().unwrap().as_slice(), &[("CH".to_string(), "CME".to_string(), listing)]);
        assert_eq!(found_front_month("CH", "CME"), Some(("CHH7".into(), true)));

        watch_root(&mut supervisor, &mut subscriptions, 2, "NC", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(heard.lock().unwrap()[1], (2, Live::Watching { contract: "NCZ6".into() }), "the nearest, left to the crate");
    }

    /// A watch on a contract subscribes to it and nothing else: no front
    /// month is asked, and it hears `Watching` with the contract it named.
    #[tokio::test]
    async fn a_watch_on_a_contract_never_asks_the_front_month() {
        let fake = Fake::default();
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();
        watch_contract(&mut supervisor, &mut subscriptions, 1, "ESM7", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(heard.lock().unwrap().as_slice(), &[(1, Live::Watching { contract: "ESM7".into() })]);
        assert_eq!(fake.subscribes.lock().unwrap().as_slice(), &["ESM7"]);
        assert!(fake.asked.lock().unwrap().is_empty(), "nothing asked");
        assert!(subscriptions.resolving.is_empty());
        supervisor.end(1, &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.unsubscribes.lock().unwrap().as_slice(), &["ESM7"]);
    }

    #[test]
    fn no_error_carries_a_password() {
        // Every constructor of an error here is built from Rithmic's words
        // or this crate's; none takes a login. What a refusal looks like:
        let refused = signed_in(Err(Error::Rejected("permission denied".into()))).unwrap_err();
        assert_eq!(refused, Error::Refused("permission denied".into()));
        assert_eq!(refused.to_string(), "Rithmic refused the login: permission denied");
    }

    /// A ticker plant that answers a front month at once, the December
    /// contract, except for a root in `unknown`, whose contracts it lists
    /// instead as Rithmic Test does. A contract named in `slow` is
    /// subscribed, and a root named there answered, only when the test
    /// lets it through the gate.
    #[derive(Clone)]
    pub(crate) struct Fake {
        pub(crate) slow: Arc<Mutex<HashSet<String>>>,
        pub(crate) gate: Arc<tokio::sync::Semaphore>,
        unknown: Arc<Mutex<HashMap<String, Listing>>>,
        /// The roots whose front month was asked, in order.
        pub(crate) asked: Arc<Mutex<Vec<String>>>,
        pub(crate) subscribes: Arc<Mutex<Vec<String>>>,
        pub(crate) unsubscribes: Arc<Mutex<Vec<String>>>,
    }

    impl Plant for Fake {
        async fn front_month(&self, root: String, exchange: String) -> Result<String, Error> {
            self.asked.lock().unwrap().push(root.clone());
            let slow = self.slow.lock().unwrap().contains(&root);
            if slow {
                self.gate.acquire().await.unwrap().forget();
            }
            let listed = self.unknown.lock().unwrap().get(&root).cloned();
            match listed {
                Some(listed) => chosen(&root, &exchange, &listed),
                None => Ok(format!("{root}Z6")),
            }
        }

        async fn subscribe(&self, contract: String, _exchange: String) -> Result<(), Error> {
            self.subscribes.lock().unwrap().push(contract.clone());
            let slow = self.slow.lock().unwrap().contains(&contract);
            if slow {
                self.gate.acquire().await.unwrap().forget();
            }
            Ok(())
        }

        async fn unsubscribe(&self, contract: String, _exchange: String) {
            self.unsubscribes.lock().unwrap().push(contract);
        }
    }

    impl Default for Fake {
        fn default() -> Fake {
            Fake {
                slow: Arc::default(),
                gate: Arc::new(tokio::sync::Semaphore::new(0)),
                unknown: Arc::default(),
                asked: Arc::default(),
                subscribes: Arc::default(),
                unsubscribes: Arc::default(),
            }
        }
    }

    type Heard = Arc<Mutex<Vec<(u64, Live)>>>;
    type Listing = Vec<(String, Expiry)>;

    fn supervisor() -> Watches {
        Watches::new(watch::channel((0, State::Connecting)).0, Arc::new(Fronts::default()))
    }

    fn watch_root(supervisor: &mut Watches, subscriptions: &mut Subscriptions<Fake>, id: u64, root: &str, heard: &Heard) {
        watch_subject(supervisor, subscriptions, id, Subject::Front { root: root.into() }, heard);
    }

    fn watch_contract(
        supervisor: &mut Watches,
        subscriptions: &mut Subscriptions<Fake>,
        id: u64,
        contract: &str,
        heard: &Heard,
    ) {
        watch_subject(supervisor, subscriptions, id, Subject::Contract { contract: contract.into() }, heard);
    }

    fn watch_subject(
        supervisor: &mut Watches,
        subscriptions: &mut Subscriptions<Fake>,
        id: u64,
        subject: Subject,
        heard: &Heard,
    ) {
        let heard = heard.clone();
        let sink: Sink = Arc::new(move |live| heard.lock().unwrap().push((id, live)));
        supervisor.watched.insert(id, Watched::new(subject, "CME".into(), sink, false));
        supervisor.begin(id, subscriptions);
    }

    /// Take the plant's next answer, as the session's loop would.
    async fn settle_next(supervisor: &mut Watches, subscriptions: &mut Subscriptions<Fake>) {
        let settled = tokio::time::timeout(Duration::from_secs(5), subscriptions.settled.recv())
            .await
            .expect("an answer")
            .expect("the channel is open");
        supervisor.settle(settled, subscriptions);
    }

    fn print(seconds: i64) -> Trade {
        Trade { price: 7_625.0, size: 1, aggressor: None, at_nanos: seconds * 1_000_000_000 }
    }

    /// Opening a chart on a contract the session has not subscribed to
    /// takes a round trip or two to the plant, and the contracts already
    /// watched go on hearing their trades all the while.
    #[tokio::test]
    async fn subscribing_to_a_contract_does_not_hold_back_the_trades_of_another() {
        let fake = Fake::default();
        fake.slow.lock().unwrap().insert("NQZ6".into());
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();

        watch_root(&mut supervisor, &mut subscriptions, 1, "ES", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(heard.lock().unwrap().as_slice(), &[(1, Live::Watching { contract: "ESZ6".into() })]);

        // NQ's front month is answered, and its subscribe is held up.
        watch_root(&mut supervisor, &mut subscriptions, 2, "NQ", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        let pending = tokio::time::timeout(Duration::from_millis(50), subscriptions.settled.recv()).await;
        assert!(pending.is_err(), "the subscribe to NQZ6 is still out");
        assert_eq!(fake.subscribes.lock().unwrap().as_slice(), &["ESZ6", "NQZ6"]);

        for second in 0..3 {
            supervisor.deliver("ESZ6", "CME", Live::Trade(print(second)));
        }
        let trades = heard.lock().unwrap().iter().filter(|(id, live)| *id == 1 && matches!(live, Live::Trade(_))).count();
        assert_eq!(trades, 3, "ES trades arrive while NQ is being subscribed");
        assert!(heard.lock().unwrap().iter().all(|(id, _)| *id == 1), "NQ hears nothing yet");

        fake.gate.add_permits(1);
        settle_next(&mut supervisor, &mut subscriptions).await;
        supervisor.deliver("NQZ6", "CME", Live::Trade(print(10)));
        let nq: Vec<Live> = heard.lock().unwrap().iter().filter(|(id, _)| *id == 2).map(|(_, l)| l.clone()).collect();
        assert_eq!(nq, vec![Live::Watching { contract: "NQZ6".into() }, Live::Trade(print(10))]);
    }

    /// A subscription opens with the last trade before it, which is told
    /// apart from the trades after it: the market's price until it trades
    /// again, already in a finished bar the history plant gives.
    #[tokio::test]
    async fn the_trade_a_subscription_opens_with_is_the_last_one() {
        let fake = Fake::default();
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();
        watch_root(&mut supervisor, &mut subscriptions, 1, "ES", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;

        let mut opening = LastTrade::default();
        opening.symbol = Some("ESZ6".into());
        opening.exchange = Some("CME".into());
        opening.trade_price = Some(7_653.25);
        opening.trade_size = Some(1);
        opening.ssboe = Some(1_793_818_562);
        opening.is_snapshot = Some(true);
        supervisor.trade(&opening);
        opening.is_snapshot = None;
        opening.trade_price = Some(7_653.5);
        supervisor.trade(&opening);
        let lives: Vec<Live> = heard.lock().unwrap().iter().skip(1).map(|(_, live)| live.clone()).collect();
        let at_nanos = 1_793_818_562 * 1_000_000_000;
        let trade = |price| Trade { price, size: 1, aggressor: None, at_nanos };
        assert_eq!(lives, vec![Live::Last(trade(7_653.25)), Live::Trade(trade(7_653.5))]);
    }

    /// Two charts opened on one contract while it is being subscribed share
    /// one subscribe; a chart closed before its subscribe lands leaves
    /// nothing subscribed behind it.
    #[tokio::test]
    async fn a_contract_is_subscribed_once_and_let_go_when_nothing_waits_for_it() {
        let fake = Fake::default();
        fake.slow.lock().unwrap().insert("ESZ6".into());
        let mut subscriptions = Subscriptions::new(fake.clone());
        let mut supervisor = supervisor();
        let heard: Heard = Arc::default();

        watch_root(&mut supervisor, &mut subscriptions, 1, "ES", &heard);
        watch_root(&mut supervisor, &mut subscriptions, 2, "ES", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        fake.gate.add_permits(1);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.subscribes.lock().unwrap().as_slice(), &["ESZ6"]);
        assert_eq!(heard.lock().unwrap().len(), 2, "both are watching");

        // Another root, closed before its subscribe answers.
        fake.slow.lock().unwrap().insert("NQZ6".into());
        watch_root(&mut supervisor, &mut subscriptions, 3, "NQ", &heard);
        settle_next(&mut supervisor, &mut subscriptions).await;
        supervisor.end(3, &mut subscriptions);
        fake.gate.add_permits(1);
        settle_next(&mut supervisor, &mut subscriptions).await;
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.unsubscribes.lock().unwrap().as_slice(), &["NQZ6"]);
        assert!(heard.lock().unwrap().iter().all(|(id, _)| *id != 3));

        supervisor.end(1, &mut subscriptions);
        assert_eq!(fake.unsubscribes.lock().unwrap().len(), 1, "ES still has a watch");
        supervisor.end(2, &mut subscriptions);
        settle_next(&mut supervisor, &mut subscriptions).await;
        assert_eq!(fake.unsubscribes.lock().unwrap().as_slice(), &["NQZ6", "ESZ6"]);
    }
}
