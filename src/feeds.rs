//! Which data feeds this process charts from, and in what order.
//!
//! Two ways to say it, and they answer different questions. The stored
//! `providers` setting is the default — the feeds this machine charts from,
//! top first, Yahoo among them wherever it was put, or not at all once it
//! was removed; nothing stored is Yahoo alone —
//! changed by the settings panel or by `config set provider`, which puts a
//! feed on top. `--provider` on the launch is this run only, for trying a
//! feed out, for a script that wants one particular feed, and for saying
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
    if order.iter().any(|id| providers::listed(id).is_some_and(|feed| trades(feed) || kept_here(feed))) {
        remember_logins(store);
    }
    providers::ordered(&order)
}

/// The setting what feeds are set up with lives in: a JSON object of what
/// each feed's dialog, or `provider setup`, handed over, by feed id. Only
/// for a feed that declares [`providers::Field`]s and is not a broker
/// connection, whose login is Trading's. Secrets are never here.
pub const SETUPS: &str = "feed_setups";

/// Does the app keep `feed`'s setup, in [`SETUPS`]?
pub fn kept_here(feed: &Listed) -> bool {
    feed.setup.is_some_and(|setup| !setup.trades() && !setup.fields().is_empty())
}

fn setups(store: &Store) -> std::collections::BTreeMap<String, providers::Saved> {
    store.setting(SETUPS).and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
}

/// What `id` was last set up with, or nothing.
pub fn setup_of(store: &Store, id: &str) -> providers::Saved {
    setups(store).remove(id).unwrap_or_default()
}

/// Keep `saved` as what `feed` is set up with, and tell the feed.
pub fn set_up(store: &Store, feed: &'static Listed, saved: providers::Saved) {
    let mut all = setups(store);
    all.insert(feed.id.to_string(), saved.clone());
    store.set_setting(SETUPS, &serde_json::to_string(&all).unwrap_or_default());
    if let Some(setup) = feed.setup {
        setup.remember(saved);
    }
}

