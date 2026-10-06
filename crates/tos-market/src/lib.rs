// The candle functions are the only production callers. Tests cover the rest
// of the session client, which the lib build otherwise reports as dead.
#![allow(dead_code)]

//! Chart candles from a thinkorswim session — any thinkorswim session.
//!
//! One connection for the process, built on the first chart request and not
//! before. A chart request returns its first snapshot. Paper or live, it is
//! the account's own charts either way, and which one it is is this crate's
//! business rather than its caller's.
//!
//! Read-only by construction, and checkably so. The gateway behind a
//! thinkorswim session is the one the platform's own order entry talks to, so
//! two properties are written down rather than left to the absence of a
//! caller: `services::ALLOWED_SERVICES` is the whole of what this crate
//! will ask for — `chart`, `login`, `login/schwab` — and the single function
//! that writes a frame refuses anything else; and the gateway's host has to
//! be one of thinkorswim's, read out of the URL rather than taken from a
//! label the session file supplies.
//!
//! Signing in is a separate, explicit act ([`sign_in`]), never something a
//! chart fetch does on somebody's behalf. A fetch that opened a browser would
//! park the one thread that fetches bars for as long as it takes a person to
//! find their phone and read a code off it, with nothing on screen to say
//! why; [`candles`] with no session fails at once and says what to do instead.

mod auth;
mod cdp;
mod client;
mod config;
mod error;
mod patch;
mod protocol;
mod redact;
mod services;
mod session;
mod stream;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use auth::{capture_browser_session, CaptureOptions};
use client::Client;
use services::chart::ChartBody;
use session::BrowserSession;

pub use error::{Error, Result};
pub use services::chart::ChartParams;
pub use stream::{fold, stream, Event, Stream};

/// One candle. `ts` is the unix second the bar opens at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// What a machine with nowhere to keep a session is told.
const NO_HOME: &str = "no home directory to keep the thinkorswim session in";

/// Where the session is kept: `TOS_ENV_FILE` when set, otherwise
/// `~/.config/omacharts/tos.env`.
///
/// Where [`sign_in`] writes what it captured, and the only place a chart
/// fetch looks. Configuration, unlike the browser profile beside it: small,
/// hand-editable, and worth carrying between machines.
///
/// Fallible, because there is one state of a machine with no right answer —
/// no `TOS_ENV_FILE`, no `XDG_CONFIG_HOME`, no home directory at all — and
/// refusing is the only honest thing to do there. This used to fall back to
/// `/tmp`, which put a brokerage session where every account on the machine
/// could watch for it; nothing in this crate reads or writes a session
/// anywhere but the path this returns.
pub fn session_path() -> Result<PathBuf> {
    session_path_from(|key| std::env::var_os(key), std::env::home_dir())
}

/// [`session_path`] with the environment handed in, so the rule can be
/// tested without a test rewriting variables every other test reads.
fn session_path_from(
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> Result<PathBuf> {
    if let Some(path) = var("TOS_ENV_FILE").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let config = var("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".config")))
        .ok_or_else(|| Error::Config(NO_HOME.into()))?;
    Ok(config.join("omacharts/tos.env"))
}

/// [`session_path`] for a line on screen.
///
/// The settings panel tells people where their session is kept, and it is
/// entitled to an answer even on a machine with nowhere to keep one — where
/// it shows the path unexpanded, because that is all there is to say. Never
/// opened: every read and every write goes through [`session_path`].
pub fn session_file() -> PathBuf {
    session_path().unwrap_or_else(|_| PathBuf::from("~/.config/omacharts/tos.env"))
}

/// Where the browser profile the sign-in uses lives.
///
/// Shown in the settings panel, because a persistent brokerage profile is
/// something a person is entitled to know the location of — and to delete.
pub fn profile_file() -> PathBuf {
    auth::default_profile_dir()
}

/// The Chromium-family browser a sign-in would use, if this machine has one.
///
/// `None` is the honest answer to give before the button is pressed rather
/// than after: the login is a real browser window somebody signs into, so
/// with nothing to open there is nothing to try.
pub fn browser() -> Option<PathBuf> {
    auth::find_browser()
}

/// What a sign-in would be starting from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// No session file, or nothing in it.
    Missing,
    /// A session is saved. `account` is the account code the gateway last
    /// reported, and `saved` the file's mtime in unix seconds — which is
    /// "signed in since", near enough to say out loud.
    Saved {
        account: Option<String>,
        saved: Option<i64>,
    },
    /// Saved, and the gateway has since refused it. Signing in again is the
    /// only fix, and it is the one state worth interrupting somebody over.
    Expired { at: i64 },
}

impl SessionState {
    /// Can a chart be fetched with what is saved?
    pub fn usable(&self) -> bool {
        matches!(self, SessionState::Saved { .. })
    }
}

/// What the saved session file says, without connecting to anything.
///
/// Expiry is the one thing a file cannot be read for — only the gateway knows
/// — so it is reported from the mark a refused connection leaves behind. A
/// machine with nowhere to keep a session has none of one.
pub fn session_state() -> SessionState {
    match session_path() {
        Ok(env) => state_of(&env),
        Err(_) => SessionState::Missing,
    }
}

pub(crate) fn state_of(env: &Path) -> SessionState {
    if let Some(at) = BrowserSession::expired_at(env) {
        return SessionState::Expired { at };
    }
    match stored_session(env) {
        Some(session) => SessionState::Saved {
            account: session.account_code,
            saved: std::fs::metadata(env)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64),
        },
        None => SessionState::Missing,
    }
}

