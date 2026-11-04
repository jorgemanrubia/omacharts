//! Preferences → Trading: the broker logins, each with its accounts under
//! it, edited the way the Data page edits its data feeds.
//!
//! With nothing configured it is a line and a button. "Add a broker…"
//! offers every broker in the registry, and opens that broker's own login
//! dialog, with a Name field; the first login added whose broker has a data
//! feed also adds that feed as the first data feed, through it.
//! Each login says how it stands — live, not signed in, or the broker's own
//! words for a refusal — and which one is the data login; its accounts are
//! the ones its last sign-in listed, cached, so the page draws without
//! connecting.
//!
//! Signing in runs on a thread of its own and reports back over a channel
//! the main loop awaits, because it waits on the broker's gateway. The data
//! login signs in its data session as well as its trading one.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::providers::{self, Secrets};

use crate::log_words;
use crate::store::Store;
use crate::trading::{self, Account, Desk, Login, Standing};
use crate::ui::sign_in_dialogs;

/// What a login's data feed is called, in a sentence about it.
fn feed_label(login: &Login) -> &'static str {
    trading::data_feed_of(login).map_or("broker", |feed| feed.label)
}

/// The keyring a broker keeps its passwords in.
struct Keyring(Option<&'static dyn Secrets>);

impl Secrets for Keyring {
    fn save(&self, login: &str, user: &str, system: &str, password: &str) -> Result<(), String> {
        match self.0 {
            Some(secrets) => secrets.save(login, user, system, password),
            None => Err("there is nowhere to keep a password".into()),
        }
    }

    fn forget(&self, login: &str, user: &str, system: &str) -> Result<(), String> {
        self.0.map_or(Ok(()), |secrets| secrets.forget(login, user, system))
    }
}

/// Ask for a login of the broker `broker` over `anchor` — a new one, or
/// `known` again — in the broker's own dialog.
fn ask(anchor: &gtk::Widget, broker: &str, known: Option<&Login>, signed: impl Fn(trading::Typed) + 'static) {
    if let Some(dialog) = sign_in_dialogs::login_dialog(broker) {
        dialog(anchor, known, Box::new(signed));
    }
}

/// Opens the Trading sizes dialog over the widget it is given.
type EditSizes = Rc<dyn Fn(&gtk::Widget)>;

pub struct Panel {
    store: Rc<Store>,
    desk: Rc<Desk>,
    /// Where passwords go instead of each broker's own keyring: a list in
    /// memory, under test.
    secrets: Option<Rc<dyn Secrets>>,
    /// Has the window chart from the data feeds now stored and redraw its
    /// headers. Called after every change here.
    on_change: Rc<dyn Fn()>,
    /// Draws the Data page again, whose row for the connection follows the
    /// logins.
    data_page: RefCell<Option<Rc<dyn Fn()>>>,
    page: adw::PreferencesPage,
    edit_sizes: RefCell<Option<EditSizes>>,
    groups: RefCell<Vec<adw::PreferencesGroup>>,
    /// The latest word from a sign-in, by login, until the next one.
    notes: RefCell<HashMap<String, String>>,
    /// Keeps the desk's call to redraw alive as long as the page is.
    redraw: RefCell<Option<Rc<dyn Fn()>>>,
}

/// Fills the Trading section.
pub fn build(page: &adw::PreferencesPage, store: Rc<Store>, desk: Rc<Desk>, on_change: Rc<dyn Fn()>) -> Rc<Panel> {
    let panel = Panel::new(page.clone(), store, desk, None, on_change);
    rebuild(&panel);
    panel
}

impl Panel {
    fn new(
        page: adw::PreferencesPage,
        store: Rc<Store>,
        desk: Rc<Desk>,
        secrets: Option<Rc<dyn Secrets>>,
        on_change: Rc<dyn Fn()>,
    ) -> Rc<Panel> {
        let panel = Rc::new(Panel {
            store,
            desk,
            secrets,
            on_change,
            data_page: RefCell::new(None),
            page,
            edit_sizes: RefCell::new(None),
            groups: RefCell::new(Vec::new()),
            notes: RefCell::new(HashMap::new()),
            redraw: RefCell::new(None),
        });
        // A balance arriving, or a session ending by itself, redraws the
        // page while it is open; held weakly both ways.
        let weak = Rc::downgrade(&panel);
        let redraw: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(panel) = weak.upgrade() {
                rebuild(&panel);
            }
        });
        panel.desk.listen(&redraw);
        *panel.redraw.borrow_mut() = Some(redraw);
        panel
    }

    /// What "Edit trading sizes…" opens.
    pub fn set_edit_sizes(self: &Rc<Self>, edit: EditSizes) {
        *self.edit_sizes.borrow_mut() = Some(edit);
        rebuild(self);
    }

    /// The Data page's redraw, for when a login changes what it says.
    pub fn redraw_data_page(&self, redraw: Rc<dyn Fn()>) {
        *self.data_page.borrow_mut() = Some(redraw);
    }

    /// Where the passwords of `broker`'s logins go.
    fn secrets(&self, broker: &str) -> Rc<dyn Secrets> {
        match &self.secrets {
            Some(secrets) => secrets.clone(),
            None => Rc::new(Keyring(trading::broker_for(broker).and_then(|broker| broker.keyring()))),
        }
    }

    fn changed(self: &Rc<Self>) {
        (self.on_change)();
        self.desk.tell();
        rebuild(self);
        if let Some(redraw) = self.data_page.borrow().clone() {
            redraw();
        }
    }
}

