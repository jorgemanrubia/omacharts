//! A captured browser session, its `.env` persistence, and logging in with it.

use std::collections::BTreeMap;
use std::path::Path;

use crate::client::{Client, ClientConfig, Credentials};
use crate::config::TradingSystem;
use crate::services::login::LoginBody;
use crate::tsm::TSM_URL;

/// Shared Schwab session cookie (`Cookie` header value) for `getAuthCode`.
/// One key for both trading systems — a paper slot and a live slot are
/// different gateway sessions, and this cookie is how the web app mints the
/// other one.
pub const TSM_COOKIE_KEY: &str = "TOS_TSM_COOKIE";
/// Optional override of [`TSM_URL`] in the same session file (tests, a moved host).
pub const TSM_URL_KEY: &str = "TOS_TSM_URL";
/// When the gateway last refused this session's token, in unix seconds.
///
/// Written beside the session rather than kept in memory because the question
/// "am I still signed in?" is asked by a settings panel in whatever process
/// happens to be running, and a token's expiry is not something a file can be
/// read for: the only way to learn it is to be told by the gateway, once,
/// possibly hours ago in another process. Marking the file is how that one
/// answer survives to be shown. Cleared by the next connection that works.
pub const EXPIRED_KEY: &str = "TOS_SESSION_EXPIRED_AT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSession {
    pub trading_system: TradingSystem,
    pub gateway_url: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub account_code: Option<String>,
    pub user_code: Option<String>,
}

const KEYS: [&str; 14] = [
    "TOS_TRADING_SYSTEM",
    "TOS_GATEWAY_URL",
    "TOS_ACCESS_TOKEN",
    "TOS_REFRESH_TOKEN",
    "TOS_ACCOUNT_CODE",
    "TOS_USER_CODE",
    "TOS_PAPER_ACCESS_TOKEN",
    "TOS_PAPER_REFRESH_TOKEN",
    "TOS_PAPER_GATEWAY_URL",
    "TOS_PAPER_ACCOUNT_CODE",
    "TOS_LIVE_ACCESS_TOKEN",
    "TOS_LIVE_REFRESH_TOKEN",
    "TOS_LIVE_GATEWAY_URL",
    "TOS_LIVE_ACCOUNT_CODE",
];

struct SlotKeys {
    access: &'static str,
    refresh: &'static str,
    gateway: &'static str,
    account: &'static str,
}

fn slot_keys(ts: TradingSystem) -> SlotKeys {
    match ts {
        TradingSystem::PaperMoney => SlotKeys {
            access: "TOS_PAPER_ACCESS_TOKEN",
            refresh: "TOS_PAPER_REFRESH_TOKEN",
            gateway: "TOS_PAPER_GATEWAY_URL",
            account: "TOS_PAPER_ACCOUNT_CODE",
        },
        TradingSystem::LiveTrading => SlotKeys {
            access: "TOS_LIVE_ACCESS_TOKEN",
            refresh: "TOS_LIVE_REFRESH_TOKEN",
            gateway: "TOS_LIVE_GATEWAY_URL",
            account: "TOS_LIVE_ACCOUNT_CODE",
        },
    }
}

/// The system a session really talks to: the one its gateway URL implies. A
/// label (or slot name) that says otherwise is logged and overridden, so a
/// `.env` that calls a live gateway "paper" cannot bypass the live gate. Only
/// a loopback gateway (a local fake) leaves the label in charge; with neither
/// a label nor a classifiable host the session is live.
fn system_of(label: Option<TradingSystem>, gateway_url: &str, source: &str) -> TradingSystem {
    let Some(of_url) = TradingSystem::implied_by_gateway_url(gateway_url) else {
        return label.unwrap_or(TradingSystem::LiveTrading);
    };
    if let Some(label) = label {
        if label != of_url {
            eprintln!(
                "{source} says {label} but its gateway {gateway_url} is {of_url}; using {of_url}"
            );
        }
    }
    of_url
}

