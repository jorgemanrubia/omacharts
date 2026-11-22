//! A footprint's rows: what was bought less what was sold at each price of
//! a bar, a row being one tick or several together when the rows would be
//! too thin to tell apart.
//!
//! Also the two things a footprint is drawn by that are arithmetic rather
//! than drawing: how strongly a row is filled against the strongest row it
//! is compared with, and the short way its number is written. Nothing here
//! allocates; rows go into a buffer the caller keeps.

/// One row of a footprint.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Row {
    /// Its lowest tick. The row covers this and the `ticks - 1` above it.
    pub low_tick: i32,
    /// Volume traded at the ask less volume traded at the bid.
    pub delta: i64,
    /// Everything that traded in it, sides or not.
    pub volume: u64,
}

/// Fold a bar's prices, lowest first as `(tick, at_bid, at_ask, unknown)`,
/// into rows of `ticks` ticks each, aligned to multiples of `ticks` so a row
/// covers the same prices on every bar. Rows with nothing traded are left
/// out. `out` is cleared first and keeps its allocation.
pub fn rows(levels: impl Iterator<Item = (i32, u64, u64, u64)>, ticks: u32, out: &mut Vec<Row>) {
    out.clear();
    let ticks = ticks.max(1) as i32;
    for (tick, at_bid, at_ask, unknown) in levels {
        let volume = at_bid + at_ask + unknown;
        if volume == 0 {
            continue;
        }
        let low_tick = tick.div_euclid(ticks) * ticks;
        let delta = at_ask as i64 - at_bid as i64;
        match out.last_mut() {
            Some(row) if row.low_tick == low_tick => {
                row.delta += delta;
                row.volume += volume;
            }
            _ => out.push(Row { low_tick, delta, volume }),
        }
    }
}

/// The steps a row may be, in ticks.
const STEPS: [u32; 15] = [1, 2, 4, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 5000];

/// The fewest ticks a row may hold and still be `least` pixels tall, when a
/// tick is `tick_px` pixels: one of 1, 2, 4, 5, 10, 20, 25, 50 and on.
pub fn ticks_per_row(tick_px: f64, least: f64) -> u32 {
    if !(tick_px > 0.0) {
        return *STEPS.last().unwrap_or(&1);
    }
    STEPS.into_iter().find(|step| f64::from(*step) * tick_px >= least).unwrap_or(*STEPS.last().unwrap_or(&1))
}

/// How strongly a row of `delta` is filled, as one of `levels` steps from 1
/// to `levels`, against the strongest row it is compared with, `largest`
/// (by size, either sign). The strongest is the last step; a row of nought
/// is the first.
#[inline]
pub fn strength(delta: i64, largest: u64, levels: usize) -> usize {
    let levels = levels.max(1);
    if largest == 0 {
        return levels;
    }
    let share = (delta.unsigned_abs() as f64 / largest as f64).clamp(0.0, 1.0);
    ((share * levels as f64).ceil() as usize).clamp(1, levels)
}

/// The opacity of step `level` of `levels`, from `faint` at the first to
/// `full` at the last, in even steps.
#[inline]
pub fn step_alpha(level: usize, levels: usize, faint: f64, full: f64) -> f64 {
    let levels = levels.max(1);
    if levels == 1 {
        return full;
    }
    let at = (level.clamp(1, levels) - 1) as f64 / (levels - 1) as f64;
    faint + (full - faint) * at
}

