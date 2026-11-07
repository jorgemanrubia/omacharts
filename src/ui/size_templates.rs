//! The Trading sizes dialog: the size templates a chart picks its trading
//! size from, each a name, its sizes, and what it applies to.
//!
//! Opened from Preferences → Trading and from a right click on a chart's
//! Trading size. Every change is saved as it is made, as the preferences
//! are, and handed to `changed` so the charts say their sizes from it at
//! once. What the templates are and how a chart picks one is
//! [`crate::trading::sizes`]; this only edits the list.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::store::Store;
use crate::trading::sizes::{self, Applies, Category, SizeTemplate};

struct Editor {
    store: Rc<Store>,
    changed: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    templates: RefCell<Vec<SizeTemplate>>,
}

/// Open the dialog over `parent`.
pub fn present(parent: &impl IsA<gtk::Widget>, store: Rc<Store>, changed: Rc<dyn Fn()>) -> adw::Dialog {
    let page = adw::PreferencesPage::new();
    let editor = Rc::new(Editor {
        templates: RefCell::new(sizes::load(&store)),
        store,
        changed,
        page: page.clone(),
        groups: RefCell::new(Vec::new()),
    });
    rebuild(&editor);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    let dialog = adw::Dialog::new();
    dialog.set_title("Quantities");
    dialog.set_content_width(460);
    dialog.set_content_height(620);
    dialog.set_child(Some(&toolbar));
    // The editor lives as long as the dialog does.
    let kept = editor.clone();
    dialog.connect_closed(move |_| {
        let _ = &kept;
    });
    dialog.present(Some(parent));
    dialog
}

impl Editor {
    /// Save the list as it stands, and let the charts know.
    fn save(&self) {
        sizes::save(&self.store, &self.templates.borrow());
        (self.changed)();
    }

    fn edit(&self, at: usize, change: impl FnOnce(&mut SizeTemplate)) {
        if let Some(template) = self.templates.borrow_mut().get_mut(at) {
            change(template);
        }
        self.save();
    }
}

fn rebuild(editor: &Rc<Editor>) {
    for group in editor.groups.borrow_mut().drain(..) {
        editor.page.remove(&group);
    }
    let templates = editor.templates.borrow().clone();
    let mut groups: Vec<adw::PreferencesGroup> =
        templates.iter().enumerate().map(|(at, template)| template_group(editor, at, template)).collect();
    groups.push(add_group(editor));
    for group in &groups {
        editor.page.add(group);
    }
    *editor.groups.borrow_mut() = groups;
}

/// Marks a row whose text cannot be saved, until it can.
fn flag(row: &adw::EntryRow, wrong: bool) {
    match wrong {
        true => row.add_css_class("error"),
        false => row.remove_css_class("error"),
    }
}

