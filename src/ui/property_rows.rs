//! The rows every properties dialog shares, drawings' and studies' alike.
//!
//! A concept two dialogs both have — a colour, how much of a fill shows, how
//! thick a line is, a switch — is titled, ranged and built here, once, so a
//! drawing's Thickness and a study's are the same control with the same
//! range, and both say Opacity for the same thing.
//!
//! What is stored, and what the command line takes, is a transparency: how
//! much of the chart shows through, 0 solid. A dialog shows it the other way
//! up, as opacity, 100% solid and 0% invisible, which is how a slider that
//! makes a colour stronger as it moves right reads. The rows here turn one
//! into the other, so a caller hands them and hears back a transparency.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::ui::controls;

pub const COLOUR: &str = "Colour";
pub const THICKNESS: &str = "Thickness";
pub const LINE_STYLE: &str = "Line style";
pub const BACKGROUND: &str = "Background";
/// A row of its own for how solid a fill is.
pub const OPACITY: &str = "Opacity %";
/// The word before the slider on a colour row, and its tooltip.
pub const OPACITY_LABEL: &str = "Opacity";
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

/// How solid a fill is, shown as its opacity: 100% its colour, solid, 0%
/// invisible. Handed and heard as a transparency, `share` of the chart
/// showing through.
pub fn transparency_row(
    share: f64,
    changed: impl Fn(f64) + 'static,
) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    let (row, show) = percent_row(OPACITY, 1.0 - share, 0.0, 1.0, move |opacity| changed(1.0 - opacity));
    (row, Rc::new(move |share| show(1.0 - share)))
}

/// How solid a colour is drawn, shown as its opacity, as a slider and a box
/// not yet on a row: what the standard colour row carries beside its swatch
/// ([`colour_row`]). [`Transparency::alone`] puts it on a row of its own
/// for a transparency with no colour to sit beside.
pub struct Transparency {
    slider: gtk::Scale,
    entry: gtk::SpinButton,
}

impl Transparency {
    /// A transparency of `share`, 0 solid to 1 invisible, shown as its
    /// opacity, 100% to 0%; hearing each one settled on, as a transparency,
    /// and the way to show it another without calling back.
    pub fn new(share: f64, changed: impl Fn(f64) + 'static) -> (Transparency, Rc<dyn Fn(f64)>) {
        let quiet = Rc::new(Cell::new(false));
        let figure = {
            let quiet = quiet.clone();
            controls::bounded((1.0 - share) * 100.0, 0.0, 100.0, 1.0, 0, move |percent| {
                if !quiet.get() {
                    changed(1.0 - percent / 100.0);
                }
            })
        };
        let adjustment = figure.adjustment;
        let show: Rc<dyn Fn(f64)> = Rc::new(move |share| {
            quiet.set(true);
            adjustment.set_value((1.0 - share) * 100.0);
            quiet.set(false);
        });
        (Transparency { slider: figure.slider, entry: figure.entry }, show)
    }

    /// On a row of its own, titled `title`: a transparency of something
    /// with no colour of its own, as a profile's rows outside its value area.
    /// `tip` says what the slider is where `title` is too short to: its
    /// tooltip, and what a screen reader calls it.
    pub fn alone(self, title: &str, tip: &str) -> adw::ActionRow {
        let row = adw::ActionRow::new();
        row.set_title(title);
        self.name(tip);
        self.slider.set_tooltip_text(Some(tip));
        self.entry.set_tooltip_text(Some(tip));
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
/// The colour's name, then the colour itself (`picker`, the swatch that
/// opens the palette), then a small **Opacity** and how solid it is drawn,
/// as a slider and a box: 100% solid, 0% invisible. There is no Reset beside it: a colour with a default to
/// go back to offers **Default** at the top of its palette. A colour the
/// model gives no transparency of its own is passed `None`, and its row is
/// the name and the picker alone.
///
/// The slider and the box are the ones every other slider row has, at the
/// same width and last on the row, so they line up in one column with the
/// sliders above and below them, and the swatch sits just before them.
///
/// A colour and how solid it is drawn are one question asked of one part —
/// a profile's value area, a line, a fill — so they are answered on one
/// row rather than two, and the transparency is not titled apart.
pub fn colour_row(title: &str, picker: &impl IsA<gtk::Widget>, transparency: Option<Transparency>) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    row.add_suffix(picker);
    if let Some(transparency) = transparency {
        transparency.name(&format!("{title} opacity"));
        transparency.slider.set_tooltip_text(Some(OPACITY_LABEL));
        transparency.entry.set_tooltip_text(Some(OPACITY_LABEL));
        // Says what the slider is, small, so the row's name stays the
        // colour's.
        let word = gtk::Label::new(Some(OPACITY_LABEL));
        word.add_css_class("dim-label");
        word.add_css_class("caption");
        word.set_valign(gtk::Align::Center);
        word.set_margin_start(6);
        row.add_suffix(&word);
        row.add_suffix(&transparency.slider);
        row.add_suffix(&transparency.entry);
    }
    row
}

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

    /// The standard colour row: its name, then the swatch, then its
    /// transparency, named for a screen reader by the colour, as wide as
    /// every other slider; and with no transparency, the name and the swatch
    /// alone.
    #[test]
    fn a_colour_row_carries_its_swatch_then_its_transparency() {
        crate::ui::on_gtk(|| {
            let heard = Rc::new(Cell::new(None));
            let (transparency, show) = {
                let heard = heard.clone();
                Transparency::new(0.3, move |share| heard.set(Some(share)))
            };
            let swatch = gtk::MenuButton::new();
            let row = colour_row("Point of control", &swatch, Some(transparency));
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
            assert_eq!(order[0], swatch.clone().upcast::<gtk::Widget>(), "{order:?}");
            assert!(order[1].is::<gtk::Scale>() && order[2].is::<gtk::SpinButton>(), "{order:?}");
            let slider = order[1].clone().downcast::<gtk::Scale>().unwrap();
            let other = crate::ui::controls::bounded_row("Width %", None, 40.0, 10.0, 100.0, 1.0, 0, |_| {});
            let mut widths = Vec::new();
            let mut stack = vec![other.row.upcast::<gtk::Widget>()];
            while let Some(widget) = stack.pop() {
                if let Some(scale) = widget.downcast_ref::<gtk::Scale>() {
                    widths.push(scale.width_request());
                }
                let mut child = widget.first_child();
                while let Some(next) = child {
                    child = next.next_sibling();
                    stack.push(next);
                }
            }
            assert_eq!(widths, [slider.width_request()], "as wide as every other slider");
            assert!((slider.value() - 70.0).abs() < 1e-9, "a transparency of 0.3 shown as 70% opaque");

            show(0.5);
            assert_eq!(heard.get(), None, "shown, not heard");
            slider.set_value(80.0);
            assert!((heard.get().unwrap() - 0.2).abs() < 1e-9, "heard back as a transparency");

            let swatch = gtk::MenuButton::new();
            let plain = colour_row("Close", &swatch, None);
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
