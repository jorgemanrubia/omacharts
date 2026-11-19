//! Market profiles, drawn.
//!
//! Each period's TPOs in their rows, as letters or as blocks, a cell each,
//! laid out where a volume profile would be: the same box across the
//! period, the same edge to grow from, and side by side with any volume
//! profile on the chart in the order the legend lists them. A cell is the
//! longest row's share of that box, so the row with the most TPOs reaches
//! as far as the profile's width says, and a click finds a row where it is
//! drawn.
//!
//! Nothing here allocates per frame: the letters are drawn as glyphs from a
//! table looked up once, a stack's worth at a time, and the cells of one
//! colour are filled as one path.

use std::cell::Cell;
use std::ffi::c_ulong;

use gtk::cairo;
use omacharts_engine::indicators::market::{self, MarketProfile, Strengths, Style};
use omacharts_engine::indicators::Profile;

use super::{executions, selection, Columns, Drawn, State};
use crate::colors;

/// The least a cell must be, across and down, for its letter to be read:
/// below either the profile is drawn as blocks, whatever it is set to.
const LETTER_MIN_W: f64 = 6.0;
const LETTER_MIN_H: f64 = 7.0;
/// The largest a letter is drawn, however big its cell.
const LETTER_MAX_SIZE: f64 = 15.0;
/// The least a cell must be for blocks to be drawn one by one with a gap
/// between: narrower, a row is one bar as long as its TPOs.
const BLOCK_MIN: f64 = 3.0;

/// The face letters are drawn in: fixed width, so they line up in columns
/// down the profile as its periods do.
const FACE: &str = "monospace";

/// Glyphs drawn in one call.
const BATCH: usize = 128;

thread_local! {
    /// The glyph of each letter in [`FACE`], looked up once: a face's glyphs
    /// are the same at every size.
    static GLYPHS: Cell<Option<[c_ulong; 52]>> = const { Cell::new(None) };
}

/// Which pass a row is drawn in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Outside = 0,
    Inside = 1,
}

