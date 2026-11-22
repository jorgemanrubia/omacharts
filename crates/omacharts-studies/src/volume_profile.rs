//! A volume profile: how much traded at each price over a period, the
//! price that saw the most (the point of control), the band around it that
//! holds most of the volume (the value area), and the shelves and thin
//! stretches in it (the high- and low-volume nodes).
//!
//! The rows come from one of two places. From bars alone, [`spread`] shares
//! each bar's volume across the rows its range touches, by how much of the
//! bar is in each; from volume at price, [`rows_at_prices`] puts each
//! price's volume in the row that holds it. Either way [`Profile::of_rows`]
//! reads the point of control and the value area off them, by
//! [`point_of_control`] and [`value_area`], the same rule a market profile
//! grows its value area by on TPOs and [`crate::DevelopingProfile`] by on
//! each tick.
//!
//! ```
//! use omacharts_studies::volume_profile::{spread, Profile};
//! use omacharts_studies::Bar;
//!
//! let bars = [
//!     Bar { ts: 0, open: 10.0, high: 12.0, low: 10.0, close: 11.0, volume: 200.0 },
//!     Bar { ts: 60, open: 11.0, high: 11.5, low: 10.5, close: 11.0, volume: 400.0 },
//! ];
//! // Rows half a point tall from 10.
//! let (low, step, rows) = (10.0, 0.5, 4);
//! let volumes = spread(&bars, low, step, rows);
//! let profile = Profile::of_rows((0, 60), (0, 1), low, step, volumes, 0.7, false).unwrap();
//! assert_eq!(profile.poc_price(), 11.25);
//! let (val, vah) = profile.value_area_bounds();
//! assert!(val <= 10.5 && vah >= 11.5);
//! ```

use crate::Ohlcv;

/// One row of a profile: the prices it covers and what traded there.
#[derive(Clone, PartialEq, Debug)]
pub struct ProfileRow {
    pub low: f64,
    pub high: f64,
    /// What traded in the row; for a delta profile, the ask volume less the
    /// bid volume, so below zero where more was sold.
    pub volume: f64,
}

/// One period's profile.
#[derive(Clone, PartialEq, Debug)]
pub struct Profile {
    /// The period this profile covers.
    pub start_ts: i64,
    /// Where each period it was made of starts: one for a period on its
    /// own, more for a composite of several merged into one.
    pub parts: Vec<i64>,
    pub end_ts: i64,
    /// Index of the first and last bar of the period, for placing it on the
    /// chart without searching for them again.
    pub first_bar: usize,
    pub last_bar: usize,
    pub rows: Vec<ProfileRow>,
    /// Row holding the most volume; for a delta profile, the greatest delta
    /// either way.
    pub poc: usize,
    /// Inclusive row range covering `value_area` of the volume. A delta has
    /// none, a share of a sum that cancels itself out being no share of
    /// anything, so it is the point of control alone.
    pub value_area: (usize, usize),
    /// The longest row: the most volume, or the greatest delta either way.
    pub max_volume: f64,
    /// Everything traded, or for a delta profile the period's net delta.
    pub total_volume: f64,
}

impl Profile {
    /// Whether this is several periods merged into one.
    pub fn is_composite(&self) -> bool {
        self.parts.len() > 1
    }

    pub fn poc_price(&self) -> f64 {
        let row = &self.rows[self.poc];
        (row.low + row.high) / 2.0
    }

    /// Price bounds of the value area.
    pub fn value_area_bounds(&self) -> (f64, f64) {
        (self.rows[self.value_area.0].low, self.rows[self.value_area.1].high)
    }
}

/// A high- or low-volume node of a profile: a shelf the volume piles up on,
/// or the thin stretch between two of them.
#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    /// The row at the node's heart: the highest of a high-volume node, the
    /// lowest of a low-volume one.
    pub row: usize,
    /// The rows it spans, inclusive: as far either way as the volume stays
    /// within half its strength of the heart's.
    pub rows: (usize, usize),
    /// The volume a row there holds, smoothed as the nodes were found.
    pub volume: f64,
    /// How far it stands out, as a share of the point of control's smoothed
    /// volume: a high-volume node's height over the ground around it, a
    /// low-volume node's depth under the lower of its two neighbours.
    pub strength: f64,
}

