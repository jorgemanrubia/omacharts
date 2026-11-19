//! The rows of a profile's properties: a volume profile study's, a volume
//! profile drawing's and a market profile's alike.
//!
//! One set of names, controls, ranges, groups and one order, laid out here,
//! so the three dialogs cannot drift apart: a row two of them mean the same
//! thing by is built by the same function and put in the same place. What
//! each builds on its own is only what its own storage makes different — a
//! colour, which is a preset on a drawing and a theme colour on a study —
//! and what only one of them has: a study's period, and whether only the
//! newest is drawn, which a span between two anchors has not; a volume
//! profile's method and outline; a market profile's TPO period and its
//! open and close marks. Those sit where the others' would.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::indicators::profile::{Method, DELTA_COLOURINGS};
use omacharts_engine::BarColouring;

pub use crate::ui::property_rows::{
    COLOUR, LINE_STYLE, SHOW, THICKNESS, OPACITY, Transparency, colour_row, percent_row, switch_row,
    thickness_row,
};
#[cfg(test)]
pub use crate::ui::property_rows::titles;

/// The groups, in their order.
pub const STYLE_GROUP: &str = "Style";
pub const VALUE_AREA_GROUP: &str = "Value area";
pub const POC_GROUP: &str = "Point of control";

/// The Style group's own rows. Colour and Transparency are the shared ones.
/// `DISPLAY_AS`, not `DISPLAY`: a setting key already goes by that name, and
/// the check that every key written is classified resolves keys by name.
pub const DISPLAY_AS: &str = "Display";
pub const OUTLINE: &str = "Outline";
pub const OUTLINE_COLOUR: &str = "Outline colour";
pub const OUTLINE_STYLE: &str = "Outline style";
pub const OUTLINE_THICKNESS: &str = "Outline thickness";
pub const OPEN_CLOSE: &str = "Open and close";
pub const OPEN_COLOUR: &str = "Open";
pub const CLOSE_COLOUR: &str = "Close";
pub const ALIGNMENT: &str = "Alignment";
/// The Value area group's switch; its Colour and Transparency are the
/// shared ones.
pub const HIGHLIGHT: &str = "Highlight";
pub const OUTSIDE_TRANSPARENCY: &str = "Outside rows opacity %";
/// What the Outside rows slider is, for its tooltip and a screen reader.
pub const OUTSIDE_TIP: &str = "Opacity of the rows outside the value area";
/// The Parameters group's.
pub const LAST_ONLY: &str = "Last period only";
/// A volume profile's switch for its row count, and a market profile's for
/// its row height: one name on both, the row under it saying which number
/// it makes automatic.
pub const AUTOMATIC: &str = "Automatic rows";
pub const ROWS: &str = "Rows";
pub const VALUE_AREA: &str = "Value area %";
pub const METHOD: &str = "Calculation method";
pub const COLOURING: &str = "Colouring";
pub const WIDTH: &str = "Width %";
/// What the Colouring row offers first: the colouring chosen on the
/// Defaults page of Preferences, whatever that is.
pub const FOLLOWS_DEFAULTS: &str = "Default";

/// The fewest and most rows the Rows box offers: the engine's own floor and
/// ceiling, past which a count is clamped anyway.
const FEWEST_ROWS: f64 = 4.0;
const MOST_ROWS: f64 = 800.0;

/// What the Rows box shows when the count is automatic and nothing has been
/// drawn yet to say how many it came to.
const ROWS_UNKNOWN: usize = 48;

/// An outline's switch and the rows it shows.
pub struct OutlineRows {
    pub switch: adw::SwitchRow,
    /// Its colour and transparency, on the standard colour row.
    pub colour: gtk::Widget,
    pub thickness: gtk::Widget,
    pub style: gtk::Widget,
}

/// The open and close marks' switch and their colours: a market profile's.
pub struct EndRows {
    pub switch: adw::SwitchRow,
    pub open: gtk::Widget,
    pub close: gtk::Widget,
}

