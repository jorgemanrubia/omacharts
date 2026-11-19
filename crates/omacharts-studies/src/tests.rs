use super::*;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn series(seed: u64, n: usize) -> Vec<Bar> {
    let mut rng = Lcg(seed);
    let mut close = 101.37;
    (0..n)
        .map(|i| {
            let open = close;
            close = open * (1.0 + (rng.next() - 0.5) * 0.02);
            if i % 17 == 0 {
                close = open; // a bar that closes where it opened
            }
            let high = open.max(close) * (1.0 + rng.next() * 0.004);
            let low = open.min(close) * (1.0 - rng.next() * 0.004);
            let volume = if i % 13 == 0 { 0.0 } else { rng.next() * 1_000.0 };
            Bar { ts: i as i64 * 60, open, high, low, close, volume }
        })
        .collect()
}

fn period_starts(i: usize) -> bool {
    i.is_multiple_of(97)
}

/// A value, every bit of it, for comparing two runs exactly.
fn bits<T: std::fmt::Debug + ?Sized>(value: &T) -> String {
    format!("{value:?}")
}

/// Holds a study to the three promises the crate makes.
fn holds<S: Study + Clone>(make: impl Fn() -> S)
where
    S::Value: std::fmt::Debug,
{
    let bars = series(3, 700);

    // Peeking at a bar answers what stepping it will, and changes nothing.
    let mut study = make();
    for (i, bar) in bars.iter().enumerate() {
        let new_period = period_starts(i);
        // A bar forming: several prints, each peeked.
        for print in 0..3 {
            let forming = Bar { close: bar.open + (bar.close - bar.open) * print as f64 / 3.0, ..*bar };
            let _ = study.peek(&forming, new_period);
        }
        let peeked = study.peek(bar, new_period);
        let stepped = study.step(bar, new_period);
        assert_eq!(bits(&peeked), bits(&stepped), "peek and step disagree at bar {i}");
    }

    // A prefix of the bars gives the prefix of the values: nothing a study
    // answers at a bar depends on a bar after it.
    let whole = over(make(), &bars, period_starts);
    for cut in [0, 1, 2, 50, 333, 699] {
        let part = over(make(), &bars[..cut], period_starts);
        assert_eq!(bits(&part), bits(&whole[..cut]), "the first {cut} bars differ");
    }

    // The same bars twice give the same values twice.
    assert_eq!(bits(&over(make(), &bars, period_starts)), bits(&whole));
}

#[test]
fn every_study_keeps_its_promises() {
    for period in [0usize, 1, 2, 3, 14, 50] {
        holds(|| Sma::new(period));
        holds(|| Ema::new(period));
        holds(|| Rsi::new(period));
        holds(|| Atr::new(period));
        holds(|| Atr::smoothed(period, Smoothing::Mean));
        holds(|| Bollinger::new(period));
    }
    for (p, k, d) in [(14, 1, 3), (14, 3, 3), (1, 1, 1), (0, 3, 3), (5, 2, 9)] {
        holds(|| Stochastic::new(p, k, d));
    }
    for (f, s, g) in [(12, 26, 9), (26, 12, 9), (1, 1, 1), (0, 26, 9), (3, 10, 16)] {
        holds(|| Macd::new(f, s, g));
    }
    for source in [Source::Hlc3, Source::Hl2, Source::Ohlc4, Source::Close] {
        holds(|| Vwap::new(source));
    }
    holds(CvdEstimate::new);
    holds(Volume::new);
}

#[test]
fn every_stepped_indicator_in_the_catalog_has_a_study_by_its_key() {
    let keys = [
        Sma::new(1).key(),
        Ema::new(1).key(),
        Vwap::new(Source::Hlc3).key(),
        Bollinger::new(1).key(),
        Macd::new(1, 2, 3).key(),
        Volume::new().key(),
        CvdEstimate::new().key(),
        Rsi::new(1).key(),
        Atr::new(1).key(),
        Stochastic::new(1, 1, 1).key(),
    ];
    let stepped: Vec<&str> =
        INDICATORS.iter().filter(|(_, form)| *form == Form::Stepped).map(|(key, _)| *key).collect();
    let mut have = keys.to_vec();
    have.sort_unstable();
    let mut want = stepped.clone();
    want.sort_unstable();
    assert_eq!(have, want);
}