fn session_from_slot(vars: &BTreeMap<String, String>, ts: TradingSystem) -> Option<BrowserSession> {
    let k = slot_keys(ts);
    let access_token = vars.get(k.access).cloned().filter(|s| !s.is_empty())?;
    let gateway_url = vars.get(k.gateway).cloned().filter(|s| !s.is_empty())?;
    Some(BrowserSession {
        trading_system: system_of(Some(ts), &gateway_url, k.gateway),
        gateway_url,
        access_token,
        refresh_token: vars.get(k.refresh).cloned().filter(|s| !s.is_empty()),
        account_code: vars.get(k.account).cloned().filter(|s| !s.is_empty()),
        user_code: vars.get("TOS_USER_CODE").cloned().filter(|s| !s.is_empty()),
    })
}

impl BrowserSession {
    /// Builds a session from `TOS_*` variables (the process environment by
    /// default). Returns `None` unless both a token and a gateway URL are set.
    /// The trading system is the one the URL implies; a disagreeing
    /// `TOS_TRADING_SYSTEM` label is warned about and ignored.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let access_token = get("TOS_ACCESS_TOKEN").filter(|s| !s.is_empty())?;
        let gateway_url = get("TOS_GATEWAY_URL").filter(|s| !s.is_empty())?;
        let label = get("TOS_TRADING_SYSTEM").and_then(|s| TradingSystem::parse(&s));
        let trading_system = system_of(label, &gateway_url, "TOS_TRADING_SYSTEM");
        Some(BrowserSession {
            trading_system,
            gateway_url,
            access_token,
            refresh_token: get("TOS_REFRESH_TOKEN").filter(|s| !s.is_empty()),
            account_code: get("TOS_ACCOUNT_CODE").filter(|s| !s.is_empty()),
            user_code: get("TOS_USER_CODE").filter(|s| !s.is_empty()),
        })
    }

    pub fn from_env() -> Option<Self> {
        Self::from_vars(|k| std::env::var(k).ok())
    }

    /// Reads a `.env`-style file (`KEY=value` lines, `#` comments) and builds a
    /// session from it. Missing file → `None`.
    pub fn from_dotenv(path: impl AsRef<Path>) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let vars = parse_dotenv(&text);
        Self::from_vars(|k| vars.get(k).cloned())
    }

    /// Prefers the process environment, falls back to the file.
    pub fn load(path: impl AsRef<Path>) -> Option<Self> {
        Self::from_env().or_else(|| Self::from_dotenv(path))
    }

    /// Session for `trading_system` from the file: the paper/live slot, or the
    /// current `TOS_*` keys if they already match. Does not retarget a token
    /// from the other gateway — those are different server session ids.
    pub fn load_for(path: impl AsRef<Path>, trading_system: TradingSystem) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let vars = parse_dotenv(&text);
        session_from_slot(&vars, trading_system)
            .filter(|s| s.trading_system == trading_system)
            .or_else(|| {
                let cur = Self::from_vars(|k| vars.get(k).cloned())?;
                (cur.trading_system == trading_system).then_some(cur)
            })
    }

    /// Logs in with this session's access token on its own gateway, then saves
    /// what the gateway's reply says about the session back to `env_file` —
    /// unless the file now holds a token another process refreshed while this
    /// one connected (see [`Self::save_shared`]). The live gate is
    /// [`Client::connect`]'s, judged by the gateway URL.
    pub fn connect(
        self,
        env_file: &Path,
        allow_live_trading: bool,
    ) -> crate::Result<(Client, BrowserSession)> {
        let token = self.access_token.clone();
        self.connect_with(env_file, allow_live_trading, Credentials::AccessToken(token))
    }

    /// Like [`Self::connect`] with an explicit login frame. [`Credentials::AuthCode`]
    /// sends `login/schwab`. `self.access_token` is only the token
    /// [`Self::save_shared`] may replace; a field the reply leaves empty (the
    /// account) stays as it was on `self`.
    pub fn connect_with(
        mut self,
        env_file: &Path,
        allow_live_trading: bool,
        credentials: Credentials,
    ) -> crate::Result<(Client, BrowserSession)> {
        let config = ClientConfig {
            trading_system: self.trading_system,
            gateway_url: self.gateway_url.clone(),
            allow_live_trading,
            ..Default::default()
        };
        let logged_in_with = self.access_token.clone();
        let (client, login) = Client::connect(config, credentials)?;
        self.absorb_login(&login);
        match self.save_shared(env_file, &logged_in_with) {
            Ok(false) => eprintln!("kept a newer ToS session in {}", env_file.display()),
            Err(e) => eprintln!("could not save session to {}: {e}", env_file.display()),
            Ok(true) => {}
        }
        Ok((client, self))
    }

    /// The stored `Cookie` header for TSM, if a browser login captured one.
    pub fn tsm_cookie(path: impl AsRef<Path>) -> Option<String> {
        shared_var(path.as_ref(), TSM_COOKIE_KEY)
    }

    /// `TOS_TSM_URL` in the session file, or the published TSM host.
    pub fn tsm_url(path: impl AsRef<Path>) -> String {
        shared_var(path.as_ref(), TSM_URL_KEY).unwrap_or_else(|| TSM_URL.to_string())
    }

    /// Writes the shared TSM cookie through the same `0600` replace as a
    /// session. A newline in the value would break the file (and could add a
    /// key), so it is refused. Does not log the value.
    pub fn save_tsm_cookie(path: impl AsRef<Path>, cookie: &str) -> std::io::Result<()> {
        if cookie.is_empty() || cookie.contains(['\n', '\r']) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "refusing a TSM cookie that is empty or has a newline",
            ));
        }
        let path = path.as_ref();
        with_dotenv_lock(path, || {
            let existing = read_dotenv(path)?.unwrap_or_default();
            write_dotenv(
                path,
                &existing,
                &[TSM_COOKIE_KEY],
                &[(TSM_COOKIE_KEY, cookie.to_string())],
            )
        })
    }

    /// The gateway's own idea of the session beats whatever the browser
    /// capture saw: a rotated token, the refresh token, the user code. A field
    /// the reply leaves empty keeps its stored value.
    fn absorb_login(&mut self, login: &LoginBody) {
        if !login.token.is_empty() {
            self.access_token = login.token.clone();
        }
        if let Some(refresh) = login.refresh_token() {
            self.refresh_token = Some(refresh.to_string());
        }
        if !login.user_code.is_empty() {
            self.user_code = Some(login.user_code.clone());
        }
    }

    fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("TOS_TRADING_SYSTEM", self.trading_system.to_string()),
            ("TOS_GATEWAY_URL", self.gateway_url.clone()),
            ("TOS_ACCESS_TOKEN", self.access_token.clone()),
        ];
        if let Some(v) = &self.refresh_token {
            out.push(("TOS_REFRESH_TOKEN", v.clone()));
        }
        if let Some(v) = &self.account_code {
            out.push(("TOS_ACCOUNT_CODE", v.clone()));
        }
        if let Some(v) = &self.user_code {
            out.push(("TOS_USER_CODE", v.clone()));
        }
        out
    }

    fn slot_pairs(&self) -> Vec<(&'static str, String)> {
        let k = slot_keys(self.trading_system);
        let mut out = vec![
            (k.access, self.access_token.clone()),
            (k.gateway, self.gateway_url.clone()),
        ];
        if let Some(v) = &self.refresh_token {
            out.push((k.refresh, v.clone()));
        }
        if let Some(v) = &self.account_code {
            out.push((k.account, v.clone()));
        }
        out
    }

    /// Writes the active `TOS_*` keys and the paper/live slot for this
    /// session, keeping the other system's slot and every unrelated line.
    pub fn save_to_dotenv(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        with_dotenv_lock(path, || self.write_dotenv_unlocked(path))
    }

    /// Like [`Self::save_to_dotenv`], with two rules for a session file another
    /// process also loads. Same trading system: skip the write when the
    /// file's token is one this login neither sent (`logged_in_with`) nor
    /// received (`self.access_token`). Other trading system: write only this
    /// system's slot and keep `TOS_TSM_COOKIE`. The active keys stay, so a
    /// live login does not point the other process at live.
    pub fn save_shared(
        &self,
        path: impl AsRef<Path>,
        logged_in_with: &str,
    ) -> std::io::Result<bool> {
        let path = path.as_ref();
        with_dotenv_lock(path, || {
            if let Some(current) = Self::from_dotenv(path) {
                if current.trading_system != self.trading_system {
                    self.write_slot_unlocked(path)?;
                    return Ok(true);
                }
                if current.access_token != logged_in_with
                    && current.access_token != self.access_token
                {
                    return Ok(false);
                }
            }
            self.write_dotenv_unlocked(path)?;
            Ok(true)
        })
    }

    /// This system's slot only. Active keys, the other slot, and every
    /// unrelated line stay. The shared TSM cookie is rewritten in place so
    /// the slot update does not drop it.
    fn write_slot_unlocked(&self, path: &Path) -> std::io::Result<()> {
        let existing = read_dotenv(path)?.unwrap_or_default();
        let vars = parse_dotenv(&existing);
        let k = slot_keys(self.trading_system);
        let remove = [k.access, k.refresh, k.gateway, k.account, TSM_COOKIE_KEY];
        let mut pairs = self.slot_pairs();
        if let Some(cookie) = vars.get(TSM_COOKIE_KEY).filter(|s| !s.is_empty()) {
            pairs.push((TSM_COOKIE_KEY, cookie.clone()));
        }
        write_dotenv(path, &existing, &remove, &pairs)
    }

    fn write_dotenv_unlocked(&self, path: &Path) -> std::io::Result<()> {
        let existing = read_dotenv(path)?.unwrap_or_default();
        let vars = parse_dotenv(&existing);
        let other = match self.trading_system {
            TradingSystem::PaperMoney => TradingSystem::LiveTrading,
            TradingSystem::LiveTrading => TradingSystem::PaperMoney,
        };
        let mut pairs = self.pairs();
        pairs.extend(self.slot_pairs());
        pairs.extend(raw_slot_pairs(&vars, other));
        write_dotenv(path, &existing, &KEYS, &pairs)
    }

    /// Whatever session the file describes, whichever system it belongs to
    /// and whatever it calls itself.
    ///
    /// For telling "nothing here" apart from "something here this feed will
    /// not use": a login that landed on live trading, or a paper slot
    /// pointing at a live gateway, is a sign-in that worked and a feed that
    /// still cannot chart — and reporting that as "no session" sends somebody
    /// round the browser loop again to the same end.
    pub fn any_in(path: impl AsRef<Path>) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let vars = parse_dotenv(&text);
        session_from_slot(&vars, TradingSystem::PaperMoney)
            .or_else(|| session_from_slot(&vars, TradingSystem::LiveTrading))
            .or_else(|| Self::from_vars(|k| vars.get(k).cloned()))
    }

    /// Notes that the gateway refused this file's session, so that whoever
    /// asks later can say "expired" rather than "signed in".
    ///
    /// Best effort by design: the mark is a nicety for the settings panel,
    /// and a feed that cannot write it still reports the refusal through the
    /// failure that caused it.
    pub fn mark_expired(path: impl AsRef<Path>, when: i64) -> std::io::Result<()> {
        let path = path.as_ref();
        with_dotenv_lock(path, || {
            let Some(existing) = read_dotenv(path)? else {
                return Ok(());
            };
            write_dotenv(path, &existing, &[EXPIRED_KEY], &[(EXPIRED_KEY, when.to_string())])
        })
    }

    /// Forgets a refusal, after a connection that worked.
    pub fn clear_expired(path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        with_dotenv_lock(path, || {
            let Some(existing) = read_dotenv(path)? else {
                return Ok(());
            };
            if !existing.lines().any(|line| line_key(line) == EXPIRED_KEY) {
                return Ok(());
            }
            write_dotenv(path, &existing, &[EXPIRED_KEY], &[])
        })
    }

    /// When the gateway last refused this file's session, if it has.
    pub fn expired_at(path: impl AsRef<Path>) -> Option<i64> {
        shared_var(path.as_ref(), EXPIRED_KEY)?.parse().ok()
    }

    /// Removes every `TOS_*` key from the file (a sign-out of both systems),
    /// keeping every unrelated line. A missing file is fine. Prefer
    /// [`BrowserSession::clear_dotenv_for`] to forget one system only.
    pub fn clear_dotenv(path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        with_dotenv_lock(path, || {
            let Some(existing) = read_dotenv(path)? else {
                return Ok(());
            };
            let mut remove: Vec<&str> = KEYS.to_vec();
            remove.push(TSM_COOKIE_KEY);
            remove.push(EXPIRED_KEY);
            write_dotenv(path, &existing, &remove, &[])
        })
    }

    /// Forgets one trading system: removes its paper/live slot and the active
    /// `TOS_*` keys, keeping the other system's slot and every unrelated line.
    pub fn clear_dotenv_for(
        path: impl AsRef<Path>,
        trading_system: TradingSystem,
    ) -> std::io::Result<()> {
        let path = path.as_ref();
        with_dotenv_lock(path, || {
            let Some(existing) = read_dotenv(path)? else {
                return Ok(());
            };
            let k = slot_keys(trading_system);
            let remove = [
                "TOS_TRADING_SYSTEM",
                "TOS_GATEWAY_URL",
                "TOS_ACCESS_TOKEN",
                "TOS_REFRESH_TOKEN",
                "TOS_ACCOUNT_CODE",
                "TOS_USER_CODE",
                k.access,
                k.refresh,
                k.gateway,
                k.account,
                // A sign-out leaves no stale "expired" behind for the next
                // sign-in to be judged by.
                EXPIRED_KEY,
            ];
            write_dotenv(path, &existing, &remove, &[])
        })
    }
}

