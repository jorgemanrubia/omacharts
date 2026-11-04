//! One login's connection to Rithmic, and the thread that keeps it.
//!
//! Rithmic signs a session out when the same login signs in to the same
//! plant again, so a login is connected once, here, whatever wants it. A
//! [`Connection`] owns the login's runtime, where its sign-in comes from and
//! the one supervisor that signs its plants in, and it lends out two halves
//! of it: the data half ([`Client`]: the ticker and history plants) and the
//! trading half ([`Trading`]: the order and PnL plants). Either opens without
//! the other's plants, and each closes on its own; when both are closed the
//! connection ends and its threads stop.
//!
//! Both halves live by one policy. A drop is reported, and the half signs
//! in again by itself with a doubling wait the two share, after reading the
//! login again; a refusal is not retried, because the same password would
//! only be refused again; and a forced logout — the same login signing in
//! somewhere else — stops both, since signing back in would log the other
//! one out, and the two would take turns for ever.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rithmic_rs::rti::messages::RithmicMessage;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, mpsc, oneshot, watch};

use crate::client::{self, dropped, DataHalf, DataPlants, Fronts, Settled, Sink, Watched};
use crate::trading::{self, Pulse, PulseSink, TradingHalf, TradingPlants};
use crate::{App, Client, Error, Live, Login, Trading};

/// How long closing waits for a session to sign out before it lets go:
/// long enough for Rithmic to hear the logout, short enough that quitting
/// is not held up by a gateway that has gone quiet.
pub const SIGN_OUT_WAIT: Duration = Duration::from_secs(4);

/// The wait before reconnecting after a drop, doubling to the ceiling, and
/// put back to the floor by a half that stayed up for [`STABLE`].
const BACKOFF_FLOOR: Duration = Duration::from_secs(1);
const BACKOFF_CEILING: Duration = Duration::from_secs(60);
const STABLE: Duration = Duration::from_secs(60);

/// What a forced logout is reported as.
pub(crate) const EVICTED: &str = "Rithmic signed this session out, as it does when the same login signs in \
                                  somewhere else";

pub(crate) type Source = Arc<dyn Fn() -> Result<Login, Error> + Send + Sync>;

/// Where the plants are signed in: Rithmic, or a test's gateway.
#[derive(Clone)]
pub(crate) enum Dial {
    Rithmic,
    #[cfg(test)]
    Fake(Arc<tests::Gateway>),
}

/// Where a half stands, as the supervisor keeps it.
#[derive(Clone, Debug)]
pub(crate) enum Phase {
    /// Closed, or never opened.
    Off,
    /// Signing in.
    Opening,
    Up(Instant),
    /// Lost, and signing in again when the wait is out.
    Down,
    /// Will not try again by itself: refused, nothing to sign in with, or
    /// signed out from elsewhere.
    Stopped(Error),
}

impl Phase {
    fn stable(&self) -> bool {
        matches!(self, Phase::Up(since) if since.elapsed() >= STABLE)
    }
}

/// Why opening the trading half failed, and whether it is closed for it.
pub(crate) type Opened = Result<(), (Error, bool)>;

pub(crate) enum Command {
    OpenData { generation: u64 },
    CloseData { done: oneshot::Sender<()> },
    Watch { generation: u64, id: u64, root: String, exchange: String, sink: Sink },
    Unwatch { id: u64 },
    OpenTrading { pulse: PulseSink, reply: oneshot::Sender<Opened> },
    CloseTrading { done: oneshot::Sender<()> },
    /// Cut every plant as a network failure would, to prove recovery.
    Sever,
    /// What a forced logout on any plant does, for a test's fake plants.
    #[cfg(test)]
    ForcedOut,
    Stop { done: oneshot::Sender<()> },
}

/// Which halves are lent out, by generation: a handle from before a half
/// was closed is closed for good, even once the half opens again.
#[derive(Default)]
struct Halves {
    data: Option<u64>,
    trading: Option<u64>,
    next: u64,
}

impl Halves {
    fn mint(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

pub(crate) struct Inner {
    login: String,
    pub(crate) handle: tokio::runtime::Handle,
    /// Held to keep the threads alive, and shut down without waiting when
    /// the last handle goes. Never dropped on one of its own threads: no
    /// task holds an `Inner`.
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    pub(crate) commands: mpsc::UnboundedSender<Command>,
    source: Arc<Mutex<Source>>,
    halves: Mutex<Halves>,
    pub(crate) data: watch::Receiver<client::Told>,
    pub(crate) trading: watch::Receiver<trading::Standing>,
    pub(crate) fronts: Arc<Fronts>,
    pub(crate) book: Arc<Mutex<trading::State>>,
    pub(crate) accounts: Arc<Mutex<Vec<trading::Account>>>,
    pub(crate) next_watch: AtomicU64,
    ended: AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            runtime.shutdown_background();
        }
    }
}

impl Inner {
    /// Is the data half lent out as `generation` still open?
    pub(crate) fn data_open(&self, generation: u64) -> bool {
        self.halves.lock().unwrap_or_else(|e| e.into_inner()).data == Some(generation)
    }

