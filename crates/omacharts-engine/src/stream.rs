//! What a stream's ticks become between the socket and the screen.
//!
//! A provider that streams calls its [`crate::Sink`] once per print, from its
//! own thread, as fast as the feed goes. A chart draws once per frame, on the
//! main loop, and nothing like as fast. Everything in this module is about
//! the gap between the two, and it is all plain data so that it can be tested
//! without a socket on one side or a window on the other.
//!
//! Three pieces. The [`Mailbox`] is where a provider's thread puts what it
//! heard: it keeps the newest state of each bar and nothing else, which is
//! the whole of the backpressure — a thousand ticks on one forming bar are one
//! pending bar, and a snapshot discards everything before it. [`upsert`] is
//! how a bar joins a series: replacing the one with its timestamp, appending
//! when it is newer, which is the common case and costs nothing. And
//! [`tail_start`] says how far back a derived resolution has to look to refold
//! the bucket a changed native bar belongs to, so that a four-hour bar built
//! from hourly bars is replaced rather than appended to.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::bars::{bucket_of, Bar, Timeframe};
use crate::provider::{FetchFailure, Update};

/// What a stream has delivered and nobody has drawn yet.
///
/// The provider's thread writes it; the main loop takes it. The lock is
/// uncontended in practice — the main loop holds it for a swap — and a tick
/// after the first costs no allocation: a bar with the same timestamp as the
/// last pending one is overwritten in place, and the vector keeps its
/// capacity across takes.
#[derive(Default)]
pub struct Mailbox {
    pending: Mutex<Pending>,
    /// Set by a push, cleared by a take, so the drain can skip a mailbox
    /// with nothing in it without touching the lock.
    dirty: AtomicBool,
}

/// What was waiting, in the order it has to be applied: the snapshot first
/// if there is one, then the bars, and the loss last.
#[derive(Default, Debug, PartialEq)]
pub struct Pending {
    /// A whole series, superseding everything that came before it.
    pub snapshot: Option<Vec<Bar>>,
    /// Bars newer than the snapshot, if any, oldest first, one per
    /// timestamp.
    pub bars: Vec<Bar>,
    /// Why nothing newer is coming, if the provider said so.
    pub lost: Option<FetchFailure>,
}

impl Pending {
    pub fn is_empty(&self) -> bool {
        self.snapshot.is_none() && self.bars.is_empty() && self.lost.is_none()
    }
}

impl Mailbox {
    pub fn new() -> Mailbox {
        Mailbox::default()
    }

    /// Fold `update` into what is waiting. True when the mailbox was empty
    /// before, which is the one moment a wake is worth sending: a mailbox
    /// that is already dirty is already going to be drained.
    pub fn push(&self, update: Update) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        match update {
            // Everything before a snapshot is older than it, the loss
            // included: a feed that managed a snapshot is back.
            Update::Snapshot(bars) => {
                pending.snapshot = Some(bars);
                pending.bars.clear();
                pending.lost = None;
            }
            // Into the snapshot if one is waiting — it is the series as of a
            // moment ago, and this bar is newer — otherwise beside the other
            // bars, where the same timestamp is the same bar.
            Update::Bar(bar) => match pending.snapshot.as_mut() {
                Some(snapshot) => {
                    upsert(snapshot, bar);
                }
                None => {
                    upsert(&mut pending.bars, bar);
                }
            },
            Update::Lost(why) => pending.lost = Some(why),
        }
        !self.dirty.swap(true, Ordering::AcqRel)
    }

    /// Is anything waiting? Cheap enough to ask of every mailbox on every
    /// drain.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// Take everything that is waiting, leaving the mailbox empty.
    ///
    /// Swapped with `into` rather than returned, so the vector the bars were
    /// in — and the one they go into — keep their capacity from one frame
    /// to the next. `into` is cleared first; whatever it held is gone.
    pub fn take(&self, into: &mut Pending) {
        into.snapshot = None;
        into.bars.clear();
        into.lost = None;
        // Cleared before the swap, so a push that lands between the two is
        // seen as a fresh dirtying and wakes the next drain rather than being
        // read as already handled.
        self.dirty.store(false, Ordering::Release);
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::swap(&mut *pending, into);
    }
}

/// Put `bar` into `series`, which is ordered by timestamp with one bar per
/// timestamp: replacing the bar it matches, or going where it belongs.
///
/// Returns the index it landed at. The common case — the forming bar again,
/// or the next one — is the last slot or a push, and costs nothing; a bar
/// older than the end is a binary search, for the correction a feed
/// occasionally sends.
pub fn upsert(series: &mut Vec<Bar>, bar: Bar) -> usize {
    match series.last() {
        Some(last) if last.ts == bar.ts => {
            let at = series.len() - 1;
            series[at] = bar;
            at
        }
        Some(last) if last.ts < bar.ts => {
            series.push(bar);
            series.len() - 1
        }
        None => {
            series.push(bar);
            0
        }
        Some(_) => match series.binary_search_by_key(&bar.ts, |b| b.ts) {
            Ok(at) => {
                series[at] = bar;
                at
            }
            Err(at) => {
                series.insert(at, bar);
                at
            }
        },
    }
}

