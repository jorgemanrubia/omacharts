//! The data provider boundary.
//!
//! Everything that knows how to talk to a market data source lives behind this
//! trait. v0 ships Yahoo alone, but a broker feed or a paid vendor arrives as
//! another implementation and nothing above this line changes — which is the
//! whole reason the boundary exists.

use std::fmt;
use std::time::{Duration, Instant};

use crate::bars::{Bar, Timeframe};
use crate::symbols::{Instrument, InstrumentKind};

#[derive(Debug)]
pub enum ProviderError {
    /// The provider cannot serve this instrument or timeframe at all.
    Unsupported(String),
    /// Rate limited. The cache is the defence; back off and show what we have.
    RateLimited,
    /// Nothing on this machine could get to the provider: no route, no name
    /// resolution, nothing listening. Apart from [`ProviderError::Network`]
    /// because the two are different sentences on a chart, and only one of
    /// them is about the provider at all.
    Offline(String),
    Network(String),
    /// A response arrived but did not look like data.
    Malformed(String),
    /// The provider is not set up on this machine yet — a feed that needs an
    /// account signed into, with nothing signed in. Carries the sentence to
    /// put in front of the user, which is the provider's to write because
    /// only it knows what is missing.
    NeedsSetup(String),
    /// The provider has no such symbol.
    NotFound,
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Unsupported(what) => write!(f, "not supported: {what}"),
            ProviderError::RateLimited => write!(f, "rate limited"),
            ProviderError::Offline(e) => write!(f, "offline: {e}"),
            ProviderError::Network(e) => write!(f, "network: {e}"),
            ProviderError::Malformed(e) => write!(f, "unexpected response: {e}"),
            ProviderError::NeedsSetup(what) => write!(f, "{what}"),
            ProviderError::NotFound => write!(f, "no such symbol"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Why a chart has nothing new, in the words it is allowed to use on screen.
///
/// This exists because "No data for this symbol" was shown for every one of
/// these, and it is only true for one of them. A first-time user whose first
/// fetch is refused reads that line as "this app does not have the S&P 500"
/// and concludes the app is empty — when the symbol is fine and the request
/// never arrived.
///
/// Separate from [`ProviderError`] because the two answer different
/// questions. That one says what went wrong, in as much detail as the
/// transport had. This one says which of a handful of sentences is the true
/// one, and several provider errors collapse onto a single sentence: a 503
/// and a dropped connection are both "the provider is not answering" to
/// somebody looking at a chart.
///
/// `Copy` and allocation-free on purpose: it crosses the worker-to-UI channel
/// on every failed fetch and is read again on every repaint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FetchFailure {
    /// The provider is throttling us. Temporary and self-correcting, which is
    /// the one thing the user has to be told, because waiting is the fix.
    RateLimited,
    /// This machine cannot reach anything.
    Offline,
    /// This machine is fine; the provider did not answer.
    Unreachable,
    /// Something answered, and it was not data.
    Garbled,
    /// The provider does not serve this instrument at this resolution.
    Unsupported,
    /// The provider answered, and does not know this symbol. Distinct from a
    /// provider that knows it and has no bars for it, which is not a failure
    /// and keeps "No data for this symbol" to itself.
    NoSuchSymbol,
    /// The local price cache could not be opened. Nothing to do with the
    /// network, and saying so stops a disk problem looking like one.
    LocalCache,
    /// The feed needs signing in to and nobody has. The one failure here with
    /// something the user can do about it.
    NeedsSignIn,
    /// This feed has no name for this instrument at all, so nothing was
    /// asked. Apart from [`FetchFailure::NoSuchSymbol`], which is a provider
    /// that was asked and said no: this one is known before any request, and
    /// the fix is a different feed rather than a different spelling.
    Unserved,
}

impl FetchFailure {
    /// The line a chart with no bars at all puts where the bars would be, and
    /// the tooltip on the dot a chart that does have bars shows in its corner.
    ///
    /// One sentence for what happened, and — only where it is true — a second
    /// for the fact that it fixes itself. On a chart with bars what is drawn
    /// is real and older than it should be, and this says why nothing newer
    /// arrived; it replaced a marker that only ever said "stale", which left
    /// the user to guess between a dead symbol, a broken app and a provider
    /// having a bad minute.
    ///
    /// An instruction appears in exactly one of these, and only because
    /// there is genuinely something to do: a feed nobody has signed in to
    /// stays broken until somebody signs in, so the line says where. For all
    /// the rest there is nothing for the user to act on — they fix themselves
    /// or they are not about the chart — and inventing an instruction for
    /// those would be advice to go and fiddle with something that is working.
    pub fn message(self) -> &'static str {
        match self {
            FetchFailure::RateLimited => {
                "Rate limited by the data provider. Prices fill in shortly."
            }
            FetchFailure::Offline => "No network connection. Prices fill in when there is one.",
            FetchFailure::Unreachable => {
                "The data provider is not answering. Prices fill in when it does."
            }
            FetchFailure::Garbled => "The data provider sent a reply this could not read.",
            FetchFailure::Unsupported => "The data provider does not serve this resolution.",
            FetchFailure::NoSuchSymbol => "The data provider does not have this symbol.",
            FetchFailure::LocalCache => "Could not open the local price cache.",
            FetchFailure::NeedsSignIn => {
                "This data provider is not signed in. Open Preferences → Market data → Provider."
            }
            FetchFailure::Unserved => {
                "This data provider cannot chart this symbol. Another one may be able to."
            }
        }
    }

}

