//! The omacharts chart, as a GTK4 widget any GTK application can embed.
//!
//! [`ChartView`] is one chart: the bars, their scales, the indicator strips
//! stacked with them, the drawings over them, the crosshair, and everything
//! a hand or a keyboard does to them. It draws what it is handed and says
//! what was done to it; it never reads a store, a feed or a setting of its
//! own. Whoever embeds it owns all of that.
//!
//! ```ignore
//! let view = ChartView::new(theme, scheme);
//! container.append(&view.root);
//! view.set_series(instrument, timeframe, bars);
//! view.set_indicators(drawn);            // computed with omacharts_engine
//! view.set_drawings(drawings);
//! view.set_marks(marks, AnyPrice);       // price marks, see `marks`
//! view.set_drawing_handler(|event| { /* store it */ });
//! view.set_mark_handler(|event| { /* act on it */ });
//! ```
//!
//! What goes in:
//!
//! - **The series**: [`ChartView::set_series`], and
//!   [`ChartView::apply_tail`] as bars form.
//! - **How it looks**: the theme and bar colours ([`ChartView::new`],
//!   [`ChartView::restyle`]), the bar style, the grid, the clock
//!   ([`Clock`]), and [`STYLESHEET`] added to the display's CSS.
//! - **Indicators**, computed by the engine and coloured by the caller:
//!   [`ChartView::set_indicators`] with a [`Drawn`] each.
//! - **Drawings**: [`ChartView::set_drawings`], with the
//!   [`omacharts_engine::drawings`] the engine defines; anything anchored in
//!   both time and price is one of these.
//! - **Price marks**: [`ChartView::set_marks`], see [`marks`].
//!
//! What comes out is said through handlers — `set_*_handler` — and never
//! acted on here: a drawing added or moved ([`DrawingEvent`]), an indicator
//! picked ([`IndicatorEvent`]), a mark clicked or dragged ([`MarkEvent`]),
//! the crosshair moving ([`Hover`]), a pane resized, closed or moved, a
//! context menu asked for.
//!
//! [`bench`] drives the same drawing off screen, for measuring it.

pub mod colors;
pub mod marks;
pub mod shapes;
pub mod text;
mod text_editor;
mod view;
pub mod zoom;

pub use marks::{AnyPrice, AxisChip, LabelSide, MarkDrag, MarkEvent, MarkLink, MarkPreview, MarkRules, MarkTab, PriceMark, MARK_NEAR};
pub use view::bench;
pub use view::{
    clamp_thickness, delta_colours, volume_colours, volume_opacity, ChartView, Clock, DrawingEvent, Drawn, Echo, Hidden, Hover,
    IndicatorEvent, ProfileSpot, DEFAULT_OHLC_THICKNESS, MAX_OHLC_THICKNESS, MIN_OHLC_THICKNESS, POINTER_LAYER,
    PRICE_AXIS_W, TIME_AXIS_H, VISIBLE_BARS,
};

/// The class the editor typed into on the chart wears.
pub const TEXT_EDITOR_CLASS: &str = text_editor::CLASS;

/// The CSS the chart's own widgets need, for the display's provider: the
/// editor typed into on the chart is see-through, so the chart shows
/// through it.
pub const STYLESHEET: &str = "\
/* Typing on the chart: nothing behind the glyphs, so what is under them is
   the chart. The view and its text layer are transparent so the chart shows
   through. The selection is NOT — it was in this list, under
   `selection:focus`, which is the selection you have while you are typing:
   every range you took was taken invisibly. */
.drawing-text-editor, .drawing-text-editor text { background: none; background-color: transparent; }
/* The caret is the accent, which is the one thing on a text widget that has
   to be found rather than read. Nothing else: the words are already in the
   drawing's own face, size and ink, over the drawing's own ground, and a
   border or a highlight round them would be the one thing on screen that
   changes when the edit is committed. A chart you can type on should not
   flinch when you press Enter. */
.drawing-text-editor { caret-color: @accent_bg_color; }
.drawing-text-editor text selection, .drawing-text-editor text selection:focus { background-color: alpha(@accent_bg_color, 0.45); color: @window_fg_color; }
";

/// Run a test that builds widgets on the one thread GTK is ever
/// initialised on, and say whether it ran.
///
/// libtest gives every test its own thread, and GTK belongs to the first
/// thread that initialises it, so every widget test is handed to a single
/// thread that owns GTK for the life of the binary, one at a time, and its
/// panic is carried back to the test that sent it. With no display nothing
/// runs and the answer is false. The app's own suite has the same helper,
/// with the reasoning at length.
#[cfg(test)]
pub(crate) fn on_gtk(test: impl FnOnce() + Send + 'static) -> bool {
    use std::sync::{OnceLock, mpsc};
    type Job = (Box<dyn FnOnce() + Send>, mpsc::Sender<std::thread::Result<()>>);
    static GTK: OnceLock<Option<mpsc::Sender<Job>>> = OnceLock::new();
    let gtk = GTK.get_or_init(|| {
        let (jobs, queue) = mpsc::channel::<Job>();
        let (ready, answer) = mpsc::channel();
        std::thread::Builder::new()
            .name("gtk".into())
            .spawn(move || {
                let ok = gtk::init().is_ok();
                let _ = ready.send(ok);
                if !ok {
                    return;
                }
                for (test, done) in queue {
                    let _ = done.send(std::panic::catch_unwind(std::panic::AssertUnwindSafe(test)));
                }
            })
            .expect("a thread for GTK");
        answer.recv().unwrap_or(false).then_some(jobs)
    });
    let Some(gtk) = gtk else { return false };
    let (done, finished) = mpsc::channel();
    gtk.send((Box::new(test), done)).expect("the GTK thread");
    if let Err(panic) = finished.recv().expect("the GTK thread") {
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("a panic with no message");
        panic!("on the GTK thread: {message}");
    }
    true
}
