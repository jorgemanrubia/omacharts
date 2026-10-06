//! The candlestick chart.
//!
//! Two `DrawingArea`s, one over the other, and a cairo draw function each. The
//! view is two numbers — the index of the leftmost visible bar and how many
//! are visible — so panning and zooming are arithmetic, not a re-layout, and a
//! repaint touches only what is on screen.
//!
//! The lower widget is the chart: the bars, the scales, the strips and
//! everything else that changes only when the chart does. The upper one is
//! whatever follows the pointer. They are separate widgets because GTK4
//! invalidates a whole widget or none of it, and a crosshair that moved a
//! pixel would otherwise re-rasterise every candle on screen to redraw two
//! dashed lines. The upper one takes no events — the lower is still the chart
//! you drag, scroll and focus — so the split is a drawing arrangement and
//! nothing else.
//!
//! Candles are drawn in two passes, up and down, each accumulating one cairo
//! path for the wicks and one for the bodies. Two strokes and two fills for a
//! screen of bars rather than four calls per candle, which is what keeps a
//! drag at the frame rate.

pub mod bench;

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use gtk::cairo;
use gtk::prelude::*;
use omacharts_engine::drawings::{
    self, Anchor, Configurations, Drawing, Grip, Kind as DrawingKind, Projected, Sharing,
};
use omacharts_engine::indicators::{self as indicators, vwap, Output, Profile};
use omacharts_engine::{
    Bar, BarScheme, BarStyle, Direction, FetchFailure, Indicator, Instrument, Theme, Timeframe,
};

use crate::ui::colors;
use gtk::glib;

const PRICE_AXIS_W: f64 = 64.0;
const TIME_AXIS_H: f64 = 24.0;
const PAD: f64 = 10.0;
/// Space between two rows of the stack.
const PANE_GAP: f64 = 6.0;
/// The least of the chart the price keeps, however many panes are stacked
/// around it.
///
/// Small on purpose. It is here to stop the price plot reaching zero height,
/// where its own scale stops meaning anything — not to decide how much room a
/// volume pane deserves. That is the chart owner's business, and a tall pane
/// over or under a thin price is a legitimate thing to want.
const MIN_PRICE_SHARE: f64 = 0.08;
/// How near the line between two rows the pointer has to be to grab it.
const EDGE_GRAB: f64 = 4.0;
/// The box each of a pane's corner controls is drawn in.
const PANE_CONTROL: f64 = 13.0;
/// Left edge to left edge of two neighbouring corner controls.
const CONTROL_PITCH: f64 = PANE_CONTROL + 2.0;

/// Fewest bars we will zoom into, and the most we will put on screen at once.
///
/// The ceiling is about how much history a chart will read, not about how much
/// work a frame is: past a bar a pixel the bars are aggregated into columns, so
/// what a frame draws is bounded by the width of the plot however far out the
/// view is zoomed.
const MIN_VISIBLE: usize = 12;
const MAX_VISIBLE: usize = 3000;

/// How far the price scale may be stretched either way.
const MIN_PRICE_ZOOM: f64 = 0.05;
const MAX_PRICE_ZOOM: f64 = 40.0;

/// Pixels of drag for one doubling of either scale.
const DRAG_PER_DOUBLING: f64 = 180.0;

/// One indicator, computed and coloured, ready to draw.
///
/// The chart does no arithmetic: the window computes outputs through the
/// engine and resolves colours through the theme, and this is what arrives.
#[derive(Clone, PartialEq, Debug)]
pub struct Drawn {
    pub indicator: Indicator,
    pub output: Output,
    pub color: String,
}

/// What the crosshair is over, handed to the window for the readout.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hover {
    pub bar: Bar,
    pub index: usize,
    /// The price the pointer is actually at, read off this chart's scale —
    /// not the bar's close. Between two bars at different resolutions the bar
    /// is a different thing on each chart; the price is the same number.
    pub price: f64,
}

/// Another chart's pointer, in the only terms two charts can both read.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Echo {
    pub ts: i64,
    pub price: f64,
}

struct State {
    bars: Vec<Bar>,
    /// Where another chart's pointer is, drawn here as a quieter crosshair.
    echo: Option<Echo>,
    theme: Theme,
    scheme: BarScheme,
    instrument: Option<Instrument>,
    timeframe: Timeframe,
    /// Leftmost visible bar.
    first: usize,
    /// How many bars are visible.
    visible: usize,
    pointer: Option<(f64, f64)>,
    /// Pinned to the right edge, so new bars keep the view at "now" until the
    /// user pans away.
    anchored: bool,
    drag: Option<Drag>,
    indicators: Vec<Drawn>,
    bar_style: BarStyle,
    show_grid: bool,
    /// Why the last fetch for this chart came back with nothing, if it did.
    ///
    /// One piece of state for both of the things that used to be separate —
    /// the corner marker on a chart that has bars, and the line in the middle
    /// of one that has none. They were the same thought said twice, and only
    /// the marker could ever be reached, because a chart with no bars returns
    /// before it is drawn.
    trouble: Option<FetchFailure>,
    loading: bool,
    /// Multiplier on the auto-fitted price range. 1.0 shows exactly what the
    /// visible bars need; above that is zoomed in.
    price_zoom: f64,
    /// Vertical shift, as a fraction of the auto-fitted range.
    price_offset: f64,
    /// While true the price scale follows the data and the two values above
    /// are held at their neutral settings.
    price_auto: bool,
    /// What has been drawn on this symbol, in the order it was drawn. The
    /// chart draws what it is handed; the store is where they live.
    drawings: Vec<Drawing>,
    /// Which of them has the grips, by position in `drawings`.
    /// The drawings that wear grips, in the order they were taken: the last
    /// is the one most recently picked, and what a single-drawing question
    /// is about. Shift with a click adds one or takes one out.
    selected: Vec<usize>,
    /// The tool that is armed: the next press on the plot starts a drawing
    /// of this kind. `None` is the usual state, where a press pans.
    tool: Option<DrawingKind>,
    /// A drawing with its first anchor down and its second following the
    /// pointer. Drawn on the pointer layer, since it moves with the hand;
    /// committed to `drawings` on the second press or at the end of a drag.
    placing: Option<Drawing>,
    /// The configuration the next drawing gets: Alt+N while a tool is armed
    /// chooses it, so Alt+R, Alt+2, click, click draws a rectangle in the
    /// second configuration. Back to the first once the drawing is down or
    /// the tool is put down: the choice was for that drawing.
    next_config: u8,
    /// The nine configurations of each kind, as the window has them: what a
    /// drawing that follows configuration N looks like.
    configs: Configurations,
    /// What this chart shares its drawings with, which is also the scope a
    /// drawing made on it gets.
    sharing: Sharing,
    /// Every change to the drawings, as the list was before it, so Ctrl+Z
    /// puts it back; and what Ctrl+Z took away, so Ctrl+Y can put it back
    /// again. One list rather than one event per kind of change, because a
    /// drag, a property, a configuration and a deletion all come down to
    /// "the drawings were this and are now that".
    undo: Vec<Vec<Drawing>>,
    redo: Vec<Vec<Drawing>>,
}

/// Something that happened to a drawing, for whoever keeps them.
///
/// The chart owns the hand; the window owns the store. A drawing that was
/// added needs a row, one that moved needs its row rewritten, and one that
/// was deleted needs its row gone — and the window, which knows the symbol
/// and the store, is the one that can do any of that.
#[derive(Clone, PartialEq, Debug)]
pub enum DrawingEvent {
    Added(Drawing),
    Changed(Drawing),
    /// Under the hand, between a press and its release: where the drawing
    /// is at this moment, for the other charts that show it to follow
    /// along. Nothing is written down until `Changed`.
    Moving(Drawing),
    Removed(i64),
    /// The whole list, after an undo or a redo: whatever is in the store for
    /// this chart that is not here goes, and whatever is here that is not
    /// there comes back, ids and all.
    Replaced(Vec<Drawing>),
}

/// How many changes back Ctrl+Z can go. Plenty for a session; a bound so
/// that an afternoon of dragging does not keep an afternoon of lists.
const HISTORY: usize = 200;

/// What a drag is doing, decided by where it started.
///
/// This is the whole of the direct-manipulation model: the chart body pans,
/// the price axis scales price, the time axis scales time. Same as every other
/// charting tool, because muscle memory is the feature.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Drag {
    Pan { first: usize, offset: f64 },
    PriceScale { zoom: f64 },
    TimeScale { visible: usize },
    /// Pulling a line between two rows, to make the pane beside it taller or
    /// shorter.
    PaneEdge { id: u32, share: f64, total_h: f64, grows_downward: bool },
    /// Laying a drawing down: the second anchor follows the pointer. A press
    /// that does not travel leaves the drawing waiting for a second press;
    /// one that does finishes it where the hand let go, so both ways of
    /// drawing a line — click, click, and press-drag-release — work.
    Place { moved: bool },
    /// Moving one of a drawing's grips, or the whole of it. `origin` is where
    /// the hand took hold, in the chart's own units, and `before` is the
    /// drawing as it was, so the motion is applied to a fixed starting point
    /// rather than accumulated.
    Grip { index: usize, grip: Grip, origin: Anchor },
}

/// One row of the chart's vertical stack: the price plot, or one indicator's
/// strip.
///
/// The price is a row like any other rather than a fixed first one, because a
/// strip can be moved above it.
struct Row {
    /// The indicator whose strip this is, or `None` for the price plot.
    pane: Option<u32>,
    top: f64,
    height: f64,
}

impl Row {
    /// Is the pointer anywhere in this row?
    fn covers(&self, y: f64) -> bool {
        y >= self.top && y <= self.top + self.height
    }
}

/// One of the boxes a strip wears in its top right corner while the pointer is
/// in it: a way up the stack, a way down it, and the way off the chart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Control {
    Up,
    Down,
    Close,
}

/// A line between two rows, and the strip that line resizes.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Edge {
    id: u32,
    /// Whether pulling the line down is what makes that strip taller. The line
    /// above the price plot belongs to the strip above it, so it grows
    /// downwards; every other line has its strip underneath.
    grows_downward: bool,
}

/// Where everything goes.
///
/// Worked out in one place so the drawing and the pointer cannot disagree. A
/// resize handle that is a pixel off the line it appears to grab is worse than
/// no handle at all.
struct Layout {
    plot_x: f64,
    plot_w: f64,
    /// The top of the whole stack, which is the top of the price plot only
    /// while nothing has been moved above it.
    top: f64,
    total_h: f64,
    /// The price plot's own box, wherever in the stack it ended up.
    price_y: f64,
    price_h: f64,
    rows: Vec<Row>,
}

impl Layout {
    /// Where one of a strip's corner boxes sits: its top right corner, inside
    /// the plot rather than out over the price axis, which belongs to the
    /// price.
    ///
    /// Reading right to left: close, down, up. Every strip reserves all three
    /// places whether or not it offers all three, so the box under the pointer
    /// stays where it is when the strip travels.
    fn control_box(&self, row: &Row, control: Control) -> (f64, f64) {
        let slot = match control {
            Control::Close => 0.0,
            Control::Down => 1.0,
            Control::Up => 2.0,
        };
        let right = self.plot_x + self.plot_w - 4.0;
        (right - PANE_CONTROL - slot * CONTROL_PITCH, row.top + 4.0)
    }

    /// Which boxes the row at `at` offers.
    ///
    /// The arrow at the end of the travel is left out rather than drawn dead.
    /// This app withholds an affordance it cannot honour — there is no Maximize
    /// with one chart — and a greyed arrow invites the click it would refuse.
    fn controls(&self, at: usize) -> Vec<Control> {
        let mut offered = Vec::new();
        if at > 0 {
            offered.push(Control::Up);
        }
        if at + 1 < self.rows.len() {
            offered.push(Control::Down);
        }
        offered.push(Control::Close);
        offered
    }

    /// The strip under this point and which of its boxes, if any.
    ///
    /// One answer for the drawing, the cursor and the click, so none of the
    /// three can disagree about where the boxes are. A box only exists while
    /// the pointer is in the strip it belongs to, so it cannot be clicked by
    /// accident from elsewhere.
    fn control_at(&self, x: f64, y: f64) -> Option<(u32, Control)> {
        self.rows.iter().enumerate().find_map(|(at, row)| {
            let id = row.pane?;
            if !row.covers(y) {
                return None;
            }
            self.controls(at)
                .into_iter()
                .find(|control| {
                    let (bx, by) = self.control_box(row, *control);
                    x >= bx && x <= bx + PANE_CONTROL && y >= by && y <= by + PANE_CONTROL
                })
                .map(|control| (id, control))
        })
    }

    /// The separator above a row, which is also what you grab to resize the
    /// strip beside it. The topmost row has nothing above it.
    fn edge(&self, at: usize) -> Option<(f64, Edge)> {
        if at == 0 {
            return None;
        }
        let row = self.rows.get(at)?;
        let y = row.top - PANE_GAP / 2.0;
        // The line above the price plot resizes the strip above it: the price
        // has no height of its own to change, being whatever the strips leave.
        let edge = match row.pane {
            Some(id) => Edge { id, grows_downward: false },
            None => Edge { id: self.rows[at - 1].pane?, grows_downward: true },
        };
        Some((y, edge))
    }

    fn edge_at(&self, y: f64) -> Option<Edge> {
        (0..self.rows.len())
            .filter_map(|at| self.edge(at))
            .find(|(line, _)| (y - line).abs() <= EDGE_GRAB)
            .map(|(_, edge)| edge)
    }
}

/// The bars as they will be drawn: one per pixel column across the plot.
///
/// A plot 1500 pixels wide showing 3000 bars gives each candle half a pixel,
/// so two of them round to the same column and the second paints over the
/// first. Half the drawing went into pixels that were immediately overwritten,
/// and which of the two candles survived was decided by the order the up and
/// down passes happen to run in rather than by anything about the market.
///
/// So each column is aggregated first: first open, last close, lowest low,
/// highest high, summed volume. Nothing is sampled and nothing is averaged,
/// which is what makes this safe to do to a price chart — a column's wick is
/// exactly the union of the wicks it stands for, so no high and no low can go
/// missing at any zoom. It is the picture the old code was trying to draw, for
/// a bounded amount of work.
///
/// At a bar a pixel or wider there is nothing to aggregate and the visible
/// slice is borrowed as it is, untouched, which is the case every chart at a
/// normal zoom is in — including the 160 bars one opens at.
///
/// Everything that draws goes through this, because an indicator line is
/// indexed by bar and the candles are indexed by column: two places deciding
/// for themselves where bar 2000 is would put the moving average off the
/// candles it describes. The crosshair is the exception, and deliberately —
/// it is on another widget, it answers about a real bar rather than a column,
/// and the two differ by less than the pixel it is drawn on.
struct Columns<'a> {
    /// One bar per column, in column order.
    bars: Cow<'a, [Bar]>,
    /// The first visible bar, which is what series indexed by bar are read
    /// through.
    first: usize,
    /// How many real bars the columns stand for.
    visible: usize,
    plot_x: f64,
    /// One column's width, never less than a pixel.
    bar_w: f64,
}

