//! The connection actor and its handle.
//!
//! One tokio task owns the socket. Every request goes through it, and every
//! response is routed back by `header.id` — never by service alone, which was
//! the single hardest lesson from the TypeScript client. One-shot calls get
//! the first frame for their id; subscriptions get a stream. The actor lives
//! as long as its socket: when that dies, every one-shot fails, every stream
//! ends, and the caller builds a new client.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{Sink, SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

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

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = SplitSink<WsStream, Message>;
type WsSource = SplitStream<WsStream>;

/// Default time a one-shot [`Client::request`] waits for its first frame.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Bound on the initial handshake + login in [`Client::connect`].
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// The socket closed, died, went silent or slept; this client is done.
    Disconnected { reason: String },
}

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

type Reply = mpsc::UnboundedSender<Result<Response>>;

enum Command {
    Send {
        request: Request,
        reply: Reply,
        subscribe: bool,
        token: u64,
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
    /// never lost, and the flag is set before awaiting write+flush because
    /// bytes can leave before that future succeeds. False only if a write
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
    cmd: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<ConnectionEvent>,
    config: Arc<ClientConfig>,
    next_token: Arc<AtomicU64>,
}

impl Client {
    /// Connects, performs the handshake and logs in with `credentials`.
    /// Resolves once the gateway has accepted them; the actor keeps running in
    /// the background on the current tokio runtime until the socket goes.
    ///
    /// A `String` is an access token (`login`). [`Credentials::AuthCode`] sends
    /// `login/schwab` instead. The live gate is applied to the trading system
    /// the gateway URL implies, not to the `trading_system` label: a label
    /// that disagrees with the URL is an [`Error::Config`], and any host that
    /// is not a papermoney host needs `allow_live_trading`. The gate runs
    /// before the socket opens.
    pub async fn connect(
        config: ClientConfig,
        credentials: impl Into<Credentials>,
    ) -> Result<(Client, LoginBody)> {
        // rustls 0.23 does not pick a process default.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        assert_trading_system_allowed(config.trading_system, config.allow_live_trading)?;
        let url = config.gateway_url.clone();
        assert_gateway_matches(config.trading_system, &url, config.allow_live_trading)?;

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(64);
        let (login_tx, login_rx) = oneshot::channel();
        let actor = Actor {
            url,
            credentials: credentials.into(),
            heartbeat_timeout: config.heartbeat_timeout,
            events: events.clone(),
            cmd_rx,
            routes: HashMap::new(),
            subscriptions: HashMap::new(),
            store: DocumentStore::default(),
            pending_login: Some(login_tx),
        };
        tokio::spawn(actor.run());
        // On timeout `cmd_tx` is dropped, which the actor sees as a Stop.
        let body = tokio::time::timeout(LOGIN_TIMEOUT, login_rx)
            .await
            .map_err(|_| Error::Timeout("login".into()))?
            .map_err(|_| Error::Closed)??;
        Ok((
            Client {
                cmd: cmd_tx,
                events,
                config: Arc::new(config),
                next_token: Arc::new(AtomicU64::new(1)),
            },
            body,
        ))
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// The one `Disconnected` this client emits when its socket goes.
    pub fn events(&self) -> broadcast::Receiver<ConnectionEvent> {
        self.events.subscribe()
    }

    /// Sends a request and returns the first frame carrying its id. Error
    /// frames become `Err(Error::Gateway)`.
    pub async fn request(&self, request: Request) -> Result<Response> {
        self.request_timeout(request, REQUEST_TIMEOUT).await
    }

    /// Like [`Client::request`] with an explicit timeout. Fails with
    /// [`Error::ConnectionLost`] as soon as the socket dies while the request
    /// is in flight rather than waiting out the timeout.
    pub async fn request_timeout(&self, request: Request, timeout: Duration) -> Result<Response> {
        let mut sub = self.route(request, false)?;
        match tokio::time::timeout(timeout, sub.rx.recv()).await {
            Ok(Some(Ok(response))) => response.into_result(),
            Ok(Some(Err(e))) => Err(e),
            // Waiter vanished without a classified loss — not proof of no write.
            Ok(None) => Err(Error::ConnectionLost { sent: true }),
            Err(_) => Err(Error::Timeout(format!("no response for {}", sub.id))),
        }
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
        let (reply, rx) = mpsc::unbounded_channel();
        let id = request.id().to_string();
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        self.cmd
            .send(Command::Send {
                request,
                reply,
                subscribe,
                token,
            })
            .map_err(|_| Error::Closed)?;
        Ok(Subscription {
            id,
            rx,
            cmd: self.cmd.clone(),
            token,
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
    rx: mpsc::UnboundedReceiver<Result<Response>>,
    cmd: mpsc::UnboundedSender<Command>,
    token: u64,
}

impl Subscription {
    /// Next frame, or `None` once the socket that carried it is gone.
    pub async fn next(&mut self) -> Option<Response> {
        self.rx.recv().await?.ok()
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

enum Phase {
    AwaitingConnection,
    AwaitingLogin,
    Ready,
}

struct Actor {
    url: String,
    credentials: Credentials,
    heartbeat_timeout: Duration,
    events: broadcast::Sender<ConnectionEvent>,
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    routes: HashMap<String, Vec<Route>>,
    /// Streaming requests to re-send when a patch arrives without a usable
    /// base document, keyed by header id.
    subscriptions: HashMap<String, Request>,
    store: DocumentStore,
    pending_login: Option<oneshot::Sender<Result<LoginBody>>>,
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

enum Outcome {
    /// Socket died, went silent, or its session ended.
    Lost(String),
    /// Deliberate stop: all handles dropped or `disconnect()` called.
    Stop,
}

impl Actor {
    fn emit(&self, event: ConnectionEvent) {
        let _ = self.events.send(event);
    }

    async fn run(mut self) {
        let outcome = match self.open().await {
            Ok((sink, source)) => self.serve(sink, source).await,
            Err(e) => {
                if let Some(tx) = self.pending_login.take() {
                    let _ = tx.send(Err(e));
                }
                return;
            }
        };
        let reason = match outcome {
            Outcome::Stop => "closed".into(),
            Outcome::Lost(reason) => reason,
        };
        self.emit(ConnectionEvent::Disconnected { reason });
        if let Some(tx) = self.pending_login.take() {
            // Lost before the login completed.
            let _ = tx.send(Err(Error::Closed));
        }
        self.fail_routes();
    }

    async fn open(&mut self) -> Result<(WsSink, WsSource)> {
        let mut req = self.url.as_str().into_client_request()?;
        let h = req.headers_mut();
        h.insert("Origin", TOS_WEB_ORIGIN.parse().unwrap());
        h.insert("Pragma", "no-cache".parse().unwrap());
        h.insert("Cache-Control", "no-cache".parse().unwrap());
        let (ws, _) = tokio::time::timeout(LOGIN_TIMEOUT, tokio_tungstenite::connect_async(req))
            .await
            .map_err(|_| Error::Timeout("websocket connect".into()))??;
        let (mut sink, source) = ws.split();
        sink.send(Message::text(CONNECTION_REQUEST_MESSAGE)).await?;
        if redact::frames_on_stderr() {
            eprintln!("➡️ {CONNECTION_REQUEST_MESSAGE}");
        }
        Ok((sink, source))
    }

    async fn send<S>(sink: &mut S, request: &Request) -> Result<()>
    where
        S: Sink<Message> + Unpin,
        Error: From<S::Error>,
    {
        let text = serde_json::to_string(request)?;
        if redact::frames_on_stderr() {
            eprintln!("➡️ {}", redact::outbound(request, &text));
        }
        sink.send(Message::text(text)).await?;
        Ok(())
    }

    /// Ends every route the dead socket leaves behind: one-shots fail now
    /// instead of waiting out their timeout, and streams close so
    /// `Subscription::next` returns `None`.
    fn fail_routes(&mut self) {
        for route in self.routes.drain().flat_map(|(_, routes)| routes) {
            if !route.subscribe {
                let _ = route
                    .reply
                    .send(Err(Error::ConnectionLost { sent: route.sent }));
            }
        }
    }

    /// Runs the socket until it dies.
    async fn serve(&mut self, mut sink: WsSink, mut source: WsSource) -> Outcome {
        let mut phase = Phase::AwaitingConnection;
        let mut last_message_at = Instant::now();
        let tick = self
            .heartbeat_timeout
            .div_f64(2.0)
            .clamp(Duration::from_millis(100), Duration::from_secs(5));
        let mut watchdog = tokio::time::interval(tick);
        watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut sleep = SleepCheck::new();

        loop {
            tokio::select! {
                frame = source.next() => {
                    let text = match frame {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Close(c))) => {
                            return Outcome::Lost(c.map(|c| c.reason.to_string()).unwrap_or_else(|| "closed by gateway".into()));
                        }
                        Some(Ok(_)) => continue,
                        Some(Err(e)) => return Outcome::Lost(e.to_string()),
                        None => return Outcome::Lost("socket ended".into()),
                    };
                    last_message_at = Instant::now();
                    if redact::frames_on_stderr() {
                        eprintln!("⬅️ {}", redact::inbound(&text));
                    }
                    let inbound = match decode_inbound(&text) {
                        Ok(i) => i,
                        Err(e) => {
                            eprintln!("undecodable frame: {e} :: {}", text.chars().take(200).collect::<String>());
                            continue;
                        }
                    };
                    match inbound {
                        Inbound::Heartbeat(_) => {}
                        Inbound::Connection { .. } => {
                            let request = match &self.credentials {
                                Credentials::AccessToken(token) => login_request(token),
                                Credentials::AuthCode(code) => schwab_login_request(code),
                            };
                            if let Err(e) = Self::send(&mut sink, &request).await {
                                return Outcome::Lost(e.to_string());
                            }
                            phase = Phase::AwaitingLogin;
                        }
                        Inbound::Payload(items) => {
                            for item in items {
                                if item.is_terminal_session_error() {
                                    return self.fail_login(Error::Login(message_of(&item.body)));
                                }
                                if redact::is_login_service(&item.header.service)
                                    && matches!(phase, Phase::AwaitingLogin)
                                {
                                    if let Err(e) = self.handle_login(item.body) {
                                        return self.fail_login(e);
                                    }
                                    phase = Phase::Ready;
                                    continue;
                                }
                                if let Some(response) = self.store.apply(item) {
                                    self.dispatch(response);
                                }
                            }
                            for id in self.store.take_needs_resync() {
                                let Some(req) = self.subscriptions.get(&id) else { continue };
                                eprintln!("re-requesting {id}: patch arrived without a usable base document");
                                if let Err(e) = Self::send(&mut sink, req).await {
                                    return Outcome::Lost(e.to_string());
                                }
                            }
                        }
                        Inbound::Unknown(_) => {}
                    }
                }
                cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = cmd else { return Outcome::Stop };
                    let ready = matches!(phase, Phase::Ready);
                    match cmd {
                        Command::Disconnect => {
                            let _ = sink.close().await;
                            return Outcome::Stop;
                        }
                        cmd => {
                            if let Err(e) = self.handle_command(cmd, ready.then_some(&mut sink)).await {
                                return Outcome::Lost(e.to_string());
                            }
                        }
                    }
                }
                _ = watchdog.tick() => {
                    if let Some(gap) = sleep.slept() {
                        eprintln!("system slept for {gap:?}; the socket is stale, closing it");
                        let _ = sink.close().await;
                        return Outcome::Lost(format!("resumed after {gap:?} asleep"));
                    }
                    let silent = last_message_at.elapsed();
                    if silent > self.heartbeat_timeout {
                        eprintln!("no frames for {silent:?}; closing the socket");
                        let _ = sink.close().await;
                        return Outcome::Lost(format!("no frames for {silent:?}"));
                    }
                }
            }
        }
    }

    fn fail_login(&mut self, e: Error) -> Outcome {
        eprintln!("login failed: {e}");
        if let Some(tx) = self.pending_login.take() {
            let _ = tx.send(Err(e));
            // Login rejected: stop, there is nothing to retry with.
            return Outcome::Stop;
        }
        Outcome::Lost(e.to_string())
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

    async fn handle_command<S>(&mut self, cmd: Command, sink: Option<&mut S>) -> Result<()>
    where
        S: Sink<Message> + Unpin,
        Error: From<S::Error>,
    {
        match cmd {
            Command::Send {
                request,
                reply,
                subscribe,
                token,
            } => {
                // The waiter may have given up (deadline, drop) before this
                // queued Send is processed. A one-shot with a closed reply
                // must not hit the socket.
                if !subscribe && reply.is_closed() {
                    return Ok(());
                }
                if sink.is_none() && !subscribe {
                    let _ = reply.send(Err(Error::ConnectionLost { sent: false }));
                    return Ok(());
                }
                let id = request.id().to_string();
                self.routes.entry(id.clone()).or_default().push(Route {
                    token,
                    subscribe,
                    ver: request.header().ver,
                    // Set before write+flush: bytes can leave before send() returns.
                    sent: sink.is_some(),
                    reply,
                });
                if subscribe && is_replayable(request.service()) {
                    self.subscriptions.insert(id, request.clone());
                }
                if let Some(sink) = sink {
                    Self::send(sink, &request).await?;
                }
            }
            Command::Unroute { id, token } => {
                if let Some(routes) = self.routes.get_mut(&id) {
                    routes.retain(|r| r.token != token && !r.reply.is_closed());
                    if routes.is_empty() {
                        self.routes.remove(&id);
                        self.subscriptions.remove(&id);
                    }
                }
            }
            Command::Disconnect => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use serde_json::json;
    use tokio_tungstenite::tungstenite::Error as WsError;

    /// Controllable sink: records `start_send`, then fails on flush (the
    /// tungstenite write-then-BrokenPipe-on-flush shape) or succeeds.
    #[derive(Default)]
    struct TestSink {
        writes: usize,
        fail_flush: bool,
    }

    impl Sink<Message> for TestSink {
        type Error = WsError;

        fn poll_ready(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, _: Message) -> std::result::Result<(), Self::Error> {
            self.get_mut().writes += 1;
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            if self.fail_flush {
                Poll::Ready(Err(WsError::Io(std::io::Error::from(
                    std::io::ErrorKind::BrokenPipe,
                ))))
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<std::result::Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    fn test_actor() -> Actor {
        let (_tx, cmd_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(8);
        Actor {
            url: "ws://test".into(),
            credentials: Credentials::AccessToken("t".into()),
            heartbeat_timeout: Duration::from_secs(30),
            events,
            cmd_rx,
            routes: HashMap::new(),
            subscriptions: HashMap::new(),
            store: DocumentStore::default(),
            pending_login: None,
        }
    }

    fn send_cmd(reply: Reply, token: u64) -> Command {
        Command::Send {
            request: Request::new("chart", "chart-1", 0, json!({"symbol": "/ES"})),
            reply,
            subscribe: false,
            token,
        }
    }

    #[tokio::test]
    async fn write_then_flush_failure_is_connection_lost_sent() {
        let mut actor = test_actor();
        let (reply, mut rx) = mpsc::unbounded_channel();
        let mut sink = TestSink {
            fail_flush: true,
            ..TestSink::default()
        };
        let err = actor
            .handle_command(send_cmd(reply, 1), Some(&mut sink))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::WebSocket(_)), "{err}");
        assert_eq!(sink.writes, 1);
        let route = &actor.routes["chart-1"][0];
        assert!(route.sent, "dispatch was attempted before the failed flush");
        actor.fail_routes();
        let lost = rx.recv().await.expect("waiter").unwrap_err();
        assert!(
            matches!(lost, Error::ConnectionLost { sent: true }),
            "{lost}"
        );
    }

    #[tokio::test]
    async fn a_closed_queued_one_shot_never_writes() {
        let mut actor = test_actor();
        let (reply, rx) = mpsc::unbounded_channel();
        drop(rx);
        assert!(reply.is_closed());
        let mut sink = TestSink::default();
        actor
            .handle_command(send_cmd(reply, 1), Some(&mut sink))
            .await
            .unwrap();
        assert_eq!(sink.writes, 0);
        assert!(actor.routes.is_empty());
    }

    #[tokio::test]
    async fn unsent_and_unexplained_losses_are_classified() {
        // No sink: write never attempted → sent: false (Definitive downstream).
        let mut actor = test_actor();
        let (reply, mut rx) = mpsc::unbounded_channel();
        actor
            .handle_command(send_cmd(reply, 1), None::<&mut TestSink>)
            .await
            .unwrap();
        assert!(actor.routes.is_empty());
        let lost = rx.recv().await.expect("waiter").unwrap_err();
        assert!(
            matches!(lost, Error::ConnectionLost { sent: false }),
            "{lost}"
        );

        // Unexplained waiter loss (reply dropped, no classified error) → sent: true.
        let (cmd, mut cmd_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(8);
        let client = Client {
            cmd,
            events,
            config: Arc::new(ClientConfig::default()),
            next_token: Arc::new(AtomicU64::new(1)),
        };
        let dropper = tokio::spawn(async move {
            let Some(Command::Send { reply, .. }) = cmd_rx.recv().await else {
                panic!("expected Send");
            };
            drop(reply);
        });
        let err = client
            .request_timeout(
                Request::new("chart", "chart-1", 0, json!({"symbol": "/ES"})),
                Duration::from_secs(1),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, Error::ConnectionLost { sent: true }),
            "unexplained waiter loss must be conservative, got {err}"
        );
        dropper.await.unwrap();
    }
}
