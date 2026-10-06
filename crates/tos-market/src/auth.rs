//! Interactive browser login for thinkorswim Web.
//!
//! Opens a real (headful) Chrome with a persistent profile, lets you log in
//! normally (password, MFA, device trust), and passively captures what the
//! client needs from the SPA's own WebSocket traffic via the DevTools protocol:
//!
//!   - the service-gateway URL the SPA connected to (live A/B or papermoney)
//!   - the `login/schwab` / `login` response: access token + refresh token
//!   - `user_properties`: default account code
//!
//! Nothing is typed into the page and no request is intercepted or aborted, so
//! the flow is exactly what Schwab sees from a normal user. As a fallback the
//! SPA's `sessionStorage` (`token`, `tradingSystem`) is read.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::cdp::{Browser, Page};
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

/// Where the persistent browser profile lives: `tos-browser`, beside the
/// session file.
///
/// Beside the session because the two are one thing — a login and the
/// trusted device that produced it — so moving, backing up or deleting one
/// should take the other with it. Never a relative path: `./browser-profile`
/// would land wherever the app happened to be launched from, which means a
/// different login per directory and a signed-in brokerage profile dropped
/// into whatever source tree somebody was standing in. Created 0700 by
/// [`capture_browser_session`], because a trusted-device profile is as good
/// as the password that made it.
pub fn default_profile_dir() -> PathBuf {
    profile_dir_beside(&crate::session_file())
}

/// [`default_profile_dir`] with the session file handed in, so the rule can
/// be tested without a test rewriting the variables every other test reads.
fn profile_dir_beside(session: &Path) -> PathBuf {
    session
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join("omacharts"))
        .join("tos-browser")
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