impl From<&ProviderError> for FetchFailure {
    fn from(error: &ProviderError) -> FetchFailure {
        match error {
            ProviderError::RateLimited => FetchFailure::RateLimited,
            ProviderError::Offline(_) => FetchFailure::Offline,
            ProviderError::Network(_) => FetchFailure::Unreachable,
            ProviderError::Malformed(_) => FetchFailure::Garbled,
            ProviderError::NeedsSetup(_) => FetchFailure::NeedsSignIn,
            ProviderError::Unsupported(_) => FetchFailure::Unsupported,
            ProviderError::NotFound => FetchFailure::NoSuchSymbol,
        }
    }
}

/// What a provider can serve for one timeframe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capability {
    pub timeframe: Timeframe,
    /// How far back the provider will go, in days. `None` means "as far as it
    /// has".
    pub history_days: Option<u32>,
    /// The widest window a single request may ask for, in days.
    ///
    /// Separate from `history_days` because providers cap the two
    /// independently: Yahoo keeps a month of one-minute bars but refuses to
    /// hand over more than a week at a time, and asking for the month returns
    /// nothing at all rather than an error.
    pub max_request_days: Option<u32>,
}

/// How far apart a provider needs its requests kept.
///
/// Rules, not timing. A provider says what it will tolerate, and whoever
/// holds the queue of requests does the waiting — on something it can be
/// woken from, so that the chart somebody has just asked for is never stuck
/// behind a gap being sat out on behalf of a request nobody wanted. A
/// provider that slept here instead would be a second scheduler, one that
/// knows nothing about the queue and overrules it from inside a call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pacing {
    /// The least time after any request before the next one somebody is
    /// waiting on may go.
    pub min_gap: Duration,
    /// The same, before the next one nobody asked for. Expected to be the
    /// longer of the two: work done on a guess has no reason to spend the
    /// request budget quickly.
    pub min_gap_speculative: Duration,
}

impl Pacing {
    /// No pacing at all: every request may go the moment it is wanted.
    pub const NONE: Pacing =
        Pacing { min_gap: Duration::ZERO, min_gap_speculative: Duration::ZERO };

    /// The gap that applies before a request of this kind.
    pub fn gap(self, speculative: bool) -> Duration {
        if speculative { self.min_gap_speculative } else { self.min_gap }
    }
}

