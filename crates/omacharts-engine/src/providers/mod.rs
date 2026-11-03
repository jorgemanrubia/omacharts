//! The data feeds, one folder each.
//!
//! Yahoo is the default, and the first entry in [`LISTED`] is what "default"
//! means — there is no second place that says so. A stored `providers`
//! setting puts others above it.
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
//! belongs to the first fetch, which is why [`built`] can be called to
//! build any feed in the list and still touch nothing.
//!
//! Feeds are an ordered list, with Yahoo always last: each chart is served
//! by the first feed that can be used and has a name for its instrument —
//! see [`ordered`] and [`route`].

pub mod rithmic;
pub mod synthetic;
pub mod tos;
pub mod yahoo;

use crate::provider::{Delivery, Provider};
use crate::symbols::{Instrument, InstrumentKind};

pub use rithmic::Rithmic;
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
    /// The same, in the few words a row in a list of sources has room for:
    /// the kinds of instrument, as somebody choosing between sources thinks
    /// of them.
    pub covers: &'static str,
    /// What signing in to it involves, for a feed that charts an account's
    /// own data. `None` for a feed anybody can use the moment they pick it,
    /// which is both the better kind and, so far, the default one.
    ///
    /// Its presence is also the answer to "does this feed need signing in
    /// to" — one fact in one place, rather than a boolean beside a block of
    /// text that can come to disagree with it.
    pub setup: Option<&'static Setup>,
    /// Does this feed rest on something that can be taken away?
    ///
    /// True for a feed that talks to an undocumented gateway through a
    /// captured web session: it works until the vendor changes their web
    /// client, and then it stops, through no fault of anybody here.
    /// Somebody choosing it is owed that word before they do.
    pub experimental: bool,
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
}

impl Access {
    /// The line a settings row shows.
    pub fn line(&self) -> String {
        match self {
            Access::Missing => "Not signed in".into(),
            Access::Signed(detail) => format!("Signed in · {detail}"),
            Access::Expired(detail) => format!("Session expired · {detail}"),
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
        "rithmic" => Some(rithmic::session::access()),
        _ => None,
    }
}

/// The browser a sign-in would use, or why it cannot start at all.
pub fn can_sign_in(id: &str) -> Result<String, String> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::can_sign_in(),
        Some("rithmic") => rithmic::session::can_sign_in(),
        Some(other) => Err(format!("{other} needs no signing in")),
        None => Err(format!("no such data feed: {id}")),
    }
}

/// Signs in to `id`, blocking until the person is done. `log` is handed one
/// progress line at a time.
pub fn sign_in(id: &str, log: impl Fn(&str) + Send + Sync + 'static) -> Result<(), String> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::sign_in(log),
        Some("rithmic") => rithmic::session::sign_in(log),
        Some(other) => Err(format!("{other} needs no signing in")),
        None => Err(format!("no such data feed: {id}")),
    }
}

/// Forgets what is saved for `id`.
pub fn sign_out(id: &str) -> Result<(), String> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::sign_out(),
        Some("rithmic") => rithmic::session::sign_out(),
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
        Some("rithmic") => rithmic::session::connected(),
        _ => false,
    }
}

/// The files a feed keeps on this machine, for showing rather than
/// guessing: a signed-in brokerage session is something somebody is
/// entitled to know the location of, and to delete.
pub fn places(id: &str) -> Vec<(&'static str, String)> {
    match listed(id).map(|feed| feed.id) {
        Some("tos") => tos::session::places(),
        Some("rithmic") => rithmic::session::places(),
        _ => Vec::new(),
    }
}

/// Every feed, in the order they are offered.
///
/// Order is the interface: the first is the default, the panel lists them
/// like this, and a feed added at the end cannot change what an existing
/// install is using.
pub const LISTED: &[Listed] = &[yahoo::LISTED, tos::LISTED, rithmic::LISTED];

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
    let provider = built_or_default(id);
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

/// The feed with this id, built and connected to nothing, or `None` for an
/// id that is not one of ours.
///
/// Constructing one connects to nothing, opens nothing and starts no thread:
/// see the module docs.
///
/// Behind the trait, not behind an enum of the feeds. An enum here was
/// ninety lines re-stating every method of [`Provider`] to forward it, which
/// is a second copy of the boundary that has to be edited in step with the
/// first — and a copy the next feed has to be added to as well, which is
/// exactly the kind of thing that gets forgotten. The trait is already the
/// one description of what a feed can do, and it is object-safe, so this is
/// the whole dispatch.
pub fn built(id: &str) -> Option<Box<dyn Provider>> {
    match listed(id)?.id {
        "tos" => Some(Box::new(Tos::new())),
        "rithmic" => Some(Box::new(Rithmic::new())),
        _ => Some(Box::new(Yahoo::new())),
    }
}

