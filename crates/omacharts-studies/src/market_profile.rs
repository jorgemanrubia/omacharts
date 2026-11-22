//! Market profile: time at price, counted in TPOs.
//!
//! A period — a session, a week — is cut into TPO periods of so many
//! minutes, each with a letter. Every price row a bar's range touches in a
//! TPO period gets one TPO for that period's letter, once however many of
//! its bars touch the row. The row with the most is the point of control;
//! the value area is grown outward from it, taking the side with more,
//! until it holds the value-area share of every TPO in the period.
//!
//! The arithmetic is here, and the chart's whole-period profile is made of
//! it, so a profile read a bar at a time ends its period on the chart's
//! numbers: the same rows, the same point of control, the same value area.
//!
//! Rows are whole ticks of a tick size, `row_ticks` to a row, and a price is
//! put in one by rounding it to its tick first, so a price that is a hair
//! off its tick in floating point lands where it should.

use crate::at_price::ProfileReading;
use crate::{Ohlcv, Study};

/// The minutes a TPO period lasts unless somebody says otherwise: half an
/// hour, as the profile was first drawn.
pub const DEFAULT_MINUTES: u32 = 30;

/// The letters TPO periods are named by, in order: A to Z, then a to z, and
/// round again for a period with more than fifty-two of them.
pub const LETTERS: &[u8; 52] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The row `price` falls in: its tick, `row_ticks` ticks to a row, counted
/// from nought. `None` for a price that is not a number, or rows that are
/// not a size.
#[inline]
pub fn row_of(price: f64, tick_size: f64, row_ticks: u32) -> Option<i64> {
    if !price.is_finite() || !(tick_size > 0.0 && tick_size.is_finite()) || row_ticks == 0 {
        return None;
    }
    let ticks = (price / tick_size).round();
    // Far enough out to be nonsense, and too far to count rows to.
    if ticks.abs() > 1e15 {
        return None;
    }
    Some((ticks as i64).div_euclid(i64::from(row_ticks)))
}

/// The rows a bar's range touches, lowest first: `None` for a bar with a
/// price that is not a number.
#[inline]
pub fn rows_of<B: Ohlcv>(bar: &B, tick_size: f64, row_ticks: u32) -> Option<(i64, i64)> {
    let (low, high) = (bar.low().min(bar.high()), bar.low().max(bar.high()));
    Some((row_of(low, tick_size, row_ticks)?, row_of(high, tick_size, row_ticks)?))
}

/// Which TPO period of a period that began at `start` a bar at `ts` is in,
/// from nought, `minutes` to each. A period's TPO periods are counted from
/// its first bar.
#[inline]
pub fn tpo_of(ts: i64, start: i64, minutes: u32) -> i64 {
    (ts - start).div_euclid(i64::from(minutes.max(1)) * 60)
}

/// Put TPO period `tpo`'s TPO in each of rows `rows` (indices into
/// `counts` and `marks`) that it has not put one in yet: a row gets one TPO
/// a TPO period however many of its bars touch it. `marks` holds the TPO
/// period that last marked each row. Answers how many TPOs went in.
#[inline]
pub fn mark(counts: &mut [u32], marks: &mut [i64], rows: std::ops::RangeInclusive<usize>, tpo: i64) -> u32 {
    let mut added = 0;
    for at in rows {
        if marks[at] != tpo {
            marks[at] = tpo;
            counts[at] += 1;
            added += 1;
        }
    }
    added
}

/// The letter of TPO period `tpo`, as an index into [`LETTERS`].
#[inline]
pub fn letter_of(tpo: i64) -> u8 {
    tpo.rem_euclid(LETTERS.len() as i64) as u8
}

/// The row of `len` holding the most TPOs, `count(row)` of them each: of
/// two that tie, the one nearer the middle of the profile's range, and of
/// two as near, the higher. `None` for a profile with no rows.
pub fn point_of_control(len: usize, count: impl Fn(usize) -> u32) -> Option<usize> {
    let mut best: Option<(usize, u32, u64)> = None;
    for row in 0..len {
        let tpos = count(row);
        // Twice the distance from the middle, so a middle between two rows
        // is still a whole number.
        let off = (2 * row as i64 - (len as i64 - 1)).unsigned_abs();
        let better = match best {
            None => true,
            Some((_, most, nearest)) => tpos > most || (tpos == most && off <= nearest),
        };
        if better {
            best = Some((row, tpos, off));
        }
    }
    best.map(|(row, _, _)| row)
}

