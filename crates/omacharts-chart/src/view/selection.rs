//! Picking an indicator off the chart with the pointer.
//!
//! Worked out from the same frame the chart is drawn from, so what a click
//! lands on is what was drawn under it.

use gtk::cairo;
use omacharts_engine::drawings::distance_to_segment;
use omacharts_engine::theme::Mode;
use omacharts_engine::Theme;

use crate::colors;
use omacharts_engine::indicators::profile::{self, Align, Style};
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
        if !self.shown_indicators().any(|d| d.indicator.kind.selectable()) {
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
        let pickable = || self.shown_indicators().rev().filter(|d| d.indicator.kind.selectable());
        match row.pane {
            None => {
                let to_y = |price: f64| frame.scale.to_y(price, frame.plan.price_y, frame.plan.price_h);
                pickable()
                    .find(|d| on_overlay_line(self, d, &frame.columns, &to_y, x, y))
                    .or_else(|| pickable().find(|d| on_overlay_shape(self, d, &frame.columns, &to_y, x, y)))
                    .map(|d| d.indicator.id)
            }
            Some(id) => {
                let drawn = pickable().find(|d| d.indicator.id == id)?;
                on_strip(drawn, &frame, row, x, y).then_some(id)
            }
        }
    }

    /// The profile of a volume profile indicator whose period is under `x`:
    /// where its periods start, and whether there is one before it on the
    /// chart to merge it with.
    pub(super) fn profile_at(&self, width: f64, height: f64, id: u32, x: f64) -> Option<ProfileSpot> {
        let frame = frame(self, width, height)?;
        let drawn = self.shown_indicators().find(|d| d.indicator.id == id)?;
        let Output::Profiles(profiles) = &drawn.output else { return None };
        let columns = &frame.columns;
        let at = profiles.iter().position(|p| {
            (columns.left_of(p.first_bar)..columns.left_of(p.last_bar + 1)).contains(&x)
        })?;
        Some(ProfileSpot { parts: profiles[at].parts.clone(), has_previous: at > 0 })
    }

    /// The profile of a volume profile indicator holding the moment `ts`: the
    /// last one to begin at or before it.
    pub(super) fn profile_of(&self, id: u32, ts: i64) -> Option<ProfileSpot> {
        let drawn = self.indicators.iter().find(|d| d.indicator.id == id)?;
        let Output::Profiles(profiles) = &drawn.output else { return None };
        // The one holding the moment; else one starting within the day after
        // it, since a day typed in a clock east of the exchange's is read
        // as the evening before its session; else the last to start before.
        let at = profiles
            .iter()
            .position(|p| p.start_ts <= ts && ts <= p.end_ts)
            .or_else(|| profiles.iter().position(|p| p.start_ts > ts && p.start_ts < ts + 86_400))
            .or_else(|| profiles.iter().rposition(|p| p.start_ts <= ts))?;
        Some(ProfileSpot { parts: profiles[at].parts.clone(), has_previous: at > 0 })
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
fn on_overlay_line(state: &State, drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64, x: f64, y: f64) -> bool {
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
        // The point of control is its line, while it is drawn.
        Output::Profiles(profiles) => poc_shown(drawn) && profiles.iter().any(|profile| {
            let area = profile_box(profile, columns, &placed_look(state, drawn));
            let poc = to_y(profile.poc_price());
            area.span > 0.0 && area.covers(x, 1.0) && (y - poc).abs() <= LINE_REACH
        }),
        Output::Volume { .. } | Output::Pane(_) | Output::Time(_) | Output::Unmet(_) => false,
    }
}

/// Whether a volume profile draws its point of control.
fn poc_shown(drawn: &Drawn) -> bool {
    !matches!(drawn.indicator.params, omacharts_engine::Params::VolumeProfile { poc: false, .. })
}

/// Inside one of the shapes an indicator draws over the price: a volume
/// profile's rows. Not a band's fill, which covers so much of the chart that
/// a press anywhere in it would pick the band rather than pan.
fn on_overlay_shape(state: &State, drawn: &Drawn, columns: &Columns, to_y: &impl Fn(f64) -> f64, x: f64, y: f64) -> bool {
    let Output::Profiles(profiles) = &drawn.output else { return false };
    let look = placed_look(state, drawn);
    profiles.iter().any(|profile| {
        let area = profile_box(profile, columns, &look);
        if area.span <= 0.0 || profile.max_volume <= 0.0 {
            return false;
        }
        profile.rows.iter().any(|row| {
            let top = to_y(row.high);
            let bottom = top + (to_y(row.low) - top).max(1.0);
            row.volume != 0.0 && area.covers(x, row.volume.abs() / profile.max_volume) && (top..=bottom).contains(&y)
        })
    })
}

/// One profile of a volume profile indicator, as a menu about it needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileSpot {
    /// Where each of its periods starts: more than one for a composite.
    pub parts: Vec<i64>,
    /// Whether a profile before it is on the chart to merge it into.
    pub has_previous: bool,
}

