//! The connection actor and its handle.
//!
//! One thread owns the socket. Every request goes through it, and every
//! response is routed back by `header.id` — never by service alone, which was
//! the single hardest lesson from the TypeScript client. One-shot calls get
//! the first frame for their id; subscriptions get a stream. The actor lives
//! as long as its socket: when that dies, every one-shot fails, every stream
//! ends, and the caller builds a new client.
//!
//! Blocking, like the rest of this application: the thread reads frames with
//! a short read timeout, and between reads it picks up queued commands and
//! looks at its watchdog. That timeout is also what the heartbeat rule is
//! built on — the gateway sends a heartbeat every two seconds and the socket
//! is torn down after thirty seconds of silence, which a blocking read with a
//! read timeout gives directly.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::config::{
    assert_gateway_matches, assert_trading_system_allowed, TradingSystem, TOS_WEB_ORIGIN,
};
use crate::error::{Error, Result};
use crate::patch::DocumentStore;
use crate::protocol::{
    decode_inbound, message_of, Inbound, Request, Response, CONNECTION_REQUEST_MESSAGE,
};
use crate::redact;
use crate::services::chart::{chart_request, ChartParams};
use crate::services::is_replayable;
use crate::services::login::{login_request, schwab_login_request, LoginBody};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Default time a one-shot [`Client::request`] waits for its first frame.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Bound on the initial handshake + login in [`Client::connect`].
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a read blocks before the actor gets its turn to send what it has
/// been asked to send and to look at its watchdog.
const READ_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub trading_system: TradingSystem,
    /// The gateway to connect to (e.g. the one captured from the browser session).
    pub gateway_url: String,
    pub allow_live_trading: bool,
    /// Close the socket when no frame (heartbeat included) arrives for this
    /// long. The gateway heartbeats every ~2 s; 30 s mirrors the ToS web UI.
    pub heartbeat_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            trading_system: TradingSystem::PaperMoney,
            gateway_url: String::new(),
            allow_live_trading: false,
            heartbeat_timeout: Duration::from_secs(30),
        }
    }
}

type Reply = Sender<Result<Response>>;

enum Command {
    Send {
        request: Request,
        reply: Reply,
        subscribe: bool,
        token: u64,
        /// Dropped when the waiter gives up, which is how a queued request
        /// nobody is waiting for any more is recognised before it is written.
        alive: Weak<()>,
    },
    Unroute {
        id: String,
        token: u64,
    },
    Disconnect,
}

struct Route {
    token: u64,
    subscribe: bool,
    ver: i64,
    /// Whether dispatch was attempted. True means the request *may* have been
    /// sent: a route is registered before the write so a reply racing it is
    /// never lost, and the flag is set before the write because bytes can
    /// leave before the call that wrote them returns. False only if a write
    /// was never attempted.
    sent: bool,
    reply: Reply,
}

/// What [`Client::connect`] sends as the login frame.
///
/// Debug output names the variant only. The token and the auth code are never
/// written into a log line from this type.
#[derive(Clone)]
pub enum Credentials {
    /// `login` with an access token.
    AccessToken(String),
    /// `login/schwab` with a one-time `getAuthCode` code.
    AuthCode(String),
}

