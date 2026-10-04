//! Fetching bars off the UI thread.
//!
//! The window never waits on the network. It paints whatever the cache holds,
//! asks for the gap, and repaints when the answer arrives. A failure is not an
//! empty chart — it is the same chart, saying what went wrong. On a fresh
//! install there is no chart to keep, so the reason is all there is, which is
//! why it travels rather than being thrown away here.
//!
//! The same thread fills in the rest of a chart's resolution strip once the
//! chart somebody asked for has arrived, so that the first click on 15m is a
//! repaint rather than a wait. One request at a time, each earned by the last
//! one landing — see [`Loader::warm_strip`].
//!
//! The worker opens its own [`Store`]: a rusqlite connection is `Send` but not
//! `Sync`, and WAL means a writer here never blocks the reader there.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use omacharts_engine::{Bar, FetchFailure, Provider, Timeframe};

use crate::store::Store;

/// How many already-cached bars a tail fetch deliberately re-requests.
///
/// Yahoo serves split- and dividend-adjusted prices, so a split rewrites an
/// instrument's whole history behind our back and the cache silently stops
/// matching. Overlapping a few bars catches that the moment it happens, for
/// the price of a handful of comparisons.
const OVERLAP: i64 = 5;

/// Prices agreeing to within this fraction are the same price. Yahoo's
/// adjusted values wobble in the last decimal place between responses.
const TOLERANCE: f64 = 0.0005;

#[derive(Clone, PartialEq, Debug)]
pub struct Request {
    /// True when nobody asked for this yet. Providers pace speculative work
    /// further apart and refuse it outright while being throttled, so filling
    /// the rail can never cost someone the chart they are looking at.
    pub speculative: bool,
    /// Cache key: `provider:symbol`.
    pub key: String,
    /// The provider's own spelling.
    pub symbol: String,
    /// What the chart is showing. Fetched at `timeframe.native()`.
    pub timeframe: Timeframe,
}

pub enum Response {
    /// Bars at the *native* timeframe. The caller folds to what it is showing.
    Bars { key: String, timeframe: Timeframe, bars: Vec<Bar> },
    /// Nothing new arrived. `bars` is whatever the cache already had, and
    /// `failure` is why nothing joined it.
    ///
    /// The reason travels rather than a flag, because the window has to say
    /// it out loud: a chart with no cached bars at all is the first thing a
    /// new user sees, and "could not fetch" and "the provider has nothing"
    /// are not the same sentence. This used to be a string nobody read and a
    /// `rate_limited` bool, which is how every failure ended up looking like
    /// a symbol with no data.
    ///
    /// `unasked` is true when nobody was waiting for this — the window went
    /// and asked on a timer, to keep a chart from going stale. Such a failure
    /// still travels, because a refusal is the one thing that must stop the
    /// timer offering more, but it must not put words on a chart whose user
    /// asked for nothing: "rate limited" on a chart somebody is reading
    /// happily is a sentence they then have to have explained to them.
    Failed {
        key: String,
        timeframe: Timeframe,
        bars: Vec<Bar>,
        failure: FetchFailure,
        unasked: bool,
    },
}

/// The chart you are looking at. Jumps every queued prefetch.
pub const FOREGROUND: u32 = 0;

/// Where speculative work starts, leaving room beneath it for the symbols
/// immediately around the selection.
pub const BACKGROUND: u32 = 1_000;

/// A standing to-do list: symbols with nothing cached at all, which the rail
/// has no price to draw for until something fetches them.
///
/// Below every positional priority, and — unlike those — not forgotten when
/// the selection moves. A prefetch is a guess about where you are going next
/// and stops being true the moment you go somewhere else; a symbol with no
/// data is missing data wherever you are, so dropping it would mean a dash
/// that only fills in if you stop arrowing.
pub const BACKFILL: u32 = 2_000_000;

/// A resolution the chart on screen could be switched to and has no bars for.
///
/// Above every positional priority, because somebody arrowing through the rail
/// is going somewhere: the symbols a keypress away are worth more than the
/// other buttons on the strip of the symbol being left. Below [`BACKFILL`],
/// because a backfill is a standing list that is never forgotten and can
/// therefore afford to wait, while a warm-up is only true while this symbol is
/// on screen — queued behind forty rows with no price it would never run.
///
/// Dropped when the selection moves, unlike the backfill. Which resolution
/// somebody might click is a guess about the symbol they are looking at, and
/// they have just stopped looking at it.
pub const WARM: u32 = 1_000_000;

/// A chart on screen whose bars have gone stale, fetched again on a timer.
///
/// Behind [`WARM`], because warming is a bounded first-visit cost — three
/// requests for a symbol and then silence for ever — while a refresh comes
/// back every few minutes for as long as the chart stays open. A strip still
/// filling in belongs to somebody about to click a resolution; a chart a
/// quarter of an hour old can wait the couple of seconds that takes.
///
/// Ahead of [`BACKFILL`], because this is the chart the user is actually
/// looking at, and the backfill is a standing list that is never forgotten
/// and can therefore afford to go last.
///
/// Dropped when the selection moves, like a warm-up and for the same reason:
/// it was a statement about the chart that was on screen, and that is the
/// thing which just changed. Nothing has to resume it — the timer offers it
/// again on its next tick if it still matters, which is also what lets it
/// come back after the provider has refused one.
pub const REFRESH: u32 = 1_500_000;

/// Fetches bars on one worker thread, nearest-wanted first.
///
/// One thread, not one per request, for two reasons. The provider is paced —
/// requests are deliberately kept apart — so concurrency would only queue
/// inside the throttle anyway. And a queue can be reordered: when you arrow
/// onto a different symbol, the chart you are now looking at must overtake
/// the twenty prefetches queued behind it, not wait for them.
pub struct Loader {
    inner: Arc<Inner>,
}

