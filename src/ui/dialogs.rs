//! What Ctrl+Enter and Escape mean in a dialog.
//!
//! Anything that creates something has one action that finishes it, and in
//! this app that action is always a button in a corner you are not looking at:
//! you are looking at the list you just picked from. Ctrl+Enter is the key for
//! it.
//!
//! Enter alone could not be. Every dialog here is built around a search box
//! and a list, where Return already means "take the highlighted row" — which
//! is a different thing from "I am done", and sometimes the step before it.

use adw::prelude::*;
use gtk::glib;

/// Finish a dialog with Ctrl+Enter.
///
/// Caught on the way down rather than on the way up, because by then the key
/// may be gone: a dialog's content is a text box and a list, and either may
/// decide a Return belongs to it before a controller on the dialog itself
/// hears about it. Catching it first and stopping it there also settles how
/// many times it can happen, which for a button that adds an indicator is the
/// difference between one indicator and two.
pub fn commit_on_ctrl_enter(dialog: &impl IsA<gtk::Widget>, commit: impl Fn() + 'static) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, key, _, state| {
        if !state.contains(gtk::gdk::ModifierType::CONTROL_MASK) || !is_enter(key) {
            return glib::Propagation::Proceed;
        }
        commit();
        glib::Propagation::Stop
    });
    dialog.as_ref().add_controller(keys);
}

/// Close a dialog from Escape in its search box.
///
/// Everything else inside a dialog already works: an `AdwDialog` closes itself
/// on Escape, a popover inside one closes only the popover, and a pushed
/// subpage pops rather than taking the whole dialog with it. All of that is
/// the dialog's own shortcut, which sits on the sheet the dialog is drawn in
/// and so only ever sees a key on its way up from whatever holds the keyboard
/// inside it — a dialog the keyboard has left hears nothing, which is the
/// window's problem to catch and not this one's.
///
/// A `GtkSearchEntry` is the one hole in here. It claims Escape and emits
/// `stop-search` instead — and then does
/// nothing at all with it, not even clearing the text, because the clearing
/// everyone remembers belongs to `GtkSearchBar` and there is no search bar
/// here. So Escape in the box is simply dead unless something listens.
///
/// It closes rather than clearing. The box is the dialog: emptying it would
/// leave an empty picker open and a second Escape to press, when the hand
/// that reached for the key had already decided against the whole thing.
pub fn close_on_search_escape(entry: &gtk::SearchEntry, dialog: &adw::Dialog) {
    let dialog = dialog.clone();
    entry.connect_stop_search(move |_| {
        dialog.close();
    });
}

/// "Ctrl+Enter", written the way this desktop writes it.
///
/// For the tooltip on the button the key stands in for. A finishing key that
/// is only in a key handler is a finishing key nobody finds, and the button it
/// belongs to is the one place somebody is already looking for it.
pub fn commit_label() -> String {
    gtk::accelerator_get_label(gtk::gdk::Key::Return, gtk::gdk::ModifierType::CONTROL_MASK)
        .to_string()
}

/// Return, wherever on the keyboard it was pressed.
///
/// The keypad's is a different keyval from the main one, and a keyboard laid
/// out for some other script can send a third. A dialog that commits from one
/// Return and not another is a dialog that looks broken.
fn is_enter(key: gtk::gdk::Key) -> bool {
    use gtk::gdk::Key;
    matches!(key, Key::Return | Key::KP_Enter | Key::ISO_Enter)
}

/// Alt and an arrow walks a dialog's tabs.
///
/// `step` is handed `true` for the next tab and `false` for the one before,
/// and does the switching: the two dialogs that have tabs hold them in
/// different widgets — a plain stack in one, a preferences dialog's own
/// pages in the other — and there is nothing to share between them but the
/// key.
///
/// Alt because every other arrow is spoken for: a bare one moves inside a
/// row, and on the chart behind it nudges a drawing. Alt with an arrow
/// already means "to the next thing" in this app — it is how the focus
/// walks from chart to chart.
///
/// Caught on the way down, before a row that has the keyboard can take it:
/// a combo row and a spin button both answer to arrows, and the hand is
/// usually on one.
pub fn step_tabs_with_alt(dialog: &impl IsA<gtk::Widget>, step: impl Fn(bool) + 'static) {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        use gtk::gdk::Key;
        if !modifiers.contains(gtk::gdk::ModifierType::ALT_MASK) {
            return glib::Propagation::Proceed;
        }
        match key {
            Key::Right => step(true),
            Key::Left => step(false),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    dialog.as_ref().add_controller(keys);
}

/// The one after `at` in a run of `count`, or the one before, wrapping at
/// both ends.
///
/// Wrapping because there are two of them: stopping at an end would make
/// the second press do nothing for no reason anybody could see.
pub fn step_round(at: u32, count: u32, forward: bool) -> u32 {
    match forward {
        true => (at + 1) % count,
        false => (at + count - 1) % count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_return_key_commits() {
        use gtk::gdk::Key;
        for key in [Key::Return, Key::KP_Enter, Key::ISO_Enter] {
            assert!(is_enter(key), "{key:?} should finish a dialog");
        }
    }

    #[test]
    fn nothing_else_does() {
        use gtk::gdk::Key;
        // Space is the one worth naming: it activates a focused button, so a
        // dialog that treated it as Return would commit from a Cancel button
        // somebody was only trying to press.
        for key in [Key::space, Key::Escape, Key::Tab, Key::a] {
            assert!(!is_enter(key), "{key:?} should not finish a dialog");
        }
    }
}
