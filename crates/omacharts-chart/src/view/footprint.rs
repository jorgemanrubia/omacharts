//! Footprints, drawn.
//!
//! Zoomed in past the bar width its settings say, a footprint takes the
//! candles' place: a narrow candle at each bar's left, and beside it a row
//! for each price the bar traded at, as long as what was bought there less
//! what was sold, in buying's colour or selling's, with the figure written
//! in it at its left where the row is tall enough to read.
//!
//! **Rows.** A row holds so many ticks as its settings say, or
//! automatically a compact line's worth for the price scale
//! ([`rows::auto_ticks`]): worked out again only when the scale changes,
//! and then kept while it still makes a comfortable row, so scrolling never
//! makes the rows jump. Rows sit on the same grid on every bar, and are
//! never drawn taller than a line of small type ([`ROW_MAX`]), centred on
//! their prices, however far the chart is zoomed in.
//!
//! **Smooth.** A footprint is redrawn on every frame of a scroll and every
//! tick, so each bar's ladder is kept once read ([`Cache`]), keyed by the
//! bar's volume: a tick changes the forming bar's volume, and only that bar
//! is read again. Only the bars on screen are read and only their rows
//! drawn. Nothing is allocated per frame once the bars on screen are kept:
//! figures are written into a buffer of bytes and drawn as glyphs looked up
//! once, and each colour's rows are filled as one path.
//!
//! **Numbers.** With them off, only the rows and candles are drawn: no
//! figure is placed, no font is set and no glyph is looked up.
//!
//! **Strength.** With intensity on, a row is filled more solidly the bigger
//! its delta against the biggest on screen — the same reference the rows'
//! lengths are measured against, so a row's length and its strength say the
//! same thing — in [`LEVELS`] steps, from faint to the opacity chosen, which
//! the strongest row is painted at exactly. Measured against each bar's
//! own biggest instead, a quiet bar would shout as loud as a busy one; and
//! against the session's, one spike would leave every other row faint.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_ulong;

use gtk::cairo;
use omacharts_engine::indicators::flow;
use omacharts_engine::ladders::{Held, Ladder};
use omacharts_engine::Params;
use omacharts_engine::studies::footprint as rows;

use super::{Columns, Drawn, State};
use crate::colors;

/// How many strengths a row is filled at: few, so the source is set once a
/// strength rather than once a row.
pub(super) const LEVELS: usize = 5;
/// The weakest row's opacity, against the strongest's.
const FAINT: f64 = 0.16;

/// The space between a number and a row's edge.
const PAD: f64 = 4.0;
/// Where a number starts in its row: at the row's left, just inside.
const INSIDE: f64 = 3.0;
/// The least a row is drawn tall, in pixels: thinner, ticks fold together.
const ROW_MIN: f64 = 3.0;
/// The least a row is tall for its figure to be written in it.
const FIGURE_MIN: f64 = 11.0;
/// The tallest a row is drawn, in pixels, however tall its ticks are: a line
/// of small type, centred on the row's prices, so a tick zoomed in a long
/// way is still a row and never a block.
const ROW_MAX: f64 = 15.0;
/// The figures' type size, in pixels, at its largest.
const FIGURE_SIZE: f64 = 11.0;
/// What a figure is written with: [`rows::short`]'s characters.
const CHARS: &[u8; 14] = b"0123456789-.KM";
const FACE: &str = "monospace";

thread_local! {
    /// The glyph of each of [`CHARS`] in [`FACE`], looked up once.
    static GLYPHS: Cell<Option<[c_ulong; 14]>> = const { Cell::new(None) };
    /// [`FACE`] made once: selecting it by name copies the name every time.
    static FONT: std::cell::OnceCell<Option<cairo::FontFace>> = const { std::cell::OnceCell::new() };
}

/// Write in [`FACE`], without a copy of its name a frame.
fn use_face(cr: &cairo::Context) {
    FONT.with(|font| {
        match font.get_or_init(|| cairo::FontFace::toy_create(FACE, cairo::FontSlant::Normal, cairo::FontWeight::Normal).ok()) {
            Some(face) => cr.set_font_face(face),
            None => cr.select_font_face(FACE, cairo::FontSlant::Normal, cairo::FontWeight::Normal),
        }
    });
}

/// One bar's footprint as the chart keeps it to draw: each tick's delta
/// and total, folded into rows by the fold it is read out by too.
#[derive(Default, Debug)]
struct Cells {
    footprint: rows::BarFootprint<rows::Tick>,
    /// The bar's volume when it was read, by its bits: a bar that has
    /// traded since is read again.
    volume: u64,
}

/// What a chart's footprint has read, by bar.
#[derive(Default, Debug)]
pub(super) struct Cache {
    /// The ladders' generation, the tick size and the bar length it was
    /// read for: any change and nothing kept holds.
    key: Option<(u64, u64, i64)>,
    bars: HashMap<i64, Cells>,
    /// The forming minute when last read, by its open and volume: a change
    /// reads the newest bars again.
    forming: Option<(i64, u64)>,
    /// A ladder summed into, kept for its allocation.
    scratch: Ladder,
    /// Kept bars let go, kept for theirs.
    spare: Vec<Cells>,
    /// The ticks a row held when last drawn, and the height of a tick it
    /// was worked out for: worked out again only when the price scale
    /// changes, and then held while it still makes a comfortable row.
    step: Option<(u32, u64)>,
}

impl Cache {
    /// The ticks a row holds: so many as chosen, or automatically a compact
    /// line's worth, held from frame to frame while it still is one.
    fn step_for(&mut self, chosen: Option<u32>, tick_px: f64) -> u32 {
        match chosen {
            Some(chosen) => rows::fixed_ticks(chosen, tick_px, ROW_MIN),
            None => {
                let scale = tick_px.to_bits();
                match self.step {
                    Some((step, at)) if at == scale => step,
                    held => {
                        let step = rows::auto_ticks(tick_px, held.map(|(step, _)| step));
                        self.step = Some((step, scale));
                        step
                    }
                }
            }
        }
    }

    /// The ticks a row held when last drawn automatically.
    pub(super) fn auto_step(&self) -> Option<u32> {
        self.step.map(|(step, _)| step)
    }

    fn reset(&mut self) {
        self.spare.extend(self.bars.drain().map(|(_, cells)| cells));
    }

