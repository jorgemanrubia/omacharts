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
//! # Readings
//!
//! Each indicator in [`INDICATORS`] reads, a bar at a time, as one of this
//! crate's types, and the chart draws exactly that reading:
//!
//! | indicator | a bar's reading |
//! | --- | --- |
//! | `sma`, `ema`, `rsi`, `atr`, `volume` | `f64` ([`Sma`], [`Ema`], [`Rsi`], [`Atr`], [`Volume`]) |
//! | `macd` | [`MacdValue`] `{ line, signal, histogram }` |
//! | `stochastic` | [`StochasticValue`] `{ k, d }` |
//! | `vwap`, `bollinger` | [`Spread`] `{ center, deviation }`, a band `k` out at [`Spread::upper`] and [`Spread::lower`] |
//! | `cvd`, `volume_delta` | [`DeltaBar`] `{ open, high, low, close, estimated, starts }`: [`CvdEstimate`] and [`BarDeltaEstimate`] from bars, [`VolumeDelta`] from volume at price |
//! | `volume_profile` | [`volume_profile::Profile`], a whole period's |
//! | `market_profile` | [`ProfileReading`] `{ poc, vah, val }`, so far: [`DevelopingMarketProfile`] |
//! | `footprint` | [`footprint::BarFootprint`], a bar's rows of [`footprint::Sides`] |
//! | `large_orders` | [`orders::Placed`] in an [`orders::Book`], from [`orders::Builder`] |
//! | `session_times` | none: a calendar |
//!
//! Live, a study is pushed each bar as it closes ([`Study::step`]) and
//! asked about the bar still forming on every print ([`Study::peek`]),
//! which changes nothing; stepping that bar once it closes answers what the
//! last peek did, to the bit.
//!
//! ```
//! use omacharts_studies::{Bar, Macd, Study};
//!
//! let mut macd = Macd::new(12, 26, 9);
//! let closed = (0..40).map(|i| {
//!     let close = 100.0 + (i as f64 * 0.7).sin();
//!     Bar { ts: i * 60, open: close, high: close + 0.5, low: close - 0.5, close, volume: 10.0 }
//! });
//! for bar in closed {
//!     macd.step(&bar, false);
//! }
//! let forming = Bar { ts: 40 * 60, open: 100.0, high: 101.0, low: 99.5, close: 100.8, volume: 3.0 };
//! let now = macd.peek(&forming, false).expect("warmed up");
//! assert_eq!(macd.step(&forming, false), Some(now));
//! println!("line {} signal {:?} histogram {:?}", now.line, now.signal, now.histogram);
//! ```
//!
//! The crate has no dependencies and no `unsafe`.

#![forbid(unsafe_code)]

mod window;

pub mod at_price;
pub mod average;
pub mod catalog;
pub mod bollinger;
pub mod cvd;
pub mod footprint;
pub mod market_profile;
pub mod oscillators;
pub mod orders;
pub mod series;
pub mod volume;
pub mod volume_profile;
pub mod vwap;

pub use at_price::{AtPrice, AtPriceStudy, DeltaReading, DevelopingProfile, ProfileReading, VolumeDelta};
pub use average::{Ema, EmaOf, Sma};
pub use bollinger::Bollinger;
pub use cvd::{BarDeltaEstimate, CvdEstimate, DeltaBar};
pub use catalog::{Form, AT_PRICE, DEVELOPING, INDICATORS};
pub use market_profile::DevelopingMarketProfile;
pub use orders::{Aggressor, Combiner, Order, Print, Thresholds};
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
