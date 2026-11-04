//! Which data feeds this process charts from, and in what order.
//!
//! Two ways to say it, and they answer different questions. The stored
//! `providers` setting is the default — the feeds this machine charts from,
//! top first, Yahoo among them wherever it was put, or not at all once it
//! was removed; nothing stored is Yahoo alone —
//! changed by the settings panel or by `config set provider`, which puts a
//! feed on top. `--provider` on the launch is this run only, for trying a
//! feed out, for a script that wants one particular source, and for saying
//! which feed a bug report is about: it puts that feed alone above Yahoo.
//!
//! Each chart is served by the first feed in the order that can be used
//! and has a name for its instrument — see
//! `omacharts_engine::providers::route`. With nothing stored that is Yahoo
//! for everything, as it always was.
//!
//! The flag wins for the launch it was typed at, and a flag typed at a
//! window already open switches that window without storing anything. A
//! window switches feeds live — see `Window::adopt_feed` — so what is
//! stored and what is on screen can differ only until the window reads the
//! setting back, which the panel and `config set provider` make it do at
//! once.
//!
//! There is deliberately no environment variable. `OMACHARTS_PROVIDER` used
//! to do this job, and an environment variable is the one way of passing an
//! argument that appears in no help output, no completion and no surface
//! description — so an agent cannot discover it, and nobody can be told
//! about it except in prose. A declared flag is the same capability, written
//! down once in `cli::spec`.

use std::sync::OnceLock;

use omacharts_engine::providers::{self, Listed};
use omacharts_engine::Provider;

use crate::store::Store;

/// The setting the stored order lives in: a JSON array of feed ids, top
/// first, Yahoo included where it is in the order. Not stored at all while
/// the order is Yahoo alone, which is what a machine that never chose has.
pub const SETTING: &str = "providers";

/// What `config set` still answers to: the one feed a machine charted from
/// before there was an order. Setting it puts that feed on top.
pub const ONE_FEED: &str = "provider";

static FOR_THIS_LAUNCH: OnceLock<&'static Listed> = OnceLock::new();

/// The feed on top of the window's order right now, set each time it builds
/// its feeds. What `provider list` and `provider status` mean by "in use"
/// once a window is open; absent, the launch flag and then the stored
/// setting say what a window would start on.
static RUNNING: std::sync::RwLock<Option<&'static Listed>> = std::sync::RwLock::new(None);

/// The window is charting from `id` first from now on. A feed that is not
/// listed — the synthetic one, in a benchmark — is nobody's business here.
pub fn note_running(id: &str) {
    if let Some(feed) = providers::listed(id) {
        *RUNNING.write().unwrap_or_else(|e| e.into_inner()) = Some(feed);
    }
}

/// Chart from `feed` for the life of this process, whatever is stored.
///
/// Takes a feed from the catalogue rather than a name, so an unknown one is
/// refused where the argument is parsed — by then this cannot fail, and
/// there is no second place deciding what a valid feed is called.
pub fn use_for_this_launch(feed: &'static Listed) {
    let _ = FOR_THIS_LAUNCH.set(feed);
}

/// The feed `--provider` asked for, if this launch was given one.
pub fn for_this_launch() -> Option<&'static Listed> {
    FOR_THIS_LAUNCH.get().copied()
}

/// The stored order, top first, as it was written: ids this build does not
/// know are kept, so a setting from a newer build survives being read.
/// Nothing stored, or something that is not a list, or an empty one, is the
/// default alone.
pub fn stored_order(store: &Store) -> Vec<String> {
    store
        .setting(SETTING)
        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .filter(|order| !order.is_empty())
        .unwrap_or_else(|| vec![providers::DEFAULT.to_string()])
}

/// Write `order` down, each id once. The default alone, or nothing, is no
/// row at all, which is what a machine that never chose has.
pub fn save_order(store: &Store, order: &[String]) {
    let kept: Vec<&str> = order
        .iter()
        .map(|id| providers::listed(id).map_or(id.as_str(), |feed| feed.id))
        .fold(Vec::new(), |mut kept, id| {
            if !kept.contains(&id) {
                kept.push(id);
            }
            kept
        });
    match kept.as_slice() {
        [] => store.remove_setting(SETTING),
        [only] if *only == providers::DEFAULT => store.remove_setting(SETTING),
        _ => store.set_setting(SETTING, &serde_json::to_string(&kept).unwrap_or_default()),
    }
}

