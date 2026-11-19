//! Market profile.
//!
//! One profile per reset period, as the volume profile has: but where that
//! one weighs each price by what traded there, this counts time. The period
//! is cut into TPO periods of so many minutes, each named by a letter, and
//! every price row a TPO period's bars reach gets that letter once. The row
//! with the most letters is the point of control; the value area is grown
//! out from it until it holds most of them.
//!
//! The counting, the point of control and the value area are
//! `omacharts_studies::market_profile`'s, the arithmetic the developing
//! profile reads a bar at a time, so the two agree at the end of every
//! period. What is here is the chart's part: which bars make each period,
//! how tall a row is when nobody said, and every letter in its row, for
//! drawing them.

use omacharts_studies::market_profile as tpo;
use omacharts_studies::ProfileReading;
use serde::{Deserialize, Serialize};

use crate::bars::{price_decimals, Bar};
use crate::symbols::InstrumentKind;

use super::periods::Reset;
use super::profile::{Profile, ProfileRow};

pub use tpo::{DEFAULT_MINUTES, LETTERS};

/// The value area's share of the TPOs unless somebody says otherwise.
pub const DEFAULT_VALUE_AREA: f64 = 0.70;

/// The shortest and longest a TPO period may be set to: a minute, and a
/// day.
pub const MIN_MINUTES: u32 = 1;
pub const MAX_MINUTES: u32 = 1_440;

/// The tallest a row may be set to, in ticks.
pub const MAX_ROW_TICKS: u32 = 10_000;

/// How each TPO is drawn: its period's letter, or a block.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Letters,
    Blocks,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Letters, Style::Blocks];

    pub fn label(self) -> &'static str {
        match self {
            Style::Letters => "Letters",
            Style::Blocks => "Blocks",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Style::Letters => "letters",
            Style::Blocks => "blocks",
        }
    }

    pub fn from_key(key: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.key() == key)
    }
}

pub fn default_minutes() -> u32 {
    DEFAULT_MINUTES
}

pub fn default_value_area() -> f64 {
    DEFAULT_VALUE_AREA
}

/// One period's market profile.
#[derive(Clone, PartialEq, Debug)]
pub struct MarketProfile {
    /// The period as a profile: a row for each row of prices, centred on
    /// its ticks, holding its TPOs as its volume; the point of control and
    /// the value area by row; the most TPOs a row holds as the longest row,
    /// and every TPO as the total. What lays it out beside a volume profile
    /// and finds it under the pointer, as one.
    pub profile: Profile,
    /// Every TPO's letter, as an index into [`LETTERS`]: the lowest row's
    /// first, in the order their periods came, then the next row's.
    pub letters: Vec<u8>,
    /// Where each row's letters begin in `letters`, and one past the last.
    pub starts: Vec<u32>,
    /// Which rows the profile's first TPO period reached ([`FIRST`]) and
    /// which its last did ([`LAST`]), a row each. The first's letter is
    /// the first of its row, the last's the last, as letters are kept in
    /// the order their periods came.
    pub ends: Vec<u8>,
    /// The point of control and the value area, as prices: what the
    /// developing profile reads at the period's last bar.
    pub reading: ProfileReading,
    /// How tall a row is, in ticks, and the tick.
    pub row_ticks: u32,
    pub tick_size: f64,
}

/// In [`MarketProfile::ends`], a row the first TPO period reached, and one
/// the last did.
pub const FIRST: u8 = 1;
pub const LAST: u8 = 2;

impl MarketProfile {
    /// Whether the `k`th letter of row `row` is the profile's first TPO
    /// period's, and whether it is its last's: both, for a profile of one.
    pub fn end_of(&self, row: usize, k: usize) -> (bool, bool) {
        let ends = self.ends.get(row).copied().unwrap_or(0);
        let len = (self.starts[row + 1] - self.starts[row]) as usize;
        (ends & FIRST != 0 && k == 0, ends & LAST != 0 && k + 1 == len)
    }

    /// The letters of row `row`, in the order their periods came.
    pub fn letters_of(&self, row: usize) -> &[u8] {
        let (from, to) = (self.starts[row] as usize, self.starts[row + 1] as usize);
        &self.letters[from..to]
    }
}

