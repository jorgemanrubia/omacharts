//! The data provider boundary.
//!
//! Everything that knows how to talk to a market data source lives behind this
//! trait. v0 ships Yahoo alone, but a broker feed or a paid vendor arrives as
//! another implementation and nothing above this line changes — which is the
//! whole reason the boundary exists.

use std::fmt;
use std::time::{Duration, Instant};

use crate::bars::{Bar, Timeframe};
use crate::ladders::Minute;
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
                "Rate limited by the data feed. Prices fill in shortly."
            }
            FetchFailure::Offline => "No network connection. Prices fill in when there is one.",
            FetchFailure::Unreachable => {
                "The data feed is not answering. Prices fill in when it does."
            }
            FetchFailure::Garbled => "The data feed sent a reply this could not read.",
            FetchFailure::Unsupported => "The data feed does not serve this resolution.",
            FetchFailure::NoSuchSymbol => "The data feed does not have this symbol.",
            FetchFailure::LocalCache => "Could not open the local price cache.",
            FetchFailure::NeedsSignIn => {
                "This data feed is not signed in. Open Preferences → Data."
            }
            FetchFailure::Unserved => {
                "No data feed in Preferences → Data charts this symbol."
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

/// The latest two daily closes of one symbol, which is all a quote in a list
/// is: a price and how far it has moved since the session before.
///
/// Apart from [`Bar`] on purpose. A provider that answers many symbols in one
/// request answers with closes alone, and a bar made of a close would have an
/// open, high and low that never traded — which is exactly what a later fetch
/// of the real series compares against, and would read as history rewritten.
#[derive(Clone, Debug, PartialEq)]
pub struct Closes {
    /// The provider's own spelling, as it was asked for.
    pub symbol: String,
    /// The latest close, and its session's stamp in unix seconds.
    pub last: f64,
    pub at: i64,
    /// The close of the session before, and its stamp. The stamp is how a
    /// reader tells an answer that skipped a session: one comes back now and
    /// then with a day missing, and its "previous" is then two sessions old.
    pub previous: f64,
    pub previous_at: i64,
}

/// A provider's cooldown in terms another process can read.
///
/// Wall-clock seconds rather than an [`Instant`], which means nothing outside
/// the process that took it. The bar widget asks from a fresh process every
/// couple of minutes, and a cooldown that lived and died with each one let
/// every run walk straight into the refusal the last one had just been given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SharedCooldown {
    /// Unix seconds until which nothing may be asked.
    pub until: i64,
    /// What the next refusal would cost, so that the backoff keeps climbing
    /// across processes instead of starting over in each one.
    pub next: Duration,
}

impl SharedCooldown {
    /// Is this still news at `now`?
    ///
    /// While it runs, plainly. After it has run out, only for as long again
    /// as the next one would last: a provider that has been left alone that
    /// long has had its quiet, and its next refusal starts the climb from
    /// the bottom rather than from wherever a process last left it.
    pub fn stands_at(self, now: i64) -> bool {
        let next = i64::try_from(self.next.as_secs()).unwrap_or(i64::MAX);
        now < self.until.saturating_add(next)
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
/// Nor is it declared. It is read off whether the provider has a [`Stream`]
/// to offer — see [`Provider::delivery`] — so that "this feed streams" is
/// said in exactly one place, by handing the stream over, and every label,
/// timer and status line that reads it is reading the same fact.
///
/// What is *not* said here is how often a polled chart is asked. That
/// belongs to whoever holds the charts, one chart at a time, in
/// [`crate::refresh`]: the provider says it needs asking, and the cadence
/// that keeps the asking polite is the caller's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// Nothing arrives unasked. A chart left open stays current only by being
    /// fetched again.
    Polled,
    /// New bars arrive as they print, so a chart left open stays current on
    /// its own and nothing should be fetching it on a timer.
    Streamed,
}

impl Delivery {
    /// The word a settings row, `provider list` and `provider status` all
    /// use, so that they cannot drift into three renderings of one fact.
    pub fn word(self) -> &'static str {
        match self {
            Delivery::Polled => "polled",
            Delivery::Streamed => "streamed",
        }
    }
}

/// One piece of news from a stream, in the order the feed heard it.
///
/// Three kinds, and the mailbox that receives them ([`crate::stream::Mailbox`])
/// knows how to fold each into whatever is already waiting: a snapshot
/// supersedes everything before it, a bar replaces the bar with its
/// timestamp or joins the series, and a loss is remembered beside whatever
/// bars arrived first.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// The series as the feed holds it now, oldest first, the last bar
    /// possibly still forming. The first thing a subscription delivers, and
    /// what it delivers again whenever it has had to start over — after a
    /// reconnect, say — so that nothing the chart missed in between stays
    /// missing.
    Snapshot(Vec<Bar>),
    /// One bar: the one forming, or the one that just closed. Replaces the
    /// bar with the same timestamp, or joins the series if it is new.
    Bar(Bar),
    /// Nothing newer is coming until the provider says otherwise, and this
    /// is why. The chart keeps what it has and shows the reason; a later
    /// [`Update::Snapshot`] clears it. The provider owns getting back —
    /// reconnecting, re-requesting — and nothing above it retries on its
    /// behalf.
    Lost(FetchFailure),
}

