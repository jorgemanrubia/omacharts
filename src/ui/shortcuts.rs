//! What the keys do, in one table.
//!
//! A shortcut that only exists inside a key handler is a shortcut
//! nobody finds. The menus and the tooltips should say so themselves,
//! and they should say it in the desktop's own words — "Ctrl+K" here,
//! something else on a machine set up differently — which is what
//! `gtk::accelerator_get_label` is for.
//!
//! So the table is the one place a binding is written down, and the
//! constants under it are the one place for the keys no action owns.
//! The application registers what it can from the table, the menu
//! items carry a key as the `accel` attribute GTK draws on the right
//! of a row, the tooltips append it in brackets, and the shortcuts
//! window reads it rather than repeating it.

use adw::prelude::*;
use gtk::gio;

/// One key, and what it does.
pub struct Binding {
    /// The action it activates, group and all: "win.find".
    pub action: &'static str,
    /// In GTK's accelerator syntax, most important first — the first
    /// is what menus and tooltips show.
    pub accels: &'static [&'static str],
    /// Whether the application may handle it.
    ///
    /// Registering an accelerator puts it above the focused widget,
    /// which is right for Ctrl+K and wrong for Ctrl+V: the window
    /// would split while somebody was pasting a symbol into a box. The
    /// ones marked false are handled by a controller that checks what
    /// has the keyboard first, and appear here only so that they are
    /// still written down where people look.
    pub global: bool,
}