/// One market profile indicator's profiles.
pub(super) fn draw_market_profiles(
    cr: &cairo::Context,
    state: &State,
    drawn: &Drawn,
    profiles: &[std::sync::Arc<MarketProfile>],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let omacharts_engine::Params::MarketProfile {
        style,
        poc,
        highlight,
        poc_color,
        value_area_color,
        open_close,
        open_color,
        close_color,
        open_alpha,
        close_alpha,
        alpha,
        value_area_alpha,
        outside_alpha,
        poc_alpha,
        poc_width,
        poc_style,
        outline,
        outline_color,
        outline_style,
        outline_width,
        outline_alpha,
        ..
    } = &drawn.indicator.params
    else {
        return;
    };
    let ends = open_close.then(|| {
        (
            open_color.as_ref().map_or_else(|| state.theme.tpo_open(), |c| c.resolve(&state.theme)),
            close_color.as_ref().map_or_else(|| state.theme.tpo_close(), |c| c.resolve(&state.theme)),
        )
    });
    let poc_ink = poc.then(|| state.theme.companion_or(poc_color.as_ref(), &drawn.color));
    let outline_ink = outline.then(|| outline_color.as_ref().map_or_else(|| drawn.color.clone(), |c| c.resolve(&state.theme)));
    let area_ink = match (highlight, value_area_color) {
        (true, Some(choice)) => Some(choice.resolve(&state.theme)),
        _ => None,
    };
    let inks = Inks {
        outside: &drawn.color,
        inside: area_ink.as_deref().unwrap_or(&drawn.color),
        highlight: *highlight,
        strengths: Strengths::of(*alpha, *value_area_alpha, *outside_alpha, *poc_alpha),
    };
    let look = selection::placed_look(state, drawn);
    let (top, bottom) = match cr.clip_extents() {
        Ok((_, y1, _, y2)) => (y1, y2),
        Err(_) => (f64::NEG_INFINITY, f64::INFINITY),
    };
    for (at, market) in profiles.iter().enumerate() {
        let profile = &market.profile;
        // Where a click finds it, too, so the two cannot drift apart.
        let Some((area, cell, row_h, spread)) =
            placed(state, drawn, profiles, at, columns, &look, to_y)
        else {
            continue;
        };
        let place = Place {
            area: &area,
            cell,
            top,
            bottom,
            spread,
        };
        // Expanded, its TPOs lie over the bars: drawn lighter, so the bars
        // read first and the TPOs as the ground under them.
        let dimmed;
        let inks = match spread {
            Some(_) => {
                let set = inks.strengths;
                dimmed = Inks {
                    strengths: Strengths {
                        rows: set.rows * EXPANDED_STRENGTH,
                        value_area: set.value_area * EXPANDED_STRENGTH,
                        outside: set.outside * EXPANDED_STRENGTH,
                        poc: set.poc,
                    },
                    ..inks
                };
                &dimmed
            }
            None => &inks,
        };
        match *style == Style::Letters && cell >= LETTER_MIN_W && row_h >= LETTER_MIN_H {
            true => letters(cr, market, &place, inks, row_h, to_y),
            false => blocks(cr, market, &place, inks, row_h, to_y),
        }
        // The edge round the cells, whichever they are drawn as, so turning
        // letters to blocks never moves it; a run of rows each, a gap in the
        // profile left open.
        if let Some(ink) = &outline_ink {
            let width = outline_width.max(0.5);
            cr.set_line_width(width);
            cr.set_line_join(cairo::LineJoin::Miter);
            super::set_edge_pattern(cr, *outline_style, width, 0.0);
            colors::set_source_alpha(cr, ink, market::outline_strength(*outline_alpha));
            match place.spread {
                None => selection::profile_path(cr, &area, profile, to_y, selection::Side::Up),
                Some(_) => spread_path(cr, market, &place, to_y),
            }
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
        // The point of control as a volume profile's is, a line across the
        // profile over its rows, through the middle of its own row: the
        // longest, so the line is as long as it.
        if let (Some(ink), Some(row)) = (&poc_ink, profile.rows.get(profile.poc)) {
            let width = poc_width.max(0.5);
            let y = ((to_y(row.high) + to_y(row.low)) / 2.0).round() + 0.5;
            let (left, right) = place.extent(market);
            colors::set_source_alpha(cr, ink, inks.strengths.poc);
            cr.set_line_width(width);
            super::set_edge_pattern(cr, *poc_style, width, left);
            cr.move_to(left, y);
            cr.line_to(right, y);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
        // The open and the close, an arrow each beside its TPO.
        if let Some((open, close)) = &ends {
            let size = arrow_size(state);
            let marks = arrows_of(market, &place, to_y);
            for ((tip, y, points), (ink, alpha)) in marks
                .into_iter()
                .zip([(open, *open_alpha), (close, *close_alpha)])
            {
                let Some(tip) = tip else { continue };
                if y < top - size.1 || y > bottom + size.1 {
                    continue;
                }
                executions::arrow(cr, state, tip, y, points, size, ink, market::end_strength(alpha));
            }
        }
        if state.picked == Some(drawn.indicator.id) {
            match place.spread {
                None => selection::outline_profile(
                    cr,
                    &state.theme,
                    &drawn.color,
                    profile,
                    &area,
                    to_y,
                    selection::Side::All,
                ),
                Some(_) => selection::outline_with(cr, &state.theme, &drawn.color, |cr| {
                    spread_path(cr, market, &place, to_y)
                }),
            }
        }
    }
}

/// Where the `at`th of a market profile indicator's `profiles` is drawn:
/// its room, how wide and tall a TPO is, and, expanded, where its columns
/// are. Nothing for one off screen or with nothing to draw. What a click
/// is measured against too, so the two cannot drift apart.
#[allow(clippy::type_complexity)]
pub(super) fn placed(
    state: &State,
    drawn: &Drawn,
    profiles: &[std::sync::Arc<MarketProfile>],
    at: usize,
    columns: &Columns,
    look: &selection::ProfileLook,
    to_y: &impl Fn(f64) -> f64,
) -> Option<(selection::ProfileBox, f64, f64, Option<Spread>)> {
    let market = profiles.get(at)?;
    let profile = &market.profile;
    if profile.max_volume <= 0.0 || profile.rows.is_empty() {
        return None;
    }
    if market::expands(&drawn.indicator.params, at, profiles.len(), profile) {
        return spread(market, columns, to_y);
    }
    let area = selection::profile_box(profile, columns, look);
    if area.span <= 0.0 {
        return None;
    }
    let (area, cell, row_h) = cells(state, &drawn.indicator.params, profile, area, to_y);
    Some((area, cell, row_h, None))
}

/// Where an expanded profile's columns are: a linear map from a bar's
/// index on the chart to the middle of its slot, each column centred on
/// its TPO period's first bar.
#[derive(Clone, Copy, Debug)]
pub(super) struct Spread {
    /// The middle of bar nought's slot, wherever that is off screen.
    origin: f64,
    bar_px: f64,
}

impl Spread {
    fn middle(&self, bar: usize) -> f64 {
        self.origin + bar as f64 * self.bar_px
    }
}

/// An expanded profile laid out over its bars: a TPO as wide as a stacked
/// one's, narrower only when its periods are closer together than that,
/// so no two columns overlap. Nothing for one wholly off screen.
fn spread(
    market: &MarketProfile,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) -> Option<(selection::ProfileBox, f64, f64, Option<Spread>)> {
    let profile = &market.profile;
    let (first, last) = (columns.first, columns.first + columns.visible);
    if profile.last_bar < first || profile.first_bar >= last || market.column_bars.is_empty() {
        return None;
    }
    let spread = Spread {
        origin: columns.start + (0.5 - columns.first as f64) * columns.bar_px,
        bar_px: columns.bar_px,
    };
    let row_h = profile
        .rows
        .first()
        .map_or(0.0, |row| to_y(row.low) - to_y(row.high));
    let pitch = market
        .column_bars
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 * columns.bar_px)
        .fold(f64::INFINITY, f64::min);
    let cell = (row_h * CELL_PER_ROW).max(CELL_MIN).min(pitch).max(1.0);
    let left = spread.middle(market.column_bars[0]) - cell / 2.0;
    let right = spread.middle(*market.column_bars.last()?) + cell / 2.0;
    let area = selection::ProfileBox {
        edge: left,
        span: right - left,
        towards: 1.0,
    };
    Some((area, cell, row_h, Some(spread)))
}

/// The stepped edge round an expanded profile: each row from its first
/// TPO's left to its last's right, a run of rows at a time.
fn spread_path(
    cr: &cairo::Context,
    market: &MarketProfile,
    place: &Place,
    to_y: &impl Fn(f64) -> f64,
) {
    let rows = &market.profile.rows;
    let span = |at: usize| {
        let count = market.letters_of(at).len();
        (count > 0).then(|| {
            (
                place.tpo(market, at, 0).0,
                place.tpo(market, at, count - 1).1,
            )
        })
    };
    let mut at = 0;
    while at < rows.len() {
        if span(at).is_none() {
            at += 1;
            continue;
        }
        let from = at;
        while at < rows.len() && span(at).is_some() {
            at += 1;
        }
        // Up the left side from the lowest row, across the top, and back
        // down the right.
        let run = from..at;
        for (n, row) in run.clone().enumerate() {
            let (left, _) = span(row).unwrap_or_default();
            let (bottom, top) = (to_y(rows[row].low), to_y(rows[row].high));
            if n == 0 {
                cr.move_to(left, bottom);
            } else {
                cr.line_to(left, bottom);
            }
            cr.line_to(left, top);
        }
        for row in run.rev() {
            let (_, right) = span(row).unwrap_or_default();
            let (bottom, top) = (to_y(rows[row].low), to_y(rows[row].high));
            cr.line_to(right, top);
            cr.line_to(right, bottom);
        }
        cr.close_path();
    }
}

/// Whether `(x, y)` is on one of the TPOs of the `at`th profile, as they
/// are drawn: a row from its first TPO to its last.
#[allow(clippy::too_many_arguments)]
pub(super) fn hits(
    state: &State,
    drawn: &Drawn,
    profiles: &[std::sync::Arc<MarketProfile>],
    at: usize,
    columns: &Columns,
    look: &selection::ProfileLook,
    to_y: &impl Fn(f64) -> f64,
    (x, y): (f64, f64),
) -> bool {
    let Some((area, cell, _, spread)) = placed(state, drawn, profiles, at, columns, look, to_y)
    else {
        return false;
    };
    let market = &profiles[at];
    let place = Place {
        area: &area,
        cell,
        top: f64::NEG_INFINITY,
        bottom: f64::INFINITY,
        spread,
    };
    market.profile.rows.iter().enumerate().any(|(row, r)| {
        let count = market.letters_of(row).len();
        let top = to_y(r.high);
        let bottom = top + (to_y(r.low) - top).max(1.0);
        count > 0 && (top..=bottom).contains(&y) && {
            let (left, _) = place.tpo(market, row, 0);
            let (_, right) = place.tpo(market, row, count - 1);
            (left.min(right)..=left.max(right)).contains(&x)
        }
    })
}

/// Where a profile's open and close arrows go: each tip, the middle of its
/// row, and which way it points, one rightward and minus one leftward. The
/// open's stands outside the profile's own edge pointing in at its TPO,
/// the close's past its row's last TPO pointing back. Outside the cells, so
/// neither covers a TPO, the outline or the point of control's line; inside
/// the room [`cells`] keeps for them, so neither reaches a profile beside
/// it. `None` for a row the profile does not have.
#[cfg(test)]
pub(super) fn arrows(
    market: &MarketProfile,
    area: &selection::ProfileBox,
    cell: f64,
    to_y: &impl Fn(f64) -> f64,
) -> [(Option<f64>, f64, f64); 2] {
    let place = Place {
        area,
        cell,
        top: f64::NEG_INFINITY,
        bottom: f64::INFINITY,
        spread: None,
    };
    arrows_of(market, &place, to_y)
}

/// [`arrows`], wherever the profile's TPOs are placed: expanded, the
/// open's beside the first TPO period's, on its first bar, and the
/// close's beside the last's.
fn arrows_of(
    market: &MarketProfile,
    place: &Place,
    to_y: &impl Fn(f64) -> f64,
) -> [(Option<f64>, f64, f64); 2] {
    let profile = &market.profile;
    let area = place.area;
    let mark = |row: usize, last: bool, points: f64| {
        let Some(r) = profile.rows.get(row) else {
            return (None, 0.0, points);
        };
        let k = if last {
            market.letters_of(row).len().saturating_sub(1)
        } else {
            0
        };
        let (left, right) = place.tpo(market, row, k);
        let tip = match points > 0.0 {
            true => left - executions::STAND_OFF,
            false => right + executions::STAND_OFF,
        };
        (Some(tip), (to_y(r.high) + to_y(r.low)) / 2.0, points)
    };
    [mark(market.open_row, false, area.towards), mark(market.close_row, true, -area.towards)]
}

/// Every TPO of the `at`th profile as it is drawn, its row, its column
/// and its left and right, and its open and close arrows: what a test
/// holds the layout to.
#[cfg(test)]
#[allow(clippy::type_complexity)]
pub(super) fn laid_out(
    state: &State,
    drawn: &Drawn,
    profiles: &[std::sync::Arc<MarketProfile>],
    at: usize,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) -> Option<(Vec<(usize, u16, f64, f64)>, [(Option<f64>, f64, f64); 2])> {
    let look = selection::placed_look(state, drawn);
    let (area, cell, _, spread) = placed(state, drawn, profiles, at, columns, &look, to_y)?;
    let place = Place {
        area: &area,
        cell,
        top: f64::NEG_INFINITY,
        bottom: f64::INFINITY,
        spread,
    };
    let market = &profiles[at];
    let mut tpos = Vec::new();
    for row in 0..market.profile.rows.len() {
        for (k, column) in market.columns_of(row).iter().enumerate() {
            let (left, right) = place.tpo(market, row, k);
            tpos.push((row, *column, left, right));
        }
    }
    Some((tpos, arrows_of(market, &place, to_y)))
}

/// The room kept between an arrow's base and the edge of the profile's
/// room, in pixels.
const ARROW_CLEARANCE: f64 = 4.0;

/// A fill's arrow size, as the chart is drawing fills.
pub(super) fn arrow_size(state: &State) -> (f64, f64) {
    (executions::LENGTH * state.execution_scale, executions::HALF * state.execution_scale)
}

/// Where a profile's TPOs are drawn in `area`, its room, and how wide and
/// tall each is: while it marks its open and close, the room an arrow
/// needs is kept at each end, so the arrows stay in the profile's room.
/// What a click is measured against too, so the two cannot drift apart.
pub(super) fn cells(
    state: &State,
    params: &omacharts_engine::Params,
    profile: &Profile,
    mut area: selection::ProfileBox,
    to_y: &impl Fn(f64) -> f64,
) -> (selection::ProfileBox, f64, f64) {
    if let omacharts_engine::Params::MarketProfile { open_close: true, .. } = params {
        // The tip hugs its TPO; the base keeps clear of whatever is beside
        // the profile, so an arrow reads as this profile's.
        let gutter = arrow_size(state).0 + executions::STAND_OFF + ARROW_CLEARANCE;
        // However narrow the profile, so an arrow never reaches past it.
        area.edge += area.towards * gutter;
        area.span = (area.span - 2.0 * gutter).max(1.0);
    }
    let (cell, row_h) = cell(profile, &area, to_y);
    area.span = cell * profile.max_volume;
    (area, cell, row_h)
}

/// How wide a TPO is drawn, and how tall a row: about square, a letter's
/// room, so a profile reads as a block of letters hugging its edge however
/// wide its period, and at most what fits the longest row in its width.
/// What a click is measured against too.
pub(super) fn cell(profile: &Profile, area: &selection::ProfileBox, to_y: &impl Fn(f64) -> f64) -> (f64, f64) {
    let row_h = profile.rows.first().map_or(0.0, |row| to_y(row.low) - to_y(row.high));
    let fits = area.span / profile.max_volume.max(1.0);
    (fits.min((row_h * CELL_PER_ROW).max(CELL_MIN)), row_h)
}

/// A TPO's width as a share of its row's height, and the least it is
/// drawn at once rows are thinner than that.
const CELL_PER_ROW: f64 = 1.1;
const CELL_MIN: f64 = 6.0;

/// How much of its set strength an expanded profile's TPOs are drawn at,
/// over the bars they were traded in.
const EXPANDED_STRENGTH: f64 = 0.45;

/// The colours a profile's rows are drawn in.
#[derive(Clone, Copy)]
struct Inks<'a> {
    outside: &'a str,
    inside: &'a str,
    highlight: bool,
    /// How solid each sort of row is drawn, as letters.
    strengths: Strengths,
}