struct Inner {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// Shared with the worker, which gives up on the rest of a strip itself
    /// when the one request it tried was refused.
    warming: Mutex<Warming>,
}

struct Queue {
    jobs: Vec<Job>,
    /// Breaks ties so equal priorities keep the order they were asked in.
    next_seq: u64,
    shutdown: bool,
    /// The request nobody asked for that is being fetched right now, so that
    /// somebody asking for that very series joins it instead of starting a
    /// second fetch for it. A warm-up or a refresh — see [`on_a_charts_behalf`].
    warming_now: Option<Claim>,
}

/// A request made on a chart's behalf, in flight, and whether anybody has
/// come to want it.
struct Claim {
    key: String,
    /// The series being fetched, which is what a derived resolution folds
    /// from: clicking 4h joins the hourly already on its way.
    native: Timeframe,
    /// Set when somebody asks for this series for real while it is in flight.
    /// Its reply is their reply from then on — bars or failure.
    claimed: bool,
}

struct Job {
    request: Request,
    priority: u32,
    seq: u64,
}

impl Loader {
    pub fn new<P>(provider: P, sender: async_channel::Sender<Response>) -> Loader
    where
        P: Provider + 'static,
    {
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue {
                jobs: Vec::new(),
                next_seq: 0,
                shutdown: false,
                warming_now: None,
            }),
            wake: Condvar::new(),
            warming: Mutex::new(Warming::default()),
        });

        let worker = inner.clone();
        std::thread::spawn(move || {
            while let Some(job) = worker.take() {
                let mut response = run(&job.request, &provider);
                if !worker.finished(job.priority, &mut response) {
                    continue;
                }
                // The window closing before we finish is normal, not an error.
                if sender.send_blocking(response).is_err() {
                    break;
                }
            }
        });

        Loader { inner }
    }

    /// Queue a fetch. Lower `priority` runs first.
    ///
    /// Asking again for something already queued keeps the better priority
    /// rather than queueing it twice — which is what happens constantly as you
    /// arrow down a watchlist and the same neighbours keep being re-offered.
    pub fn fetch(&self, request: Request, priority: u32) {
        let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        let queued = enqueue(&mut queue, request, priority);
        drop(queue);
        if queued {
            self.inner.wake.notify_one();
        }
    }

    /// Forget everything that is not the chart on screen.
    ///
    /// Called when the selection moves: the old neighbours are no longer the
    /// nearest ones, and leaving them queued would spend the request budget on
    /// symbols that are now far away.
    ///
    /// [`BACKFILL`] work survives, because it was never about position.
    ///
    /// The warm-up plan goes with them, for the same reason and so that a
    /// request dropped from under it cannot leave the chain waiting for ever
    /// on a reply that is never coming.
    pub fn drop_prefetches(&self) {
        let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.jobs.retain(survives_a_move);
        drop(queue);
        *self.warm_plan() = Warming::default();
    }

    /// Forget the standing backfill.
    ///
    /// For a 429: the provider is now refusing everything speculative, so a
    /// hundred queued symbols would be a hundred instant refusals and a
    /// hundred pointless repaints. Dropping them and asking again later is
    /// the same answer the provider's own cooldown gives, applied one level up.
    pub fn drop_backfill(&self) {
        let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.jobs.retain(|job| job.priority < BACKFILL);
    }

    pub fn queued(&self) -> usize {
        self.inner.queue.lock().unwrap_or_else(|e| e.into_inner()).jobs.len()
    }

    // -- warming the strip -------------------------------------------------

    /// Plan to fill in the resolutions this chart's strip offers and the cache
    /// does not have, and queue none of them yet.
    ///
    /// Call it when a chart is put on screen. Nothing leaves here: the first
    /// request goes out when [`Self::keep_warming`] says the chart somebody
    /// actually asked for has arrived. That is the whole protection — a chart
    /// that failed to load warms nothing, and a symbol passed through on the
    /// way down a watchlist never starts, because the next move cancels the
    /// plan before its own load ever finished.
    ///
    /// `cached` answers "is this series already on disk" and is asked again
    /// before every request, because warming is a first-visit cost only: a
    /// resolution already fetched is already instant and must never be asked
    /// for twice.
    pub fn warm_strip(
        &self,
        key: &str,
        symbol: &str,
        strip: &[Timeframe],
        showing: Timeframe,
        cached: impl Fn(Timeframe) -> bool,
    ) {
        let pending = warming_order(strip, showing)
            .into_iter()
            .filter(|timeframe| !cached(*timeframe))
            .collect();
        *self.warm_plan() =
            Warming { key: key.to_string(), symbol: symbol.to_string(), pending, in_flight: None };
    }

    /// A reply for `key` landed; ask for the next resolution, if there is one.
    ///
    /// One request at a time, and only ever the next one, so that everything
    /// which could make the rest pointless — the symbol moving on, the chart
    /// closing, a 429, the user clicking a resolution themselves — stops this
    /// after the request in flight rather than after a queue of six.
    pub fn keep_warming(&self, key: &str, arrived: Timeframe, cached: impl Fn(Timeframe) -> bool) {
        let request = {
            let mut warming = self.warm_plan();
            match warming.advance(key, arrived, &cached) {
                Some(timeframe) => Request {
                    speculative: true,
                    key: warming.key.clone(),
                    symbol: warming.symbol.clone(),
                    timeframe,
                },
                None => return,
            }
        };
        self.fetch(request, WARM);
    }

    /// Give up on the rest of `key`'s strip.
    ///
    /// For a load that failed. Asking for five more things is the worst
    /// possible answer to a provider that has just refused us or a symbol it
    /// says it does not have. Nothing resumes it, and nothing needs to: the
    /// next time somebody lands here whatever did land is free, and the rest
    /// starts again from there.
    pub fn stop_warming(&self, key: &str) {
        self.warm_plan().stop(key);
    }

    fn warm_plan(&self) -> std::sync::MutexGuard<'_, Warming> {
        self.inner.warming.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Filling in the rest of one chart's resolution strip, one request at a time.
