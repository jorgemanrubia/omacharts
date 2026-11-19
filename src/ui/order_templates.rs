//! The Order templates dialog: the named brackets a chart sends its orders
//! with, each a name, its targets and its stops.
//!
//! Opened from Preferences → Trading, from a chart's Order template part
//! and from its menu. The first page lists the templates in the order
//! Ctrl+Shift and a digit counts them in, dragged into another order by
//! their handles; a row opens its template on a page of its own. Every
//! change is saved as it is made, as the preferences are, and handed to
//! `changed` so the charts show it at once. What a template is and how it
//! becomes an order's exits is [`crate::trading::order_templates`]; this
//! only edits the list.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::store::Store;
use crate::trading::order_templates::{self as templates, Distance, Exit, Group, Mode, OrderTemplate, Size};
use crate::ui::drop_line;

pub const TITLE: &str = "Order templates";

struct Editor {
    store: Rc<Store>,
    changed: Rc<dyn Fn()>,
    nav: adw::NavigationView,
    list: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
    templates: RefCell<Vec<OrderTemplate>>,
    /// What 1R is worth, which money and a stop's risk start at.
    r: f64,
}

/// Open the dialog over `parent`.
pub fn present(parent: &impl IsA<gtk::Widget>, store: Rc<Store>, changed: Rc<dyn Fn()>) -> adw::Dialog {
    let dialog = build(store, changed);
    dialog.present(Some(parent));
    dialog
}

/// The dialog, not yet shown: for [`present`] and the tests.
fn build(store: Rc<Store>, changed: Rc<dyn Fn()>) -> adw::Dialog {
    let nav = adw::NavigationView::new();
    let list = adw::PreferencesGroup::new();
    let editor = Rc::new(Editor {
        templates: RefCell::new(templates::load(&store)),
        r: crate::trading::chart_pnl::r_amount(&store),
        store,
        changed,
        nav: nav.clone(),
        list: list.clone(),
        rows: RefCell::new(Vec::new()),
    });

    let page = adw::PreferencesPage::new();
    page.add(&list);
    let add_group = adw::PreferencesGroup::new();
    let add = gtk::Button::with_label("Add a template");
    add.set_halign(gtk::Align::Start);
    let adding = editor.clone();
    add.connect_clicked(move |_| {
        let fresh = {
            let mut list = adding.templates.borrow_mut();
            let fresh = templates::new_template(&list, adding.r);
            list.push(fresh.clone());
            fresh
        };
        adding.save();
        rebuild(&adding);
        open(&adding, fresh.id);
    });
    add_group.add(&add);
    page.add(&add_group);
    rebuild(&editor);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    let root = adw::NavigationPage::new(&toolbar, TITLE);
    nav.add(&root);

    let dialog = adw::Dialog::new();
    dialog.set_title(TITLE);
    dialog.set_content_width(480);
    dialog.set_content_height(620);
    dialog.set_child(Some(&nav));
    // The editor lives as long as the dialog does.
    let kept = editor.clone();
    dialog.connect_closed(move |_| {
        let _ = &kept;
    });
    dialog
}

impl Editor {
    /// Save the list as it stands, and let the charts know.
    fn save(&self) {
        templates::save(&self.store, &self.templates.borrow());
        (self.changed)();
    }

    /// Change the template `id`, save, and say whether there was one.
    fn edit(&self, id: u32, change: impl FnOnce(&mut OrderTemplate)) -> bool {
        let found = match self.templates.borrow_mut().iter_mut().find(|template| template.id == id) {
            Some(template) => {
                change(template);
                true
            }
            None => false,
        };
        if found {
            self.save();
        }
        found
    }

    fn template(&self, id: u32) -> Option<OrderTemplate> {
        templates::by_id(&self.templates.borrow(), id).cloned()
    }
}

/// The list's rows again, from the templates as they stand.
fn rebuild(editor: &Rc<Editor>) {
    for row in editor.rows.borrow_mut().drain(..) {
        editor.list.remove(&row);
    }
    let list = editor.templates.borrow().clone();
    if list.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("No order templates");
        row.add_css_class("dim-label");
        editor.list.add(&row);
        editor.rows.borrow_mut().push(row.upcast());
        return;
    }
    for (at, template) in list.iter().enumerate() {
        let row = template_row(editor, at, template);
        editor.list.add(&row);
        editor.rows.borrow_mut().push(row.upcast());
    }
}

