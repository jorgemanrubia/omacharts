//! Volume profile.
//!
//! One profile per reset period: how much volume traded at each price, the
//! price that saw the most (the point of control), and the band around it
//! that covers most of the period's volume (the value area).
//!
//! Each bar's volume is spread across every row its range touches, in
//! proportion to how much of the bar sits in each. Dropping it all on the
//! close would make the profile a histogram of closes, which is a different
//! and less useful picture.

use crate::bars::{price_decimals, Bar};
use crate::ladders::{to_price, Ladder};
use crate::symbols::InstrumentKind;

use serde::{Deserialize, Serialize};

use super::periods::Reset;

/// Periods of one volume profile on one chart that were merged into the one
/// before them, for one symbol.
///
/// Kept by the chart rather than in the indicator, because the indicator may
/// be a template's, shared by charts of other symbols; and by symbol, because
/// a Tuesday merged into Monday on SPY says nothing about AAPL's Tuesday.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Merge {
    pub indicator: u32,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// The period the profiles were merged under. Weeks and sessions share
    /// their starts on a Monday, so a merge kept without it would join two
    /// weeks the moment the profile was switched to weekly.
    #[serde(default)]
    pub reset: Reset,
    /// The start of each merged period; see [`compute`].
    pub starts: Vec<i64>,
}

impl Merge {
    /// Whether these are the merges of indicator `id` on this instrument,
    /// under this period.
    pub fn is(&self, id: u32, instrument: &crate::symbols::Instrument, reset: Reset) -> bool {
        self.indicator == id && self.symbol == instrument.symbol && self.suffix == instrument.suffix && self.reset == reset
    }
}

/// The study's lines as they have always been drawn: a hairline.
pub fn hairline() -> f64 {
    1.0
}

/// How a profile's rows are painted: as solid bars, or as only the shape
/// they make together, which leaves the candles under it in plain sight.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    #[default]
    Filled,
    Outline,
}

impl Style {
    pub const ALL: [Style; 2] = [Style::Filled, Style::Outline];

    pub fn label(self) -> &'static str {
        match self {
            Style::Filled => "Filled",
            Style::Outline => "Outline",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Style::Filled => "filled",
            Style::Outline => "outline",
        }
    }

    pub fn from_key(key: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// Which edge of its period a profile grows from: the left, into the period,
/// or the right, back towards its start, so the rows end where the period
/// does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Right,
}

impl Align {
    pub const ALL: [Align; 2] = [Align::Left, Align::Right];

    pub fn label(self) -> &'static str {
        match self {
            Align::Left => "Left",
            Align::Right => "Right",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Right => "right",
        }
    }

    pub fn from_key(key: &str) -> Option<Align> {
        Align::ALL.into_iter().find(|a| a.key() == key)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ProfileRow {
    pub low: f64,
    pub high: f64,
    pub volume: f64,
}

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
    /// Row holding the most volume.
    pub poc: usize,
    /// Inclusive row range covering `value_area` of the volume.
    pub value_area: (usize, usize),
    pub max_volume: f64,
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

/// The fewest and most rows a period is ever divided into.
///
/// The floor keeps a motionless period from collapsing to a single block; the
/// ceiling keeps a profile from becoming a smear of hairlines on a chart only
/// a few hundred pixels tall.
const MIN_ROWS: usize = 4;
const MAX_ROWS: usize = 800;

/// How many rows the typical period should get when the count is automatic.
///
/// Not a target to hit exactly — the row height is snapped to a round price
/// step and the count falls out of that, so the realised count lands anywhere
/// from about a hundred to this. It is a ceiling the snapping stays under.
///
/// Set where it is because of what it has to beat. Forty-eight rows across an
/// Apple session is nine cents a row, which hides exactly the shelf a profile
/// is read for; a nickel a row is the increment the stock actually trades on,
/// and that comes out near a hundred and sixty. Raised from 250 when that
/// read as blocky: a bar's volume is spread over every row it covers, so finer
/// rows draw a smoother curve rather than a noisier one.
const TARGET_ROWS: f64 = 400.0;

/// The fewest rows an automatic period is drawn with before its rows are
/// split. The step is the chart's, so a period far quieter than the typical
/// one — this month two days in, against whole months — would be a handful
/// of blocks.
const SMOOTH_ROWS: f64 = 40.0;

/// One profile per period, with each period whose start is in `merged` folded
/// into the one before it: a composite, profiled as a single period.
pub fn compute(
    bars: &[Bar],
    reset: Reset,
    session_origin: i64,
    rows: Option<usize>,
    value_area: f64,
    kind: Option<InstrumentKind>,
    merged: &[i64],
) -> Vec<Profile> {
    compute_with(bars, reset, session_origin, rows, value_area, kind, merged, None)
}

/// What traded at each price, for a chart whose source says: the ladder of
/// any run of its bars, and the tick its prices are whole multiples of.
pub struct AtPrice<'a> {
    pub tick_size: f64,
    /// The ladder of exactly these bars, or `None` when it is not known
    /// whole — before the source's history begins, say. That period keeps
    /// the estimate.
    pub of: &'a dyn Fn(&[Bar]) -> Option<Ladder>,
}

