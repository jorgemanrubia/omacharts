//! The data feeds, one folder each.
//!
//! Yahoo is the default, and the first entry in [`LISTED`] is what "default"
//! means — there is no second place that says so. A stored `provider` setting
//! picks another; the choice is read when the process starts.
//!
//! # Adding a feed
//!
//! One folder here, and three things in it: an implementation of
//! [`Provider`], a [`Listed`] describing how it is offered, and whatever that
//! feed alone needs — its symbology, its session handling, its instructions.
//! Then an arm in [`Feed`] and an entry in [`LISTED`], and the settings
//! panel, `provider list`, `config set provider` and the shell completions
//! all pick it up without being told about it separately.
//!
//! A heavy client with dependencies of its own is a workspace crate instead
//! (`crates/tos-market`), and the folder here is the thin layer that turns it
//! into a [`Provider`]. That keeps a vendored browser driver out of the
//! engine's own dependency list, and it keeps the awkward part of a feed —
//! the part with a protocol in it — somewhere it can be tested on its own.
//!
//! What a feed must never do is cost anything to the people not using it.
//! Everything in [`LISTED`] is read at startup, so it is plain data;
//! everything expensive — a connection, a thread, a runtime, a browser —
//! belongs to the first fetch, which is why [`selected`] can be called to
//! build any feed in the list and still touch nothing.

pub mod tos;
pub mod yahoo;

use crate::provider::Provider;

pub use tos::Tos;
pub use yahoo::Yahoo;

/// A feed as it is offered: what to call it, what it serves, and whether
/// picking it is the end of the matter or the start of signing in to
/// something.
///
/// Plain data, and deliberately so. It is read to draw a settings row, to
/// answer `provider list`, and to decide whether a name typed at
/// `config set provider` is one of ours — none of which may touch a network,
/// a session file or a browser.
#[derive(Debug)]
pub struct Listed {
    /// Stored in the `provider` setting, and the `adapter` key in cached
    /// symbol mappings. Never translated, never changed.
    pub id: &'static str,
    /// What to call it in front of somebody.
    pub label: &'static str,
    /// The line under the name: what it serves, and how fresh it is.
    pub summary: &'static str,
    /// Does choosing it leave something for the user to do? True for a feed
    /// that charts an account's own data, which somebody has to sign in to
    /// first — and which is therefore the feed the panel has more to say
    /// about, and the only kind `provider login` applies to.
    pub needs_sign_in: bool,
}

/// Every feed, in the order they are offered.
///
/// Order is the interface: the first is the default, the panel lists them
/// like this, and a feed added at the end cannot change what an existing
/// install is using.
pub const LISTED: &[Listed] = &[yahoo::LISTED, tos::LISTED];

/// What runs when nothing has been chosen.
pub const DEFAULT: &str = LISTED[0].id;

/// The feed with this id, if it is one of ours. Case-insensitive, because a
/// setting is typed by hand as often as it is clicked.
pub fn listed(id: &str) -> Option<&'static Listed> {
    let id = id.trim();
    LISTED.iter().find(|feed| feed.id.eq_ignore_ascii_case(id))
}

/// The feed a stored setting names, or the default.
///
/// Constructing one connects to nothing, opens nothing and starts no thread:
/// see the module docs. An unknown name falls back to the default rather than
/// failing, because a settings file is not a command line — refusing to start
/// over a word in a database would be a worse answer than charting from
/// Yahoo and saying so in the settings.
///
/// Behind the trait, not behind an enum of the feeds. An enum here was
/// ninety lines re-stating every method of [`Provider`] to forward it, which
/// is a second copy of the boundary that has to be edited in step with the
/// first — and a copy the next feed has to be added to as well, which is
/// exactly the kind of thing that gets forgotten. The trait is already the
/// one description of what a feed can do, and it is object-safe, so this is
/// the whole dispatch.
pub fn selected(stored: Option<&str>) -> Box<dyn Provider> {
    match stored.and_then(listed) {
        Some(feed) if feed.id == tos::LISTED.id => Box::new(Tos::new()),
        _ => Box::new(Yahoo::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The condition the whole feature is held to: a machine not using
    /// thinkorswim pays nothing for it. Choosing it builds a value with no
    /// fields in it — the runtime, the socket and the browser belong to the
    /// first fetch — so this can choose every feed in the list and then
    /// assert that nothing has been connected to.
    #[test]
    fn choosing_a_feed_connects_to_nothing() {
        for feed in LISTED {
            assert_eq!(selected(Some(feed.id)).id(), feed.id);
        }
        assert!(!tos_market::connected(), "a chosen feed must not have connected");
    }

    #[test]
    fn nothing_stored_and_nothing_recognised_are_both_the_default() {
        assert_eq!(DEFAULT, "yahoo");
        assert_eq!(selected(None).id(), DEFAULT);
        assert_eq!(selected(Some("")).id(), DEFAULT);
        assert_eq!(selected(Some("a feed we have never heard of")).id(), DEFAULT);
    }

    /// A setting is typed as often as it is clicked, and " TOS " out of a
    /// shell script is the same choice as "tos".
    #[test]
    fn a_name_is_matched_however_it_was_typed() {
        assert_eq!(selected(Some("  TOS ")).id(), "tos");
        assert_eq!(listed("Yahoo").map(|f| f.id), Some("yahoo"));
        assert!(listed("tosx").is_none());
    }

    /// Two feeds sharing an id would share a cache namespace, and the stored
    /// setting would stop naming one thing.
    #[test]
    fn every_feed_is_listed_once_and_matches_its_implementation() {
        let mut ids: Vec<&str> = LISTED.iter().map(|feed| feed.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "duplicate feed id");
        for feed in LISTED {
            let built = selected(Some(feed.id));
            assert_eq!(built.id(), feed.id);
            assert_eq!(built.label(), feed.label);
            assert!(!feed.summary.is_empty(), "{} has nothing to say", feed.id);
        }
    }
}