/// `value` written short into `buf`, as a footprint writes a row: whole
/// below a thousand, then one decimal and K or M, the decimal dropped where
/// it is nought or the figure is past a hundred: `-870`, `1.1K`, `-12K`,
/// `340K`, `2.5M`. Answers the text written, from `buf`.
pub fn short(value: i64, buf: &mut [u8; 12]) -> &str {
    let mut at = 0;
    let mut put = |byte: u8, at: &mut usize| {
        if *at < buf.len() {
            buf[*at] = byte;
            *at += 1;
        }
    };
    if value < 0 {
        put(b'-', &mut at);
    }
    let magnitude = value.unsigned_abs();
    // In tenths of the unit, rounded half up.
    let (tenths, unit) = match magnitude {
        0..=999 => (magnitude * 10, None),
        1_000..=999_949 => ((magnitude + 50) / 100, Some(b'K')),
        _ => ((magnitude + 50_000) / 100_000, Some(b'M')),
    };
    // Rounding 999.95K up is a thousand K: a million.
    let (tenths, unit) = match unit {
        Some(b'K') if tenths >= 10_000 => ((magnitude + 50_000) / 100_000, Some(b'M')),
        other => (tenths, other),
    };
    let whole = tenths / 10;
    let tenth = tenths % 10;
    let mut digits = [0u8; 20];
    let mut n = 0;
    let mut rest = whole;
    loop {
        digits[n] = b'0' + (rest % 10) as u8;
        n += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        put(digits[i], &mut at);
    }
    if unit.is_some() && tenth != 0 && whole < 100 {
        put(b'.', &mut at);
        put(b'0' + tenth as u8, &mut at);
    }
    if let Some(unit) = unit {
        put(unit, &mut at);
    }
    std::str::from_utf8(&buf[..at]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_fold_prices_into_aligned_steps_and_skip_empty_ones() {
        let levels = [(99, 1, 0, 0), (100, 3, 5, 1), (101, 0, 0, 0), (102, 2, 2, 0), (103, 0, 4, 0)];
        let mut out = Vec::new();
        rows(levels.into_iter(), 1, &mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(out[1], Row { low_tick: 100, delta: 2, volume: 9 });
        rows(levels.into_iter(), 2, &mut out);
        assert_eq!(
            out,
            vec![Row { low_tick: 98, delta: -1, volume: 1 }, Row { low_tick: 100, delta: 2, volume: 9 }, Row {
                low_tick: 102,
                delta: 4,
                volume: 8
            }]
        );
        let negative = [(-3, 1, 0, 0), (-2, 0, 1, 0)];
        rows(negative.into_iter(), 2, &mut out);
        assert_eq!(out.iter().map(|r| r.low_tick).collect::<Vec<_>>(), vec![-4, -2]);
    }

    #[test]
    fn a_row_holds_the_fewest_round_ticks_that_leave_it_tall_enough() {
        assert_eq!(ticks_per_row(12.0, 3.0), 1);
        assert_eq!(ticks_per_row(1.0, 3.0), 4);
        assert_eq!(ticks_per_row(0.7, 3.0), 5);
        assert_eq!(ticks_per_row(0.0, 3.0), 5000);
    }

    #[test]
    fn strength_steps_up_to_the_strongest_row() {
        assert_eq!(strength(0, 100, 5), 1);
        assert_eq!(strength(-1, 100, 5), 1);
        assert_eq!(strength(-21, 100, 5), 2);
        assert_eq!(strength(100, 100, 5), 5);
        assert_eq!(strength(-500, 100, 5), 5);
        assert_eq!(strength(3, 0, 5), 5);
        assert_eq!(step_alpha(5, 5, 0.1, 0.8), 0.8, "the strongest row is the chosen opacity");
        assert_eq!(step_alpha(1, 5, 0.1, 0.8), 0.1);
    }

    #[test]
    fn numbers_are_written_short() {
        let mut buf = [0u8; 12];
        let cases = [
            (0, "0"),
            (-12, "-12"),
            (870, "870"),
            (-999, "-999"),
            (1_000, "1K"),
            (-1_120, "-1.1K"),
            (1_150, "1.2K"),
            (12_345, "12.3K"),
            (123_456, "123K"),
            (999_949, "999K"),
            (999_950, "1M"),
            (2_500_000, "2.5M"),
        ];
        for (value, written) in cases {
            assert_eq!(short(value, &mut buf), written, "{value}");
        }
        assert!(short(i64::MIN, &mut buf).starts_with('-'), "held to the buffer");
    }
}