/// Open the login dialog for a new login of `broker` over `anchor`; what
/// the Data page's "Add a data feed…" does for a broker connection's feed
/// too.
pub fn add_broker(panel: &Rc<Panel>, broker: &str, anchor: &gtk::Widget) {
    let panel = panel.clone();
    ask(anchor, broker, None, move |typed| {
        match trading::add_login(&panel.store, panel.secrets(&typed.broker).as_ref(), &typed) {
            Ok(id) => {
                panel.changed();
                sign_in(&panel, &id);
            }
            Err(error) => {
                eprintln!("omacharts: {error}");
                panel.changed();
            }
        }
    });
}

fn rebuild(panel: &Rc<Panel>) {
    for group in panel.groups.borrow_mut().drain(..) {
        panel.page.remove(&group);
    }
    let logins = trading::logins(&panel.store);
    let accounts = trading::accounts(&panel.store);
    let data = trading::data_login_id(&panel.store, &logins);
    let default = trading::default_account(&accounts, panel.store.setting(trading::DEFAULT_ACCOUNT).as_deref())
        .map(Account::key);
    let mut groups = Vec::new();
    for login in &logins {
        let theirs: Vec<&Account> = accounts.iter().filter(|account| account.login == login.id).collect();
        let is_data = data.as_deref() == Some(login.id.as_str());
        groups.push(login_group(panel, login, is_data, logins.len() > 1, &theirs, default.as_deref()));
    }
    groups.push(add_group(panel, logins.is_empty()));
    // Sizes are for somebody who trades: only once there is a login.
    if let Some(edit) = panel.edit_sizes.borrow().clone().filter(|_| !logins.is_empty()) {
        groups.push(sizes_group(edit));
    }
    for group in &groups {
        panel.page.add(group);
    }
    *panel.groups.borrow_mut() = groups;
}

/// How a login stands, in words: progress while a sign-in runs, else its
/// standing, and "data" on the data login.
fn subtitle(panel: &Panel, login: &Login, data: bool) -> String {
    let standing = match panel.notes.borrow().get(&login.id) {
        Some(note) => note.clone(),
        None => panel.desk.standing(login, data).words(),
    };
    let mut words: Vec<String> =
        panel.desk.broker_for(login).and_then(|broker| broker.signs_in_to(login)).into_iter().collect();
    words.push(standing);
    if data {
        words.push("data".into());
    }
    words.join(" · ")
}

