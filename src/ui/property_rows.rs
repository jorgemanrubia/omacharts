//! The rows every properties dialog shares, drawings' and studies' alike.
//!
//! A concept two dialogs both have — a colour, how much of a fill shows, how
//! thick a line is, a switch — is titled, ranged and built here, once, so a
//! drawing's Thickness and a study's are the same control with the same
//! range, and both say Transparency for the same thing.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::ui::controls;

pub const COLOUR: &str = "Colour";
pub const THICKNESS: &str = "Thickness";
pub const LINE_STYLE: &str = "Line style";
pub const BACKGROUND: &str = "Background";
pub const TRANSPARENCY: &str = "Transparency %";
pub const SHOW: &str = "Show";
pub const SHADED: &str = "Shaded";

/// A share as a whole percent, between `least` and `most`, on the slider and
/// box every bounded figure has, and the way to show it another.
pub fn percent_row(
    title: &str,
    share: f64,
    least: f64,
    most: f64,
    changed: impl Fn(f64) + 'static,
) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    let quiet = Rc::new(Cell::new(false));
    let bounded = {
        let quiet = quiet.clone();
        controls::bounded_row(
            title,
            None,
            share * 100.0,
            least * 100.0,
            most * 100.0,
            1.0,
            0,
            move |percent| {
                if !quiet.get() {
                    changed(percent / 100.0);
                }
            },
        )
    };
    let adjustment = bounded.adjustment.clone();
    let show: Rc<dyn Fn(f64)> = Rc::new(move |share| {
        quiet.set(true);
        adjustment.set_value(share * 100.0);
        quiet.set(false);
    });
    (bounded.row, show)
}

/// How much of the chart shows through a fill: none at 0%, where it is its
/// colour, solid, and all of it at 100%.
pub fn transparency_row(
    share: f64,
    changed: impl Fn(f64) + 'static,
) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    percent_row(TRANSPARENCY, share, 0.0, 1.0, changed)
}

/// How much of the chart shows through a colour, as a slider and a box
/// not yet on a row: what the standard colour row carries beside its swatch
/// ([`colour_row`]). [`Transparency::alone`] puts it on a row of its own
/// for a transparency with no colour to sit beside.
pub struct Transparency {
    slider: gtk::Scale,
    entry: gtk::SpinButton,
}

impl Transparency {
    /// A transparency of `share`, 0 solid to 1 invisible, hearing each one
    /// settled on, and the way to show it another without calling back.
    pub fn new(share: f64, changed: impl Fn(f64) + 'static) -> (Transparency, Rc<dyn Fn(f64)>) {
        let quiet = Rc::new(Cell::new(false));
        let figure = {
            let quiet = quiet.clone();
            controls::bounded(share * 100.0, 0.0, 100.0, 1.0, 0, move |percent| {
                if !quiet.get() {
                    changed(percent / 100.0);
                }
            })
        };
        let adjustment = figure.adjustment;
        let show: Rc<dyn Fn(f64)> = Rc::new(move |share| {
            quiet.set(true);
            adjustment.set_value(share * 100.0);
            quiet.set(false);
        });
        (Transparency { slider: figure.slider, entry: figure.entry }, show)
    }

    /// On a row of its own, titled `title`: a transparency of something
    /// with no colour of its own, as a profile's rows outside its value area.
    pub fn alone(self, title: &str) -> adw::ActionRow {
        let row = adw::ActionRow::new();
        row.set_title(title);
        self.name(title);
        row.add_suffix(&self.slider);
        row.add_suffix(&self.entry);
        row
    }

    /// What a screen reader calls the slider and the box, which on a colour
    /// row have no title of their own.
    fn name(&self, name: &str) {
        for widget in [self.slider.upcast_ref::<gtk::Widget>(), self.entry.upcast_ref()] {
            widget.update_property(&[gtk::accessible::Property::Label(name)]);
        }
    }
}

/// The app's standard colour row: what a colour is picked on, in every
/// properties dialog, a study's and a drawing's alike.
///
/// The colour's name, then how much of the chart shows through it, as a
/// slider and a box, then the colour itself. The caller adds the picker,
/// and the Reset before it where the colour has a default to go back to,
/// with `add_suffix`, so they land after the transparency. A colour the
/// model gives no transparency of its own is passed `None`, and its row is
/// the name and the picker alone, at the same place.
///
/// A colour and how solid it is drawn are one question asked of one part —
/// a profile's value area, a line, a fill — so they are answered on one
/// row rather than two, and the transparency is not titled apart.
pub fn colour_row(title: &str, transparency: Option<Transparency>) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    if let Some(transparency) = transparency {
        transparency.name(&format!("{title} transparency"));
        // The figure is typed or dragged, never stepped: the box drops its
        // − and + so the slider keeps its width beside the swatch and Reset
        // at the dialog's width.
        transparency.slider.set_size_request(COLOUR_ROW_SLIDER, -1);
        let mut part = transparency.entry.first_child();
        while let Some(widget) = part {
            part = widget.next_sibling();
            if widget.is::<gtk::Button>() {
                widget.set_visible(false);
            }
        }
        transparency.slider.set_tooltip_text(Some(TRANSPARENCY));
        transparency.entry.set_tooltip_text(Some(TRANSPARENCY));
        row.add_suffix(&transparency.slider);
        row.add_suffix(&transparency.entry);
    }
    row
}

