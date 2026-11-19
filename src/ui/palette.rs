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
use gtk::subclass::prelude::ObjectSubclassIsExt;
use std::rc::Rc;
use omacharts_engine::theme::{ColorChoice, Theme};

use crate::ui::colors;

const SWATCH: i32 = 26;

/// The swatch inside a colour button, a study's and a drawing's alike.
pub const BUTTON_SWATCH: (i32, i32) = (30, 20);

/// A button showing `resolved`, which opens the theme's palette.
///
/// `choice` is what is stored now, so the palette can tick the swatch in use.
/// `on_pick` is handed the new choice; the button repaints itself.
/// `default`, where the colour has one to go back to, is offered first as
/// **Default**: it clears the choice and says what colour that came to, which
/// the button then shows.
pub fn picker(
    theme: &Theme,
    choice: Option<ColorChoice>,
    resolved: &str,
    on_pick: impl Fn(ColorChoice) + Clone + 'static,
    default: Option<Rc<dyn Fn() -> String>>,
) -> gtk::MenuButton {
    let shown = swatch_area(resolved, false);
    shown.set_size_request(BUTTON_SWATCH.0, BUTTON_SWATCH.1);
    let button = menu_button(theme, choice, resolved, &shown, shown.clone(), on_pick, default);
    button.add_css_class("swatch-button");
    button
}

/// [`picker`] with the colour's name beside its swatch, for a choice that
/// is read as much as seen: "Green" still says what it is after a theme
/// swap has made it a different green. In a [`slot`] as wide as Foreground,
/// the longest name a bar is offered.
pub fn chip(theme: &Theme, choice: ColorChoice, on_pick: impl Fn(ColorChoice) + Clone + 'static) -> adw::Bin {
    let resolved = choice.resolve(theme);
    // At 12px the swatch's rounded corners meet: a dot.
    let shown = swatch_area(&resolved, false);
    shown.set_size_request(12, 12);
    shown.set_valign(gtk::Align::Center);
    let name = gtk::Label::new(Some(&chip_name(&choice)));
    let inside = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    inside.append(&shown);
    inside.append(&name);
    inside.append(&gtk::Image::from_icon_name("pan-down-symbolic"));
    let named = name.clone();
    let button = menu_button(theme, Some(choice), &resolved, &shown, inside, move |choice: ColorChoice| {
        named.set_text(&chip_name(&choice));
        on_pick(choice);
    }, None);
    button.add_css_class("colour-chip");
    let names: Vec<String> = theme.palette().into_iter().map(|swatch| swatch.name).chain([CUSTOM.to_string()]).collect();
    slot(&button, &names.iter().map(String::as_str).collect::<Vec<_>>(), move || name.text().to_string())
}

/// Hold `control` in a slot as wide as it is with the widest of `words` in
/// place of `shown()`, the word it shows now. The control's box hugs what
/// it shows, at the slot's end, and the room left over sits before it,
/// outside the box: the control's right edge, and what stands beside the
/// slot, do not move as the word changes, and the highlight is only as wide
/// as what it highlights.
pub(crate) fn slot(control: &impl IsA<gtk::Widget>, words: &[&str], shown: impl Fn() -> String + 'static) -> adw::Bin {
    let layout: SlotLayout = glib::Object::new();
    layout.imp().words.replace(words.iter().map(|word| word.to_string()).collect());
    layout.imp().shown.replace(Some(Box::new(shown)));
    control.set_halign(gtk::Align::End);
    let slot = adw::Bin::new();
    slot.set_layout_manager(Some(layout));
    slot.set_child(Some(control));
    slot.set_valign(gtk::Align::Center);
    slot
}

glib::wrapper! {
    /// How a [`slot`] is sized: its one child, and the room the widest
    /// word would need beyond the word shown.
    pub struct SlotLayout(ObjectSubclass<slot_layout::SlotLayout>) @extends gtk::LayoutManager;
}

