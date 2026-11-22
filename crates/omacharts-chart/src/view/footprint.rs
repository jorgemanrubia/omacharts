//! Footprints, drawn.
//!
//! Zoomed in past the bar width its settings say, a footprint takes the
//! candles' place: a narrow candle at each bar's left, and beside it a row
//! for each price the bar traded at, as long as what was bought there less
//! what was sold, in buying's colour or selling's, with the figure written
//! in it where the row is tall enough to read. Rows too thin to tell apart
//! are folded into rows of several ticks.
//!
//! **Smooth.** A footprint is redrawn on every frame of a scroll and every
//! tick, so each bar's ladder is kept once read ([`Cache`]), keyed by the
//! bar's volume: a tick changes the forming bar's volume, and only that bar
//! is read again. Only the bars on screen are read and only their rows
//! drawn. Nothing is allocated per frame once the bars on screen are kept:
//! figures are written into a buffer of bytes and drawn as glyphs looked up
//! once, and each colour's rows are filled as one path.
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
/// The least a row is drawn tall, in pixels: thinner, ticks fold together.
const ROW_MIN: f64 = 3.0;
/// The least a row is tall for its figure to be written in it.
const FIGURE_MIN: f64 = 10.0;
/// What a figure is written with: [`rows::short`]'s characters.
const CHARS: &[u8; 14] = b"0123456789-.KM";
const FACE: &str = "monospace";

thread_local! {
    /// The glyph of each of [`CHARS`] in [`FACE`], looked up once.
    static GLYPHS: Cell<Option<[c_ulong; 14]>> = const { Cell::new(None) };
}

/// What a tick nothing traded at keeps in place of a delta.
const UNTRADED: i64 = i64::MIN;

/// One bar's ladder as a footprint keeps it: each tick's delta, from
/// `low_tick` up, or [`UNTRADED`].
#[derive(Default, Debug)]
struct Cells {
    low_tick: i32,
    cells: Vec<i64>,
    /// The bar's volume when it was read, by its bits: a bar that has
    /// traded since is read again.
    volume: u64,
}

impl Cells {
    /// Each row of `step` ticks that traded, lowest first, as its low tick
    /// and its delta.
    #[inline]
    fn each_row(&self, step: u32, mut each: impl FnMut(i32, i64)) {
        let step = step.max(1) as i32;
        let mut open: Option<(i32, i64)> = None;
        for (i, delta) in self.cells.iter().enumerate() {
            if *delta == UNTRADED {
                continue;
            }
            let tick = self.low_tick + i as i32;
            let low = tick.div_euclid(step) * step;
            match &mut open {
                Some((at, sum)) if *at == low => *sum += *delta,
                _ => {
                    if let Some((at, sum)) = open.take() {
                        each(at, sum);
                    }
                    open = Some((low, *delta));
                }
            }
        }
        if let Some((at, sum)) = open {
            each(at, sum);
        }
    }
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
}

impl Cache {
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
        let days = match tf.is_intraday() {
            true => None,
            false => state.instrument.as_ref().map(omacharts_engine::ladders::Days::new),
        };
        for bar in columns.slice {
            let volume = bar.volume.to_bits();
            if self.bars.get(&bar.ts).is_some_and(|kept| kept.volume == volume) {
                continue;
            }
            self.scratch.clear();
            let scratch = &mut self.scratch;
            let read = match &days {
                None => held.visit_between(bar.ts, bar.ts + tf.seconds(), |part| scratch.add_ladder(part)),
                Some(days) => {
                    let end = tf.shift(bar.ts, 1);
                    match held.days_between(bar.ts.div_euclid(86_400), end.div_euclid(86_400), days) {
                        Some(ladder) => {
                            *scratch = ladder;
                            true
                        }
                        None => false,
                    }
                }
            };
            if !read {
                continue;
            }
            let mut cells = self.bars.remove(&bar.ts).or_else(|| self.spare.pop()).unwrap_or_default();
            cells.low_tick = self.scratch.low_tick;
            cells.volume = volume;
            cells.cells.clear();
            cells.cells.extend(self.scratch.levels.iter().map(|level| match level.volume() {
                0 => UNTRADED,
                _ => level.at_ask as i64 - level.at_bid as i64,
            }));
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

/// The footprint a chart draws in place of its candles at this zoom: the
/// first one shown whose bar width it is zoomed to.
pub(super) fn shown<'a>(state: &'a State, columns: &Columns) -> Option<&'a Drawn> {
    if columns.sub_pixel() || state.volume_at_price.is_none() {
        return None;
    }
    state.shown_indicators().find(|drawn| match &drawn.indicator.params {
        Params::Footprint { zoom, .. } => {
            matches!(drawn.output, omacharts_engine::Output::Footprint) && columns.bar_px >= *zoom
        }
        _ => false,
    })
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

/// Buying's and selling's colours for `inks`: its own, or its colouring's.
pub(super) fn inks<'a>(state: &'a State, inks: &'a flow::Inks) -> (Cow<'a, str>, Cow<'a, str>) {
    let (up, down) = state.delta_pair(inks.colouring);
    let buy = inks.buy.as_ref().map_or(Cow::Borrowed(up), |c| chosen(c, state));
    let sell = inks.sell.as_ref().map_or(Cow::Borrowed(down), |c| chosen(c, state));
    (buy, sell)
}

