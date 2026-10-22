//! Which dialog a feed is signed in to through: the one place in the UI
//! that maps a provider to the dialog of its own.
//!
//! A dialog is the provider's, in a module of its own — what a Rithmic
//! login asks for is nothing like what the next trading service's does, and one form
//! made to fit both would fit neither. Everything else reaches them through
//! here, by what the feed declares, so adding a provider is its dialog and
//! one line below. The dialogs are this module's children, so the registry
//! is the only way to them.

// Beside this file rather than under it, where the dialogs have always been.
#[path = "rithmic_attribution.rs"]
mod rithmic_attribution;
#[path = "rithmic_login.rs"]
mod rithmic_login;
#[path = "sierra_login.rs"]
mod sierra_login;
#[path = "sierra_setup.rs"]
mod sierra_setup;

use gtk::prelude::*;
use omacharts_engine::providers::{Listed, Saved};

use crate::trading::{Login, Typed};

/// A trading service connection's login dialog: what Trading opens to add a login
/// over the widget given, or to sign `known` in again, handing over what
/// was typed when somebody presses Sign in. Its name starts as the service's
/// own, numbered apart from the names the other logins have taken.
pub type LoginDialog = fn(&gtk::Widget, Option<&Login>, &[String], Box<dyn Fn(Typed)>);

/// A dialog of a feed's own, for the Market data page: built, not yet shown,
/// filled in from what the feed was last set up with. It hands what it is
/// set up with now to `done`, for the page to keep and sign the feed in
/// with; a secret it asks for goes to the feed's keyring, never into that.
pub type FeedDialog = fn(Saved, Box<dyn Fn(Saved)>) -> adw::AlertDialog;

/// The login dialog of the trading service logins call `service`.
pub fn login_dialog(service: &str) -> Option<LoginDialog> {
    match service {
        id if id == omacharts_engine::providers::rithmic::LISTED.id => Some(rithmic_login::ask),
        id if id == omacharts_engine::providers::sierra::LISTED.id => Some(sierra_login::ask),
        #[cfg(any(test, feature = "fakes"))]
        "fake" => Some(fake::login),
        _ => None,
    }
}

/// A login dialog opened over a window of its own, with what it hands over
/// when somebody signs in, for its tests.
#[cfg(test)]
fn opened(dialog: LoginDialog, known: Option<&Login>, taken: &[&str]) -> (adw::AlertDialog, std::rc::Rc<std::cell::RefCell<Option<Typed>>>) {
    use adw::prelude::*;
    let window = adw::Window::new();
    let parent = gtk::Box::new(gtk::Orientation::Vertical, 0);
    window.set_content(Some(&parent));
    let taken: Vec<String> = taken.iter().map(|name| name.to_string()).collect();
    let signed = std::rc::Rc::new(std::cell::RefCell::new(None));
    let handed = signed.clone();
    dialog(parent.upcast_ref(), known, &taken, Box::new(move |typed| *handed.borrow_mut() = Some(typed)));
    let shown = window.visible_dialog().and_downcast::<adw::AlertDialog>().expect("the dialog");
    (shown, signed)
}

/// The row under `widget` titled `title`.
#[cfg(test)]
fn row_titled<Row: IsA<gtk::Widget> + IsA<adw::PreferencesRow>>(widget: &gtk::Widget, title: &str) -> Option<Row> {
    use adw::prelude::*;
    let mut next = widget.first_child();
    while let Some(child) = next {
        if let Some(row) = child.downcast_ref::<Row>().filter(|row| row.title() == title) {
            return Some(row.clone());
        }
        if let Some(row) = row_titled::<Row>(&child, title) {
            return Some(row);
        }
        next = child.next_sibling();
    }
    None
}

/// The dialog of a feed signed in to on the Market data page.
pub fn feed_dialog(feed: &Listed) -> Option<FeedDialog> {
    match feed.id {
        id if id == omacharts_engine::providers::sierra::LISTED.id => Some(sierra_setup::dialog),
        #[cfg(test)]
        id if id == omacharts_engine::providers::fake::LISTED.id => Some(fake::dialog),
        _ => None,
    }
}

/// What a feed has to show while it is set up — its artwork and its
/// notices, as its terms ask — under the feeds on the Market data page.
pub fn credits(feed: &Listed) -> Option<adw::PreferencesGroup> {
    match feed.id {
        id if id == omacharts_engine::providers::rithmic::LISTED.id => Some(rithmic_attribution::group()),
        _ => None,
    }
}

/// The custom item a feed dot's menu leaves room for a feed's artwork in.
fn credits_item(feed: &str) -> String {
    format!("credits-{feed}")
}

/// Leave room in `menu`, in a section of its own, for the artwork of each
/// feed in `feeds` that has some to show.
pub fn leave_room_for_credits(menu: &gtk::gio::Menu, feeds: &[&str]) {
    let section = gtk::gio::Menu::new();
    for feed in feeds.iter().filter(|feed| menu_credits(feed).is_some()) {
        let item = gtk::gio::MenuItem::new(None, None);
        item.set_attribute_value("custom", Some(&credits_item(feed).to_variant()));
        section.append_item(&item);
    }
    if section.n_items() > 0 {
        menu.append_section(None, &section);
    }
}

/// A feed's artwork, small, for the menu its feed dot opens.
fn menu_credits(feed: &str) -> Option<gtk::Widget> {
    match feed {
        id if id == omacharts_engine::providers::rithmic::LISTED.id => Some(rithmic_attribution::menu_logos().upcast()),
        _ => None,
    }
}

/// Put each feed's artwork where `popover`'s menu left room for it.
pub fn fill_credits(popover: &gtk::PopoverMenu) {
    let Some(model) = popover.menu_model() else { return };
    for feed in omacharts_engine::providers::offered() {
        let item = credits_item(feed.id);
        if leaves_room(&model, &item)
            && let Some(widget) = menu_credits(feed.id)
        {
            popover.add_child(&widget, &item);
        }
    }
}

/// Does `model`, or a section of it, have the custom item `id`?
fn leaves_room(model: &gtk::gio::MenuModel, id: &str) -> bool {
    (0..model.n_items()).any(|at| {
        let custom = model.item_attribute_value(at, "custom", Some(gtk::glib::VariantTy::STRING));
        custom.and_then(|custom| custom.get::<String>()).is_some_and(|custom| custom == id)
            || model.item_link(at, "section").is_some_and(|section| leaves_room(&section, id))
    })
}

/// The fake rich feed's dialog, as a provider's own dialog would be: a
/// member number and a token, the token into the feed's keyring; and the
/// fake trading service's, a name, a user and a password.
#[cfg(any(test, feature = "fakes"))]
pub(crate) mod fake {
    use adw::prelude::*;
    #[cfg(test)]
    use omacharts_engine::providers::{fake, Saved};

    use crate::trading::{Login, Typed};

    pub const SIGN_IN: &str = "sign-in";

    /// The fake trading service's login dialog.
    pub fn login(parent: &gtk::Widget, known: Option<&Login>, _taken: &[String], signed: Box<dyn Fn(Typed)>) {
        let dialog = adw::AlertDialog::new(Some("Sign in to the fake trading service"), None);
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
                service: "fake".into(),
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
