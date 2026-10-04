//! The candlestick chart.
//!
//! One `DrawingArea` and a cairo draw function. The view is two numbers — the
//! index of the leftmost visible bar and how many are visible — so panning and
//! zooming are arithmetic, not a re-layout, and a repaint touches only what is
//! on screen.
//!
//! Candles are drawn in two passes, up and down, each accumulating one cairo
//! path for the wicks and one for the bodies. Two strokes and two fills for a
//! screen of bars rather than four calls per candle, which is what keeps a
//! drag at the frame rate.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::cairo;
use gtk::prelude::*;
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

/// Fewest bars we will zoom into, and the most we will draw at once.
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
}

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

pub struct ChartView {
    pub area: gtk::DrawingArea,
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
}

impl ChartView {
    pub fn new(theme: Theme, scheme: BarScheme) -> Rc<ChartView> {
        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_focusable(true);

        let state = Rc::new(RefCell::new(State {
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
        }));
        let on_hover: Handler<dyn Fn(Option<Hover>)> = Rc::new(RefCell::new(None));

        let view = Rc::new(ChartView {
            area,
            state,
            on_hover,
            on_context_menu: Rc::new(RefCell::new(None)),
            on_pane_resize: Rc::new(RefCell::new(None)),
            on_pane_close: Rc::new(RefCell::new(None)),
            on_pane_move: Rc::new(RefCell::new(None)),
            on_axis_menu: Rc::new(RefCell::new(None)),
        });
        view.wire_drawing();
        view.wire_pointer();
        view.wire_zoom();
        view.wire_drag();
        view.wire_axis_menu();
        view
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
            self.area.queue_draw();
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
        self.area.queue_draw();
    }

    /// Show or hide the gridlines. The axes and their labels stay: without
    /// them a chart is a shape with no scale.
    pub fn set_show_grid(&self, show: bool) {
        self.state.borrow_mut().show_grid = show;
        self.area.queue_draw();
    }

    pub fn set_bar_style(&self, style: BarStyle) {
        self.state.borrow_mut().bar_style = style;
        self.area.queue_draw();
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
        self.area.queue_draw();
    }

