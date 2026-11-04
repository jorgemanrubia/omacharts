//! The dialog a Rithmic login is typed into: a name, user ID, password,
//! system and gateway.
//!
//! Prefilled from the login being signed in again, except the password,
//! which is never read back out of the keyring to be shown. Left empty, the
//! password already kept is the one used.

use std::rc::Rc;

use adw::prelude::*;

use crate::trading::{Login, Typed};
use omacharts_engine::providers::rithmic::{self, session};

/// The system and gateway a new login starts on, and one typed with
/// neither signs in to: Rithmic Test, where every developer kit login is.
pub const TEST_SYSTEM: &str = ::rithmic::TEST_SYSTEM;
pub const TEST_GATEWAY: &str = ::rithmic::TEST_URL;

/// What the system and gateway are kept under in a login's values, as the
/// Rithmic adapter and data session read them.
const SYSTEM: &str = "system";
const GATEWAY: &str = "gateway";

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
    system.set_text(known.and_then(|login| login.value(SYSTEM)).unwrap_or(TEST_SYSTEM));
    list.append(&system);

    let gateway = adw::EntryRow::new();
    gateway.set_title("Gateway");
    gateway.set_text(known.and_then(|login| login.value(GATEWAY)).unwrap_or(TEST_GATEWAY));
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
        signed(typed(&name.text(), &user.text(), &password.text(), &system.text(), &gateway.text()));
        // Nothing of the password is kept in the widget once it is handed
        // over.
        password.set_text("");
    });
    dialog.present(Some(parent));
}

/// What was typed, with the system and gateway left empty as Rithmic
/// Test's, and a login with no name called what its system is.
fn typed(name: &str, user: &str, password: &str, system: &str, gateway: &str) -> Typed {
    let or = |text: &str, fallback: &str| match text.trim() {
        "" => fallback.to_string(),
        text => text.to_string(),
    };
    let system = or(system, TEST_SYSTEM);
    Typed {
        broker: rithmic::LISTED.id.to_string(),
        name: or(name, &system),
        user: user.to_string(),
        password: password.to_string(),
        values: vec![(SYSTEM.into(), system), (GATEWAY.into(), or(gateway, TEST_GATEWAY))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Left empty, the system and gateway are Rithmic Test's, and the name
    /// is the system's.
    #[test]
    fn what_is_left_empty_is_rithmic_tests() {
        let empty = typed(" ", "trader", "secret", "", "");
        assert_eq!(empty.broker, "rithmic");
        assert_eq!(empty.name, "Rithmic Test");
        assert_eq!(
            empty.values,
            vec![("system".to_string(), TEST_SYSTEM.to_string()), ("gateway".to_string(), TEST_GATEWAY.to_string())]
        );
        let named = typed("Lucid", "trader", "secret", "LucidTrading", "wss://x");
        assert_eq!((named.name.as_str(), named.values[0].1.as_str()), ("Lucid", "LucidTrading"));
    }
}
