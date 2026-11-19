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
use gtk::{gio, glib};

use crate::store::Store;
use crate::trading::order_templates::{self as templates, Distance, Exit, Group, Mode, OrderTemplate, Size};
use crate::ui::drop_line;

pub const TITLE: &str = "Order templates";

/// The width the size of an exit is given: its widest preset's.
const SIZE_SLOT: i32 = 150;

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
    // A template starts as one of the shapes traders use, named after it.
    let add_group = adw::PreferencesGroup::new();
    let menu = gio::Menu::new();
    for (at, preset) in templates::Preset::ALL.into_iter().enumerate() {
        menu.append(Some(preset.label()), Some(&format!("templates.add({at})")));
    }
    let add = gtk::MenuButton::new();
    add.set_label("Add a template");
    add.set_always_show_arrow(true);
    add.set_menu_model(Some(&menu));
    add.set_halign(gtk::Align::Start);
    let actions = gio::SimpleActionGroup::new();
    let adding_action = gio::SimpleAction::new("add", Some(glib::VariantTy::INT32));
    let adding = editor.clone();
    adding_action.connect_activate(move |_, at| {
        let Some(preset) = at.and_then(|at| at.get::<i32>()).and_then(|at| templates::Preset::ALL.get(at as usize).copied()) else {
            return;
        };
        let fresh = {
            let mut list = adding.templates.borrow_mut();
            let fresh = preset.template(&list);
            list.push(fresh.clone());
            fresh
        };
        adding.save();
        rebuild(&adding);
        open(&adding, fresh.id, true);
    });
    actions.add_action(&adding_action);
    page.insert_action_group("templates", Some(&actions));
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
    dialog.set_content_width(640);
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
        row.set_subtitle("A template's exits go in with every order from a chart on it");
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
    row.connect_activated(move |_| open(&opening, id, false));

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
    let alert = adw::AlertDialog::new(
        Some(&format!("Remove {name}?")),
        Some("Charts on it go back to No template, and their orders are sent with no exits."),
    );
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
fn open(editor: &Rc<Editor>, id: u32, fresh: bool) {
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
        let taken = renaming
            .templates
            .borrow()
            .iter()
            .any(|other| other.id != id && other.name.eq_ignore_ascii_case(&text));
        match text.is_empty() || taken {
            true => row.add_css_class("error"),
            false => row.remove_css_class("error"),
        }
        row.set_tooltip_text(taken.then_some("Another template has this name"));
        if text.is_empty() || taken {
            return;
        }
        titled.set_title(&text);
        renaming.edit(id, |template| template.name = text);
        rebuild(&renaming);
    });
    naming.add(&name);
    page.add(&naming);

    let columns = Rc::new(Columns::new());
    let targets = ExitGroup::new(editor, id, Group::Targets, &columns);
    let stops = ExitGroup::new(editor, id, Group::Stops, &columns);
    page.add(&targets.group);
    page.add(&stops.group);
    editor.nav.push(&nav_page);
    // A new template is named first; an old one is not taken over by its
    // name.
    let focused = name.clone();
    glib::idle_add_local_once(move || match fresh {
        true => {
            focused.grab_focus();
            focused.select_region(0, -1);
        }
        false => {
            if let Some(root) = focused.root() {
                root.set_focus(None::<&gtk::Widget>);
            }
            focused.select_region(0, 0);
        }
    });
}

/// The columns the exit rows of a template line up in, across both
/// groups: the handle, the name, the mode, the amount, "close" and the
/// size.
struct Columns {
    sizes: [gtk::SizeGroup; 6],
}

impl Columns {
    fn new() -> Columns {
        Columns { sizes: std::array::from_fn(|_| gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal)) }
    }

    fn add(&self, column: usize, widget: &impl IsA<gtk::Widget>) {
        self.sizes[column].add_widget(widget);
    }
}

/// The targets, or the stops, of the template open.
struct ExitGroup {
    editor: Rc<Editor>,
    id: u32,
    which: Group,
    group: adw::PreferencesGroup,
    columns: Rc<Columns>,
    rows: RefCell<Vec<gtk::Widget>>,
    notices: RefCell<Vec<gtk::Widget>>,
}

