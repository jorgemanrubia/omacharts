//! A form big enough to get a window of its own.
//!
//! The way Preferences is one: a toplevel the compositor can see, rather than
//! a sheet drawn inside the main window. A sheet cannot be moved, floated
//! beside the chart it is changing or pushed aside to read what is behind
//! it, and it takes the charts with it while it is open. Everything one of
//! these windows holds applies as it is changed, so being able to see the
//! chart while changing it is the point.
//!
//! One window per kind: asking for a second raises the first, so a key
//! pressed twice does not stack two copies of the same form.
//!
//! Escape closes, once nothing inside wanted it: a pushed page pops first,
//! because the navigation view takes the key on its way up, and an open
//! dropdown closes only itself. It never asks, even on an edited form —
//! nothing here is unsaved, it is already on the chart.

use std::cell::RefCell;
use std::collections::HashMap;

use adw::prelude::*;
use gtk::glib;

thread_local! {
    /// Each kind's window while it is open.
    static OPEN: RefCell<HashMap<&'static str, glib::WeakRef<adw::Window>>> =
        RefCell::new(HashMap::new());
}

/// The window of this kind, if one is on screen.
///
/// Only a visible one: a closed window can outlive its closing while a
/// closure inside it still holds it.
pub fn open(kind: &'static str) -> Option<adw::Window> {
    OPEN.with(|open| open.borrow().get(kind).and_then(|w| w.upgrade())).filter(|w| w.is_visible())
}

/// The forms open over `parent`. They are windows of their own, not in
/// `parent`'s widget tree, so this is the only way to reach them from it.
pub(crate) fn over(parent: &impl IsA<gtk::Window>) -> Vec<adw::Window> {
    let parent = parent.as_ref();
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<adw::Window>().ok())
        .filter(|window| window.is_visible() && window.transient_for().as_ref() == Some(parent))
        .collect()
}

/// A new window of this kind, transient for `parent`, styled as a form and
/// closed by Escape. It is not shown: the caller fills it and presents it.
///
/// Closing it hands the keyboard back to `parent`, which is where it was
/// before the window took it.
pub fn new(parent: &impl IsA<gtk::Window>, kind: &'static str, title: &str, size: (i32, i32)) -> adw::Window {
    let parent = parent.as_ref().clone();
    let window = untracked(&parent, title, size);
    window.connect_close_request(move |_| {
        OPEN.with(|open| open.borrow_mut().remove(kind));
        parent.present();
        glib::Propagation::Proceed
    });

    OPEN.with(|open| open.borrow_mut().insert(kind, window.downgrade()));
    window
}

/// A window as `new` makes one, transient for `parent`, styled as a form and
/// closed by Escape, but not one of the kinds `open` knows about, and with
/// nothing done when it closes.
pub fn untracked(parent: &impl IsA<gtk::Window>, title: &str, size: (i32, i32)) -> adw::Window {
    let window = adw::Window::new();
    window.set_title(Some(title));
    window.set_default_size(size.0, size.1);
    window.set_transient_for(Some(parent));
    window.set_application(parent.as_ref().application().as_ref());
    window.add_css_class("form-window");
    window.connect_map(|window| steady_columns(window.upcast_ref()));

    let keys = gtk::EventControllerKey::new();
    let closing = window.downgrade();
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if key != gtk::gdk::Key::Escape || !modifiers.is_empty() {
            return glib::Propagation::Proceed;
        }
        if let Some(window) = closing.upgrade() {
            window.close();
        }
        glib::Propagation::Stop
    });
    window.add_controller(keys);
    window
}

/// Holds every settings page under `widget` to a column as wide as its
/// window allows, up to the page's maximum, whatever its rows need.
///
/// A page's clamp slides between its tightening threshold and its maximum,
/// and starts from the rows' own minimum width when that is the larger. So a
/// row that shows or hides something wide, such as the bar styles' body
/// strip, widened or narrowed the whole column, and the window looked as if
/// it had resized. With the threshold at the maximum, the column follows the
/// window alone.
///
/// Run when the window maps, over the pages it holds by then: a page pushed
/// later keeps libadwaita's sliding column.
fn steady_columns(widget: &gtk::Widget) {
    if let Some(page) = widget.downcast_ref::<adw::PreferencesPage>() {
        let clamp = page
            .first_child()
            .and_downcast::<gtk::ScrolledWindow>()
            .and_then(|scroller| scroller.child())
            .and_downcast::<gtk::Viewport>()
            .and_then(|viewport| viewport.child())
            .and_downcast::<adw::Clamp>();
        if let Some(clamp) = clamp {
            clamp.set_tightening_threshold(clamp.maximum_size());
        }
    }
    for child in crate::ui::children(widget) {
        steady_columns(&child);
    }
}