    /// Read every bar on screen not already kept as it stands.
    fn fill(&mut self, state: &State, held: &Held, columns: &Columns) {
        let tf = state.timeframe;
        let key = (held.generation, held.tick_size.to_bits(), tf.seconds());
        if self.key != Some(key) {
            self.reset();
            self.key = Some(key);
        }
        let forming = held.forming.as_ref().map(|m| (m.ts, m.ladder.total()));
        if forming != self.forming {
            // The minute forming is in the newest bar, or the one before at
            // a boundary; a minute sealed with it may be too.
            if let Some(last) = state.bars.last() {
                let from = last.ts - tf.seconds();
                let spare = &mut self.spare;
                self.bars.retain(|ts, cells| {
                    let keep = *ts < from;
                    if !keep {
                        spare.push(std::mem::take(cells));
                    }
                    keep
                });
            }
            self.forming = forming;
        }
        let days = bar_days(state);
        for bar in columns.slice {
            let volume = bar.volume.to_bits();
            if self.bars.get(&bar.ts).is_some_and(|kept| kept.volume == volume) {
                continue;
            }
            if !bar_ladder(held, bar.ts, tf, days.as_ref(), &mut self.scratch) {
                continue;
            }
            let mut cells = self.bars.remove(&bar.ts).or_else(|| self.spare.pop()).unwrap_or_default();
            cells.volume = volume;
            let ticks = self.scratch.levels.iter().map(|level| rows::Tick::of(level.at_bid, level.at_ask, level.unknown));
            cells.footprint.refill(self.scratch.low_tick, ticks);
            self.bars.insert(bar.ts, cells);
        }
        // Scrolled a long way, the bars far off screen go.
        let room = columns.slice.len() * 4 + 512;
        if self.bars.len() > room
            && let (Some(first), Some(last)) = (columns.slice.first(), columns.slice.last())
        {
            let reach = (last.ts - first.ts).max(tf.seconds());
            let (from, to) = (first.ts - reach, last.ts + reach);
            let spare = &mut self.spare;
            self.bars.retain(|ts, cells| {
                let keep = (from..=to).contains(ts);
                if !keep {
                    spare.push(std::mem::take(cells));
                }
                keep
            });
        }
        self.spare.truncate(64);
    }
}

/// The days a bar longer than a day is summed by: none for an intraday
/// bar, which is summed by its minutes.
fn bar_days(state: &State) -> Option<omacharts_engine::ladders::Days> {
    match state.timeframe.is_intraday() {
        true => None,
        false => state.instrument.as_ref().map(omacharts_engine::ladders::Days::new),
    }
}

/// Sum the bar opening at `ts` into `into`: the minutes in it, or a bar of
/// days its days. False where `held` cannot answer it whole. What the
/// footprint draws a bar from and what it is read out by, both.
fn bar_ladder(held: &Held, ts: i64, tf: omacharts_engine::Timeframe, days: Option<&omacharts_engine::ladders::Days>, into: &mut Ladder) -> bool {
    into.clear();
    match days {
        None => held.visit_between(ts, ts + tf.seconds(), |part| into.add_ladder(part)),
        Some(days) => {
            let end = tf.shift(ts, 1);
            match held.days_between(ts.div_euclid(86_400), end.div_euclid(86_400), days) {
                Some(ladder) => {
                    *into = ladder;
                    true
                }
                None => false,
            }
        }
    }
}

/// Which bars a footprint is read out for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPick {
    /// The bars on screen.
    Visible,
    /// The last so many, the one forming among them.
    Last(usize),
    /// The bars opening from the first time up to, not including, the second.
    Between(i64, i64),
}

/// One row of a bar read out: its lowest tick and each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FootprintRow {
    pub low_tick: i32,
    pub sides: rows::Sides,
}

/// One bar read out: the bar, its rows lowest first, the low tick of its
/// most traded row, and everything in it as one.
#[derive(Clone, Debug, PartialEq)]
pub struct FootprintBar {
    pub bar: omacharts_engine::Bar,
    pub rows: Vec<FootprintRow>,
    pub poc: Option<i32>,
    pub totals: rows::Sides,
}

/// A chart's footprint read out: the tick its rows are counted in, how many
/// ticks a row holds, and the bars.
#[derive(Clone, Debug, PartialEq)]
pub struct FootprintRead {
    pub tick_size: f64,
    pub row_ticks: u32,
    pub bars: Vec<FootprintBar>,
}

/// The ticks a row holds read out, unless told: the footprint's own, or the
/// automatic step it drew last, or one tick where it has drawn none.
fn read_step(state: &State) -> u32 {
    let own = state.indicators.iter().find_map(|drawn| match drawn.indicator.params {
        Params::Footprint { row_ticks, .. } => Some(row_ticks),
        _ => None,
    });
    match own {
        Some(Some(ticks)) => ticks.max(1),
        _ => state.footprint.borrow().auto_step().unwrap_or(1),
    }
}

/// `pick`'s bars of `state` read out as its footprint draws them, rows of
/// `row_ticks` or [`read_step`]'s: the same ladders, summed by the same
/// [`bar_ladder`] and folded by the same [`rows::fold`]. None where the
/// chart has nothing to read them from.
pub(super) fn read_out(state: &State, pick: BarPick, row_ticks: Option<u32>) -> Option<FootprintRead> {
    let read = state.volume_at_price.as_ref()?;
    let bars = &state.bars;
    let range = match pick {
        BarPick::Visible => {
            let (first, visible) = state.slice();
            first..(first + visible).min(bars.len())
        }
        BarPick::Last(n) => bars.len().saturating_sub(n)..bars.len(),
        BarPick::Between(from, to) => bars.partition_point(|bar| bar.ts < from)..bars.partition_point(|bar| bar.ts < to),
    };
    let first = bars.first()?.ts;
    let step = row_ticks.unwrap_or_else(|| read_step(state)).clamp(1, rows::MAX_ROW_TICKS);
    let tf = state.timeframe;
    let days = bar_days(state);
    let mut out = None;
    read(first, &mut |held, _| {
        let Some(held) = held else { return };
        let mut ladder = Ladder::default();
        let mut footprint: rows::BarFootprint<rows::Sides> = rows::BarFootprint::new();
        let mut read_bars = Vec::with_capacity(range.len());
        for bar in &bars[range.clone()] {
            let mut rows_out = Vec::new();
            let (mut poc, mut totals) = (None, rows::Sides::default());
            if bar_ladder(held, bar.ts, tf, days.as_ref(), &mut ladder) {
                let sides = ladder.levels.iter().map(|level| rows::Sides {
                    at_bid: level.at_bid,
                    at_ask: level.at_ask,
                    unknown: level.unknown,
                    trades: u64::from(level.bid_trades) + u64::from(level.ask_trades),
                });
                footprint.refill(ladder.low_tick, sides);
                footprint.each_row(step, |low_tick, sides| rows_out.push(FootprintRow { low_tick, sides }));
                poc = footprint.point_of_control(step, rows::Sides::total);
                totals = footprint.totals();
            }
            read_bars.push(FootprintBar { bar: *bar, rows: rows_out, poc, totals });
        }
        out = Some(FootprintRead { tick_size: held.tick_size, row_ticks: step, bars: read_bars });
    });
    out
}