/// The flags the login browser is launched with, and the whole of them.
///
/// A driver crate's defaults are puppeteer's list, which includes
/// `--enable-automation` and a mock keychain; Google's sign-in refuses such a
/// browser ("This browser or app may not be secure"). This is a real
/// interactive login, not a headless bot, so the browser is as near stock as
/// it can be and still be spoken to.
const LAUNCH_FLAGS: &[&str] = &[
    "no-first-run",
    "no-default-browser-check",
    "disable-popup-blocking",
    "disable-blink-features=AutomationControlled",
    "window-size=1280,900",
    // Chrome probes org.freedesktop.secrets on startup and waits out the
    // D-Bus activation timeout when nothing answers — half a minute sat at
    // about:blank — and on a desktop that does answer it can put a keyring
    // prompt in front of the login. This profile is ours alone and never
    // saves a password, so the keyring buys nothing.
    "password-store=basic",
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

/// A page's URL, short enough for a progress line.
///
/// The query and the fragment go: a sign-in flow carries its state in them,
/// and none of it is worth reading off a terminal — or leaving in a log
/// somebody pastes into an issue.
fn short_url(url: &str) -> String {
    if url.is_empty() {
        return "(empty)".into();
    }
    match url.split_once(['?', '#']) {
        Some((head, _)) => format!("{head}…"),
        None => url.to_string(),
    }
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

fn read_storage(browser: &mut Browser, pages: &[Page]) -> Option<Stored> {
    for page in pages {
        if !page.url.contains("thinkorswim.com") && !page.url.contains("schwab.com") {
            continue;
        }
        let Ok(value) = browser.evaluate(
            page,
            "JSON.stringify({token: sessionStorage.getItem('token'), tradingSystem: sessionStorage.getItem('tradingSystem')})",
        ) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(text) else {
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

/// Drives a headful login and returns the captured session.
pub fn capture_browser_session(opts: CaptureOptions) -> Result<BrowserSession> {
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
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    let mut browser = Browser::launch(
        &executable,
        &opts.user_data_dir,
        LAUNCH_FLAGS,
        Duration::from_secs(30),
    )?;

    let mut sockets: HashMap<String, SocketSession> = HashMap::new();

    log(&format!("Opening {TOS_WEB_ORIGIN} — log in as usual."));
    match opts.trading_system {
        TradingSystem::PaperMoney => {
            log("Wanted: PaperMoney. If the UI lands in live trading, use the account-menu toggle to switch to paperMoney.");
        }
        TradingSystem::LiveTrading => {
            log("Wanted: Live Trading. If the UI lands in paperMoney, use the account-menu toggle to switch to live.");
        }
    }
    let result = capture(&mut browser, &opts, &log, &mut sockets);

    if result.is_ok() {
        log("Closing the browser so this token has a single consumer.");
    }
    browser.close();
    result
}

/// The capture loop, with the browser already open. Split out so that every
/// way of leaving it still closes the browser.
fn capture(
    browser: &mut Browser,
    opts: &CaptureOptions,
    log: &dyn Fn(&str),
    sockets: &mut HashMap<String, SocketSession>,
) -> Result<BrowserSession> {
    let mut pages = browser.pages()?;
    if pages.is_empty() {
        browser.call(
            "Target.createTarget",
            serde_json::json!({"url": "about:blank"}),
            None,
        )?;
        browser.pump(Duration::from_millis(500))?;
        pages = browser.pages()?;
    }
    let first = pages
        .first()
        .cloned()
        .ok_or_else(|| Error::Other("the browser opened no page".into()))?;
    browser.navigate(&first, &format!("{TOS_WEB_ORIGIN}/"))?;

    let deadline = Instant::now() + opts.timeout;
    let mut last_status = String::new();
    loop {
        // A page the SPA opens for the sign-in flow has to be attached before
        // its traffic can be seen, so this runs every turn, not just once.
        let pages = browser.pages()?;
        for event in browser.events() {
            match event.method.as_str() {
                "Network.webSocketCreated" => {
                    let url = event.params["url"].as_str().unwrap_or_default().to_string();
                    let Some(request) = event.params["requestId"].as_str() else {
                        continue;
                    };
                    if url.contains("/Services/WsJson") {
                        let ts = TradingSystem::of_gateway_url(&url);
                        log(&format!("↔ SPA opened gateway socket: {url} ({ts})"));
                        sockets.insert(
                            request.to_string(),
                            SocketSession {
                                gateway_url: url,
                                trading_system: Some(ts),
                                ..Default::default()
                            },
                        );
                    }
                }
                "Network.webSocketFrameReceived" => {
                    let Some(request) = event.params["requestId"].as_str() else {
                        continue;
                    };
                    let Some(socket) = sockets.get_mut(request) else {
                        continue;
                    };
                    let payload = event.params["response"]["payloadData"]
                        .as_str()
                        .unwrap_or_default();
                    apply_frame(socket, payload, log);
                }
                _ => {}
            }
        }

        // 1) sniffed socket session (has everything)
        let sniffed = sockets
            .values()
            .find(|s| s.trading_system == Some(opts.trading_system) && s.access_token.is_some())
            .cloned();
        // 2) sessionStorage session (token + trading system)
        let stored = read_storage(browser, &pages);
        let stored_system = stored
            .as_ref()
            .and_then(|s| s.trading_system.as_deref())
            .and_then(TradingSystem::parse);

        let found = if let Some(s) = sniffed {
            s.access_token.map(|access_token| {
                (
                    BrowserSession {
                        trading_system: opts.trading_system,
                        gateway_url: s.gateway_url,
                        access_token,
                        refresh_token: s.refresh_token,
                        account_code: s.account_code,
                        user_code: s.user_code,
                    },
                    "websocket",
                )
            })
        } else if let Some(st) = stored
            .as_ref()
            .filter(|_| stored_system == Some(opts.trading_system))
        {
            st.token.clone().map(|access_token| {
                (
                    BrowserSession {
                        trading_system: opts.trading_system,
                        gateway_url: fallback_gateway(opts.trading_system),
                        access_token,
                        refresh_token: None,
                        account_code: None,
                        user_code: None,
                    },
                    "sessionStorage",
                )
            })
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
            return Ok(session);
        }

        let status = if let Some(st) = &stored {
            format!(
                "Logged in ({}); waiting for the UI to be in {}… (switch via the account menu in the Chrome window)",
                st.trading_system.clone().unwrap_or_else(|| "unknown system".into()),
                opts.trading_system
            )
        } else {
            let urls: Vec<String> = pages.iter().map(|p| short_url(&p.url)).collect();
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
            return Err(Error::Timeout(format!(
                "waiting for a {} session",
                opts.trading_system
            )));
        }
        // Reading the socket is also the wait: traffic arriving during it is
        // what the next turn classifies.
        browser.pump(Duration::from_millis(1500))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The profile holds a logged-in brokerage session, and it belongs with
    /// the session file that login produced: moving, backing up or deleting
    /// one should take the other with it. It must also never be relative —
    /// as `./browser-profile` it landed in whatever directory the app was
    /// started from, which is a different login per directory and somebody's
    /// Schwab session dropped into a source tree.
    #[test]
    fn the_browser_profile_sits_beside_the_session_file() {
        assert_eq!(
            profile_dir_beside(Path::new("/home/p/.config/omacharts/tos.env")),
            PathBuf::from("/home/p/.config/omacharts/tos-browser")
        );
    }

    #[test]
    fn the_profile_is_never_a_relative_path() {
        for session in [Path::new("tos.env"), Path::new("")] {
            let dir = profile_dir_beside(session);
            assert!(dir.is_absolute(), "{}", dir.display());
        }
    }

    /// A progress line names which pages are open, and a sign-in flow puts
    /// one-time state in the query.
    #[test]
    fn a_page_url_on_a_progress_line_carries_no_sign_in_state() {
        assert_eq!(short_url(""), "(empty)");
        assert_eq!(
            short_url("https://trade.thinkorswim.com/auth"),
            "https://trade.thinkorswim.com/auth"
        );
        assert_eq!(
            short_url("https://trade.thinkorswim.com/oauth?code=SECRET&state=SECRET"),
            "https://trade.thinkorswim.com/oauth…"
        );
        assert_eq!(short_url("about:blank#SECRET"), "about:blank…");
    }

    /// The flags are the finding, not an implementation detail: a driver
    /// crate's puppeteer defaults get a Google sign-in refused outright, so
    /// the list stays short and stays free of automation markers.
    #[test]
    fn the_login_browser_carries_no_automation_flags() {
        for flag in LAUNCH_FLAGS {
            assert!(!flag.starts_with("--"), "{flag} is written without dashes");
            assert!(
                !flag.contains("enable-automation") && !flag.contains("headless"),
                "{flag} would get the sign-in refused"
            );
        }
        assert!(LAUNCH_FLAGS.contains(&"disable-blink-features=AutomationControlled"));
        // The one deliberate departure from stock: the keyring probe costs a
        // D-Bus activation timeout on a profile that saves no passwords.
        assert!(LAUNCH_FLAGS.contains(&"password-store=basic"));
    }
}