/// How a volume profile indicator is drawn: filled or outlined, from which
/// edge of its period, how much of its room it may take, and where among
/// the others aligned to the same edge it sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ProfileLook {
    pub style: Style,
    pub align: Align,
    /// A share of the room, already inside the range a profile is drawn at.
    pub width: f64,
    /// The widths of the profiles on screen aligned to the same edge before
    /// this one in the legend, and after it, and of them all with it.
    pub before: f64,
    pub after: f64,
    pub total: f64,
    /// How many of them come after it, and how many there are.
    pub later: usize,
    pub count: usize,
}

/// A profile's look on its own, as if no other were beside it.
pub(super) fn profile_look(drawn: &Drawn) -> ProfileLook {
    let (style, align, width) = match &drawn.indicator.params {
        omacharts_engine::Params::VolumeProfile { style, align, width, .. } => {
            (*style, *align, profile::drawn_width(*width))
        }
        _ => (Style::Filled, Align::Left, profile::DEFAULT_WIDTH),
    };
    ProfileLook { style, align, width, before: 0.0, after: 0.0, total: width, later: 0, count: 1 }
}

/// A volume profile on screen with profiles to draw, aligned to `align`.
fn stacks(drawn: &Drawn, align: Align) -> Option<f64> {
    match (&drawn.indicator.params, &drawn.output) {
        (omacharts_engine::Params::VolumeProfile { align: a, width, .. }, Output::Profiles(profiles))
            if *a == align && !profiles.is_empty() =>
        {
            Some(profile::drawn_width(*width))
        }
        _ => None,
    }
}

/// A profile's look among the others on screen aligned to the same edge:
/// side by side in the order the legend lists them, so none is drawn over
/// another.
pub(super) fn placed_look(state: &State, drawn: &Drawn) -> ProfileLook {
    let mut look = profile_look(drawn);
    let (mut before, mut after, mut later, mut count, mut seen) = (0.0, 0.0, 0, 0, false);
    for other in state.shown_indicators() {
        if other.indicator.id == drawn.indicator.id {
            seen = true;
            count += 1;
            continue;
        }
        let Some(width) = stacks(other, look.align) else { continue };
        count += 1;
        match seen {
            false => before += width,
            true => {
                after += width;
                later += 1;
            }
        }
    }
    look.before = before;
    look.after = after;
    look.total = before + look.width + after;
    look.later = later;
    look.count = count.max(1);
    look
}

/// Of the plot, how much a profile at 100% takes past the last bar.
pub(super) const SLOT_SHARE: f64 = 0.25;
/// Of the plot, the space before each profile past the last bar: off the
/// last bar, and off the one before it.
pub(super) const SLOT_GAP_SHARE: f64 = 0.01;
/// The most of the plot the profiles past the last bar may take between
/// them, so a stack of wide ones still leaves the bars most of it.
pub(super) const MARGIN_MAX_SHARE: f64 = 0.6;

/// How much of the plot the empty space past the last bar has to be for
/// the live session of every right-aligned profile on screen to sit in it
/// side by side, each as wide as its width says: none when there is none.
pub(super) fn margin_share(state: &State) -> f64 {
    let len = state.bars.len();
    let (mut total, mut count) = (0.0, 0usize);
    for drawn in state.shown_indicators() {
        let Some(width) = stacks(drawn, Align::Right) else { continue };
        let Output::Profiles(profiles) = &drawn.output else { continue };
        if profiles.last().is_some_and(|p| p.last_bar + 1 == len) {
            total += width;
            count += 1;
        }
    }
    match count {
        0 => 0.0,
        _ => (total * SLOT_SHARE + count as f64 * SLOT_GAP_SHARE).min(MARGIN_MAX_SHARE),
    }
}

/// Where a profile is drawn across: the edge its rows grow from, the width
/// its longest row reaches, and which way they grow.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ProfileBox {
    pub edge: f64,
    pub span: f64,
    /// One growing rightward from the left edge, minus one leftward from the
    /// right.
    pub towards: f64,
}

impl ProfileBox {
    /// Where a row reaching `share` of the longest one begins and ends,
    /// left to right.
    pub fn row(&self, share: f64) -> (f64, f64) {
        let end = self.edge + self.towards * self.span * share.max(0.0);
        (self.edge.min(end), self.edge.max(end))
    }

    /// Whether `x` is across a row reaching `share` of the longest.
    pub fn covers(&self, x: f64, share: f64) -> bool {
        let (from, to) = self.row(share);
        (from..=to).contains(&x)
    }
}

/// The least empty space past the last bar that a right-aligned profile is
/// moved into. Narrower, the profile at any width would be a sliver beside
/// the price axis, and over its own bars it is at least readable.
pub(super) const PROFILE_ROOM_MIN: f64 = 48.0;

