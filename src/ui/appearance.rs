//! How every chart looks: the theme and what candles look like, on one page.
//!
//! Both are built wherever they are wanted — in Preferences, and beside a
//! chart's own settings, where you are when you notice the candles are the
//! wrong colour. A theme is an Omarchy theme, and making one is how a
//! palette is made your own — Omarchy already has the tools for that, and a
//! theme made there dresses the whole desktop rather than one app.
//!
//! The charts behind the window are the preview: they change as a choice is
//! made.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::theme::{bar_colour, ink, BarLook, ColorChoice, Swatch, INK};

use crate::store::Store;
use crate::theming::Theming;
use crate::ui::{palette, theme_picker};

struct Context {
    store: Rc<Store>,
    theming: Rc<RefCell<Theming>>,
    on_change: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    /// The groups [`rebuild`] owns.
    groups: RefCell<Vec<adw::PreferencesGroup>>,
}

thread_local! {
    /// Every page built and not yet thrown away, so a desktop theme change
    /// can redraw the card that follows it. More than one: Preferences and
    /// a chart's settings can both be open.
    static SHOWN: RefCell<Vec<Weak<Context>>> = const { RefCell::new(Vec::new()) };
}

/// The Appearance page. `on_change` repaints the charts after the theme or
/// the bars have been stored.
pub fn page(
    store: Rc<Store>,
    theming: Rc<RefCell<Theming>>,
    on_change: Rc<dyn Fn()>,
) -> adw::PreferencesPage {
    let context = Rc::new(Context {
        store,
        theming,
        on_change,
        page: adw::PreferencesPage::new(),
        groups: RefCell::new(Vec::new()),
    });
    rebuild(&context);
    SHOWN.with(|shown| {
        let mut shown = shown.borrow_mut();
        shown.retain(|c| c.upgrade().is_some_and(|c| c.page.root().is_some()));
        shown.push(Rc::downgrade(&context));
    });
    context.page.clone()
}

/// The theme or the bars changed from somewhere other than one of these
/// pages — the desktop, a command — so every page on screen shows it.
pub fn restyled() {
    let shown: Vec<Rc<Context>> =
        SHOWN.with(|shown| shown.borrow().iter().filter_map(Weak::upgrade).collect());
    for context in shown.iter().filter(|c| c.page.root().is_some()) {
        rebuild(context);
    }
}

/// Clear and re-add the groups.
///
/// Called whenever the theme changes, because the bar colours on offer are
/// the theme's.
fn rebuild(context: &Rc<Context>) {
    let page = &context.page;
    for group in context.groups.borrow_mut().drain(..) {
        page.remove(&group);
    }
    let groups = vec![theme_group(context), bars_group(context)];
    for group in &groups {
        page.add(group);
    }
    *context.groups.borrow_mut() = groups;
}

/// Store a change, then repaint the charts.
fn apply(context: &Rc<Context>) {
    context.theming.borrow_mut().apply();
    (context.on_change)();
}

fn theme_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let theming = context.theming.borrow();
    let look = theming.bar_look().clone();
    let choices = theme_picker::Choices {
        themes: theming.themes().into_iter().cloned().collect(),
        current: theming.theme_id().to_string(),
        bars: Rc::new(move |theme| look.scheme(theme)),
        pick: {
            let ctx = context.clone();
            Rc::new(move |id| choose_theme(&ctx, id))
        },
    };
    drop(theming);
    theme_picker::group(choices)
}

/// Wear a theme, and rebuild the page once the card that chose it has
/// finished: the cards and the bar colours on offer are the theme's.
fn choose_theme(context: &Rc<Context>, id: &str) {
    if id == context.theming.borrow().theme_id() {
        return;
    }
    context.theming.borrow_mut().select_theme(id, &context.store);
    apply(context);
    // Every page, not only this one: the other one open shows the old
    // theme's cards and colours otherwise.
    glib::idle_add_local_once(restyled);
}

/// Four answers, each a row: a colour for each direction, and whether its
/// body is hollow. Red for a rise is the two colours swapped; monochrome is
/// Ink both ways.
fn bars_group(context: &Rc<Context>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Bars");
    group.set_description(Some(
        "Colours are the theme's, so they change with it. Ink is its text colour, for bars with no colour.",
    ));

    let theming = context.theming.borrow();
    let theme = theming.theme();
    let look = theming.bar_look().clone();
    drop(theming);

    // The theme's swatches and Ink: what a bar can be drawn in.
    let mut offered = theme.clone();
    offered.swatches.push(Swatch::new(INK, &ink(&theme)));

    group.add(&colour_row(context, &offered, "Up colour", &look.up, |look, c| look.up = c));
    group.add(&colour_row(context, &offered, "Down colour", &look.down, |look, c| look.down = c));
    group.add(&hollow_row(context, "Hollow up candles", look.up_hollow, |look, on| look.up_hollow = on));
    group.add(&hollow_row(context, "Hollow down candles", look.down_hollow, |look, on| {
        look.down_hollow = on
    }));
    group
}

/// Change one of the four answers, store it, and repaint everything.
fn change_bars(context: &Rc<Context>, edit: impl FnOnce(&mut BarLook)) {
    let mut look = context.theming.borrow().bar_look().clone();
    edit(&mut look);
    context.theming.borrow_mut().set_bar_look(look, &context.store);
    apply(context);
}

/// A direction's colour, picked from `offered`: the theme's palette and Ink.
fn colour_row(
    context: &Rc<Context>,
    offered: &omacharts_engine::Theme,
    title: &str,
    choice: &ColorChoice,
    edit: fn(&mut BarLook, ColorChoice),
) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(title);
    let ctx = context.clone();
    let picker = palette::picker(offered, Some(choice.clone()), &bar_colour(offered, choice), move |choice| {
        change_bars(&ctx, |look| edit(look, choice));
    });
    row.add_suffix(&picker);
    row.set_activatable_widget(Some(&picker));
    row
}

fn hollow_row(context: &Rc<Context>, title: &str, on: bool, edit: fn(&mut BarLook, bool)) -> adw::SwitchRow {
    let row = adw::SwitchRow::new();
    row.set_title(title);
    row.set_active(on);
    let ctx = context.clone();
    row.connect_active_notify(move |row| {
        let on = row.is_active();
        change_bars(&ctx, |look| edit(look, on));
    });
    row
}