/// How new bars reach a chart drawn from a provider.
///
/// A chart is a photograph until something refetches it, and whether anything
/// needs to is a fact about the provider, not a preference: a feed that pushes
/// each print has nothing to be asked for, and one that only ever answers a
/// request has to be asked again or the chart quietly stops being true. There
/// is no switch for this anywhere above the provider — there used to be, and
/// the off position was a chart that looked current and was not, which is the
/// one way a charting application is actively misleading rather than merely
/// incomplete.
///
/// What is *not* said here is how often. That belongs to whoever holds the
/// charts, one chart at a time, in [`crate::refresh`]: the provider says it
/// needs asking, and the cadence that keeps the asking polite is the caller's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// Nothing arrives unasked. A chart left open stays current only by being
    /// fetched again.
    Polled,
    /// New bars arrive as they print, so a chart left open stays current on
    /// its own and nothing should be fetching it on a timer.
    Streamed,
}

pub trait Provider: Send + Sync {
    /// Stable id, used as the `adapter` key in stored symbol mappings.
    fn id(&self) -> &'static str;

    /// What to call it in the UI.
    fn label(&self) -> &'static str;

    /// How stale this provider's data is for a kind of instrument, in minutes.
    /// Shown to the user — a chart must never imply it is live when it is not.
    fn delay_minutes(&self, kind: InstrumentKind) -> u32;

    /// This instrument in the provider's own symbology, if it serves it.
    fn symbol_for(&self, instrument: &Instrument) -> Option<String>;

    /// Natively served timeframes. Derived ones are folded by the caller.
    fn capabilities(&self) -> &'static [Capability];

    /// Bars for `symbol`, oldest first, at a timeframe the provider serves
    /// natively.
    ///
    /// `since` is an inclusive lower bound in unix seconds; `None` asks for as
    /// much history as the provider will give. Implementations must not return
    /// bars they had to invent, and the final bar may still be forming.
    ///
    /// One request, made now. An implementation never waits before it and
    /// never tries again after it: how far apart requests must be kept, when
    /// none may be made at all, and whether a failure is worth another go
    /// are rules it *states* — [`Self::pacing`], [`Self::cooldown_until`],
    /// [`Self::retry_after`] — and the caller's queue applies. One place
    /// waits, and it is the place that knows what else is waiting.
    fn bars(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
    ) -> Result<Vec<Bar>, ProviderError>;

    /// How far apart this provider's requests must be kept.
    ///
    /// Rules only, and constant: the provider is never the one that holds a
    /// request back. Defaults to none, for a provider with no rate limit
    /// worth respecting.
    fn pacing(&self) -> Pacing {
        Pacing::NONE
    }

    /// Until when nothing at all may be asked of the provider, if such a
    /// time is set — after it has said it is being asked too much, say.
    ///
    /// A request wanted before then is refused by the caller without
    /// reaching the network, as [`ProviderError::RateLimited`]. The state is
    /// the provider's because only it sees the answers that set and clear
    /// it; sitting the cooldown out is not, and an implementation must
    /// never block a caller on it. Defaults to never.
    fn cooldown_until(&self) -> Option<Instant> {
        None
    }

    /// Is a request that failed with `error` on its `attempt`th try, counted
    /// from zero, worth another — and how long after?
    ///
    /// Policy only. The caller holds the request back for that long and
    /// makes the next attempt itself, so a request being retried is still a
    /// queued request and everything more wanted overtakes it. `None` is a
    /// final answer, and the default: a provider that has no transient
    /// failures worth a second look says nothing here.
    fn retry_after(&self, error: &ProviderError, attempt: u32) -> Option<Duration> {
        let _ = (error, attempt);
        None
    }

    /// How a chart drawn from this provider is kept current.
    ///
    /// Polled unless the provider says otherwise, because that is the safe
    /// direction to be wrong in: a provider that streams and forgot to say so
    /// is asked for bars it was about to push anyway, while one that is
    /// polled and claimed to stream would leave every chart stale with
    /// nothing on screen to say so.
    fn delivery(&self) -> Delivery {
        Delivery::Polled
    }

