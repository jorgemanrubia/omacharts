//! The one session a process has, and the thread that keeps it.
//!
//! A [`Client`] is a handle on it. Its calls block the caller, never the
//! session: each is a request handed to the runtime and waited for, with a
//! deadline. The connection itself belongs to a supervisor task that signs
//! in, hands out the plants, notices a drop, reconnects with a doubling wait
//! and subscribes again to whatever is still watched.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::request_search_symbols::{InstrumentType, Pattern};
use rithmic_rs::rti::LastTrade;
use rithmic_rs::{
    ConnectStrategy, RithmicConfig, RithmicEnv, RithmicError, RithmicHistoryPlant,
    RithmicHistoryPlantHandle, RithmicResponse, RithmicTickerPlant, RithmicTickerPlantHandle,
    TimeBarType, VolumeProfileMinuteBarsRequest,
};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{Bar, Error, LadderMinute, Live, Login, Period, Side, Trade, APP_NAME};

/// How long a call waits for the session to be signed in before it gives up
/// and says why it is not.
const PATIENCE: Duration = Duration::from_secs(20);

/// How long one ordinary request may take.
const REQUEST: Duration = Duration::from_secs(15);

/// How long a replay of bars may take. The history plant answers a large
/// window in pieces, and the whole of it can take a while; past this it is
/// not coming.
const REPLAY: Duration = Duration::from_secs(120);

/// The wait before reconnecting after a drop, doubling to the ceiling, and
/// put back to the floor by a session that stayed up for [`STABLE`].
const BACKOFF_FLOOR: Duration = Duration::from_secs(1);
const BACKOFF_CEILING: Duration = Duration::from_secs(60);
const STABLE: Duration = Duration::from_secs(60);

/// How long a front month is believed before it is asked for again. A roll
/// happens once a quarter; asking once an hour costs nothing.
const FRONT_FRESH: Duration = Duration::from_secs(3_600);

/// When Rithmic will not say which contract is the front month, the nearest
/// to expire is, until this many days before it expires. Traders roll the
/// quarterly index contracts about eight days early.
const ROLL_DAYS: i64 = 8;

/// What a forced logout is reported as.
const EVICTED: &str = "Rithmic signed this session out, as it does when the same login signs in \
                       somewhere else";

type Sink = Arc<dyn Fn(Live) + Send + Sync>;
type Source = Arc<dyn Fn() -> Result<Login, Error> + Send + Sync>;

/// The plants' handles, for requests from any thread. A handle's methods
/// take `&self`, so one is shared rather than one made per call.
struct Handles {
    ticker: RithmicTickerPlantHandle,
    history: RithmicHistoryPlantHandle,
}

#[derive(Clone)]
enum State {
    /// Signing in for the first time.
    Connecting,
    Up(Arc<Handles>),
    /// Lost, and reconnecting by itself.
    Down(Error),
    /// Will not try again by itself: refused, nothing to sign in with, or
    /// signed out.
    Stopped(Error),
}

enum Command {
    Watch { id: u64, root: String, exchange: String, sink: Sink },
    Unwatch { id: u64 },
    /// Cut the connection as a network failure would, to prove recovery.
    Sever,
    Stop { done: oneshot::Sender<()> },
}

/// Front months already asked for, by root and exchange.
#[derive(Default)]
struct Fronts(Mutex<HashMap<(String, String), (String, Instant)>>);

impl Fronts {
    fn get(&self, root: &str, exchange: &str) -> Option<String> {
        let fronts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let (contract, at) = fronts.get(&(root.to_string(), exchange.to_string()))?;
        (at.elapsed() < FRONT_FRESH).then(|| contract.clone())
    }

    fn put(&self, root: &str, exchange: &str, contract: &str) {
        let mut fronts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        fronts.insert((root.to_string(), exchange.to_string()), (contract.to_string(), Instant::now()));
    }

    fn forget(&self, root: &str, exchange: &str) {
        let mut fronts = self.0.lock().unwrap_or_else(|e| e.into_inner());
        fronts.remove(&(root.to_string(), exchange.to_string()));
    }

    fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

struct Inner {
    handle: tokio::runtime::Handle,
    /// Held to keep the threads alive, and shut down without waiting when
    /// the last client goes. Never dropped on one of its own threads: no
    /// task holds an `Inner`.
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    commands: mpsc::UnboundedSender<Command>,
    state: watch::Receiver<State>,
    fronts: Arc<Fronts>,
    next_watch: AtomicU64,
    closed: AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            runtime.shutdown_background();
        }
    }
}

/// The process's Rithmic session. Cheap to clone; every clone is the same
/// session.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

static SESSION: Mutex<Option<Client>> = Mutex::new(None);

/// The session, started if there is none yet.
///
/// `login` is asked for the login each time the session signs in, which is
/// when it starts and after every drop, so a password changed in the keyring
/// is the one the next sign-in uses. It runs on the session's thread and may
/// be slow; it must not block on the session.
///
/// Starting is the only thing here that costs anything — a runtime on two
/// threads, then a connection — and nothing calls this until a chart asks
/// Rithmic for something.
pub fn session(login: impl Fn() -> Result<Login, Error> + Send + Sync + 'static) -> Result<Client, Error> {
    let mut session = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(client) = session.as_ref()
        && !client.inner.closed.load(Ordering::Acquire)
    {
        return Ok(client.clone());
    }
    let client = Client::start(Arc::new(login))?;
    *session = Some(client.clone());
    Ok(client)
}

/// Is there a session in this process?
pub fn running() -> bool {
    SESSION
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .is_some_and(|client| !client.inner.closed.load(Ordering::Acquire))
}

/// Sign out of every plant and end the session, waiting a few seconds at
/// most for Rithmic to acknowledge. Whatever is watching hears
/// [`Error::Closed`], and the next call to [`session`] starts afresh.
pub fn shutdown() {
    let client = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(client) = client {
        client.close();
    }
}