const fn global(action: &'static str, accels: &'static [&'static str]) -> Binding {
    Binding { action, accels, global: true }
}

const fn careful(action: &'static str, accels: &'static [&'static str]) -> Binding {
    Binding { action, accels, global: false }
}

pub const BINDINGS: &[Binding] = &[
    // Nothing here can be typed into a box, so the application can own
    // them outright.
    global("win.find", &["<Ctrl>k", "<Ctrl>f"]),
    global("win.watchlist", &["<Ctrl>b"]),
    global("win.preferences", &["<Ctrl>comma"]),
    global("chart.indicators", &["<Ctrl>i"]),
    // A letter, because Shift and punctuation together is a trap. GTK
    // matches the keyval the key produces *after* modifiers, and shifting
    // the comma key does not produce a comma: it gives "<" on a US layout
    // and ";" on a Spanish one, so `<Ctrl><Shift>comma` matched on neither
    // and chart settings had no key at all. Naming the shifted keyval
    // instead only moves which layouts it is broken on. Punctuation can be
    // bound here, but only bare, the way `question` below is.
    global("chart.settings", &["<Ctrl><Shift>s"]),
    // Shift as well, because Ctrl+G on its own is "find the next one" to
    // everything with a box you can type in, and this window has one.
    global("chart.grid", &["<Ctrl><Shift>g"]),
    // O, not P: printing a chart is a thing this will grow, and the key
    // everything else on the desktop prints with has to still be free when it
    // does. A screenshot of the chartbook is the same key with Shift, the way
    // closing one already is.
    global("chart.screenshot", &["<Ctrl>o"]),
    global("win.screenshot", &["<Ctrl><Shift>o"]),
    // Alt with a letter is the drawing tools' register: a line and a
    // rectangle. Resetting the view used to be Alt+R and gave the key up:
    // a tool is reached for many times an hour and a reset once. Escape
    // already means "back to the chart", and Ctrl with it means all the way
    // back, to how the chart opens.
    global("chart.draw-line", &["<Alt>l"]),
    global("chart.draw-hline", &["<Alt>h"]),
    global("chart.draw-arrow", &["<Alt>a"]),
    global("chart.draw-zigzag", &["<Alt>z"]),
    global("chart.draw-rect", &["<Alt>r"]),
    global("chart.draw-ellipse", &["<Alt>c"]),
    global("chart.draw-text", &["<Alt>t"]),
    // The bar itself is a panel, so its key is the watchlist's neighbour:
    // Ctrl+B for the rail on the right, Ctrl+D for the tools on the left.
    global("win.drawing-tools", &["<Ctrl>d"]),
    global("chart.reset-view", &["<Ctrl>Escape"]),
    global("chart.split-h", &["<Ctrl>h"]),
    global("chart.maximize", &["<Ctrl>m"]),
    global("win.new-chartbook", &["<Ctrl>n"]),
    global("win.rename-chartbook", &["<Ctrl><Shift>r"]),
    // Ctrl+X closes a chart, so the chartbook holding it is the same
    // key with Shift. Safe to own outright: Shift+X is nobody's cut.
    global("win.close-chartbook", &["<Ctrl><Shift>x"]),
    // Paste and cut. The keys are the keys; what changes is whether
    // the keyboard is in something you can type into.
    careful("chart.split-v", &["<Ctrl>v"]),
    careful("chart.close", &["<Ctrl>x"]),
    // A bare key, which an entry has to see first.
    careful("win.shortcuts", &["question"]),
];

/// The keys no action owns, for saying so where somebody is looking.
///
/// The table above is addressed by action, which these have nothing to be
/// addressed by: a key controller reads them, or GTK does, and no `GAction` is
/// ever activated. Ctrl+N is the window's new-chartbook until the keyboard is
/// in the rail, where it means a symbol instead. Ctrl+Shift+N and Ctrl+Shift+I
/// each serve one list, and an application accelerator is owned everywhere —
/// taking a key from the whole app to serve one list is not a trade worth
/// making. Delete and F10 are what GTK and the desktop already call them.
///
/// They are written down here all the same, because a button that spells its
/// own key is a button that goes on spelling the old one.
pub const ADD_SYMBOL: &str = "<Ctrl>n";
pub const ADD_SECTION: &str = "<Ctrl><Shift>n";
pub const REMOVE_SYMBOL: &str = "Delete";
pub const ADD_INDICATOR: &str = "<Ctrl><Shift>i";
pub const MAIN_MENU: &str = "F10";

/// Hand the application everything it is safe to own.
pub fn install(app: &adw::Application) {
    for binding in BINDINGS.iter().filter(|binding| binding.global) {
        app.set_accels_for_action(binding.action, binding.accels);
    }
}

/// The chords a label being typed on the chart needs for itself.
///
/// Bold and italic, which is what those two keys mean in every application
/// that has text in it, and what they have to mean here while there is a
/// caret in a label.
const TEXT_EDITING_CHORDS: &[&str] = &["<Ctrl>b", "<Ctrl>i"];

/// Lend those chords to the text editor, or take them back.
///
/// An application accelerator is owned everywhere: GTK activates it at the
/// window, on the way down, before the widget with the keyboard is offered
/// the key at all. So a controller on the editor cannot win Ctrl+B from the
/// rail by being closer to the hand — Ctrl+B simply opened the rail, with
/// the caret still blinking in the label. The only way the editor gets them
/// is for the accelerators not to be there while it is open, which is also
/// the honest description of what is true: while you are typing a label,
/// Ctrl+B is bold.
///
/// Driven off the same table the accelerators are installed from, so a chord
/// moved to another action goes on being lent to the editor, and one of
/// these taken off an action stops being lent without anybody remembering to
/// come back here.
pub fn lend_to_text_editor(app: &adw::Application, lend: bool) {
    for binding in BINDINGS.iter().filter(|binding| binding.global) {
        if !binding.accels.iter().any(|accel| TEXT_EDITING_CHORDS.contains(accel)) {
            continue;
        }
        match lend {
            // Only the lent chord goes; an action with another accelerator
            // keeps it, so Ctrl+K still finds a symbol while Ctrl+F is away.
            true => {
                let left: Vec<&str> = binding
                    .accels
                    .iter()
                    .copied()
                    .filter(|accel| !TEXT_EDITING_CHORDS.contains(accel))
                    .collect();
                app.set_accels_for_action(binding.action, &left);
            }
            false => app.set_accels_for_action(binding.action, binding.accels),
        }
    }
}

fn binding(action: &str) -> Option<&'static Binding> {
    BINDINGS.iter().find(|binding| binding.action == action)
}

/// The first accelerator for an action, in GTK's syntax.
pub fn accel(action: &str) -> Option<&'static str> {
    binding(action).and_then(|binding| binding.accels.first().copied())
}

/// An accelerator written the way this desktop writes it: "Ctrl+K".
///
/// One place does this, so that every row and every button says a key in the
/// same words — and in the words of the machine it is running on rather than
/// the ones somebody typed into a format string.
pub fn accel_label(accel: &str) -> Option<String> {
    let (key, mods) = gtk::accelerator_parse(accel)?;
    let label = gtk::accelerator_get_label(key, mods);
    (!label.is_empty()).then(|| label.to_string())
}

/// The same, for an action the table names.
pub fn label(action: &str) -> Option<String> {
    accel_label(accel(action)?)
}

/// A menu row that says what it does and what key does it.
///
/// The `accel` attribute is what a GTK popover menu draws down the
/// right-hand side, and it draws it whether or not the application
/// registered the key — which is the only way to show a shortcut that
/// has to be handled more carefully than an accelerator can be.
pub fn item(label: &str, action: &str) -> gio::MenuItem {
    match accel(action) {
        Some(accel) => item_with_key(label, action, accel),
        None => gio::MenuItem::new(Some(label), Some(action)),
    }
}

