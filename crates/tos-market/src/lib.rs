// The candle functions are the only production callers. Tests cover the rest
// of the session client, which the lib build otherwise reports as dead.
#![allow(dead_code)]

//! Chart candles from a thinkorswim session.
//!
//! One connection for the process, built on the first chart request and not
//! before. A chart request returns its first snapshot. The live gateway is
//! refused: every connection this crate opens is gated on the trading system
//! the gateway URL implies, with `allow_live_trading` hard-coded false, and
//! there is no order-entry code here to route anything with in the first
//! place — the only services are `chart` and `login`.
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

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use auth::{capture_browser_session, CaptureOptions};
use client::Client;
use config::TradingSystem;
use services::chart::{ChartBody, ChartParams};
use session::BrowserSession;

pub use error::{Error, Result};

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

/// `TOS_ENV_FILE` when set, otherwise `~/.config/omacharts/tos.env`.
///
/// Where [`sign_in`] writes what it captured, and the only place a chart
/// fetch looks. Configuration, unlike the browser profile beside it: small,
/// hand-editable, and worth carrying between machines.
pub fn session_file() -> PathBuf {
    if let Some(path) = std::env::var_os("TOS_ENV_FILE").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("omacharts/tos.env")
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
    /// No session file, or nothing in it for paperMoney.
    Missing,
    /// A paperMoney session is saved. `account` is the account code the
    /// gateway last reported, and `saved` the file's mtime in unix seconds —
    /// which is "signed in since", near enough to say out loud.
    Saved {
        account: Option<String>,
        saved: Option<i64>,
    },
    /// Saved, and the gateway has since refused it. Signing in again is the
    /// only fix, and it is the one state worth interrupting somebody over.
    Expired { at: i64 },
    /// The file holds a live-trading session and nothing else. Refused rather
    /// than used: this is a chart feed, and it connects to paperMoney only.
    RefusedLive,
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
/// — so it is reported from the mark a refused connection leaves behind.
pub fn session_state() -> SessionState {
    state_of(&session_file())
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
        // A live session is worth saying out loud, because "no session"
        // would read as a sign-in that failed when in fact it worked and
        // landed somewhere this feed will not follow.
        None if BrowserSession::any_in(env).is_some() => SessionState::RefusedLive,
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
    browser_sign_in_with(&session_file(), Box::new(log))?;
    Ok(())
}

/// Forgets the saved session. The browser profile stays, so the next sign-in
/// is still a trusted device and usually just a click.
pub fn sign_out() -> std::io::Result<()> {
    BrowserSession::clear_dotenv_for(session_file(), TradingSystem::PaperMoney)
}

/// Has this process connected to the gateway?
///
/// For the one test that matters to everybody who does not use this feed: a
/// provider that was merely constructed must not have built a runtime, opened
/// a socket or launched anything.
pub fn connected() -> bool {
    CELL.get().is_some()
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
    let built = Market::connect(&session_file())?;
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
        // `false`: a chart feed does not open the live gateway.
        let (client, _) = match session.connect(env, false) {
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

fn stored_session(env: &Path) -> Option<BrowserSession> {
    BrowserSession::load_for(env, TradingSystem::PaperMoney)
        .filter(|session| session.trading_system == TradingSystem::PaperMoney)
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
        trading_system: TradingSystem::PaperMoney,
        timeout: Duration::from_secs(10 * 60),
        user_data_dir: auth::default_profile_dir(),
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
    match session.connect(env, false) {
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
    let body: ChartBody = serde_json::from_value(res.into_result()?.body)?;
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
    use serde_json::json;
    use crate::services::chart::ChartBody;

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

    /// Charts only, paperMoney only. A live session in the file is not used,
    /// and not reported as nothing either.
    #[test]
    fn a_live_session_is_refused_rather_than_used() {
        let path = env_path("live");
        std::fs::write(
            &path,
            format!("TOS_LIVE_ACCESS_TOKEN=tok\nTOS_LIVE_GATEWAY_URL={LIVE}\n"),
        )
        .unwrap();
        assert_eq!(state_of(&path), SessionState::RefusedLive);
        assert!(stored_session(&path).is_none());
        assert!(matches!(Market::connect(&path), Err(Error::NoSession(_))));
    }

    /// The lie worth testing: a paper slot pointing at a live gateway. The
    /// label loses to the URL, so this is a live session and refused.
    #[test]
    fn a_paper_label_on_a_live_gateway_is_still_live() {
        let path = env_path("mislabelled");
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={LIVE}\nTOS_TRADING_SYSTEM=PaperMoney\n"),
        )
        .unwrap();
        assert!(stored_session(&path).is_none());
        assert_eq!(state_of(&path), SessionState::RefusedLive);
        assert!(matches!(Market::connect(&path), Err(Error::NoSession(_))));
    }

    #[test]
    fn signing_out_forgets_the_session_and_keeps_unrelated_lines() {
        let path = env_path("signout");
        std::fs::write(
            &path,
            format!("KEEP=1\nTOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={PAPER}\n"),
        )
        .unwrap();
        BrowserSession::clear_dotenv_for(&path, TradingSystem::PaperMoney).unwrap();
        assert_eq!(state_of(&path), SessionState::Missing);
        assert!(std::fs::read_to_string(&path).unwrap().contains("KEEP=1"));
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
}
