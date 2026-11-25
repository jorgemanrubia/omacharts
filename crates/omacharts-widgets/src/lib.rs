//! The pieces around an omacharts chart, for any GTK application that
//! embeds [`omacharts_chart::ChartView`]: what the chart's own window puts
//! round it, so a program embedding the chart can offer the same things,
//! behaving and looking the same.
//!
//! - [`drawing_bar`]: the drawing tools, down the left edge.
//! - [`search`]: the symbol picker, over an [`inventory::Inventory`] the
//!   embedder fills.
//! - [`range_card`]: Ctrl+G's range and a digit's resolution, with
//!   [`range_card::plan`] for the resolution a range would show at.
//! - [`strip`]: a chart's row of resolutions.
//! - [`pick_frame`]: the frame those pickers are drawn in.
//! - [`shortcuts`]: what the keys do, in one table.
//! - [`drawing_settings`]: a drawing's properties and a kind's nine
//!   configurations, over a [`drawing_settings::Host`]; built from
//!   [`property_rows`], [`profile_rows`], [`colouring_rows`], [`palette`],
//!   [`controls`] and [`stroke`], which a chart's own settings use too.
//!
//! Each says what was picked through a handler and changes nothing itself.
//! [`STYLESHEET`] goes into the display's CSS beside the chart's.

pub mod colouring_rows;
pub mod controls;
pub mod dialogs;
pub mod drawing_bar;
pub mod drawing_settings;
pub mod inventory;
pub mod menus;
pub mod palette;
pub mod pick_frame;
pub mod profile_rows;
pub mod property_rows;
pub mod range_card;
pub mod search;
pub mod shortcuts;
pub mod strip;
pub mod stroke;

/// The CSS these widgets are drawn with, in libadwaita's named colours.
pub const STYLESHEET: &str = r#"
/* The symbol palette: a quiet list where the row, not a border, carries the
   selection. */
.symbol-row-ticker { font-weight: 700; }
.symbol-row-name { font-size: 0.9em; opacity: 0.7; }
.symbol-kind {
  font-size: 0.75em;
  padding: 1px 6px;
  border-radius: 6px;
  background: alpha(@accent_bg_color, 0.18);
  color: @accent_color;
}

/* Timeframe buttons: flat strip, the active one filled. */
.timeframe-strip { padding: 2px; }
.timeframe-strip button { min-width: 34px; padding: 2px 6px; }

/* Where a symbol trades: quieter than its name, beside what it is. */
.pick-venue {
  font-family: monospace;
  font-size: 0.78em;
  opacity: 0.45;
}

/* The row offering a symbol nobody listed. Quieter than a real match, because
   it is a guess rather than an answer. */
.symbol-row-unlisted .pick-name { font-style: italic; }
.symbol-row-unlisted .symbol-kind { opacity: 0.4; }

/* A picker over a chart (see `pick_frame`) is the same surface with no title
   bar: the search box is its top edge, on a hairline, and the chart it will
   change is named at the end of it. The box takes no frame of its own, the
   surface already being one. */
popover.pick-frame > contents { padding: 0; }
.pick-query {
  padding: 4px 12px 4px 4px;
  border-bottom: 1px solid alpha(@window_fg_color, 0.08);
}
.pick-query entry,
.pick-query entry:focus-within {
  background: none;
  box-shadow: none;
  outline: none;
  font-size: 1.08em;
}
.pick-target { font-family: monospace; font-size: 0.78em; opacity: 0.55; }
.pick-list { padding: 4px; background: none; }
/* A picker's rows are the rail's: 26px, the ticker in the face numbers and
   keys are written in, the name a dim reminder beside it, and the rail's
   tint for the one Enter would take. */