/// Where a profile is drawn across, the arithmetic the profile is drawn
/// with. Nothing for one off screen.
///
/// A share of its period's width, from the edge it is aligned to — except
/// the period still forming at the live edge, aligned right: that one is
/// drawn back from the plot's right edge across the empty space past the
/// last bar, so it describes the session without sitting on the bars it is
/// about. Past periods keep the end of their own, which is where the next
/// one starts: the profiles read as a row of them, each beside the session
/// it describes.
///
/// Several aligned to the same edge sit side by side in it, in the order
/// the legend lists them, each in a slot as wide as its width says, all of
/// them squeezed alike when their widths come to more than the room.
pub(super) fn profile_box(profile: &Profile, columns: &Columns, look: &ProfileLook) -> ProfileBox {
    let (first, last) = (columns.first, columns.first + columns.visible);
    if profile.last_bar < first || profile.first_bar >= last {
        return ProfileBox { edge: 0.0, span: 0.0, towards: 1.0 };
    }
    let from = profile.first_bar.max(first);
    let to = profile.last_bar.min(last - 1) + 1;
    let width = columns.width_of(from, to).max(columns.bar_w);
    // Of the period, what a share of one is once they all fit in it.
    let share = width / look.total.max(1.0);
    let span = share * look.width;
    match look.align {
        Align::Left => ProfileBox { edge: columns.left_of(from) + share * look.before, span, towards: 1.0 },
        Align::Right => {
            let room = columns.room_after();
            match profile.last_bar + 1 == last && room >= PROFILE_ROOM_MIN {
                true => {
                    let (edge, span) = in_margin(columns, room, look);
                    ProfileBox { edge, span, towards: -1.0 }
                }
                false => ProfileBox { edge: columns.left_of(from) + width - share * look.after, span, towards: -1.0 },
            }
        }
    }
}

/// Where in the empty space past the last bar a right-aligned profile's
/// live session is drawn back from, and how far it reaches.
///
/// Each is given a gap and then its slot, the last in the legend against
/// the price axis. A slot is its width's share of a profile's room at
/// 100%, [`SLOT_SHARE`] of the plot, or of the whole space when a hand has
/// opened more than that, so one profile takes the share of the space its
/// width says, as on its own; slots that come to more than the space are
/// squeezed alike until they fit.
fn in_margin(columns: &Columns, room: f64, look: &ProfileLook) -> (f64, f64) {
    let plot_w = (columns.end - columns.plot_x).max(1.0);
    let gap = plot_w * SLOT_GAP_SHARE;
    let usable = (room - gap * look.count as f64).max(room * 0.5);
    let total = look.total.max(f64::EPSILON);
    let unit = (usable / total).min((plot_w * SLOT_SHARE).max(usable / total.max(1.0)));
    let edge = columns.end - unit * look.after - gap * look.later as f64;
    (edge, unit * look.width)
}

/// Which of a profile's rows a shape is made of: all of them, for a total,
/// or for a delta those on one side of nothing, each side drawn in its own
/// colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Side {
    All,
    Up,
    Down,
}

impl Side {
    /// How far a row of `volume` reaches in this shape.
    pub(super) fn length(self, volume: f64) -> f64 {
        match self {
            Side::All | Side::Up => volume.max(0.0),
            Side::Down => (-volume).max(0.0),
        }
    }
}

/// The shape a profile's rows make together: along the edge they grow from,
/// and back the stepped way their ends make. Narrower than a few pixels the
/// steps are a smear, and the edge says it alone.
///
/// One side of a delta is a shape for each run of rows on that side, so the
/// edge is not drawn in its colour along rows the other side holds.
pub(super) fn profile_path(
    cr: &cairo::Context,
    area: &ProfileBox,
    profile: &Profile,
    to_y: &dyn Fn(f64) -> f64,
    side: Side,
) {
    let mut rows: Vec<_> = profile.rows.iter().collect();
    rows.sort_by(|a, b| b.high.total_cmp(&a.high));
    if side == Side::All {
        let (top, bottom) = (to_y(rows[0].high), to_y(rows[rows.len() - 1].low));
        cr.move_to(area.edge, top);
        if area.span >= 4.0 {
            for row in &rows {
                let end = area.edge + area.towards * area.span * (side.length(row.volume) / profile.max_volume);
                cr.line_to(end, to_y(row.high));
                cr.line_to(end, to_y(row.low));
            }
        }
        cr.line_to(area.edge, bottom);
        cr.close_path();
        return;
    }
    let mut open: Option<f64> = None;
    for row in &rows {
        let length = side.length(row.volume);
        if length <= 0.0 {
            if let Some(bottom) = open.take() {
                cr.line_to(area.edge, bottom);
                cr.close_path();
            }
            continue;
        }
        if open.is_none() {
            cr.move_to(area.edge, to_y(row.high));
        }
        let reach = if area.span >= 4.0 { area.span * length / profile.max_volume } else { 0.0 };
        let end = area.edge + area.towards * reach;
        cr.line_to(end, to_y(row.high));
        cr.line_to(end, to_y(row.low));
        open = Some(to_y(row.low));
    }
    if let Some(bottom) = open {
        cr.line_to(area.edge, bottom);
        cr.close_path();
    }
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
    glow_path(cr, theme, colour, width, (0..columns.len()).map(point));
}

