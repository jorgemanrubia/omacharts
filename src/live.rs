//! One source per series being streamed, shared by every chart that shows it.
//!
//! The provider's half of streaming is thin: start a subscription for a
//! symbol and resolution, push what arrives into a sink, stop when the
//! handle goes. Everything that makes that usable by a window lives here,
//! once, above any provider — and knows nothing about which one it is.
//!
//! A chart *watches* a series, which is a cache key and the native
//! resolution it is served at. The first watcher opens the provider's
//! subscription; the rest join it; the last to leave sends it idle, and an
//! idle series is torn down on the next pass rather than at once, so that a
//! chartbook switch or a resolution change and back costs no network at all.
//! Ticks land in a per-series [`Mailbox`] on the provider's thread, where only
//! the newest state of each bar is kept — that is the backpressure — and the
//! main loop is woken at most once per pending drain. The drain folds what
//! waited into the native series in memory and says which charts have to
//! change and from which bar; the window does the drawing.
//!
//! The in-memory series is the truth while a subscription is open. It is
//! written behind to the cache on a thread of its own: a snapshot at once,
//! because it is history and the chart draws from the merge; ticks ten
//! seconds after the series last changed; and everything dirty at teardown.
//! A lost stream is retried from here too, on a bounded backoff that a
//! snapshot resets and that a final refusal — a symbol the feed does not
//! have — never starts. Nothing in this module runs on a timer while
//! nothing is happening.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use omacharts_engine::stream::{upsert, Mailbox, Pending};
use omacharts_engine::{Bar, FetchFailure, Provider, Subscription, Timeframe, Update};

use crate::store::Store;

/// How long a series may be ahead of the cache.
///
/// Ten seconds is the most anything reading `bar_series` — the rail, the bar
/// widget, a window opened later — is behind the screen, and the most a
/// crash can lose of a forming bar, which the next snapshot restores anyway.
/// Shorter would mean rewriting a few hundred kilobytes of encoded columns
/// several times a minute per series for nobody's benefit.
pub const WRITE_BEHIND: Duration = Duration::from_secs(10);

/// The first wait before a lost stream is asked for again, doubling each
/// time to [`RETRY_CEILING`]. A snapshot puts it back to the start.
pub const RETRY_FLOOR: Duration = Duration::from_secs(1);
pub const RETRY_CEILING: Duration = Duration::from_secs(60);

/// One thing being streamed: a cache key and the resolution it is served at.
///
/// The resolution is the native one, which is what the cache is keyed by and
/// what a loader reply is fanned out by: a chart on four hours fed from
/// hourly bars watches the hourly series, and two charts on the same symbol
/// at the same native resolution watch one thing.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Series {
    pub key: String,
    pub native: Timeframe,
}

/// What watching a series got a chart, right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Watching {
    /// The series is live and in memory; paint from it.
    Showing,
    /// A subscription is open or being opened; the snapshot will arrive.
    Opening,
    /// The provider refused before any request, and will not be asked
    /// again for this chart.
    Refused(FetchFailure),
}

/// What a drain found for a series, in the terms the window redraws in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Change {
    /// The whole series changed: paint it as a new one.
    Snapshot(Series),
    /// Bars from this index of the native series on changed; everything
    /// before it is as it was.
    Tail { series: Series, from: usize },
    /// Nothing newer is coming for now, and this is why.
    Lost { series: Series, why: FetchFailure },
}

/// What came out of a drain.
#[derive(Default, Debug)]
pub struct Drained {
    pub changes: Vec<Change>,
    /// Something became dirty and no flush is scheduled: schedule one for
    /// [`WRITE_BEHIND`] from now.
    pub schedule_flush: bool,
    /// Something was lost and will be retried: schedule a retry for
    /// [`Live::next_retry_at`].
    pub schedule_retry: bool,
}

/// One subscription as `provider status` reports it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Report {
    pub symbol: String,
    pub native: Timeframe,
    pub charts: usize,
    pub bars: usize,
    /// The last bar's timestamp, once a snapshot has arrived.
    pub last_bar: Option<i64>,
    /// How long since the last bar arrived, once one has.
    pub updated_ago: Option<Duration>,
    pub state: ReportState,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ReportState {
    Opening,
    Live,
    Lost { why: FetchFailure, retrying: bool },
}

/// Where a lost series stands.
#[derive(Clone, Copy, Debug)]
enum State {
    Opening,
    Live { last_update: Instant },
    Lost { why: FetchFailure, attempt: u32, retry_at: Option<Instant> },
}

