//! Cumulative delta estimated from bars.
//!
//! Which side each trade took is in no bar, so this is an estimate: a bar's
//! whole volume counted as bought when it closed up and sold when it closed
//! down. Where volume at price is available, the real delta is read from it
//! instead; this is what stands in when it is not.

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

/// The running delta over one bar, drawn as a candle.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Delta {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
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
    fn next<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Delta {
        let open = if new_period { 0.0 } else { self.running };
        let close = open + estimate(bar, self.previous);
        Delta { open, high: open.max(close), low: open.min(close), close }
    }
}

impl Study for CvdEstimate {
    type Value = Delta;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, new_period: bool) -> Option<Delta> {
        let delta = self.next(bar, new_period);
        self.running = delta.close;
        self.previous = Some(bar.close());
        Some(delta)
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Option<Delta> {
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
