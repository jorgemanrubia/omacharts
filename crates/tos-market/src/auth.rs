//! Interactive browser login for thinkorswim Web.
//!
//! Opens a real (headful) Chrome with a persistent profile, lets you log in
//! normally (password, MFA, device trust), and passively captures what the
//! client needs from the SPA's own WebSocket traffic via the DevTools protocol:
//!
//!   - the service-gateway URL the SPA connected to (live A/B or papermoney)
//!   - the `login/schwab` / `login` response: access token + refresh token
//!   - `user_properties`: default account code
//!   - the cookies `Network.getCookies` would send to the TSM host (HttpOnly
//!     included), as one `Cookie` header for a later paper/live switch
//!
//! Nothing is typed into the page and no request is intercepted or aborted, so
//! the flow is exactly what Schwab sees from a normal user. As a fallback the
//! SPA's `sessionStorage` (`token`, `tradingSystem`) is read.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::{
    EventWebSocketCreated, EventWebSocketFrameReceived, GetCookiesParams,
};
use chromiumoxide::Page;
use futures_util::StreamExt;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::config::{fallback_gateway_urls, TradingSystem, TOS_WEB_ORIGIN};
use crate::error::{Error, Result};
use crate::session::BrowserSession;

pub struct CaptureOptions {
    /// Which trading system you want a session for.
    pub trading_system: TradingSystem,
    /// Give up after this long (default 10 minutes).
    pub timeout: Duration,
    /// Persistent profile so device trust survives between logins.
    pub user_data_dir: PathBuf,
    /// Progress messages (one line each).
    pub log: Box<dyn Fn(&str) + Send + Sync>,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        CaptureOptions {
            trading_system: TradingSystem::PaperMoney,
            timeout: Duration::from_secs(10 * 60),
            user_data_dir: default_profile_dir(),
            log: Box::new(|m| eprintln!("{m}")),
        }
    }
}