/// Draw `drawn`'s footprint over the columns: falls back to the candles
/// where there is nothing to read it from.
pub(super) fn draw(cr: &cairo::Context, state: &State, drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64) {
    let Params::Footprint { intensity, inks: chosen_inks, .. } = &drawn.indicator.params else { return };
    let Some(read) = state.volume_at_price.as_ref() else { return };
    let Some(first) = state.bars.first() else { return };
    let mut drew = false;
    read(first.ts, &mut |held, _| {
        let Some(held) = held else { return };
        let mut cache = state.footprint.borrow_mut();
        cache.fill(state, held, columns);
        paint(cr, state, &cache, held.tick_size, *intensity, chosen_inks, columns, to_y);
        drew = true;
    });
    if !drew {
        super::candles(cr, state.bar_style, state.ohlc_thickness, &state.scheme, columns, to_y);
    }
}

/// Where a bar's parts go across its column.
#[derive(Clone, Copy)]
struct Across {
    /// The candle's body, as a left edge and a width, and its wick.
    body: (f64, f64),
    wick: f64,
    /// Where the rows start and how far they may reach.
    rows: (f64, f64),
}

fn across(columns: &Columns, at: usize) -> Across {
    let bar_px = columns.bar_px;
    let left = columns.x(at) - bar_px / 2.0;
    let gap = (bar_px * 0.06).clamp(2.0, 6.0).round();
    let body_w = ((bar_px * 0.1).clamp(3.0, 9.0) / 2.0).floor() * 2.0 + 1.0;
    let body_x = (left + gap).round();
    let rows_x = body_x + body_w + gap;
    let rows_w = (left + bar_px - gap - rows_x).max(0.0);
    Across { body: (body_x, body_w), wick: body_x + (body_w - 1.0) / 2.0 + 0.5, rows: (rows_x, rows_w) }
}