/// A profile's high-volume nodes and the low-volume ones between them, each
/// lowest price first. See [`Profile::nodes`].
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Nodes {
    pub high: Vec<Node>,
    pub low: Vec<Node>,
}

/// How many rows either side a row is averaged with before nodes are looked
/// for, as a share of the profile's rows: a fortieth, so a profile of a tick
/// a row and one of thirty rows are smoothed over the same share of their
/// height, about five per cent, and a lone busy tick is not a node. Never
/// less than one row.
pub const NODE_SMOOTHING: f64 = 1.0 / 40.0;

/// How far a peak must stand above the ground around it to be a high-volume
/// node, as a share of the point of control's smoothed volume. A tenth keeps
/// the shelves a trader reads off a session and drops the ripples left on a
/// profile drawn a tick a row once it is smoothed.
pub const NODE_PROMINENCE: f64 = 0.10;

impl Profile {
    /// The high-volume nodes and the low-volume nodes between them, found
    /// the same way every time:
    ///
    /// 1. Each row's volume is averaged with the rows within
    ///    [`NODE_SMOOTHING`] of the profile's height either way.
    /// 2. A high-volume node is a peak of those, a row or a run of equal
    ///    rows with less on both sides, or on its one side at the profile's
    ///    edge, whose prominence — its height over the higher of the lowest
    ///    points between it and anything higher on each side — is at least
    ///    [`NODE_PROMINENCE`] of the highest. A profile flat throughout has
    ///    none.
    /// 3. A low-volume node is the lowest point between two neighbouring
    ///    high-volume nodes, so there is always one fewer of them.
    ///
    /// A delta profile is read by the size of its rows, either way.
    pub fn nodes(&self) -> Nodes {
        let volume: Vec<f64> = self.rows.iter().map(|row| row.volume.abs()).collect();
        find_nodes(&smooth(&volume))
    }
}

/// `volume` averaged over a window [`NODE_SMOOTHING`] of its length either
/// way, cut short at the ends.
fn smooth(volume: &[f64]) -> Vec<f64> {
    let reach = ((volume.len() as f64 * NODE_SMOOTHING).round() as usize).max(1);
    (0..volume.len())
        .map(|at| {
            let window = &volume[at.saturating_sub(reach)..(at + reach + 1).min(volume.len())];
            window.iter().sum::<f64>() / window.len() as f64
        })
        .collect()
}

/// Rows told apart by less than this share of the point of control's
/// volume are level. Spreading a bar over its rows leaves noise in the last
/// places of a float, and without this a shelf of equal rows reads as a
/// peak with no prominence beside a row a hair lower.
const LEVEL: f64 = 1e-6;