impl<'a> Columns<'a> {
    /// Divide `plot_w` pixels among `visible` bars, aggregating if there is
    /// more than one bar to a pixel.
    fn of(bars: &'a [Bar], first: usize, visible: usize, plot_x: f64, plot_w: f64) -> Columns<'a> {
        let visible = visible.max(1);
        let columns = (plot_w.floor().max(1.0) as usize).min(visible);
        let bar_w = plot_w / columns as f64;
        if columns >= visible || bars.len() < visible {
            return Columns { bars: Cow::Borrowed(bars), first, visible, plot_x, bar_w };
        }
        let mut aggregated = Vec::with_capacity(columns);
        for at in 0..columns {
            // Integer division, so the columns tile the slice exactly and
            // every one of them gets at least one bar.
            let from = at * visible / columns;
            let to = (at + 1) * visible / columns;
            let group = &bars[from..to];
            let (Some(opening), Some(closing)) = (group.first(), group.last()) else {
                continue;
            };
            let mut column = Bar {
                ts: opening.ts,
                open: opening.open,
                high: opening.high,
                low: opening.low,
                close: closing.close,
                volume: 0.0,
            };
            for bar in group {
                column.high = column.high.max(bar.high);
                column.low = column.low.min(bar.low);
                column.volume += bar.volume;
            }
            aggregated.push(column);
        }
        Columns { bars: Cow::Owned(aggregated), first, visible, plot_x, bar_w }
    }

    fn len(&self) -> usize {
        self.bars.len()
    }

    /// The middle of a column, which is where a candle is centred.
    fn x(&self, at: usize) -> f64 {
        self.plot_x + (at as f64 + 0.5) * self.bar_w
    }

    /// The bar a column is read from when a series is indexed by bar: the last
    /// one in it, which is the bar whose close the column carries.
    fn source(&self, at: usize) -> usize {
        self.first + self.offset(at)
    }

    /// The same, counted from the first visible bar.
    fn offset(&self, at: usize) -> usize {
        ((at + 1) * self.visible / self.len()).saturating_sub(1)
    }

    /// Where a bar's own column begins, in pixels. For the things drawn across
    /// a span of bars rather than at one.
    fn left_of(&self, index: usize) -> f64 {
        self.plot_x + self.column_of(index) * self.bar_w
    }

    /// How wide the bars from `from` up to but not including `to` are.
    fn width_of(&self, from: usize, to: usize) -> f64 {
        (self.column_of(to) - self.column_of(from)) * self.bar_w
    }

    /// Which column a bar falls in, kept fractional: rounding here and again
    /// at the pixel would move a span by a column.
    fn column_of(&self, index: usize) -> f64 {
        index.saturating_sub(self.first) as f64 * self.len() as f64 / self.visible as f64
    }
}

/// The moment at a fractional bar index, on and off the ends of the series.
///
/// Inside the series it is the bar's own timestamp. Beyond either end it
/// continues at the step the end was taking, so an anchor dropped to the
/// right of the last candle is a moment later than it by a whole number of
/// bars — which is what a line drawn into the future has to mean for it to
/// land on the same bar when the bar arrives.
fn ts_at(bars: &[Bar], index: f64) -> i64 {
    let Some(last) = bars.len().checked_sub(1) else { return index as i64 };
    if index <= 0.0 {
        let step = if last > 0 { bars[1].ts - bars[0].ts } else { 0 };
        return bars[0].ts + (index * step as f64).round() as i64;
    }
    if index >= last as f64 {
        let step = if last > 0 { bars[last].ts - bars[last - 1].ts } else { 0 };
        return bars[last].ts + ((index - last as f64) * step as f64).round() as i64;
    }
    let (lo, hi) = (index.floor() as usize, index.ceil() as usize);
    let within = index - lo as f64;
    bars[lo].ts + ((bars[hi].ts - bars[lo].ts) as f64 * within).round() as i64
}

/// The inverse: where a moment falls among the bars, as a fractional index.
///
/// Between two bars it interpolates, so a moment that no bar carries — a
/// daily drawing seen on an hourly chart has those at the weekend — still
/// has a place. Off the ends it extrapolates at the end's step, the way
/// [`ts_at`] does, so the two agree.
fn index_of_ts(bars: &[Bar], ts: i64) -> f64 {
    let Some(last) = bars.len().checked_sub(1) else { return 0.0 };
    let at = bars.partition_point(|b| b.ts < ts);
    if at == 0 {
        let step = if last > 0 { (bars[1].ts - bars[0].ts).max(1) } else { 1 };
        return (ts - bars[0].ts) as f64 / step as f64;
    }
    if at > last {
        let step = if last > 0 { (bars[last].ts - bars[last - 1].ts).max(1) } else { 1 };
        return last as f64 + (ts - bars[last].ts) as f64 / step as f64;
    }
    let (before, after) = (bars[at - 1].ts, bars[at].ts);
    let gap = (after - before).max(1);
    (at - 1) as f64 + (ts - before) as f64 / gap as f64
}

/// The price scale over the visible bars, padded so candles never touch the
/// edges, and then held to whatever vertical window the user has dragged to.
///
/// Shared rather than inlined because the pointer has to answer the same
/// question the drawing does: the price under the crosshair is only the same
/// number on two charts if both worked it out from the same scale.
///
/// `None` when the bars carry nothing finite to measure.
fn price_range(state: &State, bars: &[Bar]) -> Option<(f64, f64)> {
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    for b in bars {
        low = low.min(b.low);
        high = high.max(b.high);
    }
    if !low.is_finite() || !high.is_finite() {
        return None;
    }
    if (high - low).abs() < f64::EPSILON {
        high += 1.0;
        low -= 1.0;
    }
    let span = high - low;
    low -= span * 0.04;
    high += span * 0.04;
    Some(state.price_window(low, high))
}

/// Volume, RSI and ATR each want a strip of their own. They stack in the order
/// they are listed, under the price plot or — for the ones that say so — above
/// it.
fn layout(state: &State, width: f64, height: f64) -> Layout {
    let plot_x = PAD;
    let plot_w = (width - PRICE_AXIS_W - PAD).max(1.0);
    let top = PAD;
    let total_h = (height - TIME_AXIS_H - PAD).max(1.0);

    let wanted = |above: bool| -> Vec<(u32, f64)> {
        state
            .indicators
            .iter()
            .filter(|drawn| drawn.indicator.visible && drawn.indicator.above_price == above)
            .filter_map(|drawn| drawn.output.pane_height().map(|share| (drawn.indicator.id, share)))
            .collect()
    };
    let stacked = plan_rows(&wanted(true), &wanted(false), top, total_h);
    Layout {
        plot_x,
        plot_w,
        top,
        total_h,
        price_y: stacked.price_y,
        price_h: stacked.price_h,
        rows: stacked.rows,
    }
}

/// The rows and the price plot's box, as the shares alone decide them.
struct Stacked {
    rows: Vec<Row>,
    price_y: f64,
    price_h: f64,
}

/// Lay the stack out: the strips above the price, the price, then the strips
/// below, each separated by one gap.
///
/// Price keeps most of the chart whatever is stacked around it — three strips
/// sharing the window equally would leave the candles, the thing you came for,
/// as a sliver — and `MIN_PRICE_SHARE` is the floor it keeps wherever in the
/// stack it has ended up. The strips are what have heights; the price is
/// whatever they leave, which is why moving one above it changes nothing about
/// how tall anything is.
///
/// Taken out of [`layout`] so the arithmetic can be read and tested without a
/// window: shares in, pixels out.
fn plan_rows(above: &[(u32, f64)], below: &[(u32, f64)], top: f64, total_h: f64) -> Stacked {
    let sum: f64 = above.iter().chain(below).map(|(_, share)| share).sum();
    let squeeze = if sum > 1.0 - MIN_PRICE_SHARE { (1.0 - MIN_PRICE_SHARE) / sum } else { 1.0 };
    let height_of = |share: f64| total_h * share * squeeze;
    // One gap per strip: a strip has a line above it, except the topmost row,
    // which hands its gap to the line above the price instead.
    let used: f64 = above.iter().chain(below).map(|(_, share)| height_of(*share) + PANE_GAP).sum();
    let price_h = (total_h - used).max(1.0);

    let mut rows = Vec::with_capacity(above.len() + below.len() + 1);
    let mut y = top;
    for (id, share) in above {
        let height = height_of(*share);
        rows.push(Row { pane: Some(*id), top: y, height });
        y += height + PANE_GAP;
    }
    let price_y = y;
    rows.push(Row { pane: None, top: price_y, height: price_h });
    y += price_h + PANE_GAP;
    for (id, share) in below {
        let height = height_of(*share);
        rows.push(Row { pane: Some(*id), top: y, height });
        y += height + PANE_GAP;
    }
    Stacked { rows, price_y, price_h }
}

/// Which part of the widget a point is over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Region {
    Plot,
    PriceAxis,
    TimeAxis,
}

fn region_at(x: f64, y: f64, width: f64, height: f64) -> Region {
    if x >= width - PRICE_AXIS_W {
        Region::PriceAxis
    } else if y >= height - TIME_AXIS_H {
        Region::TimeAxis
    } else {
        Region::Plot
    }
}