/// Put `feed` on top of the stored order, which is what choosing one feed
/// means now that there are several. The rest of the order stays under it.
pub fn put_on_top(store: &Store, feed: &'static Listed) {
    let mut order = vec![feed.id.to_string()];
    order.extend(stored_order(store).into_iter().filter(|id| !id.eq_ignore_ascii_case(feed.id)));
    save_order(store, &order);
}

/// What `--provider` charts from: that feed alone above Yahoo, or Yahoo
/// alone.
pub fn launch_order(feed: &'static Listed) -> Vec<String> {
    match feed.id == providers::DEFAULT {
        true => vec![feed.id.to_string()],
        false => vec![feed.id.to_string(), providers::DEFAULT.to_string()],
    }
}

/// The order this process charts from: the launch flag's, otherwise what
/// is stored.
pub fn order(store: &Store) -> Vec<String> {
    match for_this_launch() {
        Some(feed) => launch_order(feed),
        None => stored_order(store),
    }
}

/// The feeds themselves, top first, each ready to
/// be asked for bars. Cheap: building them connects to nothing. See
/// `omacharts_engine::providers`.
pub fn ordered(store: &Store) -> Vec<Box<dyn Provider>> {
    let order = order(store);
    if order.iter().any(|id| providers::listed(id).is_some_and(trades)) {
        remember_logins(store);
    }
    providers::ordered(&order)
}

/// Is `feed`'s login a broker connection, kept in Preferences → Trading?
fn trades(feed: &Listed) -> bool {
    feed.setup.is_some_and(|setup| setup.trades())
}

/// Tell the engine which release of Omacharts is signing in: this crate's
/// version, the app's, which no library crate's is. Called once at
/// startup.
pub fn identify() {
    providers::identify(env!("CARGO_PKG_VERSION"));
}

/// Hand every feed whose login is a broker connection the data login: the
/// login its data signs in through, out of the logins in Preferences →
/// Trading, which are settings, `Local` in `setting_keys`. The password is
/// never here; it is in the keyring under the login's id. The engine keeps
/// no database, so it is told, whenever the feeds are read and whenever a
/// sign-in or a sign-out may have changed what is saved.
///
/// Two settings reads, and only for an order that names such a feed or a
/// command about feeds: with nothing configured nothing is read, built or
/// started.
pub fn remember_logins(store: &Store) {
    let data = crate::trading::data_login(store);
    for feed in providers::every().filter(|feed| trades(feed)) {
        if let Some(setup) = feed.setup {
            setup.remember(saved_for(data.as_ref(), feed));
        }
    }
}

/// The data login as the engine is told it: no user while it is signed
/// out, which is what makes the data feed not signed in.
pub fn saved_of(store: &Store) -> providers::Saved {
    let login = crate::trading::data_login(store);
    let feed = login.as_ref().and_then(crate::trading::data_feed_of);
    feed.map(|feed| saved_for(login.as_ref(), feed)).unwrap_or_default()
}

/// What `feed` is told of the data login: the login, when it is one of
/// the feed's own broker's, and nothing when it is another's.
fn saved_for(login: Option<&crate::trading::Login>, feed: &Listed) -> providers::Saved {
    let Some(login) = login.filter(|login| login.broker.eq_ignore_ascii_case(feed.id)) else {
        return providers::Saved::default();
    };
    providers::Saved {
        user: (!login.signed_out).then(|| login.user.clone()),
        values: login.values.clone(),
        login: Some(login.id.clone()),
    }
}

/// The settings a feed's sign-in is read from: the window charts again when
/// one changes, since which charts a login can serve may have too.
pub fn signs_in_from(key: &str) -> bool {
    [crate::trading::LOGINS, crate::trading::DATA_LOGIN].contains(&key)
}

/// What a feed keeps in the settings beyond its session, forgotten when it
/// is signed out of. For a broker connection's feed, the data login is
/// marked signed out: its password went with the session, and it stays in
/// Trading, with its accounts, for the next sign-in.
pub fn forget_login(store: &Store, id: &str) {
    if !providers::listed(id).is_some_and(trades) {
        return;
    }
    if let Some(data) = crate::trading::data_login(store) {
        crate::trading::mark_signed_out(store, &data.id);
    }
    remember_logins(store);
}

