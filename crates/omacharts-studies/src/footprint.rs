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
    if tick_px.is_nan() || tick_px <= 0.0 {
        return *STEPS.last().unwrap_or(&1);
    }
    STEPS.into_iter().find(|step| f64::from(*step) * tick_px >= least).unwrap_or(*STEPS.last().unwrap_or(&1))
}

/// The compact pitch an automatic row is aimed at, in logical pixels: a
/// line of small type and a gap of a pixel or two.
pub const ROW_PITCH: f64 = 16.0;
/// The least pitch an automatic row may keep before a coarser step is
/// taken, and the most before a finer one is: inside the two, the step held
/// is kept, so a price scale that breathes a little leaves the rows alone.
pub const PITCH_KEPT: (f64, f64) = (13.0, 40.0);
/// The most ticks a row may be set to hold.
pub const MAX_ROW_TICKS: u32 = 10_000;

/// The ticks an automatic row holds when a tick is `tick_px` pixels tall:
/// `held`, the step drawn so far, while its rows stay between
/// [`PITCH_KEPT`]'s bounds; otherwise the fewest of 1, 2, 4, 5, 10 and on
/// that make a row [`ROW_PITCH`] tall or more.
pub fn auto_ticks(tick_px: f64, held: Option<u32>) -> u32 {
    if let Some(held) = held.filter(|held| STEPS.contains(held)) {
        let pitch = f64::from(held) * tick_px;
        // Finer is only taken when a finer step would still be tall enough.
        let finer = STEPS.iter().rev().find(|step| **step < held).copied();
        let finer_fits = finer.is_some_and(|finer| f64::from(finer) * tick_px >= ROW_PITCH);
        if pitch >= PITCH_KEPT.0 && (pitch <= PITCH_KEPT.1 || !finer_fits) {
            return held;
        }
    }
    ticks_per_row(tick_px, ROW_PITCH)
}

/// The ticks a row of a footprint set to `chosen` ticks holds: `chosen`
/// itself, or the fewest of its multiples that leave a row `least` pixels
/// tall when a tick is `tick_px`, so a fine setting zoomed far out still
/// draws rows that can be told apart. A multiple of `chosen` keeps the rows
/// on its grid.
pub fn fixed_ticks(chosen: u32, tick_px: f64, least: f64) -> u32 {
    let chosen = chosen.clamp(1, MAX_ROW_TICKS);
    if !(tick_px.is_finite() && tick_px > 0.0) {
        return chosen;
    }
    let needed = (least / (f64::from(chosen) * tick_px)).ceil().max(1.0);
    (f64::from(chosen) * needed).min(f64::from(MAX_ROW_TICKS * 10)) as u32
}

/// What a footprint keeps of one tick and sums into a row: the renderer's
/// [`Tick`] and the export's [`Sides`] are folded by the one [`fold`], so
/// what is drawn and what is read out are the same rows.
pub trait Cell: Copy {
    /// Whether anything traded at it: a row of nothing is no row.
    fn traded(&self) -> bool;
    fn add(&mut self, other: &Self);
}

/// One tick as a footprint draws it: what was bought less what was sold,
/// and everything that traded, sides or not. A total of nought is a tick
/// nothing traded at.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Tick {
    pub delta: i64,
    pub total: u64,
}

impl Tick {
    /// The tick of `at_bid` sold, `at_ask` bought and `unknown` traded on
    /// neither side that anyone said.
    #[inline]
    pub fn of(at_bid: u64, at_ask: u64, unknown: u64) -> Tick {
        Tick { delta: at_ask as i64 - at_bid as i64, total: at_bid + at_ask + unknown }
    }
}

impl Cell for Tick {
    #[inline]
    fn traded(&self) -> bool {
        self.total != 0
    }
    #[inline]
    fn add(&mut self, other: &Tick) {
        self.delta += other.delta;
        self.total += other.total;
    }
}

