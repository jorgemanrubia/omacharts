//! Typing on the chart.
//!
//! Click where the words go and type there: no dialog, no field at the side,
//! the caret on the plot at the place the text will live. What makes that
//! work is a `GtkTextView` with nothing drawn behind it, sat on the chart at
//! the pixel the glyphs will occupy, set in the font the drawing is set in
//! and inked in the colour the drawing is inked in. A caret on a canvas is
//! what it looks like; a text widget is what it is, which is why selecting a
//! word with the pointer, the clipboard, an input method and every editing
//! key a keyboard has all work without being written here.
//!
//! The one thing written here is the formatting, because that is the thing a
//! text view does not already have an opinion about: Ctrl+B and Ctrl+I over
//! a selection, as tags in the buffer, read back out as the spans a drawing
//! stores. And Enter, which commits — the chart is not a word processor and
//! a label is usually one line — leaving Shift+Enter as the newline.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{glib, pango};
use omacharts_engine::drawings::{Align, Span, Text, TextStyle};

/// The class the editor wears, so the stylesheet can take its background
/// away and leave the chart showing through.
pub const CLASS: &str = "drawing-text-editor";

/// Bold, as a text tag counts weight: the number Pango's own `Bold` is.
const BOLD: i32 = 700;

thread_local! {
    /// The one stylesheet the editor is dressed by, put on the display the
    /// first time an editor opens and rewritten every time one does.
    ///
    /// One, because there is one editor at a time: a provider per editor
    /// would be a provider per edit left on the display for the life of the
    /// process. It is keyed by the class, so it dresses whichever editor is
    /// currently wearing it and nothing else.
    static DRESS: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

/// Put the drawing's face, size and ink on the editor, in the units the
/// chart draws them in.
fn dress(style: &TextStyle, ink: &str) {
    let family = style
        .family_name()
        .map(str::to_string)
        .or_else(crate::ui::text::system_family)
        .unwrap_or_else(|| "sans-serif".to_string());
    // Quoted, because a family name has spaces in it as often as not and an
    // unquoted one would be read as a list.
    // Not `.{CLASS} text selection`: a selection's own colours belong to the
    // stylesheet, and a rule here putting the drawing's ink on everything
    // inside the view would repaint the selected range in it too.
    let css = format!(
        ".{CLASS}, .{CLASS} text {{ font-family: \"{family}\"; font-size: {size}px; color: {ink};          padding: 0; margin: 0; border: none; }}",
        size = style.clamped_size(),
    );
    DRESS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let provider = slot.get_or_insert_with(|| {
            let provider = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                // Above the application's own sheet, so the editor's size
                // wins over anything a theme has to say about a text view.
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
                );
            }
            provider
        });
        provider.load_from_data(&css);
    });
}

/// How much room the editor keeps past the glyphs, as a multiple of the
/// font size, and never less than this many pixels.
///
/// Sized exactly to its contents, the editor is always one frame too narrow:
/// the width is re-requested on the keystroke and granted on the next layout
/// pass, so what is on screen is the new text in the old frame. A text view
/// used to paper over that by scrolling to keep the caret in view — which is
/// what pushed the first character off the left — and with the scrolling
/// pinned off it clipped the last one instead. Neither is a fix; the frame
/// being a character behind is.
///
/// So the frame is never tight. It carries slack for a character and a
/// caret, which costs nothing — the view has no background — and the slack
/// is taken off the margin again so the glyphs still land exactly where the
/// chart will draw them.
const SLACK: f64 = 1.6;
const MIN_SLACK: f64 = 24.0;

/// The editor in the middle of being typed into.
pub struct Editing {
    /// The widget, already on the chart.
    pub view: gtk::TextView,
    /// Which drawing it is editing, by position in the chart's list.
    ///
    /// A cell, because the list can be rebuilt under an open editor — a
    /// reload, another chart's change arriving — and the drawing being typed
    /// into may come back at a different position. The editor follows it
    /// rather than the caret jumping to whatever moved into its place.
    pub index: Cell<usize>,
    /// Whether the drawing existed before this edit. One that did not — a
    /// click with the text tool — is removed again if nothing is typed,
    /// since an empty text drawing is invisible and unselectable and would
    /// be litter nobody could clear.
    pub fresh: bool,
    bold: gtk::TextTag,
    italic: gtk::TextTag,
}

