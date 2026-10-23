//! Picking an indicator off the chart with the pointer.
//!
//! Worked out from the same frame the chart is drawn from, so what a click
//! lands on is what was drawn under it.

use gtk::cairo;
use omacharts_engine::drawings::distance_to_segment;
use omacharts_engine::theme::Mode;
use omacharts_engine::Theme;

use crate::ui::colors;
use omacharts_engine::indicators::{Output, Pane, Profile};

use super::{frame, layout, on_screen, pane_plot, pane_range, Columns, Drawn, Frame, Row, State};

/// How near a line the pointer has to be to pick it: a little less than a
/// drawing's reach, since a line here is one of several sharing the bars.
const LINE_REACH: f64 = 5.0;

/// How far clear of every drawing a pick on an indicator has to be. More than
/// a drawing's own reach, so a near miss on a drawing is not taken for a hit
/// on what is under it: the drawing is what the hand was most likely after,
/// and the indicator can be picked anywhere else along its length.
const DRAWING_CLEARANCE: f64 = 14.0;

impl State {
    /// The indicator under a pixel, if a click there should pick one.
    ///
    /// Lines before shapes, since a line drawn over a profile is the smaller
    /// target and would otherwise never be reachable; and among each, the
    /// one drawn last first, as it is the one on top.
    ///
    /// It runs on every motion of the pointer, for the cursor, so what is
    /// cheap is asked first: whether there is anything to pick, and whether
    /// the pointer is on the plot at all, before the frame is worked out.
    pub(super) fn indicator_at(&self, width: f64, height: f64, x: f64, y: f64) -> Option<u32> {
        if !self.indicators.iter().any(|d| d.indicator.visible && d.indicator.kind.selectable()) {
            return None;
        }
        let plan = layout(self, width, height);
        if x < plan.plot_x || x > plan.plot_x + plan.plot_w || !plan.rows.iter().any(|row| row.covers(y)) {
            return None;
        }
        let frame = frame(self, width, height)?;
        if self.drawing_near(&frame, x, y) {
            return None;
        }
        let row = frame.plan.rows.iter().find(|row| row.covers(y))?;
        if x < frame.plan.plot_x || x > frame.plan.plot_x + frame.plan.plot_w {
            return None;
        }
        let pickable = || {
            self.indicators
                .iter()
                .rev()
                .filter(|d| d.indicator.visible && d.indicator.kind.selectable())
        };
        match row.pane {
            None => {
                let to_y = |price: f64| frame.scale.to_y(price, frame.plan.price_y, frame.plan.price_h);
                pickable()
                    .find(|d| on_overlay_line(d, &frame.columns, &to_y, x, y))
                    .or_else(|| pickable().find(|d| on_overlay_shape(d, &frame.columns, &to_y, x, y)))
                    .map(|d| d.indicator.id)
            }
            Some(id) => {
                let drawn = pickable().find(|d| d.indicator.id == id)?;
                on_strip(drawn, &frame, row, x, y).then_some(id)
            }
        }
    }

    /// Whether a drawing is within the clearance of a pixel.
    fn drawing_near(&self, frame: &Frame, x: f64, y: f64) -> bool {
        let (plan, scale) = (&frame.plan, &frame.scale);
        let in_price = y >= plan.price_y && y <= plan.price_y + plan.price_h;
        in_price
            && self
                .drawings
                .iter()
                .any(|drawing| self.project(plan, scale, drawing).near(x, y, DRAWING_CLEARANCE))
    }
}

/// On one of the lines an indicator draws over the price.
fn on_overlay_line(drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64, x: f64, y: f64) -> bool {
    match &drawn.output {
        Output::Line(values) => {
            !drawn.indicator.stroke.is_hidden() && near_series(values, columns, Indexing::Series, to_y, x, y)
        }
        Output::Bands(bands) => {
            let centre = !drawn.indicator.stroke.is_hidden()
                && near_series(&bands.center, columns, Indexing::Series, to_y, x, y);
            centre
                || bands.bands.iter().filter(|b| b.enabled && !b.stroke.is_hidden()).any(|band| {
                    near_series(&band.upper, columns, Indexing::Series, to_y, x, y)
                        || near_series(&band.lower, columns, Indexing::Series, to_y, x, y)
                })
        }
        Output::Profiles(profiles) => profiles.iter().any(|profile| {
            let (left, span) = profile_box(profile, columns);
            let poc = to_y(profile.poc_price());
            span > 0.0 && (left..=left + span).contains(&x) && (y - poc).abs() <= LINE_REACH
        }),
        Output::Volume { .. } | Output::Pane(_) | Output::Time(_) => false,
    }
}

