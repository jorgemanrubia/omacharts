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

use std::sync::Arc;

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

/// How solid each sort of row is drawn as designed, as letters and as
/// blocks: the rows outside the value area, those in it, and the point of
/// control's. Blocks are fainter, a filled cell weighing more than a letter.
pub const LETTER_ALPHAS: [f64; 3] = [0.55, 0.95, 1.0];
pub const BLOCK_ALPHAS: [f64; 3] = [0.28, 0.55, 0.8];

/// How thick a market profile's point of control is drawn until set:
/// thicker than a volume profile's hairline, which through a row of
/// letters reads as struck through rather than marked.
pub const POC_WIDTH: f64 = 2.0;

pub fn poc_width() -> f64 {
    POC_WIDTH
}

pub fn is_poc_width(width: &f64) -> bool {
    *width == POC_WIDTH
}

/// How solid a market profile's outline is drawn until set: solid, the
/// profile's own colour, so it reads as the edge of the TPOs it holds.
pub const OUTLINE_ALPHA: f64 = 1.0;

/// The outline's strength: as set, or [`OUTLINE_ALPHA`].
pub fn outline_strength(alpha: Option<f64>) -> f64 {
    held(alpha, OUTLINE_ALPHA)
}

/// How solid a profile's open and close marks are drawn until set: solid,
/// as a fill's arrow is.
pub const END_ALPHA: f64 = 1.0;

/// An open or close mark's strength: as set, or [`END_ALPHA`].
pub fn end_strength(alpha: Option<f64>) -> f64 {
    held(alpha, END_ALPHA)
}

/// How solid each sort of row is drawn, as letters: every row while the
/// value area is not highlighted, the value area's and the rest's while it
/// is, and the point of control's. What a market profile was given of its
/// own, each unset following [`LETTER_ALPHAS`] as designed. Blocks are drawn
/// as much fainter than these as [`BLOCK_ALPHAS`] are than letters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strengths {
    pub rows: f64,
    pub value_area: f64,
    pub outside: f64,
    pub poc: f64,
}

/// A stored strength inside nothing to solid, or `otherwise` for anything
/// that is not a number.
fn held(alpha: Option<f64>, otherwise: f64) -> f64 {
    match alpha {
        Some(alpha) if alpha.is_finite() => alpha.clamp(0.0, 1.0),
        _ => otherwise,
    }
}

impl Strengths {
    /// From a market profile's own `alpha` for its rows and those it set
    /// apart from them: the value area as the rows, the rest the designed
    /// share of them, the point of control solid.
    pub fn of(alpha: Option<f64>, value_area: Option<f64>, outside: Option<f64>, poc: Option<f64>) -> Strengths {
        let [out, inside, at_poc] = LETTER_ALPHAS;
        let rows = held(alpha, inside);
        Strengths {
            rows,
            value_area: held(value_area, rows),
            // Exactly as designed while the rows are.
            outside: held(outside, if alpha.is_none() { out } else { rows * out / inside }),
            poc: held(poc, at_poc),
        }
    }

    /// The same as blocks are drawn: each of the designed ones exactly as
    /// designed, the rest as much fainter as their design.
    pub fn as_blocks(&self) -> Strengths {
        let [out, inside, poc] = LETTER_ALPHAS;
        let [b_out, b_inside, b_poc] = BLOCK_ALPHAS;
        let scale = |v: f64, designed: f64, block: f64| match v == designed {
            true => block,
            false => (v * block / designed).clamp(0.0, 1.0),
        };
        Strengths {
            rows: scale(self.rows, inside, b_inside),
            value_area: scale(self.value_area, inside, b_inside),
            outside: scale(self.outside, out, b_out),
            poc: scale(self.poc, poc, b_poc),
        }
    }
}

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
    /// Each TPO's column, beside `letters`: its TPO period's place among
    /// the profile's, counted from nought across every part of a
    /// composite. What an expanded profile lays its TPOs out by.
    pub columns: Vec<u16>,
    /// The chart's index of each column's first bar: where an expanded
    /// profile draws that column.
    pub column_bars: Vec<usize>,
    /// The row of the profile's open, its first bar's opening price, and
    /// the row of its close, its last bar's closing price (the latest, while
    /// the period forms). The open's TPO is the first letter of its row and
    /// the close's the last of its, as letters are kept in the order their
    /// periods came: in a composite, the first part's open and the last
    /// part's close.
    pub open_row: usize,
    pub close_row: usize,
    /// The point of control and the value area, as prices: what the
    /// developing profile reads at the period's last bar.
    pub reading: ProfileReading,
    /// How tall a row is, in ticks, and the tick.
    pub row_ticks: u32,
    pub tick_size: f64,
}