impl State {
    /// A chart with nothing on it yet.
    ///
    /// Named rather than written out where it is used, because the benchmark
    /// needs the same starting point the widget has: a frame measured from a
    /// state assembled by hand is a frame of something else.
    fn blank(theme: Theme, scheme: BarScheme) -> State {
        State {
            bars: Vec::new(),
            echo: None,
            theme,
            scheme,
            instrument: None,
            timeframe: Timeframe::days(1),
            first: 0,
            visible: 160,
            pointer: None,
            anchored: true,
            indicators: Vec::new(),
            bar_style: BarStyle::default(),
            show_grid: true,
            drag: None,
            trouble: None,
            loading: false,
            price_zoom: 1.0,
            price_offset: 0.0,
            price_auto: true,
            drawings: Vec::new(),
            selected: Vec::new(),
            tool: None,
            placing: None,
            next_config: 1,
            configs: Configurations::default(),
            sharing: Sharing::default(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Remember the drawings as they are, for undo, before they change. A
    /// new change is a new future, so what was redoable is not any more.
    fn remember(&mut self) {
        self.undo.push(self.drawings.clone());
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The drawing most recently taken into the selection.
    fn primary(&self) -> Option<usize> {
        self.selected.last().copied()
    }

    /// The kind the whole selection is, when it is all one kind: what the
    /// properties and the configurations can be asked about. Mixed, or
    /// nothing selected, is `None`.
    fn selection_kind(&self) -> Option<DrawingKind> {
        let mut kinds = self.selected.iter().filter_map(|i| self.drawings.get(*i)).map(|d| d.kind);
        let first = kinds.next()?;
        kinds.all(|k| k == first).then_some(first)
    }

    /// Keep the selection on the same drawings after the list was rebuilt,
    /// by id; a drawing with no id yet is the newest row.
    fn reselect(&mut self, ids: Vec<i64>) {
        self.selected = ids
            .into_iter()
            .filter_map(|id| match id {
                0 => self.drawings.len().checked_sub(1),
                id => self.drawings.iter().position(|d| d.id == id),
            })
            .collect();
        self.selected.dedup();
    }

    fn selected_ids(&self) -> Vec<i64> {
        self.selected.iter().filter_map(|i| self.drawings.get(*i)).map(|d| d.id).collect()
    }

    /// Where a pixel is in the chart's own units: the moment of the bar
    /// under it and the price at that height. `None` when there is nothing
    /// on the chart to measure against.
    ///
    /// The moment snaps to a bar, so a drawing dropped on a candle lands on
    /// it exactly; past either end of the loaded bars it keeps counting at
    /// the last bar's step, so a line can be drawn into the future.
    fn locate(&self, width: f64, height: f64, x: f64, y: f64) -> Option<Anchor> {
        let (first, visible) = self.slice();
        if visible == 0 {
            return None;
        }
        let plan = layout(self, width, height);
        let (low, high) = price_range(self, &self.bars[first..first + visible])?;
        let price = high - (y - plan.price_y) / plan.price_h * (high - low);
        let bar_w = plan.plot_w / visible as f64;
        let index = first as f64 + (x - plan.plot_x) / bar_w - 0.5;
        Some(Anchor::new(ts_at(&self.bars, index.round()), price))
    }

    /// The first press with a tool in hand: a drawing of that kind, in the
    /// configuration chosen for it and the scope this chart draws into, with
    /// both anchors where the hand is until the hand moves.
    fn begin_placing(&mut self, width: f64, height: f64, x: f64, y: f64) -> bool {
        let Some(kind) = self.tool else { return false };
        let Some(anchor) = self.locate(width, height, x, y) else { return false };
        let mut drawing = Drawing::new(kind, anchor, anchor);
        drawing.follow(self.next_config);
        drawing.scope = self.sharing.scope_for_new();
        self.placing = Some(drawing);
        self.drag = Some(Drag::Place { moved: false });
        true
    }

    /// A drawing as pixels, through the same scales the candles use.
    fn project(&self, plan: &Layout, low: f64, high: f64, drawing: &Drawing) -> Projected {
        let (first, visible) = self.slice();
        let bar_w = plan.plot_w / visible.max(1) as f64;
        let point = |anchor: &Anchor| {
            let index = index_of_ts(&self.bars, anchor.ts);
            let x = plan.plot_x + (index - first as f64 + 0.5) * bar_w;
            let y = plan.price_y + plan.price_h * (high - anchor.price) / (high - low);
            (x, y)
        };
        Projected { kind: drawing.kind, from: point(&drawing.from), to: point(&drawing.to) }
    }

    /// The drawing under a pixel, topmost first, and which part of it.
    fn drawing_at(&self, width: f64, height: f64, x: f64, y: f64) -> Option<(usize, Grip)> {
        let (first, visible) = self.slice();
        if visible == 0 {
            return None;
        }
        let plan = layout(self, width, height);
        let (low, high) = price_range(self, &self.bars[first..first + visible])?;
        // The selected ones first, since their grips are what the hand is
        // most likely reaching for; then the most recently drawn.
        let order = self.selected.iter().rev().copied().chain((0..self.drawings.len()).rev());
        for index in order {
            let drawing = self.drawings.get(index)?;
            let projected = self.project(&plan, low, high, drawing);
            let hit = projected.hit(x, y);
            // Grips belong to the selected drawing only: on the others the
            // whole thing is a body, so a press on an unselected line's end
            // selects it rather than starting to move the end.
            let hit = match hit {
                Some(_) if !self.selected.contains(&index) => Some(Grip::Body),
                other => other,
            };
            if let Some(grip) = hit {
                return Some((index, grip));
            }
        }
        None
    }

    /// The price range to draw, after the user's scaling.
    ///
    /// The auto fit is always the starting point, so taking manual control
    /// never makes the chart jump — it stretches around what was already on
    /// screen.
    fn price_window(&self, low: f64, high: f64) -> (f64, f64) {
        if self.price_auto {
            return (low, high);
        }
        let range = (high - low).max(f64::EPSILON);
        let middle = (low + high) / 2.0 + range * self.price_offset;
        let half = range / 2.0 / self.price_zoom.clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM);
        (middle - half, middle + half)
    }

    /// Shift the view by a number of bars. Positive moves forward in time.
    fn pan_by(&mut self, bars: f64) {
        let (first, visible) = self.slice();
        if self.bars.len() <= visible {
            return;
        }
        let max_first = self.bars.len() - visible;
        let next = (first as i64 + bars.round() as i64).clamp(0, max_first as i64) as usize;
        self.first = next;
        self.anchored = next >= max_first;
    }

    /// Zoom the time axis about `anchor`, a fraction across the plot.
    fn zoom_time(&mut self, factor: f64, anchor: f64) {
        let (first, visible) = self.slice();
        let next = ((visible as f64 * factor).round() as usize)
            .clamp(MIN_VISIBLE, MAX_VISIBLE)
            .min(self.bars.len().max(MIN_VISIBLE));
        // Anchored to the right edge, zooming reveals history and the last bar
        // stays put — which is what you want when looking at the live edge.
        if !self.anchored {
            let focus = first as f64 + anchor * visible as f64;
            self.first = (focus - anchor * next as f64).max(0.0) as usize;
        }
        self.visible = next;
    }

    /// Stretch or compress the price scale, taking it off automatic.
    fn scale_price(&mut self, factor: f64) {
        if self.price_auto {
            self.price_auto = false;
            self.price_zoom = 1.0;
            self.price_offset = 0.0;
        }
        self.price_zoom = (self.price_zoom * factor).clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM);
    }

    /// Back to fitting the data, at the live edge.
    fn reset_view(&mut self) {
        self.price_auto = true;
        self.price_zoom = 1.0;
        self.price_offset = 0.0;
        self.visible = 160;
        self.anchored = true;
    }

    /// The visible slice, clamped to what we actually have.
    fn slice(&self) -> (usize, usize) {
        if self.bars.is_empty() {
            return (0, 0);
        }
        let visible = self.visible.clamp(MIN_VISIBLE, MAX_VISIBLE).min(self.bars.len());
        let first = if self.anchored {
            self.bars.len() - visible
        } else {
            self.first.min(self.bars.len() - visible)
        };
        (first, visible)
    }
}

/// Something the window hangs off the chart after building it. Optional
/// because the chart is constructed before there is a window to tell.
type Handler<F> = Rc<RefCell<Option<Box<F>>>>;

/// The CSS class the pointer layer carries, so the one other module that has
/// an opinion about it — the screenshot, which leaves it out — can say so by
/// name rather than by walking to the second child of an overlay.
pub const POINTER_LAYER: &str = "chart-pointer-layer";

pub struct ChartView {
    /// The chart itself, and the chart's event target: the drags, the wheel
    /// and the keyboard all land here, which is why this is the widget the
    /// window reaches for when it wants to focus a chart.
    pub area: gtk::DrawingArea,
    /// The crosshair and the rest of what follows the pointer, over the top.
    pointer: gtk::DrawingArea,
    /// The two of them stacked. This is what goes in the layout.
    pub root: gtk::Overlay,
    state: Rc<RefCell<State>>,
    on_hover: Handler<dyn Fn(Option<Hover>)>,
    on_context_menu: Handler<dyn Fn(f64, f64)>,
    /// Told when a pane's edge has been dragged, so the indicator it belongs
    /// to can be stored at its new height.
    on_pane_resize: Handler<dyn Fn(u32, f64)>,
    /// Told when a strip's close box was clicked.
    on_pane_close: Handler<dyn Fn(u32)>,
    /// Told when one of a strip's arrows was clicked, so the indicators can be
    /// reordered where they are stored — the chart draws the stack it is handed
    /// and does not keep an order of its own.
    on_pane_move: Handler<dyn Fn(u32, indicators::Move)>,
    /// Right-clicking the price axis, which has its own short menu.
    on_axis_menu: Handler<dyn Fn(f64, f64)>,
    /// Told when a drawing was added, moved or deleted, so it can be written
    /// down. The chart never touches the store.
    on_drawing: Handler<dyn Fn(DrawingEvent)>,
    /// Right-clicking a drawing, which has a menu of its own: about the
    /// drawing and nothing else.
    on_drawing_menu: Handler<dyn Fn(f64, f64)>,
    /// Enter on a selected drawing: its properties.
    on_drawing_properties: Handler<dyn Fn()>,
    /// The tool in hand, or the configuration it will draw with, changed —
    /// so whatever shows the tool can show it.
    on_tool: Handler<dyn Fn()>,
    /// Told when a gesture on the chart takes the price scale off automatic
    /// or puts it back — a drag or a wheel on the axis, a double-click to
    /// reset it. Whether the scale is automatic is written down with the
    /// chart, and these are the only ways it changes that nothing outside
    /// the chart can see.
    on_price_auto: Handler<dyn Fn(bool)>,
}

impl ChartView {
    pub fn new(theme: Theme, scheme: BarScheme) -> Rc<ChartView> {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_focusable(true);

        // Exactly the chart's own size, so a point on one layer is the same
        // point on the other and the crosshair needs no coordinates of its
        // own. It answers to no events: the chart underneath is still what
        // you drag and scroll, and a layer that could take a click would
        // swallow every one of them.
        let pointer = gtk::DrawingArea::new();
        pointer.add_css_class(POINTER_LAYER);
        pointer.set_can_target(false);
        pointer.set_hexpand(true);
        pointer.set_vexpand(true);

        let root = gtk::Overlay::new();
        root.set_child(Some(&area));
        root.add_overlay(&pointer);

        let state = Rc::new(RefCell::new(State::blank(theme, scheme)));
        let on_hover: Handler<dyn Fn(Option<Hover>)> = Rc::new(RefCell::new(None));

        let view = Rc::new(ChartView {
            area,
            pointer,
            root,
            state,
            on_hover,
            on_context_menu: Rc::new(RefCell::new(None)),
            on_pane_resize: Rc::new(RefCell::new(None)),
            on_pane_close: Rc::new(RefCell::new(None)),
            on_pane_move: Rc::new(RefCell::new(None)),
            on_axis_menu: Rc::new(RefCell::new(None)),
            on_drawing: Rc::new(RefCell::new(None)),
            on_drawing_menu: Rc::new(RefCell::new(None)),
            on_drawing_properties: Rc::new(RefCell::new(None)),
            on_tool: Rc::new(RefCell::new(None)),
            on_price_auto: Rc::new(RefCell::new(None)),
        });
        view.wire_drawing();
        view.wire_pointer();
        view.wire_zoom();
        view.wire_drag();
        view.wire_axis_menu();
        view.wire_keys();
        view
    }

    // -- drawings ----------------------------------------------------------

    pub fn set_drawing_handler(&self, handler: impl Fn(DrawingEvent) + 'static) {
        *self.on_drawing.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_drawing_menu_handler(&self, handler: impl Fn(f64, f64) + 'static) {
        *self.on_drawing_menu.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_drawing_properties_handler(&self, handler: impl Fn() + 'static) {
        *self.on_drawing_properties.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_tool_handler(&self, handler: impl Fn() + 'static) {
        *self.on_tool.borrow_mut() = Some(Box::new(handler));
    }

    fn tool_changed(&self) {
        if let Some(handler) = self.on_tool.borrow().as_ref() {
            handler();
        }
    }

    /// The configuration the next drawing gets.
    pub fn next_config(&self) -> u8 {
        self.state.borrow().next_config
    }

    /// A drawing another chart has in hand, where it is now. Shown in
    /// place of this chart's copy, so the two charts agree while the hand
    /// is still moving; the written-down version arrives at the release.
    pub fn follow_moving(&self, moving: &Drawing) {
        let shown = {
            let mut state = self.state.borrow_mut();
            match state.drawings.iter_mut().find(|d| d.id == moving.id) {
                Some(slot) => {
                    *slot = moving.clone();
                    true
                }
                None => false,
            }
        };
        if shown {
            self.redraw();
        }
    }

    /// Hand the chart what is drawn on its symbol. The selection survives
    /// when the same drawing is still in the list, which is the usual case:
    /// the window hands the list back after writing a change down.
    pub fn set_drawings(&self, drawings: Vec<Drawing>) {
        {
            let mut state = self.state.borrow_mut();
            let ids = state.selected_ids();
            state.drawings = drawings;
            state.reselect(ids);
        }
        self.redraw();
    }

    pub fn drawings(&self) -> Vec<Drawing> {
        self.state.borrow().drawings.clone()
    }

    /// Arm a tool: the next press on the plot starts a drawing of this kind.
    /// `None` is the pointer, which Escape goes back to.
    pub fn arm(&self, kind: Option<DrawingKind>) {
        {
            let mut state = self.state.borrow_mut();
            if state.tool != kind {
                // A different tool, or none: the configuration chosen for
                // the last one is not a choice about this one. The same
                // tool again keeps it, so Alt+R, Alt+2, Alt+R is still 2.
                state.next_config = 1;
            }
            state.tool = kind;
            state.placing = None;
            if kind.is_some() {
                state.selected.clear();
            }
        }
        self.area.set_cursor_from_name(Some(if kind.is_some() { "crosshair" } else { "default" }));
        self.redraw();
        self.tool_changed();
    }

    pub fn armed(&self) -> Option<DrawingKind> {
        self.state.borrow().tool
    }

    /// Escape: back to the pointer, with nothing in hand and nothing
    /// selected. A drawing half laid down — even one mid-drag — is dropped
    /// rather than finished. Says whether there was anything to drop, so a
    /// key that found nothing can go on to whoever else wants it.
    pub fn cancel(&self) -> bool {
        let busy = {
            let mut s = self.state.borrow_mut();
            let busy = s.tool.is_some() || s.placing.is_some() || !s.selected.is_empty();
            s.selected.clear();
            // The drag the gesture is still in ends with nothing to commit.
            if matches!(s.drag, Some(Drag::Place { .. })) {
                s.drag = None;
            }
            busy
        };
        self.arm(None);
        busy
    }

    /// The drawing most recently taken into the selection, if any has the
    /// grips: the one a single-drawing question is about.
    pub fn selected_drawing(&self) -> Option<Drawing> {
        let state = self.state.borrow();
        state.primary().and_then(|i| state.drawings.get(i)).cloned()
    }

    /// Everything that wears grips.
    pub fn selected_drawings(&self) -> Vec<Drawing> {
        let state = self.state.borrow();
        state.selected.iter().filter_map(|i| state.drawings.get(*i)).cloned().collect()
    }

    /// The kind the whole selection is, when it is all one kind: a line's
    /// properties can be put on every selected line, and on nothing else.
    pub fn selection_kind(&self) -> Option<DrawingKind> {
        self.state.borrow().selection_kind()
    }

    /// Select whatever drawing is under a pixel, for a right-click: the menu
    /// that opens there should be about the thing under the pointer. A
    /// drawing already in the selection keeps its company, so the menu is
    /// about all of them.
    pub fn select_at(&self, x: f64, y: f64) -> bool {
        let hit = {
            let state = self.state.borrow();
            state.drawing_at(self.area.width() as f64, self.area.height() as f64, x, y)
        };
        let changed = {
            let mut state = self.state.borrow_mut();
            match hit {
                Some((index, _)) if state.selected.contains(&index) => false,
                Some((index, _)) => {
                    state.selected = vec![index];
                    true
                }
                None => {
                    let had = !state.selected.is_empty();
                    state.selected.clear();
                    had
                }
            }
        };
        if changed {
            self.redraw();
        }
        hit.is_some()
    }

    /// Change every selected drawing the same way, and say so for each one
    /// that changed. One step back, however many there were.
    pub fn edit_selected(&self, edit: impl Fn(&mut Drawing)) {
        let changed = {
            let mut state = self.state.borrow_mut();
            if state.selected.is_empty() {
                return;
            }
            let before = state.drawings.clone();
            let mut changed = Vec::new();
            for index in state.selected.clone() {
                let Some(drawing) = state.drawings.get_mut(index) else { continue };
                edit(drawing);
                if before.get(index) != Some(drawing) {
                    changed.push(drawing.clone());
                }
            }
            // An edit that changed nothing is not a step back worth having.
            if changed.is_empty() {
                return;
            }
            state.undo.push(before);
            if state.undo.len() > HISTORY {
                state.undo.remove(0);
            }
            state.redo.clear();
            changed
        };
        self.redraw();
        for drawing in changed {
            self.tell(DrawingEvent::Changed(drawing));
        }
    }

    /// Delete everything selected, as one step back.
    pub fn delete_selected(&self) {
        let removed = {
            let mut state = self.state.borrow_mut();
            let mut indices: Vec<usize> = std::mem::take(&mut state.selected);
            indices.sort_unstable();
            indices.dedup();
            indices.retain(|i| *i < state.drawings.len());
            if indices.is_empty() {
                return;
            }
            state.remember();
            // From the back, so the ones still to go keep their places.
            indices.into_iter().rev().map(|i| state.drawings.remove(i)).collect::<Vec<_>>()
        };
        self.redraw();
        for removed in removed {
            if removed.id != 0 {
                self.tell(DrawingEvent::Removed(removed.id));
            }
        }
    }

    /// Delete every drawing this chart shows, as one step back.
    pub fn delete_all(&self) {
        let removed = {
            let mut state = self.state.borrow_mut();
            if state.drawings.is_empty() {
                return;
            }
            state.selected.clear();
            state.remember();
            std::mem::take(&mut state.drawings)
        };
        self.redraw();
        for removed in removed {
            if removed.id != 0 {
                self.tell(DrawingEvent::Removed(removed.id));
            }
        }
    }

    fn tell(&self, event: DrawingEvent) {
        if let Some(handler) = self.on_drawing.borrow().as_ref() {
            handler(event);
        }
    }

    /// Ctrl+Z: the drawings as they were before the last change. `false`
    /// when there is nothing to go back to, so the key goes on to whoever
    /// else wants it.
    pub fn undo(&self) -> bool {
        self.step(true)
    }

    /// Ctrl+Y: what the last Ctrl+Z took away.
    pub fn redo(&self) -> bool {
        self.step(false)
    }

    fn step(&self, back: bool) -> bool {
        let restored = {
            let mut state = self.state.borrow_mut();
            let Some(list) = (if back { state.undo.pop() } else { state.redo.pop() }) else {
                return false;
            };
            let now = std::mem::replace(&mut state.drawings, list.clone());
            if back {
                state.redo.push(now);
            } else {
                state.undo.push(now);
            }
            // The selection follows the drawings, those still there.
            let ids: Vec<i64> = state.selected.iter().filter_map(|i| now_id(&now_list(&list, *i))).collect();
            state.reselect(ids);
            state.placing = None;
            list
        };
        self.redraw();
        self.tell(DrawingEvent::Replaced(restored));
        true
    }

    /// Escape and Delete, for the drawing that is selected or being laid
    /// down. On the chart itself rather than the window, because they only
    /// mean anything while the chart has the keyboard — and a Delete typed
    /// into a search box must stay in the box.
    fn wire_keys(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        let view = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(view) = view.upgrade() else { return glib::Propagation::Proceed };
            use gtk::gdk::Key;
            match key {
                Key::Escape => match view.cancel() {
                    true => glib::Propagation::Stop,
                    false => glib::Propagation::Proceed,
                },
                Key::Delete | Key::BackSpace | Key::KP_Delete => {
                    if view.state.borrow().selected.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    view.delete_selected();
                    glib::Propagation::Stop
                }
                Key::Return | Key::KP_Enter | Key::ISO_Enter => {
                    if view.state.borrow().selected.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    if let Some(handler) = view.on_drawing_properties.borrow().as_ref() {
                        handler();
                    }
                    glib::Propagation::Stop
                }
                Key::z | Key::Z | Key::y | Key::Y
                    if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) =>
                {
                    let redo = matches!(key, Key::y | Key::Y)
                        || modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                    let done = if redo { view.redo() } else { view.undo() };
                    match done {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                // Ctrl+Shift with up or down: to the front, or to the back,
                // among the other drawings. Up is front, the way a stack
                // reads.
                Key::Up | Key::Down
                    if !view.state.borrow().selected.is_empty()
                        && modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
                        && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) =>
                {
                    view.restack_selected(key == Key::Up);
                    glib::Propagation::Stop
                }
                Key::Left | Key::Right | Key::Up | Key::Down
                    if !view.state.borrow().selected.is_empty()
                        && !modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) =>
                {
                    // A pixel at a time; ten with Shift.
                    let step = if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {
                        10.0
                    } else {
                        1.0
                    };
                    let (dx, dy) = match key {
                        Key::Left => (-step, 0.0),
                        Key::Right => (step, 0.0),
                        Key::Up => (0.0, -step),
                        _ => (0.0, step),
                    };
                    view.nudge_selected(dx, dy);
                    glib::Propagation::Stop
                }
                // Alt+N: configuration N for the selected drawing, or for
                // the one about to be drawn while a tool is in hand.
                _ if modifiers.contains(gtk::gdk::ModifierType::ALT_MASK) => {
                    let Some(number) = preset_number(key) else {
                        return glib::Propagation::Proceed;
                    };
                    match view.apply_configuration(number as u8) {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.area.add_controller(keys);
    }

    /// Configuration `n` (1 to 9) for the selected drawing, or, while a tool
    /// is in hand, for the drawing it is about to make. `false` when neither
    /// is the case, so the key goes on to whoever else wants it.
    pub fn apply_configuration(&self, n: u8) -> bool {
        if !(1..=drawings::CONFIGURATIONS).contains(&n) {
            return false;
        }
        let (armed, selected) = {
            let s = self.state.borrow();
            (s.tool.is_some() || s.placing.is_some(), !s.selected.is_empty())
        };
        if armed {
            let mut s = self.state.borrow_mut();
            s.next_config = n;
            if let Some(placing) = s.placing.as_mut() {
                placing.follow(n);
            }
            drop(s);
            self.pointer.queue_draw();
            self.tool_changed();
            return true;
        }
        if selected {
            // A configuration is a kind's: the key means something only
            // when the selection is all lines or all rectangles.
            if self.selection_kind().is_some() {
                self.edit_selected(|d| d.follow(n));
            }
            return true;
        }
        false
    }

    /// The configurations every drawing on this chart is read through.
    pub fn set_configurations(&self, configs: Configurations) {
        self.state.borrow_mut().configs = configs;
        self.redraw();
    }

    pub fn configurations(&self) -> Configurations {
        self.state.borrow().configs.clone()
    }

    /// What this chart shares its drawings with.
    pub fn set_sharing(&self, sharing: Sharing) {
        self.state.borrow_mut().sharing = sharing;
    }

    pub fn sharing(&self) -> Sharing {
        self.state.borrow().sharing
    }

    /// Put the selected drawings in front of, or behind, every other, in
    /// the order they already have among themselves. Over the candles
    /// either way: the order is among drawings only.
    pub fn restack_selected(&self, to_front: bool) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let mut indices = state.selected.clone();
            indices.sort_unstable();
            indices.dedup();
            indices.retain(|i| *i < state.drawings.len());
            if indices.is_empty() {
                return;
            }
            let (lowest, highest) = state
                .drawings
                .iter()
                .fold((i64::MAX, i64::MIN), |(lo, hi), d| (lo.min(d.order), hi.max(d.order)));
            state.remember();
            let count = indices.len() as i64;
            let mut moved = Vec::new();
            for (n, index) in indices.into_iter().enumerate() {
                let Some(drawing) = state.drawings.get_mut(index) else { continue };
                drawing.order = if to_front { highest + 1 + n as i64 } else { lowest - count + n as i64 };
                moved.push(drawing.clone());
            }
            let ids = state.selected_ids();
            drawings::sort_for_painting(&mut state.drawings);
            state.reselect(ids);
            moved
        };
        self.redraw();
        for drawing in changed {
            self.tell(DrawingEvent::Changed(drawing));
        }
    }

    /// Move the selected drawing by a few pixels, from the arrow keys. The
    /// pixels become a span of time and a difference in price through the
    /// chart's own scales, so the nudge is the same size whatever the zoom.
    pub fn nudge_selected(&self, dx: f64, dy: f64) {
        let moved = {
            let mut state = self.state.borrow_mut();
            if state.selected.is_empty() {
                return;
            }
            let (w, h) = (self.area.width() as f64, self.area.height() as f64);
            let (first, visible) = state.slice();
            if visible == 0 {
                return;
            }
            let plan = layout(&state, w, h);
            let Some((low, high)) = price_range(&state, &state.bars[first..first + visible]) else {
                return;
            };
            let bar_w = plan.plot_w / visible as f64;
            let by_price = -dy / plan.price_h * (high - low);
            state.remember();
            let mut moved = Vec::new();
            for index in state.selected.clone() {
                // A pixel is a fraction of a bar, and a bar is a step in
                // time — measured from where each drawing starts, since
                // the bars are not evenly spaced in time.
                let Some(from_ts) = state.drawings.get(index).map(|d| d.from.ts) else { continue };
                let at = index_of_ts(&state.bars, from_ts);
                let by_ts = ts_at(&state.bars, at + dx / bar_w) - from_ts;
                let Some(drawing) = state.drawings.get_mut(index) else { continue };
                drawing.shift(by_ts, by_price);
                moved.push(drawing.clone());
            }
            moved
        };
        self.redraw();
        for drawing in moved {
            self.tell(DrawingEvent::Changed(drawing));
        }
    }

    /// A drawing is finished: it joins the list, takes the grips, and is
    /// told about. The tool is put down, since one line at a time is what a
    /// hand draws; arming it again is one key.
    fn commit_placing(&self) {
        let added = {
            let mut state = self.state.borrow_mut();
            let Some(drawing) = state.placing.take() else { return };
            state.remember();
            state.drawings.push(drawing.clone());
            state.selected = vec![state.drawings.len() - 1];
            state.tool = None;
            state.next_config = 1;
            drawing
        };
        self.area.set_cursor_from_name(Some("default"));
        self.redraw();
        self.tool_changed();
        self.tell(DrawingEvent::Added(added));
    }

    /// Show where another chart's pointer is, in time and in price.
    ///
    /// Bar indices are not shared — two resolutions of the same symbol count
    /// their bars differently — so both halves travel as values and each
    /// chart maps them through its own scales. Price is worth carrying
    /// because an echo only ever reaches charts in the same link group, and a
    /// group shares a symbol: the number means the same thing at both ends.
    pub fn set_echo(&self, echo: Option<Echo>) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.echo != echo;
            state.echo = echo;
            changed
        };
        if changed {
            // Somebody else's crosshair, which is drawn on the same layer as
            // our own and changes nothing underneath it.
            self.pointer.queue_draw();
        }
    }

    pub fn set_hover_handler(&self, handler: impl Fn(Option<Hover>) + 'static) {
        *self.on_hover.borrow_mut() = Some(Box::new(handler));
    }

    /// Replace the series. Resets the view only when the instrument changed,
    /// so a background refresh of the same chart does not throw away a pan.
    pub fn set_series(&self, instrument: Instrument, timeframe: Timeframe, bars: Vec<Bar>) {
        let mut state = self.state.borrow_mut();
        let changed = state
            .instrument
            .as_ref()
            .map(|i| i.symbol != instrument.symbol || i.suffix != instrument.suffix)
            .unwrap_or(true)
            || state.timeframe != timeframe;
        state.bars = bars;
        state.instrument = Some(instrument);
        state.timeframe = timeframe;
        if changed {
            state.anchored = true;
            state.visible = 160;
        }
        drop(state);
        self.redraw();
    }

    /// Show or hide the gridlines. The axes and their labels stay: without
    /// them a chart is a shape with no scale.
    pub fn set_show_grid(&self, show: bool) {
        self.state.borrow_mut().show_grid = show;
        self.redraw();
    }

    pub fn set_bar_style(&self, style: BarStyle) {
        self.state.borrow_mut().bar_style = style;
        self.redraw();
    }

    /// What to do when the chart itself is right-clicked. The axis keeps its
    /// own menu.
    pub fn set_pane_resize_handler(&self, handler: impl Fn(u32, f64) + 'static) {
        *self.on_pane_resize.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_pane_close_handler(&self, handler: impl Fn(u32) + 'static) {
        *self.on_pane_close.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_pane_move_handler(&self, handler: impl Fn(u32, indicators::Move) + 'static) {
        *self.on_pane_move.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_axis_menu_handler(&self, handler: impl Fn(f64, f64) + 'static) {
        *self.on_axis_menu.borrow_mut() = Some(Box::new(handler));
    }

    /// Is the price scale fitting itself to what is on screen?
    ///
    /// False only once the axis has been dragged: until then the menu's
    /// "auto scale" is describing what is already happening, which is why it
    /// has to be shown as a state rather than offered as an action.
    pub fn price_auto(&self) -> bool {
        self.state.borrow().price_auto
    }

    /// Is the chart showing the newest bars it has?
    ///
    /// False once the view has been panned back into history, and the
    /// question a background refresh has to ask before fetching anything:
    /// somebody reading last March is the person least served by new bars
    /// arriving, and a chart that is not showing the live edge does not
    /// change when the live edge does.
    pub fn at_latest(&self) -> bool {
        self.state.borrow().anchored
    }

    pub fn set_price_auto(&self, auto: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.price_auto = auto;
            if auto {
                state.price_zoom = 1.0;
                state.price_offset = 0.0;
            }
        }
        self.redraw();
    }

    /// Be told when a gesture on the chart changes whether the price scale
    /// is automatic. Not called for [`set_price_auto`](Self::set_price_auto)
    /// or the resets: whoever called those already knows.
    pub fn set_price_auto_handler(&self, handler: impl Fn(bool) + 'static) {
        *self.on_price_auto.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_context_menu_handler(&self, handler: impl Fn(f64, f64) + 'static) {
        *self.on_context_menu.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_indicators(&self, indicators: Vec<Drawn>) {
        self.state.borrow_mut().indicators = indicators;
        self.redraw();
    }

    /// Say why the last fetch brought nothing back, or `None` once one works.
    ///
    /// Taken rather than discarded because "No data for this symbol" is a
    /// claim about the symbol, and a fetch that never arrived is not evidence
    /// for it. A first-time user on a machine with no network read that line
    /// as the app not having the S&P 500.
    pub fn set_trouble(&self, trouble: Option<FetchFailure>) {
        self.state.borrow_mut().trouble = trouble;
        self.redraw();
    }

    /// Whether a fetch is outstanding for what is on screen.
    ///
    /// An empty chart that is still loading and an empty chart that came back
    /// empty are different things, and telling the user they are the same is
    /// how "no data" ends up meaning nothing.
    pub fn set_loading(&self, loading: bool) {
        self.state.borrow_mut().loading = loading;
        self.redraw();
    }

    pub fn restyle(&self, theme: Theme, scheme: BarScheme) {
        {
            let mut state = self.state.borrow_mut();
            state.theme = theme;
            state.scheme = scheme;
        }
        self.redraw();
    }

    pub fn timeframe(&self) -> Timeframe {
        self.state.borrow().timeframe
    }

    /// How many rows the volume profile with this id is actually drawing.
    ///
    /// The settings panel shows this when the count is automatic, so the
    /// number on screen is the number being drawn rather than a placeholder.
    pub fn profile_rows(&self, id: u32) -> Option<usize> {
        let state = self.state.borrow();
        state.indicators.iter().find(|drawn| drawn.indicator.id == id).and_then(|drawn| {
            match &drawn.output {
                Output::Profiles(profiles) => profiles.first().map(|p| p.rows.len()),
                _ => None,
            }
        })
    }

    pub fn bar_count(&self) -> usize {
        self.state.borrow().bars.len()
    }

    /// Jump back to the right edge and follow new bars again.
    pub fn go_to_latest(&self) {
        self.state.borrow_mut().anchored = true;
        self.redraw();
    }

    pub fn zoom(&self, factor: f64) {
        self.state.borrow_mut().zoom_time(factor, 0.5);
        self.redraw();
    }

    pub fn pan_bars(&self, delta: i64) {
        self.state.borrow_mut().pan_by(delta as f64);
        self.redraw();
    }

    /// Everything: the chart and the layer over it.
    ///
    /// Anything that changes the chart has to invalidate both, because what
    /// the pointer layer draws is read off the scales the chart is drawn on —
    /// a crosshair over a zoom it has not been told about would be labelling a
    /// price the axis no longer shows.
    fn redraw(&self) {
        redraw(&self.area, &self.pointer);
    }

    fn wire_drawing(&self) {
        let state = self.state.clone();
        self.area.set_draw_func(move |_, cr, width, height| {
            draw(cr, width as f64, height as f64, &state.borrow());
        });

        let state = self.state.clone();
        self.pointer.set_draw_func(move |_, cr, width, height| {
            draw_pointer(cr, width as f64, height as f64, &state.borrow());
        });
    }

    fn wire_pointer(&self) {
        let motion = gtk::EventControllerMotion::new();
        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let on_hover = self.on_hover.clone();
        motion.connect_motion(move |_, x, y| {
            {
                let mut s = state.borrow_mut();
                s.pointer = Some((x, y));
                // A drawing with one anchor down follows the pointer with
                // the other, button or no button: click, move, click is
                // the way a line is drawn, and the line has to be there
                // during the move.
                if s.drag.is_none() && s.placing.is_some() {
                    let (w, h) = (area.width() as f64, area.height() as f64);
                    if let Some(anchor) = s.locate(w, h, x, y)
                        && let Some(placing) = s.placing.as_mut()
                    {
                        placing.to = anchor;
                    }
                }
            }
            // The cursor is the only thing that says a line can be dragged, so
            // it changes the moment the pointer is close enough to grab it.
            let over_edge = {
                let s = state.borrow();
                layout(&s, area.width() as f64, area.height() as f64).edge_at(y).is_some()
            };
            let over_control = {
                let s = state.borrow();
                layout(&s, area.width() as f64, area.height() as f64).control_at(x, y).is_some()
            };
            let (armed, over_drawing) = {
                let s = state.borrow();
                let over = s.drag.is_none()
                    && s.tool.is_none()
                    && s.drawing_at(area.width() as f64, area.height() as f64, x, y).is_some();
                (s.tool.is_some(), over)
            };
            area.set_cursor_from_name(Some(match (over_edge, over_control, armed, over_drawing) {
                (true, _, _, _) => "ns-resize",
                (_, true, _, _) => "pointer",
                (_, _, true, _) => "crosshair",
                (_, _, _, true) => "pointer",
                _ => "default",
            }));
            notify_hover(&state, &on_hover, &area);
            // The chart itself is untouched: nothing under the crosshair has
            // changed, and this is the whole of why the crosshair has a widget
            // of its own.
            pointer.queue_draw();
        });

        let state = self.state.clone();
        let pointer = self.pointer.clone();
        let on_hover = self.on_hover.clone();
        motion.connect_leave(move |_| {
            state.borrow_mut().pointer = None;
            if let Some(handler) = on_hover.borrow().as_ref() {
                handler(None);
            }
            pointer.queue_draw();
        });
        self.area.add_controller(motion);

        // The trouble dot's words. The chart has no other tooltip, so that
        // nothing opens over the crosshair readout.
        self.area.set_has_tooltip(true);
        let state = self.state.clone();
        self.area.connect_query_tooltip(move |area, x, y, _keyboard, tooltip| {
            let s = state.borrow();
            if !over_trouble_dot(&s, area.width() as f64, x as f64, y as f64) {
                return false;
            }
            tooltip.set_text(s.trouble.map(FetchFailure::message));
            true
        });
    }

    /// The wheel, with the conventions every charting tool shares.
    ///
    /// Plain wheel zooms time about the cursor. Shift pans. Ctrl scales price.
    /// Over an axis, the wheel scales that axis whatever the modifiers — the
    /// axis is the control.
    fn wire_zoom(&self) {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let on_price_auto = self.on_price_auto.clone();
        scroll.connect_scroll(move |controller, dx, dy| {
            if dx == 0.0 && dy == 0.0 {
                return glib::Propagation::Proceed;
            }
            let modifiers = controller.current_event_state();
            let shift = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);

            let (width, height) = (area.width() as f64, area.height() as f64);
            let mut s = state.borrow_mut();
            let was_auto = s.price_auto;
            let region = s
                .pointer
                .map(|(x, y)| region_at(x, y, width, height))
                .unwrap_or(Region::Plot);

            // A trackpad's horizontal axis always pans, whatever is held.
            if dx != 0.0 && dy == 0.0 {
                s.pan_by(dx * 2.0);
                drop(s);
                redraw(&area, &pointer);
                return glib::Propagation::Stop;
            }

            match (region, shift, ctrl) {
                (Region::PriceAxis, _, _) | (Region::Plot, false, true) => {
                    // Same sense as dragging the axis.
                    s.scale_price(2f64.powf(dy / 4.0));
                }
                (Region::Plot, true, _) => {
                    let visible = s.slice().1 as f64;
                    s.pan_by(dy * visible / 20.0);
                }
                _ => {
                    let plot_w = (width - PRICE_AXIS_W - PAD).max(1.0);
                    let anchor = s
                        .pointer
                        .map(|(x, _)| ((x - PAD) / plot_w).clamp(0.0, 1.0))
                        .unwrap_or(0.5);
                    s.zoom_time(if dy > 0.0 { 1.15 } else { 1.0 / 1.15 }, anchor);
                }
            }
            drop(s);
            redraw(&area, &pointer);
            notify_price_auto(&state, &on_price_auto, was_auto);
            glib::Propagation::Stop
        });
        self.area.add_controller(scroll);
    }

    fn wire_drag(self: &Rc<Self>) {
        // Before the drag gesture, so clicking a strip's corner acts on the
        // strip rather than starting a pan under the pointer.
        let controls = gtk::GestureClick::new();
        controls.set_button(gtk::gdk::BUTTON_PRIMARY);
        controls.set_propagation_phase(gtk::PropagationPhase::Capture);
        let state = self.state.clone();
        let area = self.area.clone();
        let on_close = self.on_pane_close.clone();
        let on_move = self.on_pane_move.clone();
        controls.connect_pressed(move |gesture, _, x, y| {
            let hit = {
                let s = state.borrow();
                layout(&s, area.width() as f64, area.height() as f64).control_at(x, y)
            };
            let Some((id, control)) = hit else { return };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            match control {
                Control::Close => {
                    if let Some(handler) = on_close.borrow().as_ref() {
                        handler(id);
                    }
                }
                Control::Up | Control::Down => {
                    let direction = if control == Control::Up {
                        indicators::Move::Up
                    } else {
                        indicators::Move::Down
                    };
                    if let Some(handler) = on_move.borrow().as_ref() {
                        handler(id, direction);
                    }
                }
            }
        });
        self.area.add_controller(controls);

        let drag = gtk::GestureDrag::new();

        let state = self.state.clone();
        let area = self.area.clone();
        let view = Rc::downgrade(self);
        let on_price_auto = self.on_price_auto.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
        let (width, height) = (area.width() as f64, area.height() as f64);
        let shift = gesture.current_event_state().contains(gtk::gdk::ModifierType::SHIFT_MASK);
        // A drawing being laid down takes the press before anything
        // else: the second press is the drawing's second anchor.
        let finishing = {
            let mut s = state.borrow_mut();
            if s.placing.is_some() {
                if let Some(anchor) = s.locate(width, height, x, y)
                    && let Some(placing) = s.placing.as_mut()
                {
                    placing.to = anchor;
                }
                s.drag = None;
                true
            } else {
                false
            }
        };
        if finishing {
            if let Some(view) = view.upgrade() {
                view.commit_placing();
            }
            return;
        }
        let mut s = state.borrow_mut();
        let was_auto = s.price_auto;
        let (first, visible) = s.slice();
        // The edge wins over whatever region it crosses, because that is
        // what the cursor was already promising.
        let plan = layout(&s, width, height);
        if let Some(edge) = plan.edge_at(y) {
            let share = s
                .indicators
                .iter()
                .find(|d| d.indicator.id == edge.id)
                .and_then(|d| d.output.pane_height())
                .unwrap_or(0.18);
            s.drag = Some(Drag::PaneEdge {
                id: edge.id,
                share,
                total_h: plan.total_h,
                grows_downward: edge.grows_downward,
            });
            return;
        }
        let region = region_at(x, y, width, height);
        if region == Region::Plot {
            // An armed tool: this press is the first anchor.
            if s.tool.is_some() {
                s.begin_placing(width, height, x, y);
                return;
            }
            // A drawing under the hand: take hold of it. With Shift it
            // joins the selection, or leaves it, and nothing moves. One
            // already selected keeps its company, so a drag on it moves
            // them all. Otherwise a press on the chart lets go of whatever
            // was selected, and pans as it always did.
            match s.drawing_at(width, height, x, y) {
                Some((index, _)) if shift => {
                    match s.selected.iter().position(|i| *i == index) {
                        Some(at) => {
                            s.selected.remove(at);
                        }
                        None => s.selected.push(index),
                    }
                    drop(s);
                    area.queue_draw();
                    return;
                }
                Some((index, grip)) => {
                    if let Some(at) = s.selected.iter().position(|i| *i == index) {
                        s.selected.remove(at);
                    } else {
                        s.selected.clear();
                    }
                    s.selected.push(index);
                    if let Some(origin) = s.locate(width, height, x, y) {
                        // Before the first motion, so a drag is one step
                        // back however far it went. A press that never
                        // moves is forgotten at the end.
                        s.remember();
                        s.drag = Some(Drag::Grip { index, grip, origin });
                    }
                    drop(s);
                    area.queue_draw();
                    return;
                }
                None if shift => {}
                None => {
                    if !s.selected.is_empty() {
                        s.selected.clear();
                        area.queue_draw();
                    }
                }
            }
        }
        s.drag = Some(
            match region {
                Region::PriceAxis => {
                    // Touching the axis takes the scale off automatic, from
                    // exactly where it was, so nothing jumps.
                    if s.price_auto {
                        s.price_auto = false;
                        s.price_zoom = 1.0;
                        s.price_offset = 0.0;
                    }
                    Drag::PriceScale { zoom: s.price_zoom }
                }
                Region::TimeAxis => Drag::TimeScale { visible },
                Region::Plot => Drag::Pan { first, offset: s.price_offset },
            },
        );
        drop(s);
        notify_price_auto(&state, &on_price_auto, was_auto);
    });

        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let view = Rc::downgrade(self);
        drag.connect_drag_update(move |gesture, offset_x, offset_y| {
            let mut s = state.borrow_mut();
            let Some(drag) = s.drag else { return };
            let plot_w = (area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0);
            let (width, height) = (area.width() as f64, area.height() as f64);
            let hand = gesture.start_point().map(|(sx, sy)| (sx + offset_x, sy + offset_y));

            match drag {
                Drag::Place { moved } => {
                    let Some((x, y)) = hand else { return };
                    if let Some(anchor) = s.locate(width, height, x, y)
                        && let Some(placing) = s.placing.as_mut()
                    {
                        placing.to = anchor;
                    }
                    if !moved && offset_x.hypot(offset_y) > 4.0 {
                        s.drag = Some(Drag::Place { moved: true });
                    }
                    drop(s);
                    // The drawing in hand is on the pointer layer; the chart
                    // underneath has not changed.
                    pointer.queue_draw();
                    return;
                }
                Drag::Grip { index, grip, origin } => {
                    let Some((x, y)) = hand else { return };
                    let Some(now) = s.locate(width, height, x, y) else { return };
                    let moving: Vec<Drawing> = match grip {
                        Grip::Body => {
                            // The whole selection follows the hand by the
                            // distance it has travelled since the last
                            // motion, so a drag past the end of the bars
                            // keeps moving at the bars' own step.
                            let (by_ts, by_price) = (now.ts - origin.ts, now.price - origin.price);
                            let indices = s.selected.clone();
                            let mut moving = Vec::new();
                            for i in indices {
                                if let Some(drawing) = s.drawings.get_mut(i) {
                                    drawing.shift(by_ts, by_price);
                                    moving.push(drawing.clone());
                                }
                            }
                            s.drag = Some(Drag::Grip { index, grip, origin: now });
                            moving
                        }
                        grip => {
                            // A corner is one drawing's own.
                            match s.drawings.get_mut(index) {
                                Some(drawing) => {
                                    drawing.move_grip(grip, now);
                                    vec![drawing.clone()]
                                }
                                None => Vec::new(),
                            }
                        }
                    };
                    drop(s);
                    area.queue_draw();
                    // The other charts of this symbol follow the hand too,
                    // not only the release.
                    if let Some(view) = view.upgrade() {
                        for drawing in moving {
                            view.tell(DrawingEvent::Moving(drawing));
                        }
                    }
                    return;
                }
                _ => {}
            }

            match drag {
                Drag::Pan { first, offset } => {
                    let (_, visible) = s.slice();
                    if s.bars.len() > visible {
                        let bar_w = plot_w / visible as f64;
                        // Dragging right reveals older bars.
                        let shift = -(offset_x / bar_w).round() as i64;
                        let max_first = s.bars.len() - visible;
                        let next = (first as i64 + shift).clamp(0, max_first as i64) as usize;
                        s.first = next;
                        s.anchored = next >= max_first;
                    }
                    // Vertical panning only means something once the scale is
                    // no longer fitting itself to the data. The content follows
                    // the hand: drag down and the bars come down with it, which
                    // means the window moves up the price axis.
                    if !s.price_auto {
                        let plot_h = (area.height() as f64 - TIME_AXIS_H - PAD).max(1.0);
                        s.price_offset = offset + offset_y / plot_h / s.price_zoom;
                    }
                }
                Drag::PriceScale { zoom } => {
                    // Grabbing the axis and pulling up stretches it: the
                    // numbers spread apart and less price fits on screen.
                    // Pulling down squeezes them together and shows more.
                    let factor = 2f64.powf(offset_y / DRAG_PER_DOUBLING);
                    s.price_zoom = (zoom * factor).clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM);
                }
                Drag::PaneEdge { id, share, total_h, grows_downward } => {
                    // The boundary goes where the hand goes, so the pane that
                    // owns the line grows towards it: pulling up grows the pane
                    // under the line, and pulling down the one above it.
                    let travel = if grows_downward { offset_y } else { -offset_y };
                    let next = (share + travel / total_h)
                        .clamp(indicators::MIN_PANE_SHARE, indicators::MAX_PANE_SHARE);
                    if let Some(drawn) = s.indicators.iter_mut().find(|d| d.indicator.id == id) {
                        drawn.output.set_pane_height(next);
                    }
                }
                Drag::TimeScale { visible } => {
                    // Dragging left compresses: more time on screen.
                    let factor = 2f64.powf(-offset_x / DRAG_PER_DOUBLING);
                    let next = ((visible as f64 * factor).round() as usize)
                        .clamp(MIN_VISIBLE, MAX_VISIBLE);
                    s.visible = next;
                }
                Drag::Place { .. } | Drag::Grip { .. } => unreachable!("handled above"),
            }
            drop(s);
            redraw(&area, &pointer);
        });

        let state = self.state.clone();
        let on_resize = self.on_pane_resize.clone();
        let view = Rc::downgrade(self);
        drag.connect_drag_end(move |_, _, _| {
            let finished = state.borrow_mut().drag.take();
            // Stored when the drag ends rather than on every motion event,
            // which would be a database write per pixel of travel.
            match finished {
                // Pressed and released in place: the first anchor is down
                // and the second waits for the next press.
                Some(Drag::Place { moved: false }) => return,
                Some(Drag::Place { moved: true }) => {
                    if let Some(view) = view.upgrade() {
                        view.commit_placing();
                    }
                    return;
                }
                Some(Drag::Grip { .. }) => {
                    let moved: Vec<Drawing> = {
                        let mut s = state.borrow_mut();
                        let before = s.undo.last().cloned().unwrap_or_default();
                        let moved: Vec<Drawing> = s
                            .drawings
                            .iter()
                            .enumerate()
                            .filter(|(i, d)| before.get(*i) != Some(*d))
                            .map(|(_, d)| d.clone())
                            .collect();
                        // A press that selected and let go moved nothing.
                        if moved.is_empty() {
                            s.undo.pop();
                        }
                        moved
                    };
                    if let Some(view) = view.upgrade() {
                        for drawing in moved {
                            view.tell(DrawingEvent::Changed(drawing));
                        }
                    }
                    return;
                }
                _ => {}
            }
            let Some(Drag::PaneEdge { id, .. }) = finished else { return };
            let share = state
                .borrow()
                .indicators
                .iter()
                .find(|d| d.indicator.id == id)
                .and_then(|d| d.output.pane_height());
            if let (Some(share), Some(handler)) = (share, on_resize.borrow().as_ref()) {
                handler(id, share);
            }
        });

        self.area.add_controller(drag);

        // Double-clicking an axis puts it back on automatic, which is the way
        // out of any scale you have stretched into uselessness.
        let click = gtk::GestureClick::new();
        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let on_price_auto = self.on_price_auto.clone();
        click.connect_pressed(move |_, presses, x, y| {
            if presses < 2 {
                return;
            }
            let region = region_at(x, y, area.width() as f64, area.height() as f64);
            let mut s = state.borrow_mut();
            let was_auto = s.price_auto;
            match region {
                Region::PriceAxis => {
                    s.price_auto = true;
                    s.price_zoom = 1.0;
                    s.price_offset = 0.0;
                }
                Region::TimeAxis => {
                    s.visible = 160;
                    s.anchored = true;
                }
                // Double-clicking the chart itself does nothing, the same as
                // everywhere else. Resetting is Ctrl+Esc or the axis menu.
                Region::Plot => return,
            }
            drop(s);
            redraw(&area, &pointer);
            notify_price_auto(&state, &on_price_auto, was_auto);
        });
        self.area.add_controller(click);
    }

    /// Put the price scale back to fitting the data.
    pub fn reset_price_scale(&self) {
        let mut state = self.state.borrow_mut();
        state.price_auto = true;
        state.price_zoom = 1.0;
        state.price_offset = 0.0;
        drop(state);
        self.redraw();
    }

    /// Everything back to how the chart opens. Ctrl+Esc, and the axis menu.
    pub fn reset_view(&self) {
        self.state.borrow_mut().reset_view();
        self.redraw();
    }

    /// Offer the reset where people right-click for it.
    fn wire_axis_menu(self: &Rc<Self>) {
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let area = self.area.clone();
        let on_context_menu = self.on_context_menu.clone();
        let on_axis_menu = self.on_axis_menu.clone();
        let view = Rc::downgrade(self);
        click.connect_pressed(move |_, _, x, y| {
            let (width, height) = (area.width() as f64, area.height() as f64);
            if region_at(x, y, width, height) != Region::PriceAxis {
                // The menu is about what is under the pointer: a drawing
                // there is selected and gets a menu of its own, about the
                // drawing and nothing else. A right-click while a tool is
                // armed puts it down.
                if let Some(view) = view.upgrade() {
                    if view.armed().is_some() {
                        view.arm(None);
                    }
                    if view.select_at(x, y) {
                        if let Some(handler) = view.on_drawing_menu.borrow().as_ref() {
                            handler(x, y);
                        }
                        return;
                    }
                }
                // The chart's own menu belongs to whoever owns the chart.
                if let Some(handler) = on_context_menu.borrow().as_ref() {
                    handler(x, y);
                }
                return;
            }
            // The axis has its own menu, built by whoever owns the chart so it
            // is a real menu like every other one rather than a box of buttons.
            if let Some(handler) = on_axis_menu.borrow().as_ref() {
                handler(x, y);
            }
        });
        self.area.add_controller(click);
    }
}

/// Invalidate both of a chart's layers.
///
/// A free function because the pointer handlers hold the two widgets and not
/// the view they belong to, and because leaving them to call `queue_draw`
/// twice each is how one of them eventually calls it once.
fn redraw(body: &gtk::DrawingArea, pointer: &gtk::DrawingArea) {
    body.queue_draw();
    pointer.queue_draw();
}

/// Tell the window if a gesture just changed whether the price scale is
/// automatic.
///
/// Compared against what it `was` rather than reported outright, because
/// every notch of the wheel over the axis passes through here and only the
/// first one changes anything. Called with the state no longer borrowed: the
/// handler is the window's, and the window reads the chart back.
fn notify_price_auto(
    state: &Rc<RefCell<State>>,
    on_price_auto: &Handler<dyn Fn(bool)>,
    was: bool,
) {
    let now = state.borrow().price_auto;
    if now != was && let Some(handler) = on_price_auto.borrow().as_ref() {
        handler(now);
    }
}

fn notify_hover(
    state: &Rc<RefCell<State>>,
    on_hover: &Handler<dyn Fn(Option<Hover>)>,
    area: &gtk::DrawingArea,
) {
    let hover = {
        let s = state.borrow();
        let (first, visible) = s.slice();
        match (s.pointer, visible) {
            (Some((x, y)), v) if v > 0 => {
                let plot_w = (area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0);
                let frac = (x - PAD) / plot_w;
                if (0.0..=1.0).contains(&frac) {
                    let index = (first as f64 + frac * visible as f64).floor() as usize;
                    let plan = layout(&s, area.width() as f64, area.height() as f64);
                    let range = price_range(&s, &s.bars[first..first + visible]);
                    s.bars.get(index.min(s.bars.len().saturating_sub(1))).map(|bar| Hover {
                        bar: *bar,
                        index,
                        price: match range {
                            Some((low, high)) => {
                                high - (y - plan.price_y) / plan.price_h * (high - low)
                            }
                            None => bar.close,
                        },
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    };
    if let Some(handler) = on_hover.borrow().as_ref() {
        handler(hover);
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

fn draw(cr: &cairo::Context, width: f64, height: f64, state: &State) {
    let ui = &state.theme.ui;

    colors::set_source(cr, &ui.background);
    let _ = cr.paint();

    let (first, visible) = state.slice();
    if visible == 0 {
        draw_placeholder(cr, width, height, state);
        return;
    }
    let bars = &state.bars[first..first + visible];

    let plan = layout(state, width, height);
    let (plot_x, plot_w, price_y, price_h) = (plan.plot_x, plan.plot_w, plan.price_y, plan.price_h);

    // What will actually be drawn: the visible bars themselves at any normal
    // zoom, and one aggregate per pixel column past that.
    let columns = Columns::of(bars, first, visible, plot_x, plot_w);

    let mut max_volume: f64 = 0.0;
    for b in columns.bars.iter() {
        max_volume = max_volume.max(b.volume);
    }
    // Off the real bars rather than the columns. The two agree, because a
    // column keeps the highest high and the lowest low of what it stands for —
    // and the crosshair works the same range out from the same bars on its own
    // layer, where going through the columns would be work for nothing.
    let Some((low, high)) = price_range(state, bars) else {
        draw_placeholder(cr, width, height, state);
        return;
    };

    let to_y = |price: f64| price_y + price_h * (high - price) / (high - low);
    let bar_w = columns.bar_w;

    // One answer for how precisely prices are written, used by the gridlines,
    // the last-price chip and the crosshair alike — a chart saying 1.1257 on
    // one label and 1.13 on another is describing two different prices.
    let step = nice_step(high - low, (price_h / 52.0).max(2.0) as usize);
    let kind = state.instrument.as_ref().map(|i| i.kind);
    let decimals = omacharts_engine::price_decimals(step, (low + high) / 2.0, kind);

    draw_price_grid(cr, state, plot_x, plot_w, price_y, price_h, low, high, &to_y);
    draw_time_axis(cr, state, &columns.bars, plot_x, plot_w, height, bar_w, first);
    // Shaded things go under the candles; lines go over. A band drawn on top
    // of the bars hides the thing it is describing.
    draw_indicator_fills(cr, state, &columns, &to_y);
    draw_candles(cr, state, &columns.bars, plot_x, bar_w, &to_y);
    draw_indicator_lines(cr, state, &columns, &to_y);
    draw_drawings(cr, state, &plan, low, high);

    for (at, row) in plan.rows.iter().enumerate() {
        draw_row_edge(cr, state, &plan, at);
        let Some(id) = row.pane else { continue };
        let Some(drawn) = state.indicators.iter().find(|d| d.indicator.id == id) else {
            continue;
        };
        match &drawn.output {
            Output::Volume { .. } => {
                if max_volume > 0.0 {
                    draw_volume(
                        cr,
                        state,
                        &columns.bars,
                        plot_x,
                        bar_w,
                        row.top,
                        row.height,
                        max_volume,
                    );
                }
                // Named like the others now that it is one strip among several.
                // Three unlabelled boxes under a chart is a puzzle.
                draw_pane_name(cr, state, drawn, plot_x, row.top);
            }
            Output::Pane(pane) => {
                draw_pane(cr, state, pane, drawn, &columns, plot_w, row.top, row.height)
            }
            _ => {}
        }
    }

    draw_last_price(cr, state, plot_x, plot_w, price_y, width, decimals, &to_y);

    if state.trouble.is_some() {
        draw_trouble_dot(cr, state, width);
    }
}

/// The layer over the chart: the crosshair and its labels, another chart's
/// crosshair, and the boxes a strip wears while the pointer is in it.
///
/// Everything here is decided by where the pointer is, which is why it is a
/// widget of its own. A pointer motion invalidates this and nothing else, so
/// the candles underneath are composited from the frame they were already
/// drawn in rather than rasterised again — which at a screenful of bars is the
/// difference between a crosshair costing microseconds and costing a frame.
///
/// It works the scales out again rather than being handed them. They are two
/// passes over the visible bars and a layout, which is nothing beside drawing
/// them, and reading them off a cache would be one more thing to invalidate.
fn draw_pointer(cr: &cairo::Context, width: f64, height: f64, state: &State) {
    let (first, visible) = state.slice();
    if visible == 0 {
        return;
    }
    let bars = &state.bars[first..first + visible];
    let plan = layout(state, width, height);
    let Some((low, high)) = price_range(state, bars) else { return };
    let to_y = |price: f64| plan.price_y + plan.price_h * (high - price) / (high - low);
    let bar_w = plan.plot_w / visible as f64;

    // The way around and out of a strip, offered only while the pointer is in
    // it: four panes each wearing permanent buttons is a dozen things
    // competing with the chart.
    for (at, row) in plan.rows.iter().enumerate() {
        if row.pane.is_some() && state.pointer.map(|(_, y)| row.covers(y)).unwrap_or(false) {
            draw_pane_controls(cr, state, &plan, row, at);
        }
    }

    // The drawing in hand, with its second anchor wherever the pointer is.
    if let Some(placing) = &state.placing {
        cr.save().ok();
        cr.rectangle(plan.plot_x, plan.price_y, plan.plot_w, plan.price_h);
        cr.clip();
        draw_drawing(cr, state, &state.project(&plan, low, high, placing), placing, true);
        cr.restore().ok();
    }

    if let Some((px, py)) = state.pointer {
        draw_crosshair(
            cr, state, px, py, plan.plot_x, plan.plot_w, plan.top, plan.price_y, plan.price_h,
            width, height, bar_w, first, low, high,
        );
    }

    // Another chart's pointer, if nothing is pointing at this one. The real
    // crosshair wins: an echo under your own pointer is a second line saying
    // the same thing.
    if state.pointer.is_none()
        && let Some(echo) = state.echo
    {
        draw_echo(
            cr, state, bars, plan.plot_x, plan.plot_w, bar_w, plan.top, plan.price_y,
            plan.price_h, height, echo, &to_y,
        );
    }
}

/// The one line an empty chart gets, and the whole of what a first-time user
/// is told when nothing can be fetched.
///
/// "No data for this symbol" is reserved for the case it describes: the
/// provider answered, and had nothing — a delisted ticker, or one it does not
/// carry. It was shown for every failure, which on a fresh install with no
/// network meant a column of untouched defaults each claiming the app does not
/// have the S&P 500.
///
/// Loading wins over a failure because a retry already in flight is the newer
/// fact, and leaving the last failure up while it runs would make a chart that
/// is about to fill in look broken.
fn placeholder_text(
    have_instrument: bool,
    loading: bool,
    trouble: Option<FetchFailure>,
) -> &'static str {
    match (have_instrument, loading, trouble) {
        (false, _, _) => "Press Ctrl+K to find a symbol",
        (true, true, _) => "Loading…",
        (true, false, Some(trouble)) => trouble.message(),
        (true, false, None) => "No data for this symbol",
    }
}

fn draw_placeholder(cr: &cairo::Context, width: f64, height: f64, state: &State) {
    let text = placeholder_text(state.instrument.is_some(), state.loading, state.trouble);
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.8);
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(13.0);
    if let Ok(extents) = cr.text_extents(text) {
        cr.move_to((width - extents.width()) / 2.0, height / 2.0);
        let _ = cr.show_text(text);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_price_grid(
    cr: &cairo::Context,
    state: &State,
    plot_x: f64,
    plot_w: f64,
    plot_y: f64,
    price_h: f64,
    low: f64,
    high: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let step = nice_step(high - low, (price_h / 52.0).max(2.0) as usize);
    if step <= 0.0 {
        return;
    }
    let decimals = omacharts_engine::price_decimals(
        step,
        (low + high) / 2.0,
        state.instrument.as_ref().map(|i| i.kind),
    );

    cr.set_line_width(1.0);
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);

    let mut price = (low / step).ceil() * step;
    while price <= high {
        let y = to_y(price).round() + 0.5;
        if y > plot_y && y < plot_y + price_h {
            if state.show_grid {
                colors::set_source(cr, &state.theme.ui.grid);
                cr.move_to(plot_x, y);
                cr.line_to(plot_x + plot_w, y);
                let _ = cr.stroke();
            }

            colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.9);
            cr.move_to(plot_x + plot_w + 6.0, y + 3.5);
            let _ = cr.show_text(&format!("{price:.decimals$}"));
        }
        price += step;
    }

    // The axis itself.
    colors::set_source(cr, &state.theme.ui.axis);
    let x = (plot_x + plot_w).round() + 0.5;
    cr.move_to(x, plot_y);
    cr.line_to(x, plot_y + price_h);
    let _ = cr.stroke();
}

#[allow(clippy::too_many_arguments)]
fn draw_time_axis(
    cr: &cairo::Context,
    state: &State,
    bars: &[Bar],
    plot_x: f64,
    plot_w: f64,
    height: f64,
    bar_w: f64,
    _first: usize,
) {
    let y = height - TIME_AXIS_H;
    colors::set_source(cr, &state.theme.ui.axis);
    cr.set_line_width(1.0);
    cr.move_to(plot_x, y.round() + 0.5);
    cr.line_to(plot_x + plot_w, y.round() + 0.5);
    let _ = cr.stroke();

    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);

    // About one label per 90px, on a whole number of bars so labels do not
    // jitter as the view scrolls.
    let target = (plot_w / 90.0).max(2.0) as usize;
    let stride = (bars.len() / target).max(1);
    // How far apart two labels land decides the format: a decade of daily bars
    // labelled "01 Jun" tells you nothing, and nine months of them all
    // labelled "Mar 2026" tells you less.
    let span = match (bars.first(), bars.last()) {
        (Some(first), Some(last)) => last.ts - first.ts,
        _ => 0,
    };
    let labels = (bars.len().div_ceil(stride)).max(1) as i64;
    let tick_seconds = span / labels;
    for (i, bar) in bars.iter().enumerate() {
        if i % stride != 0 {
            continue;
        }
        let x = plot_x + (i as f64 + 0.5) * bar_w;
        if x < plot_x + 18.0 || x > plot_x + plot_w - 18.0 {
            continue;
        }
        if state.show_grid {
            colors::set_source(cr, &state.theme.ui.grid);
            cr.move_to(x.round() + 0.5, PAD);
            cr.line_to(x.round() + 0.5, y);
            let _ = cr.stroke();
        }

        let label = format_axis_time(bar.ts, tick_seconds, state.timeframe.is_intraday());
        colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.9);
        if let Ok(extents) = cr.text_extents(&label) {
            cr.move_to(x - extents.width() / 2.0, height - 7.0);
            let _ = cr.show_text(&label);
        }
    }
}

/// Up and down candles in two passes, each building one path for wicks and one
/// for bodies.
fn draw_candles(
    cr: &cairo::Context,
    state: &State,
    bars: &[Bar],
    plot_x: f64,
    bar_w: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    if state.bar_style == BarStyle::Ohlc {
        draw_ohlc(cr, state, bars, plot_x, bar_w, to_y);
        return;
    }
    let scheme = &state.scheme;
    let body_w = (bar_w * 0.68).clamp(1.0, 24.0);
    // Below about three pixels a candle is a line; outlining it just muddies
    // the colour.
    let hairline = bar_w < 3.0;

    for rising in [false, true] {
        let (outline, fill) = if rising {
            (&scheme.up, &scheme.up_fill)
        } else {
            (&scheme.down, &scheme.down_fill)
        };

        // Wicks.
        cr.set_line_width(1.0);
        colors::set_source(cr, outline);
        let mut any = false;
        for (i, bar) in bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising {
                continue;
            }
            any = true;
            let x = (plot_x + (i as f64 + 0.5) * bar_w).round() + 0.5;
            cr.move_to(x, to_y(bar.high).round());
            cr.line_to(x, to_y(bar.low).round());
        }
        if any {
            let _ = cr.stroke();
        }
        if !any {
            continue;
        }

        if hairline {
            // At this density the body is the wick.
            continue;
        }

        // Bodies, as one path.
        for (i, bar) in bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising {
                continue;
            }
            let x = plot_x + (i as f64 + 0.5) * bar_w - body_w / 2.0;
            let top = to_y(bar.open.max(bar.close)).round();
            let bottom = to_y(bar.open.min(bar.close)).round();
            cr.rectangle(x.round(), top, body_w.round(), (bottom - top).max(1.0));
        }
        if colors::is_transparent(fill) {
            // Hollow: stroke the outline and leave the background showing.
            colors::set_source(cr, outline);
            let _ = cr.stroke();
        } else {
            colors::set_source(cr, fill);
            let _ = cr.fill();
        }
    }
}

/// Open and close as ticks either side of a high-low line.
fn draw_ohlc(
    cr: &cairo::Context,
    state: &State,
    bars: &[Bar],
    plot_x: f64,
    bar_w: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let tick = (bar_w * 0.32).clamp(1.0, 10.0);
    cr.set_line_width(1.0);
    for rising in [false, true] {
        let direction = if rising { Direction::Up } else { Direction::Down };
        let mut any = false;
        for (i, bar) in bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising {
                continue;
            }
            any = true;
            let x = (plot_x + (i as f64 + 0.5) * bar_w).round() + 0.5;
            cr.move_to(x, to_y(bar.high).round());
            cr.line_to(x, to_y(bar.low).round());
            if tick > 1.0 {
                let open = to_y(bar.open).round() + 0.5;
                cr.move_to(x - tick, open);
                cr.line_to(x, open);
                let close = to_y(bar.close).round() + 0.5;
                cr.move_to(x, close);
                cr.line_to(x + tick, close);
            }
        }
        if any {
            colors::set_source(cr, state.scheme.outline(direction));
            let _ = cr.stroke();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_volume(
    cr: &cairo::Context,
    state: &State,
    bars: &[Bar],
    plot_x: f64,
    bar_w: f64,
    top: f64,
    height: f64,
    max_volume: f64,
) {
    let body_w = (bar_w * 0.68).clamp(1.0, 24.0);
    for rising in [false, true] {
        let colour = if rising { &state.scheme.volume_up } else { &state.scheme.volume_down };
        let mut any = false;
        for (i, bar) in bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising || bar.volume <= 0.0 {
                continue;
            }
            any = true;
            let h = (bar.volume / max_volume * height).max(1.0);
            let x = plot_x + (i as f64 + 0.5) * bar_w - body_w / 2.0;
            cr.rectangle(x.round(), (top + height - h).round(), body_w.round(), h.round());
        }
        if any {
            colors::set_source_alpha(cr, colour, 0.85);
            let _ = cr.fill();
        }
    }
}

/// The line between two rows of the stack.
///
/// Faint on purpose: it is there to say where one box ends and the next
/// begins, not to be read. It is also the handle — the pointer turns into a
/// resize cursor within a few pixels of it — so it has to be visible enough to
/// aim at.
fn draw_row_edge(cr: &cairo::Context, state: &State, plan: &Layout, at: usize) {
    let Some((line, _)) = plan.edge(at) else { return };
    let y = line.round() + 0.5;
    cr.save().ok();
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, &state.theme.ui.border, 0.9);
    cr.move_to(plan.plot_x, y);
    cr.line_to(plan.plot_x + plan.plot_w, y);
    let _ = cr.stroke();
    cr.restore().ok();
}

/// A strip's corner: the ways up and down the stack, and the way off the chart.
fn draw_pane_controls(cr: &cairo::Context, state: &State, plan: &Layout, row: &Row, at: usize) {
    cr.save().ok();
    cr.set_line_width(1.2);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.75);
    for control in plan.controls(at) {
        let (x, y) = plan.control_box(row, control);
        match control {
            Control::Close => draw_close(cr, x, y),
            Control::Up => draw_chevron(cr, x, y, true),
            Control::Down => draw_chevron(cr, x, y, false),
        }
    }
    cr.restore().ok();
}

/// The × that takes a strip off the chart.
fn draw_close(cr: &cairo::Context, x: f64, y: f64) {
    let inset = 3.5;
    cr.move_to(x + inset, y + inset);
    cr.line_to(x + PANE_CONTROL - inset, y + PANE_CONTROL - inset);
    cr.move_to(x + PANE_CONTROL - inset, y + inset);
    cr.line_to(x + inset, y + PANE_CONTROL - inset);
    let _ = cr.stroke();
}

/// One step up or down the stack.
///
/// A chevron rather than a filled triangle, so it carries the same weight as
/// the × beside it: two strokes of the same width in the same box. Its points
/// sit on half pixels, which is what keeps a 6-pixel mark from smearing.
fn draw_chevron(cr: &cairo::Context, x: f64, y: f64, up: bool) {
    let cx = (x + PANE_CONTROL / 2.0).floor() + 0.5;
    let near = (y + 4.5).floor() + 0.5;
    let far = (y + 8.5).floor() + 0.5;
    let (apex, arms) = if up { (near, far) } else { (far, near) };
    let arm = 3.0;
    cr.move_to(cx - arm, arms);
    cr.line_to(cx, apex);
    cr.line_to(cx + arm, arms);
    let _ = cr.stroke();
}

/// What a strip is, written in its own top-left corner.
fn draw_pane_name(cr: &cairo::Context, state: &State, drawn: &Drawn, plot_x: f64, top: f64) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(10.0);
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.8);
    cr.move_to(plot_x + 2.0, top + 11.0);
    let _ = cr.show_text(&drawn.indicator.label_for(state.timeframe));
}

/// One indicator in its own strip: guides, then the line.
///
/// The strip carries its own name because the legend at the top cannot say
/// which of three stacked panes is which, and a pane you have to count down to
/// identify is a pane you misread.
#[allow(clippy::too_many_arguments)]
fn draw_pane(
    cr: &cairo::Context,
    state: &State,
    pane: &omacharts_engine::indicators::Pane,
    drawn: &Drawn,
    columns: &Columns,
    plot_w: f64,
    top: f64,
    height: f64,
) {
    let plot_x = columns.plot_x;
    let (first, visible) = (columns.first, columns.visible);
    // Every visible value, not one per column: what the strip is scaled to has
    // to be the range the line actually covers, or a peak that falls between
    // two columns would push the line off the top of its own strip.
    let values = &pane.values[first.min(pane.values.len())..(first + visible).min(pane.values.len())];
    let (low, high) = match pane.bounds {
        Some(bounds) => bounds,
        // Fit what is on screen, with a little air: an ATR pressed against the
        // top and bottom of its strip has no shape to read.
        None => {
            let mut low = f64::MAX;
            let mut high = f64::MIN;
            for value in values.iter().flatten() {
                low = low.min(*value);
                high = high.max(*value);
            }
            if !low.is_finite() || !high.is_finite() {
                return;
            }
            if (high - low).abs() < f64::EPSILON {
                (low - 1.0, high + 1.0)
            } else {
                let air = (high - low) * 0.12;
                (low - air, high + air)
            }
        }
    };
    let to_y = |value: f64| top + height * (high - value) / (high - low);

    // The band between the guides, so overbought and oversold read as regions
    // rather than two lines you have to remember the meaning of.
    if let Some((from, to)) = pane.band {
        cr.rectangle(plot_x, to_y(to), plot_w, (to_y(from) - to_y(to)).abs());
        colors::set_source_alpha(cr, &state.theme.ui.grid, 0.35);
        let _ = cr.fill();
    }

    cr.save().ok();
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, &state.theme.ui.grid, 0.9);
    for guide in &pane.guides {
        let y = to_y(*guide).round() + 0.5;
        cr.move_to(plot_x, y);
        cr.line_to(plot_x + plot_w, y);
    }
    let _ = cr.stroke();
    cr.restore().ok();