/// A template in the list: its handle, its name, what it holds, its key,
/// its bin, and the way into it.
fn template_row(editor: &Rc<Editor>, at: usize, template: &OrderTemplate) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(&template.name);
    row.set_subtitle(&template.summary());
    row.set_activatable(true);

    let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
    handle.add_css_class("dim-label");
    handle.set_tooltip_text(Some("Drag to reorder"));
    row.add_prefix(&handle);

    if at < 9 {
        let key = gtk::Label::new(Some(&format!("Ctrl+Shift+{}", at + 1)));
        key.add_css_class("dim-label");
        key.add_css_class("caption");
        row.add_suffix(&key);
    }
    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_valign(gtk::Align::Center);
    remove.set_tooltip_text(Some(&format!("Remove {}", template.name)));
    let removing = editor.clone();
    let (id, name) = (template.id, template.name.clone());
    remove.connect_clicked(move |button| confirm_remove(&removing, button, id, &name));
    row.add_suffix(&remove);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));

    let opening = editor.clone();
    row.connect_activated(move |_| open(&opening, id));

    // Dragged by id rather than by position: the list is rebuilt under
    // the drag.
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::MOVE);
    let payload = id.to_string();
    source.connect_prepare(move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&payload.to_value())));
    row.add_controller(source);
    let places = {
        let editor = editor.clone();
        move |value: &glib::Value| {
            let moving = value.get::<String>().ok()?.parse::<u32>().ok()?;
            let list = editor.templates.borrow();
            Some((list.iter().position(|t| t.id == moving)?, list.iter().position(|t| t.id == id)?))
        }
    };
    let landing = places.clone();
    let moving = editor.clone();
    let target = drop_line::target(
        &row,
        move |value| landing(value).and_then(|(from, to)| drop_line::edge(from, to)),
        move |value| {
            let Some((from, to)) = places(value) else { return false };
            if from == to {
                return false;
            }
            templates::move_to(&mut moving.templates.borrow_mut(), from, to);
            moving.save();
            let again = moving.clone();
            glib::idle_add_local_once(move || rebuild(&again));
            true
        },
    );
    row.add_controller(target);
    row
}

fn confirm_remove(editor: &Rc<Editor>, over: &impl IsA<gtk::Widget>, id: u32, name: &str) {
    let alert = adw::AlertDialog::new(Some(&format!("Remove {name}?")), None);
    alert.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
    alert.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");
    let removing = editor.clone();
    alert.connect_response(None, move |_, response| {
        if response != "remove" {
            return;
        }
        removing.templates.borrow_mut().retain(|template| template.id != id);
        removing.save();
        rebuild(&removing);
    });
    alert.present(Some(over));
}

/// The template `id` on a page of its own.
fn open(editor: &Rc<Editor>, id: u32) {
    let Some(template) = editor.template(id) else { return };
    let page = adw::PreferencesPage::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    let nav_page = adw::NavigationPage::new(&toolbar, &template.name);

    let naming = adw::PreferencesGroup::new();
    let name = adw::EntryRow::new();
    name.set_title("Name");
    name.set_text(&template.name);
    let renaming = editor.clone();
    let titled = nav_page.clone();
    name.connect_changed(move |row| {
        let text = row.text().trim().to_string();
        match text.is_empty() {
            true => row.add_css_class("error"),
            false => row.remove_css_class("error"),
        }
        if text.is_empty() {
            return;
        }
        titled.set_title(&text);
        renaming.edit(id, |template| template.name = text);
        rebuild(&renaming);
    });
    naming.add(&name);
    page.add(&naming);

    let targets = ExitGroup::new(editor, id, Group::Targets);
    let stops = ExitGroup::new(editor, id, Group::Stops);
    page.add(&targets.group);
    page.add(&stops.group);
    editor.nav.push(&nav_page);
}

/// The targets, or the stops, of the template open.
struct ExitGroup {
    editor: Rc<Editor>,
    id: u32,
    which: Group,
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
}

impl ExitGroup {
    fn new(editor: &Rc<Editor>, id: u32, which: Group) -> Rc<ExitGroup> {
        let group = adw::PreferencesGroup::new();
        let (title, tip) = match which {
            Group::Targets => ("Targets", "Add a target"),
            Group::Stops => ("Stops", "Add a stop"),
        };
        group.set_title(title);
        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.add_css_class("flat");
        add.set_valign(gtk::Align::Center);
        add.set_tooltip_text(Some(tip));
        group.set_header_suffix(Some(&add));
        let this = Rc::new(ExitGroup { editor: editor.clone(), id, which, group, rows: RefCell::new(Vec::new()) });
        let adding = this.clone();
        add.connect_clicked(move |_| adding.add());
        this.rebuild();
        this
    }