struct Watched {
    symbol: String,
    /// The provider's handle while a subscription is open. `None` once the
    /// stream was lost, until it is asked for again.
    handle: Option<Box<dyn Subscription>>,
    mailbox: Arc<Mailbox>,
    /// The native series, once the first snapshot has been merged with
    /// what the cache held.
    bars: Option<Vec<Bar>>,
    /// Bars that arrived while there was nothing to apply them to yet — a
    /// snapshot still with the writer — applied on top once there is.
    held: Vec<Bar>,
    /// A snapshot is with the writer being merged.
    merging: bool,
    watchers: HashSet<u32>,
    /// When ticks last made the series newer than the cache; `None` when
    /// the cache is current.
    dirty_since: Option<Instant>,
    state: State,
    /// Set when the last watcher left; cleared when one comes back.
    idle: bool,
}

/// What the writer thread is asked to do.
enum Job {
    /// A snapshot arrived: merge it with the cache and with what memory
    /// held, write the result, and hand it back as the series to draw.
    Merge { series: Series, snapshot: Vec<Bar>, memory: Vec<Bar> },
    /// Ticks since the last write: fold them into the cache.
    Write { series: Series, bars: Vec<Bar> },
    /// Say when everything before this has been written.
    Sync(mpsc::Sender<()>),
}

/// What the writer thread says back.
pub enum Written {
    Merged { series: Series, bars: Vec<Bar> },
    Flushed { series: Series },
}

struct Writer {
    /// `None` once the writer has been finished: with the last sender gone
    /// the thread's loop ends, and with the thread gone so does the channel
    /// the window's listener waits on — which is what lets a retired
    /// registry be dropped at all.
    jobs: std::sync::Mutex<Option<mpsc::Sender<Job>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

pub struct Live {
    provider: Arc<dyn Provider>,
    watched: RefCell<HashMap<Series, Watched>>,
    by_pane: RefCell<HashMap<u32, Series>>,
    /// A wake for the main loop, capacity one: a tick while one is pending
    /// finds the channel full and sends nothing.
    nudge: async_channel::Sender<()>,
    nudges: async_channel::Receiver<()>,
    /// Set by the first tick after a drain, cleared by the next drain, so a
    /// tick costs its mailbox and one atomic load while a drain is pending.
    armed: Arc<AtomicBool>,
    writer: Option<Writer>,
    written: async_channel::Receiver<Written>,
    flush_scheduled: Cell<bool>,
    /// Swapped with each mailbox on a drain, keeping its capacity.
    scratch: RefCell<Pending>,
}

impl Live {
    /// Streaming for `provider`, which must have a stream to offer, writing
    /// behind to the database at `store` — or nowhere, for a database with
    /// no file, in which case a snapshot is drawn as it arrived.
    pub fn new(provider: Arc<dyn Provider>, store: Option<PathBuf>) -> Live {
        let (nudge, nudges) = async_channel::bounded(1);
        let (written_tx, written) = async_channel::unbounded();
        let writer = store.map(|path| Writer::start(path, written_tx));
        Live {
            provider,
            watched: RefCell::new(HashMap::new()),
            by_pane: RefCell::new(HashMap::new()),
            nudge,
            nudges,
            armed: Arc::new(AtomicBool::new(false)),
            writer,
            written,
            flush_scheduled: Cell::new(false),
            scratch: RefCell::new(Pending::default()),
        }
    }

    /// Wakes, one per pending drain. The window drains on the next frame.
    pub fn nudges(&self) -> async_channel::Receiver<()> {
        self.nudges.clone()
    }

    /// What the writer finished. A merge is a series to paint; a flush is
    /// news for whatever reads the cache.
    pub fn written(&self) -> async_channel::Receiver<Written> {
        self.written.clone()
    }

    // -- watching ----------------------------------------------------------

    /// Chart `pane` now shows `series`, whose provider symbol is `symbol`
    /// and whose cache holds bars up to `since`. Whatever it watched before
    /// is released.
    pub fn watch(&self, pane: u32, series: Series, symbol: &str, since: Option<i64>) -> Watching {
        if self.by_pane.borrow().get(&pane) != Some(&series) {
            self.unwatch(pane);
        }
        let mut watched = self.watched.borrow_mut();
        if let Some(entry) = watched.get_mut(&series) {
            entry.watchers.insert(pane);
            entry.idle = false;
            self.by_pane.borrow_mut().insert(pane, series);
            return if entry.bars.is_some() { Watching::Showing } else { Watching::Opening };
        }
        let mailbox = Arc::new(Mailbox::new());
        let handle = match self.open(&series, symbol, since, &mailbox) {
            Ok(handle) => handle,
            Err(refused) => return Watching::Refused(refused),
        };
        watched.insert(
            series.clone(),
            Watched {
                symbol: symbol.to_string(),
                handle: Some(handle),
                mailbox,
                bars: None,
                held: Vec::new(),
                merging: false,
                watchers: HashSet::from([pane]),
                dirty_since: None,
                state: State::Opening,
                idle: false,
            },
        );
        self.by_pane.borrow_mut().insert(pane, series);
        Watching::Opening
    }

