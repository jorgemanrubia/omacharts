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

use omacharts_engine::providers::Listed;

use crate::trading::{Login, Typed};

/// A broker connection's login dialog: what Trading opens to add a login
/// over the widget given, or to sign `known` in again, handing over what
/// was typed when somebody presses Sign In.
pub type LoginDialog = fn(&gtk::Widget, Option<&Login>, Box<dyn Fn(Typed)>);

/// A dialog of a feed's own, for the Data page: built, not yet shown. It
/// keeps what is typed itself, and calls `done` once it has, for the page to
/// sign the feed in.
pub type FeedDialog = fn(Box<dyn Fn()>) -> adw::AlertDialog;

/// The login dialog of a feed whose login is a broker connection.
pub fn login_dialog(feed: &Listed) -> Option<LoginDialog> {
    match feed.id {
        id if id == omacharts_engine::providers::rithmic::LISTED.id => {
            Some(rithmic_login::ask)
        }
        _ => None,
    }
}

/// The dialog of a feed signed in to on the Data page.
pub fn feed_dialog(feed: &Listed) -> Option<FeedDialog> {
    match feed.id {
        #[cfg(test)]
        id if id == omacharts_engine::providers::fake::LISTED.id => Some(fake::dialog),
        _ => None,
    }
}

/// The fake rich feed's dialog, as a provider's own dialog would be: a
/// member number and a token, the token into the feed's keyring.
#[cfg(test)]
pub(crate) mod fake {
    use adw::prelude::*;
    use omacharts_engine::providers::{fake, Saved};

    pub const SIGN_IN: &str = "sign-in";

    pub fn dialog(done: Box<dyn Fn()>) -> adw::AlertDialog {
        let dialog = adw::AlertDialog::new(Some(&format!("Sign in to {}", fake::LISTED.label)), None);
        let list = gtk::ListBox::new();
        let member = adw::EntryRow::new();
        member.set_title("Member number");
        list.append(&member);
        let token = adw::PasswordEntryRow::new();
        token.set_title("Token");
        list.append(&token);
        dialog.set_extra_child(Some(&list));
        dialog.add_responses(&[("cancel", "Cancel"), (SIGN_IN, "Sign In")]);
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
            setup.remember(Saved { login: Some(login.into()), user: Some(user), values: Vec::new() });
            token.set_text("");
            done();
        });
        dialog
    }
}