/// The rows of the Style group, built by each side, laid out here.
pub struct StyleRows {
    /// Filled or outline; letters or blocks.
    pub display: gtk::Widget,
    /// The rows' colour and transparency, on the standard colour row.
    pub colour: gtk::Widget,
    pub outline: Option<OutlineRows>,
    pub ends: Option<EndRows>,
    pub alignment: gtk::Widget,
    /// How much of its room the longest row reaches: of a study's period,
    /// of a drawing's span.
    pub width: gtk::Widget,
}

/// The rows of the Value area group.
pub struct ValueAreaRows {
    pub highlight: adw::SwitchRow,
    /// Its colour and transparency, on the standard colour row.
    pub colour: gtk::Widget,
    /// The rows outside the value area.
    pub outside: gtk::Widget,
}

/// The rows of the Point of control group: a line across the profile, on
/// both.
pub struct PocRows {
    pub show: adw::SwitchRow,
    /// Its colour and transparency, on the standard colour row.
    pub colour: gtk::Widget,
    pub thickness: Option<gtk::Widget>,
    pub style: Option<gtk::Widget>,
}

/// Every look row of a profile.
pub struct LookRows {
    pub style: StyleRows,
    pub value_area: ValueAreaRows,
    pub poc: PocRows,
}

/// The Style, Value area and Point of control groups, in that order, each
/// in its one order: a stroke's colour and transparency, on one row, then
/// its thickness and style. The rows a switch
/// governs are there while it is on.
pub fn look_groups(rows: LookRows) -> Vec<adw::PreferencesGroup> {
    let LookRows { style, value_area, poc } = rows;
    let group = |title: &str| {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        group
    };

    let styled = group(STYLE_GROUP);
    // The shape first, its colour next, and a switch with rows under it
    // last, so nothing nested reads as the group's end.
    styled.add(&style.display);
    styled.add(&style.alignment);
    styled.add(&style.width);
    styled.add(&style.colour);
    if let Some(outline) = style.outline {
        styled.add(&outline.switch);
        let rows = vec![outline.colour, outline.thickness, outline.style];
        for row in &rows {
            styled.add(row);
        }
        follow_switch(&outline.switch, rows);
    }
    if let Some(ends) = style.ends {
        styled.add(&ends.switch);
        let rows = vec![ends.open, ends.close];
        for row in &rows {
            styled.add(row);
        }
        follow_switch(&ends.switch, rows);
    }

    let area = group(VALUE_AREA_GROUP);
    area.add(&value_area.highlight);
    let rows = vec![value_area.colour, value_area.outside];
    for row in &rows {
        area.add(row);
    }
    follow_switch(&value_area.highlight, rows);

    let point = group(POC_GROUP);
    point.add(&poc.show);
    let mut rows = vec![poc.colour];
    rows.extend(poc.thickness);
    rows.extend(poc.style);
    for row in &rows {
        point.add(row);
    }
    follow_switch(&poc.show, rows);

    vec![styled, area, point]
}

/// `rows` shown while `switch` is on, from now and as it is turned.
fn follow_switch(switch: &adw::SwitchRow, rows: Vec<gtk::Widget>) {
    for row in &rows {
        row.set_visible(switch.is_active());
    }
    switch.connect_active_notify(move |switch| {
        for row in &rows {
            row.set_visible(switch.is_active());
        }
    });
}

/// How solid something is drawn, as a transparency says it: one minus
/// `alpha`, and the way to show another strength. Hands back a strength,
/// not a transparency. Goes on a colour row, or on one of its own.
pub fn strength(alpha: f64, changed: impl Fn(f64) + 'static) -> (Transparency, Rc<dyn Fn(f64)>) {
    let (transparency, show) = Transparency::new(1.0 - alpha, move |share| changed(1.0 - share));
    (transparency, Rc::new(move |alpha| show(1.0 - alpha)))
}

