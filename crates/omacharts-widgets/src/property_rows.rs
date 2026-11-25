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

use crate::controls;

pub const COLOUR: &str = "Colour";
pub const THICKNESS: &str = "Thickness";
pub const LINE_STYLE: &str = "Line style";
pub const BACKGROUND: &str = "Background";
/// The word before the slider on a colour row, and its tooltip.
pub const OPACITY_LABEL: &str = "Opacity";
pub const SHOW: &str = "Show";
pub const SHADED: &str = "Shaded";
/// How solid a band's shading is, on a row of its own, titled as a
/// profile's outside rows are: the shading is in the band's colour, which
/// is on the band's colour row with its edges' opacity.
pub const SHADING: &str = "Shading opacity %";
pub const SHADING_TIP: &str = "Opacity of the shading between the band's edges";

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

    /// On a row of its own, titled `title`, with the colour row's bare
    /// figure: the one strength of colours whose own rows are hidden for
    /// now, as a study's Opacity under a preset, so that the slider and the
    /// box are where they are on those rows and the thumb stays put when
    /// they swap.
    pub fn bare(self, title: &str) -> adw::ActionRow {
        let (slider, entry) = (self.slider.clone(), self.entry.clone());
        let row = self.alone(title, title);
        bare_figure(&slider, &entry);
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
        bare_figure(&transparency.slider, &transparency.entry);
        whole_words(&row, title);
    }
    row
}

/// A name of one word is never broken inside it: beside the slider, the
/// title's column is narrow enough that "Background" came out hyphenated.
/// A name of several words still wraps between them.
fn whole_words(row: &adw::ActionRow, title: &str) {
    if title.contains(' ') {
        return;
    }
    let mut stack = vec![row.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = stack.pop() {
        if let Some(label) = widget.downcast_ref::<gtk::Label>()
            && label.has_css_class("title")
        {
            label.set_wrap(false);
            return;
        }
        let mut next = widget.first_child();
        while let Some(child) = next {
            next = child.next_sibling();
            stack.push(child);
        }
    }
}

/// The colour row's figure without its − and +: the row holds a swatch and
/// a word already, and the slider and the box say all a stepper would. The
/// box still takes a typed figure, the arrow keys and, once clicked into,
/// the wheel.
///
/// The slider takes the steppers' room, so both ends of the column line up
/// with every other slider row's: the slider starts where theirs do, and
/// the box ends where their + does. Measured once styled, as the steppers'
/// width is the theme's.
fn bare_figure(slider: &gtk::Scale, entry: &gtk::SpinButton) {
    let steppers = steppers(entry);
    for stepper in &steppers {
        stepper.set_visible(false);
    }
    let slider = slider.clone();
    let base = slider.width_request();
    // A hidden widget measures nothing, so each is shown for its measure.
    entry.connect_realize(move |_| {
        let room: i32 = steppers
            .iter()
            .map(|stepper| {
                stepper.set_visible(true);
                let width = stepper.measure(gtk::Orientation::Horizontal, -1).1;
                stepper.set_visible(false);
                width
            })
            .sum();
        slider.set_width_request(base + room);
    });
}

/// A spin button's − and +.
fn steppers(entry: &gtk::SpinButton) -> Vec<gtk::Widget> {
    let mut out = Vec::new();
    let mut child = entry.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        if widget.is::<gtk::Button>() {
            out.push(widget);
        }
    }
    out
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
#[cfg(any(test, feature = "testing"))]
pub fn titles(group: &adw::PreferencesGroup) -> Vec<String> {
    let mut out = Vec::new();
    let mut child = group.first_child();
    while let Some(widget) = child {
        collect_titles(&widget, &mut out);
        child = widget.next_sibling();
    }
    out
}

#[cfg(any(test, feature = "testing"))]
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
    /// On screen, a colour row's slider starts where every other slider
    /// row's does and its box ends where their + does: one column, though
    /// the colour row has no − or +.
    #[test]
    fn a_colour_rows_figure_lines_up_with_the_other_sliders() {
        crate::on_gtk(|| {
            let (transparency, _) = Transparency::new(0.3, |_| {});
            let swatch = gtk::MenuButton::new();
            let colour = colour_row("Colour", &swatch, Some(transparency));
            let other = controls::bounded_row("Width %", None, 40.0, 10.0, 100.0, 1.0, 0, |_| {});
            let list = gtk::ListBox::new();
            list.append(&colour);
            list.append(&other.row);
            let window = gtk::Window::new();
            window.set_default_size(560, -1);
            window.set_child(Some(&list));
            window.present();
            let context = gtk::glib::MainContext::default();
            for _ in 0..50 {
                while context.iteration(false) {}
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let find = |row: &gtk::Widget, pick: &dyn Fn(&gtk::Widget) -> bool| {
                let mut stack = vec![row.clone()];
                let mut found = Vec::new();
                while let Some(widget) = stack.pop() {
                    if pick(&widget) && widget.is_visible() {
                        found.push(widget.clone());
                    }
                    let mut child = widget.first_child();
                    while let Some(next) = child {
                        child = next.next_sibling();
                        stack.push(next);
                    }
                }
                found
            };
            let colour = colour.upcast::<gtk::Widget>();
            let other = other.row.upcast::<gtk::Widget>();
            let bounds = |widget: &gtk::Widget| widget.compute_bounds(&list).expect("placed");
            let slider = |row: &gtk::Widget| bounds(&find(row, &|w| w.is::<gtk::Scale>())[0]);
            let spin = |row: &gtk::Widget| bounds(&find(row, &|w| w.is::<gtk::SpinButton>())[0]);
            let (a, b) = (slider(&colour), slider(&other));
            assert!(
                (a.x() - b.x()).abs() <= 1.0,
                "the sliders start together: {} {}",
                a.x(),
                b.x()
            );
            let (a, b) = (spin(&colour), spin(&other));
            let ends = |r: &gtk::graphene::Rect| r.x() + r.width();
            assert!(
                (ends(&a) - ends(&b)).abs() <= 1.0,
                "the figures end together: {} {}",
                ends(&a),
                ends(&b)
            );
            window.destroy();
        });
    }

    #[test]
    fn a_colour_row_carries_its_swatch_then_its_transparency() {
        crate::on_gtk(|| {
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
            let other = crate::controls::bounded_row("Width %", None, 40.0, 10.0, 100.0, 1.0, 0, |_| {});
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
            assert_eq!(
                widths,
                [slider.width_request()],
                "as wide as every other slider, until styled"
            );
            let entry = order[2].clone().downcast::<gtk::SpinButton>().unwrap();
            assert!(
                steppers(&entry).len() == 2 && steppers(&entry).iter().all(|s| !s.is_visible()),
                "no − or +"
            );
            assert!(
                (slider.value() - 70.0).abs() < 1e-9,
                "a transparency of 0.3 shown as 70% opaque"
            );

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
