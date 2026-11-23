//! The running delta, a bar at a time: the cumulative delta and the volume
//! delta, each bar a candle of it.
//!
//! A bar's open is the running delta it started from and its close the one
//! it ended on, with the furthest it went either way between. Fed the
//! deltas of what traded inside a bar — a minute's bought less sold, say —
//! [`DeltaBar::add`] moves it along; where nobody said which side each trade
//! took, [`estimate`] stands in, and the bar is marked [`DeltaBar::estimated`].
//!
//! ```
//! use omacharts_studies::cvd::DeltaBar;
//!
//! // A bar opening where the period's running delta stood, at 120, then
//! // three minutes of bought less sold.
//! let mut bar = DeltaBar::opening(120.0, false);
//! for minute in [30.0, -80.0, 25.0] {
//!     bar.add(minute);
//! }
//! assert_eq!((bar.open, bar.high, bar.low, bar.close), (120.0, 150.0, 70.0, 95.0));
//! ```

use crate::{AtPrice, AtPriceStudy, Ohlcv, Study};

/// A bar's estimated delta: its volume, signed by the way it closed — on its
/// open, or on the close before it when it closed where it opened. Volume
/// that is not a finite number counts as none.
#[inline]
pub fn estimate<B: Ohlcv>(bar: &B, previous: Option<f64>) -> f64 {
    let volume = bar.volume();
    let volume = if volume.is_finite() { volume.max(0.0) } else { 0.0 };
    let sign = |a: f64, b: f64| if a > b { 1.0 } else if a < b { -1.0 } else { 0.0 };
    let side = match sign(bar.close(), bar.open()) {
        0.0 => previous.map_or(0.0, |previous| sign(bar.close(), previous)),
        side => side,
    };
    side * volume
}

/// One bar of the running delta, drawn as a candle.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct DeltaBar {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Estimated from the bar's direction rather than read from what
    /// traded at the bid and the ask.
    pub estimated: bool,
    /// The first bar of its period, which starts from nought.
    pub starts: bool,
}

impl DeltaBar {
    /// A bar opening at `open`, before anything has traded in it: open,
    /// high, low and close all there.
    #[inline]
    pub fn opening(open: f64, starts: bool) -> DeltaBar {
        DeltaBar { open, high: open, low: open, close: open, estimated: false, starts }
    }

    /// Move the running delta by `delta`: the close goes with it, and the
    /// high and low reach it.
    #[inline(always)]
    pub fn add(&mut self, delta: f64) {
        self.close += delta;
        self.high = self.high.max(self.close);
        self.low = self.low.min(self.close);
    }

    /// `bar` opening at `open` and moving all one way, by its estimate: see
    /// [`estimate`]. `previous` is the close of the bar before.
    #[inline]
    pub fn estimated<B: Ohlcv>(open: f64, starts: bool, bar: &B, previous: Option<f64>) -> DeltaBar {
        let mut delta = DeltaBar::opening(open, starts);
        delta.add(estimate(bar, previous));
        DeltaBar { estimated: true, ..delta }
    }

    /// The bars of a cumulative delta as one, as a column of a chart too
    /// zoomed out for one each shows them: from the first one's open to the
    /// last one's close, as far as any went either way.
    pub fn spanning(bars: &[DeltaBar]) -> Option<DeltaBar> {
        let (first, last) = (bars.first()?, bars.last()?);
        let mut column = DeltaBar { close: last.close, starts: bars.iter().any(|bar| bar.starts), ..*first };
        for bar in bars {
            column.high = column.high.max(bar.high);
            column.low = column.low.min(bar.low);
        }
        Some(column)
    }

    /// The bars of a volume delta as one: each starts from nought, so the
    /// column runs on from one to the next, from nought to their sum, as far
    /// as it went either way on the way.
    pub fn summed(bars: &[DeltaBar]) -> Option<DeltaBar> {
        let first = bars.first()?;
        let mut column = DeltaBar { open: 0.0, high: 0.0, low: 0.0, close: 0.0, ..*first };
        let mut running = 0.0;
        for bar in bars {
            column.high = column.high.max(running + bar.high);
            column.low = column.low.min(running + bar.low);
            running += bar.close;
            column.estimated |= bar.estimated;
        }
        column.close = running;
        Some(column)
    }
}