impl Inks<'_> {
    /// What a row of `sort` is drawn in, and how solid, as letters or as
    /// blocks.
    fn of(&self, sort: Sort, letters: bool) -> (&str, f64) {
        let strengths = match letters {
            true => self.strengths,
            false => self.strengths.as_blocks(),
        };
        match (sort, self.highlight) {
            (_, false) => (self.outside, strengths.rows),
            (Sort::Inside, true) => (self.inside, strengths.value_area),
            (Sort::Outside, true) => (self.outside, strengths.outside),
        }
    }

    /// Which pass row `at` of `profile` is drawn in.
    fn sort(&self, market: &MarketProfile, at: usize) -> Sort {
        let profile = &market.profile;
        let (low, high) = profile.value_area;
        match (low..=high).contains(&at) {
            true => Sort::Inside,
            false => Sort::Outside,
        }
    }
}

/// Where a profile's cells go.
struct Place<'a> {
    area: &'a selection::ProfileBox,
    cell: f64,
    /// The plot's top and bottom: rows outside are not drawn.
    top: f64,
    bottom: f64,
    /// Expanded, where its columns are; stacked, `None`.
    spread: Option<Spread>,
}

impl Place<'_> {
    /// The left and right of the `k`th TPO of row `row`: stacked, the
    /// `k`th cell from the edge; expanded, centred on its column's bar.
    fn tpo(&self, market: &MarketProfile, row: usize, k: usize) -> (f64, f64) {
        match self.spread {
            None => self.cell(k),
            Some(spread) => {
                let column = market.columns_of(row).get(k).copied().unwrap_or_default() as usize;
                let middle =
                    spread.middle(market.column_bars.get(column).copied().unwrap_or_default());
                (middle - self.cell / 2.0, middle + self.cell / 2.0)
            }
        }
    }

    /// The left and right of the whole profile: the point of control's
    /// line runs between them.
    fn extent(&self, market: &MarketProfile) -> (f64, f64) {
        match self.spread {
            None => {
                let (from, to) = self.cell(0);
                let (_, end) = self.cell((market.profile.max_volume as usize).saturating_sub(1));
                (from.min(end), to.max(end))
            }
            Some(_) => (self.area.edge, self.area.edge + self.area.span),
        }
    }

    /// The left and right of cell `k` from the edge the rows grow from.
    fn cell(&self, k: usize) -> (f64, f64) {
        let near = self.area.edge + self.area.towards * self.cell * k as f64;
        let far = near + self.area.towards * self.cell;
        (near.min(far), near.max(far))
    }

    fn shows(&self, y_top: f64, y_bottom: f64) -> bool {
        y_bottom >= self.top && y_top <= self.bottom
    }
}