impl From<String> for Credentials {
    fn from(access_token: String) -> Self {
        Credentials::AccessToken(access_token)
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credentials::AccessToken(_) => f.write_str("AccessToken(<redacted>)"),
            Credentials::AuthCode(_) => f.write_str("AuthCode(<redacted>)"),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    cmd: Sender<Command>,
    config: Arc<ClientConfig>,
    next_token: Arc<AtomicU64>,
}

impl Client {
    /// Connects, performs the handshake and logs in with `credentials`.
    /// Returns once the gateway has accepted them; the actor carries on in
    /// the background on a thread of its own until the socket goes.
    ///
    /// A `String` is an access token (`login`). [`Credentials::AuthCode`] sends
    /// `login/schwab` instead. The live gate is applied to the trading system
    /// the gateway URL implies, not to the `trading_system` label: a label
    /// that disagrees with the URL is an [`Error::Config`], and any host that
    /// is not a papermoney host needs `allow_live_trading`. The gate runs
    /// before the socket opens.
    pub fn connect(
        config: ClientConfig,
        credentials: impl Into<Credentials>,
    ) -> Result<(Client, LoginBody)> {
        // rustls 0.23 does not pick a process default, and the one it gets
        // has to be the provider the rest of the process already built with:
        // ureq, on the engine's Yahoo calls, is compiled against ring.
        let _ = rustls::crypto::ring::default_provider().install_default();
        assert_trading_system_allowed(config.trading_system, config.allow_live_trading)?;
        let url = config.gateway_url.clone();
        assert_gateway_matches(config.trading_system, &url, config.allow_live_trading)?;

        // Opened on this thread rather than the actor's, because what went
        // wrong only survives if it is classified where it happened.
        let mut socket = open(&url)?;
        socket.send_text(CONNECTION_REQUEST_MESSAGE.to_string())?;
        if redact::frames_on_stderr() {
            eprintln!("➡️ {CONNECTION_REQUEST_MESSAGE}");
        }

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (login_tx, login_rx) = mpsc::channel();
        let actor = Actor {
            socket,
            credentials: credentials.into(),
            heartbeat_timeout: config.heartbeat_timeout,
            cmd_rx,
            routes: HashMap::new(),
            subscriptions: HashMap::new(),
            store: DocumentStore::default(),
            pending_login: Some(login_tx),
        };
        std::thread::Builder::new()
            .name("tos-gateway".into())
            .spawn(move || actor.run())
            .map_err(|e| Error::Other(format!("gateway thread: {e}")))?;
        // On timeout `cmd_tx` is dropped, which the actor sees as a stop.
        let body = login_rx
            .recv_timeout(LOGIN_TIMEOUT)
            .map_err(|_| Error::Timeout("login".into()))??;
        Ok((
            Client {
                cmd: cmd_tx,
                config: Arc::new(config),
                next_token: Arc::new(AtomicU64::new(1)),
            },
            body,
        ))
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Sends a request and returns the first frame carrying its id. Error
    /// frames become `Err(Error::Gateway)`.
    pub fn request(&self, request: Request) -> Result<Response> {
        self.request_timeout(request, REQUEST_TIMEOUT)
    }

    /// Like [`Client::request`] with an explicit timeout.
    pub fn request_timeout(&self, request: Request, timeout: Duration) -> Result<Response> {
        self.route(request, false)?
            .next_timeout(timeout)?
            .into_result()
    }

    /// Sends a request and streams every frame carrying its id.
    pub fn subscribe(&self, request: Request) -> Result<Subscription> {
        self.route(request, true)
    }

    /// Price history for one symbol. The first frame is the candle snapshot.
    pub fn chart(&self, params: &ChartParams) -> Result<Subscription> {
        self.subscribe(chart_request(params))
    }

    fn route(&self, request: Request, subscribe: bool) -> Result<Subscription> {
        let (reply, rx) = mpsc::channel();
        let id = request.id().to_string();
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let waiting = Arc::new(());
        self.cmd
            .send(Command::Send {
                request,
                reply,
                subscribe,
                token,
                alive: Arc::downgrade(&waiting),
            })
            .map_err(|_| Error::Closed)?;
        Ok(Subscription {
            id,
            rx,
            cmd: self.cmd.clone(),
            token,
            waiting,
        })
    }

    /// Closes the socket and ends every subscription stream.
    pub fn disconnect(&self) {
        let _ = self.cmd.send(Command::Disconnect);
    }
}

/// A stream of frames for one request id. Dropping it stops routing.
pub struct Subscription {
    id: String,
    rx: Receiver<Result<Response>>,
    cmd: Sender<Command>,
    token: u64,
    /// What the actor watches to know this waiter is still here.
    waiting: Arc<()>,
}

impl Subscription {
    /// The next frame, or the reason there will not be one.
    ///
    /// A socket that dies while this is waiting fails it at once rather than
    /// making it wait out the timeout, and a gateway that refused the session
    /// says so here as [`Error::Login`] — the one failure somebody can act on.
    pub fn next_timeout(&mut self, timeout: Duration) -> Result<Response> {
        match self.rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                Err(Error::Timeout(format!("no response for {}", self.id)))
            }
            // The actor went without classifying the loss: not proof of no write.
            Err(RecvTimeoutError::Disconnected) => Err(Error::ConnectionLost { sent: true }),
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.cmd.send(Command::Unroute {
            id: self.id.clone(),
            token: self.token,
        });
    }
}

/// What the actor writes requests to.
///
/// A trait rather than the socket itself, because how a failed write is
/// classified for the caller is the subtlest rule in this file and it is
/// worth testing without a gateway on the other end.
trait Wire {
    fn send_text(&mut self, text: String) -> Result<()>;
}

impl Wire for Socket {
    fn send_text(&mut self, text: String) -> Result<()> {
        self.send(Message::text(text))?;
        Ok(())
    }
}

enum Phase {
    AwaitingConnection,
    AwaitingLogin,
    Ready,
}

struct Actor {
    socket: Socket,
    credentials: Credentials,
    heartbeat_timeout: Duration,
    cmd_rx: Receiver<Command>,
    routes: HashMap<String, Vec<Route>>,
    /// Streaming requests to re-send when a patch arrives without a usable
    /// base document, keyed by header id.
    subscriptions: HashMap<String, Request>,
    store: DocumentStore,
    pending_login: Option<Sender<Result<LoginBody>>>,
}

/// A sleep counts as a gap once the wall clock has outrun the monotonic one
/// by this much between two checks.
const SLEEP_GAP: Duration = Duration::from_secs(5);

/// Notices a system sleep (or a locked laptop, a suspended VM…): the wall
/// clock keeps counting while the monotonic clock, which `Instant` reads and
/// which stops during suspend on macOS and Linux, does not. A socket that was
/// open across a sleep is stale — the gateway has long since closed its end —
/// but nothing tells us so until the heartbeat watchdog expires, so this
/// tears it down immediately instead.
struct SleepCheck {
    wall: SystemTime,
    mono: Instant,
}

impl SleepCheck {
    fn new() -> Self {
        SleepCheck {
            wall: SystemTime::now(),
            mono: Instant::now(),
        }
    }

