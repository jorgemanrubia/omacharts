//! Indicators that need their own pane: they are not prices, so they cannot
//! share the price scale.
//!
//! RSI and ATR are Wilder's, the stochastic is the slow one, and the MACD is
//! Appel's — and every one of them is computed the way TradingView computes
//! it, because a number that disagrees with the chart everybody else is
//! looking at is worse than no number. Wilder's smoothing is an exponential
//! average with alpha `1/period` rather than `2/(period+1)`, seeded with the
//! simple average of the first `period` values, which is why an RSI here
//! matches one there bar for bar rather than drifting apart over a long
//! series. The MACD's averages are seeded the same way, for the same reason.
//!
//! The MACD is also the one strip here whose values go either side of nothing,
//! so it is where the strip drawing learned not to assume a floor of zero.

use crate::bars::Bar;

/// What a strip draws: a value per bar, and `None` on a bar it has none for.
///
/// Named because a MACD answers with three of them, and three of the long
/// spelling in one signature says less than three of the short one.
pub type Series = Vec<Option<f64>>;

/// Wilder's smoothing, seeded with the simple average of the first `period`.
///
/// `None` until there is enough history, never a number made up to fill the
/// gap: a chart that draws an RSI from bar one is drawing a guess.
fn wilder(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }
    let mut average: f64 = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(average);
    for (i, value) in values.iter().enumerate().skip(period) {
        average += (value - average) / period as f64;
        out[i] = Some(average);
    }
    out
}

/// Relative strength index, 0 to 100.
///
/// The first bar has no change to measure, so everything is shifted one place:
/// the first reading lands on bar `period`, not `period - 1`.
pub fn rsi(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 || bars.len() <= period {
        return out;
    }

    let mut gains = Vec::with_capacity(bars.len() - 1);
    let mut losses = Vec::with_capacity(bars.len() - 1);
    for pair in bars.windows(2) {
        let change = pair[1].close - pair[0].close;
        gains.push(change.max(0.0));
        losses.push((-change).max(0.0));
    }

    let average_gain = wilder(&gains, period);
    let average_loss = wilder(&losses, period);
    for i in 0..gains.len() {
        let (Some(gain), Some(loss)) = (average_gain[i], average_loss[i]) else { continue };
        // A run with no down closes at all is RSI 100, not a division by zero.
        out[i + 1] = Some(if loss <= f64::EPSILON {
            100.0
        } else {
            100.0 - 100.0 / (1.0 + gain / loss)
        });
    }
    out
}

/// Average true range, in the instrument's own price units.
pub fn atr(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 || bars.is_empty() {
        return out;
    }

    // The first bar has no previous close, so its true range is just its own
    // range. Dropping it instead would shift every later value by one.
    let mut ranges = Vec::with_capacity(bars.len());
    ranges.push(bars[0].high - bars[0].low);
    for pair in bars.windows(2) {
        let (previous, bar) = (&pair[0], &pair[1]);
        ranges.push(
            (bar.high - bar.low)
                .max((bar.high - previous.close).abs())
                .max((bar.low - previous.close).abs()),
        );
    }

    for (i, value) in wilder(&ranges, period).into_iter().enumerate() {
        out[i] = value;
    }
    out
}

/// Stochastic oscillator: %K and %D, both 0 to 100.
///
/// Where the close sits in the range of the last `period` bars, smoothed over
/// `k_smooth` bars into %K, and %K smoothed again over `d_period` into %D —
/// TradingView's slow stochastic, simple averages both times.
///
/// A window with no range at all has no answer, so it is `None` rather than a
/// midpoint nobody measured, and the averages that would take it in wait for it
/// to pass, as they do there.
pub fn stochastic(
    bars: &[Bar],
    period: usize,
    k_smooth: usize,
    d_period: usize,
) -> (Vec<Option<f64>>, Vec<Option<f64>>) {
    if period == 0 || k_smooth == 0 || d_period == 0 {
        return (vec![None; bars.len()], vec![None; bars.len()]);
    }

    let mut raw = vec![None; bars.len()];
    for i in period - 1..bars.len() {
        let window = &bars[i + 1 - period..=i];
        let low = window.iter().map(|bar| bar.low).fold(f64::INFINITY, f64::min);
        let high = window.iter().map(|bar| bar.high).fold(f64::NEG_INFINITY, f64::max);
        if high - low > f64::EPSILON {
            raw[i] = Some(100.0 * (bars[i].close - low) / (high - low));
        }
    }

    let k = mean_of(&raw, k_smooth);
    let d = mean_of(&k, d_period);
    (k, d)
}