/// Opens a browser at thinkorswim, waits for the person to sign in, and saves
/// the captured session.
///
/// Blocking, and slow by nature: it returns when a human has finished typing
/// a password and a one-time code, or after ten minutes. Call it on a thread
/// of its own. `log` is handed one progress line at a time.
pub fn sign_in(log: impl Fn(&str) + Send + Sync + 'static) -> Result<()> {
    browser_sign_in_with(&session_path()?, Box::new(log))?;
    Ok(())
}

/// Forgets the saved session, whichever account it was, and both of the
/// file's slots with it. The browser profile stays, so the next sign-in is
/// still a trusted device and usually just a click.
pub fn sign_out() -> Result<()> {
    let env = session_path()?;
    BrowserSession::clear_dotenv(&env).map_err(|e| Error::Other(format!("sign out: {e}")))
}

/// Has this process connected to the gateway?
///
/// For the one test that matters to everybody who does not use this feed: a
/// provider that was merely constructed must not have built a runtime, opened
/// a socket or launched anything.
pub fn connected() -> bool {
    CELL.get().is_some()
}

/// Close the connection this process holds, if it holds one.
///
/// For a window that has stopped charting from this feed: a socket kept open
/// to a brokerage for nothing is not a cost worth paying, and every stream on
/// it has already been let go of. The next request, if one ever comes, finds
/// the connection dead and replaces it the way a lost one is replaced.
pub fn disconnect() {
    if let Some(market) = CELL.get() {
        market.current().1.disconnect();
    }
}

/// Candles for one symbol, oldest first. `aggregation` and `range` are the
/// platform's own codes (`MIN5`, `DAY`, `DAY1`, `YEAR2`).
pub fn candles(symbol: &str, aggregation: &str, range: &str) -> Result<Vec<Candle>> {
    market()?.snapshot(symbol, aggregation, range)
}

static CELL: OnceLock<Market> = OnceLock::new();

/// Held while the one connection is being built.
///
/// `OnceLock::get_or_init` cannot carry a failure out, which is why the build
/// does not live inside it — and without this, two threads racing the first
/// chart would each open a socket to the gateway and one of them would be
/// thrown away with its session still logged in.
static BUILDING: Mutex<()> = Mutex::new(());

fn market() -> Result<&'static Market> {
    if let Some(ready) = CELL.get() {
        return Ok(ready);
    }
    let _building = BUILDING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ready) = CELL.get() {
        return Ok(ready);
    }
    let built = Market::connect(&session_path()?)?;
    Ok(CELL.get_or_init(|| built))
}

struct Market {
    /// The connection, and which one it is. The number is what tells a thread
    /// whose fetch failed that somebody else has already replaced the client
    /// it was holding, so that one lost socket costs one reconnection rather
    /// than one per thread.
    client: Mutex<(u64, Client)>,
    env: PathBuf,
}