    fn exits(&self) -> Vec<Exit> {
        self.editor.template(self.id).map(|template| template.exits(self.which).clone()).unwrap_or_default()
    }

    fn change(self: &Rc<Self>, change: impl FnOnce(&mut Vec<Exit>), again: bool) {
        let which = self.which;
        self.editor.edit(self.id, |template| change(template.exits_mut(which)));
        rebuild(&self.editor);
        if again {
            self.rebuild();
        }
    }

    /// A new exit at the end, like the last one and further, the last
    /// taking what is left and the one before it half.
    fn add(self: &Rc<Self>) {
        let r = self.editor.r;
        let stop = self.which == Group::Stops;
        self.change(
            |exits| {
                let distance = match exits.last().map(|exit| exit.distance) {
                    None if stop => Distance::Risk(r),
                    None => Distance::Risk(2.0),
                    Some(Distance::Ticks(ticks)) => Distance::Ticks(ticks.saturating_mul(2).max(1)),
                    Some(Distance::Money(money)) => Distance::Money(money * 2.0),
                    Some(Distance::Risk(risk)) if stop => Distance::Risk(risk * 2.0),
                    Some(Distance::Risk(multiple)) => Distance::Risk(multiple + 1.0),
                };
                if let Some(last) = exits.last_mut() {
                    last.size = Size::Share(1, 2);
                }
                exits.push(Exit { distance, size: Size::Share(1, 2) });
            },
            true,
        );
    }

    fn rebuild(self: &Rc<Self>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let exits = self.exits();
        if exits.is_empty() {
            let row = adw::ActionRow::new();
            row.set_title(match self.which {
                Group::Targets => "No targets",
                Group::Stops => "No stops",
            });
            row.add_css_class("dim-label");
            self.group.add(&row);
            self.rows.borrow_mut().push(row.upcast());
            return;
        }
        for (at, exit) in exits.iter().enumerate() {
            let row = self.exit_row(at, *exit, exits.len());
            self.group.add(&row);
            self.rows.borrow_mut().push(row.upcast());
        }
    }

    /// One exit: its handle with another to order it against, how far it
    /// is said, the amount, how much it closes, and its bin.
    fn exit_row(self: &Rc<Self>, at: usize, exit: Exit, count: usize) -> adw::PreferencesRow {
        let stop = self.which == Group::Stops;
        let row = adw::PreferencesRow::new();
        row.set_activatable(false);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.set_margin_top(8);
        line.set_margin_bottom(8);
        line.set_margin_start(12);
        line.set_margin_end(8);

        if count > 1 {
            let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
            handle.add_css_class("dim-label");
            handle.set_tooltip_text(Some("Drag to reorder"));
            line.append(&handle);
        }

        let modes = gtk::DropDown::from_strings(&Mode::ALL.map(Mode::label));
        modes.set_valign(gtk::Align::Center);
        modes.set_selected(Mode::ALL.iter().position(|mode| *mode == exit.distance.mode()).unwrap_or(0) as u32);
        line.append(&modes);

        let (prefix, suffix) = units(exit.distance.mode(), stop);
        if let Some(prefix) = prefix {
            let label = gtk::Label::new(Some(prefix));
            label.add_css_class("dim-label");
            line.append(&label);
        }
        let spin = amount_spin(exit.distance, stop);
        line.append(&spin);
        if let Some(suffix) = suffix {
            let label = gtk::Label::new(Some(suffix));
            label.add_css_class("dim-label");
            line.append(&label);
        }

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        line.append(&spacer);

        // The last takes what is left: there is nothing to say for it.
        if at + 1 == count {
            let rest = gtk::Label::new(Some(if count == 1 { "All" } else { "Rest" }));
            rest.add_css_class("dim-label");
            rest.set_tooltip_text(Some(if count == 1 { "Closes the whole order" } else { "Closes what the others leave" }));
            line.append(&rest);
        } else {
            line.append(&self.size_button(at, exit.size));
        }

        let remove = gtk::Button::from_icon_name("user-trash-symbolic");
        remove.add_css_class("flat");
        remove.set_valign(gtk::Align::Center);
        remove.set_tooltip_text(Some("Remove"));
        let removing = self.clone();
        remove.connect_clicked(move |_| {
            let again = removing.clone();
            glib::idle_add_local_once(move || {
                again.change(
                    |exits| {
                        if at < exits.len() {
                            exits.remove(at);
                        }
                    },
                    true,
                )
            });
        });
        line.append(&remove);
        row.set_child(Some(&line));

        let valued = self.clone();
        spin.connect_value_changed(move |spin| {
            let value = spin.value();
            valued.change(
                |exits| {
                    if let Some(exit) = exits.get_mut(at) {
                        exit.distance = with_value(exit.distance, value);
                    }
                },
                false,
            );
        });
        let moded = self.clone();
        let r = self.editor.r;
        modes.connect_selected_notify(move |modes| {
            let Some(mode) = Mode::ALL.get(modes.selected() as usize).copied() else { return };
            let again = moded.clone();
            glib::idle_add_local_once(move || {
                again.change(
                    |exits| {
                        if let Some(exit) = exits.get_mut(at)
                            && exit.distance.mode() != mode
                        {
                            exit.distance = starting(mode, stop, r);
                        }
                    },
                    true,
                )
            });
        });

        if count > 1 {
            self.draggable(&row, at);
        }
        row
    }