    /// Ask the provider for `series`, into `mailbox`, waking the main loop
    /// on the first tick after each drain.
    fn open(
        &self,
        series: &Series,
        symbol: &str,
        since: Option<i64>,
        mailbox: &Arc<Mailbox>,
    ) -> Result<Box<dyn Subscription>, FetchFailure> {
        let Some(stream) = self.provider.stream() else {
            return Err(FetchFailure::Unsupported);
        };
        let into = mailbox.clone();
        let armed = self.armed.clone();
        let nudge = self.nudge.clone();
        let sink = Box::new(move |update: Update| {
            // The mailbox says whether this is the first tick since it was
            // last drained; the flag says whether a drain is already on its
            // way for any series. Only the first of both sends.
            if into.push(update) && !armed.swap(true, Ordering::AcqRel) {
                let _ = nudge.try_send(());
            }
        });
        stream.subscribe(symbol, series.native, since, sink).map_err(|error| FetchFailure::from(&error))
    }

    /// Chart `pane` stopped showing whatever it watched. True when a series
    /// went idle and a reap should be scheduled.
    pub fn unwatch(&self, pane: u32) -> bool {
        let Some(series) = self.by_pane.borrow_mut().remove(&pane) else {
            return false;
        };
        let mut watched = self.watched.borrow_mut();
        let Some(entry) = watched.get_mut(&series) else {
            return false;
        };
        entry.watchers.remove(&pane);
        if entry.watchers.is_empty() {
            entry.idle = true;
            return true;
        }
        false
    }

    /// Every chart stopped showing everything: a chartbook is being taken
    /// down. True when anything went idle.
    pub fn unwatch_all(&self) -> bool {
        let panes: Vec<u32> = self.by_pane.borrow().keys().copied().collect();
        let mut idle = false;
        for pane in panes {
            idle |= self.unwatch(pane);
        }
        idle
    }

    /// Tear down every series that is still idle: flush what it holds,
    /// drop the provider's handle, forget it.
    ///
    /// Deferred to here rather than done in `unwatch` so that a chart which
    /// comes straight back — the same symbol in the next chartbook, the
    /// resolution changed and changed back — finds its subscription where it
    /// left it.
    pub fn reap(&self) {
        let mut watched = self.watched.borrow_mut();
        let reaped: Vec<Series> = watched
            .iter()
            .filter(|(_, entry)| entry.idle && entry.watchers.is_empty())
            .map(|(series, _)| series.clone())
            .collect();
        for series in reaped {
            if let Some(entry) = watched.remove(&series) {
                self.flush_one(&series, &entry);
                drop(entry.handle);
            }
        }
    }

    /// Is `series` live, with bars in memory?
    pub fn is_showing(&self, series: &Series) -> bool {
        self.watched.borrow().get(series).is_some_and(|entry| entry.bars.is_some())
    }

    /// The native series in memory, for painting from.
    pub fn with_bars<T>(&self, series: &Series, read: impl FnOnce(&[Bar]) -> T) -> Option<T> {
        let watched = self.watched.borrow();
        watched.get(series)?.bars.as_deref().map(read)
    }

    /// The charts watching `series`.
    pub fn watchers(&self, series: &Series) -> Vec<u32> {
        self.watched
            .borrow()
            .get(series)
            .map(|entry| entry.watchers.iter().copied().collect())
            .unwrap_or_default()
    }

    // -- from the socket to the frame --------------------------------------

