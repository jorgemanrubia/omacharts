//! The dialog a login is typed into, drawn from the fields the feed or the
//! broker declares: one row per field, a password row for a secret, a
//! choice of a few for a choice.
//!
//! Prefilled from the login being signed in again, or else from each
//! field's own default, except a secret, which is never read back out of
//! the keyring to be shown. Left empty, the secret already kept is the one
//! used.

use std::rc::Rc;

use adw::prelude::*;
use omacharts_engine::providers::{Field, FieldKind, Listed};

const SIGN_IN: &str = "sign-in";
const CANCEL: &str = "cancel";

/// What a form asks, and of whom.
pub struct Form {
    pub heading: String,
    /// Where what is typed goes, in the feed's words.
    pub body: String,
    pub fields: Vec<Field>,
    /// What to prefill, by field key, for a login signed in again.
    pub known: Vec<(&'static str, String)>,
    /// Something other than the form signs in, so nothing need be typed.
    pub stood_in: bool,
}

impl Form {
    /// The form `feed` signs in through, if it signs in through one.
    pub fn for_feed(feed: &'static Listed) -> Option<Form> {
        let setup = feed.setup?;
        Some(Form {
            heading: format!("Sign in to {}", feed.label),
            body: setup.promise(),
            fields: feed.form()?.to_vec(),
            known: Vec::new(),
            stood_in: setup.stood_in(),
        })
    }
}

/// What was typed, by field key.
///
/// No `Debug`, so that it cannot be printed by accident: a secret is in it.
pub struct Typed(Vec<(Field, String)>);

impl Typed {
    /// What was typed for `key`, or nothing.
    pub fn get(&self, key: &str) -> String {
        self.0.iter().find(|(field, _)| field.key == key).map(|(_, value)| value.clone()).unwrap_or_default()
    }

    /// The first secret typed, or nothing.
    pub fn secret(&self) -> String {
        self.0
            .iter()
            .find(|(field, _)| field.kind == FieldKind::Secret)
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    /// Every field with what was typed in it.
    pub fn values(&self) -> Vec<(&'static str, String)> {
        self.0.iter().map(|(field, value)| (field.key, value.clone())).collect()
    }
}

/// One field's row, and how to read it back.
enum Row {
    Entry(adw::EntryRow),
    Secret(adw::PasswordEntryRow),
    Choice(adw::ComboRow, &'static [&'static str]),
}

impl Row {
    fn new(field: &Field, known: Option<&str>) -> Row {
        let text = known.unwrap_or(field.default);
        match field.kind {
            FieldKind::Text | FieldKind::Url => {
                let row = adw::EntryRow::new();
                row.set_title(field.title);
                if field.kind == FieldKind::Url {
                    row.set_input_purpose(gtk::InputPurpose::Url);
                }
                row.set_text(text);
                Row::Entry(row)
            }
            FieldKind::Secret => {
                let row = adw::PasswordEntryRow::new();
                row.set_title(field.title);
                Row::Secret(row)
            }
            FieldKind::Choice(choices) => {
                let row = adw::ComboRow::new();
                row.set_title(field.title);
                row.set_model(Some(&gtk::StringList::new(choices)));
                if let Some(at) = choices.iter().position(|choice| *choice == text) {
                    row.set_selected(at as u32);
                }
                Row::Choice(row, choices)
            }
        }
    }

    fn widget(&self) -> gtk::Widget {
        match self {
            Row::Entry(row) => row.clone().upcast(),
            Row::Secret(row) => row.clone().upcast(),
            Row::Choice(row, _) => row.clone().upcast(),
        }
    }

    fn text(&self) -> String {
        match self {
            Row::Entry(row) => row.text().to_string(),
            Row::Secret(row) => row.text().to_string(),
            Row::Choice(row, choices) => {
                choices.get(row.selected() as usize).map(|choice| choice.to_string()).unwrap_or_default()
            }
        }
    }

    fn on_change(&self, changed: impl Fn() + 'static) {
        match self {
            Row::Entry(row) => {
                row.connect_changed(move |_| changed());
            }
            Row::Secret(row) => {
                row.connect_changed(move |_| changed());
            }
            Row::Choice(row, _) => {
                row.connect_selected_notify(move |_| changed());
            }
        }
    }