    pub(crate) fn trading_open(&self, generation: u64) -> bool {
        self.halves.lock().unwrap_or_else(|e| e.into_inner()).trading == Some(generation)
    }
}

/// A login's connection. Cheap to clone; every clone is the same one.
#[derive(Clone)]
pub struct Connection {
    inner: Arc<Inner>,
}

static CONNECTIONS: Mutex<Option<HashMap<String, Connection>>> = Mutex::new(None);

fn registry() -> std::sync::MutexGuard<'static, Option<HashMap<String, Connection>>> {
    CONNECTIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The connection of the login `login`, started if there is none yet.
///
/// `app` is who Rithmic is told is signing in, and is the first caller's.
/// `source` is asked for the login each time a half signs in, which is
/// when it opens and after every drop, so a password changed in the
/// keyring is the one the next sign-in uses; the latest caller's source is
/// the one asked. It runs on the connection's thread and may be slow; it
/// must not block on the connection.
///
/// Starting is the only thing here that costs anything — a runtime on two
/// threads — and nothing signs in until a half is asked for.
pub fn connection(
    app: App,
    login: &str,
    source: impl Fn() -> Result<Login, Error> + Send + Sync + 'static,
) -> Result<Connection, Error> {
    let source: Source = Arc::new(source);
    let mut registry = registry();
    let connections = registry.get_or_insert_with(HashMap::new);
    if let Some(connection) = connections.get(login)
        && !connection.inner.ended.load(Ordering::Acquire)
    {
        *connection.inner.source.lock().unwrap_or_else(|e| e.into_inner()) = source;
        return Ok(connection.clone());
    }
    let connection = Connection::start(app, login, source, Dial::Rithmic)?;
    connections.insert(login.to_string(), connection.clone());
    Ok(connection)
}

/// Close both halves of the login `login`'s connection, if it has one,
/// waiting [`SIGN_OUT_WAIT`] at most for Rithmic to hear it.
pub fn close(login: &str) {
    let found = registry().as_ref().and_then(|connections| connections.get(login).cloned());
    if let Some(connection) = found {
        connection.close();
    }
}

/// Every connection with its data half open.
fn feeding() -> Vec<Connection> {
    let registry = registry();
    let Some(connections) = registry.as_ref() else { return Vec::new() };
    connections
        .values()
        .filter(|connection| connection.inner.halves.lock().unwrap_or_else(|e| e.into_inner()).data.is_some())
        .cloned()
        .collect()
}

/// Is market data open on any connection in this process?
pub fn running() -> bool {
    !feeding().is_empty()
}

/// Close the data half of every connection, waiting a few seconds at most
/// for Rithmic to acknowledge: what charting is done with. Whatever is
/// watching hears [`Error::Closed`], and the next [`Connection::data`]
/// opens it afresh. A trading half is left as it is.
pub fn shutdown() {
    client::forget_entitlements();
    for connection in feeding() {
        connection.close_data();
    }
}

impl Connection {
    pub(crate) fn of(inner: Arc<Inner>) -> Connection {
        Connection { inner }
    }