impl Editing {
    /// What has been typed, as the runs a drawing stores.
    pub fn text(&self, at: omacharts_engine::Place) -> Text {
        let buffer = self.view.buffer();
        let (start, end) = buffer.bounds();
        let typed = buffer.text(&start, &end, true).to_string();
        // A character at a time, by offset, run together below. The buffer
        // can say where the next tag change is, but only for one tag at a
        // time, and two tags that change at different places would need
        // both answers reconciled; a label is a few dozen characters.
        //
        // By offset rather than by walking an iterator, because
        // `forward_char` reports whether what it landed on can be read, and
        // the position after the last character cannot — so walking until
        // it says no stops one character early and ate the last one off
        // every label.
        let spans: Vec<Span> = typed
            .chars()
            .enumerate()
            .map(|(at_char, ch)| {
                let at = buffer.iter_at_offset(at_char as i32);
                Span {
                    text: ch.to_string(),
                    bold: at.has_tag(&self.bold),
                    italic: at.has_tag(&self.italic),
                }
            })
            .collect();
        Text { spans, at }.tidied()
    }

    /// Whether anything has been typed.
    pub fn is_empty(&self) -> bool {
        self.view.buffer().char_count() == 0
    }

    /// Take all of it, the way opening a field to edit a name does.
    ///
    /// A label is a handful of words that is usually being replaced rather
    /// than appended to, so the first keystroke should replace it. Anything
    /// else — a click, an arrow — drops the selection and leaves the caret
    /// where it was aimed, so nothing is lost by offering it.
    pub fn select_all(&self) {
        let buffer = self.view.buffer();
        let (start, end) = buffer.bounds();
        buffer.select_range(&start, &end);
    }

    /// Set it in a style it was not opened with: what a size key pressed
    /// while the caret is still in the text has to do, or the words would
    /// grow on the chart and not under the caret.
    pub fn restyle(&self, style: &TextStyle, ink: &str) {
        dress(style, ink);
    }

    /// Move it to where the words now belong, and size it to them.
    /// Move it to where the words now belong, and size it to them.
    ///
    /// By margin against the top-left of the overlay, which is how a widget
    /// is placed in one. The alternative — a `GtkFixed` layer over the whole
    /// chart — has to be targetable for the editor inside it to take a
    /// click, and a targetable layer the size of the chart takes *every*
    /// click: selecting a word with the pointer worked, and the chart
    /// underneath stopped answering to anything.
    pub fn place(&self, (x, y): (f64, f64), size: (f64, f64), font: f64) {
        let slack = (font * SLACK).max(MIN_SLACK);
        self.view
            .set_size_request((size.0 + slack).ceil() as i32, (size.1 + slack).ceil() as i32);
        // How much of the slack sits to the left of the glyphs, which is
        // where the view's own justification puts it: all of it on the right
        // for a block flush left, half each side for a centred one, all of
        // it on the left for one flush right. Taken off the margin so the
        // text itself does not move.
        let before = match self.view.justification() {
            gtk::Justification::Center => slack / 2.0,
            gtk::Justification::Right => slack,
            _ => 0.0,
        };
        self.view.set_margin_start((x - before).max(0.0).round() as i32);
        self.view.set_margin_top(y.max(0.0).round() as i32);
    }

    /// How big what has been typed is, through the same layout the chart
    /// draws with — so the editor is the size of the result rather than the
    /// size a text widget would like to be.
    pub fn measure(&self, style: &TextStyle, align: Align, at: omacharts_engine::Place) -> (f64, f64) {
        let text = self.text(at);
        match text.is_empty() {
            // An empty editor still needs a caret's worth of height, or it
            // would open as a sliver and the caret would not be visible.
            true => (0.0, crate::ui::text::measure(&Text::plain("X"), style, align).1),
            false => crate::ui::text::measure(&text, style, align),
        }
    }
}

/// What an editor is opened on: which drawing, what it already says, and how
/// it is set.
pub struct Opened<'a> {
    pub index: usize,
    /// Whether the drawing existed before this edit. See [`Editing::fresh`].
    pub fresh: bool,
    pub text: &'a Text,
    pub style: &'a TextStyle,
    pub ink: &'a str,
    pub align: Align,
}