impl Market {
    fn connect(env: &Path) -> Result<Self> {
        // Nothing above this line in the process has opened a socket or
        // started a thread, and nothing does unless somebody has chosen this
        // feed and asked for a chart: the first `candles` call gets here.
        let session = usable_session(env).ok_or_else(|| no_session(env))?;
        let (client, _) = match session.connect(env) {
            // The one answer only the gateway can give. Written down so the
            // settings panel can say "expired" instead of "signed in" next
            // time somebody looks, in this process or another.
            Err(error) if auth_failure(&error) => {
                let _ = BrowserSession::mark_expired(env, now());
                return Err(error);
            }
            other => other?,
        };
        let _ = BrowserSession::clear_expired(env);
        Ok(Market {
            client: Mutex::new((0, client)),
            env: env.to_path_buf(),
        })
    }

    /// The connection as it stands, and which generation it is.
    fn current(&self) -> (u64, Client) {
        self.client.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn snapshot(&self, symbol: &str, aggregation: &str, range: &str) -> Result<Vec<Candle>> {
        let (generation, client) = self.client.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match fetch(&client, symbol, aggregation, range) {
            Err(error) if lost(&error) || auth_failure(&error) => {
                let fresh = self.replace(generation)?;
                fetch(&fresh, symbol, aggregation, range)
            }
            other => other,
        }
    }

    /// The connection to use after `cause` killed generation `generation`.
    ///
    /// Reconnecting is what keeps an expired token from wedging the feed for
    /// the life of the process: the session file is read again, so a person
    /// who has signed in since gets their charts back without a restart —
    /// which is also why the refusal is not written down before trying. A
    /// session that really is dead is marked by [`reconnect`], once, on the
    /// attempt that proves it.
    fn replace(&self, generation: u64) -> Result<Client> {
        let mut held = self.client.lock().unwrap_or_else(|e| e.into_inner());
        if held.0 != generation {
            return Ok(held.1.clone());
        }
        let (fresh, _) = reconnect(&self.env)?;
        let _ = BrowserSession::clear_expired(&self.env);
        *held = (generation + 1, fresh.clone());
        Ok(fresh)
    }
}

impl Drop for Market {
    fn drop(&mut self) {
        if let Ok(client) = self.client.lock() {
            client.1.disconnect();
        }
    }
}

/// The saved session, whichever thinkorswim account it belongs to.
fn stored_session(env: &Path) -> Option<BrowserSession> {
    BrowserSession::any_in(env)
}

/// The saved session, unless the gateway has already refused it.
///
/// A token the gateway has said no to will be said no to again, and a chart
/// on a refresh timer would ask it once a minute for ever — a TLS connection
/// to a brokerage per minute, to be told the same thing. The mark is cleared
/// by a sign-in and by a connection that works, so it being there means
/// nobody has done anything that could change the answer.
fn usable_session(env: &Path) -> Option<BrowserSession> {
    stored_session(env).filter(|_| BrowserSession::expired_at(env).is_none())
}

fn auth_failure(error: &Error) -> bool {
    match error {
        Error::Login(_) => true,
        Error::Gateway { id, message, .. }
            if id == "session_expired"
                || message.to_ascii_lowercase().contains("log in again")
                || message.to_ascii_lowercase().contains("session has expired") =>
        {
            true
        }
        _ => false,
    }
}

/// Why a fetch with nothing saved fails, in words the UI and the CLI both
/// repeat verbatim. Never a browser: see the module docs.
fn no_session(env: &Path) -> Error {
    Error::NoSession(env.display().to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Opens a browser at thinkorswim and writes the captured session to `env`.
fn browser_sign_in_with(
    env: &Path,
    log: Box<dyn Fn(&str) + Send + Sync>,
) -> Result<BrowserSession> {
    session::ensure_private_dir(env).map_err(|e| Error::Other(format!("session dir: {e}")))?;
    let session = capture_browser_session(CaptureOptions {
        // Whichever account the browser lands in: both of them chart.
        trading_system: None,
        timeout: Duration::from_secs(10 * 60),
        user_data_dir: auth::profile_dir_beside(env),
        log,
    })?;
    session
        .save_to_dotenv(env)
        .map_err(|e| Error::Other(format!("save session: {e}")))?;
    // A session that was just captured is not expired, whatever the file
    // remembered about the one it replaces.
    let _ = BrowserSession::clear_expired(env);
    Ok(session)
}

fn reconnect(env: &Path) -> Result<(Client, BrowserSession)> {
    let session = usable_session(env).ok_or_else(|| no_session(env))?;
    match session.connect(env) {
        Err(error) if auth_failure(&error) => {
            let _ = BrowserSession::mark_expired(env, now());
            Err(error)
        }
        other => other,
    }
}

fn fetch(client: &Client, symbol: &str, aggregation: &str, range: &str) -> Result<Vec<Candle>> {
    let mut sub = client.chart(&ChartParams::new(symbol, aggregation, range))?;
    let res = sub.next_timeout(Duration::from_secs(30))?;
    let body: ChartBody = serde_json::from_value((*res.into_result()?.body).clone())?;
    Ok(candles_of(&body))
}

fn lost(error: &Error) -> bool {
    matches!(
        error,
        Error::Timeout(_) | Error::Closed | Error::ConnectionLost { .. }
    )
}

/// Parallel candle arrays from a chart snapshot, oldest first.
pub fn candles_of(body: &ChartBody) -> Vec<Candle> {
    let c = &body.candles;
    let mut out = Vec::with_capacity(c.len());
    for i in 0..c.len() {
        let (open, high, low, close) = (c.opens[i], c.highs[i], c.lows[i], c.closes[i]);
        if ![open, high, low, close].iter().all(|p| p.is_finite()) {
            continue;
        }
        let volume = c.volumes.get(i).copied().unwrap_or(0.0);
        out.push(Candle {
            ts: (c.timestamps[i] / 1000.0) as i64,
            open,
            high,
            low,
            close,
            volume: if volume.is_finite() { volume } else { 0.0 },
        });
    }
    out.sort_by_key(|bar| bar.ts);
    out.dedup_by_key(|bar| bar.ts);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::assert_gateway_allowed;
    use crate::services::chart::ChartBody;
    use serde_json::json;

    #[test]
    fn a_snapshot_becomes_oldest_first_candles() {
        let body: ChartBody = serde_json::from_value(json!({
            "symbol": "/ES",
            "candles": {
                "opens": [2.0, 1.0],
                "highs": [2.5, 1.5],
                "lows": [1.5, 0.5],
                "closes": [2.2, 1.2],
                "volumes": [10.0, null],
                "timestamps": [2_000_000.0, 1_000_000.0]
            }
        }))
        .unwrap();
        let bars = candles_of(&body);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].ts, 1_000);
        assert_eq!(bars[0].volume, 0.0);
        assert_eq!(bars[1].close, 2.2);
    }

    /// `tos.env` in a fresh temp directory of its own.
    fn env_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tos-market-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("tos.env")
    }

