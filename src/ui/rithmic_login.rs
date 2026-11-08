//! The dialog a Rithmic login is typed into: a name, user ID and password,
//! and the system and gateway picked from Rithmic's, with Rithmic's artwork
//! under them (see `rithmic_attribution`).
//!
//! Prefilled from the login being signed in again, except the password,
//! which is never read back out of the keyring to be shown. Left empty, the
//! password already kept is the one used.

use std::rc::Rc;

use adw::prelude::*;

use crate::trading::{Login, Typed};
use omacharts_engine::providers::rithmic::session::{self, GATEWAY, SYSTEM};
use omacharts_engine::providers::rithmic;

/// The system and gateway a new login starts on, and one typed with
/// neither signs in to: Rithmic Test, where every developer kit login is.
pub const TEST_SYSTEM: &str = ::rithmic::TEST_SYSTEM;
pub const TEST_GATEWAY: &str = ::rithmic::TEST_URL;

const SIGN_IN: &str = "sign-in";
const CANCEL: &str = "cancel";

/// Ask for a login over `parent` — a new one, or `known` again — and hand
/// what was typed to `signed` when somebody presses Sign in. Cancelling
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

    // Rithmic's systems and their gateways, picked rather than typed: the
    // ones omacharts cannot sign in to yet are listed, and Sign in is off
    // while one of them is chosen.
    let saved_system = known.and_then(|login| login.value(SYSTEM)).unwrap_or(TEST_SYSTEM);
    let saved_gateway = known.and_then(|login| login.value(GATEWAY)).unwrap_or(TEST_GATEWAY);
    let systems = Rc::new(choices(saved_system, saved_gateway));
    let system = adw::ComboRow::new();
    system.set_title("System");
    let names: Vec<&str> = systems.iter().map(|choice| choice.name.as_str()).collect();
    system.set_model(Some(&gtk::StringList::new(&names)));
    system.set_list_factory(Some(&system_items(systems.clone())));
    list.append(&system);

    let gateway = adw::ComboRow::new();
    gateway.set_title("Gateway");
    list.append(&gateway);

    let at = systems.iter().position(|choice| choice.name == saved_system).unwrap_or(0);
    let show_gateways = {
        let systems = systems.clone();
        let gateway = gateway.clone();
        let system = system.clone();
        move |picked: Option<&str>| {
            let Some(choice) = systems.get(system.selected() as usize) else { return };
            let names: Vec<&str> = choice.gateways.iter().map(|(name, _)| name.as_str()).collect();
            gateway.set_model(Some(&gtk::StringList::new(&names)));
            let at = picked.and_then(|url| choice.gateways.iter().position(|(_, known)| known == url)).unwrap_or(0);
            gateway.set_selected(at as u32);
            system.set_subtitle(if choice.open { "" } else { NOT_YET });
        }
    };
    system.set_selected(at as u32);
    show_gateways(Some(saved_gateway));

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.append(&list);
    content.append(&super::rithmic_attribution::logos(20, gtk::Orientation::Horizontal));
    dialog.set_extra_child(Some(&content));
    dialog.add_responses(&[(CANCEL, "Cancel"), (SIGN_IN, "Sign in")]);
    dialog.set_response_appearance(SIGN_IN, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(SIGN_IN));
    dialog.set_close_response(CANCEL);

    // A user ID is the least a sign-in needs, unless a development file
    // stands in for the whole login.
    // A system omacharts cannot sign in to yet is never signed in to.
    let ready = {
        let dialog = dialog.clone();
        let user = user.clone();
        let system = system.clone();
        let systems = systems.clone();
        move || {
            let open = systems.get(system.selected() as usize).is_some_and(|choice| choice.open);
            let enough = session::development() || !user.text().trim().is_empty();
            dialog.set_response_enabled(SIGN_IN, open && enough);
        }
    };
    ready();
    let ready = Rc::new(ready);
    let again = ready.clone();
    user.connect_changed(move |_| again());
    let again = ready.clone();
    system.connect_selected_notify(move |_| {
        show_gateways(None);
        again();
    });

    let signed = Rc::new(signed);
    dialog.connect_response(None, move |_, response| {
        if response != SIGN_IN {
            return;
        }
        let Some(choice) = systems.get(system.selected() as usize) else { return };
        let url = choice.gateways.get(gateway.selected() as usize).map_or("", |(_, url)| url.as_str());
        signed(typed(&name.text(), &user.text(), &password.text(), &choice.name, url));
        // Nothing of the password is kept in the widget once it is handed
        // over.
        password.set_text("");
    });
    dialog.present(Some(parent));
}

/// What a system omacharts cannot sign in to yet says, beside its name.
const NOT_YET: &str = "Not available yet";

/// The worldwide gateways of Rithmic's production systems, as Rithmic
/// names them. Rithmic 01 and Rithmic 04 Colo have New York's too.
const WORLDWIDE: [&str; 11] = [
    "Chicago Area",
    "Europe",
    "Frankfurt",
    "Cape Town",
    "Mumbai",
    "Hong Kong",
    "Singapore",
    "Seoul",
    "Tokyo",
    "Sydney",
    "S\u{e3}o Paulo",
];