    /// Does this provider serve the timeframe natively?
    fn serves(&self, timeframe: Timeframe) -> bool {
        self.capabilities().iter().any(|c| c.timeframe == timeframe)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider that states nothing beyond what the trait requires.
    struct Bare;

    impl Provider for Bare {
        fn id(&self) -> &'static str {
            "bare"
        }
        fn label(&self) -> &'static str {
            "Bare"
        }
        fn delay_minutes(&self, _: InstrumentKind) -> u32 {
            0
        }
        fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
            Some(instrument.symbol.clone())
        }
        fn capabilities(&self) -> &'static [Capability] {
            &[]
        }
        fn bars(&self, _: &str, _: Timeframe, _: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
            Ok(Vec::new())
        }
    }

    /// The safe direction to be wrong in. A provider that says nothing about
    /// how it delivers bars gets asked for them, so a new implementation
    /// cannot leave charts stale by forgetting a method.
    #[test]
    fn a_provider_that_says_nothing_is_polled() {
        assert_eq!(Bare.delivery(), Delivery::Polled);
    }

    /// The bug this whole type exists for: every failure used to reach the
    /// chart as the same empty placeholder, which said the symbol had no data.
    #[test]
    fn a_fetch_that_failed_never_says_the_symbol_has_no_data() {
        let failures = [
            FetchFailure::RateLimited,
            FetchFailure::Offline,
            FetchFailure::Unreachable,
            FetchFailure::Garbled,
            FetchFailure::Unsupported,
            FetchFailure::NoSuchSymbol,
            FetchFailure::LocalCache,
            FetchFailure::NeedsSignIn,
            FetchFailure::Unserved,
        ];
        for failure in failures {
            assert_ne!(failure.message(), "No data for this symbol");
        }
    }

    #[test]
    fn being_rate_limited_says_so_and_says_it_passes() {
        let message = FetchFailure::RateLimited.message();
        assert!(message.contains("Rate limited"), "{message}");
        assert!(message.contains("shortly"), "waiting is the fix, so say so: {message}");
    }

    #[test]
    fn having_no_network_is_not_blamed_on_the_provider() {
        let message = FetchFailure::Offline.message();
        assert!(message.contains("No network"), "{message}");
        assert!(!message.contains("provider"), "this one is about the machine: {message}");
    }

    /// A provider that is up and does not know the ticker is a different
    /// thing from one nothing could reach, and the rail and the chart have to
    /// be able to tell a person which they are looking at.
    #[test]
    fn an_unknown_symbol_and_an_unreachable_provider_read_differently() {
        assert_ne!(
            FetchFailure::NoSuchSymbol.message(),
            FetchFailure::Unreachable.message()
        );
    }

    #[test]
    fn every_provider_error_carries_its_own_words_to_the_chart() {
        let cases = [
            (ProviderError::RateLimited, FetchFailure::RateLimited),
            (ProviderError::Offline(String::new()), FetchFailure::Offline),
            (ProviderError::Network(String::new()), FetchFailure::Unreachable),
            (ProviderError::Malformed(String::new()), FetchFailure::Garbled),
            (ProviderError::Unsupported(String::new()), FetchFailure::Unsupported),
            (ProviderError::NotFound, FetchFailure::NoSuchSymbol),
            (ProviderError::NeedsSetup(String::new()), FetchFailure::NeedsSignIn),
        ];
        for (error, expected) in &cases {
            assert_eq!(FetchFailure::from(error), *expected, "{error}");
        }
    }

    /// Two failures that read the same are two failures the user cannot tell
    /// apart, which is the state this replaced.
    #[test]
    fn no_two_failures_say_the_same_thing() {
        let mut messages: Vec<&str> = [
            FetchFailure::RateLimited,
            FetchFailure::Offline,
            FetchFailure::Unreachable,
            FetchFailure::Garbled,
            FetchFailure::Unsupported,
            FetchFailure::NoSuchSymbol,
            FetchFailure::LocalCache,
            FetchFailure::NeedsSignIn,
            FetchFailure::Unserved,
        ]
        .iter()
        .map(|f| f.message())
        .collect();
        let total = messages.len();
        messages.sort_unstable();
        messages.dedup();
        assert_eq!(messages.len(), total);
    }
}