    /// Nothing of a secret is kept in the widget once it is handed over.
    fn forget(&self) {
        if let Row::Secret(row) = self {
            row.set_text("");
        }
    }
}

/// The dialog for `form`, not yet shown, handing what was typed to `signed`
/// when somebody presses Sign In. Cancelling hands over nothing.
pub fn dialog(form: Form, signed: impl Fn(Typed) + 'static) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(&form.heading), None);
    dialog.set_body(&form.body);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let rows: Rc<Vec<(Field, Row)>> = Rc::new(
        form.fields
            .iter()
            .map(|field| {
                let known = form.known.iter().find(|(key, _)| *key == field.key).map(|(_, value)| value.as_str());
                (*field, Row::new(field, known))
            })
            .collect(),
    );
    for (_, row) in rows.iter() {
        list.append(&row.widget());
    }

    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[(CANCEL, "Cancel"), (SIGN_IN, "Sign In")]);
    dialog.set_response_appearance(SIGN_IN, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(SIGN_IN));
    dialog.set_close_response(CANCEL);

    // Every required field filled in is the least a sign-in needs, unless
    // something else stands in for the whole login.
    let ready = {
        let dialog = dialog.clone();
        let rows = Rc::downgrade(&rows);
        let stood_in = form.stood_in;
        move || {
            let Some(rows) = rows.upgrade() else { return };
            let enough = stood_in
                || rows.iter().all(|(field, row)| !field.required || !row.text().trim().is_empty());
            dialog.set_response_enabled(SIGN_IN, enough);
        }
    };
    ready();
    let ready = Rc::new(ready);
    for (field, row) in rows.iter() {
        if field.required {
            let again = ready.clone();
            row.on_change(move || again());
        }
    }

    dialog.connect_response(None, move |_, response| {
        if response != SIGN_IN {
            return;
        }
        signed(Typed(rows.iter().map(|(field, row)| (*field, row.text())).collect()));
        for (_, row) in rows.iter() {
            row.forget();
        }
    });
    dialog
}

/// Ask for a login over `parent`.
pub fn ask(parent: &impl IsA<gtk::Widget>, form: Form, signed: impl Fn(Typed) + 'static) {
    dialog(form, signed).present(Some(parent));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The title of every row in a dialog's form, top first.
    pub(crate) fn titles(dialog: &adw::AlertDialog) -> Vec<String> {
        let Some(list) = dialog.extra_child().and_downcast::<gtk::ListBox>() else { return Vec::new() };
        let mut titles = Vec::new();
        let mut next = list.first_child();
        while let Some(row) = next {
            if let Ok(row) = row.clone().downcast::<adw::PreferencesRow>() {
                titles.push(row.title().to_string());
            }
            next = row.next_sibling();
        }
        titles
    }

    const FIELDS: &[Field] = &[
        Field { key: "user", title: "User", kind: FieldKind::Text, default: "", required: true },
        Field { key: "secret", title: "Secret", kind: FieldKind::Secret, default: "", required: false },
        Field { key: "where", title: "Where", kind: FieldKind::Choice(&["Here", "There"]), default: "There", required: false },
        Field { key: "url", title: "Address", kind: FieldKind::Url, default: "wss://example.invalid", required: false },
    ];

    fn form(known: Vec<(&'static str, String)>, stood_in: bool) -> Form {
        Form { heading: "Sign in".into(), body: String::new(), fields: FIELDS.to_vec(), known, stood_in }
    }

    /// A row per field, in order, each prefilled with what is known or its
    /// default, never a secret; and Sign In waits for what is required.
    #[test]
    fn the_rows_are_the_fields() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let asked = dialog(form(Vec::new(), false), |_| {});
        assert_eq!(titles(&asked), vec!["User", "Secret", "Where", "Address"]);
        assert!(!asked.is_response_enabled(SIGN_IN), "no user yet");

        let list = asked.extra_child().and_downcast::<gtk::ListBox>().unwrap();
        let user = list.first_child().and_downcast::<adw::EntryRow>().unwrap();
        user.set_text("trader");
        assert!(asked.is_response_enabled(SIGN_IN));
        let choice = list.row_at_index(2).and_downcast::<adw::ComboRow>().unwrap();
        assert_eq!(choice.selected(), 1, "the default");
        let url = list.row_at_index(3).and_downcast::<adw::EntryRow>().unwrap();
        assert_eq!(url.text(), "wss://example.invalid");

        let known = dialog_with(vec![("url", "wss://known.invalid".into()), ("secret", "LEAKED".into())]);
        let list = known.extra_child().and_downcast::<gtk::ListBox>().unwrap();
        assert_eq!(list.row_at_index(3).and_downcast::<adw::EntryRow>().unwrap().text(), "wss://known.invalid");
        assert_eq!(list.row_at_index(1).and_downcast::<adw::PasswordEntryRow>().unwrap().text(), "");

        let stood_in = dialog(form(Vec::new(), true), |_| {});
        assert!(stood_in.is_response_enabled(SIGN_IN), "nothing need be typed");
    }

    fn dialog_with(known: Vec<(&'static str, String)>) -> adw::AlertDialog {
        dialog(form(known, false), |_| {})
    }
}