impl MarketProfile {
    /// Whether the `k`th letter of row `row` is the profile's open TPO,
    /// and whether it is its close: both, for a profile of one TPO that
    /// opened and closed in one row.
    pub fn end_of(&self, row: usize, k: usize) -> (bool, bool) {
        let len = (self.starts[row + 1] - self.starts[row]) as usize;
        (row == self.open_row && k == 0, row == self.close_row && k + 1 == len)
    }

    /// The columns of row `row`'s TPOs, beside [`Self::letters_of`].
    pub fn columns_of(&self, row: usize) -> &[u16] {
        let (from, to) = (self.starts[row] as usize, self.starts[row + 1] as usize);
        &self.columns[from..to]
    }

    /// The letters of row `row`, in the order their periods came.
    pub fn letters_of(&self, row: usize) -> &[u8] {
        let (from, to) = (self.starts[row] as usize, self.starts[row + 1] as usize);
        &self.letters[from..to]
    }
}

/// Whether the `at`th of a market profile's `count` profiles, `profile`,
/// is drawn expanded: its TPOs laid out over time, a column for each TPO
/// period at that period's bars. One expanded by hand is; and the newest,
/// while the study expands the last.
pub fn expands(params: &super::Params, at: usize, count: usize, profile: &Profile) -> bool {
    match params {
        super::Params::MarketProfile { expanded, expand_last, .. } => {
            by_hand(expanded, profile) || (*expand_last && at + 1 == count)
        }
        _ => false,
    }
}

/// Whether `profile` is one of those `expanded` names. A profile is kept
/// by its first period's start, as a merge is: the period it was cut
/// from, not its first bar, so it is the same one whatever bars came in.
pub fn by_hand(expanded: &[i64], profile: &Profile) -> bool {
    profile.parts.first().is_some_and(|start| expanded.contains(start))
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
    let spans: Vec<Option<f64>> = periods
        .iter()
        .map(|&(from, to)| span_in_ticks(&bars[from..to], tick_size))
        .collect();
    auto_from_spans(&spans)
}

/// One market profile per period of `reset`, in rows of `row_ticks` ticks of
/// `tick_size`, or of the automatic height when `None`, cut into TPO periods
/// of `minutes`.
///
/// A period whose start is in `merged` is folded into the one before it,
/// as a volume profile's is: one composite of them all, its TPO periods
/// lettered afresh from each part's first bar, as they were apart.
///
/// `last_only` profiles the newest period alone, in rows of the height
/// every period's would be, as a volume profile's is.
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
    last_only: bool,
) -> Vec<Arc<MarketProfile>> {
    let mut memo = Memo::default();
    compute_kept(
        bars,
        reset,
        session_origin,
        minutes,
        row_ticks,
        value_area,
        tick_size,
        merged,
        last_only,
        &mut memo,
    )
}

