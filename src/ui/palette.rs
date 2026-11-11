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
use omacharts_engine::theme::{ColorChoice, Theme, FOREGROUND};

use crate::ui::colors;

const SWATCH: i32 = 26;

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
    let shown = swatch_area(resolved, false);
    shown.set_size_request(30, 20);
    let button = menu_button(theme, choice, resolved, &shown, shown.clone(), on_pick);
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
    });
    button.add_css_class("colour-chip");
    slot(&button, &[FOREGROUND], move || name.text().to_string())
}

/// Hold `control` in a slot as wide as it is with the widest of `words` in
/// place of `shown()`, the word it shows now. The control's box hugs what
/// it shows, at the slot's start, and the room left over sits after it,
/// outside the box: what stands beside the slot does not move as the word
/// changes, and the highlight is only as wide as what it highlights.
pub(crate) fn slot(control: &impl IsA<gtk::Widget>, words: &[&str], shown: impl Fn() -> String + 'static) -> adw::Bin {
    let layout: SlotLayout = glib::Object::new();
    layout.imp().words.replace(words.iter().map(|word| word.to_string()).collect());
    layout.imp().shown.replace(Some(Box::new(shown)));
    control.set_halign(gtk::Align::Start);
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

        /// The child's own start alignment holds it at the slot's start, at
        /// its natural width.
        fn allocate(&self, slot: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            if let Some(child) = slot.first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

/// What a chip calls a colour: the swatch's name, or Custom for one picked
/// on the wheel, whose hex nobody reads.
fn chip_name(choice: &ColorChoice) -> String {
    match choice {
        ColorChoice::Swatch { name } => name.clone(),
        ColorChoice::Fixed { .. } => "Custom".to_string(),
    }
}

/// A menu button holding `child`, whose popover is the palette; a pick
/// repaints `shown`.
fn menu_button(
    theme: &Theme,
    choice: Option<ColorChoice>,
    resolved: &str,
    shown: &gtk::DrawingArea,
    child: impl IsA<gtk::Widget>,
    on_pick: impl Fn(ColorChoice) + Clone + 'static,
) -> gtk::MenuButton {
    let button = gtk::MenuButton::new();
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Pick a colour from the theme's palette"));
    button.set_child(Some(&child));
    let popover = gtk::Popover::new();
    popover.set_child(Some(&palette_box(theme, choice, resolved, &popover, shown, on_pick)));
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