    // The scale, written where the price axis is written. Fixed scales label
    // their guides; a fitted one labels its extremes, which is the only way to
    // know what the line is worth.
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(10.0);
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.75);
    let marks: Vec<(f64, String)> = if pane.bounds.is_some() {
        pane.guides.iter().map(|g| (*g, format!("{g:.0}"))).collect()
    } else {
        let decimals = decimals_for(nice_step(high - low, 3));
        vec![(low, format!("{low:.decimals$}")), (high, format!("{high:.decimals$}"))]
    };
    for (value, text) in marks {
        let y = to_y(value);
        if y < top || y > top + height {
            continue;
        }
        cr.move_to(plot_x + plot_w + 6.0, (y + 3.0).min(top + height));
        let _ = cr.show_text(&text);
    }

    draw_pane_name(cr, state, drawn, plot_x, top);

    // The line last, over its own furniture.
    let stroke = drawn.indicator.stroke;
    if stroke.is_hidden() {
        return;
    }
    cr.save().ok();
    cr.set_line_width(stroke.width);
    cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
    colors::set_source(cr, &drawn.color);
    let mut pen_down = false;
    for at in 0..columns.len() {
        let Some(Some(value)) = values.get(columns.offset(at)) else {
            pen_down = false;
            continue;
        };
        let x = columns.x(at);
        let y = to_y(*value).clamp(top, top + height);
        if pen_down {
            cr.line_to(x, y);
        } else {
            cr.move_to(x, y);
            pen_down = true;
        }
    }
    let _ = cr.stroke();
    cr.restore().ok();
}

