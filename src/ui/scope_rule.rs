//! The rule over Preferences' visual sections, and the choice on it: whether
//! those sections change every chart or only the focused one.
//!
//! `VISUAL        GLOBAL / LOCAL`, the choice against the right edge. Local
//! rather than the chart's symbol, which can be FRED:IRLTLT01GBM156N long;
//! the symbol is in its tooltip. Picking Local greys the rows that can never
//! be a chart's own. While the focused chart's own look differs from every
//! chart's, under either choice, a reset after LOCAL gives it every chart's
//! look back: under Global too, so a chart that looks different says why.
//!
//! In a split, while Local is picked, the focused chart wears the ring a
//! picker gives the chart it is about to change, so which chart the rows
//! change is plain from the window.
//!
//! The visual pages read and change the look through [`look`] and
//! [`change`], so which charts an edit reaches is decided here alone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::chart_look::{ChartLook, LookEdit};
use crate::ui::window::Window;

/// The name a row that only ever means every chart is given, so the rule
/// can find it and grey it while one chart is picked.
pub const GLOBAL_ONLY: &str = "global-only";

/// The class that rings the chart Local changes, as `picked` rings the chart
/// a picker changes; one rule draws both.
const LOCAL_TARGET: &str = "local-target";

/// Names the focused chart to the rule, or says there is none.
type Rename = Rc<dyn Fn(Option<String>)>;

/// Tells the rule whether the chart's look differs from every chart's, so
/// the reset shows.
type Note = Rc<dyn Fn(bool)>;

thread_local! {
    /// The open rule's way to rename its chart, so focus moving to another
    /// chart while Preferences is open renames it.
    static RENAME: RefCell<Option<Rename>> = const { RefCell::new(None) };
    /// Whether Local is picked. Global whenever Preferences is closed, so a
    /// chart's settings, which show the same pages, change every chart.
    static LOCAL: Cell<bool> = const { Cell::new(false) };
    /// The rule's way to show the reset while the chart's look differs.
    static NOTE: RefCell<Option<Note>> = const { RefCell::new(None) };
    /// The chart wearing [`LOCAL_TARGET`], so the ring moves with the focus
    /// and comes off with the rule.
    static RINGED: RefCell<Option<glib::WeakRef<gtk::Box>>> = const { RefCell::new(None) };
}

/// The look the visual pages show: every chart's, or with Local picked the
/// one the focused chart wears.
pub fn look(window: &Window) -> ChartLook {
    window.look(!LOCAL.get())
}

/// Change the look where the rule says: every chart's, or the focused
/// chart's alone, copying every chart's look onto it on its first change.
pub fn change(window: &Rc<Window>, edit: LookEdit) {
    window.change_look(edit, !LOCAL.get());
    note(window);
}

/// The focused chart changed, or what it shows did: the open rule names it,
/// and with Local picked the pages show its look.
pub fn follow(window: &Window) {
    let Some(rename) = RENAME.with(|rename| rename.borrow().clone()) else { return };
    let symbol = window.focused_pane().instrument.borrow().as_ref().map(|i| i.display_symbol());
    rename(symbol);
    note(window);
    ring(local_chart(window));
}

/// The focused chart's pane while Local is picked; none under Global.
fn local_chart(window: &Window) -> Option<gtk::Box> {
    LOCAL.get().then(|| window.focused_pane().root.clone())
}

/// Ring `chart`, taking the ring off whichever chart wore it.
fn ring(chart: Option<gtk::Box>) {
    RINGED.with(|ringed| {
        let mut ringed = ringed.borrow_mut();
        if let Some(was) = ringed.take().and_then(|was| was.upgrade()) {
            was.remove_css_class(LOCAL_TARGET);
        }
        if let Some(chart) = chart {
            chart.add_css_class(LOCAL_TARGET);
            *ringed = Some(chart.downgrade());
        }
    });
}

/// Tell the rule whether the focused chart's look differs from every
/// chart's.
fn note(window: &Window) {
    if let Some(note) = NOTE.with(|note| note.borrow().clone()) {
        note(window.own_look_differs());
    }
}

/// Every visual page on screen shows the look in scope again, with the
/// rows under `pages` that are every chart's alone greyed while Local is
/// picked. Greyed after, since a page rebuilds its rows.
fn reshow(pages: &[gtk::Widget]) {
    crate::ui::appearance::restyled();
    crate::ui::overlays::header_changed();
    crate::ui::overlays::charts_changed();
    for page in pages {
        grey(page, !LOCAL.get());
    }
}