/// Every TPO a block: a cell each with a gap between while they are big
/// enough to tell apart, else each row one bar as long as its TPOs.
fn blocks(
    cr: &cairo::Context,
    market: &MarketProfile,
    place: &Place,
    inks: &Inks,
    row_h: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let profile = &market.profile;
    let apart = place.cell >= BLOCK_MIN && row_h >= BLOCK_MIN;
    let gap = if apart { 1.0 } else { 0.0 };
    for sort in [Sort::Outside, Sort::Inside] {
        let mut any = false;
        for (at, row) in profile.rows.iter().enumerate() {
            if inks.sort(market, at) != sort || row.volume <= 0.0 {
                continue;
            }
            let y = to_y(row.high);
            let h = (to_y(row.low) - y).max(1.0);
            if !place.shows(y, y + h) {
                continue;
            }
            let count = row.volume as usize;
            match apart || place.spread.is_some() {
                true => {
                    for k in 0..count {
                        let (left, right) = place.tpo(market, at, k);
                        cr.rectangle(
                            left,
                            y + gap * 0.5,
                            (right - left - gap).max(0.5),
                            (h - gap).max(0.5),
                        );
                    }
                }
                false => {
                    let (from, to) = place.cell(0);
                    let (_, end) = place.cell(count.saturating_sub(1));
                    let (left, right) = (from.min(end), to.max(end));
                    cr.rectangle(left, y, right - left, h);
                }
            }
            any = true;
        }
        if any {
            let (ink, alpha) = inks.of(sort, false);
            colors::set_source_alpha(cr, ink, alpha);
            let _ = cr.fill();
        }
    }
}