/// The rows of the value area, inclusive: outward from `poc`, always taking
/// the side with more TPOs — the upper of two that tie — until they hold
/// `fraction` of `total`. The rule a volume profile grows its value area
/// by, on TPOs.
pub fn value_area(len: usize, count: impl Fn(usize) -> u32, poc: usize, total: u64, fraction: f64) -> (usize, usize) {
    crate::volume_profile::value_area(len, |row| f64::from(count(row)), poc, total as f64, fraction)
}

/// The prices a row holds: its lowest tick and its highest.
#[inline]
pub fn row_prices(row: i64, tick_size: f64, row_ticks: u32) -> (f64, f64) {
    let ticks = i64::from(row_ticks);
    ((row * ticks) as f64 * tick_size, (row * ticks + ticks - 1) as f64 * tick_size)
}

/// Where a profile of `len` rows from row `low`, `count(row)` TPOs each and
/// `total` in all, has its point of control and value area, as prices: the
/// middle of the point of control's row, the highest tick of the value
/// area's top row and the lowest of its bottom one. `None` with no TPOs.
#[allow(clippy::too_many_arguments)]
pub fn reading(
    low: i64,
    len: usize,
    count: impl Fn(usize) -> u32,
    total: u64,
    tick_size: f64,
    row_ticks: u32,
    fraction: f64,
) -> Option<ProfileReading> {
    if total == 0 {
        return None;
    }
    let poc = point_of_control(len, &count)?;
    let (lower, upper) = value_area(len, &count, poc, total, fraction);
    let ticks = i64::from(row_ticks);
    let row = low + poc as i64;
    Some(ProfileReading {
        poc: (2 * row * ticks + ticks - 1) as f64 * tick_size / 2.0,
        vah: row_prices(low + upper as i64, tick_size, row_ticks).1,
        val: row_prices(low + lower as i64, tick_size, row_ticks).0,
    })
}

/// Rows a profile starts with room for: a busy session of the busiest
/// contracts a tick a row fits, so a period rarely has to grow.
const ROOM: usize = 2048;

/// The market profile of the period so far: its point of control and its
/// value area as could be known at each bar, a bar at a time.
///
/// A whole period's profile is only whole once the period is over; this is
/// what it was at every bar along the way, and at the period's last bar it
/// is the whole period's. Its TPO periods are counted from the first bar of
/// each period, which `new_period` marks.
#[derive(Clone, Debug)]
pub struct DevelopingMarketProfile {
    tick_size: f64,
    row_ticks: u32,
    minutes: u32,
    value_area: f64,
    /// TPOs in each row from `low`, for the period so far.
    counts: Vec<u32>,
    /// The TPO period that last put a TPO in each row, so a row gets one
    /// a TPO period however many of its bars touch it.
    marks: Vec<i64>,
    low: i64,
    total: u64,
    /// Where the period began: its first bar's time.
    start: Option<i64>,
}

/// A row no TPO period has marked.
const UNMARKED: i64 = i64::MIN;

impl DevelopingMarketProfile {
    /// A profile in rows of `row_ticks` ticks of `tick_size`, cut into TPO
    /// periods of `minutes`, with a value area of 70%.
    pub fn new(tick_size: f64, row_ticks: u32, minutes: u32) -> DevelopingMarketProfile {
        DevelopingMarketProfile {
            tick_size,
            row_ticks,
            minutes,
            value_area: 0.7,
            counts: Vec::with_capacity(ROOM),
            marks: Vec::with_capacity(ROOM),
            low: 0,
            total: 0,
            start: None,
        }
    }

    /// The share of the TPOs the value area holds, `0.0..=1.0`.
    pub fn value_area(mut self, fraction: f64) -> DevelopingMarketProfile {
        self.value_area = fraction;
        self
    }

