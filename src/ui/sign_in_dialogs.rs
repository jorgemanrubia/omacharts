//! Which dialog a feed is signed in to through: the one place in the UI
//! that maps a provider to the dialog of its own.
//!
//! A dialog is the provider's, in a module of its own — what a Rithmic
//! login asks for is nothing like what the next broker's does, and one form
//! made to fit both would fit neither. Everything else reaches them through
//! here, by what the feed declares, so adding a provider is its dialog and
//! one line below. The dialogs are this module's children, so the registry
//! is the only way to them.

// Beside this file rather than under it, where the dialogs have always been.
#[path = "rithmic_login.rs"]
mod rithmic_login;
#[path = "sierra_login.rs"]
mod sierra_login;
#[path = "sierra_setup.rs"]
mod sierra_setup;

use omacharts_engine::providers::{Listed, Saved};

use crate::trading::{Login, Typed};

/// A broker connection's login dialog: what Trading opens to add a login
/// over the widget given, or to sign `known` in again, handing over what
/// was typed when somebody presses Sign in.
pub type LoginDialog = fn(&gtk::Widget, Option<&Login>, Box<dyn Fn(Typed)>);

/// A dialog of a feed's own, for the Data page: built, not yet shown,
/// filled in from what the feed was last set up with. It hands what it is
/// set up with now to `done`, for the page to keep and sign the feed in
/// with; a secret it asks for goes to the feed's keyring, never into that.
pub type FeedDialog = fn(Saved, Box<dyn Fn(Saved)>) -> adw::AlertDialog;

/// The login dialog of the broker logins call `broker`.
pub fn login_dialog(broker: &str) -> Option<LoginDialog> {
    match broker {
        id if id == omacharts_engine::providers::rithmic::LISTED.id => Some(rithmic_login::ask),
        id if id == omacharts_engine::providers::sierra::LISTED.id => Some(sierra_login::ask),
        #[cfg(any(test, feature = "fakes"))]
        "fake" => Some(fake::login),
        _ => None,
    }
}

/// The dialog of a feed signed in to on the Data page.
pub fn feed_dialog(feed: &Listed) -> Option<FeedDialog> {
    match feed.id {
        id if id == omacharts_engine::providers::sierra::LISTED.id => Some(sierra_setup::dialog),
        #[cfg(test)]
        id if id == omacharts_engine::providers::fake::LISTED.id => Some(fake::dialog),
        _ => None,
    }
}

/// The fake rich feed's dialog, as a provider's own dialog would be: a
/// member number and a token, the token into the feed's keyring; and the
/// fake broker's, a name, a user and a password.
#[cfg(any(test, feature = "fakes"))]
pub(crate) mod fake {
    use adw::prelude::*;
    #[cfg(test)]
    use omacharts_engine::providers::{fake, Saved};

    use crate::trading::{Login, Typed};

    pub const SIGN_IN: &str = "sign-in";

    /// The fake broker's login dialog.
    pub fn login(parent: &gtk::Widget, known: Option<&Login>, signed: Box<dyn Fn(Typed)>) {
        let dialog = adw::AlertDialog::new(Some("Sign in to the fake broker"), None);
        let list = gtk::ListBox::new();
        let name = adw::EntryRow::new();
        name.set_title("Name");
        name.set_text(known.map_or("", |login| login.name.as_str()));
        list.append(&name);
        let user = adw::EntryRow::new();
        user.set_title("User");
        user.set_text(known.map_or("", |login| login.user.as_str()));
        list.append(&user);
        let password = adw::PasswordEntryRow::new();
        password.set_title("Password");
        list.append(&password);
        dialog.set_extra_child(Some(&list));
        dialog.add_responses(&[("cancel", "Cancel"), (SIGN_IN, "Sign in")]);
        dialog.connect_response(None, move |_, response| {
            if response != SIGN_IN {
                return;
            }
            signed(Typed {
                broker: "fake".into(),
                name: name.text().to_string(),
                user: user.text().to_string(),
                password: password.text().to_string(),
                values: Vec::new(),
            });
            password.set_text("");
        });
        dialog.present(Some(parent));
    }

    #[cfg(test)]
    pub fn dialog(known: Saved, done: Box<dyn Fn(Saved)>) -> adw::AlertDialog {
        let dialog = adw::AlertDialog::new(Some(&format!("Sign in to {}", fake::LISTED.label)), None);
        let list = gtk::ListBox::new();
        let member = adw::EntryRow::new();
        member.set_title("Member number");
        member.set_text(known.user.as_deref().unwrap_or(""));
        list.append(&member);
        let token = adw::PasswordEntryRow::new();
        token.set_title("Token");
        list.append(&token);
        dialog.set_extra_child(Some(&list));
        dialog.add_responses(&[("cancel", "Cancel"), (SIGN_IN, "Sign in")]);
        dialog.connect_response(None, move |_, response| {
            if response != SIGN_IN {
                return;
            }
            let Some(setup) = fake::LISTED.setup else { return };
            let login = fake::LISTED.id;
            let user = member.text().to_string();
            if let Some(keyring) = setup.keyring() {
                let _ = keyring.save(login, &user, "", &token.text());
            }
            token.set_text("");
            done(Saved { login: Some(login.into()), user: Some(user), values: known.values.clone() });
        });
        dialog
    }
}
