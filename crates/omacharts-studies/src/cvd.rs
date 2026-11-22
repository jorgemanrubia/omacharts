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

use crate::{Ohlcv, Study};

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