#[allow(clippy::too_many_arguments)]
fn paint(
    cr: &cairo::Context,
    state: &State,
    cache: &Cache,
    tick_size: f64,
    intensity: bool,
    chosen_inks: &flow::Inks,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let bars = columns.slice;
    let tick_px = (to_y(0.0) - to_y(tick_size)).abs();
    let step = rows::ticks_per_row(tick_px, ROW_MIN);
    let row_y = |low: i32| {
        let top = to_y((f64::from(low) + f64::from(step) - 0.5) * tick_size);
        let bottom = to_y((f64::from(low) - 0.5) * tick_size);
        (top, bottom)
    };
    let row_h = tick_px * f64::from(step);
    // A pixel between rows, when they have one to spare.
    let inset = if row_h >= 5.0 { 0.5 } else { 0.0 };

    // The biggest row on screen, which every row's length and strength is
    // measured against.
    let mut largest: u64 = 0;
    for bar in bars {
        if let Some(cells) = cache.bars.get(&bar.ts) {
            cells.each_row(step, |_, delta| largest = largest.max(delta.unsigned_abs()));
        }
    }

    let (buy, sell) = inks(state, chosen_inks);
    let (buy_alpha, sell_alpha) = chosen_inks.alphas(flow::FOOTPRINT_ALPHA);
    let levels = if intensity { LEVELS } else { 1 };
    let length = |delta: i64, room: f64| match largest {
        0 => 0.0,
        largest => (room * delta.unsigned_abs() as f64 / largest as f64).max(1.0),
    };

    cr.save().ok();
    // Each side's rows, a strength at a time, each strength one path.
    for (buying, ink, full) in [(true, &buy, buy_alpha), (false, &sell, sell_alpha)] {
        for level in 1..=levels {
            let mut any = false;
            for (at, bar) in bars.iter().enumerate() {
                let Some(cells) = cache.bars.get(&bar.ts) else { continue };
                let (x, room) = across(columns, at).rows;
                cells.each_row(step, |low, delta| {
                    if delta == 0 || (delta > 0) != buying || rows::strength(delta, largest, levels) != level {
                        return;
                    }
                    let (top, bottom) = row_y(low);
                    cr.rectangle(x, top.round() + inset, length(delta, room), (bottom.round() - top.round() - 2.0 * inset).max(1.0));
                    any = true;
                });
            }
            if any {
                let alpha = match intensity {
                    true => rows::step_alpha(level, levels, FAINT * full, full),
                    false => full,
                };
                colors::set_source_alpha(cr, ink, alpha);
                let _ = cr.fill();
            }
        }
        // The outlines, in the same hue, so a faint row still has an edge.
        let mut any = false;
        for (at, bar) in bars.iter().enumerate() {
            let Some(cells) = cache.bars.get(&bar.ts) else { continue };
            let (x, room) = across(columns, at).rows;
            cells.each_row(step, |low, delta| {
                if delta == 0 || (delta > 0) != buying {
                    return;
                }
                let (top, bottom) = row_y(low);
                let h = (bottom.round() - top.round() - 2.0 * inset).max(1.0);
                if h < 3.0 {
                    return;
                }
                cr.rectangle(x + 0.5, top.round() + inset + 0.5, (length(delta, room) - 1.0).max(0.0), h - 1.0);
                any = true;
            });
        }
        if any {
            cr.set_line_width(1.0);
            colors::set_source_alpha(cr, ink, full.max(0.35));
            let _ = cr.stroke();
        }
    }

    // The narrow candles, hollow, in the chart's own colours.
    cr.set_line_width(1.0);
    for rising in [false, true] {
        let mut any = false;
        for (at, bar) in bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising || !bar.close.is_finite() {
                continue;
            }
            let Across { body: (x, w), wick, .. } = across(columns, at);
            let top = to_y(bar.open.max(bar.close)).round();
            let bottom = to_y(bar.open.min(bar.close)).round().max(top + 1.0);
            let (high, low) = (to_y(bar.high).round(), to_y(bar.low).round());
            cr.rectangle(x + 0.5, top + 0.5, w - 1.0, bottom - top);
            if high < top {
                cr.move_to(wick, high);
                cr.line_to(wick, top);
            }
            if low > bottom {
                cr.move_to(wick, bottom + 1.0);
                cr.line_to(wick, low);
            }
            any = true;
        }
        if any {
            colors::set_source(cr, if rising { &state.scheme.up } else { &state.scheme.down });
            let _ = cr.stroke();
        }
    }

    // The figures, where the rows are tall enough to read them.
    if row_h >= FIGURE_MIN {
        // A number goes dark where it sits wholly on a row whose fill, laid
        // over the background, is nearer the text's lightness than the
        // background's: there a light number would fade into it. Lightness
        // is a sum of the channels, so a fill's is the blend of the two.
        use omacharts_engine::theme::luminance;
        let ground = luminance(&state.theme.ui.background);
        let text = luminance(&state.theme.ui.text);
        let (buy_light, sell_light) = (luminance(&buy), luminance(&sell));
        let on_fill = |delta: i64, room: f64, text_px: f64| {
            let (full, light) = if delta > 0 { (buy_alpha, buy_light) } else { (sell_alpha, sell_light) };
            let alpha = match intensity {
                true => rows::step_alpha(rows::strength(delta, largest, levels), levels, FAINT * full, full),
                false => full,
            };
            let fill = ground + (light - ground) * alpha;
            (fill - ground).abs() > (fill - text).abs() && length(delta, room) >= text_px + 5.0
        };
        figures(cr, state, cache, step, columns, &row_y, &on_fill);
    }
    cr.restore().ok();
}

/// Each row's delta written at its left, inside it, in one face, as glyphs
/// drawn a batch at a time.
fn figures(
    cr: &cairo::Context,
    state: &State,
    cache: &Cache,
    step: u32,
    columns: &Columns,
    row_y: &impl Fn(i32) -> (f64, f64),
    on_fill: &impl Fn(i64, f64, f64) -> bool,
) {
    const BATCH: usize = 256;
    let row_h = row_y(0).1 - row_y(0).0;
    let size = (row_h * 0.7).clamp(8.0, 12.0);
    cr.select_font_face(FACE, cairo::FontSlant::Normal, cairo::FontWeight::Normal);
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
            let (x, room) = across(columns, at).rows;
            cells.each_row(step, |low, delta| {
                let text = rows::short(delta, &mut buf);
                let text_px = text.len() as f64 * advance;
                if text_px + 4.0 > room || on_fill(delta, room, text_px) != dark {
                    return;
                }
                let (top, bottom) = row_y(low);
                let baseline = ((top + bottom) / 2.0 + size * 0.36).round();
                let mut pen = x + 3.0;
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

    #[test]
    fn rows_fold_kept_ticks_on_the_fly() {
        let cells = Cells { low_tick: 99, cells: vec![-1, 2, 0, UNTRADED, 4], volume: 0 };
        let mut seen = Vec::new();
        cells.each_row(1, |low, delta| seen.push((low, delta)));
        assert_eq!(seen, vec![(99, -1), (100, 2), (101, 0), (103, 4)], "nothing traded, no row");
        seen.clear();
        cells.each_row(2, |low, delta| seen.push((low, delta)));
        assert_eq!(seen, vec![(98, -1), (100, 2), (102, 4)]);
    }
}
