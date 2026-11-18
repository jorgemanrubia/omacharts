//! Volume-weighted average price, and the spread around it.

use crate::{Ohlcv, Study};

/// A centre line and the standard deviation around it, from which any band
/// is `center ± deviation × k`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spread {
    pub center: f64,
    pub deviation: f64,
}

impl Spread {
    /// The band `k` deviations above the centre.
    #[inline]
    pub fn upper(&self, k: f64) -> f64 {
        self.center + self.deviation * k
    }

    /// The band `k` deviations below the centre.
    #[inline]
    pub fn lower(&self, k: f64) -> f64 {
        self.center - self.deviation * k
    }
}

/// The price of a bar a VWAP weighs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Source {
    /// (high + low + close) / 3.
    #[default]
    Hlc3,
    /// (high + low) / 2.
    Hl2,
    /// (open + high + low + close) / 4.
    Ohlc4,
    Close,
}

impl Source {
    #[inline(always)]
    pub fn of<B: Ohlcv>(self, bar: &B) -> f64 {
        match self {
            Source::Hlc3 => (bar.high() + bar.low() + bar.close()) / 3.0,
            Source::Hl2 => (bar.high() + bar.low()) / 2.0,
            Source::Ohlc4 => (bar.open() + bar.high() + bar.low() + bar.close()) / 4.0,
            Source::Close => bar.close(),
        }
    }
}

/// VWAP since the start of the period, and the volume-weighted standard
/// deviation of price around it. A bar with no volume (or none that is a
/// positive number) weighs one, so a source without volume still has a line.
#[derive(Clone, Debug)]
pub struct Vwap {
    source: Source,
    sum_w: f64,
    sum_wp: f64,
    sum_wp2: f64,
}

impl Vwap {
    pub fn new(source: Source) -> Vwap {
        Vwap { source, sum_w: 0.0, sum_wp: 0.0, sum_wp2: 0.0 }
    }

    #[inline(always)]
    fn next<B: Ohlcv>(&self, bar: &B, new_period: bool) -> (f64, f64, f64) {
        let (sum_w, sum_wp, sum_wp2) =
            if new_period { (0.0, 0.0, 0.0) } else { (self.sum_w, self.sum_wp, self.sum_wp2) };
        let volume = bar.volume();
        let weight = if volume > 0.0 { volume } else { 1.0 };
        let price = self.source.of(bar);
        (sum_w + weight, sum_wp + weight * price, sum_wp2 + weight * price * price)
    }

    #[inline(always)]
    fn spread((sum_w, sum_wp, sum_wp2): (f64, f64, f64)) -> Spread {
        let center = sum_wp / sum_w;
        let variance = (sum_wp2 / sum_w - center * center).max(0.0);
        Spread { center, deviation: variance.sqrt() }
    }

    /// The reading at the last bar taken, whatever the weights summed to.
    pub fn current(&self) -> Spread {
        Vwap::spread((self.sum_w, self.sum_wp, self.sum_wp2))
    }
}

impl Study for Vwap {
    type Value = Spread;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, new_period: bool) -> Option<Spread> {
        let sums = self.next(bar, new_period);
        (self.sum_w, self.sum_wp, self.sum_wp2) = sums;
        (sums.0 > 0.0 || sums.0.is_nan()).then(|| Vwap::spread(sums))
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Option<Spread> {
        let sums = self.next(bar, new_period);
        (sums.0 > 0.0 || sums.0.is_nan()).then(|| Vwap::spread(sums))
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "vwap"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("source", self.source as u8 as f64)]
    }
}