/// What the editor asks the chart to do when the edit ends.
pub enum Ended {
    /// Enter: keep what was typed, and the chart has the keyboard again.
    Commit,
    /// Escape: leave the drawing as it was.
    Cancel,
    /// The keyboard went somewhere else — another chart, another window, a
    /// click on the rail. What was typed is kept, the way clicking away from
    /// a label keeps it everywhere else, but the keyboard is left where it
    /// has gone: it was moved on purpose, and taking it back to the chart
    /// would undo the very keystroke that moved it.
    LostFocus,
}

/// Build an editor for a drawing's text, loaded with what it already says.
///
/// `style` is the drawing's text style and `ink` the colour it is drawn in,
/// so what is typed looks like what will be drawn. `changed` is called after
/// every change to the buffer, for the chart to re-place the editor as the
/// block grows. `ended` is called once, with how it ended.
pub fn build(
    opened: Opened<'_>,
    changed: impl Fn() + 'static,
    ended: impl Fn(Ended) + 'static,
) -> Rc<Editing> {
    let Opened { index, fresh, text, style, ink, align } = opened;
    let buffer = gtk::TextBuffer::new(None);
    let table = buffer.tag_table();

    // The face, the size and the ink go on through CSS rather than through a
    // text tag, and that is not a detail: a tag's `size` is in *points*, and
    // every size in this app is in pixels — the chart sets its labels at 11
    // and 13 device pixels and the engine stores a drawing's text the same
    // way. A 13 handed to a tag is 13 points, which on a 96-dpi screen is
    // 17.3 pixels and on a scaled one more again. The editor came up a third
    // too big, and since the block is placed from what Pango measures at the
    // real size, everything under it was out as well.
    //
    // CSS `px` is the unit the rest of the window is laid out in and the one
    // `set_absolute_size` draws in, so going through the stylesheet makes the
    // editor and the chart agree by construction rather than by a conversion
    // that has to be kept right.
    let base = gtk::TextTag::new(Some("base"));
    // Except the digits, which have no CSS: the chart sets labels with
    // tabular figures, and without the same here a label with a number in it
    // would be a different width under the caret than beside it.
    base.set_font_features(Some(crate::ui::text::FIGURES));
    table.add(&base);

    let bold = gtk::TextTag::new(Some("bold"));
    bold.set_weight(BOLD);
    table.add(&bold);
    let italic = gtk::TextTag::new(Some("italic"));
    italic.set_style(pango::Style::Italic);
    table.add(&italic);

    // What it already says, run by run, with each run's tags on it.
    for span in &text.spans {
        let mut end = buffer.end_iter();
        let offset = end.offset();
        buffer.insert(&mut end, &span.text);
        let from = buffer.iter_at_offset(offset);
        let to = buffer.end_iter();
        if span.bold {
            buffer.apply_tag(&bold, &from, &to);
        }
        if span.italic {
            buffer.apply_tag(&italic, &from, &to);
        }
    }
    // The base tag over the lot, and over everything typed from here on.
    let (start, end) = buffer.bounds();
    buffer.apply_tag(&base, &start, &end);

    dress(style, ink);
    let view = gtk::TextView::with_buffer(&buffer);
    view.add_css_class(CLASS);
    // The editor never scrolls. It is exactly as big as what is in it, and
    // it is re-sized after every keystroke — but the re-size lands on the
    // *next* layout pass, and in between a text view does what a text view
    // does and scrolls to keep the caret in view. Against a frame still the
    // width of the empty text that was there a moment ago, that pushed the
    // first character off the left-hand edge, where it stayed. Pinning both
    // adjustments at nothing says the thing that is actually true: there is
    // nowhere to scroll to.
    for adjustment in [view.hadjustment(), view.vadjustment()].into_iter().flatten() {
        adjustment.connect_value_changed(|adjustment| {
            if adjustment.value() != 0.0 {
                adjustment.set_value(0.0);
            }
        });
    }
    view.set_wrap_mode(gtk::WrapMode::None);
    view.set_accepts_tab(false);
    view.set_left_margin(0);
    view.set_right_margin(0);
    view.set_top_margin(0);
    view.set_bottom_margin(0);
    view.set_pixels_above_lines(0);
    view.set_pixels_below_lines(0);
    view.set_halign(gtk::Align::Start);
    view.set_valign(gtk::Align::Start);
    view.set_justification(match align {
        Align::Start => gtk::Justification::Left,
        Align::Center => gtk::Justification::Center,
        Align::End => gtk::Justification::Right,
    });

    let editing = Rc::new(Editing {
        view: view.clone(),
        index: Cell::new(index),
        fresh,
        bold: bold.clone(),
        italic: italic.clone(),
    });

    // Everything typed carries the face, the size and the ink: a tag applied
    // to a range does not cover what is inserted after it, so the range is
    // re-covered as the text grows.
    {
        let base = base.clone();
        let changed = Rc::new(changed);
        let changed_for_buffer = changed.clone();
        buffer.connect_changed(move |buffer| {
            let (start, end) = buffer.bounds();
            buffer.apply_tag(&base, &start, &end);
            changed_for_buffer();
        });
    }

    let ended = Rc::new(ended);
    // Enter commits, Shift+Enter is a newline, Escape leaves it as it was,
    // and Ctrl+B and Ctrl+I turn the selection bold or italic. Caught before
    // the text view, which has its own ideas about Enter and about Ctrl+B.
    {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let editing = Rc::downgrade(&editing);
        let ended = ended.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            use gtk::gdk::Key;
            let Some(editing) = editing.upgrade() else { return glib::Propagation::Proceed };
            let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let shift = modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            match key {
                Key::Escape => {
                    ended(Ended::Cancel);
                    glib::Propagation::Stop
                }
                // Shift+Enter falls through to the text view, which inserts
                // the newline itself.
                Key::Return | Key::KP_Enter | Key::ISO_Enter if !shift => {
                    ended(Ended::Commit);
                    glib::Propagation::Stop
                }
                Key::b | Key::B if ctrl => {
                    editing.toggle(&editing.bold.clone());
                    glib::Propagation::Stop
                }
                Key::i | Key::I if ctrl => {
                    editing.toggle(&editing.italic.clone());
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        view.add_controller(keys);
    }

    // Clicking away keeps what was typed, which is what every canvas does:
    // the alternative is losing a label to a stray click on the chart.
    //
    // Not here, though — on the next turn of the loop. GTK is part way
    // through handing the keyboard to something else when this runs, and
    // taking the widget it is handing it *from* out of the tree underneath
    // it leaves the focus machinery walking a parent chain whose first link
    // has gone. That is not a crash with a backtrace: it is
    // `gtk_widget_get_parent` failing forever, millions of times a second,
    // until the log fills the disk or the kernel kills the process. Alt and
    // an arrow while typing — move the keyboard to the next chart — was all
    // it took. An idle callback runs after the focus change has finished,
    // when there is nothing left to re-enter.
    {
        let focus = gtk::EventControllerFocus::new();
        let ended = ended.clone();
        focus.connect_leave(move |_| {
            let ended = ended.clone();
            glib::idle_add_local_once(move || ended(Ended::LostFocus));
        });
        view.add_controller(focus);
    }

    editing
}

impl Editing {
    /// Turn a tag on over the selection, or off when all of it already has
    /// it — which is what a formatting key means on a selection everywhere
    /// else. With nothing selected it applies to the word the caret is in,
    /// since a caret in the middle of a word is the usual way of saying
    /// "this word".
    fn toggle(&self, tag: &gtk::TextTag) {
        let buffer = self.view.buffer();
        let (mut from, mut to) = match buffer.selection_bounds() {
            Some(bounds) => bounds,
            None => word_at(&buffer.iter_at_offset(buffer.cursor_position())),
        };
        if from == to {
            return;
        }
        if from > to {
            std::mem::swap(&mut from, &mut to);
        }
        match all_tagged(&from, &to, tag) {
            true => buffer.remove_tag(tag, &from, &to),
            false => buffer.apply_tag(tag, &from, &to),
        }
    }
}

/// Whether every character from `from` to `to` wears `tag`.
fn all_tagged(from: &gtk::TextIter, to: &gtk::TextIter, tag: &gtk::TextTag) -> bool {
    let mut at = *from;
    while at < *to {
        if !at.has_tag(tag) {
            return false;
        }
        if !at.forward_char() {
            break;
        }
    }
    true
}

/// The word the caret is in, as a pair of iterators. An empty pair when the
/// caret is not in a word, which the caller reads as "nothing to do".
fn word_at(at: &gtk::TextIter) -> (gtk::TextIter, gtk::TextIter) {
    let mut from = *at;
    let mut to = *at;
    if !from.starts_word() && !from.backward_word_start() {
        return (*at, *at);
    }
    if !to.ends_word() && !to.forward_word_end() {
        return (*at, *at);
    }
    (from, to)
}