impl ExitGroup {
    fn new(editor: &Rc<Editor>, id: u32, which: Group, columns: &Rc<Columns>) -> Rc<ExitGroup> {
        let group = adw::PreferencesGroup::new();
        let (title, tip) = match which {
            Group::Targets => ("Targets", "Add target"),
            Group::Stops => ("Stops", "Add stop"),
        };
        group.set_title(title);
        let add = gtk::Button::from_icon_name("list-add-symbolic");
        add.add_css_class("flat");
        add.set_valign(gtk::Align::Center);
        add.set_tooltip_text(Some(tip));
        group.set_header_suffix(Some(&add));
        let this = Rc::new(ExitGroup {
            editor: editor.clone(),
            id,
            which,
            group,
            columns: columns.clone(),
            rows: RefCell::new(Vec::new()),
            notices: RefCell::new(Vec::new()),
        });
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
        match again {
            true => self.rebuild(),
            false => self.notice(),
        }
    }

    /// A new exit at the end, the rest, further than the last: a target
    /// by the step between the last two, or half as far again; a stop as
    /// far as the one above. The one that was the rest closes 1 contract.
    fn add(self: &Rc<Self>) {
        let stop = self.which == Group::Stops;
        self.change(
            |exits| {
                let last = exits.last().map(|exit| exit.distance);
                let before = exits.len().checked_sub(2).and_then(|at| exits.get(at)).map(|exit| exit.distance);
                let distance = match (last, before) {
                    (None, _) => Distance::R(if stop { 1.0 } else { 2.0 }),
                    (Some(last), _) if stop => last,
                    (Some(last), Some(before)) if before.mode() == last.mode() && last.value() > before.value() => {
                        with_value(last, last.value() * 2.0 - before.value())
                    }
                    (Some(last), _) => with_value(last, last.value() * 1.5),
                };
                if let Some(last) = exits.last_mut().filter(|exit| exit.size.is_all()) {
                    last.size = Size::Fixed(1);
                }
                exits.push(Exit { distance, size: Size::ALL });
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
        }
        for (at, exit) in exits.iter().enumerate() {
            let row = self.exit_row(at, *exit, exits.len());
            self.group.add(&row);
            self.rows.borrow_mut().push(row.upcast());
        }
        self.notice();
    }

    /// Under the exits, where their sizes do not match the position:
    /// said, never refused.
    fn notice(self: &Rc<Self>) {
        for row in self.notices.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let notices = templates::fit(&self.exits(), self.which);
        if notices.is_empty() {
            return;
        }
        // One light footer, a line for each, warnings first.
        let row = adw::PreferencesRow::new();
        row.set_activatable(false);
        row.set_focusable(false);
        row.add_css_class("notices");
        let lines = gtk::Box::new(gtk::Orientation::Vertical, 2);
        lines.set_margin_top(8);
        lines.set_margin_bottom(8);
        lines.set_margin_start(12);
        lines.set_margin_end(12);
        for notice in notices {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            let label = gtk::Label::new(Some(&notice.text));
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.add_css_class("caption");
            match notice.warning {
                true => {
                    line.add_css_class("warning");
                    let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
                    icon.set_pixel_size(12);
                    line.append(&icon);
                }
                false => label.add_css_class("dim-label"),
            }
            line.append(&label);
            lines.append(&line);
        }
        row.set_child(Some(&lines));
        self.group.add(&row);
        self.notices.borrow_mut().push(row.upcast());
    }

    /// One exit: its handle, its name, the mode, the amount, what it
    /// closes and its bin, in the columns of the page.
    fn exit_row(self: &Rc<Self>, at: usize, exit: Exit, count: usize) -> adw::PreferencesRow {
        let stop = self.which == Group::Stops;
        let row = adw::PreferencesRow::new();
        row.set_activatable(false);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.set_margin_top(8);
        line.set_margin_bottom(8);
        line.set_margin_start(8);
        line.set_margin_end(8);

        // Its room kept when there is nothing to drag, so the rows line up.
        let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        handle.add_css_class("dim-label");
        if count > 1 {
            handle.set_tooltip_text(Some("Drag to reorder"));
        } else {
            handle.set_opacity(0.0);
            handle.set_sensitive(false);
        }
        self.columns.add(0, &handle);
        line.append(&handle);

        let name = format!("{} {}", if stop { "Stop" } else { "Target" }, at + 1);
        let title = gtk::Label::new(Some(&name));
        title.set_xalign(0.0);
        self.columns.add(1, &title);
        line.append(&title);

        let modes = gtk::DropDown::from_strings(&Mode::ALL.map(Mode::label));
        modes.set_factory(Some(&mode_factory(true)));
        modes.set_list_factory(Some(&mode_factory(false)));
        modes.update_property(&[gtk::accessible::Property::Label(&format!("{name} mode"))]);
        modes.set_valign(gtk::Align::Center);
        modes.set_selected(Mode::ALL.iter().position(|mode| *mode == exit.distance.mode()).unwrap_or(0) as u32);
        self.columns.add(2, &modes);
        line.append(&modes);

        let spin = amount_spin(exit.distance);
        spin.update_property(&[gtk::accessible::Property::Label(&format!("{name} amount"))]);
        self.columns.add(3, &spin);
        line.append(&spin);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        line.append(&spacer);

        let close = gtk::Label::new(Some("close"));
        close.add_css_class("dim-label");
        self.columns.add(4, &close);
        line.append(&close);
        // As wide as "1/2 of position", whatever it says, so that picking
        // a size never moves the page.
        let slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        slot.set_size_request(SIZE_SLOT, -1);
        slot.append(&self.size_button(at, exit.size));
        self.columns.add(5, &slot);
        line.append(&slot);

        let remove = gtk::Button::from_icon_name("user-trash-symbolic");
        remove.add_css_class("flat");
        remove.set_valign(gtk::Align::Center);
        remove.set_tooltip_text(Some("Remove"));
        remove.update_property(&[gtk::accessible::Property::Label(&format!("Remove {}", name.to_lowercase()))]);
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
                            exit.distance = said_as(exit.distance, mode, stop, r);
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

    /// What an exit closes: so many contracts, first; a share of the
    /// position, picked or typed; or the rest, all of it for the first.
    fn size_button(self: &Rc<Self>, at: usize, size: Size) -> gtk::MenuButton {
        let button = gtk::MenuButton::new();
        button.set_label(&size_label(size, at));
        button.set_valign(gtk::Align::Center);
        button.update_property(&[gtk::accessible::Property::Label("Closes")]);
        let popover = gtk::Popover::new();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
        body.set_margin_top(6);
        body.set_margin_bottom(6);
        body.set_margin_start(6);
        body.set_margin_end(6);
        let heading = |text: &str| {
            let label = gtk::Label::new(Some(text));
            label.add_css_class("heading");
            label.set_xalign(0.0);
            label
        };
        let changed = Rc::new(std::cell::Cell::new(false));

        body.append(&heading("Contracts"));
        let count = gtk::SpinButton::with_range(1.0, 999.0, 1.0);
        count.set_digits(0);
        count.set_value(match size {
            Size::Fixed(n) => f64::from(n),
            Size::Share(..) => 1.0,
        });
        count.update_property(&[gtk::accessible::Property::Label("Contracts")]);
        body.append(&count);

        body.append(&heading("Of position"));
        let shares = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        shares.add_css_class("linked");
        // The rest first, "Rest of position", then the shares going down.
        for share in std::iter::once(Size::ALL).chain(Size::SHARES) {
            let text = match share.is_all() {
                true => size_label(share, at),
                false => share.text(),
            };
            let toggle = gtk::ToggleButton::with_label(&text);
            toggle.set_active(share == size);
            toggle.set_focus_on_click(false);
            let (sizing, closing, marked) = (self.clone(), popover.clone(), changed.clone());
            toggle.connect_clicked(move |_| {
                marked.set(true);
                sizing.set_size(at, share);
                closing.popdown();
            });
            shares.append(&toggle);
        }
        body.append(&shares);
        let typed = gtk::Entry::new();
        typed.set_placeholder_text(Some("33% or 0.33"));
        typed.update_property(&[gtk::accessible::Property::Label("Share of position")]);
        if let Size::Share(_, 100) = size {
            typed.set_text(&size.text());
        }
        body.append(&typed);

        let (sizing, closing, marked) = (self.clone(), popover.clone(), changed.clone());
        typed.connect_activate(move |entry| match Size::parse_share(&entry.text()) {
            Ok(share) => {
                entry.remove_css_class("error");
                marked.set(true);
                sizing.set_size(at, share);
                closing.popdown();
            }
            Err(why) => {
                entry.add_css_class("error");
                entry.set_tooltip_text(Some(&why));
            }
        });
        // Contracts take as they are typed or stepped; Enter closes.
        let (sizing, shown, marked) = (self.clone(), button.clone(), changed.clone());
        count.connect_value_changed(move |count| {
            let size = Size::Fixed((count.value() as u32).max(1));
            shown.set_label(&size_label(size, at));
            marked.set(true);
            sizing.set_size(at, size);
        });
        let keys = gtk::EventControllerKey::new();
        let (sizing, closing, counted, marked) = (self.clone(), popover.clone(), count.clone(), changed.clone());
        keys.connect_key_pressed(move |_, key, _, _| {
            if !matches!(key, gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter) {
                return glib::Propagation::Proceed;
            }
            counted.update();
            marked.set(true);
            sizing.set_size(at, Size::Fixed((counted.value() as u32).max(1)));
            closing.popdown();
            glib::Propagation::Stop
        });
        count.add_controller(keys);
        let focused = count.clone();
        popover.connect_show(move |_| {
            focused.grab_focus();
            focused.select_region(0, -1);
        });
        // Drawn again once closed, so the button and the popover say what
        // was picked.
        let redraw = self.clone();
        popover.connect_closed(move |_| {
            if changed.replace(false) {
                let again = redraw.clone();
                glib::idle_add_local_once(move || again.rebuild());
            }
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

/// What an exit's size reads on its button: "1", "2", "1/3 of position",
/// "Rest", or "All" for the first.
fn size_label(size: Size, at: usize) -> String {
    match size {
        Size::Fixed(n) => n.to_string(),
        size if size.is_all() && at == 0 => "All".to_string(),
        size if size.is_all() => "Rest".to_string(),
        share => format!("{} of position", share.text()),
    }
}

/// The mode picker's words: shut, the unit before the amount, "Ticks",
/// "$", "R"; open, the modes by name.
fn mode_factory(shut: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_hexpand(true);
        line.append(&label);
        // Open, the mode picked is checked, as a drop-down's own are.
        if !shut {
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            item.property_expression("selected").bind(&check, "visible", gtk::Widget::NONE);
            line.append(&check);
        }
        item.set_child(Some(&line));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let text = item.item().and_downcast::<gtk::StringObject>().map(|text| text.string());
        let label = item.child().and_then(|line| line.first_child()).and_downcast::<gtk::Label>();
        let (Some(text), Some(label)) = (text, label) else { return };
        label.set_label(if shut && text == Mode::Money.label() { "$" } else { &text });
    });
    factory
}

/// The amount, stepping as its mode does.
fn amount_spin(distance: Distance) -> gtk::SpinButton {
    let (low, high, step, digits) = match distance.mode() {
        Mode::Ticks => (1.0, 100_000.0, 1.0, 0),
        Mode::Money => (1.0, 10_000_000.0, 25.0, 0),
        // A multiple of R, in fractions: 0.5, 1.5.
        Mode::R => (0.05, 100.0, 0.25, 2),
    };
    let spin = gtk::SpinButton::with_range(low, high, step);
    spin.set_digits(digits);
    spin.set_valign(gtk::Align::Center);
    spin.set_width_chars(6);
    spin.set_value(distance.value());
    // R as a person writes it: 1, 1.5, 0.25.
    if distance.mode() == Mode::R {
        spin.connect_output(|spin| {
            spin.set_text(&templates::amount(spin.value()));
            glib::Propagation::Stop
        });
    }
    spin
}

fn with_value(distance: Distance, value: f64) -> Distance {
    match distance {
        Distance::Ticks(_) => Distance::Ticks(value.round().max(1.0) as u32),
        Distance::Money(_) => Distance::Money(value),
        Distance::R(_) => Distance::R(value),
    }
}

/// An exit said another way: money and R one into the other with what
/// 1R is worth, as both are the whole trade's P&L there; else where the
/// mode starts, for a stop 20 ticks, what 1R is worth, or 1R, and a
/// target twice as far.
fn said_as(distance: Distance, mode: Mode, stop: bool, r: f64) -> Distance {
    let far = if stop { 1.0 } else { 2.0 };
    match (distance, mode) {
        (Distance::R(multiple), Mode::Money) => Distance::Money((multiple * r).round()),
        (Distance::Money(money), Mode::R) if r > 0.0 => Distance::R(((money / r) * 100.0).round().max(5.0) / 100.0),
        (_, Mode::Ticks) => Distance::Ticks((20.0 * far) as u32),
        (_, Mode::Money) => Distance::Money(r * far),
        (_, Mode::R) => Distance::R(far),
    }
}

/// The dialog's pages and rows, for the tests.
#[cfg(test)]
mod tests {
    use super::*;

    /// Half off at 1.5R, opened: the half left without a target is said
    /// under the targets, and goes once a target for the rest is added.
    #[test]
    fn half_off_says_its_open_half_until_a_rest_is_added() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let half = OrderTemplate {
                id: 1,
                name: "Half off".into(),
                targets: vec![Exit { distance: Distance::R(1.5), size: Size::Share(1, 2) }],
                stops: vec![Exit { distance: Distance::R(1.0), size: Size::ALL }],
            };
            templates::save(&store, &[half]);
            let dialog = build(store.clone(), Rc::new(|| {}));
            let nav = dialog.child().and_downcast::<adw::NavigationView>().unwrap();
            let root = nav.visible_page().unwrap();
            let row = action_rows_in(root.upcast_ref()).into_iter().find(|row| row.title() == "Half off").unwrap();
            row.emit_by_name::<()>("activated", &[]);
            let page = nav.visible_page().unwrap();
            assert_eq!(page.title(), "Half off");
            let groups = groups_in(page.upcast_ref());
            let titles: Vec<String> = groups.iter().map(|group| group.title().to_string()).collect();
            assert_eq!(titles, ["", "Targets", "Stops"]);
            assert_eq!(notices_of(&groups[1]), ["1/2 of position has no target", "Needs 2 contracts to send the target"]);
            assert_eq!(notices_of(&groups[2]), Vec::<String>::new(), "a stop for all says nothing");
            let sizes: Vec<String> = menu_buttons_in(groups[1].upcast_ref()).iter().filter_map(|b| b.label().map(|l| l.to_string())).collect();
            assert_eq!(sizes, ["1/2 of position"]);

            let add = buttons_in(groups[1].upcast_ref()).into_iter().find(|b| b.tooltip_text().as_deref() == Some("Add target")).unwrap();
            add.emit_clicked();
            let saved = templates::load(&store);
            assert_eq!(saved[0].summary(), "+1.5R 1/2 · +2.25R rest | −1R", "the rest, half as far again");
            assert_eq!(notices_of(&groups[1]), ["Needs 2 contracts to send every target"]);
            let sizes: Vec<String> = menu_buttons_in(groups[1].upcast_ref()).iter().filter_map(|b| b.label().map(|l| l.to_string())).collect();
            assert_eq!(sizes, ["1/2 of position", "Rest"]);
        });
    }

    /// The lines of a group's footer.
    fn notices_of(group: &adw::PreferencesGroup) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![group.clone().upcast::<gtk::Widget>()];
        let mut inside = false;
        while let Some(widget) = stack.pop() {
            if widget.has_css_class("notices") {
                inside = true;
                stack.clear();
                stack.push(widget.first_child().unwrap());
                continue;
            }
            if let Some(label) = widget.downcast_ref::<gtk::Label>().filter(|_| inside) {
                found.push(label.label().to_string());
            }
            let mut child = widget.last_child();
            while let Some(c) = child {
                stack.push(c.clone());
                child = c.prev_sibling();
            }
        }
        found
    }

