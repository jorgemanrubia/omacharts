//! A bar's volume, as a study, so it is read the way every other line is.

use crate::{Ohlcv, Study};

/// The volume of each bar.
#[derive(Clone, Copy, Debug, Default)]
pub struct Volume;

impl Volume {
    pub fn new() -> Volume {
        Volume
    }
}

impl Study for Volume {
    type Value = f64;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<f64> {
        Some(bar.volume())
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<f64> {
        Some(bar.volume())
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "volume"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        Vec::new()
    }
}
