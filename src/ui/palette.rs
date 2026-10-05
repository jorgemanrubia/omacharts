//! Picking a colour from the theme's palette.
//!
//! GTK's own colour dialog offers a grid baked into the toolkit: the same
//! nine columns of primaries whatever the desktop looks like. On a chart whose
//! every other colour comes from the Omarchy theme, that grid is the one place
//! the theme stops applying — and a colour picked there stays put when the
//! theme changes, while everything around it moves.
//!
//! So the palette comes first and the wheel second. Picking a swatch stores
//! its *name*, which is re-resolved against whichever theme is active, so an
//! indicator set to Amber stays amber-ish across themes rather than freezing
//! on one theme's amber. The wheel is still there, one click away, for anyone
//! who wants an exact colour — that one is stored as a fixed hex and is
//! deliberately not re-resolved.

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::theme::{ColorChoice, Theme};

use crate::ui::colors;

const SWATCH: i32 = 26;
const COLUMNS: i32 = 4;

/// A button showing `resolved`, which opens the theme's palette.
///
/// `choice` is what is stored now, so the palette can tick the swatch in use.
/// `on_pick` is handed the new choice; the button repaints itself.
pub fn picker(
    theme: &Theme,
    choice: Option<ColorChoice>,
    resolved: &str,
    on_pick: impl Fn(ColorChoice) + Clone + 'static,
) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_valign(gtk::Align::Center);
    button.add_css_class("swatch-button");
    button.set_tooltip_text(Some("Pick a colour from the theme's palette"));

    let shown = swatch_area(resolved, false);
    shown.set_size_request(30, 20);
    button.set_child(Some(&shown));

    let popover = gtk::Popover::new();
    popover.set_child(Some(&palette_box(theme, choice, resolved, &popover, &shown, on_pick)));
    button.set_popover(Some(&popover));
    button
}

/// Repaint a picker's swatch, for when the colour changed somewhere else —
/// Reset, which hands the indicator back to the palette and so cannot know
/// what it will come out as until the set has been asked.
pub fn show(button: &gtk::MenuButton, hex: &str) {
    if let Some(area) = button.child().and_downcast::<gtk::DrawingArea>() {
        paint(&area, hex, false);
    }
}

/// The popover's contents: the theme's swatches, then the way out to the wheel.
#[allow(clippy::too_many_arguments)]
fn palette_box(
    theme: &Theme,
    choice: Option<ColorChoice>,
    resolved: &str,
    popover: &gtk::Popover,
    shown: &gtk::DrawingArea,
    on_pick: impl Fn(ColorChoice) + Clone + 'static,
) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 8);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);

    let current_name = match &choice {
        Some(ColorChoice::Swatch { name }) => Some(name.clone()),
        _ => None,
    };

    let grid = gtk::Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    for (n, swatch) in theme.swatches.iter().enumerate() {
        let picked = current_name.as_deref() == Some(swatch.name.as_str());
        let cell = gtk::Button::new();
        cell.add_css_class("flat");
        cell.set_tooltip_text(Some(&swatch.name));
        cell.set_child(Some(&swatch_area(&swatch.hex, picked)));

        let choose = on_pick.clone();
        let popover_weak = popover.downgrade();
        let shown_weak = shown.downgrade();
        let name = swatch.name.clone();
        let hex = swatch.hex.clone();
        cell.connect_clicked(move |_| {
            repaint(&shown_weak, &hex);
            choose(ColorChoice::swatch(&name));
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
        });
        grid.attach(&cell, n as i32 % COLUMNS, n as i32 / COLUMNS, 1, 1);
    }
    content.append(&grid);

    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let custom = gtk::Button::with_label("Custom…");
    custom.add_css_class("flat");
    custom.set_tooltip_text(Some("Pick an exact colour, which the theme will not change"));
    let choose = on_pick.clone();
    let popover_weak = popover.downgrade();
    let shown_weak = shown.downgrade();
    // The wheel opens on the colour in use, whether that was pinned or came
    // from the palette — starting on transparent black would be a worse guess
    // than the one already on screen.
    let start = colors::parse(&choice.map(|c| c.resolve(theme)).unwrap_or_else(|| resolved.to_string()));
    custom.connect_clicked(move |button| {
        if let Some(popover) = popover_weak.upgrade() {
            popover.popdown();
        }
        let dialog = gtk::ColorDialog::new();
        let window = button.root().and_downcast::<gtk::Window>();
        let choose = choose.clone();
        let shown_weak = shown_weak.clone();
        dialog.choose_rgba(
            window.as_ref(),
            Some(&start),
            None::<&gtk::gio::Cancellable>,
            move |answer| {
                // Cancelling is an answer too, and not one to act on.
                let Ok(rgba) = answer else { return };
                let hex = colors::to_hex(&rgba);
                repaint(&shown_weak, &hex);
                choose(ColorChoice::Fixed { hex });
            },
        );
    });
    content.append(&custom);

    content
}

fn repaint(area: &glib::WeakRef<gtk::DrawingArea>, hex: &str) {
    if let Some(area) = area.upgrade() {
        paint(&area, hex, false);
    }
}

/// One colour, drawn as a rounded tile. `ringed` marks the one in use.
pub(crate) fn swatch_area(hex: &str, ringed: bool) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_size_request(SWATCH, SWATCH);
    paint(&area, hex, ringed);
    area
}

pub(crate) fn paint(area: &gtk::DrawingArea, hex: &str, ringed: bool) {
    let hex = hex.to_string();
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        rounded(cr, 1.0, 1.0, w - 2.0, h - 2.0, 5.0);
        colors::set_source(cr, &hex);
        let _ = cr.fill();

        // The ring is drawn in whatever the foreground is rather than a fixed
        // white: a pale swatch on a light theme needs a dark ring to show at
        // all.
        if ringed {
            let fg = area.color();
            rounded(cr, 1.0, 1.0, w - 2.0, h - 2.0, 5.0);
            cr.set_source_rgba(fg.red() as f64, fg.green() as f64, fg.blue() as f64, 0.9);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
    });
    area.queue_draw();
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}