/// [`compute`], drawing each period from the volume that actually traded at
/// each price when `at_price` knows it. Without it, or for a period it does
/// not know, the volume of each bar is spread over its range as always.
#[allow(clippy::too_many_arguments)]
pub fn compute_with(
    bars: &[Bar],
    reset: Reset,
    session_origin: i64,
    rows: Option<usize>,
    value_area: f64,
    kind: Option<InstrumentKind>,
    merged: &[i64],
    at_price: Option<&AtPrice>,
) -> Vec<Profile> {
    let mut out = Vec::new();
    if bars.is_empty() {
        return out;
    }

    let periods = composites(bars, periods(bars, reset, session_origin), reset, session_origin, merged);
    // One price step for the whole chart rather than one per period, so a
    // quiet session is drawn as a short profile next to a busy one rather than
    // being stretched to the same height. Comparing two sessions is most of
    // what a volume profile is for.
    let step = rows.is_none().then(|| auto_step(bars, &periods, kind));

    let grid = match step {
        // A row a fraction of the contract's tick would hold every price or
        // none, a comb rather than a profile: with ladders the step is whole
        // ticks, for the estimated periods beside them too.
        Some((step, tick)) => match at_price {
            Some(at) => Grid::Step { step: on_ticks(step, at.tick_size), tick: tick.max(at.tick_size) },
            None => Grid::Step { step, tick },
        },
        None => Grid::Rows(rows.unwrap_or(MIN_ROWS).clamp(MIN_ROWS, MAX_ROWS)),
    };

    for (start, end) in periods {
        let slice = &bars[start..end];
        let bucket = reset.bucket(bars[start].ts, session_origin);
        let exact = at_price.and_then(|at| {
            let ladder = (at.of)(slice)?;
            ladder.levels.iter().any(|level| level.volume() > 0).then_some((ladder, at.tick_size))
        });
        let profile = match exact {
            Some((ladder, tick)) => one_exact(slice, start, bucket, grid, value_area, &ladder, tick),
            None => one(slice, start, bucket, grid, value_area),
        };
        if let Some(mut profile) = profile {
            profile.parts = parts(slice, reset, session_origin);
            out.push(profile);
        }
    }
    out
}

/// The periods with each one that starts at a merged bucket joined onto the
/// one before it. A merged start with no period before it on the chart is
/// left alone: there is nothing loaded to join it to.
fn composites(
    bars: &[Bar],
    periods: Vec<(usize, usize)>,
    reset: Reset,
    session_origin: i64,
    merged: &[i64],
) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(periods.len());
    for (start, end) in periods {
        let joins = merged.contains(&reset.bucket(bars[start].ts, session_origin));
        match out.last_mut() {
            Some(last) if joins => last.1 = end,
            _ => out.push((start, end)),
        }
    }
    out
}

/// The start of every period among `bars`, in order.
fn parts(bars: &[Bar], reset: Reset, session_origin: i64) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for bar in bars {
        let bucket = reset.bucket(bar.ts, session_origin);
        if out.last() != Some(&bucket) {
            out.push(bucket);
        }
    }
    out
}