    pub(crate) fn start(app: App, login: &str, source: Source, dial: Dial) -> Result<Connection, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("rithmic")
            .enable_all()
            .build()
            .map_err(|error| Error::Connection(error.to_string()))?;
        let (commands, inbox) = mpsc::unbounded_channel();
        let (data_state, data) = watch::channel((0, client::State::Connecting));
        let (trading_state, trading) = watch::channel(trading::Standing::Connecting);
        let fronts = Arc::new(Fronts::default());
        let book = Arc::new(Mutex::new(trading::State::default()));
        let accounts = Arc::new(Mutex::new(Vec::new()));
        let source = Arc::new(Mutex::new(source));
        let (rounds, opened) = mpsc::unbounded_channel();
        let supervisor = Supervisor {
            app,
            dial,
            source: source.clone(),
            inbox,
            rounds,
            opened,
            data: DataHalf::new(data_state, fronts.clone()),
            trading: TradingHalf::new(trading_state, book.clone(), accounts.clone()),
            backoff: BACKOFF_FLOOR,
            retry: None,
        };
        runtime.spawn(supervisor.run());
        Ok(Connection {
            inner: Arc::new(Inner {
                login: login.to_string(),
                handle: runtime.handle().clone(),
                runtime: Mutex::new(Some(runtime)),
                commands,
                source,
                halves: Mutex::new(Halves::default()),
                data,
                trading,
                fronts,
                book,
                accounts,
                next_watch: AtomicU64::new(1),
                ended: AtomicBool::new(false),
            }),
        })
    }

    /// The login this is the connection of.
    pub fn login(&self) -> &str {
        &self.inner.login
    }

    /// The data half: the ticker and history plants, opened if they are not
    /// yet. Returns at once; the first call that needs Rithmic waits for the
    /// sign-in.
    pub fn data(&self) -> Client {
        let mut halves = self.inner.halves.lock().unwrap_or_else(|e| e.into_inner());
        let generation = match halves.data {
            Some(generation) => generation,
            None => {
                let generation = halves.mint();
                halves.data = Some(generation);
                let _ = self.inner.commands.send(Command::OpenData { generation });
                generation
            }
        };
        Client::of(self.inner.clone(), generation)
    }

    /// The trading half: the order and PnL plants, signed in if they are
    /// not yet, which this blocks for, so it is for a thread of its own.
    /// The accounts are listed, and what they push subscribed to, before it
    /// returns; what they hold now is [`Trading::refresh`]'s to ask.
    ///
    /// `pulse` hears, on the connection's thread, whenever an order,
    /// position or balance moves ([`Pulse::Moved`]), the half drops or is
    /// signed out from elsewhere ([`Pulse::Lost`]), and it is back after a
    /// drop ([`Pulse::Back`]), which is the cue to refresh and then to
    /// reconcile. It replaces the pulse of whoever opened it before.
    ///
    /// Signing in fails the first time and the half is closed again; once
    /// it has been up, a drop is reconnected, and orders are refused until
    /// it is back. An order is never sent again behind a trader's back.
    pub fn trading(&self, pulse: impl Fn(Pulse) + Send + Sync + 'static) -> Result<Trading, Error> {
        let pulse: PulseSink = Arc::new(pulse);
        let (reply, answer) = oneshot::channel();
        let generation = {
            let mut halves = self.inner.halves.lock().unwrap_or_else(|e| e.into_inner());
            let stopped = matches!(&*self.inner.trading.borrow(), trading::Standing::Stopped(_));
            let generation = match halves.trading {
                Some(generation) if !stopped => generation,
                _ => {
                    let generation = halves.mint();
                    halves.trading = Some(generation);
                    generation
                }
            };
            let _ = self.inner.commands.send(Command::OpenTrading { pulse, reply });
            generation
        };
        let outcome = self.inner.handle.block_on(answer).unwrap_or(Err((Error::Closed, true)));
        match outcome {
            Ok(()) => Ok(Trading::of(self.inner.clone(), generation)),
            Err((error, closed)) => {
                if closed {
                    self.forget_trading(generation);
                }
                Err(error)
            }
        }
    }

    /// The trading half failed to open as `generation` and is closed: if
    /// nothing else is open, the connection is over.
    fn forget_trading(&self, generation: u64) {
        let mut registry = registry();
        let mut halves = self.inner.halves.lock().unwrap_or_else(|e| e.into_inner());
        if halves.trading != Some(generation) {
            return;
        }
        halves.trading = None;
        if halves.data.is_none() {
            self.end(&mut registry);
            drop(halves);
            drop(registry);
            let (done, _) = oneshot::channel();
            let _ = self.inner.commands.send(Command::Stop { done });
        }
    }

    /// No half is open: take it out of the registry, so the next caller
    /// starts afresh. Called with the registry held.
    fn end(&self, registry: &mut Option<HashMap<String, Connection>>) {
        self.inner.ended.store(true, Ordering::Release);
        if let Some(connections) = registry.as_mut()
            && connections.get(&self.inner.login).is_some_and(|held| Arc::ptr_eq(&held.inner, &self.inner))
        {
            connections.remove(&self.inner.login);
        }
    }

    /// Close one half, or the whole connection when it is the last, and
    /// wait [`SIGN_OUT_WAIT`] at most.
    fn close_half(&self, data: bool, expected: Option<u64>) {
        let command = {
            let mut registry = registry();
            let mut halves = self.inner.halves.lock().unwrap_or_else(|e| e.into_inner());
            let held = match data {
                true => halves.data,
                false => halves.trading,
            };
            // Closed already, or closed and opened again by somebody else.
            if held.is_none() || expected.is_some_and(|expected| held != Some(expected)) {
                return;
            }
            let (_, other) = match data {
                true => (halves.data.take(), halves.trading),
                false => (halves.trading.take(), halves.data),
            };
            let (done, finished) = oneshot::channel();
            let command = match (other.is_none(), data) {
                (true, _) => {
                    self.end(&mut registry);
                    Command::Stop { done }
                }
                (false, true) => Command::CloseData { done },
                (false, false) => Command::CloseTrading { done },
            };
            (command, finished)
        };
        let (command, finished) = command;
        if self.inner.commands.send(command).is_ok() {
            let _ = self.inner.handle.block_on(async { tokio::time::timeout(SIGN_OUT_WAIT, finished).await });
        }
    }

    /// Sign the data half out of its plants. Whatever is watching hears
    /// [`Error::Closed`].
    pub fn close_data(&self) {
        self.close_half(true, None);
    }

    /// Sign the trading half out of its plants.
    pub fn close_trading(&self) {
        self.close_half(false, None);
    }

    /// Sign the trading half out if it is still the one lent as
    /// `generation`.
    pub(crate) fn close_trading_as(&self, generation: u64) {
        self.close_half(false, Some(generation));
    }

    /// Sign both halves out and end the connection, waiting
    /// [`SIGN_OUT_WAIT`] at most.
    pub fn close(&self) {
        let finished = {
            let mut registry = registry();
            let mut halves = self.inner.halves.lock().unwrap_or_else(|e| e.into_inner());
            halves.data = None;
            halves.trading = None;
            self.end(&mut registry);
            let (done, finished) = oneshot::channel();
            self.inner.commands.send(Command::Stop { done }).ok().map(|_| finished)
        };
        if let Some(finished) = finished {
            let _ = self.inner.handle.block_on(async { tokio::time::timeout(SIGN_OUT_WAIT, finished).await });
        }
    }

    /// Cut every plant the way a dropped network does, from underneath, for
    /// proving that both halves come back. Nothing in the app calls it.
    #[doc(hidden)]
    pub fn sever(&self) {
        let _ = self.inner.commands.send(Command::Sever);
    }
}