/// Exclusive lock beside the env file so two processes cannot interleave a
/// read-modify-write of the shared session.
fn with_dotenv_lock<T>(
    path: &Path,
    body: impl FnOnce() -> std::io::Result<T>,
) -> std::io::Result<T> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(".env");
    let lock_path = path.with_file_name(format!("{name}.lock"));
    if let Some(parent) = lock_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(&lock_path)?;
    lock.lock()?; // released when `lock` is dropped
    body()
}

/// Missing file → `None`. Invalid UTF-8 and other IO errors propagate so a
/// later atomic replace cannot treat a damaged file as empty.
fn shared_var(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut vars = parse_dotenv(&text);
    vars.remove(key).filter(|s| !s.is_empty())
}

fn read_dotenv(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The other system's slot exactly as stored, so saving one session never
/// rewrites or relabels the other.
fn raw_slot_pairs(
    vars: &BTreeMap<String, String>,
    ts: TradingSystem,
) -> Vec<(&'static str, String)> {
    let k = slot_keys(ts);
    [k.access, k.refresh, k.gateway, k.account]
        .into_iter()
        .filter_map(|key| {
            vars.get(key)
                .filter(|v| !v.is_empty())
                .map(|v| (key, v.clone()))
        })
        .collect()
}

/// `KEY` of a `.env` line, tolerating `export KEY=…` and surrounding blanks.
fn line_key(line: &str) -> &str {
    let key = line.split('=').next().unwrap_or("").trim();
    key.strip_prefix("export ").map(str::trim).unwrap_or(key)
}

/// Rewrites the file without the lines whose key is in `remove`, appending
/// `pairs`. The new content goes to a `0600` temp file in the same directory
/// that is fsynced and renamed over the target, so a crash mid-write can never
/// leave a truncated file or world-readable tokens.
fn write_dotenv(
    path: &Path,
    existing: &str,
    remove: &[&str],
    pairs: &[(&'static str, String)],
) -> std::io::Result<()> {
    let mut kept: Vec<&str> = existing
        .lines()
        .filter(|l| !remove.contains(&line_key(l)))
        .collect();
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    let mut text = kept.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    for (k, v) in pairs {
        text.push_str(&format!("{k}={v}\n"));
    }
    write_private_atomically(path, text.as_bytes())
}

fn write_private_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(".env");
    let tmp_name = format!(".{name}.{}.tmp", std::process::id());
    let tmp = match dir {
        Some(d) => d.join(&tmp_name),
        None => Path::new(&tmp_name).to_path_buf(),
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// `KEY=value` lines (`#` comments skipped, optional `export`, quotes stripped).
pub fn parse_dotenv(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            let v = v.trim().trim_matches('"').trim_matches('\'');
            Some((line_key(k).to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const PAPER_URL: &str = "wss://papermoney-services.schwab.com/Services/WsJson";
    const LIVE_URL: &str = "wss://thinkorswim-services.schwab.com/Services/WsJson";

    /// A session with just a token, on its own system's gateway.
    fn session(trading_system: TradingSystem, token: &str) -> BrowserSession {
        let gateway_url = match trading_system {
            TradingSystem::PaperMoney => PAPER_URL,
            TradingSystem::LiveTrading => LIVE_URL,
        };
        BrowserSession {
            trading_system,
            gateway_url: gateway_url.into(),
            access_token: token.into(),
            refresh_token: None,
            account_code: None,
            user_code: None,
        }
    }

    /// `.env` in a fresh `tos-market-<tag>-<pid>` temp directory, and that directory.
    fn env_path(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("tos-market-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (dir.join(".env"), dir)
    }

    #[test]
    fn round_trips_through_dotenv_and_keeps_other_lines() {
        let (path, dir) = env_path("roundtrip");
        std::fs::write(&path, "OTHER=1\nTOS_ACCESS_TOKEN=old\n").unwrap();
        let s = BrowserSession {
            account_code: Some("D-1".into()),
            ..session(TradingSystem::PaperMoney, "tok")
        };
        s.save_to_dotenv(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("OTHER=1\n"));
        assert!(!text.contains("old"));
        assert_eq!(BrowserSession::from_dotenv(&path).unwrap(), s);
        BrowserSession::clear_dotenv(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "OTHER=1\n");
        assert!(BrowserSession::from_dotenv(&path).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_shared_keeps_a_token_this_login_did_not_hold() {
        let (path, dir) = env_path("shared");
        let base = session(TradingSystem::PaperMoney, "tok");
        base.save_to_dotenv(&path).unwrap();
        let newer = session(TradingSystem::PaperMoney, "newer");
        newer.save_to_dotenv(&path).unwrap();

        let stale = BrowserSession {
            user_code: Some("stale".into()),
            ..base
        };
        assert!(!stale.save_shared(&path, "tok").unwrap());
        assert_eq!(
            BrowserSession::from_dotenv(&path).unwrap().access_token,
            "newer"
        );

        let same_login = BrowserSession {
            user_code: Some("u".into()),
            ..newer
        };
        assert!(same_login.save_shared(&path, "newer").unwrap());
        assert_eq!(
            BrowserSession::from_dotenv(&path)
                .unwrap()
                .user_code
                .as_deref(),
            Some("u")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn save_shared_writes_this_system_when_the_active_session_is_the_other() {
        let (path, dir) = env_path("shared-other");
        let paper = BrowserSession {
            account_code: Some("D-1".into()),
            ..session(TradingSystem::PaperMoney, "paper-tok")
        };
        paper.save_to_dotenv(&path).unwrap();
        BrowserSession::save_tsm_cookie(&path, "tsm=keep").unwrap();
        let live = BrowserSession {
            account_code: None,
            ..session(TradingSystem::LiveTrading, "live-new")
        };
        assert!(live.save_shared(&path, "live-old").unwrap());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("TOS_TSM_COOKIE=tsm=keep"), "{text}");
        let loaded_paper = BrowserSession::load_for(&path, TradingSystem::PaperMoney).unwrap();
        assert_eq!(loaded_paper.access_token, "paper-tok");
        assert_eq!(loaded_paper.account_code.as_deref(), Some("D-1"));
        let loaded_live = BrowserSession::load_for(&path, TradingSystem::LiveTrading).unwrap();
        assert_eq!(loaded_live.access_token, "live-new");
        assert_eq!(loaded_live.account_code, None);
        let active = BrowserSession::from_dotenv(&path).unwrap();
        assert_eq!(active.trading_system, TradingSystem::PaperMoney);
        assert_eq!(active.access_token, "paper-tok");
        assert_eq!(active.account_code.as_deref(), Some("D-1"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tsm_cookie_round_trips_and_rejects_a_newline() {
        let (path, dir) = env_path("cookie");
        session(TradingSystem::PaperMoney, "tok")
            .save_to_dotenv(&path)
            .unwrap();
        assert!(BrowserSession::save_tsm_cookie(&path, "a=b\nc=d").is_err());
        BrowserSession::save_tsm_cookie(&path, "sid=abc").unwrap();
        assert_eq!(
            BrowserSession::tsm_cookie(&path).as_deref(),
            Some("sid=abc")
        );
        assert_eq!(BrowserSession::tsm_url(&path), TSM_URL);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("TOS_TSM_URL=http://127.0.0.1:9\n");
        std::fs::write(&path, text).unwrap();
        assert_eq!(BrowserSession::tsm_url(&path), "http://127.0.0.1:9");
        assert_eq!(
            BrowserSession::load_for(&path, TradingSystem::PaperMoney)
                .unwrap()
                .access_token,
            "tok"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_login_reply_updates_only_what_it_carries() {
        let stored = BrowserSession {
            refresh_token: Some("stored-refresh".into()),
            user_code: Some("stored-user".into()),
            ..session(TradingSystem::PaperMoney, "tok")
        };
        let mut s = stored.clone();
        s.absorb_login(&LoginBody::default());
        assert_eq!(s, stored, "an empty reply keeps every stored value");
        let reply: LoginBody = serde_json::from_value(serde_json::json!({
            "authenticationStatus": "OK", "token": "rotated", "userCode": "u1",
            "accessTokenInfo": {"refreshToken": "fresh"}
        }))
        .unwrap();
        s.absorb_login(&reply);
        assert_eq!(s.access_token, "rotated");
        assert_eq!(s.refresh_token.as_deref(), Some("fresh"));
        assert_eq!(s.user_code.as_deref(), Some("u1"));
        assert_eq!(s.gateway_url, stored.gateway_url);
    }

    fn vars_session(label: Option<&str>, url: &str) -> BrowserSession {
        BrowserSession::from_vars(|k| match k {
            "TOS_ACCESS_TOKEN" => Some("t".into()),
            "TOS_TRADING_SYSTEM" => label.map(str::to_string),
            "TOS_GATEWAY_URL" => Some(url.to_string()),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn the_gateway_url_beats_a_disagreeing_label() {
        use TradingSystem::{LiveTrading, PaperMoney};
        let loopback = "ws://127.0.0.1:8765/Services/WsJson";
        for (label, url, system) in [
            // A "paper" label on a live gateway is live: the client's gate then
            // refuses it without allow_live_trading.
            (Some("PaperMoney"), LIVE_URL, LiveTrading),
            // A "live" label on the paper gateway is paper.
            (Some("LiveTrading"), PAPER_URL, PaperMoney),
            (Some("PaperMoney"), PAPER_URL, PaperMoney),
            (Some("LiveTrading"), LIVE_URL, LiveTrading),
            (None, PAPER_URL, PaperMoney),
            // A local fake gateway is whatever the label says; unlabelled it is live.
            (Some("PaperMoney"), loopback, PaperMoney),
            (Some("LiveTrading"), loopback, LiveTrading),
            (None, loopback, LiveTrading),
        ] {
            assert_eq!(
                vars_session(label, url).trading_system,
                system,
                "{label:?} {url}"
            );
        }
        assert!(BrowserSession::from_vars(|_| None).is_none());
    }

    #[test]
    fn a_slot_whose_url_points_at_the_other_system_is_not_returned_for_it() {
        let (path, dir) = env_path("slot-url");
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={LIVE_URL}\n"),
        )
        .unwrap();
        assert!(BrowserSession::load_for(&path, TradingSystem::PaperMoney).is_none());
        assert!(BrowserSession::load_for(&path, TradingSystem::LiveTrading).is_none());
        std::fs::write(
            &path,
            format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={PAPER_URL}\n"),
        )
        .unwrap();
        let s = BrowserSession::load_for(&path, TradingSystem::PaperMoney).unwrap();
        assert_eq!(s.trading_system, TradingSystem::PaperMoney);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dotenv_writes_tolerate_export_and_padded_keys() {
        let (path, dir) = env_path("export");
        std::fs::write(
            &path,
            "export OTHER=1\nexport TOS_ACCESS_TOKEN=old\n  TOS_GATEWAY_URL = wss://old \n# TOS_ACCESS_TOKEN=comment\n",
        )
        .unwrap();
        let vars = parse_dotenv(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(vars.get("OTHER").map(String::as_str), Some("1"));
        assert_eq!(
            vars.get("TOS_ACCESS_TOKEN").map(String::as_str),
            Some("old")
        );
        assert_eq!(
            vars.get("TOS_GATEWAY_URL").map(String::as_str),
            Some("wss://old")
        );
        let s = session(TradingSystem::PaperMoney, "new");
        s.save_to_dotenv(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("export OTHER=1\n# TOS_ACCESS_TOKEN=comment\n"),
            "{text}"
        );
        assert!(!text.contains("old"), "{text}");
        assert_eq!(text.matches("TOS_ACCESS_TOKEN=").count(), 2, "{text}");
        assert_eq!(BrowserSession::from_dotenv(&path).unwrap(), s);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dotenv_is_written_private_and_no_temp_file_remains() {
        use std::os::unix::fs::PermissionsExt;
        let (path, dir) = env_path("mode");
        std::fs::write(&path, "OTHER=1\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        session(TradingSystem::PaperMoney, "tok")
            .save_to_dotenv(&path)
            .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{mode:o}");
        let lock_mode = std::fs::metadata(dir.join(".env.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(lock_mode, 0o600, "{lock_mode:o}");
        let mut entries: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        entries.sort();
        assert_eq!(entries, vec![".env".to_string(), ".env.lock".to_string()]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clearing_one_system_keeps_the_other_slot() {
        let (path, dir) = env_path("clear");
        std::fs::write(&path, "OTHER=1\n").unwrap();
        let paper = BrowserSession {
            refresh_token: Some("paper-ref".into()),
            account_code: Some("D-1".into()),
            user_code: Some("u".into()),
            ..session(TradingSystem::PaperMoney, "paper-tok")
        };
        let live = BrowserSession {
            account_code: Some("400".into()),
            user_code: Some("u".into()),
            ..session(TradingSystem::LiveTrading, "live-tok")
        };
        paper.save_to_dotenv(&path).unwrap();
        live.save_to_dotenv(&path).unwrap();
        BrowserSession::save_tsm_cookie(&path, "tsm=keep").unwrap();
        BrowserSession::clear_dotenv_for(&path, TradingSystem::LiveTrading).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("OTHER=1\n"), "{text}");
        assert!(text.contains("TOS_TSM_COOKIE=tsm=keep"), "{text}");
        assert!(!text.contains("live-tok"), "{text}");
        assert!(!text.contains("TOS_ACCESS_TOKEN="), "{text}");
        assert!(!text.contains("TOS_USER_CODE="), "{text}");
        assert!(BrowserSession::load_for(&path, TradingSystem::LiveTrading).is_none());
        let kept = BrowserSession::load_for(&path, TradingSystem::PaperMoney).unwrap();
        assert_eq!(kept.access_token, "paper-tok");
        assert_eq!(kept.refresh_token.as_deref(), Some("paper-ref"));
        assert_eq!(kept.account_code.as_deref(), Some("D-1"));
        // The full sign-out still wipes both.
        BrowserSession::clear_dotenv(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "OTHER=1\n");
        // A missing file is fine for either.
        std::fs::remove_file(&path).unwrap();
        BrowserSession::clear_dotenv_for(&path, TradingSystem::PaperMoney).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn paper_and_live_slots_are_independent() {
        let (path, dir) = env_path("slots");
        let paper = BrowserSession {
            account_code: Some("D-1".into()),
            user_code: Some("u".into()),
            ..session(TradingSystem::PaperMoney, "paper-tok")
        };
        let live = BrowserSession {
            account_code: Some("400".into()),
            user_code: Some("u".into()),
            ..session(TradingSystem::LiveTrading, "live-tok")
        };
        paper.save_to_dotenv(&path).unwrap();
        live.save_to_dotenv(&path).unwrap();
        let loaded_paper = BrowserSession::load_for(&path, TradingSystem::PaperMoney).unwrap();
        let loaded_live = BrowserSession::load_for(&path, TradingSystem::LiveTrading).unwrap();
        assert_eq!(loaded_paper.access_token, "paper-tok");
        assert_eq!(loaded_paper.account_code.as_deref(), Some("D-1"));
        assert_eq!(loaded_live.access_token, "live-tok");
        assert_eq!(loaded_live.account_code.as_deref(), Some("400"));
        assert_eq!(
            BrowserSession::from_dotenv(&path).unwrap().access_token,
            "live-tok"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_read_failure_preserves_file() {
        let (path, dir) = env_path("bad-utf8");
        let garbage = b"TOS_ACCESS_TOKEN=\xff\xfe\nOTHER=keep\n";
        std::fs::write(&path, garbage).unwrap();
        let s = session(TradingSystem::PaperMoney, "tok");
        assert_eq!(
            s.save_to_dotenv(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path).unwrap(), garbage);
        assert_eq!(
            BrowserSession::clear_dotenv(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path).unwrap(), garbage);
        assert_eq!(
            BrowserSession::clear_dotenv_for(&path, TradingSystem::PaperMoney)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path).unwrap(), garbage);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
