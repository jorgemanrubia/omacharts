//! Records folded into bars.
//!
//! The engine never charts raw ticks: it asks for bars of some width, and
//! this is where they are made, straight off the mapped records. A pass is
//! one forward walk with no allocation but the bars themselves, and the
//! bucket a record falls in is worked out only when its time leaves the
//! current bucket, not once per record, which is what keeps it at the speed
//! memory can deliver records.
//!
//! A tick's price is its `close`: in a tick file `high` and `low` are the
//! ask and bid, never trade prices, and `open` is a marker. A record that
//! is already a bar contributes its own open, high, low and close. Either
//! way a record whose close is not a positive number is skipped.
//!
//! Above [`PARALLEL_THRESHOLD`] records the slice is cut into one piece per
//! core and each piece folded on its own thread. Where a bucket straddles
//! two pieces its two halves are joined: the earlier half's open, the later
//! half's close, the wider extremes, the summed volumes. Nothing is
//! accumulated in floating point (prices are only compared, volumes are
//! integers), so the result is identical to the serial fold, bit for bit.

use crate::scid::{FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE, LAST_SUB_TRADE_OF_UNBUNDLED_TRADE, Record, SC_UNIX_OFFSET_MICROS};
use crate::settings::Scaler;

/// Above this many records a fold runs on every core.
pub const PARALLEL_THRESHOLD: usize = 2_000_000;

/// A bar, in real prices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// The start of the bar's bucket, unix seconds.
    pub start: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Contracts traded.
    pub volume: u64,
    /// Contracts that traded at the bid.
    pub bid_volume: u64,
    /// Contracts that traded at the ask.
    pub ask_volume: u64,
    /// Trades, as the records counted them.
    pub trades: u64,
}

impl Bar {
    fn merge(&mut self, later: &Bar) {
        self.high = self.high.max(later.high);
        self.low = self.low.min(later.low);
        self.close = later.close;
        self.volume += later.volume;
        self.bid_volume += later.bid_volume;
        self.ask_volume += later.ask_volume;
        self.trades += later.trades;
    }
}

/// Bars `step_seconds` wide, aligned to multiples of the step since the
/// unix epoch (so one-minute bars start on the minute, daily ones at
/// midnight UTC), from records in time order. `multiplier` turns stored
/// prices into real ones.
pub fn bars(records: &[Record], step_seconds: i64, multiplier: f64) -> Vec<Bar> {
    assert!(step_seconds > 0, "a bar is at least a second wide");
    run(records, multiplier, &Step(step_seconds), PARALLEL_THRESHOLD)
}

/// Bars over buckets of any shape: `bucket` takes a record's unix second
/// and returns the start of the bucket it belongs to, for instance the
/// start of its trading day. It is called once per distinct second, not
/// per record. Records must be in time order, and `bucket` must not go
/// backwards as time goes forwards.
pub fn bars_with(records: &[Record], multiplier: f64, bucket: impl Fn(i64) -> i64 + Sync) -> Vec<Bar> {
    run(records, multiplier, &ByFunction(bucket), PARALLEL_THRESHOLD)
}

/// [`bars`] on one thread, whatever the size.
pub fn bars_serial(records: &[Record], step_seconds: i64, multiplier: f64) -> Vec<Bar> {
    assert!(step_seconds > 0, "a bar is at least a second wide");
    fold(records, Scaler::new(multiplier), &Step(step_seconds))
}

/// [`bars`] cut into this many pieces, whatever the size. For tests and
/// benchmarks.
pub fn bars_in_pieces(records: &[Record], step_seconds: i64, multiplier: f64, pieces: usize) -> Vec<Bar> {
    parallel(records, Scaler::new(multiplier), &Step(step_seconds), pieces)
}

/// Which bucket a record's time is in, and over what range of Sierra
/// Chart times the answer stays the same.
trait Bucketing: Sync {
    /// The bucket start in unix seconds, and the half-open range of
    /// Sierra Chart timestamps sharing it.
    fn locate(&self, datetime: i64) -> (i64, i64, i64);
}

struct Step(i64);

impl Bucketing for Step {
    #[inline]
    fn locate(&self, datetime: i64) -> (i64, i64, i64) {
        let secs = (datetime - SC_UNIX_OFFSET_MICROS).div_euclid(1_000_000);
        let start = secs.div_euclid(self.0) * self.0;
        let lo = start * 1_000_000 + SC_UNIX_OFFSET_MICROS;
        (start, lo, lo + self.0 * 1_000_000)
    }
}

struct ByFunction<F>(F);

