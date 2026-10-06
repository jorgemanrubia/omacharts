//! Which data feed this process charts from.
//!
//! Two ways to say it, and they answer different questions. The stored
//! `provider` setting is the default — what this machine charts from until
//! somebody changes their mind, written by the settings panel or by
//! `config set provider`. `--provider` on the launch is this run only, for
//! trying a feed out, for a script that wants one particular source, and for
//! saying which feed a bug report is about.
//!
//! The flag wins, and it only ever applies to the process it was typed at.
//! That is a consequence of the choice being read once, at startup: the
//! loader is built around one feed, with one request queue paced to that
//! feed's rules, and a cache keyed by it. Nothing here pretends otherwise —
//! an invocation that asks a *running* window for a different feed is told
//! that is not what the flag does, rather than being quietly ignored.
//!
//! There is deliberately no environment variable. `OMACHARTS_PROVIDER` used
//! to do this job, and an environment variable is the one way of passing an
//! argument that appears in no help output, no completion and no surface
//! description — so an agent cannot discover it, and nobody can be told
//! about it except in prose. A declared flag is the same capability, written
//! down once in `cli::spec`.

use std::sync::OnceLock;

use omacharts_engine::providers::{self, Feed, Listed};

use crate::store::Store;

/// The setting the stored default lives in.
pub const SETTING: &str = "provider";

static FOR_THIS_LAUNCH: OnceLock<&'static Listed> = OnceLock::new();

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

/// How this feed is offered — its name, its summary, whether it needs
/// signing in to. The flag for this launch, otherwise the stored setting,
/// otherwise the default.
pub fn in_use(store: &Store) -> &'static Listed {
    for_this_launch()
        .or_else(|| store.setting(SETTING).as_deref().and_then(providers::listed))
        .unwrap_or_else(|| providers::listed(providers::DEFAULT).expect("the default is listed"))
}

/// The feed itself, ready to be asked for bars.
///
/// Cheap, and called from several places for that reason: building one
/// connects to nothing. See `omacharts_engine::providers`.
pub fn selected(store: &Store) -> Feed {
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
    use omacharts_engine::Provider;

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