///
/// The shape is deliberate. Queueing the whole strip when a chart loads is a
/// queue of requests for a symbol the user has already left by the time it
/// drains — and every one of them holds the provider's two-second speculative
/// gap, so forty rows arrowed through would be hundreds of requests and
/// minutes of pacing spent on charts nobody opened. Asking for the next only
/// when the last one has replied means there is never anything queued ahead to
/// cancel, and walking a watchlist costs nothing.
#[derive(Default, Debug)]
struct Warming {
    /// The chart this plan belongs to. A reply for any other is not ours.
    key: String,
    symbol: String,
    /// Still worth asking for, nearest to what is on screen first.
    pending: VecDeque<Timeframe>,
    /// Asked for and not yet heard about. At most one, which is the point.
    in_flight: Option<Timeframe>,
}

impl Warming {
    /// What to ask for next, now that `arrived` has come back for `key`.
    ///
    /// `None` whenever there is nothing to do: a reply for another chart, a
    /// request of ours still outstanding, or a strip with nothing left on it.
    fn advance(
        &mut self,
        key: &str,
        arrived: Timeframe,
        cached: &impl Fn(Timeframe) -> bool,
    ) -> Option<Timeframe> {
        if self.key != key {
            return None;
        }
        if self.in_flight == Some(arrived) {
            self.in_flight = None;
        }
        // Something else for this chart is still out — the rail's daily, say.
        // One at a time means waiting for it rather than racing it.
        if self.in_flight.is_some() {
            return None;
        }
        while let Some(next) = self.pending.pop_front() {
            // The cache is re-read here and not only when the plan was made,
            // because the user may have clicked this very resolution in the
            // meantime and already have it.
            if !cached(next) {
                self.in_flight = Some(next);
                return Some(next);
            }
        }
        None
    }

    /// Forget the rest of `key`'s strip, leaving a reply still in flight for
    /// it with nowhere to continue to.
    fn stop(&mut self, key: &str) {
        if self.key == key {
            *self = Warming::default();
        }
    }
}

/// Which series to fetch for a strip showing `showing`, in the order to ask.
///
/// Three things at once, and the first is why this is not just a list of the
/// strip. Resolutions are *folded* from a smaller set than the strip shows: 1W
/// is cut from the daily series and 4h from the hourly, so the six default
/// buttons are four series, and a chart on 1D already holds everything 1W
/// needs. Deduplicating by [`Timeframe::native`] is what turns a six-request
/// warm-up into a three-request one.
///
/// Then nearest-first, because warming is incremental and the early ones are
/// the ones that will be there when somebody clicks: from 1D the next click is
/// 4h or 1W long before it is 5m. And ties go to the coarser side, which has
/// fewer bars, more history available, and more often than not folds from a
/// series already in hand.
///
/// Expects the strip in the order the header shows it, which is by length.
fn warming_order(strip: &[Timeframe], showing: Timeframe) -> Vec<Timeframe> {
    let native = showing.native();
    let anchor =
        strip.iter().position(|t| t.seconds() >= showing.seconds()).unwrap_or(strip.len());
    // A typed resolution need not be on the strip at all, in which case it
    // sits between two buttons and both of them are one step away.
    let listed = strip.get(anchor).is_some_and(|t| t.seconds() == showing.seconds());

    let mut ranked: Vec<(usize, Timeframe)> = strip
        .iter()
        .enumerate()
        .map(|(at, timeframe)| {
            let steps = if at >= anchor {
                at - anchor + usize::from(!listed)
            } else {
                anchor - at
            };
            (steps, *timeframe)
        })
        .collect();
    ranked.sort_by_key(|(steps, timeframe)| (*steps, std::cmp::Reverse(timeframe.seconds())));

    let mut order: Vec<Timeframe> = Vec::new();
    for (_, timeframe) in ranked {
        let fetched = timeframe.native();
        // The series the chart is already loading, and the buttons that fold
        // from a series an earlier button has already claimed.
        if fetched == native || order.contains(&fetched) {
            continue;
        }
        order.push(fetched);
    }
    order
}

impl Drop for Loader {
    fn drop(&mut self) {
        let mut queue = self.inner.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.shutdown = true;
        drop(queue);
        self.inner.wake.notify_all();
    }
}