.pick-list.navigation-sidebar > row {
  min-height: 26px;
  padding: 0 8px;
  margin: 0 0 1px 0;
  border-radius: 6px;
}
.pick-list.navigation-sidebar > row:selected { background-color: alpha(@accent_color, 0.22); }
.pick-list.navigation-sidebar > row:selected:hover { background-color: alpha(@accent_color, 0.28); }
.pick-list.navigation-sidebar > row:selected:focus-visible { outline: none; }
.pick-ticker { font-family: monospace; font-weight: 700; font-size: 0.9em; }
.pick-name { font-size: 0.9em; opacity: 0.55; }
/* A futures root's fold, at the row's far edge: no padding of its own, so
   the row keeps its height, and quiet until pointed at. Under an open root,
   its contracts; Front, on the one trading now, sits where a kind would,
   in the chip's grey rather than the kind's accent, being a date's doing
   and not a category. */
.pick-expander { min-height: 0; min-width: 0; padding: 0 2px; opacity: 0.45; -gtk-icon-size: 14px; }
.pick-expander:hover { opacity: 1; }
.pick-front {
  font-size: 0.75em;
  padding: 1px 6px;
  border-radius: 6px;
  background: alpha(@card_fg_color, 0.08);
  opacity: 0.75;
}
/* Every chart while a picker is open, faded under it; the one it will
   change wears the wide ring. */
.pick-shade { background-color: alpha(black, 0.4); }

/* A picker that takes something typed — Ctrl+G's range, a digit's resolution:
   a centred title with an ⓘ for the syntax, a framed box with the text
   centred, and under it what the answer comes to. Nothing under the box
   comes or goes with typing; the list drops down over it in a popover of
   its own. */
popover.pick-bare > contents { padding: 0; }
popover.pick-bare > contents > box { padding-bottom: 22px; }
popover.pick-bare .pick-header { padding: 14px 20px 0 20px; }
popover.pick-bare .pick-title { font-weight: 700; font-size: 1.2em; }
popover.pick-bare .pick-info { opacity: 0.35; -gtk-icon-size: 14px; }
popover.pick-bare .pick-info:hover { opacity: 0.8; }
popover.pick-bare .pick-query { padding: 14px 20px 0 20px; border-bottom: none; }
popover.pick-bare .pick-query entry {
  min-height: 42px;
  border-radius: 7px;
  font-family: monospace;
  font-size: 1.1em;
  background: alpha(@window_fg_color, 0.05);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.14);
}
popover.pick-bare .pick-query entry:focus-within { box-shadow: inset 0 0 0 2px @accent_color; }
popover.pick-bare .pick-query entry.error { box-shadow: inset 0 0 0 2px @error_color; }
popover.range-goto .pick-query entry { min-height: 48px; }
popover.pick-drop > contents { padding: 0; }
/* The month under the Go to card, as wide as it: one header between two
   arrows, From to To shaded, the end being set filled and the other ringed.
   The band stops under the two ends' squares rather than past them. */
popover.range-month-pop > contents { padding: 12px 14px 14px 14px; }
.range-month-title { font-weight: 700; padding: 4px 10px; min-height: 0; border-radius: 7px; }
.range-month-step { min-width: 30px; min-height: 30px; padding: 0; border-radius: 7px; -gtk-icon-size: 16px; }
.range-month-pick { min-height: 40px; padding: 0; border-radius: 8px; font-size: 0.9em; font-weight: 400; font-feature-settings: 'tnum'; }
.range-month-pick.today { box-shadow: inset 0 0 0 1px alpha(currentColor, 0.35); }
.range-month-pick.setting { background: @accent_color; color: @accent_fg_color; font-weight: 700; }
.range-month-name { font-size: 0.78em; opacity: 0.55; min-height: 22px; }
.range-month-cell.between { background: alpha(@accent_color, 0.14); }
.range-month-cell.start { background-image: linear-gradient(to right, transparent 50%, alpha(@accent_color, 0.14) 50%); }
.range-month-cell.end { background-image: linear-gradient(to left, transparent 50%, alpha(@accent_color, 0.14) 50%); }
.range-month-day {
  min-width: 30px;
  min-height: 30px;
  padding: 0;
  border-radius: 8px;
  font-size: 0.9em;
  font-weight: 400;
  font-feature-settings: 'tnum';
}
.range-month-day.out { opacity: 0.3; }
.range-month-day.today { box-shadow: inset 0 0 0 1px alpha(currentColor, 0.35); }
/* The ringed end is filled with the band's own colour, opaque, so the band
   seems to end under it rather than cut through its middle. */