/// One profile of every bar in `bars`, which start at `offset` on the chart:
/// what a volume profile drawing shows of the span it covers.
pub fn of_span(
    bars: &[Bar],
    offset: usize,
    rows: Option<usize>,
    value_area: f64,
    kind: Option<InstrumentKind>,
) -> Option<Profile> {
    let first = bars.first()?;
    let grid = match rows {
        Some(rows) => Grid::Rows(rows.clamp(MIN_ROWS, MAX_ROWS)),
        None => {
            let (step, tick) = auto_step(bars, &[(0, bars.len())], kind);
            Grid::Step { step, tick }
        }
    };
    one(bars, offset, first.ts, grid, value_area)
}

/// How a period is divided up.
///
/// A fixed count stretches to whatever the period happened to cover, so two
/// sessions drawn side by side have rows of different heights and cannot be
/// read against each other. A fixed step does the opposite: every row on the
/// chart is the same slice of price, and a session that moved half as far is
/// drawn half as tall, which is the comparison the profile is read for.
#[derive(Clone, Copy)]
enum Grid {
    Rows(usize),
    /// Rows `step` tall, which a quiet period may split down to `tick`.
    Step { step: f64, tick: f64 },
}

/// Where each reset period starts and ends, as indices into `bars`.
fn periods(bars: &[Bar], reset: Reset, session_origin: i64) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut bucket = reset.bucket(bars[0].ts, session_origin);
    for i in 1..=bars.len() {
        let next = bars.get(i).map(|b| reset.bucket(b.ts, session_origin));
        if next == Some(bucket) {
            continue;
        }
        out.push((start, i));
        if let Some(next) = next {
            bucket = next;
            start = i;
        }
    }
    out
}

fn span(bars: &[Bar]) -> f64 {
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    for bar in bars {
        low = low.min(bar.low);
        high = high.max(bar.high);
    }
    if low.is_finite() && high.is_finite() {
        high - low
    } else {
        0.0
    }
}

/// The height of one row, in price, when the count is left automatic.
///
/// A row boundary at 182.3617 is a boundary nobody can read off an axis, so
/// the height is snapped to a round multiple of the instrument's own quote
/// increment: five cents on a share, a tenth of a pip on a currency major,
/// twenty-five dollars on bitcoin. The multiple is the smallest one that keeps
/// the typical period near [`TARGET_ROWS`], so a quiet instrument gets fine
/// rows and a volatile one coarse ones without anybody choosing a number.
///
/// This is what the user means by rows that suit the price: forty-odd rows
/// across an Apple session is nine cents a row, which hides exactly the shelf
/// a profile is read for.
///
/// The widest period has a say as well as the typical one. The step is one
/// step for the whole chart, but the row count is capped per period, so a
/// period that moved much further than the median would run out of rows and
/// be drawn across a fraction of its own range — a sliver, with everything
/// above it clamped into the top row. Picking a step that fits the widest
/// period inside [`MAX_ROWS`] is what keeps every period whole.
fn auto_step(bars: &[Bar], periods: &[(usize, usize)], kind: Option<InstrumentKind>) -> (f64, f64) {
    let price = bars[bars.len() / 2].close.abs().max(f64::MIN_POSITIVE);
    let tick = 10f64.powi(-(price_decimals(0.0, price, kind) as i32));

    // The median period, not the mean: one opening gap or one holiday session
    // should not set the scale for every other day on the chart.
    let mut spans: Vec<f64> =
        periods.iter().map(|&(a, b)| span(&bars[a..b])).filter(|s| *s > 0.0).collect();
    if spans.is_empty() {
        return (tick, tick);
    }
    spans.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let typical = spans[spans.len() / 2];
    let widest = *spans.last().unwrap_or(&typical);

    // 1, 2 and 5 ticks, then the same at every power of ten — the increments
    // an exchange and a trader both think in.
    let mut multiple = 1.0f64;
    for decade in 0..12 {
        for base in [1.0, 2.0, 5.0] {
            multiple = base * 10f64.powi(decade);
            let step = multiple * tick;
            if typical / step <= TARGET_ROWS && widest / step <= MAX_ROWS as f64 {
                return (step, tick);
            }
        }
    }
    (multiple * tick, tick)
}