/// The footprint a chart draws in place of its bars at this zoom: the first
/// one shown, once a column holds the narrow bar and a row that reads.
///
/// A row reads when the widest figure fits in it, or, with the numbers off,
/// when it is a short bar. It comes at that width and goes [`HYSTERESIS`]
/// pixels under it, so zooming across the edge never flickers.
pub(super) fn shown<'a>(state: &'a State, columns: &Columns) -> Option<&'a Drawn> {
    let found = match columns.sub_pixel() || state.volume_at_price.is_none() {
        true => None,
        false => state.shown_indicators().find(|drawn| {
            matches!(drawn.indicator.params, Params::Footprint { .. })
                && matches!(drawn.output, omacharts_engine::Output::Footprint)
        }),
    };
    let Some(drawn) = found else {
        state.footprint_on.set(false);
        return None;
    };
    let Params::Footprint { numbers, profile, .. } = &drawn.indicator.params else { return None };
    let profile = profile.on.then_some(profile.numbers);
    let on = fits(columns.bar_px, bar_drawn(state), *numbers, profile, state.footprint_on.get());
    state.footprint_on.set(on);
    on.then_some(drawn)
}

/// Do a footprint's rows fit a column `bar_px` wide, the narrow bar
/// beside them or not, with their figures or not, and its profile's rows
/// too where it has one on (`profile`, with its figures or not)? A little
/// less room will do for one shown already.
fn fits(bar_px: f64, with_bar: bool, numbers: bool, profile: Option<bool>, shown: bool) -> bool {
    let slack = if shown { HYSTERESIS } else { 0.0 };
    let needed = match profile {
        None => room_needed(numbers),
        Some(profile_numbers) => room_needed(numbers).max(room_needed(profile_numbers)),
    };
    rows_room(bar_px, with_bar, profile.is_some()) >= needed - slack
}

/// How many pixels under the width it comes at a footprint goes.
const HYSTERESIS: f64 = 4.0;
/// The room a row needs with its numbers off: a bar long enough to tell
/// a big delta from a small one.
const BARS_ONLY: f64 = 12.0;
/// The widest figure written: [`rows::short`] makes nothing longer than
/// six characters, such as `-12.5K`, for any delta a row of a contract
/// trades.
const WIDEST_FIGURE: &str = "-88.8K";

thread_local! {
    /// [`WIDEST_FIGURE`]'s width in [`FACE`] at [`FIGURE_SIZE`], measured
    /// once: the face and the size never change, and a width in logical
    /// pixels does not change with the display's scale.
    static WIDEST: Cell<Option<f64>> = const { Cell::new(None) };
}

/// The room a row needs: the widest figure inside it, or with the numbers
/// off, a short bar.
fn room_needed(numbers: bool) -> f64 {
    match numbers {
        true => widest() + 2.0 * INSIDE,
        false => BARS_ONLY,
    }
}

fn widest() -> f64 {
    if let Some(known) = WIDEST.get() {
        return known;
    }
    let measured = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1)
        .ok()
        .and_then(|surface| cairo::Context::new(&surface).ok())
        .and_then(|cr| {
            use_face(&cr);
            cr.set_font_size(FIGURE_SIZE);
            cr.text_extents(WIDEST_FIGURE).ok().map(|extents| extents.x_advance())
        })
        .filter(|width| *width > 0.0)
        .unwrap_or(FIGURE_SIZE * 0.6 * WIDEST_FIGURE.len() as f64);
    WIDEST.set(Some(measured));
    measured
}

/// The room a column `bar_px` wide leaves its rows: past the narrow bar
/// and the gaps round it, or with no bar, all but the gaps; with a profile,
/// each side's half of that, less the gap between them.
fn rows_room(bar_px: f64, with_bar: bool, profile: bool) -> f64 {
    let (gap, body_w) = narrow(bar_px);
    let room = match with_bar {
        true => bar_px - 3.0 * gap - body_w,
        false => bar_px - 2.0 * gap,
    };
    match profile {
        false => room,
        true => profile_half(room - gap),
    }
}

/// A profile's width, out of the `room` it and the delta rows share: half,
/// on whole pixels.
fn profile_half(room: f64) -> f64 {
    (room / 2.0).floor().max(0.0)
}

/// A colour of an order flow indicator's, as chosen, borrowed where it can
/// be: a fixed colour or a swatch of the theme's.
pub(super) fn chosen<'a>(choice: &'a omacharts_engine::theme::ColorChoice, state: &'a State) -> Cow<'a, str> {
    use omacharts_engine::theme::ColorChoice;
    match choice {
        ColorChoice::Fixed { hex } => Cow::Borrowed(hex),
        ColorChoice::Swatch { name } => match state.theme.swatches.iter().find(|s| &s.name == name) {
            Some(swatch) => Cow::Borrowed(&swatch.hex),
            None => Cow::Owned(choice.resolve(&state.theme)),
        },
    }
}

/// A footprint profile's colour: its own, or the theme's swatch for it,
/// borrowed, so a frame allocates nothing for it.
fn profile_colour<'a>(profile: &'a flow::BarProfile, state: &'a State) -> Cow<'a, str> {
    match &profile.colour {
        Some(choice) => chosen(choice, state),
        None => match state.theme.swatches.iter().find(|s| s.name == flow::PROFILE_SWATCH) {
            Some(swatch) => Cow::Borrowed(&swatch.hex),
            None => Cow::Owned(profile.chosen().resolve(&state.theme)),
        },
    }
}