    /// Fold everything that arrived since the last drain into the series in
    /// memory, and say what changed.
    ///
    /// Once per frame, on the main loop. Every mailbox is asked whether it
    /// is dirty — an atomic load each — and only the dirty ones are locked.
    pub fn drain(&self) -> Drained {
        // Cleared first: a tick landing from here on is a fresh wake.
        self.armed.store(false, Ordering::Release);
        let mut drained = Drained::default();
        let mut scratch = self.scratch.borrow_mut();
        let mut watched = self.watched.borrow_mut();
        let now = Instant::now();
        for (series, entry) in watched.iter_mut() {
            if !entry.mailbox.is_dirty() {
                continue;
            }
            entry.mailbox.take(&mut scratch);

            if let Some(snapshot) = scratch.snapshot.take() {
                // The feed is back, whatever it said before.
                entry.state = State::Opening;
                match &self.writer {
                    Some(writer) => {
                        let memory = entry.bars.take().unwrap_or_default();
                        entry.merging = true;
                        writer.send(Job::Merge { series: series.clone(), snapshot, memory });
                    }
                    None => {
                        let mut bars = entry.bars.take().unwrap_or_default();
                        for bar in snapshot {
                            upsert(&mut bars, bar);
                        }
                        entry.bars = Some(bars);
                        entry.state = State::Live { last_update: now };
                        drained.changes.push(Change::Snapshot(series.clone()));
                    }
                }
            }

            if !scratch.bars.is_empty() {
                match entry.bars.as_mut() {
                    Some(bars) if !entry.merging => {
                        let mut from = usize::MAX;
                        for bar in scratch.bars.drain(..) {
                            from = from.min(upsert(bars, bar));
                        }
                        entry.state = State::Live { last_update: now };
                        if entry.dirty_since.is_none() {
                            entry.dirty_since = Some(now);
                            if !self.flush_scheduled.replace(true) {
                                drained.schedule_flush = true;
                            }
                        }
                        drained.changes.push(Change::Tail { series: series.clone(), from });
                    }
                    // Nothing to apply them to yet: kept for when there is.
                    _ => entry.held.append(&mut scratch.bars),
                }
            }

            if let Some(why) = scratch.lost.take() {
                let attempt = match entry.state {
                    State::Lost { attempt, .. } => attempt + 1,
                    _ => 0,
                };
                let retry_at = retry_after(why, attempt).map(|wait| now + wait);
                entry.state = State::Lost { why, attempt, retry_at };
                // The provider's stream has ended; the handle only holds
                // its thread's last breath.
                entry.handle = None;
                // Always asked for, even with a retry already timed: the new
                // loss may be due sooner, and the window keeps one timer at
                // the earliest of them.
                drained.schedule_retry |= retry_at.is_some();
                drained.changes.push(Change::Lost { series: series.clone(), why });
            }
        }
        drained
    }

    /// The writer handed a merged snapshot back: it is the series now, with
    /// whatever ticks arrived meanwhile on top.
    pub fn merged(&self, series: &Series, bars: Vec<Bar>) -> Option<Change> {
        let mut watched = self.watched.borrow_mut();
        let entry = watched.get_mut(series)?;
        let mut bars = bars;
        for bar in entry.held.drain(..) {
            upsert(&mut bars, bar);
        }
        entry.bars = Some(bars);
        entry.merging = false;
        entry.state = State::Live { last_update: Instant::now() };
        // The merge wrote the snapshot; the held ticks have not been written.
        entry.dirty_since = Some(Instant::now());
        Some(Change::Snapshot(series.clone()))
    }

    // -- the cache ---------------------------------------------------------

    /// Write every series that is ahead of the cache.
    pub fn flush(&self) {
        self.flush_scheduled.set(false);
        let mut watched = self.watched.borrow_mut();
        for (series, entry) in watched.iter_mut() {
            if entry.dirty_since.is_some() {
                self.flush_one(series, entry);
                entry.dirty_since = None;
            }
        }
    }

    fn flush_one(&self, series: &Series, entry: &Watched) {
        if entry.dirty_since.is_none() {
            return;
        }
        if let (Some(writer), Some(bars)) = (&self.writer, &entry.bars) {
            writer.send(Job::Write { series: series.clone(), bars: bars.clone() });
        }
    }

    /// Everything dirty, written, and then everything stopped: the window
    /// is closing, or the feed is being retired.
    ///
    /// The writes are waited for — bounded, because a window closing is not
    /// the moment to hang on a disk. Then every subscription is dropped, so
    /// the provider's threads end, and the writer's channel is closed, so
    /// its thread ends and whatever was listening for its answers ends too.
    /// After this nothing of the registry is running, whoever still holds
    /// a reference to it.
    pub fn shutdown(&self) {
        self.flush();
        if let Some(writer) = &self.writer {
            writer.finish();
        }
        self.watched.borrow_mut().clear();
        self.by_pane.borrow_mut().clear();
    }

    // -- recovery ----------------------------------------------------------

    /// When the earliest retry is due, if any series is waiting for one.
    pub fn next_retry_at(&self) -> Option<Instant> {
        self.watched
            .borrow()
            .values()
            .filter_map(|entry| match entry.state {
                State::Lost { retry_at, .. } => retry_at,
                _ => None,
            })
            .min()
    }

    /// Ask the provider again for every series whose retry is due. True
    /// when something is still waiting for a later retry.
    pub fn retry_due(&self, now: Instant) -> bool {
        let mut watched = self.watched.borrow_mut();
        for (series, entry) in watched.iter_mut() {
            let State::Lost { why, attempt, retry_at: Some(at) } = entry.state else {
                continue;
            };
            if at > now {
                continue;
            }
            let since = entry.bars.as_ref().and_then(|bars| bars.last()).map(|bar| bar.ts);
            match self.open(series, &entry.symbol, since, &entry.mailbox) {
                Ok(handle) => {
                    entry.handle = Some(handle);
                    entry.state = State::Lost { why, attempt, retry_at: None };
                }
                Err(refused) => {
                    let retry_at = retry_after(refused, attempt + 1).map(|wait| now + wait);
                    entry.state = State::Lost { why: refused, attempt: attempt + 1, retry_at };
                }
            }
        }
        drop(watched);
        self.next_retry_at().is_some()
    }