/// Simple average of the last `period` values, `None` while any of them is.
fn mean_of(values: &[Option<f64>], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    for i in period.saturating_sub(1)..values.len() {
        let window = &values[i + 1 - period..=i];
        out[i] = window.iter().copied().sum::<Option<f64>>().map(|sum| sum / period as f64);
    }
    out
}

/// Moving average convergence/divergence: the line, its signal, and the
/// histogram between them.
///
/// The difference of two exponential averages of close — `fast` less `slow` —
/// then an exponential average of that difference over `signal`, and the gap
/// between the two drawn as bars. TradingView's `ta.macd` to the letter,
/// including the seeding, which is the usual reason two platforms disagree
/// about this one: both averages here are [`super::ema`], seeded with the
/// simple average of their first `period` closes, so the fast average is the
/// same number an EMA overlay of the same length would draw on the same chart.
///
/// Unlike every other strip here, all three series go negative — that is the
/// whole of what they say, which side of the slow average the fast one is on —
/// so the strip they are drawn in fits what it is given rather than assuming a
/// floor of zero.
///
/// The warm-up follows from the seeding and is worth stating, because it is
/// what a chart compared against another platform is compared on: the line
/// starts where the longer of the two averages does — bar `slow - 1`, for the
/// `fast` under `slow` that a MACD means — and the signal needs `signal` of
/// those before it has a seed of its own, so it starts at bar
/// `slow + signal - 2`. For the usual 12, 26 and 9 that is bar 25 and bar 33.
pub fn macd(bars: &[Bar], fast: usize, slow: usize, signal: usize) -> (Series, Series, Series) {
    let blank = || vec![None; bars.len()];
    if fast == 0 || slow == 0 || signal == 0 {
        return (blank(), blank(), blank());
    }

    let quick = super::ema(bars, fast);
    let slower = super::ema(bars, slow);
    let line: Series = quick
        .iter()
        .zip(&slower)
        .map(|pair| match pair {
            (Some(quick), Some(slower)) => Some(quick - slower),
            _ => None,
        })
        // A close that is not a number turns an exponential average into one
        // too, from that bar on, and a figure that is not a figure must not
        // reach the renderer. There is nothing to draw, so nothing is drawn.
        .map(|value| value.filter(|v: &f64| v.is_finite()))
        .collect();

    let signal_line = ema_of(&line, signal);
    let histogram = line
        .iter()
        .zip(&signal_line)
        .map(|pair| match pair {
            (Some(line), Some(signal)) => Some(line - signal),
            _ => None,
        })
        .collect();
    (line, signal_line, histogram)
}

