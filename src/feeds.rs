//! Which data feeds this process charts from, and in what order.
//!
//! Two ways to say it, and they answer different questions. The stored
//! `providers` setting is the default — the feeds this machine charts from,
//! top first, with Yahoo implied at the bottom and never written down —
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
/// first. Yahoo is never in it.
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
/// Nothing stored, or something that is not a list, is no order at all.
pub fn stored_order(store: &Store) -> Vec<String> {
    store
        .setting(SETTING)
        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .unwrap_or_default()
}

/// Write `order` down, without the default, which is implied. An empty
/// order is no row at all, which is what a machine that never chose has.
pub fn save_order(store: &Store, order: &[String]) {
    let kept: Vec<&str> = order
        .iter()
        .map(|id| providers::listed(id).map_or(id.as_str(), |feed| feed.id))
        .filter(|id| *id != providers::DEFAULT)
        .fold(Vec::new(), |mut kept, id| {
            if !kept.contains(&id) {
                kept.push(id);
            }
            kept
        });
    match kept.is_empty() {
        true => store.remove_setting(SETTING),
        false => store.set_setting(SETTING, &serde_json::to_string(&kept).unwrap_or_default()),
    }
}

/// Put `feed` on top of the stored order, which is what choosing one feed
/// means now that there are several. The default on top is the default
/// alone: it serves everything, so nothing under it would be reached.
pub fn put_on_top(store: &Store, feed: &'static Listed) {
    if feed.id == providers::DEFAULT {
        save_order(store, &[]);
        return;
    }
    let mut order = vec![feed.id.to_string()];
    order.extend(stored_order(store).into_iter().filter(|id| !id.eq_ignore_ascii_case(feed.id)));
    save_order(store, &order);
}

/// The order this process charts from: the launch flag's feed alone above
/// the default, otherwise what is stored. Without the default, which
/// [`providers::ordered`] adds.
pub fn order(store: &Store) -> Vec<String> {
    match for_this_launch() {
        Some(feed) if feed.id == providers::DEFAULT => Vec::new(),
        Some(feed) => vec![feed.id.to_string()],
        None => stored_order(store),
    }
}

/// The feeds themselves, top first, ending with the default, each ready to
/// be asked for bars. Cheap: building them connects to nothing. See
/// `omacharts_engine::providers`.
pub fn ordered(store: &Store) -> Vec<Box<dyn Provider>> {
    let order = order(store);
    if order.iter().any(|id| id.eq_ignore_ascii_case(RITHMIC)) {
        remember_logins(store);
    }
    providers::ordered(&order)
}

const RITHMIC: &str = "rithmic";

/// Tell the engine which release of Omacharts signs in to Rithmic: this
/// crate's version, the app's, which no library crate's is. Called once at
/// startup.
pub fn identify() {
    omacharts_engine::providers::rithmic::session::identify(env!("CARGO_PKG_VERSION"));
}

/// Hand the engine the Rithmic login this machine has saved: the user ID,
/// the system and the gateway, which are settings, `Local` in
/// `setting_keys`. The password is never here; it is in the keyring. The
/// engine keeps no database, so it is told, whenever the feeds are read and
/// whenever a sign-in or a sign-out may have changed what is saved.
///
/// Three settings reads, and only for an order that names Rithmic or a
/// command about feeds: with nothing configured nothing of Rithmic's is
/// read, built or started.
pub fn remember_logins(store: &Store) {
    use omacharts_engine::providers::rithmic::session;
    session::remember(session::Saved {
        user: store.setting(session::SETTING_USER),
        system: store.setting(session::SETTING_SYSTEM),
        gateway: store.setting(session::SETTING_GATEWAY),
    });
}

/// What a feed keeps in the settings beyond its session, forgotten when it
/// is signed out of. For Rithmic, the user ID: signing out is the login
/// going, and the system and gateway it was for are kept for next time.
pub fn forget_login(store: &Store, id: &str) {
    if id.eq_ignore_ascii_case(RITHMIC) {
        store.remove_setting(omacharts_engine::providers::rithmic::session::SETTING_USER);
        remember_logins(store);
    }
}

