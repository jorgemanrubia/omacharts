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

pub mod synthetic;
pub mod tos;
pub mod yahoo;

use crate::provider::{Delivery, Provider};
use crate::symbols::InstrumentKind;

pub use synthetic::Synthetic;
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
    /// The line under the name: what it serves. How *fresh* it is does not
    /// belong here — see [`freshness`], which asks the provider instead.
    pub serves: &'static str,
    /// What signing in to it involves, for a feed that charts an account's
    /// own data. `None` for a feed anybody can use the moment they pick it,
    /// which is both the better kind and, so far, the default one.
    ///
    /// Its presence is also the answer to "does this feed need signing in
    /// to" — one fact in one place, rather than a boolean beside a block of
    /// text that can come to disagree with it.
    pub setup: Option<&'static Setup>,
}

impl Listed {
    /// Is there anything to do after picking it?
    pub fn needs_sign_in(&self) -> bool {
        self.setup.is_some()
    }
}

/// What a feed that needs an account tells a settings panel about itself.
///
/// Prose, written in the feed's own folder, rendered by whoever is asking.
/// The panel knows how to lay out three fields; it knows nothing about
/// brokerages, browsers or gateways, which is what stops the next feed's
/// instructions from landing in a UI file.
#[derive(Debug)]
pub struct Setup {
    /// What happens when you sign in, and what the app does and does not
    /// see while you do.
    pub explain: &'static str,
    /// What has to be true of the machine and the account first. Shown as a
    /// list, because a prerequisite buried in a paragraph is a prerequisite
    /// somebody discovers by failing.
    pub requires: &'static [&'static str],
    /// What the feed may do once it is signed in, and what happens when the
    /// session ends. The honest small print.
    pub scope: &'static str,
}

/// What is saved for a feed that needs signing in to.
///
/// Four states, because there are four different things to do about them,
/// and a panel that collapses any two of them cannot tell somebody why
/// their charts are empty. Read from disk: asking this must never cost a
/// network round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    /// Nothing saved. Signing in is the next step.
    Missing,
    /// Signed in, with whatever is worth saying about it.
    Signed(String),
    /// Signed in once, and the provider has since refused it. Only signing
    /// in again fixes this, which is why it is not merely "missing".
    Expired(String),
    /// Something is saved and this feed will not use it. Not a failure to
    /// sign in — a sign-in that worked and landed somewhere else.
    Refused(String),
}

impl Access {
    /// The line a settings row shows.
    pub fn line(&self) -> String {
        match self {
            Access::Missing => "Not signed in".into(),
            Access::Signed(detail) => format!("Signed in · {detail}"),
            Access::Expired(detail) => format!("Session expired · {detail}"),
            Access::Refused(detail) => format!("Unusable session · {detail}"),
        }
    }

    /// Can bars be fetched with what is saved?
    pub fn ready(&self) -> bool {
        matches!(self, Access::Signed(_))
    }
}

/// What is saved for `id`, or `None` for a feed that needs nothing saved.
pub fn access(id: &str) -> Option<Access> {
    match listed(id)?.id {
        "tos" => Some(tos::session::access()),
        _ => None,
    }
}

/// The browser a sign-in would use, or why it cannot start at all.
pub fn can_sign_in(id: &str) -> Result<String, String> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::can_sign_in(),
        Some(other) => Err(format!("{other} needs no signing in")),
        None => Err(format!("no such data feed: {id}")),
    }
}

/// Signs in to `id`, blocking until the person is done. `log` is handed one
/// progress line at a time.
pub fn sign_in(id: &str, log: impl Fn(&str) + Send + Sync + 'static) -> Result<(), String> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::sign_in(log),
        Some(other) => Err(format!("{other} needs no signing in")),
        None => Err(format!("no such data feed: {id}")),
    }
}

/// Forgets what is saved for `id`.
pub fn sign_out(id: &str) -> Result<(), String> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::sign_out(),
        Some(other) => Err(format!("{other} has nothing saved to forget")),
        None => Err(format!("no such data feed: {id}")),
    }
}

/// Has this feed connected to anything in this process?
///
/// For the tests that hold the feature to its one condition: opening the
/// settings, reading a session state, or merely choosing a feed must not
/// build a connection. Only fetching bars may.
pub fn connected(id: &str) -> bool {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::connected(),
        _ => false,
    }
}

/// The files a feed keeps on this machine, for showing rather than
/// guessing: a signed-in brokerage session is something somebody is
/// entitled to know the location of, and to delete.
pub fn places(id: &str) -> Vec<(&'static str, String)> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::places(),
        _ => Vec::new(),
    }
}

