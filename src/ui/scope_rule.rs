//! The rule over a settings window's visual sections, and the choice on it:
//! whether those sections change every chart or only the focused one.
//!
//! `VISUAL        GLOBAL / LOCAL`, the choice against the right edge. Local
//! rather than the chart's symbol, which can be FRED:IRLTLT01GBM156N long;
//! the symbol is in its tooltip. Picking Local greys the rows that can never
//! be a chart's own. While the focused chart's own look differs from every
//! chart's, under either choice, a reset after LOCAL gives it every chart's
//! look back: under Global too, so a chart that looks different says why.
//!
//! Each settings window that shows the visual pages has a [`Scope`] of its
//! own, Preferences one and a chart's settings another, so picking Local in
//! one leaves the other as it was. The pages read and change the look
//! through the scope they were built for ([`Scope::look`],
//! [`Scope::change`]), so which charts an edit reaches is decided here
//! alone.
//!
//! In a split, while any open scope has Local picked, the focused chart
//! wears the ring a picker gives the chart it is about to change, so which
//! chart the rows change is plain from the window.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;

use omacharts_engine::theme::{BarScheme, Theme};

use crate::chart_look::{ChartLook, LookEdit};
use crate::ui::window::Window;

/// The class that rings the chart Local changes, as `picked` rings the chart
/// a picker changes; one rule draws both.
const LOCAL_TARGET: &str = "local-target";

/// One settings window's choice of which charts its visual pages change.
/// Whether its Local choice is active is the one record of whether Local is
/// picked there.
pub struct Scope {
    window: Rc<Window>,
    global: gtk::ToggleButton,
    local: gtk::ToggleButton,
    /// Shown only while the focused chart's look differs from every chart's.
    reset: gtk::Button,
}

thread_local! {
    /// Every scope whose rule is still standing, so focus moving to another
    /// chart renames each, and the ring knows whether any has Local picked.
    /// A scope leaves with its rule.
    static OPEN: RefCell<Vec<Rc<Scope>>> = const { RefCell::new(Vec::new()) };
    /// The chart wearing [`LOCAL_TARGET`], so the ring moves with the focus
    /// and comes off with the last Local rule.
    static RINGED: RefCell<Option<glib::WeakRef<gtk::Box>>> = const { RefCell::new(None) };
}

/// The look in scope as a page draws it: the look, the theme it is drawn
/// on, and its bars on that theme. Worked out once per change, not per
/// picture drawn.
#[derive(Clone, PartialEq)]
pub struct Worn {
    pub look: ChartLook,
    pub theme: Theme,
    pub scheme: BarScheme,
}

impl Scope {
    /// A scope for a settings window over `window`'s charts, and its rule,
    /// for the window's sidebar. It starts on Local when `local` asks and the
    /// focused chart shows something to have a look of, else on Global.
    /// Picking either has the pages show the look in scope, through the
    /// window's watchers ([`Window::watch_chart`]).
    pub fn new(window: &Rc<Window>, local: bool) -> (Rc<Scope>, gtk::Widget) {
        let global = choice("GLOBAL");
        let local_choice = choice("LOCAL");
        local_choice.add_css_class("scope-local");
        local_choice.set_group(Some(&global));
        global.set_active(true);
        local_choice.set_active(local);

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
        row.append(&local_choice);
        // Only while the focused chart's look differs from every chart's, so
        // its showing is what says the chart has been changed.
        let reset = gtk::Button::new();
        reset.set_child(Some(&reset_glyph(&reset)));
        reset.add_css_class("flat");
        reset.add_css_class("scope-reset");
        reset.set_tooltip_text(Some("Reset to global defaults"));
        row.append(&reset);

        let scope = Rc::new(Scope { window: window.clone(), global, local: local_choice, reset });
        // Named before anything is wired, so a chart with nothing to change
        // falling back to Global is the start, not a change of scope.
        scope.name();
        OPEN.with(|open| open.borrow_mut().push(scope.clone()));
        settle(window);

        let held = Rc::downgrade(&scope);
        scope.reset.connect_clicked(move |_| {
            let Some(scope) = held.upgrade() else { return };
            scope.window.follow_global_look();
            settle(&scope.window);
            scope.window.chart_changed_elsewhere();
        });
        let held = Rc::downgrade(&scope);
        scope.local.connect_toggled(move |_| {
            let Some(scope) = held.upgrade() else { return };
            settle(&scope.window);
            scope.window.chart_changed_elsewhere();
        });
        // Gone with its settings window, so a closed one is not renamed and
        // its Local no longer rings the chart.
        let held = Rc::downgrade(&scope);
        row.connect_destroy(move |_| {
            let Some(scope) = held.upgrade() else { return };
            OPEN.with(|open| open.borrow_mut().retain(|other| !Rc::ptr_eq(other, &scope)));
            settle(&scope.window);
        });
        (scope, row.upcast())
    }