#[allow(clippy::too_many_arguments)]
fn draw_last_price(
    cr: &cairo::Context,
    state: &State,
    plot_x: f64,
    plot_w: f64,
    price_y: f64,
    width: f64,
    decimals: usize,
    to_y: &impl Fn(f64) -> f64,
) {
    let Some(last) = state.bars.last() else { return };
    let y = to_y(last.close).round() + 0.5;
    if y < price_y {
        return;
    }
    let rising = last.close >= last.open;
    let colour = if rising { &state.scheme.up } else { &state.scheme.down };

    cr.save().ok();
    cr.set_dash(&[3.0, 3.0], 0.0);
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, colour, 0.7);
    cr.move_to(plot_x, y);
    cr.line_to(plot_x + plot_w, y);
    let _ = cr.stroke();
    cr.restore().ok();

    label_on_axis(
        cr,
        state,
        &format!("{:.*}", decimals, last.close),
        plot_x + plot_w,
        y,
        width,
        colour,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_crosshair(
    cr: &cairo::Context,
    state: &State,
    px: f64,
    py: f64,
    plot_x: f64,
    plot_w: f64,
    top: f64,
    price_y: f64,
    price_h: f64,
    width: f64,
    height: f64,
    bar_w: f64,
    first: usize,
    low: f64,
    high: f64,
) {
    if px < plot_x || px > plot_x + plot_w || py < top || py > height - TIME_AXIS_H {
        return;
    }
    let crosshair = &state.theme.ui.crosshair;

    // Snap to the centre of the bar under the pointer.
    let index_in_view = ((px - plot_x) / bar_w).floor();
    let snapped_x = (plot_x + (index_in_view + 0.5) * bar_w).round() + 0.5;

    cr.save().ok();
    cr.set_dash(&[2.0, 3.0], 0.0);
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, crosshair, 0.55);
    cr.move_to(snapped_x, top);
    cr.line_to(snapped_x, height - TIME_AXIS_H);
    cr.move_to(plot_x, py.round() + 0.5);
    cr.line_to(plot_x + plot_w, py.round() + 0.5);
    let _ = cr.stroke();
    cr.restore().ok();

    // Price under the pointer, only while it is over the price plot.
    if py >= price_y && py <= price_y + price_h {
        let price = high - (py - price_y) / price_h * (high - low);
        let step = nice_step(high - low, (price_h / 52.0).max(2.0) as usize);
        let decimals = omacharts_engine::price_decimals(
            step,
            price,
            state.instrument.as_ref().map(|i| i.kind),
        );
        label_on_axis(
            cr,
            state,
            &format!("{price:.decimals$}"),
            plot_x + plot_w,
            py.round() + 0.5,
            width,
            crosshair,
        );
    }

    // Time under the pointer.
    if let Some(bar) = state.bars.get(first + index_in_view.max(0.0) as usize) {
        let label = format_time_full(bar.ts, state.timeframe);
        cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(11.0);
        if let Ok(extents) = cr.text_extents(&label) {
            let w = extents.width() + 10.0;
            // Split a window enough times and the label is wider than the
            // chart it belongs to, which puts the right edge left of the left
            // one. `clamp` panics on that, inside a draw callback, which does
            // not unwind — so the whole app goes rather than one chart drawing
            // badly for a moment.
            let x = (snapped_x - w / 2.0).clamp(plot_x, (plot_x + plot_w - w).max(plot_x));
            let y = height - TIME_AXIS_H + 2.0;
            colors::set_source(cr, crosshair);
            cr.rectangle(x, y, w, TIME_AXIS_H - 4.0);
            let _ = cr.fill();
            colors::set_source(cr, colors::readable_on(crosshair));
            cr.move_to(x + 5.0, y + TIME_AXIS_H - 10.0);
            let _ = cr.show_text(&label);
        }
    }
}