#[test]
fn a_mean_true_range_forgets_a_bar_once_it_has_left_the_window() {
    let bar = |high: f64, low: f64, close: f64| Bar { high, low, close, open: close, ..Bar::default() };
    let mut mean = Atr::smoothed(2, Smoothing::Mean);
    let mut wilder = Atr::new(2);
    let bars = [bar(10.0, 0.0, 5.0), bar(6.0, 4.0, 5.0), bar(6.0, 4.0, 5.0), bar(6.0, 4.0, 5.0)];
    let (mut m, mut w) = (None, None);
    for bar in &bars {
        m = mean.step(bar, false);
        w = wilder.step(bar, false);
    }
    assert_eq!(m, Some(2.0), "the violent first bar has left a two-bar window");
    assert!(w.unwrap() > 2.0, "Wilder's still carries some of it");
}

#[test]
fn the_first_value_comes_after_the_warmup() {
    let bars = series(5, 200);
    fn check<S: Study>(study: S, bars: &[Bar]) {
        let warmup = study.warmup();
        let values = over(study, bars, |_| false);
        assert!(values[..warmup].iter().all(Option::is_none), "a value before the end of the warm-up, {warmup}");
        assert!(values[warmup].is_some(), "nothing at the end of the warm-up, bar {warmup}");
    }
    check(Sma::new(20), &bars);
    check(Ema::new(20), &bars);
    check(Rsi::new(14), &bars);
    check(Atr::new(14), &bars);
    check(Bollinger::new(20), &bars);
    check(Stochastic::new(14, 3, 3), &bars);
    check(Macd::new(12, 26, 9), &bars);
}

#[test]
fn a_sum_starts_where_iterator_sum_does() {
    // The seeds of the averages start from -0.0 because `Iterator::sum`
    // does; if the standard library ever changes that, the averages here
    // stop matching anything computed with it, and this says so first.
    let empty: [f64; 0] = [];
    assert!(empty.iter().sum::<f64>().is_sign_negative());
}

#[test]
fn the_crate_has_no_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    let section = manifest.split("[dependencies]").nth(1).unwrap_or("");
    let declared: Vec<&str> = section
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    assert!(declared.is_empty(), "this crate must stay dependency-free: {declared:?}");
    assert!(!manifest.contains("[dev-dependencies]"), "no dev-dependencies either");
}

#[test]
fn a_series_computed_in_place_is_the_studies_to_the_bit() {
    let mut bars = series(9, 900);
    bars[300].close = f64::NAN; // a hole a Bollinger has to wait out
    for period in [0usize, 1, 2, 3, 14, 20, 50, 300] {
        assert_eq!(bits(&series::sma(&bars, period)), bits(&over(Sma::new(period), &bars, |_| false)), "sma {period}");
        assert_eq!(bits(&series::ema(&bars, period)), bits(&over(Ema::new(period), &bars, |_| false)), "ema {period}");
        let mut spreads = vec![None; bars.len()];
        series::bollinger(&bars, period, |i, spread| spreads[i] = Some(spread));
        assert_eq!(bits(&spreads), bits(&over(Bollinger::new(period), &bars, |_| false)), "bollinger {period}");
    }
    // The study's sum skips the series' "is the window full" branch by
    // taking off the slot's 0.0 instead, which must change nothing even
    // where the closes are the awkward ones and arrive while it fills.
    let mut awkward = series(3, 64);
    for (i, close) in [-0.0, -0.0, f64::INFINITY, -0.0, f64::NEG_INFINITY, f64::NAN, 0.0, -0.0].into_iter().enumerate() {
        awkward[i].close = close;
        awkward[40 + i].close = close;
    }
    for period in [1usize, 2, 3, 5, 9, 30] {
        assert_eq!(bits(&series::sma(&awkward, period)), bits(&over(Sma::new(period), &awkward, |_| false)), "awkward sma {period}");
        assert_eq!(bits(&series::ema(&awkward, period)), bits(&over(Ema::new(period), &awkward, |_| false)), "awkward ema {period}");
    }
    for (p, k, d) in [(14, 1, 3), (14, 3, 3), (1, 1, 1), (0, 3, 3), (5, 2, 9), (3, 9, 2)] {
        let (ks, ds) = series::stochastic(&bars, p, k, d);
        let stepped = over(Stochastic::new(p, k, d), &bars, |_| false);
        assert_eq!(bits(&ks), bits(&stepped.iter().map(|v| v.map(|v| v.k)).collect::<Vec<_>>()), "%K {p} {k} {d}");
        assert_eq!(bits(&ds), bits(&stepped.iter().map(|v| v.and_then(|v| v.d)).collect::<Vec<_>>()), "%D {p} {k} {d}");
    }
}
