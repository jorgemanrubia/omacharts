//! Menus of choices, drawn the way GTK draws every other menu in the app.
//!
//! A list of flat buttons in a popover looks like a menu until it sits next
//! to one: tighter rows, its own hover, its own mark for the chosen one, and
//! no keyboard of its own. These give a `MenuButton` a real `PopoverMenu`
//! from a model instead, with actions of its own under `prefix`, so a menu
//! needs nothing from the window's action groups and two of them never share
//! a name.

use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};

/// One row of an action menu: what it says, whether it can be picked, and
/// what picking it does.
pub struct Item {
    pub label: String,
    pub enabled: bool,
    pub picked: Rc<dyn Fn()>,
}

impl Item {
    pub fn new(label: impl Into<String>, picked: impl Fn() + 'static) -> Item {
        Item { label: label.into(), enabled: true, picked: Rc::new(picked) }
    }

    /// The same, greyed out when it cannot be picked now.
    pub fn enabled(mut self, enabled: bool) -> Item {
        self.enabled = enabled;
        self
    }
}

/// Give `button` a menu of `items`, each an action of its own.
pub fn actions(button: &gtk::MenuButton, prefix: &str, items: Vec<Item>) {
    let group = gio::SimpleActionGroup::new();
    let menu = gio::Menu::new();
    for (at, item) in items.into_iter().enumerate() {
        let name = format!("item-{at}");
        let action = gio::SimpleAction::new(&name, None);
        action.set_enabled(item.enabled);
        let picked = item.picked;
        action.connect_activate(move |_, _| picked());
        group.add_action(&action);
        menu.append(Some(&item.label), Some(&format!("{prefix}.{name}")));
    }
    insert(button, prefix, &group);
    attach(button, &menu, None);
}

/// Give `button` a menu of one-of-several `choices`, each a label and the
/// key it stands for, with GTK's radio mark beside the one `current` names.
/// `current` is asked every time the menu opens, not once now: what is in
/// force changes underneath a menu that lives as long as its button.
pub fn choice(
    button: &gtk::MenuButton,
    prefix: &str,
    choices: Vec<(String, String)>,
    current: impl Fn() -> String + 'static,
    chosen: impl Fn(&str) + 'static,
) {
    let current: Rc<dyn Fn() -> String> = Rc::new(current);
    let group = gio::SimpleActionGroup::new();
    let action = gio::SimpleAction::new_stateful("pick", Some(glib::VariantTy::STRING), &current().to_variant());
    action.connect_activate(move |action, target| {
        let Some(key) = target.and_then(|target| target.get::<String>()) else { return };
        action.set_state(&key.to_variant());
        chosen(&key);
    });
    group.add_action(&action);
    let menu = gio::Menu::new();
    for (label, key) in choices {
        let item = gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(Some(&format!("{prefix}.pick")), Some(&key.to_variant()));
        menu.append_item(&item);
    }
    insert(button, prefix, &group);
    attach(button, &menu, Some(Rc::new(move || action.set_state(&current().to_variant()))));
}

/// The group on the button, and kept where a test can ask it: GTK has no
/// way to read back a group once inserted.
fn insert(button: &gtk::MenuButton, prefix: &str, group: &gio::SimpleActionGroup) {
    button.insert_action_group(prefix, Some(group));
    // SAFETY: stored and read back as the same type, under a key only this
    // module uses.
    unsafe { button.set_data(GROUP, group.clone()) };
}

const GROUP: &str = "omacharts-menu-group";

#[cfg(test)]
fn group_of(button: &gtk::MenuButton) -> Option<gio::SimpleActionGroup> {
    // SAFETY: only `insert` writes this key, with this type.
    unsafe { button.data::<gio::SimpleActionGroup>(GROUP).map(|group| group.as_ref().clone()) }
}

/// The same choice as a menu dropped from `anchor` rather than from a
/// button of its own: for a chip that is drawn as part of something else.
/// The caller pops it, and unparents it once it has closed.
pub fn drop_choice(
    anchor: &impl IsA<gtk::Widget>,
    prefix: &str,
    choices: Vec<(String, String)>,
    current: &str,
    chosen: impl Fn(&str) + 'static,
) -> gtk::PopoverMenu {
    let group = gio::SimpleActionGroup::new();
    let action = gio::SimpleAction::new_stateful("pick", Some(glib::VariantTy::STRING), &current.to_variant());
    action.connect_activate(move |action, target| {
        let Some(key) = target.and_then(|target| target.get::<String>()) else { return };
        action.set_state(&key.to_variant());
        chosen(&key);
    });
    group.add_action(&action);
    let menu = gio::Menu::new();
    for (label, key) in choices {
        let item = gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(Some(&format!("{prefix}.pick")), Some(&key.to_variant()));
        menu.append_item(&item);
    }
    anchor.as_ref().insert_action_group(prefix, Some(&group));
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.set_has_arrow(false);
    popover.set_position(gtk::PositionType::Bottom);
    popover.set_parent(anchor);
    popover
}