    /// The reading of what is held, with `bar`'s TPOs added if it is given,
    /// starting a period at it when `fresh`: worked out in place, so a bar
    /// still forming is read without copying the profile.
    fn read_with<B: Ohlcv>(&self, bar: Option<(&B, (i64, i64))>, fresh: bool) -> Option<ProfileReading> {
        let held = if fresh { 0 } else { self.counts.len() };
        let Some((bar, (from, to))) = bar else {
            let counts = &self.counts[..held];
            return reading(self.low, held, |row| counts[row], if fresh { 0 } else { self.total }, self.tick_size, self.row_ticks, self.value_area);
        };
        let start = match (fresh, self.start) {
            (false, Some(start)) => start,
            _ => bar.ts(),
        };
        let tpo = tpo_of(bar.ts(), start, self.minutes);
        let (low, high) = match held {
            0 => (from, to),
            _ => (self.low.min(from), (self.low + held as i64 - 1).max(to)),
        };
        let stored = |row: i64| -> Option<usize> {
            let at = row - self.low;
            (at >= 0 && (at as usize) < held).then_some(at as usize)
        };
        let new = |row: i64| (from..=to).contains(&row) && stored(row).is_none_or(|at| self.marks[at] != tpo);
        let mut added = 0u64;
        for row in from..=to {
            added += u64::from(new(row));
        }
        let total = if fresh { 0 } else { self.total } + added;
        let count = |at: usize| {
            let row = low + at as i64;
            stored(row).map_or(0, |at| self.counts[at]) + u32::from(new(row))
        };
        reading(low, (high - low + 1) as usize, count, total, self.tick_size, self.row_ticks, self.value_area)
    }
}

impl Study for DevelopingMarketProfile {
    type Value = ProfileReading;

    fn step<B: Ohlcv>(&mut self, bar: &B, new_period: bool) -> Option<ProfileReading> {
        if new_period || self.start.is_none() {
            self.counts.clear();
            self.marks.clear();
            self.total = 0;
            self.start = Some(bar.ts());
        }
        let Some((from, to)) = rows_of(bar, self.tick_size, self.row_ticks) else {
            return self.read_with::<B>(None, false);
        };
        let start = self.start.unwrap_or(bar.ts());
        let tpo = tpo_of(bar.ts(), start, self.minutes);
        if self.counts.is_empty() {
            self.low = from;
        }
        // Room below, then above, for the rows this bar reaches.
        if from < self.low {
            let more = (self.low - from) as usize;
            self.counts.splice(0..0, std::iter::repeat_n(0, more));
            self.marks.splice(0..0, std::iter::repeat_n(UNMARKED, more));
            self.low = from;
        }
        let len = (to - self.low + 1) as usize;
        if len > self.counts.len() {
            self.counts.resize(len, 0);
            self.marks.resize(len, UNMARKED);
        }
        let rows = (from - self.low) as usize..=(to - self.low) as usize;
        self.total += u64::from(mark(&mut self.counts, &mut self.marks, rows, tpo));
        let counts = &self.counts;
        reading(self.low, counts.len(), |row| counts[row], self.total, self.tick_size, self.row_ticks, self.value_area)
    }

    fn peek<B: Ohlcv>(&self, bar: &B, new_period: bool) -> Option<ProfileReading> {
        let fresh = new_period || self.start.is_none();
        let rows = rows_of(bar, self.tick_size, self.row_ticks);
        self.read_with(rows.map(|rows| (bar, rows)), fresh)
    }

    fn warmup(&self) -> usize {
        0
    }