/// The width of the transparency slider on a colour row.
pub const COLOUR_ROW_SLIDER: i32 = 180;

/// A line's thickness in pixels, 0.5 to 12 by halves, and the way to show
/// it another.
pub fn thickness_row(
    title: &str,
    width: f64,
    changed: impl Fn(f64) + 'static,
) -> (adw::SpinRow, Rc<dyn Fn(f64)>) {
    use omacharts_engine::indicators::{MAX_OUTLINE_WIDTH, MIN_OUTLINE_WIDTH, OUTLINE_WIDTH_STEP};
    let quiet = Rc::new(Cell::new(false));
    let row = adw::SpinRow::with_range(MIN_OUTLINE_WIDTH, MAX_OUTLINE_WIDTH, OUTLINE_WIDTH_STEP);
    row.set_title(title);
    row.set_digits(1);
    row.set_value(width);
    {
        let quiet = quiet.clone();
        row.connect_value_notify(move |row| {
            if !quiet.get() {
                changed(row.value());
            }
        });
    }
    let shown = row.clone();
    let show: Rc<dyn Fn(f64)> = Rc::new(move |width| {
        quiet.set(true);
        shown.set_value(width);
        quiet.set(false);
    });
    (row, show)
}

/// A switch, and the way to show it on or off without calling back.
pub fn switch_row(
    title: &str,
    on: bool,
    changed: impl Fn(bool) + 'static,
) -> (adw::SwitchRow, Rc<dyn Fn(bool)>) {
    let quiet = Rc::new(Cell::new(false));
    let row = adw::SwitchRow::new();
    row.set_title(title);
    row.set_active(on);
    {
        let quiet = quiet.clone();
        row.connect_active_notify(move |row| {
            if !quiet.get() {
                changed(row.is_active());
            }
        });
    }
    let shown = row.clone();
    let show: Rc<dyn Fn(bool)> = Rc::new(move |on| {
        quiet.set(true);
        shown.set_active(on);
        quiet.set(false);
    });
    (row, show)
}

/// The rows of each group, by title, in the order they are laid out: what a
/// test holds both dialogs to.
#[cfg(test)]
pub fn titles(group: &adw::PreferencesGroup) -> Vec<String> {
    let mut out = Vec::new();
    let mut child = group.first_child();
    while let Some(widget) = child {
        collect_titles(&widget, &mut out);
        child = widget.next_sibling();
    }
    out
}

#[cfg(test)]
fn collect_titles(widget: &gtk::Widget, out: &mut Vec<String>) {
    if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>() {
        out.push(row.title().to_string());
        return;
    }
    let mut child = widget.first_child();
    while let Some(next) = child {
        collect_titles(&next, out);
        child = next.next_sibling();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The standard colour row: its name, then its transparency, named for
    /// a screen reader by the colour, then what the caller adds; and with no
    /// transparency, the name alone before it.
    #[test]
    fn a_colour_row_carries_its_transparency_before_the_swatch() {
        crate::ui::on_gtk(|| {
            let heard = Rc::new(Cell::new(None));
            let (transparency, show) = {
                let heard = heard.clone();
                Transparency::new(0.3, move |share| heard.set(Some(share)))
            };
            let row = colour_row("Point of control", Some(transparency));
            let swatch = gtk::MenuButton::new();
            row.add_suffix(&swatch);
            assert_eq!(row.title(), "Point of control");

            let mut order: Vec<gtk::Widget> = Vec::new();
            let mut stack = vec![row.clone().upcast::<gtk::Widget>()];
            while let Some(widget) = stack.pop() {
                if widget.is::<gtk::Scale>() || widget.is::<gtk::SpinButton>() || widget.is::<gtk::MenuButton>() {
                    order.push(widget.clone());
                }
                let mut children = Vec::new();
                let mut child = widget.first_child();
                while let Some(next) = child {
                    child = next.next_sibling();
                    children.push(next);
                }
                stack.extend(children.into_iter().rev());
            }
            assert!(order[0].is::<gtk::Scale>() && order[1].is::<gtk::SpinButton>(), "{order:?}");
            assert_eq!(order[2], swatch.clone().upcast::<gtk::Widget>());
            let slider = order[0].clone().downcast::<gtk::Scale>().unwrap();
            assert_eq!(slider.width_request(), COLOUR_ROW_SLIDER);
            assert!((slider.value() - 30.0).abs() < 1e-9);
            let entry = order[1].clone();
            let mut part = entry.first_child();
            while let Some(widget) = part {
                assert!(!(widget.is::<gtk::Button>() && widget.is_visible()), "no − and + on its figure");
                part = widget.next_sibling();
            }

            show(0.5);
            assert_eq!(heard.get(), None, "shown, not heard");
            slider.set_value(80.0);
            assert_eq!(heard.get(), Some(0.8));

            let plain = colour_row("Last TPO colour", None);
            let swatch = gtk::MenuButton::new();
            plain.add_suffix(&swatch);
            let mut stack = vec![plain.upcast::<gtk::Widget>()];
            let mut sliders = 0;
            while let Some(widget) = stack.pop() {
                sliders += usize::from(widget.is::<gtk::Scale>());
                let mut child = widget.first_child();
                while let Some(next) = child {
                    child = next.next_sibling();
                    stack.push(next);
                }
            }
            assert_eq!(sliders, 0, "a colour with no transparency has no slider");
        });
    }
}
