//! The dialog a Rithmic login is typed into: a user ID and password, and
//! the system and gateway picked from Rithmic's, with Rithmic's artwork
//! under them (see `rithmic_attribution`).
//!
//! The systems are the ones Rithmic's gateway names (see
//! `rithmic_systems`): those last kept show at once, and a list a week old
//! is asked for again while the dialog is open. A login is called what its
//! system is, numbered past the names taken: Tradeify, Tradeify 2.
//!
//! Prefilled from the login being signed in again, except the password,
//! which is never read back out of the keyring to be shown. Left empty, the
//! password already kept is the one used.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::trading::services::rithmic_systems;
use crate::trading::{self, Login, Typed};
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
pub fn ask(parent: &impl IsA<gtk::Widget>, known: Option<&Login>, taken: &[String], signed: impl Fn(Typed) + 'static) {
    let dialog = adw::AlertDialog::new(Some("Sign in to Rithmic"), None);
    dialog.set_body(match session::development() {
        true => "A development login is in use, so its file signs in and nothing typed here goes \
                 to the keyring.",
        false => "Your password is kept in the desktop keyring and sent only to Rithmic.",
    });

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    let saved_system = known.and_then(|login| login.value(SYSTEM)).unwrap_or(TEST_SYSTEM).to_string();
    let saved_gateway = known.and_then(|login| login.value(GATEWAY)).unwrap_or(TEST_GATEWAY).to_string();
    let name = namer(known, taken, &saved_system);

    let user = adw::EntryRow::new();
    user.set_title("User ID");
    user.set_text(known.map_or("", |login| login.user.as_str()));
    list.append(&user);

    let password = adw::PasswordEntryRow::new();
    password.set_title("Password");
    list.append(&password);

    // Rithmic's systems and their gateways, picked rather than typed.
    let systems = Rc::new(RefCell::new(choices(
        &saved_system,
        &saved_gateway,
        &production(rithmic_systems::kept()),
    )));
    let system = adw::ComboRow::new();
    system.set_title("System");
    // Rithmic names a few dozen, so the list can be searched by name.
    system.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    system.set_enable_search(true);
    list.append(&system);

    let gateway = adw::ComboRow::new();
    gateway.set_title("Gateway");
    list.append(&gateway);

    // Put the systems in the list, with `picked` and its gateway chosen.
    // Filling the list says nothing to the handlers below.
    let filling = Rc::new(Cell::new(false));
    let fill = {
        let (systems, system, gateway, filling) = (systems.clone(), system.clone(), gateway.clone(), filling.clone());
        move |picked: &str, url: Option<&str>| {
            filling.set(true);
            let systems = systems.borrow();
            let names: Vec<&str> = systems.iter().map(|choice| choice.name.as_str()).collect();
            system.set_model(Some(&gtk::StringList::new(&names)));
            let at = systems.iter().position(|choice| choice.name == picked).unwrap_or(0);
            system.set_selected(at as u32);
            show_gateways(&systems[at], &gateway, url);
            filling.set(false);
        }
    };
    fill(&saved_system, Some(&saved_gateway));

    // A list a week old, or none, is asked for again; what comes back takes
    // its place, with what is chosen kept chosen.
    if rithmic_systems::stale() && !cfg!(test) {
        let (tell, heard) = async_channel::bounded(1);
        std::thread::Builder::new()
            .name("omacharts rithmic systems".into())
            .spawn(move || {
                let _ = tell.send_blocking(rithmic_systems::ask());
            })
            .ok();
        let (systems, system, gateway) = (systems.clone(), system.clone(), gateway.clone());
        let (saved_system, saved_gateway) = (saved_system.clone(), saved_gateway.clone());
        glib::spawn_future_local(async move {
            let Ok(Ok(asked)) = heard.recv().await else { return };
            let (picked, url) = {
                let systems = systems.borrow();
                let choice = &systems[(system.selected() as usize).min(systems.len() - 1)];
                let url = choice.gateways.get(gateway.selected() as usize).map(|(_, url)| url.clone());
                (choice.name.clone(), url)
            };
            *systems.borrow_mut() = choices(&saved_system, &saved_gateway, &production(Some(asked)));
            fill(&picked, url.as_deref());
        });
    }

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
    let ready = {
        let dialog = dialog.clone();
        let user = user.clone();
        move || {
            let enough = session::development() || !user.text().trim().is_empty();
            dialog.set_response_enabled(SIGN_IN, enough);
        }
    };
    ready();
    user.connect_changed(move |_| ready());
    {
        let (systems, gateway) = (systems.clone(), gateway.clone());
        system.connect_selected_notify(move |system| {
            if filling.get() {
                return;
            }
            if let Some(choice) = systems.borrow().get(system.selected() as usize) {
                show_gateways(choice, &gateway, None);
            }
        });
    }

    let signed = Rc::new(signed);
    dialog.connect_response(None, move |_, response| {
        if response != SIGN_IN {
            return;
        }
        let systems = systems.borrow();
        let Some(choice) = systems.get(system.selected() as usize) else { return };
        let url = choice.gateways.get(gateway.selected() as usize).map_or("", |(_, url)| url.as_str());
        signed(typed(&name(&choice.name), &user.text(), &password.text(), &choice.name, url));
        // Nothing of the password is kept in the widget once it is handed
        // over.
        password.set_text("");
    });
    dialog.present(Some(parent));
}