/// Is a feed's standing worth a dot in the header? With Yahoo alone there
/// is nothing to choose and nothing to connect to, and a chart is as it
/// was before there were feeds. With more than one feed on `order`, or
/// one that signs in to something, there is.
pub fn worth_a_dot<'a>(order: impl Iterator<Item = &'a str>) -> bool {
    let mut feeds = 0;
    for id in order {
        feeds += 1;
        if feeds > 1 || providers::listed(id).is_some_and(|feed| feed.setup.is_some()) {
            return true;
        }
    }
    false
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
///
/// A feed whose setup the app keeps itself, in [`SETUPS`], is handed that
/// the same way, so that every feed learns what was saved for it from here.
pub fn remember_logins(store: &Store) {
    let data = crate::trading::data_login(store);
    let mut setups = None;
    for feed in providers::every() {
        let Some(setup) = feed.setup else { continue };
        if trades(feed) {
            setup.remember(saved_for(data.as_ref(), feed));
        } else if kept_here(feed) {
            let all = setups.get_or_insert_with(|| self::setups(store));
            setup.remember(all.get(feed.id).cloned().unwrap_or_default());
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
    [crate::trading::LOGINS, crate::trading::DATA_LOGIN, SETUPS].contains(&key)
}

/// What a feed keeps in the settings beyond its session, forgotten when it
/// is signed out of. For a broker connection's feed, the data login is
/// marked signed out: its password went with the session, and it stays in
/// Trading, with its accounts, for the next sign-in.
///
/// A feed whose setup the app keeps forgets it.
pub fn forget_login(store: &Store, id: &str) {
    if let Some(feed) = providers::listed(id).filter(|feed| kept_here(feed)) {
        let mut all = setups(store);
        if all.remove(feed.id).is_some() {
            match all.is_empty() {
                true => store.remove_setting(SETUPS),
                false => store.set_setting(SETUPS, &serde_json::to_string(&all).unwrap_or_default()),
            }
        }
        if let Some(setup) = feed.setup {
            setup.remember(providers::Saved::default());
        }
        return;
    }
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
pub fn add_feed(store: &Store, id: &str) {
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
pub fn remove_feed(store: &Store, id: &str) -> bool {
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

/// Ask the feed that serves `root` which contracts of it are listed, on a
/// thread, and hand what it says to [`omacharts_engine::contracts`]; then
/// `done` on the main loop, whether or not it said anything. A feed that
/// lists none costs nothing: the rule's own listing stands, and `done` is
/// called all the same.
pub fn list_contracts(order: Vec<String>, root: &omacharts_engine::Instrument, done: impl Fn() + 'static) {
    let (tell, heard) = async_channel::bounded::<()>(1);
    let root = root.as_root();
    std::thread::Builder::new()
        .name("omacharts contracts".into())
        .spawn(move || {
            let feeds = Feeds::of(providers::ordered(&order));
            if let Some((provider, _)) = feeds.route(&root)
                && let Ok(listed) = provider.contracts(root.root())
            {
                omacharts_engine::contracts::supply(root.root(), listed);
            }
            let _ = tell.send_blocking(());
        })
        .ok();
    gtk::glib::MainContext::default().spawn_local(async move {
        if heard.recv().await.is_ok() {
            done();
        }
    });
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

    /// Yahoo alone is a chart as it was before there were feeds: no dot.
    /// Any second feed, or any feed that signs in, is a dot.
    #[test]
    fn a_dot_is_worth_drawing_only_past_yahoo_alone() {
        assert!(!worth_a_dot(std::iter::empty()));
        assert!(!worth_a_dot(["yahoo"].into_iter()));
        assert!(worth_a_dot(["yahoo", "yahoo"].into_iter()));
        assert!(worth_a_dot(["rithmic"].into_iter()));
        assert!(worth_a_dot(["tos", "yahoo"].into_iter()));
        assert!(!worth_a_dot(["unknown"].into_iter()), "a feed this build does not know connects to nothing");
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
        assert!(remove_feed(&store, "rithmic"));
        assert_eq!(store.setting(SETTING), None);
        assert_eq!(trading::logins(&store).len(), 1);
    }

    /// What a feed is set up with is kept by the app, handed to the feed
    /// whenever the settings are read, and forgotten with it.
    #[test]
    fn a_feed_set_up_here_is_kept_handed_back_and_forgotten() {
        use omacharts_engine::providers::{fake, Saved};
        let store = store();
        let feed = providers::listed(fake::LISTED.id).expect("in a test build");
        assert!(kept_here(feed));
        assert!(!kept_here(providers::listed("rithmic").unwrap()), "a broker's login is Trading's");
        assert!(!kept_here(providers::listed("yahoo").unwrap()));
        set_up(&store, feed, Saved { user: Some("member".into()), ..Saved::default() }.with("desk", "North"));
        assert_eq!(setup_of(&store, feed.id).value("desk"), Some("North"));
        assert!(store.setting(SETUPS).is_some_and(|json| !json.contains("PLANTED")));
        assert!(signs_in_from(SETUPS));
        forget_login(&store, feed.id);
        assert_eq!(store.setting(SETUPS), None);
        assert_eq!(setup_of(&store, feed.id), Saved::default());
    }

    /// A feed added goes after the others, above Yahoo, and only once.
    #[test]
    fn an_added_feed_goes_last_above_yahoo() {
        let store = store();
        add_feed(&store, "rithmic");
        add_feed(&store, "tos");
        add_feed(&store, "TOS");
        assert_eq!(stored_order(&store), vec!["rithmic", "tos", "yahoo"]);
        assert!(remove_feed(&store, "tos"));
        assert_eq!(stored_order(&store), vec!["rithmic", "yahoo"]);
    }

    /// Yahoo is a feed like the others: it can be removed, added back, at
    /// the bottom, and moved above another. The last feed cannot go.
    #[test]
    fn yahoo_can_be_removed_added_back_and_moved_but_the_last_feed_stays() {
        let store = store();
        assert!(!removable(&store, "yahoo"), "Yahoo alone is the last feed");
        assert!(!remove_feed(&store, "yahoo"));
        add_feed(&store, "rithmic");
        assert!(remove_feed(&store, "yahoo"));
        assert_eq!(store.setting(SETTING).as_deref(), Some(r#"["rithmic"]"#));
        assert!(!remove_feed(&store, "rithmic"), "the last feed");
        assert_eq!(stored_order(&store), vec!["rithmic"]);
        add_feed(&store, "yahoo");
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
            ("src/ui/rithmic_attribution.rs", "Rithmic's own artwork and notices, which its terms ask for"),
            ("src/ui/sierra_setup.rs", "Sierra Chart's own setup dialog"),
            ("src/ui/sierra_login.rs", "Sierra Chart's own trading login dialog"),
            // The flag table names what each flag switches on.
            ("src/features.rs", "the flag a feed is behind"),
            // A hidden verb's example has to name the one feed it is for.
            ("src/cli/spec.rs", "the example of `provider setup`, which only a flagged feed has a use for"),
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
        line.contains("thinkorswim") || line.contains("test_system") || line.contains("test_gateway") || word("rithmic") || word("tos") || word("sierra")
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
