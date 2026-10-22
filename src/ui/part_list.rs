//! A short list of parts to add to, take from and drag into order: what the
//! charts' header shows, and which columns the watchlist has.
//!
//! The same rows for both, because they are the same question — which of a
//! handful of things, and in what order — asked about two lines of text.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::ui::drop_line;

/// Something a [`PartList`] can hold.
pub trait Part: Copy + PartialEq + 'static {
    /// Every one there is, in the order the + offers them.
    fn every() -> Vec<Self>;
    /// How it is written down; what a drag carries.
    fn key(self) -> &'static str;
    /// How the list names it.
    fn label(self) -> &'static str;
    /// Always there and always first: no bin, no handle, and nothing lands
    /// above it.
    fn fixed(self) -> bool {
        false
    }
}

impl Part for crate::header::Part {
    fn every() -> Vec<Self> {
        Self::ALL.to_vec()
    }
    fn key(self) -> &'static str {
        crate::header::Part::key(self)
    }
    fn label(self) -> &'static str {
        crate::header::Part::label(self)
    }
}

/// The parts, as a group of rows.
///
/// Every change is handed to `changed` as the whole list, in order, the moment
/// it is made, as the indicator list applies its changes.
pub struct PartList<P: Part> {
    pub group: adw::PreferencesGroup,
    add: gtk::MenuButton,
    parts: RefCell<Vec<P>>,
    rows: RefCell<Vec<gtk::Widget>>,
    changed: Box<dyn Fn(Vec<P>)>,
}

impl<P: Part> PartList<P> {
    pub fn new(
        title: &str,
        add_tip: &str,
        current: Vec<P>,
        changed: impl Fn(Vec<P>) + 'static,
    ) -> Rc<PartList<P>> {
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

    fn set(self: &Rc<Self>, parts: Vec<P>) {
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
            row.set_title(part.label());
            if part.fixed() {
                // Where the handle would be, so its name lines up with the
                // others.
                let blank = gtk::Image::from_icon_name("list-drag-handle-symbolic");
                blank.set_opacity(0.0);
                row.add_prefix(&blank);
                row.set_subtitle("Always shown");
                self.group.add(&row);
                self.rows.borrow_mut().push(row.upcast());
                continue;
            }
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
                    let moving = P::every().into_iter().find(|p| p.key() == key)?;
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
        let missing: Vec<P> = P::every().into_iter().filter(|p| !parts.contains(p)).collect();
        self.add.set_visible(!missing.is_empty());
        let choices = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::new();
        for part in missing {
            let button = gtk::Button::with_label(part.label());
            button.add_css_class("flat");
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_xalign(0.0);
            }
            let this = self.clone();
            let shut = popover.clone();
            button.connect_clicked(move |_| {
                shut.popdown();
                let mut parts = this.parts.borrow().clone();
                parts.push(part);
                this.set(parts);
            });
            choices.append(&button);
        }
        popover.set_child(Some(&choices));
        self.add.set_popover(Some(&popover));
    }
}
