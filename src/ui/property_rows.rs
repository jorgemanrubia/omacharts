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