fn find_nodes(smoothed: &[f64]) -> Nodes {
    let top = smoothed.iter().copied().fold(0.0, f64::max);
    let count = smoothed.len();
    if count < 3 || top <= 0.0 {
        return Nodes::default();
    }
    // In steps of LEVEL, the point of control's being one.
    let level: Vec<f64> = smoothed.iter().map(|volume| (volume / top / LEVEL).round() * LEVEL).collect();
    let level = level.as_slice();
    let top = 1.0;
    // Peaks, a run of equal rows counting once at its middle.
    let mut peaks = Vec::new();
    let mut start = 0;
    while start < count {
        let mut end = start;
        while end + 1 < count && level[end + 1] == level[start] {
            end += 1;
        }
        let lower_before = start == 0 || level[start - 1] < level[start];
        let lower_after = end + 1 == count || level[end + 1] < level[start];
        if lower_before && lower_after && !(start == 0 && end + 1 == count) {
            let at = (start + end) / 2;
            let prominence = prominence(level, start, end);
            if prominence >= NODE_PROMINENCE * top {
                peaks.push((at, prominence));
            }
        }
        start = end + 1;
    }
    let mut valleys = Vec::new();
    for pair in peaks.windows(2) {
        let (left, right) = (pair[0].0, pair[1].0);
        let floor = level[left..=right].iter().copied().fold(f64::INFINITY, f64::min);
        let first = (left..=right).find(|&at| level[at] == floor).unwrap_or(left);
        let last = (left..=right).rev().find(|&at| level[at] == floor).unwrap_or(right);
        let depth = level[left].min(level[right]) - floor;
        valleys.push(((first + last) / 2, depth, left, right));
    }
    let high = peaks
        .iter()
        .enumerate()
        .map(|(n, &(at, prominence))| {
            // Bounded by the valleys either side, so two nodes never share a row.
            let from = n.checked_sub(1).map_or(0, |v| valleys[v].0 + 1);
            let to = valleys.get(n).map_or(count - 1, |v| v.0.saturating_sub(1).max(at));
            let floor = level[at] - prominence / 2.0;
            let mut low = at;
            while low > from && level[low - 1] >= floor {
                low -= 1;
            }
            let mut high = at;
            while high < to && level[high + 1] >= floor {
                high += 1;
            }
            Node { row: at, rows: (low, high), volume: smoothed[at], strength: prominence / top }
        })
        .collect();
    let low = valleys
        .iter()
        .map(|&(at, depth, left, right)| {
            let ceiling = level[at] + depth / 2.0;
            let mut low = at;
            while low > left + 1 && level[low - 1] <= ceiling {
                low -= 1;
            }
            let mut high = at;
            while high + 1 < right && level[high + 1] <= ceiling {
                high += 1;
            }
            Node { row: at, rows: (low, high), volume: smoothed[at], strength: depth / top }
        })
        .collect();
    Nodes { high, low }
}

/// How far the run of equal rows `start..=end` stands over its ground: its
/// height less the higher of the lowest points on each side before a higher
/// row or the profile's edge. A side with no rows at all is no ground.
fn prominence(level: &[f64], start: usize, end: usize) -> f64 {
    let height = level[start];
    let ground = |rows: &mut dyn Iterator<Item = usize>| {
        let mut floor: Option<f64> = None;
        for at in rows {
            if level[at] > height {
                break;
            }
            floor = Some(floor.map_or(level[at], |floor: f64| floor.min(level[at])));
        }
        floor
    };
    let before = ground(&mut (0..start).rev());
    let after = ground(&mut (end + 1..level.len()));
    match (before, after) {
        (Some(a), Some(b)) => height - a.max(b),
        (Some(a), None) | (None, Some(a)) => height - a,
        (None, None) => 0.0,
    }
}


/// Each row's volume, `rows` rows of `step` from `low`, from bars alone:
/// each bar's volume, or one where it has none, shared across the rows its
/// range touches by how much of the bar is in each — the two it ends in by
/// their overlap, the ones between in full. A bar outside the rows is
/// counted in the nearest.
pub fn spread<B: Ohlcv>(bars: &[B], low: f64, step: f64, rows: usize) -> Vec<f64> {
    let mut volumes = vec![0.0f64; rows];
    // The rows a bar covers wholly all get the same share of it, so they are
    // written as one step up where they begin and one down after they end,
    // summed into place at the end: a bar costs the same however many rows
    // it spans, which at a few hundred rows a period is most of the work.
    let mut whole = vec![0.0f64; rows + 1];

    for bar in bars {
        let (bar_low, bar_high, volume) = (bar.low(), bar.high(), bar.volume());
        let weight = if volume > 0.0 { volume } else { 1.0 };
        let span = (bar_high - bar_low).max(f64::EPSILON);
        let first = (((bar_low - low) / step).floor() as isize).clamp(0, rows as isize - 1) as usize;
        let last = (((bar_high - low) / step).floor() as isize).clamp(0, rows as isize - 1) as usize;

        if first == last {
            volumes[first] += weight;
            continue;
        }
        let share = |row: usize| {
            let row_low = low + row as f64 * step;
            let overlap = (bar_high.min(row_low + step) - bar_low.max(row_low)).max(0.0);
            weight * overlap / span
        };
        volumes[first] += share(first);
        volumes[last] += share(last);
        if last > first + 1 {
            let full = weight * step / span;
            whole[first + 1] += full;
            whole[last] -= full;
        }
    }
    let mut running = 0.0;
    for (row, volume) in volumes.iter_mut().enumerate() {
        running += whole[row];
        *volume += running;
    }
    volumes
}