/// What a login signed in on a system is called: a new one, or one called
/// after the system it was on, is called after the one chosen, numbered past
/// the names taken; one somebody named before keeps its name.
fn namer(known: Option<&Login>, taken: &[String], saved_system: &str) -> impl Fn(&str) -> String + 'static {
    let taken: Vec<String> = taken.to_vec();
    let after = move |system: &str| trading::numbered(system, &taken);
    let kept = known.map(|login| login.name.clone()).filter(|name| *name != after(saved_system));
    move |system: &str| kept.clone().unwrap_or_else(|| after(system))
}

/// Show `choice`'s gateways in `gateway`, with `url` chosen if it is one.
fn show_gateways(choice: &Choice, gateway: &adw::ComboRow, url: Option<&str>) {
    let names: Vec<&str> = choice.gateways.iter().map(|(name, _)| name.as_str()).collect();
    gateway.set_model(Some(&gtk::StringList::new(&names)));
    let at = url.and_then(|url| choice.gateways.iter().position(|(_, known)| known == url)).unwrap_or(0);
    gateway.set_selected(at as u32);
}

/// The production systems to offer, from what Rithmic's gateway named:
/// Rithmic 01 and Rithmic Paper Trading first, then the firms' own by name.
/// With nothing named yet, those two. Rithmic 04 Colo is for colocated
/// servers, not a desktop, and is left out.
fn production(named: Option<Vec<String>>) -> Vec<String> {
    const FIRST: [&str; 2] = ["Rithmic 01", "Rithmic Paper Trading"];
    let named = named.unwrap_or_else(|| FIRST.iter().map(|name| name.to_string()).collect());
    let mut rest: Vec<String> = named
        .iter()
        .filter(|name| !FIRST.contains(&name.as_str()) && *name != "Rithmic 04 Colo" && *name != TEST_SYSTEM)
        .cloned()
        .collect();
    rest.sort_by_key(|name| name.to_lowercase());
    rest.dedup();
    FIRST.iter().filter(|first| named.iter().any(|name| name == *first)).map(|first| first.to_string()).chain(rest).collect()
}