    // -- for provider status -----------------------------------------------

    pub fn report(&self) -> Vec<Report> {
        let now = Instant::now();
        let mut reports: Vec<Report> = self
            .watched
            .borrow()
            .iter()
            .map(|(series, entry)| Report {
                symbol: entry.symbol.clone(),
                native: series.native,
                charts: entry.watchers.len(),
                bars: entry.bars.as_ref().map(Vec::len).unwrap_or(0),
                last_bar: entry.bars.as_ref().and_then(|bars| bars.last()).map(|bar| bar.ts),
                updated_ago: match entry.state {
                    State::Live { last_update } => Some(now.saturating_duration_since(last_update)),
                    _ => None,
                },
                state: match entry.state {
                    State::Opening => ReportState::Opening,
                    State::Live { .. } => ReportState::Live,
                    State::Lost { why, retry_at, .. } => {
                        ReportState::Lost { why, retrying: retry_at.is_some() || entry.handle.is_some() }
                    }
                },
            })
            .collect();
        reports.sort_by(|a, b| (&a.symbol, a.native.seconds()).cmp(&(&b.symbol, b.native.seconds())));
        reports
    }
}

/// How long to wait before asking for a lost series again, or never.
///
/// A failure that is about the request — a symbol the feed does not have,
/// a resolution it does not serve — is final: asking again would get the
/// same answer for ever. Everything else is the network's or the session's
/// doing and is asked again on a doubling wait from a second to a minute.
/// An expired session is retried too, because signing in again is what
/// fixes it and the retry is how the chart notices without a restart; the
/// provider refuses locally until then, at no network cost.
pub fn retry_after(why: FetchFailure, attempt: u32) -> Option<Duration> {
    match why {
        FetchFailure::NoSuchSymbol | FetchFailure::Unsupported | FetchFailure::Unserved => None,
        _ => Some((RETRY_FLOOR * 2u32.saturating_pow(attempt.min(16))).min(RETRY_CEILING)),
    }
}

impl Writer {
    fn start(path: PathBuf, written: async_channel::Sender<Written>) -> Writer {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("omacharts live writer".into())
            .spawn(move || {
                let Ok(store) = Store::open_at(&path) else { return };
                while let Ok(job) = inbox.recv() {
                    let done = match job {
                        Job::Merge { series, snapshot, memory } => {
                            // Memory first, then the snapshot over it: the
                            // snapshot is the feed's latest word on every
                            // bar it covers, and memory keeps whatever older
                            // history was never in a snapshot.
                            let mut fresh = memory;
                            for bar in snapshot {
                                upsert(&mut fresh, bar);
                            }
                            let bars = store.merge_bars(&series.key, series.native, &fresh);
                            Written::Merged { series, bars }
                        }
                        Job::Write { series, bars } => {
                            store.merge_bars(&series.key, series.native, &bars);
                            Written::Flushed { series }
                        }
                        Job::Sync(done) => {
                            let _ = done.send(());
                            continue;
                        }
                    };
                    if written.send_blocking(done).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Writer { jobs: std::sync::Mutex::new(Some(jobs)), thread }
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = jobs.send(job);
        }
    }

    /// Wait for everything queued so far to be written, or for five
    /// seconds, and then let the thread go.
    ///
    /// The queue is in order, so a marker sent after the writes is answered
    /// after them. Dropping the sender afterwards is what ends the thread:
    /// nothing else holds one.
    fn finish(&self) {
        let (done, landed) = mpsc::channel::<()>();
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(jobs) = jobs
            && jobs.send(Job::Sync(done)).is_ok()
        {
            let _ = landed.recv_timeout(Duration::from_secs(5));
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // The thread's loop ends when the last sender is gone, and ours is
        // a field of the thing being dropped — which happens *after* this
        // runs. So it is let go of here, first, or the join below would wait
        // on a loop that is waiting on us.
        self.jobs.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(thread) = self.thread.take() {
            // Waits exactly for what was already queued.
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use omacharts_engine::{
        Capability, Instrument, InstrumentKind, ProviderError, Sink, Stream,
    };

    use super::*;

    /// A provider that streams whatever the test pushes, and records what
    /// was asked of it.
    struct Fake {
        inner: Arc<Mutex<FakeState>>,
    }

    #[derive(Default)]
    struct FakeState {
        /// Every subscribe, in order: symbol, timeframe, since.
        opened: Vec<(String, Timeframe, Option<i64>)>,
        /// The sinks of the subscriptions still open, by symbol and timeframe.
        sinks: Vec<(String, Timeframe, Arc<Sink>)>,
        dropped: usize,
        refuse: Option<ProviderError>,
    }

    struct FakeHandle {
        inner: Arc<Mutex<FakeState>>,
        symbol: String,
        timeframe: Timeframe,
    }

    impl Subscription for FakeHandle {}

    impl Drop for FakeHandle {
        fn drop(&mut self) {
            let mut state = self.inner.lock().unwrap();
            state.sinks.retain(|(s, t, _)| !(s == &self.symbol && *t == self.timeframe));
            state.dropped += 1;
        }
    }

    impl Stream for Fake {
        fn subscribe(
            &self,
            symbol: &str,
            timeframe: Timeframe,
            since: Option<i64>,
            sink: Sink,
        ) -> Result<Box<dyn Subscription>, ProviderError> {
            let mut state = self.inner.lock().unwrap();
            if let Some(refuse) = state.refuse.take() {
                return Err(refuse);
            }
            state.opened.push((symbol.to_string(), timeframe, since));
            state.sinks.push((symbol.to_string(), timeframe, Arc::new(sink)));
            Ok(Box::new(FakeHandle { inner: self.inner.clone(), symbol: symbol.to_string(), timeframe }))
        }
    }

    impl Provider for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn label(&self) -> &'static str {
            "Fake"
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

    fn live() -> (Live, Arc<Mutex<FakeState>>) {
        let inner = Arc::new(Mutex::new(FakeState::default()));
        let provider: Arc<dyn Provider> = Arc::new(Fake { inner: inner.clone() });
        (Live::new(provider, None), inner)
    }

    fn series(symbol: &str, native: Timeframe) -> Series {
        Series { key: format!("fake:{symbol}"), native }
    }

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { ts, open: close, high: close, low: close, close, volume: 1.0 }
    }

    /// Push `update` into the open subscription for `symbol` at `native`,
    /// from this thread, the way a provider's thread would.
    fn push(state: &Mutex<FakeState>, symbol: &str, native: Timeframe, update: Update) {
        let sink = {
            let state = state.lock().unwrap();
            state
                .sinks
                .iter()
                .find(|(s, t, _)| s == symbol && *t == native)
                .map(|(_, _, sink)| sink.clone())
                .expect("an open subscription")
        };
        sink(update);
    }

    const M1: Timeframe = Timeframe::minutes(1);
    const H1: Timeframe = Timeframe::hours(1);

    /// The single shared source: two charts on one series open one
    /// subscription, and a chart on a resolution that folds from the same
    /// native series joins it too.
    #[test]
    fn charts_on_one_series_share_one_subscription() {
        let (live, fake) = live();
        assert_eq!(live.watch(1, series("ES", M1), "ES", None), Watching::Opening);
        assert_eq!(live.watch(2, series("ES", M1), "ES", None), Watching::Opening);
        // Four hours is served as hourly bars: the hourly series is the thing.
        assert_eq!(live.watch(3, series("ES", H1), "ES", None), Watching::Opening);
        assert_eq!(live.watch(4, series("ES", H1), "ES", None), Watching::Opening);
        let opened = &fake.lock().unwrap().opened;
        assert_eq!(opened.len(), 2, "{opened:?}");
        assert_eq!(live.watchers(&series("ES", M1)).len(), 2);
    }

    /// Different things are different subscriptions, and a symbol change
    /// releases the old one while keeping the new.
    #[test]
    fn a_chart_moving_to_another_symbol_releases_the_old_series() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        live.watch(1, series("NQ", M1), "NQ", None);
        assert!(live.watchers(&series("ES", M1)).is_empty());
        assert_eq!(live.watchers(&series("NQ", M1)), vec![1]);
        // Idle, not torn down: the reap does that, later.
        assert_eq!(fake.lock().unwrap().dropped, 0);
        live.reap();
        assert_eq!(fake.lock().unwrap().dropped, 1);
        assert_eq!(fake.lock().unwrap().sinks.len(), 1);
    }

    /// The last chart to stop watching tears the subscription down — on the
    /// next pass, so a chart that comes straight back finds it still open.
    #[test]
    fn the_last_watcher_leaving_tears_down_on_the_next_pass_unless_somebody_returns() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        live.watch(2, series("ES", M1), "ES", None);
        assert!(!live.unwatch(1), "one watcher left");
        assert!(live.unwatch(2), "none left: idle");
        // A chartbook switch: the same series is watched again before the
        // idle pass runs.
        live.watch(7, series("ES", M1), "ES", None);
        live.reap();
        assert_eq!(fake.lock().unwrap().dropped, 0, "nobody re-subscribed and nothing was dropped");
        assert_eq!(fake.lock().unwrap().opened.len(), 1);
        // And when nobody comes back, it goes.
        assert!(live.unwatch_all());
        live.reap();
        assert_eq!(fake.lock().unwrap().dropped, 1);
        assert!(live.watchers(&series("ES", M1)).is_empty());
    }

    /// A thousand ticks are one drain with one tail from one bar, and the
    /// main loop was woken once for the lot.
    #[test]
    fn a_burst_of_ticks_is_one_change_and_one_wake() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        push(&fake, "ES", M1, Update::Snapshot(vec![bar(0, 1.0), bar(60, 2.0)]));
        let drained = live.drain();
        assert_eq!(drained.changes, vec![Change::Snapshot(series("ES", M1))]);
        assert_eq!(live.nudges().try_recv().ok(), Some(()), "the snapshot woke the loop");

        for i in 0..1000 {
            push(&fake, "ES", M1, Update::Bar(bar(60, 2.0 + i as f64)));
        }
        push(&fake, "ES", M1, Update::Bar(bar(120, 5.0)));
        assert_eq!(live.nudges().try_recv().ok(), Some(()), "one wake for the burst");
        assert!(live.nudges().try_recv().is_err(), "and only one");

        let drained = live.drain();
        assert_eq!(drained.changes, vec![Change::Tail { series: series("ES", M1), from: 1 }]);
        assert!(drained.schedule_flush, "the series is now ahead of the cache");
        live.with_bars(&series("ES", M1), |bars| {
            assert_eq!(bars.len(), 3);
            assert_eq!(bars[1].close, 1001.0, "the newest state of the forming bar, and only that");
            assert_eq!(bars[2].ts, 120);
        })
        .expect("bars in memory");
    }

    /// Ticks that arrive before there is a series to apply them to are not
    /// lost: they land on top of the snapshot once it comes.
    #[test]
    fn ticks_ahead_of_the_snapshot_wait_for_it() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        push(&fake, "ES", M1, Update::Bar(bar(60, 9.0)));
        let drained = live.drain();
        assert!(drained.changes.is_empty(), "nothing to draw yet: {:?}", drained.changes);
        push(&fake, "ES", M1, Update::Snapshot(vec![bar(0, 1.0), bar(60, 2.0)]));
        live.drain();
        // The snapshot is the feed's latest word on bar 60: the earlier tick
        // was older than it, and the mailbox folded it away.
        live.with_bars(&series("ES", M1), |bars| assert_eq!(bars[1].close, 2.0)).unwrap();
    }