/// Exponential average of a series that has not started yet at bar nought.
///
/// Seeded with the simple average of the first `period` values it finds, which
/// is how [`super::ema`] seeds and how TradingView's `ta.ema` does — so the
/// signal line here is the signal line there.
///
/// A hole in the middle, which only a series that stopped being a number can
/// produce, ends the average and seeds a fresh one on the far side. Carrying a
/// value across a gap would draw a line through bars it was never told about.
fn ema_of(values: &[Option<f64>], period: usize) -> Series {
    let mut out = vec![None; values.len()];
    if period == 0 {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut average: Option<f64> = None;
    let (mut run, mut seed) = (0usize, 0.0f64);

    for (i, value) in values.iter().enumerate() {
        let Some(value) = *value else {
            average = None;
            run = 0;
            seed = 0.0;
            continue;
        };
        match average {
            Some(previous) => {
                let next = value * alpha + previous * (1.0 - alpha);
                average = Some(next);
                out[i] = Some(next);
            }
            None => {
                run += 1;
                seed += value;
                if run == period {
                    let mean = seed / period as f64;
                    average = Some(mean);
                    out[i] = Some(mean);
                }
            }
        }
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

    #[test]
    fn rsi_waits_for_enough_history() {
        let series = bars(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        let values = rsi(&series, 14);
        assert!(values.iter().all(Option::is_none), "no reading without a full period");

        let series = bars(&(0..30).map(|i| 100.0 + i as f64).collect::<Vec<_>>());
        let values = rsi(&series, 14);
        assert!(values[13].is_none(), "the first change is on bar 1, so the first reading is 14");
        assert!(values[14].is_some());
    }

    #[test]
    fn rsi_is_a_hundred_when_nothing_falls() {
        let series = bars(&(0..40).map(|i| 100.0 + i as f64).collect::<Vec<_>>());
        let last = rsi(&series, 14).pop().flatten().unwrap();
        assert!((last - 100.0).abs() < 1e-9, "{last}");
    }

    #[test]
    fn rsi_is_zero_when_nothing_rises() {
        let series = bars(&(0..40).map(|i| 200.0 - i as f64).collect::<Vec<_>>());
        let last = rsi(&series, 14).pop().flatten().unwrap();
        assert!(last.abs() < 1e-9, "{last}");
    }

    #[test]
    fn rsi_sits_at_the_midpoint_when_gains_and_losses_match() {
        // Alternating equal up and down closes: no side has the upper hand.
        let closes: Vec<f64> = (0..60).map(|i| if i % 2 == 0 { 100.0 } else { 101.0 }).collect();
        // Not exactly fifty: the series ends on an up bar, and Wilder's
        // average still carries that. Close to it is the claim.
        let last = rsi(&bars(&closes), 14).pop().flatten().unwrap();
        assert!((last - 50.0).abs() < 3.0, "{last}");
    }

    #[test]
    fn rsi_stays_inside_its_scale() {
        let closes: Vec<f64> =
            (0..200).map(|i| 100.0 + (i as f64 * 0.7).sin() * 12.0 + i as f64 * 0.1).collect();
        for value in rsi(&bars(&closes), 14).into_iter().flatten() {
            assert!((0.0..=100.0).contains(&value), "{value}");
        }
    }

    #[test]
    fn atr_of_a_steady_range_is_that_range() {
        // Every bar two wide, no gaps: the average true range is two.
        let series = bars(&vec![100.0; 40]);
        let last = atr(&series, 14).pop().flatten().unwrap();
        assert!((last - 2.0).abs() < 1e-9, "{last}");
    }

    #[test]
    fn atr_counts_the_gap_not_just_the_bar() {
        // Same two-wide bars, but one opens ten above the last close. True
        // range has to notice; the bar's own high minus low does not.
        let mut series = bars(&vec![100.0; 40]);
        for bar in series.iter_mut().skip(20) {
            bar.open += 10.0;
            bar.high += 10.0;
            bar.low += 10.0;
            bar.close += 10.0;
        }
        let gapped = atr(&series, 14).pop().flatten().unwrap();
        let flat = atr(&bars(&vec![100.0; 40]), 14).pop().flatten().unwrap();
        assert!(gapped > flat, "{gapped} should exceed {flat}");
    }

    #[test]
    fn atr_is_never_negative() {
        let closes: Vec<f64> = (0..120).map(|i| 50.0 + (i as f64 * 0.3).cos() * 8.0).collect();
        for value in atr(&bars(&closes), 14).into_iter().flatten() {
            assert!(value >= 0.0, "{value}");
        }
    }

    #[test]
    fn degenerate_periods_produce_nothing_rather_than_panicking() {
        let series = bars(&[1.0, 2.0, 3.0]);
        assert!(rsi(&series, 0).iter().all(Option::is_none));
        assert!(atr(&series, 0).iter().all(Option::is_none));
        assert!(rsi(&[], 14).is_empty());
        assert!(atr(&[], 14).is_empty());
        let (k, d) = stochastic(&series, 0, 3, 3);
        assert!(k.iter().chain(&d).all(Option::is_none));
        let (k, d) = stochastic(&[], 14, 3, 3);
        assert!(k.is_empty() && d.is_empty());
    }

    fn candle(high: f64, low: f64, close: f64) -> Bar {
        Bar { ts: 0, open: close, high, low, close, volume: 100.0 }
    }

    #[test]
    fn stochastic_waits_for_both_smoothings() {
        let closes: Vec<f64> = (0..40).map(|i| 100.0 + (i as f64 * 0.4).sin() * 5.0).collect();
        let (k, d) = stochastic(&bars(&closes), 14, 3, 3);
        // Fourteen bars for the range, two more for %K's average, two more for %D's.
        assert!(k[14].is_none() && k[15].is_some());
        assert!(d[16].is_none() && d[17].is_some());
    }

    #[test]
    fn stochastic_is_where_the_close_sits_in_the_range() {
        let series = [
            candle(10.0, 0.0, 5.0),
            candle(20.0, 10.0, 20.0),
            candle(20.0, 10.0, 10.0),
            candle(15.0, 12.0, 14.0),
        ];
        let (k, d) = stochastic(&series, 2, 1, 2);
        // Ranges 0-20, 10-20, 10-20: the close at the top, the bottom, and 40%.
        assert_eq!(k, vec![None, Some(100.0), Some(0.0), Some(40.0)]);
        assert_eq!(d, vec![None, None, Some(50.0), Some(20.0)]);
    }

    #[test]
    fn stochastic_has_no_reading_for_a_window_with_no_range() {
        let series: Vec<Bar> = (0..20).map(|_| candle(100.0, 100.0, 100.0)).collect();
        let (k, d) = stochastic(&series, 5, 3, 3);
        assert!(k.iter().chain(&d).all(Option::is_none));
    }

    #[test]
    fn stochastic_stays_inside_its_scale() {
        let closes: Vec<f64> = (0..200).map(|i| 50.0 + (i as f64 * 0.17).sin() * 20.0).collect();
        let (k, d) = stochastic(&bars(&closes), 14, 3, 3);
        for value in k.into_iter().chain(d).flatten() {
            assert!((0.0..=100.0).contains(&value), "{value}");
        }
    }

    /// Hand-computed, every value of it, against `ta.macd(close, 2, 3, 2)`.
    ///
    /// Closes 1 to 8. The two-bar average is seeded at 1.5 on bar 1 and runs
    /// 2.5, 3.5, 4.5, 5.5, 6.5, 7.5; the three-bar one is seeded at 2 on bar 2
    /// and runs 3, 4, 5, 6, 7. Their difference is a half from bar 2 on, so the
    /// signal — seeded with the mean of the first two of those — is a half from
    /// bar 3, and the histogram is nothing at all.
    #[test]
    fn macd_is_the_difference_of_two_averages_and_an_average_of_that() {
        let series = bars(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        let (line, signal, histogram) = macd(&series, 2, 3, 2);

        assert_eq!(line[0], None, "nothing until the slower average starts");
        assert_eq!(line[1], None);
        for (at, value) in line.iter().enumerate().skip(2) {
            assert!((value.unwrap() - 0.5).abs() < 1e-12, "bar {at}: {value:?}");
        }

        assert_eq!(signal[2], None, "the signal needs two readings of the line");
        for (at, value) in signal.iter().enumerate().skip(3) {
            assert!((value.unwrap() - 0.5).abs() < 1e-12, "bar {at}: {value:?}");
        }

        assert_eq!(histogram[2], None, "and so does the histogram");
        for (at, value) in histogram.iter().enumerate().skip(3) {
            assert!(value.unwrap().abs() < 1e-12, "bar {at}: {value:?}");
        }
    }

    /// The warm-up at the usual settings, which is the number a chart put
    /// beside another platform's is compared on: the line starts at bar 25 and
    /// the signal at bar 33.
    #[test]
    fn the_warmup_at_twelve_twenty_six_and_nine_is_twenty_five_bars_and_then_eight_more() {
        let closes: Vec<f64> = (0..80).map(|i| 100.0 + (i as f64 / 5.0).sin() * 4.0).collect();
        let (line, signal, histogram) = macd(&bars(&closes), 12, 26, 9);

        assert_eq!(line.iter().take_while(|v| v.is_none()).count(), 25);
        assert_eq!(signal.iter().take_while(|v| v.is_none()).count(), 33);
        assert_eq!(histogram.iter().take_while(|v| v.is_none()).count(), 33);
        // And nothing is missing after that: a hole in the middle of a strip
        // is a line that stops for no reason anybody can see.
        assert!(line[25..].iter().all(Option::is_some));
        assert!(signal[33..].iter().all(Option::is_some));
    }

    /// The line has to be the difference of the two averages an EMA overlay of
    /// the same lengths would draw on the same chart, to the last place.
    /// Anything else and the strip is arguing with the lines above it.
    #[test]
    fn the_line_agrees_with_the_two_moving_averages_it_is_made_of() {
        let closes: Vec<f64> =
            (0..300).map(|i| 50.0 + (i as f64 / 23.0).sin() * 6.0 + i as f64 * 0.01).collect();
        let series = bars(&closes);
        let (line, _, _) = macd(&series, 12, 26, 9);
        let fast = super::super::ema(&series, 12);
        let slow = super::super::ema(&series, 26);

        for at in 0..closes.len() {
            match (fast[at], slow[at]) {
                (Some(fast), Some(slow)) => {
                    let wanted = fast - slow;
                    assert!((line[at].unwrap() - wanted).abs() < 1e-12, "bar {at}");
                }
                _ => assert_eq!(line[at], None, "bar {at}"),
            }
        }
    }

    /// Which is the whole point of it: above nothing when the quick average is
    /// over the slow one, below when it is under.
    #[test]
    fn the_line_crosses_nothing_when_the_averages_cross() {
        let mut closes = vec![100.0; 60];
        closes.extend((0..40).map(|i| 100.0 + i as f64)); // a run up
        closes.extend((0..60).map(|i| 140.0 - i as f64 * 2.0)); // and a harder run down
        let (line, _, histogram) = macd(&bars(&closes), 12, 26, 9);

        assert!(line[95].unwrap() > 0.0, "climbing: {:?}", line[95]);
        assert!(line[155].unwrap() < 0.0, "falling: {:?}", line[155]);
        // The histogram changes sign before the line does, which is what it is
        // read for, so it must be allowed to.
        assert!(histogram.iter().flatten().any(|v| *v > 0.0));
        assert!(histogram.iter().flatten().any(|v| *v < 0.0));
    }

    /// A series that does not move has two identical averages, so the line is
    /// nothing — exactly nothing, not a rounding error either side of it.
    #[test]
    fn a_flat_series_has_no_divergence_at_all() {
        let (line, signal, histogram) = macd(&bars(&[48_000.125; 60]), 12, 26, 9);
        for (at, value) in line.iter().enumerate().skip(25) {
            assert_eq!(*value, Some(0.0), "bar {at}");
        }
        for (at, value) in signal.iter().enumerate().skip(33) {
            assert_eq!(*value, Some(0.0), "bar {at}");
            assert_eq!(histogram[at], Some(0.0), "bar {at}");
        }
    }

    #[test]
    fn macd_degenerate_lengths_draw_nothing_rather_than_guess() {
        let series = bars(&(0..60).map(|i| 100.0 + i as f64).collect::<Vec<_>>());
        for (fast, slow, signal) in [(0, 26, 9), (12, 0, 9), (12, 26, 0)] {
            let (line, sig, hist) = macd(&series, fast, slow, signal);
            assert!(line.iter().all(Option::is_none), "{fast}/{slow}/{signal}");
            assert!(sig.iter().all(Option::is_none));
            assert!(hist.iter().all(Option::is_none));
        }

        // Shorter than the slow average: nothing yet, and no panic for it.
        let (line, _, _) = macd(&bars(&[1.0, 2.0, 3.0]), 12, 26, 9);
        assert!(line.iter().all(Option::is_none));

        // Nothing at all.
        let (line, sig, hist) = macd(&[], 12, 26, 9);
        assert!(line.is_empty() && sig.is_empty() && hist.is_empty());
    }

    /// Lengths of one, where the exponential average is the close itself, and
    /// a fast average no faster than the slow one, where the line is nothing
    /// or the wrong way round. Neither is a MACD anybody means — the command
    /// line refuses both — but a stored indicator can hold them and the engine
    /// has to answer.
    #[test]
    fn macd_lengths_that_make_no_sense_still_answer_something_finite() {
        let series = bars(&(0..60).map(|i| 100.0 + (i as f64 / 4.0).sin()).collect::<Vec<_>>());

        let (line, signal, _) = macd(&series, 1, 1, 1);
        assert_eq!(line[0], Some(0.0), "one average less itself");
        assert_eq!(signal[0], Some(0.0));

        // Equal lengths: no divergence to measure.
        let (line, _, _) = macd(&series, 26, 26, 9);
        assert!(line[25..].iter().flatten().all(|v| v.abs() < 1e-12));

        // The fast one slower than the slow one: still finite, just mirrored.
        let (fast_slower, _, _) = macd(&series, 26, 12, 9);
        let (right_way, _, _) = macd(&series, 12, 26, 9);
        for (at, (a, b)) in fast_slower.iter().zip(&right_way).enumerate().skip(25) {
            let (a, b) = (a.unwrap(), b.unwrap());
            assert!((a + b).abs() < 1e-12, "bar {at}: {a} is not {b} mirrored");
        }
        assert!(fast_slower.iter().flatten().all(|v| v.is_finite()));
    }

    /// A close that is not a number stops the line rather than filling the
    /// strip with NaN: an exponential average never recovers from one, so the
    /// honest answer is that there is nothing more to draw.
    #[test]
    fn a_close_that_is_not_a_number_stops_the_line_instead_of_poisoning_it() {
        for nonsense in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut closes: Vec<f64> = (0..60).map(|i| 100.0 + i as f64 * 0.5).collect();
            closes[40] = nonsense;
            let (line, signal, histogram) = macd(&bars(&closes), 12, 26, 9);

            assert!(line[39].is_some(), "{nonsense}: fine up to the bad bar");
            assert!(line[40..].iter().all(Option::is_none), "{nonsense}: and stops there");
            for series in [&line, &signal, &histogram] {
                assert!(
                    series.iter().flatten().all(|v| v.is_finite()),
                    "{nonsense} reached a chart"
                );
            }
        }
    }

    /// A bar with its high under its low cannot change an indicator of closes,
    /// but a chart can hold one.
    #[test]
    fn macd_ignores_a_bar_that_is_upside_down() {
        let closes: Vec<f64> = (0..60).map(|i| 100.0 + (i as f64 / 6.0).sin() * 3.0).collect();
        let clean = macd(&bars(&closes), 12, 26, 9).0;
        let mut broken = bars(&closes);
        broken[30].high = 0.0;
        broken[30].low = 1_000.0;
        assert_eq!(macd(&broken, 12, 26, 9).0, clean);
    }

    /// An average of a series with a hole in it starts again on the far side
    /// rather than carrying a value across bars it was never told about.
    #[test]
    fn an_average_of_a_series_with_a_hole_seeds_again_past_it() {
        let values: Vec<Option<f64>> =
            vec![Some(1.0), Some(3.0), None, Some(5.0), Some(7.0), Some(9.0)];
        let out = ema_of(&values, 2);
        assert_eq!(out[0], None, "waiting for a second value");
        assert_eq!(out[1], Some(2.0), "seeded with the mean of the first two");
        assert_eq!(out[2], None, "the hole");
        assert_eq!(out[3], None, "and one value is not a seed");
        assert_eq!(out[4], Some(6.0), "seeded again with the mean of 5 and 7");
        assert_eq!(out[5], Some(8.0), "9 * 2/3 + 6 * 1/3");
    }
}