/// Buying's and selling's colours for `inks`: its colouring's, or while it
/// is Custom, its own.
pub(super) fn inks<'a>(state: &'a State, inks: &'a flow::Inks) -> (Cow<'a, str>, Cow<'a, str>) {
    state.study_pair(inks.colouring, [inks.buy.as_ref(), inks.sell.as_ref()])
}

/// Draw `drawn`'s footprint over the columns: falls back to the candles
/// where there is nothing to read it from.
pub(super) fn draw(cr: &cairo::Context, state: &State, drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64) {
    let Params::Footprint { intensity, numbers, row_ticks, inks: chosen_inks, profile } = &drawn.indicator.params else {
        return;
    };
    let profile = profile.on.then_some(profile);
    let Some(read) = state.volume_at_price.as_ref() else { return };
    let Some(first) = state.bars.first() else { return };
    let mut drew = false;
    read(first.ts, &mut |held, _| {
        let Some(held) = held else { return };
        let mut cache = state.footprint.borrow_mut();
        cache.fill(state, held, columns);
        let tick_px = (to_y(0.0) - to_y(held.tick_size)).abs();
        let step = cache.step_for(*row_ticks, tick_px);
        paint(cr, state, &cache, held.tick_size, step, (*intensity, *numbers), chosen_inks, profile, columns, to_y);
        drew = true;
    });
    if !drew && bar_drawn(state) {
        super::candles(cr, state.bar_style, state.ohlc_thickness, &state.scheme, columns, to_y);
    }
}

/// Where a bar's parts go across its column.
#[derive(Clone, Copy)]
struct Across {
    /// Where the rows start and how far they may reach.
    rows: (f64, f64),
    /// Where its profile's rows may start and how far they reach, ending
    /// against the narrow bar: nothing wide without one.
    profile: (f64, f64),
}

/// The space either side of the narrow bar, and its body's width: an odd
/// number of pixels, so its wick has a middle.
fn narrow(bar_px: f64) -> (f64, f64) {
    let gap = (bar_px * 0.06).clamp(2.0, 6.0).round();
    let body_w = ((bar_px * 0.1).clamp(3.0, 9.0) / 2.0).floor() * 2.0 + 1.0;
    (gap, body_w)
}

/// Across column `at`: the rows start past the narrow bar, or at the
/// column's left where the chart draws no bar. With a profile, it takes
/// the column's left, as wide as the delta rows may be, and the narrow bar
/// and the rows move right past it.
fn across(columns: &Columns, at: usize, bar: bool, profile: bool) -> Across {
    let bar_px = columns.bar_px;
    let left = columns.x(at) - bar_px / 2.0;
    let (gap, body_w) = narrow(bar_px);
    let start = (left + gap).round();
    let (profile, start) = match profile {
        false => ((start, 0.0), start),
        true => {
            let width = rows_room(bar_px, bar, true);
            ((start, width), start + width + gap)
        }
    };
    let rows_x = match bar {
        true => start + body_w + gap,
        false => start,
    };
    let rows_w = (left + bar_px - gap - rows_x).max(0.0);
    Across { rows: (rows_x, rows_w), profile }
}

/// Whether the chart draws a bar at all, which a footprint then draws
/// narrow: not for a line, nor with its bars hidden.
fn bar_drawn(state: &State) -> bool {
    !state.bars_hidden && state.bar_style != omacharts_engine::bars::BarStyle::Line
}

/// The chart's own bars, in its own style and colours, narrow at each
/// column's left: the routine that draws them across a whole column, handed
/// the columns narrowed.
fn narrow_bars(cr: &cairo::Context, state: &State, columns: &Columns, profile: bool, to_y: &impl Fn(f64) -> f64) {
    let (gap, body_w) = narrow(columns.bar_px);
    // The face a body of `body_w` is drawn from, and its middle's offset
    // from the column's: past the profile, where there is one.
    let face = body_w / 0.68;
    let past = match profile {
        false => 0.0,
        true => rows_room(columns.bar_px, true, true) + gap,
    };
    let nudge = past + gap + body_w / 2.0 - columns.bar_px / 2.0;
    let narrowed = columns.narrowed(face, nudge);
    super::candles(cr, state.bar_style, state.ohlc_thickness, &state.scheme, &narrowed, to_y);
}