fn login_group(
    panel: &Rc<Panel>,
    login: &Login,
    data: bool,
    several: bool,
    accounts: &[&Account],
    default: Option<&str>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(&glib::markup_escape_text(&login.name));
    group.set_description(Some(&subtitle(panel, login, data)));

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_valign(gtk::Align::Center);
    // Only the data login feeds charts. Another can take over, which ends
    // the data session and signs charts in through it from then on.
    if several && !data && trading::data_feed_of(login).is_some() {
        let feed = gtk::Button::with_label("Use for data");
        feed.add_css_class("flat");
        feed.set_tooltip_text(Some(&format!("Have charts' {} data come through this login", feed_label(login))));
        let panel_for_feed = panel.clone();
        let id = login.id.clone();
        feed.connect_clicked(move |_| {
            trading::make_data_login(&panel_for_feed.store, &id);
            panel_for_feed.changed();
        });
        actions.append(&feed);
    }
    if panel.desk.opening(&login.id) {
        let spinner = gtk::Spinner::new();
        spinner.start();
        actions.append(&spinner);
    } else {
        actions.append(&action_button(panel, login, data));
    }
    let remove = gtk::Button::with_label("Remove");
    remove.add_css_class("flat");
    let panel_for_remove = panel.clone();
    let id = login.id.clone();
    remove.connect_clicked(move |button| confirm_remove(&panel_for_remove, &id, button.upcast_ref()));
    actions.append(&remove);
    group.set_header_suffix(Some(&actions));

    if accounts.is_empty() {
        let row = adw::ActionRow::new();
        row.set_title("No accounts listed yet");
        row.add_css_class("dim-label");
        group.add(&row);
    }
    for account in accounts {
        group.add(&account_row(panel, account, default == Some(account.key().as_str())));
    }
    group
}

/// The one thing to do about a login's sign-in: Sign out while it is live,
/// Sign in with the password kept, or Sign in… to type one.
fn action_button(panel: &Rc<Panel>, login: &Login, data: bool) -> gtk::Button {
    let standing = panel.desk.standing(login, data);
    let label = match standing {
        Standing::Live => "Sign out",
        Standing::Saved => "Sign in",
        Standing::SignedOut | Standing::Refused(_) => "Sign in…",
    };
    let button = gtk::Button::with_label(label);
    let panel = panel.clone();
    let id = login.id.clone();
    match standing {
        Standing::Live => {
            button.connect_clicked(move |_| sign_out(&panel, &id));
        }
        Standing::Saved => {
            button.add_css_class("suggested-action");
            button.connect_clicked(move |_| sign_in(&panel, &id));
        }
        Standing::SignedOut | Standing::Refused(_) => {
            button.add_css_class("suggested-action");
            button.connect_clicked(move |button| ask_again(&panel, &id, button.upcast_ref()));
        }
    }
    button
}

fn account_row(panel: &Rc<Panel>, account: &Account, default: bool) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_use_markup(false);
    row.set_title(&match &account.name {
        Some(name) if name != &account.id => format!("{} — {name}", account.id),
        _ => account.id.clone(),
    });
    let mut words: Vec<String> = Vec::new();
    words.extend(account.currency.clone());
    words.extend(account.balance_text());
    if !words.is_empty() {
        row.set_subtitle(&words.join(" · "));
    }
    if default {
        let tag = gtk::Label::new(Some("Default"));
        tag.add_css_class("dim-label");
        tag.set_valign(gtk::Align::Center);
        row.add_suffix(&tag);
    } else {
        let make = gtk::Button::with_label("Make default");
        make.add_css_class("flat");
        make.set_valign(gtk::Align::Center);
        let panel = panel.clone();
        let key = account.key();
        make.connect_clicked(move |_| {
            trading::set_default_account(&panel.store, &key);
            panel.changed();
        });
        row.add_suffix(&make);
    }
    row
}

/// The size templates every chart picks its trading size from, behind a
/// button.
fn sizes_group(edit: EditSizes) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Trading sizes");
    let button = gtk::Button::with_label("Edit trading sizes…");
    button.set_halign(gtk::Align::Start);
    button.connect_clicked(move |button| edit(button.upcast_ref()));
    group.add(&button);
    group
}

fn add_group(panel: &Rc<Panel>, empty: bool) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    if empty {
        group.set_title("Accounts");
        group.set_description(Some(&format!(
            "Add the broker login a prop firm gave you to see its accounts here. The first one \
             also becomes your {} data feed.",
            providers::connections().next().map_or("broker", |feed| feed.label),
        )));
    }
    let add = gtk::MenuButton::new();
    add.set_label("Add a broker…");
    add.set_halign(gtk::Align::Start);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    for broker in panel.desk.brokers().iter() {
        let choice = gtk::Button::with_label(broker.label());
        choice.add_css_class("flat");
        if let Some(label) = choice.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
        }
        let shut = popover.clone();
        let anchor = add.clone();
        let panel = panel.clone();
        let id = broker.id();
        choice.connect_clicked(move |_| {
            shut.popdown();
            add_broker(&panel, id, anchor.upcast_ref());
        });
        choices.append(&choice);
    }
    popover.set_child(Some(&choices));
    add.set_popover(Some(&popover));
    group.add(&add);
    group
}