    /// How long the system was asleep since the previous call, if it was.
    fn slept(&mut self) -> Option<Duration> {
        let (wall, mono) = (SystemTime::now(), Instant::now());
        let wall_elapsed = wall.duration_since(self.wall).unwrap_or_default();
        let mono_elapsed = mono.duration_since(self.mono);
        self.wall = wall;
        self.mono = mono;
        let gap = wall_elapsed.saturating_sub(mono_elapsed);
        (gap >= SLEEP_GAP).then_some(gap)
    }
}

#[derive(Debug)]
enum Outcome {
    /// Socket died or went silent.
    Lost(String),
    /// The gateway refused this session, in its own words. Everything waiting
    /// has to hear that rather than a transport failure: signing in again is
    /// something a person can do, and "the provider is not answering" makes
    /// them wait instead.
    Refused(String),
    /// Deliberate stop: all handles dropped or `disconnect()` called.
    Stop,
}

impl Actor {
    fn run(mut self) {
        let outcome = self.serve();
        if let Some(tx) = self.pending_login.take() {
            // Lost before the login completed.
            let _ = tx.send(Err(Error::Closed));
        }
        match outcome {
            Outcome::Refused(message) => self.fail_routes(Some(message)),
            Outcome::Lost(_) | Outcome::Stop => self.fail_routes(None),
        }
    }

    /// Ends every route the dead socket leaves behind: one-shots and streams
    /// both fail now instead of waiting out their timeout. `refused` is the
    /// gateway's own words when it rejected the session, which reaches the
    /// caller as a login failure rather than as a lost connection.
    fn fail_routes(&mut self, refused: Option<String>) {
        for route in self.routes.drain().flat_map(|(_, routes)| routes) {
            let error = match &refused {
                Some(message) => Error::Login(message.clone()),
                None => Error::ConnectionLost { sent: route.sent },
            };
            let _ = route.reply.send(Err(error));
        }
    }