#[allow(clippy::too_many_arguments)]
fn paint(
    cr: &cairo::Context,
    state: &State,
    cache: &Cache,
    tick_size: f64,
    step: u32,
    (intensity, numbers): (bool, bool),
    chosen_inks: &flow::Inks,
    profile: Option<&flow::BarProfile>,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let bars = columns.slice;
    let with_bar = bar_drawn(state);
    let profiled = profile.is_some();
    let tick_px = (to_y(0.0) - to_y(tick_size)).abs();
    // A row's pitch, and the height drawn in it: a gap of a pixel or two
    // between rows, and never taller than a line of small type.
    let pitch = tick_px * f64::from(step);
    let gap = if pitch >= 8.0 {
        2.0
    } else if pitch >= 4.0 {
        1.0
    } else {
        0.0
    };
    let row_h = (pitch - gap).clamp(1.0, ROW_MAX).round();
    // Each row centred on the middle of its prices, on whole pixels.
    let row_y = |low: i32| {
        let middle = to_y((f64::from(low) + (f64::from(step) - 1.0) / 2.0) * tick_size);
        let top = (middle - row_h / 2.0).round();
        (top, top + row_h)
    };

    // The biggest row on screen, which every row's length and strength is
    // measured against; and the most traded, which every profile row is,
    // in the same pass.
    let mut on_screen = rows::Largest::default();
    for bar in bars {
        if let Some(cells) = cache.bars.get(&bar.ts) {
            cells.footprint.each_row(step, |_, tick| on_screen.take(tick));
        }
    }
    let (largest, most) = (on_screen.delta, on_screen.total);

    let (buy, sell) = inks(state, chosen_inks);
    let (buy_alpha, sell_alpha) = chosen_inks.alphas(flow::FOOTPRINT_ALPHA);
    let levels = if intensity { LEVELS } else { 1 };
    // Each side's faintest and strongest. In Monochrome both sides are the
    // one ink at the one opacity: a row's strength says how big its delta
    // is, never which way.
    let span = |buying: bool| {
        let full = if buying { buy_alpha } else { sell_alpha };
        (FAINT * full, full)
    };
    let alpha_of = |delta: i64| {
        let (faint, strong) = span(delta > 0);
        match intensity {
            true => rows::step_alpha(rows::strength(delta, largest, levels), levels, faint, strong),
            false => strong,
        }
    };
    // A row is drawn at least as long as its figure, so the figure sits
    // inside its box however small the delta: a box round each number, the
    // bar running on past it as the delta is big.
    let advance = figure_size(row_h) * 0.6;
    let fitted = |value: i64, room: f64, largest: u64, written: bool| {
        let long = rows::length(value, room, largest);
        if !written {
            return long;
        }
        let mut buf = [0u8; 12];
        let text = rows::short(value, &mut buf).len() as f64 * advance + 2.0 * INSIDE;
        long.max(text.min(room)).round()
    };
    let written = numbers && row_h >= FIGURE_MIN;
    let length = |delta: i64, room: f64| fitted(delta, room, largest, written);
    // A profile's rows, measured as the delta rows are, against the most
    // traded on screen, and written as they are.
    let profile_written = profile.is_some_and(|p| p.numbers) && row_h >= FIGURE_MIN;
    let profile_length = |total: u64, room: f64| fitted(total as i64, room, most, profile_written);

    cr.save().ok();
    // Each side's rows, a strength at a time, each strength one path.
    for (buying, ink) in [(true, &buy), (false, &sell)] {
        let (faint, strong) = span(buying);
        for level in 1..=levels {
            let mut any = false;
            for (at, bar) in bars.iter().enumerate() {
                let Some(cells) = cache.bars.get(&bar.ts) else { continue };
                let (x, room) = across(columns, at, with_bar, profiled).rows;
                cells.footprint.each_row(step, |low, tick| {
                    let delta = tick.delta;
                    if delta == 0 || (delta > 0) != buying || rows::strength(delta, largest, levels) != level {
                        return;
                    }
                    let (top, bottom) = row_y(low);
                    cr.rectangle(x, top, length(delta, room), bottom - top);
                    any = true;
                });
            }
            if any {
                let alpha = match intensity {
                    true => rows::step_alpha(level, levels, faint, strong),
                    false => strong,
                };
                colors::set_source_alpha(cr, ink, alpha);
                let _ = cr.fill();
            }
        }
        // The outlines, in the same hue, so a faint row still has an edge.
        let mut any = false;
        for (at, bar) in bars.iter().enumerate() {
            let Some(cells) = cache.bars.get(&bar.ts) else { continue };
            let (x, room) = across(columns, at, with_bar, profiled).rows;
            cells.footprint.each_row(step, |low, tick| {
                let delta = tick.delta;
                if delta == 0 || (delta > 0) != buying {
                    return;
                }
                let (top, bottom) = row_y(low);
                let h = bottom - top;
                if h < 3.0 {
                    return;
                }
                cr.rectangle(x + 0.5, top + 0.5, (length(delta, room) - 1.0).max(0.0), h - 1.0);
                any = true;
            });
        }
        if any {
            cr.set_line_width(1.0);
            colors::set_source_alpha(cr, ink, strong.max(0.35));
            let _ = cr.stroke();
        }
    }

    // The profile's rows, in its one colour, each ending against the bar:
    // a strength at a time, as the delta's, then their outlines.
    let profile_ink = profile.map(|p| (profile_colour(p, state), p.drawn_alpha()));
    if let Some((ink, full)) = &profile_ink {
        let faint = FAINT * full;
        for level in 1..=levels {
            let mut any = false;
            for (at, bar) in bars.iter().enumerate() {
                let Some(cells) = cache.bars.get(&bar.ts) else { continue };
                let (x, room) = across(columns, at, with_bar, true).profile;
                cells.footprint.each_row(step, |low, tick| {
                    if rows::strength(tick.total as i64, most, levels) != level {
                        return;
                    }
                    let (top, bottom) = row_y(low);
                    let long = profile_length(tick.total, room);
                    cr.rectangle(x + room - long, top, long, bottom - top);
                    any = true;
                });
            }
            if any {
                let alpha = match intensity {
                    true => rows::step_alpha(level, levels, faint, *full),
                    false => *full,
                };
                colors::set_source_alpha(cr, ink, alpha);
                let _ = cr.fill();
            }
        }
        let mut any = false;
        for (at, bar) in bars.iter().enumerate() {
            let Some(cells) = cache.bars.get(&bar.ts) else { continue };
            let (x, room) = across(columns, at, with_bar, true).profile;
            cells.footprint.each_row(step, |low, tick| {
                let (top, bottom) = row_y(low);
                let h = bottom - top;
                if h < 3.0 {
                    return;
                }
                let long = profile_length(tick.total, room);
                cr.rectangle(x + room - long + 0.5, top + 0.5, (long - 1.0).max(0.0), h - 1.0);
                any = true;
            });
        }
        if any {
            cr.set_line_width(1.0);
            colors::set_source_alpha(cr, ink, full.max(0.35));
            let _ = cr.stroke();
        }
    }

    // The chart's own bars, narrow, in its own style and colours: the
    // footprint's colouring is the rows' alone.
    if with_bar {
        narrow_bars(cr, state, columns, profiled, to_y);
    }

    // The profile's figures, each at its row's end against the bar, dark
    // on a row solid enough to want it, as the delta's.
    if let Some((ink, full)) = &profile_ink
        && profile_written
    {
        use omacharts_engine::theme::luminance;
        let ground = luminance(&state.theme.ui.background);
        let text = luminance(&state.theme.ui.text);
        let light = luminance(ink);
        let alpha_of = |total: u64| match intensity {
            true => rows::step_alpha(rows::strength(total as i64, most, levels), levels, FAINT * full, *full),
            false => *full,
        };
        let place = |total: i64, room: f64, text_px: f64| {
            let fill = ground + (light - ground) * alpha_of(total as u64);
            let dark = (fill - ground).abs() > (fill - text).abs();
            (room - INSIDE - text_px, dark && profile_length(total as u64, room) >= text_px + INSIDE)
        };
        figures(cr, state, cache, (step, with_bar), Lane::Profile, columns, &row_y, &place);
    }

    // The figures, where the rows are tall enough to read them, unless
    // they are turned off: then no text is laid out or drawn at all.
    if numbers && row_h >= FIGURE_MIN {
        // A number goes dark where it sits wholly on a row whose fill, laid
        // over the background, is nearer the text's lightness than the
        // background's: there a light number would fade into it. Lightness
        // is a sum of the channels, so a fill's is the blend of the two.
        use omacharts_engine::theme::luminance;
        let ground = luminance(&state.theme.ui.background);
        let text = luminance(&state.theme.ui.text);
        let (buy_light, sell_light) = (luminance(&buy), luminance(&sell));
        // Where a number goes along its row, and whether it is dark: always
        // inside at the row's left, on the bar, so the figures make a column
        // down the bar; dark only where the row runs on past all of it.
        let place = |delta: i64, room: f64, text_px: f64| {
            let long = length(delta, room);
            let light = if delta > 0 { buy_light } else { sell_light };
            let fill = ground + (light - ground) * alpha_of(delta);
            let dark = (fill - ground).abs() > (fill - text).abs();
            (INSIDE, dark && long >= text_px + INSIDE)
        };
        figures(cr, state, cache, (step, with_bar), Lane::Delta { profiled }, columns, &row_y, &place);
    }
    cr.restore().ok();
}