/// Inside one of the shapes an indicator draws over the price: a volume
/// profile's rows. Not a band's fill, which covers so much of the chart that
/// a press anywhere in it would pick the band rather than pan.
fn on_overlay_shape(drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64, x: f64, y: f64) -> bool {
    let Output::Profiles(profiles) = &drawn.output else { return false };
    profiles.iter().any(|profile| {
        let (left, span) = profile_box(profile, columns);
        if span <= 0.0 || profile.max_volume <= 0.0 {
            return false;
        }
        profile.rows.iter().any(|row| {
            let width = span * (row.volume / profile.max_volume);
            let top = to_y(row.high);
            let bottom = top + (to_y(row.low) - top).max(1.0);
            row.volume > 0.0 && (left..=left + width).contains(&x) && (top..=bottom).contains(&y)
        })
    })
}

/// Where a profile is drawn across: its left edge and the width its longest
/// row reaches, the arithmetic the profile is drawn with. Nothing for one off
/// screen.
pub(super) fn profile_box(profile: &Profile, columns: &Columns) -> (f64, f64) {
    let (first, last) = (columns.first, columns.first + columns.visible);
    if profile.last_bar < first || profile.first_bar >= last {
        return (0.0, 0.0);
    }
    let from = profile.first_bar.max(first);
    let to = profile.last_bar.min(last - 1) + 1;
    // A profile may use at most this much of its own period's width, so it
    // describes the bars rather than burying them.
    (columns.left_of(from), columns.width_of(from, to).max(columns.bar_w) * 0.4)
}

/// On what an indicator draws in its own strip.
fn on_strip(drawn: &Drawn, frame: &Frame, row: &Row, x: f64, y: f64) -> bool {
    let columns = &frame.columns;
    match &drawn.output {
        Output::Volume { .. } => {
            if frame.max_volume <= 0.0 {
                return false;
            }
            let (inner_top, inner_h) = (row.top + row.height * 0.1, row.height * 0.9);
            column_at(columns, x).is_some_and(|at| {
                let bar = &columns.bars[at];
                let h = (bar.volume / frame.max_volume * inner_h).max(1.0);
                bar.volume > 0.0 && y >= inner_top + inner_h - h - 1.0
            })
        }
        Output::Pane(pane) => on_pane(drawn, pane, columns, row, x, y),
        _ => false,
    }
}

/// On a strip's lines, or its histogram.
fn on_pane(drawn: &Drawn, pane: &Pane, columns: &Columns, row: &Row, x: f64, y: f64) -> bool {
    let (first, visible) = (columns.first, columns.visible);
    let Some((low, high)) = pane_range(pane, first, visible) else { return false };
    let (inner_top, inner_h) = pane_plot(row.top, row.height);
    let to_y = |value: f64| {
        (inner_top + inner_h * (high - value) / (high - low)).clamp(row.top, row.top + row.height)
    };
    let lines = !drawn.indicator.stroke.is_hidden()
        && [Some(&pane.values), pane.signal.as_ref()].into_iter().flatten().any(|series| {
            near_series(on_screen(series, first, visible), columns, Indexing::Screen, &to_y, x, y)
        });
    lines
        || pane.histogram.as_ref().is_some_and(|histogram| {
            let values = on_screen(histogram, first, visible);
            column_at(columns, x).is_some_and(|at| {
                let Some(Some(value)) = values.get(columns.offset(at)) else { return false };
                let (bar, base) = (to_y(*value), to_y(0.0));
                value.is_finite() && *value != 0.0 && y >= bar.min(base) - 1.0 && y <= bar.max(base) + 1.0
            })
        })
}

/// How a series is read by column: by the bar's place in the whole series, as
/// a line over the price is, or by its place on screen, as a strip's is.
#[derive(Clone, Copy)]
enum Indexing {
    Series,
    Screen,
}

/// Whether a series, drawn as a line through the columns, passes within
/// reach of a pixel. Only the columns near it are tried.
fn near_series(
    values: &[Option<f64>],
    columns: &Columns,
    indexing: Indexing,
    to_y: &impl Fn(f64) -> f64,
    x: f64,
    y: f64,
) -> bool {
    let value = |at: usize| match indexing {
        Indexing::Series => values.get(columns.source(at)).copied().flatten(),
        Indexing::Screen => values.get(columns.offset(at)).copied().flatten(),
    };
    let point = |at: usize| value(at).map(|v| (columns.x(at), to_y(v)));
    let Some(near) = column_at(columns, x) else { return false };
    // The segments either side of the column under the pointer, and the next
    // ones out, which at a few pixels a column are still within reach.
    let from = near.saturating_sub(2);
    let to = (near + 2).min(columns.len().saturating_sub(1));
    (from..=to).any(|at| {
        let Some(here) = point(at) else { return false };
        let alone = (here.0 - x).hypot(here.1 - y) <= LINE_REACH;
        let joined = point(at + 1)
            .filter(|_| at < to)
            .is_some_and(|next| distance_to_segment((x, y), here, next) <= LINE_REACH);
        alone || joined
    })
}

