//! The dialog a Sierra Chart trading login is added in: a name, and where
//! its DTC server is, asked as it is typed whether it is there and allows
//! trading.
//!
//! There is no password. Sierra Chart is signed in to its broker itself,
//! and its server, set to "Local Computer Only", answers this machine
//! alone. The steps that turn trading on in Sierra Chart are shown while
//! the server does not allow it.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::providers::sierra::{self as feed, setup};

use crate::trading::{Login, Typed};

const SIGN_IN: &str = "sign-in";
const CANCEL: &str = "cancel";

/// The settings that let Omacharts trade through Sierra Chart, in its own
/// words.
const STEPS: &[(&str, &str)] = &[
    ("In Sierra Chart", "Global Settings → Sierra Chart Server Settings → DTC Protocol Server"),
    ("Enable DTC Protocol Server", "Yes"),
    ("Allow Trading", "Yes"),
    ("Listening Port", "the port above"),
    ("Allowed Incoming IPs", "Local Computer Only"),
];

/// Ask for a login over `parent` — a new one, or `known` again — and hand
/// what was typed to `signed` when somebody presses Sign in.
pub fn ask(parent: &gtk::Widget, known: Option<&Login>, signed: Box<dyn Fn(Typed)>) {
    let dialog = adw::AlertDialog::new(Some("Trade through Sierra Chart"), None);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    let name = adw::EntryRow::new();
    name.set_title("Name");
    name.set_text(known.map_or("", |login| login.name.as_str()));
    list.append(&name);

    let host = adw::EntryRow::new();
    host.set_title("Host");
    host.set_text(known.and_then(|login| login.value(setup::HOST)).unwrap_or(setup::DEFAULT_HOST));
    list.append(&host);

    let port = adw::EntryRow::new();
    port.set_title("DTC port");
    port.set_input_purpose(gtk::InputPurpose::Digits);
    let default_port = setup::DEFAULT_PORT.to_string();
    port.set_text(known.and_then(|login| login.value(setup::PORT)).unwrap_or(&default_port));
    list.append(&port);

    let server = adw::ActionRow::new();
    server.set_use_markup(false);
    server.add_css_class("property");
    list.append(&server);

    let steps = adw::ExpanderRow::new();
    steps.set_title("Turn trading on in Sierra Chart");
    for (setting, value) in STEPS {
        let row = adw::ActionRow::new();
        row.set_use_markup(false);
        row.set_title(setting);
        row.set_subtitle(value);
        row.add_css_class("property");
        steps.add_row(&row);
    }
    list.append(&steps);

    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[(CANCEL, "Cancel"), (SIGN_IN, "Sign in")]);
    dialog.set_response_appearance(SIGN_IN, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(SIGN_IN));
    dialog.set_close_response(CANCEL);

    // Asked on a thread whenever the host or the port changes; only the
    // latest answer is shown, and Sign in waits for one that trades.
    let asked = Rc::new(Cell::new(0u64));
    let ask_server = {
        let (dialog, host, port, server, steps) = (dialog.clone(), host.clone(), port.clone(), server.clone(), steps.clone());
        move || {
            dialog.set_response_enabled(SIGN_IN, false);
            let Ok(number) = port.text().trim().parse::<u16>() else {
                server.set_title("Not a port");
                server.add_css_class("error");
                return;
            };
            let endpoint = dtc::Endpoint { host: host.text().trim().to_string(), port: number, ..dtc::Endpoint::default() };
            let ask = asked.get() + 1;
            asked.set(ask);
            server.remove_css_class("error");
            server.set_title("Asking…");
            let (tell, heard) = async_channel::bounded(1);
            std::thread::Builder::new()
                .name("omacharts dtc probe".into())
                .spawn(move || {
                    let _ = tell.send_blocking(dtc::probe(&endpoint, std::time::Duration::from_millis(800)));
                })
                .ok();
            let (dialog, server, steps, asked) = (dialog.clone(), server.clone(), steps.clone(), asked.clone());
            glib::spawn_future_local(async move {
                let Ok(probe) = heard.recv().await else { return };
                if asked.get() != ask {
                    return;
                }
                let trades = matches!(&probe, dtc::Probe::Ready(found) if found.trading);
                server.set_title(&probe.to_string());
                match trades {
                    true => server.remove_css_class("error"),
                    false => server.add_css_class("error"),
                }
                steps.set_expanded(!trades);
                dialog.set_response_enabled(SIGN_IN, trades);
            });
        }
    };
    ask_server();
    let again = Rc::new(ask_server);
    let on_host = again.clone();
    host.connect_changed(move |_| on_host());
    port.connect_changed(move |_| again());

    dialog.connect_response(None, move |_, response| {
        if response != SIGN_IN {
            return;
        }
        signed(typed(&name.text(), &host.text(), &port.text()));
    });
    dialog.present(Some(parent));
}

/// What was typed, a login with no name called what it is.
fn typed(name: &str, host: &str, port: &str) -> Typed {
    let or = |text: &str, fallback: &str| match text.trim() {
        "" => fallback.to_string(),
        text => text.to_string(),
    };
    Typed {
        broker: feed::LISTED.id.to_string(),
        name: or(name, feed::LISTED.label),
        user: String::new(),
        password: String::new(),
        values: vec![
            (setup::HOST.into(), or(host, setup::DEFAULT_HOST)),
            (setup::PORT.into(), or(port, &setup::DEFAULT_PORT.to_string())),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Left empty, a login is this machine's server on Sierra Chart's
    /// port, called Sierra Chart, with no password.
    #[test]
    fn what_is_left_empty_is_this_machines_server() {
        let empty = typed(" ", "", "");
        assert_eq!((empty.broker.as_str(), empty.name.as_str(), empty.password.as_str()), ("sierra", "Sierra Chart", ""));
        assert_eq!(empty.values, vec![("host".to_string(), "127.0.0.1".to_string()), ("port".to_string(), "11099".to_string())]);
        let named = typed("Sim", "10.0.0.2", "11100");
        assert_eq!((named.name.as_str(), named.values[1].1.as_str()), ("Sim", "11100"));
    }
}