/// Which of a bar's rows a figure is written in.
#[derive(Clone, Copy)]
enum Lane {
    /// The delta rows, past the bar: past a profile too, if there is one.
    Delta { profiled: bool },
    /// The profile's rows, before the bar, each its total.
    Profile,
}

/// Each row's figure in `lane` written where `place` puts it, in one face,
/// as glyphs drawn a batch at a time.
#[allow(clippy::too_many_arguments)]
fn figures(
    cr: &cairo::Context,
    state: &State,
    cache: &Cache,
    (step, with_bar): (u32, bool),
    lane: Lane,
    columns: &Columns,
    row_y: &impl Fn(i32) -> (f64, f64),
    place: &impl Fn(i64, f64, f64) -> (f64, bool),
) {
    const BATCH: usize = 256;
    let row_h = row_y(0).1 - row_y(0).0;
    let size = figure_size(row_h);
    use_face(cr);
    cr.set_font_size(size);
    let Some(table) = glyphs(cr) else { return };
    let advance = cr
        .scaled_font()
        .glyph_extents(&[cairo::Glyph::new(table[0], 0.0, 0.0)])
        .x_advance();
    let advance = if advance > 0.0 { advance } else { size * 0.6 };
    let mut batch = [cairo::Glyph::new(0, 0.0, 0.0); BATCH];
    let mut buf = [0u8; 12];
    // Light numbers first, then the dark ones on solid rows: one colour,
    // and a batch of glyphs, at a time.
    for (dark, ink, alpha) in [(false, &state.theme.ui.text, 0.92), (true, &state.theme.ui.background, 0.95)] {
        colors::set_source_alpha(cr, ink, alpha);
        let mut n = 0;
        for (at, bar) in columns.slice.iter().enumerate() {
            let Some(cells) = cache.bars.get(&bar.ts) else { continue };
            let (x, room) = match lane {
                Lane::Delta { profiled } => across(columns, at, with_bar, profiled).rows,
                Lane::Profile => across(columns, at, with_bar, true).profile,
            };
            cells.footprint.each_row(step, |low, tick| {
                let value = match lane {
                    Lane::Delta { .. } => tick.delta,
                    Lane::Profile => tick.total as i64,
                };
                let delta = value;
                let text = rows::short(delta, &mut buf);
                let text_px = text.len() as f64 * advance;
                if text_px + PAD > room {
                    return;
                }
                let (offset, on_dark) = place(delta, room, text_px);
                if on_dark != dark {
                    return;
                }
                let (top, bottom) = row_y(low);
                let baseline = ((top + bottom) / 2.0 + size * 0.36).round();
                let mut pen = x + offset;
                for byte in text.bytes() {
                    let Some(at) = CHARS.iter().position(|c| *c == byte) else { continue };
                    batch[n] = cairo::Glyph::new(table[at], pen, baseline);
                    pen += advance;
                    n += 1;
                    if n == BATCH {
                        let _ = cr.show_glyphs(&batch);
                        n = 0;
                    }
                }
            });
        }
        if n > 0 {
            let _ = cr.show_glyphs(&batch[..n]);
        }
    }
}

/// The figures' type size in a row `row_h` pixels tall.
fn figure_size(row_h: f64) -> f64 {
    (row_h * 0.75).clamp(8.0, FIGURE_SIZE)
}