/// What a round of signing in came back with, for each half it was for.
pub(crate) struct Round {
    data: Option<Result<DataPlants, Error>>,
    trading: Option<Result<TradingPlants, Error>>,
}

impl Round {
    /// Nobody wanted it after all: sign out of whatever signed in.
    async fn discard(self) {
        if let Some(Ok(plants)) = self.data {
            plants.close(true).await;
        }
        if let Some(Ok(plants)) = self.trading {
            plants.close(true).await;
        }
    }
}

/// Sign in to the halves asked for, with the login read once for both.
async fn open(app: App, dial: Dial, source: Source, data: bool, trading: bool) -> Round {
    let credentials = match source() {
        Ok(login) => login.credentials().await,
        Err(error) => Err(error),
    };
    let credentials = match credentials {
        Ok(credentials) => credentials,
        Err(error) => {
            return Round { data: data.then(|| Err(error.clone())), trading: trading.then_some(Err(error)) };
        }
    };
    let (data, trading) = match dial {
        Dial::Rithmic => (
            match data {
                true => Some(client::connect(app, &credentials).await),
                false => None,
            },
            match trading {
                true => Some(trading::connect(app, &credentials).await),
                false => None,
            },
        ),
        #[cfg(test)]
        Dial::Fake(gateway) => gateway.dial(data, trading),
    };
    drop(credentials);
    Round { data, trading }
}

/// What woke the supervisor.
enum Event {
    Command(Option<Command>),
    Round(Round),
    Retry,
    Ticker(Result<rithmic_rs::RithmicResponse, RecvError>),
    History(Result<rithmic_rs::RithmicResponse, RecvError>),
    Settled(Settled),
    Orders(Result<rithmic_rs::RithmicResponse, RecvError>),
    Pnl(Result<rithmic_rs::RithmicResponse, RecvError>),
}

async fn next<T: Clone>(receiver: Option<&mut broadcast::Receiver<T>>) -> Result<T, RecvError> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn settled(receiver: Option<&mut mpsc::UnboundedReceiver<Settled>>) -> Option<Settled> {
    match receiver {
        Some(receiver) => receiver.recv().await,
        None => std::future::pending().await,
    }
}