/// Every TPO its period's letter, centred in its cell.
fn letters(
    cr: &cairo::Context,
    market: &MarketProfile,
    place: &Place,
    inks: &Inks,
    row_h: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let profile = &market.profile;
    cr.save().ok();
    cr.select_font_face(FACE, cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    let size = (row_h * 0.9).min(place.cell * 1.4).min(LETTER_MAX_SIZE);
    cr.set_font_size(size);
    let Some(glyphs) = glyphs(cr) else {
        cr.restore().ok();
        return blocks(cr, market, place, inks, row_h, to_y);
    };
    let (advance, rise) = match cr.font_extents() {
        Ok(extents) => (extents.max_x_advance(), (extents.ascent() - extents.descent()) / 2.0),
        Err(_) => (size * 0.6, size * 0.35),
    };
    let mut batch = [cairo::Glyph::new(0, 0.0, 0.0); BATCH];
    for sort in [Sort::Outside, Sort::Inside] {
        let (ink, alpha) = inks.of(sort, true);
        let mut set = false;
        let mut n = 0;
        for (at, row) in profile.rows.iter().enumerate() {
            if inks.sort(market, at) != sort {
                continue;
            }
            let (y_top, y_bottom) = (to_y(row.high), to_y(row.low));
            if !place.shows(y_top, y_bottom) {
                continue;
            }
            if !set {
                colors::set_source_alpha(cr, ink, alpha);
                set = true;
            }
            let baseline = (y_top + y_bottom) / 2.0 + rise;
            for (k, letter) in market.letters_of(at).iter().enumerate() {
                let (left, right) = place.tpo(market, at, k);
                let x = (left + right - advance) / 2.0;
                batch[n] = cairo::Glyph::new(glyphs[*letter as usize % glyphs.len()], x, baseline);
                n += 1;
                if n == BATCH {
                    let _ = cr.show_glyphs(&batch);
                    n = 0;
                }
            }
        }
        if n > 0 {
            let _ = cr.show_glyphs(&batch[..n]);
        }
    }
    cr.restore().ok();
}

/// The glyph of each letter in the face `cr` has selected, looked up the
/// first time and kept.
fn glyphs(cr: &cairo::Context) -> Option<[c_ulong; 52]> {
    if let Some(known) = GLYPHS.get() {
        return Some(known);
    }
    let text = std::str::from_utf8(omacharts_engine::indicators::market::LETTERS).ok()?;
    let (found, _) = cr.scaled_font().text_to_glyphs(0.0, 0.0, text).ok()?;
    let mut table: [c_ulong; 52] = [0; 52];
    if found.len() != table.len() {
        return None;
    }
    for (slot, glyph) in table.iter_mut().zip(&found) {
        *slot = glyph.index();
    }
    GLYPHS.set(Some(table));
    Some(table)
}