/// Add `id` to the stored order, at the bottom, just above the default:
/// a source somebody has just added is tried after the ones they already
/// put in order. Adding one already there changes nothing.
pub fn add_source(store: &Store, id: &str) {
    let mut order = stored_order(store);
    if !order.iter().any(|known| known.eq_ignore_ascii_case(id)) {
        order.push(id.to_string());
        save_order(store, &order);
    }
}

/// Take `id` off the stored order and forget what it keeps here. For
/// Rithmic that is the password, out of `secrets`, and the user ID; the
/// system and gateway are kept for the next time it is added.
/// thinkorswim's browser session is its own, and is signed out of from its
/// row rather than by removing it.
pub fn remove_source(
    store: &Store,
    secrets: &dyn omacharts_engine::providers::rithmic::session::Secrets,
    id: &str,
) -> Result<(), String> {
    let order: Vec<String> =
        stored_order(store).into_iter().filter(|known| !known.eq_ignore_ascii_case(id)).collect();
    save_order(store, &order);
    if !id.eq_ignore_ascii_case(RITHMIC) {
        return Ok(());
    }
    remember_logins(store);
    let forgotten = omacharts_engine::providers::rithmic::session::sign_out_of(secrets);
    forget_login(store, RITHMIC);
    forgotten
}

/// A Rithmic login as it is typed into the settings.
///
/// No `Debug`, so that it cannot be printed by accident: the password is
/// in it.
pub struct RithmicLogin {
    pub user: String,
    pub password: String,
    pub system: String,
    pub gateway: String,
}

/// Save a typed login: the user ID, the system and the gateway as settings,
/// and the password into `secrets` through the provider's own code, which
/// writes nothing while a development login is in charge. An empty password
/// keeps whatever the keyring already holds.
///
/// A system or gateway left empty is the Test one, and is not stored.
pub fn save_rithmic_login(
    store: &Store,
    secrets: &dyn omacharts_engine::providers::rithmic::session::Secrets,
    login: &RithmicLogin,
) -> Result<(), String> {
    use omacharts_engine::providers::rithmic::session;

    let user = login.user.trim();
    let system = login.system.trim();
    let gateway = login.gateway.trim();
    let set = |key: &str, value: &str| match value.is_empty() {
        true => store.remove_setting(key),
        false => store.set_setting(key, value),
    };
    set(session::SETTING_USER, user);
    set(session::SETTING_SYSTEM, system);
    set(session::SETTING_GATEWAY, gateway);
    remember_logins(store);
    if login.password.is_empty() || user.is_empty() {
        return Ok(());
    }
    let system = match system.is_empty() {
        true => rithmic_test_system(),
        false => system,
    };
    session::save_password(secrets, user, system, &login.password)
}

/// The system a login with none named is for.
pub fn rithmic_test_system() -> &'static str {
    "Rithmic Test"
}

/// The gateway a login with none named signs in at.
pub fn rithmic_test_gateway() -> &'static str {
    "wss://rituz00100.rithmic.com:443"
}