    const PAPER: &str = "wss://papermoney-services.schwab.com/Services/WsJson";
    const LIVE: &str = "wss://thinkorswim-services.schwab.com/Services/WsJson";

    #[test]
    fn with_nothing_saved_there_is_no_session_and_no_browser() {
        let path = env_path("missing");
        assert_eq!(state_of(&path), SessionState::Missing);
        // The error a chart fetch gets, instead of a browser window and ten
        // minutes of a fetch thread.
        let error = match Market::connect(&path) {
            Err(error) => error,
            Ok(_) => panic!("connected with no session"),
        };
        assert!(matches!(error, Error::NoSession(_)), "{error}");
        assert!(!connected(), "connecting must not have been attempted");
    }

    /// A file the previous version of this crate wrote: a paperMoney slot,
    /// and nothing else in it.
    #[test]
    fn a_saved_paper_session_reads_as_signed_in() {
        let path = env_path("saved");
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={PAPER}\nTOS_PAPER_ACCOUNT_CODE=D-1\n"),
        )
        .unwrap();
        match state_of(&path) {
            SessionState::Saved { account, saved } => {
                assert_eq!(account.as_deref(), Some("D-1"));
                assert!(saved.is_some_and(|t| t > 0));
            }
            other => panic!("{other:?}"),
        }
        assert!(state_of(&path).usable());
    }

    /// The gateway is the only thing that knows a token has gone stale, and
    /// it says so once. The panel is opened later, often in another process.
    #[test]
    fn a_refused_session_reads_as_expired_until_the_next_one_works() {
        let path = env_path("expired");
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={PAPER}\n"),
        )
        .unwrap();
        BrowserSession::mark_expired(&path, 1_700_000_000).unwrap();
        assert_eq!(state_of(&path), SessionState::Expired { at: 1_700_000_000 });
        assert!(!state_of(&path).usable());
        BrowserSession::clear_expired(&path).unwrap();
        assert!(state_of(&path).usable());
    }

    /// The change the crate exists to make: a live thinkorswim session is a
    /// session. It reads as signed in, it is the one a chart would use, and
    /// nothing between the file and the socket refuses its gateway.
    #[test]
    fn a_saved_live_session_reads_as_signed_in_too() {
        let path = env_path("live");
        std::fs::write(
            &path,
            format!("TOS_LIVE_ACCESS_TOKEN=tok\nTOS_LIVE_GATEWAY_URL={LIVE}\n"),
        )
        .unwrap();
        assert!(state_of(&path).usable(), "{:?}", state_of(&path));
        let session = stored_session(&path).expect("the live session");
        assert_eq!(session.access_token, "tok");
        assert_eq!(session.gateway_url, LIVE);
        assert!(assert_gateway_allowed(session.trading_system, &session.gateway_url).is_ok());
    }

    /// A paper slot pointing at a live gateway. The label still loses to the
    /// URL — that is what keeps the session filed under the right keys — but
    /// losing no longer means being refused.
    #[test]
    fn a_paper_label_on_a_live_gateway_is_the_live_session_it_names() {
        let path = env_path("mislabelled");
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={LIVE}\nTOS_TRADING_SYSTEM=PaperMoney\n"),
        )
        .unwrap();
        let session = stored_session(&path).expect("the session the URL names");
        assert_eq!(session.gateway_url, LIVE);
        assert!(assert_gateway_allowed(session.trading_system, &session.gateway_url).is_ok());
        assert!(state_of(&path).usable());
    }

    /// A gateway that is nobody's thinkorswim is refused, and it is refused
    /// before a socket is opened — a session file is an ordinary text file
    /// another program on the machine can write to.
    #[test]
    fn a_session_pointing_somewhere_that_is_not_thinkorswim_is_refused() {
        let path = env_path("elsewhere");
        std::fs::write(
            &path,
            "TOS_ACCESS_TOKEN=tok\nTOS_GATEWAY_URL=wss://evil.example/Services/WsJson\n",
        )
        .unwrap();
        let Err(error) = Market::connect(&path) else {
            panic!("a gateway that is not thinkorswim's must be refused");
        };
        assert!(
            matches!(error, Error::Config(ref m) if m.contains("not a thinkorswim gateway")),
            "{error}"
        );
        assert!(!error.to_string().contains("tok"), "{error}");
    }

    /// Nowhere to keep a session is a refusal, not a fallback. This used to
    /// land in `/tmp`, where every account on the machine could watch for it.
    #[test]
    fn with_no_home_there_is_nowhere_to_keep_a_session() {
        let nothing = |_: &str| None;
        let error = session_path_from(nothing, None).expect_err("no home");
        assert!(matches!(error, Error::Config(ref m) if m == NO_HOME), "{error}");
        assert!(!error.to_string().contains("tmp"), "{error}");
        // What it does with a home, and with each way of naming one.
        let home = PathBuf::from("/home/p");
        assert_eq!(
            session_path_from(nothing, Some(home.clone())).unwrap(),
            PathBuf::from("/home/p/.config/omacharts/tos.env")
        );
        let xdg = |k: &str| (k == "XDG_CONFIG_HOME").then(|| "/xdg".into());
        assert_eq!(
            session_path_from(xdg, None).unwrap(),
            PathBuf::from("/xdg/omacharts/tos.env")
        );
        let named = |k: &str| (k == "TOS_ENV_FILE").then(|| "/elsewhere/tos.env".into());
        assert_eq!(
            session_path_from(named, Some(home)).unwrap(),
            PathBuf::from("/elsewhere/tos.env")
        );
        // An empty variable names nothing.
        let empty = |_: &str| Some(std::ffi::OsString::new());
        assert!(session_path_from(empty, None).is_err());
        // The path shown on screen is never a shared directory either.
        let shown = session_file();
        assert!(shown.is_absolute() || shown.starts_with("~"), "{}", shown.display());
    }

    /// Signing out forgets both accounts, not just the one that happens to
    /// be active: somebody who presses it has finished with this file.
    #[test]
    fn signing_out_forgets_every_session_and_keeps_unrelated_lines() {
        let path = env_path("signout");
        std::fs::write(
            &path,
            format!(
                "KEEP=1\nTOS_ACCESS_TOKEN=tok\nTOS_GATEWAY_URL={PAPER}\nTOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={PAPER}\nTOS_LIVE_ACCESS_TOKEN=live\nTOS_LIVE_GATEWAY_URL={LIVE}\n"
            ),
        )
        .unwrap();
        BrowserSession::clear_dotenv(&path).unwrap();
        assert_eq!(state_of(&path), SessionState::Missing);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("KEEP=1"), "{text}");
        assert!(!text.contains("tok"), "{text}");
        assert!(!text.contains("live"), "{text}");
    }

    /// A chart on a refresh timer asks once a minute, for ever. Asking a
    /// gateway that has already refused this token costs a TLS connection to
    /// a brokerage to be told the same thing again, so the mark the refusal
    /// left is read before anything is opened.
    #[test]
    fn a_session_the_gateway_has_already_refused_costs_no_socket() {
        let path = env_path("expired-no-socket");
        // Port 1 on loopback: anything that opened a socket would fail with
        // Unreachable rather than NoSession, which is how this test can tell.
        std::fs::write(
            &path,
            "TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL=ws://127.0.0.1:1/Services/WsJson\n",
        )
        .unwrap();
        assert!(matches!(
            Market::connect(&path),
            Err(Error::Unreachable(_))
        ));
        BrowserSession::mark_expired(&path, 1_700_000_000).unwrap();
        assert!(
            matches!(Market::connect(&path), Err(Error::NoSession(_))),
            "a refused session must be refused from the file"
        );
        // Signing in again clears the mark, and the feed tries once more.
        BrowserSession::clear_expired(&path).unwrap();
        assert!(matches!(
            Market::connect(&path),
            Err(Error::Unreachable(_))
        ));
    }

    /// A token that dies mid-session used to wedge the feed for the life of
    /// the process: `Market` lives in a `OnceLock`, so every later fetch
    /// failed instantly and for ever, and restarting was the only cure.
    ///
    /// What it does instead is read the session file again — so somebody who
    /// has signed in since gets their charts back without a restart — and
    /// clear the expired mark the refusal left behind.
    #[test]
    fn an_expired_session_reconnects_instead_of_wedging_the_feed() {
        use client::fake::{gateway, log_in, EXPIRED, ONE_BAR};
        use tungstenite::Message;

        let (url, server) = gateway(vec![
            // The first connection logs in, then loses the session under a
            // chart that is waiting for its snapshot.
            Box::new(|ws| {
                log_in(ws);
                ws.read().unwrap();
                ws.send(Message::text(EXPIRED)).unwrap();
            }),
            // The second is a session that works.
            Box::new(|ws| {
                log_in(ws);
                ws.read().unwrap();
                ws.send(Message::text(ONE_BAR)).unwrap();
            }),
        ]);

        let path = env_path("expired-reconnect");
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={url}\n"),
        )
        .unwrap();

        let market = Market::connect(&path).expect("first connection");
        let bars = market
            .snapshot("/ES", "MIN5", "DAY1")
            .expect("the refused fetch has to come back through a fresh connection");
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].close, 1.0);
        // The refusal was written down on the way past and taken back by the
        // connection that worked, so the settings panel does not keep saying
        // "expired" at somebody who is signed in.
        assert!(state_of(&path).usable(), "{:?}", state_of(&path));

        drop(market);
        server.join().unwrap();
    }

    /// Live chart. Point `TOS_ENV_FILE` at a session file.
    #[test]
    #[ignore]
    fn chart_snapshot() {
        let es = candles("/ES", "MIN5", "DAY1").expect("/ES");
        assert!(es.len() > 10, "/ES bars: {}", es.len());
        let aapl = candles("AAPL", "DAY", "MONTH6").expect("AAPL");
        assert!(aapl.len() > 20, "AAPL bars: {}", aapl.len());
        let hour = candles("/ES", "HOUR1", "DAY1").expect("HOUR1");
        assert!(hour.len() > 5, "HOUR1 bars: {}", hour.len());
        let spx = candles("SPX", "DAY", "MONTH6").expect("SPX");
        assert!(spx.len() > 20, "SPX bars: {}", spx.len());
        eprintln!(
            "/ES MIN5 {} bars, AAPL DAY {} bars, HOUR1 {} bars, SPX {} bars",
            es.len(),
            aapl.len(),
            hour.len(),
            spx.len()
        );
    }

    /// Live stream, written to stderr: what the gateway pushes for a chart
    /// subscription, frame by frame, with the clock beside each one.
    ///
    /// Point `TOS_ENV_FILE` at a session file. `TOS_CAPTURE` is a comma
    /// separated list of `symbol:aggregation` pairs (default `SPY:MIN1`), and
    /// `TOS_CAPTURE_SECONDS` how long to listen (default 120). With
    /// `TOS_TRACE=1` the raw frames print as well, patch operations included,
    /// which is the evidence of what a tick actually looks like on the wire.
    #[test]
    #[ignore]
    fn capture_live_stream() {
        let specs = std::env::var("TOS_CAPTURE").unwrap_or_else(|_| "SPY:MIN1".into());
        let seconds: u64 =
            std::env::var("TOS_CAPTURE_SECONDS").ok().and_then(|s| s.parse().ok()).unwrap_or(120);
        let market = market().expect("a session");
        let started = std::time::Instant::now();
        let mut threads = Vec::new();
        let drop_at: Option<u64> =
            std::env::var("TOS_CAPTURE_DROP_AT").ok().and_then(|s| s.parse().ok());
        for (n, spec) in specs.split(',').enumerate() {
            // `symbol:aggregation[@ver][!range]`, so a request can be sent
            // again with a bumped version or a different range.
            let (spec, range) = spec.split_once('!').unwrap_or((spec, ""));
            let (spec, ver) = spec.split_once('@').unwrap_or((spec, "1"));
            let (symbol, aggregation) = spec.split_once(':').expect("symbol:aggregation");
            let range = if !range.is_empty() {
                range
            } else if aggregation == "DAY" {
                "MONTH6"
            } else {
                "DAY1"
            };
            let (_, client) = market.client.lock().unwrap().clone();
            let mut request = crate::services::chart::chart_request(&ChartParams::new(
                symbol,
                aggregation,
                range,
            ));
            request.payload[0].header.ver = ver.parse().expect("a version");
            let mut sub = client.subscribe(request).expect("a subscription");
            let tag = format!("#{n} {symbol} {aggregation} v{ver} {range}");
            if let Some(at) = drop_at.filter(|_| n == 0) {
                let dropper = client.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(at));
                    eprintln!("[{at:>8.3}s] dropping the connection");
                    dropper.disconnect();
                });
            }
            eprintln!("[{:>8.3}s] {tag}: subscribed", started.elapsed().as_secs_f64());
            threads.push(std::thread::spawn(move || {
                let mut last: Option<Candle> = None;
                let mut frames = 0u32;
                loop {
                    let left = Duration::from_secs(seconds).saturating_sub(started.elapsed());
                    if left.is_zero() {
                        break;
                    }
                    match sub.next_timeout(left) {
                        Ok(res) => {
                            frames += 1;
                            let at = started.elapsed().as_secs_f64();
                            let body: ChartBody = serde_json::from_value((*res.body).clone())
                                .unwrap_or_default();
                            let candles = candles_of(&body);
                            let newest = candles.last().copied();
                            let changed = match (last, newest) {
                                (Some(a), Some(b)) if a.ts == b.ts => format!(
                                    "same bar: close {} -> {} vol {} -> {}",
                                    a.close, b.close, a.volume, b.volume
                                ),
                                (Some(a), Some(b)) => format!("new bar: ts {} -> {}", a.ts, b.ts),
                                _ => "first".into(),
                            };
                            eprintln!(
                                "[{at:>8.3}s] {tag}: {:?} ver {} {} candles, newest {:?}; {changed}",
                                res.kind,
                                res.ver,
                                candles.len(),
                                newest.map(|c| (c.ts, c.open, c.high, c.low, c.close, c.volume)),
                            );
                            last = newest;
                        }
                        Err(e) => {
                            eprintln!(
                                "[{:>8.3}s] {tag}: ended: {e}",
                                started.elapsed().as_secs_f64()
                            );
                            break;
                        }
                    }
                }
                eprintln!("{tag}: {frames} frames in {seconds}s");
            }));
            // Staggered on purpose, so a second request for the same id is
            // seen to land on a stream that is already flowing.
            std::thread::sleep(Duration::from_secs(10));
        }
        for thread in threads {
            let _ = thread.join();
        }
    }
}