/// The rows of the Parameters group, built by each side, laid out here.
pub struct ParameterRows {
    /// What only a study has, first: its period and whether only the newest
    /// is drawn.
    pub leading: Vec<gtk::Widget>,
    /// What each kind has of its own: a volume profile's method and its
    /// colouring, a market profile's TPO period.
    pub own: Vec<gtk::Widget>,
    pub granularity: Granularity,
    pub value_area: gtk::Widget,
}

/// The Parameters rows, in their one order, onto `group`. How the value
/// area looks is the Value area group's: not what it is worked out from.
pub fn add_parameters(group: &adw::PreferencesGroup, rows: &ParameterRows) {
    for row in rows.leading.iter().chain(&rows.own) {
        group.add(row);
    }
    group.add(&rows.granularity.automatic);
    group.add(&rows.granularity.rows);
    group.add(&rows.value_area);
}

/// How finely a profile is divided: automatically, or into so many rows.
pub struct Granularity {
    pub automatic: adw::SwitchRow,
    pub rows: adw::SpinRow,
    /// Show another count, or automatic, without calling back.
    pub show: Rc<dyn Fn(Option<usize>)>,
}

/// The Automatic switch and the Rows box under it.
///
/// While the count is automatic the box is greyed and shows the count being
/// drawn, which `drawn` says, so the number on screen is the number on the
/// chart rather than a placeholder. Turning Automatic off hands over that
/// count, so taking control does not also change the chart.
pub fn granularity(
    rows: Option<usize>,
    drawn: Rc<dyn Fn() -> Option<usize>>,
    changed: impl Fn(Option<usize>) + 'static,
) -> Granularity {
    let changed: Rc<dyn Fn(Option<usize>)> = Rc::new(changed);
    let quiet = Rc::new(Cell::new(false));

    let automatic = adw::SwitchRow::new();
    automatic.set_title(AUTOMATIC);
    automatic.set_active(rows.is_none());

    let count = adw::SpinRow::with_range(FEWEST_ROWS, MOST_ROWS, 1.0);
    count.set_title(ROWS);
    count.set_value(rows.or_else(|| drawn()).unwrap_or(ROWS_UNKNOWN) as f64);
    count.set_sensitive(rows.is_some());

    {
        let changed = changed.clone();
        let quiet = quiet.clone();
        let drawn = drawn.clone();
        let count = count.downgrade();
        automatic.connect_active_notify(move |switch| {
            let on = switch.is_active();
            let Some(count) = count.upgrade() else { return };
            count.set_sensitive(!on);
            if quiet.get() {
                return;
            }
            let taken = count.value() as usize;
            changed(if on { None } else { Some(taken) });
            if on && let Some(drawn) = drawn() {
                quiet.set(true);
                count.set_value(drawn as f64);
                quiet.set(false);
            }
        });
    }
    {
        let changed = changed.clone();
        let quiet = quiet.clone();
        count.connect_value_notify(move |count| {
            if quiet.get() || !count.is_sensitive() {
                return;
            }
            changed(Some(count.value() as usize));
        });
    }

    let show: Rc<dyn Fn(Option<usize>)> = {
        let automatic = automatic.clone();
        let count = count.clone();
        Rc::new(move |rows: Option<usize>| {
            quiet.set(true);
            automatic.set_active(rows.is_none());
            count.set_sensitive(rows.is_some());
            count.set_value(rows.or_else(|| drawn()).unwrap_or(ROWS_UNKNOWN) as f64);
            quiet.set(false);
        })
    };

    Granularity {
        automatic,
        rows: count,
        show,
    }
}