/// The column under a pixel across the plot, if any.
fn column_at(columns: &Columns, x: f64) -> Option<usize> {
    let count = columns.len();
    if count == 0 {
        return None;
    }
    // Columns are evenly spaced, so the nearest middle is a division away;
    // the neighbours are tried too, for the rounding at a pixel a column.
    let pitch = if count > 1 { columns.x(1) - columns.x(0) } else { 1.0 };
    let guess = ((x - columns.x(0)) / pitch.max(f64::EPSILON)).round();
    let guess = guess.clamp(0.0, (count - 1) as f64) as usize;
    let candidates = guess.saturating_sub(1)..=(guess + 1).min(count - 1);
    candidates
        .min_by(|a, b| (columns.x(*a) - x).abs().total_cmp(&(columns.x(*b) - x).abs()))
        .filter(|at| (columns.x(*at) - x).abs() <= pitch.max(1.0) / 2.0 + LINE_REACH)
}

// ---------------------------------------------------------------------------
// Showing what is selected
// ---------------------------------------------------------------------------
//
// A selected indicator glows. It never grows grips, which say "take hold of
// this" — and an indicator cannot be moved or reshaped, so grips on it would
// be a promise the chart does not keep. The glow is the indicator's own
// colour, wide and faint, painted just under it, so the indicator keeps the
// colour and width it is read by and looks lit.

/// How strong the glow is: fainter on a light theme, where the same colour
/// over white reads stronger.
pub(super) fn glow_alpha(theme: &Theme) -> f64 {
    match theme.mode {
        Mode::Dark => 0.22,
        Mode::Light => 0.16,
    }
}

/// The glow under a line `width` wide, through the points a column at a time.
/// Solid and round whatever the line is, so a dashed line glows unbroken.
pub(super) fn glow_line(
    cr: &cairo::Context,
    theme: &Theme,
    colour: &str,
    width: f64,
    columns: &Columns,
    point: impl Fn(usize) -> Option<(f64, f64)>,
) {
    cr.save().ok();
    cr.set_dash(&[], 0.0);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_line_width((width + 6.0).max(7.0));
    colors::set_source_alpha(cr, colour, glow_alpha(theme));
    let mut pen_down = false;
    for at in 0..columns.len() {
        match point(at) {
            Some((x, y)) if pen_down => cr.line_to(x, y),
            Some((x, y)) => {
                cr.move_to(x, y);
                pen_down = true;
            }
            None => pen_down = false,
        }
    }
    let _ = cr.stroke();
    cr.restore().ok();
}

