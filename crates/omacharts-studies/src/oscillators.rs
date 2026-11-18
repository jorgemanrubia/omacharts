//! Oscillators: RSI, ATR, stochastic and MACD.

use crate::average::{Ema, EmaOf, Wilder};
use crate::window::Window;
use crate::{Ohlcv, Study};

/// Relative strength index, Wilder's: the smoothed gains against the
/// smoothed losses of each close on the one before it. The first reading is
/// at bar `period`, the first bar with `period` changes behind it.
#[derive(Clone, Debug)]
pub struct Rsi {
    period: usize,
    previous: Option<f64>,
    gains: Wilder,
    losses: Wilder,
}

impl Rsi {
    pub fn new(period: usize) -> Rsi {
        Rsi { period, previous: None, gains: Wilder::new(period), losses: Wilder::new(period) }
    }

    #[inline(always)]
    fn next(&self, close: f64) -> Option<(Wilder, Wilder, Option<f64>)> {
        let previous = self.previous?;
        let change = close - previous;
        let (gains, gain) = self.gains.next(change.max(0.0));
        let (losses, loss) = self.losses.next((-change).max(0.0));
        let value = match (gain, loss) {
            (Some(gain), Some(loss)) => Some(if loss <= f64::EPSILON {
                100.0
            } else {
                100.0 - 100.0 / (1.0 + gain / loss)
            }),
            _ => None,
        };
        Some((gains, losses, value))
    }
}

impl Study for Rsi {
    type Value = f64;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        let next = self.next(bar.close());
        self.previous = Some(bar.close());
        let (gains, losses, value) = next?;
        self.gains = gains;
        self.losses = losses;
        value
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        self.next(bar.close())?.2
    }

    fn warmup(&self) -> usize {
        self.period
    }

    fn key(&self) -> &'static str {
        "rsi"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("period", self.period as f64)]
    }
}

/// How an average true range averages its ranges.
///
/// The two disagree by a long way after one violent bar: Wilder's lets it
/// fade out, a plain mean forgets it completely once it leaves the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Smoothing {
    /// Wilder's, which is what "ATR" usually means.
    #[default]
    Wilder,
    /// A plain mean of the last `period` true ranges.
    Mean,
}

/// Average true range. The first bar's true range is its own range, having
/// no close before it.
#[derive(Clone, Debug)]
pub struct Atr {
    period: usize,
    previous: Option<f64>,
    ranges: Wilder,
    /// The plain mean's window, for [`Smoothing::Mean`] only.
    mean: Option<crate::average::Sma>,
}

impl Atr {
    /// Wilder-smoothed, which is what "ATR" usually means.
    pub fn new(period: usize) -> Atr {
        Atr::smoothed(period, Smoothing::Wilder)
    }

    pub fn smoothed(period: usize, smoothing: Smoothing) -> Atr {
        let mean = (smoothing == Smoothing::Mean).then(|| crate::average::Sma::new(period));
        Atr { period, previous: None, ranges: Wilder::new(period), mean }
    }

    pub fn smoothing(&self) -> Smoothing {
        if self.mean.is_some() { Smoothing::Mean } else { Smoothing::Wilder }
    }

    #[inline(always)]
    fn range<B: Ohlcv>(&self, bar: &B) -> f64 {
        match self.previous {
            None => bar.high() - bar.low(),
            Some(previous) => (bar.high() - bar.low())
                .max((bar.high() - previous).abs())
                .max((bar.low() - previous).abs()),
        }
    }
}

impl Study for Atr {
    type Value = f64;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        let range = self.range(bar);
        self.previous = Some(bar.close());
        if let Some(mean) = &mut self.mean {
            return mean.step(&Range(range), false);
        }
        let (ranges, value) = self.ranges.next(range);
        self.ranges = ranges;
        value
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<f64> {
        if self.period == 0 {
            return None;
        }
        let range = self.range(bar);
        if let Some(mean) = &self.mean {
            return mean.peek(&Range(range), false);
        }
        self.ranges.next(range).1
    }