/// A filled chip on the price axis.
fn label_on_axis(
    cr: &cairo::Context,
    state: &State,
    text: &str,
    axis_x: f64,
    y: f64,
    width: f64,
    colour: &str,
) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
    let Ok(extents) = cr.text_extents(text) else { return };
    let h = 16.0;
    let w = (extents.width() + 10.0).min(width - axis_x - 2.0);
    colors::set_source(cr, colour);
    cr.rectangle(axis_x + 1.0, y - h / 2.0, w, h);
    let _ = cr.fill();
    colors::set_source(cr, colors::readable_on(colour));
    cr.move_to(axis_x + 6.0, y + 3.5);
    let _ = cr.show_text(text);
    let _ = state;
}

/// Another chart's pointer: the same dashes as the crosshair, both lines, and
/// quieter — it is somebody else's pointer, not yours.
///
/// The time half can miss — this chart may not be showing that moment at all
/// — while the price half always draws and is simply clipped to the price
/// pane when it falls outside it. Clipping rather than testing on purpose: a
/// line nudged to the nearest edge would be pointing at a price that is not
/// the price, and leaving it out would hide the very thing worth carrying
/// across.
#[allow(clippy::too_many_arguments)]
fn draw_echo(
    cr: &cairo::Context,
    state: &State,
    bars: &[Bar],
    plot_x: f64,
    plot_w: f64,
    bar_w: f64,
    top: f64,
    price_y: f64,
    price_h: f64,
    height: f64,
    echo: Echo,
    to_y: &impl Fn(f64) -> f64,
) {
    cr.save().ok();
    cr.set_dash(&[2.0, 3.0], 0.0);
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, &state.theme.ui.crosshair, 0.3);

    if let Some(index) = nearest_bar(bars, echo.ts) {
        let x = (plot_x + (index as f64 + 0.5) * bar_w).round() + 0.5;
        cr.move_to(x, top);
        cr.line_to(x, height - TIME_AXIS_H);
        let _ = cr.stroke();
    }

    cr.rectangle(plot_x, price_y, plot_w, price_h);
    cr.clip();
    let y = to_y(echo.price).round() + 0.5;
    cr.move_to(plot_x, y);
    cr.line_to(plot_x + plot_w, y);
    let _ = cr.stroke();
    cr.restore().ok();
}

fn now_list(list: &[Drawing], at: usize) -> Option<&Drawing> {
    list.get(at)
}

fn now_id(drawing: &Option<&Drawing>) -> Option<i64> {
    drawing.map(|d| d.id).filter(|id| *id != 0)
}

/// The configuration a number key names, 1 to 9, from the main row or the
/// keypad. Zero names nothing: there are nine.
fn preset_number(key: gtk::gdk::Key) -> Option<usize> {
    use gtk::gdk::Key;
    Some(match key {
        Key::_1 | Key::KP_1 => 1,
        Key::_2 | Key::KP_2 => 2,
        Key::_3 | Key::KP_3 => 3,
        Key::_4 | Key::KP_4 => 4,
        Key::_5 | Key::KP_5 => 5,
        Key::_6 | Key::KP_6 => 6,
        Key::_7 | Key::KP_7 => 7,
        Key::_8 | Key::KP_8 => 8,
        Key::_9 | Key::KP_9 => 9,
        _ => return None,
    })
}

/// Half the side of a grip, the small square at a selected drawing's anchor.
const GRIP_HALF: f64 = 3.5;

/// What has been drawn on the symbol, over the candles and the indicator
/// lines and under the strips, clipped to the price plot so a line drawn
/// off the top does not cross an RSI.
fn draw_drawings(cr: &cairo::Context, state: &State, plan: &Layout, low: f64, high: f64) {
    if state.drawings.is_empty() {
        return;
    }
    cr.save().ok();
    cr.rectangle(plan.plot_x, plan.price_y, plan.plot_w, plan.price_h);
    cr.clip();
    for (index, drawing) in state.drawings.iter().enumerate() {
        let projected = state.project(plan, low, high, drawing);
        draw_drawing(cr, state, &projected, drawing, state.selected.contains(&index));
    }
    cr.restore().ok();
}

/// One drawing, as its configuration or its own look says, on this theme.
///
/// A line is its colour at its width, with an arrowhead at whichever ends
/// want one. A rectangle is its fill at its alpha under an edge at less than
/// half strength — the numbers behind the defaults are in the engine,
/// measured over every theme — so the candles read through it and the edge
/// is the edge of the shape rather than a line drawn around it. A selected
/// drawing wears a grip at each of its corners.
fn draw_drawing(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    selected: bool,
) {
    let style = drawing.style(&state.configs);
    let colour = style.colour.hex(&state.theme);
    match drawing.kind {
        DrawingKind::Line => {
            colors::set_source(cr, &colour);
            cr.set_line_width(style.width);
            cr.set_line_cap(cairo::LineCap::Round);
            cr.move_to(projected.from.0, projected.from.1);
            cr.line_to(projected.to.0, projected.to.1);
            let _ = cr.stroke();
            if style.arrow.at_end() {
                arrowhead(cr, projected.from, projected.to, style.width);
            }
            if style.arrow.at_start() {
                arrowhead(cr, projected.to, projected.from, style.width);
            }
        }
        DrawingKind::Rect => {
            let (x, y, w, h) = projected.bounds();
            colors::set_source_alpha(cr, &style.fill.hex(&state.theme), style.alpha);
            cr.rectangle(x, y, w, h);
            let _ = cr.fill();
            if style.border {
                // On the pixel grid, so a one-pixel edge is one pixel.
                let width = style.width.max(1.0);
                colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
                cr.set_line_width(width);
                let inset = if (width.round() as i64) % 2 == 1 { 0.5 } else { 0.0 };
                cr.rectangle(x.round() + inset, y.round() + inset, w.round().max(1.0), h.round().max(1.0));
                let _ = cr.stroke();
            }
        }
    }
    if selected {
        for grip in drawing.grips() {
            let Some((x, y)) = projected.grip(*grip) else { continue };
            colors::set_source(cr, &state.theme.ui.background);
            cr.rectangle(
                x - GRIP_HALF - 1.0,
                y - GRIP_HALF - 1.0,
                2.0 * GRIP_HALF + 2.0,
                2.0 * GRIP_HALF + 2.0,
            );
            let _ = cr.fill();
            colors::set_source(cr, &colour);
            cr.rectangle(x - GRIP_HALF, y - GRIP_HALF, 2.0 * GRIP_HALF, 2.0 * GRIP_HALF);
            let _ = cr.fill();
        }
    }
}

/// A filled arrowhead at `tip`, pointing away from `tail`, sized to the
/// line's width so a thick line gets a head to match.
fn arrowhead(cr: &cairo::Context, tail: (f64, f64), tip: (f64, f64), width: f64) {
    let (dx, dy) = (tip.0 - tail.0, tip.1 - tail.1);
    let length = dx.hypot(dy);
    if length < 1.0 {
        return;
    }
    let (ux, uy) = (dx / length, dy / length);
    let size = 6.0 + 3.0 * width;
    let (bx, by) = (tip.0 - ux * size, tip.1 - uy * size);
    let (px, py) = (-uy * size * 0.45, ux * size * 0.45);
    cr.move_to(tip.0, tip.1);
    cr.line_to(bx + px, by + py);
    cr.line_to(bx - px, by - py);
    cr.close_path();
    let _ = cr.fill();
}

/// The visible bar closest in time to `ts`, if any is near enough to mean it.
fn nearest_bar(bars: &[Bar], ts: i64) -> Option<usize> {
    if bars.is_empty() {
        return None;
    }
    let (mut best, mut gap) = (0usize, i64::MAX);
    for (i, bar) in bars.iter().enumerate() {
        let delta = (bar.ts - ts).abs();
        if delta < gap {
            best = i;
            gap = delta;
        }
    }
    // Past a couple of bars' worth the echo is pointing at a time this chart
    // is not showing, and a line at the nearest edge would be a lie.
    let step = if bars.len() > 1 { (bars[1].ts - bars[0].ts).abs().max(1) } else { 1 };
    (gap <= step * 2).then_some(best)
}

/// Half the width of the trouble dot. Six pixels across: four reads as a
/// stray wick, eight as a button.
const TROUBLE_DOT_RADIUS: f64 = 3.0;

/// How near the pointer has to be to the dot for its tooltip to open. Wider
/// than the dot, since nobody aims at six pixels.
const TROUBLE_DOT_REACH: f64 = 9.0;

/// Where the trouble dot sits: the top-right of the plot, inside it and
/// clear of the price axis, so it never lands on a label.
fn trouble_dot_centre(width: f64) -> (f64, f64) {
    (width - PRICE_AXIS_W - PAD - TROUBLE_DOT_RADIUS, PAD + TROUBLE_DOT_RADIUS + 2.0)
}