/// Add `id` to the stored order, after the feeds already there and just
/// above Yahoo while Yahoo is on it: a feed somebody has just added is tried
/// after the ones they already put in order, and under Yahoo, which serves
/// nearly everything, it would hardly ever be reached. Yahoo itself goes at
/// the bottom. Adding one already there changes nothing.
pub fn add_source(store: &Store, id: &str) {
    let mut order = stored_order(store);
    if order.iter().any(|known| known.eq_ignore_ascii_case(id)) {
        return;
    }
    let yahoo = order.iter().position(|known| known.eq_ignore_ascii_case(providers::DEFAULT));
    match yahoo {
        Some(at) if !id.eq_ignore_ascii_case(providers::DEFAULT) => order.insert(at, id.to_string()),
        _ => order.push(id.to_string()),
    }
    save_order(store, &order);
}

/// May `id` be taken off the order? Not when it is the only feed this
/// build knows on it: with none at all no chart could draw.
pub fn removable(store: &Store, id: &str) -> bool {
    stored_order(store)
        .iter()
        .any(|known| !known.eq_ignore_ascii_case(id) && providers::listed(known).is_some())
}

/// Take `id` off the stored order. For a broker connection's feed the data
/// session ends; the logins it signed in through are Trading's, and stay. A
/// feed's own session is signed out of from its row rather than by removing
/// it.
pub fn remove_source(store: &Store, id: &str) -> bool {
    if !removable(store, id) {
        return false;
    }
    let order: Vec<String> =
        stored_order(store).into_iter().filter(|known| !known.eq_ignore_ascii_case(id)).collect();
    save_order(store, &order);
    if let Some(setup) = providers::listed(id).filter(|feed| trades(feed)).and_then(|feed| feed.setup) {
        setup.hand_over();
        remember_logins(store);
    }
    true
}

/// Sign in to `id`, blocking until it is done: a browser the person drives,
/// or a check of what is saved followed by the window's own session signing
/// in, so that the answer is the provider's. For a thread of its own.
///
/// Each step and how it ended go in the Log too, under the feed's name.
pub fn sign_in_now(id: &str, log: impl Fn(&str) + Send + Sync + 'static) -> Result<(), String> {
    let name = providers::listed(id).map_or(id.to_string(), |feed| feed.label.to_string());
    let said = name.clone();
    let outcome = providers::sign_in(id, move |line| {
        crate::log_words::say(crate::log_words::feed_step(&said, line));
        log(line);
    })
    .and_then(|()| providers::listed(id).and_then(|feed| feed.setup).map_or(Ok(()), |setup| setup.connect()));
    crate::log_words::say(crate::log_words::feed_signed_in(&name, &outcome));
    outcome
}

/// A process's feeds with whether each can be used, for routing a chart to
/// one. Usable is read from what is saved, never by connecting.
pub struct Feeds {
    pub feeds: Vec<(Box<dyn Provider>, bool)>,
}

impl Feeds {
    pub fn load(store: &Store) -> Feeds {
        Feeds::of(ordered(store))
    }

    pub fn of(providers: Vec<Box<dyn Provider>>) -> Feeds {
        let feeds = providers
            .into_iter()
            .map(|provider| {
                let usable = providers::usable(provider.id());
                (provider, usable)
            })
            .collect();
        Feeds { feeds }
    }

    /// The feed serving `instrument`, and its spelling there.
    pub fn route(&self, instrument: &omacharts_engine::Instrument) -> Option<(&dyn Provider, String)> {
        let route = providers::route(self.iter(), instrument)?;
        let (provider, _) = self.feeds.get(route.at)?;
        Some((provider.as_ref(), route.symbol))
    }

    fn iter(&self) -> impl Iterator<Item = (&dyn Provider, bool)> {
        self.feeds.iter().map(|(provider, usable)| (provider.as_ref(), *usable))
    }
}

/// How the top feed is offered — its name, what it serves, whether it needs
/// signing in to. The one a window is charting from first, otherwise the
/// flag for this launch, otherwise the stored order, otherwise the default.
pub fn in_use(store: &Store) -> &'static Listed {
    (*RUNNING.read().unwrap_or_else(|e| e.into_inner()))
        .or_else(for_this_launch)
        .unwrap_or_else(|| stored(store))
}