/// Cumulative delta estimated from bars, starting again from nought at each
/// new period.
#[derive(Clone, Debug, Default)]
pub struct CvdEstimate {
    running: f64,
    previous: Option<f64>,
}

impl CvdEstimate {
    pub fn new() -> CvdEstimate {
        CvdEstimate::default()
    }

    #[inline(always)]
    fn next<B: Ohlcv>(&self, bar: &B, new_period: bool) -> DeltaBar {
        let open = if new_period { 0.0 } else { self.running };
        DeltaBar::estimated(open, new_period, bar, self.previous)
    }
}

impl Study for CvdEstimate {
    type Value = DeltaBar;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, new_period: bool) -> Option<DeltaBar> {
        let delta = self.next(bar, new_period);
        self.running = delta.close;
        self.previous = Some(bar.close());
        Some(delta)
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Option<DeltaBar> {
        Some(self.next(bar, new_period))
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "cvd"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        Vec::new()
    }
}

/// Each bar's own delta estimated from its direction, starting from nought
/// at every bar: the volume delta where no side is known. Where it is, the
/// bar's delta is [`crate::VolumeDelta`]'s reading.
#[derive(Clone, Debug, Default)]
pub struct BarDeltaEstimate {
    previous: Option<f64>,
}

impl BarDeltaEstimate {
    pub fn new() -> BarDeltaEstimate {
        BarDeltaEstimate::default()
    }

    #[inline(always)]
    fn next<B: Ohlcv>(&self, bar: &B) -> DeltaBar {
        let close = estimate(bar, self.previous);
        DeltaBar { open: 0.0, high: close.max(0.0), low: close.min(0.0), close, estimated: true, starts: true }
    }
}

impl Study for BarDeltaEstimate {
    type Value = DeltaBar;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _new_period: bool) -> Option<DeltaBar> {
        let delta = self.next(bar);
        self.previous = Some(bar.close());
        Some(delta)
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _new_period: bool) -> Option<DeltaBar> {
        Some(self.next(bar))
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "volume_delta"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        Vec::new()
    }
}

/// The running delta read from what traded, a bar at a time: each bar a
/// [`DeltaBar`] opening where the running delta stood — or at nought, at
/// the start of a period or, for each bar's own delta, of every bar — and
/// moved along by every piece of the bar's bought less sold, so its high
/// and low are the furthest it went either way between. A bar whose pieces
/// cannot be told whole is estimated from its direction instead (see
/// [`estimate`]) and marked so.
///
/// Stepped on bars of volume at price ([`AtPrice`]) it never estimates, and
/// reads each bar in the pieces [`AtPrice::each_delta`] hands over. A
/// caller that reads the pieces from elsewhere — minute ladders held apart
/// from the bars — steps it with [`DeltaRead::step_with`].
#[derive(Clone, Debug, Default)]
pub struct DeltaRead {
    /// Every bar starts from nought: each bar's own delta.
    each_bar: bool,
    /// The running delta the last bar closed at.
    running: f64,
    /// The last bar's close, for an estimate's side.
    previous: Option<f64>,
}

impl DeltaRead {
    /// The cumulative delta, started again at each new period: the `cvd`
    /// indicator read from volume at price.
    pub fn cumulative() -> DeltaRead {
        DeltaRead::default()
    }

    /// Each bar's own delta, from nought at its open: the `volume_delta`
    /// indicator read from volume at price.
    pub fn each_bar() -> DeltaRead {
        DeltaRead { each_bar: true, ..DeltaRead::default() }
    }

    /// Carrying on after a bar whose running delta closed at `running` and
    /// whose own close was `previous`: for a caller that keeps the bars it
    /// worked out and steps only the ones after them again.
    pub fn resumed(mut self, running: f64, previous: Option<f64>) -> DeltaRead {
        self.running = running;
        self.previous = previous;
        self
    }