/// The dot in the top corner of a chart that has bars but could not get
/// newer ones. What is drawn is real and out of date; the dot says so and
/// its tooltip says why. Nothing is drawn while all is well, since a chart
/// that is up to date has nothing to flag. Solid rather than pulsing: a
/// layout of eight panes and a provider having a bad minute would otherwise
/// blink at the user.
///
/// The colour is the theme's Rose — the red its down candles wear, so the
/// alarm belongs to the theme rather than being a traffic light pasted on —
/// and not the bar scheme's down colour, which the monochrome scheme makes
/// a grey and no alarm at all. A ring of the chart background keeps it a dot
/// over a red candle or the crosshair.
fn draw_trouble_dot(cr: &cairo::Context, state: &State, width: f64) {
    let (x, y) = trouble_dot_centre(width);
    let rose = state.theme.swatch("Rose").map(|s| s.hex.as_str()).unwrap_or(&state.scheme.down);
    colors::set_source(cr, &state.theme.ui.background);
    cr.arc(x, y, TROUBLE_DOT_RADIUS + 1.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
    colors::set_source(cr, rose);
    cr.arc(x, y, TROUBLE_DOT_RADIUS, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}

/// Is the pointer on the trouble dot? Only while there is one: a chart that
/// is up to date, or one with no bars whose placeholder already says what is
/// wrong, has no dot and no tooltip.
fn over_trouble_dot(state: &State, width: f64, x: f64, y: f64) -> bool {
    if state.trouble.is_none() || state.slice().1 == 0 {
        return false;
    }
    let (cx, cy) = trouble_dot_centre(width);
    (x - cx).hypot(y - cy) <= TROUBLE_DOT_REACH
}

/// Band shading and volume profiles, beneath the bars.
fn draw_indicator_fills(
    cr: &cairo::Context,
    state: &State,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    for drawn in state.indicators.iter().filter(|d| d.indicator.visible) {
        match &drawn.output {
            Output::Bands(bands) => {
                // One shaded region, between the first band's own edges: the
                // area price spends most of its time in, drawn once. Shading
                // every band stacks into a wash that says less than the lines.
                for band in bands.bands.iter() {
                    if !band.enabled || !band.fill {
                        continue;
                    }
                    let colour = band_colour(band, drawn, &state.theme);
                    colors::set_source_alpha(cr, &colour, band.fill_alpha);
                    fill_between(cr, &band.upper, &band.lower, columns, to_y);
                }
            }
            Output::Profiles(profiles) => {
                draw_profiles(cr, state, drawn, profiles, columns, to_y);
            }
            // These draw in a strip of their own, where the price scale does
            // not reach, so there is nothing to do over the candles.
            Output::Volume { .. } | Output::Pane(_) | Output::Line(_) => {}
        }
    }
}

/// Indicator lines, over the bars.
fn draw_indicator_lines(
    cr: &cairo::Context,
    state: &State,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    for drawn in state.indicators.iter().filter(|d| d.indicator.visible) {
        match &drawn.output {
            Output::Line(values) => {
                let stroke = drawn.indicator.stroke;
                if stroke.is_hidden() {
                    continue;
                }
                cr.save().ok();
                cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
                colors::set_source(cr, &drawn.color);
                cr.set_line_width(stroke.width);
                stroke_series(cr, values, columns, to_y);
                cr.restore().ok();
            }
            Output::Bands(bands) => {
                cr.save().ok();
                for band in bands.bands.iter() {
                    // A width of zero means no line, which is how a shaded
                    // band gets a clean edge without painting one in the
                    // background colour.
                    if !band.enabled || band.stroke.is_hidden() {
                        continue;
                    }
                    cr.set_dash(&band.stroke.style.dashes(band.stroke.width), 0.0);
                    cr.set_line_width(band.stroke.width);
                    colors::set_source(cr, &band_colour(band, drawn, &state.theme));
                    stroke_series(cr, &band.upper, columns, to_y);
                    stroke_series(cr, &band.lower, columns, to_y);
                }
                cr.restore().ok();

                let stroke = drawn.indicator.stroke;
                if !stroke.is_hidden() {
                    cr.save().ok();
                    cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
                    colors::set_source(cr, &drawn.color);
                    cr.set_line_width(stroke.width);
                    stroke_series(cr, &bands.vwap, columns, to_y);
                    cr.restore().ok();
                }
            }
            Output::Profiles(_) | Output::Volume { .. } | Output::Pane(_) => {}
        }
    }
}

/// A band's own colour, or the indicator's if it has none.
fn band_colour(band: &vwap::BandSeries, drawn: &Drawn, theme: &Theme) -> String {
    band.color
        .as_ref()
        .map(|choice| choice.resolve(theme))
        .unwrap_or_else(|| drawn.color.clone())
}

/// One histogram per period, anchored where its period begins.
fn draw_profiles(
    cr: &cairo::Context,
    state: &State,
    drawn: &Drawn,
    profiles: &[Profile],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let poc = match &drawn.indicator.params {
        omacharts_engine::Params::VolumeProfile { poc_color: Some(choice), .. } => {
            choice.resolve(&state.theme)
        }
        _ => state.theme.companion(&drawn.color),
    };

    let (first, last) = (columns.first, columns.first + columns.visible);
    for profile in profiles {
        if profile.last_bar < first || profile.first_bar >= last {
            continue;
        }
        let from = profile.first_bar.max(first);
        let to = profile.last_bar.min(last - 1) + 1;
        let left = columns.left_of(from);
        // A profile may use at most this much of its own period's width, so it
        // describes the bars rather than burying them.
        let span = columns.width_of(from, to).max(columns.bar_w) * 0.4;

        let (area_low, area_high) = profile.value_area_bounds();
        for row in &profile.rows {
            if row.volume <= 0.0 || profile.max_volume <= 0.0 {
                continue;
            }
            let width = span * (row.volume / profile.max_volume);
            let top = to_y(row.high);
            let height = (to_y(row.low) - top).max(1.0);
            let inside = row.low >= area_low && row.high <= area_high;
            colors::set_source_alpha(cr, &drawn.color, if inside { 0.30 } else { 0.14 });
            cr.rectangle(left, top, width, height.max(1.0));
            let _ = cr.fill();
        }

        // The point of control, across the period it belongs to, in a colour
        // set against the profile rather than the same one brighter — it is a
        // different fact about the period, drawn on top of it.
        let y = to_y(profile.poc_price()).round() + 0.5;
        colors::set_source_alpha(cr, &poc, 0.9);
        cr.set_line_width(1.0);
        cr.move_to(left, y);
        cr.line_to(left + span, y);
        let _ = cr.stroke();
    }
}

/// Stroke a series, breaking the path wherever it has no value.
fn stroke_series(
    cr: &cairo::Context,
    values: &[Option<f64>],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let mut drawing = false;
    for at in 0..columns.len() {
        let x = columns.x(at);
        match values.get(columns.source(at)).copied().flatten() {
            Some(value) => {
                let y = to_y(value);
                if drawing {
                    cr.line_to(x, y);
                } else {
                    cr.move_to(x, y);
                    drawing = true;
                }
            }
            // A gap is a gap. Joining across it would draw a line through
            // prices that were never there.
            None => drawing = false,
        }
    }
    let _ = cr.stroke();
}

/// Fill the region between two series.
fn fill_between(
    cr: &cairo::Context,
    upper: &[Option<f64>],
    lower: &[Option<f64>],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let mut run: Vec<(f64, f64, f64)> = Vec::new();
    let flush = |run: &mut Vec<(f64, f64, f64)>| {
        if run.len() < 2 {
            run.clear();
            return;
        }
        cr.move_to(run[0].0, run[0].1);
        for (x, y, _) in run.iter().skip(1) {
            cr.line_to(*x, *y);
        }
        for (x, _, y2) in run.iter().rev() {
            cr.line_to(*x, *y2);
        }
        cr.close_path();
        let _ = cr.fill();
        run.clear();
    };

    for at in 0..columns.len() {
        let x = columns.x(at);
        match (
            upper.get(columns.source(at)).copied().flatten(),
            lower.get(columns.source(at)).copied().flatten(),
        ) {
            (Some(a), Some(b)) => run.push((x, to_y(a), to_y(b))),
            _ => flush(&mut run),
        }
    }
    flush(&mut run);
}

// ---------------------------------------------------------------------------
// Scales and formatting
// ---------------------------------------------------------------------------

/// A round step — 1, 2, 2.5 or 5 times a power of ten — giving roughly
/// `target` gridlines across `range`.
pub fn nice_step(range: f64, target: usize) -> f64 {
    if range <= 0.0 || target == 0 {
        return 0.0;
    }
    let rough = range / target as f64;
    let magnitude = 10f64.powf(rough.log10().floor());
    let normalised = rough / magnitude;
    let step = if normalised <= 1.0 {
        1.0
    } else if normalised <= 2.0 {
        2.0
    } else if normalised <= 2.5 {
        2.5
    } else if normalised <= 5.0 {
        5.0
    } else {
        10.0
    };
    step * magnitude
}

/// Enough decimals to tell two adjacent gridlines apart, and no more.
pub fn decimals_for(step: f64) -> usize {
    if step <= 0.0 {
        return 2;
    }
    let places = -step.log10().floor();
    places.clamp(0.0, 6.0) as usize
}

/// Label an axis tick at the detail the gap between ticks justifies.
///
/// The gap, not the whole span: twenty ticks across nine months are about a
/// fortnight apart, and labelling each of them "Mar 2026" prints the same
/// thing three times running. What decides the format is how far apart two
/// neighbouring labels are, because that is what has to tell them apart.
fn format_axis_time(ts: i64, tick_seconds: i64, intraday: bool) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    const DAY: i64 = 86_400;
    if intraday {
        return match tick_seconds {
            t if t >= DAY => dt.format("%d %b %H:%M").to_string(),
            t if t >= 4 * 3_600 => dt.format("%a %H:%M").to_string(),
            _ => dt.format("%H:%M").to_string(),
        };
    }
    match tick_seconds {
        t if t >= 300 * DAY => dt.format("%Y").to_string(),
        t if t >= 25 * DAY => dt.format("%b %Y").to_string(),
        _ => dt.format("%d %b").to_string(),
    }
}

/// The crosshair label always carries the year — it is the one place you look
/// to know exactly which bar you are on.
fn format_time_full(ts: i64, timeframe: Timeframe) -> String {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    if timeframe.is_intraday() {
        dt.format("%d %b %Y %H:%M").to_string()
    } else {
        dt.format("%a %d %b %Y").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug: a chart that could not fetch told the user the symbol had no
    /// data, which on a first run reads as the app not having the S&P 500.
    #[test]
    fn a_chart_that_could_not_fetch_does_not_blame_the_symbol() {
        assert_eq!(
            placeholder_text(true, false, Some(FetchFailure::Offline)),
            FetchFailure::Offline.message()
        );
        assert_ne!(
            placeholder_text(true, false, Some(FetchFailure::RateLimited)),
            "No data for this symbol"
        );
    }

    /// The one case the old line was true for, and the one it keeps.
    #[test]
    fn a_provider_that_answered_with_nothing_still_says_there_is_no_data() {
        assert_eq!(placeholder_text(true, false, None), "No data for this symbol");
    }

    #[test]
    fn a_retry_in_flight_outranks_the_failure_it_is_retrying() {
        assert_eq!(placeholder_text(true, true, Some(FetchFailure::Unreachable)), "Loading…");
    }

    #[test]
    fn a_chart_with_no_symbol_on_it_asks_for_one_whatever_else_went_wrong() {
        assert_eq!(
            placeholder_text(false, false, Some(FetchFailure::Offline)),
            "Press Ctrl+K to find a symbol"
        );
    }

    /// A chart 500 pixels of stack tall, with its rows in this order. `None`
    /// is the price plot, which is always one of them.
    fn with_rows(order: &[Option<u32>]) -> Layout {
        let at = order.iter().position(|row| row.is_none()).expect("the price is always a row");
        let share = |rows: &[Option<u32>]| -> Vec<(u32, f64)> {
            rows.iter().flatten().map(|id| (*id, 0.15)).collect()
        };
        let stacked = plan_rows(&share(&order[..at]), &share(&order[at + 1..]), PAD, 500.0);
        Layout {
            plot_x: PAD,
            plot_w: 400.0,
            top: PAD,
            total_h: 500.0,
            price_y: stacked.price_y,
            price_h: stacked.price_h,
            rows: stacked.rows,
        }
    }

    fn order(plan: &Layout) -> Vec<Option<u32>> {
        plan.rows.iter().map(|row| row.pane).collect()
    }

    fn bottom(plan: &Layout) -> f64 {
        plan.rows.last().map(|row| row.top + row.height).unwrap_or_default()
    }

    #[test]
    fn strips_stack_in_the_order_they_are_given_around_the_price() {
        let plan = with_rows(&[Some(1), None, Some(2), Some(3)]);
        assert_eq!(order(&plan), vec![Some(1), None, Some(2), Some(3)]);
        // Top to bottom, with a gap between each.
        for pair in plan.rows.windows(2) {
            assert!(
                (pair[1].top - (pair[0].top + pair[0].height) - PANE_GAP).abs() < 1e-9,
                "rows should be one gap apart"
            );
        }
    }

    #[test]
    fn a_strip_above_the_price_starts_at_the_top_of_the_chart() {
        let plan = with_rows(&[Some(1), Some(2), None]);
        assert_eq!(plan.rows[0].top, PAD, "nothing sits above the first row");
        assert!(plan.price_y > plan.rows[1].top, "and the price has moved down to the bottom");
    }

    /// The price plot is whatever the strips leave, so somebody dragging a
    /// strip to fill the window would otherwise take the candles to nothing —
    /// wherever in the stack the price has ended up.
    #[test]
    fn the_price_keeps_its_floor_however_tall_the_strip_above_it_is() {
        for order in [vec![Some(1), None], vec![None, Some(1)]] {
            let at = order.iter().position(|row| row.is_none()).unwrap();
            let share = |rows: &[Option<u32>]| -> Vec<(u32, f64)> {
                rows.iter().flatten().map(|id| (*id, indicators::MAX_PANE_SHARE)).collect()
            };
            let stacked = plan_rows(&share(&order[..at]), &share(&order[at + 1..]), PAD, 500.0);
            // The floor, less the one gap that has to come out of somewhere.
            assert!(
                stacked.price_h + PANE_GAP + 0.5 >= 500.0 * MIN_PRICE_SHARE,
                "the price was squeezed to {}",
                stacked.price_h
            );
        }
    }

    /// The axis belongs to the whole stack, not to the price plot, so the rows
    /// have to end exactly where [`layout`] leaves room for it — whichever row
    /// is last.
    #[test]
    fn the_stack_ends_where_the_time_axis_starts_wherever_the_price_sits() {
        for rows in [
            vec![None],
            vec![None, Some(1)],
            vec![Some(1), None],
            vec![Some(1), None, Some(2), Some(3)],
            vec![Some(1), Some(2), Some(3), None],
        ] {
            let plan = with_rows(&rows);
            let ends_at = bottom(&plan);
            assert!((ends_at - (PAD + 500.0)).abs() < 1e-9, "{rows:?} ended at {ends_at}");
        }
    }

    #[test]
    fn the_strip_at_either_end_of_the_stack_is_offered_no_way_out_of_it() {
        let plan = with_rows(&[Some(1), None, Some(2)]);
        let first = plan.controls(0);
        assert_eq!(first, vec![Control::Down, Control::Close], "nothing above the first row");
        assert_eq!(plan.controls(2), vec![Control::Up, Control::Close], "nothing below the last");
    }

    /// One strip and the price is still two rows, so there is always somewhere
    /// to go: the strip can cross the price even with nothing else on the
    /// chart.
    #[test]
    fn the_only_strip_on_a_chart_is_still_offered_the_way_across_the_price() {
        let under = with_rows(&[None, Some(1)]);
        assert_eq!(under.controls(1), vec![Control::Up, Control::Close]);
        let over = with_rows(&[Some(1), None]);
        assert_eq!(over.controls(0), vec![Control::Down, Control::Close]);
    }

    /// Whichever arrows a strip offers, they sit in the same places: the boxes
    /// are reserved rather than packed, so the one you are pointing at does not
    /// move out from under the pointer when the strip travels.
    #[test]
    fn a_corner_control_sits_in_the_same_place_whichever_are_offered() {
        let plan = with_rows(&[Some(1), None, Some(2)]);
        let top = plan.control_box(&plan.rows[0], Control::Down);
        let middle = plan.control_box(&plan.rows[2], Control::Down);
        assert_eq!(top.0, middle.0, "the same column in every strip");
        let (close, down, up) = (
            plan.control_box(&plan.rows[0], Control::Close).0,
            plan.control_box(&plan.rows[0], Control::Down).0,
            plan.control_box(&plan.rows[0], Control::Up).0,
        );
        assert!(up < down && down < close, "up, down, close, reading left to right");
        assert!(close + PANE_CONTROL < plan.plot_x + plan.plot_w, "inside the plot");
    }

    /// The hover highlight, the cursor and the click all ask the same question,
    /// so the only thing that could put them out of step is the boxes moving
    /// between the drawing and the hit test.
    #[test]
    fn a_click_in_the_middle_of_a_box_finds_that_box() {
        let plan = with_rows(&[Some(1), None, Some(2)]);
        for (at, id) in [(0usize, 1u32), (2, 2)] {
            for control in plan.controls(at) {
                let (x, y) = plan.control_box(&plan.rows[at], control);
                let hit = plan.control_at(x + PANE_CONTROL / 2.0, y + PANE_CONTROL / 2.0);
                assert_eq!(hit, Some((id, control)), "row {at} {control:?}");
            }
        }
        // And an arrow that is not offered is not clickable either.
        let (x, y) = plan.control_box(&plan.rows[0], Control::Up);
        assert_eq!(plan.control_at(x + PANE_CONTROL / 2.0, y + PANE_CONTROL / 2.0), None);
    }

    #[test]
    fn a_strips_corner_is_only_live_inside_that_strip() {
        let plan = with_rows(&[None, Some(1)]);
        let (x, y) = plan.control_box(&plan.rows[1], Control::Close);
        assert!(plan.control_at(x + 6.0, y + 6.0).is_some());
        // The same column, up in the price plot, where there is no close box.
        assert_eq!(plan.control_at(x + 6.0, plan.price_y + 6.0), None);
    }

    /// The price plot has no height of its own — it is whatever the strips
    /// leave — so the line above it has to resize the strip above it instead.
    #[test]
    fn the_line_above_the_price_resizes_the_strip_above_it() {
        let plan = with_rows(&[Some(1), None, Some(2)]);
        let price_line = plan.edge(1).expect("the price has a line above it");
        assert_eq!(price_line.1, Edge { id: 1, grows_downward: true });
        let strip_line = plan.edge(2).expect("so does the strip under it");
        assert_eq!(strip_line.1, Edge { id: 2, grows_downward: false });
        assert!(plan.edge(0).is_none(), "the topmost row has no line above it");
    }

    #[test]
    fn a_line_is_grabbed_from_either_side_of_it() {
        let plan = with_rows(&[None, Some(1)]);
        let (line, edge) = plan.edge(1).unwrap();
        assert_eq!(plan.edge_at(line - EDGE_GRAB + 0.5), Some(edge));
        assert_eq!(plan.edge_at(line + EDGE_GRAB - 0.5), Some(edge));
        assert_eq!(plan.edge_at(line + EDGE_GRAB * 4.0), None);
    }

    /// A series with a different high and low in every bar, so a column that
    /// dropped one can be told from a column that kept it.
    fn ramp(count: usize) -> Vec<Bar> {
        (0..count)
            .map(|i| {
                let base = 100.0 + i as f64;
                Bar {
                    ts: 1_600_000_000 + i as i64 * 60,
                    open: base,
                    high: base + (i % 7) as f64 + 1.0,
                    low: base - (i % 5) as f64 - 1.0,
                    close: base + 0.5,
                    volume: (i + 1) as f64,
                }
            })
            .collect()
    }

    fn highest(bars: &[Bar]) -> f64 {
        bars.iter().map(|bar| bar.high).fold(f64::MIN, f64::max)
    }

    fn lowest(bars: &[Bar]) -> f64 {
        bars.iter().map(|bar| bar.low).fold(f64::MAX, f64::min)
    }

    /// The case every chart at a normal zoom is in, and the one that has to
    /// stay exactly as it was: a pixel each, nothing aggregated, the bars
    /// drawn as they came.
    #[test]
    fn a_plot_with_room_for_every_bar_draws_every_bar() {
        let bars = ramp(160);
        let columns = Columns::of(&bars, 40, 160, PAD, 1500.0);
        assert_eq!(columns.len(), 160, "nothing to aggregate at a pixel each");
        assert_eq!(&*columns.bars, &bars[..], "and the bars are the bars");
        assert!((columns.bar_w - 1500.0 / 160.0).abs() < 1e-9);
        for at in 0..columns.len() {
            assert_eq!(columns.source(at), 40 + at, "column {at}");
            assert!((columns.x(at) - (PAD + (at as f64 + 0.5) * columns.bar_w)).abs() < 1e-9);
        }
    }

    /// The whole claim the aggregation rests on: a column is the union of what
    /// it stands for, so no high and no low can go missing however far out the
    /// chart is zoomed.
    #[test]
    fn a_column_keeps_the_extremes_of_every_bar_it_stands_for() {
        let bars = ramp(3000);
        let columns = Columns::of(&bars, 0, 3000, PAD, 1500.0);
        assert_eq!(columns.len(), 1500, "a column a pixel");
        for (at, column) in columns.bars.iter().enumerate() {
            let group = &bars[at * 2..at * 2 + 2];
            assert_eq!(column.ts, group[0].ts, "a column starts when its first bar does");
            assert_eq!(column.open, group[0].open, "first open");
            assert_eq!(column.close, group[1].close, "last close");
            assert_eq!(column.high, highest(group), "highest high");
            assert_eq!(column.low, lowest(group), "lowest low");
            assert_eq!(column.volume, group[0].volume + group[1].volume, "summed volume");
        }
        // Which is also what keeps the price axis the same axis: the scale is
        // worked out from the highest high and the lowest low on screen.
        assert_eq!(highest(&columns.bars), highest(&bars));
        assert_eq!(lowest(&columns.bars), lowest(&bars));
    }

    /// Bars rarely divide evenly into pixels. The columns still have to tile
    /// the slice — no bar in two of them, none in none of them.
    #[test]
    fn columns_tile_the_bars_even_when_the_two_do_not_divide() {
        let bars = ramp(1000);
        let columns = Columns::of(&bars, 13, 1000, PAD, 299.0);
        assert_eq!(columns.len(), 299);
        assert_eq!(highest(&columns.bars), highest(&bars), "a high went missing");
        assert_eq!(lowest(&columns.bars), lowest(&bars), "a low went missing");
        let drawn: f64 = columns.bars.iter().map(|bar| bar.volume).sum();
        let real: f64 = bars.iter().map(|bar| bar.volume).sum();
        assert!((drawn - real).abs() < 1e-6, "{drawn} of {real}");
        assert_eq!(columns.bars[0].open, bars[0].open, "the screen still opens where it did");
        assert_eq!(
            columns.bars.last().expect("a column").close,
            bars.last().expect("a bar").close,
            "and still closes where it did"
        );
    }

    /// An indicator is one value per bar and the candles are one per column,
    /// so every column has to name a bar of its own — inside itself, in order,
    /// and ending on the last bar on screen. Anything else draws a moving
    /// average that has slid off the candles it describes.
    #[test]
    fn every_column_reads_its_series_from_a_bar_inside_it() {
        let (first, visible, wide) = (25usize, 1000usize, 300.0);
        let bars = ramp(first + visible);
        let columns = Columns::of(&bars[first..], first, visible, PAD, wide);
        assert_eq!(columns.len(), 300);

        let mut previous: Option<usize> = None;
        for at in 0..columns.len() {
            let from = first + at * visible / columns.len();
            let to = first + (at + 1) * visible / columns.len();
            let source = columns.source(at);
            assert!(
                (from..to).contains(&source),
                "column {at} reads bar {source}, which is not in {from}..{to}"
            );
            if let Some(previous) = previous {
                assert!(source > previous, "column {at} went backwards");
            }
            previous = Some(source);
        }
        assert_eq!(
            columns.source(columns.len() - 1),
            first + visible - 1,
            "the last column carries the last bar's value"
        );
    }

    /// The other half of the mapping: the things drawn across a span of bars
    /// rather than at one — a volume profile's width, say — have to land on
    /// the same columns the candles did.
    #[test]
    fn a_span_of_bars_covers_the_columns_those_bars_are_drawn_in() {
        let (first, visible, wide) = (10usize, 600usize, 300.0);
        let bars = ramp(first + visible);
        let columns = Columns::of(&bars[first..], first, visible, PAD, wide);
        assert!(
            (columns.width_of(first, first + visible) - wide).abs() < 1e-9,
            "the visible bars are the whole plot"
        );
        for at in 0..columns.len() {
            let from = first + at * visible / columns.len();
            let left = PAD + at as f64 * columns.bar_w;
            assert!(
                (columns.left_of(from) - left).abs() < 1e-9,
                "bar {from} starts at {} rather than {left}",
                columns.left_of(from)
            );
        }
    }

    /// A chart with a volume strip under it, which is the arrangement that
    /// has corner controls as well as a crosshair.
    fn charted(bars: usize) -> State {
        let theme = omacharts_engine::theme::builtin_themes()
            .into_iter()
            .next()
            .expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let series = ramp(bars);
        let indicator = Indicator::new(1, indicators::Kind::Volume);
        let mut state = State::blank(theme, scheme);
        state.indicators = vec![Drawn {
            color: "#5588ff".to_string(),
            output: indicators::compute(&indicator, &series, 0, state.timeframe, None),
            indicator,
        }];
        state.bars = series;
        state.visible = bars;
        state
    }

    /// Draw something onto a surface of this size and hand back its pixels.
    fn frame(width: f64, height: f64, paint: impl FnOnce(&cairo::Context)) -> Vec<u8> {
        let mut surface =
            cairo::ImageSurface::create(cairo::Format::ARgb32, width as i32, height as i32)
                .expect("a surface to draw on");
        {
            let cr = cairo::Context::new(&surface).expect("a context");
            paint(&cr);
        }
        surface.data().expect("the pixels").to_vec()
    }

    fn painted(pixels: &[u8]) -> usize {
        pixels.chunks(4).filter(|pixel| pixel[3] != 0).count()
    }

    /// The whole point of the split, and the one thing a screenshot cannot
    /// check — pictures leave the pointer layer out, so a layer that drew
    /// nothing would look exactly like one that worked.
    ///
    /// If anything left in the body still reads `pointer`, moving the mouse
    /// goes on invalidating every candle on screen and the change has bought
    /// nothing. So: the body is the same picture wherever the pointer is, and
    /// the layer over it is not.
    #[test]
    fn the_chart_is_the_same_picture_wherever_the_pointer_is() {
        let (w, h) = (600.0, 400.0);
        let mut state = charted(300);

        let away = frame(w, h, |cr| draw(cr, w, h, &state));
        // In the price plot, and then in the volume strip, which is where the
        // corner controls appear.
        for at in [(300.0, 150.0), (300.0, 360.0)] {
            state.pointer = Some(at);
            let over = frame(w, h, |cr| draw(cr, w, h, &state));
            assert!(over == away, "the body redrew for a pointer at {at:?}");
        }

        state.pointer = None;
        let quiet = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
        assert_eq!(painted(&quiet), 0, "nothing is drawn over a chart nobody is pointing at");

        state.pointer = Some((300.0, 150.0));
        let crosshair = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
        // Two lines across a 600x400 chart, and the chips at the ends of them.
        assert!(painted(&crosshair) > 500, "the crosshair drew {} pixels", painted(&crosshair));
    }

    /// The two layers have to be exactly the same size and in exactly the same
    /// place, because the crosshair is drawn in the coordinates the pointer
    /// arrives in and those are the chart's. An overlay child that came out a
    /// few pixels adrift would put the crosshair where the mouse is not, and a
    /// zero-sized one would draw nothing at all — which looks exactly like a
    /// screenshot leaving it out.
    #[test]
    fn the_pointer_layer_covers_the_chart_exactly() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let theme = omacharts_engine::theme::builtin_themes()
            .into_iter()
            .next()
            .expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let view = ChartView::new(theme, scheme);

        view.root.allocate(800, 600, -1, None);

        assert_eq!((view.area.width(), view.area.height()), (800, 600), "the chart");
        assert_eq!((view.pointer.width(), view.pointer.height()), (800, 600), "the layer over it");
        let at = view
            .pointer
            .compute_point(&view.area, &gtk::graphene::Point::new(0.0, 0.0))
            .expect("the two layers are in the same window");
        assert_eq!((at.x(), at.y()), (0.0, 0.0), "the layer is offset from the chart");
    }

    /// The chart wears one tooltip — the trouble dot's — and GTK opens it by
    /// asking which widget is under the pointer. There is a second drawing
    /// area over the chart now, and everything the chart is driven by, the
    /// tooltip included, depends on that layer never being the answer to that
    /// question.
    #[test]
    fn the_pointer_layer_lets_every_event_through_to_the_chart() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let theme = omacharts_engine::theme::builtin_themes()
            .into_iter()
            .next()
            .expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let view = ChartView::new(theme, scheme);
        view.root.allocate(800, 600, -1, None);

        // What GTK's own pick consults, and the whole of why a layer over the
        // chart changes nothing about what the chart receives. A widget that
        // could be targeted would be picked instead of the chart under it,
        // and the tooltip would be asked of a widget that has none.
        assert!(!view.pointer.can_target(), "the layer over the chart would take events");

        // And the dot is where the tooltip goes looking for it, so the handler
        // the chart carries answers when the pointer gets there.
        view.set_series(
            Instrument {
                symbol: "ES".into(),
                name: "S&P 500 futures".into(),
                kind: omacharts_engine::symbols::InstrumentKind::FutureRoot,
                suffix: None,
                currency: None,
                tier: 0,
                session_origin: 0,
                overrides: Vec::new(),
                exchange: None,
                popularity: 0,
                local_name: None,
            },
            Timeframe::days(1),
            ramp(200),
        );
        view.set_trouble(Some(FetchFailure::Unreachable));
        let (dot_x, dot_y) = trouble_dot_centre(800.0);
        assert!(
            over_trouble_dot(&view.state.borrow(), 800.0, dot_x, dot_y),
            "the dot is not where the tooltip looks for it"
        );
    }

    #[test]
    fn steps_are_round_numbers() {
        assert_eq!(nice_step(100.0, 5), 20.0);
        assert_eq!(nice_step(10.0, 5), 2.0);
        assert_eq!(nice_step(1.0, 4), 0.25);
        assert_eq!(nice_step(0.0, 5), 0.0);
        assert_eq!(nice_step(100.0, 0), 0.0);
    }

    #[test]
    fn steps_scale_across_magnitudes() {
        // A chart of an index in the thousands and one of a penny stock both
        // need readable gridlines.
        for range in [0.01, 0.5, 7.0, 320.0, 48000.0] {
            let step = nice_step(range, 6);
            assert!(step > 0.0, "{range}");
            let lines = range / step;
            assert!((2.0..=15.0).contains(&lines), "{range} -> {lines} lines");
        }
    }

    #[test]
    fn axis_labels_match_the_span_on_screen() {
        const DAY: i64 = 86_400;
        // A fixed instant so the assertions do not drift with the clock.
        let ts = 1_700_000_000;

        // Years apart: years only. This is the case that read "01 Jun" for
        // every tick before the spacing was taken into account.
        let decade = format_axis_time(ts, 400 * DAY, false);
        assert_eq!(decade.len(), 4, "{decade}");
        assert!(decade.chars().all(|c| c.is_ascii_digit()), "{decade}");

        // A year or two: month and year.
        assert!(format_axis_time(ts, 30 * DAY, false).contains("20"), "months get a year");
        // A few days apart: day and month, no year.
        assert!(!format_axis_time(ts, 5 * DAY, false).contains("20"));
        // Intraday always carries a clock, however far apart the ticks.
        assert!(format_axis_time(ts, 3600, true).contains(':'));
        assert!(format_axis_time(ts, 30 * DAY, true).contains(':'));
    }

    #[test]
    fn intraday_ticks_a_few_days_apart_do_not_repeat_a_date() {
        // A week of 15-minute bars: several ticks land inside each day, so a
        // date-only label printed the same thing over and over.
        let tick = 6 * 3600;
        let labels: Vec<String> = (0..6)
            .map(|i| format_axis_time(1_700_000_000 + i * tick, tick, true))
            .collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "repeated labels: {labels:?}");
    }

    #[test]
    fn months_of_daily_bars_do_not_repeat_the_same_month() {
        // Nine months of daily bars labelled every fortnight: the case that
        // printed "Mar 2026" three times in a row.
        const DAY: i64 = 86_400;
        let tick = 14 * DAY;
        let labels: Vec<String> = (0..18)
            .map(|i| format_axis_time(1_767_225_600 + i * tick, tick, false))
            .collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "repeated labels: {labels:?}");
    }

    #[test]
    fn two_ticks_a_decade_apart_get_different_labels() {
        const DAY: i64 = 86_400;
        let tick = 400 * DAY;
        let a = format_axis_time(1_200_000_000, tick, false);
        let b = format_axis_time(1_700_000_000, tick, false);
        assert_ne!(a, b);
    }

    #[test]
    fn decimals_follow_the_step() {
        assert_eq!(decimals_for(100.0), 0);
        assert_eq!(decimals_for(1.0), 0);
        assert_eq!(decimals_for(0.5), 1);
        assert_eq!(decimals_for(0.01), 2);
        assert_eq!(decimals_for(0.0001), 4);
    }
}