/// The rule, for the focused chart showing `symbol`. Picking Local shows
/// `pages` the chart's look and greys every [`GLOBAL_ONLY`] row under them.
/// `scoped` hears whether Local is picked each time that changes, and again
/// when the focus moves while it is.
pub fn rule(window: &Rc<Window>, symbol: Option<String>, pages: Vec<gtk::Widget>, scoped: impl Fn(bool) + 'static) -> gtk::Widget {
    let global = choice("GLOBAL");
    let local = choice("LOCAL");
    local.add_css_class("scope-local");
    local.set_group(Some(&global));
    global.set_active(true);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("scope-rule");
    row.set_margin_top(20);
    row.set_margin_bottom(8);
    row.set_margin_start(12);
    row.set_margin_end(12);
    let heading = crate::ui::preferences::rule_word("VISUAL");
    // The choice against the sidebar's right edge, the heading at its left.
    heading.set_hexpand(true);
    heading.set_xalign(0.0);
    row.append(&heading);
    row.append(&global);
    row.append(&crate::ui::preferences::rule_word("/"));
    row.append(&local);
    // Only while the focused chart's look differs from every chart's, so
    // its showing is what says the chart has been changed.
    let reset = gtk::Button::new();
    reset.set_child(Some(&reset_glyph(&reset)));
    reset.add_css_class("flat");
    reset.add_css_class("scope-reset");
    reset.set_tooltip_text(Some("Reset to global defaults"));
    row.append(&reset);

    let scoped: Rc<dyn Fn(bool)> = Rc::new(scoped);
    let pages: Rc<[gtk::Widget]> = pages.into();
    let rename: Rename = {
        let (global, local, scoped, pages) = (global.clone(), local.clone(), scoped.clone(), pages.clone());
        Rc::new(move |symbol: Option<String>| {
            local.set_sensitive(symbol.is_some());
            local.set_tooltip_text(symbol.as_deref());
            if symbol.is_none() {
                global.set_active(true);
            } else if local.is_active() {
                scoped(true);
                reshow(&pages);
            }
        })
    };
    rename(symbol);
    RENAME.with(|slot| *slot.borrow_mut() = Some(rename));

    let show: Note = {
        let reset = reset.clone();
        Rc::new(move |differs: bool| reset.set_visible(differs))
    };
    show(window.own_look_differs());
    NOTE.with(|slot| *slot.borrow_mut() = Some(show));
    {
        let (window, pages) = (window.clone(), pages.clone());
        reset.connect_clicked(move |_| {
            window.follow_global_look();
            note(&window);
            reshow(&pages);
        });
    }

    // Gone with the window, so a closed Preferences is not renamed, and the
    // pages a chart's settings show change every chart again.
    row.connect_destroy(|_| {
        RENAME.with(|slot| *slot.borrow_mut() = None);
        NOTE.with(|slot| *slot.borrow_mut() = None);
        LOCAL.set(false);
        ring(None);
    });

    {
        let window = window.clone();
        local.connect_toggled(move |local| {
            LOCAL.set(local.is_active());
            note(&window);
            ring(local_chart(&window));
            scoped(local.is_active());
            reshow(&pages);
        });
    }
    row.upcast()
}

/// ↺, in the colour of `button`'s text. Drawn rather than taken from the
/// icon theme, whose revert arrows are hooks or documents depending on the
/// theme.
fn reset_glyph(button: &gtk::Button) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(12);
    area.set_content_height(12);
    area.set_valign(gtk::Align::Center);
    area.set_draw_func(|area, cr, width, height| {
        let colour = area.color();
        cr.set_source_color(&colour);
        let scale = f64::from(width.min(height)) / 16.0;
        cr.scale(scale, scale);
        cr.set_line_width(1.8);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        // Round from the left, the long way, to the upper left, where the
        // head points back the way it came.
        cr.arc_negative(8.0, 8.0, 5.0, std::f64::consts::PI, (-3.7f64).atan2(-3.4));
        cr.move_to(3.0, 2.5);
        cr.line_to(3.0, 5.5);
        cr.line_to(6.0, 5.5);
        let _ = cr.stroke();
    });
    // The colour follows the pointer onto the button.
    let drawn = area.clone();
    button.connect_state_flags_changed(move |_, _| drawn.queue_draw());
    area
}

/// One of the two choices: a word of the rule that can be clicked.
fn choice(text: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    button.add_css_class("flat");
    button.set_child(Some(&crate::ui::preferences::rule_word(text)));
    button
}

/// Set every [`GLOBAL_ONLY`] widget under `root` to `sensitive`.
fn grey(root: &gtk::Widget, sensitive: bool) {
    let mut child = root.first_child();
    while let Some(widget) = child {
        if widget.widget_name() == GLOBAL_ONLY {
            widget.set_sensitive(sensitive);
        } else {
            grey(&widget, sensitive);
        }
        child = widget.next_sibling();
    }
}