/// The dialog again, prefilled, for a login with no password to sign in
/// with or one the broker refused.
fn ask_again(panel: &Rc<Panel>, id: &str, anchor: &gtk::Widget) {
    let Some(login) = trading::logins(&panel.store).into_iter().find(|login| login.id == id) else { return };
    let panel = panel.clone();
    let id = id.to_string();
    let secrets = panel.secrets(&login.broker);
    ask(anchor, &login.broker, Some(&login), move |typed| {
        if let Err(error) = trading::retype_login(&panel.store, secrets.as_ref(), &id, &typed) {
            panel.notes.borrow_mut().insert(id.clone(), format!("not saved · {error}"));
        }
        panel.changed();
        sign_in(&panel, &id);
    });
}

fn sign_out(panel: &Rc<Panel>, id: &str) {
    panel.desk.end(id);
    panel.notes.borrow_mut().remove(id);
    let Some(login) = trading::logins(&panel.store).into_iter().find(|login| login.id == id) else { return };
    match trading::sign_out_login(&panel.store, panel.secrets(&login.broker).as_ref(), id) {
        Ok(_) => log_words::say(log_words::signed_out(&login.name)),
        Err(error) => {
            eprintln!("omacharts: {error}");
            log_words::say(log_words::trouble(&login.name, "Password not forgotten", &error));
        }
    }
    panel.changed();
}

/// Ask before removing a login, saying what becomes of the data feed when
/// market data came through it: another login takes it over, or the
/// trader chooses whether the data feed goes too, or it stays
/// because it is the last data feed.
fn confirm_remove(panel: &Rc<Panel>, id: &str, anchor: &gtk::Widget) {
    let Some(login) = trading::logins(&panel.store).into_iter().find(|login| login.id == id) else { return };
    let removal = trading::removal(&panel.store, id);
    let (heading, body) = removal_words(&login.name, feed_label(&login), &removal);
    let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
    dialog.add_response("cancel", "Cancel");
    match removal {
        trading::Removal::Ask => {
            dialog.add_response("keep", "Keep data feed");
            dialog.add_response("both", "Remove both");
            dialog.set_response_appearance("both", adw::ResponseAppearance::Destructive);
        }
        _ => {
            dialog.add_response("remove", "Remove");
            dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        }
    }
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    let panel = panel.clone();
    let id = id.to_string();
    dialog.connect_response(None, move |_, response| match response {
        "remove" | "keep" => remove_login(&panel, &id, false),
        "both" => remove_login(&panel, &id, true),
        _ => {}
    });
    dialog.present(Some(anchor));
}

/// The dialog's heading and body for removing the login `name`.
fn removal_words(name: &str, feed: &str, removal: &trading::Removal) -> (String, String) {
    let body = match removal {
        trading::Removal::Login => "Its password leaves the keyring and its accounts leave this page.".to_string(),
        trading::Removal::HandOver { to } => format!(
            "Its password leaves the keyring and its accounts leave this page. Charts' {feed} data will \
             now come through {to}."
        ),
        trading::Removal::Ask => format!(
            "Charts' {feed} data comes through this login. Remove the {feed} data feed too? Kept, it \
             stays on the Data page, not signed in, ready to be signed in again."
        ),
        trading::Removal::KeepFeed => format!(
            "Charts' {feed} data comes through this login. The {feed} data feed stays on the Data page, \
             not signed in, because it is the last data feed."
        ),
    };
    let heading = match removal {
        trading::Removal::Ask => format!("Remove the {feed} data feed too?"),
        _ => format!("Remove {name}?"),
    };
    (heading, body)
}

fn remove_login(panel: &Rc<Panel>, id: &str, feed_too: bool) {
    panel.desk.end(id);
    panel.notes.borrow_mut().remove(id);
    let Some(login) = trading::logins(&panel.store).into_iter().find(|login| login.id == id) else { return };
    if let Err(error) = trading::remove_login(&panel.store, panel.secrets(&login.broker).as_ref(), id, feed_too) {
        eprintln!("omacharts: {error}");
        log_words::say(log_words::trouble(&login.name, "Login not removed completely", &error));
    }
    panel.changed();
}