#[cfg(test)]
mod drawing_tests {
    use super::*;

    fn bars(count: usize) -> Vec<Bar> {
        (0..count)
            .map(|i| {
                let base = 100.0 + (i % 11) as f64;
                Bar {
                    ts: 1_700_000_000 + i as i64 * 3600,
                    open: base,
                    high: base + 2.0,
                    low: base - 2.0,
                    close: base + 0.5,
                    volume: 1.0,
                }
            })
            .collect()
    }

    fn charted(count: usize) -> State {
        let theme = omacharts_engine::theme::builtin_themes()
            .into_iter()
            .next()
            .expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.bars = bars(count);
        state
    }

    /// A moment and its index agree in both directions: on a bar, between
    /// two, and off either end at the end's step.
    #[test]
    fn moments_and_indices_round_trip() {
        let series = bars(10);
        for (i, bar) in series.iter().enumerate() {
            assert_eq!(ts_at(&series, i as f64), bar.ts);
            assert_eq!(index_of_ts(&series, bar.ts), i as f64);
        }
        assert_eq!(ts_at(&series, 2.5), series[2].ts + 1800);
        assert_eq!(index_of_ts(&series, series[2].ts + 1800), 2.5);
        // Past the last bar, counting on at the last step.
        assert_eq!(ts_at(&series, 12.0), series[9].ts + 3 * 3600);
        assert_eq!(index_of_ts(&series, series[9].ts + 3 * 3600), 12.0);
        // And before the first.
        assert_eq!(ts_at(&series, -2.0), series[0].ts - 2 * 3600);
        assert_eq!(index_of_ts(&series, series[0].ts - 2 * 3600), -2.0);
    }

    /// Where the hand is becomes an anchor, and the anchor is drawn back
    /// under the hand: the two conversions share one scale.
    #[test]
    fn a_pixel_located_is_projected_back_under_the_pointer() {
        let state = charted(400);
        let (w, h) = (800.0, 400.0);
        let (first, visible) = state.slice();
        let plan = layout(&state, w, h);
        let (low, high) = price_range(&state, &state.bars[first..first + visible]).unwrap();
        let bar_w = plan.plot_w / visible as f64;
        for (x, y) in [(120.0, 90.0), (400.0, 200.0), (plan.plot_x + plan.plot_w - 3.0, 150.0)] {
            let anchor = state.locate(w, h, x, y).expect("a chart with bars locates");
            let drawing = Drawing::new(DrawingKind::Line, anchor, anchor);
            let projected = state.project(&plan, low, high, &drawing);
            // The moment snapped to a bar, so x is off by at most half a bar.
            assert!(
                (projected.from.0 - x).abs() <= bar_w / 2.0 + 0.01,
                "x {x} came back as {}",
                projected.from.0
            );
            assert!((projected.from.1 - y).abs() < 0.01, "y {y} came back as {}", projected.from.1);
        }
    }

    /// An anchor dropped past the last candle is a whole number of bars
    /// after it, which is what keeps it on the same bar when that bar
    /// arrives.
    #[test]
    fn an_anchor_past_the_last_bar_lands_on_a_future_bar() {
        let state = charted(100);
        let (w, h) = (800.0, 400.0);
        let plan = layout(&state, w, h);
        // A hundred bars fill the plot, so the twenty-first slot past the
        // last one is twenty-one steps after it.
        let (_, visible) = state.slice();
        assert_eq!(visible, 100);
        let bar_w = plan.plot_w / visible as f64;
        let x = plan.plot_x + 120.5 * bar_w;
        let anchor = state.locate(w, h, x, 100.0).unwrap();
        assert_eq!(anchor.ts, state.bars[99].ts + 21 * 3600);
    }

    /// Alt+R, Alt+3, click: the rectangle laid down follows configuration 3
    /// and draws into the chart's drawing group.
    #[test]
    fn a_tool_in_hand_draws_in_the_configuration_chosen_for_it() {
        let mut state = charted(400);
        state.sharing = Sharing::Group(2);
        state.tool = Some(DrawingKind::Rect);
        state.next_config = 3;
        assert!(state.begin_placing(800.0, 400.0, 200.0, 150.0));
        let placing = state.placing.clone().expect("a drawing in hand");
        assert_eq!(placing.kind, DrawingKind::Rect);
        assert_eq!(placing.config, Some(3));
        assert_eq!(placing.scope, drawings::Scope::Group(2));
        assert_eq!(state.drag, Some(Drag::Place { moved: false }));
        // Nothing in hand, nothing begins.
        state.tool = None;
        state.placing = None;
        assert!(!state.begin_placing(800.0, 400.0, 200.0, 150.0));
    }

    /// The newest drawing is picked first, and a press on an unselected
    /// drawing's end selects it rather than taking hold of the end.
    #[test]
    fn the_newest_drawing_under_the_hand_is_the_one_picked() {
        let mut state = charted(400);
        let (w, h) = (800.0, 400.0);
        let a = state.locate(w, h, 200.0, 150.0).unwrap();
        let b = state.locate(w, h, 500.0, 250.0).unwrap();
        state.drawings.push(Drawing::new(DrawingKind::Line, a, b));
        state.drawings.push(Drawing::new(DrawingKind::Rect, a, b));
        assert_eq!(state.drawing_at(w, h, 350.0, 200.0), Some((1, Grip::Body)));
        assert_eq!(state.drawing_at(w, h, 200.0, 150.0), Some((1, Grip::Body)));
        state.selected = vec![1];
        assert_eq!(state.drawing_at(w, h, 200.0, 150.0), Some((1, Grip::From)));
        assert_eq!(state.drawing_at(w, h, 50.0, 30.0), None);
    }

    /// A selection of one kind can be asked for its properties; a mixed
    /// one cannot, and the newest pick is the one a single question is
    /// about.
    #[test]
    fn a_selection_has_a_kind_only_when_it_is_all_one_kind() {
        let mut state = charted(400);
        let (w, h) = (800.0, 400.0);
        let a = state.locate(w, h, 200.0, 150.0).unwrap();
        let b = state.locate(w, h, 500.0, 250.0).unwrap();
        state.drawings.push(Drawing::new(DrawingKind::Line, a, b));
        state.drawings.push(Drawing::new(DrawingKind::Line, a, b));
        state.drawings.push(Drawing::new(DrawingKind::Rect, a, b));
        assert_eq!(state.selection_kind(), None);
        state.selected = vec![0, 1];
        assert_eq!(state.selection_kind(), Some(DrawingKind::Line));
        assert_eq!(state.primary(), Some(1));
        state.selected.push(2);
        assert_eq!(state.selection_kind(), None);
        assert_eq!(state.primary(), Some(2));
    }

    /// The fill a box paints is the engine's alpha and not a solid: the
    /// candles under it are meant to read through.
    #[test]
    fn a_box_is_painted_translucent() {
        let mut state = charted(400);
        let (w, h) = (800.0, 400.0);
        let a = state.locate(w, h, 200.0, 150.0).unwrap();
        let b = state.locate(w, h, 500.0, 250.0).unwrap();
        state.drawings.push(Drawing::new(DrawingKind::Rect, a, b));
        let mut surface =
            cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            draw(&cr, w, h, &state);
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let at = |x: usize, y: usize| {
            let i = y * stride + x * 4;
            (data[i + 2], data[i + 1], data[i])
        };
        let inside = at(350, 200);
        let outside = at(50, 30);
        let colour = colors::rgba(&drawings::colour(&state.theme, drawings::Preset::Blue));
        let solid = (
            (colour.0 * 255.0).round() as u8,
            (colour.1 * 255.0).round() as u8,
            (colour.2 * 255.0).round() as u8,
        );
        assert_ne!(inside, outside, "the box painted nothing");
        assert_ne!(inside, solid, "the box is opaque");
    }
}