.range-month-day.other {
  background-color: mix(@popover_bg_color, @accent_color, 0.14);
  box-shadow: inset 0 0 0 1.5px @accent_color;
  font-weight: 600;
}
.range-month-day.setting { background: @accent_color; color: @accent_fg_color; font-weight: 700; }

/* The ⓘ's help, beside the card on its own ground: examples as key caps in
   the accent, what they mean right-aligned. */
popover.background.pick-help-pop > contents { padding: 14px 16px; }
.pick-help { min-width: 268px; }
.pick-help-section { font-size: 0.78em; letter-spacing: 0.06em; color: alpha(currentColor, 0.5); }
.pick-help-group.later,
.pick-help-foot { margin-top: 9px; padding-top: 9px; border-top: 1px solid alpha(currentColor, 0.13); }
.pick-help-key,
.pick-help-cap {
  font-family: monospace;
  font-weight: 700;
  font-size: 0.85em;
  padding: 1px 5px;
  border-radius: 4px;
  background: alpha(@window_fg_color, 0.07);
  box-shadow: inset 0 -1px 0 alpha(@window_fg_color, 0.18), inset 0 0 0 1px alpha(@window_fg_color, 0.12);
}
.pick-help-key { color: @accent_color; }
.pick-help-what { color: alpha(currentColor, 0.75); font-size: 0.9em; }
.pick-help-foot { font-size: 0.85em; color: alpha(currentColor, 0.5); }
.pick-help-cap { color: @window_fg_color; }
.pick-help-note { font-size: 0.85em; color: alpha(currentColor, 0.6); margin-top: 4px; }
.pick-help-note.first { margin-top: 9px; padding-top: 9px; border-top: 1px solid alpha(currentColor, 0.13); }

.range-hint { padding: 8px 20px 0 20px; font-size: 0.9em; opacity: 0.6; }
.range-hint.error { color: @error_color; opacity: 1; }
.range-dates { padding: 12px 20px 0 20px; }
.range-cell-name { font-size: 0.78em; opacity: 0.45; }
.range-today { font-size: 0.78em; color: @success_color; }
.range-arrow { opacity: 0.4; margin-bottom: 8px; }
/* From and To: plain boxes a day is typed into, with GTK's calendar in the
   corner for the pointer. */
entry.range-date {
  min-height: 0;
  padding: 0 6px 0 9px;
  font-family: monospace;
  font-size: 0.88em;
  font-feature-settings: 'tnum';
  border-radius: 7px;
  background: alpha(@window_fg_color, 0.05);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.14);
  outline: none;
}
entry.range-date > text { padding: 7px 0; }
entry.range-date:focus-within { box-shadow: inset 0 0 0 2px @accent_color; outline: none; }
entry.range-date.idle { color: alpha(@window_fg_color, 0.7); }
entry.range-date.error { box-shadow: inset 0 0 0 2px @error_color; }
entry.range-date.unread { color: @error_color; }
entry.range-date > image { opacity: 0.45; -gtk-icon-size: 13px; }
entry.range-date > image:hover { opacity: 0.9; }

/* The resolution, pinned in the range box's bottom-right corner. */
.range-chip-row { padding: 8px 20px 0 20px; }
.range-chip {
  margin: 0;
  padding: 1px 6px;
  border-radius: 5px;
  font-family: monospace;
  font-size: 0.78em;
  font-weight: 700;
  background: alpha(@window_fg_color, 0.05);
  box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.18);
}
.range-chip:focus { box-shadow: inset 0 0 0 2px @accent_color; outline: none; }
.range-chip:hover { box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.35); }
/* Nothing typed: just `auto`, faded, with nothing to open and no hover. */
.range-chip.inert, .range-chip.inert:hover { opacity: 0.5; box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.18); }
.range-caret { font-size: 0.8em; opacity: 0.5; }
.range-auto { font-family: sans-serif; font-weight: 400; font-size: 0.9em; opacity: 0.55; }