    /// How much an exit closes: a share of the order, or a number.
    fn size_button(self: &Rc<Self>, at: usize, size: Size) -> gtk::MenuButton {
        let button = gtk::MenuButton::new();
        button.set_label(&size.text());
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some("Quantity"));
        let popover = gtk::Popover::new();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 10);
        body.set_margin_top(6);
        body.set_margin_bottom(6);
        body.set_margin_start(6);
        body.set_margin_end(6);

        let shares = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        shares.add_css_class("linked");
        let mut first: Option<gtk::ToggleButton> = None;
        for share in Size::SHARES {
            let toggle = gtk::ToggleButton::with_label(&share.text());
            toggle.set_active(share == size);
            if let Some(first) = &first {
                toggle.set_group(Some(first));
            } else {
                first = Some(toggle.clone());
            }
            let sizing = self.clone();
            let (shown, closing) = (button.clone(), popover.clone());
            toggle.connect_toggled(move |toggle| {
                if !toggle.is_active() {
                    return;
                }
                shown.set_label(&share.text());
                closing.popdown();
                sizing.set_size(at, share);
            });
            shares.append(&toggle);
        }
        body.append(&shares);

        let fixed = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let word = gtk::Label::new(Some("Fixed"));
        word.set_hexpand(true);
        word.set_xalign(0.0);
        fixed.append(&word);
        let count = gtk::SpinButton::with_range(1.0, 10_000.0, 1.0);
        count.set_digits(0);
        count.set_value(match size {
            Size::Fixed(n) => f64::from(n),
            Size::Share(..) => 1.0,
        });
        fixed.append(&count);
        body.append(&fixed);
        let sizing = self.clone();
        let (shown, untoggled) = (button.clone(), first);
        count.connect_value_changed(move |count| {
            let n = count.value() as u32;
            shown.set_label(&n.to_string());
            if let Some(first) = &untoggled {
                // The shares let go: a number is none of them.
                let mut toggle = Some(first.clone().upcast::<gtk::Widget>());
                while let Some(widget) = toggle {
                    if let Some(button) = widget.downcast_ref::<gtk::ToggleButton>() {
                        button.set_active(false);
                    }
                    toggle = widget.next_sibling();
                }
            }
            sizing.set_size(at, Size::Fixed(n.max(1)));
        });
        popover.set_child(Some(&body));
        button.set_popover(Some(&popover));
        button
    }

    fn set_size(self: &Rc<Self>, at: usize, size: Size) {
        self.change(
            |exits| {
                if let Some(exit) = exits.get_mut(at) {
                    exit.size = size;
                }
            },
            false,
        );
    }

    /// Dragged by its place in the group onto another of the same group.
    fn draggable(self: &Rc<Self>, row: &adw::PreferencesRow, at: usize) {
        let tag = match self.which {
            Group::Targets => "t",
            Group::Stops => "s",
        };
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let payload = format!("{tag}{at}");
        source.connect_prepare(move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&payload.to_value())));
        row.add_controller(source);
        let from = move |value: &glib::Value| -> Option<usize> { value.get::<String>().ok()?.strip_prefix(tag)?.parse().ok() };
        let landing = from;
        let moving = self.clone();
        let target = drop_line::target(
            row,
            move |value| landing(value).and_then(|from| drop_line::edge(from, at)),
            move |value| {
                let Some(from) = from(value).filter(|from| *from != at) else { return false };
                let again = moving.clone();
                glib::idle_add_local_once(move || {
                    again.change(
                        |exits| {
                            if from < exits.len() {
                                let lifted = exits.remove(from);
                                exits.insert(at.min(exits.len()), lifted);
                            }
                        },
                        true,
                    )
                });
                true
            },
        );
        row.add_controller(target);
    }
}