/// Sign in to `id`, blocking until it is done: thinkorswim's browser, or
/// Rithmic's check of what is saved followed by the window's own session
/// signing in, so that the answer is Rithmic's. For a thread of its own.
pub fn sign_in_now(id: &str, log: impl Fn(&str) + Send + Sync + 'static) -> Result<(), String> {
    providers::sign_in(id, log)?;
    match id.eq_ignore_ascii_case(RITHMIC) {
        true => omacharts_engine::providers::rithmic::session::connect(),
        false => Ok(()),
    }
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
        store.set_setting(SETTING, r#"["tos"]"#);
        assert_eq!(stored(&store).id, "tos");
        let ids: Vec<&str> = providers::ordered(&stored_order(&store)).iter().map(|p| p.id()).collect();
        assert_eq!(ids, vec!["tos", "yahoo"]);
    }

    /// Choosing one feed puts it on top and keeps the rest of the order;
    /// choosing the default is the default alone.
    #[test]
    fn a_feed_put_on_top_goes_first() {
        let store = store();
        let tos = providers::listed("tos").unwrap();
        let yahoo = providers::listed("yahoo").unwrap();
        put_on_top(&store, tos);
        assert_eq!(store.setting(SETTING).as_deref(), Some(r#"["tos"]"#));
        store.set_setting(SETTING, r#"["later","tos"]"#);
        put_on_top(&store, tos);
        assert_eq!(stored_order(&store), vec!["tos", "later"]);
        put_on_top(&store, yahoo);
        assert_eq!(store.setting(SETTING), None);
    }

    /// A password store that is a list in memory, so that nothing a test
    /// does reaches a keyring.
    #[derive(Default)]
    pub(crate) struct Kept(pub std::cell::RefCell<Vec<(String, String, String)>>);

    impl omacharts_engine::providers::rithmic::session::Secrets for Kept {
        fn save(&self, user: &str, system: &str, password: &str) -> Result<(), String> {
            let mut kept = self.0.borrow_mut();
            kept.retain(|(u, s, _)| !(u == user && s == system));
            kept.push((user.into(), system.into(), password.into()));
            Ok(())
        }
        fn forget(&self, user: &str, system: &str) -> Result<(), String> {
            self.0.borrow_mut().retain(|(u, s, _)| !(u == user && s == system));
            Ok(())
        }
    }

    /// The user ID, system and gateway become settings; the password goes
    /// to the secret store and is in no setting. Removing the source takes
    /// both the user ID and the password away again, and the order with
    /// them.
    #[test]
    fn a_rithmic_login_is_settings_and_a_secret_and_removing_it_forgets_both() {
        use omacharts_engine::providers::rithmic::session;
        if session::development() {
            // The development file wins and the store is never written.
            return;
        }
        let store = store();
        let kept = Kept::default();
        let login = RithmicLogin {
            user: " trader ".into(),
            password: "PLANTED-PASSWORD".into(),
            system: String::new(),
            gateway: "wss://example.invalid:443".into(),
        };
        save_rithmic_login(&store, &kept, &login).unwrap();
        add_source(&store, "rithmic");
        assert_eq!(store.setting(session::SETTING_USER).as_deref(), Some("trader"));
        assert_eq!(store.setting(session::SETTING_SYSTEM), None, "Test is the default");
        assert_eq!(store.setting(session::SETTING_GATEWAY).as_deref(), Some("wss://example.invalid:443"));
        assert_eq!(
            kept.0.borrow().as_slice(),
            &[("trader".to_string(), "Rithmic Test".to_string(), "PLANTED-PASSWORD".to_string())]
        );
        for key in [SETTING, session::SETTING_USER, session::SETTING_SYSTEM, session::SETTING_GATEWAY] {
            assert!(!store.setting(key).unwrap_or_default().contains("PLANTED"), "{key}");
        }
        assert_eq!(stored_order(&store), vec!["rithmic"]);

        remove_source(&store, &kept, "rithmic").unwrap();
        assert!(kept.0.borrow().is_empty(), "the password is still kept");
        assert_eq!(store.setting(session::SETTING_USER), None);
        assert_eq!(store.setting(SETTING), None);
        assert_eq!(
            store.setting(session::SETTING_GATEWAY).as_deref(),
            Some("wss://example.invalid:443"),
            "the gateway is kept for next time"
        );
    }

    /// A source added goes at the bottom, above Yahoo, and only once.
    #[test]
    fn an_added_source_goes_last_above_yahoo() {
        let store = store();
        add_source(&store, "rithmic");
        add_source(&store, "tos");
        add_source(&store, "TOS");
        assert_eq!(stored_order(&store), vec!["rithmic", "tos"]);
        remove_source(&store, &Kept::default(), "tos").unwrap();
        assert_eq!(stored_order(&store), vec!["rithmic"]);
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
}
