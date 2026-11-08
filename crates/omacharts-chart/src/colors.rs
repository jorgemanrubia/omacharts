//! Colour conversion between stored `#rrggbb` strings, GDK and cairo.

use gtk::gdk;

/// Parse `#rgb`, `#rrggbb` or `#rrggbbaa`, falling back to transparent so a
/// malformed stored colour draws nothing rather than a black slab.
pub fn parse(hex: &str) -> gdk::RGBA {
    hex.parse::<gdk::RGBA>().unwrap_or(gdk::RGBA::new(0.0, 0.0, 0.0, 0.0))
}

pub fn rgba(hex: &str) -> (f64, f64, f64, f64) {
    let c = parse(hex);
    (c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64)
}

/// Format as `#rrggbb`, dropping alpha.
pub fn to_hex(color: &gdk::RGBA) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.red() * 255.0).round() as u8,
        (color.green() * 255.0).round() as u8,
        (color.blue() * 255.0).round() as u8,
    )
}

pub fn set_source(cr: &gtk::cairo::Context, hex: &str) {
    let (r, g, b, a) = rgba(hex);
    cr.set_source_rgba(r, g, b, a);
}

pub fn set_source_alpha(cr: &gtk::cairo::Context, hex: &str, alpha: f64) {
    let (r, g, b, a) = rgba(hex);
    cr.set_source_rgba(r, g, b, a * alpha);
}

/// Is this colour effectively invisible? Hollow candle bodies are stored as a
/// fully transparent colour, and filling with it is wasted work.
pub fn is_transparent(hex: &str) -> bool {
    rgba(hex).3 < 0.01
}

/// Black or white, whichever is readable on `hex`.
///
/// The engine owns this so the chart, the watchlist and the bar widget cannot
/// disagree about which way to go.
pub fn readable_on(hex: &str) -> &'static str {
    omacharts_engine::theme::readable_on(hex)
}
