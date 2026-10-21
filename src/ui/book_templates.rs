//! The two dialogs behind a chartbook tab's Template submenu: saving the book
//! as a template, and the list of templates to rename and delete.
//!
//! In the shape of the Preferences' own lists and name dialogs, because they
//! are the same things — a name typed into an alert, a list of rows with a
//! pencil and a bin — for a different kind of template.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::book_templates::{self, BookTemplate};
use crate::ui::preferences::ask_name;
use crate::ui::Window;

/// Ask for a name and save the chartbook on screen under it.
///
/// The templates there are already are listed under the box, and choosing one
/// puts its name in it: saving over one is asked about, saying how many other
/// chartbooks will change with it. A new name makes a new template. Either
/// way the book is on the template afterwards.
pub fn save_as(window: &Rc<Window>) {
    let templates = window.book_templates();
    let name = adw::EntryRow::new();
    name.set_title("Name");
    let rows = adw::PreferencesGroup::new();
    rows.add(&name);

    let holder = gtk::Box::new(gtk::Orientation::Vertical, 12);
    holder.append(&rows);
    if !templates.is_empty() {
        let existing = adw::PreferencesGroup::new();
        existing.set_title("Replace a template");
        for template in &templates {
            let row = adw::ActionRow::new();
            row.set_title(&template.name);
            row.set_activatable(true);
            let field = name.clone();
            let chosen = template.name.clone();
            row.connect_activated(move |_| {
                field.set_text(&chosen);
                field.grab_focus();
            });
            existing.add(&row);
        }
        holder.append(&existing);
    }

    let dialog = adw::AlertDialog::new(Some("Save as Chartbook Template"), None);
    dialog.set_extra_child(Some(&holder));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", "Save");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.set_response_enabled("ok", false);

    let gate = dialog.clone();
    name.connect_changed(move |field| {
        gate.set_response_enabled("ok", !field.text().trim().is_empty());
    });
    let finish = dialog.clone();
    name.connect_entry_activated(move |field| {
        if !field.text().trim().is_empty() {
            finish.emit_by_name::<()>("response", &[&"ok"]);
            finish.close();
        }
    });

    let field = name.clone();
    let answered = std::cell::Cell::new(false);
    let this = window.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "ok" || answered.replace(true) {
            return;
        }
        let typed = field.text().trim().to_string();
        match book_templates::find(&templates, &typed) {
            book_templates::Lookup::Found(over) => confirm_overwrite(&this, over),
            // Two with one name can only come from a rename; the first is the
            // one the list shows first, and asking still says which.
            book_templates::Lookup::Ambiguous(ids) => {
                if let Some(over) = ids.first().and_then(|id| templates.iter().find(|t| t.id == *id)) {
                    confirm_overwrite(&this, over);
                }
            }
            book_templates::Lookup::Missing => this.save_as_new_book_template(&typed),
        }
    });

    dialog.present(Some(&window.window));
    name.grab_focus();
}