    fn action_rows_in(widget: &gtk::Widget) -> Vec<adw::ActionRow> {
        let mut found = Vec::new();
        let mut stack = vec![widget.clone()];
        while let Some(widget) = stack.pop() {
            if let Some(row) = widget.downcast_ref::<adw::ActionRow>() {
                found.push(row.clone());
            }
            let mut child = widget.last_child();
            while let Some(c) = child {
                stack.push(c.clone());
                child = c.prev_sibling();
            }
        }
        found
    }

    fn menu_buttons_in(widget: &gtk::Widget) -> Vec<gtk::MenuButton> {
        let mut found = Vec::new();
        let mut stack = vec![widget.clone()];
        while let Some(widget) = stack.pop() {
            if let Some(button) = widget.downcast_ref::<gtk::MenuButton>() {
                found.push(button.clone());
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
            assert_eq!(rows_of(&groups[0]), ["No order templates | A template's exits go in with every order from a chart on it"]);

            let menu = menu_buttons_in(root.upcast_ref()).into_iter().find(|b| b.label().as_deref() == Some("Add a template")).unwrap();
            let add = |preset: i32| {
                let picked = menu.activate_action("templates.add", Some(&preset.to_variant()));
                assert!(picked.is_ok(), "{picked:?}");
            };
            add(0);
            add(0);
            add(1);
            let saved = templates::load(&store);
            let names: Vec<&str> = saved.iter().map(|t| t.name.as_str()).collect();
            assert_eq!(names, ["Target and stop", "Target and stop 2", "Half off"]);
            assert_eq!(saved[0].summary(), "+2R | −1R", "a target at 2R and a stop at 1R");
            assert!(told.get() >= 3, "every change is handed on");
            assert_eq!(nav.visible_page().unwrap().title(), "Half off", "a new template opens");
            let groups = groups_in(root.upcast_ref());
            assert_eq!(
                rows_of(&groups[0]),
                ["Target and stop | +2R | −1R", "Target and stop 2 | +2R | −1R", "Half off | +1.5R 1/2 | −1R"]
            );
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