/// The feed with this id, or the default for one nobody has heard of. For
/// asking a feed about itself — how fresh it is, how it delivers — where an
/// unknown name has nowhere better to go.
pub fn built_or_default(id: &str) -> Box<dyn Provider> {
    built(id).unwrap_or_else(|| Box::new(Yahoo::new()))
}

/// The feeds a stored order names, top first, and always ending with the
/// default.
///
/// The default is the floor: it serves everything, so a feed listed below
/// it would never be reached, and it is never stored, so an order is only
/// the feeds somebody added. An id nobody has heard of is dropped rather
/// than refused — a setting written by a newer build, or by hand, charts
/// from what this build knows — and an id named twice is built once.
/// Constructing them connects to nothing.
pub fn ordered(ids: &[String]) -> Vec<Box<dyn Provider>> {
    let mut seen: Vec<&'static str> = Vec::with_capacity(ids.len() + 1);
    let mut feeds: Vec<Box<dyn Provider>> = Vec::with_capacity(ids.len() + 1);
    for id in ids {
        let Some(feed) = listed(id) else { continue };
        if feed.id == DEFAULT || seen.contains(&feed.id) {
            continue;
        }
        if let Some(provider) = built(feed.id) {
            seen.push(feed.id);
            feeds.push(provider);
        }
    }
    feeds.push(Box::new(Yahoo::new()));
    feeds
}

/// Which feed serves a chart, and what it is called there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The feed's place in the order.
    pub at: usize,
    /// The instrument in that feed's spelling.
    pub symbol: String,
    /// Feeds above it that would have served the instrument and were passed
    /// over because they cannot be used: not signed in, or signed out. For
    /// the header, which says so. Empty, and unallocated, when nothing was.
    pub passed: Vec<usize>,
}

/// The first feed that can be used and serves `instrument`, top first.
///
/// `feeds` is the order with whether each can be used — read from what is
/// saved, never by connecting. A feed is passed over only for a reason
/// known before anything is asked of it: it cannot be used, or it has no
/// name for the instrument. A feed that fails while running keeps its
/// charts, and that is the caller's to keep to: this is asked when a chart
/// is put on screen and when the order or a sign-in changes, never because
/// a request failed.
///
/// `None` when nothing in the order serves it, which with the default at
/// the bottom means an instrument even it has no name for.
pub fn route<'a>(
    feeds: impl IntoIterator<Item = (&'a dyn Provider, bool)>,
    instrument: &Instrument,
) -> Option<Route> {
    let mut passed = Vec::new();
    for (at, (provider, usable)) in feeds.into_iter().enumerate() {
        if !usable {
            if provider.symbol_for(instrument).is_some() {
                passed.push(at);
            }
            continue;
        }
        if let Some(symbol) = provider.symbol_for(instrument) {
            return Some(Route { at, symbol, passed });
        }
    }
    None
}

/// The feeds in the order that have a name for `instrument`, by place: what
/// a chart of it can be pointed at instead of the order.
pub fn serving<'a>(feeds: impl IntoIterator<Item = &'a dyn Provider>, instrument: &Instrument) -> Vec<usize> {
    feeds
        .into_iter()
        .enumerate()
        .filter(|(_, provider)| provider.symbol_for(instrument).is_some())
        .map(|(at, _)| at)
        .collect()
}

/// Can a chart of `instrument` still be pointed at the feed `chosen`? Not
/// once it is off the order, nor when it has no name for the instrument; a
/// chart whose choice fails this goes back to following the order.
pub fn choosable<'a>(
    feeds: impl IntoIterator<Item = &'a dyn Provider>,
    chosen: &str,
    instrument: &Instrument,
) -> bool {
    feeds
        .into_iter()
        .any(|provider| provider.id().eq_ignore_ascii_case(chosen) && provider.symbol_for(instrument).is_some())
}

/// Where a chart goes when somebody has chosen its feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    /// The chosen feed serves it, or nothing was chosen and the order did.
    Route(Route),
    /// The chosen feed would serve it and cannot be used: the chart says so
    /// rather than going to the next feed down without telling anybody.
    Refused { at: usize, symbol: String },
}