/// What a computation leaves for the next one on the same chart: the bars
/// it was worked out over, their periods apart and each one's span, and
/// the profiles. A chart computes its indicators again on every tick, and
/// a tick moves the forming period alone; every period before it is kept.
#[derive(Clone, Debug, Default)]
pub struct Memo {
    key: Option<Key>,
    bars: Vec<Bar>,
    alone: Vec<(usize, usize)>,
    spans: Vec<Option<f64>>,
    row_ticks: u32,
    /// Shared with what the chart draws, so keeping one costs nothing.
    out: Vec<Arc<MarketProfile>>,
    /// How many periods the last computation worked out rather than kept.
    pub worked: usize,
}

/// Everything besides the bars that the profiles depend on.
#[derive(Clone, PartialEq, Debug)]
struct Key {
    reset: Reset,
    session_origin: i64,
    minutes: u32,
    row_ticks: Option<u32>,
    value_area: u64,
    tick_size: u64,
    merged: Vec<i64>,
    last_only: bool,
}

/// Each chart's memos, by the indicator's id.
#[derive(Debug, Default)]
pub struct Memos(std::cell::RefCell<std::collections::HashMap<u32, Memo>>);

impl Memos {
    /// Run `work` with the memo kept for indicator `id`.
    pub fn with<T>(&self, id: u32, work: impl FnOnce(&mut Memo) -> T) -> T {
        let mut memos = self.0.borrow_mut();
        work(memos.entry(id).or_default())
    }

    /// Forget every memo but those of `ids`: an indicator taken off.
    pub fn keep(&self, ids: &[u32]) {
        self.0.borrow_mut().retain(|id, _| ids.contains(id));
    }
}

/// [`compute`], keeping in `memo` what the next computation over the same
/// bars and a tick more need not work out again: every period that ended
/// before the first bar that changed, and the newest one before that
/// besides, which the next bar may yet belong to.
#[allow(clippy::too_many_arguments)]
pub fn compute_kept(
    bars: &[Bar],
    reset: Reset,
    session_origin: i64,
    minutes: u32,
    row_ticks: Option<u32>,
    value_area: f64,
    tick_size: f64,
    merged: &[i64],
    last_only: bool,
    memo: &mut Memo,
) -> Vec<Arc<MarketProfile>> {
    memo.worked = 0;
    if bars.is_empty() || !(tick_size > 0.0 && tick_size.is_finite()) {
        *memo = Memo::default();
        return Vec::new();
    }
    let key = Key {
        reset,
        session_origin,
        minutes,
        row_ticks,
        value_area: value_area.to_bits(),
        tick_size: tick_size.to_bits(),
        merged: merged.to_vec(),
        last_only,
    };
    if memo.key.as_ref() != Some(&key) {
        *memo = Memo {
            key: Some(key),
            ..Memo::default()
        };
    }

    // The periods apart that are kept: each that ended before the first
    // bar that changed, short of the memo's newest, which may go on.
    let same = memo
        .bars
        .iter()
        .zip(bars)
        .take_while(|(a, b)| a == b)
        .count();
    let kept = memo
        .alone
        .len()
        .saturating_sub(1)
        .min(memo.alone.iter().take_while(|&&(_, to)| to <= same).count());
    let restart = kept.checked_sub(1).map_or(0, |last| memo.alone[last].1);
    memo.alone.truncate(kept);
    memo.spans.truncate(kept);
    memo.alone.extend(
        super::profile::periods(&bars[restart..], reset, session_origin)
            .into_iter()
            .map(|(from, to)| (from + restart, to + restart)),
    );
    for &(from, to) in &memo.alone[kept..] {
        memo.spans.push(span_in_ticks(&bars[from..to], tick_size));
    }
    memo.bars.truncate(restart);
    memo.bars.extend_from_slice(&bars[restart..]);

    // One height for every period, so a quiet session is drawn short beside
    // a busy one rather than stretched to match it; and a new height is a
    // new look for every one of them.
    let row_ticks = match row_ticks {
        Some(ticks) => ticks.clamp(1, MAX_ROW_TICKS),
        None => auto_from_spans(&memo.spans),
    };
    let reuse = if row_ticks == memo.row_ticks {
        std::mem::take(&mut memo.out)
    } else {
        Vec::new()
    };
    memo.row_ticks = row_ticks;
    let minutes = minutes.clamp(MIN_MINUTES, MAX_MINUTES);
    let joined = (!merged.is_empty()).then(|| {
        super::profile::composites(bars, memo.alone.clone(), reset, session_origin, merged)
    });
    let periods = joined.as_deref().unwrap_or(&memo.alone);
    // A profile kept is one of a period that ends where it did, short of
    // the restart; one whose period goes on past it is worked out again.
    let mut reuse = reuse
        .into_iter()
        .filter(|kept| kept.profile.last_bar < restart)
        .peekable();
    let mut out = Vec::with_capacity(periods.len());
    // Room for counting, reused from period to period.
    let mut scratch = Scratch::default();
    // Where each period apart starts: a composite's parts.
    let mut alone = memo.alone.iter().map(|&(from, _)| from).peekable();
    let mut cuts = Vec::new();
    let skipped = if last_only { periods.len().saturating_sub(1) } else { 0 };
    for (at, &(from, to)) in periods.iter().enumerate() {
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
        if at < skipped {
            continue;
        }
        while reuse
            .next_if(|kept| kept.profile.first_bar < from)
            .is_some()
        {}
        if to <= restart
            && let Some(kept) = reuse
                .next_if(|kept| kept.profile.first_bar == from && kept.profile.last_bar + 1 == to)
        {
            out.push(kept);
            continue;
        }
        memo.worked += 1;
        let bucket = reset.bucket(bars[from].ts, session_origin);
        let slice = &bars[from..to];
        if let Some(mut profile) = period(
            slice,
            from,
            bucket,
            minutes,
            row_ticks,
            value_area,
            tick_size,
            &cuts,
            &mut scratch,
        ) {
            profile.profile.parts.extend(
                cuts.iter()
                    .map(|&at| reset.bucket(slice[at].ts, session_origin)),
            );
            out.push(Arc::new(profile));
        }
    }
    memo.out = out.clone();
    out
}