    /// Runs the socket until it dies.
    fn serve(&mut self) -> Outcome {
        let mut phase = Phase::AwaitingConnection;
        let mut last_message_at = Instant::now();
        let mut sleep = SleepCheck::new();

        loop {
            match self.socket.read() {
                Ok(Message::Text(text)) => {
                    last_message_at = Instant::now();
                    if let Some(outcome) = self.frame(&text, &mut phase) {
                        return outcome;
                    }
                }
                Ok(Message::Close(frame)) => {
                    return Outcome::Lost(
                        frame
                            .map(|f| f.reason.to_string())
                            .unwrap_or_else(|| "closed by gateway".into()),
                    );
                }
                Ok(_) => last_message_at = Instant::now(),
                Err(tungstenite::Error::Io(e)) if would_wait(&e) => {}
                Err(e) => return Outcome::Lost(e.to_string()),
            }

            let ready = matches!(phase, Phase::Ready);
            loop {
                match self.cmd_rx.try_recv() {
                    Ok(Command::Disconnect) => {
                        let _ = self.socket.close(None);
                        return Outcome::Stop;
                    }
                    Ok(cmd) => {
                        let Actor {
                            socket,
                            routes,
                            subscriptions,
                            ..
                        } = self;
                        if let Err(e) =
                            dispatch_command(routes, subscriptions, cmd, ready.then_some(socket))
                        {
                            return Outcome::Lost(e.to_string());
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Outcome::Stop,
                }
            }

            if let Some(gap) = sleep.slept() {
                eprintln!("system slept for {gap:?}; the socket is stale, closing it");
                let _ = self.socket.close(None);
                return Outcome::Lost(format!("resumed after {gap:?} asleep"));
            }
            let silent = last_message_at.elapsed();
            if silent > self.heartbeat_timeout {
                eprintln!("no frames for {silent:?}; closing the socket");
                let _ = self.socket.close(None);
                return Outcome::Lost(format!("no frames for {silent:?}"));
            }
        }
    }

    /// One text frame. A `Some` answer ends the connection.
    fn frame(&mut self, text: &str, phase: &mut Phase) -> Option<Outcome> {
        if redact::frames_on_stderr() {
            eprintln!("⬅️ {}", redact::inbound(text));
        }
        let inbound = match decode_inbound(text) {
            Ok(inbound) => inbound,
            Err(e) => {
                // The frame is counted, never quoted: it failed to parse, so
                // nothing here can say which of its fields are secrets.
                eprintln!("undecodable frame of {} bytes: {e}", text.len());
                return None;
            }
        };
        match inbound {
            Inbound::Heartbeat(_) | Inbound::Unknown(_) => {}
            Inbound::Connection { .. } => {
                let request = match &self.credentials {
                    Credentials::AccessToken(token) => login_request(token),
                    Credentials::AuthCode(code) => schwab_login_request(code),
                };
                if let Err(e) = write_request(&mut self.socket, &request) {
                    return Some(Outcome::Lost(e.to_string()));
                }
                *phase = Phase::AwaitingLogin;
            }
            Inbound::Payload(items) => {
                for item in items {
                    if item.is_terminal_session_error() {
                        return Some(self.fail_login(Error::Login(message_of(&item.body))));
                    }
                    if redact::is_login_service(&item.header.service)
                        && matches!(phase, Phase::AwaitingLogin)
                    {
                        if let Err(e) = self.handle_login(item.body) {
                            return Some(self.fail_login(e));
                        }
                        *phase = Phase::Ready;
                        continue;
                    }
                    if let Some(response) = self.store.apply(item) {
                        self.dispatch(response);
                    }
                }
                for id in self.store.take_needs_resync() {
                    let Some(request) = self.subscriptions.get(&id).cloned() else {
                        continue;
                    };
                    eprintln!("re-requesting {id}: patch arrived without a usable base document");
                    if let Err(e) = write_request(&mut self.socket, &request) {
                        return Some(Outcome::Lost(e.to_string()));
                    }
                }
            }
        }
        None
    }

    fn fail_login(&mut self, e: Error) -> Outcome {
        eprintln!("login failed: {e}");
        let message = e.to_string();
        if let Some(tx) = self.pending_login.take() {
            let _ = tx.send(Err(e));
            // Login rejected: stop, there is nothing to retry with.
            return Outcome::Stop;
        }
        // The session died under a connection that was working, which is what
        // a token expiring mid-use looks like from here.
        Outcome::Refused(message)
    }

    fn handle_login(&mut self, body: Value) -> Result<()> {
        let parsed: LoginBody = serde_json::from_value(body)?;
        if !parsed.successful() {
            return Err(Error::Login(
                parsed.message.unwrap_or(parsed.authentication_status),
            ));
        }
        if let Some(tx) = self.pending_login.take() {
            let _ = tx.send(Ok(parsed));
        }
        Ok(())
    }

    fn dispatch(&mut self, response: Response) {
        if let Some(routes) = self.routes.get_mut(&response.id) {
            routes.retain(|r| {
                if !r.subscribe && r.ver != response.ver {
                    return true;
                }
                r.reply.send(Ok(response.clone())).is_ok()
            });
            if routes.is_empty() {
                self.routes.remove(&response.id);
                self.subscriptions.remove(&response.id);
            }
        }
    }
}

fn write_request(wire: &mut impl Wire, request: &Request) -> Result<()> {
    let text = serde_json::to_string(request)?;
    if redact::frames_on_stderr() {
        eprintln!("➡️ {}", redact::outbound(request, &text));
    }
    wire.send_text(text)
}

/// Registers a route and, when the gateway is ready for it, writes the
/// request. `wire` absent means the login has not completed yet.
///
/// A free function rather than a method so that the actor can hand it the
/// socket and the routing tables at once, and so that a test can hand it
/// something that is not a socket.
fn dispatch_command(
    routes: &mut HashMap<String, Vec<Route>>,
    subscriptions: &mut HashMap<String, Request>,
    cmd: Command,
    wire: Option<&mut impl Wire>,
) -> Result<()> {
    match cmd {
        Command::Send {
            request,
            reply,
            subscribe,
            token,
            alive,
        } => {
            // The waiter may have given up (deadline, drop) before this
            // queued Send is processed. A one-shot nobody awaits must not
            // reach the socket.
            if !subscribe && alive.strong_count() == 0 {
                return Ok(());
            }
            if wire.is_none() && !subscribe {
                let _ = reply.send(Err(Error::ConnectionLost { sent: false }));
                return Ok(());
            }
            let id = request.id().to_string();
            routes.entry(id.clone()).or_default().push(Route {
                token,
                subscribe,
                ver: request.header().ver,
                // Set before the write: bytes can leave before it returns.
                sent: wire.is_some(),
                reply,
            });
            if subscribe && is_replayable(request.service()) {
                subscriptions.insert(id, request.clone());
            }
            if let Some(wire) = wire {
                write_request(wire, &request)?;
            }
        }
        Command::Unroute { id, token } => {
            if let Some(routes_for_id) = routes.get_mut(&id) {
                routes_for_id.retain(|r| r.token != token);
                if routes_for_id.is_empty() {
                    routes.remove(&id);
                    subscriptions.remove(&id);
                }
            }
        }
        Command::Disconnect => {}
    }
    Ok(())
}

/// A read that found nothing yet rather than a socket that failed.
fn would_wait(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

/// Opens the gateway socket, saying which kind of failure it was when it
/// fails.
///
/// The difference is a different sentence on a chart, and nothing downstream
/// can recover it afterwards: a name that does not resolve or a route that
/// does not exist mean this machine is off the network, while a refusal, a
/// timeout or a TLS failure mean the network is fine and the gateway is not
/// answering. Only one of those is worth waiting out.
fn open(url: &str) -> Result<Socket> {
    let mut request = url.into_client_request()?;
    let headers = request.headers_mut();
    headers.insert("Origin", TOS_WEB_ORIGIN.parse().unwrap());
    headers.insert("Pragma", "no-cache".parse().unwrap());
    headers.insert("Cache-Control", "no-cache".parse().unwrap());

    let uri = request.uri().clone();
    let host = uri
        .host()
        .ok_or_else(|| Error::Config(format!("gateway {url} names no host")))?
        .to_string();
    let port = uri.port_u16().unwrap_or(match uri.scheme_str() {
        Some("wss") | Some("https") => 443,
        _ => 80,
    });
    let addresses: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| Error::Offline(format!("cannot resolve {host}: {e}")))?
        .collect();
    let mut refusal = Error::Offline(format!("{host} resolves to no address"));
    let mut connected = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, LOGIN_TIMEOUT) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(e) => refusal = reach_failure(&host, e),
        }
    }
    let Some(stream) = connected else {
        return Err(refusal);
    };
    let _ = stream.set_nodelay(true);
    // Generous while the handshake runs, because a read that gives up
    // part-way through one leaves nothing to carry on with. Tightened below,
    // so that the serve loop gets its turn on a quiet socket.
    stream
        .set_read_timeout(Some(LOGIN_TIMEOUT))
        .map_err(|e| Error::Unreachable(format!("{host}: {e}")))?;
    let (socket, _) = tungstenite::client_tls(request, stream)
        .map_err(|e| Error::Unreachable(format!("{host}: {e}")))?;
    read_timeout(&socket, READ_TIMEOUT)?;
    Ok(socket)
}