    /// The chart window whose charts this scope changes.
    pub fn window(&self) -> &Rc<Window> {
        &self.window
    }

    /// Whether Local is picked.
    pub fn local(&self) -> bool {
        self.local.is_active()
    }

    /// The look the pages show: every chart's, or with Local picked the one
    /// the focused chart wears.
    pub fn look(&self) -> ChartLook {
        self.window.look(!self.local())
    }

    pub fn worn(&self) -> Worn {
        let look = self.look();
        let theme = self.window.theme_of(&look);
        let scheme = look.bars.scheme(&theme);
        Worn { look, theme, scheme }
    }

    /// Change the look where the rule says: every chart's, or the focused
    /// chart's alone, copying every chart's look onto it on its first change.
    pub fn change(&self, edit: LookEdit) {
        self.window.change_look(edit, !self.local());
        settle(&self.window);
    }

    /// Grey `row` while Local is picked: what it changes is every chart's
    /// alone. Set as it is built, since a page rebuilds its rows, and again on
    /// each change of scope, which the rule tells the window's watchers of.
    pub fn global_only(self: &Rc<Self>, row: &impl IsA<gtk::Widget>) {
        row.set_sensitive(!self.local());
        let shown = row.upcast_ref::<gtk::Widget>().downgrade();
        let scope: Weak<Scope> = Rc::downgrade(self);
        self.window.watch_chart(move || {
            let Some(row) = shown.upgrade().filter(|row| row.root().is_some()) else { return false };
            let Some(scope) = scope.upgrade() else { return false };
            row.set_sensitive(!scope.local());
            true
        });
    }

    /// Name the focused chart in Local's tooltip. A chart showing no symbol
    /// has no look of its own to change, so Local is greyed and Global picked.
    fn name(&self) {
        let symbol = self.window.focused_pane().instrument.borrow().as_ref().map(|i| i.display_symbol());
        self.local.set_sensitive(symbol.is_some());
        self.local.set_tooltip_text(symbol.as_deref());
        if symbol.is_none() {
            self.global.set_active(true);
        }
    }
}

/// The open scopes over `window`'s charts. Cloned out, so nothing they set
/// off finds the list borrowed.
fn open(window: &Window) -> Vec<Rc<Scope>> {
    OPEN.with(|open| open.borrow().iter().filter(|scope| std::ptr::eq(Rc::as_ptr(&scope.window), window)).cloned().collect())
}

/// The focused chart changed, or what it shows did: every open rule names
/// it, and the pages of a scope with Local picked show its look.
pub fn follow(window: &Window) {
    let scopes = open(window);
    for scope in &scopes {
        scope.name();
    }
    if scopes.iter().any(|scope| scope.local()) {
        window.chart_changed_elsewhere();
    }
    settle(window);
}

/// Tell every open rule whether the focused chart's look differs from
/// every chart's, and ring the focused chart while any has Local picked.
fn settle(window: &Window) {
    let scopes = open(window);
    let differs = window.own_look_differs();
    for scope in &scopes {
        scope.reset.set_visible(differs);
    }
    ring(scopes.iter().any(|scope| scope.local()).then(|| window.focused_pane().root.clone()));
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