async fn until(at: Option<tokio::time::Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

pub(crate) struct Supervisor {
    app: App,
    dial: Dial,
    source: Arc<Mutex<Source>>,
    inbox: mpsc::UnboundedReceiver<Command>,
    rounds: mpsc::UnboundedSender<Round>,
    opened: mpsc::UnboundedReceiver<Round>,
    data: DataHalf,
    trading: TradingHalf,
    /// The wait before the next retry, shared by both halves.
    backoff: Duration,
    /// When the halves that are down sign in again.
    retry: Option<tokio::time::Instant>,
}

impl Supervisor {
    async fn run(mut self) {
        loop {
            let (ticker, history, answers) = match &mut self.data.live {
                Some((plants, subscriptions)) => (
                    Some(&mut plants.ticker_updates),
                    Some(&mut plants.history_updates),
                    Some(&mut subscriptions.settled),
                ),
                None => (None, None, None),
            };
            let (orders, pnl) = match &mut self.trading.live {
                Some(plants) => (Some(&mut plants.order_updates), Some(&mut plants.pnl_updates)),
                None => (None, None),
            };
            let event = tokio::select! {
                command = self.inbox.recv() => Event::Command(command),
                Some(round) = self.opened.recv() => Event::Round(round),
                _ = until(self.retry) => Event::Retry,
                update = next(ticker) => Event::Ticker(update),
                update = next(history) => Event::History(update),
                Some(answer) = settled(answers) => Event::Settled(answer),
                update = next(orders) => Event::Orders(update),
                update = next(pnl) => Event::Pnl(update),
            };
            match event {
                // Every handle went away.
                Event::Command(None) => return self.stop().await,
                Event::Command(Some(Command::Stop { done })) => {
                    self.stop().await;
                    let _ = done.send(());
                    return;
                }
                Event::Command(Some(command)) => self.command(command).await,
                Event::Round(round) => self.adopt(round),
                Event::Retry => {
                    self.retry = None;
                    self.reopen();
                }
                Event::Ticker(update) => match update {
                    Ok(update) => {
                        if matches!(update.message, RithmicMessage::ForcedLogout(_)) {
                            self.evict();
                        } else if let Some(error) = dropped(&update) {
                            self.data_lost(error);
                        } else if let RithmicMessage::LastTrade(trade) = &update.message {
                            self.data.watches.trade(trade);
                        } else if let RithmicMessage::OrderPriceLimits(limits) = &update.message {
                            client::heard_limits(limits);
                        }
                    }
                    // Trades were missed. Everything watching starts over,
                    // which to a chart means a fresh snapshot.
                    Err(RecvError::Lagged(_)) => self.data.watches.start_over(),
                    Err(RecvError::Closed) => self.data_lost(Error::Connection("the ticker plant closed".into())),
                },
                Event::History(update) => match update {
                    Ok(update) => {
                        if matches!(update.message, RithmicMessage::ForcedLogout(_)) {
                            self.evict();
                        } else if let Some(error) = dropped(&update) {
                            self.data_lost(error);
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => self.data_lost(Error::Connection("the history plant closed".into())),
                },
                Event::Settled(answer) => {
                    if let Some((_, subscriptions)) = &mut self.data.live {
                        self.data.watches.settle(answer, subscriptions);
                    }
                }
                Event::Orders(update) | Event::Pnl(update) => match update {
                    Ok(update) => {
                        if matches!(update.message, RithmicMessage::ForcedLogout(_)) {
                            self.evict();
                        } else if let Some(error) = dropped(&update) {
                            self.trading_lost(error);
                        } else {
                            self.trading.apply(&update.message);
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => self.trading_lost(Error::Connection("the session closed".into())),
                },
            }
        }
    }

    async fn command(&mut self, command: Command) {
        match command {
            Command::OpenData { generation } => {
                if matches!(self.data.phase, Phase::Off) {
                    self.data.watches.generation = generation;
                    self.data.phase = Phase::Opening;
                    self.data.watches.set(client::State::Connecting);
                    self.sign_in(true, false);
                }
            }
            Command::CloseData { done } => {
                self.data.close().await;
                let _ = done.send(());
            }
            Command::Watch { generation, id, root, exchange, sink } => {
                if generation != self.data.watches.generation {
                    return sink(Live::Lost(Error::Closed));
                }
                match &self.data.phase {
                    Phase::Off => sink(Live::Lost(Error::Closed)),
                    Phase::Stopped(error) => sink(Live::Lost(error.clone())),
                    _ => {
                        let told_lost = match self.data.watches.now() {
                            client::State::Down(error) => {
                                sink(Live::Lost(error));
                                true
                            }
                            _ => false,
                        };
                        let watched = Watched { root, exchange, sink, contract: None, told_lost };
                        self.data.watches.watched.insert(id, watched);
                        if let Some((_, subscriptions)) = &mut self.data.live {
                            self.data.watches.begin(id, subscriptions);
                        }
                    }
                }
            }
            Command::Unwatch { id } => match &mut self.data.live {
                Some((_, subscriptions)) => self.data.watches.end(id, subscriptions),
                None => {
                    self.data.watches.watched.remove(&id);
                }
            },
            Command::OpenTrading { pulse, reply } => {
                self.trading.pulse = Some(pulse);
                match self.trading.phase {
                    // Up, or coming back by itself: the handle is good now.
                    Phase::Up(_) | Phase::Down => {
                        let _ = reply.send(Ok(()));
                    }
                    Phase::Opening if self.trading.been_up => {
                        let _ = reply.send(Ok(()));
                    }
                    Phase::Opening => self.trading.waiting.push(reply),
                    Phase::Off | Phase::Stopped(_) => {
                        self.trading.reset();
                        self.trading.phase = Phase::Opening;
                        self.trading.waiting.push(reply);
                        self.sign_in(false, true);
                    }
                }
            }
            Command::CloseTrading { done } => {
                self.trading.close().await;
                let _ = done.send(());
            }
            #[cfg(test)]
            Command::ForcedOut => self.evict(),
            Command::Sever => {
                if let Some((plants, _)) = &self.data.live {
                    plants.abort();
                }
                if let Some(plants) = &self.trading.live {
                    plants.abort();
                }
            }
            Command::Stop { .. } => unreachable!("the loop stops"),
        }
    }

    /// Sign every half out, as the connection ends.
    async fn stop(&mut self) {
        self.data.close().await;
        self.trading.close().await;
    }

    /// Start a round of signing in for the halves named, on a task of its
    /// own, so that what is already up goes on being served meanwhile.
    fn sign_in(&self, data: bool, trading: bool) {
        let source = self.source.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (app, dial, rounds) = (self.app, self.dial.clone(), self.rounds.clone());
        tokio::spawn(async move {
            let round = open(app, dial, source, data, trading).await;
            if let Err(mpsc::error::SendError(round)) = rounds.send(round) {
                round.discard().await;
            }
        });
    }

    /// Sign in again whatever is down.
    fn reopen(&mut self) {
        let data = matches!(self.data.phase, Phase::Down);
        let trading = matches!(self.trading.phase, Phase::Down);
        if data {
            self.data.phase = Phase::Opening;
        }
        if trading {
            self.trading.phase = Phase::Opening;
        }
        if data || trading {
            self.sign_in(data, trading);
        }
    }

    /// Try again after the shared wait, which doubles each time; a half
    /// that stayed up a while puts it back to the floor first. A second
    /// half going down before the retry joins the same one.
    fn later(&mut self, stable: bool) {
        if stable {
            self.backoff = BACKOFF_FLOOR;
        }
        if self.retry.is_none() {
            self.retry = Some(tokio::time::Instant::now() + self.backoff);
            self.backoff = (self.backoff * 2).min(BACKOFF_CEILING);
        }
    }

    fn adopt(&mut self, round: Round) {
        if let Some(outcome) = round.data {
            self.adopt_data(outcome);
        }
        if let Some(outcome) = round.trading {
            self.adopt_trading(outcome);
        }
    }

    fn adopt_data(&mut self, outcome: Result<DataPlants, Error>) {
        if !matches!(self.data.phase, Phase::Opening) {
            if let Ok(plants) = outcome {
                tokio::spawn(plants.close(true));
            }
            return;
        }
        match outcome {
            Ok(plants) => {
                self.data.phase = Phase::Up(Instant::now());
                self.data.up(plants);
            }
            Err(error @ (Error::Refused(_) | Error::NoLogin(_))) => {
                self.data.phase = Phase::Stopped(error.clone());
                self.data.watches.refuse(error);
            }
            Err(error) => {
                self.data.phase = Phase::Down;
                self.data.watches.lose(error);
                self.later(false);
            }
        }
    }

    fn adopt_trading(&mut self, outcome: Result<TradingPlants, Error>) {
        if !matches!(self.trading.phase, Phase::Opening) {
            if let Ok(plants) = outcome {
                tokio::spawn(plants.close(true));
            }
            return;
        }
        match outcome {
            Ok(plants) => {
                self.trading.phase = Phase::Up(Instant::now());
                self.trading.up(plants);
            }
            // Never up: the one who asked hears why, and it is closed.
            Err(error) if !self.trading.been_up => {
                self.trading.phase = Phase::Off;
                self.trading.fail(error, true);
            }
            Err(error @ (Error::Refused(_) | Error::NoLogin(_))) => {
                self.trading.phase = Phase::Stopped(error.clone());
                self.trading.fail(error, false);
            }
            Err(error) => {
                self.trading.phase = Phase::Down;
                self.trading.fail(error, false);
                self.later(false);
            }
        }
    }

    fn data_lost(&mut self, error: Error) {
        let Some((plants, _)) = self.data.live.take() else { return };
        tokio::spawn(plants.close(false));
        let stable = self.data.phase.stable();
        self.data.phase = Phase::Down;
        self.data.watches.lose(error);
        self.later(stable);
    }

    fn trading_lost(&mut self, error: Error) {
        let Some(plants) = self.trading.live.take() else { return };
        tokio::spawn(plants.close(false));
        let stable = self.trading.phase.stable();
        self.trading.phase = Phase::Down;
        self.trading.lose(error);
        self.later(stable);
    }

    /// Rithmic logged a half out, which it does when the same login signs
    /// in somewhere else. Both halves stop: signing straight back in would
    /// log that one out in turn.
    fn evict(&mut self) {
        let error = Error::Refused(EVICTED.into());
        self.retry = None;
        if !matches!(self.data.phase, Phase::Off) {
            if let Some((plants, _)) = self.data.live.take() {
                tokio::spawn(plants.close(false));
            }
            self.data.phase = Phase::Stopped(error.clone());
            self.data.watches.refuse(error.clone());
        }
        if !matches!(self.trading.phase, Phase::Off) {
            if let Some(plants) = self.trading.live.take() {
                tokio::spawn(plants.close(false));
            }
            let never = !self.trading.been_up;
            self.trading.phase = if never { Phase::Off } else { Phase::Stopped(error.clone()) };
            self.trading.fail(error, never);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: App = App { name: "test", version: "0" };

    fn nobody() -> Result<Login, Error> {
        Err(Error::NoLogin("a test signs in to nothing".into()))
    }

    /// One login is one connection, however many ask for it; another login
    /// is another.
    #[test]
    fn a_login_has_one_connection() {
        let first = connection(APP, "test-one", nobody).unwrap();
        let again = connection(APP, "test-one", nobody).unwrap();
        let other = connection(APP, "test-other", nobody).unwrap();
        assert!(Arc::ptr_eq(&first.inner, &again.inner));
        assert!(!Arc::ptr_eq(&first.inner, &other.inner));
        assert_eq!(first.login(), "test-one");
        first.close();
        other.close();
        let after = connection(APP, "test-one", nobody).unwrap();
        assert!(!Arc::ptr_eq(&first.inner, &after.inner), "a closed connection is not handed out again");
        after.close();
    }

    /// Both halves are lent from the one connection, and the data half is
    /// one session however many charts ask.
    #[test]
    fn the_halves_share_the_connection() {
        let connection = connection(APP, "test-halves", nobody).unwrap();
        let data = connection.data();
        let again = connection.data();
        assert!(Arc::ptr_eq(&data.inner, &again.inner));
        assert_eq!(data.generation, again.generation);
        // Nothing to sign in with: the data half says so, and is not
        // retried; the trading half fails to open, and is closed for it.
        assert!(matches!(data.sign_in(), Err(Error::NoLogin(_))));
        assert!(matches!(connection.trading(|_| {}), Err(Error::NoLogin(_))));
        assert!(connection.inner.halves.lock().unwrap().trading.is_none());
        assert!(connection.inner.halves.lock().unwrap().data.is_some());
        connection.close();
        assert!(matches!(data.sign_in(), Err(Error::Closed)));
    }

    /// Closing the data half leaves the connection to the trading half, and
    /// a client from before the close stays closed when it opens again.
    #[test]
    fn closing_a_half_closes_its_handles_for_good() {
        let connection = connection(APP, "test-close", nobody).unwrap();
        let before = connection.data();
        // Held open by a trading half that is still lent out.
        connection.inner.halves.lock().unwrap().trading = Some(999);
        connection.close_data();
        assert!(!connection.inner.ended.load(Ordering::Acquire), "the trading half keeps it");
        assert!(matches!(before.sign_in(), Err(Error::Closed)));
        let after = connection.data();
        assert_ne!(before.generation, after.generation);
        assert!(matches!(after.sign_in(), Err(Error::NoLogin(_))));
        assert!(matches!(before.sign_in(), Err(Error::Closed)), "still closed");
        connection.close_trading();
        connection.close_data();
        assert!(connection.inner.ended.load(Ordering::Acquire), "nothing is open: it is over");
    }

    /// A gateway that signs in to fake plants, and keeps their channels to
    /// push on or to cut.
    #[derive(Default)]
    pub(crate) struct Gateway {
        /// What each sign-in was for, in order.
        dialled: Mutex<Vec<&'static str>>,
        ticker: Mutex<Option<broadcast::Sender<rithmic_rs::RithmicResponse>>>,
        orders: Mutex<Option<broadcast::Sender<rithmic_rs::RithmicResponse>>>,
        plant: client::tests::Fake,
    }

    type Dialled = (Option<Result<DataPlants, Error>>, Option<Result<TradingPlants, Error>>);

    impl Gateway {
        pub(crate) fn dial(&self, data: bool, trading: bool) -> Dialled {
            let data = data.then(|| {
                self.dialled.lock().unwrap().push("data");
                let (ticker, updates) = broadcast::channel(64);
                *self.ticker.lock().unwrap() = Some(ticker);
                Ok(DataPlants::fake(updates, self.plant.clone()))
            });
            let trading = trading.then(|| {
                self.dialled.lock().unwrap().push("trading");
                let (orders, updates) = broadcast::channel(64);
                *self.orders.lock().unwrap() = Some(orders);
                Ok(TradingPlants::fake(updates, Vec::new()))
            });
            (data, trading)
        }

        fn dialled(&self) -> Vec<&'static str> {
            self.dialled.lock().unwrap().clone()
        }

        /// The network goes: every plant's channel closes.
        fn cut(&self) {
            self.ticker.lock().unwrap().take();
            self.orders.lock().unwrap().take();
        }
    }

    /// A connection to the gateway, and how many times it read the login.
    fn faked(gateway: &Arc<Gateway>) -> (Connection, Arc<AtomicU64>) {
        let reads = Arc::new(AtomicU64::new(0));
        let counted = reads.clone();
        let source: Source = Arc::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(Login::Given(crate::Credentials {
                user: "trader".into(),
                password: "secret".into(),
                system: crate::TEST_SYSTEM.into(),
                url: crate::TEST_URL.into(),
            }))
        });
        (Connection::start(APP, "fake", source, Dial::Fake(gateway.clone())).unwrap(), reads)
    }

    fn next<T>(heard: &std::sync::mpsc::Receiver<T>) -> T {
        heard.recv_timeout(Duration::from_secs(10)).expect("heard in time")
    }

    /// Trading opens without the data plants, and data without the trading
    /// ones. A drop takes both down; they come back together, on one read
    /// of the login: the data half subscribes again to what is watched, and
    /// the trading half says it is back.
    #[test]
    fn a_reconnect_subscribes_data_again_and_tells_trading() {
        let gateway = Arc::new(Gateway::default());
        let (connection, reads) = faked(&gateway);
        let (pulse, pulses) = std::sync::mpsc::channel();
        let trading = connection.trading(move |news| pulse.send(news).unwrap()).expect("signed in");
        assert_eq!(gateway.dialled(), ["trading"], "trading opened no data plants");

        let (heard, hearing) = std::sync::mpsc::channel();
        let watch = connection.data().watch("ES", "CME", move |live| heard.send(live).unwrap());
        assert_eq!(next(&hearing), Live::Watching { contract: "ESZ6".into() });
        assert_eq!(gateway.dialled(), ["trading", "data"], "data opened no trading plants");
        assert_eq!(reads.load(Ordering::SeqCst), 2);

        gateway.cut();
        assert!(matches!(next(&hearing), Live::Lost(Error::Connection(_))));
        assert_eq!(next(&hearing), Live::Watching { contract: "ESZ6".into() }, "subscribed again");
        assert!(matches!(next(&pulses), Pulse::Lost(Error::Connection(_))));
        assert_eq!(next(&pulses), Pulse::Back);
        assert!(trading.trouble().is_none(), "trading is back");
        assert_eq!(gateway.dialled(), ["trading", "data", "data", "trading"]);
        assert_eq!(reads.load(Ordering::SeqCst), 3, "one read of the login for both halves");
        assert_eq!(gateway.plant.subscribes.lock().unwrap().as_slice(), ["ESZ6", "ESZ6"]);

        drop(watch);
        connection.close();
        assert_eq!(trading.trouble(), Some(Error::Closed));
    }

    /// Rithmic signing one half out stops both, and neither signs in again
    /// by itself: that would sign the other session out in turn. Opening
    /// trading again is a trader's choice, and signs in afresh.
    #[test]
    fn a_forced_logout_stops_both_halves() {
        let gateway = Arc::new(Gateway::default());
        let (connection, _) = faked(&gateway);
        let (pulse, pulses) = std::sync::mpsc::channel();
        let trading = connection.trading(move |news| pulse.send(news).unwrap()).expect("signed in");
        let (heard, hearing) = std::sync::mpsc::channel();
        let watch = connection.data().watch("ES", "CME", move |live| heard.send(live).unwrap());
        assert_eq!(next(&hearing), Live::Watching { contract: "ESZ6".into() });

        // What a `ForcedLogout` on any plant does; `rithmic-rs` builds no
        // update outside its own plants.
        connection.inner.commands.send(Command::ForcedOut).unwrap();
        let evicted = Error::Refused(EVICTED.into());
        assert_eq!(next(&hearing), Live::Lost(evicted.clone()));
        assert_eq!(next(&pulses), Pulse::Lost(evicted.clone()));
        assert_eq!(trading.trouble(), Some(evicted.clone()));
        std::thread::sleep(BACKOFF_FLOOR + Duration::from_millis(500));
        assert_eq!(gateway.dialled(), ["trading", "data"], "nothing signed in again");
        assert!(matches!(connection.data().sign_in(), Err(Error::Refused(_))));

        let again = connection.trading(|_| {}).expect("signed in again");
        assert!(again.trouble().is_none());
        assert_eq!(trading.trouble(), Some(Error::Closed), "the one from before stays signed out");
        assert_eq!(gateway.dialled(), ["trading", "data", "trading"]);
        drop(watch);
        connection.close();
    }

    /// Signing trading out leaves the data half watching; closing the
    /// connection ends both, and what was watching hears so.
    #[test]
    fn closing_trading_leaves_data_and_closing_both_ends_it() {
        let gateway = Arc::new(Gateway::default());
        let (connection, _) = faked(&gateway);
        let trading = connection.trading(|_| {}).expect("signed in");
        let (heard, hearing) = std::sync::mpsc::channel();
        let watch = connection.data().watch("ES", "CME", move |live| heard.send(live).unwrap());
        assert_eq!(next(&hearing), Live::Watching { contract: "ESZ6".into() });

        trading.sign_out();
        assert_eq!(trading.trouble(), Some(Error::Closed));
        assert!(!connection.inner.ended.load(Ordering::Acquire), "the data half keeps it");
        assert!(hearing.recv_timeout(Duration::from_millis(300)).is_err(), "the watch heard nothing");

        connection.close();
        assert_eq!(next(&hearing), Live::Lost(Error::Closed));
        assert!(connection.inner.ended.load(Ordering::Acquire));
        drop(watch);
    }

    /// An order on a chart fed from elsewhere still finds its contract: the
    /// trading half asks the data half, opening it to ask, watches only
    /// until it is answered, and remembers the answer once the data half
    /// has closed.
    #[test]
    fn trading_finds_a_front_month_no_chart_asked_for() {
        let gateway = Arc::new(Gateway::default());
        let (connection, _) = faked(&gateway);
        let trading = connection.trading(|_| {}).expect("signed in");
        assert_eq!(client::found_front_month("RTY", "CME"), None);

        let (found, finding) = std::sync::mpsc::channel();
        let once = found.clone();
        assert_eq!(trading.front_month("RTY", "CME", move || once.send(()).unwrap()), None, "never waits");
        assert_eq!(trading.front_month("RTY", "CME", move || found.send(()).unwrap()), None);
        next(&finding);
        assert!(finding.recv_timeout(Duration::from_millis(300)).is_err(), "asked once");
        assert_eq!(gateway.dialled(), ["trading", "data"], "the data half opened to ask");
        assert_eq!(trading.front_month("RTY", "CME", || panic!("known")), Some("RTYZ6".into()));
        assert_eq!(gateway.plant.subscribes.lock().unwrap().as_slice(), ["RTYZ6"]);

        connection.close_data();
        let kept = client::found_front_month("RTY", "CME");
        assert_eq!(kept, Some(("RTYZ6".into(), true)), "kept past the data half");
        assert_eq!(trading.front_month("RTY", "CME", || panic!("known")), Some("RTYZ6".into()));
        connection.close();
    }
}