/// The chart's step, split for a period too quiet to be drawn smoothly at it:
/// by 2, 5 or 10 and on, so every row edge is still on the chart's grid, and
/// never finer than the instrument's own increment.
fn smoothed(step: f64, tick: f64, span: f64) -> f64 {
    if span <= 0.0 || span / step >= SMOOTH_ROWS {
        return step;
    }
    let mut finest = step;
    for split in [2.0, 5.0, 10.0, 20.0, 50.0, 100.0] {
        let fine = step / split;
        if fine < tick * 0.999 {
            break;
        }
        finest = fine;
        if span / fine >= SMOOTH_ROWS {
            break;
        }
    }
    finest
}

fn one(
    bars: &[Bar],
    offset: usize,
    start_ts: i64,
    grid: Grid,
    value_area: f64,
) -> Option<Profile> {
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    for bar in bars {
        low = low.min(bar.low);
        high = high.max(bar.high);
    }
    if !low.is_finite() || !high.is_finite() {
        return None;
    }

    let (low, step, rows) = match grid {
        Grid::Rows(rows) => {
            // A period that never moved still has a profile; give it somewhere
            // to live rather than dividing by a zero range.
            if (high - low).abs() < f64::EPSILON {
                high = low + 1.0;
            }
            (low, (high - low) / rows as f64, rows)
        }
        // Boundaries on multiples of the step, not on wherever the period's
        // low happened to fall. A row that runs from 182.35 to 182.40 is one
        // you can find on the axis; the same row starting at 182.3617 is not.
        Grid::Step { step, tick } => {
            let step = smoothed(step, tick, high - low);
            let base = (low / step).floor() * step;
            let rows = ((high - base) / step).ceil().max(1.0);
            let rows = if rows.is_finite() { rows as usize } else { MIN_ROWS };
            (base, step, rows.clamp(MIN_ROWS, MAX_ROWS))
        }
    };
    let mut volumes = vec![0.0f64; rows];
    // The rows a bar covers wholly all get the same share of it, so they are
    // written as one step up where they begin and one down after they end,
    // summed into place at the end: a bar costs the same however many rows
    // it spans, which at a few hundred rows a period is most of the work.
    let mut whole = vec![0.0f64; rows + 1];

    for bar in bars {
        let weight = if bar.volume > 0.0 { bar.volume } else { 1.0 };
        let span = (bar.high - bar.low).max(f64::EPSILON);
        let first = (((bar.low - low) / step).floor() as isize).clamp(0, rows as isize - 1) as usize;
        let last = (((bar.high - low) / step).floor() as isize).clamp(0, rows as isize - 1) as usize;

        if first == last {
            volumes[first] += weight;
            continue;
        }
        // Split across the rows the bar touches, by how much of it is in each:
        // the two it ends in by their overlap, the ones between in full.
        let share = |row: usize| {
            let row_low = low + row as f64 * step;
            let overlap = (bar.high.min(row_low + step) - bar.low.max(row_low)).max(0.0);
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
    finish(bars, offset, start_ts, low, step, volumes, value_area)
}

/// `step` rounded up to whole ticks of `tick`, and never under one.
fn on_ticks(step: f64, tick: f64) -> f64 {
    if tick.is_nan() || tick <= 0.0 || !step.is_finite() {
        return step;
    }
    // A hair under a whole number of ticks is that number: 0.5 / 0.25 read
    // as 2.0000000001 must not become three ticks.
    ((step / tick - 1e-9).ceil()).max(1.0) * tick
}

/// [`smoothed`] in whole ticks: a split only where it leaves rows a whole
/// number of ticks tall, so no row falls between two prices.
fn smoothed_ticks(step: f64, tick: f64, span: f64) -> f64 {
    let ticks = (step / tick).round().max(1.0) as u64;
    if span <= 0.0 || span / step >= SMOOTH_ROWS {
        return step;
    }
    let mut finest = ticks;
    for split in [2u64, 5, 10, 20, 50, 100] {
        if !ticks.is_multiple_of(split) {
            continue;
        }
        finest = ticks / split;
        if span / (finest as f64 * tick) >= SMOOTH_ROWS {
            break;
        }
    }
    finest as f64 * tick
}

/// [`one`] from what traded at each price: every price's volume — bid, ask
/// and with no known aggressor — in the row that holds it, nothing spread.
/// Rows are whole ticks tall and cover the period's bars and its ladder
/// both, so the price that traded at the high has a row of its own.
///
/// Volume a provider reported with no price at all is in no row.
fn one_exact(
    bars: &[Bar],
    offset: usize,
    start_ts: i64,
    grid: Grid,
    value_area: f64,
    ladder: &Ladder,
    tick: f64,
) -> Option<Profile> {
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    for bar in bars {
        low = low.min(bar.low);
        high = high.max(bar.high);
    }
    let top = ladder.high_tick()?;
    low = low.min(to_price(ladder.low_tick, tick));
    high = high.max(to_price(top, tick));
    if !low.is_finite() || !high.is_finite() || tick.is_nan() || tick <= 0.0 {
        return None;
    }
    let step = match grid {
        Grid::Rows(rows) => on_ticks((high - low) / rows as f64, tick),
        Grid::Step { step, .. } => smoothed_ticks(on_ticks(step, tick), tick, high - low),
    };
    // Rows of whole ticks start on a whole multiple of the step, and a
    // price on a boundary is the row above it.
    const HAIR: f64 = 1e-6;
    let base = ((low / step) + HAIR).floor() * step;
    let rows = (((high - base) / step + HAIR).floor() as isize + 1).max(1) as usize;
    let rows = rows.clamp(MIN_ROWS, MAX_ROWS);
    let mut volumes = vec![0.0f64; rows];
    for (at, level) in ladder.iter() {
        let volume = level.volume();
        if volume == 0 {
            continue;
        }
        let row = ((to_price(at, tick) - base) / step + HAIR).floor();
        let row = (row.max(0.0) as usize).min(rows - 1);
        volumes[row] += volume as f64;
    }
    finish(bars, offset, start_ts, base, step, volumes, value_area)
}

/// A profile from its rows' volumes: the point of control, the value area.
fn finish(
    bars: &[Bar],
    offset: usize,
    start_ts: i64,
    low: f64,
    step: f64,
    volumes: Vec<f64>,
    value_area: f64,
) -> Option<Profile> {
    let total: f64 = volumes.iter().sum();
    let poc = volumes
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)?;
    let max_volume = volumes[poc];
    Some(Profile {
        start_ts,
        parts: vec![start_ts],
        end_ts: bars.last()?.ts,
        first_bar: offset,
        last_bar: offset + bars.len() - 1,
        value_area: expand_value_area(&volumes, poc, total, value_area),
        rows: (0..volumes.len())
            .map(|i| ProfileRow {
                low: low + i as f64 * step,
                high: low + (i + 1) as f64 * step,
                volume: volumes[i],
            })
            .collect(),
        poc,
        max_volume,
        total_volume: total,
    })
}

/// Grow outward from the point of control, always taking the busier side,
/// until the rows cover `fraction` of the volume.
fn expand_value_area(
    volumes: &[f64],
    poc: usize,
    total: f64,
    fraction: f64,
) -> (usize, usize) {
    let target = total * fraction.clamp(0.0, 1.0);
    let (mut lower, mut upper) = (poc, poc);
    let mut covered = volumes[poc];

    while covered < target && (lower > 0 || upper + 1 < volumes.len()) {
        let below = if lower > 0 { Some(volumes[lower - 1]) } else { None };
        let above = if upper + 1 < volumes.len() { Some(volumes[upper + 1]) } else { None };
        match (below, above) {
            (Some(b), Some(a)) if b > a => {
                lower -= 1;
                covered += b;
            }
            (Some(_), Some(a)) => {
                upper += 1;
                covered += a;
            }
            (Some(b), None) => {
                lower -= 1;
                covered += b;
            }
            (None, Some(a)) => {
                upper += 1;
                covered += a;
            }
            (None, None) => break,
        }
    }
    (lower, upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;

    fn bar(ts: i64, low: f64, high: f64, volume: f64) -> Bar {
        Bar { ts, open: low, high, low, close: high, volume }
    }

    #[test]
    fn one_profile_per_period() {
        let bars = vec![
            bar(0, 10.0, 11.0, 100.0),
            bar(3600, 10.0, 11.0, 100.0),
            bar(DAY, 20.0, 21.0, 100.0),
            bar(DAY + 3600, 20.0, 21.0, 100.0),
        ];
        let profiles = compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[]);
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].first_bar, 0);
        assert_eq!(profiles[0].last_bar, 1);
        assert_eq!(profiles[1].first_bar, 2);
        assert_eq!(profiles[1].last_bar, 3);
    }

    #[test]
    fn the_point_of_control_is_where_volume_concentrated() {
        // Lots of trade at 10, a little at 20.
        let mut bars = vec![bar(0, 10.0, 10.1, 1000.0); 1];
        bars.push(bar(60, 20.0, 20.1, 1.0));
        let profile = &compute(&bars, Reset::Session, 0, Some(20), 0.7, None, &[])[0];
        assert!(profile.poc_price() < 11.0, "poc at {}", profile.poc_price());
    }

    #[test]
    fn volume_is_spread_across_the_rows_a_bar_touches() {
        // One bar spanning the whole range: every row gets a share, and they
        // sum back to the bar's volume.
        let bars = vec![bar(0, 0.0, 10.0, 100.0)];
        let profile = &compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[])[0];
        let total: f64 = profile.rows.iter().map(|r| r.volume).sum();
        assert!((total - 100.0).abs() < 1e-6, "{total}");
        assert!(profile.rows.iter().all(|r| r.volume > 0.0), "every row touched");
    }

    #[test]
    fn the_value_area_covers_the_requested_fraction() {
        // A peak with thin tails.
        let bars = vec![
            bar(0, 9.0, 9.1, 10.0),
            bar(60, 10.0, 10.1, 100.0),
            bar(120, 10.0, 10.1, 100.0),
            bar(180, 11.0, 11.1, 10.0),
        ];
        let profile = &compute(&bars, Reset::Session, 0, Some(24), 0.70, None, &[])[0];
        let (lo, hi) = profile.value_area;
        let covered: f64 = profile.rows[lo..=hi].iter().map(|r| r.volume).sum();
        assert!(covered >= profile.total_volume * 0.70 - 1e-6, "{covered}");
        assert!(lo <= profile.poc && profile.poc <= hi, "the value area contains the poc");
    }

    #[test]
    fn a_full_value_area_is_the_whole_profile() {
        let bars = vec![bar(0, 9.0, 11.0, 50.0), bar(60, 9.5, 10.5, 50.0)];
        let profile = &compute(&bars, Reset::Session, 0, Some(12), 1.0, None, &[])[0];
        assert_eq!(profile.value_area, (0, profile.rows.len() - 1));
    }

    #[test]
    fn value_area_bounds_bracket_the_poc_price() {
        let bars = vec![bar(0, 9.0, 11.0, 50.0), bar(60, 9.5, 10.5, 80.0)];
        let profile = &compute(&bars, Reset::Session, 0, Some(20), 0.7, None, &[])[0];
        let (low, high) = profile.value_area_bounds();
        let poc = profile.poc_price();
        assert!(low <= poc && poc <= high, "{low} <= {poc} <= {high}");
    }

    #[test]
    fn a_motionless_period_still_produces_a_profile() {
        let bars = vec![bar(0, 10.0, 10.0, 5.0), bar(60, 10.0, 10.0, 5.0)];
        let profiles = compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[]);
        assert_eq!(profiles.len(), 1);
        assert!(profiles[0].max_volume > 0.0);
        assert!(profiles[0].poc_price().is_finite());
    }

    #[test]
    fn a_volumeless_instrument_still_profiles() {
        let bars = vec![bar(0, 9.0, 10.0, 0.0), bar(60, 9.5, 10.5, 0.0)];
        let profile = &compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[])[0];
        assert!(profile.total_volume > 0.0, "bars count even without volume");
    }

    #[test]
    fn empty_input_produces_nothing() {
        assert!(compute(&[], Reset::Session, 0, Some(10), 0.7, None, &[]).is_empty());
    }

    #[test]
    fn a_merged_period_joins_the_one_before_it() {
        let bars: Vec<Bar> = (0..3)
            .flat_map(|day| [bar(day * DAY, 10.0 + day as f64, 11.0 + day as f64, 100.0), bar(day * DAY + 3600, 10.0, 11.0, 100.0)])
            .collect();
        let profiles = compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[DAY]);
        assert_eq!(profiles.len(), 2);
        assert_eq!((profiles[0].first_bar, profiles[0].last_bar), (0, 3));
        assert_eq!(profiles[0].parts, vec![0, DAY]);
        assert!(profiles[0].is_composite());
        assert!((profiles[0].total_volume - 400.0).abs() < 1e-6);
        assert!(!profiles[1].is_composite());

        // Merging the next one as well makes one composite of all three.
        let all = compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[DAY, 2 * DAY]);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].parts, vec![0, DAY, 2 * DAY]);
    }

    #[test]
    fn a_merge_with_nothing_before_it_on_the_chart_is_left_alone() {
        let bars = vec![bar(DAY, 10.0, 11.0, 100.0), bar(2 * DAY, 10.0, 11.0, 100.0)];
        let profiles = compute(&bars, Reset::Session, 0, Some(10), 0.7, None, &[DAY]);
        assert_eq!(profiles.len(), 2);
        assert_eq!(profiles[0].parts, vec![DAY]);
    }

    /// A session of one-minute bars wandering over `span` around `price`.
    fn session(day: i64, price: f64, span: f64) -> Vec<Bar> {
        (0..390)
            .map(|i| {
                let t = i as f64 / 389.0;
                let low = price + span * (t * std::f64::consts::TAU).sin() * 0.5;
                Bar {
                    ts: day * DAY + i * 60,
                    open: low,
                    high: low + span * 0.02,
                    low,
                    close: low,
                    volume: 1000.0,
                }
            })
            .collect()
    }

    /// Every number a profile holds, as one hash: `{:?}` of an `f64` is its
    /// shortest exact spelling, so two equal hashes are equal bits.
    fn fingerprint(profiles: &[Profile]) -> u64 {
        format!("{profiles:?}").bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
    }

    /// The estimate from bar ranges, held to what it computed before volume at
    /// price existed: a chart without ladders draws exactly what it drew.
    #[test]
    fn without_ladders_the_profile_is_what_it_always_was() {
        let mut bars = session(0, 333.0, 8.0);
        bars.extend(session(1, 333.0, 2.0));
        bars.extend(session(2, 340.0, 5.0));
        let cases = [
            compute(&bars, Reset::Session, 0, None, 0.7, Some(InstrumentKind::Equity), &[]),
            compute(&bars, Reset::Session, 0, Some(50), 0.68, None, &[]),
            compute(&bars, Reset::Week, 0, None, 0.7, Some(InstrumentKind::FutureRoot), &[]),
            compute(&bars, Reset::Session, 3_600, None, 0.7, Some(InstrumentKind::FutureRoot), &[DAY + 3_600]),
        ];
        let prints: Vec<u64> = cases.iter().map(|profiles| fingerprint(profiles)).collect();
        assert_eq!(prints, GOLDEN, "{prints:?}");
    }

    const GOLDEN: [u64; 4] = [17829156222254821403, 1918181280838795739, 7844774359875511761, 10693579781058083716];

    fn row_height(p: &Profile) -> f64 {
        p.rows[0].high - p.rows[0].low
    }

    #[test]
    fn automatic_rows_follow_the_price_increment_not_a_fixed_count() {
        // Apple: a session that travels about eight dollars around $333. Rows
        // of nine cents — which is what forty-eight of them gives — hide the
        // shelves the profile exists to show.
        let bars = session(0, 333.0, 8.0);
        let auto = &compute(&bars, Reset::Session, 0, None, 0.7, Some(InstrumentKind::Equity), &[])[0];
        let height = row_height(auto);
        assert!(
            (0.009..0.051).contains(&height),
            "a share should get rows of a few cents, got {height}"
        );
        // And a round number of cents, not 0.0417: the boundaries have to be
        // prices you can find on the axis.
        let cents = height * 100.0;
        assert!((cents - cents.round()).abs() < 1e-6, "{height} is not a round number of cents");
        assert!(auto.rows.len() > 100, "and enough of them to show a shelf: {}", auto.rows.len());
    }

    #[test]
    fn a_currency_major_gets_rows_a_pip_can_fit_in() {
        // EURUSD moves eighty pips in a session and is quoted to five places.
        // Cent-sized rows would put the whole day in one block.
        let bars = session(0, 1.1250, 0.0080);
        let auto = &compute(&bars, Reset::Session, 0, None, 0.7, Some(InstrumentKind::Fx), &[])[0];
        let height = row_height(auto);
        assert!(height < 0.00021, "a pair wants sub-pip rows, got {height}");
        assert!(height >= 0.00001, "and not finer than it is quoted, got {height}");
    }

    #[test]
    fn automatic_rows_are_the_same_height_across_periods() {
        // A busy day and a quiet one. The quiet day should be a shorter
        // profile, not the same profile stretched: comparing two sessions is
        // most of what the thing is for.
        let mut bars = session(0, 333.0, 8.0);
        bars.extend(session(1, 333.0, 2.0));
        let profiles = compute(&bars, Reset::Session, 0, None, 0.7, Some(InstrumentKind::Equity), &[]);
        assert_eq!(profiles.len(), 2);
        let (busy, quiet) = (&profiles[0], &profiles[1]);
        assert!(
            (row_height(busy) - row_height(quiet)).abs() < 1e-9,
            "{} vs {}",
            row_height(busy),
            row_height(quiet)
        );
        assert!(
            quiet.rows.len() < busy.rows.len(),
            "the quiet session should be shorter: {} vs {}",
            quiet.rows.len(),
            busy.rows.len()
        );
    }

    #[test]
    fn a_chosen_row_count_is_still_obeyed() {
        let bars = session(0, 333.0, 8.0);
        let fixed = &compute(&bars, Reset::Session, 0, Some(182), 0.7, Some(InstrumentKind::Equity), &[])[0];
        assert_eq!(fixed.rows.len(), 182);
    }

    #[test]
    fn row_counts_are_clamped_to_something_drawable() {
        let bars = vec![bar(0, 9.0, 11.0, 50.0)];
        assert_eq!(compute(&bars, Reset::Session, 0, Some(0), 0.7, None, &[])[0].rows.len(), 4);
        assert_eq!(compute(&bars, Reset::Session, 0, Some(10_000), 0.7, None, &[])[0].rows.len(), 800);
    }

    /// A quarter on a daily chart, on an instrument whose history is mostly
    /// much quieter than its present — which is every stock that has grown.
    ///
    /// The step is one step for the whole chart, chosen from the median
    /// period. The row count is capped per period. Together those two meant a
    /// period that moved far further than the median ran out of rows: it was
    /// drawn across eight dollars of its own hundred-and-fifty-dollar range,
    /// with everything above clamped into the top row, so every other row
    /// came out at a width of zero and the profile was invisible.
    #[test]
    fn a_period_that_moved_further_than_the_rest_is_still_drawn_whole() {
        let day = 86_400i64;
        let start = 1_735_689_600i64;
        let mut bars = Vec::new();
        // Years at twenty dollars, then a quarter that ranges over a hundred.
        for i in 0..1600i64 {
            let p = 20.0 + ((i % 11) as f64 * 0.4);
            bars.push(Bar {
                ts: start + i * day,
                open: p,
                high: p + 0.5,
                low: p - 0.5,
                close: p,
                volume: 5_000_000.0,
            });
        }
        for i in 1600..1780i64 {
            let p = 520.0 + ((i - 1600) as f64 * 1.4) + ((i % 7) as f64 * 6.0);
            bars.push(Bar {
                ts: start + i * day,
                open: p,
                high: p + 8.0,
                low: p - 8.0,
                close: p + 1.0,
                volume: 20_000_000.0,
            });
        }

        let profiles = compute(&bars, Reset::Quarter, 0, None, 0.7, None, &[]);
        let last = profiles.last().expect("a quarter to draw");
        let bars_in = &bars[last.first_bar..=last.last_bar];
        let low = bars_in.iter().fold(f64::MAX, |a, b| a.min(b.low));
        let high = bars_in.iter().fold(f64::MIN, |a, b| a.max(b.high));

        let covered = last.rows.last().unwrap().high - last.rows.first().unwrap().low;
        assert!(
            covered >= (high - low) * 0.99,
            "the profile covers {covered:.2} of a {:.2} range",
            high - low
        );

        // And it is a profile rather than one block: the rows that are not the
        // point of control still have a width worth drawing.
        let visible = last
            .rows
            .iter()
            .filter(|r| r.volume / last.max_volume >= 0.01)
            .count();
        assert!(visible > 10, "only {visible} rows would be drawn");
    }
}