    #[inline(always)]
    fn next<B: Ohlcv>(&self, bar: &B, new_period: bool, read: impl FnOnce(&mut DeltaBar) -> bool) -> DeltaBar {
        let starts = self.each_bar || new_period;
        let open = if starts { 0.0 } else { self.running };
        let mut delta = DeltaBar::opening(open, starts);
        if read(&mut delta) { delta } else { DeltaBar::estimated(open, starts, bar, self.previous) }
    }

    /// Take the next bar, which has closed, reading its pieces with `read`:
    /// it moves the bar it is handed along by each piece's bought less
    /// sold, in order, with [`DeltaBar::add`], and answers whether it could
    /// tell the bar whole. When it could not, whatever it added is thrown
    /// away and the bar is estimated.
    #[inline]
    pub fn step_with<B: Ohlcv>(&mut self, bar: &B, new_period: bool, read: impl FnOnce(&mut DeltaBar) -> bool) -> DeltaBar {
        let delta = self.next(bar, new_period, read);
        self.running = delta.close;
        self.previous = Some(bar.close());
        delta
    }

    /// What [`DeltaRead::step_with`] would answer for `bar`, without taking
    /// it, to the bit.
    #[inline]
    pub fn peek_with<B: Ohlcv>(&self, bar: &B, new_period: bool, read: impl FnOnce(&mut DeltaBar) -> bool) -> DeltaBar {
        self.next(bar, new_period, read)
    }
}

/// A bar of volume at price, read in its own pieces.
#[inline(always)]
fn pieces<B: AtPrice>(bar: &B) -> impl FnOnce(&mut DeltaBar) -> bool + '_ {
    move |delta| {
        bar.each_delta(|piece| delta.add(piece as f64));
        true
    }
}

impl AtPriceStudy for DeltaRead {
    type Value = DeltaBar;

    #[inline]
    fn step<B: AtPrice>(&mut self, bar: &B, new_period: bool) -> Option<DeltaBar> {
        Some(self.step_with(bar, new_period, pieces(bar)))
    }

