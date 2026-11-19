//! What each study costs a bar, folded over a series and peeked per print.
//!
//! `cargo run --release -p omacharts-studies --example bench`

use std::hint::black_box;
use std::time::Instant;

use omacharts_studies::*;

fn series(n: usize) -> Vec<Bar> {
    let mut close = 5000.0;
    let mut state = 0x2545_f491_4f6c_dd1du64;
    (0..n)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let open = close;
            close += ((state % 17) as f64 - 8.0) * 0.25;
            let high = open.max(close) + (state % 5) as f64 * 0.25;
            let low = open.min(close) - (state % 3) as f64 * 0.25;
            Bar { ts: i as i64 * 60, open, high, low, close, volume: (state % 900) as f64 + 1.0 }
        })
        .collect()
}

/// The cheapest of several runs, in nanoseconds per bar.
fn cheapest(bars: usize, mut run: impl FnMut()) -> f64 {
    (0..15)
        .map(|_| {
            let started = Instant::now();
            run();
            started.elapsed().as_nanos() as f64 / bars as f64
        })
        .fold(f64::INFINITY, f64::min)
}

fn measure<S: Study + Clone>(name: &str, make: impl Fn() -> S, bars: &[Bar]) {
    let fold = cheapest(bars.len(), || {
        black_box(over(make(), bars, |i| i % 1_380 == 0));
    });
    let mut warm = make();
    for bar in &bars[..bars.len() / 2] {
        warm.step(bar, false);
    }
    let rest = &bars[bars.len() / 2..];
    let peek = cheapest(rest.len(), || {
        for bar in rest {
            black_box(warm.peek(bar, false));
        }
    });
    println!("{name:<28} step {fold:>6.2} ns/bar   peek {peek:>6.2} ns/print");
}

fn main() {
    let bars = series(100_000);
    measure("sma 20", || Sma::new(20), &bars);
    measure("sma 200", || Sma::new(200), &bars);
    measure("ema 20", || Ema::new(20), &bars);
    measure("rsi 14", || Rsi::new(14), &bars);
    measure("atr 14", || Atr::new(14), &bars);
    measure("atr 14 mean", || Atr::smoothed(14, Smoothing::Mean), &bars);
    measure("bollinger 20", || Bollinger::new(20), &bars);
    measure("stochastic 14 3 3", || Stochastic::new(14, 3, 3), &bars);
    measure("macd 12 26 9", || Macd::new(12, 26, 9), &bars);
    measure("vwap", || Vwap::new(Source::Hlc3), &bars);
    measure("cvd estimate", CvdEstimate::new, &bars);
    measure("volume", Volume::new, &bars);
    measure("developing market profile", || DevelopingMarketProfile::new(0.25, 1, 30), &bars);

    // The windowed ones over a series in place, as a chart computes them.
    let n = bars.len();
    let sma = cheapest(n, || {
        black_box(series::sma(&bars, 20));
    });
    let bollinger = cheapest(n, || {
        let mut out = vec![None; n];
        series::bollinger(&bars, 20, |i, spread| out[i] = Some(spread));
        black_box(out);
    });
    let stochastic = cheapest(n, || {
        black_box(series::stochastic(&bars, 14, 3, 3));
    });
    println!("series sma 20                {sma:>6.2} ns/bar");
    println!("series bollinger 20          {bollinger:>6.2} ns/bar");
    println!("series stochastic 14 3 3     {stochastic:>6.2} ns/bar");
}