/// The same glow through any run of points, a `None` breaking it: what an
/// anchored VWAP drawing glows with, so a selected one and a selected study
/// look the same.
pub(super) fn glow_path(
    cr: &cairo::Context,
    theme: &Theme,
    colour: &str,
    width: f64,
    points: impl Iterator<Item = Option<(f64, f64)>>,
) {
    cr.save().ok();
    cr.set_dash(&[], 0.0);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.set_line_width((width + 6.0).max(7.0));
    colors::set_source_alpha(cr, colour, glow_alpha(theme));
    let mut pen_down = false;
    for point in points {
        match point {
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

/// The glow as a disc of `radius`, for a line that is still one point.
pub(super) fn glow_dot(cr: &cairo::Context, theme: &Theme, colour: &str, (x, y): (f64, f64), radius: f64) {
    cr.save().ok();
    colors::set_source_alpha(cr, colour, glow_alpha(theme));
    cr.arc(x, y, radius, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
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

/// The outline of a selected volume profile, the shape its rows on `side`
/// make. Stroked twice, as a wide glow and then a thin line, both in the
/// colour those rows are seen in.
pub(super) fn outline_profile(
    cr: &cairo::Context,
    theme: &Theme,
    colour: &str,
    profile: &Profile,
    area: &ProfileBox,
    to_y: &impl Fn(f64) -> f64,
    side: Side,
) {
    if area.span <= 0.0 || profile.max_volume <= 0.0 || profile.rows.is_empty() {
        return;
    }
    let path = |cr: &cairo::Context| profile_path(cr, area, profile, to_y, side);
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
    use super::{profile, profile_box, profile_look, Profile, ProfileBox, PROFILE_ROOM_MIN};

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

    const DAY: i64 = 86_400;

    /// Two whole sessions of hourly bars and half of a third, still forming,
    /// with a session volume profile on them, shown at rest: the last bar
    /// on screen, and the resting margin open past it.
    fn profiled(align: profile::Align, width: f64) -> State {
        use omacharts_engine::Timeframe;
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        let start = 19_675 * DAY;
        state.bars = (0..60)
            .map(|i| {
                let mid = 100.0 + (i % 7) as f64;
                Bar { ts: start + i * 3600, open: mid, high: mid + 2.0, low: mid - 2.0, close: mid + 1.0, volume: 10.0 + (i % 5) as f64 }
            })
            .collect();
        state.timeframe = Timeframe::hours(1);
        state.visible = 50;
        let mut indicator = Indicator::new(1, Kind::VolumeProfile);
        if let omacharts_engine::Params::VolumeProfile { align: a, width: w, .. } = &mut indicator.params {
            *a = align;
            *w = width;
        }
        let output = omacharts_engine::indicators::compute(&indicator, &state.bars, 0, state.timeframe, None);
        state.indicators = vec![Drawn { indicator, output, color: "#5588ff".to_string() }];
        state
    }

    /// Each profile on screen and the box it is drawn in.
    fn boxes(state: &State) -> Vec<(Profile, ProfileBox, f64, f64)> {
        let frame = frame(state, W, H).expect("a frame");
        let Output::Profiles(profiles) = &state.indicators[0].output else { panic!("profiles") };
        let look = profile_look(&state.indicators[0]);
        profiles
            .iter()
            .map(|p| {
                let area = profile_box(p, &frame.columns, &look);
                let period = (frame.columns.left_of(p.first_bar.max(frame.columns.first)), frame.columns.left_of(p.last_bar + 1));
                (p.clone(), area, period.0, period.1)
            })
            .collect()
    }

    fn last_bar_end(state: &State) -> f64 {
        let frame = frame(state, W, H).expect("a frame");
        frame.columns.left_of(frame.columns.first + frame.columns.visible)
    }

    #[test]
    fn a_right_aligned_session_still_forming_is_drawn_past_the_last_bar() {
        let state = profiled(profile::Align::Right, 0.5);
        let frame = frame(&state, W, H).expect("a frame");
        let room = frame.columns.room_after();
        assert!(room >= PROFILE_ROOM_MIN, "the resting margin is room enough: {room}");
        let all = boxes(&state);
        let (current, area, _, _) = all.last().expect("the live session");
        assert_eq!(current.last_bar, state.bars.len() - 1);
        assert_eq!(area.edge, frame.columns.end, "against the plot's right edge");
        let plot_w = frame.columns.end - frame.columns.plot_x;
        let slot = plot_w * super::SLOT_SHARE * 0.5;
        assert!((area.span - slot).abs() < 1e-9, "half a profile's room at half: {} vs {slot}", area.span);
        let (from, to) = area.row(1.0);
        assert!(from >= last_bar_end(&state) - 1e-9, "clear of the bars: {from}");
        assert!(to <= frame.columns.end + 1e-9);
    }

    /// Shown in the last period only, a profile is the newest period alone,
    /// drawn where it is drawn beside the others: past the last bar aligned
    /// right, with the margin sized for it as it was, and at its own start
    /// aligned left.
    #[test]
    fn a_profile_in_the_last_period_only_is_drawn_where_the_newest_is() {
        for align in [profile::Align::Left, profile::Align::Right] {
            let mut state = profiled(align, 0.5);
            let every = boxes(&state);
            let margin = super::margin_share(&state);
            assert!(every.len() > 1, "{align:?}");
            let drawn = &mut state.indicators[0];
            if let omacharts_engine::Params::VolumeProfile { last_only, .. } = &mut drawn.indicator.params {
                *last_only = true;
            }
            drawn.output = omacharts_engine::indicators::compute(&drawn.indicator, &state.bars, 0, state.timeframe, None);
            let only = boxes(&state);
            assert_eq!(only.len(), 1, "{align:?}");
            assert_eq!(only.last(), every.last(), "{align:?}");
            assert_eq!(super::margin_share(&state), margin, "{align:?}");
        }
    }

    #[test]
    fn a_right_aligned_session_that_is_over_ends_where_it_does() {
        let state = profiled(profile::Align::Right, 0.4);
        let all = boxes(&state);
        for (_, area, start, end) in &all[..all.len() - 1] {
            assert_eq!(area.edge, *end, "at the end of its own period");
            assert!((area.span - (end - start) * 0.4).abs() < 1e-9, "{} of {}", area.span, end - start);
        }
    }

    #[test]
    fn with_no_room_past_the_last_bar_the_live_session_keeps_its_own_end() {
        let mut state = profiled(profile::Align::Right, 0.4);
        state.overhang = 0;
        state.overhang_by_hand = true;
        let all = boxes(&state);
        let (_, area, start, end) = all.last().expect("the live session");
        assert_eq!(area.edge, *end);
        assert!((area.span - (end - start) * 0.4).abs() < 1e-9);

        // Room for only a sliver is no room either.
        state.overhang = 1;
        let (_, area, _, end) = boxes(&state).pop().expect("the live session");
        assert!(frame(&state, W, H).unwrap().columns.room_after() < PROFILE_ROOM_MIN);
        assert_eq!(area.edge, end);
    }

    #[test]
    fn scrolled_back_the_last_session_on_screen_is_not_moved_into_the_margin() {
        let mut state = profiled(profile::Align::Right, 0.4);
        state.anchored = false;
        state.first = 0;
        state.lead = 0;
        let frame = frame(&state, W, H).expect("a frame");
        assert!(frame.columns.room_after() < 1e-9, "bars to the edge");
        for (_, area, _, end) in boxes(&state) {
            assert!(area.edge <= end + 1e-9);
        }
    }

    #[test]
    fn the_width_is_a_share_of_the_room_at_either_edge() {
        // With room opened by hand, past what a profile takes at rest.
        let opened = |align, width| {
            let mut state = profiled(align, width);
            state.overhang = 30;
            state.overhang_by_hand = true;
            state
        };
        for align in profile::Align::ALL {
            let narrow = boxes(&opened(align, 0.2));
            let wide = boxes(&opened(align, 0.8));
            for ((_, a, ..), (_, b, ..)) in narrow.iter().zip(&wide) {
                assert!((b.span - a.span * 4.0).abs() < 1e-6, "{align:?}: {} vs {}", a.span, b.span);
            }
        }
        // Left-aligned, the live session stays on its own bars.
        let state = profiled(profile::Align::Left, 0.4);
        let (_, area, start, _) = boxes(&state).pop().expect("the live session");
        assert_eq!(area.edge, start);
    }

    /// [`profiled`] with a volume profile for each of `looks`, in legend
    /// order.
    fn stacked(looks: &[(profile::Align, f64)]) -> State {
        let mut state = profiled(profile::Align::Left, 0.4);
        state.indicators = looks
            .iter()
            .enumerate()
            .map(|(i, &(align, width))| {
                let mut indicator = Indicator::new(i as u32 + 1, Kind::VolumeProfile);
                if let omacharts_engine::Params::VolumeProfile { align: a, width: w, .. } = &mut indicator.params {
                    *a = align;
                    *w = width;
                }
                let output = omacharts_engine::indicators::compute(&indicator, &state.bars, 0, state.timeframe, None);
                Drawn { indicator, output, color: "#5588ff".to_string() }
            })
            .collect();
        state
    }

    /// Each profile's box for one period, in legend order, as drawn.
    fn period_boxes(state: &State, period: usize) -> Vec<(f64, f64)> {
        let frame = frame(state, W, H).expect("a frame");
        state
            .indicators
            .iter()
            .map(|drawn| {
                let Output::Profiles(profiles) = &drawn.output else { panic!("profiles") };
                let profile = &profiles[period.min(profiles.len() - 1)];
                profile_box(profile, &frame.columns, &super::placed_look(state, drawn)).row(1.0)
            })
            .collect()
    }

    fn apart(boxes: &[(f64, f64)]) -> bool {
        boxes.windows(2).all(|pair| pair[0].1 <= pair[1].0 + 1e-9)
    }

    /// The empty space past the last bar at rest: the resting margin with
    /// no profile aligned right, widened by what each one's live session
    /// takes, and narrowed again as they go. One aligned left adds nothing.
    #[test]
    fn the_margin_is_widened_for_each_profile_aligned_right_and_only_for_them() {
        use profile::Align::{Left, Right};
        let margin = |looks: &[(profile::Align, f64)]| {
            let state = stacked(looks);
            (state.overhang_now(), state.columns())
        };
        let (none, columns) = margin(&[(Left, 0.4)]);
        assert_eq!(none, super::super::RIGHT_MARGIN, "no profile aligned right, no room added");
        assert_eq!(margin(&[]).0, super::super::RIGHT_MARGIN);

        let needed = |widths: f64, count: f64| {
            (columns as f64 * (widths * super::SLOT_SHARE + count * super::SLOT_GAP_SHARE)).ceil() as usize
        };
        let (one, _) = margin(&[(Right, 1.0), (Left, 0.4)]);
        assert_eq!(one, needed(1.0, 1.0));
        let (two, _) = margin(&[(Right, 1.0), (Right, 0.8), (Left, 0.4)]);
        assert_eq!(two, needed(1.8, 2.0));
        // Narrower than the resting margin needs, the resting margin it is.
        assert_eq!(margin(&[(Right, 0.2)]).0, super::super::RIGHT_MARGIN);
        assert!(two > one && one > none, "{none} < {one} < {two}");

        // Taken off, the room goes with it.
        let mut state = stacked(&[(Right, 1.0), (Right, 0.8)]);
        state.indicators.pop();
        assert_eq!(state.overhang_now(), one);
        state.indicators.clear();
        assert_eq!(state.overhang_now(), super::super::RIGHT_MARGIN);
        // However many there are, the bars keep most of the plot.
        let crowd = stacked(&[(Right, 1.0); 5]);
        assert!(crowd.overhang_now() as f64 <= crowd.columns() as f64 * super::MARGIN_MAX_SHARE + 1.0);
    }

    /// Their live sessions sit side by side past the last bar, in legend
    /// order, the last against the price axis, none over another or over
    /// the bars, each as wide as its width says.
    #[test]
    fn right_aligned_live_sessions_stack_past_the_last_bar() {
        use profile::Align::Right;
        let state = stacked(&[(Right, 0.4), (Right, 0.8)]);
        let live = state.bars.len();
        let boxes = period_boxes(&state, live);
        let frame = frame(&state, W, H).expect("a frame");
        assert!(apart(&boxes), "{boxes:?}");
        assert!(boxes[0].0 >= last_bar_end(&state) - 1e-9, "clear of the bars: {boxes:?}");
        assert!((boxes[1].1 - frame.columns.end).abs() < 1e-9, "the last against the axis");
        let (a, b) = (boxes[0].1 - boxes[0].0, boxes[1].1 - boxes[1].0);
        assert!((b - a * 2.0).abs() < 1e-6, "sized by their widths: {a} and {b}");
        // Picked where each is drawn.
        let y = frame.scale.to_y(
            match &state.indicators[0].output {
                Output::Profiles(p) => p.last().unwrap().poc_price(),
                _ => unreachable!(),
            },
            frame.plan.price_y,
            frame.plan.price_h,
        );
        assert_eq!(state.indicator_at(W, H, (boxes[0].0 + boxes[0].1) / 2.0, y), Some(1));
    }

    /// A chart of 2000 hourly bars with two profiles aligned right, 300
    /// columns across, at rest at the live edge.
    fn long_stacked() -> State {
        use profile::Align::Right;
        let mut state = stacked(&[(Right, 1.0), (Right, 0.8)]);
        state.bars = (0..2000)
            .map(|i| {
                let mid = 100.0 + (i % 7) as f64;
                Bar { ts: 19_675 * DAY + i * 3600, open: mid, high: mid + 2.0, low: mid - 2.0, close: mid + 1.0, volume: 10.0 }
            })
            .collect();
        for drawn in &mut state.indicators {
            drawn.output = omacharts_engine::indicators::compute(&drawn.indicator, &state.bars, 0, state.timeframe, None);
        }
        state.visible = 300;
        state
    }

    /// The empty room past the last bar on screen, and a bar's width there.
    fn room_px(state: &State) -> (f64, f64) {
        let frame = frame(state, W, H).expect("a frame");
        let bar = (frame.columns.end - frame.columns.plot_x) / state.columns() as f64;
        (frame.columns.room_after(), bar)
    }

    /// Zooming the bars in and out leaves the room past the last bar as wide
    /// on screen as it was, at rest and as a hand left it alike: the bars
    /// zoom around it, the last bar stays where it was, and the profiles in
    /// it keep their size.
    #[test]
    fn zooming_keeps_the_margin_as_wide_on_screen() {
        for by_hand in [false, true] {
            let mut state = long_stacked();
            let plot_w = frame(&state, W, H).expect("a frame").plan.plot_w;
            if by_hand {
                state.pan_by(-3.0, plot_w);
                assert!(state.overhang_by_hand && state.anchored);
            }
            let (wide, _) = room_px(&state);
            assert!(wide > 0.0);
            for factor in [0.5, 0.5, 4.0, 1.5, 0.25] {
                state.zoom_time(factor, 0.5, plot_w);
                let (now, bar) = room_px(&state);
                assert!((now - wide).abs() <= bar + 1e-6, "by hand {by_hand}, {} columns: {now} vs {wide}", state.columns());
                assert!(state.anchored, "the last bar stays on screen");
            }
        }
    }

    /// Zoomed, then scrolled forward, then back into history and forward to
    /// the live edge again, the room past the last bar comes back as wide
    /// as it was: the view stops at it, rather than running on into more.
    #[test]
    fn scrolling_back_to_the_live_edge_finds_the_margin_as_it_was() {
        let mut state = long_stacked();
        let plot_w = frame(&state, W, H).expect("a frame").plan.plot_w;
        let (wide, _) = room_px(&state);
        for factor in [0.5, 2.5] {
            state.zoom_time(factor, 0.5, plot_w);
            let columns = state.columns() as f64;
            state.pan_by(columns, plot_w);
            let (now, bar) = room_px(&state);
            assert!((now - wide).abs() <= bar + 1e-6, "forward at the edge, x{factor}: {now} vs {wide}");
            state.pan_by(-2.0 * columns, plot_w);
            assert!(!state.anchored, "into history");
            state.pan_by(3.0 * columns, plot_w);
            let (now, bar) = room_px(&state);
            assert!(state.anchored);
            assert!((now - wide).abs() <= bar + 1e-6, "back at the edge, x{factor}: {now} vs {wide}");
            assert!(!state.overhang_by_hand, "at rest there, so it follows the profiles");
        }
    }

    /// A hand that drags the room shut is not argued with: the margin is
    /// what it left until the view is reset or the hand brings it back to
    /// the profiles' margin, which it does not drag past.
    #[test]
    fn a_hand_on_the_margin_is_left_where_it_put_it() {
        use profile::Align::Right;
        let mut state = stacked(&[(Right, 0.4), (Right, 0.8)]);
        let floor = state.overhang_now();
        let plot_w = frame(&state, W, H).expect("a frame").plan.plot_w;
        state.pan_by(5.0, plot_w);
        assert_eq!(state.overhang_now(), floor, "not dragged further open");
        assert!(!state.overhang_by_hand);
        state.pan_by(-((floor - 5) as f64), plot_w);
        assert_eq!(state.overhang_now(), 5, "dragged nearly shut");
        assert!(state.overhang_by_hand);
        state.pan_by(2.0, plot_w);
        assert_eq!(state.overhang_now(), 7, "and part way open again");
        state.reset_view();
        assert_eq!(state.overhang_now(), state.margin_floor().max(super::super::RIGHT_MARGIN));
    }

    /// Profiles aligned to the same edge of a period sit side by side from
    /// that edge, in legend order, and are squeezed alike when their widths
    /// come to more than the period.
    #[test]
    fn profiles_aligned_to_the_same_edge_of_a_period_sit_side_by_side() {
        use profile::Align::{Left, Right};
        let period = 1;
        for looks in [[(Left, 0.4), (Left, 0.5)], [(Left, 0.8), (Left, 0.7)], [(Right, 0.4), (Right, 0.5)], [(Right, 0.9), (Right, 0.9)]] {
            let state = stacked(&looks);
            let boxes = period_boxes(&state, period);
            assert!(apart(&boxes), "{looks:?}: {boxes:?}");
            let (_, _, start, end) = super::tests::boxes_of(&state, period);
            match looks[0].0 {
                Left => assert!((boxes[0].0 - start).abs() < 1e-9, "from the period's start"),
                Right => assert!((boxes[1].1 - end).abs() < 1e-9, "to the period's end"),
            }
            assert!(boxes[0].0 >= start - 1e-9 && boxes[1].1 <= end + 1e-9, "inside the period: {looks:?}");
        }
        // One of each edge is no stack: each has its period's edge.
        let state = stacked(&[(Left, 0.4), (Right, 0.4)]);
        let boxes = period_boxes(&state, period);
        let (_, _, start, end) = super::tests::boxes_of(&state, period);
        assert!((boxes[0].0 - start).abs() < 1e-9 && (boxes[1].1 - end).abs() < 1e-9);
    }

    fn boxes_of(state: &State, period: usize) -> (Profile, ProfileBox, f64, f64) {
        let frame = frame(state, W, H).expect("a frame");
        let Output::Profiles(profiles) = &state.indicators[0].output else { panic!("profiles") };
        let p = &profiles[period];
        let area = profile_box(p, &frame.columns, &super::placed_look(state, &state.indicators[0]));
        (p.clone(), area, frame.columns.left_of(p.first_bar.max(frame.columns.first)), frame.columns.left_of(p.last_bar + 1))
    }

    #[test]
    fn a_stored_width_out_of_range_is_drawn_inside_it() {
        let state = profiled(profile::Align::Left, 7.0);
        assert_eq!(profile_look(&state.indicators[0]).width, profile::MAX_WIDTH);
    }

    /// A selected profile is outlined in the colour it is seen in: its rows'
    /// when filled, its outline's in the outline style, where the rows'
    /// colour is drawn nowhere.
    #[test]
    fn a_selected_profile_is_outlined_in_the_colour_it_is_drawn_in() {
        use omacharts_engine::theme::ColorChoice;
        use profile::Method;
        let state = profiled(profile::Align::Left, 0.4);
        let drawn = &state.indicators[0];
        let amber = ColorChoice::Fixed { hex: "#ffaa00".to_string() };
        let inks = super::super::profile_inks(Method::Total, Some(&amber), drawn, &state);
        assert_eq!(inks.len(), 1);
        assert_eq!(inks[0].shown(profile::Style::Outline), "#ffaa00");
        assert_eq!(inks[0].shown(profile::Style::Filled), drawn.color);
        // With no outline colour chosen, the outline is the profile's own.
        let unset = super::super::profile_inks(Method::Total, None, drawn, &state);
        assert_eq!(unset[0].shown(profile::Style::Outline), drawn.color);
        // A delta is seen, and outlined when selected, in both its colours.
        let delta = super::super::profile_inks(Method::Delta, None, drawn, &state);
        let shown: Vec<&str> = delta.iter().map(|ink| ink.shown(profile::Style::Filled)).collect();
        let (up, down) = &state.delta_ink;
        assert_eq!(shown, vec![up.as_str(), down.as_str()]);
        let outlined = super::super::profile_inks(Method::Delta, Some(&amber), drawn, &state);
        assert!(outlined.iter().all(|ink| ink.shown(profile::Style::Outline) == "#ffaa00"));
    }

    /// Bought rows are drawn in the colour bars rise in and sold rows in the
    /// colour they fall in, of the look chosen in Preferences, whatever this
    /// chart's candles wear, and as that look changes.
    #[test]
    fn a_delta_profile_is_drawn_in_the_colours_bars_rise_and_fall_in() {
        use omacharts_engine::BarLook;
        use profile::Method;
        use super::Side;
        let mut state = profiled(profile::Align::Left, 0.4);
        let drawn = state.indicators[0].clone();
        let fills = |state: &State| -> Vec<(Side, String)> {
            super::super::profile_inks(Method::Delta, None, &drawn, state)
                .into_iter()
                .map(|ink| (ink.side, ink.fill))
                .collect()
        };
        let worn = |state: &State, house: &BarLook| {
            let scheme = house.scheme(&state.theme);
            vec![(Side::Up, scheme.up), (Side::Down, scheme.down)]
        };
        assert_ne!(state.scheme.up, state.scheme.down);
        assert_eq!(fills(&state), worn(&state, &BarLook::default()), "the house look to begin with");

        // The chart's own candles are not what it follows.
        state.scheme = BarLook::monochrome().scheme(&state.theme);
        assert_eq!(fills(&state), worn(&state, &BarLook::default()), "not the chart's");

        for theme in omacharts_engine::theme::builtin_themes() {
            state.theme = theme;
            for house in [BarLook::default(), BarLook::red_up(), BarLook::monochrome()] {
                state.house = house.clone();
                state.delta_ink = super::super::delta_ink(&state.theme, &state.house);
                assert_eq!(fills(&state), worn(&state, &house), "{house:?}");
            }
        }
    }

    /// Each side of a delta reaches as far as its rows do on that side and
    /// not at all on the other's; a total is every row it has.
    #[test]
    fn each_side_of_a_delta_is_its_own_rows() {
        use super::Side;
        assert_eq!([Side::Up.length(3.0), Side::Up.length(-3.0)], [3.0, 0.0]);
        assert_eq!([Side::Down.length(3.0), Side::Down.length(-3.0)], [0.0, 3.0]);
        assert_eq!([Side::All.length(3.0), Side::All.length(0.0)], [3.0, 0.0]);
    }

    /// A point of control turned off is not a line to pick the profile by;
    /// turned back on, it is again.
    #[test]
    fn a_hidden_point_of_control_is_no_line_to_pick() {
        let mut state = profiled(profile::Align::Left, 0.6);
        let on_its_line = |state: &State| {
            let frame = frame(state, W, H).expect("a frame");
            let to_y = |price: f64| frame.scale.to_y(price, frame.plan.price_y, frame.plan.price_h);
            let (profile, area, _, _) = boxes(state).pop().expect("a period");
            let (from, to) = area.row(1.0);
            super::on_overlay_line(state, &state.indicators[0], &frame.columns, &to_y, (from + to) / 2.0, to_y(profile.poc_price()))
        };
        assert!(on_its_line(&state), "drawn, it is a line");
        for shown in [false, true] {
            if let omacharts_engine::Params::VolumeProfile { poc, .. } = &mut state.indicators[0].indicator.params {
                *poc = shown;
            }
            assert_eq!(on_its_line(&state), shown, "shown {shown}");
        }
    }

    /// What is drawn in the margin is what a click there picks.
    #[test]
    fn the_profile_in_the_margin_is_picked_where_it_is_drawn() {
        let state = profiled(profile::Align::Right, 0.6);
        let frame = frame(&state, W, H).expect("a frame");
        let (current, area, _, _) = boxes(&state).pop().expect("the live session");
        let y = frame.scale.to_y(current.poc_price(), frame.plan.price_y, frame.plan.price_h);
        let (from, to) = area.row(1.0);
        assert_eq!(state.indicator_at(W, H, (from + to) / 2.0, y), Some(1));
        assert_eq!(state.indicator_at(W, H, from - 30.0, y), None, "left of it, over the last bars");
    }
}
