//! Bollinger bands: a simple moving average with an envelope at a multiple of
//! the standard deviation of close around it.
//!
//! The same picture as a VWAP with bands, which is why it is the same type and
//! the same renderer — see [`super::bands`]. The two differences are the
//! centre, a simple average of closes rather than a volume-weighted average of
//! typical price, and the window, a fixed number of bars rather than everything
//! since the session opened.
//!
//! **Population standard deviation**, dividing by `n` rather than `n - 1`,
//! because that is what TradingView's `ta.stdev` does unless asked otherwise
//! and `ta.bb` does not ask: a band that disagreed with the chart everybody
//! else is looking at would be worse than no band. The difference is a factor
//! of `sqrt(n/(n-1))` — 2.6% at the default twenty bars — which is small
//! enough to look right and large enough to argue about.
//!
//! One pass, rolling. The window's sum and sum of squares are carried from bar
//! to bar, and both are kept as sums of `x - anchor` rather than of `x`: a
//! variance taken as `E[x²] - E[x]²` on raw prices subtracts two numbers of
//! the order of the price squared to get one of the order of the price
//! movement squared, and at index levels that throws away most of the
//! significant digits. Subtracting a figure near the series before squaring
//! makes the quantities squared the size of the movement instead.
//!
//! How much that buys depends on staying near the anchor, and it is worth
//! being exact about which claim is which. Near it — where an instrument's own
//! prices spend a chart — it is several orders of magnitude: over 100,000 bars
//! around 48,000 the rolled deviation comes out 9e-11 from a fresh
//! recomputation of the last window, against 3e-5 unshifted, and a flat window
//! there is exactly nothing. It is not a general guarantee. A series that has
//! travelled a long way from its first close has lost the advantage and can be
//! a shade worse than the unshifted form; a flat window out there lands a hair
//! off nothing rather than on it; and a one-bar window, which is nothing by
//! construction in exact arithmetic, is a few parts in 10^15 of the price in
//! practice. All of those are orders of magnitude below a tick at any price
//! this is drawn at, which is why the anchor is a fixed figure and not a
//! running one.

use crate::bars::Bar;

use super::bands::{Band, Bands};
use super::{LineStyle, Stroke};

/// The band a fresh Bollinger ships with: two standard deviations, drawn.
///
/// One band rather than a VWAP's three, because the multiple is what a
/// Bollinger *is* — a second envelope at 1σ would be a different indicator
/// wearing the same name.
///
/// Drawn, where a VWAP's bands start switched off, and for the same reason: a
/// VWAP is a useful line on its own and its bands are an extra, while a
/// Bollinger with no band is a moving average with a longer name.
///
/// Shaded *and* outlined, which is the one place the convention needs saying
/// out loud. A VWAP's inner band is shaded with no outline because it marks a
/// region, and its outer band is a plain line because it marks a boundary. A
/// Bollinger's single band is both at once — the region price keeps to, and
/// the edge a touch of which is the signal — so it gets both treatments, at
/// the same shading weight and the same line width the VWAP's bands use.
pub fn default_bands() -> Vec<Band> {
    vec![Band {
        deviations: 2.0,
        enabled: true,
        color: None,
        fill: true,
        fill_alpha: None,
        stroke: Stroke::new(1.0, LineStyle::Solid),
    }]
}

/// The widest multiple a band may sit at.
///
/// Not a judgement about what is useful — nobody reads a 40σ band — but a
/// figure past which the number is not a band setting at all, so that the
/// command line has something to refuse. TradingView's own input stops at 50.
pub const MAX_DEVIATIONS: f64 = 50.0;

