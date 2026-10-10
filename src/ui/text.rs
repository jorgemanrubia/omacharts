//! A drawing's words: laid out, measured, and put on the chart.
//!
//! Everything else the chart writes is one font in one slope on one line,
//! which is what cairo's own text calls do. A drawing's text is not: it has
//! runs of bold and italic inside it, it wraps where the typist pressed
//! Shift+Enter, and its box is what the drawing is selected and hit by. So it
//! goes through Pango, where a weight over a range of characters is an
//! attribute rather than three measured strings stitched together, and where
//! the box comes back measured instead of estimated.
//!
//! One context does both jobs. Measuring happens away from a frame — the
//! pointer asks what it is over between paints — and drawing happens inside
//! one, and if the two used different contexts a word could be hit where it
//! is not drawn. The context here is made once, from the same cairo font map
//! the chart paints through, and both go through it.

use std::cell::RefCell;

use gtk::pango;
use omacharts_engine::drawings::{self, Align, Kind, Place, Style, Text, TextStyle};

thread_local! {
    /// The one context, made on first use. A Pango context is a font map and
    /// a language, not a surface, so it costs nothing to keep and nothing to
    /// use off a frame.
    static CONTEXT: RefCell<Option<pango::Context>> = const { RefCell::new(None) };
}

fn context() -> pango::Context {
    CONTEXT.with(|slot| {
        slot.borrow_mut()
            .get_or_insert_with(|| {
                use gtk::prelude::FontMapExt;
                pangocairo::FontMap::default().create_context()
            })
            .clone()
    })
}

/// The desktop's own font family, which is what a drawing's text is set in
/// until somebody picks another.
///
/// Read from GTK's settings rather than named here, so a chart annotated on
/// this desktop is annotated in this desktop's font — the same one the
/// window's own labels are in. The settings give a family and a size
/// together ("Cantarell 11"); only the family is taken, because the size of
/// a drawing's text is the drawing's business.
pub fn system_family() -> Option<String> {
    let name = gtk::Settings::default()?.gtk_font_name()?;
    let described = pango::FontDescription::from_string(&name);
    described.family().map(|f| f.to_string())
}

/// A laid-out block of text, ready to measure or to draw.
pub fn layout(text: &Text, style: &TextStyle, align: Align) -> pango::Layout {
    let layout = pango::Layout::new(&context());
    layout.set_text(&text.plain_text());
    layout.set_font_description(Some(&font(style)));
    layout.set_attributes(Some(&attributes(text)));
    layout.set_alignment(match align {
        Align::Start => pango::Alignment::Left,
        Align::Center => pango::Alignment::Center,
        Align::End => pango::Alignment::Right,
    });
    // Wrapping is the typist's: a line ends where Shift+Enter was pressed
    // and nowhere else. A drawing has no column to wrap into — the box it
    // labels is a price range, not a text frame — and a label that reflowed
    // as the chart was zoomed would never hold still.
    layout.set_width(-1);
    layout
}

/// How the text is set, as a font description.
fn font(style: &TextStyle) -> pango::FontDescription {
    let mut desc = pango::FontDescription::new();
    if let Some(family) = style.family_name().map(str::to_string).or_else(system_family) {
        desc.set_family(&family);
    }
    // In pixels, not points. Everything else the chart writes is sized in
    // pixels against a plot measured in pixels, and a label that changed
    //  size with the screen's DPI while the candles beside it did not would
    // be the only thing on the chart that did.
    desc.set_absolute_size(style.clamped_size() * pango::SCALE as f64);
    desc
}

/// The weights and slopes inside the text, as attributes over byte ranges.
///
/// Byte ranges because that is what Pango wants and what the spans add up
/// to: each span's characters are appended in order, so a span's range is
/// the bytes already written through the bytes it writes.
fn attributes(text: &Text) -> pango::AttrList {
    let attrs = pango::AttrList::new();
    // Digits that line up, so two notes on two levels read as a column
    // rather than as a ragged pair. Costs nothing on a face without the
    // feature, which simply ignores it.
    let mut figures = pango::AttrFontFeatures::new("tnum 1");
    figures.set_start_index(0);
    figures.set_end_index(u32::MAX);
    attrs.insert(figures);

    let mut at = 0u32;
    for span in &text.spans {
        let len = span.text.len() as u32;
        let (start, end) = (at, at + len);
        at = end;
        if len == 0 {
            continue;
        }
        if span.bold {
            let mut attr = pango::AttrInt::new_weight(pango::Weight::Bold);
            attr.set_start_index(start);
            attr.set_end_index(end);
            attrs.insert(attr);
        }
        if span.italic {
            let mut attr = pango::AttrInt::new_style(pango::Style::Italic);
            attr.set_start_index(start);
            attr.set_end_index(end);
            attrs.insert(attr);
        }
    }
    attrs
}

/// How big the block is, in pixels.
pub fn measure(text: &Text, style: &TextStyle, align: Align) -> (f64, f64) {
    size_of(&layout(text, style, align))
}

fn size_of(layout: &pango::Layout) -> (f64, f64) {
    let (w, h) = layout.pixel_size();
    (w as f64, h as f64)
}

/// Where a drawing's text goes and how big it is, given the drawing's box on
/// screen: the answer the chart needs to draw it and the pointer needs to hit
/// it, worked out the same way for both.
pub fn block(
    kind: Kind,
    bounds: (f64, f64, f64, f64),
    text: &Text,
    style: &Style,
) -> Option<(f64, f64, f64, f64)> {
    if text.is_empty() {
        return None;
    }
    let at = placement(kind, text);
    let size = measure(text, &style.text, at.alignment());
    let (x, y) = drawings::text_origin(kind, bounds, size, at);
    Some((x, y, size.0, size.1))
}

/// Where the text sits. A figure's label sits where its own setting says; a
/// text drawing hangs from its anchor by the top-left corner, because that
/// is where the caret was when it was typed.
pub fn placement(kind: Kind, text: &Text) -> Place {
    match kind.is_text() {
        true => Place::TopLeft,
        false => text.at,
    }
}

/// Put the block on the chart at (`x`, `y`), in `colour`.
pub fn draw(
    cr: &gtk::cairo::Context,
    (x, y): (f64, f64),
    text: &Text,
    style: &TextStyle,
    align: Align,
    colour: &str,
) {
    if text.is_empty() {
        return;
    }
    let layout = layout(text, style, align);
    crate::ui::colors::set_source(cr, colour);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
    // `show_layout` leaves the path where the glyphs were; anything stroking
    // after this would stroke them.
    cr.new_path();
}
