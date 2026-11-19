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
//! is already a bar contributes its own open, high, low and close, its high
//! and low the right way round even where they were written the wrong way.
//! A record whose close is not a number is skipped; a negative or zero
//! price is a price, and kept — crude oil settled below zero in April 2020.
//!
//! A file is almost in time order, but a record can come a few seconds
//! before the one written ahead of it. Such a record is taken at the time
//! of the latest before it, as part of the bar already open, so the bars
//! stay in time order and none of the volume is lost. Only up to
//! [`OUT_OF_ORDER`] behind, though: a record further back than that is
//! taken at its own time, so one record stamped far ahead does not draw
//! every record after it into its bar. See [`in_order`].
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

/// How far behind the latest record, in Sierra Chart's microseconds, a
/// record may be stamped and still be taken at the latest's time.
pub const OUT_OF_ORDER: i64 = 60_000_000;

/// The time a record stamped `datetime` is taken at, after records up to
/// `latest` (`i64::MIN` before the first): the latest's time when it is
/// stamped no more than [`OUT_OF_ORDER`] behind it, and its own otherwise.
/// What it returns is the next record's `latest`.
#[inline(always)]
pub fn in_order(datetime: i64, latest: i64) -> i64 {
    // Behind by 1 to OUT_OF_ORDER, as one unsigned comparison. A record
    // at or after the latest makes `behind` 0 or wraps it to the top of
    // the range, as a lag too big for an i64 does (only `latest` at
    // `i64::MIN`, or a damaged stamp, makes one): either way it fails.
    let behind = latest.wrapping_sub(datetime) as u64;
    if behind.wrapping_sub(1) < OUT_OF_ORDER as u64 { latest } else { datetime }
}

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
    run(records, Scaler::new(multiplier), &Step(step_seconds), PARALLEL_THRESHOLD, i64::MIN).0
}

/// Bars over buckets of any shape: `bucket` takes a record's unix second
/// and returns the start of the bucket it belongs to, for instance the
/// start of its trading day. It is called once per distinct second, not
/// per record. Records must be in time order, and `bucket` must not go
/// backwards as time goes forwards.
pub fn bars_with(records: &[Record], multiplier: f64, bucket: impl Fn(i64) -> i64 + Sync) -> Vec<Bar> {
    run(records, Scaler::new(multiplier), &ByFunction(bucket), PARALLEL_THRESHOLD, i64::MIN).0
}

/// How many records [`bars_reporting`] folds between one report and the next.
pub const REPORT_EVERY: usize = 8_000_000;

/// [`bars`], saying after each [`REPORT_EVERY`] records how far through it
/// is, from 0 to 1: the same bars, each piece folded as [`bars`] folds it.
pub fn bars_reporting(records: &[Record], step_seconds: i64, multiplier: f64, mut report: impl FnMut(f64)) -> Vec<Bar> {
    if records.len() <= REPORT_EVERY {
        let bars = bars(records, step_seconds, multiplier);
        report(1.0);
        return bars;
    }
    assert!(step_seconds > 0, "a bar is at least a second wide");
    let scaler = Scaler::new(multiplier);
    let mut out: Vec<Bar> = Vec::new();
    let mut done = 0;
    // Each piece goes on from the latest time before it, as one fold would.
    let mut latest = i64::MIN;
    for piece in records.chunks(REPORT_EVERY) {
        let folded;
        (folded, latest) = run(piece, scaler, &Step(step_seconds), PARALLEL_THRESHOLD, latest);
        join(&mut out, &folded);
        done += piece.len();
        report(done as f64 / records.len() as f64);
    }
    out
}

/// [`bars`] on one thread, whatever the size.
pub fn bars_serial(records: &[Record], step_seconds: i64, multiplier: f64) -> Vec<Bar> {
    assert!(step_seconds > 0, "a bar is at least a second wide");
    fold(records, Scaler::new(multiplier), &Step(step_seconds), i64::MIN).0
}

