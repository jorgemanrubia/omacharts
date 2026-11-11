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
mod compare;
mod selection;
pub use compare::{CompareLine, Comparing, Reading, Readout};
pub use selection::ProfileSpot;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use gtk::cairo;
use gtk::prelude::*;
use crate::text_editor;
use omacharts_engine::drawings::{
    self, Anchor, Configurations, Drawing, Grip, Kind as DrawingKind, Projected, Scope,
};
use omacharts_engine::bars::Unit;
use omacharts_engine::clock::{ChartZone, TimeFormat};
use omacharts_engine::indicators::{self as indicators, BandSeries, LineStyle, Output, Profile};
use omacharts_engine::indicators::profile::DELTA_COLOURINGS;
use omacharts_engine::{
    grouped_decimals, Bar, BarColouring, BarScheme, BarStyle, Direction, FetchFailure, Indicator, Instrument,
    Range, Tag, Theme, Timeframe,
};

use crate::colors;
use crate::marks::{
    AnyPrice, KeptRules, LabelSide, MarkDrag, MarkEvent, MarkLink, MarkRules, MarkTab, PriceMark, MARK_NEAR,
};
use gtk::glib;

/// How wide the price axis is, and how tall the time axis: for whoever
/// lays something beside or under them.
pub const PRICE_AXIS_W: f64 = 64.0;
pub const TIME_AXIS_H: f64 = 24.0;
const PAD: f64 = 10.0;
/// How far past either end of the series the chart can be panned: empty
/// columns before the first bar or after the last, so there is room to move
/// things about at the edges. Fixed for now; a setting one day, if anyone
/// wants a different amount.
const OVERHANG_PX: f64 = 500.0;
/// Empty columns past the last bar when the chart opens or is reset, so the
/// live bar is not pressed against the price axis. About what other
/// charting software leaves.
const RIGHT_MARGIN: usize = 10;
/// Kept short of the whole view, so dividing by what is left is safe.
const MARGIN_MAX_SHARE_OF_VIEW: f64 = 0.9;
/// Room above the highest price on screen when the price is the top row, so
/// it clears the header and the legend laid over the plot.
const PRICE_TOP_AIR: f64 = 70.0;
/// The most of the price plot that room may take: a short chart, in a split
/// or a small window, keeps its candles rather than a fixed band of nothing.
const PRICE_TOP_AIR_SHARE: f64 = 0.15;
/// Room below the lowest price on screen, and above the highest when
/// something else is the top row, as a share of the price span.
const PRICE_AIR: f64 = 0.06;
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

/// How many pixels wide an OHLC bar's line and ticks may be drawn, and are
/// unless asked otherwise.
pub const MIN_OHLC_THICKNESS: u8 = 1;
pub const MAX_OHLC_THICKNESS: u8 = 4;
pub const DEFAULT_OHLC_THICKNESS: u8 = 2;

/// A thickness the chart draws: from one pixel to four.
pub fn clamp_thickness(thickness: u8) -> u8 {
    thickness.clamp(MIN_OHLC_THICKNESS, MAX_OHLC_THICKNESS)
}

/// Fewest bars we will zoom into, and the most we will put on screen at once.
///
/// The ceiling is about how much history a chart will read, not about how much
/// work a frame is: past a bar a pixel the bars are aggregated into columns, so
/// what a frame draws is bounded by the width of the plot however far out the
/// view is zoomed.
const MIN_VISIBLE: usize = 12;
const MAX_VISIBLE: usize = 3000;
/// The fewest and the most bars a chart spreads across its width, for
/// whoever has to choose a resolution a stretch of time fits.
pub const VISIBLE_BARS: (usize, usize) = (MIN_VISIBLE, MAX_VISIBLE);

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

/// What a profile drawing's profile depends on, for keeping it.
#[derive(Clone, Copy, PartialEq, Debug)]
struct ProfileKey {
    span: (usize, usize),
    first: i64,
    last: (i64, u64, u64, u64),
    rows: Option<usize>,
    value_area: u64,
}

/// What an anchored VWAP drawing's line depends on, for keeping it: the bar
/// it starts from, the bars after it, and the price it weights.
#[derive(Clone, Copy, PartialEq, Debug)]
struct VwapKey {
    start: (usize, i64),
    len: usize,
    last: (i64, u64, u64, u64, u64),
    source: drawings::VwapSource,
}

struct State {
    bars: Vec<Bar>,
    /// Where another chart's pointer is, drawn here as a quieter crosshair.
    echo: Option<Echo>,
    theme: Theme,
    scheme: BarScheme,
    /// The colouring chosen in Preferences, and whether it is worn the
    /// other way round: what a delta profile with no colouring of its own
    /// is painted in, rather than this chart's.
    house: (BarColouring, bool),
    /// A delta profile's bought and sold colours, for the house colouring
    /// and then each of [`DELTA_COLOURINGS`] in turn, worked out when the
    /// theme or the house colouring changes rather than on every frame.
    delta_inks: [(String, String); 1 + DELTA_COLOURINGS.len()],
    /// The last-price tag for a rise and a fall, worked out from the scheme
    /// whenever it changes rather than on every frame.
    price_tags: [Tag; 2],
    /// The last price's colour and its tag, from the theme, worked out
    /// whenever the theme changes rather than on every frame.
    last_ink: LastInk,
    /// The feed's latest trade when the bars drawn leave it out, a chart
    /// of the regular session out of hours: the price is the price,
    /// whatever candles are shown. `None` when the last bar has it.
    latest: Option<f64>,
    instrument: Option<Instrument>,
    /// The profiles the volume profile drawings show, as last worked out:
    /// a drawing is projected on every motion of the pointer, and its
    /// profile only changes with its span or the bars.
    profile_cache: RefCell<Vec<(ProfileKey, Option<Profile>)>>,
    /// The lines the anchored VWAP drawings show, kept for the same reason:
    /// asked on every motion, changed only by the anchor or the bars.
    vwap_cache: RefCell<Vec<(VwapKey, Rc<Vec<omacharts_engine::indicators::vwap::Anchored>>)>>,
    timeframe: Timeframe,
    /// Leftmost visible bar.
    first: usize,
    /// How many bars are visible.
    visible: usize,
    pointer: Option<(f64, f64)>,
    /// Ctrl is held, as of the last event that said: what a drawing is laid
    /// down or a corner dragged with snaps to the candle under it, and so
    /// does the crosshair while it does, to show where.
    magnet: bool,
    /// Pinned to the right edge, so new bars keep the view at "now" until the
    /// user pans away.
    anchored: bool,
    /// Empty columns past the last bar, at the right edge, while anchored:
    /// the room a hand dragged open. Kept as new bars arrive, so the margin
    /// stays what it was set to. `RIGHT_MARGIN` at rest.
    overhang: usize,
    /// Whether a hand set `overhang`, panning. At rest the room the live
    /// sessions of the right-aligned volume profiles take is added to it,
    /// and taken off as they go, so the empty room before them is the same
    /// as with none; set by hand, it is what the hand left.
    overhang_by_hand: bool,
    /// The share of the columns a hand left `overhang` at, short of the
    /// profiles' margin: what zooming keeps it at, so it stays as wide on
    /// screen without a column's rounding piling up zoom after zoom.
    overhang_share: f64,
    /// Empty columns before the first bar, at the left edge, when the hand
    /// has dragged past the start of the series. Zero at rest.
    lead: usize,
    /// The stretch of the calendar the view was told to show — the year to
    /// date, the last month — held until a hand moves the view. Held rather
    /// than turned into a bar count once, because the count is the series':
    /// a year is 250 bars of a stock and 365 of bitcoin, and the month to
    /// date grows a bar a day.
    range: Option<Range>,
    drag: Option<Drag>,
    /// The grip of a selected drawing the pointer rests on, lit so the hand
    /// can see what a press would take hold of. Not while anything is being
    /// dragged: then the grip in hand is the one lit, from `drag`.
    hover_grip: Option<(usize, Grip)>,
    /// The press just made ended a zig-zag on its last corner. Read by the
    /// double-click handler, so the second press of a quick double-click
    /// that already ended the drawing does not go on to open its text.
    closed_by_press: bool,
    indicators: Vec<Drawn>,
    /// Whether the indicators are drawn at all. Off hides every one of them,
    /// strips and all, without touching each one's own `visible`, so turning
    /// it back on brings back what was on screen and no more.
    show_indicators: bool,
    /// How far down the price axis something stands over its top, in
    /// pixels: the window's corner, on the chart under it. The labels there
    /// fade out under its edge, and the last price and the crosshair's
    /// price stop below it rather than slide under.
    covered_axis_top: f64,
    /// The square under the price axis holds a control of the window's,
    /// so the range the view holds to is not written there.
    axis_corner_taken: bool,
    /// How far in from the chart's left edge the window's drawing tools'
    /// toggle stands over the time axis, on the bottom-left chart while the
    /// tools are put away: a date that would be written under it is left
    /// out, and the crosshair's is pushed in past it. Zero on every other
    /// chart.
    axis_start_taken: f64,
    bar_style: BarStyle,
    /// How many pixels wide an OHLC bar's line and ticks are drawn.
    ohlc_thickness: u8,
    bars_hidden: bool,
    show_grid: bool,
    /// The clock the time axis and the crosshair read.
    clock: Clock,
    /// Why the last fetch for this chart came back with nothing, if it did.
    ///
    /// One piece of state for both of the things that used to be separate —
    /// the corner marker on a chart that has bars, and the line in the middle
    /// of one that has none. They were the same thought said twice, and only
    /// the marker could ever be reached, because a chart with no bars returns
    /// before it is drawn.
    trouble: Option<FetchFailure>,
    /// What to say about `trouble` instead of its own message, when the
    /// embedder knows more than the failure does: "MES isn't in Sierra
    /// Chart's data". Cleared with it.
    trouble_words: Option<String>,
    /// The price marks laid over the chart by whoever embeds it, and the
    /// rules a drag on them keeps to; empty, which costs nothing, unless
    /// somebody marks it.
    marks: Marks,
    /// Which end of their lines the marks' labels sit at.
    mark_labels: LabelSide,
    /// The step a mark is dragged on, set by whoever lays the marks: the
    /// contract it trades, which need not step as the series charted does
    /// — a 6E chart trading M6E. The series' own step when unset.
    mark_tick: Option<f64>,
    /// The handle at the end of a mark the pointer is over, lit.
    hover_mark: Option<MarkHit>,
    loading: bool,
    /// The first bar's time the last time older bars were asked for: see
    /// [`ChartView::set_history_handler`].
    history_asked: Cell<Option<i64>>,
    /// How far the load being waited on has got, from 0 to 1, when its feed
    /// can say: a bar under Loading…. This is how far the bar is drawn; it
    /// eases towards `progress_target` a frame at a time, see
    /// [`ChartView::set_progress`].
    progress: Option<f64>,
    /// How far the load has really got, the last the feed said: where the
    /// bar is heading. Only ever grows within one load.
    progress_target: f64,
    /// Whether a frame callback is moving `progress` towards its target.
    progress_easing: bool,
    /// Multiplier on the held price range. 1.0 shows exactly the range that
    /// was held; above that is zoomed in.
    price_zoom: f64,
    /// Vertical shift, as a fraction of the held range.
    price_offset: f64,
    /// While true the price scale follows the data and the values above and
    /// below are held at their neutral settings.
    price_auto: bool,
    /// The price range the scale was fitted to when it was taken off
    /// automatic, low and high in prices rather than on the axis, so it
    /// means the same on a log axis or a linear one.
    ///
    /// What a held scale's zoom and offset are worked on. It used to be the
    /// fit of whatever bars were on screen, which moved with them: a held
    /// scale stretched and shrank as the chart was scrolled sideways. Fixed
    /// once it is taken, so scrolling moves the time axis and nothing else.
    ///
    /// Empty while automatic, and on a held scale nobody has drawn yet — one
    /// restored before the chart had a size — which the first frame fills
    /// with what it fits. A cell for that reason: drawing reads the state.
    price_base: Cell<Option<(f64, f64)>>,
    /// The strips zoomed or dragged on their own scale, by indicator, each
    /// with the values its bottom and top show. A strip not in here fits
    /// what is on screen, which every strip does until it is zoomed; one in
    /// here keeps its range however the bars change, as a held price scale
    /// does, and is let go of by a reset.
    strip_held: BTreeMap<u32, (f64, f64)>,
    /// The kind of indicator each held strip was held for: an id is reused
    /// once its indicator is removed, and one of another kind under it, or
    /// one a template brings in under an id it had of its own, fits itself
    /// rather than take on a range that was never its.
    strip_kinds: BTreeMap<u32, indicators::Kind>,
    /// Whether the price axis is logarithmic: equal distances on it are equal
    /// ratios rather than equal differences. Asked for, not promised — see
    /// [`PriceScale`] for when it is drawn linear anyway.
    price_log: bool,
    /// What has been drawn on this symbol, in the order it was drawn. The
    /// chart draws what it is handed; the store is where they live.
    drawings: Vec<Drawing>,
    /// Whether the drawings are drawn. Off hides every one of them, the
    /// chart's own and the shared, and none answers the pointer or a key;
    /// they are still held, so turning it back on finds them as they were.
    show_drawings: bool,
    /// Which of them has the grips, by position in `drawings`.
    /// The drawings that wear grips, in the order they were taken: the last
    /// is the one most recently picked, and what a single-drawing question
    /// is about. Shift with a click adds one or takes one out.
    selected: Vec<usize>,
    /// The indicator selected, by id: one at a time, and never together with
    /// a drawing. Not kept anywhere, as a drawing's selection is not.
    picked: Option<u32>,
    /// An indicator to select once it arrives: one just put on the chart,
    /// which is computed on its way here.
    pending_pick: Option<u32>,
    /// What was selected when a marquee began, kept under it: the box adds
    /// to the selection rather than starting one, since Shift is held.
    marquee_base: Vec<usize>,
    /// Whether the tool stays in hand after a drawing is finished.
    ///
    /// Off by default: a tool is normally for the one drawing you reached
    /// for it to make, and going back to the pointer is what lets you take
    /// hold of it straight away. On, it is for laying down six of
    /// something, and Escape is the way out.
    sticky: bool,
    /// The tool that is armed: the next press on the plot starts a drawing
    /// of this kind. `None` is the usual state, where a press pans.
    tool: Option<DrawingKind>,
    /// The ruler lives only on the pointer layer; its tuple is start, end, finished.
    ruler_armed: bool,
    ruler: Option<(Anchor, Anchor, bool)>,
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
    /// Whether a drawing made here goes to the symbol or stays on this
    /// chart. The other half of the pair — whether what others send is
    /// shown — is the window's, since it decides which drawings the chart
    /// is handed in the first place.
    sends: bool,
    /// Every change to the drawings, as the list was before it, so Ctrl+Z
    /// puts it back; and what Ctrl+Z took away, so Ctrl+Y can put it back
    /// again. One list rather than one event per kind of change, because a
    /// drag, a property, a configuration and a deletion all come down to
    /// "the drawings were this and are now that".
    undo: Vec<Vec<Drawing>>,
    redo: Vec<Vec<Drawing>>,
    /// The drawing whose words are being typed, by position in `drawings`.
    ///
    /// Its label is left off the chart while this is set: the editor is
    /// sitting exactly where the label would be, in the same font and the
    /// same ink, and drawing both would be drawing it twice.
    editing: Option<usize>,
    /// Where that editor is, in pixels, so the chart can put the label's own
    /// ground under it.
    ///
    /// A committed label gets its ground as a halo round each glyph, which a
    /// text widget cannot draw. So while the caret is in it the ground goes
    /// down as a plate instead: the same colour, under the whole block, which
    /// is if anything more readable than the result and never less. The
    /// editor is told where to sit after every keystroke, and tells the
    /// chart here.
    editing_at: Option<(f64, f64, f64, f64)>,
    /// The other markets drawn over this one, rebased, while there are any.
    /// Nothing at all otherwise, so a chart that compares nothing pays for
    /// nothing.
    compare: Option<Box<compare::Overlay>>,
    /// Told what the compared markets read at the bar in question.
    on_compare_readout: Handler<dyn Fn(Readout)>,
}

/// What happened to the selected indicator, for the window, which owns the
/// indicators and the menus about them.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum IndicatorEvent {
    /// The selection moved to this indicator, or off every one.
    Picked(Option<u32>),
    /// Right-clicked, at a pixel of the chart: its menu.
    Menu(u32, f64, f64),
    /// Alt+Enter, or a double-click: its properties.
    Properties(u32),
    /// Delete.
    Remove(u32),
    /// Alt+Shift with + or −: its line a step thicker, or thinner.
    StepWidth(u32, bool),
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
#[derive(Clone, PartialEq, Debug)]
enum Drag {
    /// The chart dragged: across in time, and up or down the price's scale
    /// once it is held — or, `strip` pressed in, that strip's, whose range
    /// it was taken at is there once it is held, with the height it is
    /// drawn in, so the values follow the hand.
    Pan { left: i64, offset: f64, strip: Option<StripPan> },
    PriceScale { zoom: f64 },
    /// A strip's own axis pulled: its scale stretched or squeezed from
    /// `from`, the range it showed as the hand took hold, which `held` says
    /// whether it was holding then or fitting itself.
    StripScale { id: u32, from: (f64, f64), held: bool },
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
    /// Shift and a press on empty chart: a box dragged out from `from`,
    /// in pixels, that selects every drawing it touches as it goes.
    Marquee { from: (f64, f64), to: (f64, f64) },
    /// A mark's tab pulled out, or a mark's line taken to another price:
    /// `at` is where the hand has it, on the tick, and `allowed` whether
    /// the mark's rules let it go there. Only a preview until the release;
    /// nothing is said from here.
    Mark { what: Grab, at: f64, allowed: bool },
}

/// A strip a pan was pressed in: see [`Drag::Pan`].
#[derive(Clone, Copy, Debug, PartialEq)]
struct StripPan {
    id: u32,
    held: Option<(f64, f64)>,
    inner_h: f64,
}

/// Which scale a gesture over the chart's right-hand column, or a strip,
/// works on: the price's, or one strip's own.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Axis {
    Price,
    Strip(u32),
}

/// What a pinch scales, and what it showed as the fingers met it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pinch {
    Price { zoom: f64 },
    Strip { id: u32, from: (f64, f64) },
}

/// Every vertical scale the chart has, as a gesture is compared against
/// to tell whether it changed anything worth writing down: whether the
/// price's is automatic, the zoom and shift of a held one, and the strips
/// held on their own.
#[derive(Clone, Debug, PartialEq)]
struct Scales {
    price: (bool, f64, f64),
    strips: BTreeMap<u32, (f64, f64)>,
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
    ///
    /// A strip does not cross the price, so the price's edge is an end of the
    /// travel too.
    fn controls(&self, at: usize) -> Vec<Control> {
        let strip = |row: Option<&Row>| row.is_some_and(|row| row.pane.is_some());
        let mut offered = Vec::new();
        if at > 0 && strip(self.rows.get(at - 1)) {
            offered.push(Control::Up);
        }
        if strip(self.rows.get(at + 1)) {
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
/// highest high. Nothing is sampled, which is what makes this safe to do to a
/// price chart — a column's wick is exactly the union of the wicks it stands
/// for, so no high and no low can go missing at any zoom, and no gap between
/// two of them is filled in. It is the picture the old code was trying to
/// draw, for a bounded amount of work.
///
/// Which bars share a pixel is decided from the start of the series, not
/// from the first bar on screen. The bars are cut into groups of the same
/// whole number, at multiples of it counted from the first bar, and every
/// group goes to the pixel its middle falls in, counted from where the
/// series begins rounded to a whole pixel. Counted from the screen, scrolling
/// by one bar moved every boundary, so every column took a different open
/// and close and candles all across the chart flipped colour at each step.
/// Counted from the series, a scroll moves the columns by whole pixels or
/// not at all, and only the ones at the edges, cut off by the plot, change.
///
/// A group is half a pixel or more, so a pixel holds one or two of them and
/// a column is still exactly a pixel, as wide as it was before grouping.
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
    /// Where the first column begins: `plot_x` at a bar a column, the
    /// pixel the first bars fall in past that.
    left: f64,
    /// One column's width: a bar's, or a pixel once a bar is narrower.
    bar_w: f64,
    /// One bar's width.
    bar_px: f64,
    /// Where the first visible bar begins, which a span of bars is placed
    /// from.
    start: f64,
    /// Where the plot ends on the right: past the last bar by whatever room
    /// is open after it, or at its end when there is none.
    end: f64,
    /// The visible bars the columns were made from.
    slice: &'a [Bar],
    /// Bars to a group: the fewest that make half a pixel.
    per: usize,
    /// The visible groups, counted from the start of the series, and the
    /// pixel the first column is, counted from the series' own first pixel.
    groups: std::ops::RangeInclusive<usize>,
    pixel: i64,
}

/// The fewest whole bars that make a group at least half a pixel wide,
/// shaved so half a pixel each exactly is not two.
fn bars_per_column(bar_px: f64) -> usize {
    ((0.5 / bar_px) - 1e-9).ceil().max(1.0) as usize
}

impl<'a> Columns<'a> {
    /// Divide `plot_w` pixels among `visible` bars, aggregating if there is
    /// more than one bar to a pixel.
    fn of(bars: &'a [Bar], first: usize, visible: usize, plot_x: f64, plot_w: f64) -> Columns<'a> {
        let visible = visible.max(1);
        let bar_px = plot_w.max(1.0) / visible as f64;
        let per = bars_per_column(bar_px);
        let mut columns = Columns {
            bars: Cow::Borrowed(bars),
            first,
            visible,
            plot_x,
            left: plot_x,
            bar_w: bar_px,
            bar_px,
            start: plot_x,
            end: plot_x + plot_w.max(1.0),
            slice: bars,
            per,
            groups: 0..=0,
            pixel: 0,
        };
        if bar_px >= 1.0 || bars.len() < visible {
            return columns;
        }
        // Where the series' first bar would begin, rounded to a pixel: the
        // origin every group's pixel is counted from.
        let origin = (plot_x - first as f64 * bar_px).round();
        let groups = first / per..=(first + visible - 1) / per;
        let end = plot_x + visible as f64 * bar_px;
        let from = columns.group_pixel(*groups.start()).max((plot_x.round() - origin) as i64);
        let to = columns.group_pixel(*groups.end()).min((end.round() - origin) as i64 - 1).max(from);
        columns.groups = groups;
        columns.pixel = from;
        columns.left = origin + from as f64;
        columns.bar_w = 1.0;
        columns.start = origin + first as f64 * bar_px;
        let count = (to - from + 1) as usize;
        let mut aggregated = Vec::with_capacity(count);
        for at in 0..count {
            let Some(group) = bars.get(columns.span(at)) else { continue };
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
            // A group's worth, so a pixel holding two is not twice as tall
            // as its neighbour holding one: a comb across the volume.
            column.volume *= per as f64 / group.len() as f64;
            aggregated.push(column);
        }
        columns.bars = Cow::Owned(aggregated);
        columns
    }

    fn len(&self) -> usize {
        self.bars.len()
    }

    /// Whether a column is a pixel holding several bars rather than a bar.
    fn sub_pixel(&self) -> bool {
        self.bar_px < 1.0
    }

    /// The pixel a group falls in, counted from the series' first pixel.
    fn group_pixel(&self, group: usize) -> i64 {
        ((group as f64 + 0.5) * self.per as f64 * self.bar_px).floor() as i64
    }

    /// The first group in `pixel` or past it. Worked out from
    /// [`group_pixel`](Columns::group_pixel) itself, so the two can never
    /// disagree about a group sitting on a boundary.
    fn group_from(&self, pixel: i64) -> usize {
        let wide = self.per as f64 * self.bar_px;
        let mut group = (pixel as f64 / wide - 0.5).ceil().max(0.0) as usize;
        while group > 0 && self.group_pixel(group - 1) >= pixel {
            group -= 1;
        }
        while self.group_pixel(group) < pixel {
            group += 1;
        }
        group
    }

    /// The visible bars a column stands for, counted from the first visible
    /// bar. The two at the ends are cut short by the edges of the plot.
    fn span(&self, at: usize) -> std::ops::Range<usize> {
        if !self.sub_pixel() {
            return at..at + 1;
        }
        let (first, last) = (*self.groups.start(), *self.groups.end());
        let pixel = self.pixel + at as i64;
        let from = if at == 0 { first } else { self.group_from(pixel).max(first) };
        let to = if at + 1 >= self.len() { last + 1 } else { self.group_from(pixel + 1).min(last + 1) };
        let bar = |group: usize| (group * self.per).clamp(self.first, self.first + self.visible) - self.first;
        bar(from)..bar(to)
    }

    /// How far left of `plot_x` the first column begins.
    fn shift(&self) -> f64 {
        self.plot_x - self.left
    }

    /// The middle of a column, which is where a candle is centred.
    fn x(&self, at: usize) -> f64 {
        self.left + (at as f64 + 0.5) * self.bar_w
    }

    /// How many bars a column stands for, on average: one at a bar a
    /// column, and as many as a pixel holds past that.
    fn bars_each(&self) -> f64 {
        match self.sub_pixel() {
            true => 1.0 / self.bar_px,
            false => 1.0,
        }
    }

    /// A candle body's whole pixels, as a left edge and a width: an odd
    /// number of them, with the pixel the wick runs down in the middle.
    ///
    /// Rounded apart, the body's left edge and the wick each took the nearest
    /// pixel on their own, so which side of the middle the wick fell on
    /// depended on where the column's centre sat within its pixel. Anything
    /// that resized the plot — a panel opening beside it — moved every centre
    /// and knocked half the wicks a pixel off their bodies.
    ///
    /// Odd only while that leaves a pixel to the next candle. Neighbours land
    /// as few as the column's width rounded down apart, and at three to four
    /// pixels a column a three-pixel body touched them, fusing a run of
    /// candles into one block. There the body keeps the width it had, and
    /// the wick takes the same side of its middle on every candle.
    fn body(&self, at: usize) -> (f64, f64) {
        let wanted = (self.bar_w * 0.68).clamp(1.0, 24.0);
        let room = (self.bar_w.floor() - 1.0).max(1.0);
        let odd = ((wanted - 1.0) / 2.0).round() * 2.0 + 1.0;
        let width = if odd <= room { odd } else { wanted.round().min(room) };
        (self.x(at).round() - ((width - 1.0) / 2.0).floor(), width)
    }

    /// The pixel a column holding several bars is, as a left edge and a
    /// width. `None` at a bar a column, where a column is drawn by its
    /// middle.
    ///
    /// Filled rather than stroked down the middle, so every pixel under the
    /// bars is drawn and the chart is solid, with nothing for a scroll to
    /// open and close between the columns.
    fn tile(&self, at: usize) -> Option<(f64, f64)> {
        self.sub_pixel().then_some((self.left + at as f64, 1.0))
    }

    /// A column's wick as high-low pairs, one per run of its bars whose ranges
    /// overlap. One line from the lowest low to the highest high would fill in
    /// any gap between two bars that happened to share the column.
    fn wicks(&self, at: usize) -> impl Iterator<Item = (f64, f64)> + '_ {
        let group = match self.bars {
            Cow::Borrowed(_) => self.bars.get(at..at + 1).unwrap_or_default(),
            Cow::Owned(_) => self.slice.get(self.span(at)).unwrap_or_default(),
        };
        let mut group = group.iter().peekable();
        std::iter::from_fn(move || {
            let first = group.next()?;
            let (mut high, mut low) = (first.high, first.low);
            while let Some(bar) = group.next_if(|bar| bar.low <= high && bar.high >= low) {
                high = high.max(bar.high);
                low = low.min(bar.low);
            }
            Some((high, low))
        })
    }

    /// The bar a column is read from when a series is indexed by bar: the last
    /// one in it, which is the bar whose close the column carries.
    fn source(&self, at: usize) -> usize {
        self.first + self.offset(at)
    }

    /// The same, counted from the first visible bar.
    fn offset(&self, at: usize) -> usize {
        self.span(at).end.saturating_sub(1)
    }

    /// Where a bar's own column begins, in pixels. For the things drawn across
    /// a span of bars rather than at one.
    fn left_of(&self, index: usize) -> f64 {
        self.start + index.saturating_sub(self.first) as f64 * self.bar_px
    }

    /// How wide the bars from `from` up to but not including `to` are.
    fn width_of(&self, from: usize, to: usize) -> f64 {
        self.left_of(to) - self.left_of(from)
    }

    /// The empty space past the last bar on screen, in pixels: where the
    /// series ends short of the plot's right edge. Nothing with the bars
    /// running to the edge, or off it.
    fn room_after(&self) -> f64 {
        (self.end - self.left_of(self.first + self.visible)).max(0.0)
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

/// Move a drawing by a number of columns and a distance up the price scale,
/// in the scale's own units — a ratio, on a log axis.
///
/// Through the index and back, rather than by adding to the timestamps,
/// because the chart's columns are evenly spaced and its timestamps are not:
/// see the body drag for what that costs.
fn shift_by_columns(
    bars: &[Bar],
    drawing: &mut Drawing,
    by_index: f64,
    scale: &PriceScale,
    by_price: f64,
) {
    // Moved whole, a profile would be a profile of other bars, and a VWAP a
    // VWAP from another bar: their anchors are what change what they cover.
    if drawing.kind.reads_bars() {
        return;
    }
    let move_anchor = |anchor: &mut Anchor| {
        anchor.ts = ts_at(bars, index_of_ts(bars, anchor.ts) + by_index);
        anchor.price = scale.shift(anchor.price, by_price);
    };
    // Every corner, not only the two ends. A zig-zag is drawn from its own
    // corners, so a shift that moved the ends and left them behind moved
    // nothing anybody could see — which is exactly what it did.
    for anchor in drawing.points.iter_mut() {
        move_anchor(anchor);
    }
    move_anchor(&mut drawing.from);
    move_anchor(&mut drawing.to);
    // The ends are the ends of the run, and were just moved the same way as
    // the corners, so they already agree; this is only in case a drawing
    // ever carries corners that do not reach them.
    if let (Some(first), Some(last)) = (drawing.points.first(), drawing.points.last()) {
        drawing.from = *first;
        drawing.to = *last;
    }
}

/// An anchor pulled onto a bar the chart has: a press past the last bar is
/// the last, and one before the first the first. An anchored VWAP from a
/// moment no bar has reached yet would be a drawing with nothing to draw.
fn anchor_on_a_bar(bars: &[Bar], at: Anchor) -> Anchor {
    match (bars.first(), bars.last()) {
        (Some(first), _) if at.ts < first.ts => Anchor::new(first.ts, at.price),
        (_, Some(last)) if at.ts > last.ts => Anchor::new(last.ts, at.price),
        _ => at,
    }
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

/// How prices become heights on the price plot, and back.
///
/// `low` and `high` are in the scale's own units: prices on a linear axis,
/// their natural logarithms on a logarithmic one. Padding, the user's zoom
/// and the user's drag all work in those units, which is what lets one
/// gesture code serve both axes — dragging a log axis moves it by a ratio
/// because a difference of logarithms is one.
///
/// One value rather than a `(low, high)` pair beside a flag, because every
/// place that turned a price into a pixel had its own copy of the formula,
/// and a second formula is a second copy of each.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PriceScale {
    low: f64,
    high: f64,
    log: bool,
}

impl PriceScale {
    /// A price in the scale's own units. A price at or below zero has no
    /// logarithm; it is taken as the smallest positive number instead, which
    /// puts it far below the plot, where the clip hides it, rather than at
    /// NaN, where nothing can be said about it.
    fn value(self, price: f64) -> f64 {
        match self.log {
            true => price.max(f64::MIN_POSITIVE).ln(),
            false => price,
        }
    }

    /// The inverse of [`value`](Self::value).
    fn price(self, value: f64) -> f64 {
        match self.log {
            true => value.exp(),
            false => value,
        }
    }

    fn span(self) -> f64 {
        self.high - self.low
    }

    /// The height of a price on a plot `h` tall starting at `top`.
    fn to_y(self, price: f64, top: f64, h: f64) -> f64 {
        top + h * (self.high - self.value(price)) / self.span()
    }

    /// The price at a height on a plot `h` tall starting at `top`.
    fn price_at(self, y: f64, top: f64, h: f64) -> f64 {
        self.price(self.high - (y - top) / h * self.span())
    }

    /// A price moved by a distance in the scale's own units: by a difference
    /// on a linear axis and by a ratio on a log one, so a drawing dragged up
    /// a log chart keeps its shape on screen.
    fn shift(self, price: f64, by: f64) -> f64 {
        match by == 0.0 {
            true => price,
            false => self.price(self.value(price) + by),
        }
    }

    /// The lowest and highest prices on the plot.
    fn prices(self) -> (f64, f64) {
        (self.price(self.low), self.price(self.high))
    }

    /// The gap between gridlines, and so how precisely prices are written.
    ///
    /// On a log axis the gap in price grows as the price does, so it is
    /// taken at the bottom of the plot, where it is smallest: precision
    /// enough to tell the lowest two lines apart is enough for the rest.
    fn step(self, h: f64) -> f64 {
        let rows = (h / 52.0).max(2.0);
        match self.log {
            false => nice_step(self.span(), rows as usize),
            true => {
                let low = self.price(self.low);
                nice_step(low * ((self.span() / rows).exp() - 1.0), 1)
            }
        }
    }

    /// The prices the axis is labelled at, lowest first.
    ///
    /// A linear axis counts up in one round step. A log axis walks up in
    /// equal heights and rounds each to a step that suits its own magnitude
    /// — 10, 20, 50, 100, 200 — dropping one that rounds onto the last or
    /// lands too close to it to be read.
    fn ticks(self, h: f64, mut each: impl FnMut(f64)) {
        if self.span().is_nan() || self.span() <= 0.0 {
            return;
        }
        let rows = (h / 52.0).max(2.0);
        if !self.log {
            let step = nice_step(self.span(), rows as usize);
            if step <= 0.0 {
                return;
            }
            let mut price = (self.low / step).ceil() * step;
            while price <= self.high {
                each(price);
                price += step;
            }
            return;
        }
        let gap = self.span() / rows;
        let mut last: Option<f64> = None;
        // Counted rather than walked to the top, so no gap can be too small
        // to move a float along.
        for row in 0..=rows.ceil() as usize + 1 {
            let rough = self.price(self.low + row as f64 * gap);
            let step = nice_step(rough * (gap.exp() - 1.0), 1);
            if step <= 0.0 {
                continue;
            }
            let price = (rough / step).round() * step;
            let at = self.value(price);
            if price <= 0.0 || at < self.low || at > self.high {
                continue;
            }
            // Half a row apart at the least, measured where it is drawn.
            if last.is_some_and(|last| at - self.value(last) < gap * 0.5) {
                continue;
            }
            each(price);
            last = Some(price);
        }
    }
}

/// The price scale over the visible bars, padded so candles never touch the
/// edges, and then held to whatever vertical window the user has dragged to.
///
/// Shared rather than inlined because the pointer has to answer the same
/// question the drawing does: the price under the crosshair is only the same
/// number on two charts if both worked it out from the same scale.
///
/// Logarithmic when the chart asks for it and every visible bar is above
/// zero. A spread or a contract that has traded negative has no logarithm, and
/// the chart draws that stretch linear rather than drawing nothing; the choice
/// is kept, so it comes back once the bars on screen allow it.
///
/// `None` when the bars carry nothing finite to measure.
fn price_range(state: &State, bars: &[Bar], plan: &Layout) -> Option<PriceScale> {
    let (mut low, mut high) = (f64::MAX, f64::MIN);
    // A line is drawn through the closes alone, so it fits them rather than
    // the ranges it never shows.
    let line = state.drawn_as_line();
    for b in bars {
        low = low.min(if line { b.close } else { b.low });
        high = high.max(if line { b.close } else { b.high });
    }
    if let Some(compare) = &state.compare {
        let (first, _) = state.slice();
        (low, high) = compare.fit(&state.bars, first, bars.len(), low, high);
    }
    // The latest trade the bars leave out, while the chart shows its end:
    // its line would otherwise run off the plot, out of hours, the moment
    // the price left the session's range.
    if let Some(latest) = state.latest
        && bars.last().map(|bar| bar.ts) == state.bars.last().map(|bar| bar.ts)
    {
        low = low.min(latest);
        high = high.max(latest);
    }
    if !low.is_finite() || !high.is_finite() {
        return None;
    }
    let log = state.price_log && low > 0.0;
    if log {
        (low, high) = (low.ln(), high.ln());
    }
    if (high - low).abs() < f64::EPSILON {
        // A flat series: a dollar either side, or a percent on a log axis,
        // where a whole unit would be a factor of e.
        let air = if log { 0.01 } else { 1.0 };
        high += air;
        low -= air;
    }
    let span = high - low;
    low -= span * PRICE_AIR;
    // The top gets its air in pixels, because what it has to clear is drawn
    // in pixels: the header and the legend sit over the top of the plot
    // whenever the price is the top row. Capped at a share of the height, so
    // a chart in a quarter of the window keeps its candles.
    high += match plan.price_y <= plan.top {
        true => {
            let air = (PRICE_TOP_AIR / plan.price_h).min(PRICE_TOP_AIR_SHARE);
            (high - low) * air / (1.0 - air)
        }
        false => span * PRICE_AIR,
    };
    let (low, high) = state.price_window(low, high, log);
    Some(PriceScale { low, high, log })
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
            .shown_indicators()
            .filter(|drawn| drawn.indicator.above_price == above)
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
    /// The step its prices are on: a futures contract's, from its
    /// specification, and otherwise whatever step the bars show.
    fn tick(&self) -> Option<f64> {
        self.instrument.as_ref().and_then(Instrument::tick_size).or_else(|| omacharts_engine::tick_of(&self.bars))
    }

    /// A chart with nothing on it yet.
    ///
    /// Named rather than written out where it is used, because the benchmark
    /// needs the same starting point the widget has: a frame measured from a
    /// state assembled by hand is a frame of something else.
    fn blank(theme: Theme, scheme: BarScheme) -> State {
        State {
            bars: Vec::new(),
            echo: None,
            last_ink: LastInk::of(&theme),
            delta_inks: delta_inks(&theme, HOUSE_LOOK),
            house: HOUSE_LOOK,
            theme,
            price_tags: price_tags(&scheme),
            scheme,
            latest: None,
            instrument: None,
            profile_cache: RefCell::new(Vec::new()),
            vwap_cache: RefCell::new(Vec::new()),
            timeframe: Timeframe::days(1),
            first: 0,
            visible: 160,
            pointer: None,
            magnet: false,
            hover_grip: None,
            closed_by_press: false,
            anchored: true,
            overhang: RIGHT_MARGIN,
            overhang_by_hand: false,
            overhang_share: 0.0,
            lead: 0,
            range: None,
            indicators: Vec::new(),
            show_indicators: true,
            covered_axis_top: 0.0,
            axis_corner_taken: false,
            axis_start_taken: 0.0,
            bar_style: BarStyle::default(),
            ohlc_thickness: DEFAULT_OHLC_THICKNESS,
            bars_hidden: false,
            show_grid: false,
            clock: Clock::default(),
            drag: None,
            trouble: None,
            trouble_words: None,
            marks: Marks::default(),
            mark_labels: LabelSide::default(),
            mark_tick: None,
            hover_mark: None,
            loading: false,
            progress: None,
            progress_target: 0.0,
            progress_easing: false,
            history_asked: Cell::new(None),
            price_zoom: 1.0,
            price_offset: 0.0,
            price_auto: true,
            price_base: Cell::new(None),
            strip_held: BTreeMap::new(),
            strip_kinds: BTreeMap::new(),
            price_log: false,
            drawings: Vec::new(),
            show_drawings: true,
            sticky: false,
            editing: None,
            editing_at: None,
            selected: Vec::new(),
            picked: None,
            pending_pick: None,
            marquee_base: Vec::new(),
            tool: None,
            placing: None,
            ruler_armed: false,
            ruler: None,
            next_config: 1,
            configs: Configurations::default(),
            sends: true,
            undo: Vec::new(),
            redo: Vec::new(),
            compare: None,
            on_compare_readout: Rc::new(RefCell::new(None)),
        }
    }

    /// The indicators on screen: the ones shown, while the chart shows any.
    fn shown_indicators(&self) -> impl DoubleEndedIterator<Item = &Drawn> {
        self.indicators.iter().filter(|drawn| self.show_indicators && drawn.indicator.visible)
    }

    /// Drop the step most recently remembered, for a change that turned out
    /// not to be one: the step is still the top of the stack, because
    /// nothing else can have been remembered in between.
    fn forget(&mut self) {
        self.undo.pop();
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
        // A drawing with no id yet has only just been made and has not come
        // back from wherever drawings are kept, so there is nothing to find
        // it by: it is one of the newest rows. One of those used to mean
        // *the* newest row, which was right while only one drawing could be
        // made at a time — and then pasting three of them selected the same
        // last row three times over, and the dedup below left one selected.
        // As many as there are, in the order they went down.
        let fresh = ids.iter().filter(|id| **id == 0).count();
        let mut next = self.drawings.len().saturating_sub(fresh);
        self.selected = ids
            .into_iter()
            .filter_map(|id| match id {
                0 => {
                    let at = (next < self.drawings.len()).then_some(next);
                    next += 1;
                    at
                }
                id => self.drawings.iter().position(|d| d.id == id),
            })
            .collect();
        self.selected.dedup();
    }

    fn selected_ids(&self) -> Vec<i64> {
        self.selected.iter().filter_map(|i| self.drawings.get(*i)).map(|d| d.id).collect()
    }

    /// Whether the hand is placing an anchor with the magnet on: a tool in
    /// hand, a drawing being laid down, or a corner being dragged, with Ctrl
    /// held. Moving a whole drawing does not snap, so it is not this.
    fn snapping(&self) -> bool {
        let corner = matches!(self.drag, Some(Drag::Grip { grip, .. }) if grip != Grip::Body);
        self.magnet && (self.tool.is_some() || self.placing.is_some() || corner)
    }

    /// A drawing with one anchor down follows the pointer with the other,
    /// button or no button: click, move, click is the way a line is drawn,
    /// and the line has to be there during the move.
    fn follow_hand(&mut self, width: f64, height: f64) {
        if self.drag.is_some() {
            return;
        }
        let Some((x, y)) = self.pointer else { return };
        if let Some(anchor) = self.locate_with(width, height, x, y, self.magnet)
            && let Some(placing) = self.placing.as_mut()
        {
            match placing.kind.is_path() {
                true => placing.move_last_corner(anchor),
                false => placing.move_grip(Grip::To, anchor)
            }
        }
    }

    /// Where a pixel is in the chart's own units: the moment of the bar
    /// under it and the price at that height. `None` when there is nothing
    /// on the chart to measure against.
    ///
    /// The moment snaps to a bar, so a drawing dropped on a candle lands on
    /// it exactly; past either end of the loaded bars it keeps counting at
    /// the last bar's step, so a line can be drawn into the future.
    fn locate(&self, width: f64, height: f64, x: f64, y: f64) -> Option<Anchor> {
        self.locate_with(width, height, x, y, false)
    }

    /// [`locate`](Self::locate), and with `snap` the price is pulled onto
    /// the candle under the pointer: whichever of the prices the chart draws
    /// of it is nearest on screen. That is Ctrl while a drawing is laid down
    /// or a corner dragged, the magnet every charting tool has, because a
    /// line from one low to another is meant to touch both lows exactly and
    /// a hand lands a pixel off. Past either end of the bars there is no
    /// candle and the price is where the pointer is.
    fn locate_with(&self, width: f64, height: f64, x: f64, y: f64, snap: bool) -> Option<Anchor> {
        let (first, visible, lead) = self.view();
        if visible == 0 {
            return None;
        }
        let plan = layout(self, width, height);
        let scale = price_range(self, &self.bars[first..first + visible], &plan)?;
        let price = scale.price_at(y, plan.price_y, plan.price_h);
        // A column's width, the room at either end counted: the plot's
        // first column may stand before the first bar.
        let bar_w = plan.plot_w / self.columns().max(1) as f64;
        let index = (first as f64 - lead as f64 + (x - plan.plot_x) / bar_w - 0.5).round();
        let price = match snap && index >= 0.0 {
            true => self.bars.get(index as usize).map_or(price, |bar| {
                let off = |price: f64| (scale.to_y(price, plan.price_y, plan.price_h) - y).abs();
                [bar.open, bar.high, bar.low, bar.close]
                    .into_iter()
                    .min_by(|a, b| off(*a).total_cmp(&off(*b)))
                    .unwrap_or(price)
            }),
            false => price,
        };
        Some(Anchor::new(ts_at(&self.bars, index), price))
    }

    /// The first press with a tool in hand: a drawing of that kind, in the
    /// configuration chosen for it and the scope this chart draws into, with
    /// both anchors where the hand is until the hand moves.
    fn begin_placing(&mut self, width: f64, height: f64, x: f64, y: f64, snap: bool) -> bool {
        let Some(kind) = self.tool else { return false };
        let Some(anchor) = self.locate_with(width, height, x, y, snap) else { return false };
        let mut drawing = match kind.is_path() {
            true => Drawing::zigzag_from(anchor),
            false => Drawing::new(kind, anchor, anchor),
        };
        drawing.follow(self.next_config);
        drawing.scope = scope_for(self.sends);
        self.placing = Some(drawing);
        self.drag = Some(Drag::Place { moved: false });
        true
    }

    fn ruler_anchor(&self, width: f64, height: f64, x: f64, y: f64) -> Option<Anchor> {
        let plan = layout(self, width, height);
        if !(plan.plot_x..=plan.plot_x + plan.plot_w).contains(&x)
            || !(plan.price_y..=plan.price_y + plan.price_h).contains(&y)
        {
            return None;
        }
        self.locate(width, height, x, y)
    }

    fn ruler_click(&mut self, anchor: Anchor) {
        self.ruler = Some(match self.ruler {
            Some((from, _, false)) => (from, anchor, true),
            _ => (anchor, anchor, false),
        });
        self.drag = None;
    }

    fn ruler_motion(&mut self, anchor: Anchor) {
        if let Some((from, _, false)) = self.ruler {
            self.ruler = Some((from, anchor, false));
        }
    }

    fn project_anchor(&self, plan: &Layout, scale: &PriceScale, anchor: Anchor) -> (f64, f64) {
        let (first, _, lead) = self.view();
        let bar_w = plan.plot_w / self.columns().max(1) as f64;
        let left = first as f64 - lead as f64;
        let index = index_of_ts(&self.bars, anchor.ts);
        (
            plan.plot_x + (index - left + 0.5) * bar_w,
            scale.to_y(anchor.price, plan.price_y, plan.price_h),
        )
    }

    /// The volume profile a profile drawing shows: of the bars between its
    /// two anchors, on this chart's own bars.
    fn span_profile(&self, drawing: &Drawing) -> Option<Profile> {
        let (from, to) = (drawing.from.ts.min(drawing.to.ts), drawing.from.ts.max(drawing.to.ts));
        let first = self.bars.partition_point(|bar| bar.ts < from);
        let end = self.bars.partition_point(|bar| bar.ts <= to);
        if first >= end {
            return None;
        }
        let look = &drawing.style(&self.configs).profile;
        // The last bar's volume as well as its time: a live bar grows in
        // place, and a profile over it has to grow with it.
        let last = &self.bars[end - 1];
        let key = ProfileKey {
            span: (first, end),
            first: self.bars[first].ts,
            last: (last.ts, last.volume.to_bits(), last.high.to_bits(), last.low.to_bits()),
            rows: look.rows,
            value_area: look.value_area.to_bits(),
        };
        if let Some((_, profile)) = self.profile_cache.borrow().iter().find(|(k, _)| *k == key) {
            return profile.clone();
        }
        let kind = self.instrument.as_ref().map(|i| i.kind);
        let profile =
            omacharts_engine::indicators::profile::of_span(&self.bars[first..end], first, look.rows, look.value_area, kind);
        let mut cache = self.profile_cache.borrow_mut();
        // A handful of drawings, each dragged through a few spans: enough
        // to keep, small enough never to sweep.
        if cache.len() >= 32 {
            cache.remove(0);
        }
        cache.push((key, profile.clone()));
        profile
    }

    /// The anchored VWAP a VWAP drawing shows, and the bar it starts from:
    /// every bar from the one at its anchor to the last, on this chart's
    /// own bars, so it grows as they arrive. `None` before the first bar
    /// reaches the anchor.
    fn anchored_vwap(&self, drawing: &Drawing) -> Option<(usize, Rc<Vec<omacharts_engine::indicators::vwap::Anchored>>)> {
        let start = self.bars.partition_point(|bar| bar.ts < drawing.from.ts);
        let last = self.bars.last()?;
        if start >= self.bars.len() {
            return None;
        }
        let source = drawing.style(&self.configs).vwap.source;
        // The last bar whole, as for a profile: a live bar grows in place.
        let key = VwapKey {
            start: (start, self.bars[start].ts),
            len: self.bars.len(),
            last: (last.ts, last.volume.to_bits(), last.high.to_bits(), last.low.to_bits(), last.close.to_bits()),
            source,
        };
        if let Some((_, line)) = self.vwap_cache.borrow().iter().find(|(k, _)| *k == key) {
            return Some((start, line.clone()));
        }
        let line = Rc::new(omacharts_engine::indicators::vwap::anchored(&self.bars[start..], source));
        let mut cache = self.vwap_cache.borrow_mut();
        if cache.len() >= 32 {
            cache.remove(0);
        }
        cache.push((key, line.clone()));
        Some((start, line))
    }

    /// The bars of an anchored VWAP that are on screen, with one either side
    /// so the line runs off the edge rather than stopping short of it.
    fn vwap_on_screen(&self, start: usize, count: usize) -> std::ops::Range<usize> {
        let (first, visible, _) = self.view();
        let from = first.saturating_sub(1).max(start);
        let to = (first + visible + 1).min(start + count);
        from..to.max(from)
    }

    /// Where a bar's column begins, in pixels.
    fn column_edge(&self, plan: &Layout, index: f64) -> f64 {
        let (first, _, lead) = self.view();
        let bar_w = plan.plot_w / self.columns().max(1) as f64;
        plan.plot_x + (index - (first as f64 - lead as f64)) * bar_w
    }

    /// A drawing as pixels, through the same scales the candles use.
    fn project(&self, plan: &Layout, scale: &PriceScale, drawing: &Drawing) -> Projected {
        let point = |anchor: &Anchor| self.project_anchor(plan, scale, *anchor);
        // A profile is as wide as the bars it covers, edge to edge, and as
        // tall as the prices they reached: its anchors only say when.
        if drawing.kind == DrawingKind::Profile
            && let Some(profile) = self.span_profile(drawing)
            && let (Some(low), Some(high)) = (profile.rows.first(), profile.rows.last())
        {
            let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
            let left = self.column_edge(plan, profile.first_bar as f64);
            let right = self.column_edge(plan, profile.last_bar as f64 + 1.0);
            return Projected::new(drawing.kind, (left, to_y(high.high)), (right, to_y(low.low)));
        }
        // An anchored VWAP is the line through its values, one a bar, from
        // the anchor on; its anchor is where the line starts, at the
        // anchor bar's own value rather than wherever the hand clicked.
        if drawing.kind == DrawingKind::Vwap {
            let Some((start, line)) = self.anchored_vwap(drawing) else {
                return Projected::new(drawing.kind, point(&drawing.from), point(&drawing.from));
            };
            let at = |index: usize| {
                let value = line[index - start].vwap;
                (
                    self.column_edge(plan, index as f64 + 0.5),
                    scale.to_y(value, plan.price_y, plan.price_h),
                )
            };
            let corners: Vec<(f64, f64)> = self.vwap_on_screen(start, line.len()).map(at).collect();
            let anchor = at(start);
            let end = corners.last().copied().unwrap_or(anchor);
            return Projected { from: anchor, to: end, ..Projected::through(drawing.kind, corners) };
        }
        // A retracement is its two ends, and a height for each level it
        // draws: through the price scale, since on a log scale the levels
        // are not evenly spaced on screen.
        if drawing.kind == DrawingKind::Fib {
            let look = &drawing.style(&self.configs).fib;
            let levels = look
                .enabled()
                .map(|level| {
                    scale.to_y(level.price(drawing.from.price, drawing.to.price), plan.price_y, plan.price_h)
                })
                .collect();
            return Projected::new(drawing.kind, point(&drawing.from), point(&drawing.to)).with_levels(levels);
        }
        let projected = match drawing.kind.is_path() {
            true => Projected::through(drawing.kind, drawing.corners().iter().map(point).collect()),
            false => Projected::new(drawing.kind, point(&drawing.from), point(&drawing.to)),
        };
        // Measured here, for every caller, because the words are part of
        // the drawing and not only part of the picture of it: a label is
        // what the pointer takes hold of when it sits clear of the stroke,
        // which on a line it usually does.
        //
        // This is a Pango layout per *labelled* drawing per question asked,
        // and the questions include "what is under the pointer", which is
        // asked on every motion. A drawing with nothing written on it pays
        // nothing, which is most of them.
        match drawing.has_text() {
            false => projected,
            true => self.with_words(projected, drawing),
        }
    }

    fn with_words(&self, projected: Projected, drawing: &Drawing) -> Projected {
        match crate::text::block(
            drawing.kind,
            projected.bounds(),
            &drawing.text,
            drawing.style(&self.configs),
        ) {
            Some(words) => projected.with_words(words),
            None => projected,
        }
    }

    /// Put copies down, offset if asked, and take hold of them. Hands back
    /// what was added, for whoever has to write it down.
    fn paste(
        &mut self,
        copied: &[Drawing],
        offset: bool,
        width: f64,
        height: f64,
    ) -> Vec<Drawing> {
        if copied.is_empty() {
            return Vec::new();
        }
        let (first, visible) = self.slice();
        if visible == 0 {
            return Vec::new();
        }
        let plan = layout(self, width, height);
        let Some(scale) = price_range(self, &self.bars[first..first + visible], &plan) else {
            return Vec::new();
        };
        let bar_w = plan.plot_w / self.columns().max(1) as f64;
        let (by_index, by_price) = match offset {
            true => (PASTE_OFFSET / bar_w, -PASTE_OFFSET / plan.price_h * scale.span()),
            false => (0.0, 0.0),
        };
        let order = self.drawings.iter().map(|d| d.order).max().unwrap_or(0);
        let scope = scope_for(self.sends);
        self.remember();
        let mut added = Vec::new();
        let State { bars, drawings, .. } = self;
        for copy in copied {
            let mut copy = copy.clone();
            copy.id = 0;
            copy.order = order;
            copy.scope = scope;
            shift_by_columns(bars, &mut copy, by_index, &scale, by_price);
            // A profile is not moved whole, but a copy put down exactly on
            // its original would be one nobody could see: its span moves.
            // So does an anchored VWAP's anchor.
            if copy.kind.reads_bars() {
                for anchor in [&mut copy.from, &mut copy.to] {
                    anchor.ts = ts_at(bars, index_of_ts(bars, anchor.ts) + by_index);
                }
            }
            drawings.push(copy.clone());
            added.push(copy);
        }
        // The copies are what is in hand now, which is what lets a paste be
        // nudged, restyled or pasted on again straight away.
        let from = self.drawings.len() - added.len();
        self.selected = (from..self.drawings.len()).collect();
        added
    }

    /// Every drawing with any of itself on the plot right now: what Ctrl+A
    /// takes. One scrolled or zoomed out of view is not part of "all".
    fn visible_drawings(&self, width: f64, height: f64) -> Vec<usize> {
        let plan = layout(self, width, height);
        self.drawings_in(width, height, (plan.plot_x, plan.price_y, plan.plot_w, plan.price_h))
    }

    /// Every drawing a box in pixels touches, in list order.
    fn drawings_in(&self, width: f64, height: f64, rect: (f64, f64, f64, f64)) -> Vec<usize> {
        let (first, visible) = self.slice();
        if visible == 0 || !self.show_drawings {
            return Vec::new();
        }
        let plan = layout(self, width, height);
        let Some(scale) = price_range(self, &self.bars[first..first + visible], &plan) else {
            return Vec::new();
        };
        (0..self.drawings.len())
            .filter(|i| self.project(&plan, &scale, &self.drawings[*i]).touches(rect))
            .collect()
    }

    /// The grip on drawing `index` that the hand is on, if any, and whether
    /// it is holding it. A body is never lit: the whole drawing is not a
    /// point to aim at.
    fn lit_grip(&self, index: usize) -> Option<(Grip, Lit)> {
        match self.drag {
            Some(Drag::Grip { index: held, grip, .. }) => {
                (held == index && grip != Grip::Body).then_some((grip, Lit::Held))
            }
            Some(_) => None,
            None => self
                .hover_grip
                .filter(|(at, grip)| *at == index && *grip != Grip::Body)
                .map(|(_, grip)| (grip, Lit::Hover)),
        }
    }

    /// Whether a press at (`x`, `y`) lands on the last corner a zig-zag
    /// being laid down has pinned, which ends it there.
    ///
    /// A double-click ended one, but only a quick one: two presses further
    /// apart than the desktop's double-click time are two presses, and each
    /// pinned another corner on the same spot — a leg nobody can see, and an
    /// end that comes apart when dragged. A press on the last corner is what
    /// a slow double-click meant, and something to aim at besides.
    ///
    /// Only once two corners are pinned: ending on the first would leave a
    /// single point, which is no drawing.
    fn closes_path(&self, width: f64, height: f64, x: f64, y: f64) -> bool {
        let Some(placing) = &self.placing else { return false };
        let pinned = placing.points.len().saturating_sub(1);
        if !placing.kind.is_path() || pinned < 2 {
            return false;
        }
        let (first, visible) = self.slice();
        if visible == 0 {
            return false;
        }
        let plan = layout(self, width, height);
        let Some(scale) = price_range(self, &self.bars[first..first + visible], &plan) else {
            return false;
        };
        let projected = self.project(&plan, &scale, placing);
        projected.corners.get(pinned - 1).is_some_and(|at| (at.0 - x).hypot(at.1 - y) <= omacharts_engine::drawings::GRIP_REACH)
    }

    /// The cursor for grip `grip` of drawing `index`: which way it moves
    /// what it is on.
    ///
    /// The same over a grip and while it is held, so the pointer does not
    /// turn into an arrow halfway through resizing a box.
    fn grip_cursor(&self, index: usize, grip: Grip, held: bool) -> &'static str {
        let Some(drawing) = self.drawings.get(index) else { return "default" };
        let (from, to) = (drawing.from, drawing.to);
        match grip {
            Grip::Top | Grip::Bottom => "ns-resize",
            Grip::Left | Grip::Right => "ew-resize",
            // Not moved whole, so nothing to take hold of; and its corners
            // move only sideways.
            Grip::Body if drawing.kind.reads_bars() => "default",
            _ if drawing.kind.reads_bars() => "ew-resize",
            Grip::Body => match held {
                true => "grabbing",
                false => "grab",
            },
            // A box's corner, by the diagonal it sits on as drawn rather than
            // by which anchor it is: a box drawn from the bottom up has `from`
            // at its bottom left, and its anchors named the wrong diagonal.
            // Later is always further right and higher always further up, so
            // the anchors say where it sits without projecting anything.
            Grip::From | Grip::To | Grip::FromTo | Grip::ToFrom if drawing.kind.is_bounded() => {
                let (ts, price) = match grip {
                    Grip::From => (from.ts, from.price),
                    Grip::To => (to.ts, to.price),
                    Grip::FromTo => (from.ts, to.price),
                    _ => (to.ts, from.price),
                };
                let left = ts == from.ts.min(to.ts);
                let top = price == from.price.max(to.price);
                match left == top {
                    true => "nwse-resize",
                    false => "nesw-resize",
                }
            }
            // A point: the ring round it says it is under the hand, and the
            // arrow's tip shows exactly where it will land, which a hand or a
            // cross of arrows would cover.
            _ => "default",
        }
    }

    /// The drawing under a pixel, topmost first, and which part of it.
    fn drawing_at(&self, width: f64, height: f64, x: f64, y: f64) -> Option<(usize, Grip)> {
        let (first, visible) = self.slice();
        if visible == 0 || !self.show_drawings {
            return None;
        }
        let plan = layout(self, width, height);
        let scale = price_range(self, &self.bars[first..first + visible], &plan)?;
        // The selected ones first, since their grips are what the hand is
        // most likely reaching for; then the most recently drawn.
        let order = self.selected.iter().rev().copied().chain((0..self.drawings.len()).rev());
        for index in order {
            let drawing = self.drawings.get(index)?;
            let projected = self.project(&plan, &scale, drawing);
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

    /// The price range to draw, after the user's scaling, from the fit of
    /// the bars on screen: `low` and `high` on the axis, logarithms when
    /// `log`.
    ///
    /// Automatic draws the fit. Held draws the range held when it was taken
    /// off automatic, zoomed and shifted — which starts as the fit then, so
    /// taking manual control never makes the chart jump, and stays put
    /// however the bars on screen change after.
    fn price_window(&self, low: f64, high: f64, log: bool) -> (f64, f64) {
        if self.price_auto {
            return (low, high);
        }
        let (low, high) = self.held_range(low, high, log);
        let range = (high - low).max(f64::EPSILON);
        let middle = (low + high) / 2.0 + range * self.price_offset;
        let half = range / 2.0 / self.price_zoom.clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM);
        (middle - half, middle + half)
    }

    /// The range a held scale works on, on the axis: the one it holds, or
    /// the fit it is handed if it holds none yet, which it then keeps.
    fn held_range(&self, low: f64, high: f64, log: bool) -> (f64, f64) {
        let to_price = |value: f64| if log { value.exp() } else { value };
        let fit = (to_price(low), to_price(high));
        let (held_low, held_high) = self.price_base.get().unwrap_or_else(|| {
            self.price_base.set(Some(fit));
            fit
        });
        if !log {
            return (held_low, held_high);
        }
        if held_low > 0.0 {
            return (held_low.ln(), held_high.ln());
        }
        // Held on a linear axis down to zero or below, which has no
        // logarithm: the log axis holds its own fit from here instead.
        self.price_base.set(Some(fit));
        (low, high)
    }

    /// The prices a held scale shows, bottom and top, which is what is
    /// written down to bring it back after a restart. Prices rather than a
    /// zoom on what is on screen, because what is on screen after a restart
    /// is the live edge, with whatever has traded since: the prices somebody
    /// zoomed to are the ones they wanted to look at, wherever the bars went.
    ///
    /// `None` while automatic, and for a held scale nothing has been drawn
    /// on yet, which has nothing to hold but the fit it will be drawn with.
    fn held_window(&self) -> Option<(f64, f64)> {
        if self.price_auto {
            return None;
        }
        let (low, high) = self.price_base.get()?;
        let log = self.price_log && low > 0.0;
        let (on_axis, to_price): (fn(f64) -> f64, fn(f64) -> f64) = match log {
            true => (f64::ln, f64::exp),
            false => (|v| v, |v| v),
        };
        let (low, high) = (on_axis(low), on_axis(high));
        let range = (high - low).max(f64::EPSILON);
        let middle = (low + high) / 2.0 + range * self.price_offset;
        let half = range / 2.0 / self.price_zoom.clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM);
        let (low, high) = (to_price(middle - half), to_price(middle + half));
        (low.is_finite() && high.is_finite() && low < high).then_some((low, high))
    }

    /// Hold the scale at these prices, bottom and top, as
    /// [`held_window`](Self::held_window) wrote them down. Anything that is
    /// not a range of prices leaves the scale as it was.
    fn hold_window(&mut self, low: f64, high: f64) {
        if !(low.is_finite() && high.is_finite() && low < high) {
            return;
        }
        self.price_auto = false;
        self.price_zoom = 1.0;
        self.price_offset = 0.0;
        self.price_base.set(Some((low, high)));
    }

    /// The values the strip `id` shows, bottom and top: the range it is
    /// held at, or `fit`, what is on screen, while it fits itself.
    fn strip_window(&self, id: u32, fit: Option<(f64, f64)>) -> Option<(f64, f64)> {
        self.strip_held.get(&id).copied().or(fit)
    }

    /// What the strip `id` fits over the bars on screen in a chart this
    /// size: where a held one starts from, so taking hold of it never makes
    /// it jump. A volume strip stands on nought and reaches its tallest bar.
    fn strip_fit(&self, id: u32, width: f64, height: f64) -> Option<(f64, f64)> {
        let drawn = self.indicators.iter().find(|d| d.indicator.id == id)?;
        match &drawn.output {
            Output::Volume { .. } => {
                let most = frame(self, width, height)?.max_volume;
                (most > 0.0).then_some((0.0, most))
            }
            Output::Pane(pane) => {
                let (first, visible, _) = self.view();
                pane_range(pane, first, visible)
            }
            _ => None,
        }
    }

    /// Hold the strip `id` at these values, bottom and top. Anything that
    /// is not a range leaves it as it was.
    fn hold_strip(&mut self, id: u32, low: f64, high: f64) {
        if low.is_finite() && high.is_finite() && high - low > f64::EPSILON * low.abs().max(high.abs()).max(1.0) {
            self.strip_held.insert(id, (low, high));
            if let Some(drawn) = self.indicators.iter().find(|d| d.indicator.id == id) {
                self.strip_kinds.insert(id, drawn.indicator.kind);
            }
        }
    }

    /// Let go of the strips held for an indicator the chart no longer
    /// draws, or for one of another kind than now has its id.
    fn prune_strips(&mut self) {
        let now: BTreeMap<u32, indicators::Kind> = self.indicators.iter().map(|d| (d.indicator.id, d.indicator.kind)).collect();
        let kinds = &mut self.strip_kinds;
        self.strip_held.retain(|id, _| match (now.get(id), kinds.get(id)) {
            (None, _) => false,
            (Some(kind), Some(held)) => kind == held,
            // Brought back before its kind was known: it is this one's.
            (Some(kind), None) => {
                kinds.insert(*id, *kind);
                true
            }
        });
        let held = &self.strip_held;
        self.strip_kinds.retain(|id, _| held.contains_key(id));
    }

    /// Stretch or squeeze the strip `id`'s scale about its middle, taking
    /// it off fitting itself: above one shows less of it, larger.
    fn scale_strip(&mut self, id: u32, factor: f64, width: f64, height: f64) {
        let Some(from) = self.strip_window(id, self.strip_fit(id, width, height)) else { return };
        let (low, high) = scaled_window(from, factor);
        self.hold_strip(id, low, high);
    }

    /// Every strip back to fitting what is on screen.
    fn free_strips(&mut self) {
        self.strip_held.clear();
        self.strip_kinds.clear();
    }

    /// Which scale a gesture at `x`, `y` works on: a strip's when it is
    /// over that strip's row, on its axis or, with `in_plot`, anywhere in
    /// it; otherwise the price's, over the axis. `None` over the price's
    /// plot when `in_plot` is not asked for, and the time axis.
    fn axis_at(&self, x: f64, y: f64, width: f64, height: f64, in_plot: bool) -> Option<Axis> {
        let region = region_at(x, y, width, height);
        if region == Region::TimeAxis || (region == Region::Plot && !in_plot) {
            return None;
        }
        let plan = layout(self, width, height);
        match plan.rows.iter().find(|row| row.covers(y)).and_then(|row| row.pane) {
            Some(id) if self.indicators.iter().any(|d| d.indicator.id == id) => Some(Axis::Strip(id)),
            _ => (region == Region::PriceAxis).then_some(Axis::Price),
        }
    }

    /// Where a pan pressed at `y` would move a strip's scale: the strip's
    /// row it is in, with its held range if it holds one. `None` in the
    /// price's row.
    fn strip_pan_at(&self, y: f64, width: f64, height: f64) -> Option<StripPan> {
        let plan = layout(self, width, height);
        let row = plan.rows.iter().find(|row| row.covers(y))?;
        let id = row.pane?;
        let (_, inner_h) = pane_plot(row.top, row.height);
        Some(StripPan { id, held: self.strip_held.get(&id).copied(), inner_h })
    }

    /// What a pinch begun at `x`, `y` scales: the strip it is over, from
    /// what it shows, or the price's held scale over the price's axis. Nothing
    /// over the price's plot or the time axis.
    fn pinch_at(&mut self, x: f64, y: f64, width: f64, height: f64) -> Option<Pinch> {
        match self.axis_at(x, y, width, height, true)? {
            Axis::Strip(id) => {
                let from = self.strip_window(id, self.strip_fit(id, width, height))?;
                Some(Pinch::Strip { id, from })
            }
            Axis::Price => {
                self.hold_price(width, height);
                Some(Pinch::Price { zoom: self.price_zoom })
            }
        }
    }

    /// The fingers `scale` times as far apart as they started: that much
    /// larger, on whichever scale the pinch took.
    fn pinch(&mut self, pinch: Pinch, scale: f64) {
        if !(scale.is_finite() && scale > 0.0) {
            return;
        }
        match pinch {
            Pinch::Strip { id, from } => {
                let (low, high) = scaled_window(from, scale);
                self.hold_strip(id, low, high);
            }
            Pinch::Price { zoom } => self.price_zoom = (zoom * scale).clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM),
        }
    }

    /// Every vertical scale as it stands: see [`Scales`].
    fn scales(&self) -> Scales {
        Scales { price: self.price_hold(), strips: self.strip_held.clone() }
    }

    /// Shift the view by a number of bars. Positive moves forward in time.
    /// Past either end of the series the view runs into empty room, as far
    /// as `OVERHANG_PX` allows at this zoom — which needs the plot's width.
    fn pan_by(&mut self, bars: f64, plot_w: f64) {
        let columns = self.columns();
        if columns == 0 {
            return;
        }
        let len = self.bars.len() as i64;
        let room = self.max_overhang(plot_w) as i64;
        // With room kept for the profiles past the last bar, the view stops
        // at that room rather than running on into more: coming back to
        // the live edge lands with the margin as wide as it was, not
        // stretched or squeezed by how far the hand went.
        let ahead = self.profile_stop().map_or(room.max(self.margin_floor() as i64), |stop| stop as i64);
        // The left edge as a column index: negative is room before the
        // first bar.
        let was = self.left_edge();
        let left = was + bars.round() as i64;
        let left = left.clamp(-room, (len - columns as i64 + ahead).max(-room));
        // Held at the stop, nothing moved: at rest the margin stays the
        // resting margin it is, with the profiles' room still on top.
        if left == was {
            return;
        }
        let right = left + columns as i64;
        self.lead = (-left).max(0) as usize;
        self.overhang = (right - len).max(0) as usize;
        self.first = left.max(0) as usize;
        self.anchored = right >= len;
        // Moving the view is choosing where to look: whatever range it was
        // holding to, this is it now.
        self.range = None;
        self.overhang_by_hand = true;
        self.overhang_share = self.overhang as f64 / columns as f64;
        self.settle_at_stop();
    }

    /// The room past the last bar the view stops at while right-aligned
    /// profiles keep a margin there: the resting margin, with the room they
    /// take on top. None without such profiles, when the view may run on as
    /// far as `OVERHANG_PX`.
    fn profile_stop(&self) -> Option<usize> {
        let floor = self.margin_floor();
        (floor > 0).then(|| floor + RIGHT_MARGIN)
    }

    /// Back at rest once the view reaches the profiles' margin, so it is
    /// widened and narrowed with them, and stays the same share of the plot
    /// whatever the zoom.
    fn settle_at_stop(&mut self) {
        if let Some(stop) = self.profile_stop() {
            if self.anchored && self.overhang_by_hand && self.overhang >= stop {
                self.overhang = RIGHT_MARGIN;
                self.overhang_by_hand = false;
            }
        }
    }

    /// After the columns across the plot changed: room a hand left past the
    /// last bar, short of the profiles' margin, is kept as
    /// wide on screen, as the margin itself is, so zooming moves the bars
    /// and not the edge of that room.
    fn keep_room_on_screen(&mut self) {
        let now = self.columns();
        if self.anchored && self.overhang_by_hand && self.profile_stop().is_some() {
            self.overhang = (self.overhang_share * now as f64).round() as usize;
            self.settle_at_stop();
        }
    }

    /// The first bar's time, when the view has come within a screen of it
    /// and older bars have not been asked for since it was first: then
    /// they are, as of now.
    fn wants_older(&self) -> Option<i64> {
        let first = self.bars.first()?.ts;
        self.instrument.as_ref()?;
        let near = self.left_edge() < self.columns() as i64;
        (near && self.history_asked.get() != Some(first)).then(|| {
            self.history_asked.set(Some(first));
            first
        })
    }

    /// The column index of the view's left edge, negative when there is
    /// room before the first bar.
    fn left_edge(&self) -> i64 {
        let (first, _, lead) = self.view();
        first as i64 - lead as i64
    }

    /// Keep the room at either end within `OVERHANG_PX` at this zoom.
    fn trim_room(&mut self, plot_w: f64) {
        let room = self.max_overhang(plot_w);
        self.lead = self.lead.min(room);
        // At rest it is the resting margin alone, trimmed as with no
        // profiles; set by hand it may reach as far as the view stops.
        let most = match self.overhang_by_hand {
            true => room.max(self.profile_stop().unwrap_or(0)),
            false => room,
        };
        self.overhang = self.overhang.min(most);
    }

    /// The columns past the last bar the live sessions of the right-aligned
    /// volume profiles on screen take, side by side, and none when there are
    /// none: added to the resting margin at rest. A share of the columns, so
    /// it is the same share of the plot at any zoom.
    fn margin_floor(&self) -> usize {
        let share = selection::margin_share(self);
        if share <= 0.0 {
            return 0;
        }
        (self.columns() as f64 * share).ceil() as usize
    }

    /// The empty columns past the last bar, while anchored: what a hand
    /// left, or at rest the resting margin with the profiles' room on top,
    /// so the room between the last bar and the first of them is the room a
    /// chart with none has.
    fn overhang_now(&self) -> usize {
        match self.overhang_by_hand {
            true => self.overhang,
            false => self.overhang + self.margin_floor(),
        }
    }

    /// How many empty columns `OVERHANG_PX` is at this zoom — short of the
    /// whole view, so a bar always stays on screen.
    fn max_overhang(&self, plot_w: f64) -> usize {
        let columns = self.columns();
        if columns == 0 {
            return 0;
        }
        let bar_w = plot_w / columns as f64;
        ((OVERHANG_PX / bar_w).floor() as usize).min(columns - 1)
    }

    /// Columns across the plot: the bars in view and the empty ones past
    /// the last. What a bar's width is measured against.
    fn columns(&self) -> usize {
        if self.bars.is_empty() {
            return 0;
        }
        self.visible.clamp(MIN_VISIBLE, MAX_VISIBLE).min(self.span())
    }

    /// The most columns the view spreads across: the series and the resting
    /// margin past it, so a short series still opens with room after its
    /// last bar instead of losing bars to it.
    fn span(&self) -> usize {
        self.bars.len() + RIGHT_MARGIN
    }

    /// Zoom the time axis about `anchor`, a fraction across the plot. On
    /// the columns across the plot, not the bars in view: with room open
    /// past an end those differ, and zooming on the smaller count would
    /// shrink the view a step at a time.
    fn zoom_time(&mut self, factor: f64, anchor: f64, plot_w: f64) {
        let columns = self.columns();
        let next = ((columns as f64 * factor).round() as usize)
            .clamp(MIN_VISIBLE, MAX_VISIBLE)
            .min(self.span().max(MIN_VISIBLE));
        // Anchored to the right edge, zooming reveals history and the last bar
        // stays put — which is what you want when looking at the live edge.
        if !self.anchored {
            let focus = self.left_edge() as f64 + anchor * columns as f64;
            let left = (focus - anchor * next as f64).round() as i64;
            self.lead = (-left).max(0) as usize;
            self.first = left.max(0) as usize;
        }
        self.visible = next;
        self.range = None;
        self.keep_room_on_screen();
        // The room is so many pixels, which is fewer columns zoomed in.
        self.trim_room(plot_w);
    }

    /// Stretch or compress the price scale, taking it off automatic.
    fn scale_price(&mut self, factor: f64, width: f64, height: f64) {
        self.hold_price(width, height);
        self.price_zoom = (self.price_zoom * factor).clamp(MIN_PRICE_ZOOM, MAX_PRICE_ZOOM);
    }

    /// Take the price scale off automatic, holding exactly what a chart of
    /// this size shows now, so nothing jumps. A scale already held is left
    /// as it is.
    fn hold_price(&mut self, width: f64, height: f64) {
        if !self.price_auto {
            return;
        }
        self.price_auto = false;
        self.price_zoom = 1.0;
        self.price_offset = 0.0;
        self.price_base.set(None);
        // Too small to have been drawn — a chart restored before it has a
        // size — and the first frame holds what it fits instead.
        if width <= PRICE_AXIS_W + PAD || height <= TIME_AXIS_H + PAD {
            return;
        }
        let (first, visible) = self.slice();
        if visible == 0 {
            return;
        }
        // A held scale with nothing held yet keeps the fit it is asked
        // about, which is what the chart was showing a moment ago.
        let plan = layout(self, width, height);
        let _ = price_range(self, &self.bars[first..first + visible], &plan);
    }

    /// The bought and sold colours a delta profile or a cumulative delta is
    /// painted in: its own colouring's, or the house colouring's when it has
    /// none, or one that is not offered to a delta.
    fn delta_ink(&self, drawn: &Drawn) -> (&str, &str) {
        let chosen = match &drawn.indicator.params {
            omacharts_engine::Params::VolumeProfile { colouring, .. }
            | omacharts_engine::Params::Cvd { colouring, .. } => *colouring,
            _ => None,
        };
        let at = chosen.and_then(|c| DELTA_COLOURINGS.iter().position(|o| *o == c)).map_or(0, |at| at + 1);
        let (up, down) = &self.delta_inks[at];
        (up, down)
    }

    /// Whether the price scale is automatic, and the zoom and shift of a
    /// held one: what a gesture is compared against to tell whether it
    /// changed anything worth writing down.
    fn price_hold(&self) -> (bool, f64, f64) {
        (self.price_auto, self.price_zoom, self.price_offset)
    }

    /// Put the price scale back to fitting the bars on screen.
    fn free_price(&mut self) {
        self.price_auto = true;
        self.price_zoom = 1.0;
        self.price_offset = 0.0;
        self.price_base.set(None);
    }

    /// Back to fitting the data, at the live edge, with the resting margin
    /// past it: the price and every strip.
    fn reset_view(&mut self) {
        self.range = None;
        self.free_price();
        self.free_strips();
        self.visible = 160;
        self.anchored = true;
        self.overhang = RIGHT_MARGIN;
        self.overhang_by_hand = false;
        self.lead = 0;
    }

    /// Take on a series, starting the view over when the instrument or the
    /// resolution changed. A range it is holding to is laid across the new
    /// series rather than let go of.
    /// Says whether the instrument or the resolution changed.
    fn replace_series(&mut self, instrument: Instrument, timeframe: Timeframe, bars: Vec<Bar>) -> bool {
        let changed = self
            .instrument
            .as_ref()
            .map(|i| i.symbol != instrument.symbol || i.suffix != instrument.suffix)
            .unwrap_or(true)
            || self.timeframe != timeframe;
        let first = self.instrument.is_none();
        // Older bars come in before the ones shown: the view stays on them.
        let before = match (self.bars.first(), changed || self.anchored) {
            (Some(was), false) => bars.partition_point(|bar| bar.ts < was.ts),
            _ => 0,
        };
        if before > 0 {
            let left = self.first as i64 - self.lead as i64 + before as i64;
            self.lead = (-left).max(0) as usize;
            self.first = left.max(0) as usize;
        }
        self.bars = bars;
        self.instrument = Some(instrument);
        self.timeframe = timeframe;
        if let Some(compare) = self.compare.as_mut() {
            compare.realign(&self.bars, timeframe);
        }
        if changed {
            // A measured range belongs to the series it was measured on.
            self.ruler = None;
            self.ruler_armed = false;
            self.anchored = true;
            self.overhang = RIGHT_MARGIN;
            self.overhang_by_hand = false;
            self.lead = 0;
            self.visible = 160;
            // A held range is in this series' prices, which another
            // symbol's need not be anywhere near: it holds its own fit,
            // zoomed and shifted as this one was. The first series a chart
            // gets is not another symbol's: a range it holds already was
            // brought back for it, from before a restart.
            if !first {
                self.price_base.set(None);
                // A strip's range is in values of this series, which
                // another symbol's or timeframe's are nowhere near: each
                // fits itself again.
                self.free_strips();
            }
        }
        self.fit_range();
        changed
    }

    /// Show `range`, and keep showing it as the series changes.
    fn show_range(&mut self, range: Range) {
        self.range = Some(range);
        self.fit_range();
    }

    /// Lay the view across the range it holds, if it holds one: from the
    /// first bar on or after the range's first day to the live edge, with
    /// the resting margin past it, or to its last day's bar for one that
    /// ends before then. "Today" is the last bar's day, on the same clock
    /// the time axis reads.
    fn fit_range(&mut self) {
        let Some(range) = self.range else { return };
        let Some(last) = self.bars.last() else { return };
        let utc = self.timeframe.is_calendar();
        let clock = self.clock;
        let date = |ts: i64| clock.read(ts, utc).map(|at| at.date());
        let Some(today) = date(last.ts) else { return };
        let len = self.bars.len();
        // A day on its own shows the length of time that was on screen, from
        // the start of that day or up to now, into empty room past the last
        // bar when it gets there, counted as the time axis counts it. A
        // month shows the whole of itself the same way, so this month runs
        // to its end.
        let window = match (range, range.start(today), range.end(today)) {
            (Range::Lasting(..), ..) => clock
                .read(self.timeframe.shift(last.ts, RIGHT_MARGIN as i64), utc)
                .and_then(|edge| range.lasting_on(today, edge)),
            (Range::Month(..), Some(start), Some(end)) => {
                Some((start.and_time(chrono::NaiveTime::MIN), (end - start).num_days() * 86_400 + 86_399))
            }
            _ => None,
        };
        if let Some((from, seconds)) = window {
            let (first, columns, room) = omacharts_engine::range::lasting_window(
                &self.bars,
                |bar| clock.read(bar.ts, utc),
                from,
                seconds,
                self.timeframe.seconds(),
            );
            self.lead = 0;
            self.visible = columns.clamp(MIN_VISIBLE, MAX_VISIBLE);
            self.anchored = room > 0;
            self.overhang = room;
            self.overhang_by_hand = true;
            self.overhang_share = room as f64 / self.visible.max(1) as f64;
            self.first = first.min(len.saturating_sub(1));
            return;
        }
        let first = match range.start(today) {
            None => 0,
            Some(start) => self.bars.partition_point(|bar| date(bar.ts).is_some_and(|d| d < start)),
        };
        let first = first.min(len);
        let end = match range.end(today) {
            None => len,
            Some(end) => self.bars.partition_point(|bar| date(bar.ts).is_some_and(|d| d <= end)),
        };
        self.lead = 0;
        if end >= len {
            // Wide enough that the margin, widened for the profiles beside
            // the live session, still leaves the range's first bar on screen.
            let share = selection::margin_share(self).min(MARGIN_MAX_SHARE_OF_VIEW);
            let wanted = ((len - first + RIGHT_MARGIN) as f64 / (1.0 - share)).ceil() as usize;
            self.visible = wanted.clamp(MIN_VISIBLE, MAX_VISIBLE);
            self.anchored = true;
            self.overhang = RIGHT_MARGIN;
            self.overhang_by_hand = false;
            return;
        }
        // Over before the last bar: its own bars edge to edge, from the
        // first of them, and new bars arriving move nothing.
        self.visible = end.saturating_sub(first).clamp(MIN_VISIBLE, MAX_VISIBLE);
        self.anchored = false;
        self.overhang = 0;
        self.overhang_by_hand = true;
        self.first = first.min(len.saturating_sub(self.visible));
    }

    /// Whether the chart's own bars are drawn as a line through the closes:
    /// the Line style, or a comparison drawing every market as a line.
    fn drawn_as_line(&self) -> bool {
        !self.bars_hidden
            && (self.bar_style == BarStyle::Line || self.compare.as_ref().is_some_and(|overlay| overlay.spec.lines_only))
    }

    /// The bars in view: the first, and how many. Fewer than the columns
    /// across the plot when the view hangs past either end.
    fn slice(&self) -> (usize, usize) {
        let (first, visible, _) = self.view();
        (first, visible)
    }

    /// The view: the first bar in it, how many bars, and how many empty
    /// columns stand before the first. The room at either end is capped at
    /// one column short of the view, so a bar always stays on screen.
    fn view(&self) -> (usize, usize, usize) {
        let columns = self.columns();
        if columns == 0 {
            return (0, 0, 0);
        }
        let len = self.bars.len() as i64;
        let left = if self.anchored {
            len + self.overhang_now().min(columns - 1) as i64 - columns as i64
        } else {
            self.first as i64 - self.lead.min(columns - 1) as i64
        };
        let left = left.min(len - 1);
        let first = left.max(0);
        let right = (left + columns as i64).min(len);
        (first as usize, (right - first).max(0) as usize, (-left).max(0) as usize)
    }
}

/// Something the window hangs off the chart after building it. Optional
/// because the chart is constructed before there is a window to tell.
type Handler<F> = Rc<RefCell<Option<Box<F>>>>;

/// What a chart is hiding all of at once, each by its own key, for the
/// legend to name while it is hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hidden {
    pub drawings: bool,
    pub indicators: bool,
    pub bars: bool,
}

/// How far a pasted copy lands from what it was copied from, in pixels.
///
/// Enough to see that there are two of them and to take hold of the one on
/// top; little enough that it is plainly the same drawing moved rather than
/// one somewhere else.
const PASTE_OFFSET: f64 = 16.0;

/// How far the hand has to travel before a press counts as a drag.
///
/// The desktop's own number — the one it uses to tell a click from the start
/// of a drag everywhere else — rather than one of ours. It matters here
/// because a rectangle can be laid down two ways: press-drag-release, or
/// click, move, click. A press that travels is the first; a press that does
/// not is the first half of the second. Four pixels, which this used to be,
/// is less than a deliberate click moves on a sensitive mouse — so a click
/// meant as "this corner" was read as a whole drag and committed a
/// rectangle a few pixels across.
///
/// Never under eight, which is GTK's own default. A desktop tuned lower is
/// tuned for dragging things that have nowhere else to go; here the press
/// has a second meaning waiting for it, and reading it wrong costs a
/// drawing.
fn drag_threshold() -> f64 {
    gtk::Settings::default().map(|s| s.gtk_dnd_drag_threshold()).unwrap_or(8).max(8) as f64
}

/// Where a drawing made on a chart goes: to the symbol when the chart sends
/// what is drawn on it, and no further than the chart when it does not.
fn scope_for(sends: bool) -> Scope {
    match sends {
        true => Scope::Shared,
        false => Scope::Local,
    }
}

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
    /// The editor open on the chart, if any.
    ///
    /// It hangs straight off [`root`], placed by its margins, and is there
    /// only while somebody is typing. A layer of its own over the whole
    /// chart was the first try and the wrong one: to let the editor inside
    /// it take a click the layer has to be targetable, and a targetable
    /// layer the size of the chart takes every click there is.
    ///
    /// [`root`]: ChartView::root
    editing: RefCell<Option<Rc<text_editor::Editing>>>,
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
    /// Told what the hand did to the marks.
    on_mark: Handler<dyn Fn(MarkEvent)>,
    /// Right-clicking a drawing, which has a menu of its own: about the
    /// drawing and nothing else.
    on_drawing_menu: Handler<dyn Fn(f64, f64)>,
    /// Enter on a selected drawing: its properties.
    on_drawing_properties: Handler<dyn Fn()>,
    /// The selected indicator: picked, asked about, or removed.
    on_indicator: Handler<dyn Fn(IndicatorEvent)>,
    /// A caret went into a label, or came out of one. The window lends the
    /// editor the chords it needs while this is true.
    on_typing: Handler<dyn Fn(bool)>,
    /// The tool in hand, or the configuration it will draw with, changed —
    /// so whatever shows the tool can show it.
    on_tool: Handler<dyn Fn()>,
    /// Told when a gesture on the chart takes the price scale off automatic
    /// or puts it back — a drag or a wheel on the axis, a double-click to
    /// reset it. Whether the scale is automatic is written down with the
    /// chart, and these are the only ways it changes that nothing outside
    /// the chart can see.
    on_price_auto: Handler<dyn Fn(bool)>,
    /// Told when the drawings, the indicators or the bars are hidden or
    /// shown, whatever did it, for the legend's glyphs.
    on_hidden: Handler<dyn Fn(Hidden)>,
    /// Told the first bar's time when the view comes within a screen of
    /// it, once for each first bar: see [`ChartView::set_history_handler`].
    on_history: Handler<dyn Fn(i64)>,
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
            editing: RefCell::new(None),
            root,
            state,
            on_hover,
            on_context_menu: Rc::new(RefCell::new(None)),
            on_pane_resize: Rc::new(RefCell::new(None)),
            on_pane_close: Rc::new(RefCell::new(None)),
            on_pane_move: Rc::new(RefCell::new(None)),
            on_axis_menu: Rc::new(RefCell::new(None)),
            on_drawing: Rc::new(RefCell::new(None)),
            on_mark: Rc::new(RefCell::new(None)),
            on_drawing_menu: Rc::new(RefCell::new(None)),
            on_drawing_properties: Rc::new(RefCell::new(None)),
            on_indicator: Rc::new(RefCell::new(None)),
            on_typing: Rc::new(RefCell::new(None)),
            on_tool: Rc::new(RefCell::new(None)),
            on_price_auto: Rc::new(RefCell::new(None)),
            on_hidden: Rc::new(RefCell::new(None)),
            on_history: Rc::new(RefCell::new(None)),
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

    /// Who hears what the hand does to the marks: a ×, a mark let go at
    /// another price, a tab dragged out.
    pub fn set_mark_handler(&self, handler: impl Fn(MarkEvent) + 'static) {
        *self.on_mark.borrow_mut() = Some(Box::new(handler));
    }

    /// Say `event` to whoever hears the marks, as a hand on them would
    /// have: for tests, which have no hand.
    pub fn tell_mark(&self, event: MarkEvent) {
        if let Some(handler) = self.on_mark.borrow().as_ref() {
            handler(event);
        }
    }

    pub fn set_drawing_menu_handler(&self, handler: impl Fn(f64, f64) + 'static) {
        *self.on_drawing_menu.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_drawing_properties_handler(&self, handler: impl Fn() + 'static) {
        *self.on_drawing_properties.borrow_mut() = Some(Box::new(handler));
    }

    // -- indicators --------------------------------------------------------

    pub fn set_indicator_handler(&self, handler: impl Fn(IndicatorEvent) + 'static) {
        *self.on_indicator.borrow_mut() = Some(Box::new(handler));
    }

    fn tell_indicator(&self, event: IndicatorEvent) {
        if let Some(handler) = self.on_indicator.borrow().as_ref() {
            handler(event);
        }
    }

    /// The indicator selected, if any.
    pub fn picked(&self) -> Option<u32> {
        self.state.borrow().picked
    }

    /// Select an indicator, or none. Taking one lets go of every drawing:
    /// one thing is selected at a time, and the keys act on that one.
    pub fn pick(&self, id: Option<u32>) {
        let id = id.filter(|id| {
            self.state.borrow().shown_indicators().any(|d| d.indicator.id == *id && d.indicator.kind.selectable())
        });
        let changed = {
            let mut s = self.state.borrow_mut();
            let changed = s.picked != id;
            // Only when the selection moves onto an indicator: the same one
            // checked again, as it is whenever the indicators are redrawn,
            // leaves the drawings be.
            let drawings = changed && id.is_some() && !s.selected.is_empty();
            if drawings {
                s.selected.clear();
            }
            s.picked = id;
            changed || drawings
        };
        if changed {
            self.redraw();
            self.tell_indicator(IndicatorEvent::Picked(id));
        }
    }

    /// The profile of indicator `id` whose period is under `x`.
    pub fn profile_at(&self, id: u32, x: f64) -> Option<selection::ProfileSpot> {
        let s = self.state.borrow();
        s.profile_at(self.area.width() as f64, self.area.height() as f64, id, x)
    }

    /// Every profile indicator `id` draws, and whether each is on screen,
    /// for a command that reads them.
    pub fn profiles(&self, id: u32) -> Option<Vec<(Profile, bool)>> {
        let s = self.state.borrow();
        let drawn = s.indicators.iter().find(|d| d.indicator.id == id)?;
        let Output::Profiles(profiles) = &drawn.output else { return None };
        let (first, count, _) = s.view();
        let end = first + count;
        // Begun at its first bar rather than at its period's start, which for
        // a session is midnight UTC, the evening before in New York; the
        // periods keep their starts.
        Some(
            profiles
                .iter()
                .map(|p| {
                    let mut p = p.clone();
                    if let Some(bar) = s.bars.get(p.first_bar) {
                        p.start_ts = bar.ts;
                    }
                    let on = p.last_bar >= first && p.first_bar < end;
                    (p, on)
                })
                .collect(),
        )
    }

    /// The profile of indicator `id` holding the moment `ts`.
    pub fn profile_of(&self, id: u32, ts: i64) -> Option<selection::ProfileSpot> {
        self.state.borrow().profile_of(id, ts)
    }

    /// The indicator under a pixel that a click there would select.
    fn indicator_at(&self, x: f64, y: f64) -> Option<u32> {
        let s = self.state.borrow();
        if s.tool.is_some() || s.placing.is_some() {
            return None;
        }
        s.indicator_at(self.area.width() as f64, self.area.height() as f64, x, y)
    }

    pub fn set_typing_handler(&self, handler: impl Fn(bool) + 'static) {
        *self.on_typing.borrow_mut() = Some(Box::new(handler));
    }

    fn tell_typing(&self, typing: bool) {
        if let Some(handler) = self.on_typing.borrow().as_ref() {
            handler(typing);
        }
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
        // An open editor is pointed at a position in the list, and the list
        // is about to be a different list: follow the drawing it is on by
        // id, and close the editor if that drawing is not in the new list at
        // all. Without this a reload while somebody is typing would leave
        // the caret on whatever had moved into that position.
        let editing_id = self
            .editing
            .borrow()
            .as_ref()
            .and_then(|editing| self.state.borrow().drawings.get(editing.index.get()).map(|d| d.id));
        let still_there = {
            let mut state = self.state.borrow_mut();
            let ids = state.selected_ids();
            state.drawings = drawings;
            state.reselect(ids);
            match editing_id {
                // Zero is the drawing that has just been added and not yet
                // written down, which comes back as the newest row — the
                // same convention the selection follows, for the same
                // reason: there is no id to find it by yet.
                Some(0) => {
                    let now = state.drawings.len().checked_sub(1);
                    state.editing = now;
                    now
                }
                Some(id) => {
                    let now = state.drawings.iter().position(|d| d.id == id);
                    state.editing = now;
                    now
                }
                None => None,
            }
        };
        match (editing_id.is_some(), still_there) {
            // It moved: the editor keeps the caret, pointed at the new place.
            (true, Some(index)) => {
                if let Some(editing) = self.editing.borrow().as_ref() {
                    editing.index.set(index);
                }
                self.place_editor();
            }
            // It went: nothing to type into.
            (true, None) => self.close_editor(),
            (false, _) => {}
        }
        self.redraw();
    }

    /// Take the editor off the chart without reading it back: for when the
    /// drawing it was on is no longer there to read it into.
    fn close_editor(&self) {
        let Some(editing) = self.editing.borrow_mut().take() else { return };
        self.root.remove_overlay(&editing.view);
        {
            let mut state = self.state.borrow_mut();
            state.editing = None;
            state.editing_at = None;
        }
        self.tell_typing(false);
        self.area.grab_focus();
    }

    pub fn drawings(&self) -> Vec<Drawing> {
        self.state.borrow().drawings.clone()
    }

    /// Arm a tool: the next press on the plot starts a drawing of this kind.
    /// `None` is the pointer, which Escape goes back to.
    pub fn arm(&self, kind: Option<DrawingKind>) {
        self.arm_sticky(kind, false);
    }

    /// Arm a tool, and say whether it stays in hand after each drawing.
    pub fn arm_sticky(&self, kind: Option<DrawingKind>, sticky: bool) {
        {
            let mut state = self.state.borrow_mut();
            state.sticky = sticky && kind.is_some();
            // Putting the tool down abandons the choice; picking one up
            // keeps it, whichever order the two were asked for in. The
            // choice belongs to the next drawing, not to the tool that
            // happens to be in hand when it is made — and it is cleared
            // where that drawing is committed.
            if kind.is_none() {
                state.next_config = 1;
            }
            state.ruler_armed = false;
            state.ruler = None;
            state.tool = kind;
            state.placing = None;
            if kind.is_some() {
                state.selected.clear();
                // A tool picked up is a drawing about to be made, and one
                // made out of sight would read as one that never was.
                state.show_drawings = true;
            }
        }
        self.hidden_changed();
        // A tool in hand is about to make a drawing, which is selected once
        // it is down: whatever indicator was selected stops being.
        if kind.is_some() {
            self.pick(None);
        }
        self.area.set_cursor_from_name(Some(if kind.is_some() { "crosshair" } else { "default" }));
        self.redraw();
        self.tool_changed();
    }

    /// Pass the tool in hand to `to`, with whether it stays there and the
    /// configuration chosen for the next drawing, and go back to the
    /// pointer. The tool belongs to the window, not to one chart: picking
    /// the line and then clicking into the chart beside draws a line there.
    pub fn hand_over(&self, to: &ChartView) {
        let (kind, sticky, next, ruler) = {
            let state = self.state.borrow();
            (state.tool, state.sticky, state.next_config, state.ruler_armed)
        };
        if ruler {
            self.arm(None);
            to.arm_ruler();
            to.tool_changed();
            return;
        }
        if self.armed().is_some() {
            self.arm(None);
        }
        to.arm_sticky(kind, sticky);
        to.state.borrow_mut().next_config = next;
        to.tool_changed();
    }

    pub fn arm_ruler(&self) {
        self.arm(None);
        {
            let mut state = self.state.borrow_mut();
            state.ruler_armed = true;
            state.selected.clear();
            state.drag = None;
        }
        self.area.set_cursor_from_name(Some("crosshair"));
        self.pointer.queue_draw();
    }

    pub fn ruler_armed(&self) -> bool {
        self.state.borrow().ruler_armed
    }

    pub fn measure(
        &self,
        anchors: Option<(Anchor, Anchor)>,
    ) -> Option<omacharts_engine::measurement::Measurement> {
        if anchors.is_some() {
            self.arm(None);
        }
        let mut state = self.state.borrow_mut();
        state.ruler_armed = false;
        state.ruler = anchors.map(|(a, b)| (a, b, true));
        let kind = state.instrument.as_ref().map(|i| i.kind);
        let measured = anchors
            .map(|(a, b)| omacharts_engine::measurement::Measurement::between(a, b, &state.bars, kind));
        let drawing_armed = state.tool.is_some();
        drop(state);
        self.area.set_cursor_from_name(Some(if drawing_armed { "crosshair" } else { "default" }));
        self.pointer.queue_draw();
        self.tool_changed();
        measured
    }

    pub fn armed(&self) -> Option<DrawingKind> {
        self.state.borrow().tool
    }

    /// Whether the tool in hand stays there.
    pub fn is_sticky(&self) -> bool {
        self.state.borrow().sticky
    }

    /// Escape: back to the pointer, with nothing in hand and nothing
    /// selected. A drawing half laid down — even one mid-drag — is dropped
    /// rather than finished. Says whether there was anything to drop, so a
    /// key that found nothing can go on to whoever else wants it.
    pub fn cancel(&self) -> bool {
        // Typing is the thing Escape ends first, and the editor's own key
        // handler normally gets there before this does; arriving here with
        // an editor open means Escape came from somewhere else.
        if self.is_typing_text() {
            self.end_text_edit(text_editor::Ended::Cancel);
            return true;
        }
        let busy = {
            let mut s = self.state.borrow_mut();
            let busy = s.ruler_armed
                || s.ruler.is_some()
                || s.tool.is_some()
                || s.placing.is_some()
                || !s.selected.is_empty();
            s.selected.clear();
            // The drag the gesture is still in ends with nothing to commit.
            if matches!(s.drag, Some(Drag::Place { .. })) {
                s.drag = None;
            }
            // A mark being dragged goes back where it was, unsaid.
            let dragging = matches!(s.drag, Some(Drag::Mark { .. }));
            if dragging {
                s.drag = None;
                self.pointer.queue_draw();
            }
            busy || dragging || s.picked.is_some()
        };
        self.pick(None);
        self.arm(None);
        busy
    }

    /// Drop a drawing half laid down, keeping the tool that was laying it
    /// down in hand. Says whether there was one to drop.
    fn drop_placing(&self) -> bool {
        {
            let mut s = self.state.borrow_mut();
            if s.placing.take().is_none() {
                return false;
            }
            // The drag the gesture is still in ends with nothing to commit.
            if matches!(s.drag, Some(Drag::Place { .. })) {
                s.drag = None;
            }
        }
        // The drawing in hand is on the pointer layer.
        self.pointer.queue_draw();
        true
    }

    /// The drawing most recently taken into the selection, if any has the
    /// grips: the one a single-drawing question is about.
    pub fn selected_drawing(&self) -> Option<Drawing> {
        let state = self.state.borrow();
        state.primary().and_then(|i| state.drawings.get(i)).cloned()
    }

    /// Ctrl+A: select every drawing on the plot. Says whether there was
    /// anything to select, so an empty chart lets the key go on.
    pub fn select_visible(&self) -> bool {
        let (w, h) = (self.area.width() as f64, self.area.height() as f64);
        let mut state = self.state.borrow_mut();
        let visible = state.visible_drawings(w, h);
        if visible.is_empty() {
            return false;
        }
        state.selected = visible;
        drop(state);
        self.pick(None);
        self.redraw();
        true
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
        if hit.is_some() {
            self.pick(None);
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
            // Hidden, there is nothing on screen to delete: the drawings
            // are kept for when they come back.
            if state.drawings.is_empty() || !state.show_drawings {
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
            // A step nobody could see taken is not one to take.
            if !state.show_drawings {
                return false;
            }
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
        use gtk::gdk::ModifierType;
        fn ctrl(m: ModifierType) -> bool {
            m.contains(ModifierType::CONTROL_MASK)
        }
        fn shift(m: ModifierType) -> bool {
            m.contains(ModifierType::SHIFT_MASK)
        }
        fn alt(m: ModifierType) -> bool {
            m.contains(ModifierType::ALT_MASK)
        }

        let keys = gtk::EventControllerKey::new();
        let view = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let Some(view) = view.upgrade() else { return glib::Propagation::Proceed };
            use gtk::gdk::Key;
            match key {
                // Ctrl alone says nothing to anybody else, so it goes on;
                // here it is the magnet, and the crosshair and a drawing
                // half laid down move onto the candle without waiting for
                // the hand to.
                Key::Control_L | Key::Control_R => {
                    view.set_magnet(true);
                    glib::Propagation::Proceed
                }
                Key::Escape => match view.cancel() {
                    true => glib::Propagation::Stop,
                    false => glib::Propagation::Proceed,
                },
                Key::Delete | Key::BackSpace | Key::KP_Delete => {
                    if let Some(id) = view.picked() {
                        view.tell_indicator(IndicatorEvent::Remove(id));
                        return glib::Propagation::Stop;
                    }
                    if view.state.borrow().selected.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    view.delete_selected();
                    glib::Propagation::Stop
                }
                // Alt and Enter opens the properties, which is what it opens
                // on a selected object in Explorer, Visual Studio, Visio and
                // AutoCAD. Enter acts on the thing; Alt+Enter asks about it.
                Key::Return | Key::KP_Enter | Key::ISO_Enter if alt(modifiers) => {
                    if let Some(id) = view.picked() {
                        view.tell_indicator(IndicatorEvent::Properties(id));
                        return glib::Propagation::Stop;
                    }
                    if view.state.borrow().selected.is_empty() {
                        return glib::Propagation::Proceed;
                    }
                    if let Some(handler) = view.on_drawing_properties.borrow().as_ref() {
                        handler();
                    }
                    glib::Propagation::Stop
                }
                // Enter and F2 both put the caret in the drawing's text, and
                // they do it because that is what they do in PowerPoint,
                // Keynote, Google Slides, draw.io, LibreOffice and Figma.
                // The pointer's way in is a double-click. Enter used to open
                // the properties here, which left Enter and F2 meaning two
                // different things — the one arrangement none of those apps
                // has.
                // Enter ends a zig-zag, which is the keyboard's way of
                // saying "that is the last corner". Tried before the text
                // keys, since while one is being laid down that is what the
                // key is for.
                Key::Return | Key::KP_Enter | Key::ISO_Enter
                    if view.state.borrow().placing.as_ref().is_some_and(|d| d.kind.is_path()) =>
                {
                    view.commit_placing();
                    glib::Propagation::Stop
                }
                Key::F2 | Key::Return | Key::KP_Enter | Key::ISO_Enter => {
                    match view.edit_selected_text() {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                Key::a | Key::A
                    if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK)
                        && !modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) =>
                {
                    match view.select_visible() {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
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
                // Alt+Shift and an arrow: put the selected drawings' labels
                // against that edge of the shape, and Alt+Shift+. puts them
                // back in the middle. Four edges and a centre, not nine
                // places: the corners are a thing to aim at with the picture
                // in the properties, and a key for each would need two
                // arrows pressed together to mean one of them.
                //
                // Before the nudge arm below, which otherwise takes any
                // arrow on a selected drawing. It only claims the key when
                // there is a label to move — on a figure with nothing
                // written on it the arrows go on nudging, rather than the
                // chord quietly doing nothing.
                Key::Left | Key::Right | Key::Up | Key::Down
                    if alt(modifiers) && shift(modifiers) && !ctrl(modifiers) =>
                {
                    let at = match key {
                        Key::Left => drawings::Place::Left,
                        Key::Right => drawings::Place::Right,
                        Key::Up => drawings::Place::Top,
                        _ => drawings::Place::Bottom,
                    };
                    match view.place_text(at) {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                // Alt+Shift and the size keys: the stroke of whatever is
                // selected, a drawing's or an indicator's. Ctrl with the
                // same keys is the text's size, and bare they are the
                // chart's zoom — three things that all mean "bigger", kept
                // apart by which modifier is held and what is selected.
                // Ctrl+Shift would be no fourth: Shift is how + is typed on
                // the main row, so it is Ctrl and + already.
                Key::plus | Key::equal | Key::KP_Add | Key::minus | Key::underscore | Key::KP_Subtract
                    if alt(modifiers) && shift(modifiers) && !ctrl(modifiers) =>
                {
                    let thicker = matches!(key, Key::plus | Key::equal | Key::KP_Add);
                    if let Some(id) = view.picked() {
                        view.tell_indicator(IndicatorEvent::StepWidth(id, thicker));
                        return glib::Propagation::Stop;
                    }
                    let by = if thicker { drawings::WIDTH_STEP } else { -drawings::WIDTH_STEP };
                    match view.step_width(by) {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                // Alt+Shift+A steps the arrowhead round its four states.
                // The same hand that holds Alt+Shift to place a label, on
                // the letter the arrow tool already answers to.
                Key::a | Key::A if alt(modifiers) && shift(modifiers) && !ctrl(modifiers) => {
                    match view.cycle_head() {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                // The full stop, wherever the layout hides it: Shift makes it
                // a colon on a Spanish keyboard and a greater-than on a US
                // one, and the chord is the same chord on both.
                Key::period | Key::greater | Key::colon | Key::KP_Decimal
                    if alt(modifiers) && shift(modifiers) && !ctrl(modifiers) =>
                {
                    match view.place_text(drawings::Place::Center) {
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
        let view = Rc::downgrade(self);
        keys.connect_key_released(move |_, key, _, _| {
            use gtk::gdk::Key;
            if let (Key::Control_L | Key::Control_R, Some(view)) = (key, view.upgrade()) {
                view.set_magnet(false);
            }
        });
        self.area.add_controller(keys);
    }

    /// Ctrl went down or came up: snap, or stop snapping, where the pointer
    /// already is.
    fn set_magnet(&self, on: bool) {
        {
            let mut s = self.state.borrow_mut();
            if s.magnet == on {
                return;
            }
            s.magnet = on;
            s.follow_hand(self.area.width() as f64, self.area.height() as f64);
        }
        self.pointer.queue_draw();
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
        // Nothing in hand and nothing selected: the choice is for whatever
        // is drawn next. Alt+3 then Alt+L is the same sentence as Alt+L
        // then Alt+3, and used to be two — the first half said nothing at
        // all, and arming a tool then threw the choice away.
        self.state.borrow_mut().next_config = n;
        self.tool_changed();
        true
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
    pub fn set_sends(&self, sends: bool) {
        self.state.borrow_mut().sends = sends;
    }

    pub fn sends(&self) -> bool {
        self.state.borrow().sends
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
            let Some(scale) = price_range(&state, &state.bars[first..first + visible], &plan) else {
                return;
            };
            let bar_w = plan.plot_w / state.columns().max(1) as f64;
            let by_price = -dy / plan.price_h * scale.span();
            state.remember();
            let mut moved = Vec::new();
            // By columns, like the drag: a pixel is a fraction of a bar,
            // and bars are evenly spaced on screen however unevenly they
            // are spaced in time. Nudging by a span of time instead would
            // change the shape of anything whose ends sit either side of a
            // weekend.
            let by_index = dx / bar_w;
            let selected = state.selected.clone();
            let State { bars, drawings, .. } = &mut *state;
            for index in selected {
                let Some(drawing) = drawings.get_mut(index) else { continue };
                shift_by_columns(bars, drawing, by_index, &scale, by_price);
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
            let Some(mut drawing) = state.placing.take() else { return };
            // The last corner of a path was only ever following the hand.
            // Dropping it can leave too little to be a drawing — one press
            // and Enter — and then there is nothing to commit.
            if !drawing.finish_path() {
                if !state.sticky {
                    state.tool = None;
                }
                state.next_config = 1;
                drop(state);
                self.area.set_cursor_from_name(Some("default"));
                self.redraw();
                self.tool_changed();
                return;
            }
            state.remember();
            state.drawings.push(drawing.clone());
            state.selected = vec![state.drawings.len() - 1];
            if !state.sticky {
                state.tool = None;
            }
            state.next_config = 1;
            drawing
        };
        self.area.set_cursor_from_name(Some(match self.armed().is_some() {
            true => "crosshair",
            false => "default",
        }));
        self.pick(None);
        self.redraw();
        self.tool_changed();
        self.tell(DrawingEvent::Added(added));
    }

    // -- typing on the chart -----------------------------------------------

    /// A press with the text tool in hand: a word at that point, with the
    /// caret already in it.
    ///
    /// The drawing goes into the list and is told about straight away, so it
    /// is a real drawing with a real id while it is being typed — undo,
    /// sharing and the store all see the same thing they see for any other
    /// drawing. What is special is only that it starts out saying nothing,
    /// and that saying nothing at the end means it goes away again.
    fn start_text_at(self: &Rc<Self>, at: Anchor) {
        let (index, added) = {
            let mut state = self.state.borrow_mut();
            let mut drawing = Drawing::text_at(at);
            drawing.follow(state.next_config);
            drawing.scope = scope_for(state.sends);
            drawing.order = state.drawings.iter().map(|d| d.order).max().unwrap_or(0);
            state.remember();
            state.drawings.push(drawing.clone());
            state.selected = vec![state.drawings.len() - 1];
            if !state.sticky {
                state.tool = None;
            }
            state.next_config = 1;
            (state.drawings.len() - 1, drawing)
        };
        self.area.set_cursor_from_name(Some("default"));
        self.tool_changed();
        // Announced before a character is typed, the way a line is announced
        // the moment it is laid down. Whoever keeps the drawings writes it
        // and it gets an id; without that it is a drawing with id zero, and
        // the change that carries the words is an update to a row that was
        // never written — which the store quietly drops, so the words would
        // appear under the caret and vanish on Enter.
        self.tell(DrawingEvent::Added(added));
        self.begin_text_edit(index, true);
    }

    /// Whether somebody is typing into a drawing right now.
    ///
    /// The window asks, because while this is true the chart's own keys are
    /// the typist's: Delete deletes a character, Escape ends the edit rather
    /// than the selection, and a letter is a letter rather than the start of
    /// a symbol search.
    pub fn is_typing_text(&self) -> bool {
        self.editing.borrow().is_some()
    }

    /// Whether any drawing is selected: F2 is then the drawing's, for its
    /// text, and not the chart title's.
    pub fn has_selection(&self) -> bool {
        !self.state.borrow().selected.is_empty()
    }

    /// Lay `drawing` down alone and select it, for a test that needs one.
    #[doc(hidden)]
    pub fn select_only(&self, drawing: Drawing) {
        let mut state = self.state.borrow_mut();
        state.drawings = vec![drawing];
        state.selected = vec![0];
    }

    /// Open the editor on the selected drawing, if one is selected. What
    /// "Edit text" in the drawing's menu does, and Enter is not: Enter opens
    /// the properties, which is where it has always gone.
    pub fn edit_selected_text(self: &Rc<Self>) -> bool {
        let index = {
            let state = self.state.borrow();
            match state.selected.last() {
                Some(index) if state.drawings.get(*index).is_some() => *index,
                _ => return false,
            }
        };
        self.begin_text_edit(index, false)
    }

    /// Put a caret on the drawing at `index` and let the keyboard have it.
    ///
    /// `fresh` says the drawing was made for this edit — a click with the
    /// text tool — so that nothing typed means nothing drawn, and the empty
    /// drawing goes away again rather than becoming litter nobody can see
    /// to clear.
    fn begin_text_edit(self: &Rc<Self>, index: usize, fresh: bool) -> bool {
        self.end_text_edit(text_editor::Ended::Commit);
        let (text, style, ink, align) = {
            let state = self.state.borrow();
            let Some(drawing) = state.drawings.get(index).filter(|d| d.kind.takes_text()) else { return false };
            let style = drawing.style(&state.configs).clone();
            let ground = drawings::text_ground(drawing.kind, &style, &state.theme);
            let ink = drawings::text_colour(&style.text.colour, &state.theme, &ground);
            let align = crate::text::placement(drawing.kind, &drawing.text).alignment();
            (drawing.text.clone(), style, ink, align)
        };

        let editing = text_editor::build(
            text_editor::Opened { index, fresh, text: &text, style: &style.text, ink: &ink, align },
            {
                let view = Rc::downgrade(self);
                move || {
                    if let Some(view) = view.upgrade() {
                        view.place_editor();
                    }
                }
            },
            {
                let view = Rc::downgrade(self);
                move |how| {
                    if let Some(view) = view.upgrade() {
                        view.end_text_edit(how);
                    }
                }
            },
        );

        self.state.borrow_mut().editing = Some(index);
        self.tell_typing(true);
        // Over the crosshair, since the caret is the thing in front while
        // somebody is typing, and measured out of the overlay so a long
        // label cannot make the chart want to be wider than it is.
        self.root.add_overlay(&editing.view);
        self.root.set_measure_overlay(&editing.view, false);
        *self.editing.borrow_mut() = Some(editing.clone());
        self.place_editor();
        self.redraw();
        // After the layout, or the caret lands in a widget that has not been
        // given a size yet and the first keystroke moves it.
        let editing = editing.clone();
        glib::idle_add_local_once(move || {
            editing.view.grab_focus();
            // After the focus, which places the caret and would otherwise
            // drop the selection the moment it is made.
            editing.select_all();
        });
        true
    }

    /// Put the editor where the words belong, and size it to them.
    ///
    /// Called again after every keystroke, because the place depends on the
    /// size: a label centred in a box moves left as it grows, and one that
    /// did not would drift away from where it is about to be drawn.
    fn place_editor(&self) {
        let Some(editing) = self.editing.borrow().clone() else { return };
        let (width, height) = (self.area.width() as f64, self.area.height() as f64);
        let placed = {
            let state = self.state.borrow();
            let Some(drawing) = state.drawings.get(editing.index.get()) else { return };
            let style = drawing.style(&state.configs);
            let at = crate::text::placement(drawing.kind, &drawing.text);
            let size = editing.measure(&style.text, at.alignment(), at);
            let plan = layout(&state, width, height);
            let (first, visible) = state.slice();
            let Some(scale) = price_range(&state, &state.bars[first..first + visible], &plan) else {
                return;
            };
            let projected = state.project(&plan, &scale, drawing);
            // The drawing's own box, not the label's: a figure is placed in
            // by its shape, and a text drawing hangs from its anchor, which
            // `text_origin` reads off the degenerate box either way.
            let origin = drawings::text_origin(drawing.kind, projected.bounds(), size, at);
            (origin, size, style.text.clamped_size())
        };
        editing.place(placed.0, placed.1, placed.2);
        let ((x, y), (w, h), _) = placed;
        let moved = self.state.borrow().editing_at != Some((x, y, w, h));
        if moved {
            self.state.borrow_mut().editing_at = Some((x, y, w, h));
            self.redraw();
        }
    }

    /// Take the editor off the chart, keeping what was typed or throwing it
    /// away.
    fn end_text_edit(&self, how: text_editor::Ended) {
        let Some(editing) = self.editing.borrow_mut().take() else { return };
        self.root.remove_overlay(&editing.view);
        {
            let mut state = self.state.borrow_mut();
            state.editing = None;
            state.editing_at = None;
        }
        self.tell_typing(false);

        let event = match how {
            text_editor::Ended::Cancel => {
                // A drawing made for this edit and abandoned was never
                // really made: take it away again, and take the undo step
                // that was pushed for it with it, so Ctrl+Z is not a press
                // that undoes something nobody saw.
                match editing.fresh {
                    true => {
                        let gone = self.drop_drawing(editing.index.get());
                        self.state.borrow_mut().forget();
                        gone
                    }
                    false => None,
                }
            }
            text_editor::Ended::Commit | text_editor::Ended::LostFocus => {
                let at = {
                    let state = self.state.borrow();
                    state
                        .drawings
                        .get(editing.index.get())
                        .map(|d| d.text.at)
                        .unwrap_or_default()
                };
                let typed = editing.text(at);
                // Nothing typed into a drawing that is nothing but its text:
                // there is no drawing. One that is a figure simply has no
                // label.
                let empty_text_drawing = typed.is_empty()
                    && self
                        .state
                        .borrow()
                        .drawings
                        .get(editing.index.get())
                        .is_some_and(|d| d.kind.is_text());
                if empty_text_drawing {
                    self.drop_drawing(editing.index.get())
                } else {
                    let mut state = self.state.borrow_mut();
                    let Some(drawing) = state.drawings.get_mut(editing.index.get()) else {
                        return;
                    };
                    if drawing.text == typed {
                        None
                    } else {
                        // One step for a drawing that was made to be typed
                        // into: putting it down and saying something are
                        // one action, and its own step was pushed when it
                        // was made. Two for a label added to a drawing that
                        // was already there, which are two.
                        if !editing.fresh {
                            state.remember();
                        }
                        let drawing = &mut state.drawings[editing.index.get()];
                        drawing.set_text(typed);
                        Some(DrawingEvent::Changed(drawing.clone()))
                    }
                }
            }
        };
        self.redraw();
        // Only when the edit ended on its own terms. If the keyboard was
        // moved somewhere else — the key that ended this was the key that
        // moved it — taking it back to the chart would undo what the hand
        // just asked for.
        if !matches!(how, text_editor::Ended::LostFocus) {
            self.area.grab_focus();
        }
        if let Some(event) = event {
            self.tell(event);
        }
    }

    /// Take a drawing out of the list and say so, for the one that was never
    /// wanted: an empty word.
    fn drop_drawing(&self, index: usize) -> Option<DrawingEvent> {
        let mut state = self.state.borrow_mut();
        if index >= state.drawings.len() {
            return None;
        }
        let gone = state.drawings.remove(index);
        state.selected.retain(|i| *i != index);
        for selected in state.selected.iter_mut() {
            if *selected > index {
                *selected -= 1;
            }
        }
        // Never written down, so there is nothing to remove from the store
        // and nothing to tell anybody about.
        (gone.id != 0).then_some(DrawingEvent::Removed(gone.id))
    }

    /// Put the selected drawings' labels at `at`, for the placement keys.
    ///
    /// Only the drawings it means anything for: a figure that has something
    /// to say. A text drawing hangs from its own anchor and has no placement
    /// to set, and a figure with no label has nowhere to put one — moving
    /// either would be a key that looks like it did nothing. Says whether it
    /// found any, so the caller can let the key go on to what it otherwise
    /// does.
    pub fn place_text(&self, at: drawings::Place) -> bool {
        let any = {
            let state = self.state.borrow();
            state
                .selected
                .iter()
                .filter_map(|i| state.drawings.get(*i))
                .any(|d| !d.kind.is_text() && d.has_text())
        };
        if !any {
            return false;
        }
        self.edit_selected(move |drawing| {
            if !drawing.kind.is_text() && drawing.has_text() {
                let mut text = drawing.text.clone();
                text.at = at;
                drawing.set_text(text);
            }
        });
        true
    }

    /// Step the stroke of everything selected, for Alt+Shift+= and
    /// Alt+Shift+-.
    ///
    /// One key for every kind that has one, because they are one property:
    /// a line's thickness and a box's edge are the same field, and a hand
    /// that has learned to thicken a line should not have to learn a second
    /// way to thicken a rectangle. Text has no stroke and lets the key go
    /// on.
    pub fn step_width(self: &Rc<Self>, by: f64) -> bool {
        let any = {
            let state = self.state.borrow();
            state
                .selected
                .iter()
                .filter_map(|i| state.drawings.get(*i))
                .any(|d| !d.kind.is_text())
        };
        if !any {
            return false;
        }
        let configs = self.state.borrow().configs.clone();
        self.edit_selected(move |drawing| {
            if drawing.kind.is_text() {
                return;
            }
            // A retracement's thickness is its levels': every one steps.
            // Worked out first, so a step at the end of the range changes
            // nothing and leaves the drawing on its configuration.
            if drawing.kind == DrawingKind::Fib {
                let mut fib = drawing.style(&configs).fib.clone();
                for level in fib.levels.iter_mut() {
                    level.stroke.width = (level.stroke.width + by).clamp(drawings::MIN_WIDTH, drawings::MAX_WIDTH);
                }
                if fib != drawing.style(&configs).fib {
                    drawing.edit_style(&configs, |style| style.fib = fib.clone());
                }
                return;
            }
            let now = drawing.style(&configs).width;
            let next = (now + by).clamp(drawings::MIN_WIDTH, drawings::MAX_WIDTH);
            if next != now {
                drawing.edit_style(&configs, |style| style.width = next);
            }
        });
        true
    }

    /// Put a named arrowhead on everything selected; `None` takes it off.
    ///
    /// What the row of four in the drawing's menu does. The key cycles, this
    /// picks, and both land on the same two fields.
    pub fn set_head(self: &Rc<Self>, head: Option<omacharts_engine::ArrowHead>) {
        let configs = self.state.borrow().configs.clone();
        self.edit_selected(move |drawing| {
            if !drawing.kind.is_line() {
                return;
            }
            // A line that already points at both ends goes on pointing at
            // both: this row is about the shape of the head, and the one
            // choice in it that is not a shape is "none".
            let keep = drawing.style(&configs).arrow;
            drawing.edit_style(&configs, |style| match head {
                Some(shape) => {
                    style.head = shape;
                    if keep == omacharts_engine::Arrow::None {
                        style.arrow = omacharts_engine::Arrow::End;
                    }
                }
                None => style.arrow = omacharts_engine::Arrow::None,
            });
        });
    }

    /// Everything selected, ready to be put down again.
    ///
    /// Without ids, because a copy is not the drawing it came from: the id
    /// is where it lives, and these do not live anywhere until they are
    /// pasted. Without the chart that made them, for the same reason — a
    /// pasted drawing belongs to the chart it is pasted on.
    pub fn copy_selected(&self) -> Vec<Drawing> {
        let state = self.state.borrow();
        let mut taken: Vec<Drawing> = state
            .selected
            .iter()
            .filter_map(|i| state.drawings.get(*i))
            .map(|drawing| Drawing { id: 0, origin: None, ..drawing.clone() })
            .collect();
        drawings::sort_for_painting(&mut taken);
        taken
    }

    /// Put a copy down on this chart, a little off where it came from, and
    /// take hold of it.
    ///
    /// Offset so that a paste onto the chart it was copied from is a
    /// drawing you can see rather than one hiding exactly behind another,
    /// and so that pasting twice gives two. In pixels rather than in
    /// moments, because what it has to clear is the drawing on screen.
    ///
    /// Says whether anything was put down, so the key can go on to whatever
    /// else it means when there is nothing to paste.
    pub fn paste(&self, copied: &[Drawing], offset: bool) -> bool {
        let (width, height) = (self.area.width() as f64, self.area.height() as f64);
        // One borrow, and nothing called from inside it. The first version
        // asked a getter for the chart's own sharing while holding this,
        // and a getter takes the same cell: not an error anybody sees until
        // the key is pressed, and then not a panic but an abort, because it
        // happens under a GTK callback.
        let added = self.state.borrow_mut().paste(copied, offset, width, height);
        if added.is_empty() {
            return false;
        }
        // The pasted drawings are the selection now, and only they are.
        self.pick(None);
        self.redraw();
        for drawing in added {
            self.tell(DrawingEvent::Added(drawing));
        }
        true
    }

    /// Put a width on everything selected: what the row of four in the
    /// drawing's menu does, where the keys step.
    pub fn set_width(self: &Rc<Self>, width: f64) {
        let configs = self.state.borrow().configs.clone();
        let width = width.clamp(drawings::MIN_WIDTH, drawings::MAX_WIDTH);
        self.edit_selected(move |drawing| {
            if drawing.kind.is_text() {
                return;
            }
            let fib = drawing.kind == DrawingKind::Fib;
            drawing.edit_style(&configs, |style| match fib {
                true => style.fib.set_width(width),
                false => style.width = width,
            });
        });
    }

    /// The width the selection shares, if it is all of one mind.
    pub fn selected_width(&self) -> Option<f64> {
        let state = self.state.borrow();
        let mut seen = state
            .selected
            .iter()
            .filter_map(|i| state.drawings.get(*i))
            .filter(|d| !d.kind.is_text())
            .map(|d| match d.kind {
                // A retracement's is its levels', when they share one.
                DrawingKind::Fib => d.style(&state.configs).fib.width(),
                _ => Some(d.style(&state.configs).width),
            });
        let first = seen.next()?;
        seen.all(|other| other == first).then_some(first).flatten()
    }

    /// The arrow and head the selection shares, if it is all of one mind.
    pub fn selected_head(&self) -> Option<(omacharts_engine::Arrow, omacharts_engine::ArrowHead)> {
        let state = self.state.borrow();
        let mut seen = state
            .selected
            .iter()
            .filter_map(|i| state.drawings.get(*i))
            .filter(|d| d.kind.is_line())
            .map(|d| {
                let style = d.style(&state.configs);
                (style.arrow, style.head)
            });
        let first = seen.next()?;
        seen.all(|other| other == first).then_some(first)
    }

    /// Step the arrowhead of everything selected round its four states, for
    /// Alt+Shift+A.
    ///
    /// None, then the three shapes, then none again. A line that already
    /// wears heads at both ends keeps them and only changes their shape —
    /// quietly demoting it to one end would be the key doing a second thing
    /// nobody asked for — and one that had none comes back pointing at its
    /// end, which is where an arrow points.
    pub fn cycle_head(self: &Rc<Self>) -> bool {
        let any = {
            let state = self.state.borrow();
            state
                .selected
                .iter()
                .filter_map(|i| state.drawings.get(*i))
                .any(|d| d.kind.is_line())
        };
        if !any {
            return false;
        }
        let configs = self.state.borrow().configs.clone();
        self.edit_selected(move |drawing| {
            if !drawing.kind.is_line() {
                return;
            }
            let style = drawing.style(&configs);
            let next = step_head(style.arrow, style.head);
            drawing.edit_style(&configs, |style| {
                style.arrow = next.0;
                style.head = next.1;
            });
        });
        true
    }

    /// Step the text size of everything selected, for Ctrl+= and Ctrl+-.
    ///
    /// On a figure it is the figure's label that grows, not the figure: the
    /// shape is sized by its grips and a key that quietly resized it would
    /// be a second way to do what dragging already does.
    pub fn step_text_size(self: &Rc<Self>, by: f64) -> bool {
        let changed = {
            let state = self.state.borrow();
            !state.selected.is_empty()
        };
        if !changed {
            return false;
        }
        let configs = self.state.borrow().configs.clone();
        self.edit_selected(move |drawing| {
            let now = drawing.style(&configs).text.clamped_size();
            let next = (now + by).clamp(drawings::MIN_TEXT_SIZE, drawings::MAX_TEXT_SIZE);
            if next != now {
                drawing.edit_style(&configs, |style| style.text.size = next);
            }
        });
        self.restyle_editor();
        true
    }

    /// Set an open editor in the style its drawing now has, and put it back
    /// where the new size belongs.
    fn restyle_editor(&self) {
        let Some(editing) = self.editing.borrow().clone() else { return };
        let (style, ink) = {
            let state = self.state.borrow();
            let Some(drawing) = state.drawings.get(editing.index.get()) else { return };
            let style = drawing.style(&state.configs).clone();
            let ground = drawings::text_ground(drawing.kind, &style, &state.theme);
            let ink = drawings::text_colour(&style.text.colour, &state.theme, &ground);
            (style, ink)
        };
        editing.restyle(&style.text, &ink);
        self.place_editor();
    }

    /// Show where another chart's pointer is, in time and in price.
    ///
    /// Bar indices are not shared — two resolutions of the same symbol count
    /// their bars differently — so both halves travel as values and each
    /// chart maps them through its own scales. Price is worth carrying
    /// because an echo only ever reaches the other charts of the same
    /// symbol: the number means the same thing at both ends.
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

    /// Take the crosshair off as though the pointer had left the chart.
    pub fn drop_pointer(&self) {
        if self.state.borrow_mut().pointer.take().is_none() {
            return;
        }
        if let Some(handler) = self.on_hover.borrow().as_ref() {
            handler(None);
        }
        self.pointer.queue_draw();
    }

    /// Replace the series. Resets the view only when the instrument changed,
    /// so a background refresh of the same chart does not throw away a pan.
    /// The latest trade the bars leave out goes with the bars it was of:
    /// whoever has one says so again, with [`ChartView::set_latest`].
    pub fn set_series(&self, instrument: Instrument, timeframe: Timeframe, bars: Vec<Bar>) {
        let changed = {
            let mut state = self.state.borrow_mut();
            state.latest = None;
            state.replace_series(instrument, timeframe, bars)
        };
        if changed {
            self.tool_changed();
        }
        self.redraw();
    }

    /// Say what the feed last traded at when the bars drawn leave it out,
    /// a chart of the regular session out of hours, or `None` when the
    /// last bar has it. The last price's line and tag stand there, and
    /// the price scale reaches it while the chart shows its end.
    pub fn set_latest(&self, price: Option<f64>) {
        let price = price.filter(|price| price.is_finite());
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.latest != price;
            state.latest = price;
            changed
        };
        if changed {
            self.redraw();
        }
    }

    /// Say how far down the top of the price axis the window's corner
    /// stands, or 0 on a chart it is not over.
    pub fn set_covered_axis_top(&self, px: f64) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.covered_axis_top != px;
            state.covered_axis_top = px;
            changed
        };
        if changed {
            self.redraw();
        }
    }

    /// Say whether a control of the window's stands in the square under
    /// the price axis.
    pub fn set_axis_corner_taken(&self, taken: bool) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.axis_corner_taken != taken;
            state.axis_corner_taken = taken;
            changed
        };
        if changed {
            self.redraw();
        }
    }

    /// Whether the window has said its toggle stands in the axis corner.
    pub fn axis_corner_taken(&self) -> bool {
        self.state.borrow().axis_corner_taken
    }

    /// Say how far in from the left edge the window's drawing tools'
    /// toggle stands over the time axis, zero where it does not.
    pub fn set_axis_start_taken(&self, taken: f64) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.axis_start_taken != taken;
            state.axis_start_taken = taken;
            changed
        };
        if changed {
            self.redraw();
        }
    }

    /// How far in the window has said its toggle stands over the time axis.
    pub fn axis_start_taken(&self) -> f64 {
        self.state.borrow().axis_start_taken
    }

    /// Put live bars into the series in place: each replaces the bar with
    /// its timestamp or joins the end, and nothing else about the chart
    /// moves. A view anchored to the right edge follows a new bar on its
    /// own, because the anchor is a fact about the series' end rather than
    /// an index; a view panned into history stays where it is.
    ///
    /// A bar arriving is also proof the feed is alive, so whatever the
    /// corner was saying about the last fetch stops being true.
    pub fn apply_tail(&self, tail: &[Bar]) {
        {
            let mut state = self.state.borrow_mut();
            for bar in tail {
                omacharts_engine::stream::upsert(&mut state.bars, *bar);
            }
            let state = &mut *state;
            if let Some(compare) = state.compare.as_mut() {
                compare.realign(&state.bars, state.timeframe);
            }
            state.trouble = None;
            state.trouble_words = None;
            state.loading = false;
            // A new day can start a new month, and the month to date with it.
            state.fit_range();
        }
        self.redraw();
    }

    /// Show or hide the gridlines. The axes and their labels stay: without
    /// them a chart is a shape with no scale.
    pub fn set_show_grid(&self, show: bool) {
        self.state.borrow_mut().show_grid = show;
        self.redraw();
    }

    pub fn show_indicators(&self) -> bool {
        self.state.borrow().show_indicators
    }

    /// Show or hide every indicator at once, strips included, for a look at
    /// the bars alone. Each keeps its own `visible`, so they come back as
    /// they were.
    pub fn set_show_indicators(&self, show: bool) {
        self.state.borrow_mut().show_indicators = show;
        self.hidden_changed();
        self.redraw();
    }

    pub fn bar_style(&self) -> BarStyle {
        self.state.borrow().bar_style
    }

    /// Read the time off this clock, and write it this way.
    pub fn set_clock(&self, clock: Clock) {
        if self.state.borrow().clock == clock {
            return;
        }
        self.state.borrow_mut().clock = clock;
        self.redraw();
    }

    pub fn set_bar_style(&self, style: BarStyle) {
        self.state.borrow_mut().bar_style = style;
        self.redraw();
    }

    /// How thick an OHLC bar is drawn, in pixels; held to what the setting
    /// allows.
    pub fn set_ohlc_thickness(&self, thickness: u8) {
        self.state.borrow_mut().ohlc_thickness = clamp_thickness(thickness);
        self.redraw();
    }

    /// Stop drawing the bars, or start again. Only the bars go: the scale
    /// still fits them, and what is drawn from them stays where it was.
    /// Compare other markets on this chart, or stop comparing with `None`.
    /// Handed whole each time, bars and all: a line whose bars just arrived
    /// is the same call as the first.
    pub fn set_comparing(&self, comparing: Option<Comparing>) {
        {
            let mut state = self.state.borrow_mut();
            let state = &mut *state;
            let handler = state.on_compare_readout.clone();
            state.compare = comparing
                .map(|spec| Box::new(compare::Overlay::new(spec, &state.bars, state.timeframe, handler)));
        }
        self.redraw();
    }

    /// Whether the chart is comparing markets, and so drawn in percent.
    pub fn comparing(&self) -> bool {
        self.state.borrow().compare.is_some()
    }

    /// Told what every compared market reads at the hovered bar, or at the
    /// last on screen with none hovered, whenever that changes.
    pub fn set_compare_readout_handler(&self, handler: impl Fn(Readout) + 'static) {
        *self.state.borrow().on_compare_readout.borrow_mut() = Some(Box::new(handler));
    }

    /// The time of the bar at `x` of the chart, the nearest end's past
    /// either end of the series: what "Compare from here" pins the base to.
    pub fn bar_time_at(&self, x: f64) -> Option<i64> {
        let state = self.state.borrow();
        let (first, visible, lead) = state.view();
        if visible == 0 {
            return None;
        }
        let plot_w = (self.area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0);
        let frac = ((x - PAD) / plot_w).clamp(0.0, 1.0);
        let index = (first as f64 - lead as f64 + frac * state.columns() as f64).floor().max(0.0) as usize;
        state.bars.get(index.min(state.bars.len().saturating_sub(1))).map(|bar| bar.ts)
    }

    pub fn set_bars_hidden(&self, hidden: bool) {
        self.state.borrow_mut().bars_hidden = hidden;
        self.hidden_changed();
        self.redraw();
    }

    /// What the chart is hiding all of.
    pub fn hidden(&self) -> Hidden {
        let state = self.state.borrow();
        Hidden { drawings: !state.show_drawings, indicators: !state.show_indicators, bars: state.bars_hidden }
    }

    /// What to tell when something is hidden or shown. Called once at once,
    /// so whatever listens starts out right.
    pub fn set_hidden_handler(&self, handler: impl Fn(Hidden) + 'static) {
        handler(self.hidden());
        *self.on_hidden.borrow_mut() = Some(Box::new(handler));
    }

    fn hidden_changed(&self) {
        let hidden = self.hidden();
        if let Some(handler) = self.on_hidden.borrow().as_ref() {
            handler(hidden);
        }
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
            match auto {
                true => state.free_price(),
                false => state.hold_price(self.area.width() as f64, self.area.height() as f64),
            }
        }
        self.redraw();
    }

    /// The prices a held price scale shows, bottom and top; `None` while it
    /// fits itself. What the window writes down with the chart.
    pub fn held_price_window(&self) -> Option<(f64, f64)> {
        self.state.borrow().held_window()
    }

    /// Take the price scale off automatic, showing these prices, bottom and
    /// top: a held scale brought back from where it was written down.
    pub fn hold_price_window(&self, low: f64, high: f64) {
        self.state.borrow_mut().hold_window(low, high);
        self.redraw();
    }

    /// The strips zoomed or dragged on their own scale, by indicator, each
    /// with the values its bottom and top show; a strip that fits itself is
    /// not among them. What the window writes down with the chart.
    pub fn held_strip_windows(&self) -> BTreeMap<u32, (f64, f64)> {
        self.state.borrow().strip_held.clone()
    }

    /// The kind of indicator each held strip was held for, where known.
    pub fn held_strip_kinds(&self) -> BTreeMap<u32, indicators::Kind> {
        self.state.borrow().strip_kinds.clone()
    }

    /// Hold the strip of the indicator `id` at these values, bottom and top:
    /// one brought back from where it was written down, before or after the
    /// indicator is drawn. Held for `kind` when it is known, so that only
    /// an indicator of that kind under `id` takes it.
    pub fn hold_strip_window(&self, id: u32, kind: Option<indicators::Kind>, low: f64, high: f64) {
        let mut state = self.state.borrow_mut();
        state.hold_strip(id, low, high);
        if let (Some(kind), true) = (kind, state.strip_held.contains_key(&id)) {
            state.strip_kinds.insert(id, kind);
        }
        // Drawn already: held only if it is that kind.
        if state.indicators.iter().any(|d| d.indicator.id == id) {
            state.prune_strips();
        }
        drop(state);
        self.redraw();
    }

    /// Every strip back to fitting itself: when what the series is drawn
    /// from changes in a way the chart cannot see, its trading hours.
    pub fn free_strip_windows(&self) {
        self.state.borrow_mut().free_strips();
        self.redraw();
    }

    /// Is the price axis logarithmic? What was asked for, which is what the
    /// chart remembers: a stretch of bars at or below zero draws linear
    /// without changing the answer.
    pub fn price_log(&self) -> bool {
        self.state.borrow().price_log
    }

    /// Make the price axis logarithmic or linear.
    ///
    /// Leaves the axis automatic or held as it was. A held axis keeps the
    /// prices it holds, top and bottom, on either scale.
    pub fn set_price_log(&self, log: bool) {
        self.state.borrow_mut().price_log = log;
        self.redraw();
    }

    /// Be told when a gesture on the chart changes whether the price scale
    /// is automatic, or moves a held one, or zooms or drags a strip on its
    /// own scale: a wheel notch over the axis at once, a drag or a pinch
    /// once it is let go of. Handed whether the price's is automatic now. Not called for [`set_price_auto`](Self::set_price_auto) or the
    /// resets: whoever called those already knows.
    pub fn set_price_auto_handler(&self, handler: impl Fn(bool) + 'static) {
        *self.on_price_auto.borrow_mut() = Some(Box::new(handler));
    }

    /// Be told when the view comes within a screen of the series' first
    /// bar, however it got there, with that bar's time: whoever has older
    /// bars can bring them, and the view stays on what it shows as they
    /// come in before it. Told once for each first bar, so a feed with
    /// nothing older is not asked again until a series with another first
    /// bar is drawn. Told after the chart is drawn, with nothing borrowed.
    pub fn set_history_handler(&self, handler: impl Fn(i64) + 'static) {
        *self.on_history.borrow_mut() = Some(Box::new(handler));
    }

    pub fn set_context_menu_handler(&self, handler: impl Fn(f64, f64) + 'static) {
        *self.on_context_menu.borrow_mut() = Some(Box::new(handler));
    }

    pub fn show_drawings(&self) -> bool {
        self.state.borrow().show_drawings
    }

    /// Show or hide every drawing at once, the chart's own and the shared,
    /// for a look at the bars. They are kept, not removed: hidden, nothing
    /// of them is selected or edited, and they come back as they were.
    pub fn set_show_drawings(&self, show: bool) {
        if !show {
            // What was being typed is kept, the way clicking away keeps it.
            self.end_text_edit(text_editor::Ended::Commit);
            if self.armed().is_some() {
                self.arm(None);
            }
        }
        {
            let mut state = self.state.borrow_mut();
            state.show_drawings = show;
            if !show {
                state.selected.clear();
            }
        }
        self.hidden_changed();
        self.redraw();
    }

    pub fn set_indicators(&self, indicators: Vec<Drawn>) {
        let wanted = {
            let mut s = self.state.borrow_mut();
            s.indicators = indicators;
            s.prune_strips();
            let arrived = s.pending_pick.filter(|id| s.indicators.iter().any(|d| d.indicator.id == *id));
            if arrived.is_some() {
                s.pending_pick = None;
            }
            arrived.or(s.picked)
        };
        self.redraw();
        // One removed or hidden is no longer selected.
        if wanted.is_some() {
            self.pick(wanted);
        }
    }

    /// Select an indicator now, or as soon as it is drawn.
    pub fn pick_when_shown(&self, id: u32) {
        let shown = self.state.borrow().indicators.iter().any(|d| d.indicator.id == id);
        match shown {
            true => self.pick(Some(id)),
            false => self.state.borrow_mut().pending_pick = Some(id),
        }
    }

    /// Say why the last fetch brought nothing back, or `None` once one works.
    ///
    /// Taken rather than discarded because "No data for this symbol" is a
    /// claim about the symbol, and a fetch that never arrived is not evidence
    /// for it. A first-time user on a machine with no network read that line
    /// as the app not having the S&P 500.
    pub fn set_trouble(&self, trouble: Option<FetchFailure>) {
        {
            let mut state = self.state.borrow_mut();
            state.trouble = trouble;
            state.trouble_words = None;
        }
        self.redraw();
    }

    /// [`ChartView::set_trouble`], said in the embedder's own words.
    pub fn set_trouble_saying(&self, trouble: FetchFailure, words: String) {
        {
            let mut state = self.state.borrow_mut();
            state.trouble = Some(trouble);
            state.trouble_words = Some(words);
        }
        self.redraw();
    }

    /// What the chart says about its trouble, if it has any.
    pub fn trouble_said(&self) -> Option<String> {
        let state = self.state.borrow();
        let trouble = state.trouble?;
        Some(state.trouble_words.clone().unwrap_or_else(|| trouble.message().to_string()))
    }

    /// Lay `marks` over the chart, in the order they are drawn, with the
    /// `rules` a drag on them keeps to — [`AnyPrice`] for none. Redrawn only
    /// when either changes.
    pub fn set_marks<R: MarkRules + PartialEq>(&self, marks: Vec<PriceMark>, rules: R) {
        {
            let mut state = self.state.borrow_mut();
            if state.marks.same(&marks, &rules) {
                return;
            }
            state.marks = Marks { lines: marks, rules: Box::new(rules) };
            // What was being dragged went under the hand.
            if let Some(Drag::Mark { what, .. }) = &state.drag
                && state.mark_dragged(what).is_none()
            {
                state.drag = None;
            }
        }
        self.redraw();
    }

    /// The marks the chart is laid over with, as they were given.
    pub fn price_marks(&self) -> Vec<PriceMark> {
        self.state.borrow().marks.lines.clone()
    }

    /// Put the marks' labels at `side` of the chart. Redrawn only when it
    /// changes.
    pub fn set_mark_labels(&self, side: LabelSide) {
        if std::mem::replace(&mut self.state.borrow_mut().mark_labels, side) != side {
            self.redraw();
        }
    }

    /// The price at height `y` of the chart, on the scale it is drawn at,
    /// while `y` is over the price plot.
    pub fn price_under(&self, y: f64) -> Option<f64> {
        let (width, height) = (self.area.width() as f64, self.area.height() as f64);
        let state = self.state.borrow();
        let (first, visible, _) = state.view();
        if visible == 0 {
            return None;
        }
        let plan = layout(&state, width, height);
        if y < plan.price_y || y > plan.price_y + plan.price_h {
            return None;
        }
        let scale = price_range(&state, &state.bars[first..first + visible], &plan)?;
        Some(scale.price_at(y, plan.price_y, plan.price_h))
    }

    /// The height `price` is drawn at, on the scale on screen.
    pub fn y_of(&self, price: f64) -> Option<f64> {
        let (width, height) = (self.area.width() as f64, self.area.height() as f64);
        let state = self.state.borrow();
        let (first, visible, _) = state.view();
        if visible == 0 {
            return None;
        }
        let plan = layout(&state, width, height);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan)?;
        Some(scale.to_y(price, plan.price_y, plan.price_h))
    }

    /// The last price on the chart: the latest bar's close.
    pub fn last_price(&self) -> Option<f64> {
        self.state.borrow().bars.last().map(|bar| bar.close)
    }

    /// The step the series trades in, when it shows one.
    pub fn tick(&self) -> Option<f64> {
        self.state.borrow().tick()
    }

    /// The step a mark is dragged on: the contract the marks trade, which
    /// need not step as the series charted does. `None` goes back to the
    /// series' own.
    pub fn set_mark_tick(&self, tick: Option<f64>) {
        self.state.borrow_mut().mark_tick = tick;
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

    pub fn loading(&self) -> bool {
        self.state.borrow().loading
    }

    /// How far the load the chart is waiting on has got, from 0 to 1: a
    /// thin bar under Loading…, for as long as that shows. `None` for a load
    /// that cannot say, which shows the words alone.
    ///
    /// The bar does not jump to each fraction as it is read: it starts
    /// empty and eases towards the newest one, frame by frame, closing all
    /// but an e-th of the gap every [`PROGRESS_EASE_SECONDS`]. The reads
    /// come ten times a second, so the bar is always on its way to the
    /// last of them, and never still between two. A fraction lower than
    /// the last is the same load misread, not a load going backwards, and
    /// the bar keeps the higher.
    pub fn set_progress(&self, progress: Option<f64>) {
        let (redraw, start) = {
            let mut state = self.state.borrow_mut();
            let loading = state.loading;
            match progress {
                None => {
                    let shown = state.progress.take().is_some();
                    state.progress_target = 0.0;
                    (shown && loading, false)
                }
                Some(fraction) => {
                    state.progress_target = fraction.clamp(0.0, 1.0).max(state.progress_target);
                    let appeared = state.progress.is_none();
                    let shown = *state.progress.get_or_insert(0.0);
                    (appeared && loading, loading && !state.progress_easing && shown < state.progress_target)
                }
            }
        };
        if redraw {
            self.area.queue_draw();
        }
        if start {
            self.ease_progress();
        }
    }

    /// Move the bar under Loading… towards where the load has got, a frame
    /// at a time, until it is there or Loading… is gone. One of these runs
    /// at a time: [`set_progress`](Self::set_progress) only starts another
    /// once this one has settled.
    fn ease_progress(&self) {
        self.state.borrow_mut().progress_easing = true;
        let state = Rc::downgrade(&self.state);
        let last_frame = Cell::new(None::<i64>);
        self.area.add_tick_callback(move |area, clock| {
            let Some(state) = state.upgrade() else { return glib::ControlFlow::Break };
            let mut state = state.borrow_mut();
            let now = clock.frame_time();
            let seconds = last_frame.replace(Some(now)).map_or(0.0, |then| (now - then) as f64 / 1e6);
            let (shown, target, loading) = (state.progress, state.progress_target, state.loading);
            let Some(shown) = shown.filter(|_| loading) else {
                state.progress_easing = false;
                return glib::ControlFlow::Break;
            };
            let left = (target - shown) * (-seconds / PROGRESS_EASE_SECONDS).exp();
            let settled = left < PROGRESS_SETTLED;
            state.progress = Some(if settled { target } else { target - left });
            area.queue_draw();
            if settled {
                state.progress_easing = false;
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    pub fn restyle(&self, theme: Theme, scheme: BarScheme) {
        {
            let mut state = self.state.borrow_mut();
            state.last_ink = LastInk::of(&theme);
            state.delta_inks = delta_inks(&theme, state.house);
            state.theme = theme;
            state.price_tags = price_tags(&scheme);
            state.scheme = scheme;
        }
        self.redraw();
    }

    /// The colouring chosen in Preferences and whether it is worn swapped,
    /// which a delta profile with no colouring of its own is painted in.
    pub fn set_house_look(&self, colouring: BarColouring, swapped: bool) {
        {
            let mut state = self.state.borrow_mut();
            if state.house == (colouring, swapped) {
                return;
            }
            state.house = (colouring, swapped);
            state.delta_inks = delta_inks(&state.theme, state.house);
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

    /// Read the series as the chart holds it, without copying it.
    ///
    /// `read` must not reach back into the chart: the state is borrowed for
    /// its duration, and a `set_indicators` from inside it would be a panic.
    pub fn with_bars<T>(&self, read: impl FnOnce(&[Bar]) -> T) -> T {
        read(&self.state.borrow().bars)
    }

    /// Jump back to the right edge and follow new bars again.
    pub fn go_to_latest(&self) {
        let mut state = self.state.borrow_mut();
        state.anchored = true;
        state.overhang = RIGHT_MARGIN;
        state.overhang_by_hand = false;
        state.lead = 0;
        drop(state);
        self.redraw();
    }

    /// Show a stretch of the calendar, and keep showing it — across a new
    /// symbol, new bars and a new day — until a hand moves the view.
    pub fn show_range(&self, range: Range) {
        self.state.borrow_mut().show_range(range);
        self.redraw();
    }

    /// The range the chart is holding to, until a hand moves the view.
    pub fn held_range(&self) -> Option<Range> {
        self.state.borrow().range
    }

        /// The first and last day on screen, on the clock the time axis reads.
    pub fn days_shown(&self) -> Option<(chrono::NaiveDate, chrono::NaiveDate)> {
        let state = self.state.borrow();
        let (first, visible, _) = state.view();
        let utc = state.timeframe.is_calendar();
        let day = |at: usize| state.bars.get(at).and_then(|bar| state.clock.read(bar.ts, utc)).map(|at| at.date());
        Some((day(first)?, day((first + visible).checked_sub(1)?)?))
    }

    /// How long the chart shows: from the moment at its left edge to the
    /// one at its right, as the time axis reads them, empty room past the
    /// last bar included. What a day typed on its own keeps.
    pub fn seconds_shown(&self) -> Option<i64> {
        let state = self.state.borrow();
        let columns = state.columns() as i64;
        if columns == 0 {
            return None;
        }
        let left = state.left_edge();
        Some(time_at_column(&state, left + columns - 1)? - time_at_column(&state, left)?)
    }

    /// The moment it is now, on the clock the time axis reads.
    pub fn now(&self) -> Option<chrono::NaiveDateTime> {
        let state = self.state.borrow();
        state.clock.read(chrono::Utc::now().timestamp(), state.timeframe.is_calendar())
    }

    /// The day of the first bar the chart holds.
    pub fn first_day(&self) -> Option<chrono::NaiveDate> {
        let state = self.state.borrow();
        let utc = state.timeframe.is_calendar();
        state.bars.first().and_then(|bar| state.clock.read(bar.ts, utc)).map(|at| at.date())
    }

    pub fn zoom(&self, factor: f64) {
        let plot_w = plot_width(&self.area);
        self.state.borrow_mut().zoom_time(factor, 0.5, plot_w);
        self.redraw();
    }

    pub fn pan_bars(&self, delta: i64) {
        let plot_w = plot_width(&self.area);
        self.state.borrow_mut().pan_by(delta as f64, plot_w);
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
        let on_history = self.on_history.clone();
        self.area.set_draw_func(move |_, cr, width, height| {
            let older = {
                let state = state.borrow();
                draw(cr, width as f64, height as f64, &state);
                state.wants_older()
            };
            if let (Some(first), Some(handler)) = (older, on_history.borrow().as_ref()) {
                handler(first);
            }
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
        motion.connect_motion(move |controller, x, y| {
            {
                let mut s = state.borrow_mut();
                s.magnet = controller.current_event_state().contains(gtk::gdk::ModifierType::CONTROL_MASK);
                s.pointer = Some((x, y));
                s.follow_hand(area.width() as f64, area.height() as f64);
                if s.ruler.is_some_and(|(_, _, finished)| !finished)
                    && let Some(to) = s.ruler_anchor(area.width() as f64, area.height() as f64, x, y)
                {
                    s.ruler_motion(to);
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
            let (armed, over_drawing, over_indicator, holding, over_mark) = {
                let mut s = state.borrow_mut();
                let (w, h) = (area.width() as f64, area.height() as f64);
                let free = s.drag.is_none() && s.tool.is_none() && !s.ruler_armed;
                // A mark's handle under the hand is lit; its line is only
                // said by the cursor.
                let over_mark = match free && !over_edge && !over_control {
                    true => s.mark_under(w, h, x, y),
                    false => None,
                };
                let handle = over_mark.clone().filter(|hit| !matches!(hit, MarkHit::Line(_)));
                if s.drag.is_none() && s.hover_mark != handle {
                    s.hover_mark = handle;
                    area.queue_draw();
                }
                let over = free.then(|| s.drawing_at(w, h, x, y)).flatten();
                // The grip under the hand is lit, which is a redraw of the
                // chart — but only on the motion that reaches a grip or leaves
                // one, not on every pixel of travel.
                if s.drag.is_none() {
                    let hover = over.filter(|(_, grip)| *grip != Grip::Body);
                    if s.hover_grip != hover {
                        s.hover_grip = hover;
                        area.queue_draw();
                    }
                }
                // Only when nothing drawn is under the pointer, which is
                // also when a click would select the indicator.
                let indicator = free
                    && over.is_none()
                    && !over_edge
                    && !over_control
                    && s.indicator_at(w, h, x, y).is_some();
                let holding = match &s.drag {
                    Some(Drag::Grip { index, grip, .. }) => Some(s.grip_cursor(*index, *grip, true)),
                    // A mark in hand: where it may not go, the pointer says
                    // so before the release does nothing.
                    Some(Drag::Mark { allowed: false, .. }) => Some("not-allowed"),
                    Some(Drag::Mark { .. }) => Some("ns-resize"),
                    _ => None,
                };
                (
                    s.tool.is_some() || s.ruler_armed,
                    over.map(|(index, grip)| s.grip_cursor(index, grip, false)),
                    indicator,
                    holding,
                    over_mark,
                )
            };
            let mark_handle = over_mark.as_ref().is_some_and(|hit| !matches!(hit, MarkHit::Line(_)));
            let mark_line = over_mark.as_ref().is_some_and(|hit| matches!(hit, MarkHit::Line(_)));
            area.set_cursor_from_name(Some(match (holding, over_edge, over_control, armed, over_drawing) {
                // A grip in hand keeps its cursor wherever the hand takes it,
                // over a pane's edge or a control included.
                (Some(cursor), ..) => cursor,
                (_, true, ..) => "ns-resize",
                (_, _, true, ..) => "pointer",
                // A mark's × or tab: a click or a drag does what it says.
                _ if mark_handle => "pointer",
                (_, _, _, true, _) => "crosshair",
                // What the grip under the pointer will do, said by the
                // pointer itself. A handle in the middle of an edge that
                // looks like every other handle is a handle nobody tries.
                (.., Some(cursor)) => cursor,
                // A mark's line: it can be dragged to another price.
                _ if mark_line => "ns-resize",
                // A hand over an indicator: a click there selects it. No
                // highlight on hover, which across a dense chart would
                // flicker on every line the pointer crossed.
                _ if over_indicator => "pointer",
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
        let area = self.area.clone();
        motion.connect_leave(move |_| {
            let mut s = state.borrow_mut();
            s.pointer = None;
            if s.hover_grip.take().is_some() | s.hover_mark.take().is_some() {
                area.queue_draw();
            }
            drop(s);
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
            tooltip.set_text(s.trouble_words.as_deref().or(s.trouble.map(FetchFailure::message)));
            true
        });
    }

    /// The wheel, with the conventions every charting tool shares.
    ///
    /// Plain wheel zooms time about the cursor. Shift pans. Ctrl scales price.
    /// Over an axis, the wheel scales that axis whatever the modifiers — the
    /// axis is the control.
    ///
    /// Zooming goes by how far the wheel turned or the fingers travelled,
    /// scaled by the app's zoom sensitivity — see [`crate::zoom`]. A wheel
    /// notch is a notch; a touchpad's pixels are a fraction of one each.
    fn wire_zoom(&self) {
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let on_price_auto = self.on_price_auto.clone();
        // Time-axis steps too small to move a column yet.
        let carry = std::cell::Cell::new(1.0);
        scroll.connect_scroll(move |controller, dx, dy| {
            if dx == 0.0 && dy == 0.0 {
                return glib::Propagation::Proceed;
            }
            let modifiers = controller.current_event_state();
            let shift = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);

            let (width, height) = (area.width() as f64, area.height() as f64);
            let mut s = state.borrow_mut();
            // The scale stays put under a mark being dragged.
            if matches!(s.drag, Some(Drag::Mark { .. })) {
                return glib::Propagation::Stop;
            }
            let was_price = s.scales();
            let region = s
                .pointer
                .map(|(x, y)| region_at(x, y, width, height))
                .unwrap_or(Region::Plot);
            // Over a strip's axis, or Ctrl over the strip, the wheel scales
            // that strip as it does the price over the price's.
            let strip = match s.pointer.and_then(|(x, y)| s.axis_at(x, y, width, height, ctrl && !shift)) {
                Some(Axis::Strip(id)) => Some(id),
                _ => None,
            };

            // A trackpad's horizontal axis always pans, whatever is held.
            if dx != 0.0 && dy == 0.0 {
                s.pan_by(dx * 2.0, plot_width(&area));
                drop(s);
                redraw(&area, &pointer);
                return glib::Propagation::Stop;
            }

            let unit = match controller.unit() {
                gtk::gdk::ScrollUnit::Surface => crate::zoom::Delta::Pixels,
                _ => crate::zoom::Delta::Notches,
            };
            let notches = crate::zoom::notches(dy, unit, crate::zoom::current());

            let scales = matches!((region, shift, ctrl), (Region::PriceAxis, _, _) | (Region::Plot, false, true));
            match (region, shift, ctrl) {
                _ if scales && let Some(id) = strip => {
                    s.scale_strip(id, 2f64.powf(notches / 4.0), width, height);
                }
                (Region::PriceAxis, _, _) | (Region::Plot, false, true) => {
                    // Same sense as dragging the axis.
                    s.scale_price(2f64.powf(notches / 4.0), width, height);
                }
                (Region::Plot, true, _) => {
                    let columns = s.columns() as f64;
                    s.pan_by(dy * columns / 20.0, plot_width(&area));
                }
                _ => {
                    let plot_w = (width - PRICE_AXIS_W - PAD).max(1.0);
                    let anchor = s
                        .pointer
                        .map(|(x, _)| ((x - PAD) / plot_w).clamp(0.0, 1.0))
                        .unwrap_or(0.5);
                    let factor = crate::zoom::time_factor(notches);
                    let (now, next) = crate::zoom::carried(carry.get(), factor, s.columns());
                    carry.set(next);
                    if let Some(factor) = now {
                        s.zoom_time(factor, anchor, plot_w);
                    }
                }
            }
            drop(s);
            redraw(&area, &pointer);
            notify_price_auto(&state, &on_price_auto, was_price);
            glib::Propagation::Stop
        });
        self.area.add_controller(scroll);

        // Two fingers spread or pinched on a strip scale that strip on its
        // own, and on the price's axis the price's, as a drag on the axis
        // does. Anywhere else it is not the chart's to take.
        let zoom = gtk::GestureZoom::new();
        let pinched: Rc<RefCell<Option<(Pinch, Scales)>>> = Rc::default();
        let (state, area, taken) = (self.state.clone(), self.area.clone(), pinched.clone());
        zoom.connect_begin(move |gesture, _| {
            let (width, height) = (area.width() as f64, area.height() as f64);
            let Some((x, y)) = gesture.bounding_box_center() else { return };
            let mut s = state.borrow_mut();
            let was = s.scales();
            match s.pinch_at(x, y, width, height) {
                Some(pinch) => *taken.borrow_mut() = Some((pinch, was)),
                None => {
                    gesture.set_state(gtk::EventSequenceState::Denied);
                }
            }
        });
        let (state, area, pointer, taken) = (self.state.clone(), self.area.clone(), self.pointer.clone(), pinched.clone());
        zoom.connect_scale_changed(move |_, scale| {
            let Some((pinch, _)) = *taken.borrow() else { return };
            state.borrow_mut().pinch(pinch, scale);
            redraw(&area, &pointer);
        });
        let (state, on_price_auto) = (self.state.clone(), self.on_price_auto.clone());
        zoom.connect_end(move |_, _| {
            if let Some((_, was)) = pinched.borrow_mut().take() {
                notify_price_auto(&state, &on_price_auto, was);
            }
        });
        self.area.add_controller(zoom);
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
        // A mark's × pressed: said on the release, if the hand is still on
        // it and has not travelled, so a press that slides off cancels
        // nothing, and one that moves on takes the line with it instead.
        // The press is not claimed, so the drag below sees it too.
        let pressed_mark: Rc<RefCell<Option<MarkHit>>> = Rc::default();
        let pressed = pressed_mark.clone();
        controls.connect_pressed(move |gesture, _, x, y| {
            let (hit, mark) = {
                let s = state.borrow();
                let (w, h) = (area.width() as f64, area.height() as f64);
                let plan = layout(&s, w, h);
                let hit = plan.control_at(x, y);
                // After the ruler, a drawing being placed and a pane's edge.
                let free = s.placing.is_none() && plan.edge_at(y).is_none() && hit.is_none();
                let mark = free
                    .then(|| s.mark_under(w, h, x, y))
                    .flatten()
                    .filter(|mark| matches!(mark, MarkHit::Close(_)));
                (hit, mark)
            };
            if let Some(mark) = mark {
                *pressed.borrow_mut() = Some(mark);
                return;
            }
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
        let state = self.state.clone();
        let area = self.area.clone();
        let view = Rc::downgrade(self);
        let pressed = pressed_mark.clone();
        controls.connect_released(move |_, _, x, y| {
            let Some(mark) = pressed.borrow_mut().take() else { return };
            let still = state.borrow().mark_under(area.width() as f64, area.height() as f64, x, y);
            if still.as_ref() != Some(&mark) {
                return;
            }
            let MarkHit::Close(id) = mark else { return };
            if let Some(view) = view.upgrade() {
                view.tell_mark(MarkEvent::Close(id));
            }
        });
        self.area.add_controller(controls);

        let drag = gtk::GestureDrag::new();

        let state = self.state.clone();
        let area = self.area.clone();
        let view = Rc::downgrade(self);
        let on_price_auto = self.on_price_auto.clone();
        let pointer = self.pointer.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
        let (width, height) = (area.width() as f64, area.height() as f64);
        {
            let mut s = state.borrow_mut();
            if s.ruler_armed && let Some(anchor) = s.ruler_anchor(width, height, x, y) {
                s.ruler_click(anchor);
                pointer.queue_draw();
                return;
            }
        }
        // On a drawing, Shift or Ctrl adds it to the selection: design
        // tools say Shift and charting tools say Ctrl, and a hand that
        // learned one should not have to learn the other. On empty chart
        // only Shift pulls out a box. With a tool in hand, Ctrl snaps the
        // anchor to the candle under it instead.
        let modifiers = gesture.current_event_state();
        let shift_only = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let shift = shift_only || ctrl;
        state.borrow_mut().magnet = ctrl;
        // A drawing being laid down takes the press before anything
        // else: the second press is the drawing's second anchor.
        // What this press means for a drawing already being laid down:
        // nothing, because there is none; another corner, for a zig-zag; or
        // the end of it, for every other kind.
        enum Press {
            Elsewhere,
            Corner,
            Finish,
        }
        let press = {
            let mut s = state.borrow_mut();
            s.closed_by_press = false;
            if s.placing.is_none() {
                Press::Elsewhere
            } else if s.closes_path(width, height, x, y) {
                s.drag = None;
                s.closed_by_press = true;
                Press::Finish
            } else {
                let anchor = s.locate_with(width, height, x, y, ctrl);
                let path = s.placing.as_ref().is_some_and(|d| d.kind.is_path());
                if let Some(anchor) = anchor
                    && let Some(placing) = s.placing.as_mut()
                {
                    match path {
                        // Another corner, and another following the hand.
                        // A zig-zag ends when it is told to, not when the
                        // second press lands.
                        true => {
                            placing.move_last_corner(anchor);
                            placing.add_corner(anchor);
                        }
                        false => placing.move_grip(Grip::To, anchor)
                    }
                }
                s.drag = None;
                match path {
                    true => Press::Corner,
                    false => Press::Finish,
                }
            }
        };
        match press {
            Press::Finish => {
                if let Some(view) = view.upgrade() {
                    view.commit_placing();
                }
                return;
            }
            // Handled, and nothing below applies — least of all the arm
            // below, which would otherwise take this press as the start of
            // a *new* zig-zag and throw away the one being drawn. That is
            // exactly what it did: every click after the first put a fresh
            // one-corner path in place of the run so far, so the segments
            // vanished as they were made.
            Press::Corner => {
                area.queue_draw();
                return;
            }
            Press::Elsewhere => {}
        }
        let mut s = state.borrow_mut();
        let was_price = s.scales();
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
        // A mark's tab, pulled out: its line follows the hand, at the price
        // under it, until the release says where.
        if region == Region::Plot
            && let Some(MarkHit::Tab { mark, tab }) = s.mark_under(width, height, x, y)
            && let Some((at, _)) = s.tick_price_at(width, height, y)
        {
            let what = Grab::Tab { mark, tab };
            let allowed = s.mark_allowed(&what, at);
            s.drag = Some(Drag::Mark { what, at, allowed });
            s.hover_mark = None;
            drop(s);
            redraw(&area, &pointer);
            return;
        }
        // The indicator the press selects, if it lands on no drawing: chosen
        // under the borrow, said once it is let go.
        let mut pick = None;
        if region == Region::Plot {
            // An armed tool: this press is the first anchor. Except the
            // text tool, which has no second anchor to wait for — a word
            // is at a point — so one press puts the caret down and the
            // keyboard takes over from there.
            if s.tool == Some(DrawingKind::Text) {
                let anchor = s.locate_with(width, height, x, y, ctrl);
                drop(s);
                if let (Some(view), Some(anchor)) = (view.upgrade(), anchor) {
                    view.start_text_at(anchor);
                }
                return;
            }
            // The anchored VWAP has no second anchor either: the press is
            // the bar it starts from, and the line is there at once.
            if s.tool == Some(DrawingKind::Vwap) {
                let placed = s.begin_placing(width, height, x, y, false);
                s.drag = None;
                let State { bars, placing, .. } = &mut *s;
                if placed && let Some(placing) = placing.as_mut() {
                    let at = anchor_on_a_bar(bars, placing.from);
                    placing.move_grip(Grip::From, at);
                }
                drop(s);
                if placed && let Some(view) = view.upgrade() {
                    view.commit_placing();
                }
                return;
            }
            if s.tool.is_some() {
                s.begin_placing(width, height, x, y, ctrl);
                // A zig-zag is laid down by pressing, a corner at a time,
                // so the drag that would rubber-band a line out of this
                // press is never started.
                if s.placing.as_ref().is_some_and(|d| d.kind.is_path()) {
                    s.drag = None;
                }
                return;
            }
            // A drawing under the hand: take hold of it. With Shift it
            // joins the selection, or leaves it, and nothing moves. One
            // already selected keeps its company, so a drag on it moves
            // them all. Otherwise a press on the chart lets go of whatever
            // was selected, and pans as it always did.
            // Except Ctrl on a corner, which takes hold of it so the
            // drag that follows snaps it to a candle; Shift still adds.
            let drawing = s.drawing_at(width, height, x, y);
            // A drawing taken lets go of the indicator selected, which the
            // window is told once the state is free again.
            let unpicked = drawing.is_some() && s.picked.take().is_some();
            let told = view.clone();
            let unpicked = move || {
                if unpicked && let Some(view) = told.upgrade() {
                    view.tell_indicator(IndicatorEvent::Picked(None));
                }
            };
            // A draggable mark's line, under no drawing: taken to another
            // price, starting where it is so nothing jumps. Its × too: a
            // press there that moves on is a drag, not a close.
            if drawing.is_none()
                && !shift_only
                && let Some(MarkHit::Line(id) | MarkHit::Close(id)) = s.mark_under(width, height, x, y)
                && let Some(at) = s.marks.lines.iter().find(|mark| mark.id == id && mark.draggable).map(|mark| mark.price)
            {
                s.drag = Some(Drag::Mark { what: Grab::Move(id), at, allowed: true });
                s.hover_mark = None;
                drop(s);
                pointer.queue_draw();
                return;
            }
            match drawing {
                Some((index, grip)) if shift && (shift_only || grip == Grip::Body) => {
                    match s.selected.iter().position(|i| *i == index) {
                        Some(at) => {
                            s.selected.remove(at);
                        }
                        None => s.selected.push(index),
                    }
                    drop(s);
                    area.queue_draw();
                    unpicked();
                    return;
                }
                Some((index, grip)) => {
                    if let Some(at) = s.selected.iter().position(|i| *i == index) {
                        s.selected.remove(at);
                    } else {
                        s.selected.clear();
                    }
                    s.selected.push(index);
                    // A profile's body selects it and nothing more: what
                    // follows is the chart's, so a drag across one pans. An
                    // anchored VWAP's line the same.
                    let still = grip == Grip::Body
                        && s.drawings.get(index).is_some_and(|d| d.kind.reads_bars());
                    if still {
                        let left = s.left_edge();
                        let strip = s.strip_pan_at(y, width, height);
                        s.drag = Some(Drag::Pan { left, offset: s.price_offset, strip });
                    } else if let Some(origin) = s.locate(width, height, x, y) {
                        // Before the first motion, so a drag is one step
                        // back however far it went. A press that never
                        // moves is forgotten at the end.
                        s.remember();
                        s.drag = Some(Drag::Grip { index, grip, origin });
                        area.set_cursor_from_name(Some(s.grip_cursor(index, grip, true)));
                    }
                    drop(s);
                    area.queue_draw();
                    unpicked();
                    return;
                }
                // Shift on empty chart: a box, which takes what it touches
                // as it grows and adds it to what was already selected.
                None if shift_only => {
                    s.marquee_base = s.selected.clone();
                    s.drag = Some(Drag::Marquee { from: (x, y), to: (x, y) });
                    drop(s);
                    pointer.queue_draw();
                    if let Some(view) = view.upgrade() {
                        view.pick(None);
                    }
                    return;
                }
                // Nothing drawn here: an indicator under the press is
                // selected, and the press goes on to pan, since an
                // indicator is not a thing that moves.
                None => {
                    if !s.selected.is_empty() {
                        s.selected.clear();
                        area.queue_draw();
                    }
                    pick = Some(s.indicator_at(width, height, x, y));
                }
            }
        }
        let axis = s.axis_at(x, y, width, height, false);
        let strip_from = match axis {
            Some(Axis::Strip(id)) => s.strip_window(id, s.strip_fit(id, width, height)).map(|from| (id, from)),
            _ => None,
        };
        s.drag = match (region, axis) {
            // A strip's own axis scales that strip, held from what it shows
            // once the hand moves; one with nothing to scale takes nothing.
            (Region::PriceAxis, Some(Axis::Strip(_))) => {
                strip_from.map(|(id, from)| Drag::StripScale { id, from, held: s.strip_held.contains_key(&id) })
            }
            (Region::PriceAxis, _) => {
                // Touching the axis takes the scale off automatic, from
                // exactly where it was, so nothing jumps.
                s.hold_price(width, height);
                Some(Drag::PriceScale { zoom: s.price_zoom })
            }
            (Region::TimeAxis, _) => Some(Drag::TimeScale { visible: s.columns() }),
            (Region::Plot, _) => {
                let strip = s.strip_pan_at(y, width, height);
                Some(Drag::Pan { left: s.left_edge(), offset: s.price_offset, strip })
            }
        };
        drop(s);
        notify_price_auto(&state, &on_price_auto, was_price);
        if let (Some(hit), Some(view)) = (pick, view.upgrade()) {
            view.pick(hit);
        }
    });

        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let view = Rc::downgrade(self);
        drag.connect_drag_update(move |gesture, offset_x, offset_y| {
            // A × the hand has travelled from is no longer a × to let go on.
            if offset_x.hypot(offset_y) > drag_threshold() {
                pressed_mark.borrow_mut().take();
            }
            let mut s = state.borrow_mut();
            let Some(drag) = s.drag.clone() else { return };
            let plot_w = (area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0);
            let (width, height) = (area.width() as f64, area.height() as f64);
            let hand = gesture.start_point().map(|(sx, sy)| (sx + offset_x, sy + offset_y));
            let snap = gesture.current_event_state().contains(gtk::gdk::ModifierType::CONTROL_MASK);
            s.magnet = snap;

            match drag {
                Drag::Mark { what, .. } => {
                    let Some((_, y)) = hand else { return };
                    // Gone from under the hand: filled, or cancelled elsewhere.
                    if s.mark_dragged(&what).is_none() {
                        s.drag = None;
                        drop(s);
                        pointer.queue_draw();
                        return;
                    }
                    let Some((at, _)) = s.tick_price_at(width, height, y) else { return };
                    let allowed = s.mark_allowed(&what, at);
                    s.drag = Some(Drag::Mark { what, at, allowed });
                    drop(s);
                    // A preview, on the pointer layer: the chart is as it was.
                    pointer.queue_draw();
                    return;
                }
                Drag::Place { moved } => {
                    let Some((x, y)) = hand else { return };
                    if let Some(anchor) = s.locate_with(width, height, x, y, snap)
                        && let Some(placing) = s.placing.as_mut()
                    {
                        match placing.kind.is_path() {
                            true => placing.move_last_corner(anchor),
                            false => placing.move_grip(Grip::To, anchor)
                        }
                    }
                    if !moved && offset_x.hypot(offset_y) > drag_threshold() {
                        s.drag = Some(Drag::Place { moved: true });
                    }
                    drop(s);
                    // The drawing in hand is on the pointer layer; the chart
                    // underneath has not changed.
                    pointer.queue_draw();
                    return;
                }
                Drag::Marquee { from, .. } => {
                    let Some((x, y)) = hand else { return };
                    s.drag = Some(Drag::Marquee { from, to: (x, y) });
                    let (left, right) = if from.0 <= x { (from.0, x) } else { (x, from.0) };
                    let (top, bottom) = if from.1 <= y { (from.1, y) } else { (y, from.1) };
                    let touched = s.drawings_in(width, height, (left, top, right - left, bottom - top));
                    let mut selected = s.marquee_base.clone();
                    selected.extend(touched.into_iter().filter(|i| !s.marquee_base.contains(i)));
                    let changed = selected != s.selected;
                    s.selected = selected;
                    drop(s);
                    pointer.queue_draw();
                    if changed {
                        area.queue_draw();
                    }
                    return;
                }
                Drag::Grip { index, grip, origin } => {
                    let Some((x, y)) = hand else { return };
                    // A corner snaps; the whole drawing does not, since
                    // it moves by how far the hand went, and a hand pulled
                    // onto a candle would jump it by that much.
                    let snap = snap && grip != Grip::Body;
                    let Some(now) = s.locate_with(width, height, x, y, snap) else { return };
                    let moving: Vec<Drawing> = match grip {
                        Grip::Body => {
                            // The whole selection follows the hand by the
                            // distance it has travelled since the last
                            // motion, so a drag past the end of the bars
                            // keeps moving at the bars' own step.
                            //
                            // Moved by *bars*, not by time. Time is where a
                            // drawing lives, but the chart lays bars out
                            // evenly however unevenly they are spaced, so
                            // the same number of seconds is a different
                            // number of pixels in one gap than in the next —
                            // a weekend is a column like any other. Shifting
                            // both anchors by the same span of time
                            // therefore changed the distance between them on
                            // screen as the drawing crossed one, and a box
                            // dragged over a weekend visibly breathed.
                            // Shifting both by the same number of columns
                            // cannot: columns are evenly spaced by
                            // construction.
                            let by_index = index_of_ts(&s.bars, now.ts)
                                - index_of_ts(&s.bars, origin.ts);
                            let (first, visible) = s.slice();
                            let plan = layout(&s, width, height);
                            let Some(scale) = price_range(&s, &s.bars[first..first + visible], &plan)
                            else {
                                return;
                            };
                            // Up the scale, not up the price: on a log axis
                            // the hand moves a drawing by a ratio.
                            let by_price = scale.value(now.price) - scale.value(origin.price);
                            let indices = s.selected.clone();
                            let mut moving = Vec::new();
                            let State { bars, drawings, .. } = &mut *s;
                            for i in indices {
                                if let Some(drawing) = drawings.get_mut(i) {
                                    shift_by_columns(bars, drawing, by_index, &scale, by_price);
                                    moving.push(drawing.clone());
                                }
                            }
                            s.drag = Some(Drag::Grip { index, grip, origin: now });
                            moving
                        }
                        grip => {
                            // A corner is one drawing's own.
                            let State { bars, drawings, .. } = &mut *s;
                            let Some(drawing) = drawings.get_mut(index) else {
                                return;
                            };
                            // An anchored VWAP's anchor stays on a bar the
                            // chart has, as it did when it was put down.
                            let now = match drawing.kind {
                                DrawingKind::Vwap => anchor_on_a_bar(bars, now),
                                _ => now,
                            };
                            // An edge pulled past its opposite is that side
                            // from here on.
                            let held = drawing.drag_grip(grip, now);
                            let moved = vec![drawing.clone()];
                            s.drag = Some(Drag::Grip { index, grip: held, origin });
                            moved
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
                Drag::Pan { left, offset, strip } => {
                    let columns = s.columns();
                    if columns > 0 {
                        let bar_w = plot_w / columns as f64;
                        // Dragging right reveals older bars; past either end
                        // of the series the drag opens room.
                        let shift = -(offset_x / bar_w).round() as i64;
                        let wanted = left + shift;
                        let now = s.left_edge();
                        s.pan_by((wanted - now) as f64, plot_w);
                    }
                    // Vertical panning only means something once the scale is
                    // no longer fitting itself to the data. The content follows
                    // the hand: drag down and the bars come down with it, which
                    // means the window moves up the price axis.
                    // In a strip, its own scale, once that is held, and
                    // never the price's.
                    match strip {
                        Some(StripPan { id, held: Some((low, high)), inner_h }) => {
                            let shift = offset_y / inner_h.max(1.0) * (high - low);
                            s.hold_strip(id, low + shift, high + shift);
                        }
                        Some(_) => {}
                        None if !s.price_auto => {
                            let plot_h = (area.height() as f64 - TIME_AXIS_H - PAD).max(1.0);
                            s.price_offset = offset + offset_y / plot_h / s.price_zoom;
                        }
                        None => {}
                    }
                }
                Drag::StripScale { id, from, .. } => {
                    // The same sense as the price's axis: pulling up shows
                    // less of the strip's values, larger.
                    let (low, high) = scaled_window(from, 2f64.powf(offset_y / DRAG_PER_DOUBLING));
                    s.hold_strip(id, low, high);
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
                    s.range = None;
                    s.keep_room_on_screen();
                    s.trim_room(plot_w);
                }
                Drag::Place { .. } | Drag::Grip { .. } | Drag::Marquee { .. } | Drag::Mark { .. } => {
                    unreachable!("handled above")
                }
            }
            drop(s);
            redraw(&area, &pointer);
        });

        let state = self.state.clone();
        let on_resize = self.on_pane_resize.clone();
        let on_price_auto = self.on_price_auto.clone();
        let view = Rc::downgrade(self);
        drag.connect_drag_end(move |_, offset_x, offset_y| {
            let finished = state.borrow_mut().drag.take();
            // Stored when the drag ends rather than on every motion event,
            // which would be a database write per pixel of travel.
            match finished {
                // Let go: said, if it may go there, and otherwise nothing.
                Some(Drag::Mark { what, at, allowed }) => {
                    let Some(view) = view.upgrade() else { return };
                    view.pointer.queue_draw();
                    let from = state.borrow().mark_dragged(&what).map(|dragged| dragged.from);
                    // A tab pressed and let go where it was places nothing:
                    // it would be at its mark's own price.
                    let travelled = offset_x.hypot(offset_y) > drag_threshold();
                    let event = match (what, from) {
                        (_, None) => return,
                        _ if !allowed => return,
                        (Grab::Tab { .. }, Some(_)) if !travelled => return,
                        (Grab::Tab { mark, tab }, Some(_)) => MarkEvent::Tab { mark, tab, price: at },
                        // Back where it was: nothing to move.
                        (Grab::Move(_), Some(from)) if (from - at).abs() < 1e-9 => return,
                        (Grab::Move(mark), Some(_)) => MarkEvent::Move { mark, price: at },
                    };
                    view.tell_mark(event);
                    return;
                }
                // Pressed and released in place: the first anchor is down
                // and the second waits for the next press.
                Some(Drag::Place { moved: false }) => return,
                // The box is gone with the drag; what it took stays taken.
                Some(Drag::Marquee { .. }) => {
                    state.borrow_mut().marquee_base.clear();
                    if let Some(view) = view.upgrade() {
                        view.pointer.queue_draw();
                    }
                    return;
                }
                Some(Drag::Place { moved: true }) => {
                    if let Some(view) = view.upgrade() {
                        view.commit_placing();
                    }
                    return;
                }
                Some(Drag::Grip { index, grip, .. }) => {
                    let moved: Vec<Drawing> = {
                        let mut s = state.borrow_mut();
                        // Let go of where the hand still is: lit as under it,
                        // no longer as held.
                        s.hover_grip = (grip != Grip::Body).then_some((index, grip));
                        if let Some(view) = view.upgrade() {
                            view.area.set_cursor_from_name(Some(s.grip_cursor(index, grip, false)));
                        }
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
                        view.area.queue_draw();
                        for drawing in moved {
                            view.tell(DrawingEvent::Changed(drawing));
                        }
                    }
                    return;
                }
                // A held scale dragged up or down, or stretched: told once,
                // as it is let go of, against where the drag picked it up.
                Some(Drag::Pan { offset, strip, .. }) => {
                    let mut was = state.borrow().scales();
                    match strip {
                        Some(StripPan { id, held: Some(from), .. }) => {
                            was.strips.insert(id, from);
                        }
                        Some(_) => {}
                        None => was.price.2 = offset,
                    }
                    notify_price_auto(&state, &on_price_auto, was);
                    return;
                }
                Some(Drag::PriceScale { zoom }) => {
                    let mut was = state.borrow().scales();
                    was.price.1 = zoom;
                    notify_price_auto(&state, &on_price_auto, was);
                    return;
                }
                Some(Drag::StripScale { id, from, held }) => {
                    let mut was = state.borrow().scales();
                    match held {
                        true => was.strips.insert(id, from),
                        false => was.strips.remove(&id),
                    };
                    notify_price_auto(&state, &on_price_auto, was);
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
        // out of any scale you have stretched into uselessness; double-clicking
        // a drawing puts the caret in it.
        let click = gtk::GestureClick::new();
        let state = self.state.clone();
        let area = self.area.clone();
        let pointer = self.pointer.clone();
        let on_price_auto = self.on_price_auto.clone();
        let view = Rc::downgrade(self);
        click.connect_pressed(move |_, presses, x, y| {
            if presses < 2 || state.borrow().ruler_armed {
                return;
            }
            // The press that made this a double-click already ended a
            // zig-zag on its last corner; the drawing it left is not one to
            // start typing into.
            if std::mem::take(&mut state.borrow_mut().closed_by_press) {
                return;
            }
            // A zig-zag being laid down: the second click is the end of it.
            // Before everything else, because while one is in hand that is
            // the only thing a double-click can mean.
            if state.borrow().placing.as_ref().is_some_and(|d| d.kind.is_path()) {
                if let Some(view) = view.upgrade() {
                    view.commit_placing();
                }
                return;
            }
            let region = region_at(x, y, area.width() as f64, area.height() as f64);
            // A drawing under the pointer: the second click opens its
            // properties, as it does on an indicator, so one gesture means
            // one thing across everything on the chart. Text is the
            // exception — its words are what there is to change, so the
            // caret goes in it. A label on any other drawing is Enter or F2.
            if region == Region::Plot {
                let hit = {
                    let s = state.borrow();
                    s.drawing_at(area.width() as f64, area.height() as f64, x, y)
                        .map(|(index, _)| (index, s.drawings.get(index).is_some_and(|d| d.kind.is_text())))
                };
                if let Some((index, text)) = hit
                    && let Some(view) = view.upgrade()
                {
                    view.select_at(x, y);
                    match text {
                        true => {
                            view.begin_text_edit(index, false);
                        }
                        false => {
                            if let Some(handler) = view.on_drawing_properties.borrow().as_ref() {
                                handler();
                            }
                        }
                    }
                    return;
                }
                // An indicator has no words to type, so its second click
                // opens what can be changed about it, as a double-click on
                // its name in the legend does.
                if let Some(view) = view.upgrade()
                    && let Some(id) = view.indicator_at(x, y)
                {
                    view.pick(Some(id));
                    view.tell_indicator(IndicatorEvent::Properties(id));
                    return;
                }
            }
            let mut s = state.borrow_mut();
            let was_price = s.scales();
            match region {
                // The price's axis or a strip's: every scale back to
                // fitting what is on screen, the price's and the strips'.
                Region::PriceAxis => {
                    s.free_price();
                    s.free_strips();
                }
                Region::TimeAxis => {
                    s.visible = 160;
                    s.anchored = true;
                    s.overhang = RIGHT_MARGIN;
                    s.overhang_by_hand = false;
                    s.lead = 0;
                }
                // Double-clicking bare chart does nothing, the same as
                // everywhere else — a drawing was handled above. Resetting
                // is Ctrl+Esc or the axis menu.
                Region::Plot => return,
            }
            drop(s);
            redraw(&area, &pointer);
            notify_price_auto(&state, &on_price_auto, was_price);
        });
        self.area.add_controller(click);
    }

    /// Put the price scale back to fitting the data.
    pub fn reset_price_scale(&self) {
        self.state.borrow_mut().free_price();
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
            // A drawing half laid down: the right button drops it and keeps
            // the tool for another go, and that is all it does. A menu then
            // is a menu nobody reached for. With nothing laid down the menus
            // are what they always are, and the tool stays in hand either
            // way: Escape is what puts it down.
            if let Some(view) = view.upgrade()
                && view.drop_placing()
            {
                return;
            }
            if region_at(x, y, width, height) != Region::PriceAxis {
                // The menu is about what is under the pointer: a drawing
                // there is selected and gets a menu of its own, about the
                // drawing and nothing else.
                if let Some(view) = view.upgrade()
                    && view.select_at(x, y)
                {
                    if let Some(handler) = view.on_drawing_menu.borrow().as_ref() {
                        handler(x, y);
                    }
                    return;
                }
                // Then an indicator, which gets a menu about itself the
                // same way.
                if let Some(view) = view.upgrade() {
                    let hit = view.indicator_at(x, y);
                    view.pick(hit);
                    if let Some(id) = hit {
                        view.tell_indicator(IndicatorEvent::Menu(id, x, y));
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
/// automatic, or moved a held one, or a strip's own scale.
///
/// Compared against what it `was` (see [`State::scales`]) rather than
/// reported outright, because a pan or a time zoom passes through here too
/// and changes neither. Called with the state no longer borrowed: the
/// handler is the window's, and the window reads the chart back.
fn notify_price_auto(
    state: &Rc<RefCell<State>>,
    on_price_auto: &Handler<dyn Fn(bool)>,
    was: Scales,
) {
    let now = state.borrow().scales();
    if now != was && let Some(handler) = on_price_auto.borrow().as_ref() {
        handler(now.price.0);
    }
}

fn notify_hover(
    state: &Rc<RefCell<State>>,
    on_hover: &Handler<dyn Fn(Option<Hover>)>,
    area: &gtk::DrawingArea,
) {
    let hover = {
        let s = state.borrow();
        let (_, visible) = s.slice();
        match (s.pointer, visible) {
            (Some((x, y)), v) if v > 0 => {
                let plot_w = (area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0);
                let frac = (x - PAD) / plot_w;
                if (0.0..=1.0).contains(&frac) {
                    let (first, visible, lead) = s.view();
                    let index = (first as f64 - lead as f64 + frac * s.columns() as f64)
                        .floor()
                        .max(0.0) as usize;
                    let plan = layout(&s, area.width() as f64, area.height() as f64);
                    let range = price_range(&s, &s.bars[first..first + visible], &plan);
                    s.bars.get(index.min(s.bars.len().saturating_sub(1))).map(|bar| Hover {
                        bar: *bar,
                        index,
                        price: match range {
                            Some(scale) => scale.price_at(y, plan.price_y, plan.price_h),
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

/// Where everything goes in one frame: the stack of rows, the columns the
/// bars are drawn in, and the price scale. Worked out once here, so what a
/// click lands on is what was drawn there.
struct Frame<'a> {
    plan: Layout,
    columns: Columns<'a>,
    scale: PriceScale,
    /// The tallest bar of the volume strip.
    max_volume: f64,
}

fn frame(state: &State, width: f64, height: f64) -> Option<Frame<'_>> {
    let (first, visible, lead) = state.view();
    if visible == 0 {
        return None;
    }
    let bars = &state.bars[first..first + visible];
    let plan = layout(state, width, height);

    // What will actually be drawn: the visible bars themselves at any normal
    // zoom, and one aggregate per pixel column past that. They take their
    // own columns of the plot, after whatever room was dragged open before
    // the first bar and leaving empty what was opened past the last.
    let column_w = plan.plot_w / state.columns().max(1) as f64;
    let bars_x = plan.plot_x + lead as f64 * column_w;
    let mut columns = Columns::of(bars, first, visible, bars_x, column_w * visible as f64);
    columns.end = plan.plot_x + plan.plot_w;

    let max_volume = columns.bars.iter().fold(0.0_f64, |most, b| most.max(b.volume));
    // Off the real bars rather than the columns. The two agree, because a
    // column keeps the highest high and the lowest low of what it stands for —
    // and the crosshair works the same range out from the same bars on its own
    // layer, where going through the columns would be work for nothing.
    let scale = price_range(state, bars, &plan)?;
    Some(Frame { plan, columns, scale, max_volume })
}

fn draw(cr: &cairo::Context, width: f64, height: f64, state: &State) {
    let ui = &state.theme.ui;

    colors::set_source(cr, &ui.background);
    let _ = cr.paint();

    let Some(Frame { plan, columns, scale, max_volume }) = frame(state, width, height) else {
        draw_placeholder(cr, width, height, state);
        return;
    };
    let (plot_x, plot_w, price_y, price_h) = (plan.plot_x, plan.plot_w, plan.price_y, plan.price_h);
    let bars_x = columns.plot_x;

    let to_y = |price: f64| scale.to_y(price, price_y, price_h);
    let bar_w = columns.bar_w;

    // One answer for how precisely a price is written, used by the last-price
    // chip and the crosshair alike — a chart saying 1.1257 on one label and
    // 1.13 on another is describing two different prices. The gridlines are
    // round numbers and go by their own step.
    let step = scale.step(price_h);
    let kind = state.instrument.as_ref().map(|i| i.kind);
    let (low, high) = scale.prices();
    let decimals = omacharts_engine::price_decimals(step, (low + high) / 2.0, kind);

    // The ground first: session times shaded down the whole stack and under
    // the gridlines, so they read as the chart's ground rather than as
    // something drawn on it.
    draw_time_ground(cr, state, &columns, &plan);
    let compared = state.compare.as_deref().and_then(|overlay| Some((overlay, overlay.based(&state.bars, columns.first)?)));
    match &compared {
        Some((_, based)) => compare::draw_grid(cr, state, based, plot_x, plot_w, price_y, price_h, &scale, &to_y),
        None => draw_price_grid(cr, state, plot_x, plot_w, price_y, price_h, &scale, &to_y),
    }
    draw_time_axis(cr, state, &columns.bars, columns.bars_each(), plot_x, plot_w, height, bar_w, bars_x - columns.shift());
    draw_goto_day(cr, state, &columns, &plan, height);
    // Shaded things go under the candles; lines go over. A band drawn on top
    // of the bars hides the thing it is describing.
    //
    // All of it inside the price box, the way drawings are. The scale fits the
    // bars, not the overlays, so a 200-day average far below them — or any
    // bar once the scale is dragged — would otherwise run over the time axis
    // and the strips beneath.
    cr.save().ok();
    cr.rectangle(plot_x, price_y, plot_w, price_h);
    cr.clip();
    draw_indicator_fills(cr, state, &columns, &to_y);
    draw_drawings(cr, state, &plan, &scale, true);
    // A line of the chart's own goes over the compared ones, so it is drawn
    // with them instead.
    if !(state.drawn_as_line() && compared.is_some()) {
        draw_candles(cr, state, &columns, &to_y);
    }
    draw_indicator_lines(cr, state, &columns, &to_y);
    if let Some((overlay, based)) = &compared {
        compare::draw_lines(cr, state, overlay, based, &columns, &plan, &to_y);
    }
    draw_unmet(cr, state, plot_x, price_y + price_h);
    cr.restore().ok();
    draw_drawings(cr, state, &plan, &scale, false);

    for (at, row) in plan.rows.iter().enumerate() {
        draw_row_edge(cr, state, &plan, at);
        let Some(id) = row.pane else { continue };
        let Some(drawn) = state.indicators.iter().find(|d| d.indicator.id == id) else {
            continue;
        };
        // A strip held on a scale of its own can be zoomed past what it
        // draws, which is then kept to its own box rather than drawn over
        // the next one.
        let held = state.strip_held.contains_key(&id);
        if held {
            cr.save().ok();
            cr.rectangle(0.0, row.top, width, row.height);
            cr.clip();
        }
        match &drawn.output {
            Output::Volume { .. } => {
                let fit = (max_volume > 0.0).then_some((0.0, max_volume));
                if let Some(window) = state.strip_window(id, fit) {
                    draw_volume(cr, state, &drawn.indicator, &columns, row.top, row.height, window);
                }
                // Named like the others now that it is one strip among several.
                // Three unlabelled boxes under a chart is a puzzle.
                draw_pane_name(cr, state, drawn, plot_x, row.top);
            }
            Output::Pane(pane) => {
                draw_pane(cr, state, pane, drawn, &columns, plot_w, row.top, row.height, width)
            }
            _ => {}
        }
        if held {
            cr.restore().ok();
        }
    }

    draw_axis_fade(cr, state, plot_x + plot_w, width);
    let tag_at = draw_marks(cr, state, state.lane(&plan), price_y, price_y + price_h, width, &to_y);
    match &compared {
        Some((overlay, based)) => {
            let step = compare::grid_step(&scale, based.rebase, price_h);
            compare::draw_end_labels(cr, state, overlay, based, &columns, &plan, width, step, &to_y);
        }
        None => draw_last_price(cr, state, plot_x, plot_w, price_y, width, decimals, &to_y, tag_at),
    }

    if state.trouble.is_some() {
        draw_trouble_dot(cr, state, width);
    }
    draw_range(cr, state, width, height);
}

/// The day typed into Go to, while the chart holds the window it made: the
/// day tinted down the whole stack and named on the time axis in the
/// accent. Near now the window ends at now and starts before the day, so
/// without this nothing on the chart says where the day is. It goes with
/// the range, when a hand moves the view.
fn draw_goto_day(cr: &cairo::Context, state: &State, columns: &Columns, plan: &Layout, height: f64) {
    let Some(Range::Lasting(typed, ..)) = state.range else { return };
    let utc = state.timeframe.is_calendar();
    let date = |bar: &Bar| state.clock.read(bar.ts, utc).map(|at| at.date());
    let Some(today) = state.bars.last().and_then(date) else { return };
    let Some(day) = typed.first(today) else { return };
    let from = state.bars.partition_point(|bar| date(bar).is_some_and(|d| d < day));
    let to = state.bars.partition_point(|bar| date(bar).is_some_and(|d| d <= day));
    // A bar longer than a day holds days it does not start on: a Tuesday
    // is the week that started on the Monday.
    let holds = |bar: &Bar| {
        let end = state.clock.read(state.timeframe.shift(bar.ts, 1), utc);
        state.timeframe.seconds() > 86_400 && end.is_some_and(|end| end.date() > day)
    };
    let from = match from.checked_sub(1) {
        Some(before) if from == to && state.bars.get(before).is_some_and(holds) => before,
        _ => from,
    };
    let (left, right) = (plan.plot_x, plan.plot_x + plan.plot_w);
    let (x0, x1) = (bar_left(columns, from), bar_left(columns, to));
    if x1 < left || x0 > right {
        return;
    }
    let accent = &state.theme.ui.accent;
    // Whole pixels, as a session's shading is.
    let (band_left, band_right) = (x0.max(left).round(), x1.min(right).round());
    if band_right > band_left {
        colors::set_source_alpha(cr, accent, 0.14);
        cr.rectangle(band_left, plan.top, band_right - band_left, plan.total_h);
        let _ = cr.fill();
    }
    // Named as the card names it, over the middle of the day.
    let label = Range::Day(typed).spelled(today);
    label_on_time_axis(cr, &label, (x0 + x1) / 2.0, left, plan.plot_w, height, accent);
}

/// The range the view is holding to, in the corner under the price axis,
/// for as long as it holds: "YTD" there is what tells a chart showing the
/// year to date from one that happens to be zoomed to about that much. It
/// goes when a hand moves the view, which is when it stops being true.
fn draw_range(cr: &cairo::Context, state: &State, width: f64, height: f64) {
    let Some(range) = state.range else { return };
    // The window's control's corner, on the chart it stands over.
    if state.axis_corner_taken {
        return;
    }
    let year = state.bars.last().and_then(|bar| state.clock.read(bar.ts, state.timeframe.is_calendar()));
    let text = match year {
        Some(at) => range.short_key(chrono::Datelike::year(&at)),
        None => range.key(),
    };
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(11.0);
    let Ok(extents) = cr.text_extents(&text) else { return };
    let x = width - PRICE_AXIS_W / 2.0 - extents.width() / 2.0 - extents.x_bearing();
    colors::set_source(cr, &state.theme.ui.accent);
    cr.move_to(x.round(), height - 7.0);
    let _ = cr.show_text(&text);
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
    let (first, visible, lead) = state.view();
    if visible == 0 {
        return;
    }
    let bars = &state.bars[first..first + visible];
    let plan = layout(state, width, height);
    let Some(scale) = price_range(state, bars, &plan) else { return };
    let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
    let bar_w = plan.plot_w / state.columns().max(1) as f64;
    // Where the bars begin: after the room before the first, if any.
    let bars_x = plan.plot_x + lead as f64 * bar_w;

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
        // On the last corner of a zig-zag a press ends it, so it is drawn as
        // it will be left — without the leg following the hand — with that
        // corner lit.
        let closing = state.pointer.is_some_and(|(x, y)| state.closes_path(width, height, x, y));
        let shown = match closing {
            true => {
                let mut finished = placing.clone();
                finished.finish_path();
                Cow::Owned(finished)
            }
            false => Cow::Borrowed(placing),
        };
        let lit = closing.then(|| (Grip::Corner(shown.points.len().saturating_sub(1)), Lit::Hover));
        let placed = state.project(&plan, &scale, &shown);
        let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
        draw_drawing(cr, state, &placed, &shown, true, false, lit, &to_y);
        cr.restore().ok();
    }

    if let Some((from, to, _)) = state.ruler {
        draw_ruler(cr, state, &plan, &scale, from, to);
    }

    if let Some(Drag::Mark { what, at, allowed }) = &state.drag {
        draw_mark_preview(cr, state, &plan, width, what, *at, *allowed, &to_y);
    }

    // The selection box, while a hand drags one out.
    if let Some(Drag::Marquee { from, to }) = state.drag {
        let (x, y) = (from.0.min(to.0), from.1.min(to.1));
        let (w, h) = ((from.0 - to.0).abs(), (from.1 - to.1).abs());
        cr.save().ok();
        cr.rectangle(plan.plot_x, plan.price_y, plan.plot_w, plan.price_h);
        cr.clip();
        colors::set_source_alpha(cr, &state.theme.ui.accent, 0.12);
        cr.rectangle(x, y, w, h);
        let _ = cr.fill();
        colors::set_source_alpha(cr, &state.theme.ui.accent, 0.7);
        cr.set_line_width(1.0);
        cr.rectangle(x.round() + 0.5, y.round() + 0.5, w.round(), h.round());
        let _ = cr.stroke();
        cr.restore().ok();
    }

    if let Some((px, py)) = state.pointer {
        // With the magnet on, the crosshair goes where the anchor will: onto
        // the candle, if the pointer is over the price plot.
        let over_price = py >= plan.price_y && py <= plan.price_y + plan.price_h;
        let cross_y = match state.snapping() && over_price {
            true => state.locate_with(width, height, px, py, true).map_or(py, |a| to_y(a.price)),
            false => py,
        };
        draw_crosshair(
            cr, state, px, cross_y, plan.plot_x, plan.plot_w, plan.top, plan.price_y, plan.price_h,
            width, height, bar_w, first as i64 - lead as i64, &scale,
        );
        draw_pane_value(cr, state, &plan, px, py, first, visible, width);
    }

    if let Some(overlay) = state.compare.as_deref() {
        let hovered = state
            .pointer
            .filter(|(px, _)| *px >= plan.plot_x && *px <= plan.plot_x + plan.plot_w)
            .map(|(px, _)| ((px - bars_x) / bar_w).floor())
            .filter(|at| *at >= 0.0)
            .map(|at| first + at as usize);
        compare::draw_pointer(cr, state, overlay, &plan, &scale, first, visible, hovered, bar_w, bars_x);
    }

    // Another chart's pointer, if nothing is pointing at this one. The real
    // crosshair wins: an echo under your own pointer is a second line saying
    // the same thing.
    if state.pointer.is_none()
        && let Some(echo) = state.echo
    {
        draw_echo(
            cr, state, bars, bars_x, plan.plot_w, bar_w, plan.top, plan.price_y,
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
    let text = match &state.trouble_words {
        Some(words) if state.instrument.is_some() && !state.loading && state.trouble.is_some() => words.as_str(),
        _ => placeholder_text(state.instrument.is_some(), state.loading, state.trouble),
    };
    colors::set_source_alpha(cr, &state.theme.ui.text_muted, PLACEHOLDER_ALPHA);
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(13.0);
    if let Ok(extents) = cr.text_extents(text) {
        cr.move_to((width - extents.width()) / 2.0, height / 2.0);
        let _ = cr.show_text(text);
        if let Some(fraction) = state.progress.filter(|_| state.loading && state.instrument.is_some()) {
            draw_progress(cr, width / 2.0, height / 2.0, extents.width(), fraction, &state.theme.ui.text_muted);
        }
    }
}

/// How strongly the placeholder's words are drawn, in their muted colour:
/// the progress bar under them is the same.
const PLACEHOLDER_ALPHA: f64 = 0.8;

/// The progress bar under Loading…, all of its look in one place: as tall,
/// as much wider than the words, as far under their baseline, and its track
/// as faint a share of the words' tone as these say. Its ends are round,
/// and so is the tip of its fill.
const PROGRESS_HEIGHT: f64 = 2.0;
const PROGRESS_WIDER_THAN_TEXT: f64 = 1.2;
const PROGRESS_BELOW_BASELINE: f64 = 11.0;
const PROGRESS_TRACK_SHARE: f64 = 0.3;

/// How the bar moves towards where the load has got: the time it takes to
/// close all but an e-th of the way, and the gap under which it is there
/// and stops, a quarter of a pixel on a bar the width of a long word.
const PROGRESS_EASE_SECONDS: f64 = 0.18;
const PROGRESS_SETTLED: f64 = 1.0 / 400.0;

/// The bar of a load `fraction` done, centred on `centre` under words
/// `text_width` wide whose baseline is at `baseline`, in the words' colour
/// `tone`: filled with it as they are drawn, over a track of the same tone
/// fainter.
///
/// Track and fill are each one stroke as thick as the bar with round caps,
/// along the middle of a whole row of pixels, so the long edges land on
/// pixel boundaries and only the caps are softened. The fill is never
/// shorter than it is tall: at nothing done it is a dot at the left end.
fn draw_progress(cr: &cairo::Context, centre: f64, baseline: f64, text_width: f64, fraction: f64, tone: &str) {
    let width = (text_width * PROGRESS_WIDER_THAN_TEXT).round();
    let (x, y) = ((centre - width / 2.0).round(), (baseline + PROGRESS_BELOW_BASELINE).round());
    let radius = PROGRESS_HEIGHT / 2.0;
    let middle = y + radius;
    let pill = |length: f64| {
        cr.move_to(x + radius, middle);
        cr.line_to(x + length - radius, middle);
        let _ = cr.stroke();
    };
    let _ = cr.save();
    cr.set_line_width(PROGRESS_HEIGHT);
    cr.set_line_cap(cairo::LineCap::Round);
    colors::set_source_alpha(cr, tone, PLACEHOLDER_ALPHA * PROGRESS_TRACK_SHARE);
    pill(width);
    colors::set_source_alpha(cr, tone, PLACEHOLDER_ALPHA);
    pill((width * fraction.clamp(0.0, 1.0)).round().max(PROGRESS_HEIGHT));
    let _ = cr.restore();
}

#[allow(clippy::too_many_arguments)]
fn draw_price_grid(
    cr: &cairo::Context,
    state: &State,
    plot_x: f64,
    plot_w: f64,
    plot_y: f64,
    price_h: f64,
    scale: &PriceScale,
    to_y: &impl Fn(f64) -> f64,
) {
    let step = scale.step(price_h);
    if step <= 0.0 {
        return;
    }
    // Gridlines are written to their own step, not to how the market quotes:
    // a line at 7,640 is exactly 7,640, and "7,640.00" down a whole column
    // is noise. The quote's precision belongs to the labels that point at an
    // actual price — the last-price chip and the crosshair.
    let decimals = decimals_for(step);

    cr.set_line_width(1.0);
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);

    scale.ticks(price_h, |price| {
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
            let _ = cr.show_text(&grouped_decimals(price, decimals));
        }
    });

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
    per: f64,
    plot_x: f64,
    plot_w: f64,
    height: f64,
    bar_w: f64,
    bars_x: f64,
) {
    let y = height - TIME_AXIS_H;
    colors::set_source(cr, &state.theme.ui.axis);
    cr.set_line_width(1.0);
    cr.move_to(plot_x, y.round() + 0.5);
    cr.line_to(plot_x + plot_w, y.round() + 0.5);
    let _ = cr.stroke();

    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);

    // The columns across the whole plot, the empty room at either end
    // included: past the last bar a tick is the time that column would be,
    // so the axis keeps reading as the chart is dragged into the future.
    // Before the first there is no history to date.
    let before = ((bars_x - plot_x) / bar_w).round().max(0.0) as i64;
    // What rounding to a column left over: the columns sit off the grid by
    // the part of one scrolled past the left edge.
    let nudge = bars_x - plot_x - before as f64 * bar_w;
    let total = (plot_w / bar_w).round().max(1.0) as i64;
    // A column zoomed out far enough stands for `per` bars, so the room past
    // the data steps by that many too, or its dates crawl.
    let step = (state.timeframe.seconds() as f64 * per).round() as i64;
    let time_at = |column: i64| axis_time(bars, state.timeframe, per, column - before);
    let utc = state.timeframe.is_calendar();
    let (ticks, tick_seconds) = axis_ticks(total, plot_w, step, utc, state.clock, time_at);
    // Each label waits for the next before it is drawn, so that one pushed
    // in from the left edge into its neighbour is the one left out, as at
    // the right edge, and not the neighbour sitting on its own gridline.
    // Held: its left edge, its right edge, whether it was pushed, the text.
    let mut held: Option<(f64, f64, bool, String)> = None;
    let show = |left: f64, label: &str| {
        colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.9);
        cr.move_to(left, height - 7.0);
        let _ = cr.show_text(label);
    };
    for column in ticks {
        let Some(ts) = time_at(column) else { continue };
        let x = plot_x + nudge + (column as f64 + 0.5) * bar_w;
        if x < plot_x + 18.0 || x > plot_x + plot_w - 18.0 {
            continue;
        }
        if state.show_grid {
            colors::set_source(cr, &state.theme.ui.grid);
            cr.move_to(x.round() + 0.5, PAD);
            cr.line_to(x.round() + 0.5, y);
            let _ = cr.stroke();
        }

        let label = format_axis_time(ts, tick_seconds, state.timeframe.is_intraday(), utc, state.clock);
        let Ok(extents) = cr.text_extents(&label) else { continue };
        // Centred on its tick, but kept inside the chart: on a narrow chart
        // a long intraday label centred near the left edge ran off it and
        // was cut in half.
        let w = extents.width();
        let left = inside_plot(x, w, plot_x, plot_w);
        // Under the drawing tools' toggle, on the chart it stands over: the
        // date is left out rather than moved off its tick, and only when it
        // would touch the toggle.
        if left < state.axis_start_taken {
            continue;
        }
        let pushed = left != x - w / 2.0;
        if let Some((_, right, held_pushed, _)) = held
            && left < right + 8.0
        {
            if !held_pushed {
                continue;
            }
            held = None;
        }
        if let Some((left, _, _, label)) = held.replace((left, left + w, pushed, label)) {
            show(left, &label);
        }
    }
    if let Some((left, _, _, label)) = held {
        show(left, &label);
    }
}

/// Where a label `w` wide that belongs at `x` on the time axis starts:
/// centred on it, but pushed in from either edge so it reads in full, as the
/// tick labels and the crosshair's own both are.
///
/// Split a window enough times and the label is wider than the chart it
/// belongs to, which puts the right edge left of the left one. `clamp`
/// panics on that, inside a draw callback, which does not unwind — so the
/// whole app would go rather than one chart drawing badly for a moment. The
/// left edge wins instead.
fn inside_plot(x: f64, w: f64, plot_x: f64, plot_w: f64) -> f64 {
    (x - w / 2.0).min(plot_x + plot_w - w).max(plot_x)
}

/// The moment the time axis gives the column `at`, counted from the first
/// one drawn: its own time over the data, and in the empty room past it the
/// time that column would be, `per` bars on from the last column.
///
/// None before the first column: the series has no history there, and
/// counting back a timeframe a bar, closed hours and all, named a stretch it
/// never had, spaced unlike the bars beside it.
///
/// By the column, not the bar: once a column stands for several bars, a
/// bar's time to a column made the room's dates crawl, so that a year of
/// it read as a fraction of one and its labels sat out of step with the
/// data's.
fn axis_time(bars: &[Bar], timeframe: Timeframe, per: f64, at: i64) -> Option<i64> {
    let at = usize::try_from(at).ok()?;
    if let Some(bar) = bars.get(at) {
        return Some(bar.ts);
    }
    if timeframe.seconds() <= 0 {
        return None;
    }
    let last = bars.last()?;
    let steps = ((at + 1 - bars.len()) as f64 * per.max(1.0)).round() as i64;
    Some(timeframe.shift(last.ts, steps))
}

/// Up and down candles in two passes, each building one path for wicks and one
/// for bodies.
fn draw_candles(cr: &cairo::Context, state: &State, columns: &Columns, to_y: &impl Fn(f64) -> f64) {
    if state.bars_hidden {
        return;
    }
    if state.drawn_as_line() {
        compare::draw_own_line(cr, state, columns, to_y);
        return;
    }
    candles(cr, state.bar_style, state.ohlc_thickness, &state.scheme, columns, to_y);
}

/// Candles, from nothing but what it takes to draw one: the style, the
/// scheme, and the columns they are drawn as.
///
/// Split out of the chart's own state so that what draws a candle is a
/// routine taking what a candle needs, rather than a method reaching into
/// a `State` for two fields out of thirty.
fn candles(
    cr: &cairo::Context,
    bar_style: BarStyle,
    ohlc_thickness: u8,
    scheme: &BarScheme,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    if bar_style == BarStyle::Ohlc {
        ohlc(cr, scheme, columns, to_y, ohlc_thickness);
        return;
    }
    let (bars, bar_w) = (&columns.bars, columns.bar_w);
    // Below about three pixels a candle is a line; outlining it just muddies
    // the colour.
    let hairline = bar_w < 3.0;

    for rising in [false, true] {
        let (outline, fill) = if rising {
            (&scheme.up, &scheme.up_fill)
        } else {
            (&scheme.down, &scheme.down_fill)
        };

        // A hollow body shows what is behind it, so a wick drawn through it
        // would be seen inside: it stops at the body's edges instead.
        let hollow = !hairline && colors::is_transparent(fill);

        // Wicks.
        cr.set_line_width(1.0);
        colors::set_source(cr, outline);
        let mut any = false;
        for (i, bar) in bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising {
                continue;
            }
            any = true;
            let x = columns.x(i).round() + 0.5;
            let body = hollow.then(|| {
                let top = to_y(bar.open.max(bar.close)).round();
                (top, to_y(bar.open.min(bar.close)).round().max(top + 1.0))
            });
            for (high, low) in columns.wicks(i) {
                let Some((top, bottom)) = body else {
                    draw_wick(cr, x, columns.tile(i), to_y(high), to_y(low));
                    continue;
                };
                let (high, low) = (to_y(high).round(), to_y(low).round());
                if high < top {
                    cr.move_to(x, high);
                    cr.line_to(x, top.min(low));
                }
                if low > bottom {
                    cr.move_to(x, bottom.max(high));
                    cr.line_to(x, low);
                }
            }
        }
        if any {
            stroke_wicks(cr, columns);
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
            let (x, w) = columns.body(i);
            let top = to_y(bar.open.max(bar.close)).round();
            let bottom = to_y(bar.open.min(bar.close)).round();
            cr.rectangle(x, top, w, (bottom - top).max(1.0));
        }
        if hollow {
            // Hollow: stroke the outline and leave the background showing.
            colors::set_source(cr, outline);
            let _ = cr.stroke();
        } else {
            colors::set_source(cr, fill);
            let _ = cr.fill();
        }
    }
}

/// A bar's high-low line, a pixel long at least: when the two round to the
/// same row, a line from there to itself draws nothing and the bar vanishes.
///
/// Across the column's whole pixels instead, as a box, where its columns
/// [tile](Columns::tile); [`stroke_wicks`] then fills rather than strokes.
fn draw_wick(cr: &cairo::Context, x: f64, tile: Option<(f64, f64)>, high: f64, low: f64) {
    let top = high.round();
    let bottom = low.round().max(top + 1.0);
    match tile {
        Some((left, width)) => cr.rectangle(left, top, width, bottom - top),
        None => {
            cr.move_to(x, top);
            cr.line_to(x, bottom);
        }
    }
}

/// Put down the wicks [`draw_wick`] traced for these columns.
fn stroke_wicks(cr: &cairo::Context, columns: &Columns) {
    let _ = match columns.sub_pixel() {
        true => cr.fill(),
        false => cr.stroke(),
    };
}

/// Open and close as ticks either side of a high-low line.
fn ohlc(cr: &cairo::Context, scheme: &BarScheme, columns: &Columns, to_y: &impl Fn(f64) -> f64, thickness: u8) {
    let tick = (columns.bar_w * 0.32).clamp(1.0, 10.0);
    let ticked = tick > 1.0;
    let width = ohlc_width(thickness, columns.bar_px, ticked);
    // On a pixel's edge for an even width and through its middle for an
    // odd one, so the line covers whole pixels and stays crisp.
    let centre = if width as i64 % 2 == 1 { 0.5 } else { 0.0 };
    // Showing as much past the line as a thin line's did, a pixel at the
    // least, and ending on a pixel's edge; short of the next bar's tick.
    let reach = (width / 2.0 + (tick - 0.5).max(1.0)).min(((columns.bar_px - 1.0) / 2.0).max(1.0));
    let reach = (reach - centre).round() + centre;
    cr.set_line_width(width);
    for rising in [false, true] {
        let direction = if rising { Direction::Up } else { Direction::Down };
        let mut any = false;
        for (i, bar) in columns.bars.iter().enumerate() {
            if (bar.close >= bar.open) != rising {
                continue;
            }
            any = true;
            let x = columns.x(i).round() + centre;
            for (high, low) in columns.wicks(i) {
                draw_wick(cr, x, columns.tile(i), to_y(high), to_y(low));
            }
            if ticked {
                let open = to_y(bar.open).round() + centre;
                cr.move_to(x - reach, open);
                cr.line_to(x, open);
                let close = to_y(bar.close).round() + centre;
                cr.move_to(x, close);
                cr.line_to(x + reach, close);
            }
        }
        if any {
            colors::set_source(cr, scheme.outline(direction));
            stroke_wicks(cr, columns);
        }
    }
    cr.set_line_width(1.0);
}

/// How wide an OHLC bar's line is drawn: as thick as asked while there is
/// room, and thinner where there is not, down to a pixel. Room is a pixel's
/// gap to the next bar, and with ticks a pixel of tick showing either side
/// as well, so neighbours never merge. Whole pixels, so its edges stay crisp.
fn ohlc_width(thickness: u8, bar_px: f64, ticked: bool) -> f64 {
    let spare = if ticked { 3.0 } else { 1.0 };
    f64::from(thickness).min(bar_px.floor() - spare).max(1.0)
}

fn draw_volume(
    cr: &cairo::Context,
    state: &State,
    indicator: &Indicator,
    columns: &Columns,
    top: f64,
    height: f64,
    (low, high): (f64, f64),
) {
    // A tenth of the strip clear at the top, so the tallest bar does not
    // reach the line above it. Bars run under the name, which is short.
    // Bars stand on nought, which is the foot of the strip while it fits
    // itself, from nought to the tallest bar; zoomed, it is wherever nought
    // falls on the strip's own scale.
    let inner_top = top + height * 0.1;
    let inner_h = height * 0.9;
    let to_y = |volume: f64| inner_top + inner_h * (high - volume) / (high - low).max(f64::EPSILON);
    let base = to_y(0.0);
    let (up, down) = volume_colours(indicator, &state.theme, &state.scheme);
    let opacity = volume_opacity(indicator);
    let lit = state.picked == Some(indicator.id);
    // Lit, the glow goes down first, under every bar, and the bars over it
    // at full strength, whatever their opacity.
    let passes: &[bool] = if lit { &[true, false] } else { &[false] };
    for glow in passes.iter().copied() {
        for rising in [false, true] {
            let colour = if rising { &up } else { &down };
            let mut any = false;
            for (i, bar) in columns.bars.iter().enumerate() {
                if (bar.close >= bar.open) != rising || bar.volume <= 0.0 {
                    continue;
                }
                any = true;
                let h = (base - to_y(bar.volume)).max(1.0);
                // A column's own pixel once it holds several bars, for the
                // reason the wicks take it.
                let (x, w) = columns.tile(i).unwrap_or_else(|| columns.body(i));
                let (top, h) = ((base - h).round(), h.round());
                match glow {
                    true => selection::glow_bar(cr, (x, top, w, h), false),
                    false => cr.rectangle(x, top, w, h),
                }
            }
            if any {
                let alpha = match (glow, lit) {
                    (true, _) => selection::glow_alpha(&state.theme),
                    (false, true) => 1.0,
                    (false, false) => opacity,
                };
                colors::set_source_alpha(cr, colour, alpha);
                let _ = cr.fill();
            }
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
    // In the indicator's colour while it is selected: the one mark that is
    // there whatever is on screen, when its line is scrolled out of view.
    match state.picked == Some(drawn.indicator.id) {
        true => colors::set_source(cr, &drawn.color),
        false => colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.8),
    }
    cr.move_to(plot_x + 2.0, top + 11.0);
    let _ = cr.show_text(&drawn.indicator.legend_for(state.timeframe, drawn.output.is_estimated()));
}

/// Room at the top of a strip for its name, so a line at the top of its scale
/// runs under the name rather than through it.
const PANE_NAME_ROOM: f64 = 16.0;

/// The part of a strip its scale is laid over, as a top and a height: under
/// the name with a little of the strip to spare, and a few pixels clear of
/// the strip below.
///
/// The drawing and the crosshair both read the strip through this, or the
/// value on the axis would not be the line under the pointer.
fn pane_plot(top: f64, height: f64) -> (f64, f64) {
    let room = (PANE_NAME_ROOM + height * 0.05).min(height * 0.35);
    let foot = 3.0_f64.min(height * 0.1);
    (top + room, (height - room - foot).max(1.0))
}

/// How many places a strip's values are written to. A fixed scale gets two,
/// as TradingView gives an oscillator; a fitted one gets the precision its own
/// ticks would, as the price does.
fn pane_decimals(pane: &omacharts_engine::indicators::Pane, low: f64, high: f64) -> usize {
    match pane.bounds {
        Some(_) => 2,
        None => decimals_for(nice_step(high - low, 3)),
    }
}

/// The colour of a strip's second line: a stochastic's %D, a MACD's signal.
/// Its own if somebody chose one, otherwise the theme's companion to the main
/// line, as a point of control is.
fn signal_colour(state: &State, drawn: &Drawn) -> String {
    let chosen = match &drawn.indicator.params {
        omacharts_engine::Params::Stochastic { d_color, .. } => d_color.as_ref(),
        omacharts_engine::Params::Macd { signal_color, .. } => signal_color.as_ref(),
        _ => None,
    };
    state.theme.companion_or(chosen, &drawn.color)
}

/// The part of a per-bar series that is on screen.
fn on_screen<T>(series: &[T], first: usize, visible: usize) -> &[T] {
    &series[first.min(series.len())..(first + visible).min(series.len())]
}

/// One indicator in its own strip: guides, then the line.
///
/// The strip carries its own name because the legend at the top cannot say
/// which of three stacked panes is which, and a pane you have to count down to
/// identify is a pane you misread.
/// A cumulative delta's figure on the axis: whole contracts, grouped, up to
/// a hundred thousand either way, and past that to three figures with K, M
/// or B, so a stock's running total of millions of shares stays inside the
/// axis.
fn contracts(value: f64) -> String {
    let size = value.abs();
    let (scaled, unit) = match size {
        _ if size >= 1e9 => (value / 1e9, "B"),
        _ if size >= 1e6 => (value / 1e6, "M"),
        _ if size >= 1e5 => (value / 1e3, "K"),
        _ => return grouped_decimals(value.round(), 0),
    };
    let decimals = match scaled.abs() {
        a if a >= 100.0 => 0,
        a if a >= 10.0 => 1,
        _ => 2,
    };
    format!("{scaled:.decimals$}{unit}")
}

/// The room either side of a cumulative delta's figures on the axis.
const DELTA_AXIS_PAD: f64 = 6.0;

/// How far apart, at the least, a cumulative delta's figures are written
/// down its axis: closer than the price's, as its strip is shorter, but
/// never so close that two 11px figures crowd each other.
const DELTA_LABEL_GAP: f64 = 26.0;

/// A round step, 1, 2 or 5 times a power of ten, giving about `rows`
/// figures across `range`; never smaller than one contract.
fn delta_step(range: f64, rows: f64) -> f64 {
    if range.is_nan() || rows.is_nan() || range <= 0.0 || rows <= 0.0 {
        return 0.0;
    }
    let rough = range / rows;
    let magnitude = 10f64.powf(rough.log10().floor());
    let step = match rough / magnitude {
        n if n <= 1.0 => 1.0,
        n if n <= 2.0 => 2.0,
        n if n <= 5.0 => 5.0,
        _ => 10.0,
    } * magnitude;
    step.max(1.0)
}

/// Where a cumulative delta's axis is written, `low` to `high` over `h`
/// pixels: its step, and every multiple of it in between, lowest first.
/// Zero is a multiple, so it is written whenever it is on the strip.
fn delta_ticks(low: f64, high: f64, h: f64) -> (f64, Vec<f64>) {
    let step = delta_step(high - low, (h / DELTA_LABEL_GAP).max(2.0).floor());
    if step <= 0.0 {
        return (0.0, Vec::new());
    }
    let (first, last) = ((low / step).ceil() as i64, (high / step).floor() as i64);
    // A step so fine as to make thousands of figures is no axis.
    if last - first > 1_000 {
        return (step, Vec::new());
    }
    (step, (first..=last).map(|at| at as f64 * step).collect())
}

/// A cumulative delta's axis written out: each figure whole and grouped,
/// as the price axis writes its own, "-1,200"; in K, M or B, to the
/// precision the step needs, only when one of them in full is wider than
/// the axis, as a stock's millions of shares are. A future's never are.
fn delta_figures(ticks: &[f64], step: f64, fits: &dyn Fn(&str) -> bool) -> Vec<String> {
    let full: Vec<String> = ticks.iter().map(|value| grouped_decimals(*value, 0)).collect();
    if full.iter().all(|text| fits(text)) {
        return full;
    }
    let most = ticks.iter().fold(0.0_f64, |most, value| most.max(value.abs()));
    let (by, unit) = match most {
        _ if most >= 1e9 => (1e9, "B"),
        _ if most >= 1e6 => (1e6, "M"),
        _ => (1e3, "K"),
    };
    let decimals = decimals_for(step / by);
    ticks
        .iter()
        .map(|value| match *value == 0.0 {
            true => "0".to_string(),
            false => format!("{}{unit}", grouped_decimals(*value / by, decimals)),
        })
        .collect()
}

/// A cumulative delta's close on its axis chip: in full when it fits
/// there, shortened to K, M or B only when it does not.
fn delta_figure(value: f64, fits: &dyn Fn(&str) -> bool) -> String {
    let full = grouped_decimals(value.round(), 0);
    match fits(&full) {
        true => full,
        false => contracts(value),
    }
}

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
    width: f64,
) {
    let plot_x = columns.plot_x;
    let (first, visible) = (columns.first, columns.visible);
    let values = on_screen(&pane.values, first, visible);
    let lit = state.picked == Some(drawn.indicator.id);
    let Some((low, high)) = state.strip_window(drawn.indicator.id, pane_range(pane, first, visible)) else {
        // Nothing to scale to and so nothing to draw — but the strip is still
        // a box on the chart, and a box with no name in it is one you have to
        // count down the stack to identify. An ATR zoomed into its own
        // warm-up is the case: every other strip has a scale whatever is in
        // it, because a fixed one is its bounds and a MACD's zero guide is
        // one on its own.
        draw_pane_name(cr, state, drawn, plot_x, top);
        return;
    };
    let (inner_top, inner_h) = pane_plot(top, height);
    let to_y = |value: f64| inner_top + inner_h * (high - value) / (high - low);
    // Everything drawn in the strip is kept inside it. Nothing fitted falls
    // outside, but a strip on a fixed scale can be handed a value past its own
    // end, and a rectangle that overshot would be drawn over its neighbour.
    // Zoomed past what it draws, a line runs on out of the strip, which is
    // clipped to its box, rather than lying flat along its edge as though
    // it had stopped there.
    let reach = match state.strip_held.contains_key(&drawn.indicator.id) {
        true => height,
        false => 0.0,
    };
    let strip = |value: f64| to_y(value).clamp(top - reach, top + height + reach);

    // The band between the guides, so overbought and oversold read as regions
    // rather than two lines you have to remember the meaning of.
    if let Some((from, to)) = pane.band {
        cr.rectangle(plot_x, to_y(to), plot_w, (to_y(from) - to_y(to)).abs());
        colors::set_source_alpha(cr, &state.theme.ui.grid, 0.35);
        let _ = cr.fill();
    }

    // Under the guides and the lines, because it is the backdrop they are read
    // against — the same order the volume strip's bars and the price plot's
    // band shading are drawn in.
    if let Some(histogram) = &pane.histogram {
        draw_histogram(cr, state, on_screen(histogram, first, visible), columns, &strip, lit);
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
    // A cumulative delta's axis is ruled as the price's is, in one round
    // step, its figures one right-aligned column in the price axis's type.
    let room = width - (plot_x + plot_w) - DELTA_AXIS_PAD * 2.0;
    let fits = |text: &str| cr.text_extents(text).is_ok_and(|extents| extents.x_advance() <= room);
    if pane.delta.is_some() {
        cr.save().ok();
        cr.set_font_size(11.0);
        colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.9);
        let (step, ticks) = delta_ticks(low, high, inner_h);
        let right = plot_x + plot_w + DELTA_AXIS_PAD + room;
        // The close's chip is over the axis: a figure it would cover is
        // left out rather than shown half under it.
        let close = pane.delta.as_ref().and_then(|delta| delta.last()).map(|last| to_y(last.close));
        for (value, text) in ticks.iter().zip(delta_figures(&ticks, step, &fits)) {
            let y = to_y(*value);
            if y < top || y > top + height || close.is_some_and(|close| (close - y).abs() < AXIS_CHIP_H) {
                continue;
            }
            let wide = cr.text_extents(&text).map_or(0.0, |extents| extents.x_advance());
            cr.move_to(right - wide, (y + 3.5).min(top + height));
            let _ = cr.show_text(&text);
        }
        cr.restore().ok();
    }
    let marks: Vec<(f64, String)> = if pane.bounds.is_some() {
        pane.guides.iter().map(|g| (*g, format!("{g:.0}"))).collect()
    } else if pane.delta.is_some() {
        Vec::new()
    } else {
        let decimals = pane_decimals(pane, low, high);
        vec![(low, grouped_decimals(low, decimals)), (high, grouped_decimals(high, decimals))]
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

    // A cumulative delta is its candles or its line, and its close on the
    // axis, in place of the lines below.
    if let Some(delta) = &pane.delta {
        let shown = on_screen(delta, first, visible);
        let line = matches!(
            drawn.indicator.params,
            omacharts_engine::Params::Cvd { style: omacharts_engine::indicators::cvd::Style::Line, .. }
        );
        let (up, down) = state.delta_ink(drawn);
        let chip = match line {
            true => {
                draw_delta_line(cr, state, drawn, shown, columns, &strip, lit);
                drawn.color.as_str()
            }
            false => {
                draw_delta_candles(cr, state, shown, columns, &strip, (up, down), lit);
                match delta.last() {
                    Some(last) if last.close < last.open => down,
                    _ => up,
                }
            }
        };
        let chips = delta
            .last()
            .map(|last| (to_y(last.close), delta_figure(last.close, &fits), chip))
            .filter(|(y, _, _)| *y >= top && *y <= top + height)
            .into_iter()
            .collect();
        draw_axis_chips(cr, state, chips, plot_x + plot_w, top + height, width);
        return;
    }

    // The lines last, over their own furniture.
    let stroke = drawn.indicator.stroke;
    if stroke.is_hidden() {
        return;
    }
    let signal = pane.signal.as_ref().map(|signal| (signal, signal_colour(state, drawn)));
    // Lit, every line glows under itself, and each gets a dot at its latest
    // value, in its own colour. Gathered only then.
    let lines: Vec<(&Vec<Option<f64>>, &str)> = match lit {
        false => Vec::new(),
        true => [(&pane.values, drawn.color.as_str())]
            .into_iter()
            .chain(signal.iter().map(|(series, colour)| (*series, colour.as_str())))
            .collect(),
    };
    let point = |series: &[Option<f64>], at: usize| {
        series.get(columns.offset(at)).copied().flatten().map(|value| (columns.x(at), strip(value)))
    };
    if lit {
        // Inside the strip, as its lines are: a glow past its edge would be
        // drawn over the strip next to it.
        cr.save().ok();
        cr.rectangle(plot_x, top, plot_w, height);
        cr.clip();
        for (series, colour) in &lines {
            let shown = on_screen(series, first, visible);
            selection::glow_line(cr, &state.theme, colour, stroke.width, columns, |at| point(shown, at));
        }
        cr.restore().ok();
    }
    stroke_pane_line(cr, stroke, &drawn.color, values, columns, &strip);
    if let Some((signal, colour)) = &signal {
        stroke_pane_line(cr, stroke, colour, on_screen(signal, first, visible), columns, &strip);
    }
    if lit {
        for (series, colour) in &lines {
            let shown = on_screen(series, first, visible);
            let latest = selection::latest(series).map(|bar| bar.saturating_sub(first));
            let source = |at: usize| columns.offset(at);
            selection::end_dot(cr, &state.theme, colour, columns, latest, |at| point(shown, at), source);
        }
    }

    // Each line's latest value on the axis, in the line's own colour, as the
    // last price is.
    let decimals = pane_decimals(pane, low, high);
    let lines = [(&pane.values, drawn.color.as_str())]
        .into_iter()
        .chain(signal.iter().map(|(series, colour)| (*series, colour.as_str())));
    let chips = lines
        .filter_map(|(series, colour)| {
            let last = series.iter().rev().find_map(|value| *value)?;
            let y = to_y(last);
            (y >= top && y <= top + height).then(|| (y, grouped_decimals(last, decimals), colour))
        })
        .collect();
    draw_axis_chips(cr, state, chips, plot_x + plot_w, top + height, width);
}

/// The bars of a cumulative delta a column stands for, as one: from the
/// first one's open to the last one's close, as far as any went either way.
fn delta_column(bars: &[indicators::cvd::DeltaBar]) -> Option<indicators::cvd::DeltaBar> {
    let (first, last) = (bars.first()?, bars.last()?);
    let mut column = indicators::cvd::DeltaBar { close: last.close, starts: bars.iter().any(|b| b.starts), ..*first };
    for bar in bars {
        column.high = column.high.max(bar.high);
        column.low = column.low.min(bar.low);
    }
    Some(column)
}

/// A cumulative delta's candles: rising ones in the up colour, falling ones
/// in the down colour, each a body from where its running delta opened to
/// where it closed and a wick to the furthest it went between. Solid, as
/// the volume strip's bars are, so a wick reads against the zero line.
fn draw_delta_candles(
    cr: &cairo::Context,
    state: &State,
    bars: &[indicators::cvd::DeltaBar],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
    (up, down): (&str, &str),
    lit: bool,
) {
    cr.save().ok();
    cr.set_line_width(1.0);
    let passes: &[bool] = if lit { &[true, false] } else { &[false] };
    for glow in passes.iter().copied() {
        for rising in [false, true] {
            let mut any = false;
            for at in 0..columns.len() {
                let Some(bar) = bars.get(columns.span(at)).and_then(delta_column) else { continue };
                if (bar.close >= bar.open) != rising || !bar.close.is_finite() {
                    continue;
                }
                any = true;
                let (top, bottom) = (to_y(bar.open.max(bar.close)), to_y(bar.open.min(bar.close)));
                let (x, w) = columns.tile(at).unwrap_or_else(|| columns.body(at));
                let (top, h) = (top.round(), (bottom.round() - top.round()).max(1.0));
                if glow {
                    selection::glow_bar(cr, (x, top, w, h), false);
                    continue;
                }
                cr.rectangle(x, top, w, h);
                // The wick down the column's middle, through the body.
                let cx = columns.x(at).round() + 0.5;
                let (high, low) = (to_y(bar.high).round(), to_y(bar.low).round());
                if high < top {
                    cr.rectangle(cx - 0.5, high, 1.0, top - high);
                }
                if low > top + h {
                    cr.rectangle(cx - 0.5, top + h, 1.0, low - top - h);
                }
            }
            if any {
                let alpha = if glow { selection::glow_alpha(&state.theme) } else { 1.0 };
                colors::set_source_alpha(cr, if rising { up } else { down }, alpha);
                let _ = cr.fill();
            }
        }
    }
    cr.restore().ok();
}

/// A cumulative delta as a line through each bar's close, in its own colour
/// and stroke, lifted at each reset so a period's first bar is not joined to
/// the last one's.
fn draw_delta_line(
    cr: &cairo::Context,
    state: &State,
    drawn: &Drawn,
    bars: &[indicators::cvd::DeltaBar],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
    lit: bool,
) {
    let stroke = drawn.indicator.stroke;
    if stroke.is_hidden() {
        return;
    }
    let point = |at: usize| {
        let bar = bars.get(columns.span(at)).and_then(delta_column)?;
        Some((columns.x(at), to_y(bar.close), bar.starts))
    };
    if lit {
        selection::glow_line(cr, &state.theme, &drawn.color, stroke.width, columns, |at| {
            point(at).map(|(x, y, _)| (x, y))
        });
    }
    cr.save().ok();
    cr.set_line_width(stroke.width);
    cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
    colors::set_source(cr, &drawn.color);
    let mut pen_down = false;
    for at in 0..columns.len() {
        let Some((x, y, starts)) = point(at) else {
            pen_down = false;
            continue;
        };
        if pen_down && !starts {
            cr.line_to(x, y);
        } else {
            cr.move_to(x, y);
            pen_down = true;
        }
    }
    let _ = cr.stroke();
    cr.restore().ok();
}

/// Chips on the axis at their own heights, moved apart where they would cover
/// each other, and kept above `floor`.
fn draw_axis_chips(
    cr: &cairo::Context,
    state: &State,
    mut chips: Vec<(f64, String, &str)>,
    axis_x: f64,
    floor: f64,
    width: f64,
) {
    chips.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut ys: Vec<f64> = chips.iter().map(|chip| chip.0).collect();
    spread_chips(&mut ys, AXIS_CHIP_H, floor);
    for ((_, text, colour), y) in chips.iter().zip(ys) {
        label_on_axis(cr, state, text, axis_x, y.round() + 0.5, width, colour);
    }
}

/// How tall a chip on the axis is.
const AXIS_CHIP_H: f64 = 16.0;

/// Move chips apart that would overlap, keeping their order.
///
/// `ys` are the chips' centres, top to bottom. A chip too close to the one
/// above it is pushed down until they just touch; if that pushes the last one
/// past `floor`, the run is lifted back by the overshoot, so two lines at the
/// bottom of a strip still get two readable chips inside it.
fn spread_chips(ys: &mut [f64], gap: f64, floor: f64) {
    for at in 1..ys.len() {
        ys[at] = ys[at].max(ys[at - 1] + gap);
    }
    let overshoot = ys.last().map_or(0.0, |last| last + gap / 2.0 - floor);
    if overshoot > 0.0 {
        for y in ys.iter_mut() {
            *y -= overshoot;
        }
    }
}

/// `window` stretched about its middle by `factor`: above one shows less
/// of it, larger, as a held price scale's zoom does.
fn scaled_window((low, high): (f64, f64), factor: f64) -> (f64, f64) {
    let factor = factor.clamp(1.0 / MAX_PRICE_ZOOM, 1.0 / MIN_PRICE_ZOOM);
    let middle = (low + high) / 2.0;
    let half = (high - low) / 2.0 / factor;
    (middle - half, middle + half)
}

/// The values a strip's top and bottom stand for, over the bars on screen.
///
/// Its own function because the crosshair has to read the strip on the same
/// scale it was drawn on, or the number on the axis is not the line under it.
///
/// Everything the strip draws is fitted, not just its first line: a MACD's
/// signal can lead the line it is an average of, its histogram reaches further
/// than either, and anything left out of this would be drawn outside the box
/// it belongs to. Nothing here assumes a floor of nought — a MACD is the one
/// strip whose whole point is which side of nought it is on.
fn pane_range(
    pane: &omacharts_engine::indicators::Pane,
    first: usize,
    visible: usize,
) -> Option<(f64, f64)> {
    if let Some(bounds) = pane.bounds {
        return Some(bounds);
    }
    // Every visible value, not one per column: what the strip is scaled to has
    // to be the range the line actually covers, or a peak that falls between
    // two columns would push the line off the top of its own strip.
    // Fit what is on screen, with a little air: an ATR pressed against the
    // top and bottom of its strip has no shape to read.
    let mut low = f64::MAX;
    let mut high = f64::MIN;
    let drawn = [Some(&pane.values), pane.signal.as_ref(), pane.histogram.as_ref()];
    for series in drawn.into_iter().flatten() {
        for value in on_screen(series, first, visible).iter().flatten() {
            low = low.min(*value);
            high = high.max(*value);
        }
    }
    // A cumulative delta's candles reach past their closes.
    for bar in pane.delta.as_deref().map(|bars| on_screen(bars, first, visible)).unwrap_or_default() {
        low = low.min(bar.low);
        high = high.max(bar.high);
    }
    // A guide is part of the scale rather than something drawn on top of it.
    // A MACD's histogram is measured from its zero line, so the zero line has
    // to be inside the strip — and a guide outside the fitted range would not
    // merely be invisible, it would be drawn across whichever strip is next.
    for guide in &pane.guides {
        low = low.min(*guide);
        high = high.max(*guide);
    }
    // Nothing on screen to scale to, which is what a strip still in its
    // warm-up has — thirty-three bars of it for a MACD. Neither end was ever
    // written, so `low` is still above `high`; carrying on from there made a
    // scale of plus and minus infinity, and every value drawn on it a NaN.
    // Cairo absorbs those rather than drawing anything, so it never showed —
    // but a strip that says it has no scale is the honest answer, and it is
    // what lets the one thing an empty strip should draw be drawn on purpose.
    if low > high || !low.is_finite() || !high.is_finite() {
        return None;
    }
    if (high - low).abs() < f64::EPSILON {
        Some((low - 1.0, high + 1.0))
    } else {
        let air = (high - low) * 0.12;
        Some((low - air, high + air))
    }
}

/// A strip's bars, measured from nought rather than from the foot of the box.
///
/// Which is the difference between this and the volume strip: a volume bar
/// stands on the bottom of its strip because there is no such thing as
/// negative volume, and a MACD's histogram hangs either side of nought
/// because which side it is on is the whole of what it says. Nought is in the
/// strip's own scale — [`pane_range`] puts it there — so the baseline is
/// simply where that value falls.
///
/// Two passes, one per direction, so the colour is set twice for the whole
/// strip rather than once per bar.
fn draw_histogram(
    cr: &cairo::Context,
    state: &State,
    values: &[Option<f64>],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
    lit: bool,
) {
    let base = to_y(0.0);
    let passes: &[bool] = if lit { &[true, false] } else { &[false] };
    for glow in passes.iter().copied() {
        for rising in [false, true] {
            let colour = state.scheme.volume(if rising { Direction::Up } else { Direction::Down });
            let mut any = false;
            for at in 0..columns.len() {
                let Some(Some(value)) = values.get(columns.offset(at)) else { continue };
                // A bar at nought has no height to draw, and one whose value
                // is not a figure has neither height nor a side to be on. The
                // engine does not produce the second, and a rectangle with a
                // NaN corner is not a thing to find out about from inside a
                // draw callback.
                if !value.is_finite() || *value == 0.0 || (*value >= 0.0) != rising {
                    continue;
                }
                any = true;
                let y = to_y(*value);
                let (top, h) = (y.min(base).round(), (y - base).abs().max(1.0).round());
                let (x, w) = columns.tile(at).unwrap_or_else(|| columns.body(at));
                match glow {
                    // Away from the baseline, whichever side of it the bar is.
                    true => selection::glow_bar(cr, (x, top, w, h), !rising),
                    false => cr.rectangle(x, top, w, h),
                }
            }
            if any {
                let alpha = match (glow, lit) {
                    (true, _) => selection::glow_alpha(&state.theme),
                    (false, true) => 1.0,
                    (false, false) => 0.85,
                };
                colors::set_source_alpha(cr, colour, alpha);
                let _ = cr.fill();
            }
        }
    }
}

/// One line of a strip, broken wherever the series has no value.
fn stroke_pane_line(
    cr: &cairo::Context,
    stroke: indicators::Stroke,
    colour: &str,
    values: &[Option<f64>],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    cr.save().ok();
    cr.set_line_width(stroke.width);
    cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
    colors::set_source(cr, colour);
    let mut pen_down = false;
    for at in 0..columns.len() {
        let Some(Some(value)) = values.get(columns.offset(at)) else {
            pen_down = false;
            continue;
        };
        let (x, y) = (columns.x(at), to_y(*value));
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

/// What on a mark the pointer is over.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MarkHit {
    /// A mark's ×.
    Close(String),
    /// One of a mark's tabs: drag it out.
    Tab { mark: String, tab: String },
    /// A draggable mark's line or its label: drag it to another price.
    Line(String),
}

/// How tall a mark's label and tabs are on the plot: larger than a chip
/// on the axis, since they are read across the chart, not down a column.
const MARK_H: f64 = 20.0;
/// The width of the × segment that ends every mark's label.
const MARK_CAP: f64 = 20.0;
/// The room between a mark's pieces, and after the last of them.
const MARK_GAP: f64 = 5.0;
/// The room between a label and the edge of the plot it sits at.
const MARK_EDGE: f64 = 8.0;
/// The room either side of a label's body, of its tag, and of a tab's word.
const MARK_PAD: f64 = 9.0;
const TAG_PAD: f64 = 8.0;
const TAB_PAD: f64 = 7.0;
/// How heavy the line of a draggable mark with no line of its own is in
/// hand.
const MARK_WEIGHT: f64 = 1.5;

/// Where the marks go across the plot: the side their labels sit at, and
/// the line's run, from the plot's left edge to the last bar — or to the
/// plot's right edge while the last bar is scrolled past it — so the room
/// after the last bar holds the last price's line alone.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Lane {
    side: LabelSide,
    plot_x: f64,
    plot_w: f64,
    /// Where every mark's line stops.
    end: f64,
}

impl Lane {
    /// Where every mark's line starts: the plot's edge, or with the labels
    /// at the left, under their left end, so no stub of it shows before
    /// them.
    fn start(&self) -> f64 {
        match self.side {
            LabelSide::Left => self.plot_x + MARK_EDGE,
            LabelSide::Right => self.plot_x,
        }
    }
}

/// Where a mark's pieces sit across the plot, at the end of its line the
/// labels are put at: the label in one piece — the tag, the body, the × —
/// and its tabs beside it, on the side towards the bars. Worked out here
/// for the drawing and the pointer both, so a × is clicked exactly where it
/// is drawn.
struct MarkRow {
    /// The tag: the label's left end. No width when there is no tag.
    tag_x: f64,
    tag_w: f64,
    /// The body, after the tag, when there is one.
    body: Option<(f64, f64)>,
    /// The × segment, `MARK_CAP` wide: the label's right end — or, with no
    /// ×, where it would be, the label ending a gap short of it.
    cap_x: f64,
    capped: bool,
    /// Each tab, by its place among the mark's tabs, and where it sits.
    tabs: Vec<(usize, f64, f64)>,
}

impl MarkRow {
    /// The label's left edge.
    fn left(&self) -> f64 {
        self.tag_x
    }

    /// The label's right edge.
    fn right(&self) -> f64 {
        match self.capped {
            true => self.cap_x + MARK_CAP,
            false => self.cap_x - MARK_GAP,
        }
    }

    fn width(&self) -> f64 {
        self.right() - self.left()
    }

    /// The label's outline, centred on `y`.
    fn path(&self, cr: &cairo::Context, y: f64) {
        pill_path(cr, self.left(), y - MARK_H / 2.0, self.width(), MARK_H);
    }
}

/// The font a mark's words are in.
fn mark_font(cr: &cairo::Context) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(13.0);
}

/// The font of a chip on the price axis, the last price's among them.
fn axis_font(cr: &cairo::Context) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
}

/// Where the words in the current font sit to be centred on `y`.
fn baseline(cr: &cairo::Context, y: f64) -> f64 {
    cr.font_extents().map_or(y + 3.5, |font| (y + (font.ascent() - font.descent()) / 2.0).round())
}

/// Where the label of a mark saying `tag` and `body` sits, with a × when
/// `capped`, and its `tabs` beside it. At the right, from the axis: the ×,
/// the body, the tag, the tabs. At the left, from the edge: the tag, the
/// body, the ×, the tabs — in their order either way.
fn mark_row(
    cr: &cairo::Context,
    lane: Lane,
    tag: &str,
    body: Option<&str>,
    capped: bool,
    tabs: &[MarkTab],
) -> MarkRow {
    let advance = |text: &str| cr.text_extents(text).map_or(0.0, |e| e.x_advance());
    if lane.side == LabelSide::Left {
        let tag_x = lane.plot_x + MARK_EDGE;
        let tag_w = match tag.is_empty() {
            true => 0.0,
            false => advance(tag) + 2.0 * TAG_PAD,
        };
        let mut right = tag_x + tag_w;
        let body = body.map(|body| {
            let w = advance(body) + 2.0 * MARK_PAD;
            right += w;
            (right - w, w)
        });
        // Without a ×, where it would be, the label ending a gap short of it.
        let cap_x = if capped { right } else { right + MARK_GAP };
        let mut left = if capped { cap_x + MARK_CAP } else { right };
        let placed = tabs
            .iter()
            .enumerate()
            .map(|(n, tab)| {
                let w = advance(&tab.text) + 2.0 * TAB_PAD;
                left += MARK_GAP;
                let at = left;
                left += w;
                (n, at, w)
            })
            .collect();
        return MarkRow { tag_x, tag_w, body, cap_x, capped, tabs: placed };
    }
    let cap_x = lane.plot_x + lane.plot_w - MARK_EDGE - MARK_CAP;
    let mut left = if capped { cap_x } else { cap_x - MARK_GAP };
    let body = body.map(|body| {
        let w = advance(body) + 2.0 * MARK_PAD;
        left -= w;
        (left, w)
    });
    let tag_w = match tag.is_empty() {
        true => 0.0,
        false => advance(tag) + 2.0 * TAG_PAD,
    };
    left -= tag_w;
    let tag_x = left;
    let mut placed = Vec::with_capacity(tabs.len());
    // The last tab nearest the label, so they read in their order.
    for (n, tab) in tabs.iter().enumerate().rev() {
        let w = advance(&tab.text) + 2.0 * TAB_PAD;
        left -= MARK_GAP + w;
        placed.push((n, left, w));
    }
    MarkRow { tag_x, tag_w, body, cap_x, capped, tabs: placed }
}

/// Where a mark's label sits: its tag, its body, its × and its tabs.
fn label_row(cr: &cairo::Context, lane: Lane, mark: &PriceMark) -> MarkRow {
    mark_row(cr, lane, &mark.tag, mark.body.as_deref(), mark.closable, &mark.tabs)
}

/// A context to measure words in, off screen.
fn measuring() -> Option<cairo::Context> {
    let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).ok()?;
    let cr = cairo::Context::new(&surface).ok()?;
    mark_font(&cr);
    Some(cr)
}

/// What of the marks is under `(x, y)`: a ×, a tab, or a draggable mark's
/// line within [`MARK_NEAR`] of it — where the line runs, and its label,
/// its × not. The handles first, so a × over another mark's line is the ×:
/// a raised mark's, then a fixed mark's, then a draggable one's, each in
/// the order they were given. A faint mark is not to be taken hold of.
fn mark_at(
    plan: &Layout,
    lane: Lane,
    marks: &Marks,
    x: f64,
    y: f64,
    to_y: &impl Fn(f64) -> f64,
) -> Option<MarkHit> {
    if marks.is_empty() || x < plan.plot_x || x > plan.plot_x + plan.plot_w {
        return None;
    }
    let shown = |at: f64| at >= plan.price_y && at <= plan.price_y + plan.price_h;
    let cr = measuring()?;
    let within = |left: f64, w: f64, at: f64| x >= left - 1.0 && x <= left + w + 1.0 && (y - at).abs() <= MARK_H / 2.0;
    // A mark's × or one of its tabs, when the pointer is on one.
    let handle = |mark: &PriceMark, row: &MarkRow, at: f64| {
        if row.capped && within(row.cap_x, MARK_CAP, at) {
            return Some(MarkHit::Close(mark.id.clone()));
        }
        let (n, ..) = row.tabs.iter().find(|(_, left, w)| within(*left, *w, at))?;
        Some(MarkHit::Tab { mark: mark.id.clone(), tab: mark.tabs[*n].id.clone() })
    };
    let live = || marks.lines.iter().filter(|mark| !mark.dim);
    for mark in live().filter(|mark| mark.raised).chain(live().filter(|mark| !mark.raised && !mark.draggable)) {
        let at = to_y(mark.price).round();
        if shown(at)
            && let Some(hit) = handle(mark, &label_row(&cr, lane, mark), at)
        {
            return Some(hit);
        }
    }
    let mut nearest: Option<(&PriceMark, f64)> = None;
    for mark in live().filter(|mark| !mark.raised && mark.draggable) {
        let at = to_y(mark.price).round();
        if !shown(at) {
            continue;
        }
        let row = label_row(&cr, lane, mark);
        if let Some(hit) = handle(mark, &row, at) {
            return Some(hit);
        }
        let off = (at - y).abs();
        let on_cap = row.capped && x >= row.cap_x - 1.0 && x <= row.cap_x + MARK_CAP + 1.0;
        let on_label = x >= row.left() - 1.0 && x <= row.right() + 1.0;
        let along = (x <= lane.end || on_label) && !on_cap;
        if off <= MARK_NEAR && along && nearest.is_none_or(|(_, best)| off < best) {
            nearest = Some((mark, off));
        }
    }
    nearest.map(|(mark, _)| MarkHit::Line(mark.id.clone()))
}

/// The marks a chart is handed, and the rules a drag on them keeps to.
struct Marks {
    lines: Vec<PriceMark>,
    rules: Box<dyn KeptRules>,
}

impl Default for Marks {
    fn default() -> Marks {
        Marks { lines: Vec::new(), rules: Box::new(AnyPrice) }
    }
}

impl Marks {
    fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The same marks under the same rules: nothing to redraw. With no
    /// marks the rules have nothing to say, so none are the same as any.
    fn same(&self, lines: &[PriceMark], rules: &dyn KeptRules) -> bool {
        self.lines == lines && (lines.is_empty() || self.rules.same(rules.as_any()))
    }
}

/// What a drag on the marks is doing.
#[derive(Clone, PartialEq, Debug)]
enum Grab {
    /// One of a mark's tabs, dragged out.
    Tab { mark: String, tab: String },
    /// A draggable mark, taken to another price.
    Move(String),
}

/// What a dragged mark is: its tag, its look, and the price it started
/// from.
struct Dragged<'a> {
    mark: &'a str,
    tab: Option<&'a str>,
    tag: &'a str,
    from: f64,
    colour: &'a str,
    style: LineStyle,
    weight: f64,
}

impl State {
    /// The mark, or the mark's tab, a drag is about, while it is still on
    /// the chart and not faint.
    fn mark_dragged(&self, what: &Grab) -> Option<Dragged<'_>> {
        match what {
            Grab::Tab { mark, tab } => {
                let held = self.marks.lines.iter().find(|held| held.id == *mark && !held.dim)?;
                let tab = held.tabs.iter().find(|known| known.id == *tab)?;
                Some(Dragged {
                    mark: &held.id,
                    tab: Some(&tab.id),
                    tag: &tab.tag,
                    from: held.price,
                    colour: &tab.colour,
                    style: tab.style,
                    weight: tab.weight,
                })
            }
            Grab::Move(id) => {
                let mark = self.marks.lines.iter().find(|mark| mark.id == *id && mark.draggable && !mark.dim)?;
                Some(Dragged {
                    mark: &mark.id,
                    tab: None,
                    tag: &mark.tag,
                    from: mark.price,
                    colour: &mark.colour,
                    style: mark.style,
                    weight: mark.line.unwrap_or(MARK_WEIGHT),
                })
            }
        }
    }

    /// What the rules are asked about `dragged` in hand at `at`.
    fn drag_query<'a>(&self, dragged: &Dragged<'a>, at: f64, places: usize) -> MarkDrag<'a> {
        MarkDrag {
            mark: dragged.mark,
            tab: dragged.tab,
            from: dragged.from,
            at,
            last: self.bars.last().map(|bar| bar.close),
            tick: self.dragged_tick(),
            places,
        }
    }

    /// The step a mark moves in as it is dragged: the traded contract's
    /// when one was set, else the series'.
    fn dragged_tick(&self) -> Option<f64> {
        self.mark_tick.or_else(|| self.tick())
    }

    /// The places a price `at` is written to, on the dragged tick.
    fn places_at(&self, at: f64) -> usize {
        self.dragged_tick().map_or(2, |tick| omacharts_engine::on_tick(at, tick).1)
    }

    /// The price at height `y` of the price plot, on the series' tick, and
    /// the places it is written to.
    fn tick_price_at(&self, width: f64, height: f64, y: f64) -> Option<(f64, usize)> {
        let (first, visible, _) = self.view();
        if visible == 0 {
            return None;
        }
        let plan = layout(self, width, height);
        let scale = price_range(self, &self.bars[first..first + visible], &plan)?;
        let price = scale.price_at(y.clamp(plan.price_y, plan.price_y + plan.price_h), plan.price_y, plan.price_h);
        Some(match self.dragged_tick() {
            Some(tick) => omacharts_engine::on_tick(price, tick),
            None => (price, 2),
        })
    }

    /// May the drag `what` let go at `at`? Never with no bars, and never
    /// for a mark that is gone; otherwise as the rules say.
    fn mark_allowed(&self, what: &Grab, at: f64) -> bool {
        if self.bars.is_empty() {
            return false;
        }
        let Some(dragged) = self.mark_dragged(what) else { return false };
        self.marks.rules.rules().allowed(&self.drag_query(&dragged, at, self.places_at(at)))
    }

    /// What of the marks is under the pointer at `(x, y)`, on a chart
    /// `width` by `height`: nothing at all while a tool or the ruler is in
    /// hand, which the press belongs to.
    fn mark_under(&self, width: f64, height: f64, x: f64, y: f64) -> Option<MarkHit> {
        if self.marks.is_empty() || self.tool.is_some() || self.ruler_armed {
            return None;
        }
        let (first, visible, _) = self.view();
        if visible == 0 {
            return None;
        }
        let plan = layout(self, width, height);
        let scale = price_range(self, &self.bars[first..first + visible], &plan)?;
        mark_at(&plan, self.lane(&plan), &self.marks, x, y, &|price| scale.to_y(price, plan.price_y, plan.price_h))
    }

    /// Where the marks go on a chart laid out as `plan`: the side their
    /// labels sit at, and the line's run to the last bar's column.
    fn lane(&self, plan: &Layout) -> Lane {
        let (first, visible, lead) = self.view();
        let plot_end = plan.plot_x + plan.plot_w;
        // Columns as the frame lays them: the bars after the room opened
        // before the first, the last bar's column ending at `end`.
        let column_w = plan.plot_w / self.columns().max(1) as f64;
        let end = match first + visible >= self.bars.len() {
            true => (plan.plot_x + (lead + visible) as f64 * column_w).min(plot_end),
            false => plot_end,
        };
        Lane { side: self.mark_labels, plot_x: plan.plot_x, plot_w: plan.plot_w, end }
    }
}

/// A rounded box, for a pill.
fn pill_path(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    let r = h / 2.0;
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + r, r, std::f64::consts::FRAC_PI_2, 3.0 * std::f64::consts::FRAC_PI_2);
    cr.close_path();
}

/// How a mark's price is boxed on the axis: filled with the contrast the
/// last-price tag is held to, or outlined in the colour, the text in the
/// theme's own so it reads on any ground.
enum Boxed<'a> {
    Filled(Tag),
    Outlined(&'a str),
}

/// How a mark's chip is boxed: filled in the mark's colour, or outlined in
/// it.
fn chip_look(colour: &str, filled: bool) -> Boxed<'_> {
    match filled {
        true => Boxed::Filled(Tag::on(colour)),
        false => Boxed::Outlined(colour),
    }
}

/// What a mark's label says and how it is coloured.
struct Label<'a> {
    /// The line's colour: the label's outline, its tag's fill and its ×.
    colour: &'a str,
    tag: &'a str,
    /// The body's words and the colour they are in, the ink when none.
    body: Option<(&'a str, Option<&'a str>)>,
    /// The × is under the pointer: filled solid.
    lit: bool,
}

/// A mark's label, in one piece along `row`, centred on `y`: one outline
/// in the line's colour, the tag filled in it, the body open on the
/// chart's ground, and the × as the end segment, filled only under the
/// pointer. In the font already chosen.
fn draw_mark_label(cr: &cairo::Context, row: &MarkRow, y: f64, label: &Label, background: &str, ink: &str) {
    let top = y - MARK_H / 2.0;
    let side = Tag::on(label.colour);
    // The whole on the chart's ground, then its segments cut to its shape.
    row.path(cr, y);
    colors::set_source(cr, background);
    let _ = cr.fill_preserve();
    cr.save().ok();
    cr.clip();
    if row.tag_w > 0.0 {
        cr.rectangle(row.tag_x, top, row.tag_w, MARK_H);
        colors::set_source(cr, &side.fill);
        let _ = cr.fill();
    }
    if row.capped {
        if label.lit {
            cr.rectangle(row.cap_x, top, MARK_CAP, MARK_H);
            colors::set_source(cr, &side.fill);
            let _ = cr.fill();
        }
        // A divider before the ×, so it reads as a button of its own.
        cr.move_to(row.cap_x + 0.5, top);
        cr.line_to(row.cap_x + 0.5, top + MARK_H);
        cr.set_line_width(1.0);
        colors::set_source(cr, label.colour);
        let _ = cr.stroke();
    }
    cr.restore().ok();
    pill_path(cr, row.left() + 0.5, top + 0.5, row.width() - 1.0, MARK_H - 1.0);
    cr.set_line_width(1.0);
    colors::set_source(cr, label.colour);
    let _ = cr.stroke();
    // The words: the tag's on its fill, the body's in its own colour.
    if row.tag_w > 0.0 {
        colors::set_source(cr, &side.label);
        cr.move_to(row.tag_x + TAG_PAD, baseline(cr, y));
        let _ = cr.show_text(label.tag);
    }
    if let (Some((x, _)), Some((text, colour))) = (row.body, label.body) {
        colors::set_source(cr, colour.unwrap_or(ink));
        cr.move_to(x + MARK_PAD, baseline(cr, y));
        let _ = cr.show_text(text);
    }
    // The cross: in the line's colour, and in the label's once lit.
    if row.capped {
        let (cx, r) = (row.cap_x + MARK_CAP / 2.0, if label.lit { 4.0 } else { 3.5 });
        cr.set_line_width(if label.lit { 1.75 } else { 1.25 });
        colors::set_source(cr, if label.lit { side.label.as_str() } else { label.colour });
        cr.move_to(cx - r, y - r);
        cr.line_to(cx + r, y + r);
        cr.move_to(cx + r, y - r);
        cr.line_to(cx - r, y + r);
        let _ = cr.stroke();
    }
}

/// The marks, drawn under the last price's tag: lines from the plot's left
/// edge to the last bar, the last price's over them all, then over every
/// line its label at the end of the chart chosen — the tag, the body and
/// the × in one piece, and the mark's tabs beside it — the raised marks'
/// over the line joining linked marks and everybody else's under it, and
/// the marks' prices on the axis spread apart with the last price's where
/// they would cover each other. Returns where the last price's chip goes,
/// when it had to move.
#[allow(clippy::too_many_arguments)]
fn draw_marks(
    cr: &cairo::Context,
    state: &State,
    lane: Lane,
    price_y: f64,
    floor: f64,
    width: f64,
    to_y: &impl Fn(f64) -> f64,
) -> Option<f64> {
    if state.marks.is_empty() {
        return None;
    }
    let shown = |y: f64| y >= price_y && y <= floor;
    let (plot_x, plot_w) = (lane.plot_x, lane.plot_w);
    cr.save().ok();
    mark_font(cr);
    let background = state.theme.ui.background.as_str();
    let ink = state.theme.ui.text.as_str();
    // A chip on the axis at `x`, `w` wide, centred on `y`, saying `text`
    // in the font already chosen.
    let chip = |cr: &cairo::Context, x: f64, y: f64, w: f64, text: &str, look: &Boxed| {
        cr.rectangle(x + 0.5, y - AXIS_CHIP_H / 2.0 + 0.5, w - 1.0, AXIS_CHIP_H - 1.0);
        let text_colour = match look {
            Boxed::Filled(tag) => {
                colors::set_source(cr, &tag.fill);
                let _ = cr.fill();
                tag.label.as_str()
            }
            Boxed::Outlined(colour) => {
                colors::set_source(cr, background);
                let _ = cr.fill_preserve();
                cr.set_line_width(1.0);
                colors::set_source(cr, colour);
                let _ = cr.stroke();
                ink
            }
        };
        colors::set_source(cr, text_colour);
        cr.move_to(x + 5.0, y + 3.5);
        let _ = cr.show_text(text);
    };
    let line = |cr: &cairo::Context, y: f64, colour: &str, style: LineStyle, weight: f64| {
        cr.set_line_width(weight);
        cr.set_dash(&style.dashes(weight), 0.0);
        colors::set_source(cr, colour);
        cr.move_to(lane.start(), y);
        cr.line_to(lane.end, y);
        let _ = cr.stroke();
        cr.set_dash(&[], 0.0);
    };
    // The halo a tab wears under the pointer, as a drawing's grip does.
    let halo = |cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64| {
        let pad = 2.0;
        colors::set_source_alpha(cr, ink, 0.22);
        cr.rectangle(x - pad, y - h / 2.0 - pad, w + 2.0 * pad, h + 2.0 * pad);
        let _ = cr.fill();
    };
    // A faint mark is drawn at less than full strength.
    let faint = |cr: &cairo::Context, dim: bool, paint: &dyn Fn(&cairo::Context)| match dim {
        true => {
            cr.push_group();
            paint(cr);
            let _ = cr.pop_group_to_source();
            let _ = cr.paint_with_alpha(0.4);
        }
        false => paint(cr),
    };
    // Under a faint mark's boxes, the ground at full strength, so no line
    // shows through what it says.
    let ground = |cr: &cairo::Context, dim: bool, shapes: &dyn Fn(&cairo::Context)| {
        if dim {
            shapes(cr);
            colors::set_source(cr, background);
            let _ = cr.fill();
        }
    };
    let lit = state.hover_mark.as_ref();
    let ys = |price: f64| Some(to_y(price).round()).filter(|y| shown(*y));
    let marks: Vec<(&PriceMark, f64)> =
        state.marks.lines.iter().filter_map(|mark| Some((mark, ys(mark.price)?))).collect();

    // Every line first, the last price's last of them, over every mark's
    // so it still reads with the price sitting on a mark; and then every
    // label on top, so that no line, the mark's own or another's at the
    // same price, is drawn through words.
    for (mark, y) in &marks {
        if let Some(weight) = mark.line {
            faint(cr, mark.dim, &|cr| line(cr, *y, &mark.colour, mark.style, weight));
        }
    }
    draw_last_price_line(cr, state, plot_x, plot_w, price_y, to_y);

    let tab_h = MARK_H - 2.0;
    // A mark's label and its tabs.
    let label = |cr: &cairo::Context, mark: &PriceMark, y: f64| {
        let row = label_row(cr, lane, mark);
        ground(cr, mark.dim, &|cr| {
            row.path(cr, y);
            for (_, x, w) in &row.tabs {
                cr.rectangle(*x, y - tab_h / 2.0, *w, tab_h);
            }
        });
        faint(cr, mark.dim, &|cr| {
            let lit_cap = lit.is_some_and(|hit| matches!(hit, MarkHit::Close(id) if *id == mark.id));
            let body = mark.body.as_deref().map(|body| (body, None));
            let label = Label { colour: &mark.colour, tag: &mark.tag, body, lit: lit_cap };
            draw_mark_label(cr, &row, y, &label, background, ink);
            for (n, x, w) in &row.tabs {
                let tab = &mark.tabs[*n];
                if lit.is_some_and(|hit| matches!(hit, MarkHit::Tab { mark: on, tab: which } if *on == mark.id && *which == tab.id)) {
                    halo(cr, *x, y, *w, tab_h);
                }
                pill_path(cr, x + 0.5, y - tab_h / 2.0 + 0.5, w - 1.0, tab_h - 1.0);
                colors::set_source(cr, background);
                let _ = cr.fill_preserve();
                // Its dashes are its own, and its outline as heavy as it
                // asks, so it is told by more than its colour.
                cr.set_line_width(tab.outline);
                cr.set_dash(&tab.style.dashes(tab.outline), 0.0);
                colors::set_source(cr, &tab.colour);
                let _ = cr.stroke();
                cr.set_dash(&[], 0.0);
                colors::set_source(cr, ink);
                cr.move_to(x + TAB_PAD, baseline(cr, y));
                let _ = cr.show_text(&tab.text);
            }
        });
    };

    let mut chips: Vec<(f64, &str, Option<Boxed>, bool)> = Vec::with_capacity(state.marks.lines.len() + 1);
    for (mark, y) in marks.iter().filter(|(mark, _)| !mark.raised) {
        label(cr, mark, *y);
        if let Some(at) = &mark.chip {
            chips.push((y + 0.5, &at.text, Some(chip_look(&mark.colour, at.filled)), mark.dim));
        }
    }
    draw_link(cr, lane, &marks, ink, &faint);
    for (mark, y) in marks.iter().filter(|(mark, _)| mark.raised) {
        label(cr, mark, *y);
        if let Some(at) = &mark.chip {
            chips.push((y + 0.5, &at.text, Some(chip_look(&mark.colour, at.filled)), mark.dim));
        }
    }
    let last = last_price(state).map(|price| to_y(price).round() + 0.5).filter(|y| shown(*y));
    if let Some(y) = last {
        chips.push((below_cover(state, y), "", None, false));
    }
    chips.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut ys: Vec<f64> = chips.iter().map(|chip| chip.0).collect();
    spread_chips(&mut ys, AXIS_CHIP_H, floor);
    let axis_x = plot_x + plot_w;
    let mut moved = None;
    // The prices on the axis are chips like the last price's, in its font.
    axis_font(cr);
    for ((_, text, look, dim), y) in chips.iter().zip(ys) {
        let y = y.round() + 0.5;
        let Some(look) = look else {
            moved = Some(y);
            continue;
        };
        let Ok(extents) = cr.text_extents(text) else { continue };
        let w = (extents.width() + 10.0).min(width - axis_x - 2.0);
        ground(cr, *dim, &|cr| cr.rectangle(axis_x + 1.0, y - AXIS_CHIP_H / 2.0, w, AXIS_CHIP_H));
        faint(cr, *dim, &|cr| chip(cr, axis_x + 1.0, y, w, text, look));
    }
    cr.restore().ok();
    moved
}

/// The line joining linked marks, on the side of their labels that faces
/// the plot: from the top one to the bottom one, with a short stroke into
/// each. Dashed if any of them asks, faint while any is pending.
fn draw_link(cr: &cairo::Context, lane: Lane, marks: &[(&PriceMark, f64)], ink: &str, faint: Faint) {
    let linked: Vec<(MarkRow, f64, MarkLink)> = marks
        .iter()
        .filter_map(|(mark, y)| Some((label_row(cr, lane, mark), *y, mark.link?)))
        .collect();
    if linked.len() < 2 {
        return;
    }
    let (x, into): (f64, Vec<f64>) = match lane.side {
        LabelSide::Left => {
            let x = linked.iter().map(|(row, ..)| row.right()).fold(f64::MIN, f64::max) + LINK_GAP;
            (x, linked.iter().map(|(row, ..)| row.right()).collect())
        }
        _ => {
            let x = linked.iter().map(|(row, ..)| row.left()).fold(f64::MAX, f64::min) - LINK_GAP;
            (x, linked.iter().map(|(row, ..)| row.left()).collect())
        }
    };
    let x = x.round() + 0.5;
    let top = linked.iter().map(|(_, y, _)| *y).fold(f64::MAX, f64::min);
    let bottom = linked.iter().map(|(_, y, _)| *y).fold(f64::MIN, f64::max);
    let pending = linked.iter().any(|(.., link)| link.pending);
    let dashed = linked.iter().any(|(.., link)| link.dashed);
    faint(cr, pending, &|cr| {
        cr.set_line_width(1.0);
        cr.set_dash(if dashed { &[3.0, 2.0] } else { &[] }, 0.0);
        colors::set_source_alpha(cr, ink, 0.7);
        cr.move_to(x, top + 0.5);
        cr.line_to(x, bottom + 0.5);
        for ((_, y, _), edge) in linked.iter().zip(&into) {
            cr.move_to(x, y + 0.5);
            cr.line_to(*edge, y + 0.5);
        }
        let _ = cr.stroke();
        cr.set_dash(&[], 0.0);
    });
}

/// Paint what is given, faint or not.
type Faint<'a> = &'a dyn Fn(&cairo::Context, bool, &dyn Fn(&cairo::Context));

/// How far the line joining linked marks stands off their labels.
const LINK_GAP: f64 = 4.0;

/// A mark being dragged: a ghost of its line where the hand has it, in the
/// colour the rules say and in its own look otherwise, its label without a
/// × saying what the rules say, and its price on the axis. Muted where it
/// may not go.
#[allow(clippy::too_many_arguments)]
fn draw_mark_preview(
    cr: &cairo::Context,
    state: &State,
    plan: &Layout,
    width: f64,
    what: &Grab,
    at: f64,
    allowed: bool,
    to_y: &impl Fn(f64) -> f64,
) {
    let Some(dragged) = state.mark_dragged(what) else { return };
    let y = to_y(at).round();
    if y < plan.price_y || y > plan.price_y + plan.price_h {
        return;
    }
    let places = state.places_at(at);
    let price = grouped_decimals(at, places);
    let preview = state.marks.rules.rules().preview(&state.drag_query(&dragged, at, places), allowed);
    let colour = match (allowed, preview.colour.as_deref()) {
        (true, Some(colour)) => colour,
        (true, None) => dragged.colour,
        (false, _) => state.theme.ui.text_muted.as_str(),
    };
    let text = preview.text.as_str();
    cr.save().ok();
    mark_font(cr);
    let line_y = y + 0.5;
    let lane = state.lane(plan);
    let weight = dragged.weight;
    cr.set_line_width(weight);
    cr.set_dash(&dragged.style.dashes(weight), 0.0);
    colors::set_source_alpha(cr, colour, 0.6);
    cr.move_to(lane.start(), line_y);
    cr.line_to(lane.end, line_y);
    let _ = cr.stroke();
    cr.set_dash(&[], 0.0);
    // The label where the mark's own would be, the ×'s place left empty.
    let row = mark_row(cr, lane, dragged.tag, Some(text), false, &[]);
    // The words in the ink, and in the muted colour with the rest where
    // the mark may not go.
    let body_colour = (!allowed).then_some(colour);
    let label = Label { colour, tag: dragged.tag, body: Some((text, body_colour)), lit: false };
    draw_mark_label(cr, &row, line_y, &label, &state.theme.ui.background, &state.theme.ui.text);
    let axis_x = plan.plot_x + plan.plot_w;
    axis_font(cr);
    if let Ok(extents) = cr.text_extents(&price) {
        let w = (extents.width() + 10.0).min(width - axis_x - 2.0);
        cr.rectangle(axis_x + 1.5, line_y - AXIS_CHIP_H / 2.0 + 0.5, w - 1.0, AXIS_CHIP_H - 1.0);
        colors::set_source(cr, &state.theme.ui.background);
        let _ = cr.fill_preserve();
        cr.set_line_width(1.0);
        colors::set_source(cr, colour);
        let _ = cr.stroke();
        colors::set_source(cr, &state.theme.ui.text);
        cr.move_to(axis_x + 6.0, line_y + 3.5);
        let _ = cr.show_text(&price);
    }
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
    // Where the tag goes when marks' chips on the axis moved it.
    tag_at: Option<f64>,
) {
    let Some(price) = last_price(state) else { return };
    let y = to_y(price).round() + 0.5;
    if y < price_y {
        return;
    }
    // In the last price's own colour, the line's: one thing on the chart,
    // whose rise or fall the candles and the header already say.
    let tag = &state.last_ink.tag;
    // With marks on the chart their boxes drew it already, under them.
    if state.marks.is_empty() {
        draw_last_price_line(cr, state, plot_x, plot_w, price_y, to_y);
    }

    // The tag stops under the window's corner rather than slide beneath
    // it; the line stays at the price.
    draw_tag(
        cr,
        &grouped_decimals(price, decimals),
        plot_x + plot_w,
        tag_at.unwrap_or_else(|| below_cover(state, y)),
        width,
        tag,
    );
}

/// The last price: the feed's latest trade when the bars drawn leave it
/// out, else the last bar's close.
fn last_price(state: &State) -> Option<f64> {
    let last = state.bars.last()?;
    Some(state.latest.unwrap_or(last.close))
}

/// The last price's dashed line across the plot.
fn draw_last_price_line(
    cr: &cairo::Context,
    state: &State,
    plot_x: f64,
    plot_w: f64,
    price_y: f64,
    to_y: &impl Fn(f64) -> f64,
) {
    let Some(price) = last_price(state) else { return };
    let y = to_y(price).round() + 0.5;
    if y < price_y {
        return;
    }
    cr.save().ok();
    cr.set_dash(&[3.0, 3.0], 0.0);
    cr.set_line_width(1.0);
    colors::set_source_alpha(cr, &state.last_ink.line, 0.7);
    cr.move_to(plot_x, y);
    cr.line_to(plot_x + plot_w, y);
    let _ = cr.stroke();
    cr.restore().ok();
}

/// How far a label fades out under the edge of whatever covers the top of
/// the price axis.
const AXIS_FADE_H: f64 = 10.0;

/// The labels under the window's corner fade out into it rather than end at
/// its edge, which cut a price through its digits. Drawn in the chart's own
/// colour, which is the corner's ground, so the two read as one.
fn draw_axis_fade(cr: &cairo::Context, state: &State, axis_x: f64, width: f64) {
    let top = state.covered_axis_top;
    if top <= 0.0 {
        return;
    }
    let (r, g, b, _) = colors::rgba(&state.theme.ui.background);
    let fade = cairo::LinearGradient::new(0.0, top, 0.0, top + AXIS_FADE_H);
    fade.add_color_stop_rgba(0.0, r, g, b, 1.0);
    fade.add_color_stop_rgba(1.0, r, g, b, 0.0);
    let x = axis_x.round() + 1.0;
    cr.rectangle(x, 0.0, width - x, top + AXIS_FADE_H);
    let _ = cr.set_source(&fade);
    let _ = cr.fill();
}

/// Where a chip centred at `y` on the price axis is drawn: there, or just
/// below the window's corner if it would reach under it.
fn below_cover(state: &State, y: f64) -> f64 {
    match state.covered_axis_top {
        top if top > 0.0 => y.max((top + AXIS_CHIP_H / 2.0).round() + 0.5),
        _ => y,
    }
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
    first: i64,
    scale: &PriceScale,
) {
    if px < plot_x || px > plot_x + plot_w || py < top || py > height - TIME_AXIS_H {
        return;
    }
    let crosshair = &state.theme.ui.crosshair;
    // A drawing's point in hand: the labels are its price and its time, so
    // they wear its colour and read as the point's rather than the pointer's.
    let held = match state.drag {
        Some(Drag::Grip { index, grip, .. }) if grip != Grip::Body => {
            state.drawings.get(index).map(|d| d.style(&state.configs).colour.hex(&state.theme))
        }
        _ => None,
    };
    let tint = held.as_deref().unwrap_or(crosshair);

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

    // Price under the pointer, only while it is over the price plot, and
    // not while a mark is dragged: its own price is on the axis then.
    let dragging = matches!(state.drag, Some(Drag::Mark { .. }));
    if py >= price_y && py <= price_y + price_h && !dragging {
        // In percent while markets are compared: the change at that height.
        let text = match state.compare.as_deref().and_then(|overlay| overlay.based(&state.bars, state.slice().0)) {
            Some(based) => compare::crosshair_label(&based, scale, price_h, scale.price_at(py, price_y, price_h)),
            None => price_under_pointer(state, scale, py, price_y, price_h),
        };
        label_on_axis(
            cr,
            state,
            &text,
            plot_x + plot_w,
            below_cover(state, py.round() + 0.5),
            width,
            tint,
        );
    }

    // Time under the pointer — over the room past either end as well,
    // where it is the time that column would be.
    if let Some(ts) = time_at_column(state, first + index_in_view.max(0.0) as i64) {
        let label = format_time_full(ts, state.timeframe, state.clock);
        // The pointer's own date is always written, in past the drawing
        // tools' toggle where it stands at the axis's start.
        let start = state.axis_start_taken.max(plot_x);
        label_on_time_axis(cr, &label, snapped_x, start, plot_w - (start - plot_x), height, tint);
    }
}

/// The price the crosshair names at height `py` of the price plot: on the
/// step the series trades in, when it shows one.
///
/// A pixel falls between ticks. Written as it came, ES read 7,627.55 beside
/// a last price of 7,620.25 — a price no trade can print, which looks like
/// the feed is wrong rather than the pointer being between two prices.
fn price_under_pointer(state: &State, scale: &PriceScale, py: f64, price_y: f64, price_h: f64) -> String {
    let price = scale.price_at(py, price_y, price_h);
    let decimals =
        omacharts_engine::price_decimals(scale.step(price_h), price, state.instrument.as_ref().map(|i| i.kind));
    match state.tick() {
        Some(tick) => {
            let (price, places) = omacharts_engine::on_tick(price, tick);
            grouped_decimals(price, decimals.max(places))
        }
        None => grouped_decimals(price, decimals),
    }
}

/// The value under the pointer on a strip's own scale, on the axis beside it,
/// the way the price is read off the price plot.
#[allow(clippy::too_many_arguments)]
fn draw_pane_value(
    cr: &cairo::Context,
    state: &State,
    plan: &Layout,
    px: f64,
    py: f64,
    first: usize,
    visible: usize,
    width: f64,
) {
    if px < plan.plot_x || px > plan.plot_x + plan.plot_w {
        return;
    }
    let Some(row) = plan.rows.iter().find(|row| row.pane.is_some() && row.covers(py)) else {
        return;
    };
    let Some((id, Output::Pane(pane))) = state
        .indicators
        .iter()
        .find(|drawn| Some(drawn.indicator.id) == row.pane)
        .map(|drawn| (drawn.indicator.id, &drawn.output))
    else {
        return;
    };
    let Some((low, high)) = state.strip_window(id, pane_range(pane, first, visible)) else {
        return;
    };
    let (inner_top, inner_h) = pane_plot(row.top, row.height);
    let value = high - (py - inner_top) / inner_h * (high - low);
    let text = match pane.delta.is_some() {
        true => contracts(value),
        false => grouped_decimals(value, pane_decimals(pane, low, high)),
    };
    label_on_axis(
        cr,
        state,
        &text,
        plan.plot_x + plan.plot_w,
        py.round() + 0.5,
        width,
        &state.theme.ui.crosshair,
    );
}

/// The last price's colour on the theme, and its tag filled in it.
#[derive(Clone)]
struct LastInk {
    line: String,
    tag: Tag,
}

impl LastInk {
    fn of(theme: &Theme) -> LastInk {
        let line = omacharts_engine::theme::theme_last_price(theme);
        LastInk { tag: Tag::on(&line), line }
    }
}

/// The last-price tag for each direction: the same fill and label the bar
/// widget's change pill is drawn in, from the one rule in [`Tag::on`].
fn price_tags(scheme: &BarScheme) -> [Tag; 2] {
    [Tag::on(&scheme.up), Tag::on(&scheme.down)]
}

/// The last-price tag on the price axis, the shape of every other chip there.
fn draw_tag(cr: &cairo::Context, text: &str, axis_x: f64, y: f64, width: f64, tag: &Tag) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
    let Ok(extents) = cr.text_extents(text) else { return };
    let h = AXIS_CHIP_H;
    let w = (extents.width() + 10.0).min(width - axis_x - 2.0);
    colors::set_source(cr, &tag.fill);
    cr.rectangle(axis_x + 1.0, y - h / 2.0, w, h);
    let _ = cr.fill();
    colors::set_source(cr, &tag.label);
    cr.move_to(axis_x + 6.0, y + 3.5);
    let _ = cr.show_text(text);
}

/// A filled chip on the time axis, centred on `x` and kept inside the plot.
fn label_on_time_axis(cr: &cairo::Context, text: &str, x: f64, plot_x: f64, plot_w: f64, height: f64, colour: &str) {
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
    let Ok(extents) = cr.text_extents(text) else { return };
    let w = extents.width() + 10.0;
    let x = inside_plot(x, w, plot_x, plot_w);
    let y = height - TIME_AXIS_H + 2.0;
    colors::set_source(cr, colour);
    cr.rectangle(x, y, w, TIME_AXIS_H - 4.0);
    let _ = cr.fill();
    colors::set_source(cr, colors::readable_on(colour));
    cr.move_to(x + 5.0, y + TIME_AXIS_H - 10.0);
    let _ = cr.show_text(text);
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
    let h = AXIS_CHIP_H;
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

/// Which way a measured range went, which picks its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RulerDirection {
    Up,
    Down,
    Flat,
}

impl RulerDirection {
    fn of(from: Anchor, to: Anchor) -> Self {
        match to.price.partial_cmp(&from.price) {
            Some(std::cmp::Ordering::Greater) => Self::Up,
            Some(std::cmp::Ordering::Less) => Self::Down,
            _ => Self::Flat,
        }
    }
}

/// The ruler's two arrows as (tail, tip), through the middle of the box
/// whose opposite corners are the projected start `a` and end `b`: one from
/// the start price to the end price, one from the start time to the end
/// time. The head goes on the tip.
fn ruler_arrows(a: (f64, f64), b: (f64, f64)) -> [((f64, f64), (f64, f64)); 2] {
    let (mid_x, mid_y) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    [((mid_x, a.1), (mid_x, b.1)), ((a.0, mid_y), (b.0, mid_y))]
}

/// The length of a ruler arrowhead's sides, for an arrow `length` long.
///
/// Full size on an arrow at least three heads long; on a shorter one the
/// head shrinks with it, so a narrow box is not all arrowhead; and an arrow
/// shorter than a full head has none, since it would be nothing else.
fn ruler_head(length: f64) -> Option<f64> {
    const HEAD: f64 = 9.0;
    match length {
        l if l >= 3.0 * HEAD => Some(HEAD),
        l if l >= HEAD => Some(l / 3.0),
        _ => None,
    }
}

/// A temporary range on the pointer layer: no scene rebuild on mouse motion.
fn draw_ruler(
    cr: &cairo::Context,
    state: &State,
    plan: &Layout,
    scale: &PriceScale,
    from: Anchor,
    to: Anchor,
) {
    let a = state.project_anchor(plan, scale, from);
    let b = state.project_anchor(plan, scale, to);
    let (x, y) = (a.0.min(b.0), a.1.min(b.1));
    let (w, h) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    let kind = state.instrument.as_ref().map(|i| i.kind);
    let measured = omacharts_engine::measurement::Measurement::between(from, to, &state.bars, kind);
    // The candles' own up and down, so a chart with them turned round
    // measures a rise in red; the label is the last-price tag of that direction, whose figure is
    // already chosen to read on its fill.
    let flat;
    let (colour, tag) = match RulerDirection::of(from, to) {
        RulerDirection::Up => (&state.scheme.up, &state.price_tags[0]),
        RulerDirection::Down => (&state.scheme.down, &state.price_tags[1]),
        RulerDirection::Flat => {
            flat = Tag::on(&state.scheme.neutral);
            (&state.scheme.neutral, &flat)
        }
    };
    cr.save().ok();
    cr.rectangle(plan.plot_x, plan.price_y, plan.plot_w, plan.price_h);
    cr.clip();
    colors::set_source_alpha(cr, colour, 0.18);
    cr.rectangle(x, y, w, h);
    cr.fill().ok();
    colors::set_source(cr, colour);
    cr.set_line_width(1.0);
    cr.rectangle(x, y, w, h);
    cr.stroke().ok();
    // One head on each arrow, at the end: the way price and time went.
    for (tail, tip) in ruler_arrows(a, b) {
        cr.move_to(tail.0, tail.1);
        cr.line_to(tip.0, tip.1);
        cr.stroke().ok();
        let Some(head) = ruler_head((tip.0 - tail.0).hypot(tip.1 - tail.1)) else {
            continue;
        };
        let angle = (tail.1 - tip.1).atan2(tail.0 - tip.0);
        for offset in [-0.5_f64, 0.5] {
            cr.move_to(tip.0, tip.1);
            cr.line_to(
                tip.0 + head * (angle + offset).cos(),
                tip.1 + head * (angle + offset).sin(),
            );
        }
        cr.stroke().ok();
    }
    // The percentage is what the eye goes to, so it leads in bold, with the
    // change after it on the same baseline and the span under both.
    let parts = measured.parts();
    let advance = |text: &str| cr.text_extents(text).map(|e| e.x_advance()).unwrap_or(0.0);
    let bold = |cr: &cairo::Context| {
        cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
        cr.set_font_size(13.0);
    };
    let normal = |cr: &cairo::Context| {
        cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(11.0);
    };
    bold(cr);
    let percent_w = parts.percent.as_deref().map_or(0.0, &advance);
    normal(cr);
    let gap = if parts.percent.is_some() { advance("  ") } else { 0.0 };
    let change_w = advance(&parts.change);
    let span_w = advance(&parts.span);
    let first_w = percent_w + gap + change_w;
    let bw = (first_w.max(span_w) + 20.0).min(plan.plot_w);
    let bx = (x + w / 2.0 - bw / 2.0).clamp(
        plan.plot_x,
        (plan.plot_x + plan.plot_w - bw).max(plan.plot_x),
    );
    let by = if y - 46.0 >= plan.price_y {
        y - 46.0
    } else {
        (y + h + 6.0)
            .min(plan.price_y + plan.price_h - 40.0)
            .max(plan.price_y)
    };
    colors::set_source(cr, &tag.fill);
    cr.rectangle(bx, by, bw, 40.0);
    cr.fill_preserve().ok();
    cr.stroke().ok();
    colors::set_source(cr, &tag.label);
    let left = bx + (bw - first_w) / 2.0;
    if let Some(percent) = &parts.percent {
        bold(cr);
        cr.move_to(left, by + 15.0);
        cr.show_text(percent).ok();
    }
    normal(cr);
    cr.move_to(left + percent_w + gap, by + 15.0);
    cr.show_text(&parts.change).ok();
    cr.move_to(bx + (bw - span_w) / 2.0, by + 30.0);
    cr.show_text(&parts.span).ok();
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

/// Half the side of the soft square round the grip under the hand. A whole
/// number of pixels more than the grip's own half, so both edges land on
/// the same pixel grid and the halo stays as crisp as the grip it frames.
const GRIP_HALO: f64 = 8.5;

/// How a grip the hand is on is drawn: a size up, in a square halo of its
/// own colour, and a wider, stronger one once it is held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lit {
    Hover,
    Held,
}

/// What has been drawn on the symbol, over the candles and the indicator
/// lines and under the strips, clipped to the price plot so a line drawn
/// off the top does not cross an RSI.
///
/// In two passes: `under` the bars for the kinds that are shading, and over
/// them for the rest — with the grips of the shaded ones, since a grip under
/// a candle is one nobody can find.
fn draw_drawings(cr: &cairo::Context, state: &State, plan: &Layout, scale: &PriceScale, under: bool) {
    if state.drawings.is_empty() || !state.show_drawings {
        return;
    }
    cr.save().ok();
    cr.rectangle(plan.plot_x, plan.price_y, plan.plot_w, plan.price_h);
    cr.clip();
    for (index, drawing) in state.drawings.iter().enumerate() {
        let shaded = drawing.kind.under_bars();
        let selected = state.selected.contains(&index);
        // An anchored VWAP's shaded bands are shading, as the study's are,
        // and go under the bars; its lines go over them with the rest.
        if under && drawing.kind == DrawingKind::Vwap {
            let projected = state.project(plan, scale, drawing);
            let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
            draw_vwap_shading(cr, state, &projected, drawing, &to_y);
            continue;
        }
        if shaded != under && !(shaded && selected) {
            continue;
        }
        let projected = state.project(plan, scale, drawing);
        let typing = state.editing == Some(index);
        let lit = selected.then(|| state.lit_grip(index)).flatten();
        if shaded && !under {
            let colour = drawing.style(&state.configs).colour.hex(&state.theme);
            draw_grips(cr, state, &projected, drawing, lit, &colour);
            continue;
        }
        let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
        draw_drawing(cr, state, &projected, drawing, selected && !shaded, typing, lit, &to_y);
        if typing {
            draw_editing_ground(cr, state, drawing);
        }
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
#[allow(clippy::too_many_arguments)]
fn draw_drawing(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    selected: bool,
    typing: bool,
    lit: Option<(Grip, Lit)>,
    to_y: &dyn Fn(f64) -> f64,
) {
    let style = drawing.style(&state.configs);
    let colour = style.colour.hex(&state.theme);
    match drawing.kind {
        DrawingKind::Line | DrawingKind::Horizontal | DrawingKind::Arrow | DrawingKind::Zigzag => {
            colors::set_source(cr, &colour);
            cr.set_line_width(style.width);
            cr.set_line_cap(cairo::LineCap::Round);
            // Every leg, which for all but the zig-zag is one. The ends are
            // pulled back under any head they wear: a round cap is a
            // half-disc of the line's own radius centred on the end of the
            // path, so a leg drawn all the way to the tip puts half its
            // width *past* the tip — a stub poking out of the arrowhead,
            // and the thicker the line the worse it looks.
            cr.set_line_join(cairo::LineJoin::Round);
            let legs = projected.segments();
            let last = legs.len().saturating_sub(1);
            for (n, (a, b)) in legs.iter().enumerate() {
                let (from, to) = headless_leg(*a, *b, style, n == 0, n == last);
                cr.move_to(from.0, from.1);
                cr.line_to(to.0, to.1);
            }
            let _ = cr.stroke();
            if let (Some(first), Some((a, b))) = (legs.first(), legs.last()) {
                if style.arrow.at_end() {
                    arrowhead(cr, *a, *b, style.width, style.head);
                }
                if style.arrow.at_start() {
                    arrowhead(cr, first.1, first.0, style.width, style.head);
                }
            }
        }
        DrawingKind::Rect => {
            let (x, y, w, h) = projected.bounds();
            colors::set_source_alpha(cr, &style.fill.hex(&state.theme), style.alpha);
            cr.rectangle(x, y, w, h);
            let _ = cr.fill();
            if style.border {
                // Inside the shape, not astride its edge. A stroke is
                // centred on the path it is given, so half of a four-pixel
                // edge would sit over the fill and half over the chart —
                // two bands of two different colours, which at that width
                // reads as two lines rather than one thick one. Pulled in
                // by half its own width it lands wholly on the fill and is
                // one colour all the way across. The half-pixel is what
                // keeps a hairline a hairline.
                let width = style.width.max(1.0);
                let half = width / 2.0;
                colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
                cr.set_line_width(width);
                cr.rectangle(
                    x.round() + half,
                    y.round() + half,
                    (w.round() - width).max(1.0),
                    (h.round() - width).max(1.0),
                );
                let _ = cr.stroke();
            }
        }
        // The same fill under the same edge as a box, in an ellipse drawn
        // inside the box the two anchors make: a circle scaled to the box
        // rather than an arc, so an ellipse dragged wide is wide. Not put on
        // the pixel grid the way a box is — there is no pixel grid for a
        // curve, and the only honest edge for one is the antialiased one.
        DrawingKind::Ellipse => {
            let (x, y, w, h) = projected.bounds();
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            // `inset` pulls the curve in from the box, for the same reason
            // a box's edge is pulled in: a stroke straddles its path, and an
            // edge half over the fill and half over the chart is two
            // colours.
            let ellipse = |cr: &cairo::Context, inset: f64| {
                let (rx, ry) = ((w / 2.0 - inset).max(0.1), (h / 2.0 - inset).max(0.1));
                cr.save().ok();
                cr.translate(x + w / 2.0, y + h / 2.0);
                cr.scale(rx, ry);
                cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
                // Back before the stroke, or the scale that made the circle
                // an ellipse would make the hairline an ellipse too: thick
                // on the flat sides and thin on the ends.
                cr.restore().ok();
            };
            colors::set_source_alpha(cr, &style.fill.hex(&state.theme), style.alpha);
            ellipse(cr, 0.0);
            let _ = cr.fill();
            if style.border {
                let width = style.width.max(1.0);
                colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
                cr.set_line_width(width);
                ellipse(cr, width / 2.0);
                let _ = cr.stroke();
            }
        }
        // Nothing but the words, which the label pass below draws for every
        // kind. A text drawing with nothing typed into it yet draws nothing
        // at all, which is the right picture of a caret waiting.
        DrawingKind::Text => {}
        DrawingKind::Profile => draw_profile_drawing(cr, state, projected, drawing, style, &colour, to_y),
        DrawingKind::Vwap => draw_vwap_drawing(cr, state, projected, drawing, style, &colour, to_y, selected),
        DrawingKind::Fib => draw_fib_drawing(cr, state, projected, drawing, style, &colour),
    }
    if !typing {
        draw_label(cr, state, projected, drawing, style);
    }
    if selected {
        // A text drawing wears no grips, so the box its words fill is what
        // says it is selected: a dashed outline a hair outside the glyphs,
        // in the drawing's own colour. The same outline would be noise on a
        // figure, which has grips to say it.
        if drawing.kind.is_text()
            && let Some((x, y, w, h)) = projected.words
        {
            colors::set_source_alpha(cr, &colour, drawings::BORDER_ALPHA);
            cr.set_line_width(1.0);
            cr.set_dash(&[3.0, 3.0], 0.0);
            let pad = SELECTED_TEXT_PAD;
            cr.rectangle(
                (x - pad).round() + 0.5,
                (y - pad).round() + 0.5,
                (w + 2.0 * pad).round(),
                (h + 2.0 * pad).round(),
            );
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        }
        draw_grips(cr, state, projected, drawing, lit, &colour);
    }
}

/// The squares at a selected drawing's grips, the one under the hand lit.
fn draw_grips(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    lit: Option<(Grip, Lit)>,
    colour: &str,
) {
    for grip in drawing.grips() {
        let Some((x, y)) = projected.grip(grip) else { continue };
        // The grip under the hand says so: without it, nothing tells a
        // hand reaching for a point whether a press takes the point or
        // the line, nor, once dragging, which point it has.
        let shine = lit.filter(|(at, _)| *at == grip).map(|(_, how)| how);
        if let Some(how) = shine {
            let (half, alpha) = match how {
                Lit::Hover => (GRIP_HALO, 0.22),
                Lit::Held => (GRIP_HALO + 2.0, 0.32),
            };
            colors::set_source_alpha(cr, colour, alpha);
            cr.rectangle(x - half, y - half, 2.0 * half, 2.0 * half);
            let _ = cr.fill();
        }
        let half = GRIP_HALF + if shine.is_some() { 1.0 } else { 0.0 };
        colors::set_source(cr, &state.theme.ui.background);
        cr.rectangle(x - half - 1.0, y - half - 1.0, 2.0 * half + 2.0, 2.0 * half + 2.0);
        let _ = cr.fill();
        colors::set_source(cr, colour);
        cr.rectangle(x - half, y - half, 2.0 * half, 2.0 * half);
        let _ = cr.fill();
    }
}

/// The next of the four states an arrowhead is in: no head, then each
/// shape in the order the picker offers them, then no head again.
fn step_head(
    arrow: omacharts_engine::Arrow,
    head: omacharts_engine::ArrowHead,
) -> (omacharts_engine::Arrow, omacharts_engine::ArrowHead) {
    use omacharts_engine::{Arrow, ArrowHead};
    if arrow == Arrow::None {
        return (Arrow::End, ArrowHead::ALL[0]);
    }
    let at = ArrowHead::ALL.iter().position(|h| *h == head).unwrap_or(0);
    match ArrowHead::ALL.get(at + 1) {
        Some(next) => (arrow, *next),
        None => (Arrow::None, ArrowHead::ALL[0]),
    }
}

/// How far outside the glyphs a selected text drawing's outline sits.
const SELECTED_TEXT_PAD: f64 = 3.0;

/// How far the plate under the caret reaches past the block it is under.
///
/// The same reach the halo has round a committed label, so the footprint of
/// the ground does not change when the edit ends.
const EDITING_PAD: f64 = 1.0;

/// The ground under the words being typed: the colour the ink was held
/// against, put down so that what is on screen while somebody types is as
/// readable as what they will get. See [`crate::text::draw`] for why a
/// label needs its ground drawn at all.
fn draw_editing_ground(cr: &cairo::Context, state: &State, drawing: &Drawing) {
    let Some((x, y, w, h)) = state.editing_at else { return };
    let style = drawing.style(&state.configs);
    let ground = drawings::text_ground(drawing.kind, style, &state.theme);
    colors::set_source(cr, &ground);
    let pad = EDITING_PAD;
    crate::shapes::rounded(cr, x - pad, y - pad, w + 2.0 * pad, h + 2.0 * pad, 3.0);
    let _ = cr.fill();
}

/// What the drawing says, where its placement puts it.
///
/// Every kind goes through here: for the text kind these are the drawing,
/// and for a line, a box or an ellipse they are its label. The ink is the
/// text colour held to the text floor against what it actually lands on —
/// the composited fill inside a filled figure, the chart's background
/// anywhere else — so an amber label on an amber box is amber where amber
/// reads and lifted where it does not.
fn draw_label(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    style: &omacharts_engine::Style,
) {
    let Some((x, y, _, _)) = projected.words else { return };
    let ground = drawings::text_ground(drawing.kind, style, &state.theme);
    let ink = drawings::text_colour(&style.text.colour, &state.theme, &ground);
    let at = crate::text::placement(drawing.kind, &drawing.text);
    crate::text::draw(cr, (x, y), &drawing.text, &style.text, at.alignment(), &ink, &ground);
}

/// One leg of a stroke, with either end pulled in by half the line's width
/// where a head is going to sit over it.
///
/// Only the outermost legs can wear one: a zig-zag's heads are at the ends
/// of the whole run, not at every corner.
fn headless_leg(
    a: (f64, f64),
    b: (f64, f64),
    style: &omacharts_engine::Style,
    first: bool,
    last: bool,
) -> ((f64, f64), (f64, f64)) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    if length < 1.0 {
        return (a, b);
    }
    // Never more than the leg has to give: a very short one with heads at
    // both ends keeps a sliver rather than turning inside out.
    let back = (style.width / 2.0).min(length / 2.0 - 0.5).max(0.0);
    let (ux, uy) = (dx / length * back, dy / length * back);
    let from = match first && style.arrow.at_start() {
        true => (a.0 + ux, a.1 + uy),
        false => a,
    };
    let to = match last && style.arrow.at_end() {
        true => (b.0 - ux, b.1 - uy),
        false => b,
    };
    (from, to)
}

/// A filled arrowhead at `tip`, pointing away from `tail`, sized to the
/// line's width so a thick line gets a head to match.
fn arrowhead(
    cr: &cairo::Context,
    tail: (f64, f64),
    tip: (f64, f64),
    width: f64,
    shape: omacharts_engine::ArrowHead,
) {
    crate::shapes::paint_head(cr, tail, tip, width, 6.0 + 3.0 * width, shape);
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

/// The one line an indicator shows on a chart whose source lacks what it
/// needs — "Needs a data feed with volume at price" — at the foot of the price
/// box, one above another if there are several. Nothing is drawn per bar.
fn draw_unmet(cr: &cairo::Context, state: &State, plot_x: f64, bottom: f64) {
    let mut y = bottom - 10.0;
    for drawn in state.shown_indicators() {
        let Output::Unmet(why) = &drawn.output else { continue };
        let text = format!("{} · {why}", drawn.indicator.kind.short_name());
        colors::set_source_alpha(cr, &state.theme.ui.text_muted, 0.8);
        cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cr.set_font_size(12.0);
        cr.move_to(plot_x + 10.0, y);
        let _ = cr.show_text(&text);
        y -= 16.0;
    }
}

/// Band shading and volume profiles, beneath the bars.
fn draw_indicator_fills(
    cr: &cairo::Context,
    state: &State,
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    for drawn in state.shown_indicators() {
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
                    fill_between(cr, &band.upper, &band.lower, &bands.starts, columns, to_y);
                }
            }
            Output::Profiles(profiles) => {
                draw_profiles(cr, state, drawn, profiles, columns, to_y);
            }
            // These draw in a strip of their own, where the price scale does
            // not reach, so there is nothing to do over the candles; and time
            // has its own ground beneath everything.
            Output::Volume { .. } | Output::Pane(_) | Output::Line(_) | Output::Time(_) | Output::Unmet(_) => {}
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
    for drawn in state.shown_indicators() {
        let lit = state.picked == Some(drawn.indicator.id);
        match &drawn.output {
            Output::Line(values) => {
                let stroke = drawn.indicator.stroke;
                if stroke.is_hidden() {
                    continue;
                }
                let point = |at: usize| series_point(values, columns, to_y, at);
                if lit {
                    selection::glow_line(cr, &state.theme, &drawn.color, stroke.width, columns, point);
                }
                cr.save().ok();
                cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
                colors::set_source(cr, &drawn.color);
                cr.set_line_width(stroke.width);
                stroke_series(cr, values, &[], columns, to_y);
                cr.restore().ok();
                if lit {
                    let latest = selection::latest(values);
                    selection::end_dot(cr, &state.theme, &drawn.color, columns, latest, point, |at| columns.source(at));
                }
            }
            Output::Bands(bands) => {
                // Every line it draws, each in its own colour, lit together:
                // the band is one thing. Gathered only while it is selected,
                // since every other frame has no use for the list.
                let lines: Vec<(&[Option<f64>], String, f64)> = match lit {
                    false => Vec::new(),
                    true => bands
                        .bands
                        .iter()
                        .filter(|band| band.enabled && !band.stroke.is_hidden())
                        .flat_map(|band| {
                            let colour = band_colour(band, drawn, &state.theme);
                            [
                                (band.upper.as_slice(), colour.clone(), band.stroke.width),
                                (band.lower.as_slice(), colour, band.stroke.width),
                            ]
                        })
                        .chain((!drawn.indicator.stroke.is_hidden()).then(|| {
                            (bands.center.as_slice(), drawn.color.clone(), drawn.indicator.stroke.width)
                        }))
                        .collect(),
                };
                if lit {
                    for (values, colour, width) in &lines {
                        let point = |at: usize| series_point(values, columns, to_y, at);
                        selection::glow_line(cr, &state.theme, colour, *width, columns, point);
                    }
                }
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
                    stroke_series(cr, &band.upper, &bands.starts, columns, to_y);
                    stroke_series(cr, &band.lower, &bands.starts, columns, to_y);
                }
                cr.restore().ok();

                let stroke = drawn.indicator.stroke;
                if !stroke.is_hidden() {
                    cr.save().ok();
                    cr.set_dash(&stroke.style.dashes(stroke.width), 0.0);
                    colors::set_source(cr, &drawn.color);
                    cr.set_line_width(stroke.width);
                    stroke_series(cr, &bands.center, &bands.starts, columns, to_y);
                    cr.restore().ok();
                }
                if lit {
                    for (values, colour, _) in &lines {
                        let point = |at: usize| series_point(values, columns, to_y, at);
                        let latest = selection::latest(values);
                        selection::end_dot(cr, &state.theme, colour, columns, latest, point, |at| columns.source(at));
                    }
                }
            }
            Output::Profiles(_) | Output::Volume { .. } | Output::Pane(_) | Output::Time(_) | Output::Unmet(_) => {}
        }
    }
}

/// Where a bar's column begins, by its index in the series: the arithmetic
/// the candles are placed with, so a session's shading starts on the same
/// pixel as its first candle at any zoom.
fn bar_left(columns: &Columns, bar: usize) -> f64 {
    columns.start + (bar as f64 - columns.first as f64) * columns.bar_px
}

/// Whether bars `from..=to` reach the screen at all.
fn on_screen_run(columns: &Columns, from: usize, to: usize) -> bool {
    to >= columns.first && from < columns.first + columns.visible
}

/// Shaded stretches of time, down the whole stack.
///
/// Under everything — the gridlines, the bands, the candles — because a
/// session is the ground a bar stands on, and anything drawn over a candle
/// hides the thing it is describing.
fn draw_time_ground(cr: &cairo::Context, state: &State, columns: &Columns, plan: &Layout) {
    let (left, right) = (plan.plot_x, plan.plot_x + plan.plot_w);
    let (top, bottom) = (plan.top, plan.top + plan.total_h);
    cr.save().ok();
    cr.rectangle(left, top, plan.plot_w, plan.total_h);
    cr.clip();
    for drawn in state.shown_indicators() {
        let Output::Time(time) = &drawn.output else { continue };
        for span in time.spans.iter().filter(|s| on_screen_run(columns, s.from, s.to)) {
            let (colour, alpha) =
                indicators::time::shade(span.color.as_ref(), span.alpha, span.ground, &state.theme);
            // Whole pixels, so two stretches that meet do not leave a
            // hairline of double shading between them.
            let x0 = bar_left(columns, span.from).max(left).round();
            let x1 = bar_left(columns, span.to + 1).min(right).round();
            if x1 > x0 {
                colors::set_source_alpha(cr, &colour, alpha);
                cr.rectangle(x0, top, x1 - x0, bottom - top);
                let _ = cr.fill();
            }
        }
    }
    cr.restore().ok();
}

/// A band's own colour, or the indicator's if it has none.
fn band_colour(band: &BandSeries, drawn: &Drawn, theme: &Theme) -> String {
    band.color
        .as_ref()
        .map(|choice| choice.resolve(theme))
        .unwrap_or_else(|| drawn.color.clone())
}

/// The plot's width from the area's: what a bar's width is measured on.
fn plot_width(area: &gtk::DrawingArea) -> f64 {
    (area.width() as f64 - PRICE_AXIS_W - PAD).max(1.0)
}

/// The time at a bar index, counted past either end of the series at the
/// timeframe's step: what a column in the empty room stands for.
fn time_at_column(state: &State, index: i64) -> Option<i64> {
    let len = state.bars.len() as i64;
    if index >= 0 && index < len {
        return Some(state.bars[index as usize].ts);
    }
    let step = state.timeframe.seconds();
    if step <= 0 {
        return None;
    }
    if index < 0 {
        state.bars.first().map(|b| state.timeframe.shift(b.ts, index))
    } else {
        state.bars.last().map(|b| state.timeframe.shift(b.ts, index - len + 1))
    }
}

/// The colours a volume pane paints a rising and a falling bar: the
/// indicator's own when somebody chose them, otherwise the candles' own —
/// the chart's candles, or the colouring the indicator chose for itself.
pub fn volume_colours(
    indicator: &Indicator,
    theme: &omacharts_engine::Theme,
    scheme: &BarScheme,
) -> (String, String) {
    let omacharts_engine::Params::Volume { colouring, up_color, down_color, .. } = &indicator.params
    else {
        return (scheme.up.clone(), scheme.down.clone());
    };
    let own;
    let scheme = match colouring {
        Some(colouring) => {
            own = colouring.scheme(theme);
            &own
        }
        None => scheme,
    };
    let pick = |choice: &Option<omacharts_engine::theme::ColorChoice>, fallback: &str| {
        choice.as_ref().map_or_else(|| fallback.to_string(), |c| c.resolve(theme))
    };
    (pick(up_color, &scheme.up), pick(down_color, &scheme.down))
}

/// How solid a volume pane's bars are drawn, kept within what the row offers
/// so a hand-edited chart cannot make them vanish.
pub fn volume_opacity(indicator: &Indicator) -> f64 {
    use omacharts_engine::indicators::{DEFAULT_VOLUME_OPACITY, MIN_VOLUME_OPACITY};
    match indicator.params {
        omacharts_engine::Params::Volume { opacity, .. } if opacity.is_finite() => {
            opacity.clamp(MIN_VOLUME_OPACITY, 1.0)
        }
        _ => DEFAULT_VOLUME_OPACITY,
    }
}

/// The colours one side of a volume profile is drawn in: its rows', and
/// the outline round them.
#[derive(Clone, Debug, PartialEq)]
struct ProfileInk {
    side: selection::Side,
    fill: String,
    edge: String,
}

impl ProfileInk {
    /// What the side is seen in, which is what its selection is drawn in:
    /// its rows' colour for the filled style, its outline's for the outline
    /// style, where the rows' colour is drawn nowhere.
    fn shown(&self, style: omacharts_engine::indicators::profile::Style) -> &str {
        match style {
            omacharts_engine::indicators::profile::Style::Filled => &self.fill,
            omacharts_engine::indicators::profile::Style::Outline => &self.edge,
        }
    }
}

/// The sides a volume profile is drawn as. A total is one, in the profile's
/// colour. A delta is two, in the colours the chart's candles go up and
/// down in, swapped or not, from whatever scheme they wear: bought and sold
/// read the way rising and falling do. An outline colour chosen is the
/// outline's on either; unchosen, the outline is the rows' own.
fn profile_inks(
    method: omacharts_engine::indicators::profile::Method,
    outline: Option<&omacharts_engine::theme::ColorChoice>,
    drawn: &Drawn,
    state: &State,
) -> Vec<ProfileInk> {
    use omacharts_engine::indicators::profile::Method;
    let ink = |side, fill: &str| ProfileInk {
        side,
        fill: fill.to_string(),
        edge: outline.map(|c| c.resolve(&state.theme)).unwrap_or_else(|| fill.to_string()),
    };
    match method {
        Method::Total => vec![ink(selection::Side::All, &drawn.color)],
        Method::Delta => {
            let (up, down) = state.delta_ink(drawn);
            vec![ink(selection::Side::Up, up), ink(selection::Side::Down, down)]
        }
    }
}

/// What a chart is told the house colouring is until it is told otherwise:
/// the one Preferences opens with.
const HOUSE_LOOK: (BarColouring, bool) = (BarColouring::Vivid, false);

/// A delta profile's bought and sold colours, for the house colouring and
/// then each colouring a delta can be given, in the order
/// [`DELTA_COLOURINGS`] offers them. Each is the colouring's rise and fall,
/// swapped when the house colouring is: red for a rise is said once, in
/// Preferences, and a delta reads the way the candles do.
fn delta_inks(theme: &Theme, (house, swapped): (BarColouring, bool)) -> [(String, String); 1 + DELTA_COLOURINGS.len()] {
    let pair = |colouring: BarColouring| {
        let scheme = colouring.scheme(theme).worn(swapped, false);
        (scheme.up, scheme.down)
    };
    let mut inks: [(String, String); 1 + DELTA_COLOURINGS.len()] = Default::default();
    inks[0] = pair(house);
    for (at, colouring) in DELTA_COLOURINGS.into_iter().enumerate() {
        inks[at + 1] = pair(colouring);
    }
    inks
}

/// A delta profile's bought and sold colours, as a pair of hex colours: the
/// house colouring's, or the one the profile chose of
/// [`DELTA_COLOURINGS`].
pub fn delta_colours(
    colouring: Option<BarColouring>,
    theme: &Theme,
    house: BarColouring,
    swapped: bool,
) -> (String, String) {
    let inks = delta_inks(theme, (house, swapped));
    let at = colouring.and_then(|c| DELTA_COLOURINGS.iter().position(|o| *o == c)).map_or(0, |at| at + 1);
    inks[at].clone()
}

/// How strongly a delta's rows are filled: all as strongly as a total's
/// value area, a delta having none to set apart.
const DELTA_ROW_ALPHA: f64 = 0.30;

/// One histogram per period, anchored where its period begins.
fn draw_profiles(
    cr: &cairo::Context,
    state: &State,
    drawn: &Drawn,
    profiles: &[Profile],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    use omacharts_engine::indicators::profile::Method;
    let (chosen, edged, outline, outline_width, poc_width, method, poc_shown) = match &drawn.indicator.params {
        omacharts_engine::Params::VolumeProfile {
            poc_color, outline: edged, outline_color, outline_width, poc_width, method, poc, ..
        } => (poc_color.as_ref(), *edged, outline_color.as_ref(), *outline_width, *poc_width, *method, *poc),
        _ => (None, false, None, 1.0, 1.0, Method::Total, true),
    };
    let poc = state.theme.companion_or(chosen, &drawn.color);
    let inks = profile_inks(method, outline, drawn, state);
    let look = selection::placed_look(state, drawn);
    let style = look.style;

    for profile in profiles {
        // Where a click finds it, too, so the two cannot drift apart.
        let area = selection::profile_box(profile, columns, &look);
        if area.span <= 0.0 || profile.max_volume <= 0.0 || profile.rows.is_empty() {
            continue;
        }

        match style {
            omacharts_engine::indicators::profile::Style::Filled => {
                let (area_low, area_high) = profile.value_area_bounds();
                for row in &profile.rows {
                    let Some(ink) = inks.iter().find(|ink| ink.side.length(row.volume) > 0.0) else {
                        continue;
                    };
                    let (from, to) = area.row(row.volume.abs() / profile.max_volume);
                    let top = to_y(row.high);
                    let height = (to_y(row.low) - top).max(1.0);
                    let alpha = match method {
                        Method::Total if row.low >= area_low && row.high <= area_high => 0.30,
                        Method::Total => 0.14,
                        Method::Delta => DELTA_ROW_ALPHA,
                    };
                    colors::set_source_alpha(cr, &ink.fill, alpha);
                    cr.rectangle(from, top, to - from, height);
                    let _ = cr.fill();
                }
                if edged {
                    cr.set_line_width(outline_width.max(0.5));
                    cr.set_line_join(cairo::LineJoin::Miter);
                    for ink in &inks {
                        colors::set_source_alpha(cr, &ink.edge, drawings::PROFILE_EDGE_ALPHA);
                        selection::profile_path(cr, &area, profile, to_y, ink.side);
                        let _ = cr.stroke();
                    }
                }
            }
            // Only the shape the rows make, so the candles under it stay
            // as plain as they are without it.
            omacharts_engine::indicators::profile::Style::Outline => {
                cr.set_line_width(outline_width.max(0.5));
                cr.set_line_join(cairo::LineJoin::Miter);
                for ink in &inks {
                    colors::set_source_alpha(cr, &ink.edge, 0.75);
                    selection::profile_path(cr, &area, profile, to_y, ink.side);
                    let _ = cr.stroke();
                }
            }
        }

        // The point of control, across the period it belongs to, in a colour
        // set against the profile rather than the same one brighter — it is a
        // different fact about the period, drawn on top of it.
        // Unless it is turned off, which leaves the rows to say it.
        if poc_shown {
            let y = to_y(profile.poc_price()).round() + 0.5;
            let (from, to) = area.row(1.0);
            colors::set_source_alpha(cr, &poc, 0.9);
            cr.set_line_width(poc_width.max(0.5));
            cr.move_to(from, y);
            cr.line_to(to, y);
            let _ = cr.stroke();
        }

        if state.picked == Some(drawn.indicator.id) {
            for ink in &inks {
                selection::outline_profile(cr, &state.theme, ink.shown(style), profile, &area, to_y, ink.side);
            }
        }
    }
}

/// A volume profile drawing: the study's rows across the whole span it
/// covers, set apart from the study by the edge round them, or drawn as only
/// that edge, with a point of control at the drawing's own stroke.
fn draw_profile_drawing(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    style: &drawings::Style,
    colour: &str,
    to_y: &dyn Fn(f64) -> f64,
) {
    use omacharts_engine::indicators::profile::{Align, Style as Shape};
    let Some(profile) = state.span_profile(drawing) else { return };
    if profile.max_volume <= 0.0 || profile.rows.is_empty() {
        return;
    }
    let (x, _, w, _) = projected.bounds();
    let look = &style.profile;
    let area = match look.align {
        Align::Left => selection::ProfileBox { edge: x, span: w, towards: 1.0 },
        Align::Right => selection::ProfileBox { edge: x + w, span: w, towards: -1.0 },
    };
    let opacity = look.opacity.clamp(0.0, 1.0);
    match look.shape {
        Shape::Filled => {
            let fill = style.fill.hex(&state.theme);
            let (area_low, area_high) = profile.value_area_bounds();
            for row in profile.rows.iter().filter(|row| row.volume > 0.0) {
                let (from, to) = area.row(row.volume / profile.max_volume);
                let top = to_y(row.high);
                let height = (to_y(row.low) - top).max(1.0);
                let inside = row.low >= area_low && row.high <= area_high;
                let alpha = if inside { style.alpha } else { style.alpha * drawings::PROFILE_ROW_SHARE };
                colors::set_source_alpha(cr, &fill, alpha * opacity);
                cr.rectangle(from, top, to - from, height);
                let _ = cr.fill();
            }
            // An edge the drawing can go without: off, the rows are all of it.
            let edge = if style.border { drawings::PROFILE_EDGE_ALPHA } else { 0.0 };
            colors::set_source_alpha(cr, colour, edge * opacity);
            cr.set_line_width(style.width.max(0.5));
        }
        Shape::Outline => {
            colors::set_source_alpha(cr, colour, opacity);
            cr.set_line_width(style.width.max(0.5));
        }
    }
    cr.set_line_join(cairo::LineJoin::Miter);
    selection::profile_path(cr, &area, &profile, to_y, selection::Side::All);
    let _ = cr.stroke();

    let y = to_y(profile.poc_price()).round() + 0.5;
    colors::set_source_alpha(cr, &look.poc.hex(&state.theme), drawings::PROFILE_POC_ALPHA);
    cr.set_line_width(look.poc_width.max(0.5));
    let (from, to) = area.row(1.0);
    cr.move_to(from, y);
    cr.line_to(to, y);
    let _ = cr.stroke();
}

/// A Fibonacci retracement: a level across its span at each ratio it
/// draws, each in its own colour, thickness and pattern at the strength a
/// level gets, the line from the swing's start to its end under them, and a
/// small label at the left of each level with its ratio and its price.
fn draw_fib_drawing(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    style: &drawings::Style,
    colour: &str,
) {
    let theme = &state.theme;
    let (left, right) = (projected.from.0.min(projected.to.0), projected.from.0.max(projected.to.0));
    cr.save().ok();
    // The swing itself, a quiet dashed guide.
    colors::set_source_alpha(cr, colour, drawings::FIB_TREND_ALPHA);
    cr.set_line_width(1.0);
    cr.set_dash(&[4.0, 4.0], 0.0);
    cr.move_to(projected.from.0, projected.from.1);
    cr.line_to(projected.to.0, projected.to.1);
    let _ = cr.stroke();
    let levels: Vec<(&drawings::FibLevel, f64)> = style.fib.enabled().zip(projected.levels.iter().copied()).collect();
    for (level, y) in &levels {
        if level.stroke.is_hidden() {
            continue;
        }
        let width = level.stroke.width;
        // On the pixel grid, as the chart's own lines are, so a hairline is
        // one pixel and not two half-lit ones.
        let y = match (width.round() as i64) % 2 {
            1 => y.round() + 0.5,
            _ => y.round(),
        };
        colors::set_source_alpha(cr, &level.paint(&style.colour).hex(theme), drawings::FIB_ALPHA);
        cr.set_line_width(width);
        cr.set_dash(&level.stroke.style.dashes(width), 0.0);
        cr.move_to(left, y);
        cr.line_to(right, y);
        let _ = cr.stroke();
    }
    cr.set_dash(&[], 0.0);
    if style.fib.labelled() && right - left >= 1.0 {
        draw_fib_labels(cr, state, drawing, style, &levels, left, right);
    }
    cr.restore().ok();
}

/// The ratio and the price of each level, small, above its left end, in
/// the level's colour as drawn and held to what small text needs.
///
/// The labels go where the span starts on screen: at the left edge of the
/// plot when the span runs off it, so a retracement scrolled half away still
/// reads, and nowhere when it has gone, or when what is left of it is
/// narrower than a label. A label that would sit on top of another is left
/// out: on a short swing the levels are closer together than a line of text
/// is tall. The swing's two ends, 0% and 100%, are placed first, so the
/// ones dropped are from the middle.
fn draw_fib_labels(
    cr: &cairo::Context,
    state: &State,
    drawing: &Drawing,
    style: &drawings::Style,
    levels: &[(&drawings::FibLevel, f64)],
    left: f64,
    right: f64,
) {
    let theme = &state.theme;
    let bg = &theme.ui.background;
    // The plot, as the chart clipped it for its drawings.
    let (plot_left, _, plot_right, _) = cr.clip_extents().unwrap_or((f64::MIN, 0.0, f64::MAX, 0.0));
    let (shown_left, shown_right) = (left.max(plot_left), right.min(plot_right));
    if shown_right <= shown_left {
        return;
    }
    let cut = left < plot_left;
    cr.select_font_face("sans-serif", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(drawings::FIB_LABEL_SIZE);
    let places = state.tick().map(|tick| omacharts_engine::on_tick(0.0, tick).1).unwrap_or(2);
    for (level, y) in fib_label_slots(levels).into_iter().map(|at| &levels[at]) {
        let price = || grouped_decimals(level.price(drawing.from.price, drawing.to.price), places);
        let text = match (style.fib.fib_labels, style.fib.price_labels) {
            (true, true) => format!("{}  {}", level.name(), price()),
            (true, false) => level.name(),
            _ => price(),
        };
        let Ok(extents) = cr.text_extents(&text) else { continue };
        let x = shown_left + 4.0;
        // What is left of a span scrolled off the left is no place for a
        // label that would overrun it onto the candles past its end.
        if cut && x + extents.x_advance() + 2.0 > shown_right {
            continue;
        }
        let drawn = omacharts_engine::theme::mix(&level.paint(&style.colour).hex(theme), bg, 1.0 - drawings::FIB_ALPHA);
        let ink = omacharts_engine::theme::held_to(&drawn, bg, drawings::TEXT_CONTRAST, None);
        // A plate of the chart's own background behind the text, so a label
        // laid over candles still reads, and reads as nothing else.
        let baseline = y - 4.0;
        colors::set_source_alpha(cr, bg, 0.85);
        cr.rectangle(
            (x - 2.0).round(),
            (baseline - drawings::FIB_LABEL_SIZE + 1.0).round(),
            (extents.x_advance() + 4.0).round(),
            drawings::FIB_LABEL_SIZE + 1.0,
        );
        let _ = cr.fill();
        colors::set_source(cr, &ink);
        cr.move_to(x, baseline);
        let _ = cr.show_text(&text);
    }
}

/// Which levels get a label, by where they are in `levels`: the swing's two
/// ends first, 0% and 100%, then each other level whose label would not sit
/// on one already placed, a line of text being taller than the gap between
/// two levels of a short swing.
fn fib_label_slots(levels: &[(&drawings::FibLevel, f64)]) -> Vec<usize> {
    let is_end = |level: &drawings::FibLevel| {
        drawings::same_ratio(level.ratio, 0.0) || drawings::same_ratio(level.ratio, 1.0)
    };
    let mut order: Vec<usize> = (0..levels.len()).collect();
    order.sort_by_key(|at| !is_end(levels[*at].0));
    let mut placed: Vec<usize> = Vec::new();
    for at in order {
        let y = levels[at].1;
        if placed.iter().any(|other| (y - levels[*other].1).abs() < drawings::FIB_LABEL_SIZE + 1.0) {
            continue;
        }
        placed.push(at);
    }
    placed
}

/// An anchored VWAP drawing: its line, from the anchor bar to the last, in
/// the drawing's colour, width and pattern, and the edges of each band it
/// shows, as the study draws its own: each band in its colour, or the
/// line's, at its own width and pattern. The shading is drawn under the
/// bars, by [`draw_vwap_shading`].
///
/// Selected, it glows as a selected study does, under everything it draws.
/// The bands do not: the line is the drawing, and its one grip is enough.
#[allow(clippy::too_many_arguments)]
fn draw_vwap_drawing(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    style: &drawings::Style,
    colour: &str,
    to_y: &dyn Fn(f64) -> f64,
    selected: bool,
) {
    let Some((start, line)) = state.anchored_vwap(drawing) else { return };
    let range = state.vwap_on_screen(start, line.len());
    let xs: Vec<f64> = projected.corners.iter().map(|(x, _)| *x).collect();
    let single = projected.corners.len() == 1;
    if selected {
        match single {
            false => selection::glow_path(
                cr,
                &state.theme,
                colour,
                style.width,
                projected.corners.iter().map(|at| Some(*at)),
            ),
            true => selection::glow_dot(cr, &state.theme, colour, projected.corners[0], style.width.max(1.5) + 3.0),
        }
    }
    cr.save().ok();
    cr.set_line_join(cairo::LineJoin::Round);
    for band in style.vwap.shown() {
        if band.stroke.is_hidden() {
            continue;
        }
        colors::set_source(cr, &band.paint(&style.colour).hex(&state.theme));
        cr.set_line_width(band.stroke.width);
        cr.set_dash(&band.stroke.style.dashes(band.stroke.width), 0.0);
        for side in [1.0, -1.0] {
            for (n, index) in range.clone().enumerate() {
                let Some(x) = xs.get(n) else { break };
                let value = line[index - start];
                let y = to_y(value.vwap + side * band.deviations * value.deviation);
                match n {
                    0 => cr.move_to(*x, y),
                    _ => cr.line_to(*x, y),
                }
            }
            let _ = cr.stroke();
        }
    }
    cr.restore().ok();
    if style.width <= 0.0 {
        return;
    }
    cr.save().ok();
    colors::set_source(cr, colour);
    match single {
        // One bar from the anchor: a dot where the line will start.
        true => {
            let (x, y) = projected.corners[0];
            cr.arc(x, y, style.width.max(1.5), 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
        false => {
            cr.set_line_join(cairo::LineJoin::Round);
            cr.set_line_width(style.width);
            cr.set_dash(&style.vwap.line.dashes(style.width), 0.0);
            // One path, so a dashed line's pattern runs on across the bars
            // rather than starting again at each.
            for (n, (x, y)) in projected.corners.iter().enumerate() {
                match n {
                    0 => cr.move_to(*x, *y),
                    _ => cr.line_to(*x, *y),
                }
            }
            let _ = cr.stroke();
        }
    }
    cr.restore().ok();
}

/// The shading of an anchored VWAP's shaded bands, between each one's two
/// edges, under the bars as the study's is.
fn draw_vwap_shading(
    cr: &cairo::Context,
    state: &State,
    projected: &Projected,
    drawing: &Drawing,
    to_y: &dyn Fn(f64) -> f64,
) {
    let style = drawing.style(&state.configs);
    if !style.vwap.shown().any(|band| band.fill) {
        return;
    }
    let Some((start, line)) = state.anchored_vwap(drawing) else { return };
    let range = state.vwap_on_screen(start, line.len());
    let xs: Vec<f64> = projected.corners.iter().map(|(x, _)| *x).collect();
    if xs.len() < 2 {
        return;
    }
    for band in style.vwap.shown().filter(|band| band.fill) {
        colors::set_source_alpha(cr, &band.paint(&style.colour).hex(&state.theme), band.fill_alpha);
        let edge = |index: usize, side: f64| {
            let value = line[index - start];
            to_y(value.vwap + side * band.deviations * value.deviation)
        };
        let points: Vec<(f64, usize)> = xs.iter().copied().zip(range.clone()).collect();
        for (n, (x, index)) in points.iter().enumerate() {
            match n {
                0 => cr.move_to(*x, edge(*index, 1.0)),
                _ => cr.line_to(*x, edge(*index, 1.0)),
            }
        }
        for (x, index) in points.iter().rev() {
            cr.line_to(*x, edge(*index, -1.0));
        }
        cr.close_path();
        let _ = cr.fill();
    }
}

/// Where a series indexed by bar is drawn in one column, if it has a value
/// there: the point `stroke_series` draws through.
fn series_point(
    values: &[Option<f64>],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
    at: usize,
) -> Option<(f64, f64)> {
    values.get(columns.source(at)).copied().flatten().map(|value| (columns.x(at), to_y(value)))
}

/// Stroke a series, breaking the path wherever it has no value, and where
/// `starts` says a new period begins.
fn stroke_series(
    cr: &cairo::Context,
    values: &[Option<f64>],
    starts: &[bool],
    columns: &Columns,
    to_y: &impl Fn(f64) -> f64,
) {
    let mut drawing = false;
    let mut last = usize::MAX;
    for at in 0..columns.len() {
        let x = columns.x(at);
        let source = columns.source(at);
        // A period that starts over is a new line, not the old one bent down
        // to its first price.
        if starts_between(starts, last, source) {
            drawing = false;
        }
        last = source;
        match values.get(source).copied().flatten() {
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

/// Whether a period starts after bar `last` and by bar `source`: the bars a
/// column took in since the one before it. A column that stands for several
/// bars is read from its last, so the start can be any of them.
fn starts_between(starts: &[bool], last: usize, source: usize) -> bool {
    if last == usize::MAX || source <= last {
        return false;
    }
    starts.get(last + 1..=source).is_some_and(|span| span.contains(&true))
}

/// Fill the region between two series.
fn fill_between(
    cr: &cairo::Context,
    upper: &[Option<f64>],
    lower: &[Option<f64>],
    starts: &[bool],
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

    let mut last = usize::MAX;
    for at in 0..columns.len() {
        let x = columns.x(at);
        let source = columns.source(at);
        if starts_between(starts, last, source) {
            flush(&mut run);
        }
        last = source;
        match (upper.get(source).copied().flatten(), lower.get(source).copied().flatten()) {
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

/// The decimals a multiple of `step` is written to exactly, and no more: none
/// for gridlines 20 apart, one for 2.5, two for 0.25. A gridline sits on a
/// round number, so anything past that is a zero saying nothing.
pub fn decimals_for(step: f64) -> usize {
    if step <= 0.0 {
        return 2;
    }
    (0..=8)
        .find(|&places| {
            let scaled = step * 10f64.powi(places as i32);
            (scaled - scaled.round()).abs() < 1e-6 * scaled.max(1.0)
        })
        .unwrap_or(8)
}

/// About how far apart, in pixels, two time labels should be.
const TIME_LABEL_GAP: f64 = 90.0;

/// Round stretches of time a tick can mark, finest first: the tick goes on
/// the first bar of each one.
#[derive(Clone, Copy)]
enum Every {
    Minutes(i64),
    Day,
    Week,
    Months(i64),
}

impl Every {
    const LADDER: [Every; 21] = [
        Every::Minutes(1),
        Every::Minutes(2),
        Every::Minutes(5),
        Every::Minutes(10),
        Every::Minutes(15),
        Every::Minutes(30),
        Every::Minutes(60),
        Every::Minutes(120),
        Every::Minutes(240),
        Every::Minutes(360),
        Every::Day,
        Every::Week,
        Every::Months(1),
        Every::Months(2),
        Every::Months(3),
        Every::Months(6),
        Every::Months(12),
        Every::Months(24),
        Every::Months(60),
        Every::Months(120),
        Every::Months(240),
    ];

    fn seconds(self) -> i64 {
        const DAY: i64 = 86_400;
        match self {
            Every::Minutes(n) => n * 60,
            Every::Day => DAY,
            Every::Week => 7 * DAY,
            // An average month, the length a monthly chart steps by, so a
            // monthly chart does not skip the one-month rung as too fine.
            Every::Months(n) => n * Timeframe::months(1).seconds(),
        }
    }

    /// Which stretch a moment falls in; a tick goes where this changes.
    fn period(self, at: chrono::NaiveDateTime) -> i64 {
        use chrono::{Datelike, Timelike};
        let day = at.date().num_days_from_ce() as i64;
        match self {
            Every::Minutes(n) => (day * 1_440 + (at.hour() * 60 + at.minute()) as i64).div_euclid(n),
            Every::Day => day,
            Every::Week => (day - at.weekday().num_days_from_monday() as i64).div_euclid(7),
            Every::Months(n) => (at.year() as i64 * 12 + at.month0() as i64).div_euclid(n),
        }
    }
}

/// Which columns carry a time tick, and about how many seconds apart they are.
///
/// Ticks go on round times — the first bar of an hour, a day, a month, a year —
/// using the finest round time that leaves a label room, counted on the bars
/// actually on screen so closed hours and weekends take no room. A tick every
/// so many bars instead drifts against the calendar: twenty-six weeks is not
/// six months.
///
/// Which round time to use is settled on how far apart its ticks fall; what
/// finally gets drawn is thinned so no two labels sit on top of each other.
fn axis_ticks(
    total: i64,
    plot_w: f64,
    step: i64,
    utc: bool,
    clock: Clock,
    time_at: impl Fn(i64) -> Option<i64>,
) -> (Vec<i64>, i64) {
    let times: Vec<Option<chrono::NaiveDateTime>> =
        (0..total).map(|column| clock.read(time_at(column)?, utc)).collect();
    // Lazily, so trying a rung that does not fit allocates nothing.
    let ticks = |every: Every| {
        let mut previous = None;
        times.iter().enumerate().filter_map(move |(column, at)| {
            let period = every.period((*at)?);
            let changed = previous.is_some_and(|p| p != period);
            previous = Some(period);
            changed.then_some(column as i64)
        })
    };
    // A session's first bar starts a new stretch, and the round time just
    // after it starts another: 15:30 and 16:00 land two bars apart on a 15m
    // chart and their labels overprint. The round time is what the axis is
    // for — the open only earned a tick by being first after the overnight
    // gap — so it takes the open's place rather than crowding in beside it.
    let bar_w = plot_w / total.max(1) as f64;
    let spaced = |columns: Vec<i64>| {
        let mut kept: Vec<i64> = Vec::with_capacity(columns.len());
        for column in columns {
            if kept.last().is_some_and(|last| (column - last) as f64 * bar_w < TIME_LABEL_GAP) {
                kept.pop();
            }
            kept.push(column);
        }
        kept
    };
    // Measured before thinning: thinning alone would leave every rung looking
    // like it fits, and the finest one would always win.
    //
    // A tick never lands on the first column, so n ticks share the plot with
    // the stretch before the first of them: n + 1 gaps, not n. Counting n
    // lets evenly spaced ticks — a yearly chart's, every two bars — sit just
    // under the gap, and thinning them then leaves only the last.
    let room = (plot_w / TIME_LABEL_GAP).max(2.0).floor() - 1.0;
    // The n is how many ticks the columns hold at the rung's average spacing,
    // first tick to last, not how many are on screen: that count moves by one
    // as a tick scrolls in or out at an edge, so a chart near the limit
    // flipped between years and half-years while it was dragged. The spacing
    // stays put wherever the view is.
    let fits = |every: Every| {
        let span = ticks(every).fold(None, |span, column| match span {
            None => Some((column, column, 0)),
            Some((first, _, gaps)) => Some((first, column, gaps + 1)),
        });
        match span {
            Some((first, last, gaps)) if gaps > 0 => {
                (total - 1) as f64 * gaps as f64 / (last - first) as f64 <= room
            }
            _ => true,
        }
    };
    let mut coarsest = Every::Months(240);
    for every in Every::LADDER {
        if every.seconds() < step {
            continue;
        }
        if fits(every) {
            return (spaced(ticks(every).collect()), every.seconds());
        }
        coarsest = every;
    }
    (spaced(ticks(coarsest).collect()), coarsest.seconds())
}

/// Label an axis tick at the detail the gap between ticks justifies.
///
/// The gap, not the whole span: twenty ticks across nine months are about a
/// fortnight apart, and labelling each of them "Mar 2026" prints the same
/// thing three times running. What decides the format is how far apart two
/// neighbouring labels are, because that is what has to tell them apart.
fn format_axis_time(ts: i64, tick_seconds: i64, intraday: bool, utc: bool, clock: Clock) -> String {
    let Some(dt) = clock.read(ts, utc) else {
        return String::new();
    };
    const DAY: i64 = 86_400;
    let time = clock.format.pattern();
    // Ticks a month or more apart get no clock, intraday or not: it would only
    // be the hour each month's first bar opened, and it pushes out the year.
    match tick_seconds {
        t if t >= 300 * DAY => dt.format("%Y").to_string(),
        t if t >= 25 * DAY => dt.format("%b %Y").to_string(),
        _ if !intraday => dt.format("%d %b").to_string(),
        t if t >= DAY => dt.format(&format!("%d %b {time}")).to_string(),
        t if t >= 4 * 3_600 => dt.format(&format!("%a {time}")).to_string(),
        _ => dt.format(time).to_string(),
    }
}

/// The crosshair label always carries the year — it is the one place you look
/// to know exactly which bar you are on.
fn format_time_full(ts: i64, timeframe: Timeframe, clock: Clock) -> String {
    let Some(dt) = clock.read(ts, timeframe.is_calendar()) else {
        return String::new();
    };
    match timeframe.unit {
        _ if timeframe.is_intraday() => {
            let written = dt.format(&format!("%d %b %Y {}", clock.format.pattern())).to_string();
            match clock.named {
                true => format!("{written} {}", clock.abbreviation(ts)),
                false => written,
            }
        }
        Unit::Month => dt.format("%b %Y").to_string(),
        Unit::Year => dt.format("%Y").to_string(),
        _ => dt.format("%a %d %b %Y").to_string(),
    }
}

/// The clock a chart reads its times off, and how it writes them.
///
/// The app's two settings, resolved for one chart: "the exchange's own"
/// becomes that chart's exchange's zone before it gets here.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Clock {
    pub zone: chrono_tz::Tz,
    pub format: TimeFormat,
    /// Name the zone on the crosshair. Only when each chart reads its own
    /// exchange's clock: two linked charts then disagree about the time of
    /// the same moment, and the abbreviation is what says why.
    pub named: bool,
}

impl Default for Clock {
    fn default() -> Clock {
        Clock {
            zone: ChartZone::default().for_instrument(None),
            format: TimeFormat::default(),
            named: false,
        }
    }
}

impl Clock {
    /// What the clock on the wall read at `ts`: this chart's zone, or UTC for
    /// bars that open on a UTC calendar boundary (see
    /// [`Timeframe::is_calendar`]) — a daily bar is a date, not a moment, and
    /// moving it into New York would put Monday's bar on Sunday.
    /// `CT`, `ET`, `JST`: the zone as the people in it write it, at `ts`.
    fn abbreviation(self, ts: i64) -> String {
        use chrono::TimeZone;
        let Some(at) = self.zone.timestamp_opt(ts, 0).single() else { return String::new() };
        let written = at.format("%Z").to_string();
        // Daylight and standard time are one clock to an American reader:
        // CDT and CST are both Central Time, written CT. Everywhere else the
        // zone's own letters are what people use — BST, CET, JST — and a
        // zone with none of its own comes back as an offset, said as it is.
        match written.as_str() {
            "EDT" | "EST" => "ET".to_string(),
            "CDT" | "CST" => "CT".to_string(),
            "MDT" | "MST" => "MT".to_string(),
            "PDT" | "PST" => "PT".to_string(),
            _ => written,
        }
    }

    fn read(self, ts: i64, utc: bool) -> Option<chrono::NaiveDateTime> {
        use chrono::{TimeZone, Utc};
        if utc {
            Some(Utc.timestamp_opt(ts, 0).single()?.naive_utc())
        } else {
            Some(self.zone.timestamp_opt(ts, 0).single()?.naive_local())
        }
    }
}

#[cfg(test)]
mod tests {

    /// An OHLC bar is as thick as asked while there is room, and thinner
    /// where there is not: a pixel's gap always stays between neighbours,
    /// with a pixel of tick either side where ticks are drawn, and it is
    /// never under a pixel.
    #[test]
    fn an_ohlc_bar_is_as_thick_as_asked_unless_its_neighbours_would_touch() {
        assert_eq!(ohlc_width(2, 8.0, true), 2.0);
        assert_eq!(ohlc_width(4, 12.0, true), 4.0);
        assert_eq!(ohlc_width(4, 6.0, true), 3.0, "a pixel of tick either side and a pixel's gap");
        assert_eq!(ohlc_width(2, 4.5, true), 1.0, "at this density two would hide the ticks");
        assert_eq!(ohlc_width(2, 3.0, false), 2.0, "no ticks to keep room for");
        assert_eq!(ohlc_width(2, 2.5, false), 1.0);
        assert_eq!(ohlc_width(3, 1.0, false), 1.0);
        assert_eq!(ohlc_width(1, 20.0, true), 1.0);
    }

    use super::*;
    use crate::marks::{AxisChip, MarkPreview};

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
        // A symbol the chosen feed has no name for. It used to be a click
        // that did nothing at all — the chart kept the previous symbol and
        // said nothing — so the one thing this must not be is silence or
        // "No data for this symbol", which blames the ticker.
        let unserved = placeholder_text(true, false, Some(FetchFailure::Unserved));
        assert_eq!(unserved, FetchFailure::Unserved.message());
        assert!(unserved.contains("charts this symbol"), "{unserved}");
        assert_ne!(unserved, FetchFailure::NoSuchSymbol.message());
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
        let plan = with_rows(&[None, Some(1), Some(2)]);
        assert_eq!(plan.controls(1), vec![Control::Down, Control::Close], "the price is above it");
        assert_eq!(plan.controls(2), vec![Control::Up, Control::Close], "nothing below the last");
    }

    /// A strip stays on its side of the price, so the only strip on a chart
    /// has nowhere to go and is offered only its ×.
    #[test]
    fn the_only_strip_on_a_chart_is_not_offered_the_way_across_the_price() {
        let under = with_rows(&[None, Some(1)]);
        assert_eq!(under.controls(1), vec![Control::Close]);
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

    /// Ink at one pixel of a surface a series was drawn on.
    fn inked_at(surface: &mut cairo::ImageSurface, x: usize, y: usize) -> bool {
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        data[y * stride + x * 4 + 3] > 0
    }

    /// Scrolled to within a screen of its first bar, a chart asks for older
    /// bars once for that bar, and when they come in before it the view
    /// stays on the bars it was showing.
    #[test]
    fn scrolled_back_a_chart_asks_for_older_bars_once_and_stays_put_as_they_come() {
        let mut state = charted(1_000);
        state.instrument = Some(named("ES"));
        state.visible = 100;
        assert_eq!(state.wants_older(), None, "at the live edge, far from the first bar");

        state.anchored = false;
        state.first = 50;
        let first = state.bars[0].ts;
        assert_eq!(state.wants_older(), Some(first));
        assert_eq!(state.wants_older(), None, "asked again for the same first bar");

        let shown = state.bars[state.slice().0].ts;
        let mut bars: Vec<Bar> = (1..=300).rev().map(|i| Bar { ts: first - i * 60, ..state.bars[0] }).collect();
        bars.extend(state.bars.clone());
        state.replace_series(named("ES"), state.timeframe, bars);
        assert_eq!(state.bars[state.slice().0].ts, shown, "the view moved as older bars came in");
        assert_eq!(state.first, 350);
        assert_eq!(state.wants_older(), None, "three screens from the new first bar");
    }

    /// Loading… has a bar under it while its load says how far it has got,
    /// filled that far, and none for a load that cannot say, or once the
    /// chart is no longer loading.
    #[test]
    fn a_load_that_says_how_far_it_has_got_has_a_bar_under_loading() {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.instrument = Some(named("ES"));
        state.loading = true;
        let bar_row = (100.0 + PROGRESS_BELOW_BASELINE) as usize + 1;
        let drawn = |state: &State| {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 300, 200).unwrap();
            draw_placeholder(&cairo::Context::new(&surface).unwrap(), 300.0, 200.0, state);
            let row: Vec<u8> = (0..300)
                .map(|x| {
                    let stride = surface.stride() as usize;
                    surface.data().unwrap()[bar_row * stride + x * 4 + 3]
                })
                .collect();
            row
        };

        assert!(drawn(&state).iter().all(|&alpha| alpha == 0), "a bar for a load that cannot say");
        state.progress = Some(0.5);
        let row = drawn(&state);
        let inked: Vec<usize> = (0..300).filter(|&x| row[x] > 0).collect();
        let (left, right) = (*inked.first().unwrap(), *inked.last().unwrap());
        assert!((148..=152).contains(&((left + right) / 2)), "centred: {left}..{right}");
        assert!(row[left + 2] > row[right - 2], "filled from the left, over a fainter track");
        state.loading = false;
        assert!(drawn(&state).iter().all(|&alpha| alpha == 0), "a bar once loaded");
    }

    /// A yearly VWAP's bands jump at the new year: the old year's envelope
    /// is wide and the new one starts on the line. Drawn as one path, that
    /// jump is a stroke straight down the chart; it has to be two lines.
    #[test]
    fn a_line_and_its_shading_break_where_a_period_starts_over() {
        let bars = ramp(20);
        let columns = Columns::of(&bars, 0, 20, 0.0, 200.0);
        let mut upper = vec![Some(150.0); 10];
        upper.extend(vec![Some(20.0); 10]);
        let lower: Vec<Option<f64>> = upper.iter().map(|v| v.map(|v| v + 10.0)).collect();
        let mut starts = vec![false; 20];
        starts[10] = true;
        let to_y = |value: f64| value;

        for (breaks, joined) in [(&starts[..], false), (&[][..], true)] {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 200).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                cr.set_line_width(2.0);
                stroke_series(&cr, &upper, breaks, &columns, &to_y);
            }
            // Between the last bar of one period and the first of the next,
            // half way down the jump.
            assert_eq!(inked_at(&mut surface, 100, 85), joined, "stroke, breaks {:?}", !breaks.is_empty());

            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 200, 200).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                cr.set_source_rgb(1.0, 1.0, 1.0);
                fill_between(&cr, &upper, &lower, breaks, &columns, &to_y);
            }
            assert_eq!(inked_at(&mut surface, 100, 85), joined, "fill, breaks {:?}", !breaks.is_empty());
        }
    }

    /// Zoomed out, a column stands for several bars and is read from the
    /// last of them, so the start of a period can fall on any bar it took in.
    #[test]
    fn a_period_that_starts_inside_a_column_still_breaks_the_line() {
        let mut starts = vec![false; 12];
        starts[5] = true;
        assert!(starts_between(&starts, 3, 7), "taken in by the column");
        assert!(!starts_between(&starts, 7, 11), "already behind");
        assert!(!starts_between(&starts, usize::MAX, 7), "nothing before the first column to break from");
        assert!(!starts_between(&[], 3, 7), "a series with no periods never breaks");
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
        let columns = Columns::of(&bars, 0, 3000, PAD, 750.0);
        assert_eq!(columns.len(), 750, "a column a pixel");
        for (at, column) in columns.bars.iter().enumerate() {
            let group = &bars[at * 4..at * 4 + 4];
            assert_eq!(column.ts, group[0].ts, "a column starts when its first bar does");
            assert_eq!(column.open, group[0].open, "first open");
            assert_eq!(column.close, group[3].close, "last close");
            assert_eq!(column.high, highest(group), "highest high");
            assert_eq!(column.low, lowest(group), "lowest low");
            let summed: f64 = group.iter().map(|bar| bar.volume).sum();
            assert_eq!(column.volume, summed / 2.0, "a group's worth of volume, two groups to a pixel");
        }
        // Which is also what keeps the price axis the same axis: the scale is
        // worked out from the highest high and the lowest low on screen.
        assert_eq!(highest(&columns.bars), highest(&bars));
        assert_eq!(lowest(&columns.bars), lowest(&bars));
    }

    #[test]
    fn a_gap_between_two_bars_in_one_column_stays_a_gap() {
        let bar = |low: f64, high: f64| Bar { ts: 0, open: low, high, low, close: high, volume: 1.0 };
        // Four bars to two pixels: a gap inside the first column, none in the second.
        let bars = [bar(100.0, 110.0), bar(120.0, 130.0), bar(125.0, 135.0), bar(130.0, 140.0)];
        let columns = Columns::of(&bars, 0, 4, PAD, 2.0);
        assert_eq!(columns.wicks(0).collect::<Vec<_>>(), [(110.0, 100.0), (130.0, 120.0)]);
        assert_eq!(columns.wicks(1).collect::<Vec<_>>(), [(140.0, 125.0)]);
    }

    /// Bars rarely divide evenly into pixels. The columns still have to tile
    /// the slice — no bar in two of them, none in none of them.
    #[test]
    fn columns_tile_the_bars_even_when_the_two_do_not_divide() {
        let bars = ramp(1000);
        let columns = Columns::of(&bars, 13, 1000, PAD, 149.5);
        assert_eq!(columns.len(), 150, "a column a pixel");
        let mut next = 0;
        for at in 0..columns.len() {
            let span = columns.span(at);
            assert_eq!(span.start, next, "column {at} begins at {}, not after the last", span.start);
            assert!(!span.is_empty(), "column {at} stands for nothing");
            next = span.end;
        }
        assert_eq!(next, 1000, "the last bars are in no column");
        assert_eq!(highest(&columns.bars), highest(&bars), "a high went missing");
        assert_eq!(lowest(&columns.bars), lowest(&bars), "a low went missing");
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
        let (first, visible, wide) = (25usize, 1000usize, 150.0);
        let bars = ramp(first + visible);
        let columns = Columns::of(&bars[first..], first, visible, PAD, wide);
        assert_eq!(columns.len(), 150);

        let mut previous: Option<usize> = None;
        for at in 0..columns.len() {
            let span = columns.span(at);
            let (from, to) = (first + span.start, first + span.end);
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

    /// Scrolling a zoomed-out chart by a bar has to slide the columns along,
    /// not cut them again. Cut from the first bar on screen, every column
    /// took a different open and close at every step, and candles all across
    /// the chart flickered between rising and falling.
    #[test]
    fn scrolling_by_a_bar_keeps_every_whole_column_as_it_was() {
        let (visible, wide) = (1000usize, 299.0);
        let bars: Vec<Bar> = ramp(1100)
            .into_iter()
            .enumerate()
            .map(|(i, bar)| {
                let swing = if i % 3 == 0 { 2.0 } else { -1.5 };
                Bar { open: bar.close - swing, ..bar }
            })
            .collect();
        let whole = |first: usize| -> Vec<Bar> {
            let columns = Columns::of(&bars[first..first + visible], first, visible, PAD, wide);
            (0..columns.len())
                .filter(|&at| at > 0 && at + 1 < columns.len())
                .map(|at| columns.bars[at])
                .collect()
        };
        for first in 40..48 {
            let (before, after) = (whole(first), whole(first + 1));
            let same = after.iter().filter(|column| before.contains(column)).count();
            assert!(same + 2 >= after.len(), "from bar {first}: {same} of {} columns kept", after.len());
        }
    }

    /// The other half of the mapping: the things drawn across a span of bars
    /// rather than at one — a volume profile's width, say — have to land on
    /// the same columns the candles did.
    #[test]
    fn a_span_of_bars_covers_the_columns_those_bars_are_drawn_in() {
        let (first, visible, wide) = (11usize, 600usize, 300.0);
        let bars = ramp(first + visible);
        let columns = Columns::of(&bars[first..], first, visible, PAD, wide);
        assert!(
            (columns.width_of(first, first + visible) - wide).abs() < 1e-9,
            "the visible bars are the whole plot"
        );
        // A column is the pixel its bars fall in, so each bar begins inside
        // the pixel it is drawn in.
        for at in 1..columns.len() {
            let from = first + columns.span(at).start;
            let left = columns.x(at) - columns.bar_w / 2.0;
            assert!(
                (columns.left_of(from) - (left + 0.5)).abs() <= 0.5 + 1e-9,
                "bar {from} starts at {} rather than {left}",
                columns.left_of(from)
            );
        }
    }

    /// `count` daily bars from day `from`, rising a dollar a day.
    fn daily(from: i64, count: usize) -> Vec<Bar> {
        (0..count as i64)
            .map(|i| {
                let price = 100.0 + i as f64;
                Bar { ts: (from + i) * 86_400, open: price, high: price + 1.0, low: price - 1.0, close: price, volume: 1.0 }
            })
            .collect()
    }

    /// The empty room past a zoomed-out chart's data is dated a column at a
    /// time, carrying on from the columns over the data: one bar's time to
    /// a column, four bars to each, made a year of room read as a quarter
    /// of one. The room before the data is not dated at all.
    #[test]
    fn the_room_past_zoomed_out_bars_is_dated_by_the_column() {
        let day = 86_400;
        let bars = daily(0, 1000);
        let columns = Columns::of(&bars, 0, 1000, PAD, 125.0);
        assert_eq!((columns.bars_each(), columns.len()), (8.0, 125));
        let at = |column: i64| axis_time(&columns.bars, Timeframe::days(1), columns.bars_each(), column);
        assert_eq!(at(3), Some(24 * day), "a column over the data is its first bar's");
        assert_eq!(at(124), Some(992 * day));
        assert_eq!(at(125), Some(1000 * day), "the room begins where the next column would");
        assert_eq!(at(134), Some(1072 * day));
        assert_eq!(at(-1), None, "no history before the first bar");
    }

    /// A chart with a volume strip under it, which is the arrangement that
    /// has corner controls as well as a crosshair.
    /// The price scale over what is on screen, as a frame would draw it.
    fn drawn_scale(state: &State, width: f64, height: f64) -> PriceScale {
        let plan = layout(state, width, height);
        let (first, visible) = state.slice();
        price_range(state, &state.bars[first..first + visible], &plan).expect("bars to measure")
    }

    fn same_scale(a: PriceScale, b: PriceScale) -> bool {
        (a.low - b.low).abs() < 1e-9 && (a.high - b.high).abs() < 1e-9 && a.log == b.log
    }

    /// The bug: with the price scale off automatic, scrolling sideways still
    /// changed how big the bars looked, because the held window was a zoom
    /// and an offset on the fit of whatever bars were on screen — and the
    /// fit moved with them. Held means held: a pan, a trackpad scroll, a
    /// time zoom or new bars move the time axis and nothing else.
    #[test]
    fn a_held_price_scale_stays_put_as_the_view_scrolls() {
        for log in [false, true] {
            let mut state = charted(400);
            state.instrument = Some(named("ES"));
            state.price_log = log;
            state.visible = 160;
            state.overhang = 0;
            let (w, h) = (1200.0, 600.0);
            let plot_w = layout(&state, w, h).plot_w;
            let fitted = drawn_scale(&state, w, h);
            // Taking hold of the axis starts from exactly what was drawn.
            state.hold_price(w, h);
            assert!(!state.price_auto);
            let held = drawn_scale(&state, w, h);
            assert!(same_scale(held, fitted), "{log}: nothing jumps on taking hold");
            let columns = state.columns();
            for step in [-150.0, -60.0, 90.0, 40.0] {
                state.pan_by(step, plot_w);
                assert!(same_scale(drawn_scale(&state, w, h), held), "{log}: moved by a pan of {step}");
                assert_eq!(state.columns(), columns, "a bar stays as wide");
            }
            state.zoom_time(2.0, 0.5, plot_w);
            assert!(same_scale(drawn_scale(&state, w, h), held), "{log}: moved by a time zoom");
            // A refresh of the same series, with bars that reach higher.
            state.replace_series(named("ES"), state.timeframe, ramp(420));
            assert!(same_scale(drawn_scale(&state, w, h), held), "{log}: moved by new bars");
            assert!(!state.price_auto, "and is still held");
        }
    }

    /// Automatic is what it was: the scale follows the bars on screen.
    #[test]
    fn an_automatic_price_scale_follows_the_view_as_it_scrolls() {
        let mut state = charted(400);
        state.visible = 160;
        state.overhang = 0;
        let (w, h) = (1200.0, 600.0);
        let plot_w = layout(&state, w, h).plot_w;
        let before = drawn_scale(&state, w, h);
        state.pan_by(-200.0, plot_w);
        let after = drawn_scale(&state, w, h);
        assert!(after.high < before.high, "the older, lower bars are fitted");
        assert!(state.price_auto);
    }

    /// Stretching a held scale and dragging it up and down are still worked
    /// out on the window it was holding, and letting go of it fits again.
    #[test]
    fn a_held_price_scale_is_stretched_from_where_it_was_and_let_go_of() {
        let mut state = charted(400);
        state.visible = 160;
        state.overhang = 0;
        let (w, h) = (1200.0, 600.0);
        let plot_w = layout(&state, w, h).plot_w;
        let fitted = drawn_scale(&state, w, h);
        state.scale_price(2.0, w, h);
        let stretched = drawn_scale(&state, w, h);
        assert!((stretched.span() - fitted.span() / 2.0).abs() < 1e-9, "twice as tall a price");
        state.pan_by(-120.0, plot_w);
        assert!(same_scale(drawn_scale(&state, w, h), stretched));
        state.free_price();
        assert!(state.price_auto);
        let refitted = drawn_scale(&state, w, h);
        assert!(!same_scale(refitted, stretched), "fits the bars now on screen");
        // Held again, it holds this fit, not the one it let go of.
        state.hold_price(w, h);
        assert!(same_scale(drawn_scale(&state, w, h), refitted));
    }

    /// What a restart reads back: the prices a held scale was left
    /// showing, zoomed and shifted, brought back on a chart that has no
    /// bars yet and kept through the first series it is given — at another
    /// size, at the live edge, with bars that have traded since — on a
    /// linear axis and a log one.
    #[test]
    fn a_held_price_scale_comes_back_after_a_restart() {
        for log in [false, true] {
            let mut state = charted(400);
            state.instrument = Some(named("ES"));
            state.price_log = log;
            state.visible = 160;
            state.overhang = 0;
            let (w, h) = (1200.0, 600.0);
            let plot_h = layout(&state, w, h).price_h;
            state.scale_price(3.0, w, h);
            state.price_offset = 40.0 / plot_h / state.price_zoom;
            let left = drawn_scale(&state, w, h);
            let (low, high) = state.held_window().expect("a held scale has prices");
            assert!(low < high);

            let mut restored = charted(0);
            restored.price_log = log;
            restored.hold_window(low, high);
            assert!(!restored.price_auto, "{log}: held from the start");
            restored.replace_series(named("ES"), restored.timeframe, ramp(450));
            restored.visible = 160;
            for (w, h) in [(1200.0, 600.0), (800.0, 900.0)] {
                assert!(same_scale(drawn_scale(&restored, w, h), left), "{log}: the same prices at {w}x{h}");
            }
            let (again_low, again_high) = restored.held_window().expect("still held");
            assert!((again_low - low).abs() < 1e-9 && (again_high - high).abs() < 1e-9, "{log}: written down the same");

            // Another symbol is not this one's prices, as before.
            restored.replace_series(named("NQ"), restored.timeframe, ramp(450));
            assert!(!same_scale(drawn_scale(&restored, w, h), left), "{log}: another symbol holds its own fit");
        }
    }

    /// Nothing to write down while the scale fits itself, and nothing left
    /// once a reset or auto scale has let go of a held one.
    #[test]
    fn a_price_scale_that_fits_itself_holds_no_prices() {
        let (w, h) = (1200.0, 600.0);
        let mut state = charted(400);
        assert_eq!(state.held_window(), None, "automatic");
        state.scale_price(2.0, w, h);
        let _ = drawn_scale(&state, w, h);
        assert!(state.held_window().is_some());
        state.free_price();
        assert_eq!(state.held_window(), None, "auto scale let go of it");
        state.hold_window(120.0, 180.0);
        assert_eq!(state.held_window(), Some((120.0, 180.0)));
        state.reset_view();
        assert!(state.price_auto);
        assert_eq!(state.held_window(), None, "a reset let go of it");
        // Nothing that is a range of prices is held.
        for (low, high) in [(f64::NAN, 1.0), (5.0, 5.0), (9.0, 1.0), (0.0, f64::INFINITY)] {
            state.hold_window(low, high);
            assert!(state.price_auto, "{low}..{high}");
        }
    }

    /// A chart with its volume strip, indicator 1, and a cumulative delta's,
    /// indicator 2, under the price.
    fn with_delta(bars: usize) -> State {
        let mut state = charted(bars);
        let indicator = Indicator::new(2, indicators::Kind::Cvd);
        let output = indicators::compute(&indicator, &state.bars, 0, state.timeframe, None);
        assert!(matches!(&output, Output::Pane(pane) if pane.delta.is_some()), "a cumulative delta's strip");
        state.indicators.push(Drawn { color: "#5588ff".to_string(), output, indicator });
        state
    }

    /// The middle of a strip's row, on its axis and in its plot.
    fn strip_points(state: &State, id: u32, w: f64, h: f64) -> ((f64, f64), (f64, f64)) {
        let plan = layout(state, w, h);
        let row = plan.rows.iter().find(|row| row.pane == Some(id)).expect("the strip's row");
        let y = row.top + row.height / 2.0;
        ((w - PRICE_AXIS_W / 2.0, y), (plan.plot_x + plan.plot_w / 2.0, y))
    }

    /// The cumulative delta's strip zooms on its own scale, from its axis,
    /// Ctrl and the wheel over it, a pinch on it or a drag: the price's
    /// scale stays fitting itself, the volume strip too, and the strip it
    /// zoomed keeps its range as the chart scrolls. The volume strip is
    /// zoomed the same way, on the same code.
    #[test]
    fn a_strip_zooms_on_its_own_scale_whatever_the_price_s_does() {
        let (w, h) = (1200.0, 600.0);
        let mut state = with_delta(400);
        state.visible = 160;
        let price = drawn_scale(&state, w, h);
        let ((axis_x, axis_y), (plot_x, plot_y)) = strip_points(&state, 2, w, h);
        assert_eq!(state.axis_at(axis_x, axis_y, w, h, false), Some(Axis::Strip(2)), "its own axis");
        assert_eq!(state.axis_at(plot_x, plot_y, w, h, true), Some(Axis::Strip(2)), "Ctrl or a pinch over it");
        assert_eq!(state.axis_at(plot_x, plot_y, w, h, false), None, "the wheel alone pans or zooms time");
        let plan = layout(&state, w, h);
        let price_y = plan.price_y + plan.price_h / 2.0;
        assert_eq!(state.axis_at(axis_x, price_y, w, h, false), Some(Axis::Price), "the price's axis is the price's");

        let fit = state.strip_fit(2, w, h).expect("a fit");
        assert_eq!(state.strip_window(2, Some(fit)), Some(fit), "fitting itself until zoomed");
        state.scale_strip(2, 2.0, w, h);
        let (low, high) = state.strip_window(2, None).expect("held once zoomed");
        assert!(((high - low) - (fit.1 - fit.0) / 2.0).abs() < 1e-6, "half the range: {fit:?} to {low}..{high}");
        assert!(((low + high) - (fit.0 + fit.1)).abs() < 1e-6, "about its middle");
        assert!(state.price_auto, "the price's scale is untouched");
        assert!(same_scale(drawn_scale(&state, w, h), price), "the price shows what it did");
        assert!(!state.strip_held.contains_key(&1), "the volume strip still fits itself");

        // Held, it does not follow the bars as the chart scrolls.
        state.pan_by(-60.0, w - PRICE_AXIS_W - PAD);
        assert_eq!(state.strip_window(2, state.strip_fit(2, w, h)), Some((low, high)));

        // A pinch on it, spread to twice as far apart: half again.
        let pinch = state.pinch_at(plot_x, plot_y, w, h).expect("a pinch on the strip");
        assert_eq!(pinch, Pinch::Strip { id: 2, from: (low, high) });
        state.pinch(pinch, 2.0);
        let (pinched_low, pinched_high) = state.strip_window(2, None).unwrap();
        assert!(((pinched_high - pinched_low) - (high - low) / 2.0).abs() < 1e-6);
        assert!(state.pinch_at(plot_x, price_y, w, h).is_none(), "a pinch on the price's plot is not the chart's");
        assert!(state.price_auto);

        // A drag in it moves its own scale, and the price's not at all.
        assert!(matches!(state.strip_pan_at(plot_y, w, h), Some(StripPan { id: 2, held: Some(_), .. })));
        assert_eq!(state.strip_pan_at(price_y, w, h), None, "a drag on the price pans the price");

        // The volume strip, on the same code: from nought to its tallest bar
        // until it is zoomed.
        let ((_, volume_y), _) = strip_points(&state, 1, w, h);
        assert_eq!(state.axis_at(axis_x, volume_y, w, h, false), Some(Axis::Strip(1)));
        let volume = state.strip_fit(1, w, h).expect("a volume fit");
        assert_eq!(volume.0, 0.0);
        state.scale_strip(1, 4.0, w, h);
        assert!(state.strip_held.contains_key(&1));
        assert_eq!(state.strip_held.get(&2), Some(&(pinched_low, pinched_high)), "each strip its own");

        // Drawn on its own scale: the strip looks different, the price's
        // rows not at all.
        let auto = with_delta(400);
        let rows = |pixels: &[u8], from: f64, to: f64| pixels[(from as usize) * w as usize * 4..(to as usize) * w as usize * 4].to_vec();
        let mut left = auto;
        left.visible = 160;
        left.pan_by(-60.0, w - PRICE_AXIS_W - PAD);
        let plain = frame(w, h, |cr| draw(cr, w, h, &left));
        let zoomed = frame(w, h, |cr| draw(cr, w, h, &state));
        let strip = layout(&state, w, h).rows.into_iter().find(|row| row.pane == Some(2)).unwrap();
        assert_eq!(rows(&plain, plan.price_y + 2.0, plan.price_y + plan.price_h - 2.0), rows(&zoomed, plan.price_y + 2.0, plan.price_y + plan.price_h - 2.0), "the price redrawn");
        assert_ne!(rows(&plain, strip.top, strip.top + strip.height), rows(&zoomed, strip.top, strip.top + strip.height), "the strip on its own scale");
    }

    /// A mark is dragged on the step of the contract it trades once one is
    /// set, not the series' step: a quarter-point chart trading a contract
    /// in halves lands only on halves.
    #[test]
    fn a_mark_is_dragged_on_the_traded_contract_s_tick() {
        let (w, h) = (1200.0, 600.0);
        let mut state = charted(400);
        state.visible = 160;
        let plan = layout(&state, w, h);
        let ys: Vec<f64> = (0..40).map(|at| plan.price_y + plan.price_h * f64::from(at) / 40.0).collect();
        let on = |state: &State, tick: f64| {
            ys.iter().all(|y| {
                let (price, _) = state.tick_price_at(w, h, *y).expect("a price");
                ((price / tick) - (price / tick).round()).abs() < 1e-9
            })
        };
        state.mark_tick = Some(0.5);
        assert!(on(&state, 0.5), "every price on the traded half point");
        assert_eq!(state.dragged_tick(), Some(0.5), "what the rules are asked about");
        state.mark_tick = None;
        assert_eq!(state.dragged_tick(), state.tick(), "the series' own once unset");
    }

    /// A strip held on its own scale lets go of it when the series is
    /// another symbol's or timeframe's, whose values are nowhere near, as
    /// the price's held window does; a refresh of the same series keeps it.
    #[test]
    fn a_zoomed_strip_fits_itself_again_on_another_series() {
        let mut state = with_delta(400);
        state.instrument = Some(named("ES"));
        let bars = state.bars.clone();
        state.hold_strip(2, -500.0, 500.0);
        state.replace_series(named("ES"), state.timeframe, bars.clone());
        assert_eq!(state.strip_window(2, None), Some((-500.0, 500.0)), "a refresh let go of it");
        state.replace_series(named("NQ"), state.timeframe, bars.clone());
        assert!(state.strip_held.is_empty() && state.strip_kinds.is_empty(), "held across a symbol");
        state.hold_strip(2, -500.0, 500.0);
        state.replace_series(named("NQ"), Timeframe::minutes(5), bars);
        assert!(state.strip_held.is_empty(), "held across a timeframe");
    }

    /// A strip's range goes with its indicator: removed, its id is let go
    /// of, so one added later under the same id fits itself, and one of
    /// another kind under a held id, as a template can bring, does not
    /// take on a range held for something else.
    #[test]
    fn a_strip_s_range_goes_with_its_indicator() {
        let mut state = with_delta(400);
        state.hold_strip(2, -500.0, 500.0);
        assert_eq!(state.strip_kinds.get(&2), Some(&indicators::Kind::Cvd));
        let delta = state.indicators.pop().unwrap();
        state.prune_strips();
        assert!(state.strip_held.is_empty() && state.strip_kinds.is_empty(), "kept for a removed indicator");
        state.indicators.push(delta.clone());
        state.prune_strips();
        assert_eq!(state.strip_window(2, None), None, "a new indicator under the id inherited it");

        // Another kind under the id: brought back for a CVD, drawn as an RSI.
        state.hold_strip(2, -500.0, 500.0);
        let indicator = Indicator::new(2, indicators::Kind::Rsi);
        let output = indicators::compute(&indicator, &state.bars, 0, state.timeframe, None);
        state.indicators = vec![Drawn { color: "#5588ff".to_string(), output, indicator }];
        state.prune_strips();
        assert!(state.strip_held.is_empty(), "an RSI took a CVD's range");

        // Brought back before it is drawn, with its kind: kept for it.
        state.indicators.clear();
        state.strip_kinds.insert(2, indicators::Kind::Cvd);
        state.hold_strip(2, -500.0, 500.0);
        state.indicators = vec![delta];
        state.prune_strips();
        assert_eq!(state.strip_window(2, None), Some((-500.0, 500.0)));
    }

    /// A reset, from the axis or the menu, lets go of the price's scale and
    /// of every strip's; nothing that is not a range is held.
    #[test]
    fn a_reset_lets_go_of_the_price_and_every_strip() {
        let (w, h) = (1200.0, 600.0);
        let mut state = with_delta(400);
        state.scale_price(2.0, w, h);
        state.scale_strip(1, 2.0, w, h);
        state.scale_strip(2, 3.0, w, h);
        assert!(!state.price_auto && state.strip_held.len() == 2);
        state.reset_view();
        assert!(state.price_auto, "the price's");
        assert!(state.strip_held.is_empty(), "the strips'");

        state.scale_strip(2, 3.0, w, h);
        state.free_price();
        state.free_strips();
        assert!(state.strip_held.is_empty(), "the axis's double-click");
        for (low, high) in [(f64::NAN, 1.0), (5.0, 5.0), (9.0, 1.0), (0.0, f64::INFINITY)] {
            state.hold_strip(2, low, high);
            assert!(state.strip_held.is_empty(), "{low}..{high}");
        }
    }

    /// Panning past either end opens room there — as much as
    /// `OVERHANG_PX` at this zoom, never the whole view — which new bars
    /// do not close, and which a reset puts back to the resting margin.
    #[test]
    fn the_view_can_hang_past_the_last_bar() {
        let mut state = charted(400);
        state.visible = 160;
        state.anchored = true;
        let plot_w = 1600.0; // 10px a bar: 500px is 50 columns
        // A chart opens with the resting margin past its last bar.
        assert_eq!(state.overhang, RIGHT_MARGIN);
        assert_eq!(state.slice(), (250, 150));
        // From flush against the edge.
        state.overhang = 0;
        assert_eq!(state.slice(), (240, 160));
        assert_eq!(state.columns(), 160);
        // Forward in time, past the end: the room opens and is capped.
        state.pan_by(20.0, plot_w);
        assert_eq!(state.overhang, 20);
        assert_eq!(state.slice(), (260, 140));
        assert!(state.anchored);
        state.pan_by(1000.0, plot_w);
        assert_eq!(state.overhang, 50);
        assert_eq!(state.slice(), (290, 110));
        // Still 160 columns across, so a bar is the same width as before.
        assert_eq!(state.columns(), 160);
        // New bars arrive: the margin is kept, and the view follows.
        state.bars = ramp(410);
        assert_eq!(state.slice(), (300, 110));
        assert_eq!(state.overhang, 50);
        // Back towards history: the room closes before the view moves.
        state.pan_by(-50.0, plot_w);
        assert_eq!(state.overhang, 0);
        assert_eq!(state.slice(), (250, 160));
        assert!(state.anchored);
        state.pan_by(-10.0, plot_w);
        assert_eq!(state.slice(), (240, 160));
        assert!(!state.anchored);
        // A reset is the live edge with the resting margin past it.
        state.pan_by(100.0, plot_w);
        assert_eq!(state.overhang, 50);
        state.reset_view();
        assert_eq!(state.overhang, RIGHT_MARGIN);
        assert_eq!(state.slice(), (260, 150));
        // Zoomed far in, the cap is short of the whole view.
        state.visible = 12;
        state.pan_by(1000.0, 1600.0);
        assert_eq!(state.overhang, 3);
        assert_eq!(state.slice(), (401, 9));
        // And the same room before the first bar, at the other end.
        state.reset_view();
        state.pan_by(-1000.0, plot_w);
        assert_eq!(state.lead, 50);
        assert_eq!(state.slice(), (0, 110));
        assert_eq!(state.left_edge(), -50);
        assert!(!state.anchored);
        // Zooming works on the columns across the plot, so zooming out
        // still shows more. About the middle, it pushes the edge further
        // into the room, which is so many pixels: 500px is 100 of the
        // 5px columns there are now, and no more.
        state.zoom_time(2.0, 0.5, plot_w);
        assert_eq!(state.columns(), 320);
        assert_eq!(state.lead, 100);
        assert_eq!(state.slice(), (0, 220));
        // Zooming back in about the middle leaves the room behind.
        state.zoom_time(0.25, 0.5, plot_w);
        assert_eq!(state.columns(), 80);
        assert_eq!(state.lead, 0);
        assert_eq!(state.slice(), (20, 80));
    }

    fn named(symbol: &str) -> Instrument {
        Instrument {
            symbol: symbol.into(),
            name: symbol.into(),
            kind: omacharts_engine::symbols::InstrumentKind::Equity,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    /// Every day from `from` to `to`, or only the weekdays, stamped at noon
    /// UTC so the day is the same on any machine's clock.
    fn days(from: chrono::NaiveDate, to: chrono::NaiveDate, weekends: bool) -> Vec<Bar> {
        use chrono::Datelike;
        from.iter_days()
            .take_while(|day| *day <= to)
            .filter(|day| weekends || day.weekday().number_from_monday() <= 5)
            .map(|day| {
                let ts = day.and_hms_opt(12, 0, 0).expect("noon").and_utc().timestamp();
                Bar { ts, open: 100.0, high: 101.0, low: 99.0, close: 100.0, volume: 1.0 }
            })
            .collect()
    }

    fn ymd(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(y, m, d).expect("a day")
    }

    /// The year to date is so many days, not so many bars: bitcoin's year
    /// has its weekends in it and a stock's does not, and flipping from one
    /// to the other counts again rather than keeping the first one's zoom.
    #[test]
    fn a_range_is_counted_again_for_every_series() {
        use omacharts_engine::range::Period;
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        let (start, today) = (ymd(2025, 1, 1), ymd(2026, 10, 19));
        state.replace_series(named("BTC"), Timeframe::days(1), days(start, today, true));
        state.show_range(Range::ToDate(1, Period::Year));
        assert_eq!(state.slice().1, 292, "1 January to 19 October, every day");
        assert_eq!(state.columns(), 292 + RIGHT_MARGIN);

        let stock = days(start, today, false);
        let weekdays = stock.iter().filter(|bar| bar.ts >= days(ymd(2026, 1, 1), today, true)[0].ts).count();
        state.replace_series(named("AAPL"), Timeframe::days(1), stock);
        assert_eq!(state.slice().1, weekdays, "the stock's year to date, weekdays only");
        assert_eq!(state.range, Some(Range::ToDate(1, Period::Year)));
    }

    /// The month to date grows a bar a day.
    #[test]
    fn a_range_follows_new_bars() {
        use omacharts_engine::range::Period;
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.replace_series(named("BTC"), Timeframe::days(1), days(ymd(2026, 6, 1), ymd(2026, 10, 19), true));
        state.show_range(Range::ToDate(1, Period::Month));
        assert_eq!(state.slice().1, 19);

        let next = days(ymd(2026, 10, 20), ymd(2026, 10, 20), true)[0];
        omacharts_engine::stream::upsert(&mut state.bars, next);
        state.fit_range();
        assert_eq!(state.slice().1, 20);
    }

    /// A year or a month that is over shows just its own bars, and stays
    /// on them as new ones come in.
    #[test]
    fn a_range_that_has_ended_stops_on_its_last_day() {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.replace_series(named("BTC"), Timeframe::days(1), days(ymd(2025, 1, 1), ymd(2026, 10, 19), true));
        let shown = |state: &State| {
            let (first, visible, _) = state.view();
            let date = |at: usize| chrono::DateTime::from_timestamp(state.bars[at].ts, 0).map(|t| t.date_naive());
            (date(first), date(first + visible - 1), visible)
        };
        state.show_range(Range::Month(2026, 9));
        assert_eq!(shown(&state), (Some(ymd(2026, 9, 1)), Some(ymd(2026, 9, 30)), 30));

        let next = days(ymd(2026, 10, 20), ymd(2026, 10, 20), true)[0];
        omacharts_engine::stream::upsert(&mut state.bars, next);
        state.fit_range();
        assert_eq!(shown(&state), (Some(ymd(2026, 9, 1)), Some(ymd(2026, 9, 30)), 30));

        state.show_range(Range::Year(2025));
        assert_eq!(shown(&state), (Some(ymd(2025, 1, 1)), Some(ymd(2025, 12, 31)), 365));
    }

    /// A hand moving the view is choosing where to look, and the range
    /// stops being true the moment it does.
    #[test]
    fn a_zoom_or_a_pan_lets_go_of_the_range() {
        use omacharts_engine::range::Span;
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        let mut state = State::blank(theme, scheme);
        state.replace_series(named("BTC"), Timeframe::days(1), days(ymd(2025, 1, 1), ymd(2026, 10, 19), true));
        state.show_range(Range::Back(3, Span::Month));
        state.zoom_time(2.0, 0.5, 800.0);
        assert_eq!(state.range, None);

        state.show_range(Range::Back(3, Span::Month));
        state.pan_by(-20.0, 800.0);
        assert_eq!(state.range, None);

        state.show_range(Range::Back(3, Span::Month));
        state.reset_view();
        assert_eq!(state.range, None);
    }

    /// A series shorter than the view still opens with the resting margin
    /// past it, spread across the series and the margin rather than giving
    /// up bars to it.
    #[test]
    fn a_short_series_opens_with_room_past_it() {
        let mut state = charted(5);
        state.reset_view();
        assert_eq!(state.columns(), 5 + RIGHT_MARGIN);
        assert_eq!(state.slice(), (0, 5));
        assert_eq!(state.left_edge(), 0);
    }

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

    /// A cumulative delta's axis keeps a futures total whole and grouped,
    /// and shortens a stock's millions so they fit the axis.
    /// A cumulative delta's axis is ruled in one round step, 1, 2 or 5
    /// times a power of ten: −2,200 to 1,000 down a strip as tall as the
    /// price axis has rows for sixteen figures is written every 200, zero
    /// among them, whole and grouped, the step following the strip as it
    /// is zoomed. K, M or B only when a figure in full would not fit.
    #[test]
    fn a_cumulative_delta_s_axis_is_written_in_round_steps() {
        let (step, ticks) = delta_ticks(-2_200.0, 1_000.0, 16.0 * DELTA_LABEL_GAP);
        assert_eq!(step, 200.0);
        assert_eq!(ticks.first(), Some(&-2_200.0));
        assert_eq!(ticks.last(), Some(&1_000.0));
        assert_eq!(ticks.len(), 17);
        assert!(ticks.contains(&0.0), "zero not written");
        let roomy = |_: &str| true;
        let column = delta_figures(&ticks, step, &roomy);
        assert_eq!(column[0], "-2,200");
        assert_eq!(column[11], "0");
        assert!(column.iter().all(|text| !text.contains('.')), "{column:?}");
        assert_eq!(column[16], "1,000");

        // Zoomed in on it, the step follows; a short strip has fewer.
        assert_eq!(delta_ticks(-400.0, 400.0, 16.0 * DELTA_LABEL_GAP).0, 50.0);
        assert_eq!(delta_ticks(-2_200.0, 1_000.0, 6.0 * DELTA_LABEL_GAP).0, 1_000.0);
        // Never finer than one contract.
        assert_eq!(delta_ticks(-3.0, 3.0, 16.0 * DELTA_LABEL_GAP).0, 1.0);

        // A stock's millions are shortened when they would not fit, all of
        // the column alike and to the step's precision; a future's never.
        let (step, ticks) = delta_ticks(-1_200_000.0, 2_000_000.0, 16.0 * DELTA_LABEL_GAP);
        assert_eq!(step, 200_000.0);
        let narrow = |text: &str| text.len() <= 7;
        let column = delta_figures(&ticks, step, &narrow);
        assert_eq!((column[0].as_str(), column[6].as_str(), column[7].as_str(), column[16].as_str()), ("-1.2M", "0", "0.2M", "2.0M"));
        let (step, ticks) = delta_ticks(-18_000.0, 6_000.0, 16.0 * DELTA_LABEL_GAP);
        assert_eq!(delta_figures(&ticks, step, &narrow)[0], "-18,000");
        assert_eq!(delta_figure(250_400.0, &narrow), "250,400");
        assert_eq!(delta_figure(-1_424_661.0, &narrow), "-1.42M");
    }

    #[test]
    fn a_cumulative_delta_is_written_short_on_the_axis_only_when_it_is_large() {
        assert_eq!(contracts(-2_145.0), "-2,145");
        assert_eq!(contracts(18_821.4), "18,821");
        assert_eq!(contracts(250_400.0), "250K");
        assert_eq!(contracts(11_215_413.0), "11.2M");
        assert_eq!(contracts(-1_424_661.0), "-1.42M");
        assert_eq!(contracts(2_500_000_000.0), "2.50B");
    }

    /// A volume pane on Default, which every volume pane before its
    /// Colouring was a delta's is, paints as the chart's candles do; one of
    /// the three palettes, or a Monochrome kept from before, paints as that
    /// palette does, as it always did.
    #[test]
    fn a_volume_pane_on_default_is_painted_as_the_chart_s_candles() {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = BarColouring::Quiet.scheme(&theme);
        let mut indicator = Indicator::new(1, indicators::Kind::Volume);
        assert_eq!(volume_colours(&indicator, &theme, &scheme), (scheme.up.clone(), scheme.down.clone()));
        for own in [BarColouring::Vivid, BarColouring::Soft, BarColouring::Quiet, BarColouring::Monochrome] {
            if let omacharts_engine::Params::Volume { colouring, .. } = &mut indicator.params {
                *colouring = Some(own);
            }
            let painted = own.scheme(&theme);
            assert_eq!(volume_colours(&indicator, &theme, &scheme), (painted.up, painted.down), "{own:?}");
        }
    }

    /// A chart of the regular session out of hours: the last price is the
    /// feed's newest trade, not the last close drawn, following each
    /// trade; the scale reaches it while
    /// the chart shows its end, and not once it is panned back.
    #[test]
    fn the_last_price_is_the_feed_s_newest_trade_whatever_the_bars_drawn() {
        let mut state = charted(100);
        let last = *state.bars.last().unwrap();
        assert_eq!(last_price(&state), Some(last.close));

        let plan = layout(&state, 600.0, 400.0);
        let (first, visible, _) = state.view();
        let (low, high) = price_range(&state, &state.bars[first..first + visible], &plan).unwrap().prices();
        let away = high + (high - low);
        state.latest = Some(away);
        assert_eq!(last_price(&state), Some(away), "not the close drawn");
        let (_, reach) = price_range(&state, &state.bars[first..first + visible], &plan).unwrap().prices();
        assert!(reach >= away, "the scale reaches it: {reach} for {away}");
        let (_, back) = price_range(&state, &state.bars[..50], &plan).unwrap().prices();
        assert!(back < away, "panned back, the bars alone");

        state.latest = Some(last.close - 1.0);
        assert_eq!(last_price(&state), Some(last.close - 1.0), "the next trade");

        // The line and the tag stand at it.
        let to_y = |price: f64| 200.0 - (price - (last.close - 1.0));
        let lined = frame(600.0, 400.0, |cr| draw_last_price_line(cr, &state, 0.0, 500.0, 0.0, &to_y));
        let row = (to_y(last.close - 1.0).round() + 0.5) as usize;
        let stride = 600 * 4;
        assert!(lined[row * stride..(row + 1) * stride].iter().any(|byte| *byte != 0), "a line on its row");
    }

    /// The last price is drawn in the theme's own colour for it, the tag
    /// and the line alike, and in one somebody set on a theme of their own.
    #[test]
    fn the_last_price_is_in_the_theme_s_colour() {
        let mut theme = omacharts_engine::theme::builtin_themes().remove(0);
        let state = State::blank(theme.clone(), omacharts_engine::theme::theme_bars(&theme));
        let derived = omacharts_engine::theme::theme_last_price(&theme);
        assert_eq!(state.last_ink.line, derived);
        assert_eq!(state.last_ink.tag, Tag::on(&derived));

        theme.last_price = Some("#ffcc00".into());
        let mut state = charted(20);
        state.last_ink = LastInk::of(&theme);
        assert_eq!(state.last_ink.tag, Tag::on("#ffcc00"));
        let close = state.bars.last().unwrap().close;
        let to_y = |price: f64| 100.0 - (price - close);
        let pixels = frame(200.0, 200.0, |cr| draw_last_price_line(cr, &state, 0.0, 200.0, 0.0, &to_y));
        let row = &pixels[100 * 200 * 4..101 * 200 * 4];
        // Cairo's bytes are blue, green, red, alpha: a yellow has no blue.
        let inked: Vec<&[u8]> = row.chunks(4).filter(|pixel| pixel[3] != 0).collect();
        assert!(!inked.is_empty(), "a line on the price's row");
        assert!(inked.iter().all(|pixel| pixel[0] == 0 && pixel[2] > pixel[1]), "in the yellow set: {:?}", inked[0]);
    }

    /// Draw something onto a surface of this size and hand back its pixels.
    pub(super) fn frame(width: f64, height: f64, paint: impl FnOnce(&cairo::Context)) -> Vec<u8> {
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

    /// Rules for the tests' marks: a position of `net` held at `avg`, a
    /// point worth `point`, exits on the side that closes it — a limit
    /// above the market and a stop below it for a long — and what an exit
    /// would realise as the label in hand.
    #[derive(Clone, Debug, PartialEq)]
    struct Exits {
        avg: f64,
        net: i32,
        point: f64,
        /// Each draggable mark: its id, whether it sells, whether it is a
        /// stop, its size.
        orders: Vec<(String, bool, bool, u32)>,
        profit: String,
        loss: String,
    }

    impl Exits {
        /// A limit waits for a better price and a stop for a worse one.
        fn rests(stop: bool, sell: bool, at: f64, last: f64) -> bool {
            match stop == sell {
                true => at < last,
                false => at > last,
            }
        }

        /// The order a drag would make or move: whether it is a stop,
        /// whether it sells, and its size.
        fn dragged(&self, drag: &MarkDrag) -> Option<(bool, bool, u32)> {
            match drag.tab {
                Some(tab) => Some((tab == "stop", self.net > 0, self.net.unsigned_abs())),
                None => self.orders.iter().find(|(id, ..)| id == drag.mark).map(|(_, sell, stop, qty)| (*stop, *sell, *qty)),
            }
        }

        fn money(value: f64) -> String {
            let sign = if value < 0.0 { "-" } else { "+" };
            let cents = (value.abs() * 100.0).round() as u64;
            format!("{sign}${}.{:02}", grouped_decimals((cents / 100) as f64, 0), cents % 100)
        }
    }

    impl MarkRules for Exits {
        fn allowed(&self, drag: &MarkDrag) -> bool {
            let (Some(last), Some((stop, sell, _))) = (drag.last, self.dragged(drag)) else { return false };
            Exits::rests(stop, sell, drag.at, last)
        }

        fn preview(&self, drag: &MarkDrag, allowed: bool) -> MarkPreview {
            if !allowed {
                return MarkPreview { text: "Not allowed".into(), colour: None };
            }
            match self.dragged(drag) {
                Some((_, sell, qty)) if self.net != 0 && sell == (self.net > 0) => {
                    let closed = f64::from(qty.min(self.net.unsigned_abs()));
                    let pnl = (drag.at - self.avg) * f64::from(self.net.signum()) * closed * self.point;
                    let colour = if pnl < 0.0 { &self.loss } else { &self.profit };
                    MarkPreview { text: Exits::money(pnl), colour: Some(colour.clone()) }
                }
                _ => AnyPrice.preview(drag, allowed),
            }
        }
    }

    /// The test marks' look: a tab of `kind` in `colour`, dashed as asked.
    fn exit(stop: bool, colour: &str, style: LineStyle, net: i32) -> MarkTab {
        let weight = if stop { 2.0 } else { 1.5 };
        MarkTab {
            id: if stop { "stop" } else { "limit" }.into(),
            text: if stop { "STP" } else { "L" }.into(),
            tag: format!("{} {}", if stop { "STP" } else { "L" }, net.unsigned_abs()),
            colour: colour.into(),
            style,
            outline: weight / 1.5,
            weight,
        }
    }

    /// A working order as a mark: a line, a ×, its price on the axis, and
    /// to be taken to another one.
    fn order_mark(id: &str, price: f64, tag: &str, stop: bool, colour: &str, style: LineStyle) -> PriceMark {
        PriceMark {
            chip: Some(AxisChip { text: grouped_decimals(price, 2), filled: false }),
            style,
            line: Some(if stop { 2.0 } else { 1.5 }),
            closable: true,
            draggable: true,
            ..PriceMark::new(id, price, tag, colour)
        }
    }

    /// A position as a mark: a heavier line, a ×, its tabs, over the line
    /// joining linked marks, and no chip.
    fn position(price: f64, tag: &str, body: &str, colour: &str, tabs: Vec<MarkTab>) -> PriceMark {
        PriceMark {
            body: Some(body.into()),
            line: Some(2.0),
            closable: true,
            tabs,
            raised: true,
            ..PriceMark::new("position", price, tag, colour)
        }
    }

    /// The mark `id` on the chart, to change.
    fn mark_mut<'a>(state: &'a mut State, id: &str) -> &'a mut PriceMark {
        state.marks.lines.iter_mut().find(|mark| mark.id == id).expect("the mark")
    }

    fn the_mark<'a>(state: &'a State, id: &str) -> &'a PriceMark {
        state.marks.lines.iter().find(|mark| mark.id == id).expect("the mark")
    }

    /// The rules as the chart holds them, to change.
    fn rules_of(state: &State) -> Exits {
        state.marks.rules.as_any().downcast_ref::<Exits>().expect("the tests' rules").clone()
    }

    fn set_rules(state: &mut State, rules: Exits) {
        state.marks.rules = Box::new(rules);
    }

    /// A chart of 300 bars marked with a long position, its two exit tabs,
    /// and a working sell limit, both well inside the plot.
    fn marked() -> State {
        let mut state = charted(300);
        let (first, visible, _) = state.view();
        let plan = layout(&state, 600.0, 400.0);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let (low, high) = scale.prices();
        let avg = low + (high - low) * 0.3;
        let tabs = vec![exit(false, "#ff0000", LineStyle::Dashed, 1), exit(true, "#ff0000", LineStyle::Dashed, 1)];
        let mut b1 = order_mark("B1", low + (high - low) * 0.7, "L 1", false, "#ff0000", LineStyle::Dotted);
        b1.chip = Some(AxisChip { text: "2".into(), filled: false });
        state.marks = Marks {
            lines: vec![b1, position(avg, "▲ 1", "+$125.00", "#00ff00", tabs)],
            rules: Box::new(Exits {
                avg,
                net: 1,
                point: 50.0,
                orders: vec![("B1".into(), true, false, 1)],
                profit: "#00aa00".into(),
                loss: "#aa0000".into(),
            }),
        };
        state
    }

    /// A position at the last price, its exits working: the picture the
    /// marks are judged in. In the theme `theme` of the built-in ones.
    fn at_the_last_price(theme: usize) -> State {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().nth(theme).expect("a built-in theme");
        let mut state = marked();
        state.scheme = omacharts_engine::theme::theme_bars(&theme);
        state.price_tags = price_tags(&state.scheme);
        state.last_ink = LastInk::of(&theme);
        state.theme = theme;
        let last = state.bars.last().unwrap().close;
        let up = state.scheme.up.clone();
        let down = state.scheme.down.clone();
        let held = mark_mut(&mut state, "position");
        held.price = last;
        held.body = Some("+$0.00".into());
        held.colour = up.clone();
        held.tabs = vec![exit(false, &up, LineStyle::Solid, 1), exit(true, &down, LineStyle::Dashed, 1)];
        let order = mark_mut(&mut state, "B1");
        order.tag = "▲ STP 2".into();
        order.colour = up;
        order.style = LineStyle::Dashed;
        let mut rules = rules_of(&state);
        rules.avg = last;
        set_rules(&mut state, rules);
        state
    }

    /// No line is drawn through what a mark says: not its own, not another
    /// at the same price, not the last price's when the position sits on
    /// it. Inside the label, beside its tag is its fill, and beside its
    /// body the chart's ground.
    #[test]
    fn no_line_crosses_a_marks_words() {
        let (w, h) = (600.0, 400.0);
        for theme in [0, 3] {
            let state = at_the_last_price(theme);
            let (first, visible, _) = state.view();
            let plan = layout(&state, w, h);
            let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
            let held = the_mark(&state, "position").clone();
            let y = scale.to_y(held.price, plan.price_y, plan.price_h).round() as usize;
            let cr = measuring().unwrap();
            let row = label_row(&cr, state.lane(&plan), &held);
            let pixels = frame(w, h, |cr| draw(cr, w, h, &state));
            let at = |x: f64| {
                let i = (y * w as usize + x as usize) * 4;
                (pixels[i + 2], pixels[i + 1], pixels[i])
            };
            let fill = |colour: &str| {
                let (r, g, b, _) = colors::rgba(&Tag::on(colour).fill);
                ((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8)
            };
            let x = row.tag_x + 4.0;
            assert_eq!(at(x), fill(&held.colour), "theme {theme}: the tag at x {x} is its fill, not a line");
            // Inside the body, and inside a tab beside its word, is the
            // chart's ground.
            let (r, g, b, _) = colors::rgba(&state.theme.ui.background);
            let ground = ((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8);
            let (body_x, body_w) = row.body.expect("a body");
            for x in [body_x + 3.0, body_x + body_w - 3.0] {
                assert_eq!(at(x), ground, "theme {theme}: the body at x {x} is clear of the line");
            }
            for (_, x, _) in &row.tabs {
                assert_eq!(at(x + 3.0), ground, "theme {theme}: a tab at x {x} is clear of the line");
            }
        }
    }

    fn save_png(mut surface: cairo::ImageSurface, path: &str) {
        let (w, h, stride) = (surface.width(), surface.height(), surface.stride() as usize);
        let bytes = gtk::glib::Bytes::from(&surface.data().unwrap()[..]);
        let texture = gtk::gdk::MemoryTexture::new(w, h, gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied, &bytes, stride);
        texture.save_to_png(path).unwrap();
    }

    /// Loading… and the bar under it as pictures, for a person to look at:
    /// early and late in a load, and a load that cannot say, dark and light,
    /// at two pixels to the point, and late in a load at one pixel to the
    /// point as well, to see that the bar lands on whole pixels at either. Late
    /// in a load again on a few of the desktop's own themes, on a machine
    /// that has them. Written only when asked for, to the folder in
    /// `OMACHARTS_MARK_SHOTS`.
    #[test]
    fn the_loading_bar_as_pictures() {
        let Ok(folder) = std::env::var("OMACHARTS_MARK_SHOTS") else { return };
        let (w, h) = (600.0, 400.0);
        let picture = |state: &State, scale: f64, name: &str| {
            let (pw, ph) = ((w * scale) as i32, (h * scale) as i32);
            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, pw, ph).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                cr.scale(scale, scale);
                draw(&cr, w, h, state);
            }
            save_png(surface, &format!("{folder}/{name}.png"));
        };
        let waiting = |theme: Theme| {
            let scheme = omacharts_engine::theme::theme_bars(&theme);
            let mut state = State::blank(theme, scheme);
            state.instrument = Some(named("ES"));
            state.loading = true;
            state
        };
        for (theme, name) in [(0, "dark"), (3, "light")] {
            let theme = omacharts_engine::theme::builtin_themes().into_iter().nth(theme).expect("a built-in theme");
            let mut state = waiting(theme);
            for (progress, stage) in [(Some(0.18), "early"), (Some(0.72), "later"), (None, "unmeasured")] {
                state.progress = progress;
                picture(&state, 2.0, &format!("loading-{stage}-{name}"));
            }
            picture(&state.with_progress(Some(0.72)), 1.0, &format!("loading-later-{name}-1x"));
        }
        let Some(home) = std::env::var_os("HOME") else { return };
        let wanted = ["tokyo-night", "catppuccin-latte", "gruvbox", "nord", "rose-pine", "flexoki-light", "everforest", "matte-black"];
        for theme in omacharts_engine::omarchy::installed(std::path::Path::new(&home)) {
            let slug = theme.id.rsplit(':').next().unwrap_or_default().to_string();
            if wanted.contains(&slug.as_str()) {
                picture(&waiting(theme).with_progress(Some(0.72)), 2.0, &format!("loading-later-omarchy-{slug}"));
            }
        }
    }

    impl State {
        /// This state, with the load it waits on got this far.
        fn with_progress(mut self, progress: Option<f64>) -> Self {
            self.progress = progress;
            self
        }
    }

    /// A cumulative delta's axis as pictures, for a person to look at: as
    /// fitted and zoomed in on its own scale, at two
    /// pixels to the point. Written only when asked for, to the folder in
    /// `OMACHARTS_MARK_SHOTS`.
    #[test]
    fn the_delta_axis_as_pictures() {
        let Ok(folder) = std::env::var("OMACHARTS_MARK_SHOTS") else { return };
        let (w, h) = (900.0, 600.0);
        let shot = |state: &State, name: &str| {
            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, (w * 2.0) as i32, (h * 2.0) as i32).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                cr.scale(2.0, 2.0);
                draw(&cr, w, h, state);
            }
            save_png(surface, &format!("{folder}/delta-axis-{name}.png"));
        };
        let mut state = with_delta(400);
        state.visible = 160;
        if let Some(Drawn { output, .. }) = state.indicators.last_mut() {
            output.set_pane_height(0.45);
        }
        shot(&state, "fitted");
        let (low, high) = state.strip_fit(2, w, h).unwrap();
        let middle = (low + high) / 2.0;
        state.hold_strip(2, middle - (high - low) / 6.0, middle + (high - low) / 6.0);
        shot(&state, "zoomed");
    }

    /// The marks as pictures, for a person to look at: dark and light, at
    /// one and two pixels to the point. Written only when asked for, to
    /// the folder in `OMACHARTS_MARK_SHOTS`.
    #[test]
    fn the_marks_as_pictures() {
        let Ok(folder) = std::env::var("OMACHARTS_MARK_SHOTS") else { return };
        let (w, h) = (600.0, 400.0);
        for (theme, name) in [(0, "dark"), (3, "light")] {
            let mut state = at_the_last_price(theme);
            state.hover_mark = None;
            for scale in [1.0, 2.0] {
                let surface =
                    cairo::ImageSurface::create(cairo::Format::ARgb32, (w * scale) as i32, (h * scale) as i32).unwrap();
                {
                    let cr = cairo::Context::new(&surface).unwrap();
                    cr.scale(scale, scale);
                    draw(&cr, w, h, &state);
                }
                save_png(surface, &format!("{folder}/position-marks-{name}-{}x.png", scale as u32));
            }
            mark_mut(&mut state, "B1").dim = true;
            mark_mut(&mut state, "position").dim = true;
            let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, (w * 2.0) as i32, (h * 2.0) as i32).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                cr.scale(2.0, 2.0);
                draw(&cr, w, h, &state);
            }
            save_png(surface, &format!("{folder}/position-marks-{name}-pending-2x.png"));
        }
    }

    /// The labels at either side, as pictures for a person to look at: a
    /// long with its target and its stop working, the price a little off
    /// the entry and then on it, and the stop in hand; dark and light, at
    /// two pixels to the point. Written only when asked for, to the folder
    /// in `OMACHARTS_MARK_SHOTS`.
    #[test]
    fn the_label_sides_as_pictures() {
        let Ok(folder) = std::env::var("OMACHARTS_MARK_SHOTS") else { return };
        let (w, h) = (600.0, 400.0);
        for side in LabelSide::ALL {
            for (theme, name) in [(0, "dark"), (3, "light")] {
                let mut state = at_the_last_price(theme);
                state.mark_labels = side;
                state.hover_mark = None;
                let look = omacharts_engine::theme_trading(&state.theme, Default::default());
                let (first, visible, _) = state.view();
                let plan = layout(&state, w, h);
                let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
                let (low, high) = scale.prices();
                let last = state.bars.last().unwrap().close;
                let held = mark_mut(&mut state, "position");
                held.price = last - (high - low) * 0.03;
                held.body = Some("+$62.50".into());
                held.colour = look.profit.clone();
                held.style = look.position;
                held.tabs = Vec::new();
                let avg = held.price;
                let order = |id: &str, stop: bool, tag: &str, at: f64, colour: &str, style, money: f64| PriceMark {
                    body: Some(Exits::money(money)),
                    chip: Some(AxisChip { text: grouped_decimals(at, 2), filled: true }),
                    ..order_mark(id, at, tag, stop, colour, style)
                };
                let (tp, sl) = (last + (high - last) * 0.6, avg - (high - low) * 0.2);
                let held = the_mark(&state, "position").clone();
                state.marks.lines = vec![
                    order("TP", false, "L 1", tp, &look.profit, look.limit, (tp - avg) * 50.0),
                    order("SL", true, "STP 1", sl, &look.loss, look.stop, (sl - avg) * 50.0),
                    held,
                ];
                let mut rules = rules_of(&state);
                rules.avg = avg;
                rules.orders = vec![("TP".into(), true, false, 1), ("SL".into(), true, true, 1)];
                rules.profit = look.profit.clone();
                rules.loss = look.loss.clone();
                set_rules(&mut state, rules.clone());
                let shot = |state: &State, file: &str| {
                    let surface =
                        cairo::ImageSurface::create(cairo::Format::ARgb32, (w * 2.0) as i32, (h * 2.0) as i32).unwrap();
                    {
                        let cr = cairo::Context::new(&surface).unwrap();
                        cr.scale(2.0, 2.0);
                        draw(&cr, w, h, state);
                        draw_pointer(&cr, w, h, state);
                    }
                    save_png(surface, &format!("{folder}/{file}"));
                };
                let side = side.key();
                shot(&state, &format!("labels-{side}-{name}.png"));
                mark_mut(&mut state, "position").price = last;
                rules.avg = last;
                set_rules(&mut state, rules);
                shot(&state, &format!("labels-{side}-{name}-on-entry.png"));
                let at = sl - (high - low) * 0.08;
                let what = Grab::Move("SL".into());
                state.drag = Some(Drag::Mark { allowed: state.mark_allowed(&what, at), what, at });
                shot(&state, &format!("labels-{side}-{name}-drag.png"));
            }
        }
    }

    /// A mark in hand is asked about as it is: the mark, the tab, where it
    /// started from, where the hand has it and the market's last price; and
    /// its label says what the rules say, in the colour they say.
    #[test]
    fn a_mark_in_hand_says_what_its_rules_say() {
        let state = marked();
        let avg = the_mark(&state, "position").price;
        let last = state.bars.last().map(|bar| bar.close);
        let target = Grab::Tab { mark: "position".into(), tab: "limit".into() };
        let dragged = state.mark_dragged(&target).expect("in hand");
        let query = state.drag_query(&dragged, avg + 5.0, 2);
        assert_eq!(
            query,
            MarkDrag { mark: "position", tab: Some("limit"), from: avg, at: avg + 5.0, last, tick: state.tick(), places: 2 }
        );
        assert_eq!((dragged.tag, dragged.colour, dragged.weight), ("L 1", "#ff0000", 1.5));
        let said = state.marks.rules.rules().preview(&query, true);
        assert_eq!(said, MarkPreview { text: "+$250.00".into(), colour: Some("#00aa00".into()) });

        let moved = Grab::Move("B1".into());
        let dragged = state.mark_dragged(&moved).expect("in hand");
        assert_eq!((dragged.mark, dragged.tab, dragged.from), ("B1", None, the_mark(&state, "B1").price));
        assert!(state.mark_dragged(&Grab::Move("B9".into())).is_none(), "no such mark");
        assert!(state.mark_dragged(&Grab::Move("position".into())).is_none(), "the position is not draggable");
        assert!(state.mark_dragged(&Grab::Tab { mark: "position".into(), tab: "market".into() }).is_none());

        // With no rules, anywhere, saying the price.
        let mut free = marked();
        free.marks.rules = Box::new(AnyPrice);
        let at = avg + 5.0;
        assert!(free.mark_allowed(&target, at));
        let dragged = free.mark_dragged(&target).unwrap();
        let said = free.marks.rules.rules().preview(&free.drag_query(&dragged, at, 2), true);
        assert_eq!(said, MarkPreview { text: grouped_decimals(at, 2), colour: None });
    }

    /// While an exit is in hand its pill is drawn over the chart, in the
    /// colour of its money, the profit colour for a target over the
    /// market, where the hand has it.
    #[test]
    fn a_dragged_exit_is_drawn_with_its_money() {
        let (w, h) = (600.0, 400.0);
        let mut state = marked();
        let (first, visible, _) = state.view();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let avg = the_mark(&state, "position").price;
        let last = state.bars.last().unwrap().close;
        // A target is a limit above the market for a long.
        let at = last.max(avg) + (scale.prices().1 - last.max(avg)) / 2.0;
        let target = Grab::Tab { mark: "position".into(), tab: "limit".into() };
        assert!(state.mark_allowed(&target, at));
        // Rows of the plot with the profit colour in them.
        let green = |pixels: &[u8]| -> Vec<usize> {
            (plan.price_y as usize..(plan.price_y + plan.price_h) as usize)
                .filter(|row| {
                    (plan.plot_x as usize..(plan.plot_x + plan.plot_w) as usize).any(|x| {
                        let i = (row * w as usize + x) * 4;
                        let (r, g, b) = (pixels[i + 2], pixels[i + 1], pixels[i]);
                        r < 40 && (150..190).contains(&g) && b < 40
                    })
                })
                .collect()
        };
        assert_eq!(green(&frame(w, h, |cr| draw_pointer(cr, w, h, &state))), Vec::<usize>::new(), "no money before the drag");
        state.drag = Some(Drag::Mark { what: target, at, allowed: true });
        // The drag widens the price range to keep the hand's price in it.
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let y = scale.to_y(at, plan.price_y, plan.price_h).round() as usize;
        // The hand's layer, over the chart: what moves with the pointer.
        // The pill's outline is the colour, so its rows are a pill's
        // height about the hand.
        let rows = green(&frame(w, h, |cr| draw_pointer(cr, w, h, &state)));
        let reach = (MARK_H / 2.0) as usize + 1;
        assert!(!rows.is_empty() && rows.iter().all(|row| row.abs_diff(y) <= reach), "the money where the hand is: {rows:?} at {y}");
    }

    /// An exit's money as pictures, for a person to look at: resting, and
    /// in hand as a target and as a stop, dark and light, at two pixels to
    /// the point. Written only when asked for, to `OMACHARTS_MARK_SHOTS`.
    #[test]
    fn the_exits_money_as_pictures() {
        let Ok(folder) = std::env::var("OMACHARTS_MARK_SHOTS") else { return };
        let (w, h) = (600.0, 400.0);
        for (theme, name) in [(0, "dark"), (3, "light")] {
            let mut state = at_the_last_price(theme);
            state.hover_mark = None;
            let look = omacharts_engine::theme_trading(&state.theme, Default::default());
            let (first, visible, _) = state.view();
            let plan = layout(&state, w, h);
            let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
            let (low, high) = scale.prices();
            let held = mark_mut(&mut state, "position");
            held.price -= (high - low) * 0.15;
            let avg = held.price;
            let mut rules = rules_of(&state);
            rules.avg = avg;
            rules.profit = look.profit.clone();
            rules.loss = look.loss.clone();
            set_rules(&mut state, rules);
            // A resting target, saying what it would realise, in its colour.
            let order = mark_mut(&mut state, "B1");
            order.colour = look.profit.clone();
            order.style = look.limit;
            order.line = Some(1.5);
            order.price = avg + (high - low) * 0.25;
            order.tag = "L 1".into();
            order.body = Some(Exits::money((order.price - avg) * 50.0));
            order.chip.as_mut().unwrap().filled = true;
            let shot = |state: &State, file: &str| {
                let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, (w * 2.0) as i32, (h * 2.0) as i32).unwrap();
                {
                    let cr = cairo::Context::new(&surface).unwrap();
                    cr.scale(2.0, 2.0);
                    draw(&cr, w, h, state);
                    draw_pointer(&cr, w, h, state);
                }
                save_png(surface, &format!("{folder}/{file}"));
            };
            shot(&state, &format!("drag-pnl-resting-{name}.png"));
            let last = state.bars.last().unwrap().close;
            let target = the_mark(&state, "B1").price;
            for (tab, at, file) in [("limit", last + (target - last) * 0.5, "tp"), ("stop", avg - (avg - low) * 0.5, "sl")] {
                let what = Grab::Tab { mark: "position".into(), tab: tab.into() };
                state.drag = Some(Drag::Mark { allowed: state.mark_allowed(&what, at), what, at });
                shot(&state, &format!("drag-pnl-{file}-{name}.png"));
            }
        }
    }

    /// The ×, the tabs and a mark's line are found where they are drawn:
    /// a × is a ×, not the line it sits on; the line is grabbed within a
    /// few pixels of it, label included; and nothing is found far from all
    /// of them, on a faint mark, or with a tool in hand.
    #[test]
    fn a_marks_handles_and_its_line_are_found_where_they_are_drawn() {
        let (w, h) = (600.0, 400.0);
        for side in LabelSide::ALL {
            let mut state = marked();
            state.mark_labels = side;
            let (first, visible, _) = state.view();
            let plan = layout(&state, w, h);
            let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
            let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
            let cr = measuring().unwrap();
            let lane = state.lane(&plan);
            let order = the_mark(&state, "B1").clone();
            let held = the_mark(&state, "position").clone();
            let (order_y, held_y) = (to_y(order.price).round(), to_y(held.price).round());
            let row = label_row(&cr, lane, &order);
            let at = |x: f64, y: f64| mark_at(&plan, lane, &state.marks, x, y, &to_y);
            // Somewhere along the line, clear of every label.
            let mid = (plan.plot_x + lane.end) / 2.0;

            assert_eq!(at(row.cap_x + MARK_CAP / 2.0, order_y), Some(MarkHit::Close("B1".into())), "{side:?}");
            assert_eq!(at(mid, order_y + 4.0), Some(MarkHit::Line("B1".into())), "{side:?}: near the line");
            assert_eq!(at(row.tag_x + 10.0, order_y), Some(MarkHit::Line("B1".into())), "{side:?}: the label is the line's");
            assert_eq!(at(mid, order_y + 12.0), None, "{side:?}: too far from it");

            let row = label_row(&cr, lane, &held);
            assert_eq!(at(row.cap_x + 3.0, held_y), Some(MarkHit::Close("position".into())), "{side:?}");
            let tab = |n: usize| *row.tabs.iter().find(|(k, ..)| *k == n).unwrap();
            let (_, tp, tw) = tab(0);
            let hit = |tab: &str| Some(MarkHit::Tab { mark: "position".into(), tab: tab.into() });
            assert_eq!(at(tp + tw / 2.0, held_y), hit("limit"), "{side:?}");
            let (_, sl, sw) = tab(1);
            assert_eq!(at(sl + sw / 2.0, held_y), hit("stop"), "{side:?}");
            assert!(tp < sl, "{side:?}: L left of STP");
            assert_eq!(at(mid, held_y), None, "{side:?}: a mark that is not draggable is not dragged");

            mark_mut(&mut state, "B1").dim = true;
            let row = label_row(&cr, lane, &order);
            assert_eq!(mark_at(&plan, lane, &state.marks, mid, order_y, &to_y), None, "a faint mark");
            assert_eq!(mark_at(&plan, lane, &state.marks, row.cap_x + 3.0, order_y, &to_y), None);
            mark_mut(&mut state, "B1").dim = false;
            assert!(state.mark_under(w, h, mid, order_y).is_some());
            state.tool = Some(DrawingKind::Line);
            assert_eq!(state.mark_under(w, h, mid, order_y), None, "a tool in hand takes the press");
            state.tool = None;
            state.ruler_armed = true;
            assert_eq!(state.mark_under(w, h, mid, order_y), None, "so does the ruler");
        }
    }

    /// The labels' two places. At the left: tag at the plot's edge, the ×
    /// towards the bars and the mark's tabs after it, in their order. At
    /// the right: by the axis as ever, the tabs before the label. Either
    /// way the labels stay put as the bars scroll, and the lines stop at
    /// the last bar, or at the plot's edge with the last bar past it.
    #[test]
    fn the_labels_sit_at_either_side_and_the_lines_stop_at_the_last_bar() {
        let (w, h) = (600.0, 400.0);
        let mut state = marked();
        assert_eq!(state.mark_labels, LabelSide::Left, "left unless chosen");
        let plan = layout(&state, w, h);
        let cr = measuring().unwrap();
        let held = the_mark(&state, "position").clone();
        let plot_end = plan.plot_x + plan.plot_w;

        let lane = state.lane(&plan);
        assert!(lane.end > plan.plot_x + plan.plot_w / 2.0 && lane.end < plot_end - 10.0, "room after the bars: {lane:?}");
        let row = label_row(&cr, lane, &held);
        assert_eq!(row.tag_x, plan.plot_x + MARK_EDGE, "the tag at the edge");
        let (body_x, body_w) = row.body.unwrap();
        assert!(row.tag_x < body_x && body_x + body_w <= row.cap_x, "tag, body, ×");
        assert!(row.tabs.iter().all(|(_, x, _)| *x > row.cap_x + MARK_CAP), "the tabs after the ×");
        assert_eq!(row.tabs[0].0, 0, "the first first");

        state.mark_labels = LabelSide::Right;
        let lane = state.lane(&plan);
        let row = label_row(&cr, lane, &held);
        assert_eq!(row.right(), plot_end - MARK_EDGE, "the × by the axis");
        assert!(row.tabs.iter().all(|(_, x, _)| *x < row.tag_x), "the tabs before the label");

        // Scrolled back so the last bar is past the right edge: the lines
        // run across the plot, and the labels are where they were.
        for side in LabelSide::ALL {
            let mut back = marked();
            back.mark_labels = side;
            let before = label_row(&cr, back.lane(&plan), &held).tag_x;
            let (first, ..) = back.view();
            back.anchored = false;
            back.first = first.saturating_sub(100);
            back.lead = 0;
            assert_eq!(back.lane(&plan).end, plot_end, "{side:?}: the last bar is past the edge");
            assert_eq!(label_row(&cr, back.lane(&plan), &held).tag_x, before, "{side:?}: the label stays put");
        }
    }

    /// With the price on a mark, the last price's dashes are drawn over the
    /// mark's line, and alone past the last bar.
    #[test]
    fn the_last_prices_line_is_over_the_marks() {
        let (w, h) = (600.0, 400.0);
        for side in LabelSide::ALL {
            let mut state = marked();
            state.mark_labels = side;
            let last = state.bars.last().unwrap().close;
            mark_mut(&mut state, "position").price = last;
            let (first, visible, _) = state.view();
            let plan = layout(&state, w, h);
            let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
            let y = (scale.to_y(last, plan.price_y, plan.price_h).round() + 0.5) as usize;
            let lane = state.lane(&plan);
            let pixels = frame(w, h, |cr| draw(cr, w, h, &state));
            let green = |x: usize| {
                let i = (y * w as usize + x) * 4;
                (pixels[i + 2], pixels[i + 1], pixels[i]) == (0, 255, 0)
            };
            let row = label_row(&measuring().unwrap(), lane, the_mark(&state, "position"));
            // Clear of the label and its tabs, along the bars.
            let from = row.tabs.iter().map(|(_, x, w)| x + w).fold(row.right(), f64::max) as usize + 4;
            let to = match side {
                LabelSide::Left => lane.end as usize - 4,
                LabelSide::Right => row.tabs.iter().map(|(_, x, _)| *x).fold(row.left(), f64::min) as usize - 4,
            };
            let (from, to) = match side {
                LabelSide::Left => (from, to),
                LabelSide::Right => (plan.plot_x as usize + 4, to),
            };
            let over = (from..to).filter(|x| !green(*x)).count();
            assert!(over > (to - from) / 4, "{side:?}: the last price's dashes over the mark's line: {over}");
        }
    }

    /// The axis carries the price of every mark that asks for a chip and
    /// the last price's, and nothing for a mark that asks for none.
    #[test]
    fn the_axis_has_no_chip_for_a_mark_without_one() {
        let (w, h) = (600.0, 400.0);
        let state = marked();
        let mut bare = marked();
        bare.marks = Marks::default();
        let (first, visible, _) = state.view();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let axis = (plan.plot_x + plan.plot_w) as usize + 4;
        let pixels = frame(w, h, |cr| draw(cr, w, h, &state));
        let unmarked = frame(w, h, |cr| draw(cr, w, h, &bare));
        let ink = |price: f64| {
            let y = scale.to_y(price, plan.price_y, plan.price_h).round() as usize;
            (axis..w as usize - 2).any(|x| {
                let i = (y * w as usize + x) * 4;
                pixels[i..i + 3] != unmarked[i..i + 3]
            })
        };
        assert!(ink(the_mark(&state, "B1").price), "the order's chip");
        assert!(!ink(the_mark(&state, "position").price), "no chip for the position");
    }

    /// A mark in hand goes only where its rules let it, and its preview is
    /// drawn on the layer over the chart either way. Once what is dragged
    /// has gone from the chart there is nothing to let go of.
    #[test]
    fn a_mark_in_hand_goes_only_where_its_rules_let_it() {
        let mut state = marked();
        let last = state.bars.last().unwrap().close;
        let stop = Grab::Tab { mark: "position".into(), tab: "stop".into() };
        assert!(state.mark_allowed(&stop, last - 5.0), "a long's stop below the market");
        assert!(!state.mark_allowed(&stop, last + 5.0));
        let target = Grab::Tab { mark: "position".into(), tab: "limit".into() };
        assert!(state.mark_allowed(&target, last + 5.0));
        assert!(!state.mark_allowed(&target, last - 5.0));
        let moved = Grab::Move("B1".into());
        assert!(state.mark_allowed(&moved, last + 5.0), "a sell limit stays above");
        assert!(!state.mark_allowed(&moved, last - 5.0));

        // The preview is drawn on the layer over the chart, allowed or not.
        let (w, h) = (600.0, 400.0);
        let quiet = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
        state.drag = Some(Drag::Mark { what: target.clone(), at: last + 5.0, allowed: true });
        let shown = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
        assert!(painted(&shown) > painted(&quiet) + 200, "no preview");
        state.drag = Some(Drag::Mark { what: target.clone(), at: last - 5.0, allowed: false });
        let refused = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
        assert!(painted(&refused) > painted(&quiet) + 200, "no refused preview");
        assert!(refused != shown);

        // Gone under the hand: the position and its tabs.
        state.drag = None;
        state.marks.lines.retain(|mark| mark.id != "position");
        assert!(state.mark_dragged(&target).is_none());
        assert!(!state.mark_allowed(&target, last + 5.0));
        // And with no bars there is no market to go to either side of.
        let mut empty = marked();
        empty.bars.clear();
        assert!(!empty.mark_allowed(&moved, last + 5.0));
    }

    /// The handles are drawn: a chart with marks paints more than the same
    /// chart with its lines and pills alone would, and one with none is
    /// the chart it always was.
    #[test]
    fn a_marked_chart_draws_its_handles() {
        let (w, h) = (600.0, 400.0);
        let mut state = marked();
        let with = frame(w, h, |cr| draw(cr, w, h, &state));
        mark_mut(&mut state, "position").tabs.clear();
        let without_tabs = frame(w, h, |cr| draw(cr, w, h, &state));
        assert!(with != without_tabs, "the tabs drew nothing");
        state.marks = Marks::default();
        let bare = frame(w, h, |cr| draw(cr, w, h, &state));
        let plain = charted(300);
        assert!(bare == frame(w, h, |cr| draw(cr, w, h, &plain)), "no marks, the same picture");
    }

    /// Handed the same marks and the same rules, the chart has nothing to
    /// redraw; a change to either is a change, and with no marks at all the
    /// rules make none.
    #[test]
    fn the_same_marks_under_the_same_rules_are_no_change() {
        let state = marked();
        let lines = state.marks.lines.clone();
        let rules = rules_of(&state);
        assert!(state.marks.same(&lines, &rules));
        let mut other = rules.clone();
        other.avg += 1.0;
        assert!(!state.marks.same(&lines, &other), "other rules");
        assert!(!state.marks.same(&lines, &AnyPrice), "rules of another kind");
        assert!(!state.marks.same(&lines[..1], &rules), "other marks");
        assert!(Marks::default().same(&[], &rules), "nothing marked, whatever the rules");
    }

    /// ES trades in quarters, and a price written beside its axis has to be
    /// one it could have traded at. The crosshair's is read off a pixel,
    /// which falls between ticks: 7,627.55 beside a last price of 7,620.25,
    /// on a chart whose every bar is on the 0.25 grid.
    #[test]
    fn the_crosshair_names_a_price_on_the_series_tick() {
        let (w, h) = (600.0, 400.0);
        let mut state = charted(120);
        state.bars = (0..120)
            .map(|i| {
                let base = 7_600.0 + (i % 23) as f64 * 1.25;
                Bar {
                    ts: 1_600_000_000 + i as i64 * 60,
                    open: base,
                    high: base + 2.75,
                    low: base - 1.5,
                    close: base + 0.25,
                    volume: 10.0,
                }
            })
            .collect();
        let (first, visible, _) = state.view();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let mut off = Vec::new();
        for row in 0..plan.price_h as usize {
            let py = plan.price_y + row as f64 + 0.5;
            let text = price_under_pointer(&state, &scale, py, plan.price_y, plan.price_h);
            let price: f64 = text.replace(',', "").parse().unwrap();
            if (price * 4.0 - (price * 4.0).round()).abs() > 1e-9 {
                off.push(text);
            }
        }
        assert!(off.is_empty(), "off the 0.25 grid: {off:?}");
    }

    #[test]
    fn a_bar_with_less_than_a_pixel_of_range_still_shows() {
        // Zoomed out, a quiet bar's high and low round to the same row.
        let mut state = charted(1);
        let bars = [Bar { ts: 0, open: 100.0, high: 100.2, low: 100.0, close: 100.1, volume: 0.0 }];
        let to_y = |price: f64| 50.0 - price / 100.0;
        for style in BarStyle::ALL {
            state.bar_style = style;
            let columns = Columns::of(&bars, 0, 1, 0.0, 2.0);
            let drawn = frame(10.0, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
            assert!(painted(&drawn) > 0, "{style:?} drew nothing");
        }
    }

    #[test]
    fn a_line_is_one_stroke_through_the_closes_in_the_text_colour() {
        let mut state = charted(1);
        state.bar_style = BarStyle::Line;
        // Opening low and closing high, and the reverse: a line that took
        // its colour from the direction would show two.
        let bars = [
            Bar { ts: 0, open: 20.0, high: 90.0, low: 10.0, close: 80.0, volume: 0.0 },
            Bar { ts: 60, open: 80.0, high: 90.0, low: 10.0, close: 20.0, volume: 0.0 },
        ];
        let to_y = |price: f64| 100.0 - price;
        let columns = Columns::of(&bars, 0, 2, 0.0, 20.0);
        let drawn = frame(40.0, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
        let (r, g, b, _) = crate::colors::rgba(&state.theme.ui.text);
        let text = [(b * 255.0).round() as u8, (g * 255.0).round() as u8, (r * 255.0).round() as u8];
        let solid: Vec<&[u8]> = drawn.chunks(4).filter(|pixel| pixel[3] == 255).collect();
        assert!(!solid.is_empty(), "the line is drawn");
        assert!(solid.iter().all(|pixel| pixel[..3] == text), "every solid pixel is the text colour");
        let alpha = |x: usize, y: usize| drawn[(y * 40 + x) * 4 + 3];
        let (x, high) = (columns.x(0).round() as usize, 10);
        assert_eq!(alpha(x, high), 0, "the high is not drawn");
        assert_ne!(alpha(x, 20), 0, "the close is");
    }

    #[test]
    fn a_line_fits_the_closes_rather_than_the_ranges() {
        let mut state = charted(60);
        let (first, visible, _) = state.view();
        let plan = layout(&state, 600.0, 400.0);
        let fit = |state: &State| price_range(state, &state.bars[first..first + visible], &plan).unwrap().prices();
        let candles = fit(&state);
        state.bar_style = BarStyle::Line;
        let line = fit(&state);
        assert!(line.0 > candles.0 && line.1 < candles.1, "{line:?} inside {candles:?}");
        let closes = state.bars[first..first + visible].iter().map(|bar| bar.close);
        let (low, high) = closes.fold((f64::MAX, f64::MIN), |(l, h), c| (l.min(c), h.max(c)));
        assert!(line.0 <= low && line.1 >= high, "every close is in view");
    }

    #[test]
    fn a_hollow_body_has_no_wick_through_it() {
        let mut state = charted(1);
        state.scheme.up_fill = "#00000000".into();
        // Wide enough for a body, and tall enough for wicks either side of it.
        let bars = [Bar { ts: 0, open: 40.0, high: 90.0, low: 10.0, close: 60.0, volume: 0.0 }];
        let to_y = |price: f64| 100.0 - price;
        let columns = Columns::of(&bars, 0, 1, 0.0, 20.0);
        let x = columns.x(0).round() as usize;
        let drawn = frame(20.0, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
        let alpha = |y: usize| drawn[(y * 20 + x) * 4 + 3];
        assert_ne!(alpha(20), 0, "the upper wick is drawn");
        assert_ne!(alpha(80), 0, "the lower wick is drawn");
        assert_eq!(alpha(50), 0, "the inside of a hollow body is left empty");
        assert!((11..40).all(|y| alpha(y) != 0), "the upper wick reaches the body");
        assert!((61..90).all(|y| alpha(y) != 0), "the lower wick reaches the body");
    }

    #[test]
    fn hidden_bars_draw_nothing() {
        let mut state = charted(1);
        state.bars_hidden = true;
        let bars = [Bar { ts: 0, open: 40.0, high: 90.0, low: 10.0, close: 60.0, volume: 0.0 }];
        let to_y = |price: f64| 100.0 - price;
        let columns = Columns::of(&bars, 0, 1, 0.0, 20.0);
        for style in BarStyle::ALL {
            state.bar_style = style;
            let drawn = frame(20.0, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
            assert_eq!(painted(&drawn), 0, "{style:?} drew a hidden bar");
        }
    }

    #[test]
    fn a_hollow_doji_keeps_its_whole_wick() {
        let mut state = charted(1);
        state.scheme.up_fill = "#00000000".into();
        let bars = [Bar { ts: 0, open: 50.0, high: 90.0, low: 10.0, close: 50.0, volume: 0.0 }];
        let to_y = |price: f64| 100.0 - price;
        let columns = Columns::of(&bars, 0, 1, 0.0, 20.0);
        let x = columns.x(0).round() as usize;
        let drawn = frame(20.0, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
        let alpha = |y: usize| drawn[(y * 20 + x) * 4 + 3];
        assert!((11..90).all(|y| alpha(y) != 0), "a body with no height leaves no gap");
    }

    /// The body and the wick were rounded to pixels apart, so a resize —
    /// the settings panel opening beside the chart — put half the wicks a
    /// pixel right of their bodies' middles. At every width, they line up:
    /// on the middle of an odd body, and just left of it on an even one.
    #[test]
    fn a_wick_runs_down_the_middle_of_its_body() {
        let state = charted(1);
        let bars = [Bar { ts: 0, open: 40.0, high: 90.0, low: 10.0, close: 60.0, volume: 0.0 }; 3];
        let to_y = |price: f64| 100.0 - price;
        for tenths in 120..400 {
            let wide = tenths as f64 / 10.0;
            let w = (wide + 2.0).ceil() as usize;
            let columns = Columns::of(&bars, 0, 3, 0.3, wide);
            let drawn = frame(w as f64, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
            let inked = |y: usize| (0..w).filter(|x| drawn[(y * w + x) * 4 + 3] != 0).collect::<Vec<_>>();
            let (wicks, bodies) = (inked(20), inked(50));
            assert_eq!(wicks.len(), 3, "at {wide}px, wicks at {wicks:?}");
            for (i, wick) in wicks.iter().enumerate() {
                let nearest = |x: usize| wicks.iter().min_by_key(|w| w.abs_diff(x)) == Some(wick);
                let body: Vec<_> = bodies.iter().filter(|x| nearest(**x)).collect();
                assert_eq!(*body[(body.len() - 1) / 2], *wick, "at {wide}px, candle {i}: body {body:?}, wick {wick}");
            }
        }
    }

    /// A body as wide as the gap to its neighbour touched it, and a run of
    /// candles going the same way read as one block. From two pixels a
    /// column, where there is room for one, a pixel always parts them.
    #[test]
    fn neighbouring_bodies_never_touch() {
        let bars = ramp(400);
        for hundredths in 200..1600 {
            let bar_w = hundredths as f64 / 100.0;
            let columns = Columns::of(&bars, 0, 40, 0.3, bar_w * 40.0);
            for at in 1..columns.len() {
                let ((was, width), (left, _)) = (columns.body(at - 1), columns.body(at));
                assert!(left - (was + width) >= 1.0, "at {bar_w}px a column, bodies {} and {at} touch", at - 1);
            }
        }
    }

    /// Past a bar a pixel, a column is under a pixel wide. A one-pixel wick
    /// or volume bar rounded from each middle left a pixel out wherever two
    /// rounded to one, and the gaps moved as the chart scrolled: stripes
    /// crawling across it. Wherever the scroll stands, every pixel across
    /// the bars is drawn.
    #[test]
    fn zoomed_out_columns_leave_no_gap_between_them() {
        let state = charted(1);
        let (visible, wide) = (1400usize, 450.0);
        let bars = ramp(visible + 4);
        let to_y = |price: f64| 100.0 - (price - 90.0) / 20.0;
        let w = (PAD * 2.0 + wide) as usize;
        for first in 0..4 {
            let columns = Columns::of(&bars[first..first + visible], first, visible, PAD, wide);
            assert_eq!(columns.per, 2);
            let candles = frame(w as f64, 100.0, |cr| draw_candles(cr, &state, &columns, &to_y));
            let volume = frame(w as f64, 100.0, |cr| draw_volume(cr, &state, &Indicator::new(1, indicators::Kind::Volume), &columns, 0.0, 100.0, (0.0, 2000.0)));
            for x in PAD as usize..(PAD + wide) as usize {
                let inked = |pixels: &[u8]| (0..100).any(|y| pixels[(y * w + x) * 4 + 3] != 0);
                assert!(inked(&candles), "from bar {first}, no wick at {x}");
                assert!(inked(&volume), "from bar {first}, no volume at {x}");
            }
            let outside = |pixels: &[u8]| (0..100).any(|y| pixels[(y * w + PAD as usize - 1) * 4 + 3] != 0);
            assert!(!outside(&volume), "from bar {first}, volume left of the bars");
        }
    }

    /// Grouping by whole bars must not make columns wider than a pixel, or
    /// the candles thicken the moment the chart holds more bars than it has
    /// pixels. At every zoom past a bar a pixel, a column is drawn one pixel
    /// wide, and every pixel is one column's.
    #[test]
    fn a_zoomed_out_candle_is_a_pixel_wide_at_every_zoom() {
        let bars = ramp(4000);
        let wide = 400.0;
        for visible in (401..4000).step_by(37) {
            let columns = Columns::of(&bars[..visible], 0, visible, PAD, wide);
            let drawn: Vec<f64> = (0..columns.len())
                .map(|at| {
                    let (x, w) = columns.tile(at).expect("a pixel");
                    assert_eq!(w, 1.0, "{visible} bars: column {at} is {w} wide");
                    x
                })
                .collect();
            assert!(drawn.windows(2).all(|two| two[1] == two[0] + 1.0), "{visible} bars: a pixel skipped or shared");
            assert_eq!(drawn.len(), wide as usize, "{visible} bars: {} of {wide} pixels", drawn.len());
        }
    }

    /// Bars are grouped into pixels counted from the series. Counted from
    /// the plot's edge, a scroll that moved the bars a fraction of a pixel
    /// regrouped them, and candles all along the chart changed colour. Now
    /// a scroll moves every column by the same whole pixels, holding the
    /// same bars, and only those at the edges come and go.
    #[test]
    fn scrolling_moves_whole_columns_by_whole_pixels() {
        let (visible, wide) = (1300usize, 400.0);
        let bars = ramp(1400);
        let shown = |first: usize| -> Vec<(usize, usize, f64)> {
            let columns = Columns::of(&bars[first..first + visible], first, visible, PAD, wide);
            (1..columns.len() - 1)
                .map(|at| {
                    let span = columns.span(at);
                    (first + span.start, first + span.end, columns.x(at))
                })
                .collect()
        };
        for first in 40..80 {
            let (before, after) = (shown(first), shown(first + 1));
            let moved: Vec<f64> = after
                .iter()
                .filter_map(|(from, to, x)| before.iter().find(|(f, t, _)| f == from && t == to).map(|(_, _, was)| x - was))
                .collect();
            assert!(moved.windows(2).all(|two| two[0] == two[1]), "from bar {first}: columns moved by {moved:?}");
            assert!(moved.iter().all(|by| by.fract() == 0.0), "from bar {first}: moved by part of a pixel");
            let kept = moved.len();
            assert!(kept + 2 >= after.len(), "from bar {first}: {kept} of {} still the same", after.len());
        }
    }

    #[test]
    fn chips_that_would_overlap_are_pushed_apart_in_order() {
        // Close together: the lower one moves down until they just touch.
        let mut ys = [100.0, 104.0];
        spread_chips(&mut ys, 16.0, 300.0);
        assert_eq!(ys, [100.0, 116.0]);

        // Far apart: nothing moves.
        let mut ys = [100.0, 140.0];
        spread_chips(&mut ys, 16.0, 300.0);
        assert_eq!(ys, [100.0, 140.0]);

        // At the bottom of the strip: both lift so the lower stays inside it.
        let mut ys = [295.0, 296.0];
        spread_chips(&mut ys, 16.0, 300.0);
        assert_eq!(ys, [276.0, 292.0]);
    }

    /// A strip's name is written in its top 13 pixels or so; the top of its
    /// scale has to sit under that, or a line at its high runs through it.
    #[test]
    fn the_top_of_a_strips_scale_sits_under_its_name() {
        let (top, height) = (300.0, 80.0);
        let (inner_top, inner_h) = pane_plot(top, height);
        assert!(inner_top >= top + 14.0, "{inner_top}");
        assert!(inner_top + inner_h <= top + height, "{inner_top} + {inner_h}");
        // A strip dragged down to a sliver still has a scale to draw on.
        let (_, sliver) = pane_plot(top, 12.0);
        assert!(sliver > 0.0);
    }

    /// An oscillator strip is read off its own axis the way the price is, so
    /// pointing into one puts a chip on the axis beside it. Volume is a strip
    /// with no scale written on it, and is left as it was.
    #[test]
    fn pointing_into_an_oscillator_strip_reads_its_value_on_the_axis() {
        let (w, h) = (600.0, 400.0);
        let mut state = charted(300);
        let indicator = Indicator::new(2, indicators::Kind::Stochastic);
        state.indicators.push(Drawn {
            color: "#5588ff".to_string(),
            output: indicators::compute(&indicator, &state.bars, 0, state.timeframe, None),
            indicator,
        });
        let plan = layout(&state, w, h);
        let axis = |pixels: &[u8], y: f64| {
            let (from, to) = ((plan.plot_x + plan.plot_w) as usize + 4, w as usize);
            (from..to).filter(|x| pixels[(y as usize * w as usize + x) * 4 + 3] != 0).count()
        };

        for (id, read) in [(2, true), (1, false)] {
            let Some(row) = plan.rows.iter().find(|row| row.pane == Some(id)) else {
                panic!("no strip for {id}");
            };
            let y = row.top + row.height * 0.4;
            state.pointer = Some((300.0, y));
            let pixels = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
            assert_eq!(axis(&pixels, y) > 0, read, "strip {id}");
        }
    }

    /// The two layers have to be exactly the same size and in exactly the same
    /// place, because the crosshair is drawn in the coordinates the pointer
    /// arrives in and those are the chart's. An overlay child that came out a
    /// few pixels adrift would put the crosshair where the mouse is not, and a
    /// zero-sized one would draw nothing at all — which looks exactly like a
    /// screenshot leaving it out.
    #[test]
    fn the_pointer_layer_covers_the_chart_exactly() {
        crate::on_gtk(|| {
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
        });
    }

    /// Alt+Shift with + and − step every selected drawing's stroke at once,
    /// whatever the kind calls it — an anchored VWAP's line, a box's edge, a
    /// profile's outline — by half a pixel, held to 0.5 to 12, and give each
    /// a look of its own as the properties would.
    #[test]
    fn the_stroke_keys_step_every_selected_drawing() {
        crate::on_gtk(|| {
            let theme = omacharts_engine::theme::builtin_themes()
                .into_iter()
                .next()
                .expect("a built-in theme");
            let scheme = omacharts_engine::theme::theme_bars(&theme);
            let view = ChartView::new(theme, scheme);
            {
                let mut s = view.state.borrow_mut();
                let (a, b) = (Anchor::new(1, 1.0), Anchor::new(5, 2.0));
                s.drawings = [DrawingKind::Vwap, DrawingKind::Rect, DrawingKind::Profile]
                    .into_iter()
                    .map(|kind| {
                        let mut drawing = Drawing::new(kind, a, b);
                        drawing.follow(1);
                        drawing
                    })
                    .collect();
                s.selected = vec![0, 1, 2];
            }
            let widths = |view: &Rc<ChartView>| -> Vec<f64> {
                let s = view.state.borrow();
                s.drawings.iter().map(|d| d.style(&s.configs).width).collect()
            };
            let before = widths(&view);
            assert!(view.step_width(drawings::WIDTH_STEP));
            let after = widths(&view);
            for (was, now) in before.iter().zip(&after) {
                assert_eq!(*now, was + drawings::WIDTH_STEP);
            }
            assert!(view.state.borrow().drawings.iter().all(|d| d.config.is_none()), "each its own look now");
            for _ in 0..40 {
                view.step_width(-drawings::WIDTH_STEP);
            }
            assert!(widths(&view).iter().all(|w| *w == drawings::MIN_WIDTH));
            for _ in 0..40 {
                view.step_width(drawings::WIDTH_STEP);
            }
            assert!(widths(&view).iter().all(|w| *w == drawings::MAX_WIDTH));
        });
    }

    /// A retracement already as thick or as thin as a level goes stays on
    /// its configuration when stepped further, with nothing to undo; one
    /// step back from the end is a step like any other.
    #[test]
    fn a_retracement_at_the_end_of_the_thickness_range_is_left_alone() {
        crate::on_gtk(|| {
            let theme = omacharts_engine::theme::builtin_themes()
                .into_iter()
                .next()
                .expect("a built-in theme");
            let scheme = omacharts_engine::theme::theme_bars(&theme);
            let view = ChartView::new(theme, scheme);
            {
                let mut s = view.state.borrow_mut();
                let mut thickest = s.configs.of(DrawingKind::Fib, 1).clone();
                thickest.fib.set_width(drawings::MAX_WIDTH);
                s.configs.set(DrawingKind::Fib, 1, thickest);
                let mut drawing = Drawing::new(DrawingKind::Fib, Anchor::new(1, 1.0), Anchor::new(5, 2.0));
                drawing.follow(1);
                s.drawings = vec![drawing];
                s.selected = vec![0];
            }
            view.step_width(drawings::WIDTH_STEP);
            {
                let s = view.state.borrow();
                assert_eq!(s.drawings[0].config, Some(1), "a step past the end took it off its configuration");
                assert!(s.undo.is_empty(), "a step that changed nothing can be undone");
            }
            view.step_width(-drawings::WIDTH_STEP);
            let s = view.state.borrow();
            assert_eq!(s.drawings[0].config, None);
            assert_eq!(s.drawings[0].style(&s.configs).fib.width(), Some(drawings::MAX_WIDTH - drawings::WIDTH_STEP));
            assert_eq!(s.undo.len(), 1);
        });
    }

    /// A right-click with a drawing half laid down drops it and keeps the
    /// tool in hand, for another go.
    #[test]
    fn a_drawing_half_laid_down_is_dropped_and_the_tool_kept() {
        crate::on_gtk(|| {
            let theme = omacharts_engine::theme::builtin_themes()
                .into_iter()
                .next()
                .expect("a built-in theme");
            let scheme = omacharts_engine::theme::theme_bars(&theme);
            let view = ChartView::new(theme, scheme);
            view.arm(Some(DrawingKind::Line));
            assert!(!view.drop_placing(), "nothing laid down yet");
            {
                let mut s = view.state.borrow_mut();
                let at = Anchor::new(1, 1.0);
                s.placing = Some(Drawing::new(DrawingKind::Line, at, at));
                s.drag = Some(Drag::Place { moved: true });
            }
            assert!(view.drop_placing());
            let s = view.state.borrow();
            assert!(s.placing.is_none() && s.drag.is_none(), "the drawing is gone");
            assert_eq!(s.tool, Some(DrawingKind::Line), "the tool is still in hand");
        });
    }

    /// The chart wears one tooltip — the trouble dot's — and GTK opens it by
    /// asking which widget is under the pointer. There is a second drawing
    /// area over the chart now, and everything the chart is driven by, the
    /// tooltip included, depends on that layer never being the answer to that
    /// question.
    #[test]
    fn the_pointer_layer_lets_every_event_through_to_the_chart() {
        crate::on_gtk(|| {
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
        });
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
    fn a_time_label_is_pushed_in_from_either_edge_rather_than_cut() {
        // A plot from 10 to 410, and a label 80 wide.
        assert_eq!(inside_plot(200.0, 80.0, 10.0, 400.0), 160.0);
        assert_eq!(inside_plot(28.0, 80.0, 10.0, 400.0), 10.0);
        assert_eq!(inside_plot(392.0, 80.0, 10.0, 400.0), 330.0);
        // Wider than the plot: the edges cross, which `clamp` panics on.
        assert_eq!(inside_plot(40.0, 80.0, 10.0, 50.0), 10.0);
    }

    #[test]
    fn axis_labels_match_the_span_on_screen() {
        const DAY: i64 = 86_400;
        // A fixed instant so the assertions do not drift with the clock.
        let ts = 1_700_000_000;

        // Years apart: years only. This is the case that read "01 Jun" for
        // every tick before the spacing was taken into account.
        let decade = format_axis_time(ts, 400 * DAY, false, false, Clock::default());
        assert_eq!(decade.len(), 4, "{decade}");
        assert!(decade.chars().all(|c| c.is_ascii_digit()), "{decade}");

        // A year or two: month and year.
        assert!(format_axis_time(ts, 30 * DAY, false, false, Clock::default()).contains("20"), "months get a year");
        // A few days apart: day and month, no year.
        assert!(!format_axis_time(ts, 5 * DAY, false, false, Clock::default()).contains("20"));
        // Intraday carries a clock until the ticks are months apart.
        assert!(format_axis_time(ts, 3600, true, false, Clock::default()).contains(':'));
        assert!(format_axis_time(ts, 5 * DAY, true, false, Clock::default()).contains(':'));
        assert_eq!(
            format_axis_time(ts, 30 * DAY, true, false, Clock::default()),
            format_axis_time(ts, 30 * DAY, false, false, Clock::default())
        );
    }

    #[test]
    fn intraday_ticks_a_few_days_apart_do_not_repeat_a_date() {
        // A week of 15-minute bars: several ticks land inside each day, so a
        // date-only label printed the same thing over and over.
        let tick = 6 * 3600;
        let labels: Vec<String> = (0..6)
            .map(|i| format_axis_time(1_700_000_000 + i * tick, tick, true, false, Clock::default()))
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
            .map(|i| format_axis_time(1_767_225_600 + i * tick, tick, false, false, Clock::default()))
            .collect();
        let mut unique = labels.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), labels.len(), "repeated labels: {labels:?}");
    }

    /// A monthly bar opens at 00:00 UTC on the first, which a clock west of
    /// Greenwich reads as the last evening of the month before.
    #[test]
    fn a_monthly_bar_is_named_for_its_own_month_in_any_zone() {
        let february = 1_706_745_600; // 2024-02-01 00:00 UTC
        assert_eq!(format_time_full(february, Timeframe::months(1), Clock::default()), "Feb 2024");
        assert_eq!(format_time_full(1_704_067_200, Timeframe::years(1), Clock::default()), "2024");
        assert_eq!(format_axis_time(february, 31 * 86_400, false, true, Clock::default()), "Feb 2024");
    }

    #[test]
    fn monthly_bars_get_a_tick_a_month_when_there_is_room() {
        use chrono::{TimeZone, Utc};
        let month = |column: i64| {
            let (year, month0) = (2000 + column.div_euclid(12), column.rem_euclid(12));
            Utc.with_ymd_and_hms(year as i32, month0 as u32 + 1, 1, 0, 0, 0).unwrap()
        };
        let step = Timeframe::months(1).seconds();
        let (ticks, every) =
            axis_ticks(12, 12.0 * TIME_LABEL_GAP, step, true, Clock::default(), |c| Some(month(c).timestamp()));
        assert_eq!(every, step);
        assert_eq!(ticks.len(), 11, "{ticks:?}");
    }

    #[test]
    fn the_crosshair_names_a_zone_the_way_people_in_it_do() {
        let at = |zone| Clock { zone, format: TimeFormat::H24, named: true };
        // 2024-07-01 12:00 UTC: summer on both sides of the Atlantic.
        let summer = 1_719_835_200;
        assert_eq!(at(chrono_tz::America::Chicago).abbreviation(summer), "CT");
        assert_eq!(at(chrono_tz::America::New_York).abbreviation(summer), "ET");
        assert_eq!(at(chrono_tz::Europe::London).abbreviation(summer), "BST");
        assert_eq!(at(chrono_tz::Asia::Tokyo).abbreviation(summer), "JST");
        let full = format_time_full(summer, Timeframe::minutes(15), at(chrono_tz::America::Chicago));
        assert_eq!(full, "01 Jul 2024 07:00 CT");
    }

    #[test]
    fn two_ticks_a_decade_apart_get_different_labels() {
        const DAY: i64 = 86_400;
        let tick = 400 * DAY;
        let a = format_axis_time(1_200_000_000, tick, false, false, Clock::default());
        let b = format_axis_time(1_700_000_000, tick, false, false, Clock::default());
        assert_ne!(a, b);
    }

    #[test]
    fn weekly_gridlines_fall_on_the_first_week_of_a_month() {
        use chrono::{Datelike, TimeZone};
        use chrono_tz::America::New_York as Local;
        // Every twenty-six bars put "Mar 2024" on whichever week the count
        // reached. Monday noon, so no zone moves a bar across a month.
        const WEEK: i64 = 7 * 86_400;
        let time_at = |column: i64| Some(1_704_110_400 + column * WEEK);
        let month = |column: i64| {
            let at = Local.timestamp_opt(time_at(column).unwrap(), 0).unwrap();
            (at.year(), at.month())
        };
        let (ticks, _) = axis_ticks(520, 3_120.0, WEEK, false, Clock::default(), time_at);
        assert!(ticks.len() > 5, "{ticks:?}");
        for column in ticks {
            assert_ne!(month(column), month(column - 1), "column {column} is mid-month");
        }
    }

    /// The bug: an intraday session opens mid-stretch, so its first bar and
    /// the round time just after it both took a tick — "15:30" and "16:00"
    /// two bars apart, printed over each other.
    #[test]
    fn an_intraday_session_open_does_not_crowd_the_round_time_after_it() {
        use chrono::TimeZone;
        use chrono_tz::America::New_York as Local;
        // Regular hours at 15m: 26 bars a session, then an overnight gap.
        const STEP: i64 = 900;
        let mut stamps = Vec::new();
        let mut day =
            Local.with_ymd_and_hms(2026, 10, 5, 15, 30, 0).single().expect("a session open");
        for _ in 0..5 {
            for bar in 0..26 {
                stamps.push(day.timestamp() + bar * STEP);
            }
            day += chrono::Duration::days(1);
        }

        let total = stamps.len() as i64;
        let plot_w = 2_400.0;
        let (ticks, _) = axis_ticks(total, plot_w, STEP, false, Clock::default(), |column| {
            stamps.get(column as usize).copied()
        });

        let bar_w = plot_w / total as f64;
        for pair in ticks.windows(2) {
            let gap = (pair[1] - pair[0]) as f64 * bar_w;
            assert!(gap >= TIME_LABEL_GAP, "{gap} px between columns {pair:?}");
        }
        // And the one that survives each session open is the round time.
        let minute = |column: &i64| {
            let at = Local.timestamp_opt(stamps[*column as usize], 0).unwrap();
            at.format("%H:%M").to_string()
        };
        let at = ticks.iter().map(minute).collect::<Vec<_>>();
        assert!(at.iter().all(|minute| minute.ends_with(":00")), "{at:?}");
    }

    #[test]
    fn decimals_follow_the_step() {
        assert_eq!(decimals_for(100.0), 0);
        assert_eq!(decimals_for(1.0), 0);
        assert_eq!(decimals_for(0.5), 1);
        assert_eq!(decimals_for(0.01), 2);
        assert_eq!(decimals_for(0.0001), 4);
        // Steps that are not a power of ten keep the digits they need.
        assert_eq!(decimals_for(20.0), 0);
        assert_eq!(decimals_for(2.5), 1);
        assert_eq!(decimals_for(0.25), 2);
        assert_eq!(decimals_for(0.0005), 4);
    }

    /// A strip with these values, drawn on whatever scale it asks for.
    fn strip(
        values: &[Option<f64>],
        signal: Option<&[Option<f64>]>,
        histogram: Option<&[Option<f64>]>,
        guides: &[f64],
    ) -> indicators::Pane {
        indicators::Pane {
            values: values.to_vec(),
            signal: signal.map(<[Option<f64>]>::to_vec),
            histogram: histogram.map(<[Option<f64>]>::to_vec),
            height: 0.16,
            bounds: None,
            guides: guides.to_vec(),
            band: None,
            delta: None,
        }
    }

    /// The strip code is shared, so the first thing to establish about any
    /// change to it is that RSI and ATR are drawn on the scale they always
    /// were: a fixed one is its own bounds whatever is in it, and a fitted
    /// one with nothing to anchor it is still the values with a little air.
    #[test]
    fn what_a_strip_is_drawn_on_has_not_changed_for_the_strips_that_were_here() {
        let values = [Some(10.0), Some(40.0), Some(90.0)];

        let mut fixed = strip(&values, None, None, &[30.0, 50.0, 70.0]);
        fixed.bounds = Some((0.0, 100.0));
        assert_eq!(pane_range(&fixed, 0, 3), Some((0.0, 100.0)), "an RSI is always nought to 100");

        // An ATR: fitted, no guides, and the air either side is what it was.
        let fitted = strip(&values, None, None, &[]);
        let (low, high) = pane_range(&fitted, 0, 3).expect("a range");
        let air = (90.0 - 10.0) * 0.12;
        assert!((low - (10.0 - air)).abs() < 1e-9, "{low}");
        assert!((high - (90.0 + air)).abs() < 1e-9, "{high}");
    }

    /// A MACD is the first strip whose values go either side of nothing, and
    /// three things follow that nothing else here needed: the scale has to
    /// take negatives, it has to cover the second line and the histogram as
    /// well as the first line, and it has to hold the zero guide — a guide
    /// outside the fitted range would not merely be invisible, it would be
    /// drawn across whichever strip is next down the stack.
    #[test]
    fn a_strips_scale_covers_everything_it_draws_including_below_nothing() {
        let line = [Some(-4.0), Some(-1.0), Some(2.0)];
        let signal = [Some(-6.0), Some(-2.0), Some(1.0)];
        let histogram = [Some(2.0), Some(1.0), Some(8.0)];

        let both = strip(&line, Some(&signal), Some(&histogram), &[0.0]);
        let (low, high) = pane_range(&both, 0, 3).expect("a range");
        assert!(low <= -6.0, "the signal's trough is off the bottom: {low}");
        assert!(high >= 8.0, "the histogram's peak is off the top: {high}");
        assert!(low < 0.0 && high > 0.0, "nothing has to be on the scale");

        // All one side of nothing, which is where a zero line would otherwise
        // fall outside the strip it belongs to.
        let above = strip(&[Some(5.0), Some(9.0)], None, None, &[0.0]);
        let (low, high) = pane_range(&above, 0, 2).expect("a range");
        assert!(low <= 0.0, "the zero line fell off the bottom: {low}");
        assert!(high >= 9.0);
    }


    /// Hiding every indicator takes their strips away as well, so the price
    /// gets the height, and leaves each one's own `visible` as it was.
    #[test]
    fn hiding_the_indicators_gives_their_strips_back_to_the_price() {
        let (w, h) = (600.0, 400.0);
        let mut state = charted(11);
        state.bars = ramp(40);
        let series = state.bars.clone();
        let indicator = Indicator::new(1, indicators::Kind::Atr);
        state.indicators = vec![Drawn {
            color: "#5588ff".to_string(),
            output: indicators::compute(&indicator, &series, 0, state.timeframe, None),
            indicator,
        }];
        let shown = layout(&state, w, h);
        assert!(shown.rows.iter().any(|row| row.pane == Some(1)), "the ATR has a strip");

        state.show_indicators = false;
        let hidden = layout(&state, w, h);
        assert!(hidden.rows.iter().all(|row| row.pane.is_none()), "no strip while hidden");
        assert!(hidden.price_h > shown.price_h, "the price takes the room");
        assert!(state.indicators[0].indicator.visible, "the indicator's own switch is untouched");
        assert_eq!(state.shown_indicators().count(), 0);
    }

    /// An ATR zoomed into its own warm-up has nothing to scale to and nothing
    /// to draw, and it is the one strip that can be in that position: a fixed
    /// scale is its own bounds whatever is in it, and a MACD's zero guide is a
    /// scale on its own. So it is the one that can come out as a blank band —
    /// and a blank band with no name in it is a strip you have to count down
    /// the stack to identify. It draws its name and nothing else.
    #[test]
    fn a_strip_with_nothing_to_draw_yet_still_says_which_indicator_it_is() {
        let (w, h) = (600.0, 400.0);
        // Fourteen bars of warm-up, and eleven of them on screen.
        let mut state = charted(11);
        state.bars = ramp(11);
        state.visible = 11;
        let series = state.bars.clone();
        let indicator = Indicator::new(1, indicators::Kind::Atr);
        state.indicators = vec![Drawn {
            color: "#5588ff".to_string(),
            output: indicators::compute(&indicator, &series, 0, state.timeframe, None),
            indicator,
        }];

        let Output::Pane(pane) = &state.indicators[0].output else { panic!("a strip") };
        assert!(pane.values.iter().all(Option::is_none), "every bar is still warming up");
        assert_eq!(pane_range(pane, 0, 11), None, "and so there is no scale");

        let pixels = frame(w, h, |cr| draw(cr, w, h, &state));
        let plan = layout(&state, w, h);
        let row = plan.rows.iter().find(|row| row.pane == Some(1)).expect("the ATR's own row");
        // Where the name is written, and the same box at the other end of the
        // strip, which nothing writes in. Inset so the row's own edge line is
        // in neither.
        let left = ink_in(&pixels, w as usize, plan.plot_x + 1.0, row.top + 1.0, 90.0, 14.0);
        let right =
            ink_in(&pixels, w as usize, plan.plot_x + plan.plot_w - 91.0, row.top + 1.0, 90.0, 14.0);
        assert_eq!(right, 0, "something is drawn where nothing should be");
        assert!(left > 20, "the empty strip has no name in it: {left} pixels of ink");
    }

    /// How many pixels in this box are not the colour of the box's own
    /// background, taken as the pixel in its top-right corner.
    fn ink_in(pixels: &[u8], width: usize, x: f64, y: f64, w: f64, h: f64) -> usize {
        let at = |x: usize, y: usize| -> &[u8] {
            let start = (y * width + x) * 4;
            &pixels[start..start + 4]
        };
        let (x0, y0) = (x.round() as usize, y.round() as usize);
        let (bw, bh) = (w.round() as usize, h.round() as usize);
        let ground = at(x0 + bw - 1, y0).to_vec();
        (y0..y0 + bh)
            .flat_map(|y| (x0..x0 + bw).map(move |x| (x, y)))
            .filter(|(x, y)| at(*x, *y) != ground.as_slice())
            .count()
    }

    /// Warm-up: a MACD has no value for its first 33 bars, and a strip with
    /// nothing in it yet still has a scale to draw its own furniture on
    /// rather than drawing nothing at all.
    #[test]
    fn a_strip_with_nothing_in_it_yet_still_has_somewhere_to_draw_its_zero() {
        let empty = strip(&[None, None], None, None, &[0.0]);
        let (low, high) = pane_range(&empty, 0, 2).expect("a range");
        assert!(low < 0.0 && high > 0.0, "{low} to {high}");

        // And with no guides either there is genuinely nothing to scale to.
        assert_eq!(pane_range(&strip(&[None, None], None, None, &[]), 0, 2), None);
    }

    /// The whole chart with the two new indicators on it, drawn through the
    /// real draw function: a panic in here does not unwind, it takes the app
    /// with it, so the degenerate shapes are drawn rather than reasoned about.
    #[test]
    fn drawing_the_new_indicators_paints_something_and_never_panics() {
        let (w, h) = (600.0, 400.0);
        for bars in [0usize, 1, 5, 40, 300] {
            let mut state = charted(bars.max(1));
            state.bars = ramp(bars);
            state.visible = bars;
            let series = state.bars.clone();
            state.indicators = [indicators::Kind::Bollinger, indicators::Kind::Macd]
                .into_iter()
                .enumerate()
                .map(|(at, kind)| {
                    let indicator = Indicator::new(at as u32 + 1, kind);
                    Drawn {
                        color: "#5588ff".to_string(),
                        output: indicators::compute(&indicator, &series, 0, state.timeframe, None),
                        indicator,
                    }
                })
                .collect();

            let painted_pixels = painted(&frame(w, h, |cr| draw(cr, w, h, &state)));
            match bars {
                0 => {}
                _ => assert!(painted_pixels > 0, "{bars} bars drew nothing"),
            }
            // And the crosshair layer over it, which reads the strips back on
            // the same scale they were drawn on.
            state.pointer = Some((300.0, 360.0));
            let _ = frame(w, h, |cr| draw_pointer(cr, w, h, &state));
        }
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

    #[test]
    fn ruler_ignores_the_header_axes_and_indicator_strips() {
        let mut state = charted(400);
        let indicator = Indicator::new(1, indicators::Kind::Rsi);
        state.indicators.push(Drawn {
            color: "#5588ff".into(),
            output: indicators::compute(&indicator, &state.bars, 0, state.timeframe, None),
            indicator,
        });
        let (w, h) = (800.0, 400.0);
        let plan = layout(&state, w, h);
        let x = plan.plot_x + plan.plot_w / 2.0;
        let y = plan.price_y + plan.price_h / 2.0;
        assert!(state.ruler_anchor(w, h, x, y).is_some());
        for (x, y) in [(x, 0.0), (w - 1.0, y), (x, h - 1.0),
            (x, plan.price_y + plan.price_h + 10.0)] {
            assert!(state.ruler_anchor(w, h, x, y).is_none(), "accepted ({x}, {y})");
        }
    }

    #[test]
    fn a_ruler_takes_its_direction_from_the_prices_not_the_screen() {
        let at = |price| Anchor::new(1, price);
        assert_eq!(RulerDirection::of(at(100.0), at(110.0)), RulerDirection::Up);
        assert_eq!(RulerDirection::of(at(110.0), at(100.0)), RulerDirection::Down);
        assert_eq!(RulerDirection::of(at(100.0), at(100.0)), RulerDirection::Flat);
        assert_eq!(RulerDirection::of(at(100.0), at(f64::NAN)), RulerDirection::Flat);
    }

    #[test]
    fn a_ruler_arrow_points_only_at_the_end_in_both_dimensions() {
        // Up and to the right: the end is higher on screen, so a smaller y.
        let [price, time] = ruler_arrows((100.0, 300.0), (400.0, 100.0));
        assert_eq!(price, ((250.0, 300.0), (250.0, 100.0)));
        assert_eq!(time, ((100.0, 200.0), (400.0, 200.0)));
        // Down and to the left: the tips follow, pointing down and left.
        let [price, time] = ruler_arrows((400.0, 100.0), (100.0, 300.0));
        assert_eq!(price.1, (250.0, 300.0));
        assert!(price.1.1 > price.0.1, "points down");
        assert_eq!(time.1, (100.0, 200.0));
        assert!(time.1.0 < time.0.0, "points left");
    }

    #[test]
    fn a_ruler_arrowhead_shrinks_on_a_short_arrow_and_goes_on_a_tiny_one() {
        assert_eq!(ruler_head(200.0), Some(9.0));
        assert_eq!(ruler_head(27.0), Some(9.0));
        assert_eq!(ruler_head(18.0), Some(6.0));
        assert_eq!(ruler_head(9.0), Some(3.0));
        assert_eq!(ruler_head(8.9), None);
        assert_eq!(ruler_head(0.0), None);
        assert_eq!(ruler_head(f64::NAN), None);
    }

    #[test]
    fn ruler_click_move_click_freezes_and_a_third_click_starts_again() {
        let mut state = charted(400);
        let a = Anchor::new(1, 100.0);
        let b = Anchor::new(2, 110.0);
        let c = Anchor::new(3, 95.0);
        let drawings = state.drawings.clone();
        state.ruler_click(a);
        assert_eq!(state.ruler, Some((a, a, false)));
        assert!(
            state.drag.is_none(),
            "releasing the first click must not finish it"
        );
        state.ruler_motion(b);
        assert_eq!(state.ruler, Some((a, b, false)));
        state.ruler_click(c);
        state.ruler_motion(b);
        assert_eq!(
            state.ruler,
            Some((a, c, true)),
            "motion after finishing must leave it fixed"
        );
        state.ruler_click(b);
        assert_eq!(state.ruler, Some((b, b, false)));
        assert_eq!(
            state.drawings, drawings,
            "a ruler never becomes stored content"
        );
    }

    /// Where the hand is becomes an anchor, and the anchor is drawn back
    /// under the hand: the two conversions share one scale.
    #[test]
    fn a_pixel_located_is_projected_back_under_the_pointer() {
        let state = charted(400);
        let (w, h) = (800.0, 400.0);
        let (first, visible) = state.slice();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let bar_w = plan.plot_w / visible as f64;
        for (x, y) in [(120.0, 90.0), (400.0, 200.0), (plan.plot_x + plan.plot_w - 3.0, 150.0)] {
            let anchor = state.locate(w, h, x, y).expect("a chart with bars locates");
            let drawing = Drawing::new(DrawingKind::Line, anchor, anchor);
            let projected = state.project(&plan, &scale, &drawing);
            // The moment snapped to a bar, so x is off by at most half a bar.
            assert!(
                (projected.from.0 - x).abs() <= bar_w / 2.0 + 0.01,
                "x {x} came back as {}",
                projected.from.0
            );
            assert!((projected.from.1 - y).abs() < 0.01, "y {y} came back as {}", projected.from.1);
        }
    }

    /// The same on a log axis: a pixel located and projected back lands where
    /// it started, and the price under it is not the linear one.
    #[test]
    fn a_pixel_located_on_a_log_axis_is_projected_back_under_the_pointer() {
        let mut state = charted(400);
        let (w, h) = (800.0, 400.0);
        let (first, visible) = state.slice();
        let plan = layout(&state, w, h);
        let linear = state.locate(w, h, 400.0, 150.0).unwrap().price;
        state.price_log = true;
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        assert!(scale.log);
        let anchor = state.locate(w, h, 400.0, 150.0).unwrap();
        assert!((anchor.price - linear).abs() > 0.01, "a log axis puts another price there");
        let drawing = Drawing::new(DrawingKind::Line, anchor, anchor);
        let projected = state.project(&plan, &scale, &drawing);
        assert!((projected.from.1 - 150.0).abs() < 0.01, "came back as {}", projected.from.1);
    }

    /// A log axis maps equal ratios to equal heights, and back.
    #[test]
    fn a_log_scale_spaces_ratios_evenly() {
        let scale = PriceScale { low: 10f64.ln(), high: 1000f64.ln(), log: true };
        let (top, h) = (0.0, 400.0);
        assert!((scale.to_y(1000.0, top, h) - 0.0).abs() < 1e-9);
        assert!((scale.to_y(100.0, top, h) - 200.0).abs() < 1e-9, "100 is halfway");
        assert!((scale.to_y(10.0, top, h) - 400.0).abs() < 1e-9);
        for y in [0.0, 37.5, 200.0, 399.0] {
            let back = scale.to_y(scale.price_at(y, top, h), top, h);
            assert!((back - y).abs() < 1e-9, "{y} came back as {back}");
        }
        // Moved by a ratio: up by ln 2 is doubled, whatever the price.
        assert!((scale.shift(50.0, 2f64.ln()) - 100.0).abs() < 1e-9);
        assert!((scale.shift(3.0, 2f64.ln()) - 6.0).abs() < 1e-9);
        // A price with no logarithm still lands somewhere, below the plot.
        assert!(scale.to_y(0.0, top, h).is_finite());
        assert!(scale.to_y(-5.0, top, h) > h);
    }

    /// The labels on a log axis are round, rising, inside the plot and never
    /// on top of each other — across a range a linear step could not serve.
    #[test]
    fn a_log_axis_is_labelled_at_round_prices() {
        for (low, high, h) in [(10.0, 10_000.0, 600.0), (0.5, 80.0, 300.0), (95.0, 140.0, 500.0)] {
            let scale = PriceScale { low: f64::ln(low), high: f64::ln(high), log: true };
            let mut ticks = Vec::new();
            scale.ticks(h, |price| ticks.push(price));
            assert!(ticks.len() >= 3, "{low}..{high}: {ticks:?}");
            for pair in ticks.windows(2) {
                let gap = scale.to_y(pair[0], 0.0, h) - scale.to_y(pair[1], 0.0, h);
                assert!(gap >= 52.0 * 0.5 - 1e-6, "{low}..{high}: {ticks:?} crowd at {pair:?}");
            }
            for price in &ticks {
                assert!((low..=high).contains(price), "{price} is off the plot");
                // A narrow axis counts in fives like a linear one — 95, 100,
                // 105 — but nothing is ever written as 97.3.
                let unit = 10f64.powf(price.log10().floor() - 2.0);
                assert!(((price / unit).round() - price / unit).abs() < 1e-6, "{price} is not round");
            }
        }
        // Across three decades, the decades themselves are labelled.
        let scale = PriceScale { low: 10f64.ln(), high: 10_000f64.ln(), log: true };
        let mut ticks = Vec::new();
        scale.ticks(600.0, |price| ticks.push(price));
        for decade in [100.0, 1000.0] {
            assert!(ticks.iter().any(|t| (t - decade).abs() < 1e-9), "{decade} missing: {ticks:?}");
        }
    }

    /// Bars at or below zero have no logarithm: the chart draws linear while
    /// they are on screen and keeps asking for log.
    #[test]
    fn a_log_axis_over_negative_prices_draws_linear() {
        let mut state = charted(50);
        state.price_log = true;
        for bar in state.bars.iter_mut().take(10) {
            bar.low = -3.0;
        }
        let plan = layout(&state, 800.0, 400.0);
        let scale = price_range(&state, &state.bars, &plan).unwrap();
        assert!(!scale.log, "no logarithm of a negative price");
        assert!(state.price_log, "the choice is kept");
        let scale = price_range(&state, &state.bars[10..], &plan).unwrap();
        assert!(scale.log, "and comes back once they are off screen");
    }

    /// An anchor dropped past the last candle is a whole number of bars
    /// after it, which is what keeps it on the same bar when that bar
    /// arrives.
    #[test]
    fn an_anchor_past_the_last_bar_lands_on_a_future_bar() {
        let mut state = charted(100);
        state.visible = 100;
        state.overhang = 0;
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

    /// With Ctrl, an anchor on a candle takes whichever of its open, high,
    /// low and close is nearest the pointer, on either price axis; past the
    /// last bar there is no candle and the price is left where the pointer
    /// is.
    #[test]
    fn a_snapped_anchor_lands_on_the_nearest_price_of_its_candle() {
        for log in [false, true] {
            let mut state = charted(100);
            state.visible = 100;
            state.overhang = 0;
            state.price_log = log;
            let (w, h) = (800.0, 400.0);
            let plan = layout(&state, w, h);
            let (first, visible) = state.slice();
            let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
            assert_eq!(scale.log, log);
            let bar_w = plan.plot_w / visible as f64;
            let bar = state.bars[40];
            let x = plan.plot_x + 40.5 * bar_w;
            // A pixel above the high, a pixel below the low, and a pixel
            // off the open and the close, which make up the body.
            for (target, nudge) in [(bar.high, -1.0), (bar.low, 1.0), (bar.open, 1.0), (bar.close, -1.0)] {
                let y = scale.to_y(target, plan.price_y, plan.price_h) + nudge;
                let free = state.locate(w, h, x, y).unwrap();
                assert_ne!(free.price, target, "without Ctrl the hand is followed");
                let snapped = state.locate_with(w, h, x, y, true).unwrap();
                assert_eq!(snapped.ts, bar.ts);
                assert_eq!(snapped.price, target, "aimed at {target}, log {log}");
            }
            let past = plan.plot_x + 120.5 * bar_w;
            let free = state.locate(w, h, past, 100.0).unwrap();
            assert_eq!(state.locate_with(w, h, past, 100.0, true), Some(free));
        }
    }

    /// Ctrl snaps only while an anchor is being put somewhere: a tool in
    /// hand, or a corner in hand. Held over a chart with neither, or while a
    /// whole drawing is dragged, it is not the magnet.
    #[test]
    fn the_magnet_is_on_only_while_an_anchor_is_in_hand() {
        let mut state = charted(100);
        state.magnet = true;
        assert!(!state.snapping(), "nothing in hand");
        state.tool = Some(DrawingKind::Line);
        assert!(state.snapping(), "a tool in hand");
        state.tool = None;
        let origin = Anchor::new(state.bars[0].ts, 100.0);
        state.drag = Some(Drag::Grip { index: 0, grip: Grip::To, origin });
        assert!(state.snapping(), "a corner in hand");
        state.drag = Some(Drag::Grip { index: 0, grip: Grip::Body, origin });
        assert!(!state.snapping(), "a whole drawing in hand");
        state.magnet = false;
        state.tool = Some(DrawingKind::Line);
        assert!(!state.snapping(), "no Ctrl");
    }

    /// Ctrl going down with a drawing half laid down puts its loose end on
    /// the candle where the pointer already is, without waiting for a move.
    #[test]
    fn a_drawing_half_laid_down_snaps_when_ctrl_goes_down() {
        let mut state = charted(100);
        state.visible = 100;
        state.overhang = 0;
        let (w, h) = (800.0, 400.0);
        let plan = layout(&state, w, h);
        let (first, visible) = state.slice();
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let bar = state.bars[40];
        let x = plan.plot_x + 40.5 * plan.plot_w / visible as f64;
        let y = scale.to_y(bar.high, plan.price_y, plan.price_h) - 1.0;
        state.tool = Some(DrawingKind::Line);
        assert!(state.begin_placing(w, h, 100.0, 100.0, false));
        state.drag = None;
        state.pointer = Some((x, y));
        state.follow_hand(w, h);
        assert_ne!(state.placing.as_ref().unwrap().to.price, bar.high);
        state.magnet = true;
        state.follow_hand(w, h);
        assert_eq!(state.placing.as_ref().unwrap().to.price, bar.high);
    }

    /// The configuration chosen for the next drawing survives picking a
    /// tool, so the two can be asked for in either order.
    #[test]
    fn choosing_a_configuration_and_choosing_a_tool_are_one_sentence() {
        let mut state = charted(400);

        // Tool first, then the number — which always worked.
        state.tool = Some(DrawingKind::Line);
        state.next_config = 3;
        assert!(state.begin_placing(800.0, 400.0, 200.0, 150.0, false));
        assert_eq!(state.placing.as_ref().unwrap().config, Some(3));

        // And the other way round: the number is already set when the tool
        // arrives, and arming must not throw it away.
        let mut state = charted(400);
        state.next_config = 3;
        state.tool = Some(DrawingKind::Rect);
        assert!(state.begin_placing(800.0, 400.0, 200.0, 150.0, false));
        assert_eq!(state.placing.as_ref().unwrap().config, Some(3));

        // Laying one down ends the choice: the next is the usual one.
        state.placing = None;
        state.next_config = 1;
        state.tool = Some(DrawingKind::Ellipse);
        assert!(state.begin_placing(800.0, 400.0, 200.0, 150.0, false));
        assert_eq!(state.placing.as_ref().unwrap().config, Some(1));
    }

    /// A press back on a zig-zag's last corner ends it there, which is
    /// what a double-click too slow to count as one meant: it used to pin
    /// another corner on the same spot.
    #[test]
    fn a_press_on_the_last_corner_ends_a_zigzag() {
        let (w, h) = (800.0, 400.0);
        let mut state = charted(400);
        state.tool = Some(DrawingKind::Zigzag);
        assert!(state.begin_placing(w, h, 200.0, 150.0, false));
        // One corner pinned: ending there would leave no drawing.
        assert!(!state.closes_path(w, h, 200.0, 150.0));

        // A second corner, pinned the way a press pins one.
        let corner = state.locate(w, h, 300.0, 100.0).unwrap();
        let placing = state.placing.as_mut().unwrap();
        placing.move_last_corner(corner);
        placing.add_corner(corner);

        assert!(state.closes_path(w, h, 300.0, 100.0));
        assert!(state.closes_path(w, h, 303.0, 102.0), "a hand a few pixels off still means that corner");
        assert!(!state.closes_path(w, h, 200.0, 150.0), "the first corner is not where it ends");
        assert!(!state.closes_path(w, h, 400.0, 200.0));
    }

    /// A paste puts the copies down, takes hold of them, and leaves one
    /// step back on the stack however many were pasted.
    #[test]
    fn pasting_puts_the_copies_down_and_takes_hold_of_them() {
        let mut state = charted(400);
        let (w, h) = (800.0, 400.0);
        let a = Anchor::new(state.bars[100].ts, state.bars[100].close);
        let b = Anchor::new(state.bars[140].ts, state.bars[140].close);
        let mut one = Drawing::new(DrawingKind::Rect, a, b);
        one.id = 7;
        let mut two = Drawing::new(DrawingKind::Line, a, b);
        two.id = 9;

        let added = state.paste(&[one.clone(), two.clone()], true, w, h);
        assert_eq!(added.len(), 2, "both copies went down");
        assert_eq!(state.drawings.len(), 2);
        assert!(added.iter().all(|d| d.id == 0), "a copy is not the drawing it came from");
        assert_eq!(state.selected, vec![0, 1], "the copies are what is in hand");
        assert_eq!(state.undo.len(), 1, "two drawings is still one step back");

        // Offset, so a copy of something on this chart is visible beside it.
        assert_ne!(state.drawings[0].from, a, "the copy landed exactly on the original");
        // And the shape is unchanged: both ends moved by the same amount.
        let moved = &state.drawings[0];
        assert_eq!(moved.to.ts - moved.from.ts, b.ts - a.ts);

        // Asked not to offset — a paste onto another chart — it lands on
        // the moment and the price it was copied from.
        let mut elsewhere = charted(400);
        elsewhere.paste(&[one.clone()], false, w, h);
        assert_eq!(elsewhere.drawings[0].from, a);
        assert_eq!(elsewhere.drawings[0].to, b);
    }

    /// The selection survives the list being rebuilt — which is what
    /// happens the moment a paste is written down — with every pasted copy
    /// still in hand, not just the last of them.
    #[test]
    fn every_pasted_copy_is_still_selected_after_the_list_comes_back() {
        let mut state = charted(400);
        let a = Anchor::new(state.bars[100].ts, state.bars[100].close);
        let b = Anchor::new(state.bars[140].ts, state.bars[140].close);
        // One already written down, and three just pasted.
        let mut old = Drawing::new(DrawingKind::Line, a, b);
        old.id = 4;
        state.drawings.push(old);
        let copies = vec![
            Drawing::new(DrawingKind::Rect, a, b),
            Drawing::new(DrawingKind::Ellipse, a, b),
            Drawing::new(DrawingKind::Line, a, b),
        ];
        state.paste(&copies, true, 800.0, 400.0);
        assert_eq!(state.selected, vec![1, 2, 3]);

        // The list comes back from the store with the copies still
        // unnumbered, as it does between the paste and the write.
        let ids = state.selected_ids();
        assert_eq!(ids, vec![0, 0, 0], "a copy has no id until it is written down");
        let same = state.drawings.clone();
        state.drawings = same;
        state.reselect(ids);
        assert_eq!(state.selected, vec![1, 2, 3], "only the last copy stayed in hand");

        // And one on its own still means the newest row.
        state.reselect(vec![0]);
        assert_eq!(state.selected, vec![3]);
    }

    /// A copy belongs to the chart it was pasted on, and goes where that
    /// chart sends what is drawn on it.
    #[test]
    fn a_pasted_copy_takes_the_chart_s_own_sharing() {
        let (w, h) = (800.0, 400.0);
        let a = Anchor::new(1, 1.0);
        let copy = Drawing::new(DrawingKind::Rect, a, Anchor::new(2, 2.0));

        let mut sending = charted(400);
        sending.sends = true;
        sending.paste(&[copy.clone()], false, w, h);
        assert_eq!(sending.drawings[0].scope, drawings::Scope::Shared);

        let mut keeping = charted(400);
        keeping.sends = false;
        keeping.paste(&[copy], false, w, h);
        assert_eq!(keeping.drawings[0].scope, drawings::Scope::Local);
    }

    /// Nothing copied, nothing pasted — and the caller is told, so the key
    /// can go on to what it otherwise means.
    #[test]
    fn pasting_nothing_does_nothing() {
        let mut state = charted(400);
        assert!(state.paste(&[], true, 800.0, 400.0).is_empty());
        assert!(state.drawings.is_empty());
        assert!(state.undo.is_empty(), "nothing happened, so there is nothing to undo");
    }

    /// Alt+R, Alt+3, click: the rectangle laid down follows configuration 3
    /// and goes where the chart sends what is drawn on it.
    #[test]
    fn a_tool_in_hand_draws_in_the_configuration_chosen_for_it() {
        let mut state = charted(400);
        state.sends = false;
        state.tool = Some(DrawingKind::Rect);
        state.next_config = 3;
        assert!(state.begin_placing(800.0, 400.0, 200.0, 150.0, false));
        let placing = state.placing.clone().expect("a drawing in hand");
        assert_eq!(placing.kind, DrawingKind::Rect);
        assert_eq!(placing.config, Some(3));
        assert_eq!(placing.scope, drawings::Scope::Local, "a chart that does not send keeps it");
        state.sends = true;
        assert!(state.begin_placing(800.0, 400.0, 200.0, 150.0, false));
        assert_eq!(
            state.placing.clone().expect("a drawing in hand").scope,
            drawings::Scope::Shared
        );
        assert_eq!(state.drag, Some(Drag::Place { moved: false }));
        // Nothing in hand, nothing begins.
        state.tool = None;
        state.placing = None;
        assert!(!state.begin_placing(800.0, 400.0, 200.0, 150.0, false));
    }

    /// A chart that hides its drawings draws none of them, and none answers
    /// the pointer or a marquee; they are all still held.
    #[test]
    fn hidden_drawings_are_neither_drawn_nor_picked() {
        let mut state = charted(400);
        let (w, h) = (800.0, 400.0);
        let a = state.locate(w, h, 200.0, 150.0).unwrap();
        let b = state.locate(w, h, 500.0, 250.0).unwrap();
        let bare = super::tests::frame(w, h, |cr| draw(cr, w, h, &state));
        state.drawings.push(Drawing::new(DrawingKind::Rect, a, b));
        assert!(state.drawing_at(w, h, 350.0, 200.0).is_some(), "shown, the box answers");
        assert_ne!(super::tests::frame(w, h, |cr| draw(cr, w, h, &state)), bare, "and is drawn");

        state.show_drawings = false;
        assert_eq!(state.drawing_at(w, h, 350.0, 200.0), None);
        assert!(state.drawings_in(w, h, (0.0, 0.0, w, h)).is_empty());
        assert_eq!(super::tests::frame(w, h, |cr| draw(cr, w, h, &state)), bare, "nothing of it is left on the picture");
        assert_eq!(state.drawings.len(), 1, "it is still held");
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

    /// An anchored VWAP is the engine's line from the bar it is anchored to,
    /// projected onto the chart: it starts on that bar at that bar's own
    /// value, goes on to the last bar, and grows when a bar arrives.
    #[test]
    fn an_anchored_vwap_runs_from_its_bar_to_the_last() {
        let mut state = charted(60);
        state.visible = 60;
        let (w, h) = (800.0, 400.0);
        let (first, visible) = state.slice();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let start = 40;
        let anchor = Anchor::new(state.bars[start].ts, 0.0);
        let drawing = Drawing::new(DrawingKind::Vwap, anchor, anchor);

        let projected = state.project(&plan, &scale, &drawing);
        let expected = omacharts_engine::indicators::vwap::anchored(
            &state.bars[start..],
            omacharts_engine::drawings::VwapSource::Hlc3,
        );
        assert_eq!(projected.corners.len(), 60 - start, "a point a bar, anchor to last");
        let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
        for (n, (point, value)) in projected.corners.iter().zip(&expected).enumerate() {
            assert!((point.1 - to_y(value.vwap)).abs() < 1e-9, "point {n}");
        }
        // Its anchor is the line's first point, on the anchor bar, at the
        // anchor bar's typical price rather than the price it was given.
        let bar = &state.bars[start];
        assert_eq!(projected.from, projected.corners[0]);
        assert!((projected.from.1 - to_y((bar.high + bar.low + bar.close) / 3.0)).abs() < 1e-9);
        assert_eq!(projected.hit(projected.from.0, projected.from.1), Some(Grip::From));

        // A bar arrives: the line reaches it, and its earlier values stand.
        let mut next = *state.bars.last().unwrap();
        next.ts += 3600;
        next.volume = 50.0;
        next.close += 10.0;
        next.high += 10.0;
        state.bars.push(next);
        state.visible = 61;
        let (first, visible) = state.slice();
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let grown = state.project(&plan, &scale, &drawing);
        assert_eq!(grown.corners.len(), 61 - start, "the new bar is on the line");
        let (_, line) = state.anchored_vwap(&drawing).unwrap();
        assert_eq!(line[..expected.len()], expected[..], "what was there is unchanged");
        assert!(line.last().unwrap().vwap > expected.last().unwrap().vwap, "pulled up by the heavy bar");
    }

    /// One click with the tool is the whole drawing, and a click past the
    /// last bar anchors it on the last.
    #[test]
    fn an_anchored_vwap_clicked_past_the_end_starts_on_the_last_bar() {
        let state = charted(60);
        let last = state.bars.last().unwrap().ts;
        let later = Anchor::new(last + 5 * 3600, 1.0);
        assert_eq!(anchor_on_a_bar(&state.bars, later).ts, last);
        let earlier = Anchor::new(state.bars[0].ts - 3600, 1.0);
        assert_eq!(anchor_on_a_bar(&state.bars, earlier).ts, state.bars[0].ts);
        let on = Anchor::new(state.bars[7].ts, 1.0);
        assert_eq!(anchor_on_a_bar(&state.bars, on), on);
    }

    /// A retracement's levels are where their prices are on the chart's
    /// scale, 0 at the end of the drag and 1 at its start, whichever way it
    /// was dragged.
    #[test]
    fn a_retracement_puts_0_where_the_drag_ended_up_or_down() {
        let mut state = charted(60);
        state.visible = 60;
        let (w, h) = (800.0, 400.0);
        let (first, visible) = state.slice();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let to_y = |price: f64| scale.to_y(price, plan.price_y, plan.price_h);
        let (low, high) = (Anchor::new(state.bars[10].ts, 99.0), Anchor::new(state.bars[40].ts, 109.0));
        for (from, to) in [(low, high), (high, low)] {
            let drawing = Drawing::new(DrawingKind::Fib, from, to);
            let projected = state.project(&plan, &scale, &drawing);
            assert_eq!(projected.levels.len(), drawings::FIB_RATIOS.len());
            assert!((projected.levels[0] - to_y(to.price)).abs() < 1e-9, "0 is the end of the drag");
            assert!((projected.levels[6] - to_y(from.price)).abs() < 1e-9, "1 is its start");
            let golden = to.price + (from.price - to.price) * 0.618;
            assert!((projected.levels[4] - to_y(golden)).abs() < 1e-9);
            assert_eq!(projected.hit(projected.to.0, projected.to.1), Some(Grip::To));
        }
        // Dragged up, 0.618 is under the middle; dragged down, over it.
        let up = state.project(&plan, &scale, &Drawing::new(DrawingKind::Fib, low, high));
        let down = state.project(&plan, &scale, &Drawing::new(DrawingKind::Fib, high, low));
        let middle = to_y(104.0);
        assert!(up.levels[4] > middle && down.levels[4] < middle);
    }

    /// On a swing too short for every label, the two ends keep theirs and
    /// the levels between give way.
    #[test]
    fn a_short_swing_keeps_the_labels_at_its_ends() {
        let levels = drawings::fib_levels();
        // 0% at 100 px and 100% at 130: 30 px for seven labels.
        let at: Vec<(&drawings::FibLevel, f64)> = levels.iter().map(|level| (level, 100.0 + level.ratio * 30.0)).collect();
        let slots = fib_label_slots(&at);
        let names: Vec<String> = slots.iter().map(|i| at[*i].0.name()).collect();
        assert!(names.contains(&"0%".to_string()) && names.contains(&"100%".to_string()), "{names:?}");
        for (i, a) in slots.iter().enumerate() {
            for b in &slots[i + 1..] {
                assert!((at[*a].1 - at[*b].1).abs() >= drawings::FIB_LABEL_SIZE + 1.0, "{names:?} overlap");
            }
        }
        // A tall one labels them all.
        let tall: Vec<(&drawings::FibLevel, f64)> = levels.iter().map(|level| (level, level.ratio * 400.0)).collect();
        assert_eq!(fib_label_slots(&tall).len(), 7);
    }

    /// A retracement scrolled off the left paints nothing, labels
    /// included; one half off keeps its labels whole, at the edge of the
    /// plot, and drops the ends' labels last on a swing too short for all.
    #[test]
    fn a_retracements_labels_stay_inside_the_plot() {
        let mut state = charted(60);
        state.visible = 30;
        let (w, h) = (800.0, 400.0);
        let (first, _) = state.slice();
        assert!(first >= 20, "the first bars are off the left: {first}");
        let plan = layout(&state, w, h);
        let paint = |state: &State| -> Vec<u8> {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                let (first, visible) = state.slice();
                let plan = layout(state, w, h);
                let scale = price_range(state, &state.bars[first..first + visible], &plan).unwrap();
                draw_drawings(&cr, state, &plan, &scale, false);
            }
            surface.flush();
            surface.data().unwrap().to_vec()
        };
        let stride = w as usize * 4;
        let inked_in = |pixels: &[u8], x0: f64, x1: f64| {
            pixels
                .chunks(4)
                .enumerate()
                .filter(|(i, p)| {
                    let x = ((i * 4) % stride / 4) as f64;
                    p[3] > 0 && x >= x0 && x < x1
                })
                .count()
        };
        let fib = |state: &State, from: usize, to: usize, labelled: bool| {
            let mut drawing = Drawing::new(
                DrawingKind::Fib,
                Anchor::new(state.bars[from].ts, 99.0),
                Anchor::new(state.bars[to].ts, 109.0),
            );
            if !labelled {
                drawing.edit_style(&state.configs, |s| s.fib.fib_labels = false);
            }
            drawing
        };

        state.drawings = vec![fib(&state, 2, 10, true)];
        assert_eq!(inked_in(&paint(&state), 0.0, w), 0, "a retracement off the left still painted");

        // Half off: the labels sit at the plot's left edge, whole.
        state.drawings = vec![fib(&state, first - 10, (first + 15).min(59), true)];
        let labelled = paint(&state);
        state.drawings = vec![fib(&state, first - 10, (first + 15).min(59), false)];
        let bare = paint(&state);
        let edge = plan.plot_x;
        assert!(
            inked_in(&labelled, edge, edge + 40.0) > inked_in(&bare, edge, edge + 40.0),
            "no labels at the plot's left edge"
        );

        // Half off with too little left of it for a label: none.
        state.drawings = vec![fib(&state, first - 10, first, true)];
        let thin = paint(&state);
        state.drawings = vec![fib(&state, first - 10, first, false)];
        assert_eq!(thin, paint(&state), "a label overran a sliver of a span");
    }

    /// A level that is switched off is not drawn and not picked; a level's
    /// colour, thickness and pattern each change what is painted; and the
    /// labels are one switch for all of them.
    #[test]
    fn a_retracement_paints_each_level_as_it_is_set() {
        let mut state = charted(60);
        state.visible = 60;
        let (w, h) = (800.0, 400.0);
        let drawing = Drawing::new(
            DrawingKind::Fib,
            Anchor::new(state.bars[10].ts, 99.0),
            Anchor::new(state.bars[40].ts, 109.0),
        );
        let paint = |state: &State| -> Vec<u8> {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                let (first, visible) = state.slice();
                let plan = layout(state, w, h);
                let scale = price_range(state, &state.bars[first..first + visible], &plan).unwrap();
                draw_drawings(&cr, state, &plan, &scale, false);
            }
            surface.flush();
            surface.data().unwrap().to_vec()
        };
        let inked = |pixels: &[u8]| pixels.chunks(4).filter(|p| p[3] > 0).count();
        let configs = state.configs.clone();
        let with = |edit: &dyn Fn(&mut drawings::Style)| {
            let mut changed = drawing.clone();
            changed.edit_style(&configs, |s| edit(s));
            changed
        };
        state.drawings = vec![drawing.clone()];
        let plain = paint(&state);
        assert!(inked(&plain) > 0, "nothing painted");

        state.drawings = vec![with(&|s| s.fib.price_labels = true)];
        let priced = paint(&state);
        assert!(inked(&priced) > inked(&plain), "a price beside each ratio painted nothing");
        state.drawings = vec![with(&|s| {
            s.fib.fib_labels = false;
            s.fib.price_labels = true;
        })];
        let prices = paint(&state);
        assert!(inked(&prices) < inked(&priced), "the ratios are still drawn beside the prices");
        assert_ne!(prices, plain, "a price label looks like a ratio label");

        state.drawings = vec![with(&|s| s.fib.fib_labels = false)];
        let unlabelled = paint(&state);
        assert!(inked(&unlabelled) < inked(&plain), "the labels painted nothing");

        state.drawings = vec![with(&|s| {
            s.fib.fib_labels = false;
            s.fib.level_mut(0.5).unwrap().enabled = false;
        })];
        assert!(inked(&paint(&state)) < inked(&unlabelled), "a level switched off is still drawn");

        state.drawings = vec![with(&|s| {
            s.fib.fib_labels = false;
            s.fib.level_mut(0.5).unwrap().colour = Some(drawings::Paint::fixed("#ff00ff"));
        })];
        let coloured = paint(&state);
        assert_ne!(coloured, unlabelled, "the colour changed nothing");
        assert_eq!(inked(&coloured), inked(&unlabelled), "only the colour changed");

        state.drawings = vec![with(&|s| {
            s.fib.fib_labels = false;
            s.fib.level_mut(0.5).unwrap().stroke.width = 4.0;
        })];
        assert!(inked(&paint(&state)) > inked(&unlabelled), "the thickness changed nothing");

        state.drawings = vec![with(&|s| {
            s.fib.fib_labels = false;
            s.fib.level_mut(0.5).unwrap().stroke.style = drawings::LineStyle::Dotted;
        })];
        assert!(inked(&paint(&state)) < inked(&unlabelled), "a dotted level is as solid as before");

        // Switched off, a level is not where the drawing is picked.
        let (first, visible) = state.slice();
        let plan = layout(&state, w, h);
        let scale = price_range(&state, &state.bars[first..first + visible], &plan).unwrap();
        let hidden = with(&|s| s.fib.level_mut(0.5).unwrap().enabled = false);
        let projected = state.project(&plan, &scale, &hidden);
        let y = scale.to_y(104.0, plan.price_y, plan.price_h);
        let x = projected.from.0 + (projected.to.0 - projected.from.0) * 0.9;
        assert_eq!(projected.hit(x, y), None);
        assert_eq!(state.project(&plan, &scale, &drawing).hit(x, y), Some(Grip::Body));
    }

    /// The bands are drawn round the line when asked for, and not otherwise.
    #[test]
    fn an_anchored_vwap_paints_its_bands_only_when_asked() {
        let mut state = charted(60);
        state.visible = 60;
        let (w, h) = (800.0, 400.0);
        let anchor = Anchor::new(state.bars[10].ts, 0.0);
        let mut drawing = Drawing::new(DrawingKind::Vwap, anchor, anchor);
        let paint = |state: &State, under: bool| -> Vec<u8> {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                let (first, visible) = state.slice();
                let plan = layout(state, w, h);
                let scale = price_range(state, &state.bars[first..first + visible], &plan).unwrap();
                draw_drawings(&cr, state, &plan, &scale, under);
            }
            surface.flush();
            surface.data().unwrap().to_vec()
        };
        let inked = |pixels: &[u8]| pixels.chunks(4).filter(|p| p[3] > 0).count();
        let empty = paint(&state, false);
        state.drawings = vec![drawing.clone()];
        let line = paint(&state, false);
        assert_ne!(line, empty, "the line painted nothing");
        assert_eq!(inked(&paint(&state, true)), 0, "nothing shaded before a band is");

        // The outer band is a plain line either side, over the bars.
        let configs = state.configs.clone();
        drawing.edit_style(&configs, |s| s.vwap.bands[2].enabled = true);
        state.drawings = vec![drawing.clone()];
        let banded = paint(&state, false);
        assert!(inked(&banded) > inked(&line), "the bands added nothing");

        // The inner one is shading with no outline: under the bars, and
        // nothing over them.
        drawing.edit_style(&configs, |s| {
            s.vwap.bands[2].enabled = false;
            s.vwap.bands[0].enabled = true;
        });
        state.drawings = vec![drawing];
        assert_eq!(paint(&state, false), line, "a shade with no outline draws no line");
        assert!(inked(&paint(&state, true)) > 0, "the shading went missing");
    }

    /// Selected, it glows as a selected study does: wider than the line,
    /// under it.
    #[test]
    fn a_selected_anchored_vwap_glows() {
        let mut state = charted(60);
        state.visible = 60;
        let (w, h) = (800.0, 400.0);
        let anchor = Anchor::new(state.bars[10].ts, 0.0);
        state.drawings = vec![Drawing::new(DrawingKind::Vwap, anchor, anchor)];
        let inked = |state: &State| {
            let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32).unwrap();
            {
                let cr = cairo::Context::new(&surface).unwrap();
                let (first, visible) = state.slice();
                let plan = layout(state, w, h);
                let scale = price_range(state, &state.bars[first..first + visible], &plan).unwrap();
                draw_drawings(&cr, state, &plan, &scale, false);
            }
            surface.flush();
            surface.data().unwrap().chunks(4).filter(|p| p[3] > 0).count()
        };
        let plain = inked(&state);
        state.selected = vec![0];
        assert!(inked(&state) > plain * 2, "no glow round the line");
    }

    /// Daily bars, about five and a half years across 1088 pixels: the count
    /// of half-yearly ticks on screen went 11, 12, 11 as the chart was
    /// dragged, and the axis flipped between half-years and years with it.
    #[test]
    fn dragging_a_chart_keeps_its_time_axis_step() {
        use chrono::{Datelike, TimeZone, Utc};
        let mut weekdays = Vec::new();
        let mut day = Utc.with_ymd_and_hms(2012, 1, 2, 0, 0, 0).unwrap();
        while day.year() < 2024 {
            if day.weekday().num_days_from_monday() < 5 {
                weekdays.push(day.timestamp());
            }
            day += chrono::Duration::days(1);
        }
        let (total, plot_w) = (1_480, 1_088.0);
        let every_at = |offset: usize| {
            let (_, every) = axis_ticks(total, plot_w, 86_400, true, Clock::default(), |column| {
                weekdays.get(offset + column as usize).copied()
            });
            every
        };
        let first = every_at(0);
        for offset in 1..520 {
            assert_eq!(every_at(offset), first, "dragged {offset} bars");
        }
    }

    /// Twenty-one yearly bars across 922 pixels: ten two-yearly ticks fit by
    /// count but sit 88 pixels apart, and thinning them left only the last.
    #[test]
    fn evenly_spaced_ticks_are_not_thinned_down_to_one() {
        use chrono::{TimeZone, Utc};
        let year = |c: i64| Utc.with_ymd_and_hms(2006 + c as i32, 1, 1, 0, 0, 0).unwrap().timestamp();
        let step = Timeframe::years(1).seconds();
        let (ticks, _) = axis_ticks(21, 922.0, step, true, Clock::default(), |c| Some(year(c)));
        assert!(ticks.len() > 2, "{ticks:?}");
    }
}