/// Rithmic's production gateways, as Rithmic named them when omacharts
/// passed its conformance: every production system, Rithmic 01 and Rithmic
/// Paper Trading among them, is reached through any of them.
const PRODUCTION: [(&str, &str); 13] = [
    ("Chicago", "wss://rprotocol.rithmic.com:443"),
    ("New York", "wss://rprotocol-nyc.rithmic.com:443"),
    ("Colo75", "wss://rprotocol-colo75.rithmic.com:443"),
    ("Frankfurt", "wss://rprotocol-de.rithmic.com:443"),
    ("Ireland", "wss://rprotocol-ie.rithmic.com:443"),
    ("Sydney", "wss://rprotocol-au.rithmic.com:443"),
    ("S\u{e3}o Paulo", "wss://rprotocol-br.rithmic.com:443"),
    ("Hong Kong", "wss://rprotocol-hk.rithmic.com:443"),
    ("Mumbai", "wss://rprotocol-in.rithmic.com:443"),
    ("Seoul", "wss://rprotocol-kr.rithmic.com:443"),
    ("Tokyo", "wss://rprotocol-jp.rithmic.com:443"),
    ("Singapore", "wss://rprotocol-sg.rithmic.com:443"),
    ("Cape Town", "wss://rprotocol-za.rithmic.com:443"),
];

/// A Rithmic system and its gateways, each gateway's name and address.
#[derive(Debug, PartialEq)]
struct Choice {
    name: String,
    gateways: Vec<(String, String)>,
}

/// The systems the dialog offers, Rithmic Test first and then `production`,
/// each with Rithmic's gateways, and the system and gateway a login was
/// saved with among them: a system or gateway not listed is kept under its
/// own name, so no login stops working because it is not in the list.
fn choices(saved_system: &str, saved_gateway: &str, production: &[String]) -> Vec<Choice> {
    let gateways: Vec<(String, String)> =
        PRODUCTION.iter().map(|(name, url)| (name.to_string(), url.to_string())).collect();
    let mut choices = vec![Choice { name: TEST_SYSTEM.into(), gateways: vec![("Chicago".into(), TEST_GATEWAY.into())] }];
    choices.extend(production.iter().map(|name| Choice { name: name.clone(), gateways: gateways.clone() }));
    let gateway = || (host(saved_gateway).to_string(), saved_gateway.to_string());
    match choices.iter_mut().find(|choice| choice.name == saved_system) {
        Some(choice) => {
            if !choice.gateways.iter().any(|(_, url)| url == saved_gateway) {
                choice.gateways.push(gateway());
            }
        }
        None => choices.push(Choice { name: saved_system.into(), gateways: vec![gateway()] }),
    }
    choices
}

