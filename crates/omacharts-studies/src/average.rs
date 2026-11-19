//! Moving averages of closes.

use crate::window::Ring;
use crate::{Ohlcv, Study};

/// Simple moving average of closes: a running sum, the newest close added
/// before the oldest is taken off.
#[derive(Clone, Debug)]
pub struct Sma {
    period: usize,
    /// The period as the divisor, converted once rather than every bar.
    n: f64,
    closes: Ring<f64>,
    sum: f64,
}

impl Sma {
    pub fn new(period: usize) -> Sma {
        Sma { period, n: period as f64, closes: Ring::new(period), sum: 0.0 }
    }

    /// The sum after `close`, and the reading: once there are `period`
    /// closes, which is exactly when the window is full.
    #[inline(always)]
    fn next(&self, close: f64) -> (f64, Option<f64>) {
        // The newest added before the oldest is taken off, as
        // `series::sma` does, but without its branch: until the window is
        // full the slot a push writes still holds the 0.0 the ring was made
        // with, and taking +0.0 off leaves every f64 exactly as it was (-0.0,
        // the infinities and NaN included), so the sum is the same bits.
        let sum = (self.sum + close) - self.closes.replacing();
        let value = self.closes.fills_with_next().then(|| sum / self.n);
        (sum, value)
    }
}

impl Study for Sma {
    type Value = f64;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        let (sum, value) = self.next(bar.close());
        self.sum = sum;
        self.closes.push(bar.close());
        value
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        self.next(bar.close()).1
    }

    fn warmup(&self) -> usize {
        self.period.saturating_sub(1)
    }

    fn key(&self) -> &'static str {
        "sma"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("period", self.period as f64)]
    }
}

/// Exponential moving average of closes, seeded with the simple average of
/// the first `period` closes so it does not start from an arbitrary value.
#[derive(Clone, Debug)]
pub struct Ema {
    period: usize,
    alpha: f64,
    seen: usize,
    /// The first `period` closes summed, from where `Iterator::sum` starts.
    seed: f64,
    value: f64,
}

impl Ema {
    pub fn new(period: usize) -> Ema {
        Ema { period, alpha: 2.0 / (period as f64 + 1.0), seen: 0, seed: -0.0, value: 0.0 }
    }

    #[inline(always)]
    fn next(&self, close: f64) -> (f64, f64, Option<f64>) {
        let seen = self.seen + 1;
        if seen < self.period {
            (self.seed + close, self.value, None)
        } else if seen == self.period {
            let value = (self.seed + close) / self.period as f64;
            (self.seed + close, value, Some(value))
        } else {
            let value = close * self.alpha + self.value * (1.0 - self.alpha);
            (self.seed, value, Some(value))
        }
    }
}

impl Study for Ema {
    type Value = f64;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        let (seed, value, out) = self.next(bar.close());
        self.seed = seed;
        self.value = value;
        self.seen += 1;
        out
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        self.next(bar.close()).2
    }

    fn warmup(&self) -> usize {
        self.period.saturating_sub(1)
    }

    fn key(&self) -> &'static str {
        "ema"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("period", self.period as f64)]
    }
}

/// Wilder's smoothing of a stream of numbers: seeded with the simple average
/// of the first `period`, then each new value pulls it a `period`th of the way.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Wilder {
    period: usize,
    seen: usize,
    average: f64,
}

impl Wilder {
    pub(crate) fn new(period: usize) -> Wilder {
        Wilder { period, seen: 0, average: -0.0 }
    }

    /// The state after `value`, and the reading at it.
    #[inline(always)]
    pub(crate) fn next(&self, value: f64) -> (Wilder, Option<f64>) {
        let seen = self.seen + 1;
        let n = self.period as f64;
        let (average, out) = if seen < self.period {
            (self.average + value, None)
        } else if seen == self.period {
            let average = (self.average + value) / n;
            (average, Some(average))
        } else {
            let average = self.average + (value - self.average) / n;
            (average, Some(average))
        };
        (Wilder { period: self.period, seen, average }, out)
    }
}

/// An exponential average of a stream that can have holes in it: a hole
/// starts it again, seeded afresh from the next `period` values.
#[derive(Clone, Copy, Debug)]
pub struct EmaOf {
    period: usize,
    alpha: f64,
    average: Option<f64>,
    run: usize,
    seed: f64,
}

impl EmaOf {
    pub fn new(period: usize) -> EmaOf {
        EmaOf { period, alpha: 2.0 / (period as f64 + 1.0), average: None, run: 0, seed: 0.0 }
    }

    /// Take the next value of the stream, or a hole, and answer the average.
    #[inline]
    pub fn push(&mut self, value: Option<f64>) -> Option<f64> {
        let (next, out) = self.next(value);
        *self = next;
        out
    }

    #[inline(always)]
    pub(crate) fn next(&self, value: Option<f64>) -> (EmaOf, Option<f64>) {
        let mut next = *self;
        let Some(value) = value else {
            next.average = None;
            next.run = 0;
            next.seed = 0.0;
            return (next, None);
        };
        match self.average {
            Some(previous) => {
                let average = value * self.alpha + previous * (1.0 - self.alpha);
                next.average = Some(average);
                (next, Some(average))
            }
            None => {
                next.run += 1;
                next.seed += value;
                if next.run == self.period {
                    let mean = next.seed / self.period as f64;
                    next.average = Some(mean);
                    (next, Some(mean))
                } else {
                    (next, None)
                }
            }
        }
    }
}