impl<F: Fn(i64) -> i64 + Sync> Bucketing for ByFunction<F> {
    #[inline]
    fn locate(&self, datetime: i64) -> (i64, i64, i64) {
        let secs = (datetime - SC_UNIX_OFFSET_MICROS).div_euclid(1_000_000);
        let lo = secs * 1_000_000 + SC_UNIX_OFFSET_MICROS;
        ((self.0)(secs), lo, lo + 1_000_000)
    }
}

fn run(records: &[Record], multiplier: f64, bucketing: &impl Bucketing, threshold: usize) -> Vec<Bar> {
    let scaler = Scaler::new(multiplier);
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if records.len() <= threshold || threads == 1 {
        fold(records, scaler, bucketing)
    } else {
        parallel(records, scaler, bucketing, threads)
    }
}

fn parallel(records: &[Record], scaler: Scaler, bucketing: &impl Bucketing, pieces: usize) -> Vec<Bar> {
    let pieces = pieces.clamp(1, records.len().max(1));
    let size = records.len().div_ceil(pieces);
    let folded: Vec<Vec<Bar>> = std::thread::scope(|s| {
        let handles: Vec<_> = records.chunks(size.max(1)).map(|piece| s.spawn(move || fold(piece, scaler, bucketing))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut out: Vec<Bar> = Vec::with_capacity(folded.iter().map(Vec::len).sum());
    for piece in folded {
        let mut rest = piece.as_slice();
        if let (Some(last), Some(first)) = (out.last_mut(), rest.first())
            && last.start == first.start
        {
            last.merge(first);
            rest = &rest[1..];
        }
        out.extend_from_slice(rest);
    }
    out
}

/// A bar being built, still in stored prices: scaling by a positive
/// multiplier keeps order, so the extremes can be found before scaling and
/// only a finished bar's four prices are ever scaled.
#[derive(Clone, Copy)]
struct Building {
    start: i64,
    open: f32,
    high: f32,
    low: f32,
    close: f32,
    volume: u64,
    bid_volume: u64,
    ask_volume: u64,
    trades: u64,
}

impl Building {
    fn finish(&self, scaler: Scaler) -> Bar {
        Bar {
            start: self.start,
            open: scaler.apply(self.open),
            high: scaler.apply(self.high),
            low: scaler.apply(self.low),
            close: scaler.apply(self.close),
            volume: self.volume,
            bid_volume: self.bid_volume,
            ask_volume: self.ask_volume,
            trades: self.trades,
        }
    }
}

#[inline(always)]
fn positive(p: f32) -> bool {
    p > 0.0 && p.is_finite()
}

fn fold(records: &[Record], scaler: Scaler, bucketing: &impl Bucketing) -> Vec<Bar> {
    let mut out: Vec<Bar> = Vec::new();
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    let mut b = Building { start: 0, open: 0.0, high: 0.0, low: 0.0, close: 0.0, volume: 0, bid_volume: 0, ask_volume: 0, trades: 0 };
    let mut have = false;
    for r in records {
        let close = r.close;
        if !positive(close) {
            continue;
        }
        // Worked out without branches: in a tick file a third of the
        // records carry an unbundled-trade marker, in no pattern a branch
        // predictor could learn.
        let tick = (r.open == 0.0) | (r.open == FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE) | (r.open == LAST_SUB_TRADE_OF_UNBUNDLED_TRADE);
        let (open, high, low) = if tick {
            (close, close, close)
        } else {
            let or_close = |v: f32| if positive(v) { v } else { close };
            (or_close(r.open), or_close(r.high).max(close), or_close(r.low).min(close))
        };
        if r.datetime < lo || r.datetime >= hi {
            let (start, l, h) = bucketing.locate(r.datetime);
            (lo, hi) = (l, h);
            if !have || start != b.start {
                if have {
                    out.push(b.finish(scaler));
                }
                b = Building {
                    start,
                    open,
                    high,
                    low,
                    close,
                    volume: u64::from(r.volume),
                    bid_volume: u64::from(r.bid_volume),
                    ask_volume: u64::from(r.ask_volume),
                    trades: u64::from(r.trades),
                };
                have = true;
                continue;
            }
        }
        b.high = b.high.max(high);
        b.low = b.low.min(low);
        b.close = close;
        b.volume += u64::from(r.volume);
        b.bid_volume += u64::from(r.bid_volume);
        b.ask_volume += u64::from(r.ask_volume);
        b.trades += u64::from(r.trades);
    }
    if have {
        out.push(b.finish(scaler));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::trade;

    /// Synthetic ticks a few milliseconds apart, wandering in price, with
    /// the odd bad record and bar record mixed in.
    fn synthetic(n: usize) -> Vec<Record> {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut price = 700_000i64;
        let mut micros = 1_789_084_800_000_000i64;
        (0..n)
            .map(|i| {
                let r = next();
                micros += (r % 40_000) as i64;
                price += (r >> 20) as i64 % 51 - 25;
                let mut rec = trade(0, price as f32, 1 + (r >> 40) as u32 % 5, r & 1 == 0);
                rec.datetime = crate::sc_from_unix_micros(micros);
                match i % 9973 {
                    0 => rec.close = 0.0,
                    1 => rec.close = f32::NAN,
                    2 => {
                        rec.open = rec.close - 50.0;
                        rec.high = rec.close + 100.0;
                        rec.low = rec.close - 100.0;
                        rec.trades = 7;
                    }
                    3 => rec.open = crate::scid::FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE,
                    _ => {}
                }
                rec
            })
            .collect()
    }

    #[test]
    fn serial_and_parallel_folds_are_identical() {
        let records = synthetic(3_000_000);
        for step in [1, 60, 300, 86_400] {
            let serial = bars_serial(&records, step, 0.01);
            for pieces in [2, 3, 7, 32] {
                assert_eq!(bars_in_pieces(&records, step, 0.01, pieces), serial, "step {step}, {pieces} pieces");
            }
            assert_eq!(bars(&records, step, 0.01), serial);
        }
        let day = |s: i64| (s - 22 * 3600).div_euclid(86_400) * 86_400 + 22 * 3600;
        let by_fn = bars_with(&records, 0.01, day);
        let by_fn_serial = fold(&records, Scaler::new(0.01), &ByFunction(day));
        assert_eq!(by_fn, by_fn_serial);
    }

    #[test]
    fn a_bucket_cut_at_a_seam_is_joined_whole() {
        let records: Vec<Record> = (0..10).map(|i| trade(120 + i, 100.0 + i as f32, 1, i % 2 == 0)).collect();
        let whole = bars_serial(&records, 60, 1.0);
        assert_eq!(whole.len(), 1);
        for pieces in 2..=10 {
            assert_eq!(bars_in_pieces(&records, 60, 1.0, pieces), whole);
        }
        let b = whole[0];
        assert_eq!((b.start, b.open, b.high, b.low, b.close), (120, 100.0, 109.0, 100.0, 109.0));
        assert_eq!((b.volume, b.ask_volume, b.bid_volume, b.trades), (10, 5, 5, 10));
    }

    #[test]
    fn ticks_are_priced_by_their_close_and_scaled() {
        let mut r = trade(59, 764_700.0, 2, true);
        r.high = 999_999.0;
        r.low = 1.0;
        let s = trade(60, 764_725.0, 1, false);
        let bars = bars(&[r, s], 60, 0.01);
        assert_eq!(bars.len(), 2);
        assert_eq!((bars[0].start, bars[0].open, bars[0].high, bars[0].low), (0, 7647.0, 7647.0, 7647.0));
        assert_eq!((bars[1].start, bars[1].close), (60, 7647.25));
    }

    #[test]
    fn bar_records_bring_their_own_open_high_and_low() {
        let mut r = trade(0, 105.0, 10, true);
        r.open = 101.0;
        r.high = 110.0;
        r.low = 99.0;
        r.trades = 4;
        let b = bars(&[r], 60, 1.0)[0];
        assert_eq!((b.open, b.high, b.low, b.close, b.trades), (101.0, 110.0, 99.0, 105.0, 4));
    }

    #[test]
    fn records_without_a_real_price_are_skipped() {
        let mut bad = vec![trade(0, 0.0, 5, true), trade(1, -3.0, 5, true), trade(2, f32::NAN, 5, true), trade(3, f32::INFINITY, 5, true)];
        assert!(bars(&bad, 60, 1.0).is_empty());
        bad.push(trade(4, 2.0, 1, true));
        assert_eq!(bars(&bad, 60, 1.0)[0].volume, 1);
    }

    #[test]
    fn buckets_before_the_epoch_round_down() {
        let b = bars(&[trade(-1, 1.0, 1, true), trade(-61, 1.0, 1, true)], 60, 1.0);
        assert_eq!(b.iter().map(|b| b.start).collect::<Vec<_>>(), [-60, -120]);
    }

    #[test]
    fn a_bucket_function_is_asked_once_a_second() {
        let records: Vec<Record> = (0..1000).map(|i| trade(1000 + i / 100, 1.0, 1, true)).collect();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let b = bars_with(&records, 1.0, |s| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            s / 5 * 5
        });
        assert_eq!(calls.into_inner(), 10);
        assert_eq!(b.iter().map(|b| (b.start, b.volume)).collect::<Vec<_>>(), [(1000, 500), (1005, 500)]);
    }
}