/// One tick, or a row, with each side and its trades: what a footprint is
/// read out as.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sides {
    /// Sold into the bid.
    pub at_bid: u64,
    /// Bought from the ask.
    pub at_ask: u64,
    /// Traded on neither side that anyone said.
    pub unknown: u64,
    /// Trades, where the feed counts them.
    pub trades: u64,
}

impl Sides {
    /// Bought less sold: the delta a footprint draws.
    #[inline]
    pub fn delta(&self) -> i64 {
        self.at_ask as i64 - self.at_bid as i64
    }

    /// Everything that traded: the total a footprint's profile draws.
    #[inline]
    pub fn total(&self) -> u64 {
        self.at_bid + self.at_ask + self.unknown
    }

    /// As the renderer keeps it.
    #[inline]
    pub fn tick(&self) -> Tick {
        Tick::of(self.at_bid, self.at_ask, self.unknown)
    }
}

impl Cell for Sides {
    #[inline]
    fn traded(&self) -> bool {
        self.total() != 0
    }
    #[inline]
    fn add(&mut self, other: &Sides) {
        self.at_bid += other.at_bid;
        self.at_ask += other.at_ask;
        self.unknown += other.unknown;
        self.trades += other.trades;
    }
}

/// Fold one bar's ticks, `cells[i]` being the tick `low_tick + i`, into rows
/// of `step` ticks aligned to multiples of `step`, so a row covers the same
/// prices on every bar. Each row that traded is handed to `each`, lowest
/// first, as its low tick and its sum; ticks nothing traded at add nothing.
/// Nothing is allocated.
#[inline]
pub fn fold<C: Cell>(low_tick: i32, cells: &[C], step: u32, mut each: impl FnMut(i32, C)) {
    let step = step.max(1) as i32;
    let mut open: Option<(i32, C)> = None;
    for (i, cell) in cells.iter().enumerate() {
        if !cell.traded() {
            continue;
        }
        let low = (low_tick + i as i32).div_euclid(step) * step;
        match &mut open {
            Some((at, sum)) if *at == low => sum.add(cell),
            _ => {
                if let Some((at, sum)) = open.take() {
                    each(at, sum);
                }
                open = Some((low, *cell));
            }
        }
    }
    if let Some((at, sum)) = open {
        each(at, sum);
    }
}