/// The same, for a row whose key the table has no name to give — one of the
/// constants above, on an action that lives only as long as the menu it was
/// built for.
pub fn item_with_key(label: &str, action: &str, accel: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(label), Some(action));
    item.set_attribute_value("accel", Some(&accel.to_variant()));
    item
}

/// Add one to a menu.
pub fn append(menu: &gio::Menu, label: &str, action: &str) {
    menu.append_item(&item(label, action));
}

/// Add one that brings its own key.
pub fn append_with_key(menu: &gio::Menu, label: &str, action: &str, accel: &str) {
    menu.append_item(&item_with_key(label, action, accel));
}

/// "Watchlist (Ctrl+B)", for a button that has no room to say more.
pub fn tooltip(text: &str, action: &str) -> String {
    match accel(action) {
        Some(accel) => tooltip_with_key(text, accel),
        None => text.to_string(),
    }
}

/// The same, for a key no action owns.
pub fn tooltip_with_key(text: &str, accel: &str) -> String {
    match accel_label(accel) {
        Some(key) => format!("{text} ({key})"),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binding_is_an_accelerator_gtk_understands() {
        // Parsing one is GTK's job and GTK needs a display to say so.
        // Where there is none this checks the rest, which is still the
        // part that goes wrong.
        let parses = crate::ui::gtk_ready();

        // A typo here is a shortcut that silently does nothing, and a
        // menu row that confidently advertises it.
        for binding in BINDINGS {
            for accel in binding.accels {
                assert!(
                    !parses || gtk::accelerator_parse(*accel).is_some(),
                    "{} cannot be parsed, for {}",
                    accel,
                    binding.action
                );
            }
            assert!(!binding.accels.is_empty(), "{} has no key", binding.action);
            assert!(binding.action.contains('.'), "{} has no action group", binding.action);
        }
    }

    /// The same typo, in the keys the table cannot check for it. A constant
    /// GTK cannot parse spells nothing, and the tooltip then quietly drops the
    /// half somebody hovered for.
    #[test]
    fn the_keys_no_action_owns_are_accelerators_too() {
        if !crate::ui::gtk_ready() {
            return;
        }
        for accel in [ADD_SYMBOL, ADD_SECTION, REMOVE_SYMBOL, ADD_INDICATOR, MAIN_MENU] {
            assert!(accel_label(accel).is_some(), "{accel} spells nothing");
        }
    }

    /// A tooltip says the key after what the button does, whether the key came
    /// from the table or from one of the constants beside it — and says nothing
    /// extra when there is no key, rather than inventing one.
    #[test]
    fn a_tooltip_carries_the_key_it_was_given() {
        if !crate::ui::gtk_ready() {
            return;
        }
        assert_eq!(tooltip("Watchlist", "win.watchlist"), "Watchlist (Ctrl+B)");
        assert_eq!(tooltip_with_key("Add a symbol", ADD_SYMBOL), "Add a symbol (Ctrl+N)");
        assert_eq!(tooltip("Turn into a watchlist", "section.promote"), "Turn into a watchlist");
    }

    #[test]
    fn nothing_claims_a_key_twice() {
        let mut seen: Vec<&str> = Vec::new();
        for binding in BINDINGS {
            for accel in binding.accels {
                assert!(!seen.contains(accel), "{accel} is claimed twice");
                seen.push(accel);
            }
        }
    }

    /// Screenshots went on Ctrl+O so that Ctrl+P stays what it is everywhere
    /// else. Taking it later for anything but printing would be taking it from
    /// printing, which is the one thing it was kept for.
    #[test]
    fn the_printing_keys_are_left_alone() {
        for accel in ["<Ctrl>p", "<Ctrl><Shift>p"] {
            assert!(
                !BINDINGS.iter().any(|binding| binding.accels.contains(&accel)),
                "{accel} is reserved for printing"
            );
        }
    }

    #[test]
    fn what_is_typed_into_a_box_is_never_taken_by_the_application() {
        // The three that would break pasting, cutting, or typing a
        // question mark. If one of these is ever marked global, the
        // bug it causes is somebody else's afternoon.
        for action in ["chart.split-v", "chart.close", "win.shortcuts"] {
            let binding = binding(action).expect(action);
            assert!(!binding.global, "{action} must stay out of the accelerator table");
        }
    }

    /// Nothing is typed into a box with Ctrl+Shift+G, so the gridlines are
    /// the application's to own — and what it owns is a letter rather than a
    /// shifted punctuation key, for the reason the table says above.
    #[test]
    fn the_gridlines_are_an_accelerator_the_application_owns() {
        let binding = binding("chart.grid").expect("chart.grid");
        assert!(binding.global, "the grid key has to work over a chart");
        assert_eq!(binding.accels, ["<Ctrl><Shift>g"]);
    }
}