mod slot_layout {
    use std::cell::RefCell;

    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    #[derive(Default)]
    pub struct SlotLayout {
        pub words: RefCell<Vec<String>>,
        pub shown: RefCell<Option<Box<dyn Fn() -> String>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SlotLayout {
        const NAME: &'static str = "OmachartsSlotLayout";
        type Type = super::SlotLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for SlotLayout {}

    impl LayoutManagerImpl for SlotLayout {
        /// The words are measured in the slot's font each time it is sized,
        /// so a word changed, or a font that arrived after the slot was
        /// built, is measured as it is.
        fn measure(&self, slot: &gtk::Widget, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let Some(child) = slot.first_child() else { return (0, 0, -1, -1) };
            let measured = child.measure(orientation, for_size);
            if orientation == gtk::Orientation::Vertical {
                return measured;
            }
            let width = |word: &str| slot.create_pango_layout(Some(word)).pixel_size().0;
            let widest = self.words.borrow().iter().map(|word| width(word)).max().unwrap_or(0);
            let shown = self.shown.borrow().as_ref().map(|shown| shown()).unwrap_or_default();
            let room = (widest - width(&shown)).max(0);
            (measured.0 + room, measured.1 + room, -1, -1)
        }

        /// The child's own end alignment holds it at the slot's end, at its
        /// natural width.
        fn allocate(&self, slot: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            if let Some(child) = slot.first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

/// What a chip calls a colour picked on the wheel, whose hex nobody reads.
const CUSTOM: &str = "Custom";

/// What a chip calls a colour: the swatch's name, or [`CUSTOM`].
fn chip_name(choice: &ColorChoice) -> String {
    match choice {
        ColorChoice::Swatch { name } => name.clone(),
        ColorChoice::Fixed { .. } => CUSTOM.to_string(),
    }
}

/// A menu button holding `child`, whose popover is the palette, with
/// `default` first as [`picker`] says; a pick repaints `shown`.
fn menu_button(
    theme: &Theme,
    choice: Option<ColorChoice>,
    resolved: &str,
    shown: &gtk::DrawingArea,
    child: impl IsA<gtk::Widget>,
    on_pick: impl Fn(ColorChoice) + Clone + 'static,
    default: Option<Rc<dyn Fn() -> String>>,
) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Pick a colour from the theme's palette"));
    button.set_child(Some(&child));
    let popover = gtk::Popover::new();
    let content = palette_box(theme, choice, resolved, &popover, shown, on_pick);
    if let Some(default) = default {
        let shown = shown.downgrade();
        content.prepend(&default_button(&popover, move || {
            let hex = default();
            repaint(&shown, &hex);
        }));
    }
    popover.set_child(Some(&content));
    button.set_popover(Some(&popover));
    button
}

/// Repaint a picker's swatch, for when the colour changed somewhere else.
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

    // Full rows, or a last row one short: four across for eight, three for
    // the nine of the palette, seven for the thirteen a bar is offered. A
    // swatch alone on a row reads as a leftover.
    let palette = theme.palette();
    let count = palette.len() as i32;
    let columns = [4, 3, 7].into_iter().find(|c| count % c == 0 || count % c == c - 1).unwrap_or(4);
    let grid = gtk::Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(4);
    for (n, swatch) in palette.iter().enumerate() {
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
        grid.attach(&cell, n as i32 % columns, n as i32 / columns, 1, 1);
    }
    content.append(&grid);

    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let custom = gtk::Button::with_label("Custom…");
    custom.add_css_class("flat");
    // An action word wherever a row's actions are, as it always was there.
    custom.add_css_class("action-word");
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

/// The way back to a colour's default, first in its palette: where the
/// colour rows used to carry a Reset beside the swatch. `clear` goes back to
/// it and shows what it came to; the palette closes.
pub(crate) fn default_button(popover: &gtk::Popover, clear: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::with_label(DEFAULT);
    button.add_css_class("flat");
    button.set_tooltip_text(Some("Back to the colour it has unless one is picked"));
    let popover = popover.downgrade();
    button.connect_clicked(move |_| {
        clear();
        if let Some(popover) = popover.upgrade() {
            popover.popdown();
        }
    });
    button
}

/// What the palette's first entry says.
pub const DEFAULT: &str = "Default";

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Where a colour has a default to go back to, its palette offers
    /// Default first: it clears the choice and the swatch shows what that
    /// came to. Without one there is no such entry.
    #[test]
    fn a_palette_offers_the_way_back_to_the_default_first() {
        crate::ui::on_gtk(|| {
            let theme = omacharts_engine::theme::builtin_themes().remove(0);
            let cleared = Rc::new(RefCell::new(0));
            let default: Rc<dyn Fn() -> String> = {
                let cleared = cleared.clone();
                Rc::new(move || {
                    *cleared.borrow_mut() += 1;
                    "#123456".to_string()
                })
            };
            let button = picker(&theme, Some(ColorChoice::swatch("Amber")), "#d29922", |_| {}, Some(default));
            let content = button.popover().and_then(|p| p.child()).expect("its palette");
            let first = content.first_child().and_downcast::<gtk::Button>().expect("a button first");
            assert_eq!(first.label().as_deref(), Some(DEFAULT));
            first.emit_clicked();
            assert_eq!(*cleared.borrow(), 1);

            let plain = picker(&theme, None, "#d29922", |_| {}, None);
            let content = plain.popover().and_then(|p| p.child()).expect("its palette");
            assert!(content.first_child().and_downcast::<gtk::Button>().is_none(), "no Default without one");
        });
    }
}