    fn warmup(&self) -> usize {
        self.period.saturating_sub(1)
    }

    fn key(&self) -> &'static str {
        "atr"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![("period", self.period as f64), ("mean", f64::from(u8::from(self.mean.is_some())))]
    }
}

/// A true range, read as the close of a bar so an average of closes takes it.
struct Range(f64);

impl Ohlcv for Range {
    fn ts(&self) -> i64 {
        0
    }
    fn open(&self) -> f64 {
        self.0
    }
    fn high(&self) -> f64 {
        self.0
    }
    fn low(&self) -> f64 {
        self.0
    }
    #[inline(always)]
    fn close(&self) -> f64 {
        self.0
    }
    fn volume(&self) -> f64 {
        0.0
    }
}

/// A stochastic's two lines at one bar.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct StochasticValue {
    /// %K: the close's place in the window's range, averaged over `k_smooth`.
    pub k: f64,
    /// %D: %K averaged again over `d_period`.
    pub d: Option<f64>,
}

/// Slow stochastic: where the close sits in the range of the last `period`
/// bars, as a percentage, averaged over `k_smooth` bars for %K and again over
/// `d_period` for %D. A window with no range has no reading, and neither do
/// the averages that include it.
#[derive(Clone, Debug)]
pub struct Stochastic {
    period: usize,
    k_smooth: usize,
    d_period: usize,
    highs: Window<f64>,
    lows: Window<f64>,
    raw: Window<Option<f64>>,
    k: Window<Option<f64>>,
    seen: usize,
}

impl Stochastic {
    pub fn new(period: usize, k_smooth: usize, d_period: usize) -> Stochastic {
        Stochastic {
            period,
            k_smooth,
            d_period,
            highs: Window::new(period),
            lows: Window::new(period),
            raw: Window::new(k_smooth),
            k: Window::new(d_period),
            seen: 0,
        }
    }

    fn disabled(&self) -> bool {
        self.period == 0 || self.k_smooth == 0 || self.d_period == 0
    }

    /// The raw reading, %K and %D at the next bar.
    #[inline(always)]
    fn next<B: Ohlcv>(&self, bar: &B) -> (Option<f64>, Option<f64>, Option<f64>) {
        let seen = self.seen + 1;
        let raw = (seen >= self.period)
            .then(|| {
                raw_reading(self.lows.staying().iter().copied(), self.highs.staying().iter().copied(), bar)
            })
            .flatten();
        let k = (seen >= self.k_smooth).then(|| mean(self.raw.staying().iter().copied(), raw, self.k_smooth)).flatten();
        let d = (seen >= self.d_period).then(|| mean(self.k.staying().iter().copied(), k, self.d_period)).flatten();
        (raw, k, d)
    }
}

/// Where `bar`'s close sits in the range of the window, given the lows and
/// highs of the bars before it in the window, oldest first, as a
/// percentage; `None` for a window with no range. Folded oldest first with
/// the bar itself last, as a fold over the window of a series goes.
#[inline(always)]
pub(crate) fn raw_reading<B: Ohlcv>(
    lows: impl Iterator<Item = f64>,
    highs: impl Iterator<Item = f64>,
    bar: &B,
) -> Option<f64> {
    let low = f64::min(lows.fold(f64::INFINITY, f64::min), bar.low());
    let high = f64::max(highs.fold(f64::NEG_INFINITY, f64::max), bar.high());
    (high - low > f64::EPSILON).then(|| 100.0 * (bar.close() - low) / (high - low))
}

/// The mean of the readings before, oldest first, and `next`: `None` where
/// one of them is `None`, summed from where `Iterator::sum` starts.
#[inline(always)]
pub(crate) fn mean(staying: impl Iterator<Item = Option<f64>>, next: Option<f64>, period: usize) -> Option<f64> {
    let mut sum = -0.0;
    for value in staying {
        sum += value?;
    }
    sum += next?;
    Some(sum / period as f64)
}