/// Each row's volume, `rows` rows of `step` from `base`, from what traded
/// at each price, as `(price, volume)`: every price's volume in the row
/// that holds it, a price on a boundary in the row above. A price outside
/// the rows is counted in the nearest; a volume of nought adds nothing.
pub fn rows_at_prices(prices: impl Iterator<Item = (f64, f64)>, base: f64, step: f64, rows: usize) -> Vec<f64> {
    let mut volumes = vec![0.0f64; rows];
    for (price, volume) in prices {
        if volume == 0.0 {
            continue;
        }
        let row = ((price - base) / step + HAIR).floor();
        let row = (row.max(0.0) as usize).min(rows - 1);
        volumes[row] += volume;
    }
    volumes
}

/// The part of a step a price may fall short of a row's boundary by and
/// still be on it: prices are multiples of a tick, which a float misses by
/// a hair.
pub const HAIR: f64 = 1e-6;

/// The row with the most volume — by its size either way, for a delta —
/// the last of equals. `None` for no rows.
pub fn point_of_control(volumes: &[f64], by_size: bool) -> Option<usize> {
    volumes
        .iter()
        .map(|v| if by_size { v.abs() } else { *v })
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
}

/// The rows of the value area, inclusive: outward from `poc`, always taking
/// the busier side — the upper of two that tie — until they hold `fraction`
/// of `total`. `count(row)` is what a row holds, of `len` rows: volume for
/// a volume profile, TPOs for a market profile.
pub fn value_area(len: usize, count: impl Fn(usize) -> f64, poc: usize, total: f64, fraction: f64) -> (usize, usize) {
    let target = total * fraction.clamp(0.0, 1.0);
    let (mut lower, mut upper) = (poc, poc);
    let mut covered = count(poc);
    while covered < target && (lower > 0 || upper + 1 < len) {
        let below = (lower > 0).then(|| count(lower - 1));
        let above = (upper + 1 < len).then(|| count(upper + 1));
        match (below, above) {
            (Some(b), Some(a)) if b > a => {
                lower -= 1;
                covered += b;
            }
            (Some(_), Some(a)) | (None, Some(a)) => {
                upper += 1;
                covered += a;
            }
            (Some(b), None) => {
                lower -= 1;
                covered += b;
            }
            (None, None) => break,
        }
    }
    (lower, upper)
}

impl Profile {
    /// A profile of `volumes`, rows of `step` from `low`, over the period
    /// `span` (its first and last bars' times) and `bars` (their indexes):
    /// its point of control, longest row and total, and its value area,
    /// `value_area` of the volume. A delta profile (`delta`) has its point
    /// of control at its greatest delta either way and no value area but
    /// that row. `None` with no rows.
    pub fn of_rows(span: (i64, i64), bars: (usize, usize), low: f64, step: f64, volumes: Vec<f64>, value_area: f64, delta: bool) -> Option<Profile> {
        let total: f64 = volumes.iter().sum();
        let poc = point_of_control(&volumes, delta)?;
        let max_volume = volumes[poc].abs();
        let value_area = match delta {
            false => self::value_area(volumes.len(), |row| volumes[row], poc, total, value_area),
            true => (poc, poc),
        };
        Some(Profile {
            start_ts: span.0,
            parts: vec![span.0],
            end_ts: span.1,
            first_bar: bars.0,
            last_bar: bars.1,
            value_area,
            rows: (0..volumes.len())
                .map(|i| ProfileRow { low: low + i as f64 * step, high: low + (i + 1) as f64 * step, volume: volumes[i] })
                .collect(),
            poc,
            max_volume,
            total_volume: total,
        })
    }

    /// The row holding `price`, a price on a boundary in the row above;
    /// `None` outside the profile.
    pub fn row_at(&self, price: f64) -> Option<usize> {
        let first = self.rows.first()?;
        let step = first.high - first.low;
        if step.is_nan() || step <= 0.0 {
            return None;
        }
        let row = ((price - first.low) / step + HAIR).floor();
        (row >= 0.0 && (row as usize) < self.rows.len()).then_some(row as usize)
    }
}