impl Inner {
    /// Block until there is a job, then hand back the most wanted one.
    fn take(&self) -> Option<Job> {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if queue.shutdown {
                return None;
            }
            if let Some(at) = best(&queue.jobs) {
                let job = queue.jobs.remove(at);
                queue.warming_now = on_a_charts_behalf(job.priority).then(|| Claim {
                    key: job.request.key.clone(),
                    native: job.request.timeframe.native(),
                    claimed: false,
                });
                return Some(job);
            }
            queue = self.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// A job is done. Forget any claim on it, and say whether its reply is
    /// worth sending on.
    ///
    /// A warm-up that was refused and that nobody came to want gives up on
    /// the rest of the strip here rather than letting the window decide,
    /// because the window is never told: whatever refused one speculative
    /// request will refuse the next five, and a chart nobody is waiting on
    /// has nothing to say about it.
    fn finished(&self, priority: u32, response: &mut Response) -> bool {
        let claimed = {
            let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.warming_now.take().is_some_and(|claim| claim.claimed)
        };
        let warm = priority == WARM;
        let failed = matches!(response, Response::Failed { .. });
        if warm && failed && !claimed {
            *self.warming.lock().unwrap_or_else(|e| e.into_inner()) = Warming::default();
        }
        // Say whether anybody was waiting for this, which is not the same
        // question as which band it was queued in: a resolution somebody
        // clicked while it was being fetched for them is their request now,
        // and its answer is theirs to be told.
        if let Response::Failed { unasked, .. } = response {
            *unasked = on_a_charts_behalf(priority) && !claimed;
        }
        !warm || warm_up_is_worth_reporting(failed, claimed)
    }
}

/// Was this fetched for a chart already on screen, without anybody asking?
///
/// [`WARM`] and [`REFRESH`] both were, and the distinction earns its keep
/// because they share two pieces of behaviour. A request somebody makes for
/// the same series joins the one already in flight rather than racing it. And
/// a failure reaches the window marked as nobody's news, because the chart in
/// question is on screen, looks perfectly fine, and its user asked for
/// nothing — so a marker in its corner would be a sentence somebody then has
/// to have explained to them.
///
/// [`BACKFILL`] is deliberately not one of these, though nobody asked for
/// that either. It fetches for the *rail*, on behalf of rows showing a dash,
/// and the rail does say when it cannot fill them in.
fn on_a_charts_behalf(priority: u32) -> bool {
    matches!(priority, WARM | REFRESH)
}

/// Is a finished warm-up's reply worth delivering to the window?
///
/// A refused warm-up nobody asked for is told to nobody. The chart on screen
/// is fine and the rail is fine; a failure message about a request the user
/// never made is a message about nothing, and it was the one way this feature
/// could put words on screen that somebody then has to explain.
///
/// A warm-up somebody clicked in the meantime is their request now, so its
/// answer is theirs — including "rate limited", which is exactly what they
/// would have been shown had the resolution never been warmed at all.
fn warm_up_is_worth_reporting(failed: bool, claimed: bool) -> bool {
    claimed || !failed
}

/// Add a job, or improve the one already there. True when the worker has
/// something new to wake up for.
fn enqueue(queue: &mut Queue, request: Request, priority: u32) -> bool {
    // Somebody has asked for the resolution a warm-up is already fetching.
    // Join that reply instead of queueing a second request for the same
    // series: one fetch, whose answer — bars or failure — is now the chart's,
    // so clicking 15m while 15m is being warmed looks like any other click.
    if !request.speculative
        && let Some(claim) = queue.warming_now.as_mut()
        && claim.key == request.key
        && claim.native == request.timeframe.native()
    {
        claim.claimed = true;
        return false;
    }
    if let Some(existing) = queue
        .jobs
        .iter_mut()
        .find(|j| j.request.key == request.key && j.request.timeframe == request.timeframe)
    {
        existing.priority = existing.priority.min(priority);
        // The flag decides whether the provider holds this two seconds apart
        // and refuses it outright while throttled, so a request somebody is
        // now waiting on must stop being speculative. Keeping the flag it was
        // queued with is how warming 15m, and then clicking 15m, turned the
        // click into a refusal.
        existing.request.speculative &= request.speculative;
        return false;
    }
    let seq = queue.next_seq;
    queue.next_seq += 1;
    queue.jobs.push(Job { request, priority, seq });
    true
}

/// Is this job still worth having once the selection has moved?
///
/// The chart on screen is, because it is the thing that moved. The backfill is,
/// because it was never about where you were. Everything between the two is a
/// guess about the old neighbourhood and has just stopped being true.
fn survives_a_move(job: &Job) -> bool {
    job.priority == FOREGROUND || job.priority >= BACKFILL
}

/// Index of the job to run next: lowest priority, then earliest asked.
fn best(jobs: &[Job]) -> Option<usize> {
    jobs.iter()
        .enumerate()
        .min_by_key(|(_, job)| (job.priority, job.seq))
        .map(|(at, _)| at)
}

/// Fetch one request and say what came back.
///
/// Leaves `unasked` false on a failure: whether anybody is waiting for this
/// depends on what has happened to the queue since it went out, which only
/// [`Inner::finished`] is in a position to know.
fn run<P: Provider>(request: &Request, provider: &P) -> Response {
    let fetch = |symbol: &str, timeframe, since| {
        if request.speculative {
            provider.bars_speculative(symbol, timeframe, since)
        } else {
            provider.bars(symbol, timeframe, since)
        }
    };
    let native = request.timeframe.native();
    let Ok(store) = Store::open() else {
        return Response::Failed {
            key: request.key.clone(),
            timeframe: native,
            bars: Vec::new(),
            failure: FetchFailure::LocalCache,
            unasked: false,
        };
    };

    let cached = store.load_bars(&request.key, native);
    let coverage = store.coverage(&request.key, native);

    // Ask only for the gap. A fresh series asks for everything; an existing
    // one asks from a few bars before where it ends.
    let since = coverage.map(|c| c.last_ts - OVERLAP * native.seconds());

    match fetch(&request.symbol, native, since) {
        Ok(fresh) if fresh.is_empty() => {
            // Nothing new, which is still an answer. Recording that we asked
            // is what stops a chart on a timer asking again on every tick for
            // as long as the provider has nothing to add.
            store.mark_fetched(&request.key, native);
            Response::Bars { key: request.key.clone(), timeframe: native, bars: cached }
        }
        Ok(fresh) => {
            // If the overlap disagrees, the cached history was adjusted out
            // from under us and cannot be trusted. Start over.
            if !cached.is_empty() && !overlap_agrees(&cached, &fresh) {
                store.drop_series(&request.key, native);
                let full = match fetch(&request.symbol, native, None) {
                    Ok(full) => full,
                    // We dropped the cache and could not refill it; report
                    // honestly rather than showing a series we know is stale.
                    Err(error) => {
                        return Response::Failed {
                            key: request.key.clone(),
                            timeframe: native,
                            bars: Vec::new(),
                            failure: FetchFailure::from(&error),
                            unasked: false,
                        }
                    }
                };
                store.write_bars(&request.key, native, &full);
                return Response::Bars { key: request.key.clone(), timeframe: native, bars: full };
            }

            let merged = store.merge_bars(&request.key, native, &fresh);
            Response::Bars { key: request.key.clone(), timeframe: native, bars: merged }
        }
        Err(error) => Response::Failed {
            key: request.key.clone(),
            timeframe: native,
            bars: cached,
            failure: FetchFailure::from(&error),
            unasked: false,
        },
    }
}

/// Do the bars we already had still match what the provider just sent for the
/// same timestamps?
///
/// Only completed bars count. The newest cached bar may have been forming when
/// we stored it, so it is expected to differ and is excluded.
fn overlap_agrees(cached: &[Bar], fresh: &[Bar]) -> bool {
    let newest_complete = cached.last().map(|b| b.ts).unwrap_or(i64::MIN);
    let mut compared = 0;
    for bar in fresh {
        if bar.ts >= newest_complete {
            continue;
        }
        if let Ok(at) = cached.binary_search_by_key(&bar.ts, |b| b.ts) {
            compared += 1;
            if !close(cached[at].close, bar.close) || !close(cached[at].open, bar.open) {
                return false;
            }
        }
    }
    // No shared completed bars means nothing to contradict.
    let _ = compared;
    true
}

fn close(a: f64, b: f64) -> bool {
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() / scale <= TOLERANCE
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { ts, open: close, high: close, low: close, close, volume: 1.0 }
    }