/// The glyph of each of [`CHARS`], looked up once.
fn glyphs(cr: &cairo::Context) -> Option<[c_ulong; 14]> {
    if let Some(known) = GLYPHS.get() {
        return Some(known);
    }
    let text = std::str::from_utf8(CHARS).ok()?;
    let (found, _) = cr.scaled_font().text_to_glyphs(0.0, 0.0, text).ok()?;
    let mut table: [c_ulong; 14] = [0; 14];
    if found.len() != table.len() {
        return None;
    }
    for (slot, glyph) in table.iter_mut().zip(&found) {
        *slot = glyph.index();
    }
    GLYPHS.set(Some(table));
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The automatic step is worked out when the price scale changes and
    /// held while it still makes a compact row; a fixed one is taken as set.
    #[test]
    fn a_row_step_is_held_until_the_scale_moves_it() {
        let mut cache = Cache::default();
        assert_eq!(cache.step_for(None, 4.0), 4, "16 px rows");
        assert_eq!(cache.step_for(None, 3.6), 4, "14.4 px rows are kept");
        assert_eq!(cache.step_for(None, 2.0), 10, "8 px rows are not");
        assert_eq!(cache.step_for(Some(2), 4.0), 2, "as set");
        assert_eq!(cache.auto_step(), Some(10), "a fixed step leaves the automatic one alone");
    }

    /// A frame of a footprint zoomed in, as its pixels.
    fn frame(numbers: bool) -> Vec<u8> {
        shot(crate::flow_bench::FlowScene::new(120, 20, &["footprint"], 60, 0).footprint_numbers(numbers))
    }

    fn shot(scene: crate::flow_bench::FlowScene) -> Vec<u8> {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1600, 900).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            scene.view_frame(&cr, 1600.0, 900.0);
        }
        surface.flush();
        surface.data().unwrap().to_vec()
    }

    /// Numbers off leaves the figures out and the rows and candles drawn:
    /// a frame without them differs from one with them only where they
    /// were written.
    #[test]
    fn numbers_off_draws_the_rows_alone() {
        let (on, off) = (frame(true), frame(false));
        assert_ne!(on, off, "figures written with numbers on");
        let changed = on.chunks(4).zip(off.chunks(4)).filter(|(a, b)| a != b).count();
        assert!(changed < on.len() / 4 / 10, "only the figures' pixels differ: {changed}");
        assert_eq!(off, frame(false), "the same rows every frame");
    }

    /// The bar beside each column is the chart's own, in its style and its
    /// colours: a narrow candle, a narrow OHLC bar, or none for a line. The
    /// footprint's colouring, Monochrome here, never reaches it, and the
    /// chart's colours never reach the rows.
    #[test]
    fn the_bar_beside_the_rows_follows_the_chart() {
        use omacharts_engine::bars::BarStyle;
        use omacharts_engine::theme::BarColouring;
        let frame = |style: BarStyle| {
            shot(
                crate::flow_bench::FlowScene::new(120, 20, &["footprint"], 60, 0)
                    .footprint_numbers(false)
                    .footprint_colouring(BarColouring::Monochrome)
                    .bars(style, "#ff00ff", "#00ffff"),
            )
        };
        // Cairo's pixels are blue, green, red and alpha.
        let bar_pixels = |pixels: &[u8]| {
            pixels.chunks(4).filter(|p| *p == [255, 0, 255, 255] || *p == [255, 255, 0, 255]).count()
        };
        let (candles, ohlc, line) = (frame(BarStyle::Candles), frame(BarStyle::Ohlc), frame(BarStyle::Line));
        assert!(bar_pixels(&candles) > 200, "narrow candles in the chart's colours: {}", bar_pixels(&candles));
        assert!(bar_pixels(&ohlc) > 100, "narrow OHLC bars in the chart's colours: {}", bar_pixels(&ohlc));
        assert!(bar_pixels(&ohlc) < bar_pixels(&candles), "bars are thinner than filled candles");
        assert_ne!(candles, ohlc);
        assert_eq!(bar_pixels(&line), 0, "a line chart's footprint draws no bar");
        assert_ne!(line, candles, "and its rows take the bar's room");
        // Nothing but the bars is in the chart's colours: with no bar, the
        // rows are the footprint's Monochrome ink alone.
        assert!(line.chunks(4).all(|p| !(p[2] > 200 && p[1] < 60 && p[0] > 200)), "no magenta in the rows");
    }

    /// FNV-1a over the plot's pixels, left of the price axis and above the
    /// time axis, where no word is written: a frame's fingerprint that does
    /// not hang on the machine's fonts.
    fn plot_print(pixels: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for y in 40..840 {
            let row = &pixels[y * 1600 * 4..(y + 1) * 1600 * 4];
            for byte in &row[..1400 * 4] {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        }
        hash
    }

    /// What the command line reads out of a footprint is what it draws:
    /// every bar on screen, row by row, the same delta and total as the
    /// rows the frame was painted from, in the same step; and the sides
    /// add up to both.
    #[test]
    fn the_rows_read_out_are_the_rows_drawn() {
        let scene = crate::flow_bench::FlowScene::new(120, 20, &["footprint"], 60, 0);
        let _ = shot_of(&scene);
        let state = scene.state();
        let read = read_out(state, BarPick::Visible, None).expect("volume at price");
        let cache = state.footprint.borrow();
        let step = cache.auto_step().expect("drawn automatically");
        assert_eq!(read.row_ticks, step);
        assert_eq!(read.bars.len(), 20);
        for bar in &read.bars {
            let kept = cache.bars.get(&bar.bar.ts).expect("drawn");
            let mut drawn = Vec::new();
            kept.footprint.each_row(step, |low, tick| drawn.push((low, tick)));
            let out: Vec<(i32, rows::Tick)> = bar.rows.iter().map(|row| (row.low_tick, row.sides.tick())).collect();
            assert_eq!(out, drawn, "bar {}", bar.bar.ts);
            let volume: u64 = bar.rows.iter().map(|row| row.sides.total()).sum();
            assert_eq!(volume as f64, bar.bar.volume, "bar {}'s rows hold its volume", bar.bar.ts);
            let most = bar.rows.iter().max_by_key(|row| row.sides.total()).map(|row| row.low_tick);
            assert_eq!(bar.poc.is_some(), most.is_some());
        }
        let last = read_out(state, BarPick::Last(3), Some(4)).unwrap();
        assert_eq!(last.bars.len(), 3);
        assert!(last.bars.iter().all(|bar| bar.rows.iter().all(|row| row.low_tick.rem_euclid(4) == 0)));
        let (from, to) = (read.bars[2].bar.ts, read.bars[5].bar.ts);
        let between = read_out(state, BarPick::Between(from, to), None).unwrap();
        assert_eq!(between.bars, read.bars[2..5].to_vec());
    }

    fn shot_of(scene: &crate::flow_bench::FlowScene) -> Vec<u8> {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1600, 900).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            scene.view_frame(&cr, 1600.0, 900.0);
        }
        surface.flush();
        surface.data().unwrap().to_vec()
    }

    /// The footprints the volume profile beside the rows must leave as they
    /// were when it is off: numbers off, so no font is in the frame.
    fn as_before() -> Vec<(&'static str, Vec<u8>)> {
        use omacharts_engine::bars::BarStyle;
        let scene = || crate::flow_bench::FlowScene::new(120, 20, &["footprint"], 60, 0).footprint_numbers(false);
        vec![
            ("candles", shot(scene())),
            ("ohlc", shot(scene().bars(BarStyle::Ohlc, "#ff00ff", "#00ffff"))),
            ("line", shot(scene().bars(BarStyle::Line, "#ff00ff", "#00ffff"))),
            ("wide", shot(crate::flow_bench::FlowScene::new(120, 12, &["footprint"], 60, 0).footprint_numbers(false))),
        ]
    }

    /// A footprint with its volume profile off draws exactly what it drew
    /// before the profile was there: the same pixels, bar for bar, in every
    /// bar style, zoomed in and just wide enough. Each print was taken from
    /// the footprint as it stood before the profile was added.
    #[test]
    fn a_footprint_with_its_profile_off_draws_exactly_as_before() {
        let before = [
            ("candles", 0xd861_13e3_7be1_03d3_u64),
            ("ohlc", 0xe2d5_0520_13a9_0444),
            ("line", 0xa34e_1a17_ac46_3877),
            ("wide", 0x1273_edbd_fa0f_a845),
        ];
        for ((name, pixels), (was, print)) in as_before().into_iter().zip(before) {
            assert_eq!(name, was);
            assert_eq!(plot_print(&pixels), print, "{name}: the profile off moved a pixel");
        }
    }

    /// The columns a profile's rows and a footprint's delta rows take: the
    /// profile at the column's left, the narrow bar past it, the delta rows
    /// past that, each side as wide as the other, and with it off, the
    /// delta rows where they always were.
    #[test]
    fn a_profile_takes_the_column_s_left_and_the_rows_move_past_it() {
        for bar_px in [40.0, 64.0, 90.0, 140.0] {
            let columns = Columns::of(&[], 0, 10, 0.3, bar_px * 10.0);
            for with_bar in [true, false] {
                let off = across(&columns, 3, with_bar, false);
                let on = across(&columns, 3, with_bar, true);
                let (gap, body) = narrow(bar_px);
                assert_eq!(off.profile.1, 0.0);
                let (px, pw) = on.profile;
                assert_eq!(px, off.rows.0 - if with_bar { body + gap } else { 0.0 }, "the profile starts where the column's parts did");
                let past = if with_bar { pw + gap + body + gap } else { pw + gap };
                assert_eq!(on.rows.0, px + past, "{bar_px} {with_bar}");
                assert!((on.rows.1 - pw).abs() < 2.0, "two halves, but for rounding: {pw} and {}", on.rows.1);
                assert_eq!(pw, rows_room(bar_px, with_bar, true));
            }
        }
    }

    /// With a profile on, a footprint waits for room for both its sides,
    /// with their figures or not; with it off, it comes where it came.
    #[test]
    fn a_footprint_with_a_profile_waits_for_both_sides() {
        let at = |numbers: bool, profile: Option<bool>| (1..600).map(f64::from).find(|px| fits(*px, true, numbers, profile, false)).unwrap();
        let alone = at(true, None);
        let both = at(true, Some(true));
        assert!(both > alone * 1.6, "both written: {both} against {alone}");
        assert!(rows_room(both, true, true) >= room_needed(true));
        assert!(at(true, Some(false)) == both, "the delta's figures still want their room");
        assert!(at(false, Some(false)) < at(false, Some(true)), "a profile without figures comes sooner");
        assert!(at(false, Some(false)) > at(false, None));
    }

    /// The profile drawn: every row of the bars on screen at its total, at
    /// the column's left, in its own colour, with and without its figures;
    /// the delta rows and the bar moved, not changed.
    #[test]
    fn a_profile_draws_beside_each_bar_in_its_own_colour() {
        let scene = || crate::flow_bench::FlowScene::new(120, 10, &["footprint"], 60, 0).footprint_numbers(false);
        let off = shot(scene());
        let on = shot(scene().footprint_profile(false));
        let written = shot(scene().footprint_profile(true));
        assert_ne!(off, on, "the profile draws");
        assert_ne!(on, written, "and its figures");
        // Blue, the theme's, see-through over the chart: pixels leaning to
        // blue where nothing did. Cairo's pixels are blue, green, red.
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().unwrap();
        let blue = theme.swatches.iter().find(|s| s.name == flow::PROFILE_SWATCH).unwrap().hex.clone();
        let (r, g, b, _) = crate::colors::rgba(&blue);
        assert!(b > r && b > g, "the swatch is blue");
        let near = |pixels: &[u8]| {
            pixels.chunks(4).filter(|p| i32::from(p[0]) > i32::from(p[2]) + 24 && i32::from(p[0]) > i32::from(p[1]) + 8).count()
        };
        assert!(near(&on) > near(&off) + 200, "drawn in its swatch: {} against {}", near(&on), near(&off));
    }

    /// The first bar width, a pixel at a time, a footprint comes at.
    fn comes_at(with_bar: bool, numbers: bool) -> f64 {
        (1..400).map(f64::from).find(|px| fits(*px, with_bar, numbers, None, false)).unwrap()
    }

    /// A footprint comes once a column holds the narrow bar and a row the
    /// widest figure reads in, and goes only a few pixels under that, so a
    /// zoom resting at the edge does not flicker; coming back, it waits for
    /// the whole width again.
    #[test]
    fn a_footprint_comes_once_its_figures_fit_and_goes_a_little_under() {
        let edge = comes_at(true, true);
        let (gap, body) = narrow(edge);
        assert!(edge - 3.0 * gap - body >= widest() + 2.0 * INSIDE, "the widest figure fits at {edge}");
        assert!((40.0..80.0).contains(&edge), "a readable column: {edge}");
        assert!(fits(edge, true, true, None, false));
        assert!(!fits(edge - 1.0, true, true, None, false), "narrower, not yet");
        assert!(fits(edge - 2.0, true, true, None, true), "shown, it stays a little under");
        assert!(!fits(edge - HYSTERESIS - 2.0, true, true, None, true), "and goes past that");
        assert!(!fits(edge - 2.0, true, true, None, false), "coming back, the whole width again");
    }

    /// Without its figures a row needs only to be a short bar, and without
    /// the chart's bar beside it a column has that much more for its rows.
    #[test]
    fn rows_alone_and_rows_without_a_bar_come_sooner() {
        let (figures, alone, no_bar) = (comes_at(true, true), comes_at(true, false), comes_at(false, true));
        assert!(alone < figures / 2.0, "rows alone at {alone}, with figures at {figures}");
        assert!(rows_room(alone, true, false) >= BARS_ONLY);
        assert!(no_bar < figures, "a line chart's at {no_bar}");
    }

    /// The widest figure is measured once, in the face and size figures are
    /// written in, and at least as wide as any figure [`rows::short`] makes.
    #[test]
    fn the_widest_figure_is_measured_once() {
        let first = widest();
        assert_eq!(WIDEST.get(), Some(first), "kept");
        assert_eq!(widest(), first);
        assert!(first > FIGURE_SIZE * 2.0 && first < FIGURE_SIZE * 6.0, "{first}");
        let mut buf = [0u8; 12];
        for magnitude in (0..8).flat_map(|power| [1_i64, 5, 9, 99, 999, 9_999].map(|m| m * 10_i64.pow(power))) {
            let delta = -magnitude.min(99_949_999);
            assert!(rows::short(delta, &mut buf).len() <= WIDEST_FIGURE.len(), "{delta} wider than {WIDEST_FIGURE}");
        }
    }
}