/// How many ticks a period's bars span, low to high, or `None` for one
/// that spans none.
fn span_in_ticks(bars: &[Bar], tick_size: f64) -> Option<f64> {
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    for bar in bars {
        low = low.min(bar.low);
        high = high.max(bar.high);
    }
    let ticks = (high - low) / tick_size;
    (ticks.is_finite() && ticks > 0.0).then_some(ticks)
}

/// [`auto_row_ticks`] from each period's span, in ticks.
fn auto_from_spans(spans: &[Option<f64>]) -> u32 {
    let mut spans: Vec<f64> = spans.iter().flatten().copied().collect();
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

/// What counting a period needs and does not keep: each row's TPOs, the
/// TPO period that last marked it, and where its next letter goes.
#[derive(Default)]
struct Scratch {
    counts: Vec<u32>,
    marks: Vec<i64>,
    next: Vec<u32>,
    /// Each bar's letter, its TPO period as a mark of its own, and its
    /// column.
    tpos: Vec<(u8, i64, u16)>,
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
    let mut column_bars: Vec<usize> = Vec::new();
    let (mut first_ts, mut index, mut cuts) = (bars[0].ts, 0i64, cuts.iter().peekable());
    for (at, bar) in bars.iter().enumerate() {
        if cuts.next_if_eq(&&at).is_some() {
            (first_ts, index) = (bar.ts, index + 1);
        }
        let tpo = tpo::tpo_of(bar.ts, first_ts, minutes);
        let mark = (index << 40) + tpo;
        // The bars come in order, so a TPO period's are together.
        if tpos.last().is_none_or(|&(_, last, _)| last != mark) {
            column_bars.push(offset + at);
        }
        let column = (column_bars.len() - 1).min(u16::MAX as usize) as u16;
        tpos.push((tpo::letter_of(tpo), mark, column));
    }

    // How many TPOs each row holds, then each row's letters in place.
    counts.clear();
    counts.resize(len, 0);
    marks.clear();
    marks.resize(len, i64::MIN);
    for (bar, &(_, at, _)) in bars.iter().zip(tpos.iter()) {
        let Some((from, to)) = rows_of(bar) else {
            continue;
        };
        tpo::mark(counts, marks, (from - low) as usize..=(to - low) as usize, at);
    }
    let mut starts = Vec::with_capacity(len + 1);
    let mut total = 0u32;
    starts.push(0);
    for count in counts.iter() {
        total += count;
        starts.push(total);
    }
    let mut letters = vec![0u8; total as usize];
    let mut columns = vec![0u16; total as usize];
    next.clear();
    next.extend_from_slice(&starts[..len]);
    marks.clear();
    marks.resize(len, i64::MIN);
    for (bar, &(letter, at, column)) in bars.iter().zip(tpos.iter()) {
        let Some((from, to)) = rows_of(bar) else {
            continue;
        };
        for row in from..=to {
            let row = (row - low) as usize;
            if marks[row] != at {
                marks[row] = at;
                letters[next[row] as usize] = letter;
                columns[next[row] as usize] = column;
                next[row] += 1;
            }
        }
    }

    // The open's row and the close's, kept within the bar that made each,
    // so each is a row that bar's TPO period reached.
    let row_at = |bar: &Bar, price: f64| {
        let (from, to) = rows_of(bar)?;
        let row = tpo::row_of(price, tick_size, row_ticks).unwrap_or(from).clamp(from, to);
        Some((row - low) as usize)
    };
    let (open_row, close_row) = (row_at(&bars[0], bars[0].open)?, row_at(bars.last()?, bars.last()?.close)?);

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
        columns,
        column_bars,
        open_row,
        close_row,
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
        let profiles = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[], false);
        assert_eq!(profiles.len(), 1);
        let profile = &profiles[0];
        let text = |row: usize| {
            profile
                .letters_of(row)
                .iter()
                .map(|l| letter(*l))
                .collect::<String>()
        };
        assert_eq!(
            (0..4).map(text).collect::<Vec<_>>(),
            vec!["A", "AE", "AB", "B"]
        );
        // Laid out over time, a column for each TPO period that has bars,
        // starting at its first: A at the first bar, B at the third, E at
        // the fourth, an hour of nothing between B and E taking none.
        assert_eq!(profile.column_bars, vec![0, 2, 3]);
        let columns = |row: usize| profile.columns_of(row).to_vec();
        assert_eq!(
            (0..4).map(columns).collect::<Vec<_>>(),
            vec![vec![0], vec![0, 2], vec![0, 1], vec![1]]
        );
        let counts: Vec<f64> = profile.profile.rows.iter().map(|row| row.volume).collect();
        assert_eq!(counts, vec![1.0, 2.0, 2.0, 1.0]);
        assert_eq!(profile.profile.total_volume, 6.0);
        // Rows 11 and 12 tie, as near the middle either way: the higher.
        assert_eq!(profile.profile.poc, 2);
        assert_eq!(profile.reading.poc, 12.0);
        assert_eq!(profile.profile.rows[0].low, 9.5);
        assert_eq!(profile.profile.rows[3].high, 13.5);
    }

    /// Kept from the last computation, a tick works out the forming
    /// period alone, its profile growing with it — a new TPO period's
    /// letter, the value area, the point of control and the close — and
    /// every session before it is the one already worked out.
    #[test]
    fn a_tick_works_out_the_forming_period_alone() {
        const DAY: i64 = 86_400;
        // Five sessions of minute bars, the last forming.
        let mut bars: Vec<Bar> = Vec::new();
        for day in 0..5i64 {
            for minute in 0..120i64 {
                let low = 100.0 + day as f64 + (minute % 13) as f64 * 0.25;
                bars.push(Bar {
                    ts: day * DAY + minute * 60,
                    open: low,
                    high: low + 1.0,
                    low,
                    close: low + 0.5,
                    volume: 1.0,
                });
            }
        }
        let run = |bars: &[Bar], memo: &mut Memo| {
            compute_kept(
                bars,
                Reset::Session,
                0,
                30,
                Some(1),
                0.7,
                0.25,
                &[],
                false,
                memo,
            )
        };
        let fresh =
            |bars: &[Bar]| compute(bars, Reset::Session, 0, 30, Some(1), 0.7, 0.25, &[], false);
        let mut memo = Memo::default();
        let first = run(&bars, &mut memo);
        assert_eq!(first, fresh(&bars));
        assert_eq!(memo.worked, 5);

        // A tick into the forming bar, high above everything so far.
        let last = bars.len() - 1;
        bars[last].high += 5.0;
        bars[last].close = bars[last].high;
        let ticked = run(&bars, &mut memo);
        assert_eq!(memo.worked, 1, "the forming session alone");
        assert_eq!(
            ticked,
            fresh(&bars),
            "what working out every session comes to"
        );
        assert_eq!(ticked[..4], first[..4]);
        assert_ne!(ticked[4], first[4], "the forming profile grew");
        assert_eq!(
            ticked[4].close_row,
            ticked[4].profile.rows.len() - 1,
            "the close follows the latest price"
        );

        // A new bar opening the next TPO period: its letter appears.
        let at = bars[last].ts + 60 * 15;
        bars.push(Bar {
            ts: at,
            open: 101.0,
            high: 101.5,
            low: 99.0,
            close: 99.0,
            volume: 1.0,
        });
        let grown = run(&bars, &mut memo);
        assert_eq!(memo.worked, 1);
        assert_eq!(grown, fresh(&bars));
        let letters = |p: &MarketProfile| p.letters.iter().copied().max();
        assert_eq!(
            letters(&grown[4]),
            letters(&ticked[4]).map(|l| l + 1),
            "a new period's letter"
        );

        // What was kept is not worked out again: spoiled in the memo, it
        // stays spoiled through a tick.
        Arc::make_mut(&mut memo.out[1]).open_row = 9_999;
        bars.last_mut().unwrap().close = 99.5;
        let again = run(&bars, &mut memo);
        assert_eq!(
            again[1].open_row, 9_999,
            "the sessions before the forming one were kept"
        );
        assert_eq!(memo.worked, 1);

        // A bar of a new session starts it, and the one before is kept.
        let mut memo = Memo::default();
        let _ = run(&bars, &mut memo);
        bars.push(Bar {
            ts: 5 * DAY,
            open: 110.0,
            high: 111.0,
            low: 110.0,
            close: 110.5,
            volume: 1.0,
        });
        let next = run(&bars, &mut memo);
        assert_eq!(next, fresh(&bars));
        assert_eq!(next.len(), 6);
        assert_eq!(
            memo.worked, 2,
            "the session that was forming, and the new one"
        );

        // Another setting is worked out from the start.
        let other = compute_kept(
            &bars,
            Reset::Session,
            0,
            15,
            Some(1),
            0.7,
            0.25,
            &[],
            false,
            &mut memo,
        );
        assert_eq!(memo.worked, 6);
        assert_eq!(
            other,
            compute(&bars, Reset::Session, 0, 15, Some(1), 0.7, 0.25, &[], false)
        );
    }

    /// The automatic row height reads every session, the forming one
    /// among them: when a tick changes it, every profile is worked out in
    /// the new height, and merged sessions stay one composite.
    #[test]
    fn a_tick_that_changes_the_automatic_height_works_out_every_session() {
        const DAY: i64 = 86_400;
        let mut bars: Vec<Bar> = Vec::new();
        // A session eleven points tall, then two of two points, the last
        // forming: the median is two points until it grows past eleven.
        for day in 0..3i64 {
            for minute in 0..60i64 {
                let low = if day == 0 {
                    100.0 + (minute % 10) as f64
                } else {
                    100.0
                };
                bars.push(Bar {
                    ts: day * DAY + minute * 60,
                    open: low,
                    high: low + 2.0,
                    low,
                    close: low + 1.0,
                    volume: 1.0,
                });
            }
        }
        let merged = [DAY];
        let run = |bars: &[Bar], memo: &mut Memo| {
            compute_kept(
                bars,
                Reset::Session,
                0,
                30,
                None,
                0.7,
                0.01,
                &merged,
                false,
                memo,
            )
        };
        let fresh =
            |bars: &[Bar]| compute(bars, Reset::Session, 0, 30, None, 0.7, 0.01, &merged, false);
        let mut memo = Memo::default();
        let first = run(&bars, &mut memo);
        assert_eq!(first.len(), 2, "the first two sessions merged");
        for (at, wild) in [(bars.len() - 1, 30.0), (bars.len() - 2, 10.0)] {
            bars[at].high += wild;
            let ticked = run(&bars, &mut memo);
            assert_eq!(ticked, fresh(&bars));
        }
        assert_ne!(
            run(&bars, &mut memo)[0].row_ticks,
            first[0].row_ticks,
            "a new height, for every session"
        );
    }

    #[test]
    fn one_profile_per_period_at_one_row_height() {
        const DAY: i64 = 86_400;
        let bars = vec![bar(0, 100.0, 101.0), bar(60, 100.5, 102.0), bar(DAY, 200.0, 230.0), bar(DAY + 60, 199.0, 200.0)];
        let profiles = compute(&bars, Reset::Session, 0, 30, None, 0.7, 0.25, &[], false);
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
        let apart = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[], false);
        assert_eq!(apart.len(), 3);
        let merged = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[DAY], false);
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
        let all = compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[DAY, 2 * DAY], false);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].profile.parts, vec![0, DAY, 2 * DAY]);
        // A merge with nothing before it on the chart is left alone.
        assert_eq!(compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[0], false), apart);
    }

    /// The open is the first letter of the row the first bar opened in and
    /// the close the last letter of the row the last bar closed in: one TPO
    /// each, wherever in its bar's range the price was. In a composite, the
    /// first part's open and the last part's close, so Tuesday's A is not
    /// taken for an opening.
    #[test]
    fn the_open_and_close_are_one_tpo_each_at_their_prices() {
        let ohlc = |ts: i64, open: f64, high: f64, low: f64, close: f64| Bar { ts, open, high, low, close, volume: 1.0 };
        let bars = vec![ohlc(0, 11.0, 12.0, 10.0, 12.0), ohlc(1_800, 12.0, 13.0, 12.0, 13.0), ohlc(3_600, 11.0, 11.0, 11.0, 11.0)];
        let profile = &compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[], false)[0];
        // Rows 10 to 13: A, AC, AB, B. Opened at 11, closed at 11.
        let ends = |row: usize| (0..profile.letters_of(row).len()).map(|k| profile.end_of(row, k)).collect::<Vec<_>>();
        assert_eq!(ends(0), vec![(false, false)]);
        assert_eq!(ends(1), vec![(true, false), (false, true)]);
        assert_eq!(ends(2), vec![(false, false), (false, false)]);
        assert_eq!(ends(3), vec![(false, false)]);
        // A profile of one TPO that opened and closed in one row is both.
        let one = &compute(&bars[2..], Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[], false)[0];
        assert_eq!(one.end_of(0, 0), (true, true));
        // A price outside its bar's range is kept to the bar's rows.
        let odd = vec![ohlc(0, 9.0, 12.0, 10.0, 14.0)];
        let odd = &compute(&odd, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[], false)[0];
        assert_eq!((odd.open_row, odd.close_row), (0, 2));

        const DAY: i64 = 86_400;
        let bars = vec![
            ohlc(0, 10.0, 11.0, 10.0, 11.0),
            ohlc(1_800, 11.0, 11.0, 11.0, 11.0),
            ohlc(DAY, 10.0, 10.0, 10.0, 10.0),
            ohlc(DAY + 1_800, 12.0, 12.0, 12.0, 12.0),
        ];
        let composite = &compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 1.0, &[DAY], false)[0];
        // Rows 10 to 12: Monday's A and Tuesday's, Monday's A and B, Tuesday's B.
        let ends = |row: usize| (0..composite.letters_of(row).len()).map(|k| composite.end_of(row, k)).collect::<Vec<_>>();
        assert_eq!(ends(0), vec![(true, false), (false, false)]);
        assert_eq!(ends(1), vec![(false, false), (false, false)]);
        assert_eq!(ends(2), vec![(false, true)]);
    }

    /// In the last period only, the newest period is profiled alone,
    /// exactly as it is beside the others.
    #[test]
    fn the_last_period_only_is_the_newest_profile_as_it_was() {
        const DAY: i64 = 86_400;
        let bars = vec![bar(0, 10.0, 11.0), bar(DAY, 10.0, 12.0), bar(2 * DAY, 11.0, 14.0)];
        let every = compute(&bars, Reset::Session, 0, 30, None, 0.7, 1.0, &[], false);
        let last = compute(&bars, Reset::Session, 0, 30, None, 0.7, 1.0, &[], true);
        assert_eq!(every.len(), 3);
        assert_eq!(last, vec![every[2].clone()]);
        // A composite of the newest two is the newest period.
        let merged = compute(&bars, Reset::Session, 0, 30, None, 0.7, 1.0, &[2 * DAY], true);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].profile.parts, vec![DAY, 2 * DAY]);
    }

    /// Unset, every strength is the one designed, as letters and as blocks
    /// to the bit; set, a block is as much fainter as designed, and what was
    /// not set apart follows the rows.
    #[test]
    fn a_market_profiles_strengths_are_as_designed_until_set() {
        let designed = Strengths::of(None, None, None, None);
        assert_eq!(
            [designed.outside, designed.value_area, designed.poc, designed.rows],
            [LETTER_ALPHAS[0], LETTER_ALPHAS[1], LETTER_ALPHAS[2], LETTER_ALPHAS[1]]
        );
        let blocks = designed.as_blocks();
        assert_eq!(
            [blocks.outside, blocks.value_area, blocks.poc, blocks.rows],
            [BLOCK_ALPHAS[0], BLOCK_ALPHAS[1], BLOCK_ALPHAS[2], BLOCK_ALPHAS[1]]
        );

        let half = Strengths::of(Some(0.475), None, None, None);
        assert_eq!(half.value_area, 0.475, "the value area follows the rows");
        assert!((half.outside - 0.275).abs() < 1e-12, "the rest at the designed share: {}", half.outside);
        assert_eq!(half.poc, 1.0, "the point of control keeps its own");
        assert!((half.as_blocks().rows - 0.275).abs() < 1e-12);

        let own = Strengths::of(None, Some(0.5), Some(0.1), Some(0.4));
        assert_eq!((own.value_area, own.outside, own.poc), (0.5, 0.1, 0.4));
        assert_eq!(own.rows, LETTER_ALPHAS[1]);
        assert_eq!(Strengths::of(Some(f64::NAN), Some(3.0), None, None).value_area, 1.0);
    }

    #[test]
    fn a_wild_bar_leaves_its_period_without_a_profile_rather_than_millions_of_rows() {
        let bars = vec![bar(0, 1.0, 1e9)];
        assert!(compute(&bars, Reset::Session, 0, 30, Some(1), 0.7, 0.01, &[], false).is_empty());
    }
}