    #[test]
    fn an_unchanged_overlap_agrees() {
        let cached = vec![bar(100, 10.0), bar(200, 11.0), bar(300, 12.0)];
        let fresh = vec![bar(100, 10.0), bar(200, 11.0), bar(300, 99.0), bar(400, 13.0)];
        // 300 is the newest cached bar and may have been forming, so its
        // disagreement is expected and ignored.
        assert!(overlap_agrees(&cached, &fresh));
    }

    #[test]
    fn a_split_adjusted_history_disagrees() {
        let cached = vec![bar(100, 100.0), bar(200, 110.0), bar(300, 120.0)];
        // A 10:1 split rewrites every completed bar.
        let fresh = vec![bar(100, 10.0), bar(200, 11.0), bar(300, 12.0)];
        assert!(!overlap_agrees(&cached, &fresh));
    }

    #[test]
    fn rounding_noise_is_not_a_split() {
        let cached = vec![bar(100, 100.0), bar(200, 110.0), bar(300, 120.0)];
        let fresh = vec![bar(100, 100.00001), bar(200, 109.99998), bar(300, 120.0)];
        assert!(overlap_agrees(&cached, &fresh));
    }

    #[test]
    fn no_shared_history_is_not_a_disagreement() {
        let cached = vec![bar(100, 10.0)];
        let fresh = vec![bar(500, 50.0), bar(600, 60.0)];
        assert!(overlap_agrees(&cached, &fresh));
    }

    fn job(key: &str, priority: u32, seq: u64) -> Job {
        Job {
            request: Request {
                key: key.into(),
                symbol: key.into(),
                timeframe: Timeframe::days(1),
                speculative: false,
            },
            priority,
            seq,
        }
    }

    #[test]
    fn the_most_wanted_job_runs_first() {
        let jobs = vec![job("far", 9, 0), job("near", 1, 1), job("chart", FOREGROUND, 2)];
        assert_eq!(best(&jobs), Some(2), "the chart on screen jumps the queue");

        let jobs = vec![job("far", 9, 0), job("near", 1, 1)];
        assert_eq!(best(&jobs), Some(1), "then the nearest neighbour");
    }

    #[test]
    fn equal_priorities_keep_their_order() {
        let jobs = vec![job("b", 3, 7), job("a", 3, 2)];
        assert_eq!(best(&jobs), Some(1), "asked for first, so run first");
    }

    /// Moving the selection forgets the neighbourhood, because that is what
    /// changed. A symbol with nothing cached is still missing it.
    #[test]
    fn moving_on_forgets_prefetches_but_keeps_the_backfill() {
        let queued = [
            job("chart", FOREGROUND, 0),
            job("neighbour", 2, 1),
            job("neighbour-daily", BACKGROUND + 2, 2),
            job("never-fetched", BACKFILL, 3),
        ];
        let kept: Vec<&str> = queued
            .iter()
            .filter(|job| survives_a_move(job))
            .map(|job| job.request.key.as_str())
            .collect();
        assert_eq!(kept, ["chart", "never-fetched"]);
    }

    #[test]
    fn the_backfill_runs_last_of_everything() {
        let jobs = vec![job("never-fetched", BACKFILL, 0), job("neighbour-daily", BACKGROUND, 1)];
        assert_eq!(best(&jobs), Some(1), "a guess about where you are going still wins");
    }

    #[test]
    fn an_empty_queue_has_nothing_to_run() {
        assert_eq!(best(&[]), None);
    }

    #[test]
    fn closeness_scales_with_magnitude() {
        assert!(close(1.0, 1.0000001));
        assert!(close(20000.0, 20000.5));
        assert!(!close(1.0, 1.5));
        assert!(!close(100.0, 10.0));
    }

