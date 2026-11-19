//! Bollinger bands: a simple average of closes and the standard deviation
//! around it, over the same window.

use crate::vwap::Spread;
use crate::window::Ring;
use crate::{Ohlcv, Study};

/// The average and the population standard deviation of the last `period`
/// closes.
///
/// Both come from running sums, kept relative to an anchor — the first
/// finite close — so that squaring prices in the thousands does not throw
/// away the digits the deviation is made of. A window holding a close that
/// is not a finite number has no reading until that close has left it.
#[derive(Clone, Debug)]
pub struct Bollinger {
    period: usize,
    closes: Ring<f64>,
    sums: Sums,
}

/// The running sums, relative to the anchor.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sums {
    anchor: f64,
    anchored: bool,
    sum: f64,
    sum_sq: f64,
    broken: usize,
}

impl Sums {
    pub(crate) fn new() -> Sums {
        Sums { anchor: 0.0, anchored: false, sum: 0.0, sum_sq: 0.0, broken: 0 }
    }
}

impl Bollinger {
    pub fn new(period: usize) -> Bollinger {
        Bollinger {
            period,
            closes: Ring::new(period),
            sums: Sums::new(),
        }
    }

    /// The close leaving the window when the next one comes in, if one is.
    #[inline(always)]
    fn leaving(&self) -> Option<f64> {
        self.closes.leaving()
    }
}

/// Take `add` into `sums` and let `drop` go, answering the reading if the
/// window is `ready` (has `period` closes with this one). The one place a
/// Bollinger's arithmetic is, for a window kept by a study or read off a
/// series; stepping does it to the study's own sums, peeking to a copy.
#[inline(always)]
pub(crate) fn advance(sums: &mut Sums, add: f64, drop: Option<f64>, ready: bool, period: usize) -> Option<Spread> {
    if add.is_finite() {
        if !sums.anchored {
            sums.anchor = add;
            sums.anchored = true;
        }
        let d = add - sums.anchor;
        sums.sum += d;
        sums.sum_sq += d * d;
    } else {
        sums.broken += 1;
    }
    if let Some(drop) = drop {
        if drop.is_finite() {
            // A finite close was taken, so there is an anchor.
            let d = drop - sums.anchor;
            sums.sum -= d;
            sums.sum_sq -= d * d;
        } else {
            sums.broken = sums.broken.saturating_sub(1);
        }
    }
    if !ready || sums.broken > 0 {
        return None;
    }
    let n = period as f64;
    let mean = sums.sum / n;
    let variance = ((sums.sum_sq - sums.sum * mean) / n).max(0.0);
    Some(Spread { center: sums.anchor + mean, deviation: variance.sqrt() })
}

impl Study for Bollinger {
    type Value = Spread;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<Spread> {
        if self.period == 0 {
            return None;
        }
        let close = bar.close();
        let mut sums = self.sums;
        let value = advance(&mut sums, close, self.leaving(), self.closes.fills_with_next(), self.period);
        self.sums = sums;
        self.closes.push(close);
        value
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<Spread> {
        if self.period == 0 {
            return None;
        }
        let mut sums = self.sums;
        advance(&mut sums, bar.close(), self.leaving(), self.closes.fills_with_next(), self.period)
    }

    fn warmup(&self) -> usize {
        self.period.saturating_sub(1)
    }

    fn key(&self) -> &'static str {
        "bollinger"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("period", self.period as f64)]
    }
}