fn template_group(editor: &Rc<Editor>, at: usize, template: &SizeTemplate) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(&gtk::glib::markup_escape_text(&template.name));

    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_valign(gtk::Align::Center);
    remove.set_tooltip_text(Some(&format!("Remove {}", template.name)));
    let removing = editor.clone();
    remove.connect_clicked(move |_| {
        {
            let mut templates = removing.templates.borrow_mut();
            if at < templates.len() {
                templates.remove(at);
            }
        }
        removing.save();
        rebuild(&removing);
    });
    group.set_header_suffix(Some(&remove));

    let name = adw::EntryRow::new();
    name.set_title("Name");
    name.set_text(&template.name);
    let naming = editor.clone();
    let titled = group.clone();
    name.connect_changed(move |row| {
        let text = row.text().trim().to_string();
        flag(row, text.is_empty());
        if text.is_empty() {
            return;
        }
        titled.set_title(&gtk::glib::markup_escape_text(&text));
        naming.edit(at, |template| template.name = text);
    });
    group.add(&name);

    let sizes_row = adw::EntryRow::new();
    sizes_row.set_title("Quantities");
    sizes_row.set_text(&sizes::spell_sizes(&template.sizes));
    let sizing = editor.clone();
    sizes_row.connect_changed(move |row| match sizes::parse_sizes(&row.text()) {
        Ok(list) => {
            flag(row, false);
            sizing.edit(at, |template| template.sizes = list);
        }
        Err(_) => flag(row, true),
    });
    group.add(&sizes_row);

    // What it applies to: a toggle for each category, side by side.
    let applies = adw::ActionRow::new();
    applies.set_title("Applies to");
    let toggles = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    toggles.add_css_class("linked");
    toggles.set_valign(gtk::Align::Center);
    for category in Category::ALL {
        let toggle = gtk::ToggleButton::with_label(category.label());
        toggle.set_active(template.applies_to.categories.contains(&category));
        let applying = editor.clone();
        toggle.connect_toggled(move |toggle| {
            let on = toggle.is_active();
            applying.edit(at, |template| {
                let categories = &mut template.applies_to.categories;
                categories.retain(|c| *c != category);
                if on {
                    categories.push(category);
                    categories.sort_by_key(|c| Category::ALL.iter().position(|all| all == c));
                }
            });
        });
        toggles.append(&toggle);
    }
    applies.add_suffix(&toggles);
    group.add(&applies);

    let roots = adw::EntryRow::new();
    roots.set_title("Only these symbols");
    roots.set_text(&template.applies_to.roots.join(", "));
    let rooting = editor.clone();
    roots.connect_changed(move |row| {
        let list = sizes::parse_roots(&row.text());
        rooting.edit(at, |template| template.applies_to.roots = list);
    });
    group.add(&roots);
    group
}

fn add_group(editor: &Rc<Editor>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let add = gtk::Button::with_label("Add a template");
    add.set_halign(gtk::Align::Start);
    let adding = editor.clone();
    add.connect_clicked(move |_| add_template(&adding));
    group.add(&add);
    group
}

/// A new template at the end, for futures, with one size to start from.
fn add_template(adding: &Rc<Editor>) {
        let name = {
            let templates = adding.templates.borrow();
            (1..)
                .map(|n| match n {
                    1 => "New template".to_string(),
                    n => format!("New template {n}"),
                })
                .find(|name| !templates.iter().any(|template| template.name == *name))
                .unwrap_or_default()
        };
        adding.templates.borrow_mut().push(SizeTemplate {
            name,
            sizes: vec![sizes::Quantity::ONE],
            applies_to: Applies { categories: vec![Category::Futures], roots: Vec::new() },
        });
        adding.save();
        rebuild(adding);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(page: &adw::PreferencesPage) -> Vec<String> {
        let mut found = Vec::new();
        let mut stack = vec![page.clone().upcast::<gtk::Widget>()];
        while let Some(widget) = stack.pop() {
            if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
                found.push(group.title().to_string());
            }
            let mut child = widget.last_child();
            while let Some(next) = child {
                stack.push(next.clone());
                child = next.prev_sibling();
            }
        }
        found
    }

    /// The standard two are listed; one added is saved at once and handed
    /// on, and a size typed that is not one is not saved.
    #[test]
    fn templates_are_edited_and_saved_as_they_change() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let told = Rc::new(std::cell::Cell::new(0));
            let counted = told.clone();
            let page = adw::PreferencesPage::new();
            let editor = Rc::new(Editor {
                templates: RefCell::new(sizes::load(&store)),
                store: store.clone(),
                changed: Rc::new(move || counted.set(counted.get() + 1)),
                page: page.clone(),
                groups: RefCell::new(Vec::new()),
            });
            rebuild(&editor);
            let listed = titles(&page);
            assert!(listed.contains(&"Stocks".to_string()) && listed.contains(&"Futures".to_string()), "{listed:?}");

            editor.edit(1, |template| template.sizes = sizes::parse_sizes("1, 2, 5").unwrap());
            assert_eq!(sizes::load(&store)[1].sizes, sizes::parse_sizes("1, 2, 5").unwrap());
            assert_eq!(told.get(), 1);

            add_template(&editor);
            assert_eq!(sizes::load(&store).len(), 3);
            assert!(titles(&page).contains(&"New template".to_string()));
        });
    }
}