    // -- warming the strip -------------------------------------------------

    fn tf(key: &str) -> Timeframe {
        Timeframe::parse(key).expect(key)
    }

    fn keys(listed: &[Timeframe]) -> Vec<String> {
        listed.iter().map(|t| t.key()).collect()
    }

    /// The reason warming the default strip is three requests and not six: 1W
    /// folds from the daily series the chart is already holding, and 4h folds
    /// from the hourly that 1h asks for.
    #[test]
    fn the_weekly_comes_free_from_the_daily_the_chart_already_has() {
        let order = warming_order(&Timeframe::PRESETS, tf("1D"));
        assert_eq!(keys(&order), ["1h", "15m", "5m"]);
    }

    #[test]
    fn the_resolutions_nearest_what_is_on_screen_are_warmed_first() {
        let order = warming_order(&Timeframe::PRESETS, tf("15m"));
        assert_eq!(keys(&order), ["1h", "5m", "1D"], "one step out, then two, then three");
    }

    /// Both ends of the strip are reachable from a button in the middle of it.
    #[test]
    fn warming_works_outwards_in_both_directions() {
        let order = warming_order(&Timeframe::PRESETS, tf("1h"));
        assert_eq!(keys(&order), ["15m", "1D", "5m"], "4h and 1W fold from 1h and 1D");
    }

    /// Nothing says a resolution has to be on the strip — "3m" is typed, folds
    /// from one-minute bars, and still has neighbours on either side of it.
    #[test]
    fn a_typed_resolution_that_is_not_on_the_strip_still_has_neighbours() {
        let order = warming_order(&Timeframe::PRESETS, tf("3m"));
        assert_eq!(keys(&order), ["5m", "15m", "1h", "1D"]);
    }

    #[test]
    fn a_strip_showing_only_what_is_on_screen_warms_nothing() {
        assert!(warming_order(&[tf("1D"), tf("1W")], tf("1D")).is_empty());
        assert!(warming_order(&[], tf("1D")).is_empty());
    }

    fn nothing_cached(_: Timeframe) -> bool {
        false
    }

    fn aimed_at_the_default_strip() -> Warming {
        Warming {
            key: "yahoo:AAPL".into(),
            symbol: "AAPL".into(),
            pending: warming_order(&Timeframe::PRESETS, tf("1D")).into(),
            in_flight: None,
        }
    }

    /// The user's rule: warming starts once a symbol has actually loaded in a
    /// chart. A plan on its own queues nothing, which is what makes arrowing
    /// past a symbol free — the move cancels the plan before its load lands.
    #[test]
    fn warming_starts_only_when_the_chart_somebody_asked_for_arrives() {
        let mut warming = aimed_at_the_default_strip();
        assert_eq!(warming.in_flight, None, "the plan alone asks for nothing");

        let first = warming.advance("yahoo:AAPL", tf("1D"), &nothing_cached);
        assert_eq!(first, Some(tf("1h")), "the daily landed, so the hourly can go");
    }

    #[test]
    fn only_one_warm_up_is_ever_outstanding() {
        let mut warming = aimed_at_the_default_strip();
        assert_eq!(warming.advance("yahoo:AAPL", tf("1D"), &nothing_cached), Some(tf("1h")));

        // A reply for something else about this symbol — the rail's daily, a
        // second pane — must not start a second one alongside the hourly.
        assert_eq!(warming.advance("yahoo:AAPL", tf("1D"), &nothing_cached), None);
        assert_eq!(warming.in_flight, Some(tf("1h")));

        assert_eq!(
            warming.advance("yahoo:AAPL", tf("1h"), &nothing_cached),
            Some(tf("15m")),
            "the hourly came back, so the next one goes"
        );
    }

    #[test]
    fn warming_works_down_the_strip_and_then_stops() {
        let mut warming = aimed_at_the_default_strip();
        let mut asked = Vec::new();
        let mut arrived = tf("1D");
        while let Some(next) = warming.advance("yahoo:AAPL", arrived, &nothing_cached) {
            asked.push(next);
            arrived = next;
        }
        assert_eq!(keys(&asked), ["1h", "15m", "5m"]);
        assert_eq!(
            warming.advance("yahoo:AAPL", arrived, &nothing_cached),
            None,
            "a warmed symbol goes quiet rather than going round again"
        );
    }

    /// Warming is a first-visit cost. A resolution already on disk is already
    /// instant, and asking for it again would turn "the first time only" into
    /// every time without anybody noticing.
    #[test]
    fn a_resolution_the_cache_already_holds_is_never_asked_for() {
        let mut warming = aimed_at_the_default_strip();
        let have_the_intraday = |timeframe: Timeframe| timeframe.is_intraday();
        assert_eq!(
            warming.advance("yahoo:AAPL", tf("1D"), &have_the_intraday),
            None,
            "everything left on the strip is cached, so there is nothing to do"
        );
    }

    /// The user clicking 15m while we were working towards it: the click
    /// fetches it in the foreground, and we must not fetch it again behind.
    #[test]
    fn a_resolution_fetched_while_warming_is_dropped_from_the_plan() {
        let mut warming = aimed_at_the_default_strip();
        assert_eq!(warming.advance("yahoo:AAPL", tf("1D"), &nothing_cached), Some(tf("1h")));
        let fifteen_arrived = |timeframe: Timeframe| timeframe == tf("15m");
        assert_eq!(
            warming.advance("yahoo:AAPL", tf("1h"), &fifteen_arrived),
            Some(tf("5m")),
            "15m is on disk now, so the plan skips past it"
        );
    }