/// The character a letter index stands for.
pub fn letter(index: u8) -> char {
    LETTERS[index as usize % LETTERS.len()] as char
}

/// The fewest rows a typical period is cut into when the height is left
/// automatic, and the most: a row height is the smallest of 1, 2, 4 and 5
/// ticks, then 10, 20, 40 and 50 and on, that keeps the typical period —
/// the median one, so one wild session does not decide — within this many
/// rows. Sixty keeps a letter readable at the height a chart shows a few
/// sessions at, and makes an ES session of sixty points a point a row.
pub const TARGET_ROWS: f64 = 60.0;

/// The most rows one period may be cut into. A bar far out — a bad print —
/// would otherwise ask for millions; past this the period has no profile.
const MAX_ROWS: i64 = 100_000;

/// The tick a chart's prices are whole multiples of: the instrument's own
/// when it is known, else the increment its prices are quoted to.
pub fn tick_size(bars: &[Bar], known: Option<f64>, kind: Option<InstrumentKind>) -> f64 {
    if let Some(tick) = known.filter(|tick| *tick > 0.0 && tick.is_finite()) {
        return tick;
    }
    let price = bars.get(bars.len() / 2).map_or(1.0, |bar| bar.close.abs().max(f64::MIN_POSITIVE));
    10f64.powi(-(price_decimals(0.0, price, kind) as i32))
}

/// How tall a row is, in ticks, when nobody said: see [`TARGET_ROWS`].
pub fn auto_row_ticks(bars: &[Bar], periods: &[(usize, usize)], tick_size: f64) -> u32 {
    let mut spans: Vec<f64> = periods
        .iter()
        .filter_map(|&(from, to)| {
            let (mut low, mut high) = (f64::MAX, f64::MIN);
            for bar in &bars[from..to] {
                low = low.min(bar.low);
                high = high.max(bar.high);
            }
            let ticks = (high - low) / tick_size;
            (ticks.is_finite() && ticks > 0.0).then_some(ticks)
        })
        .collect();
    if spans.is_empty() {
        return 1;
    }
    spans.sort_by(f64::total_cmp);
    let typical = spans[spans.len() / 2];
    for decade in 0..9u32 {
        for base in [1u32, 2, 4, 5] {
            let ticks = base * 10u32.pow(decade);
            if typical / f64::from(ticks) <= TARGET_ROWS {
                return ticks.min(MAX_ROW_TICKS);
            }
        }
    }
    MAX_ROW_TICKS
}

/// One market profile per period of `reset`, in rows of `row_ticks` ticks of
/// `tick_size`, or of the automatic height when `None`, cut into TPO periods
/// of `minutes`.
///
/// A period whose start is in `merged` is folded into the one before it,
/// as a volume profile's is: one composite of them all, its TPO periods
/// lettered afresh from each part's first bar, as they were apart.
#[allow(clippy::too_many_arguments)]
pub fn compute(
    bars: &[Bar],
    reset: Reset,
    session_origin: i64,
    minutes: u32,
    row_ticks: Option<u32>,
    value_area: f64,
    tick_size: f64,
    merged: &[i64],
) -> Vec<MarketProfile> {
    if bars.is_empty() || !(tick_size > 0.0 && tick_size.is_finite()) {
        return Vec::new();
    }
    let alone = super::profile::periods(bars, reset, session_origin);
    // One height for every period, so a quiet session is drawn short beside
    // a busy one rather than stretched to match it.
    let row_ticks = match row_ticks {
        Some(ticks) => ticks.clamp(1, MAX_ROW_TICKS),
        None => auto_row_ticks(bars, &alone, tick_size),
    };
    let minutes = minutes.clamp(MIN_MINUTES, MAX_MINUTES);
    let joined = (!merged.is_empty())
        .then(|| super::profile::composites(bars, alone.clone(), reset, session_origin, merged));
    let periods = joined.as_deref().unwrap_or(&alone);
    let mut out = Vec::with_capacity(periods.len());
    // Room for counting, reused from period to period.
    let mut scratch = Scratch::default();
    // Where each period apart starts: a composite's parts.
    let mut alone = alone.iter().map(|&(from, _)| from).peekable();
    let mut cuts = Vec::new();
    for &(from, to) in periods {
        cuts.clear();
        while let Some(&start) = alone.peek() {
            if start >= to {
                break;
            }
            if start > from {
                cuts.push(start - from);
            }
            alone.next();
        }
        let bucket = reset.bucket(bars[from].ts, session_origin);
        let slice = &bars[from..to];
        if let Some(mut profile) =
            period(slice, from, bucket, minutes, row_ticks, value_area, tick_size, &cuts, &mut scratch)
        {
            profile.profile.parts.extend(cuts.iter().map(|&at| reset.bucket(slice[at].ts, session_origin)));
            out.push(profile);
        }
    }
    out
}

