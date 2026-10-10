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

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{glib, pango};
use omacharts_engine::drawings::{Align, Span, Text, TextStyle};

/// The class the editor wears, so the stylesheet can take its background
/// away and leave the chart showing through.
pub const CLASS: &str = "drawing-text-editor";

/// Bold, as a text tag counts weight: the number Pango's own `Bold` is.
const BOLD: i32 = 700;

/// How much room the editor leaves itself past the glyphs.
///
/// A text view puts the caret just past the last character, and with no
/// padding at all a caret at the end of the text sits outside the widget and
/// is clipped away. Two pixels is enough for the caret and little enough
/// that the text does not visibly shift when the editor opens over it.
const CARET_ROOM: i32 = 2;

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
    /// The tag carrying the face, the size and the ink, so a size changed
    /// from the keyboard while the caret is still in the text changes what
    /// is being typed as well as what will be drawn.
    base: gtk::TextTag,
}

impl Editing {
    /// What has been typed, as the runs a drawing stores.
    pub fn text(&self, at: omacharts_engine::Place) -> Text {
        let buffer = self.view.buffer();
        let (start, end) = buffer.bounds();
        let mut spans: Vec<Span> = Vec::new();
        let mut at_iter = start;
        while at_iter < end {
            let mut next = at_iter;
            // A character at a time, run together below. The buffer can say
            // where the next tag change is, but only for one tag at a time,
            // and two tags that change at different places would need both
            // answers reconciled; a label is a few dozen characters.
            if !next.forward_char() {
                break;
            }
            let Some(text) = buffer.text(&at_iter, &next, true).to_string().into() else { break };
            spans.push(Span {
                text,
                bold: at_iter.has_tag(&self.bold),
                italic: at_iter.has_tag(&self.italic),
            });
            at_iter = next;
        }
        Text { spans, at }.tidied()
    }

    /// Whether anything has been typed.
    pub fn is_empty(&self) -> bool {
        self.view.buffer().char_count() == 0
    }

    /// Set it in a style it was not opened with: what a size key pressed
    /// while the caret is still in the text has to do, or the words would
    /// grow on the chart and not under the caret.
    pub fn restyle(&self, style: &TextStyle, ink: &str) {
        if let Some(family) = style.family_name().map(str::to_string).or_else(crate::ui::text::system_family) {
            self.base.set_family(Some(&family));
        }
        self.base.set_size(((style.clamped_size() * pango::SCALE as f64) as i32).max(1));
        self.base.set_foreground(Some(ink));
    }

    /// Move it to where the words now belong, and size it to them.
    pub fn place(&self, layer: &gtk::Fixed, (x, y): (f64, f64), size: (f64, f64)) {
        self.view
            .set_size_request(size.0.ceil() as i32 + CARET_ROOM, size.1.ceil() as i32);
        layer.move_(&self.view, x, y);
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

/// What the editor asks the chart to do when a key ends the edit.
pub enum Ended {
    /// Enter, or the focus went elsewhere: keep what was typed.
    Commit,
    /// Escape: leave the drawing as it was.
    Cancel,
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

    // The face, the size and the ink, over everything: the same three
    // properties the drawing is set in, applied as a tag rather than as CSS
    // so they go through the font description Pango will draw with.
    let base = gtk::TextTag::new(Some("base"));
    if let Some(family) = style.family_name().map(str::to_string).or_else(crate::ui::text::system_family) {
        base.set_family(Some(&family));
    }
    base.set_size(((style.clamped_size() * pango::SCALE as f64) as i32).max(1));
    base.set_foreground(Some(ink));
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

    let view = gtk::TextView::with_buffer(&buffer);
    view.add_css_class(CLASS);
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
        base: base.clone(),
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
    {
        let focus = gtk::EventControllerFocus::new();
        let ended = ended.clone();
        focus.connect_leave(move |_| ended(Ended::Commit));
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