/// What a profile's rows measure: the volume at each price, or the ask
/// volume less the bid volume there.
pub fn method_row(
    method: Method,
    changed: impl Fn(Method) + 'static,
) -> (adw::ComboRow, Rc<dyn Fn(Method)>) {
    let at = |method: Method| Method::ALL.iter().position(|m| *m == method).unwrap_or(0) as u32;
    let (row, show) = combo_row(METHOD, &Method::ALL.map(Method::label), at(method), move |picked| {
        changed(Method::ALL.get(picked).copied().unwrap_or_default())
    });
    (row, Rc::new(move |method| show(at(method))))
}

/// The way to show a Colouring row another colouring without calling back.
pub type ShowColouring = Rc<dyn Fn(Option<BarColouring>)>;

/// How a delta's bought and sold rows are painted: in the colouring chosen in
/// Preferences, followed as it changes, or in one of the theme's own. A
/// colouring outside those that a profile already has is offered last, so
/// the row shows what is drawn.
pub fn colouring_row(
    current: Option<BarColouring>,
    changed: impl Fn(Option<BarColouring>) + 'static,
) -> (adw::ComboRow, ShowColouring) {
    let mut offered: Vec<BarColouring> = DELTA_COLOURINGS.to_vec();
    if let Some(kept) = current.filter(|c| !offered.contains(c)) {
        offered.push(kept);
    }
    let mut names = vec![FOLLOWS_DEFAULTS];
    names.extend(offered.iter().map(|c| c.label()));
    let at = {
        let offered = offered.clone();
        move |current: Option<BarColouring>| {
            current.and_then(|c| offered.iter().position(|o| *o == c)).map_or(0, |at| at + 1) as u32
        }
    };
    let (row, show) = combo_row(COLOURING, &names, at(current), move |picked| {
        changed(picked.checked_sub(1).and_then(|at| offered.get(at).copied()))
    });
    (row, Rc::new(move |current| show(at(current))))
}

fn combo_row(
    title: &str,
    names: &[&str],
    at: u32,
    picked: impl Fn(usize) + 'static,
) -> (adw::ComboRow, Rc<dyn Fn(u32)>) {
    let quiet = Rc::new(Cell::new(false));
    let row = adw::ComboRow::new();
    row.set_title(title);
    row.set_model(Some(&gtk::StringList::new(names)));
    row.set_selected(at);
    {
        let quiet = quiet.clone();
        row.connect_selected_notify(move |row| {
            if !quiet.get() {
                picked(row.selected() as usize);
            }
        });
    }
    let shown = row.clone();
    let show: Rc<dyn Fn(u32)> = Rc::new(move |at| {
        quiet.set(true);
        shown.set_selected(at);
        quiet.set(false);
    });
    (row, show)
}

/// Whether a profile measuring by `method` has a value area to set: a
/// delta has none, its rows cancelling each other out.
pub fn has_value_area(method: Method) -> bool {
    method == Method::Total
}

/// Whether a profile measuring by `method` is painted in a colouring: a
/// delta is, bought and sold as rising and falling bars; a total is in the
/// profile's own colour.
pub fn has_colouring(method: Method) -> bool {
    method == Method::Delta
}

/// The rows the calculation method decides about.
pub struct ByMethod {
    pub value_area: gtk::Widget,
    pub colouring: adw::ComboRow,
    /// Rows, or groups, a delta does not paint with or has no use for,
    /// greyed while it is one.
    pub total_only: Vec<gtk::Widget>,
}

/// Grey what a delta has no use for and show its Colouring, for `method`
/// now and, through what is handed back, for any method shown later; and
/// as the Calculation method row is turned.
pub fn follow_method(method_row: &adw::ComboRow, rows: ByMethod) -> Rc<dyn Fn(Method)> {
    let rows = Rc::new(rows);
    let apply: Rc<dyn Fn(Method)> = {
        let rows = rows.clone();
        Rc::new(move |method| {
            let total = has_value_area(method);
            rows.value_area.set_sensitive(total);
            rows.colouring.set_visible(has_colouring(method));
            for row in &rows.total_only {
                row.set_sensitive(total);
            }
        })
    };
    {
        let apply = apply.clone();
        method_row.connect_selected_notify(move |row| {
            apply(Method::ALL.get(row.selected() as usize).copied().unwrap_or_default())
        });
    }
    apply(Method::ALL.get(method_row.selected() as usize).copied().unwrap_or_default());
    apply
}

