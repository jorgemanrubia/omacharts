//! Which data feed this process charts from.
//!
//! Two ways to say it, and they answer different questions. The stored
//! `provider` setting is the default — what this machine charts from until
//! somebody changes their mind, written by the settings panel or by
//! `config set provider`. `--provider` on the launch is this run only, for
//! trying a feed out, for a script that wants one particular source, and for
//! saying which feed a bug report is about.
//!
//! The flag wins for the launch it was typed at, and a flag typed at a
//! window already open switches that window without storing anything. A
//! window switches feeds live — see `Window::switch_feed` — so what is
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

/// The setting the stored default lives in.
pub const SETTING: &str = "provider";

static FOR_THIS_LAUNCH: OnceLock<&'static Listed> = OnceLock::new();

/// The feed the window is charting from right now, set each time it starts
/// on one or switches to one. What `provider list` and `provider status`
/// mean by "in use" once a window is open; absent, the launch flag and then
/// the stored setting say what a window would start on.
static RUNNING: std::sync::RwLock<Option<&'static Listed>> = std::sync::RwLock::new(None);

/// The window is charting from `id` from now on. A feed that is not listed
/// — the synthetic one, in a benchmark — is nobody's business here.
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

/// How this feed is offered — its name, what it serves, whether it needs
/// signing in to. The one a window is charting from, otherwise the flag for
/// this launch, otherwise the stored setting, otherwise the default.
pub fn in_use(store: &Store) -> &'static Listed {
    (*RUNNING.read().unwrap_or_else(|e| e.into_inner()))
        .or_else(for_this_launch)
        .or_else(|| store.setting(SETTING).as_deref().and_then(providers::listed))
        .unwrap_or_else(|| providers::listed(providers::DEFAULT).expect("the default is listed"))
}

/// The feed itself, ready to be asked for bars.
///
/// Cheap, and called from several places for that reason: building one
/// connects to nothing. See `omacharts_engine::providers`.
pub fn selected(store: &Store) -> Box<dyn Provider> {
    providers::selected(Some(in_use(store).id))
}

/// What the stored default is, as a feed in the catalogue — ignoring any
/// flag this launch was given.
///
/// For the settings panel, which has to show what is *stored* rather than
/// what is running, or a launch flag would look like a changed setting and
/// get changed back by somebody tidying up.
pub fn stored(store: &Store) -> &'static Listed {
    store
        .setting(SETTING)
        .as_deref()
        .and_then(providers::listed)
        .unwrap_or_else(|| providers::listed(providers::DEFAULT).expect("the default is listed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::memory().expect("an empty database")
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
        store.set_setting(SETTING, "tos");
        assert_eq!(in_use(&store).id, "tos");
        assert_eq!(selected(&store).id(), "tos");
    }

    /// A setting written by a future version, or by hand. Charting from
    /// Yahoo and saying so beats refusing to start.
    #[test]
    fn a_stored_feed_nobody_has_heard_of_is_the_default() {
        let store = store();
        store.set_setting(SETTING, "bloomberg");
        assert_eq!(in_use(&store).id, providers::DEFAULT);
    }

    /// The flag is a `OnceLock`, so it cannot be exercised twice in one test
    /// binary. This asserts the rule that does not need setting it: with no
    /// flag given, the stored setting decides.
    #[test]
    fn with_no_flag_the_stored_setting_decides() {
        let store = store();
        store.set_setting(SETTING, "tos");
        assert_eq!(
            in_use(&store).id,
            for_this_launch().map(|f| f.id).unwrap_or("tos"),
        );
    }
}
