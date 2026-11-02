//! The rule over Preferences' visual sections, and the choice on it: whether
//! those sections change every chart or only the focused one.
//!
//! `VISUAL        GLOBAL / LOCAL`, the choice against the right edge. Local
//! rather than the chart's symbol, which can be FRED:IRLTLT01GBM156N long;
//! the symbol is in its tooltip. Picking Local greys the rows that can never
//! be a chart's own. The look is not stored per chart yet; the Charts page's
//! settings are, and follow the choice.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::ui::window::Window;

/// The name a row that only ever means every chart is given, so the rule
/// can find it and grey it while one chart is picked.
pub const GLOBAL_ONLY: &str = "global-only";

/// Names the focused chart to the rule, or says there is none.
type Rename = Rc<dyn Fn(Option<String>)>;

thread_local! {
    /// The open rule's way to rename its chart, so focus moving to another
    /// chart while Preferences is open renames it.
    static RENAME: RefCell<Option<Rename>> = const { RefCell::new(None) };
}

/// The focused chart changed, or what it shows did: the open rule names it.
pub fn follow(window: &Window) {
    let Some(rename) = RENAME.with(|rename| rename.borrow().clone()) else { return };
    let symbol = window.focused_pane().instrument.borrow().as_ref().map(|i| i.display_symbol());
    rename(symbol);
}

/// The rule, for the focused chart showing `symbol`. Picking Local greys
/// every [`GLOBAL_ONLY`] row under `pages`. `scoped` hears whether Local is
/// picked each time that changes, and again when the focus moves while it is.
pub fn rule(symbol: Option<String>, pages: Vec<gtk::Widget>, scoped: impl Fn(bool) + 'static) -> gtk::Widget {
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

    let scoped: Rc<dyn Fn(bool)> = Rc::new(scoped);
    let rename: Rename = {
        let (global, local, scoped) = (global.clone(), local.clone(), scoped.clone());
        Rc::new(move |symbol: Option<String>| {
            local.set_sensitive(symbol.is_some());
            local.set_tooltip_text(symbol.as_deref());
            if symbol.is_none() {
                global.set_active(true);
            } else if local.is_active() {
                scoped(true);
            }
        })
    };
    rename(symbol);
    RENAME.with(|slot| *slot.borrow_mut() = Some(rename));
    // Gone with the window, so a closed Preferences is not renamed.
    row.connect_destroy(|_| RENAME.with(|slot| *slot.borrow_mut() = None));

    local.connect_toggled(move |local| {
        for page in &pages {
            grey(page, !local.is_active());
        }
        scoped(local.is_active());
    });
    row.upcast()
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