impl Client {
    fn start(source: Source) -> Result<Client, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rithmic")
            .enable_all()
            .build()
            .map_err(|error| Error::Connection(error.to_string()))?;
        let (commands, inbox) = mpsc::unbounded_channel();
        let (state, states) = watch::channel(State::Connecting);
        let fronts = Arc::new(Fronts::default());
        let supervisor = Supervisor {
            source,
            inbox,
            state,
            fronts: fronts.clone(),
            watched: HashMap::new(),
            subscribed: HashMap::new(),
        };
        runtime.spawn(supervisor.run());
        Ok(Client {
            inner: Arc::new(Inner {
                handle: runtime.handle().clone(),
                runtime: Mutex::new(Some(runtime)),
                commands,
                state: states,
                fronts,
                next_watch: AtomicU64::new(1),
                closed: AtomicBool::new(false),
            }),
        })
    }

    fn close(&self) {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let (done, finished) = oneshot::channel();
        if self.inner.commands.send(Command::Stop { done }).is_ok() {
            let _ = self.inner.handle.block_on(async { tokio::time::timeout(Duration::from_secs(4), finished).await });
        }
    }

    /// Wait for the session to be signed in, or say why it is not.
    fn up(&self) -> Result<Arc<Handles>, Error> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let mut states = self.inner.state.clone();
        self.inner.handle.block_on(async move {
            let settled = tokio::time::timeout(
                PATIENCE,
                states.wait_for(|state| matches!(state, State::Up(_) | State::Stopped(_))),
            )
            .await
            .map(|waited| waited.map(|state| state.clone()));
            let state = match settled {
                Ok(Ok(state)) => state,
                Ok(Err(_)) => return Err(Error::Closed),
                Err(_) => states.borrow().clone(),
            };
            match state {
                State::Up(handles) => Ok(handles),
                State::Stopped(error) | State::Down(error) => Err(error),
                State::Connecting => Err(Error::Timeout),
            }
        })
    }

    /// Sign in now, if the session is not already, and wait for it.
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
    /// the plants are aborted, and the supervisor finds out as it would
    /// about a real drop, reports it and reconnects. For proving that a
    /// chart survives one; nothing in the app calls it.
    #[doc(hidden)]
    pub fn sever(&self) {
        let _ = self.inner.commands.send(Command::Sever);
    }

    /// Watch `root`'s front month on `exchange`: every trade on it is handed
    /// to `sink`, on the session's thread, until the [`Watch`] is dropped.
    ///
    /// Returns at once. The first thing `sink` hears is [`Live::Watching`]
    /// once the contract is subscribed, or [`Live::Lost`] saying why it could
    /// not be. A watch outlives a dropped connection: it hears `Lost`, and
    /// `Watching` again once the session is back.
    pub fn watch(&self, root: &str, exchange: &str, sink: impl Fn(Live) + Send + Sync + 'static) -> Watch {
        let id = self.inner.next_watch.fetch_add(1, Ordering::Relaxed);
        let sink: Sink = Arc::new(sink);
        if self.inner.closed.load(Ordering::Acquire) {
            sink(Live::Lost(Error::Closed));
        } else {
            let _ = self.inner.commands.send(Command::Watch {
                id,
                root: root.to_string(),
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

struct Watched {
    root: String,
    exchange: String,
    sink: Sink,
    /// The contract it is subscribed to, once it is.
    contract: Option<String>,
    /// Told of a loss it has not yet been told is over.
    told_lost: bool,
}

/// What the supervisor holds of a signed-in session.
struct Connection {
    ticker: RithmicTickerPlant,
    history: RithmicHistoryPlant,
    /// Its own handles, for the updates they receive.
    ticker_updates: RithmicTickerPlantHandle,
    history_updates: RithmicHistoryPlantHandle,
    handles: Arc<Handles>,
}

struct Supervisor {
    source: Source,
    inbox: mpsc::UnboundedReceiver<Command>,
    state: watch::Sender<State>,
    fronts: Arc<Fronts>,
    watched: HashMap<u64, Watched>,
    /// How many watches each subscribed contract has, by contract and
    /// exchange.
    subscribed: HashMap<(String, String), usize>,
}

/// Why the supervisor stopped looking after a connection.
enum Ended {
    Dropped(Error),
    /// Rithmic logged the session out, which it does when the same login
    /// signs in somewhere else. Signing straight back in would log that one
    /// out in turn, and the two would take turns for ever.
    Evicted,
    Stopped(oneshot::Sender<()>),
    /// Every client went away.
    Abandoned,
}

impl Supervisor {
    async fn run(mut self) {
        let mut backoff = BACKOFF_FLOOR;
        loop {
            let connection = match self.connect().await {
                Ok(connection) => connection,
                Err(error @ (Error::Refused(_) | Error::NoLogin(_))) => {
                    self.refuse(error).await;
                    return;
                }
                Err(error) => {
                    self.lose(error.clone());
                    if !self.idle(backoff).await {
                        return;
                    }
                    backoff = (backoff * 2).min(BACKOFF_CEILING);
                    continue;
                }
            };
            let up_since = Instant::now();
            let _ = self.state.send(State::Up(connection.handles.clone()));
            let ids: Vec<u64> = self.watched.keys().copied().collect();
            for id in ids {
                self.begin(id, &connection.handles).await;
            }
            match self.serve(connection).await {
                Ended::Dropped(error) => {
                    self.lose(error);
                    if up_since.elapsed() >= STABLE {
                        backoff = BACKOFF_FLOOR;
                    }
                    if !self.idle(backoff).await {
                        return;
                    }
                    backoff = (backoff * 2).min(BACKOFF_CEILING);
                }
                Ended::Evicted => {
                    self.refuse(Error::Refused(EVICTED.into())).await;
                    return;
                }
                Ended::Stopped(done) => {
                    let _ = done.send(());
                    return;
                }
                Ended::Abandoned => return,
            }
        }
    }

    /// Sign in to the ticker and history plants.
    async fn connect(&mut self) -> Result<Connection, Error> {
        let login = (self.source)()?;
        let credentials = login.credentials().await?;
        let config = RithmicConfig::builder(RithmicEnv::Test)
            .url(&credentials.url)
            // Only read by a strategy that alternates, and required regardless.
            .beta_url(&credentials.url)
            .user(&credentials.user)
            .password(&credentials.password)
            .system_name(&credentials.system)
            .app_name(APP_NAME)
            .app_version(env!("CARGO_PKG_VERSION"))
            .connect_attempt_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| Error::NoLogin("the Rithmic login is incomplete".into()))?;
        drop(credentials);

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
        Ok(Connection { ticker, history, ticker_updates, history_updates, handles })
    }

    /// Run a signed-in session until it drops or is stopped.
    async fn serve(&mut self, mut connection: Connection) -> Ended {
        let ended = loop {
            tokio::select! {
                command = self.inbox.recv() => match command {
                    None => break Ended::Abandoned,
                    Some(Command::Stop { done }) => break Ended::Stopped(done),
                    Some(Command::Watch { id, root, exchange, sink }) => {
                        self.watched.insert(id, Watched { root, exchange, sink, contract: None, told_lost: false });
                        self.begin(id, &connection.handles).await;
                    }
                    Some(Command::Unwatch { id }) => self.end(id, &connection.handles).await,
                    Some(Command::Sever) => {
                        connection.ticker_updates.abort();
                        connection.history_updates.abort();
                    }
                },
                update = connection.ticker_updates.subscription_receiver.recv() => match update {
                    Ok(update) => {
                        if matches!(update.message, RithmicMessage::ForcedLogout(_)) {
                            break Ended::Evicted;
                        }
                        if let Some(error) = dropped(&update) {
                            break Ended::Dropped(error);
                        }
                        if let RithmicMessage::LastTrade(trade) = &update.message {
                            self.trade(trade);
                        }
                    }
                    // Trades were missed. Everything watching starts over,
                    // which to a chart means a fresh snapshot.
                    Err(RecvError::Lagged(_)) => self.start_over(),
                    Err(RecvError::Closed) => break Ended::Dropped(Error::Connection("the ticker plant closed".into())),
                },
                update = connection.history_updates.subscription_receiver.recv() => match update {
                    Ok(update) => {
                        if matches!(update.message, RithmicMessage::ForcedLogout(_)) {
                            break Ended::Evicted;
                        }
                        if let Some(error) = dropped(&update) {
                            break Ended::Dropped(error);
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break Ended::Dropped(Error::Connection("the history plant closed".into())),
                },
            }
        };
        let graceful = matches!(ended, Ended::Stopped(_) | Ended::Abandoned);
        if graceful {
            let _ = within(Duration::from_secs(2), connection.ticker_updates.disconnect()).await;
            let _ = within(Duration::from_secs(2), connection.history_updates.disconnect()).await;
            let _ = self.state.send(State::Stopped(Error::Closed));
            for watched in self.watched.values() {
                (watched.sink)(Live::Lost(Error::Closed));
            }
        }
        // Dropping a plant does not stop it: aborted and waited for, or the
        // old session would stay open next to the new one.
        connection.ticker_updates.abort();
        connection.history_updates.abort();
        let _ = tokio::time::timeout(Duration::from_secs(3), connection.ticker.await_shutdown()).await;
        let _ = tokio::time::timeout(Duration::from_secs(3), connection.history.await_shutdown()).await;
        self.subscribed.clear();
        for watched in self.watched.values_mut() {
            watched.contract = None;
        }
        self.fronts.clear();
        ended
    }

    /// Subscribe watch `id` to its root's front month, and tell it so.
    async fn begin(&mut self, id: u64, handles: &Handles) {
        let Some((root, exchange)) = self.watched.get(&id).map(|w| (w.root.clone(), w.exchange.clone())) else {
            return;
        };
        let contract = match self.fronts.get(&root, &exchange) {
            Some(contract) => Ok(contract),
            None => front_month(&handles.ticker, &root, &exchange).await,
        };
        let contract = match contract {
            Ok(contract) => contract,
            Err(error) => return self.tell(id, Live::Lost(error)),
        };
        self.fronts.put(&root, &exchange, &contract);
        let key = (contract.clone(), exchange.clone());
        if !self.subscribed.contains_key(&key) {
            match within(REQUEST, handles.ticker.subscribe(&contract, &exchange)).await {
                Ok(response) => {
                    if let Some(error) = &response.error {
                        return self.tell(id, Live::Lost(rejected(error)));
                    }
                }
                Err(error) => return self.tell(id, Live::Lost(error)),
            }
        }
        *self.subscribed.entry(key).or_insert(0) += 1;
        if let Some(watched) = self.watched.get_mut(&id) {
            watched.contract = Some(contract.clone());
            watched.told_lost = false;
        }
        self.tell(id, Live::Watching { contract });
    }

    /// Forget watch `id`, and unsubscribe its contract if nothing else
    /// watches it.
    async fn end(&mut self, id: u64, handles: &Handles) {
        let Some(watched) = self.watched.remove(&id) else { return };
        let Some(contract) = watched.contract else { return };
        let key = (contract, watched.exchange);
        let Some(count) = self.subscribed.get_mut(&key) else { return };
        *count -= 1;
        if *count == 0 {
            self.subscribed.remove(&key);
            let _ = within(REQUEST, handles.ticker.unsubscribe(&key.0, &key.1)).await;
        }
    }

    fn tell(&self, id: u64, live: Live) {
        if let Some(watched) = self.watched.get(&id) {
            (watched.sink)(live);
        }
    }

    fn trade(&self, trade: &LastTrade) {
        let Some(found) = trade_of(trade) else { return };
        let (Some(symbol), Some(exchange)) = (trade.symbol.as_deref(), trade.exchange.as_deref()) else {
            return;
        };
        for watched in self.watched.values() {
            if watched.contract.as_deref() == Some(symbol) && watched.exchange == exchange {
                (watched.sink)(Live::Trade(found));
            }
        }
    }

    fn start_over(&self) {
        for watched in self.watched.values() {
            if let Some(contract) = &watched.contract {
                (watched.sink)(Live::Watching { contract: contract.clone() });
            }
        }
    }

    /// The session is down and will be tried again: say so, once, to
    /// everything watching.
    fn lose(&mut self, error: Error) {
        let _ = self.state.send(State::Down(error.clone()));
        for watched in self.watched.values_mut() {
            if !watched.told_lost {
                watched.told_lost = true;
                (watched.sink)(Live::Lost(error.clone()));
            }
        }
    }

    /// Wait out `backoff` before reconnecting, still taking commands. False
    /// when the session was stopped meanwhile.
    async fn idle(&mut self, backoff: Duration) -> bool {
        let sleep = tokio::time::sleep(backoff);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => return true,
                command = self.inbox.recv() => match command {
                    None => return false,
                    Some(Command::Stop { done }) => {
                        let _ = self.state.send(State::Stopped(Error::Closed));
                        for watched in self.watched.values() {
                            (watched.sink)(Live::Lost(Error::Closed));
                        }
                        let _ = done.send(());
                        return false;
                    }
                    Some(Command::Watch { id, root, exchange, sink }) => {
                        if let State::Down(error) = &*self.state.borrow() {
                            sink(Live::Lost(error.clone()));
                        }
                        self.watched.insert(id, Watched { root, exchange, sink, contract: None, told_lost: true });
                    }
                    Some(Command::Unwatch { id }) => {
                        self.watched.remove(&id);
                    }
                    Some(Command::Sever) => {}
                },
            }
        }
    }

    /// Rithmic refused the login, or there is none: say so to everything
    /// watching now or later, and try nothing more until somebody signs in
    /// again, which starts a new session.
    async fn refuse(&mut self, error: Error) {
        let _ = self.state.send(State::Stopped(error.clone()));
        for watched in self.watched.values() {
            (watched.sink)(Live::Lost(error.clone()));
        }
        while let Some(command) = self.inbox.recv().await {
            match command {
                Command::Watch { sink, .. } => sink(Live::Lost(error.clone())),
                Command::Unwatch { id } => {
                    self.watched.remove(&id);
                }
                Command::Sever => {}
                Command::Stop { done } => {
                    let _ = done.send(());
                    return;
                }
            }
        }
    }
}

async fn within<T>(
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
fn signed_in(outcome: Result<RithmicResponse, Error>) -> Result<(), Error> {
    match outcome {
        Ok(_) => Ok(()),
        Err(Error::Rejected(why)) => Err(Error::Refused(why)),
        Err(error) => Err(error),
    }
}

fn error_of(error: RithmicError) -> Error {
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
fn dropped(update: &RithmicResponse) -> Option<Error> {
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
    // The trade a subscription opens with is the last one before it, which
    // a snapshot already holds.
    if trade.is_snapshot == Some(true) {
        return None;
    }
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
/// say, the listed contract nearest to expiring and not within
/// [`ROLL_DAYS`] of it. Rithmic Test answers the question with "no data"
/// for every root, and lists the contracts.
async fn front_month(ticker: &RithmicTickerPlantHandle, root: &str, exchange: &str) -> Result<String, Error> {
    let asked = within(REQUEST, ticker.get_front_month_contract(root, exchange, false)).await?;
    if let RithmicMessage::ResponseFrontMonthContract(front) = &asked.message
        && let Some(contract) = front
            .trading_symbol
            .as_deref()
            .or(front.symbol.as_deref())
            .filter(|symbol| is_contract(root, symbol))
    {
        return Ok(contract.to_string());
    }
    let listed = within(
        REQUEST,
        ticker.search_symbols(root, Some(exchange), Some(root), Some(InstrumentType::Future), Some(Pattern::Equals)),
    )
    .await?;
    let listed: Vec<(String, i64)> = listed
        .iter()
        .filter_map(|response| match &response.message {
            RithmicMessage::ResponseSearchSymbols(found) => {
                let symbol = found.symbol.as_deref().filter(|symbol| is_contract(root, symbol))?;
                let expires = found.expiration_date.as_deref()?.parse::<i32>().ok()?;
                Some((symbol.to_string(), date_start(expires)?))
            }
            _ => None,
        })
        .collect();
    nearest(&listed, now_seconds())
        .ok_or_else(|| Error::NotFound(format!("contract of {root} on {exchange} trading now")))
}

/// The contract expiring soonest, but not within [`ROLL_DAYS`] of `now`.
fn nearest(listed: &[(String, i64)], now: i64) -> Option<String> {
    listed
        .iter()
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
mod tests {
    use super::*;

    #[test]
    fn dates_go_both_ways() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_760), (2026, 11, 3));
        assert_eq!(date_start(20261103), Some(20_760 * 86_400));
        assert_eq!(date_number(20_760 * 86_400 + 3_600), 20261103);
        assert_eq!(date_start(20261340), None);
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

    /// The nearest to expiring trades now, until the roll a week before it
    /// expires, and then the next one does.
    #[test]
    fn the_front_month_rolls_a_week_before_it_expires() {
        let day = 86_400;
        let listed = vec![
            ("ESH7".to_string(), 100 * day),
            ("ESZ6".to_string(), 10 * day),
            ("ESM7".to_string(), 190 * day),
        ];
        assert_eq!(nearest(&listed, 0).as_deref(), Some("ESZ6"));
        assert_eq!(nearest(&listed, 3 * day).as_deref(), Some("ESH7"));
        assert_eq!(nearest(&listed, 200 * day), None);
    }

    #[test]
    fn no_error_carries_a_password() {
        // Every constructor of an error here is built from Rithmic's words
        // or this crate's; none takes a login. What a refusal looks like:
        let refused = signed_in(Err(Error::Rejected("permission denied".into()))).unwrap_err();
        assert_eq!(refused, Error::Refused("permission denied".into()));
        assert_eq!(refused.to_string(), "Rithmic refused the login: permission denied");
    }
}