/// Grey the Style group's Colour and the whole Value area group while
/// `parameters`' Calculation method is a delta, whose rows are painted in
/// its colouring and which has no value area: for a dialog whose groups are
/// built apart. Without a method, nothing.
pub fn look_follows_method(parameters: &adw::PreferencesGroup, groups: &[adw::PreferencesGroup]) {
    let Some(method) = row_titled(parameters.upcast_ref(), METHOD).and_then(|row| row.downcast::<adw::ComboRow>().ok())
    else {
        return;
    };
    let titled = |title: &str| groups.iter().find(|group| group.title() == title).cloned();
    let mut total_only: Vec<gtk::Widget> = Vec::new();
    total_only.extend(titled(STYLE_GROUP).and_then(|group| row_titled(group.upcast_ref(), COLOUR)));
    total_only.extend(titled(VALUE_AREA_GROUP).map(|group| group.upcast()));
    let follow = move |row: &adw::ComboRow| {
        let total = has_value_area(Method::ALL.get(row.selected() as usize).copied().unwrap_or_default());
        for widget in &total_only {
            widget.set_sensitive(total);
        }
    };
    follow(&method);
    method.connect_selected_notify(follow);
}

/// The row titled `title` anywhere under `widget`.
fn row_titled(widget: &gtk::Widget, title: &str) -> Option<gtk::Widget> {
    if widget.downcast_ref::<adw::PreferencesRow>().is_some_and(|row| row.title() == title) {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        if let Some(found) = row_titled(&widget, title) {
            return Some(found);
        }
        child = widget.next_sibling();
    }
    None
}

/// How much of its room the longest row reaches, 10 to 100%.
pub fn width_row(share: f64, changed: impl Fn(f64) + 'static) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    use omacharts_engine::indicators::profile::{drawn_width, MAX_WIDTH, MIN_WIDTH};
    let (row, show) = percent_row(WIDTH, drawn_width(share), MIN_WIDTH, MAX_WIDTH, changed);
    (row, Rc::new(move |share| show(drawn_width(share))))
}

/// The share of the volume the value area holds, 10 to 100%.
pub fn value_area_row(
    share: f64,
    changed: impl Fn(f64) + 'static,
) -> (adw::ActionRow, Rc<dyn Fn(f64)>) {
    percent_row(VALUE_AREA, share, 0.10, 1.0, changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_box_follows_automatic_and_hands_over_the_count_drawn() {
        crate::ui::on_gtk(|| {
            let seen: Rc<std::cell::RefCell<Vec<Option<usize>>>> = Rc::default();
            let rows = {
                let seen = seen.clone();
                granularity(None, Rc::new(|| Some(236)), move |rows| {
                    seen.borrow_mut().push(rows)
                })
            };
            assert!(rows.automatic.is_active());
            assert!(!rows.rows.is_sensitive());
            assert_eq!(
                rows.rows.value(),
                236.0,
                "the count being drawn, not a placeholder"
            );

            rows.automatic.set_active(false);
            assert_eq!(
                seen.borrow().last(),
                Some(&Some(236)),
                "taking control keeps the chart as it was"
            );
            assert!(rows.rows.is_sensitive());
            rows.rows.set_value(120.0);
            assert_eq!(seen.borrow().last(), Some(&Some(120)));

            let before = seen.borrow().len();
            (rows.show)(None);
            assert!(rows.automatic.is_active());
            assert_eq!(
                seen.borrow().len(),
                before,
                "showing a style is not changing it"
            );
        });
    }
}