    #[inline]
    fn peek<B: AtPrice>(&self, bar: &B, new_period: bool) -> Option<DeltaBar> {
        Some(self.peek_with(bar, new_period, pieces(bar)))
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        if self.each_bar { "volume_delta" } else { "cvd" }
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bar;

    fn bar(open: f64, close: f64, volume: f64) -> Bar {
        Bar { ts: 0, open, high: open.max(close), low: open.min(close), close, volume }
    }

    /// A rising bar's volume is bought, a falling one's sold, and a bar that
    /// closed where it opened takes the side its close moved from the close
    /// before it, or none.
    #[test]
    fn an_estimate_takes_its_side_from_the_bars_direction() {
        assert_eq!(estimate(&bar(10.0, 11.0, 50.0), None), 50.0);
        assert_eq!(estimate(&bar(11.0, 10.0, 50.0), None), -50.0);
        assert_eq!(estimate(&bar(10.0, 10.0, 50.0), Some(9.0)), 50.0, "a doji above the close before");
        assert_eq!(estimate(&bar(10.0, 10.0, 50.0), Some(11.0)), -50.0, "below it");
        assert_eq!(estimate(&bar(10.0, 10.0, 50.0), Some(10.0)), 0.0, "unchanged either way");
        assert_eq!(estimate(&bar(10.0, 10.0, 50.0), None), 0.0, "nothing before it");
        assert_eq!(estimate(&bar(10.0, 11.0, f64::NAN), None), 0.0);
    }

    /// A bar of minutes, bought and sold, and how many of its pieces can
    /// be told.
    struct Minutes {
        bar: Bar,
        deltas: Vec<i64>,
    }

    impl Ohlcv for Minutes {
        fn ts(&self) -> i64 {
            self.bar.ts
        }
        fn open(&self) -> f64 {
            self.bar.open
        }
        fn high(&self) -> f64 {
            self.bar.high
        }
        fn low(&self) -> f64 {
            self.bar.low
        }
        fn close(&self) -> f64 {
            self.bar.close
        }
        fn volume(&self) -> f64 {
            self.bar.volume
        }
    }

    impl AtPrice for Minutes {
        fn bought(&self) -> u64 {
            self.deltas.iter().filter(|d| **d > 0).sum::<i64>() as u64
        }
        fn sold(&self) -> u64 {
            (-self.deltas.iter().filter(|d| **d < 0).sum::<i64>()) as u64
        }
        fn each_price(&self, _: impl FnMut(i32, u64)) {}
        fn each_delta(&self, mut each: impl FnMut(i64)) {
            self.deltas.iter().for_each(|d| each(*d));
        }
    }

    /// Read in pieces, a bar's high and low are the furthest the running
    /// delta went inside it; it runs on from bar to bar and starts again at
    /// each period, and each bar's own starts at nought every bar.
    #[test]
    fn a_delta_read_in_pieces_has_the_extremes_inside_each_bar() {
        let bars = [
            Minutes { bar: bar(10.0, 9.0, 3.0), deltas: vec![10, -25, 30] },
            Minutes { bar: bar(9.0, 8.0, 3.0), deltas: vec![-5, 0, 0] },
            Minutes { bar: bar(8.0, 9.0, 3.0), deltas: vec![4, -1] },
        ];
        let mut cvd = DeltaRead::cumulative();
        let mut own = DeltaRead::each_bar();
        let out: Vec<(DeltaBar, DeltaBar)> = bars
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let new_period = i != 1;
                let peeked = (cvd.peek(b, new_period), own.peek(b, new_period));
                let stepped = (cvd.step(b, new_period).unwrap(), own.step(b, new_period).unwrap());
                assert_eq!(peeked, (Some(stepped.0), Some(stepped.1)));
                stepped
            })
            .collect();
        assert_eq!(out[0].0, DeltaBar { open: 0.0, high: 15.0, low: -15.0, close: 15.0, estimated: false, starts: true });
        assert_eq!(out[1].0, DeltaBar { open: 15.0, high: 15.0, low: 10.0, close: 10.0, estimated: false, starts: false });
        assert_eq!((out[2].0.open, out[2].0.close, out[2].0.starts), (0.0, 3.0, true), "a new period opens at nought");
        assert_eq!(out[1].1, DeltaBar { open: 0.0, high: 0.0, low: -5.0, close: -5.0, estimated: false, starts: true });
        assert_eq!((cvd.key(), own.key()), ("cvd", "volume_delta"));
    }

    /// A bar whose pieces cannot be told is the estimate, whatever was
    /// added before it gave up, and the running delta carries through it.
    #[test]
    fn a_bar_that_cannot_be_told_is_estimated_where_the_estimate_would_be() {
        let bars = [bar(10.0, 11.0, 5.0), bar(11.0, 10.0, 2.0), bar(10.0, 10.0, 4.0)];
        let mut read = DeltaRead::cumulative();
        let mut estimate = CvdEstimate::new();
        for (i, b) in bars.iter().enumerate() {
            let read = read.step_with(b, i == 0, |delta| {
                delta.add(1_000.0);
                false
            });
            assert_eq!(Some(read), estimate.step(b, i == 0));
        }
        let mut resumed = DeltaRead::each_bar().resumed(40.0, Some(9.0));
        let doji = resumed.step_with(&bar(10.0, 10.0, 3.0), false, |_| false);
        assert_eq!((doji.open, doji.close, doji.estimated), (0.0, 3.0, true), "above the close before it");
    }

    /// A column of a cumulative delta runs from the first bar's open to the
    /// last one's close; one of a volume delta runs on from bar to bar.
    #[test]
    fn columns_join_bars_as_their_kind_runs() {
        let mut first = DeltaBar::opening(10.0, true);
        first.add(-30.0);
        first.add(15.0);
        let mut second = DeltaBar::opening(first.close, false);
        second.add(40.0);
        let column = DeltaBar::spanning(&[first, second]).unwrap();
        assert_eq!((column.open, column.high, column.low, column.close, column.starts), (10.0, 35.0, -20.0, 35.0, true));

        let a = DeltaBar::estimated(0.0, true, &bar(10.0, 11.0, 5.0), None);
        let b = DeltaBar::estimated(0.0, true, &bar(11.0, 10.0, 8.0), None);
        let column = DeltaBar::summed(&[a, b]).unwrap();
        assert_eq!((column.open, column.high, column.low, column.close, column.estimated), (0.0, 5.0, -3.0, -3.0, true));
    }
}