/// The top of the stored order, as a feed in the catalogue — ignoring any
/// flag this launch was given.
///
/// For the settings panel, which has to show what is *stored* rather than
/// what is running, or a launch flag would look like a changed setting and
/// get changed back by somebody tidying up.
pub fn stored(store: &Store) -> &'static Listed {
    stored_order(store)
        .iter()
        .find_map(|id| providers::listed(id))
        .unwrap_or_else(|| providers::listed(providers::DEFAULT).expect("the default is listed"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn store() -> Store {
        Store::memory().expect("an empty database")
    }

    /// Rithmic is told the release in the root Cargo.toml, not the version
    /// of the engine or the rithmic crate, which stay at 0.1.0.
    #[test]
    fn rithmic_is_told_the_apps_version() {
        identify();
        let app = omacharts_engine::providers::rithmic::session::app().expect("identified");
        let manifest = include_str!("../Cargo.toml");
        let release = manifest
            .lines()
            .find_map(|line| line.strip_prefix("version = \""))
            .and_then(|rest| rest.strip_suffix('"'))
            .expect("the package's version");
        assert_eq!(app.version, release);
        assert_ne!(app.version, "0.1.0");
        assert_eq!(app.name, omacharts_engine::providers::rithmic::session::APP_NAME);
    }

    #[test]
    fn nothing_stored_is_the_default_feed() {
        let store = store();
        assert_eq!(in_use(&store).id, providers::DEFAULT);
        assert_eq!(stored(&store).id, providers::DEFAULT);
    }

    #[test]
    fn a_stored_feed_is_used() {
        let store = store();
        store.set_setting(SETTING, r#"["tos","yahoo"]"#);
        assert_eq!(stored(&store).id, "tos");
        let ids: Vec<&str> = providers::ordered(&stored_order(&store)).iter().map(|p| p.id()).collect();
        assert_eq!(ids, vec!["tos", "yahoo"]);
        // Without Yahoo, only what is listed.
        store.set_setting(SETTING, r#"["tos"]"#);
        let ids: Vec<&str> = providers::ordered(&stored_order(&store)).iter().map(|p| p.id()).collect();
        assert_eq!(ids, vec!["tos"]);
    }

    /// Choosing one feed puts it on top and keeps the rest of the order;
    /// Yahoo the same as any other.
    #[test]
    fn a_feed_put_on_top_goes_first() {
        let store = store();
        let tos = providers::listed("tos").unwrap();
        let yahoo = providers::listed("yahoo").unwrap();
        put_on_top(&store, tos);
        assert_eq!(store.setting(SETTING).as_deref(), Some(r#"["tos","yahoo"]"#));
        store.set_setting(SETTING, r#"["later","tos"]"#);
        put_on_top(&store, tos);
        assert_eq!(stored_order(&store), vec!["tos", "later"]);
        put_on_top(&store, yahoo);
        assert_eq!(stored_order(&store), vec!["yahoo", "tos", "later"]);
        save_order(&store, &["yahoo".into()]);
        assert_eq!(store.setting(SETTING), None, "Yahoo alone is the default");
    }

    /// The engine is handed the data login, and nothing while it is
    /// signed out; the password is in no setting.
    #[test]
    fn the_engine_is_handed_the_data_login() {
        use crate::trading::{self, fake::Kept, Typed};
        if omacharts_engine::providers::rithmic::session::development() {
            return;
        }
        let store = store();
        assert_eq!(saved_of(&store), Default::default());
        let kept = Kept::default();
        let typed = Typed {
            broker: "rithmic".into(),
            name: "Lucid".into(),
            user: " trader ".into(),
            password: "PLANTED-PASSWORD".into(),
            values: vec![("system".into(), "Rithmic Test".into()), ("gateway".into(), "wss://example.invalid:443".into())],
        };
        let id = trading::add_login(&store, &kept, &typed).unwrap();
        let saved = saved_of(&store);
        assert_eq!(saved.login.as_deref(), Some(id.as_str()));
        assert_eq!(saved.user.as_deref(), Some("trader"));
        assert_eq!(saved.value("system"), Some("Rithmic Test"));
        assert_eq!(saved.value("gateway"), Some("wss://example.invalid:443"));
        assert_eq!(stored_order(&store), vec!["rithmic", "yahoo"]);

        forget_login(&store, "rithmic");
        assert_eq!(saved_of(&store).user, None, "signed out is no user");
        assert_eq!(trading::logins(&store).len(), 1, "the login stays in Trading");
        assert!(remove_source(&store, "rithmic"));
        assert_eq!(store.setting(SETTING), None);
        assert_eq!(trading::logins(&store).len(), 1);
    }

    /// A feed added goes after the others, above Yahoo, and only once.
    #[test]
    fn an_added_source_goes_last_above_yahoo() {
        let store = store();
        add_source(&store, "rithmic");
        add_source(&store, "tos");
        add_source(&store, "TOS");
        assert_eq!(stored_order(&store), vec!["rithmic", "tos", "yahoo"]);
        assert!(remove_source(&store, "tos"));
        assert_eq!(stored_order(&store), vec!["rithmic", "yahoo"]);
    }

    /// Yahoo is a feed like the others: it can be removed, added back, at
    /// the bottom, and moved above another. The last feed cannot go.
    #[test]
    fn yahoo_can_be_removed_added_back_and_moved_but_the_last_feed_stays() {
        let store = store();
        assert!(!removable(&store, "yahoo"), "Yahoo alone is the last feed");
        assert!(!remove_source(&store, "yahoo"));
        add_source(&store, "rithmic");
        assert!(remove_source(&store, "yahoo"));
        assert_eq!(store.setting(SETTING).as_deref(), Some(r#"["rithmic"]"#));
        assert!(!remove_source(&store, "rithmic"), "the last feed");
        assert_eq!(stored_order(&store), vec!["rithmic"]);
        add_source(&store, "yahoo");
        assert_eq!(stored_order(&store), vec!["rithmic", "yahoo"]);
        save_order(&store, &["yahoo".into(), "rithmic".into()]);
        assert_eq!(store.setting(SETTING).as_deref(), Some(r#"["yahoo","rithmic"]"#));
    }

    /// A setting written by a future version, or by hand. Charting from
    /// Yahoo and saying so beats refusing to start.
    #[test]
    fn a_stored_feed_nobody_has_heard_of_is_the_default() {
        let store = store();
        store.set_setting(SETTING, r#"["bloomberg"]"#);
        assert_eq!(stored(&store).id, providers::DEFAULT);
        store.set_setting(SETTING, "tos");
        assert_eq!(stored(&store).id, providers::DEFAULT, "not a list");
    }

    /// The flag is a `OnceLock`, so it cannot be exercised twice in one test
    /// binary. This asserts the rule that does not need setting it: with no
    /// flag given, the stored setting decides.
    #[test]
    fn with_no_flag_the_stored_setting_decides() {
        let store = store();
        store.set_setting(SETTING, r#"["tos"]"#);
        assert_eq!(
            in_use(&store).id,
            for_this_launch().map(|f| f.id).unwrap_or("tos"),
        );
    }

    /// Nothing above the boundary names a feed: no identifier and no string
    /// in the app or the engine's core says Rithmic or thinkorswim. What a
    /// feed is, what it serves and how it signs in are the feed's to
    /// declare, in its folder under `providers`, and everything else reads
    /// that — which is what makes the next feed one entry in a list rather
    /// than a hunt through the UI.
    ///
    /// Read off the source up to each file's tests module, comments left
    /// out: a test may name the feed it exercises, and a comment may say
    /// which feed a rule was written for. When this fails, move what the
    /// line knows behind `providers::SignIn` or the provider, rather than
    /// adding the file to the list below.
    #[test]
    fn the_app_names_no_feed() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // Below the boundary: the feeds themselves. Their clients,
        // `crates/rithmic` and `crates/tos-market`, are not read at all.
        let below = ["crates/omacharts-engine/src/providers/"];
        // The brokers, which are named in their adapters and nowhere else.
        let adapters = ["src/trading/adapters/"];
        // Above it, for good, and why.
        let allowed = [
            // For good: a provider's own sign-in dialog is the provider's,
            // and the registry is the one place that maps a feed to it.
            ("src/ui/rithmic_login.rs", "Rithmic's own login dialog"),
            ("src/ui/sign_in_dialogs.rs", "which dialog each provider signs in through"),
            // History, for good: a migration says what was stored, under the
            // names it was stored with.
            ("src/migrations.rs", "keys as they were stored"),
        ];
        let mut found = Vec::new();
        for dir in ["src", "crates/omacharts-engine/src"] {
            for path in sources(&root.join(dir)) {
                let name = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                if below.iter().chain(&adapters).any(|prefix| name.starts_with(prefix))
                    || allowed.iter().any(|(file, _)| name == *file)
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for (at, line) in live_code(&text).lines().enumerate() {
                    if names_a_feed(line) {
                        found.push(format!("{name}:{}: {}", at + 1, line.trim()));
                    }
                }
            }
        }
        assert!(found.is_empty(), "a feed named above the boundary:\n{}", found.join("\n"));
    }

    fn sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else { return out };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(sources(&path));
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
        out.sort();
        out
    }

    /// The file without its tests modules, wherever they are in it, every
    /// line comment blanked and each line kept where it was, so that a line
    /// number is still the file's. A tests module is known by its
    /// `#[cfg(test)]` and ends at the brace that closes it: some files go on
    /// after theirs, and what follows is as live as what comes before.
    fn live_code(text: &str) -> String {
        let mut out = String::new();
        let mut lines = text.lines().peekable();
        // Inside a tests module, how deep in its braces.
        let mut tests: Option<i32> = None;
        while let Some(line) = lines.next() {
            if tests.is_none()
                && line.trim() == "#[cfg(test)]"
                && lines.peek().is_some_and(|next| next.contains("mod tests") && next.contains('{'))
            {
                tests = Some(0);
                out.push('\n');
                continue;
            }
            let mut code = String::new();
            let mut in_string = false;
            let mut escaped = false;
            let mut chars = line.chars().peekable();
            while let Some(c) = chars.next() {
                if !in_string && c == '/' && chars.peek() == Some(&'/') {
                    break;
                }
                // A character literal, '"' or '{' among them, is neither a
                // string nor a brace; a lifetime has no closing quote.
                if !in_string && c == '\'' {
                    let ahead: String = chars.clone().take(3).collect();
                    let literal = match ahead.as_bytes() {
                        [b'\\', _, b'\'', ..] => 3,
                        [_, b'\'', ..] => 2,
                        _ => 0,
                    };
                    code.push(c);
                    for _ in 0..literal {
                        code.extend(chars.next());
                    }
                    continue;
                }
                if c == '"' && !escaped {
                    in_string = !in_string;
                }
                if !in_string && let Some(depth) = tests.as_mut() {
                    match c {
                        '{' => *depth += 1,
                        '}' => *depth -= 1,
                        _ => {}
                    }
                }
                escaped = in_string && c == '\\' && !escaped;
                code.push(c);
            }
            match tests {
                Some(depth) => {
                    if depth <= 0 && code.contains('}') {
                        tests = None;
                    }
                }
                None => out.push_str(&code),
            }
            out.push('\n');
        }
        out
    }

    /// Does this line of code say a feed's name, in an identifier or a
    /// string? Not "logarithmic", and not "tos" inside another word.
    fn names_a_feed(line: &str) -> bool {
        let line = line.to_ascii_lowercase();
        let word = |name: &str| {
            line.match_indices(name).any(|(at, _)| {
                let apart = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphabetic());
                apart(line[..at].chars().next_back()) && apart(line[at + name.len()..].chars().next())
            })
        };
        // The Rithmic Test defaults by name, as well as anything of the
        // feed's: `rithmic::` and `providers::rithmic` are words already.
        line.contains("thinkorswim") || line.contains("test_system") || line.contains("test_gateway") || word("rithmic") || word("tos")
    }

    #[test]
    fn a_feed_is_told_by_its_name_and_nothing_else_is() {
        assert!(names_a_feed(r#"const RITHMIC: &str = "rithmic";"#));
        assert!(names_a_feed("use omacharts_engine::providers::rithmic::session;"));
        assert!(names_a_feed("rithmic::Trading::sign_in(app, &signing, changed)"));
        assert!(names_a_feed("providers::rithmic::session::serve_from_here();"));
        assert!(names_a_feed("known.map_or(trading::TEST_SYSTEM, |login| login.name())"));
        assert!(names_a_feed(r#"Ok(key) if key.starts_with("rithmic_") => {}"#));
        assert!(names_a_feed(r#""tos" => eprintln!("a browser"),"#));
        assert!(names_a_feed("tos_market::connected()"));
        assert!(names_a_feed(r#"Some("Sign in to thinkorswim")"#));
        assert!(!names_a_feed(r#"row.set_title("Logarithmic scale");"#));
        assert!(!names_a_feed("let photos = stops;"));
        assert_eq!(live_code("let a = 1; // rithmic\nlet b = \"//\"; // tos\n"), "let a = 1; \nlet b = \"//\"; \n");
        let after = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn b() { \"}\" }\n}\nfn c() {}\n";
        assert_eq!(live_code(after), "fn a() {}\n\n\n\n\nfn c() {}\n", "the code after a tests module is read");
    }
}
