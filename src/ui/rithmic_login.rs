//! The dialog a Rithmic login is typed into: a name, user ID, password,
//! system and gateway.
//!
//! Prefilled from the login being signed in again, except the password,
//! which is never read back out of the keyring to be shown. Left empty, the
//! password already kept is the one used.

use std::rc::Rc;

use adw::prelude::*;

use crate::trading::{self, Login, Typed};
use omacharts_engine::providers::rithmic::session;

const SIGN_IN: &str = "sign-in";
const CANCEL: &str = "cancel";

/// Ask for a login over `parent` — a new one, or `known` again — and hand
/// what was typed to `signed` when somebody presses Sign In. Cancelling
/// hands over nothing.
pub fn ask(parent: &impl IsA<gtk::Widget>, known: Option<&Login>, signed: impl Fn(Typed) + 'static) {
    let dialog = adw::AlertDialog::new(Some("Sign in to Rithmic"), None);
    dialog.set_body(match session::development() {
        true => "A development login is in use, so its file signs in and nothing typed here goes \
                 to the keyring.",
        false => "Your password is kept in the desktop keyring and sent only to Rithmic.",
    });

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    // What the trader calls it, since there can be several: "Lucid",
    // "Tradeify". Left empty, it is named after its system.
    let name = adw::EntryRow::new();
    name.set_title("Name");
    name.set_text(known.map_or("", |login| login.name.as_str()));
    list.append(&name);

    let user = adw::EntryRow::new();
    user.set_title("User ID");
    user.set_text(known.map_or("", |login| login.user.as_str()));
    list.append(&user);

    let password = adw::PasswordEntryRow::new();
    password.set_title("Password");
    list.append(&password);

    let system = adw::EntryRow::new();
    system.set_title("System");
    system.set_text(known.map_or(trading::TEST_SYSTEM, |login| login.system.as_str()));
    list.append(&system);

    let gateway = adw::EntryRow::new();
    gateway.set_title("Gateway");
    gateway.set_text(known.map_or(trading::TEST_GATEWAY, |login| login.gateway.as_str()));
    list.append(&gateway);

    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[(CANCEL, "Cancel"), (SIGN_IN, "Sign In")]);
    dialog.set_response_appearance(SIGN_IN, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(SIGN_IN));
    dialog.set_close_response(CANCEL);

    // A user ID is the least a sign-in needs, unless a development file
    // stands in for the whole login.
    let ready = {
        let dialog = dialog.clone();
        let user = user.clone();
        move || {
            let enough = session::development() || !user.text().trim().is_empty();
            dialog.set_response_enabled(SIGN_IN, enough);
        }
    };
    ready();
    let ready = Rc::new(ready);
    let again = ready.clone();
    user.connect_changed(move |_| again());

    let signed = Rc::new(signed);
    dialog.connect_response(None, move |_, response| {
        if response != SIGN_IN {
            return;
        }
        signed(Typed {
            name: name.text().to_string(),
            user: user.text().to_string(),
            password: password.text().to_string(),
            system: system.text().to_string(),
            gateway: gateway.text().to_string(),
        });
        // Nothing of the password is kept in the widget once it is handed
        // over.
        password.set_text("");
    });
    dialog.present(Some(parent));
}