/// The basis and its bands over `bars`.
///
/// `None` for the first `period - 1` bars, which is exactly how many have no
/// full window behind them, and never a figure averaged over fewer bars than
/// asked for. Value `i` describes bar `i`: the window is the `period` bars
/// ending at `i`, inclusive.
pub fn compute(bars: &[Bar], period: usize, bands: &[Band]) -> Bands {
    let mut out = Bands::blank(bars.len(), bands);
    if period == 0 || bars.len() < period {
        return out;
    }

    // Everything is summed as a distance from here. The first close is the
    // cheapest figure near the series to reach for, and the whole point is
    // that it is subtracted before squaring rather than after — but it has to
    // be a figure: anchoring on a close that is not a number would make every
    // distance from it one too, and the count below, which watches the
    // closes, would never notice.
    let anchor =
        bars.iter().map(|bar| bar.close).find(|close| close.is_finite()).unwrap_or(0.0);
    let n = period as f64;
    let (mut sum, mut sum_sq) = (0.0f64, 0.0f64);
    // How many bars in the window are not a number. A provider that sends one
    // must not silently turn the rest of the series into NaN, so a window
    // holding one has no value — and gets one back the moment it falls out of
    // the window, because the sums never took it in.
    let mut broken = 0usize;

    for i in 0..bars.len() {
        let add = bars[i].close;
        match add.is_finite() {
            true => {
                let d = add - anchor;
                sum += d;
                sum_sq += d * d;
            }
            false => broken += 1,
        }
        if i >= period {
            let drop = bars[i - period].close;
            match drop.is_finite() {
                true => {
                    let d = drop - anchor;
                    sum -= d;
                    sum_sq -= d * d;
                }
                // Saturating because the count cannot go below zero — every
                // bar that leaves the window was added to it once — and a
                // subtraction that underflows in a draw path takes the app
                // down rather than drawing something odd.
                false => broken = broken.saturating_sub(1),
            }
        }
        if i + 1 < period || broken > 0 {
            continue;
        }

        let mean = sum / n;
        // Σ(x-mean)² written in terms of the two running sums. Clamped at zero
        // because a window with no movement can land a hair below it — which
        // it does whenever the distances above are not exactly representable,
        // and not only there.
        let variance = ((sum_sq - sum * mean) / n).max(0.0);
        out.set(i, anchor + mean, variance.sqrt());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(closes: &[f64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                ts: i as i64 * 60,
                open: c,
                high: c + 1.0,
                low: c - 1.0,
                close: c,
                volume: 100.0,
            })
            .collect()
    }

    fn band(deviations: f64) -> Vec<Band> {
        vec![Band {
            deviations,
            enabled: true,
            color: None,
            fill: false,
            fill_alpha: None,
            stroke: Stroke::default(),
        }]
    }

    /// The population standard deviation of a window, worked out the slow
    /// obvious way, for the rolling one to be held against.
    fn stdev(window: &[f64]) -> f64 {
        let n = window.len() as f64;
        let mean = window.iter().sum::<f64>() / n;
        (window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt()
    }

    /// Hand-computed, against TradingView's `ta.bb(close, 5, 2)`.
    ///
    /// Closes 2, 4, 4, 4, 5, 5, 7, 9 — the textbook set whose population
    /// standard deviation is exactly 2. Over the last five of them, 5 5 7 9
    /// and the 4 before them, the average is 6 and the deviation is sqrt(3.2).
    #[test]
    fn the_basis_and_the_bands_are_what_the_formula_says() {
        let series = bars(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        let out = compute(&series, 5, &band(2.0));

        // First window: 2 4 4 4 5. Mean 3.8, variance (3.24+.04+.04+.04+1.44)/5
        // = 0.96, deviation 0.9798.
        assert!((out.center[4].unwrap() - 3.8).abs() < 1e-12);
        let spread = out.bands[0].upper[4].unwrap() - out.center[4].unwrap();
        assert!((spread - 2.0 * 0.96f64.sqrt()).abs() < 1e-12, "{spread}");

        // Last window: 4 5 5 7 9. Mean 6, variance (4+1+1+1+9)/5 = 3.2.
        assert_eq!(out.center[7], Some(6.0));
        let deviation = 3.2f64.sqrt();
        assert!((out.bands[0].upper[7].unwrap() - (6.0 + 2.0 * deviation)).abs() < 1e-12);
        assert!((out.bands[0].lower[7].unwrap() - (6.0 - 2.0 * deviation)).abs() < 1e-12);
    }

    /// The whole of the sample-versus-population question, pinned: the figure
    /// divides by `n`, as `ta.stdev`'s `biased` default does. Over the eight
    /// closes above, the population deviation is exactly 2 and the sample one
    /// is 2.138.
    #[test]
    fn the_deviation_is_the_population_one() {
        let series = bars(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]);
        let out = compute(&series, 8, &band(1.0));
        let spread = out.bands[0].upper[7].unwrap() - out.center[7].unwrap();
        assert!((spread - 2.0).abs() < 1e-12, "population: {spread}");
        assert!((spread - 2.1380899).abs() > 0.1, "not the sample deviation");
    }

    /// Exactly `period - 1` leading `None`s, and value `i` is the window
    /// ending at bar `i`.
    #[test]
    fn it_waits_for_a_full_window_and_not_a_bar_longer() {
        let out = compute(&bars(&[1.0, 2.0, 3.0, 4.0, 5.0]), 3, &band(2.0));
        let warmup = out.center.iter().take_while(|v| v.is_none()).count();
        assert_eq!(warmup, 2, "three bars needs two bars of warm-up");
        assert_eq!(out.center[2], Some(2.0), "the mean of bars 0, 1 and 2");
        assert_eq!(out.center[4], Some(4.0), "the mean of bars 2, 3 and 4");
        assert!(out.bands[0].upper.iter().take(2).all(Option::is_none), "the band waits too");
    }

    /// Alignment, the other way round: a single spike should show up on the
    /// bars whose window contains it and nowhere else.
    #[test]
    fn a_spike_widens_exactly_the_windows_that_hold_it() {
        let mut closes = vec![10.0; 12];
        closes[5] = 20.0;
        let out = compute(&bars(&closes), 3, &band(1.0));
        let widths: Vec<bool> = (0..12)
            .map(|i| out.bands[0].upper[i].map(|u| u - out.center[i].unwrap() > 1e-9).unwrap_or(false))
            .collect();
        // Windows ending at 5, 6 and 7 are the three that contain bar 5.
        assert_eq!(widths[5..=7], [true, true, true]);
        assert!(!widths[4], "the window ending before the spike is flat");
        assert!(!widths[8], "and so is the first one past it");
    }

    /// The basis is a simple moving average, so it has to agree with the one
    /// the chart draws beside it bar for bar.
    #[test]
    fn the_basis_is_the_same_average_the_sma_indicator_draws() {
        let closes: Vec<f64> = (0..200).map(|i| 100.0 + (i as f64 / 7.0).sin() * 9.0).collect();
        let series = bars(&closes);
        let basis = compute(&series, 20, &[]).center;
        let sma = super::super::sma(&series, 20);
        for (at, (a, b)) in basis.iter().zip(&sma).enumerate() {
            match (a, b) {
                (Some(a), Some(b)) => assert!((a - b).abs() < 1e-9, "bar {at}: {a} vs {b}"),
                (None, None) => {}
                _ => panic!("bar {at} is {a:?} here and {b:?} there"),
            }
        }
    }

    /// A flat window has no dispersion. Zero is the answer, not an error and
    /// not a NaN, and the bands sit on the basis — at the start of a series
    /// and in the middle of one alike.
    ///
    /// Exactly zero here because the window sits on the anchor; a flat window
    /// a long way from it lands a hair off instead, which the module comment
    /// says and this does not claim otherwise.
    #[test]
    fn a_flat_window_collapses_the_bands_onto_the_average() {
        let mut closes = vec![48_000.125; 30];
        for (at, close) in closes.iter_mut().enumerate().skip(10).take(5) {
            *close = 48_000.125 + at as f64;
        }
        let out = compute(&bars(&closes), 5, &band(2.0));

        for at in [4usize, 29] {
            let centre = out.center[at].unwrap();
            assert_eq!(out.bands[0].upper[at], Some(centre), "bar {at} upper");
            assert_eq!(out.bands[0].lower[at], Some(centre), "bar {at} lower");
        }
        // And nothing anywhere went non-finite on the way through the bump.
        assert!(out.bands[0].upper.iter().flatten().all(|v| v.is_finite()));
    }

    /// Exactly, at these prices, because the distances from the anchor are
    /// small whole numbers and the arithmetic above cancels to the bit. At an
    /// index level it is a few parts in 10^15 of the price rather than nought
    /// — far below a tick, and the reason the claim here is about these
    /// prices and not about every price.
    #[test]
    fn a_window_of_one_bar_is_the_price_with_no_width_at_all() {
        let out = compute(&bars(&[10.0, 20.0, 30.0]), 1, &band(2.0));
        assert_eq!(out.center, vec![Some(10.0), Some(20.0), Some(30.0)]);
        assert_eq!(out.bands[0].upper, vec![Some(10.0), Some(20.0), Some(30.0)]);

        // And at an index level it is nothing anybody could see.
        let index: Vec<f64> = (0..100).map(|i| 48_000.0 + i as f64 * 0.01).collect();
        let out = compute(&bars(&index), 1, &band(2.0));
        for (at, upper) in out.bands[0].upper.iter().enumerate() {
            let width = upper.unwrap() - out.center[at].unwrap();
            assert!(width.abs() < 1e-6, "bar {at} is {width:e} wide");
        }
    }

    #[test]
    fn degenerate_inputs_draw_nothing_rather_than_guess() {
        assert!(compute(&bars(&[1.0, 2.0]), 5, &band(2.0)).center.iter().all(Option::is_none));
        assert!(compute(&bars(&[1.0, 2.0]), 0, &band(2.0)).center.iter().all(Option::is_none));
        let empty = compute(&[], 20, &band(2.0));
        assert!(empty.center.is_empty());
        assert!(empty.bands[0].upper.is_empty(), "and the band is still there to style");
    }

    /// Nought deviations is a band on the line, and a negative one mirrors it.
    /// Neither is useful and neither is an error: the engine stays total and
    /// the command line is what refuses a figure nobody meant.
    #[test]
    fn a_multiple_of_nought_or_less_still_draws_something_finite() {
        let series = bars(&[1.0, 5.0, 3.0, 9.0, 2.0]);
        let zero = compute(&series, 3, &band(0.0));
        assert_eq!(zero.bands[0].upper[2], zero.center[2]);

        let negative = compute(&series, 3, &band(-2.0));
        let centre = negative.center[4].unwrap();
        assert!(negative.bands[0].upper[4].unwrap() < centre, "the bands swapped places");
        assert!(negative.bands[0].upper.iter().flatten().all(|v| v.is_finite()));
    }

    /// A bar whose close is not a number has no window it belongs to, and the
    /// windows after it recover. The sums never took it in, so there is
    /// nothing to un-poison.
    #[test]
    fn a_close_that_is_not_a_number_blanks_its_windows_and_nothing_else() {
        for nonsense in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut closes = vec![10.0; 20];
            closes[8] = nonsense;
            let out = compute(&bars(&closes), 4, &band(2.0));

            assert!(out.center[7].is_some(), "{nonsense}: the window before it is fine");
            for at in 8..=11 {
                assert!(out.center[at].is_none(), "{nonsense}: window {at} holds it");
            }
            assert_eq!(out.center[12], Some(10.0), "{nonsense}: and then it recovers");
            assert!(out.center.iter().flatten().all(|v| v.is_finite()));
        }
    }

    /// The first bar is the one the sums are anchored on, so a figure that is
    /// not a number there is the case that would poison every window rather
    /// than its own — the count watches the closes, and every distance from a
    /// NaN is a NaN without a single close being one.
    #[test]
    fn a_first_close_that_is_not_a_number_does_not_poison_the_rest() {
        for nonsense in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut closes: Vec<f64> = (0..20).map(|i| 48_000.0 + i as f64).collect();
            closes[0] = nonsense;
            let out = compute(&bars(&closes), 4, &band(2.0));

            for at in 0..=3 {
                assert!(out.center[at].is_none(), "{nonsense}: window {at} holds it");
            }
            // 48004 to 48007, which averages to 48005.5.
            assert_eq!(out.center[7], Some(48_005.5), "{nonsense}");
            assert!(out.center.iter().flatten().all(|v| v.is_finite()));
            assert!(out.bands[0].upper.iter().flatten().all(|v| v.is_finite()));
        }
    }

    /// And a series of nothing but them, which is the case the fallback is
    /// for: no anchor to be found at all.
    #[test]
    fn a_series_of_nothing_but_nonsense_draws_nothing() {
        let out = compute(&bars(&[f64::NAN; 10]), 4, &band(2.0));
        assert!(out.center.iter().all(Option::is_none));
        assert!(out.bands[0].upper.iter().all(Option::is_none));
    }

    /// A high below its own low is not this indicator's business — it reads
    /// closes — but a chart can hold one, so it must not change the answer.
    #[test]
    fn an_upside_down_bar_does_not_reach_an_indicator_of_closes() {
        let mut series = bars(&[10.0, 11.0, 12.0, 11.0, 10.0]);
        series[2].high = 0.0;
        series[2].low = 99.0;
        let out = compute(&series, 3, &band(2.0));
        assert_eq!(out.center[2], Some(11.0));
        assert!(out.bands[0].upper.iter().flatten().all(|v| v.is_finite()));
    }

    /// The rolling sums against a fresh recomputation of the final window,
    /// over a long series at an index level — which is where the arithmetic
    /// this is written the way it is would come apart.
    #[test]
    fn a_hundred_thousand_bars_of_rolling_do_not_drift() {
        let closes: Vec<f64> = (0..100_000)
            .map(|i| 48_000.0 + (i as f64 / 311.0).sin() * 120.0 + (i as f64 / 17.0).cos())
            .collect();
        let series = bars(&closes);
        let out = compute(&series, 20, &band(1.0));

        let last = out.center.len() - 1;
        let window = &closes[last + 1 - 20..=last];
        let fresh_mean = window.iter().sum::<f64>() / 20.0;
        let fresh_spread = stdev(window);

        let mean_drift = (out.center[last].unwrap() - fresh_mean).abs();
        let spread_drift =
            ((out.bands[0].upper[last].unwrap() - out.center[last].unwrap()) - fresh_spread).abs();
        eprintln!("100k bars: mean off by {mean_drift:e}, deviation off by {spread_drift:e}");
        assert!(mean_drift < 1e-8, "mean drifted by {mean_drift:e}");
        assert!(spread_drift < 1e-8, "deviation drifted by {spread_drift:e}");
    }

    #[test]
    fn a_fresh_bollinger_draws_one_band_at_two_deviations() {
        let bands = default_bands();
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].deviations, 2.0);
        assert!(bands[0].enabled, "a Bollinger with no band is a moving average");
        // Both treatments: the region, and the edge of it.
        assert!(bands[0].fill);
        assert!(!bands[0].stroke.is_hidden());
        assert!(bands[0].color.is_none(), "and it follows the line's colour");
    }
}