/// Where the persistent browser profile lives: `$XDG_STATE_HOME/omacharts/
/// tos-browser`, or `~/.local/state/omacharts/tos-browser`.
///
/// State, not configuration, and emphatically not a relative path. It holds a
/// logged-in brokerage profile, Chrome rewrites it constantly and it grows to
/// tens of megabytes, so the XDG state directory is where it belongs —
/// whereas a relative `./browser-profile` lands wherever the app happened to
/// be launched from, which means a different login each time somebody starts
/// omacharts from a different directory, and a signed-in session dropped into
/// whatever repository they were standing in. Created 0700 by
/// [`capture_browser_session`], because a trusted-device profile is as good
/// as the password that made it.
pub fn default_profile_dir() -> PathBuf {
    profile_dir_in(
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

/// [`default_profile_dir`] with the environment handed in, so the rule can be
/// tested without a test rewriting the variables every other test reads.
fn profile_dir_in(state_home: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    state_home
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| {
            home.filter(|h| !h.as_os_str().is_empty())
                .map(|home| home.join(".local/state"))
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("omacharts/tos-browser")
}

fn fallback_gateway(trading_system: TradingSystem) -> String {
    let urls = fallback_gateway_urls();
    match trading_system {
        TradingSystem::PaperMoney => urls.papermoney,
        TradingSystem::LiveTrading => urls.livetrading_a,
    }
}

const CANDIDATES: &[&str] = &[
    // macOS
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    // Linux
    "/usr/bin/google-chrome-stable",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/usr/bin/brave",
];

/// thinkorswim ships a Chromium.app under `~/thinkorswim/jxbrowser/…`.
fn thinkorswim_chromium() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("HOME")?).join("thinkorswim");
    fn walk(dir: &Path, depth: u32) -> Option<PathBuf> {
        if depth > 6 {
            return None;
        }
        let mac = dir.join("Chromium.app/Contents/MacOS/Chromium");
        if mac.is_file() {
            return Some(mac);
        }
        let rd = std::fs::read_dir(dir).ok()?;
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(found) = walk(&p, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(&root, 0)
}

/// `TOS_BROWSER`, `PUPPETEER_EXECUTABLE_PATH`, then well-known install paths.
///
/// Public because a sign-in that cannot work is worth saying before somebody
/// presses the button: the settings panel asks this to decide between an
/// enabled button and a sentence naming what has to be installed.
pub fn find_browser() -> Option<PathBuf> {
    std::env::var_os("TOS_BROWSER")
        .or_else(|| std::env::var_os("PUPPETEER_EXECUTABLE_PATH"))
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| CANDIDATES.iter().map(PathBuf::from).find(|p| p.is_file()))
        .or_else(thinkorswim_chromium)
}

#[derive(Debug, Clone, Default)]
struct SocketSession {
    gateway_url: String,
    trading_system: Option<TradingSystem>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    user_code: Option<String>,
    account_code: Option<String>,
}

enum Sniffed {
    Created { request_id: String, url: String },
    Frame { request_id: String, payload: String },
}

fn short_url(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(u) => format!("{}{}", u.origin().ascii_serialization(), u.path()),
        Err(_) if url.is_empty() => "(empty)".into(),
        Err(_) => url.to_string(),
    }
}

/// Attaches sniffers to every page not yet seen; returns all current pages.
async fn attach_all(
    browser: &Browser,
    attached: &mut HashSet<String>,
    tx: &mpsc::UnboundedSender<Sniffed>,
) -> Vec<Page> {
    let pages = browser.pages().await.unwrap_or_default();
    for p in &pages {
        let id = p.target_id().inner().to_string();
        if attached.insert(id) {
            let _ = attach(p, tx.clone()).await;
        }
    }
    pages
}

/// Attaches WebSocket sniffers to a page; events flow into `tx`.
async fn attach(page: &Page, tx: mpsc::UnboundedSender<Sniffed>) -> Result<()> {
    let cdp = |e: chromiumoxide::error::CdpError| Error::Other(format!("cdp: {e}"));
    let mut created = page
        .event_listener::<EventWebSocketCreated>()
        .await
        .map_err(cdp)?;
    let mut frames = page
        .event_listener::<EventWebSocketFrameReceived>()
        .await
        .map_err(cdp)?;
    let tx2 = tx.clone();
    tokio::spawn(async move {
        while let Some(ev) = created.next().await {
            if tx2
                .send(Sniffed::Created {
                    request_id: ev.request_id.inner().to_string(),
                    url: ev.url.clone(),
                })
                .is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(async move {
        while let Some(ev) = frames.next().await {
            if tx
                .send(Sniffed::Frame {
                    request_id: ev.request_id.inner().to_string(),
                    payload: ev.response.payload_data.clone(),
                })
                .is_err()
            {
                break;
            }
        }
    });
    Ok(())
}

fn apply_frame(session: &mut SocketSession, payload: &str, log: &dyn Fn(&str)) {
    let Ok(msg) = serde_json::from_str::<Value>(payload) else {
        return;
    };
    let Some(items) = msg.get("payload").and_then(Value::as_array) else {
        return;
    };
    for f in items {
        let svc = f["header"]["service"].as_str().unwrap_or("");
        let body = &f["body"];
        if (svc == "login/schwab" || svc == "login")
            && body["token"].as_str().is_some_and(|t| !t.is_empty())
        {
            session.access_token = body["token"].as_str().map(str::to_string);
            session.refresh_token = body["accessTokenInfo"]["refreshToken"]
                .as_str()
                .map(str::to_string);
            session.user_code = body["userCode"].as_str().map(str::to_string);
            log(&format!(
                "✓ captured {svc} token on {} socket",
                session
                    .trading_system
                    .map(|t| t.to_string())
                    .unwrap_or_default()
            ));
        } else if svc == "user_properties" {
            let code = match body.get("defaultAccountCode") {
                Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
                Some(Value::Number(n)) => Some(n.to_string()),
                _ => None,
            };
            if let Some(code) = code {
                log("✓ account captured");
                session.account_code = Some(code);
            }
        }
    }
}

#[derive(Debug, Default)]
struct Stored {
    token: Option<String>,
    trading_system: Option<String>,
}

async fn read_storage(pages: &[Page]) -> Option<Stored> {
    for p in pages {
        let Ok(Some(href)) = p.url().await else {
            continue;
        };
        if !href.contains("thinkorswim.com") && !href.contains("schwab.com") {
            continue;
        }
        let Ok(result) = p
            .evaluate(
                "JSON.stringify({token: sessionStorage.getItem('token'), tradingSystem: sessionStorage.getItem('tradingSystem')})",
            )
            .await
        else {
            continue;
        };
        let Ok(text) = result.into_value::<String>() else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let token = v["token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if token.is_some() {
            return Some(Stored {
                token,
                trading_system: v["tradingSystem"].as_str().map(str::to_string),
            });
        }
    }
    None
}

/// Chrome refuses to start on a profile whose `SingletonLock` (a symlink to
/// `<host>-<pid>`) names a running process. A login browser can outlive the
/// process that opened it, so a second login would fail forever; report that
/// pid — but only if it really is a browser running on *this* profile dir,
/// never the user's own Chrome.
#[cfg(unix)]
fn orphaned_profile_owner(user_data_dir: &Path) -> Option<u32> {
    let target = std::fs::read_link(user_data_dir.join("SingletonLock")).ok()?;
    let target = target.to_path_buf();
    let pid: u32 = target.to_str()?.rsplit('-').next()?.parse().ok()?;
    let out = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-o", "command=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout);
    let (ppid, cmd) = line.trim_start().split_once(' ')?;
    // A child of this very process is a login another part of the program is
    // still running (an engine being replaced): leave it alone.
    if ppid.trim().parse::<u32>().ok()? == std::process::id() {
        return None;
    }
    let dir = user_data_dir
        .canonicalize()
        .unwrap_or_else(|_| user_data_dir.to_path_buf());
    let dir = dir.to_string_lossy();
    let raw = user_data_dir.to_string_lossy();
    (cmd.contains(&format!("--user-data-dir={dir}"))
        || cmd.contains(&format!("--user-data-dir={raw}")))
    .then_some(pid)
}

#[cfg(not(unix))]
fn orphaned_profile_owner(_: &Path) -> Option<u32> {
    None
}

/// Cookies the browser would send to the TSM host, as one `Cookie` header.
/// `Network.getCookies` sees HttpOnly cookies. The value is not logged.
async fn tsm_cookie_from_pages(pages: &[Page]) -> std::result::Result<Option<String>, String> {
    if pages.is_empty() {
        return Ok(None);
    }
    let params = GetCookiesParams::builder().url(crate::tsm::TSM_URL).build();
    let mut last = String::from("no page accepted getCookies");
    for page in pages {
        match page.execute(params.clone()).await {
            Ok(got) => {
                let header = crate::tsm::cookie_header(
                    got.result
                        .cookies
                        .iter()
                        .map(|c| (c.name.as_str(), c.value.as_str())),
                );
                return Ok((!header.is_empty()).then_some(header));
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// Drives a headful login and returns the captured session plus the TSM
/// cookie, when the browser had one.
pub async fn capture_browser_session(
    opts: CaptureOptions,
) -> Result<(BrowserSession, Option<String>)> {
    let log = |m: &str| (opts.log)(m);
    let executable = find_browser().ok_or_else(|| {
        Error::Other("no Chrome/Chromium found; set TOS_BROWSER to a browser binary".into())
    })?;
    std::fs::create_dir_all(&opts.user_data_dir)
        .map_err(|e| Error::Other(format!("profile dir: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&opts.user_data_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| Error::Other(format!("profile dir perms: {e}")))?;
    }
    if let Some(pid) = orphaned_profile_owner(&opts.user_data_dir) {
        log(&format!(
            "closing a leftover login browser (pid {pid}) that still holds the profile"
        ));
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .status();
        for _ in 0..20 {
            if orphaned_profile_owner(&opts.user_data_dir).is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    // chromiumoxide's default flags (puppeteer's list) include
    // `--enable-automation`, a mock keychain and a plain-text password store;
    // Google's sign-in refuses such a browser ("This browser or app may not
    // be secure"). Launch a near-stock Chrome instead — this is a real
    // interactive login, not a headless bot.
    let config = BrowserConfig::builder()
        .with_head()
        .disable_default_args()
        .args([
            "no-first-run",
            "no-default-browser-check",
            "disable-popup-blocking",
            "disable-blink-features=AutomationControlled",
        ])
        .chrome_executable(&executable)
        .user_data_dir(&opts.user_data_dir)
        .window_size(1280, 900)
        .viewport(None)
        .launch_timeout(Duration::from_secs(30))
        .build()
        .map_err(Error::Other)?;
    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| Error::Other(format!("launch browser: {e}")))?;
    let pump = tokio::spawn(async move { while let Some(_ev) = handler.next().await {} });

    let (tx, mut rx) = mpsc::unbounded_channel::<Sniffed>();
    let mut attached: HashSet<String> = HashSet::new();
    let mut sockets: HashMap<String, SocketSession> = HashMap::new();

    let page = match browser
        .pages()
        .await
        .ok()
        .and_then(|p| p.into_iter().next())
    {
        Some(p) => p,
        None => browser
            .new_page("about:blank")
            .await
            .map_err(|e| Error::Other(format!("new page: {e}")))?,
    };
    attach_all(&browser, &mut attached, &tx).await;

    log(&format!("Opening {TOS_WEB_ORIGIN} — log in as usual."));
    match opts.trading_system {
        TradingSystem::PaperMoney => {
            log("Wanted: PaperMoney. If the UI lands in live trading, use the account-menu toggle to switch to paperMoney.");
        }
        TradingSystem::LiveTrading => {
            log("Wanted: Live Trading. If the UI lands in paperMoney, use the account-menu toggle to switch to live.");
        }
    }
    page.goto(format!("{TOS_WEB_ORIGIN}/"))
        .await
        .map_err(|e| Error::Other(format!("navigate: {e}")))?;

    let deadline = Instant::now() + opts.timeout;
    let mut last_status = String::new();
    let result = loop {
        // Drain sniffed traffic.
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Sniffed::Created { request_id, url } => {
                    if url.contains("/Services/WsJson") {
                        let ts = TradingSystem::of_gateway_url(&url);
                        log(&format!("↔ SPA opened gateway socket: {url} ({ts})"));
                        sockets.insert(
                            request_id,
                            SocketSession {
                                gateway_url: url,
                                trading_system: Some(ts),
                                ..Default::default()
                            },
                        );
                    }
                }
                Sniffed::Frame {
                    request_id,
                    payload,
                } => {
                    if let Some(s) = sockets.get_mut(&request_id) {
                        apply_frame(s, &payload, &log);
                    }
                }
            }
        }
        let pages = attach_all(&browser, &mut attached, &tx).await;

        // 1) sniffed socket session (has everything)
        let sniffed = sockets
            .values()
            .find(|s| s.trading_system == Some(opts.trading_system) && s.access_token.is_some())
            .cloned();
        // 2) sessionStorage session (token + trading system)
        let stored = read_storage(&pages).await;
        let stored_system = stored
            .as_ref()
            .and_then(|s| s.trading_system.as_deref())
            .and_then(TradingSystem::parse);

        let found = if let Some(s) = sniffed {
            Some((
                BrowserSession {
                    trading_system: opts.trading_system,
                    gateway_url: s.gateway_url,
                    access_token: s.access_token.unwrap(),
                    refresh_token: s.refresh_token,
                    account_code: s.account_code,
                    user_code: s.user_code,
                },
                "websocket",
            ))
        } else if let Some(st) = stored
            .as_ref()
            .filter(|_| stored_system == Some(opts.trading_system))
        {
            Some((
                BrowserSession {
                    trading_system: opts.trading_system,
                    gateway_url: fallback_gateway(opts.trading_system),
                    access_token: st.token.clone().unwrap(),
                    refresh_token: None,
                    account_code: None,
                    user_code: None,
                },
                "sessionStorage",
            ))
        } else {
            None
        };
        if let Some((session, via)) = found {
            log(&format!(
                "✓ {} session captured via {via}{}",
                opts.trading_system,
                session
                    .account_code
                    .as_ref()
                    .map(|a| format!(" (account {a})"))
                    .unwrap_or_default()
            ));
            let tsm_cookie = match tsm_cookie_from_pages(&pages).await {
                Ok(cookie) => {
                    if cookie.is_some() {
                        log("✓ TSM session cookie captured");
                    } else {
                        log("no TSM session cookie; switching trading systems will open a browser");
                    }
                    cookie
                }
                Err(e) => {
                    log(&format!("could not read TSM cookies ({e})"));
                    None
                }
            };
            break Ok((session, tsm_cookie));
        }

        let status = if let Some(st) = &stored {
            format!(
                "Logged in ({}); waiting for the UI to be in {}… (switch via the account menu in the Chrome window)",
                st.trading_system.clone().unwrap_or_else(|| "unknown system".into()),
                opts.trading_system
            )
        } else {
            let mut urls = Vec::new();
            for p in &pages {
                urls.push(short_url(&p.url().await.ok().flatten().unwrap_or_default()));
            }
            let socks: Vec<String> = sockets
                .values()
                .filter_map(|s| s.trading_system.map(|t| t.to_string()))
                .collect();
            format!(
                "Waiting for login… pages={} [{}] sockets=[{}]",
                pages.len(),
                urls.join(" | "),
                socks.join(",")
            )
        };
        if status != last_status {
            last_status = status.clone();
            log(&status);
        }
        if Instant::now() > deadline {
            break Err(Error::Timeout(format!(
                "waiting for a {} session",
                opts.trading_system
            )));
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
    };

    if result.is_ok() {
        log("Closing the browser so this token has a single consumer.");
    }
    let _ = browser.close().await;
    let _ = browser.wait().await;
    pump.abort();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The profile holds a logged-in brokerage session. It used to be
    /// `./browser-profile`, which put one in whatever directory the app was
    /// started from — a different login per directory, and somebody's Schwab
    /// session dropped into a source tree.
    #[test]
    fn the_browser_profile_lives_under_the_state_directory() {
        assert_eq!(
            profile_dir_in(Some(PathBuf::from("/x/state")), Some(PathBuf::from("/home/p"))),
            PathBuf::from("/x/state/omacharts/tos-browser")
        );
        assert_eq!(
            profile_dir_in(None, Some(PathBuf::from("/home/p"))),
            PathBuf::from("/home/p/.local/state/omacharts/tos-browser")
        );
        // An empty variable is not a directory called "".
        assert_eq!(
            profile_dir_in(Some(PathBuf::new()), Some(PathBuf::from("/home/p"))),
            PathBuf::from("/home/p/.local/state/omacharts/tos-browser")
        );
    }

    #[test]
    fn the_profile_is_never_a_relative_path() {
        for dir in [
            profile_dir_in(None, None),
            profile_dir_in(Some(PathBuf::new()), Some(PathBuf::new())),
        ] {
            assert!(dir.is_absolute(), "{}", dir.display());
        }
    }
}