/// A dot at a line's latest value, when that value is on screen: where the
/// line is now, which is what it is read for. A circle rather than a grip's
/// square, as it marks a value and is not a handle. Ringed in the
/// background, so it stands off the line it sits on.
pub(super) fn end_dot(
    cr: &cairo::Context,
    theme: &Theme,
    colour: &str,
    columns: &Columns,
    latest: Option<usize>,
    point: impl Fn(usize) -> Option<(f64, f64)>,
    source: impl Fn(usize) -> usize,
) {
    let Some(latest) = latest else { return };
    let Some(at) = (0..columns.len()).rev().find(|at| point(*at).is_some()) else { return };
    if source(at) < latest {
        return;
    }
    let Some((x, y)) = point(at) else { return };
    cr.save().ok();
    colors::set_source(cr, &theme.ui.background);
    cr.arc(x, y, END_DOT + 1.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    colors::set_source(cr, colour);
    cr.arc(x, y, END_DOT, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    cr.restore().ok();
}

/// The radius of the dot at a selected line's end: a drawing grip's half
/// side, so the two read as one family of marks.
const END_DOT: f64 = 3.5;

/// The latest bar a series has a value for.
pub(super) fn latest(values: &[Option<f64>]) -> Option<usize> {
    values.iter().rposition(Option::is_some)
}

/// The glow round a bar of a histogram or the volume strip: two pixels out on
/// either side and over the top, and none under the baseline it stands on.
pub(super) fn glow_bar(cr: &cairo::Context, (x, top, w, h): (f64, f64, f64, f64), from_top: bool) {
    match from_top {
        true => cr.rectangle(x - BAR_GLOW, top, w + 2.0 * BAR_GLOW, h + BAR_GLOW),
        false => cr.rectangle(x - BAR_GLOW, top - BAR_GLOW, w + 2.0 * BAR_GLOW, h + BAR_GLOW),
    }
}

const BAR_GLOW: f64 = 2.0;

/// The outline of a selected volume profile: down its left edge, and back up
/// the stepped right edge its rows make. Stroked twice, as a wide glow and
/// then a thin line, both in the profile's colour.
pub(super) fn outline_profile(
    cr: &cairo::Context,
    theme: &Theme,
    colour: &str,
    profile: &Profile,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let (left, span) = profile_box(profile, columns);
    if span <= 0.0 || profile.max_volume <= 0.0 || profile.rows.is_empty() {
        return;
    }
    let mut rows: Vec<_> = profile.rows.iter().collect();
    rows.sort_by(|a, b| b.high.total_cmp(&a.high));
    let (top, bottom) = (to_y(rows[0].high), to_y(rows[rows.len() - 1].low));
    let path = |cr: &cairo::Context| {
        cr.move_to(left, top);
        // Narrower than this, the steps are a smear: the left edge says it.
        if span >= 4.0 {
            for row in &rows {
                let right = left + span * (row.volume.max(0.0) / profile.max_volume);
                cr.line_to(right, to_y(row.high));
                cr.line_to(right, to_y(row.low));
            }
        }
        cr.line_to(left, bottom);
        cr.close_path();
    };
    cr.save().ok();
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_line_width(5.0);
    colors::set_source_alpha(cr, colour, glow_alpha(theme));
    path(cr);
    let _ = cr.stroke();
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, colour, 0.7);
    path(cr);
    let _ = cr.stroke();
    cr.restore().ok();
}

#[cfg(test)]
mod tests {
    use omacharts_engine::drawings::{Anchor, Drawing, Kind as DrawingKind};
    use omacharts_engine::indicators::{Kind, Output};
    use omacharts_engine::{Bar, Indicator};

    use super::super::{frame, Drawn, State};

    const W: f64 = 800.0;
    const H: f64 = 600.0;

    /// Forty bars and an indicator drawn flat across them at 105.
    fn charted(kind: Kind) -> State {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.bars = (0..40)
            .map(|i| Bar {
                ts: 1_700_000_000 + i * 3600,
                open: 100.0,
                high: 110.0,
                low: 90.0,
                close: 100.0 + (i % 3) as f64,
                volume: 1.0,
            })
            .collect();
        state.visible = 40;
        state.indicators = vec![Drawn {
            indicator: Indicator::new(1, kind),
            output: Output::Line(vec![Some(105.0); 40]),
            color: "#5588ff".to_string(),
        }];
        state
    }

    /// Where the flat line is drawn, at the middle bar.
    fn on_line(state: &State) -> (f64, f64) {
        let frame = frame(state, W, H).expect("a frame");
        let y = frame.scale.to_y(105.0, frame.plan.price_y, frame.plan.price_h);
        (frame.columns.x(20), y)
    }

    #[test]
    fn a_click_on_a_line_picks_its_indicator_and_one_beside_it_does_not() {
        let state = charted(Kind::Sma);
        let (x, y) = on_line(&state);
        assert_eq!(state.indicator_at(W, H, x, y), Some(1));
        assert_eq!(state.indicator_at(W, H, x, y + 4.0), Some(1), "within reach");
        assert_eq!(state.indicator_at(W, H, x, y + 15.0), None);
    }

    #[test]
    fn a_hidden_indicator_and_session_shading_are_never_picked() {
        let mut state = charted(Kind::Sma);
        state.indicators[0].indicator.visible = false;
        let (x, y) = on_line(&state);
        assert_eq!(state.indicator_at(W, H, x, y), None);

        let state = charted(Kind::SessionTimes);
        let (x, y) = on_line(&state);
        assert_eq!(state.indicator_at(W, H, x, y), None);
    }

    /// Drawings come first, and by more than their own reach: near one, the
    /// indicator under it is left alone, and it can still be picked further
    /// along.
    #[test]
    fn a_drawing_nearby_wins_and_the_rest_of_the_line_is_still_reachable() {
        let mut state = charted(Kind::Sma);
        // Around the middle column, counted from the first bar on screen.
        let first = frame(&state, W, H).expect("a frame").columns.first;
        let (from, to) = (state.bars[first + 18].ts, state.bars[first + 22].ts);
        state.drawings = vec![Drawing::new(DrawingKind::Line, Anchor::new(from, 105.2), Anchor::new(to, 105.2))];
        let (x, y) = on_line(&state);
        assert_eq!(state.indicator_at(W, H, x, y), None);

        let frame = frame(&state, W, H).expect("a frame");
        assert_eq!(state.indicator_at(W, H, frame.columns.x(5), y), Some(1));
    }
}