/// What is written before and after the amount: "$", "ticks", "R",
/// "total".
fn units(mode: Mode, stop: bool) -> (Option<&'static str>, Option<&'static str>) {
    match (mode, stop) {
        (Mode::Ticks, _) => (None, Some("ticks")),
        (Mode::Money, _) => (Some("$"), None),
        (Mode::Risk, true) => (Some("$"), Some("total")),
        (Mode::Risk, false) => (None, Some("R")),
    }
}

/// The amount, stepping as its mode does.
fn amount_spin(distance: Distance, stop: bool) -> gtk::SpinButton {
    let (low, high, step, digits) = match (distance.mode(), stop) {
        (Mode::Ticks, _) => (1.0, 100_000.0, 1.0, 0),
        (Mode::Money, _) | (Mode::Risk, true) => (1.0, 10_000_000.0, 25.0, 0),
        (Mode::Risk, false) => (0.25, 100.0, 0.25, 2),
    };
    let spin = gtk::SpinButton::with_range(low, high, step);
    spin.set_digits(digits);
    spin.set_valign(gtk::Align::Center);
    spin.set_width_chars(6);
    spin.set_value(distance.value());
    spin
}

fn with_value(distance: Distance, value: f64) -> Distance {
    match distance {
        Distance::Ticks(_) => Distance::Ticks(value.round().max(1.0) as u32),
        Distance::Money(_) => Distance::Money(value),
        Distance::Risk(_) => Distance::Risk(value),
    }
}

/// Where an exit said another way starts: 8 ticks, what 1R is worth, or
/// 2R for a target.
fn starting(mode: Mode, stop: bool, r: f64) -> Distance {
    match (mode, stop) {
        (Mode::Ticks, _) => Distance::Ticks(8),
        (Mode::Money, _) => Distance::Money(r),
        (Mode::Risk, true) => Distance::Risk(r),
        (Mode::Risk, false) => Distance::Risk(2.0),
    }
}

/// The dialog's pages and rows, for the tests.
#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(group: &adw::PreferencesGroup) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![group.clone().upcast::<gtk::Widget>()];
        while let Some(widget) = stack.pop() {
            if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                found.push(format!("{} | {}", row.title(), row.subtitle().unwrap_or_default()));
                continue;
            }
            let mut child = widget.last_child();
            while let Some(c) = child {
                stack.push(c.clone());
                child = c.prev_sibling();
            }
        }
        found
    }

    #[test]
    fn templates_are_added_named_and_listed_in_order() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let told = Rc::new(std::cell::Cell::new(0));
            let counted = told.clone();
            let dialog = build(store.clone(), Rc::new(move || counted.set(counted.get() + 1)));
            let nav = dialog.child().and_downcast::<adw::NavigationView>().unwrap();
            let root = nav.visible_page().unwrap();
            let groups = groups_in(root.upcast_ref());
            assert_eq!(rows_of(&groups[0]), ["No order templates | "]);

            let add = buttons_in(root.upcast_ref()).into_iter().find(|b| b.label().as_deref() == Some("Add a template")).unwrap();
            add.emit_clicked();
            add.emit_clicked();
            let saved = templates::load(&store);
            assert_eq!(saved.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["New template", "New template 2"]);
            assert_eq!(saved[0].summary(), "+2R / −$500 total", "a target at 2R and a stop of what 1R is worth");
            assert!(told.get() >= 2, "every change is handed on");
            assert_eq!(nav.visible_page().unwrap().title(), "New template 2", "a new template opens");
            let groups = groups_in(root.upcast_ref());
            assert_eq!(rows_of(&groups[0]), ["New template | +2R / −$500 total", "New template 2 | +2R / −$500 total"]);
        });
    }

    fn groups_in(widget: &gtk::Widget) -> Vec<adw::PreferencesGroup> {
        let mut found = Vec::new();
        let mut stack = vec![widget.clone()];
        while let Some(widget) = stack.pop() {
            if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
                found.push(group.clone());
                continue;
            }
            let mut child = widget.last_child();
            while let Some(c) = child {
                stack.push(c.clone());
                child = c.prev_sibling();
            }
        }
        found
    }

    fn buttons_in(widget: &gtk::Widget) -> Vec<gtk::Button> {
        let mut found = Vec::new();
        let mut stack = vec![widget.clone()];
        while let Some(widget) = stack.pop() {
            if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                found.push(button.clone());
            }
            let mut child = widget.last_child();
            while let Some(c) = child {
                stack.push(c.clone());
                child = c.prev_sibling();
            }
        }
        found
    }
}