/// The low tick of the row that traded most: a bar's point of control,
/// the lower of two that traded alike.
pub fn point_of_control<C: Cell>(low_tick: i32, cells: &[C], step: u32, total: impl Fn(&C) -> u64) -> Option<i32> {
    let mut best: Option<(i32, u64)> = None;
    fold(low_tick, cells, step, |low, sum| {
        let traded = total(&sum);
        if best.is_none_or(|(_, most)| traded > most) {
            best = Some((low, traded));
        }
    });
    best.map(|(low, _)| low)
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
    fn an_automatic_row_is_a_compact_line_and_holds_its_step() {
        assert_eq!(auto_ticks(20.0, None), 1, "a tick taller than a line is a row of its own");
        assert_eq!(auto_ticks(5.0, None), 4);
        assert_eq!(auto_ticks(1.0, None), 20);
        // The scale breathes: the step held is kept while its rows stay
        // comfortable, either way.
        assert_eq!(auto_ticks(3.5, Some(4)), 4, "14 px rows are kept");
        assert_eq!(auto_ticks(9.0, Some(4)), 4, "36 px rows are kept: 2 ticks would be 18, but 4 is held");
        assert_eq!(auto_ticks(3.0, Some(4)), 10, "12 px rows are too thin: the fewest ticks that make 16 px");
        assert_eq!(auto_ticks(11.0, Some(4)), 2, "44 px rows are too fat once 2 ticks make 22");
        assert_eq!(auto_ticks(30.0, Some(1)), 1, "nothing finer than a tick");
        assert_eq!(auto_ticks(5.0, Some(3)), 4, "a step that is not a round one is not held");
    }

    #[test]
    fn a_fixed_row_keeps_its_ticks_until_they_are_too_thin() {
        assert_eq!(fixed_ticks(4, 2.0, 3.0), 4);
        assert_eq!(fixed_ticks(1, 0.5, 3.0), 6);
        assert_eq!(fixed_ticks(4, 0.25, 3.0), 12, "a multiple of 4, on its grid");
        assert_eq!(fixed_ticks(0, 4.0, 3.0), 1, "never under a tick");
        assert_eq!(fixed_ticks(4, 0.0, 3.0), 4);
    }

    /// A bar's ticks as (at_bid, at_ask, unknown, trades), from tick 99: a
    /// tick nothing traded at in the middle, and one traded on no side.
    const BAR: [(u64, u64, u64, u64); 5] = [(3, 2, 0, 4), (1, 3, 0, 3), (2, 2, 1, 5), (0, 0, 0, 0), (0, 4, 0, 2)];

    fn ticks() -> Vec<Tick> {
        BAR.iter().map(|(bid, ask, unknown, _)| Tick::of(*bid, *ask, *unknown)).collect()
    }

    fn sides() -> Vec<Sides> {
        BAR.iter().map(|(at_bid, at_ask, unknown, trades)| Sides { at_bid: *at_bid, at_ask: *at_ask, unknown: *unknown, trades: *trades }).collect()
    }

    #[test]
    fn folding_lines_rows_up_across_bars() {
        let mut seen = Vec::new();
        fold(99, &ticks(), 1, |low, tick| seen.push((low, tick.delta)));
        assert_eq!(seen, vec![(99, -1), (100, 2), (101, 0), (103, 4)], "nothing traded, no row");
        seen.clear();
        fold(99, &ticks(), 2, |low, tick| seen.push((low, tick.delta)));
        assert_eq!(seen, vec![(98, -1), (100, 2), (102, 4)]);
        seen.clear();
        fold(-3, &[Tick::of(0, 1, 0); 3], 2, |low, tick| seen.push((low, tick.delta)));
        assert_eq!(seen, vec![(-4, 1), (-2, 2)], "below nought on the same grid");
    }

    /// Golden: each row's total is everything that traded at its prices,
    /// both sides and any traded on neither, at one tick a row and folded
    /// two, three and five together.
    #[test]
    fn each_row_totals_everything_traded_at_its_prices() {
        type Rows = &'static [(i32, i64, u64)];
        let golden: [(u32, Rows); 4] = [
            (1, &[(99, -1, 5), (100, 2, 4), (101, 0, 5), (103, 4, 4)]),
            (2, &[(98, -1, 5), (100, 2, 9), (102, 4, 4)]),
            (3, &[(99, 1, 14), (102, 4, 4)]),
            (5, &[(95, -1, 5), (100, 6, 13)]),
        ];
        for (step, rows) in golden {
            let mut seen = Vec::new();
            fold(99, &ticks(), step, |low, tick| seen.push((low, tick.delta, tick.total)));
            assert_eq!(seen, rows, "{step} ticks a row");
        }
        assert_eq!(point_of_control(99, &ticks(), 1, |tick| tick.total), Some(99), "the lower of two alike");
        assert_eq!(point_of_control(99, &ticks(), 2, |tick| tick.total), Some(100));
        assert_eq!(point_of_control::<Tick>(0, &[], 1, |tick| tick.total), None);
    }

    /// What is read out is what is drawn: the sides folded into rows give
    /// each row the delta and total the renderer's ticks fold to, and add
    /// the trades.
    #[test]
    fn the_sides_fold_to_the_rows_drawn() {
        for step in [1, 2, 3, 4, 5, 10] {
            let (mut drawn, mut read) = (Vec::new(), Vec::new());
            fold(99, &ticks(), step, |low, tick| drawn.push((low, tick)));
            fold(99, &sides(), step, |low, sides: Sides| read.push((low, sides.tick())));
            assert_eq!(drawn, read, "{step}");
        }
        let mut trades = Vec::new();
        fold(99, &sides(), 2, |low, sides: Sides| trades.push((low, sides.at_bid, sides.at_ask, sides.trades)));
        assert_eq!(trades, vec![(98, 3, 2, 4), (100, 3, 5, 8), (102, 0, 4, 2)]);
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