    fn key(&self) -> &'static str {
        "developing_market_profile"
    }

    fn params(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("tick_size", self.tick_size),
            ("row_ticks", f64::from(self.row_ticks)),
            ("minutes", f64::from(self.minutes)),
            ("value_area", self.value_area),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{over, Bar};

    /// Minutes of a market a quarter a tick, wandering.
    fn bars(seed: u64, n: usize) -> Vec<Bar> {
        let mut state = seed;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut close = 5_000.0f64;
        (0..n)
            .map(|i| {
                let open = close;
                close = open + ((next() - 0.5) * 12.0).round() * 0.25;
                let high = open.max(close) + (next() * 4.0).round() * 0.25;
                let low = open.min(close) - (next() * 4.0).round() * 0.25;
                Bar { ts: 1_700_000_000 + i as i64 * 60, open, high, low, close, volume: 1.0 }
            })
            .collect()
    }

    fn starts(i: usize) -> bool {
        i.is_multiple_of(390)
    }

    /// The profile of `bars`, one period, worked out the long way.
    fn brute(bars: &[Bar], tick: f64, row_ticks: u32, minutes: u32, fraction: f64) -> Option<ProfileReading> {
        let mut marked = std::collections::BTreeSet::<(i64, i64)>::new();
        let start = bars.first()?.ts;
        for bar in bars {
            let (from, to) = rows_of(bar, tick, row_ticks)?;
            for row in from..=to {
                marked.insert((row, tpo_of(bar.ts, start, minutes)));
            }
        }
        let low = marked.iter().map(|m| m.0).min()?;
        let high = marked.iter().map(|m| m.0).max()?;
        let counts: Vec<u32> = (low..=high).map(|row| marked.iter().filter(|m| m.0 == row).count() as u32).collect();
        reading(low, counts.len(), |row| counts[row], marked.len() as u64, tick, row_ticks, fraction)
    }

    #[test]
    fn a_developing_market_profile_is_the_profile_of_its_period_so_far() {
        let bars = bars(5, 1_000);
        for (row_ticks, minutes) in [(1, 30), (4, 30), (2, 15), (1, 60)] {
            let mut study = DevelopingMarketProfile::new(0.25, row_ticks, minutes);
            let mut start = 0;
            for i in 0..bars.len() {
                if starts(i) {
                    start = i;
                }
                let peeked = study.peek(&bars[i], starts(i));
                let stepped = study.step(&bars[i], starts(i));
                assert_eq!(peeked, stepped, "peek and step agree at bar {i}");
                assert_eq!(stepped, brute(&bars[start..=i], 0.25, row_ticks, minutes, 0.7), "bar {i}, {row_ticks} ticks, {minutes}m");
            }
        }
    }

    #[test]
    fn a_prefix_of_the_bars_reads_the_same_as_the_prefix_of_the_readings() {
        let bars = bars(9, 900);
        let make = || DevelopingMarketProfile::new(0.25, 2, 30);
        let whole = over(make(), &bars, starts);
        for cut in [0, 1, 2, 389, 390, 391, 777, 899] {
            assert_eq!(over(make(), &bars[..cut], starts), whole[..cut], "the first {cut} bars");
        }
    }

    #[test]
    fn a_row_gets_one_tpo_a_period_however_many_bars_touch_it() {
        let bar = |minute: i64, low: f64, high: f64| Bar { ts: minute * 60, open: low, high, low, close: high, volume: 1.0 };
        let mut study = DevelopingMarketProfile::new(1.0, 1, 30);
        // A: rows 10 to 12, three times over. B: rows 12 and 13.
        for minute in 0..3 {
            study.step(&bar(minute, 10.0, 12.0), minute == 0);
        }
        let reading = study.step(&bar(30, 12.0, 13.0), false).unwrap();
        assert_eq!(study.counts, vec![1, 1, 2, 1]);
        assert_eq!(study.total, 5);
        assert_eq!(reading.poc, 12.0);
    }

    #[test]
    fn a_tie_for_most_goes_to_the_row_nearest_the_middle() {
        // Rows 0..=6, the most at 1 and at 4: the middle is 3, so 4.
        assert_eq!(point_of_control(7, |row| [1, 3, 1, 1, 3, 1, 1][row]), Some(4));
        // As near either way: the higher.
        assert_eq!(point_of_control(4, |row| [1, 2, 2, 1][row]), Some(2));
        assert_eq!(point_of_control(0, |_| 0), None);
    }

    #[test]
    fn the_value_area_takes_the_side_with_more() {
        let counts = [1, 2, 5, 9, 4, 4, 1];
        let total: u32 = counts.iter().sum();
        // 70% of 26 is 18.2: 9, then 5 below, then 4 above, then 4 above.
        assert_eq!(value_area(counts.len(), |row| counts[row], 3, u64::from(total), 0.7), (2, 5));
    }

    #[test]
    fn rows_are_whole_ticks_from_nought() {
        assert_eq!(row_of(5000.25, 0.25, 4), Some(5000));
        assert_eq!(row_of(5000.75, 0.25, 4), Some(5000));
        assert_eq!(row_of(5001.0, 0.25, 4), Some(5001));
        assert_eq!(row_of(-0.25, 0.25, 4), Some(-1));
        assert_eq!(row_of(1.1, 0.1, 1), Some(11));
        assert_eq!(row_of(f64::NAN, 0.25, 1), None);
        assert_eq!(row_of(1.0, 0.0, 1), None);
        assert_eq!(row_prices(5000, 0.25, 4), (5000.0, 5000.75));
        assert_eq!(letter_of(0), 0);
        assert_eq!(LETTERS[letter_of(26) as usize], b'a');
        assert_eq!(LETTERS[letter_of(52) as usize], b'A');
    }

    #[test]
    fn a_bar_that_is_not_a_number_adds_nothing() {
        let mut study = DevelopingMarketProfile::new(0.25, 1, 30);
        let nan = Bar { ts: 0, open: f64::NAN, high: f64::NAN, low: f64::NAN, close: f64::NAN, volume: 0.0 };
        assert_eq!(study.peek(&nan, true), None);
        assert_eq!(study.step(&nan, true), None);
        let good = Bar { ts: 60, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 0.0 };
        assert!(study.step(&good, false).is_some());
        assert_eq!(study.peek(&nan, false), study.step(&nan, false));
    }
}