/// A Rithmic system and its gateways: what was signed in to is open; the
/// production systems are listed, for once omacharts has passed Rithmic's
/// conformance, and cannot be signed in to before.
#[derive(Debug, PartialEq)]
struct Choice {
    name: String,
    /// Each gateway's name and address. A system not open yet has no
    /// addresses.
    gateways: Vec<(String, String)>,
    open: bool,
}

/// The systems the dialog offers, Rithmic Test first, with the system and
/// gateway a login was saved with among them: a system or gateway Rithmic
/// does not list here is kept, open, under its own name, so no login stops
/// working because it is not in the list.
fn choices(saved_system: &str, saved_gateway: &str) -> Vec<Choice> {
    let closed = |name: &str, new_york: bool| {
        let mut gateways: Vec<(String, String)> = WORLDWIDE.iter().map(|name| (name.to_string(), String::new())).collect();
        if new_york {
            gateways.insert(1, ("NYC Area".into(), String::new()));
        }
        Choice { name: name.into(), gateways, open: false }
    };
    let mut choices = vec![
        Choice { name: TEST_SYSTEM.into(), gateways: vec![("Chicago Area".into(), TEST_GATEWAY.into())], open: true },
        closed("Rithmic 01", true),
        closed("Rithmic 04 Colo", true),
        closed("Rithmic Paper Trading", false),
    ];
    let gateway = || (host(saved_gateway).to_string(), saved_gateway.to_string());
    match choices.iter_mut().find(|choice| choice.name == saved_system) {
        Some(choice) if choice.open => {
            if !choice.gateways.iter().any(|(_, url)| url == saved_gateway) {
                choice.gateways.push(gateway());
            }
        }
        // Saved on a system that is listed but not open: signed in to before
        // the list said so, and kept that way.
        Some(choice) => {
            choice.open = true;
            choice.gateways = vec![gateway()];
        }
        None => choices.push(Choice { name: saved_system.into(), gateways: vec![gateway()], open: true }),
    }
    choices
}

/// A gateway's address without its scheme and port, to name one Rithmic
/// does not list.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split([':', '/']).next().filter(|host| !host.is_empty()).unwrap_or(url)
}

/// The systems' list: each name, and beside one omacharts cannot sign in
/// to yet, that it is not available.
fn system_items(systems: Rc<Vec<Choice>>) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let name = gtk::Label::new(None);
        name.set_xalign(0.0);
        name.set_hexpand(true);
        let note = gtk::Label::new(Some(NOT_YET));
        note.add_css_class("dim-label");
        note.add_css_class("caption");
        // The mark the gateways' list draws by the one chosen.
        let mark = gtk::Image::from_icon_name("object-select-symbolic");
        mark.set_visible(item.is_selected());
        line.append(&name);
        line.append(&note);
        line.append(&mark);
        item.connect_selected_notify(move |item| mark.set_visible(item.is_selected()));
        item.set_child(Some(&line));
    });
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
        let Some(line) = item.child().and_downcast::<gtk::Box>() else { return };
        let Some(choice) = systems.get(item.position() as usize) else { return };
        if let Some(name) = line.first_child().and_downcast::<gtk::Label>() {
            name.set_text(&choice.name);
            name.set_sensitive(choice.open);
        }
        if let Some(note) = line.first_child().and_then(|name| name.next_sibling()) {
            note.set_visible(!choice.open);
        }
    });
    factory
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

    /// Rithmic Test is open and first; Rithmic 01, Rithmic 04 Colo and
    /// Rithmic Paper Trading are listed with their gateways, and closed.
    #[test]
    fn rithmic_test_is_open_and_production_is_listed() {
        let listed = choices(TEST_SYSTEM, TEST_GATEWAY);
        let names: Vec<&str> = listed.iter().map(|choice| choice.name.as_str()).collect();
        assert_eq!(names, ["Rithmic Test", "Rithmic 01", "Rithmic 04 Colo", "Rithmic Paper Trading"]);
        assert!(listed[0].open && listed[1..].iter().all(|choice| !choice.open));
        assert_eq!(listed[0].gateways, vec![("Chicago Area".to_string(), TEST_GATEWAY.to_string())]);
        assert_eq!(listed[1].gateways.len(), 12);
        assert_eq!(listed[3].gateways.len(), 11);
        assert!(listed[1..].iter().all(|choice| choice.gateways.iter().all(|(_, url)| url.is_empty())));
    }

    /// A login saved on a system or gateway the list does not have keeps
    /// it, open.
    #[test]
    fn a_saved_system_or_gateway_is_kept() {
        let lucid = choices("LucidTrading", "wss://rprotocol.example.com:443");
        let last = lucid.last().unwrap();
        assert_eq!(last.name, "LucidTrading");
        assert!(last.open);
        assert_eq!(last.gateways, vec![("rprotocol.example.com".to_string(), "wss://rprotocol.example.com:443".to_string())]);

        let elsewhere = choices(TEST_SYSTEM, "wss://other.example.com:443");
        assert_eq!(elsewhere[0].gateways.len(), 2);
        assert_eq!(elsewhere.len(), 4);

        let before = choices("Rithmic Paper Trading", "wss://paper.example.com:443");
        assert!(before[3].open);
        assert_eq!(before[3].gateways[0].1, "wss://paper.example.com:443");
    }
}
