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
    if order.iter().any(|id| providers::listed(id).is_some_and(|feed| feed.form().is_some())) {
        remember_logins(store);
    }
    providers::ordered(&order)
}

/// The logins typed into a feed's own form on the Data page, without their
/// secrets: a JSON object of feed id to its fields by key. `Local`, because
/// a login belongs to the machine it was typed on. A login that is a broker
/// connection too is Trading's, and not here.
pub const LOGINS: &str = "feed_logins";

type Typed = std::collections::BTreeMap<String, String>;

fn own_logins(store: &Store) -> std::collections::BTreeMap<String, Typed> {
    store.setting(LOGINS).and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
}

fn save_own_logins(store: &Store, logins: &std::collections::BTreeMap<String, Typed>) {
    match logins.is_empty() {
        true => store.remove_setting(LOGINS),
        false => store.set_setting(LOGINS, &serde_json::to_string(logins).unwrap_or_default()),
    }
}

/// Tell the engine which release of Omacharts is signing in: this crate's
/// version, the app's, which no library crate's is. Called once at
/// startup.
pub fn identify() {
    providers::identify(env!("CARGO_PKG_VERSION"));
}

/// Hand every feed that signs in through a form the login the app keeps for
/// it: a broker connection's data login, out of the logins in Preferences →
/// Trading, or what was typed into the feed's own form on the Data page.
/// Both are settings, `Local` in `setting_keys`; a secret is never here,
/// it is in the keyring under the login's id. The engine keeps no database,
/// so it is told, whenever the feeds are read and whenever a sign-in or a
/// sign-out may have changed what is saved.
///
/// A few settings reads, and only for an order that names such a feed or a
/// command about feeds: with nothing configured nothing is read, built or
/// started.
pub fn remember_logins(store: &Store) {
    let mut own = None;
    for feed in providers::every() {
        let Some(setup) = feed.setup.filter(|_| feed.form().is_some()) else { continue };
        let saved = match setup.trades() {
            true => saved_of(store),
            false => own_saved(own.get_or_insert_with(|| own_logins(store)), feed),
        };
        setup.remember(saved);
    }
}

/// The data login of the broker connections as the engine is told it: no
/// user while it is signed out, which is what makes the data feed not
/// signed in.
pub fn saved_of(store: &Store) -> providers::Saved {
    let login = crate::trading::data_login(store);
    providers::Saved {
        user: login.as_ref().filter(|login| !login.signed_out).map(|login| login.user.clone()),
        values: login
            .as_ref()
            .map(|login| vec![("system".into(), login.system.clone()), ("gateway".into(), login.gateway.clone())])
            .unwrap_or_default(),
        login: login.map(|login| login.id),
    }
}

/// A feed's own login, under the feed's id.
fn own_saved(logins: &std::collections::BTreeMap<String, Typed>, feed: &Listed) -> providers::Saved {
    let Some(typed) = logins.get(feed.id) else { return providers::Saved::default() };
    providers::Saved {
        login: Some(feed.id.to_string()),
        user: typed.get(providers::USER).filter(|user| !user.trim().is_empty()).cloned(),
        values: typed.iter().filter(|(key, _)| *key != providers::USER).map(|(k, v)| (k.clone(), v.clone())).collect(),
    }
}

/// Keep what was typed into `feed`'s own form: the secrets in the keyring
/// the feed names, under the feed's id, and the rest in [`LOGINS`]. Nothing
/// of a secret is kept while something else stands in for the form.
pub fn keep_login(store: &Store, feed: &'static Listed, typed: &[(&'static str, String)]) -> Result<(), String> {
    let (Some(setup), Some(fields)) = (feed.setup, feed.form()) else {
        return Err(format!("{} signs in without a form", feed.label));
    };
    let value = |key: &str| typed.iter().find(|(known, _)| *known == key).map(|(_, value)| value.trim().to_string());
    let user = value(providers::USER).unwrap_or_default();
    let mut kept = Typed::new();
    let mut secret = Ok(());
    for field in fields {
        let typed = value(field.key).unwrap_or_default();
        match field.kind {
            providers::FieldKind::Secret if typed.is_empty() || setup.stood_in() => {}
            providers::FieldKind::Secret => {
                secret = match setup.keyring() {
                    Some(keyring) => keyring.save(feed.id, &user, "", &typed),
                    None => Err(format!("{} has nowhere to keep a secret", feed.label)),
                };
            }
            _ => {
                kept.insert(field.key.to_string(), typed);
            }
        }
    }
    let mut logins = own_logins(store);
    logins.insert(feed.id.to_string(), kept);
    save_own_logins(store, &logins);
    remember_logins(store);
    secret
}

/// The settings a feed's sign-in is read from: the window charts again when
/// one changes, since which charts a login can serve may have too.
pub fn signs_in_from(key: &str) -> bool {
    [LOGINS, crate::trading::LOGINS, crate::trading::DATA_LOGIN].contains(&key)
}

/// What a feed keeps in the settings beyond its session, forgotten when it
/// is signed out of. A broker connection's data login is marked signed out:
/// its password went with the session, and it stays in Trading, with its
/// accounts, for the next sign-in. A feed's own login loses its user, and
/// keeps the rest for the next time it is typed.
pub fn forget_login(store: &Store, id: &str) {
    let Some(setup) = providers::listed(id).filter(|feed| feed.form().is_some()).and_then(|feed| feed.setup) else {
        return;
    };
    match setup.trades() {
        true => {
            if let Some(data) = crate::trading::data_login(store) {
                crate::trading::mark_signed_out(store, &data.id);
            }
        }
        false => {
            let mut logins = own_logins(store);
            let feed = providers::listed(id).map_or(id, |feed| feed.id);
            if let Some(typed) = logins.get_mut(feed) {
                typed.remove(providers::USER);
                save_own_logins(store, &logins);
            }
        }
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

/// Take `id` off the stored order. For a feed signed in to through a form
/// the session ends; the login stays, Trading's or its own, for the next
/// time. A browser's session is the feed's own, and is signed out of from
/// its row rather than by removing it.
pub fn remove_source(store: &Store, id: &str) -> bool {
    if !removable(store, id) {
        return false;
    }
    let order: Vec<String> =
        stored_order(store).into_iter().filter(|known| !known.eq_ignore_ascii_case(id)).collect();
    save_order(store, &order);
    if let Some(setup) = providers::listed(id).filter(|feed| feed.form().is_some()).and_then(|feed| feed.setup) {
        setup.hand_over();
        remember_logins(store);
    }
    true
}

/// Sign in to `id`, blocking until it is done: a browser the person drives,
/// or a check of what is saved followed by the window's own session signing
/// in, so that the answer is the provider's. For a thread of its own.
pub fn sign_in_now(id: &str, log: impl Fn(&str) + Send + Sync + 'static) -> Result<(), String> {
    providers::sign_in(id, log)?;
    providers::listed(id).and_then(|feed| feed.setup).map_or(Ok(()), |setup| setup.connect())
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
        use crate::trading::{self, tests::Kept, Typed};
        if omacharts_engine::providers::rithmic::session::development() {
            return;
        }
        let store = store();
        assert_eq!(saved_of(&store), Default::default());
        let kept = Kept::default();
        let typed = Typed {
            name: "Lucid".into(),
            user: " trader ".into(),
            password: "PLANTED-PASSWORD".into(),
            system: String::new(),
            gateway: "wss://example.invalid:443".into(),
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
}
