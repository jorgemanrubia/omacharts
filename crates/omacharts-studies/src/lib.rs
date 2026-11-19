//! Indicators, one bar at a time.
//!
//! Every indicator here is a [`Study`]: a small state machine that is handed
//! one bar and answers with its value at that bar. It is never handed a
//! slice, so it cannot look at a bar that has not happened yet — the
//! no-lookahead guarantee is a property of the signature rather than of
//! anybody's care. A whole series is the same step folded ([`over`]); a bar
//! still forming is answered by [`Study::peek`], which reads the state
//! without changing it, so a live print costs one call per indicator rather
//! than the series computed again.
//!
//! **The numbers are the chart's numbers, to the bit.** Each study performs
//! the same floating-point operations in the same order as the series
//! functions it replaced, including where a sum starts (`-0.0`, which is
//! where `Iterator::sum` starts) and the order a window is folded in. A
//! golden test over every indicator and parameter holds the engine to it.
//!
//! **Periods are the caller's.** A study that starts again at a period — a
//! VWAP each session, a cumulative delta each day — is told that one began
//! through `new_period`. Where sessions start is a question of exchanges,
//! time zones and daylight saving, which this crate knows nothing about and
//! does not depend on anything to learn.
//!
//! **Nothing allocates after construction.** A study's windows are sized
//! when it is made; `step` and `peek` only read and write them.
//!
//! The crate has no dependencies and no `unsafe`.

#![forbid(unsafe_code)]

mod window;

pub mod at_price;
pub mod average;
pub mod catalog;
pub mod bollinger;
pub mod cvd;
pub mod market_profile;
pub mod oscillators;
pub mod series;
pub mod volume;
pub mod vwap;

pub use at_price::{AtPrice, AtPriceStudy, DeltaReading, DevelopingProfile, ProfileReading, VolumeDelta};
pub use average::{Ema, EmaOf, Sma};
pub use bollinger::Bollinger;
pub use cvd::CvdEstimate;
pub use catalog::{Form, AT_PRICE, DEVELOPING, INDICATORS};
pub use market_profile::DevelopingMarketProfile;
pub use oscillators::{Atr, Macd, MacdValue, Rsi, Smoothing, Stochastic, StochasticValue};
pub use volume::Volume;
pub use vwap::{Source, Spread, Vwap};

/// Bumped by any change that alters a value any study produces, so that
/// anything which keeps a study's output can tell that it was produced by
/// different arithmetic.
pub const NUMBERS_VERSION: u32 = 1;

/// What a study reads from a bar.
///
/// A trait rather than a struct so that a caller's own bar type is read in
/// place: nothing is copied into a second shape on the way in, and the call
/// is resolved at compile time.
pub trait Ohlcv {
    /// Unix seconds the bar opens at.
    fn ts(&self) -> i64;
    fn open(&self) -> f64;
    fn high(&self) -> f64;
    fn low(&self) -> f64;
    fn close(&self) -> f64;
    fn volume(&self) -> f64;
}

/// A bar, for callers without one of their own.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Bar {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Ohlcv for Bar {
    #[inline(always)]
    fn ts(&self) -> i64 {
        self.ts
    }
    #[inline(always)]
    fn open(&self) -> f64 {
        self.open
    }
    #[inline(always)]
    fn high(&self) -> f64 {
        self.high
    }
    #[inline(always)]
    fn low(&self) -> f64 {
        self.low
    }
    #[inline(always)]
    fn close(&self) -> f64 {
        self.close
    }
    #[inline(always)]
    fn volume(&self) -> f64 {
        self.volume
    }
}

/// An indicator computed one bar at a time.
pub trait Study {
    /// One reading: a number, or a small struct for an indicator that draws
    /// more than one line.
    type Value: Copy;

    /// Take the next bar, which has closed, and answer the value at it.
    ///
    /// `new_period` says this bar opens a period the study restarts at; a
    /// study that has none ignores it. `None` means there is not enough
    /// history yet — never a fabricated number.
    fn step<B: Ohlcv>(&mut self, bar: &B, new_period: bool) -> Option<Self::Value>;

    /// The value [`Study::step`] would answer for `bar`, without taking it.
    ///
    /// For a bar still forming: called on every print, it reads the state
    /// and changes nothing, and stepping the same bar once it closes answers
    /// the same value to the bit.
    fn peek<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Option<Self::Value>;

    /// Bars taken before the first value can appear.
    fn warmup(&self) -> usize;

    /// The indicator's key, as [`INDICATORS`] names it.
    fn key(&self) -> &'static str;

    /// Its parameters, by name, for describing it: what it was made with,
    /// not what it has worked out. Called when it is set up, never per bar.
    fn params(&self) -> Vec<(&'static str, f64)>;
}

/// A study folded over a series: the value at every bar.
///
/// `new_period(i)` says whether bar `i` opens a period.
pub fn over<S: Study, B: Ohlcv>(
    mut study: S,
    bars: &[B],
    mut new_period: impl FnMut(usize) -> bool,
) -> Vec<Option<S::Value>> {
    let mut out = Vec::with_capacity(bars.len());
    for (at, bar) in bars.iter().enumerate() {
        out.push(study.step(bar, new_period(at)));
    }
    out
}

#[cfg(test)]
mod tests;