/// What counting a period needs and does not keep: each row's TPOs, the
/// TPO period that last marked it, and where its next letter goes.
#[derive(Default)]
struct Scratch {
    counts: Vec<u32>,
    marks: Vec<i64>,
    next: Vec<u32>,
    /// Each bar's letter, and its TPO period as a mark of its own.
    tpos: Vec<(u8, i64)>,
}

/// The market profile of `bars`, one period, which start at `offset` on the
/// chart and whose period begins at `start_ts`. `cuts` are where each
/// period merged into it begins, as indices into `bars`: each part's TPO
/// periods are counted from its own first bar, and a letter in one part is
/// a TPO apart from the same letter in another.
#[allow(clippy::too_many_arguments)]
fn period(
    bars: &[Bar],
    offset: usize,
    start_ts: i64,
    minutes: u32,
    row_ticks: u32,
    value_area: f64,
    tick_size: f64,
    cuts: &[usize],
    scratch: &mut Scratch,
) -> Option<MarketProfile> {
    let Scratch { counts, marks, next, tpos } = scratch;
    bars.first()?;
    let rows_of = |bar: &Bar| tpo::rows_of(bar, tick_size, row_ticks);
    let (mut low, mut high) = (i64::MAX, i64::MIN);
    for (from, to) in bars.iter().filter_map(rows_of) {
        low = low.min(from);
        high = high.max(to);
    }
    if low > high || high - low >= MAX_ROWS {
        return None;
    }
    let len = (high - low + 1) as usize;
    // The TPO periods are counted from the period's first bar, as the
    // developing profile counts them; in a composite, from each part's.
    // Each bar's TPO period as the letter it is drawn with, and as a mark
    // no other part's TPO period shares.
    tpos.clear();
    let (mut first_ts, mut index, mut cuts) = (bars[0].ts, 0i64, cuts.iter().peekable());
    for (at, bar) in bars.iter().enumerate() {
        if cuts.next_if_eq(&&at).is_some() {
            (first_ts, index) = (bar.ts, index + 1);
        }
        let tpo = tpo::tpo_of(bar.ts, first_ts, minutes);
        tpos.push((tpo::letter_of(tpo), (index << 40) + tpo));
    }

    // How many TPOs each row holds, then each row's letters in place.
    counts.clear();
    counts.resize(len, 0);
    marks.clear();
    marks.resize(len, i64::MIN);
    for (bar, &(_, at)) in bars.iter().zip(tpos.iter()) {
        let Some((from, to)) = rows_of(bar) else { continue };
        for row in from..=to {
            let row = (row - low) as usize;
            if marks[row] != at {
                marks[row] = at;
                counts[row] += 1;
            }
        }
    }
    let mut starts = Vec::with_capacity(len + 1);
    let mut total = 0u32;
    starts.push(0);
    for count in counts.iter() {
        total += count;
        starts.push(total);
    }
    let mut letters = vec![0u8; total as usize];
    // The first TPO period's mark and the last's: the first bar's and the
    // last bar's, as bars come in order.
    let (first, last) = (tpos.first().map(|t| t.1), tpos.last().map(|t| t.1));
    let mut ends = vec![0u8; len];
    next.clear();
    next.extend_from_slice(&starts[..len]);
    marks.clear();
    marks.resize(len, i64::MIN);
    for (bar, &(letter, at)) in bars.iter().zip(tpos.iter()) {
        let Some((from, to)) = rows_of(bar) else { continue };
        for row in from..=to {
            let row = (row - low) as usize;
            if marks[row] != at {
                marks[row] = at;
                letters[next[row] as usize] = letter;
                next[row] += 1;
                if Some(at) == first {
                    ends[row] |= FIRST;
                }
                if Some(at) == last {
                    ends[row] |= LAST;
                }
            }
        }
    }

    let count = |row: usize| counts[row];
    let total = u64::from(total);
    let reading = tpo::reading(low, len, count, total, tick_size, row_ticks, value_area)?;
    let poc = tpo::point_of_control(len, count)?;
    let area = tpo::value_area(len, count, poc, total, value_area);
    // Each row reaches half a tick past its outer ticks, so a row a tick
    // tall is centred on its price and rows meet without a gap.
    let half = tick_size / 2.0;
    let rows = (0..len)
        .map(|at| {
            let (bottom, top) = tpo::row_prices(low + at as i64, tick_size, row_ticks);
            ProfileRow { low: bottom - half, high: top + half, volume: f64::from(counts[at]) }
        })
        .collect();
    Some(MarketProfile {
        profile: Profile {
            start_ts,
            parts: vec![start_ts],
            end_ts: bars.last()?.ts,
            first_bar: offset,
            last_bar: offset + bars.len() - 1,
            rows,
            poc,
            value_area: area,
            max_volume: f64::from(counts[poc]),
            total_volume: total as f64,
        },
        letters,
        starts,
        ends,
        reading,
        row_ticks,
        tick_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(ts: i64, low: f64, high: f64) -> Bar {
        Bar { ts, open: low, high, low, close: high, volume: 1.0 }
    }

    #[test]
    fn each_row_holds_its_tpo_periods_letters_once_each() {
        // A session from midnight: A over 10 to 12 twice, B over 12 to 13,
        // nothing for an hour, then E at 11.
        let bars = vec![
            bar(0, 10.0, 12.0),
            bar(600, 10.0, 12.0),
            bar(1_800, 12.0, 13.0),
            bar(4 * 1_800, 11.0, 11.0),
        ];
        let profiles = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[]);
        assert_eq!(profiles.len(), 1);
        let profile = &profiles[0];
        let text = |row: usize| profile.letters_of(row).iter().map(|l| letter(*l)).collect::<String>();
        assert_eq!((0..4).map(text).collect::<Vec<_>>(), vec!["A", "AE", "AB", "B"]);
        let counts: Vec<f64> = profile.profile.rows.iter().map(|row| row.volume).collect();
        assert_eq!(counts, vec![1.0, 2.0, 2.0, 1.0]);
        assert_eq!(profile.profile.total_volume, 6.0);
        // Rows 11 and 12 tie, as near the middle either way: the higher.
        assert_eq!(profile.profile.poc, 2);
        assert_eq!(profile.reading.poc, 12.0);
        assert_eq!(profile.profile.rows[0].low, 9.5);
        assert_eq!(profile.profile.rows[3].high, 13.5);
    }

    #[test]
    fn one_profile_per_period_at_one_row_height() {
        const DAY: i64 = 86_400;
        let bars = vec![bar(0, 100.0, 101.0), bar(60, 100.5, 102.0), bar(DAY, 200.0, 230.0), bar(DAY + 60, 199.0, 200.0)];
        let profiles = compute(&bars, Reset::Session, 0, 30, None, 0.7, 0.25, &[]);
        assert_eq!(profiles.len(), 2);
        assert_eq!((profiles[0].profile.first_bar, profiles[0].profile.last_bar), (0, 1));
        assert_eq!((profiles[1].profile.first_bar, profiles[1].profile.last_bar), (2, 3));
        assert_eq!(profiles[0].row_ticks, profiles[1].row_ticks);
    }

    #[test]
    fn an_automatic_row_is_the_smallest_round_height_that_keeps_a_period_within_sixty_rows() {
        let session = |range: f64| vec![bar(0, 5_000.0, 5_000.0 + range)];
        let rows = |range: f64| {
            let bars = session(range);
            auto_row_ticks(&bars, &[(0, 1)], 0.25)
        };
        assert_eq!(rows(10.0), 1); // 40 ticks
        assert_eq!(rows(25.0), 2); // 100 ticks
        assert_eq!(rows(60.0), 4); // 240 ticks: a point a row
        assert_eq!(rows(70.0), 5);
        assert_eq!(rows(140.0), 10);
        assert_eq!(auto_row_ticks(&session(0.0), &[(0, 1)], 0.25), 1);
    }

    #[test]
    fn the_tick_is_the_instruments_or_the_quote_increment() {
        let bars = vec![bar(0, 100.0, 101.0)];
        assert_eq!(tick_size(&bars, Some(0.25), None), 0.25);
        assert_eq!(tick_size(&bars, Some(0.0), None), 0.01);
        assert_eq!(tick_size(&bars, None, None), 0.01);
    }

    /// Merged as a volume profile's periods are: one composite of the
    /// sessions, each session's TPO periods lettered from its own first bar
    /// and counted apart, so Tuesday's A at a price is a second TPO there,
    /// not Monday's A again.
    #[test]
    fn a_merged_period_joins_the_one_before_it_its_letters_counted_apart() {
        const DAY: i64 = 86_400;
        let bars = vec![
            bar(0, 10.0, 11.0),
            bar(1_800, 11.0, 11.0),
            bar(DAY, 10.0, 10.0),
            bar(DAY + 1_800, 12.0, 12.0),
            bar(2 * DAY, 20.0, 20.0),
        ];
        let apart = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[]);
        assert_eq!(apart.len(), 3);
        let merged = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[DAY]);
        assert_eq!(merged.len(), 2);
        let composite = &merged[0];
        assert_eq!(composite.profile.parts, vec![0, DAY]);
        assert!(composite.profile.is_composite());
        assert_eq!((composite.profile.first_bar, composite.profile.last_bar), (0, 3));
        let text = |row: usize| composite.letters_of(row).iter().map(|l| letter(*l)).collect::<String>();
        assert_eq!((0..3).map(text).collect::<Vec<_>>(), vec!["AA", "AB", "B"]);
        assert_eq!(composite.profile.total_volume, 5.0);
        // The session after it is as it was apart.
        assert_eq!(merged[1], apart[2]);
        // Every period merged: one profile of them all.
        let all = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[DAY, 2 * DAY]);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].profile.parts, vec![0, DAY, 2 * DAY]);
        // A merge with nothing before it on the chart is left alone.
        assert_eq!(compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[0]), apart);
    }

    /// The first TPO period is the first letter of each row it reached and
    /// the last the last of each: in a composite, the first part's first
    /// and the last part's last, so Tuesday's A is not taken for an opening.
    #[test]
    fn the_first_and_last_tpo_periods_are_found_in_their_rows() {
        let bars = vec![bar(0, 10.0, 12.0), bar(1_800, 12.0, 13.0), bar(3_600, 11.0, 11.0)];
        let profile = &compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[])[0];
        // Rows 10 to 13: A, AC, AB, B.
        let ends = |row: usize| (0..profile.letters_of(row).len()).map(|k| profile.end_of(row, k)).collect::<Vec<_>>();
        assert_eq!(ends(0), vec![(true, false)]);
        assert_eq!(ends(1), vec![(true, false), (false, true)]);
        assert_eq!(ends(2), vec![(true, false), (false, false)]);
        assert_eq!(ends(3), vec![(false, false)]);
        // A period of one TPO is its own first and last.
        let one = &compute(&bars[..1], Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[])[0];
        assert_eq!(one.end_of(0, 0), (true, true));

        const DAY: i64 = 86_400;
        let bars = vec![bar(0, 10.0, 11.0), bar(1_800, 11.0, 11.0), bar(DAY, 10.0, 10.0), bar(DAY + 1_800, 12.0, 12.0)];
        let composite = &compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[DAY])[0];
        // Rows 10 to 12: Monday's A and Tuesday's, Monday's A and B, Tuesday's B.
        let ends = |row: usize| (0..composite.letters_of(row).len()).map(|k| composite.end_of(row, k)).collect::<Vec<_>>();
        assert_eq!(ends(0), vec![(true, false), (false, false)]);
        assert_eq!(ends(1), vec![(true, false), (false, false)]);
        assert_eq!(ends(2), vec![(false, true)]);
    }

    #[test]
    fn a_wild_bar_leaves_its_period_without_a_profile_rather_than_millions_of_rows() {
        let bars = vec![bar(0, 1.0, 1e9)];
        assert!(compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 0.01, &[]).is_empty());
    }
}