    /// A loss is reported once, the handle goes, and a retry is scheduled on
    /// a wait that doubles — until a snapshot puts everything back.
    #[test]
    fn a_lost_stream_is_asked_for_again_on_a_doubling_wait() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        push(&fake, "ES", M1, Update::Snapshot(vec![bar(0, 1.0)]));
        live.drain();
        push(&fake, "ES", M1, Update::Lost(FetchFailure::Unreachable));
        let drained = live.drain();
        assert_eq!(
            drained.changes,
            vec![Change::Lost { series: series("ES", M1), why: FetchFailure::Unreachable }]
        );
        assert!(drained.schedule_retry);
        assert_eq!(fake.lock().unwrap().dropped, 1, "the dead stream's handle is gone");
        let first = live.next_retry_at().expect("a retry is due");
        assert!(first <= Instant::now() + RETRY_FLOOR);

        // The retry asks again, from where the series got to.
        assert!(!live.retry_due(first));
        let opened = fake.lock().unwrap().opened.clone();
        assert_eq!(opened.len(), 2);
        assert_eq!(opened[1].2, Some(0), "since the last bar it has");

        // Lost again: the wait doubles.
        push(&fake, "ES", M1, Update::Lost(FetchFailure::Unreachable));
        live.drain();
        let second = live.next_retry_at().expect("another retry");
        assert!(second > Instant::now() + RETRY_FLOOR, "the second wait is longer than the first");

