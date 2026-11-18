//! The windowed studies over a whole series at once.
//!
//! A study keeps the window it needs in storage of its own, because one
//! bar at a time there is nowhere else to keep it. Over a series the window
//! is already there — it is the series — so these read it in place: the
//! same arithmetic as the studies, through the same functions, with no
//! storage to make. A test holds each to its study, bit for bit.
//!
//! EMA has no window, but its seeding is worth taking out of the loop.
//! The other studies without a window (RSI, ATR, MACD, VWAP, the cumulative
//! delta) keep nothing but a few numbers and are folded with [`crate::over`]
//! as they are.

use crate::oscillators::{mean, raw_reading};
use crate::vwap::Spread;
use crate::Ohlcv;

/// [`crate::Sma`] at every bar of `bars`.
pub fn sma<B: Ohlcv>(bars: &[B], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 {
        return out;
    }
    // `sum_after`'s arithmetic, in the same order, with the oldest close
    // read off the series rather than handed in as an `Option`.
    let mut sum = 0.0;
    for (i, bar) in bars.iter().enumerate() {
        sum += bar.close();
        if i >= period {
            sum -= bars[i - period].close();
        }
        if i + 1 >= period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

/// [`crate::Ema`] at every bar of `bars`: its seed and its step, in the same
/// order, with the seeding worked out once rather than asked of every bar.
pub fn ema<B: Ohlcv>(bars: &[B], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 || bars.len() < period {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let seed = bars[..period].iter().fold(-0.0, |sum, bar| sum + bar.close());
    let mut value = seed / period as f64;
    out[period - 1] = Some(value);
    for (i, bar) in bars.iter().enumerate().skip(period) {
        value = bar.close() * alpha + value * (1.0 - alpha);
        out[i] = Some(value);
    }
    out
}

/// [`crate::Bollinger`] at every bar of `bars`, handed to `each` with the
/// bar's index where there is one.
pub fn bollinger<B: Ohlcv>(bars: &[B], period: usize, mut each: impl FnMut(usize, Spread)) {
    if period == 0 || bars.len() < period {
        return;
    }
    // `advance`'s arithmetic, in the same order, with what it works out a
    // bar at a time worked out once: the anchor is the first finite close,
    // which is the one `advance` anchors on when it meets it, and the
    // period as a divisor. The sums stay in registers. A test holds the
    // two to the bit.
    let anchor = bars.iter().map(Ohlcv::close).find(|close| close.is_finite()).unwrap_or(0.0);
    let n = period as f64;
    let (mut sum, mut sum_sq) = (0.0f64, 0.0f64);
    let mut broken = 0usize;
    for i in 0..bars.len() {
        let add = bars[i].close();
        if add.is_finite() {
            let d = add - anchor;
            sum += d;
            sum_sq += d * d;
        } else {
            broken += 1;
        }
        if i >= period {
            let drop = bars[i - period].close();
            if drop.is_finite() {
                let d = drop - anchor;
                sum -= d;
                sum_sq -= d * d;
            } else {
                broken = broken.saturating_sub(1);
            }
        }
        if i + 1 < period || broken > 0 {
            continue;
        }
        let mean = sum / n;
        let variance = ((sum_sq - sum * mean) / n).max(0.0);
        each(i, Spread { center: anchor + mean, deviation: variance.sqrt() });
    }
}

/// [`crate::Stochastic`] at every bar of `bars`: %K and %D.
pub fn stochastic<B: Ohlcv>(
    bars: &[B],
    period: usize,
    k_smooth: usize,
    d_period: usize,
) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    let n = bars.len();
    if period == 0 || k_smooth == 0 || d_period == 0 {
        return (vec![None; n], vec![None; n]);
    }
    // Three passes, each a tight loop over one series, rather than one
    // loop doing all three: each pass then reads only what the one before
    // it wrote.
    let mut raw = vec![None; n];
    for i in period.saturating_sub(1)..n {
        let before = &bars[i + 1 - period..i];
        raw[i] = raw_reading(before.iter().map(Ohlcv::low), before.iter().map(Ohlcv::high), &bars[i]);
    }
    let mut k = vec![None; n];
    for i in k_smooth.saturating_sub(1)..n {
        k[i] = mean(raw[i + 1 - k_smooth..i].iter().copied(), raw[i], k_smooth);
    }
    let mut d = vec![None; n];
    for i in d_period.saturating_sub(1)..n {
        d[i] = mean(k[i + 1 - d_period..i].iter().copied(), k[i], d_period);
    }
    (k, d)
}