/// Sign a login in off the main loop: its trading session, refreshed with
/// what its accounts hold, and for the data login its data session too.
/// One at a time per login: the desk refuses a second while one is open or
/// opening.
fn sign_in(panel: &Rc<Panel>, id: &str) {
    let logins = trading::logins(&panel.store);
    let Some(login) = logins.iter().find(|login| login.id == id).cloned() else { return };
    let Some(broker) = panel.desk.broker_for(&login) else {
        panel.notes.borrow_mut().insert(id.to_string(), format!("no broker called {} in this build", login.broker));
        return rebuild(panel);
    };
    if !panel.desk.begin(id) {
        return;
    }
    let data = trading::data_login_id(&panel.store, &logins).as_deref() == Some(id);
    panel.notes.borrow_mut().insert(id.to_string(), "signing in".into());
    rebuild(panel);

    let waker = panel.desk.waker(id);
    let name = login.name.clone();
    let feed = trading::data_feed_of(&login).map(|feed| feed.id);
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let session = broker.sign_in(&login, waker);
        // The accounts as they stand, before the session is shown at all.
        if let Ok(session) = &session {
            let _ = session.refresh();
        }
        let fed = match (data, &session, feed) {
            (true, Ok(_), Some(feed)) => Some(crate::feeds::sign_in_now(feed, |_| {})),
            _ => None,
        };
        let _ = sender.send_blocking((session, fed));
    });

    let panel = panel.clone();
    let id = id.to_string();
    glib::spawn_future_local(async move {
        let Ok((session, fed)) = receiver.recv().await else { return };
        panel.notes.borrow_mut().remove(&id);
        log_words::say(log_words::signed_in(
            &name,
            session.as_ref().map(|session| session.accounts().len()).map_err(String::as_str),
        ));
        if let Some(Err(error)) = fed {
            log_words::say(log_words::data_not_signed_in(&name, &error));
            panel.notes.borrow_mut().insert(id.clone(), format!("data not signed in · {error}"));
        }
        panel.desk.opened(&id, session);
        panel.desk.note(&panel.store);
        panel.changed();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::{FakeBroker, Kept};
    use crate::trading::Registry;
    use crate::trading::Typed;

    fn panel(store: &Rc<Store>, desk: Rc<Desk>) -> Rc<Panel> {
        let panel =
            Panel::new(adw::PreferencesPage::new(), store.clone(), desk, Some(Rc::new(Kept::default())), Rc::new(|| {}));
        panel.set_edit_sizes(Rc::new(|_: &gtk::Widget| {}));
        panel
    }

    fn words_under(widget: &gtk::Widget) -> Vec<String> {
        let mut found = Vec::new();
        let mut next = widget.first_child();
        while let Some(child) = next {
            if let Ok(label) = child.clone().downcast::<gtk::Label>() {
                found.push(label.label().to_string());
            }
            if let Ok(button) = child.clone().downcast::<gtk::Button>()
                && let Some(label) = button.label()
            {
                found.push(label.to_string());
            }
            if let Ok(button) = child.clone().downcast::<gtk::MenuButton>()
                && let Some(label) = button.label()
            {
                found.push(label.to_string());
            }
            found.extend(words_under(&child));
            next = child.next_sibling();
        }
        found
    }

    fn page_words(panel: &Panel) -> Vec<String> {
        panel.groups.borrow().iter().flat_map(|group| {
            let mut words = vec![group.title().to_string(), group.description().unwrap_or_default().to_string()];
            words.extend(words_under(group.upcast_ref()));
            words
        }).collect()
    }

    fn typed(name: &str) -> Typed {
        Typed {
            broker: "rithmic".into(),
            name: name.into(),
            user: format!("{name}-user"),
            password: "PLANTED-PASSWORD".into(),
            values: vec![("system".into(), "Rithmic Test".into()), ("gateway".into(), "wss://example.invalid:443".into())],
        }
    }

    /// Nothing configured: a line and the button, nothing else.
    #[test]
    fn nothing_configured_is_a_line_and_a_button() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let panel = panel(&store, Rc::new(Desk::new(Registry::of(vec![Arc::new(FakeBroker::default())]))));
            rebuild(&panel);
            let words = page_words(&panel);
            assert!(words.iter().any(|w| w == "Add a broker…"), "{words:?}");
            assert!(!words.iter().any(|w| w == "Remove" || w == "Sign in…"), "{words:?}");
            assert!(!words.iter().any(|w| w == "Edit trading sizes…"), "sizes with nothing to trade: {words:?}");
        });
    }

    /// Two logins with their accounts: each says how it stands, the data
    /// login says so and the other offers to be it, and every account but
    /// the default offers to become it.
    #[test]
    fn logins_and_their_accounts_are_listed() {
        if omacharts_engine::providers::rithmic::session::development() {
            return;
        }
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let kept = Kept::default();
            let lucid = trading::add_login(&store, &kept, &typed("Lucid")).unwrap();
            let test = trading::add_login(&store, &kept, &typed("Rithmic Test")).unwrap();
            let mut funded = crate::trading::Account {
                login: lucid.clone(),
                id: "1234".into(),
                name: None,
                currency: Some("USD".into()),
                    balance: Some(50_210.0),
            };
            let other = crate::trading::Account { id: "1235".into(), balance: None, ..funded.clone() };
            trading::cache_accounts(&store, &lucid, vec![funded.clone(), other]);
            funded.login = test.clone();
            funded.id = "951".into();
            trading::cache_accounts(&store, &test, vec![funded]);
            trading::sign_out_login(&store, &kept, &test).unwrap();

            let panel = panel(&store, Rc::new(Desk::new(trading::registry())));
            rebuild(&panel);
            let groups = panel.groups.borrow().clone();
            assert_eq!(groups.len(), 4, "two logins, the button and the sizes");
            assert_eq!(groups[0].title(), "Lucid");
            assert_eq!(groups[0].description().as_deref(), Some("Rithmic Test · signed in · data"));
            assert_eq!(groups[1].description().as_deref(), Some("Rithmic Test · not signed in"));
            let lucid_words = words_under(groups[0].upcast_ref());
            for word in ["1234", "USD · $50,210", "Default", "1235", "Make default", "Sign in", "Remove"] {
                assert!(lucid_words.iter().any(|w| w == word), "{word} missing: {lucid_words:?}");
            }
            assert!(!lucid_words.iter().any(|w| w == "Use for data"), "{lucid_words:?}");
            let test_words = words_under(groups[1].upcast_ref());
            assert!(test_words.iter().any(|w| w == "Use for data"), "{test_words:?}");
            assert!(test_words.iter().any(|w| w == "Sign in…"), "{test_words:?}");
            assert!(page_words(&panel).iter().any(|w| w == "Edit trading sizes…"), "no way to the sizes with a login");

            // Removing the data login hands data to the other, and says so.
            let (_, body) = removal_words("Lucid", "Rithmic", &trading::removal(&store, &lucid));
            assert!(body.ends_with("now come through Rithmic Test."), "{body}");
            remove_login(&panel, &lucid, true);
            let groups = panel.groups.borrow().clone();
            assert_eq!(groups[0].description().as_deref(), Some("Rithmic Test · not signed in · data"));
        });
    }

    /// A broker with no data feed, which nothing above the seam names, is
    /// offered by "Add a broker…" because the registry lists it, signs in
    /// from this page, lists its accounts here, and trades: a market order
    /// fills, with its fill, and is held on the desk's book. Its login is
    /// never the data login, and the data feeds are left as they were.
    #[test]
    fn a_second_broker_signs_in_and_trades_without_being_named() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let desk = Rc::new(Desk::new(trading::registry()));
            let fake = desk.brokers().get("fake").expect("the fake broker is in the registry");
            let panel = panel(&store, desk.clone());
            rebuild(&panel);
            assert!(page_words(&panel).iter().any(|w| w == fake.label()), "not offered: {:?}", page_words(&panel));

            let typed = Typed {
                broker: fake.id().into(),
                name: "Practice".into(),
                user: "me".into(),
                password: "PLANTED-PASSWORD".into(),
                values: Vec::new(),
            };
            let id = trading::add_login(&store, &Kept::default(), &typed).unwrap();
            assert_eq!(trading::data_login(&store), None, "no data feed to come through it");
            assert_eq!(crate::feeds::stored_order(&store), vec!["yahoo"]);

            sign_in(&panel, &id);
            let context = glib::MainContext::default();
            for _ in 0..500 {
                if desk.live(&id) && !panel.notes.borrow().contains_key(&id) {
                    break;
                }
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(desk.live(&id), "signed in through the registry");
            let groups = panel.groups.borrow().clone();
            assert_eq!(groups[0].title(), "Practice");
            assert_eq!(groups[0].description().as_deref(), Some("live"));
            let words = words_under(groups[0].upcast_ref());
            for word in ["FAKE-1", "USD · $50,000", "Sign out"] {
                assert!(words.iter().any(|w| w == word), "{word} missing: {words:?}");
            }
            assert!(!words.iter().any(|w| w == "Use for data"), "{words:?}");

            let session = desk.session(&id).expect("a session");
            let es = crate::trading::fake::tests::instrument("ES", omacharts_engine::InstrumentKind::FutureRoot);
            let contract = session.contract_for(&es).expect("ES trades");
            let order = trading::OrderRequest::request(
                "FAKE-1",
                &contract,
                trading::Side::Buy,
                trading::OrderKind::Market,
                trading::Quantity::ONE,
                None,
            )
            .unwrap();
            let basket = session.place(&order).expect("placed");
            let book = desk.book(&trading::logins(&store));
            let account = format!("{id}/FAKE-1");
            assert_eq!(book.position(&account, "ESZ6").map(|held| held.net), Some(1));
            let placed = book.order(&basket).expect("on the book");
            assert_eq!((placed.status.stage, placed.fills.len()), (trading::Stage::Filled, 1));
            desk.sign_out_all(trading::SIGN_OUT_WAIT);
        });
    }

    /// Signing in is in the Log under the login's name, and a user ID never
    /// is: not from the login, and not from a broker repeating it back.
    #[test]
    fn the_log_names_a_login_and_never_its_user_id() {
        crate::ui::on_gtk(|| {
            const USER: &str = "trader-77421-planted";
            let store = Rc::new(Store::memory().unwrap());
            let desk = Rc::new(Desk::new(trading::registry()));
            let panel = panel(&store, desk.clone());
            let typed = Typed {
                broker: "fake".into(),
                name: "Practice".into(),
                user: USER.into(),
                password: "PLANTED-PASSWORD".into(),
                values: Vec::new(),
            };
            let log = omacharts_engine::log::global();
            let before = log.revision();
            let id = trading::add_login(&store, &Kept::default(), &typed).unwrap();
            sign_in(&panel, &id);
            let context = glib::MainContext::default();
            for _ in 0..500 {
                if desk.live(&id) && !panel.notes.borrow().contains_key(&id) {
                    break;
                }
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(desk.live(&id));
            // A broker that says the user ID back in its refusal.
            log_words::say(log_words::signed_in("Practice", Err(&format!("user {USER} is locked"))));
            sign_out(&panel, &id);

            let said: Vec<String> =
                log.since(before).0.into_iter().filter(|e| e.source == "Practice").map(|e| e.text).collect();
            assert!(said.iter().any(|text| text == "Signed in · 1 account"), "{said:?}");
            assert!(said.iter().any(|text| text.starts_with("Not signed in · user …")), "{said:?}");
            assert!(said.iter().any(|text| text == "Signed out"), "{said:?}");
            for entry in log.entries() {
                let all = format!("{entry:?}");
                assert!(!all.contains(USER), "a user ID in the Log: {all}");
                assert!(!all.contains("PLANTED-PASSWORD"), "a password in the Log: {all}");
            }
            desk.sign_out_all(trading::SIGN_OUT_WAIT);
        });
    }

    /// Each way removing a login can go is said in the dialog: nothing
    /// about data, data handed over, the question with its two answers, and
    /// the last data feed staying.
    #[test]
    fn the_dialog_says_what_becomes_of_the_data_feed() {
        let words = |removal| removal_words("Lucid", "Rithmic", &removal);
        assert_eq!(words(trading::Removal::Login).0, "Remove Lucid?");
        assert!(!words(trading::Removal::Login).1.contains("data"));
        assert!(words(trading::Removal::HandOver { to: "Tradeify".into() }).1.contains("through Tradeify"));
        assert_eq!(words(trading::Removal::Ask).0, "Remove the Rithmic data feed too?");
        assert!(words(trading::Removal::KeepFeed).1.contains("last data feed"));
    }

    use std::sync::Arc;
}