        // A snapshot is the feed back, and the chart is told so.
        live.retry_due(second);
        push(&fake, "ES", M1, Update::Snapshot(vec![bar(0, 1.0), bar(60, 2.0)]));
        let drained = live.drain();
        assert_eq!(drained.changes, vec![Change::Snapshot(series("ES", M1))]);
        assert!(live.next_retry_at().is_none());
        assert_eq!(live.report()[0].state, ReportState::Live);
    }

    /// A symbol the feed does not have is not asked for again, ever.
    #[test]
    fn a_final_refusal_is_never_retried() {
        assert_eq!(retry_after(FetchFailure::NoSuchSymbol, 0), None);
        assert_eq!(retry_after(FetchFailure::Unserved, 3), None);
        assert_eq!(retry_after(FetchFailure::Unreachable, 0), Some(RETRY_FLOOR));
        assert_eq!(retry_after(FetchFailure::NeedsSignIn, 1), Some(RETRY_FLOOR * 2));
        assert_eq!(retry_after(FetchFailure::Offline, 40), Some(RETRY_CEILING));

        let (live, fake) = live();
        live.watch(1, series("ZZZZ", M1), "ZZZZ", None);
        push(&fake, "ZZZZ", M1, Update::Lost(FetchFailure::NoSuchSymbol));
        let drained = live.drain();
        assert!(!drained.schedule_retry);
        assert!(live.next_retry_at().is_none());
        assert!(matches!(live.report()[0].state, ReportState::Lost { retrying: false, .. }));
    }