    #[test]
    fn a_reply_for_another_symbol_warms_nothing() {
        let mut warming = aimed_at_the_default_strip();
        assert_eq!(warming.advance("yahoo:MSFT", tf("1D"), &nothing_cached), None);
        assert_eq!(warming.in_flight, None);
    }

    /// Queueing five more requests is the worst possible answer to a provider
    /// that has just refused one, or a symbol it says it does not have.
    #[test]
    fn a_load_that_failed_warms_nothing() {
        let mut warming = aimed_at_the_default_strip();
        warming.stop("yahoo:AAPL");
        assert_eq!(warming.advance("yahoo:AAPL", tf("1D"), &nothing_cached), None);
    }

    #[test]
    fn a_failure_somewhere_else_leaves_this_plan_alone() {
        let mut warming = aimed_at_the_default_strip();
        warming.stop("yahoo:MSFT");
        assert_eq!(warming.advance("yahoo:AAPL", tf("1D"), &nothing_cached), Some(tf("1h")));
    }

    /// Where [`WARM`] sits decides two things, and both of them matter.
    #[test]
    fn a_warm_up_waits_for_the_neighbourhood_but_not_for_the_backfill() {
        let jobs = vec![job("warm", WARM, 0), job("neighbour", BACKGROUND + 2, 1)];
        assert_eq!(best(&jobs), Some(1), "the symbol a keypress away comes first");

        let jobs = vec![job("standing-backfill", BACKFILL, 0), job("warm", WARM, 1)];
        assert_eq!(best(&jobs), Some(1), "but a list that is never forgotten can wait");
    }

    /// A warm-up is a guess about the symbol on screen, so leaving it stops it
    /// being true — unlike the backfill, which was never about position.
    #[test]
    fn moving_on_forgets_the_warm_ups() {
        assert!(!survives_a_move(&job("warm", WARM, 0)));
    }

    /// The bug this prevents: 15m is queued as a warm-up, the user clicks 15m,
    /// and the click is served by a request still flagged speculative — so the
    /// provider paces it two seconds out, or refuses it outright while it is
    /// being throttled. The thing somebody is waiting for is never speculative.
    fn empty_queue() -> Queue {
        Queue { jobs: Vec::new(), next_seq: 0, shutdown: false, warming_now: None }
    }

    /// A warm-up still waiting its turn, clicked: the job is already there, so
    /// it is the same fetch promoted rather than a second one.
    #[test]
    fn clicking_a_resolution_being_warmed_stops_it_being_speculative() {
        let mut queue = empty_queue();
        let request = |speculative| Request {
            speculative,
            key: "yahoo:AAPL".into(),
            symbol: "AAPL".into(),
            timeframe: tf("15m"),
        };

        assert!(enqueue(&mut queue, request(true), WARM), "queued, so wake the worker");
        assert!(!enqueue(&mut queue, request(false), FOREGROUND), "the same job, improved");

        assert_eq!(queue.jobs.len(), 1, "asked for twice, fetched once");
        assert_eq!(queue.jobs[0].priority, FOREGROUND);
        assert!(!queue.jobs[0].request.speculative);
    }

    #[test]
    fn a_warm_up_behind_a_foreground_fetch_does_not_demote_it() {
        let mut queue = empty_queue();
        let request = |speculative| Request {
            speculative,
            key: "yahoo:AAPL".into(),
            symbol: "AAPL".into(),
            timeframe: tf("15m"),
        };
        enqueue(&mut queue, request(false), FOREGROUND);
        enqueue(&mut queue, request(true), WARM);
        assert_eq!(queue.jobs[0].priority, FOREGROUND);
        assert!(!queue.jobs[0].request.speculative);
    }

    fn warming_now(native: Timeframe) -> Queue {
        let mut queue = empty_queue();
        queue.warming_now = Some(Claim { key: "yahoo:AAPL".into(), native, claimed: false });
        queue
    }

    fn wanted(timeframe: Timeframe) -> Request {
        Request {
            speculative: false,
            key: "yahoo:AAPL".into(),
            symbol: "AAPL".into(),
            timeframe,
        }
    }

    /// The overlap the user has to be able to trust: 15m is halfway down the
    /// wire as a warm-up and they press 15m. One fetch, not two racing, and
    /// its reply is the chart's reply — so what they see is a load and then
    /// bars, exactly as if the resolution had never been warmed.
    #[test]
    fn clicking_a_resolution_being_warmed_joins_its_fetch_rather_than_racing_it() {
        let mut queue = warming_now(tf("15m"));
        assert!(!enqueue(&mut queue, wanted(tf("15m")), FOREGROUND), "nothing new to run");
        assert!(queue.jobs.is_empty(), "a second fetch for the same series is not queued");
        assert!(queue.warming_now.as_ref().expect("still in flight").claimed);
    }

    /// 4h is cut from the hourly series, so clicking it while the hourly is
    /// being warmed is the same fetch under another name.
    #[test]
    fn clicking_a_derived_resolution_joins_the_series_it_folds_from() {
        let mut queue = warming_now(tf("1h"));
        assert!(!enqueue(&mut queue, wanted(tf("4h")), FOREGROUND));
        assert!(queue.warming_now.as_ref().expect("still in flight").claimed);
    }

    #[test]
    fn a_warm_up_for_another_resolution_is_not_joined() {
        let mut queue = warming_now(tf("15m"));
        assert!(enqueue(&mut queue, wanted(tf("5m")), FOREGROUND), "its own fetch");
        assert_eq!(queue.jobs.len(), 1);
        assert!(!queue.warming_now.as_ref().expect("still in flight").claimed);
    }

