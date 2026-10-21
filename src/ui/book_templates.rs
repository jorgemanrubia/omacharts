//! The two dialogs behind a chartbook tab's Template submenu: saving the book
//! as a new template, and the list of templates to rename and delete.
//!
//! In the shape of the Preferences' own lists and name dialogs, because they
//! are the same things — a name typed into an alert, a list of rows with a
//! pencil and a bin — for a different kind of template.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use crate::book_templates::refused_name;
use crate::ui::Window;

/// Ask for a name and save the chartbook on screen as a new template under it,
/// putting the book on it.
///
/// Only ever a new one. A name a template already has, in any case, holds
/// Save back and marks the box, the way a blank one holds it back: replacing
/// a template from here would change every chartbook on it, and that is not
/// what saving a new one sounds like.
pub fn save_new(window: &Rc<Window>) {
    let this = window.clone();
    ask_template_name(
        window,
        &window.window,
        "Save as New Chartbook Template",
        "",
        "Save",
        None,
        move |name| this.save_as_new_book_template(name),
    );
}

/// An `ask_name` that also holds back a name no chartbook template may have
/// — another one's, "None" — and says why in the box. `except` is the
/// template being renamed, which may keep its own.
fn ask_template_name(
    window: &Rc<Window>,
    anchor: &impl IsA<gtk::Widget>,
    heading: &str,
    initial: &str,
    verb: &str,
    except: Option<u32>,
    done: impl Fn(&str) + 'static,
) {
    let templates = window.book_templates();
    let name = adw::EntryRow::new();
    name.set_title("Name");
    name.set_text(initial);
    let rows = adw::PreferencesGroup::new();
    rows.add(&name);

    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&rows));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", verb);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");

    let refused = Rc::new(move |text: &str| refused_name(&templates, text, except));
    dialog.set_response_enabled("ok", refused(initial).is_none());
    let gate = dialog.clone();
    let checks = refused.clone();
    name.connect_changed(move |field| {
        let text = field.text();
        let why = checks(&text);
        gate.set_response_enabled("ok", why.is_none());
        // Red only for a name that cannot be had: a blank box is not a
        // mistake yet.
        match why.filter(|_| !text.trim().is_empty()) {
            Some(why) => {
                field.add_css_class("error");
                field.set_tooltip_text(Some(&why));
            }
            None => {
                field.remove_css_class("error");
                field.set_tooltip_text(None);
            }
        }
    });
    let finish = dialog.clone();
    let checks = refused.clone();
    name.connect_entry_activated(move |field| {
        if checks(&field.text()).is_none() {
            finish.emit_by_name::<()>("response", &[&"ok"]);
            finish.close();
        }
    });

    let field = name.clone();
    let answered = std::cell::Cell::new(false);
    dialog.connect_response(None, move |_, response| {
        let typed = field.text();
        if response != "ok" || refused(&typed).is_some() || answered.replace(true) {
            return;
        }
        done(typed.trim());
    });

    dialog.present(Some(anchor));
    name.grab_focus();
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
            let window = this.clone();
            let heading = "Rename Chartbook Template";
            ask_template_name(&window, button, heading, &name, "Rename", Some(id), move |chosen| {
                // The id stays, so every chartbook on it stays on it.
                this.rename_book_template(id, chosen);
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