    /// Retiring the registry ends every subscription: the feed's threads
    /// are what a switch has to leave none of.
    #[test]
    fn shutting_down_drops_every_subscription() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        live.watch(2, series("NQ", M1), "NQ", None);
        live.shutdown();
        assert_eq!(fake.lock().unwrap().dropped, 2);
        assert!(fake.lock().unwrap().sinks.is_empty());
        assert!(live.report().is_empty());
    }

    /// A provider that refuses before any request leaves nothing behind.
    #[test]
    fn a_refusal_on_subscribe_is_reported_and_watches_nothing() {
        let (live, fake) = live();
        fake.lock().unwrap().refuse = Some(ProviderError::Unsupported("4h".into()));
        assert_eq!(
            live.watch(1, series("ES", Timeframe::hours(4)), "ES", None),
            Watching::Refused(FetchFailure::Unsupported)
        );
        assert!(live.report().is_empty());
        assert!(!live.unwatch(1));
    }

    /// A second chart on a series that is already live paints from memory
    /// at once; nothing is fetched and nothing is waited for.
    #[test]
    fn joining_a_live_series_paints_from_memory() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        push(&fake, "ES", M1, Update::Snapshot(vec![bar(0, 1.0)]));
        live.drain();
        assert_eq!(live.watch(2, series("ES", M1), "ES", None), Watching::Showing);
        assert!(live.is_showing(&series("ES", M1)));
        assert_eq!(fake.lock().unwrap().opened.len(), 1);
    }

    /// What `provider status` says, and that it says nothing has ticked
    /// when nothing has, rather than implying data is flowing.
    #[test]
    fn the_report_says_what_is_held_and_whether_anything_has_arrived() {
        let (live, fake) = live();
        live.watch(1, series("ES", M1), "ES", None);
        live.watch(2, series("ES", M1), "ES", None);
        let report = live.report();
        assert_eq!(report.len(), 1);
        assert_eq!((report[0].charts, report[0].bars, report[0].state.clone()), (2, 0, ReportState::Opening));
        assert!(report[0].updated_ago.is_none());
        push(&fake, "ES", M1, Update::Snapshot(vec![bar(0, 1.0), bar(60, 2.0)]));
        live.drain();
        let report = live.report();
        assert_eq!((report[0].bars, report[0].last_bar), (2, Some(60)));
        assert!(report[0].updated_ago.is_some());
    }

    /// With a database behind it, a snapshot goes through the writer and
    /// comes back merged with what the cache held, and ticks are written
    /// behind rather than drawn from the cache.
    #[test]
    fn a_snapshot_is_merged_with_the_cache_and_ticks_are_written_behind() {
        let dir = std::env::temp_dir().join(format!("omacharts-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("omacharts.db");
        let store = Store::open_at(&path).unwrap();
        store.write_bars("fake:ES", M1, &[bar(-60, 0.5), bar(0, 1.0)]);

        let inner = Arc::new(Mutex::new(FakeState::default()));
        let provider: Arc<dyn Provider> = Arc::new(Fake { inner: inner.clone() });
        let live = Live::new(provider, Some(path.clone()));
        live.watch(1, series("ES", M1), "ES", Some(0));
        assert_eq!(inner.lock().unwrap().opened[0].2, Some(0), "the cache's edge is passed on");

        push(&inner, "ES", M1, Update::Snapshot(vec![bar(0, 1.5), bar(60, 2.0)]));
        let drained = live.drain();
        assert!(drained.changes.is_empty(), "the snapshot is with the writer");
        let Written::Merged { series: merged_series, bars } =
            live.written().recv_blocking().expect("the writer answers")
        else {
            panic!("a merge first");
        };
        assert_eq!(merged_series, series("ES", M1));
        assert_eq!(bars.iter().map(|b| b.ts).collect::<Vec<_>>(), vec![-60, 0, 60]);
        assert_eq!(bars[1].close, 1.5, "the snapshot wins over the cache");
        assert_eq!(live.merged(&merged_series, bars), Some(Change::Snapshot(series("ES", M1))));
        assert_eq!(store.load_bars("fake:ES", M1).len(), 3, "and the cache has the merge");

        // A tick, then a flush: the cache catches up.
        push(&inner, "ES", M1, Update::Bar(bar(60, 2.5)));
        live.drain();
        assert_eq!(store.load_bars("fake:ES", M1)[2].close, 2.0, "not written per tick");
        live.flush();
        assert!(matches!(live.written().recv_blocking(), Ok(Written::Flushed { .. })));
        assert_eq!(store.load_bars("fake:ES", M1)[2].close, 2.5);

        live.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
