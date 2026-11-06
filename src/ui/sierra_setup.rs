//! The dialog Sierra Chart is set up in: its folder, found or browsed to,
//! and the port its DTC server listens on, each checked as it is typed.
//!
//! The folder is the feed. It is checked on every change — a `read_dir` —
//! and the line under it says what it holds or what is wrong, so "Set up"
//! is pressable only for a folder that charts. The DTC server is optional:
//! it lists contracts and when they expire, which the files cannot. Whether
//! it is listening is asked on a thread, and the steps that turn it on are
//! shown only while it is not.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::providers::sierra::setup::{self, FOLDER, PORT};
use omacharts_engine::providers::Saved;

pub const SET_UP: &str = "set-up";
const CANCEL: &str = "cancel";

/// The steps that turn the DTC server on, in Sierra Chart's own words.
const SERVER_STEPS: &[(&str, &str)] = &[
    ("In Sierra Chart", "Global Settings → Sierra Chart Server Settings → DTC Protocol Server"),
    ("Enable DTC Protocol Server", "Yes"),
    ("Listening Port", "the port above"),
    ("Encoding", "Automatic"),
    ("Allowed Incoming IPs", "Local Computer Only"),
];

/// For charts that keep up with the market: how often Sierra Chart writes
/// what it hears to its files.
const FLUSH: (&str, &str) =
    ("Intraday File Flush Time in Milliseconds", "Global Settings → Data/Trade Service Settings; 250 keeps charts live");

/// The dialog, filled in from `known`, or from the first folder found on
/// this machine when nothing is known yet. Hands what it is set up with to
/// `done`.
pub fn dialog(known: Saved, done: Box<dyn Fn(Saved)>) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some("Set up Sierra Chart"), None);

    let list = gtk::ListBox::new();
    list.add_css_class("boxed-list");
    list.set_selection_mode(gtk::SelectionMode::None);

    let folder = adw::EntryRow::new();
    folder.set_title("Folder");
    let found = setup::candidates();
    let start = known
        .value(FOLDER)
        .map(str::to_string)
        .or_else(|| found.first().map(|first| setup::shown(first)))
        .unwrap_or_default();
    folder.set_text(&start);
    folder.add_suffix(&browse_button(&folder));
    if found.len() > 1 {
        folder.add_suffix(&found_menu(&folder, &found));
    }
    list.append(&folder);

    let holds = status_row();
    list.append(&holds);

    let port = adw::EntryRow::new();
    port.set_title("DTC port");
    port.set_input_purpose(gtk::InputPurpose::Digits);
    port.set_text(known.value(PORT).unwrap_or(&setup::DEFAULT_PORT.to_string()));
    list.append(&port);

    let server = status_row();
    list.append(&server);

    // The steps, open while the server is not listening.
    let steps = adw::ExpanderRow::new();
    steps.set_title("Turn on the DTC server");
    for (setting, value) in SERVER_STEPS {
        steps.add_row(&step_row(setting, value));
    }
    // Only once there is trading to allow.
    if crate::features::trading() {
        steps.add_row(&step_row("Allow Trading", "Yes, to trade through Sierra Chart"));
    }
    list.append(&steps);

    let live = adw::ExpanderRow::new();
    live.set_title("Live charts");
    live.add_row(&step_row(FLUSH.0, FLUSH.1));
    list.append(&live);

    dialog.set_extra_child(Some(&list));
    dialog.add_responses(&[(CANCEL, "Cancel"), (SET_UP, "Set up")]);
    dialog.set_response_appearance(SET_UP, adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some(SET_UP));
    dialog.set_close_response(CANCEL);

    // The folder, checked as it is typed: what it holds, or what is wrong.
    let check_folder = {
        let (dialog, folder, holds) = (dialog.clone(), folder.clone(), holds.clone());
        move || {
            let text = folder.text();
            let checked = match text.trim() {
                "" => Err("Choose Sierra Chart's folder".to_string()),
                path => setup::check(&setup::expand(path)),
            };
            dialog.set_response_enabled(SET_UP, checked.is_ok());
            show(&holds, checked);
        }
    };
    check_folder();
    folder.connect_changed(move |_| check_folder());

    // The server, asked on a thread whenever the port changes; only the
    // latest answer is shown.
    let asked = Rc::new(Cell::new(0u64));
    let ask_server = {
        let (port, server, steps) = (port.clone(), server.clone(), steps.clone());
        move || {
            let Ok(number) = port.text().trim().parse::<u16>() else {
                show(&server, Err("Not a port".into()));
                steps.set_expanded(true);
                return;
            };
            let ask = asked.get() + 1;
            asked.set(ask);
            server.set_title("Asking…");
            server.set_subtitle("");
            let (tell, heard) = async_channel::bounded(1);
            std::thread::Builder::new()
                .name("omacharts dtc probe".into())
                .spawn(move || {
                    let _ = tell.send_blocking(setup::listening(number));
                })
                .ok();
            let (server, steps, asked) = (server.clone(), steps.clone(), asked.clone());
            glib::spawn_future_local(async move {
                let Ok(answer) = heard.recv().await else { return };
                if asked.get() != ask {
                    return;
                }
                steps.set_expanded(answer.is_err());
                show(&server, answer);
            });
        }
    };
    ask_server();
    port.connect_changed(move |_| ask_server());

    dialog.connect_response(None, move |_, response| {
        if response != SET_UP {
            return;
        }
        let saved = known.clone().with(FOLDER, &folder.text()).with(PORT, &port.text());
        done(saved);
    });
    dialog
}