/// Whether a connection that failed says this machine is off the network or
/// that the gateway did not answer.
fn reach_failure(host: &str, e: std::io::Error) -> Error {
    match e.kind() {
        ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable | ErrorKind::NetworkDown => {
            Error::Offline(format!("cannot reach {host}: {e}"))
        }
        _ => Error::Unreachable(format!("{host}: {e}")),
    }
}

fn read_timeout(socket: &Socket, timeout: Duration) -> Result<()> {
    let stream = match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => &stream.sock,
        _ => return Ok(()),
    };
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| Error::Unreachable(format!("socket timeout: {e}")))
}

/// A gateway that is only pretending, on loopback.
///
/// Enough of one to put the actor through the whole of its life — the
/// handshake, the login, a snapshot, a patch applied to it, a session going
/// stale — without an account and without leaving this machine.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::net::{TcpListener, TcpStream};

    /// What one connection to the fake gateway is answered with.
    pub(crate) type Session = Box<dyn FnOnce(&mut WebSocket<TcpStream>) + Send>;

    pub(crate) const LOGIN_OK: &str = r#"{"payload":[{"header":{"service":"login","id":"login","ver":0,"type":"snapshot"},"body":{"authenticationStatus":"OK","token":"rotated","userCode":"u1"}}]}"#;

    pub(crate) const ONE_BAR: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":1,"type":"snapshot"},"body":{"symbol":"/ES","candles":{"opens":[1.0],"highs":[1.0],"lows":[1.0],"closes":[1.0],"volumes":[7.0],"timestamps":[1000.0]}}}]}"#;

    pub(crate) const EXPIRED: &str = r#"{"payload":[{"header":{"id":"session_expired","ver":0,"type":"error"},"body":{"message":"Session has expired. Please log in again.","isTerminal":true}}]}"#;

    /// Serves one connection per entry in `sessions`, in order, and returns
    /// the URL to point a client at.
    pub(crate) fn gateway(sessions: Vec<Session>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
        let url = format!(
            "ws://127.0.0.1:{}/Services/WsJson",
            listener.local_addr().unwrap().port()
        );
        let handle = std::thread::spawn(move || {
            for serve in sessions {
                let (stream, _) = listener.accept().expect("accept");
                let mut ws = tungstenite::accept(stream).expect("handshake");
                // The client opens by stating the protocol it speaks.
                let opening = ws.read().expect("connection request");
                assert!(
                    opening.to_text().unwrap().contains("json-patches-structured"),
                    "{opening}"
                );
                ws.send(Message::text(
                    r#"{"session":"s","build":"b","ver":"27.0.0"}"#,
                ))
                .unwrap();
                serve(&mut ws);
                // Stay until the client lets go, so the socket never dies
                // under it in a way the test did not ask for.
                while ws.read().is_ok() {}
            }
        });
        (url, handle)
    }

    /// Takes the login frame and accepts it.
    pub(crate) fn log_in(ws: &mut WebSocket<TcpStream>) {
        let login = ws.read().unwrap();
        assert!(login.to_text().unwrap().contains(r#""service":"login""#));
        ws.send(Message::text(LOGIN_OK)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{gateway, log_in, Session, EXPIRED, ONE_BAR};
    use super::*;
    use serde_json::json;

    /// Controllable wire: counts writes, and fails them on demand.
    #[derive(Default)]
    struct TestWire {
        writes: usize,
        fail: bool,
    }

    impl Wire for TestWire {
        fn send_text(&mut self, _: String) -> Result<()> {
            self.writes += 1;
            if self.fail {
                return Err(Error::WebSocket(tungstenite::Error::Io(
                    std::io::Error::from(std::io::ErrorKind::BrokenPipe),
                )));
            }
            Ok(())
        }
    }

    /// The routing tables on their own, which is all `dispatch_command` and
    /// `fail_routes` touch.
    #[derive(Default)]
    struct Tables {
        routes: HashMap<String, Vec<Route>>,
        subscriptions: HashMap<String, Request>,
    }

    impl Tables {
        fn send(&mut self, cmd: Command, wire: Option<&mut TestWire>) -> Result<()> {
            dispatch_command(&mut self.routes, &mut self.subscriptions, cmd, wire)
        }

        /// What the actor does when the socket is gone.
        fn fail(&mut self, refused: Option<String>) {
            for route in self.routes.drain().flat_map(|(_, routes)| routes) {
                let error = match &refused {
                    Some(message) => Error::Login(message.clone()),
                    None => Error::ConnectionLost { sent: route.sent },
                };
                let _ = route.reply.send(Err(error));
            }
        }
    }

    fn send_cmd(reply: Reply, alive: Weak<()>) -> Command {
        Command::Send {
            request: Request::new("chart", "chart-1", 0, json!({"symbol": "/ES"})),
            reply,
            subscribe: false,
            token: 1,
            alive,
        }
    }

    #[test]
    fn a_failed_write_is_connection_lost_sent() {
        let mut tables = Tables::default();
        let (reply, rx) = mpsc::channel();
        let waiting = Arc::new(());
        let mut wire = TestWire {
            fail: true,
            ..TestWire::default()
        };
        let err = tables
            .send(send_cmd(reply, Arc::downgrade(&waiting)), Some(&mut wire))
            .unwrap_err();
        assert!(matches!(err, Error::WebSocket(_)), "{err}");
        assert_eq!(wire.writes, 1);
        assert!(
            tables.routes["chart-1"][0].sent,
            "dispatch was attempted before the failed write"
        );
        tables.fail(None);
        let lost = rx.recv().expect("waiter").unwrap_err();
        assert!(
            matches!(lost, Error::ConnectionLost { sent: true }),
            "{lost}"
        );
    }

    #[test]
    fn a_one_shot_nobody_is_waiting_for_any_more_never_writes() {
        let mut tables = Tables::default();
        let (reply, _rx) = mpsc::channel();
        let waiting = Arc::new(());
        let alive = Arc::downgrade(&waiting);
        drop(waiting);
        let mut wire = TestWire::default();
        tables.send(send_cmd(reply, alive), Some(&mut wire)).unwrap();
        assert_eq!(wire.writes, 0);
        assert!(tables.routes.is_empty());
    }

    /// No write was attempted, so the request is definitively not in flight.
    #[test]
    fn a_request_the_socket_never_took_is_reported_as_unsent() {
        let mut tables = Tables::default();
        let (reply, rx) = mpsc::channel();
        let waiting = Arc::new(());
        tables
            .send(send_cmd(reply, Arc::downgrade(&waiting)), None)
            .unwrap();
        assert!(tables.routes.is_empty());
        let lost = rx.recv().expect("waiter").unwrap_err();
        assert!(
            matches!(lost, Error::ConnectionLost { sent: false }),
            "{lost}"
        );
    }

    /// An actor that vanishes without saying why is not proof that nothing
    /// was sent, so the waiter is told the conservative thing.
    #[test]
    fn an_unexplained_loss_is_conservative() {
        let (reply, rx) = mpsc::channel::<Result<Response>>();
        let (cmd, _cmd_rx) = mpsc::channel();
        let mut sub = Subscription {
            id: "chart-1".into(),
            rx,
            cmd,
            token: 1,
            waiting: Arc::new(()),
        };
        drop(reply);
        let err = sub.next_timeout(Duration::from_secs(1)).unwrap_err();
        assert!(matches!(err, Error::ConnectionLost { sent: true }), "{err}");
    }

    /// A gateway that refuses the session has to reach whoever is waiting as
    /// a login failure: that is the one failure somebody can do something
    /// about, and a transport error makes them wait instead.
    #[test]
    fn an_expired_session_reaches_a_waiting_chart_as_a_login_failure() {
        let mut tables = Tables::default();
        let (reply, rx) = mpsc::channel();
        let waiting = Arc::new(());
        let mut wire = TestWire::default();
        tables
            .send(
                Command::Send {
                    request: chart_request(&ChartParams::new("/ES", "MIN5", "DAY1")),
                    reply,
                    subscribe: true,
                    token: 1,
                    alive: Arc::downgrade(&waiting),
                },
                Some(&mut wire),
            )
            .unwrap();
        // What the gateway sends when the token dies under a live connection.
        let message = Error::Login("Session has expired. Please log in again.".into()).to_string();
        tables.fail(Some(message));
        let error = rx.recv().expect("waiter").unwrap_err();
        assert!(matches!(error, Error::Login(_)), "{error}");
        assert!(error.to_string().contains("log in again"), "{error}");
    }

    fn one(serve: impl FnOnce(&mut WebSocket<std::net::TcpStream>) + Send + 'static) -> Vec<Session> {
        vec![Box::new(serve)]
    }

    fn paper(url: String) -> ClientConfig {
        ClientConfig {
            trading_system: TradingSystem::PaperMoney,
            gateway_url: url,
            allow_live_trading: false,
            ..ClientConfig::default()
        }
    }

    /// The whole life of a connection, against something that answers.
    #[test]
    fn a_chart_arrives_as_a_snapshot_and_then_as_patches_applied_to_it() {
        let (url, server) = gateway(one(|ws| {
            log_in(ws);
            let chart = ws.read().unwrap();
            let chart = chart.to_text().unwrap();
            assert!(chart.contains(r#""id":"chart-/ES-MIN5""#), "{chart}");
            assert!(chart.contains(r#""extendedHours":true"#), "{chart}");
            ws.send(Message::text(ONE_BAR)).unwrap();
            ws.send(Message::text(
                r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":1,"type":"patch"},"body":{"patches":[{"op":"replace","path":"/candles/closes/0","value":2.5}]}}]}"#,
            ))
            .unwrap();
        }));

        let (client, login) = Client::connect(paper(url), "tok".to_string()).expect("connect");
        assert_eq!(login.token, "rotated");
        assert!(login.successful());

        let mut sub = client
            .chart(&ChartParams::new("/ES", "MIN5", "DAY1"))
            .expect("chart");
        let snapshot = sub.next_timeout(Duration::from_secs(10)).expect("snapshot");
        assert_eq!(snapshot.body["candles"]["closes"][0], 1.0);
        // The patch arrives as the whole document, not as the patch list.
        let patched = sub.next_timeout(Duration::from_secs(10)).expect("patch");
        assert_eq!(patched.body["candles"]["closes"][0], 2.5);
        assert_eq!(patched.body["candles"]["timestamps"][0], 1000.0);

        client.disconnect();
        drop(sub);
        server.join().unwrap();
    }

    /// Credentials the gateway will not take are a login failure, named as
    /// one, rather than a connection that merely did not work.
    #[test]
    fn a_gateway_that_refuses_the_token_fails_the_connection_as_a_login() {
        let (url, server) = gateway(one(|ws| {
            ws.read().unwrap();
            ws.send(Message::text(
                r#"{"payload":[{"header":{"service":"login","id":"login","ver":0,"type":"snapshot"},"body":{"authenticationStatus":"FAILED","message":"Invalid token"}}]}"#,
            ))
            .unwrap();
        }));
        let Err(error) = Client::connect(paper(url), "stale".to_string()) else {
            panic!("a refused token must not produce a client");
        };
        assert!(matches!(error, Error::Login(ref m) if m == "Invalid token"), "{error}");
        server.join().unwrap();
    }

    /// The case that used to wedge the feed: the token dies after the login
    /// worked, while a chart is waiting. It has to come back as "sign in
    /// again", not as a socket that went away.
    #[test]
    fn a_session_that_expires_mid_stream_reaches_the_chart_as_a_login_failure() {
        let (url, server) = gateway(one(|ws| {
            log_in(ws);
            ws.read().unwrap();
            ws.send(Message::text(EXPIRED)).unwrap();
        }));
        let (client, _) = Client::connect(paper(url), "tok".to_string()).expect("connect");
        let mut sub = client
            .chart(&ChartParams::new("/ES", "MIN5", "DAY1"))
            .expect("chart");
        let error = sub.next_timeout(Duration::from_secs(10)).unwrap_err();
        assert!(matches!(error, Error::Login(_)), "{error}");
        assert!(error.to_string().contains("log in again"), "{error}");
        drop(sub);
        drop(client);
        server.join().unwrap();
    }

    /// Two kinds of failure to open a socket, told apart where they happen.
    #[test]
    fn a_connection_that_cannot_be_opened_says_which_kind_of_failure_it_was() {
        // Nothing listening on loopback: the network is fine, nobody answered.
        let refused = open("ws://127.0.0.1:1/Services/WsJson").unwrap_err();
        assert!(matches!(refused, Error::Unreachable(_)), "{refused}");
        // A name that does not resolve reads as being off the network.
        let unresolvable = open("wss://nothing.omacharts-test.invalid/Services/WsJson").unwrap_err();
        assert!(matches!(unresolvable, Error::Offline(_)), "{unresolvable}");
    }
}