    /// Only a request somebody is waiting on claims one. The next warm-up in
    /// the chain is still a guess, and a guess must not inherit a chart's
    /// right to be told that a fetch failed.
    #[test]
    fn one_warm_up_does_not_claim_another() {
        let mut queue = warming_now(tf("15m"));
        let speculative = Request { speculative: true, ..wanted(tf("15m")) };
        enqueue(&mut queue, speculative, WARM);
        assert!(!queue.warming_now.as_ref().expect("still in flight").claimed);
    }

    /// The one way this feature could put words on a screen that nobody asked
    /// for: a warm-up refused by the provider while the user is on 1D, saying
    /// "rate limited" about a request they never made.
    #[test]
    fn a_warm_up_that_failed_and_nobody_wanted_says_nothing() {
        assert!(!warm_up_is_worth_reporting(true, false));
    }

    #[test]
    fn a_warm_up_somebody_clicked_reports_whatever_happened_to_it() {
        assert!(warm_up_is_worth_reporting(true, true), "their request, their answer");
        assert!(warm_up_is_worth_reporting(false, true));
    }

    /// Bars are always worth having: they are what the next click is for.
    #[test]
    fn a_warm_up_that_worked_is_always_delivered() {
        assert!(warm_up_is_worth_reporting(false, false));
    }

    // -- refreshing a chart on screen --------------------------------------

    /// Where [`REFRESH`] sits decides what a stale chart is allowed to wait
    /// for, and the answer is: a strip still filling in, and nothing else.
    #[test]
    fn a_refresh_waits_for_a_warm_up_and_is_still_ahead_of_the_backfill() {
        let jobs = vec![job("stale-chart", REFRESH, 0), job("warming", WARM, 1)];
        assert_eq!(best(&jobs), Some(1), "a strip about to be clicked goes first");

        let jobs = vec![job("standing-backfill", BACKFILL, 0), job("stale-chart", REFRESH, 1)];
        assert_eq!(best(&jobs), Some(1), "but the chart on screen beats a list that can wait");
    }

    /// The thing a refresh must never do: hold up a chart somebody has only
    /// just asked for.
    #[test]
    fn a_refresh_never_delays_the_chart_somebody_asked_for() {
        let jobs = vec![job("stale-chart", REFRESH, 0), job("asked-for", FOREGROUND, 1)];
        assert_eq!(best(&jobs), Some(1));
    }

    /// A refresh is a statement about the chart that was on screen, so
    /// leaving it stops it being true — exactly like a warm-up. Nothing has
    /// to resume it: the timer offers it again on its next tick.
    #[test]
    fn moving_on_forgets_a_refresh() {
        assert!(!survives_a_move(&job("stale-chart", REFRESH, 0)));
    }

    #[test]
    fn the_bands_fetched_on_a_charts_behalf_are_the_warm_up_and_the_refresh() {
        assert!(on_a_charts_behalf(WARM));
        assert!(on_a_charts_behalf(REFRESH));
        assert!(!on_a_charts_behalf(FOREGROUND), "somebody is waiting for this one");
        assert!(!on_a_charts_behalf(BACKGROUND + 3), "a guess about where they are going");
        assert!(!on_a_charts_behalf(BACKFILL), "the rail's, and the rail does say");
    }

    fn with_queue(queue: Queue) -> Inner {
        Inner {
            queue: Mutex::new(queue),
            wake: Condvar::new(),
            warming: Mutex::new(Warming::default()),
        }
    }

    fn a_refusal() -> Response {
        Response::Failed {
            key: "yahoo:AAPL".into(),
            timeframe: tf("1D"),
            bars: Vec::new(),
            failure: FetchFailure::RateLimited,
            unasked: false,
        }
    }

    fn unasked_of(response: &Response) -> bool {
        match response {
            Response::Failed { unasked, .. } => *unasked,
            Response::Bars { .. } => panic!("expected a failure"),
        }
    }

    /// Both halves matter, and they pull in opposite directions. The window
    /// has to *hear* that a refresh was refused, because that refusal is the
    /// only thing which stops the timer offering the same request again in
    /// thirty seconds. And it must not *show* it, because the chart is fine
    /// and its user asked for nothing.
    #[test]
    fn a_refused_refresh_is_delivered_and_marked_as_nobodys_news() {
        let inner = with_queue(empty_queue());
        let mut response = a_refusal();
        assert!(inner.finished(REFRESH, &mut response), "the timer has to be told");
        assert!(unasked_of(&response), "but the chart must not say so");
    }

    /// Unlike a warm-up, which is dropped on the floor — there is no timer
    /// behind one to stop, because the worker abandons the rest of the strip
    /// itself.
    #[test]
    fn a_refused_warm_up_nobody_wanted_is_not_delivered_at_all() {
        let inner = with_queue(empty_queue());
        let mut response = a_refusal();
        assert!(!inner.finished(WARM, &mut response));
    }

    #[test]
    fn a_failure_somebody_was_waiting_for_is_never_marked_unasked() {
        let inner = with_queue(empty_queue());
        let mut response = a_refusal();
        assert!(inner.finished(FOREGROUND, &mut response));
        assert!(!unasked_of(&response), "their request, their answer");
    }

    /// Somebody asked for the very series being refreshed while the fetch was
    /// still in flight. It is their request from then on, so its failure is
    /// theirs to be told about, in the words any other failed fetch uses.
    #[test]
    fn a_refresh_somebody_asked_for_midway_reports_to_them_like_any_other_fetch() {
        let mut queue = warming_now(tf("1D"));
        queue.warming_now.as_mut().expect("in flight").claimed = true;
        let inner = with_queue(queue);

        let mut response = a_refusal();
        assert!(inner.finished(REFRESH, &mut response));
        assert!(!unasked_of(&response));
    }
}