/// Put `menu` on `button` the way every menu in the app is put there: no
/// arrow, dropped below. For a button that builds its model as it opens,
/// call it from there; a new model is a new popover, dressed afresh.
pub fn set_model(button: &gtk::MenuButton, menu: &impl IsA<gio::MenuModel>) {
    button.set_menu_model(Some(menu));
    if let Some(popover) = button.popover() {
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Bottom);
    }
}

/// The model on the button, dressed as the app's other menus are, and the
/// button's tooltip kept off the menu while it is open.
fn attach(button: &gtk::MenuButton, menu: &gio::Menu, opening: Option<Rc<dyn Fn()>>) {
    set_model(button, menu);
    let Some(popover) = button.popover() else { return };
    let shut = button.downgrade();
    popover.connect_closed(move |_| {
        if let Some(button) = shut.upgrade() {
            button.set_has_tooltip(button.tooltip_text().is_some() || button.tooltip_markup().is_some());
        }
    });
    let open = button.downgrade();
    let opened = move || {
        if let Some(button) = open.upgrade() {
            button.set_has_tooltip(false);
            button.trigger_tooltip_query();
        }
        if let Some(opening) = opening.as_ref() {
            opening();
        }
    };
    let opened: Rc<dyn Fn()> = Rc::new(opened);
    // SAFETY: stored and read back as the same type, under a key only this
    // module uses.
    unsafe { button.set_data(OPENING, opened.clone()) };
    popover.connect_show(move |_| opened());
}

const OPENING: &str = "omacharts-menu-opening";

/// What opening the menu does, without opening it: showing a popover needs
/// a mapped window around its button, which a test's bare button has not.
#[cfg(test)]
fn open(button: &gtk::MenuButton) {
    // SAFETY: only `attach` writes this key, with this type.
    let opened = unsafe { button.data::<Rc<dyn Fn()>>(OPENING).map(|opened| opened.as_ref().clone()) };
    opened.expect("a menu")();
}

/// The labels a button's menu shows, in order, and whether each can be
/// picked now: what a test asks of it.
#[cfg(test)]
pub fn rows(button: &gtk::MenuButton) -> Vec<(String, bool)> {
    let Some(model) = button.menu_model() else { return Vec::new() };
    (0..model.n_items())
        .map(|at| {
            let label = model
                .item_attribute_value(at, "label", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>())
                .unwrap_or_default();
            let action = model
                .item_attribute_value(at, "action", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>())
                .unwrap_or_default();
            let enabled = action
                .split_once('.')
                .and_then(|(_, name)| Some(group_of(button)?.is_action_enabled(name)))
                .unwrap_or(true);
            (label, enabled)
        })
        .collect()
}

/// Pick a button's menu row by its label, as a click on it would.
#[cfg(test)]
pub fn pick(button: &gtk::MenuButton, label: &str) {
    let model = button.menu_model().expect("a menu");
    let at = (0..model.n_items())
        .find(|at| {
            model.item_attribute_value(*at, "label", Some(glib::VariantTy::STRING)).and_then(|value| value.get::<String>()).as_deref()
                == Some(label)
        })
        .unwrap_or_else(|| panic!("no {label:?} in the menu"));
    let action = model
        .item_attribute_value(at, "action", Some(glib::VariantTy::STRING))
        .and_then(|value| value.get::<String>())
        .expect("an action");
    let target = model.item_attribute_value(at, "target", None);
    button.activate_action(&action, target.as_ref()).expect("the action");
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn an_action_menu_runs_the_row_picked_and_greys_the_ones_that_cannot_be() {
        crate::ui::on_gtk(|| {
            let button = gtk::MenuButton::new();
            let picked = Rc::new(RefCell::new(Vec::new()));
            let (first, second) = (picked.clone(), picked.clone());
            actions(
                &button,
                "test",
                vec![
                    Item::new("One", move || first.borrow_mut().push("one")),
                    Item::new("Two", move || second.borrow_mut().push("two")).enabled(false),
                ],
            );
            assert!(button.popover().and_downcast::<gtk::PopoverMenu>().is_some(), "a native menu");
            assert_eq!(rows(&button), vec![("One".to_string(), true), ("Two".to_string(), false)]);
            pick(&button, "One");
            assert_eq!(*picked.borrow(), vec!["one"]);
        });
    }

    #[test]
    fn a_choice_menu_marks_what_is_in_force_each_time_it_opens() {
        crate::ui::on_gtk(|| {
            let button = gtk::MenuButton::new();
            let now = Rc::new(RefCell::new("a".to_string()));
            let chosen = Rc::new(RefCell::new(String::new()));
            let (reading, writing) = (now.clone(), chosen.clone());
            choice(
                &button,
                "test",
                vec![("A".into(), "a".into()), ("B".into(), "b".into())],
                move || reading.borrow().clone(),
                move |key| *writing.borrow_mut() = key.to_string(),
            );
            let state = || group_of(&button).and_then(|group| group.action_state("pick")).and_then(|state| state.get::<String>());
            assert_eq!(state().as_deref(), Some("a"));
            *now.borrow_mut() = "b".into();
            open(&button);
            assert_eq!(state().as_deref(), Some("b"), "asked again when it opens");
            pick(&button, "A");
            assert_eq!(*chosen.borrow(), "a");
        });
    }
}