/// A row that says how something stands, filled in by [`show`].
fn status_row() -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.add_css_class("property");
    row
}

/// `answer` on `row`: what is right as it is, what is wrong dimmed as an
/// error.
fn show(row: &adw::ActionRow, answer: Result<String, String>) {
    match answer {
        Ok(words) => {
            row.remove_css_class("error");
            row.set_title(&words);
        }
        Err(words) => {
            row.add_css_class("error");
            row.set_title(&words);
        }
    }
    row.set_subtitle("");
}

fn step_row(setting: &str, value: &str) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(setting);
    row.set_subtitle(value);
    row.add_css_class("property");
    row
}

/// A folder picker, which puts what was picked in `entry`.
fn browse_button(entry: &adw::EntryRow) -> gtk::Button {
    let button = gtk::Button::from_icon_name("folder-open-symbolic");
    button.set_valign(gtk::Align::Center);
    button.add_css_class("flat");
    button.set_tooltip_text(Some("Browse"));
    let entry = entry.clone();
    button.connect_clicked(move |button| {
        let picker = gtk::FileDialog::new();
        picker.set_title("Sierra Chart's folder");
        let current = setup::expand(entry.text().trim());
        if current.is_dir() {
            picker.set_initial_folder(Some(&gio::File::for_path(&current)));
        }
        let parent = button.root().and_downcast::<gtk::Window>();
        let entry = entry.clone();
        picker.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |answer| {
            if let Some(path) = answer.ok().and_then(|file| file.path()) {
                entry.set_text(&setup::shown(&path));
            }
        });
    });
    button
}

/// The other folders found on this machine, one press each.
fn found_menu(entry: &adw::EntryRow, found: &[PathBuf]) -> gtk::MenuButton {
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("pan-down-symbolic");
    menu.set_valign(gtk::Align::Center);
    menu.add_css_class("flat");
    menu.set_tooltip_text(Some("Found on this computer"));
    let items = found
        .iter()
        .map(|path| {
            let shown = setup::shown(path);
            let entry = entry.clone();
            let label = shown.clone();
            crate::ui::menus::Item::new(&label, move || entry.set_text(&shown))
        })
        .collect();
    crate::ui::menus::actions(&menu, "sierra-folder", items);
    menu
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder that is not Sierra Chart's cannot be set up, and one that
    /// is hands back the folder and the port it was given.
    #[test]
    fn only_a_sierra_chart_folder_can_be_set_up() {
        crate::ui::on_gtk(|| {
            let root = std::env::temp_dir().join(format!("omacharts-sierra-setup-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Data")).unwrap();
            std::fs::write(root.join("Sierra4.config"), b"").unwrap();
            let handed = Rc::new(std::cell::RefCell::new(None));
            let kept = handed.clone();
            let known = Saved::default().with(FOLDER, &root.join("nowhere").display().to_string()).with(PORT, "1");
            let dialog = dialog(known, Box::new(move |saved| *kept.borrow_mut() = Some(saved)));
            assert!(!dialog.is_response_enabled(SET_UP), "not a Sierra Chart folder");
            let list = dialog.extra_child().and_downcast::<gtk::ListBox>().expect("its rows");
            let folder = list.first_child().and_downcast::<adw::EntryRow>().expect("the folder");
            folder.set_text(&root.display().to_string());
            assert!(dialog.is_response_enabled(SET_UP));
            dialog.emit_by_name::<()>("response", &[&SET_UP]);
            let saved = handed.borrow_mut().take().expect("handed over");
            assert_eq!(saved.value(FOLDER), Some(root.display().to_string().as_str()));
            assert_eq!(saved.value(PORT), Some("1"));
            let _ = std::fs::remove_dir_all(&root);
        });
    }
}