/// [`route`], with the feed somebody chose for the chart first.
///
/// `chosen` is a feed's id. A choice the order no longer holds, or one with
/// no name for the instrument, is no choice at all and the order decides —
/// which is how a chart whose feed was taken off the list goes back to
/// following it. A chosen feed is never passed over: when it cannot be used
/// the chart is refused, with the place of the feed that refused it.
pub fn route_chosen<'a>(
    feeds: impl IntoIterator<Item = (&'a dyn Provider, bool)>,
    chosen: Option<&str>,
    instrument: &Instrument,
) -> Option<Chosen> {
    let feeds: Vec<(&dyn Provider, bool)> = feeds.into_iter().collect();
    if let Some(id) = chosen {
        let found = feeds.iter().enumerate().find(|(_, (provider, _))| provider.id().eq_ignore_ascii_case(id));
        if let Some((at, (provider, usable))) = found
            && let Some(symbol) = provider.symbol_for(instrument)
        {
            return Some(match usable {
                true => Chosen::Route(Route { at, symbol, passed: Vec::new() }),
                false => Chosen::Refused { at, symbol },
            });
        }
    }
    route(feeds, instrument).map(Chosen::Route)
}

/// Can `id` be charted from with what is saved on this machine? A read from
/// disk, never a connection: a feed that needs nothing saved always can.
///
/// Rithmic only in the process holding the window: anywhere else — the bar
/// widget, a command with no window open — it is passed over rather than
/// signed in a second time, because CME counts sessions. See
/// [`rithmic::session::serve_from_here`].
pub fn usable(id: &str) -> bool {
    if listed(id).is_some_and(|feed| feed.id == "rithmic") && !rithmic::session::here() {
        return false;
    }
    access(id).is_none_or(|access| access.ready())
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
            assert_eq!(built(feed.id).map(|p| p.id()), Some(feed.id));
        }
        assert!(!tos_market::connected(), "a chosen feed must not have connected");
        assert!(!::rithmic::running(), "a chosen feed must not have started a session");
    }

    #[test]
    fn nothing_stored_and_nothing_recognised_are_both_the_default() {
        assert_eq!(DEFAULT, "yahoo");
        assert!(built("").is_none());
        assert!(built("a feed we have never heard of").is_none());
        assert_eq!(built_or_default("a feed we have never heard of").id(), DEFAULT);
        let ids = |feeds: Vec<Box<dyn Provider>>| feeds.iter().map(|p| p.id()).collect::<Vec<_>>();
        assert_eq!(ids(ordered(&[])), vec![DEFAULT]);
    }

    /// A setting is typed as often as it is clicked, and " TOS " out of a
    /// shell script is the same choice as "tos".
    #[test]
    fn a_name_is_matched_however_it_was_typed() {
        assert_eq!(built("  TOS ").map(|p| p.id()), Some("tos"));
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

        // thinkorswim streams, and the word for that follows from the
        // provider handing over its stream — not from a sentence beside it.
        // The day it stopped streaming this line would change on its own.
        let tos = freshness("tos");
        assert_eq!(
            built_or_default("tos").delivery(),
            Delivery::Streamed,
            "the claim below is only honest while this holds"
        );
        assert!(tos.contains("live"), "{tos}");
        assert!(!tos.contains("delayed"), "{tos}");

        // And a feed that says nothing about streaming is polled, and says
        // so rather than claiming to be live.
        let polled = freshness(DEFAULT);
        assert!(!polled.contains("live"), "{polled}");

        for feed in LISTED {
            assert!(!described(feed).is_empty());
        }
    }

    fn instrument(kind: InstrumentKind, symbol: &str) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: String::new(),
            kind,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    /// The order is what was stored, with the default always at the bottom,
    /// never twice, and nothing this build has not heard of.
    #[test]
    fn an_order_always_ends_with_the_default() {
        let ids = |stored: &[&str]| {
            let stored: Vec<String> = stored.iter().map(|id| id.to_string()).collect();
            ordered(&stored).iter().map(|p| p.id()).collect::<Vec<_>>()
        };
        assert_eq!(ids(&[]), vec!["yahoo"]);
        assert_eq!(ids(&["tos"]), vec!["tos", "yahoo"]);
        assert_eq!(ids(&["yahoo", "tos"]), vec!["tos", "yahoo"]);
        assert_eq!(ids(&["tos", "TOS", "bloomberg"]), vec!["tos", "yahoo"]);
    }

    /// The first usable feed with a name for the instrument serves it. One
    /// that cannot be used is passed over, and said to have been only when it
    /// would have served the chart; one with no name for it is passed over
    /// silently, because that is not something anybody can fix.
    #[test]
    fn the_first_usable_feed_that_serves_an_instrument_serves_it() {
        let feeds = ordered(&["tos".to_string()]);
        let order = |tos_usable: bool| {
            feeds.iter().map(move |p| (p.as_ref(), p.id() != "tos" || tos_usable))
        };
        let apple = instrument(InstrumentKind::Equity, "AAPL");
        // thinkorswim leaves a Madrid listing alone; Yahoo has it.
        let madrid = Instrument { suffix: Some("MC".into()), ..instrument(InstrumentKind::Equity, "SAN") };

        let on_top = route(order(true), &apple).expect("served");
        assert_eq!((on_top.at, on_top.passed.as_slice()), (0, &[][..]));

        let signed_out = route(order(false), &apple).expect("served");
        assert_eq!((signed_out.at, signed_out.passed.as_slice()), (1, &[0][..]));
        assert_eq!(signed_out.symbol, "AAPL");

        let untranslated = route(order(true), &madrid).expect("served");
        assert_eq!((untranslated.at, untranslated.passed.as_slice()), (1, &[][..]));
        assert_eq!(untranslated.symbol, "SAN.MC");

        // With nothing but the default, the default serves everything.
        let alone = ordered(&[]);
        let routed = route(alone.iter().map(|p| (p.as_ref(), true)), &madrid).expect("served");
        assert_eq!((routed.at, routed.passed.capacity()), (0, 0));
    }

    /// Only the feeds with a name for an instrument are offered for it.
    #[test]
    fn a_chart_is_offered_only_the_feeds_that_serve_it() {
        let feeds = ordered(&["tos".to_string()]);
        let offered = |instrument: &Instrument| serving(feeds.iter().map(|p| p.as_ref()), instrument);
        assert_eq!(offered(&instrument(InstrumentKind::Equity, "AAPL")), vec![0, 1]);
        let madrid = Instrument { suffix: Some("MC".into()), ..instrument(InstrumentKind::Equity, "SAN") };
        assert_eq!(offered(&madrid), vec![1]);
    }

    /// A chosen feed serves its chart over the order, and is never passed
    /// over: signed out, the chart is refused rather than moved. A choice the
    /// order no longer holds, or one with no name for the instrument, leaves
    /// the order to decide.
    #[test]
    fn a_chosen_feed_serves_its_chart_and_is_never_passed_over() {
        let feeds = ordered(&["tos".to_string()]);
        let order = |tos_usable: bool| feeds.iter().map(move |p| (p.as_ref(), p.id() != "tos" || tos_usable));
        let apple = instrument(InstrumentKind::Equity, "AAPL");

        let yahoo = route_chosen(order(true), Some("yahoo"), &apple);
        assert!(matches!(yahoo, Some(Chosen::Route(Route { at: 1, .. }))), "{yahoo:?}");
        let tos = route_chosen(order(true), Some("tos"), &apple);
        assert!(matches!(tos, Some(Chosen::Route(Route { at: 0, .. }))), "{tos:?}");
        // Signed out: refused, where the order would have passed it over.
        let refused = route_chosen(order(false), Some("tos"), &apple);
        assert_eq!(refused, Some(Chosen::Refused { at: 0, symbol: "AAPL".into() }));
        assert_eq!(route_chosen(order(false), None, &apple), route(order(false), &apple).map(Chosen::Route));

        // thinkorswim taken off the order: the chart follows what is left.
        let alone = ordered(&[]);
        let gone = route_chosen(alone.iter().map(|p| (p.as_ref(), true)), Some("tos"), &apple);
        assert!(matches!(gone, Some(Chosen::Route(Route { at: 0, .. }))), "{gone:?}");
        // And a choice with no name for the instrument is no choice.
        let madrid = Instrument { suffix: Some("MC".into()), ..instrument(InstrumentKind::Equity, "SAN") };
        let unnamed = route_chosen(order(true), Some("tos"), &madrid);
        assert!(matches!(unnamed, Some(Chosen::Route(Route { at: 1, .. }))), "{unnamed:?}");

        // Which is why both choices are dropped rather than kept.
        assert!(choosable(feeds.iter().map(|p| p.as_ref()), "tos", &apple));
        assert!(!choosable(alone.iter().map(|p| p.as_ref()), "tos", &apple));
        assert!(!choosable(feeds.iter().map(|p| p.as_ref()), "tos", &madrid));
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
            let built = built_or_default(feed.id);
            assert_eq!(built.id(), feed.id);
            assert_eq!(built.label(), feed.label);
            assert!(!feed.serves.is_empty(), "{} says nothing about what it serves", feed.id);
        }
    }
}