/* The drawing tools, down the left edge: a column of two, quiet until one is
   in hand, which lights it in the accent the way a pressed tool should. */
.drawing-bar { padding: 0; }
/* The rules between the bands of tools: present, and no louder than that. */
.drawing-tool-rule { background: alpha(@window_fg_color, 0.14); min-height: 1px; margin: 3px 7px; }
.drawing-tool { padding: 2px; border-radius: 8px; opacity: 0.8; }
.drawing-tool:hover { opacity: 1; }
.drawing-tool:checked { opacity: 1; background: alpha(@accent_bg_color, 0.25); box-shadow: inset 0 0 0 1px @accent_bg_color; }
/* A tool that stays in hand: the same lit button, held in a solid ring. The
   chart otherwise goes on drawing and nothing says why. */
.drawing-tool.drawing-tool-stuck:checked { background: alpha(@accent_bg_color, 0.38); box-shadow: inset 0 0 0 2px @accent_bg_color; }
.drawing-config-badge { font-size: 9px; font-weight: 700; min-width: 12px; min-height: 12px; padding: 0 2px; border-radius: 6px; background: @accent_bg_color; color: @accent_fg_color; margin: 0 -4px -4px 0; }

/* A drawing's settings (see `drawing_settings`). */
/* Thickness and line style, picked by looking rather than by naming a number.
   A segmented strip, so the options sit beside each other and the one in use
   is the one pressed. */
.stroke-picker button {
  padding: 2px 4px;
  min-width: 0;
  min-height: 0;
}

.swatch-button { min-width: 26px; min-height: 26px; padding: 0; border-radius: 6px; }

.drawing-preview { border-radius: 6px; }
.drawing-config-edited { font-size: 11px; font-weight: 700; padding: 1px 8px; border-radius: 9px; background: @accent_bg_color; color: @accent_fg_color; }
.drawing-config-row-edited { box-shadow: inset 3px 0 0 @accent_bg_color; }
.drawing-config-picker { padding: 0; }
/* The Custom tile is the one in use: a button, so it is not greyed, but
   nothing happens on it. */
.drawing-preview-chosen { opacity: 1; }
.drawing-preview-hover { border-radius: 6px; transition: background 120ms ease-out, box-shadow 120ms ease-out; }
.drawing-config-picker:hover .drawing-preview-hover { background: alpha(@accent_bg_color, 0.14); box-shadow: inset 0 0 0 2px @accent_bg_color; }
.drawing-config-picker:active .drawing-preview-hover, .drawing-config-picker:checked .drawing-preview-hover { background: alpha(@accent_bg_color, 0.22); }
.drawing-save-as { font-size: 12px; opacity: 0.7; padding: 2px 6px; min-height: 0; }
.drawing-save-as:hover { opacity: 1; }
.drawing-config-picker > * { padding: 0; }
/* The tag on the picture is the chart's own palette turned inside out: the
   text colour as its ground and the chart colour as its ink, so it reads on
   the picture whatever the picture is. */
.drawing-config-tag { padding: 2px 10px; border-radius: 10px; font-weight: 700; font-size: 12px; background: @window_fg_color; color: @view_bg_color; }
.drawing-preview-current { box-shadow: 0 0 0 2px @accent_bg_color; }
/* The nine places, as a block of nine cells rather than nine buttons: no
   padding between them, one ring around the lot, and the lit one filled. */
.drawing-place-grid { padding: 2px; border-radius: 7px; box-shadow: inset 0 0 0 1px alpha(@window_fg_color, 0.18); }
.drawing-place-cell { padding: 1px; min-width: 0; min-height: 0; border-radius: 4px; opacity: 0.55; }
.drawing-place-cell:hover { opacity: 0.85; }
.drawing-place-cell:checked { opacity: 1; background: alpha(@accent_bg_color, 0.28); }
"#;

/// Run a test that builds widgets on the one thread GTK is ever
/// initialised on, and say whether it ran. The app's own suite has the
/// same helper, with the reasoning at length.
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