/// Where a stream puts what it hears.
///
/// Called from whatever thread the provider reads its socket on, as often
/// as the feed likes. It is cheap by contract — it takes a lock nobody
/// contends, overwrites one bar, and at most sends one wake — so a
/// provider may call it per print without a thought.
pub type Sink = Box<dyn Fn(Update) + Send + Sync>;

/// A provider's live feed, for one that has one.
///
/// The capability a provider adds to say it streams. Not part of
/// [`Provider`] itself, because most providers have no such thing and a
/// method they all had to answer "no" to is a method somebody forgets: a
/// provider returns this from [`Provider::stream`] or it does not, and that
/// is the whole declaration.
pub trait Stream: Send + Sync {
    /// Start delivering `symbol` at `timeframe` into `sink`, beginning with
    /// an [`Update::Snapshot`], until the handle is dropped.
    ///
    /// Three rules, and every implementation has to keep them.
    ///
    /// It returns before anything touches the network. This is called on the
    /// thread that draws the window, and a connect or a login belongs on the
    /// provider's own thread, with its outcome arriving through the sink —
    /// as a snapshot, or as [`Update::Lost`] saying what went wrong.
    ///
    /// Reconnecting is the provider's. A socket that drops is a loss
    /// followed, when the provider manages it, by a fresh snapshot; the
    /// caller never retries and never polls on its behalf.
    ///
    /// Dropping the handle ends the stream and never blocks.
    ///
    /// `since` is what the caller already holds, in unix seconds: the
    /// snapshot need not go back further, though it may. `None` asks for as
    /// much history as the provider will give.
    ///
    /// An error here is a refusal known before any request — a resolution
    /// the provider does not serve, say — and is final.
    fn subscribe(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
        sink: Sink,
    ) -> Result<Box<dyn Subscription>, ProviderError>;
}

/// An open subscription. It has no methods: dropping it is how it ends.
pub trait Subscription: Send {}

/// One piece of news from a live ladder feed, in the order it was heard.
#[derive(Clone, Debug, PartialEq)]
pub enum LadderUpdate {
    /// The minute trading now, as it stands after the latest print.
    Forming(Minute),
    /// A minute that closed, final, and the span of minute opens the feed
    /// now knows complete — every print in it seen — if it knows one. A
    /// minute the feed joined part way through is sealed with a span that
    /// leaves it out.
    Sealed { minute: Minute, covered: Option<(i64, i64)> },
    /// The feed dropped, or started over after missing prints: nothing is
    /// complete from here until it is back, and this is the minute that was
    /// forming, partial. The span missed is a gap, asked of the provider's
    /// history once the feed is [`LadderUpdate::Resumed`].
    Dropped(Option<Minute>),
    /// The feed is back after a drop or a start over: whatever it missed
    /// meanwhile can be asked of the provider's history now.
    Resumed,
}

/// Where a ladder feed puts what it hears. Called on the provider's thread,
/// cheap by contract, as a [`Sink`] is.
pub type LadderSink = Box<dyn Fn(LadderUpdate) + Send + Sync>;

/// A provider's volume at price: minute ladders as history and live.
///
/// A capability of its own, reached through [`Provider::ladders`], which is
/// `None` unless a provider has them, so no provider is ever asked about
/// something it lacks and a chart of one without them holds nothing.
pub trait Ladders: Send + Sync {
    /// Does it have ladders of `symbol`, in its own spelling? Offline, as
    /// [`Provider::symbol_for`] is.
    ///
    /// Only the feed's half of the question. The tick every price is a whole
    /// number of is the instrument's — [`crate::Instrument::tick_size`] —
    /// and is handed to [`Self::minutes`] and [`Self::watch`], so a feed
    /// keeps no table of contracts of its own.
    fn serves(&self, symbol: &str) -> bool;

    /// How far back its history goes, in days. A request is never made for
    /// further back than this.
    fn history_days(&self) -> u32;

    /// The minutes of `symbol` opening in `from..to`, unix seconds, oldest
    /// first, priced in whole ticks of `tick_size`. One request, made now,
    /// on the caller's thread, which is never the one drawing the window.
    /// What it returns is the whole truth about the span: a minute it leaves
    /// out traded nothing.
    fn minutes(&self, symbol: &str, tick_size: f64, from: i64, to: i64) -> Result<Vec<Minute>, ProviderError>;

    /// Start delivering `symbol`'s live ladders, in whole ticks of
    /// `tick_size`, into `sink` until the handle is dropped. The rules of
    /// [`Stream::subscribe`]: it returns before anything touches the
    /// network, reconnecting is the provider's, and dropping the handle ends
    /// it without blocking.
    fn watch(&self, symbol: &str, tick_size: f64, sink: LadderSink) -> Result<Box<dyn Subscription>, ProviderError>;
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

