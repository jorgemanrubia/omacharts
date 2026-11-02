//! A short list of parts to add to, take from and drag into order: which
//! columns the watchlist has, as rows of text.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::ui::drop_line;
use crate::ui::watchlist::Column;

/// The parts, as a group of rows.
///
/// Every change is handed to `changed` as the whole list, in order, the moment
/// it is made, as the indicator list applies its changes.
pub struct PartList {
    pub group: adw::PreferencesGroup,
    add: gtk::MenuButton,
    parts: RefCell<Vec<Column>>,
    rows: RefCell<Vec<gtk::Widget>>,
    changed: Box<dyn Fn(Vec<Column>)>,
}

impl PartList {
    pub fn new(
        title: &str,
        add_tip: &str,
        current: Vec<Column>,
        changed: impl Fn(Vec<Column>) + 'static,
    ) -> Rc<PartList> {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        let add = gtk::MenuButton::new();
        add.set_icon_name("list-add-symbolic");
        add.add_css_class("flat");
        add.set_valign(gtk::Align::Center);
        add.set_tooltip_text(Some(add_tip));
        group.set_header_suffix(Some(&add));
        let list = Rc::new(PartList {
            group,
            add,
            parts: RefCell::new(current),
            rows: RefCell::new(Vec::new()),
            changed: Box::new(changed),
        });
        list.rebuild();
        list
    }

    /// What the + offers, in its order.
    pub fn offered(&self) -> Vec<Column> {
        let parts = self.parts.borrow();
        Column::ALL.into_iter().filter(|p| !parts.contains(p)).collect()
    }

    pub(crate) fn set(self: &Rc<Self>, parts: Vec<Column>) {
        *self.parts.borrow_mut() = parts.clone();
        (self.changed)(parts);
        self.rebuild();
    }

    // The rows hold the list they rebuild, and the list holds the rows: GTK
    // lets go of a row's handlers when the dialog is disposed, which is what
    // ends that.
    fn rebuild(self: &Rc<Self>) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        let parts = self.parts.borrow().clone();
        for part in &parts {
            let row = adw::ActionRow::new();
            // A name, not markup: "P&L" would be a broken entity.
            row.set_use_markup(false);
            row.set_title(part.label());
            let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
            handle.add_css_class("dim-label");
            handle.set_tooltip_text(Some("Drag to reorder"));
            row.add_prefix(&handle);

            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.set_valign(gtk::Align::Center);
            remove.set_tooltip_text(Some("Remove"));
            let this = self.clone();
            let gone = *part;
            remove.connect_clicked(move |_| {
                let kept = this.parts.borrow().iter().copied().filter(|p| *p != gone).collect();
                this.set(kept);
            });
            row.add_suffix(&remove);

            // Dragged by name rather than by position, for the reason the
            // indicator list drags by id: the list is rebuilt under the drag.
            let source = gtk::DragSource::new();
            source.set_actions(gtk::gdk::DragAction::MOVE);
            let payload = part.key().to_string();
            source.connect_prepare(move |_, _, _| {
                Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
            });
            row.add_controller(source);
            let onto = *part;
            // Where the dragged part is now and where it is going, in the
            // order as it stands.
            let places = {
                let this = self.clone();
                move |value: &glib::Value| {
                    let key = value.get::<String>().ok()?;
                    let moving = Column::from_key(&key)?;
                    let parts = this.parts.borrow();
                    Some((
                        parts.iter().position(|p| *p == moving)?,
                        parts.iter().position(|p| *p == onto)?,
                    ))
                }
            };
            let this = self.clone();
            let landing = places.clone();
            let target = drop_line::target(
                &row,
                move |value| landing(value).and_then(|(from, to)| drop_line::edge(from, to)),
                move |value| {
                    let Some((from, to)) = places(value) else { return false };
                    if from == to {
                        return false;
                    }
                    let mut parts = this.parts.borrow().clone();
                    let lifted = parts.remove(from);
                    parts.insert(to, lifted);
                    this.set(parts);
                    true
                },
            );
            row.add_controller(target);

            self.group.add(&row);
            self.rows.borrow_mut().push(row.upcast());
        }

        // The + offers what is not there yet, and goes when nothing is left.
        let missing = self.offered();
        self.add.set_visible(!missing.is_empty());
        let items = missing
            .into_iter()
            .map(|part| {
                let this = Rc::downgrade(self);
                crate::ui::menus::Item::new(part.label(), move || {
                    // From an idle: the list this rebuilds holds the menu
                    // whose row was just picked.
                    let this = this.clone();
                    glib::idle_add_local_once(move || {
                        let Some(this) = this.upgrade() else { return };
                        let mut parts = this.parts.borrow().clone();
                        parts.push(part);
                        this.set(parts);
                    });
                })
            })
            .collect();
        crate::ui::menus::actions(&self.add, "part", items);
    }
}