/// [`bars`] cut into this many pieces, whatever the size. For tests and
/// benchmarks.
pub fn bars_in_pieces(records: &[Record], step_seconds: i64, multiplier: f64, pieces: usize) -> Vec<Bar> {
    parallel(records, Scaler::new(multiplier), &Step(step_seconds), pieces, i64::MIN).0
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

/// The bars of `records` after records up to `latest`, and the latest
/// time once they are in.
fn run(records: &[Record], scaler: Scaler, bucketing: &impl Bucketing, threshold: usize, latest: i64) -> (Vec<Bar>, i64) {
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    if records.len() <= threshold || threads == 1 {
        fold(records, scaler, bucketing, latest)
    } else {
        parallel(records, scaler, bucketing, threads, latest)
    }
}

/// Every piece folded at once as if it came first, then joined in order.
/// A piece whose first record is stamped before the latest of the pieces
/// ahead of it is folded again, from that latest, as the serial fold would
/// have taken it: rare, since it takes a record out of order at the seam.
fn parallel(records: &[Record], scaler: Scaler, bucketing: &impl Bucketing, pieces: usize, mut latest: i64) -> (Vec<Bar>, i64) {
    let pieces = pieces.clamp(1, records.len().max(1));
    let size = records.len().div_ceil(pieces).max(1);
    let folded: Vec<(Vec<Bar>, i64)> = std::thread::scope(|s| {
        let handles: Vec<_> =
            records.chunks(size).map(|piece| s.spawn(move || fold(piece, scaler, bucketing, i64::MIN))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut out: Vec<Bar> = Vec::with_capacity(folded.iter().map(|(bars, _)| bars.len()).sum());
    for (piece, (bars, last)) in records.chunks(size).zip(folded) {
        let Some(first) = piece.iter().find(|r| finite(r.close)) else { continue };
        let (bars, last) = if first.datetime < latest { fold(piece, scaler, bucketing, latest) } else { (bars, last) };
        join(&mut out, &bars);
        latest = last;
    }
    (out, latest)
}

/// Put a later piece's bars after the bars so far, its first joined to the
/// last when the bucket straddles the two, as the serial fold would have
/// gone on building it.
fn join(out: &mut Vec<Bar>, later: &[Bar]) {
    let rest = match (out.last_mut(), later.first()) {
        (Some(last), Some(first)) if first.start == last.start => {
            last.merge(first);
            &later[1..]
        }
        _ => later,
    };
    out.extend_from_slice(rest);
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
fn finite(p: f32) -> bool {
    p.is_finite()
}

/// A record's open, high and low as a bar takes them, in stored prices,
/// for a record whose close is a number. A trade's are its close: its
/// high and low are the ask and the bid, never trade prices. A bar's are
/// its own, its close where one is not a number, its high and low the
/// right way round where they were written the wrong way, and wide enough
/// to hold its close.
#[inline(always)]
pub fn prices(r: &Record) -> (f32, f32, f32) {
    let close = r.close;
    // Worked out without branches: in a tick file a third of the records
    // carry an unbundled-trade marker, in no pattern a branch predictor
    // could learn.
    let tick = (r.open == 0.0) | (r.open == FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE) | (r.open == LAST_SUB_TRADE_OF_UNBUNDLED_TRADE);
    if tick {
        (close, close, close)
    } else {
        let or_close = |v: f32| if finite(v) { v } else { close };
        // A bar written with its high below its low has the two the wrong
        // way round, not a range of less than nothing.
        let (high, low) = (or_close(r.high), or_close(r.low));
        let (high, low) = if high < low { (low, high) } else { (high, low) };
        (or_close(r.open), high.max(close), low.min(close))
    }
}

fn fold(records: &[Record], scaler: Scaler, bucketing: &impl Bucketing, mut latest: i64) -> (Vec<Bar>, i64) {
    let mut out: Vec<Bar> = Vec::new();
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    let mut b = Building { start: 0, open: 0.0, high: 0.0, low: 0.0, close: 0.0, volume: 0, bid_volume: 0, ask_volume: 0, trades: 0 };
    let mut have = false;
    for r in records {
        let close = r.close;
        if !finite(close) {
            continue;
        }
        let datetime = in_order(r.datetime, latest);
        latest = datetime;
        let (open, high, low) = prices(r);
        if datetime < lo || datetime >= hi {
            let (start, l, h) = bucketing.locate(datetime);
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
    (out, latest)
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
                    // A record written a few seconds before the one ahead
                    // of it, as files carry now and then.
                    4 => rec.datetime -= 3_000_000,
                    // And, more rarely, one stamped a day ahead.
                    5 if i % 3 == 0 => rec.datetime += 86_400_000_000,
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
        let by_fn_serial = fold(&records, Scaler::new(0.01), &ByFunction(day), i64::MIN).0;
        assert_eq!(by_fn, by_fn_serial);
    }

    /// Folded in reported pieces, the bars are the same bars, and how far
    /// through it is only grows, to the whole.
    #[test]
    fn a_fold_that_reports_folds_the_same_bars() {
        let records = synthetic(REPORT_EVERY * 2 + 1_234);
        let mut said = Vec::new();
        let reported = bars_reporting(&records, 60, 0.25, |fraction| said.push(fraction));
        assert_eq!(reported, bars(&records, 60, 0.25));
        assert_eq!(said.len(), 3);
        assert!(said.windows(2).all(|pair| pair[0] < pair[1]) && said[2] == 1.0, "{said:?}");
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
    fn records_without_a_number_for_a_price_are_skipped() {
        let mut bad = vec![trade(2, f32::NAN, 5, true), trade(3, f32::INFINITY, 5, true), trade(3, f32::NEG_INFINITY, 5, true)];
        assert!(bars(&bad, 60, 1.0).is_empty());
        bad.push(trade(4, 2.0, 1, true));
        assert_eq!(bars(&bad, 60, 1.0)[0].volume, 1);
    }

    /// A negative or a zero price is a price: crude oil settled below zero
    /// in April 2020, and a chart that drops those records draws a market
    /// that did not happen.
    #[test]
    fn negative_and_zero_prices_are_kept() {
        let records = [trade(0, 1_000.0, 1, true), trade(1, 0.0, 2, true), trade(2, -3_763.0, 3, false), trade(3, 500.0, 4, true)];
        let b = bars(&records, 60, 0.01)[0];
        assert_eq!((b.open, b.high, b.low, b.close), (10.0, 10.0, -37.63, 5.0));
        assert_eq!(b.volume, 10, "every record counted");
        // A bar record below zero brings its own prices too.
        let mut bar = trade(0, -10.0, 1, true);
        (bar.open, bar.high, bar.low) = (-5.0, -1.0, -20.0);
        let b = bars(&[bar], 60, 1.0)[0];
        assert_eq!((b.open, b.high, b.low, b.close), (-5.0, -1.0, -20.0, -10.0));
    }

    /// A bar written with its high below its low is a bar with the two the
    /// wrong way round.
    #[test]
    fn an_inverted_bar_is_set_right() {
        let mut r = trade(0, 105.0, 10, true);
        (r.open, r.high, r.low) = (101.0, 99.0, 110.0);
        let b = bars(&[r], 60, 1.0)[0];
        assert_eq!((b.open, b.high, b.low, b.close), (101.0, 110.0, 99.0, 105.0));
    }

    /// A record written before the one ahead of it is part of the bar
    /// already open: the bars stay in time order and none of the volume is
    /// lost, folded in one piece or in many.
    #[test]
    fn a_record_out_of_time_order_joins_the_bar_already_open() {
        let records = [
            trade(100, 1.0, 1, true),
            trade(130, 2.0, 1, true),
            // Back into the bar before: it goes in the bar open at 120.
            trade(115, 9.0, 4, true),
            trade(185, 3.0, 1, true),
        ];
        let b = bars_serial(&records, 60, 1.0);
        assert_eq!(b.iter().map(|b| b.start).collect::<Vec<_>>(), [60, 120, 180]);
        assert_eq!((b[1].high, b[1].close, b[1].volume), (9.0, 9.0, 5));
        for pieces in 2..=4 {
            assert_eq!(bars_in_pieces(&records, 60, 1.0, pieces), b, "{pieces} pieces");
        }
    }

    /// One record stamped far ahead is a bar of its own, and the records
    /// after it go on in bars at their own times, in one piece or many.
    #[test]
    fn a_record_stamped_far_ahead_does_not_take_the_rest() {
        let records = [
            trade(60, 1.0, 1, true),
            trade(120, 2.0, 1, true),
            trade(120 + 86_400, 3.0, 1, true),
            trade(180, 4.0, 1, true),
            trade(240, 5.0, 1, true),
            trade(300, 6.0, 1, true),
        ];
        let b = bars_serial(&records, 60, 1.0);
        assert_eq!(b.iter().map(|b| (b.start, b.volume)).collect::<Vec<_>>(), [(60, 1), (120, 1), (86_520, 1), (180, 1), (240, 1), (300, 1)]);
        for pieces in 2..=6 {
            assert_eq!(bars_in_pieces(&records, 60, 1.0, pieces), b, "{pieces} pieces");
        }
    }

    /// Up to a minute behind is out of order and taken at the latest's
    /// time; further back is a record at its own time.
    #[test]
    fn out_of_order_is_a_minute_at_most() {
        let latest = crate::sc_from_unix_micros(1_000_000_000);
        assert_eq!(in_order(latest - OUT_OF_ORDER, latest), latest);
        assert_eq!(in_order(latest - OUT_OF_ORDER - 1, latest), latest - OUT_OF_ORDER - 1);
        assert_eq!(in_order(latest + 1, latest), latest + 1);
        assert_eq!(in_order(i64::MIN + 1, i64::MAX), i64::MIN + 1);
        assert_eq!(in_order(5, i64::MIN), 5);
    }

    /// Wherever the pieces are cut, a record out of order at a seam is
    /// taken as one fold takes it.
    #[test]
    fn seams_anywhere_fold_as_one() {
        let mut records: Vec<Record> = (0..40).map(|i| trade(1_000 + i * 7, 100.0 + i as f32, 1, i % 2 == 0)).collect();
        for (at, back) in [(5, 3), (11, 20), (12, 200), (25, 61), (26, 59), (33, 9_000)] {
            records[at].datetime -= back * 1_000_000;
        }
        records[30].datetime += 86_400 * 1_000_000;
        for step in [1, 5, 60] {
            let serial = bars_serial(&records, step, 1.0);
            assert_eq!(serial.iter().map(|b| b.volume).sum::<u64>(), 40, "step {step}");
            for pieces in 2..=40 {
                assert_eq!(bars_in_pieces(&records, step, 1.0, pieces), serial, "step {step}, {pieces} pieces");
            }
        }
    }

    #[test]
    fn buckets_before_the_epoch_round_down() {
        let b = bars(&[trade(-61, 1.0, 1, true), trade(-1, 1.0, 1, true)], 60, 1.0);
        assert_eq!(b.iter().map(|b| b.start).collect::<Vec<_>>(), [-120, -60]);
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