/// Where the tail a chart has to refold begins, given that `native[from..]`
/// changed.
///
/// For a resolution the series is served at, it is `from`. For one folded
/// from it, it is the first native bar of the bucket `native[from]` falls
/// in, because the folded bar has to be rebuilt from every native bar it
/// covers — the three earlier hours of a four-hour bar are not in the tick
/// that moved its close. That is at most a bucket's worth of native bars,
/// which is a handful.
pub fn tail_start(native: &[Bar], from: usize, shown: Timeframe, origin: i64) -> usize {
    if !shown.is_derived() || from >= native.len() {
        return from;
    }
    let bucket = bucket_of(native[from].ts, shown, origin);
    let mut start = from;
    while start > 0 && bucket_of(native[start - 1].ts, shown, origin) == bucket {
        start -= 1;
    }
    start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { ts, open: close, high: close, low: close, close, volume: 1.0 }
    }

    fn drained(mailbox: &Mailbox) -> Pending {
        let mut pending = Pending::default();
        mailbox.take(&mut pending);
        pending
    }

    /// The backpressure rule in one test: however many ticks land on the
    /// forming bar, what waits is one bar, holding the last of them.
    #[test]
    fn a_thousand_ticks_on_one_bar_are_one_pending_bar() {
        let mailbox = Mailbox::new();
        for i in 0..1000 {
            mailbox.push(Update::Bar(bar(60, i as f64)));
        }
        let pending = drained(&mailbox);
        assert_eq!(pending.bars, vec![bar(60, 999.0)]);
        assert!(pending.snapshot.is_none());
    }

    /// The first push is the one worth waking the main loop for. The rest
    /// land in a mailbox that is already going to be drained, and saying so
    /// again would be a wake per tick — the thing the mailbox exists to
    /// avoid.
    #[test]
    fn only_the_first_push_since_a_take_asks_for_a_wake() {
        let mailbox = Mailbox::new();
        assert!(mailbox.push(Update::Bar(bar(60, 1.0))));
        assert!(!mailbox.push(Update::Bar(bar(60, 2.0))));
        assert!(!mailbox.push(Update::Bar(bar(120, 3.0))));
        assert!(mailbox.is_dirty());
        drained(&mailbox);
        assert!(!mailbox.is_dirty());
        assert!(mailbox.push(Update::Bar(bar(120, 4.0))));
    }

    /// A bar that closed and the one that opened after it are two bars, in
    /// order, however they arrived.
    #[test]
    fn bars_with_different_timestamps_wait_in_order() {
        let mailbox = Mailbox::new();
        mailbox.push(Update::Bar(bar(120, 2.0)));
        mailbox.push(Update::Bar(bar(60, 1.0)));
        mailbox.push(Update::Bar(bar(180, 3.0)));
        let pending = drained(&mailbox);
        assert_eq!(pending.bars.iter().map(|b| b.ts).collect::<Vec<_>>(), vec![60, 120, 180]);
    }

    /// Everything before a snapshot is older than it. Bars that were
    /// waiting are discarded; a loss that was waiting is forgotten, because
    /// a feed that just managed a snapshot is back.
    #[test]
    fn a_snapshot_supersedes_whatever_was_waiting() {
        let mailbox = Mailbox::new();
        mailbox.push(Update::Bar(bar(60, 1.0)));
        mailbox.push(Update::Lost(FetchFailure::Unreachable));
        mailbox.push(Update::Snapshot(vec![bar(0, 0.0), bar(60, 5.0)]));
        let pending = drained(&mailbox);
        assert_eq!(pending.snapshot, Some(vec![bar(0, 0.0), bar(60, 5.0)]));
        assert!(pending.bars.is_empty());
        assert!(pending.lost.is_none());
    }

    /// A bar that arrives after a snapshot is newer than it, and folds into
    /// it rather than waiting beside it — so what the drain applies is one
    /// series, not a series and then a correction to it.
    #[test]
    fn a_bar_after_a_snapshot_joins_the_snapshot() {
        let mailbox = Mailbox::new();
        mailbox.push(Update::Snapshot(vec![bar(0, 0.0), bar(60, 5.0)]));
        mailbox.push(Update::Bar(bar(60, 6.0)));
        mailbox.push(Update::Bar(bar(120, 7.0)));
        let pending = drained(&mailbox);
        assert_eq!(pending.snapshot, Some(vec![bar(0, 0.0), bar(60, 6.0), bar(120, 7.0)]));
        assert!(pending.bars.is_empty());
    }

    /// Bars that arrived before the loss are real and are kept; the loss
    /// is remembered beside them, so the chart draws them and then says why
    /// nothing newer is coming.
    #[test]
    fn a_loss_is_kept_beside_the_bars_that_came_first() {
        let mailbox = Mailbox::new();
        mailbox.push(Update::Bar(bar(60, 1.0)));
        mailbox.push(Update::Lost(FetchFailure::NeedsSignIn));
        let pending = drained(&mailbox);
        assert_eq!(pending.bars, vec![bar(60, 1.0)]);
        assert_eq!(pending.lost, Some(FetchFailure::NeedsSignIn));
    }

    #[test]
    fn taking_leaves_nothing_behind() {
        let mailbox = Mailbox::new();
        mailbox.push(Update::Bar(bar(60, 1.0)));
        drained(&mailbox);
        assert!(drained(&mailbox).is_empty());
    }

    /// The vector that takes the bars keeps the capacity it had: the drain
    /// is on the main loop once a frame, and a fresh allocation there every
    /// time would be the per-frame cost the swap exists to avoid.
    #[test]
    fn taking_keeps_the_capacity_of_the_vector_it_takes_into() {
        let mailbox = Mailbox::new();
        let mut pending = Pending { bars: Vec::with_capacity(64), ..Pending::default() };
        mailbox.push(Update::Bar(bar(60, 1.0)));
        mailbox.take(&mut pending);
        assert_eq!(pending.bars.len(), 1);
        // And the mailbox now holds the 64-slot vector, so the next thousand
        // pushes allocate nothing either.
        for i in 0..40 {
            mailbox.push(Update::Bar(bar(60 * (i + 2), 1.0)));
        }
        mailbox.take(&mut pending);
        assert_eq!(pending.bars.len(), 40);
        assert!(pending.bars.capacity() >= 64);
    }

    #[test]
    fn upserting_the_forming_bar_replaces_it_in_place() {
        let mut series = vec![bar(0, 1.0), bar(60, 2.0)];
        assert_eq!(upsert(&mut series, bar(60, 3.0)), 1);
        assert_eq!(series, vec![bar(0, 1.0), bar(60, 3.0)]);
    }

    #[test]
    fn upserting_a_newer_bar_appends_it() {
        let mut series = vec![bar(0, 1.0), bar(60, 2.0)];
        assert_eq!(upsert(&mut series, bar(120, 3.0)), 2);
        assert_eq!(series.len(), 3);
    }

    /// A correction to an older bar lands on that bar, and a bar for a gap
    /// the series had goes into the gap — never on the end, where it would
    /// put the series out of order.
    #[test]
    fn upserting_an_older_bar_finds_its_place() {
        let mut series = vec![bar(0, 1.0), bar(120, 3.0)];
        assert_eq!(upsert(&mut series, bar(0, 9.0)), 0);
        assert_eq!(series[0].close, 9.0);
        assert_eq!(upsert(&mut series, bar(60, 2.0)), 1);
        assert_eq!(series.iter().map(|b| b.ts).collect::<Vec<_>>(), vec![0, 60, 120]);
    }

    #[test]
    fn upserting_into_an_empty_series_starts_it() {
        let mut series = Vec::new();
        assert_eq!(upsert(&mut series, bar(60, 1.0)), 0);
        assert_eq!(series.len(), 1);
    }

    /// Hourly bars at 09:00, 10:00, 11:00 and 12:00, shown as four-hour bars
    /// from midnight: the tick that moved 11:00 means refolding from 08:00's
    /// bucket, which here begins at 09:00.
    #[test]
    fn a_derived_tail_starts_at_the_bucket_the_changed_bar_is_in() {
        let native: Vec<Bar> = (9..=12).map(|h| bar(h * 3_600, 1.0)).collect();
        assert_eq!(tail_start(&native, 2, Timeframe::hours(4), 0), 0);
        // 12:00 is the first hour of the next bucket.
        assert_eq!(tail_start(&native, 3, Timeframe::hours(4), 0), 3);
    }

    /// A resolution served as itself has nothing to refold: the tail is the
    /// changed bar and whatever came after it.
    #[test]
    fn a_native_tail_starts_where_the_change_did() {
        let native: Vec<Bar> = (9..=12).map(|h| bar(h * 3_600, 1.0)).collect();
        assert_eq!(tail_start(&native, 2, Timeframe::hours(1), 0), 2);
    }

    /// The bucket is the same one the whole-series fold would use, origin
    /// included: futures count four-hour bars from 18:00, and a tail that
    /// counted from midnight would refold the wrong three hours.
    #[test]
    fn the_tail_honours_the_session_origin() {
        let native: Vec<Bar> = (17..=21).map(|h| bar(h * 3_600, 1.0)).collect();
        let origin = 18 * 3_600;
        // 21:00 is in the bucket that began at 18:00 — index 1, not 17:00.
        assert_eq!(tail_start(&native, 4, Timeframe::hours(4), origin), 1);
        assert_eq!(bucket_of(21 * 3_600, Timeframe::hours(4), origin), 18 * 3_600);
    }

    #[test]
    fn a_tail_past_the_end_is_left_alone() {
        let native: Vec<Bar> = (9..=12).map(|h| bar(h * 3_600, 1.0)).collect();
        assert_eq!(tail_start(&native, 7, Timeframe::hours(4), 0), 7);
    }
}