/// Ask before saving over a template, saying what else will change.
///
/// Saving over the one this book is on asks nothing: the book's changes are
/// already in it, so there is nothing to lose.
fn confirm_overwrite(window: &Rc<Window>, over: &BookTemplate) {
    if window.on_book_template(over.id) {
        window.save_over_book_template(over.id);
        return;
    }
    let others = window.books_on_book_template(over.id);
    let follow = match others {
        0 => String::new(),
        1 => " The chartbook on it will change to match.".to_string(),
        n => format!(" The {n} chartbooks on it will change to match."),
    };
    let body = format!(
        "\u{201c}{}\u{201d} will hold this chartbook\u{2019}s layout and indicators.{follow}",
        over.name
    );
    let dialog = adw::AlertDialog::new(Some("Replace Chartbook Template?"), Some(&body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("replace", "Replace");
    dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let this = window.clone();
    let id = over.id;
    dialog.connect_response(None, move |_, response| {
        if response == "replace" {
            this.save_over_book_template(id);
        }
    });
    dialog.present(Some(&window.window));
}

/// The chartbook templates, each with a rename and a delete.
pub fn manage(window: &Rc<Window>) {
    let dialog = adw::Dialog::new();
    dialog.set_title("Chartbook Templates");
    dialog.set_content_width(440);
    let page = adw::PreferencesPage::new();
    let header = adw::HeaderBar::new();
    let root = adw::ToolbarView::new();
    root.add_top_bar(&header);
    root.set_content(Some(&page));
    dialog.set_child(Some(&root));

    let shown: Rc<RefCell<Option<adw::PreferencesGroup>>> = Rc::new(RefCell::new(None));
    rebuild(window, &page, &shown);
    dialog.present(Some(&window.window));
}

/// The list, drawn again whole after any change, as the chart templates'
/// list is.
fn rebuild(
    window: &Rc<Window>,
    page: &adw::PreferencesPage,
    shown: &Rc<RefCell<Option<adw::PreferencesGroup>>>,
) {
    if let Some(old) = shown.borrow_mut().take() {
        page.remove(&old);
    }
    let group = adw::PreferencesGroup::new();
    group.set_title("Chartbook templates");
    let templates = window.book_templates();
    if templates.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("No chartbook templates");
        row.add_css_class("dim-label");
        group.add(&row);
    }
    for template in templates {
        let row = adw::ActionRow::new();
        row.set_title(&template.name);
        let charts = match template.shape.panes.len() {
            1 => "1 chart".to_string(),
            n => format!("{n} charts"),
        };
        let using = match window.books_on_book_template(template.id) {
            0 => String::new(),
            1 => " · 1 chartbook".to_string(),
            n => format!(" · {n} chartbooks"),
        };
        row.set_subtitle(&format!("{charts}{using}"));

        let rename = gtk::Button::from_icon_name("document-edit-symbolic");
        rename.add_css_class("flat");
        rename.set_valign(gtk::Align::Center);
        rename.set_tooltip_text(Some("Rename"));
        let (this, at, list, id, name) =
            (window.clone(), page.clone(), shown.clone(), template.id, template.name.clone());
        rename.connect_clicked(move |button| {
            let (this, at, list) = (this.clone(), at.clone(), list.clone());
            ask_name(button, "Rename Chartbook Template", &name, "Rename", move |chosen| {
                // The id stays, so every chartbook on it stays on it.
                this.rename_book_template(id, &chosen);
                rebuild(&this, &at, &list);
            });
        });
        row.add_suffix(&rename);

        let delete = gtk::Button::from_icon_name("user-trash-symbolic");
        delete.add_css_class("flat");
        delete.set_valign(gtk::Align::Center);
        delete.set_tooltip_text(Some("Delete"));
        let (this, at, list, name) =
            (window.clone(), page.clone(), shown.clone(), template.name.clone());
        delete.connect_clicked(move |button| confirm_delete(&this, button, id, &name, &at, &list));
        row.add_suffix(&delete);
        group.add(&row);
    }
    page.add(&group);
    *shown.borrow_mut() = Some(group);
}

/// Asked first, saying how many chartbooks are on it: they keep what they
/// have, but stop changing with it, and making another of the same name does
/// not put them back.
fn confirm_delete(
    window: &Rc<Window>,
    anchor: &gtk::Button,
    id: u32,
    name: &str,
    page: &adw::PreferencesPage,
    shown: &Rc<RefCell<Option<adw::PreferencesGroup>>>,
) {
    let body = match window.books_on_book_template(id) {
        0 => format!("Delete \u{201c}{name}\u{201d}? No chartbook is using it."),
        1 => format!(
            "\u{201c}{name}\u{201d} is used by 1 chartbook. It keeps its layout and \
             indicators, and stops changing with the template."
        ),
        n => format!(
            "\u{201c}{name}\u{201d} is used by {n} chartbooks. They keep their layout and \
             indicators, and stop changing with the template."
        ),
    };
    let dialog = adw::AlertDialog::new(Some("Delete Chartbook Template?"), Some(&body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("delete", "Delete");
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let (this, at, list) = (window.clone(), page.clone(), shown.clone());
    dialog.connect_response(None, move |_, response| {
        if response != "delete" {
            return;
        }
        this.delete_book_template(id);
        rebuild(&this, &at, &list);
    });
    dialog.present(Some(anchor));
}