impl Study for Stochastic {
    type Value = StochasticValue;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, _: bool) -> Option<StochasticValue> {
        if self.disabled() {
            return None;
        }
        let (raw, k, d) = self.next(bar);
        self.highs.push(bar.high());
        self.lows.push(bar.low());
        self.raw.push(raw);
        self.k.push(k);
        self.seen += 1;
        k.map(|k| StochasticValue { k, d })
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, _: bool) -> Option<StochasticValue> {
        if self.disabled() {
            return None;
        }
        let (_, k, d) = self.next(bar);
        k.map(|k| StochasticValue { k, d })
    }

    fn warmup(&self) -> usize {
        (self.period + self.k_smooth).saturating_sub(2)
    }

    fn key(&self) -> &'static str {
        "stochastic"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("period", self.period as f64),
            ("k_smooth", self.k_smooth as f64),
            ("d_period", self.d_period as f64),
        ]
    }
}

/// A MACD's three series at one bar.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct MacdValue {
    /// The quick average less the slow one.
    pub line: f64,
    /// The line averaged over `signal` bars.
    pub signal: Option<f64>,
    /// The line less its signal.
    pub histogram: Option<f64>,
}

/// Moving average convergence/divergence: two exponential averages of the
/// close and the gap between them, with that gap averaged again for a
/// signal line. A gap that is not a finite number is a hole, and the signal
/// starts again after it.
#[derive(Clone, Debug)]
pub struct Macd {
    enabled: bool,
    signal_period: usize,
    fast: Ema,
    slow: Ema,
    signal: EmaOf,
}

impl Macd {
    pub fn new(fast: usize, slow: usize, signal: usize) -> Macd {
        Macd {
            enabled: fast != 0 && slow != 0 && signal != 0,
            signal_period: signal,
            fast: Ema::new(fast),
            slow: Ema::new(slow),
            signal: EmaOf::new(signal),
        }
    }

    #[inline(always)]
    fn value(line: Option<f64>, signal: Option<f64>) -> Option<MacdValue> {
        let histogram = match (line, signal) {
            (Some(line), Some(signal)) => Some(line - signal),
            _ => None,
        };
        line.map(|line| MacdValue { line, signal, histogram })
    }

    #[inline(always)]
    fn line(quick: Option<f64>, slower: Option<f64>) -> Option<f64> {
        match (quick, slower) {
            (Some(quick), Some(slower)) => Some(quick - slower),
            _ => None,
        }
        .filter(|value| value.is_finite())
    }
}

impl Study for Macd {
    type Value = MacdValue;

    #[inline]
    fn step<B: Ohlcv>(&mut self, bar: &B, new_period: bool) -> Option<MacdValue> {
        if !self.enabled {
            return None;
        }
        let quick = self.fast.step(bar, new_period);
        let slower = self.slow.step(bar, new_period);
        let line = Macd::line(quick, slower);
        let (signal_state, signal) = self.signal.next(line);
        self.signal = signal_state;
        Macd::value(line, signal)
    }

    #[inline]
    fn peek<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Option<MacdValue> {
        if !self.enabled {
            return None;
        }
        let line = Macd::line(self.fast.peek(bar, new_period), self.slow.peek(bar, new_period));
        Macd::value(line, self.signal.next(line).1)
    }

    fn warmup(&self) -> usize {
        self.fast.warmup().max(self.slow.warmup()) + self.signal_period.saturating_sub(1)
    }

    fn key(&self) -> &'static str {
        "macd"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        let (fast, slow) = (self.fast.params()[0].1, self.slow.params()[0].1);
        vec![("fast", fast), ("slow", slow), ("signal", self.signal_period as f64)]
    }
}