/// A gateway's address without its scheme and port, to name one Rithmic
/// does not list.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split([':', '/']).next().filter(|host| !host.is_empty()).unwrap_or(url)
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
        service: rithmic::LISTED.id.to_string(),
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
        assert_eq!(empty.service, "rithmic");
        assert_eq!(empty.name, "Rithmic Test");
        assert_eq!(
            empty.values,
            vec![("system".to_string(), TEST_SYSTEM.to_string()), ("gateway".to_string(), TEST_GATEWAY.to_string())]
        );
        let named = typed("Lucid", "trader", "secret", "LucidTrading", "wss://x");
        assert_eq!((named.name.as_str(), named.values[0].1.as_str()), ("Lucid", "LucidTrading"));
    }

    /// A new login is called what its system is, numbered past the names
    /// taken, and the dialog has no name to type.
    #[test]
    fn a_new_login_is_named_after_its_system_numbered_past_the_names_taken() {
        crate::ui::on_gtk(|| {
            let ask: super::super::LoginDialog = |parent, known, taken, signed| ask(parent, known, taken, signed);
            let (dialog, signed) = super::super::opened(ask, None, &["Rithmic Test"]);
            let under: &gtk::Widget = dialog.upcast_ref();
            assert!(super::super::row_titled::<adw::EntryRow>(under, "Name").is_none(), "no name to type");
            let user = super::super::row_titled::<adw::EntryRow>(under, "User ID").expect("its user");
            user.set_text("trader");
            dialog.emit_by_name::<()>("response", &[&SIGN_IN]);
            assert_eq!(signed.borrow().as_ref().map(|typed| typed.name.clone()).as_deref(), Some("Rithmic Test 2"));

            let system = super::super::row_titled::<adw::ComboRow>(under, "System").expect("its system");
            system.set_selected(1);
            dialog.emit_by_name::<()>("response", &[&SIGN_IN]);
            let signed = signed.borrow();
            let typed = signed.as_ref().expect("signed");
            assert_eq!(typed.name, "Rithmic 01");
            assert_eq!(typed.values[1].1, "wss://rprotocol.rithmic.com:443", "Chicago, first");
            dialog.force_close();
        });
    }

    /// A login somebody named keeps its name; one called after its system
    /// follows the system chosen.
    #[test]
    fn a_named_login_keeps_its_name() {
        let login = |name: &str| crate::trading::fake::tests::test_login("a", name, "Tradeify");
        let named = login("My funded account");
        assert_eq!(namer(Some(&named), &[], "Tradeify")("Rithmic 01"), "My funded account");
        let plain = login("Tradeify");
        assert_eq!(namer(Some(&plain), &[], "Tradeify")("LucidTrading"), "LucidTrading");
        assert_eq!(namer(None, &["Tradeify".into()], TEST_SYSTEM)("Tradeify"), "Tradeify 2");
    }

    /// Rithmic 01 and Paper Trading come first, the firms after them by
    /// name; Rithmic 04 Colo and Rithmic Test are not among them.
    #[test]
    fn production_puts_rithmic_first_and_the_firms_after() {
        let named = ["Tradeify", "Apex", "Rithmic 04 Colo", "Rithmic Paper Trading", "LucidTrading", "Rithmic 01", "tradesea"];
        let offered = production(Some(named.iter().map(|name| name.to_string()).collect()));
        assert_eq!(offered, ["Rithmic 01", "Rithmic Paper Trading", "Apex", "LucidTrading", "Tradeify", "tradesea"]);
        assert_eq!(production(None), ["Rithmic 01", "Rithmic Paper Trading"]);
    }

    /// Rithmic Test is first, and the production systems follow with
    /// Rithmic's gateways, Chicago first.
    #[test]
    fn rithmic_test_is_first_and_production_follows_with_its_gateways() {
        let listed = choices(TEST_SYSTEM, TEST_GATEWAY, &production(Some(vec!["Tradeify".into()])));
        let names: Vec<&str> = listed.iter().map(|choice| choice.name.as_str()).collect();
        assert_eq!(names, ["Rithmic Test", "Tradeify"]);
        assert_eq!(listed[0].gateways, vec![("Chicago".to_string(), TEST_GATEWAY.to_string())]);
        assert_eq!(listed[1].gateways.len(), 13);
        assert_eq!(listed[1].gateways[0], ("Chicago".to_string(), "wss://rprotocol.rithmic.com:443".to_string()));
        assert!(listed[1].gateways.iter().all(|(_, url)| url.starts_with("wss://rprotocol") && url.ends_with(".rithmic.com:443")));
    }

    /// A login saved on a system or gateway the list does not have keeps
    /// it.
    #[test]
    fn a_saved_system_or_gateway_is_kept() {
        let offered = production(None);
        let lucid = choices("LucidTrading", "wss://rprotocol.example.com:443", &offered);
        let last = lucid.last().unwrap();
        assert_eq!(last.name, "LucidTrading");
        assert_eq!(last.gateways, vec![("rprotocol.example.com".to_string(), "wss://rprotocol.example.com:443".to_string())]);

        let elsewhere = choices(TEST_SYSTEM, "wss://other.example.com:443", &offered);
        assert_eq!(elsewhere[0].gateways.len(), 2);
        assert_eq!(elsewhere.len(), 3);

        let paper = choices("Rithmic Paper Trading", "wss://rprotocol-de.rithmic.com:443", &offered);
        assert_eq!(paper[2].gateways.len(), 13, "a listed gateway is not added twice");
    }
}