/// Every feed, in the order they are offered.
///
/// Order is the interface: the first is the default, the panel lists them
/// like this, and a feed added at the end cannot change what an existing
/// install is using.
pub const LISTED: &[Listed] = &[yahoo::LISTED, tos::LISTED];

/// What runs when nothing has been chosen.
pub const DEFAULT: &str = LISTED[0].id;

/// How fresh a chart from this feed is, in one phrase.
///
/// Asked of the provider rather than written down beside it. A feed's
/// freshness is a fact about how it delivers bars and how far behind the
/// vendor keeps them — both of which the provider already states — and a
/// sentence kept beside those is a sentence that goes on claiming whatever
/// it claimed when somebody last edited it. thinkorswim said "real time"
/// while its charts were snapshots refetched on the same timer as Yahoo's,
/// which is how this rule got written down.
///
/// It is also what makes the label correct the day a feed starts streaming
/// for real: the provider reports [`Delivery::Streamed`], and this changes
/// with it rather than waiting to be noticed.
pub fn freshness(id: &str) -> String {
    let provider = selected(Some(id));
    if provider.delivery() == Delivery::Streamed {
        return "live, as each print arrives".into();
    }
    match delays(provider.as_ref()) {
        // Nothing the vendor holds back, which is not the same as live: a
        // polled chart is a photograph, and the one on screen is as old as
        // the last time something fetched it.
        None => "refetched on a timer, not a live stream".into(),
        Some(delay) => format!("delayed {delay}"),
    }
}

/// "15 min for indexes, 10 for futures", or `None` where the provider holds
/// nothing back. Grouped by how long, longest first, so the worst case is
/// the first thing read.
fn delays(provider: &dyn Provider) -> Option<String> {
    const KINDS: [InstrumentKind; 6] = [
        InstrumentKind::Index,
        InstrumentKind::FutureRoot,
        InstrumentKind::Equity,
        InstrumentKind::Etf,
        InstrumentKind::Fx,
        InstrumentKind::Crypto,
    ];

    let mut by_delay: Vec<(u32, Vec<&'static str>)> = Vec::new();
    for kind in KINDS {
        let minutes = provider.delay_minutes(kind);
        if minutes == 0 {
            continue;
        }
        match by_delay.iter_mut().find(|(d, _)| *d == minutes) {
            Some((_, kinds)) => kinds.push(plural(kind)),
            None => by_delay.push((minutes, vec![plural(kind)])),
        }
    }
    by_delay.sort_by_key(|(minutes, _)| std::cmp::Reverse(*minutes));

    // Only the first carries the unit: "15 min for indexes, 10 for futures".
    let parts: Vec<String> = by_delay
        .iter()
        .enumerate()
        .map(|(i, (minutes, kinds))| {
            let unit = if i == 0 { " min" } else { "" };
            format!("{minutes}{unit} for {}", kinds.join(" and "))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// A kind as it is said in a sentence about several of them.
fn plural(kind: InstrumentKind) -> &'static str {
    match kind {
        InstrumentKind::Index => "indexes",
        InstrumentKind::FutureRoot => "futures",
        InstrumentKind::Equity => "stocks",
        InstrumentKind::Etf => "ETFs",
        InstrumentKind::Fx => "currencies",
        InstrumentKind::Crypto => "crypto",
    }
}

/// What a feed is offered as, in full: what it serves and how fresh it is.
pub fn described(feed: &Listed) -> String {
    format!("{} · {}", feed.serves, freshness(feed.id))
}

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

    /// The label a feed is offered with has to come from what the provider
    /// does, or it goes on saying whatever it said when it was written.
    /// thinkorswim claimed "real time" while it was being refetched on the
    /// same timer as Yahoo.
    #[test]
    fn how_fresh_a_feed_is_comes_from_the_provider() {
        // Yahoo holds some kinds back, and says so, in its own numbers.
        let yahoo = freshness("yahoo");
        assert_eq!(yahoo, "delayed 15 min for indexes, 10 for futures", "{yahoo}");

        // thinkorswim holds nothing back and does not stream either, so it
        // must not claim to be live.
        let tos = freshness("tos");
        assert!(tos.contains("not a live stream"), "{tos}");
        assert!(!tos.contains("real time"), "{tos}");
        assert!(!tos.contains("delayed"), "{tos}");

        for feed in LISTED {
            assert!(!described(feed).is_empty());
        }
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
            assert!(!feed.serves.is_empty(), "{} says nothing about what it serves", feed.id);
        }
    }
}