    /// The dated contracts of the futures root `root` this provider lists,
    /// each as its symbol in the exchange's spelling and the day it stops
    /// trading, in no particular order. One request, made now, off the
    /// thread that draws the window. `Err(Unsupported)` from a provider
    /// that lists none, which is what the rule in [`crate::contracts`]
    /// stands in for.
    fn contracts(&self, root: &str) -> Result<Vec<(String, chrono::NaiveDate)>, ProviderError> {
        Err(ProviderError::Unsupported(format!("{} lists no contracts of {root}", self.label())))
    }

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

    /// How many symbols one [`Self::closes`] request may name. Zero, the
    /// default, for a provider that has no such request.
    fn max_batch(&self) -> usize {
        0
    }

    /// The latest two daily closes of each of `symbols`, in one request.
    ///
    /// For a list of prices rather than a chart: one request where asking
    /// for each symbol's bars would be one each, which is the difference
    /// between a bar widget that stays current and one that spends the rate
    /// limit doing it. At most [`Self::max_batch`] symbols. One the provider
    /// does not know is left out of the answer rather than failing the rest.
    ///
    /// Paced, refused and retried like [`Self::bars`]: a batch is one
    /// request. Unsupported by default.
    fn closes(&self, symbols: &[&str]) -> Result<Vec<Closes>, ProviderError> {
        let _ = symbols;
        Err(ProviderError::Unsupported("closes in a batch".into()))
    }

    /// The cooldown as it stands, for another process to carry on from.
    ///
    /// `None` when there is nothing worth handing on: no cooldown running
    /// and none climbing. Defaults to that, for a provider that never sets
    /// one.
    fn shared_cooldown(&self) -> Option<SharedCooldown> {
        None
    }

    /// Carry on from a cooldown another process handed on.
    ///
    /// Taken as the stricter of the two: a cooldown already running here is
    /// not shortened, and a backoff that has climbed further here is not
    /// lowered. Does nothing by default.
    fn resume_cooldown(&self, cooldown: SharedCooldown) {
        let _ = cooldown;
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

    /// The live feed, for a provider that has one.
    ///
    /// `None` is the default and the safe direction to be wrong in: a
    /// provider that streams and forgot to say so is asked for bars it was
    /// about to push anyway, while one that is polled and claimed to stream
    /// would leave every chart stale with nothing on screen to say so.
    fn stream(&self) -> Option<&dyn Stream> {
        None
    }

    /// Volume at price, for a provider that has it: what traded at each
    /// price and on which side, minute by minute.
    ///
    /// `None` by default, and asked once per chart rather than per bar: a
    /// chart of a provider without ladders allocates none, queries nothing
    /// and sends no second request, and its volume profile is the estimate
    /// from bar ranges it has always been.
    fn ladders(&self) -> Option<&dyn Ladders> {
        None
    }

    /// The window is done with this provider: release whatever it holds
    /// that outlives a value — a connection, a session on a gateway.
    ///
    /// A method rather than `Drop`, because a provider is a cheap value
    /// that is built in several places to be asked how it is offered, and
    /// a connection closed every time one of those is let go of would be a
    /// connection closed under the window. This is called once, by the
    /// window, when the feed is switched away from or the window closes.
    /// Defaults to nothing, which is right for a provider that holds
    /// nothing.
    fn retire(&self) {}

    /// How a chart drawn from this provider is kept current.
    ///
    /// Derived from [`Self::stream`], and not to be overridden: a provider
    /// with a stream is streamed, one without is polled, and there is no way
    /// to say one thing here and do another.
    fn delivery(&self) -> Delivery {
        if self.stream().is_some() { Delivery::Streamed } else { Delivery::Polled }
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
        assert!(Bare.stream().is_none());
    }

    /// A provider that hands over a stream, and nothing else about it.
    struct Pushes;

    impl Stream for Pushes {
        fn subscribe(
            &self,
            _: &str,
            _: Timeframe,
            _: Option<i64>,
            _: Sink,
        ) -> Result<Box<dyn Subscription>, ProviderError> {
            Err(ProviderError::Unsupported("a test double".into()))
        }
    }

    impl Provider for Pushes {
        fn id(&self) -> &'static str {
            "pushes"
        }
        fn label(&self) -> &'static str {
            "Pushes"
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
        fn stream(&self) -> Option<&dyn Stream> {
            Some(self)
        }
    }

    /// The whole of the declaration: handing over a stream is what makes a
    /// provider streamed. There is no second method to keep in step, so the
    /// label on a settings row and the timer the window does not install
    /// are reading one fact.
    #[test]
    fn handing_over_a_stream_is_what_makes_a_provider_streamed() {
        assert_eq!(Pushes.delivery(), Delivery::Streamed);
        assert_eq!(Delivery::Streamed.word(), "streamed");
        assert_eq!(Delivery::Polled.word(), "polled");
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