    pub fn set_context_menu_handler(&self, handler: impl Fn(f64, f64) + 'static) {
        *self.on_context_menu.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_indicators(&self, indicators: Vec<Drawn>) {
        self.state.borrow_mut().indicators = indicators;
        self.area.queue_draw();
    }

    /// Say why the last fetch brought nothing back, or `None` once one works.
    ///
    /// Taken rather than discarded because "No data for this symbol" is a
    /// claim about the symbol, and a fetch that never arrived is not evidence
    /// for it. A first-time user on a machine with no network read that line
    /// as the app not having the S&P 500.
    pub fn set_trouble(&self, trouble: Option<FetchFailure>) {
        self.state.borrow_mut().trouble = trouble;
        self.area.queue_draw();
    }

    /// Whether a fetch is outstanding for what is on screen.
    ///
    /// An empty chart that is still loading and an empty chart that came back
    /// empty are different things, and telling the user they are the same is
    /// how "no data" ends up meaning nothing.
    pub fn set_loading(&self, loading: bool) {
        self.state.borrow_mut().loading = loading;
        self.area.queue_draw();
    }

    pub fn restyle(&self, theme: Theme, scheme: BarScheme) {
        {
            let mut state = self.state.borrow_mut();
            state.theme = theme;
            state.scheme = scheme;
        }
        self.area.queue_draw();
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
        self.area.queue_draw();
    }

    pub fn zoom(&self, factor: f64) {
        self.state.borrow_mut().zoom_time(factor, 0.5);
        self.area.queue_draw();
    }

    pub fn pan_bars(&self, delta: i64) {
        self.state.borrow_mut().pan_by(delta as f64);
        self.area.queue_draw();
    }

    fn wire_drawing(&self) {
        let state = self.state.clone();
        self.area.set_draw_func(move |_, cr, width, height| {
            draw(cr, width as f64, height as f64, &state.borrow());
        });
    }

    fn wire_pointer(&self) {
        let motion = gtk::EventControllerMotion::new();
        let state = self.state.clone();
        let area = self.area.clone();
        let on_hover = self.on_hover.clone();
        motion.connect_motion(move |_, x, y| {
            state.borrow_mut().pointer = Some((x, y));
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
            area.set_cursor_from_name(Some(match (over_edge, over_control) {
                (true, _) => "ns-resize",
                (_, true) => "pointer",
                _ => "default",
            }));
            notify_hover(&state, &on_hover, &area);
            area.queue_draw();
        });

        let state = self.state.clone();
        let area = self.area.clone();
        let on_hover = self.on_hover.clone();
        motion.connect_leave(move |_| {
            state.borrow_mut().pointer = None;
            if let Some(handler) = on_hover.borrow().as_ref() {
                handler(None);
            }
            area.queue_draw();
        });
        self.area.add_controller(motion);
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
        scroll.connect_scroll(move |controller, dx, dy| {
            if dx == 0.0 && dy == 0.0 {
                return glib::Propagation::Proceed;
            }
            let modifiers = controller.current_event_state();
            let shift = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);

            let (width, height) = (area.width() as f64, area.height() as f64);
            let mut s = state.borrow_mut();
            let region = s
                .pointer
                .map(|(x, y)| region_at(x, y, width, height))
                .unwrap_or(Region::Plot);

            // A trackpad's horizontal axis always pans, whatever is held.
            if dx != 0.0 && dy == 0.0 {
                s.pan_by(dx * 2.0);
                drop(s);
                area.queue_draw();
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
            area.queue_draw();
            glib::Propagation::Stop
        });
        self.area.add_controller(scroll);
    }

    fn wire_drag(&self) {
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
        drag.connect_drag_begin(move |_, x, y| {
            let mut s = state.borrow_mut();
            let (first, visible) = s.slice();
            // The edge wins over whatever region it crosses, because that is
            // what the cursor was already promising.
            let plan = layout(&s, area.width() as f64, area.height() as f64);
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
            s.drag = Some(
                match region_at(x, y, area.width() as f64, area.height() as f64) {
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
        });

        let state = self.state.clone();
        let area = self.area.clone();
        drag.connect_drag_update(move |_, offset_x, offset_y| {
            let mut s = state.borrow_mut();
            let Some(drag) = s.drag else { return };
            let plot_w = (area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0);

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
            }
            drop(s);
            area.queue_draw();
        });

        let state = self.state.clone();
        let on_resize = self.on_pane_resize.clone();
        drag.connect_drag_end(move |_, _, _| {
            let finished = state.borrow_mut().drag.take();
            // Stored when the drag ends rather than on every motion event,
            // which would be a database write per pixel of travel.
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
        click.connect_pressed(move |_, presses, x, y| {
            if presses < 2 {
                return;
            }
            let region = region_at(x, y, area.width() as f64, area.height() as f64);
            let mut s = state.borrow_mut();
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
                // everywhere else. Resetting is Alt+R or the axis menu.
                Region::Plot => return,
            }
            drop(s);
            area.queue_draw();
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
        self.area.queue_draw();
    }

    /// Everything back to how the chart opens. Alt+R, and the axis menu.
    pub fn reset_view(&self) {
        self.state.borrow_mut().reset_view();
        self.area.queue_draw();
    }

    /// Offer the reset where people right-click for it.
    fn wire_axis_menu(self: &Rc<Self>) {
        let click = gtk::GestureClick::new();
        click.set_button(gtk::gdk::BUTTON_SECONDARY);
        let area = self.area.clone();
        let on_context_menu = self.on_context_menu.clone();
        let on_axis_menu = self.on_axis_menu.clone();
        click.connect_pressed(move |_, _, x, y| {
            let (width, height) = (area.width() as f64, area.height() as f64);
            if region_at(x, y, width, height) != Region::PriceAxis {
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

    let mut max_volume: f64 = 0.0;
    for b in bars {
        max_volume = max_volume.max(b.volume);
    }
    let Some((low, high)) = price_range(state, bars) else {
        draw_placeholder(cr, width, height, state);
        return;
    };

    let to_y = |price: f64| price_y + price_h * (high - price) / (high - low);
    let bar_w = plot_w / visible as f64;

    // One answer for how precisely prices are written, used by the gridlines,
    // the last-price chip and the crosshair alike — a chart saying 1.1257 on
    // one label and 1.13 on another is describing two different prices.
    let step = nice_step(high - low, (price_h / 52.0).max(2.0) as usize);
    let kind = state.instrument.as_ref().map(|i| i.kind);
    let decimals = omacharts_engine::price_decimals(step, (low + high) / 2.0, kind);

    draw_price_grid(cr, state, plot_x, plot_w, price_y, price_h, low, high, &to_y);
    draw_time_axis(cr, state, bars, plot_x, plot_w, height, bar_w, first);
    // Shaded things go under the candles; lines go over. A band drawn on top
    // of the bars hides the thing it is describing.
    draw_indicator_fills(cr, state, first, visible, plot_x, bar_w, &to_y);
    draw_candles(cr, state, bars, plot_x, bar_w, &to_y);
    draw_indicator_lines(cr, state, first, visible, plot_x, bar_w, &to_y);

    for (at, row) in plan.rows.iter().enumerate() {
        draw_row_edge(cr, state, &plan, at);
        let Some(id) = row.pane else { continue };
        let Some(drawn) = state.indicators.iter().find(|d| d.indicator.id == id) else {
            continue;
        };
        // The way around and out of a strip, offered only while the pointer is
        // in it: four panes each wearing permanent buttons is a dozen things
        // competing with the chart.
        if state.pointer.map(|(_, y)| row.covers(y)).unwrap_or(false) {
            draw_pane_controls(cr, state, &plan, row, at);
        }
        match &drawn.output {
            Output::Volume { .. } => {
                if max_volume > 0.0 {
                    draw_volume(
                        cr, state, bars, plot_x, bar_w, row.top, row.height, max_volume,
                    );
                }
                // Named like the others now that it is one strip among several.
                // Three unlabelled boxes under a chart is a puzzle.
                draw_pane_name(cr, state, drawn, plot_x, row.top);
            }
            Output::Pane(pane) => draw_pane(
                cr,
                state,
                pane,
                drawn,
                plot_x,
                plot_w,
                bar_w,
                row.top,
                row.height,
                width,
                first,
                visible,
            ),
            _ => {}
        }
    }

    draw_last_price(cr, state, plot_x, plot_w, price_y, width, decimals, &to_y);

    if let Some((px, py)) = state.pointer {
        draw_crosshair(
            cr, state, px, py, plot_x, plot_w, plan.top, price_y, price_h, width, height, bar_w,
            first, low, high, &to_y,
        );
    }

    // Another chart's pointer, if nothing is pointing at this one. The real
    // crosshair wins: an echo under your own pointer is a second line saying
    // the same thing.
    if state.pointer.is_none()
        && let Some(echo) = state.echo
    {
        draw_echo(
            cr, state, bars, plot_x, plot_w, bar_w, plan.top, price_y, price_h, height, echo,
            &to_y,
        );
    }

    if let Some(trouble) = state.trouble {
        draw_trouble_marker(cr, state, width, trouble.tag());
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
    plot_x: f64,
    plot_w: f64,
    bar_w: f64,
    top: f64,
    height: f64,
    width: f64,
    first: usize,
    visible: usize,
) {
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
    let _ = width;

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
    for (i, value) in values.iter().enumerate() {
        let Some(value) = value else {
            pen_down = false;
            continue;
        };
        let x = plot_x + (i as f64 + 0.5) * bar_w;
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
    to_y: &impl Fn(f64) -> f64,
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
    let _ = to_y;

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

/// The marker in the top corner of a chart that has bars but could not get
/// newer ones. What is drawn is real and out of date, and this says why.
fn draw_trouble_marker(cr: &cairo::Context, state: &State, width: f64, text: &str) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(10.0);
    let Ok(extents) = cr.text_extents(text) else { return };
    let w = extents.width() + 12.0;
    let x = width - PRICE_AXIS_W - w - PAD;
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.22);
    cr.rectangle(x, PAD, w, 16.0);
    let _ = cr.fill();
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.95);
    cr.move_to(x + 6.0, PAD + 11.5);
    let _ = cr.show_text(text);
}

/// Band shading and volume profiles, beneath the bars.
fn draw_indicator_fills(
    cr: &cairo::Context,
    state: &State,
    first: usize,
    visible: usize,
    plot_x: f64,
    bar_w: f64,
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
                    fill_between(
                        cr, &band.upper, &band.lower, first, visible, plot_x, bar_w, to_y,
                    );
                }
            }
            Output::Profiles(profiles) => {
                draw_profiles(cr, state, drawn, profiles, first, visible, plot_x, bar_w, to_y);
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
    first: usize,
    visible: usize,
    plot_x: f64,
    bar_w: f64,
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
                stroke_series(cr, values, first, visible, plot_x, bar_w, to_y);
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
                    stroke_series(cr, &band.upper, first, visible, plot_x, bar_w, to_y);
                    stroke_series(cr, &band.lower, first, visible, plot_x, bar_w, to_y);
                }
                cr.restore().ok();

                let stroke = drawn.indicator.stroke;
                if !stroke.is_hidden() {
                    cr.save().ok();
                    cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
                    colors::set_source(cr, &drawn.color);
                    cr.set_line_width(stroke.width);
                    stroke_series(cr, &bands.vwap, first, visible, plot_x, bar_w, to_y);
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
#[allow(clippy::too_many_arguments)]
fn draw_profiles(
    cr: &cairo::Context,
    state: &State,
    drawn: &Drawn,
    profiles: &[Profile],
    first: usize,
    visible: usize,
    plot_x: f64,
    bar_w: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let poc = match &drawn.indicator.params {
        omacharts_engine::Params::VolumeProfile { poc_color: Some(choice), .. } => {
            choice.resolve(&state.theme)
        }
        _ => state.theme.companion(&drawn.color),
    };

    let last = first + visible;
    for profile in profiles {
        if profile.last_bar < first || profile.first_bar >= last {
            continue;
        }
        let left = plot_x + (profile.first_bar.max(first) - first) as f64 * bar_w;
        // A profile may use at most this much of its own period's width, so it
        // describes the bars rather than burying them.
        let span = ((profile.last_bar.min(last - 1) + 1 - profile.first_bar.max(first)) as f64
            * bar_w)
            .max(bar_w)
            * 0.4;

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
#[allow(clippy::too_many_arguments)]
fn stroke_series(
    cr: &cairo::Context,
    values: &[Option<f64>],
    first: usize,
    visible: usize,
    plot_x: f64,
    bar_w: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let mut drawing = false;
    for i in 0..visible {
        let x = plot_x + (i as f64 + 0.5) * bar_w;
        match values.get(first + i).copied().flatten() {
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
#[allow(clippy::too_many_arguments)]
fn fill_between(
    cr: &cairo::Context,
    upper: &[Option<f64>],
    lower: &[Option<f64>],
    first: usize,
    visible: usize,
    plot_x: f64,
    bar_w: f64,
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

    for i in 0..visible {
        let x = plot_x + (i as f64 + 0.5) * bar_w;
        match (
            upper.get(first + i).copied().flatten(),
            lower.get(first + i).copied().flatten(),
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

