//! Preferences → Trading: three sections. "Trading accounts" lists the
//! broker logins, each a row with its accounts under it, edited the way the
//! Data page edits its data feeds; "Order entry" holds what every chart's
//! orders follow; "Chart" how the orders are drawn on a chart. The last two
//! are there with no login too, to be set before the first one.
//!
//! With no login, the accounts' section is its heading and a button. "Add a
//! trading account…" offers every broker in the registry, and opens that broker's own login
//! dialog, with a Name field; the first login added whose broker has a data
//! feed also adds that feed as the first data feed, through it.
//! Each login says how it stands — live, not signed in, or the broker's own
//! words for a refusal — and which one is the data login; its accounts are
//! the ones its last sign-in listed, cached, so the page draws without
//! connecting.
//!
//! Signing in is the desk's, on a thread of its own, as at startup; the
//! page redraws when the desk tells it. The data login signs in its data
//! session as well as its trading one.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::providers::Secrets;

use crate::log_words;
use crate::store::Store;
use crate::trading::{self, Account, Desk, Login, Standing};
use crate::ui::chart::LabelSide;
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

/// The keyring `broker`'s passwords go in: what the Data page keeps a
/// broker's data login in while trading is switched off.
pub(crate) fn keyring(broker: &str) -> impl Secrets {
    Keyring(trading::broker_for(broker).and_then(|broker| broker.keyring()))
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

/// Which side of a chart its orders' and position's labels sit at.
pub const ORDER_LABELS: &str = "order_labels";

/// The side the labels sit at: the left unless somebody chose the right.
pub fn order_labels(store: &Store) -> LabelSide {
    store.setting(ORDER_LABELS).and_then(|key| LabelSide::from_key(&key)).unwrap_or_default()
}

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
    /// Puts every chart's labels where the setting now says.
    labels_moved: RefCell<Option<Rc<dyn Fn()>>>,
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
            labels_moved: RefCell::new(None),
            groups: RefCell::new(Vec::new()),
            notes: RefCell::new(HashMap::new()),
            redraw: RefCell::new(None),
        });
        // A sign-in coming back, a balance arriving, or a session ending by
        // itself redraws the page, and the Data page whose row follows the
        // data login, while it is open; held weakly both ways.
        let weak = Rc::downgrade(&panel);
        let redraw: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(panel) = weak.upgrade() {
                rebuild(&panel);
                if let Some(redraw) = panel.data_page.borrow().clone() {
                    redraw();
                }
            }
        });
        panel.desk.listen(&redraw);
        *panel.redraw.borrow_mut() = Some(redraw);
        panel
    }

    /// What "Edit quantities…" opens.
    pub fn set_edit_sizes(self: &Rc<Self>, edit: EditSizes) {
        *self.edit_sizes.borrow_mut() = Some(edit);
        rebuild(self);
    }

    /// What moving the labels does to the charts.
    pub fn set_labels_moved(&self, moved: Rc<dyn Fn()>) {
        *self.labels_moved.borrow_mut() = Some(moved);
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
    let accounts_group = adw::PreferencesGroup::new();
    accounts_group.set_title("Trading accounts");
    for login in &logins {
        let theirs: Vec<&Account> = accounts.iter().filter(|account| account.login == login.id).collect();
        let is_data = data.as_deref() == Some(login.id.as_str());
        accounts_group.add(&login_row(panel, login, is_data, logins.len() > 1, &theirs, default.as_deref()));
    }
    accounts_group.add(&add_button(panel));
    let mut groups = vec![accounts_group];
    if let Some(edit) = panel.edit_sizes.borrow().clone() {
        groups.push(order_entry_group(edit));
    }
    groups.push(chart_group(panel));
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
        None if panel.desk.opening(&login.id) => "signing in".into(),
        None => match panel.desk.data_refused(&login.id).filter(|_| data) {
            Some(why) => format!("data not signed in · {why}"),
            None => panel.desk.standing(login, data).words(),
        },
    };
    let mut words: Vec<String> =
        panel.desk.broker_for(login).and_then(|broker| broker.signs_in_to(login)).into_iter().collect();
    words.push(standing);
    if data {
        words.push("data".into());
    }
    words.join(" · ")
}

/// A login's row, its name and how it stands, with its accounts under it.
fn login_row(
    panel: &Rc<Panel>,
    login: &Login,
    data: bool,
    several: bool,
    accounts: &[&Account],
    default: Option<&str>,
) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::new();
    row.set_title(&glib::markup_escape_text(&login.name));
    row.set_subtitle(&glib::markup_escape_text(&subtitle(panel, login, data)));
    row.set_expanded(true);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    actions.set_valign(gtk::Align::Center);
    // Only the data login feeds charts. Another can take over, which ends
    // the data session and signs charts in through it from then on.
    if several && !data && trading::data_feed_of(login).is_some() {
        let feed = gtk::Button::with_label("Use for data");
        feed.add_css_class("flat");
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
    row.add_suffix(&actions);

    for account in accounts {
        row.add_row(&account_row(panel, account, default == Some(account.key().as_str())));
    }
    row
}

/// The one thing to do about a login's sign-in: Sign out while it is live,
/// Sign in with the password kept, or Sign in… to type one.
fn action_button(panel: &Rc<Panel>, login: &Login, data: bool) -> gtk::Button {
    let standing = panel.desk.standing(login, data);
    let label = match standing {
        Standing::Live => "Sign out",
        Standing::Ready => "Sign in",
        Standing::SignedOut | Standing::Refused(_) => "Sign in…",
    };
    let button = gtk::Button::with_label(label);
    let panel = panel.clone();
    let id = login.id.clone();
    match standing {
        Standing::Live => {
            button.connect_clicked(move |_| sign_out(&panel, &id));
        }
        Standing::Ready => {
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

/// The quantity templates every chart picks its quantity from, behind a
/// button.
fn order_entry_group(edit: EditSizes) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Order entry");
    let button = gtk::Button::with_label("Edit quantities…");
    button.set_halign(gtk::Align::Start);
    button.connect_clicked(move |button| edit(button.upcast_ref()));
    group.add(&button);
    group
}

/// How the orders are drawn on a chart: which side their labels sit at.
fn chart_group(panel: &Rc<Panel>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Chart");
    let row = adw::ComboRow::new();
    row.set_title("Order labels");
    let names: Vec<&str> = LabelSide::ALL.iter().map(|side| side.label()).collect();
    row.set_model(Some(&gtk::StringList::new(&names)));
    let current = order_labels(&panel.store);
    row.set_selected(LabelSide::ALL.iter().position(|side| *side == current).unwrap_or(0) as u32);
    let weak = Rc::downgrade(panel);
    row.connect_selected_notify(move |row| {
        let (Some(panel), Some(side)) = (weak.upgrade(), LabelSide::ALL.get(row.selected() as usize)) else { return };
        if *side == order_labels(&panel.store) {
            return;
        }
        panel.store.set_setting(ORDER_LABELS, side.key());
        if let Some(moved) = panel.labels_moved.borrow().clone() {
            moved();
        }
    });
    group.add(&row);
    group
}

/// "Add a trading account…", last in the accounts' section.
fn add_button(panel: &Rc<Panel>) -> gtk::MenuButton {
    let add = gtk::MenuButton::new();
    add.set_label("Add a trading account…");
    add.set_halign(gtk::Align::Start);
    // Under the logins' list, apart from it, as under the Data page's.
    add.set_margin_top(12);
    let items = panel
        .desk
        .brokers()
        .iter()
        .map(|broker| {
            let anchor = add.downgrade();
            let panel = panel.clone();
            let id = broker.id();
            crate::ui::menus::Item::new(broker.label(), move || {
                if let Some(anchor) = anchor.upgrade() {
                    add_broker(&panel, id, anchor.upcast_ref());
                }
            })
        })
        .collect();
    crate::ui::menus::actions(&add, "broker", items);
    add
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
    let dialog = adw::AlertDialog::new(Some(&heading), body.as_deref());
    dialog.add_response("cancel", "Cancel");
    match removal {
        trading::Removal::Ask => {
            dialog.add_response("keep", "Keep the data feed");
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

/// The dialog's heading and body, if it has one, for removing the login
/// `name`.
fn removal_words(name: &str, feed: &str, removal: &trading::Removal) -> (String, Option<String>) {
    let body = match removal {
        trading::Removal::Login => None,
        trading::Removal::HandOver { to } => Some(format!("Charts' {feed} data will come through {to}.")),
        trading::Removal::Ask => Some("Kept, it stays on the Data page, not signed in.".to_string()),
        trading::Removal::KeepFeed => {
            Some(format!("The {feed} data feed stays on the Data page, not signed in: it is the last one."))
        }
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

/// Sign a login in through the desk, and for the data login its data
/// session too. One at a time per login: the desk refuses a second while
/// one is open or opening.
fn sign_in(panel: &Rc<Panel>, id: &str) {
    let logins = trading::logins(&panel.store);
    let Some(login) = logins.iter().find(|login| login.id == id) else { return };
    if panel.desk.broker_for(login).is_none() {
        panel.notes.borrow_mut().insert(id.to_string(), format!("no broker called {} in this build", login.broker));
        return rebuild(panel);
    }
    panel.notes.borrow_mut().remove(id);
    let data = trading::data_login_id(&panel.store, &logins).as_deref() == Some(id);
    if panel.desk.sign_in(login, data) {
        rebuild(panel);
    }
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

    /// Each login's row, in the order the page lists them.
    fn login_rows(panel: &Panel) -> Vec<adw::ExpanderRow> {
        fn under(widget: &gtk::Widget, found: &mut Vec<adw::ExpanderRow>) {
            let mut next = widget.first_child();
            while let Some(child) = next {
                match child.clone().downcast::<adw::ExpanderRow>() {
                    Ok(row) => found.push(row),
                    Err(_) => under(&child, found),
                }
                next = child.next_sibling();
            }
        }
        let mut found = Vec::new();
        for group in panel.groups.borrow().iter() {
            under(group.upcast_ref(), &mut found);
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

    /// Take what the desk is woken with until `id` is live, as the
    /// window's loop does, for a second at most.
    fn signed_in(store: &Store, desk: &Desk, id: &str) {
        let woken = desk.woken();
        for _ in 0..500 {
            while let Ok(wake) = woken.try_recv() {
                desk.take(wake);
                desk.note(store);
                desk.tell();
            }
            if desk.live(id) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
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

    /// Nothing configured: the accounts' heading and the button, and the
    /// settings, which are there to be set before the first login.
    #[test]
    fn nothing_configured_is_a_heading_a_button_and_the_settings() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let panel = panel(&store, Rc::new(Desk::new(Registry::of(vec![Arc::new(FakeBroker::default())]))));
            rebuild(&panel);
            let words = page_words(&panel);
            assert!(words.iter().any(|w| w == "Trading accounts"), "{words:?}");
            assert!(words.iter().any(|w| w == "Add a trading account…"), "{words:?}");
            assert!(!words.iter().any(|w| w == "Remove" || w == "Sign in…"), "{words:?}");
            let titles: Vec<String> = panel.groups.borrow().iter().map(|group| group.title().to_string()).collect();
            assert_eq!(titles, ["Trading accounts", "Order entry", "Chart"]);
            assert!(words.iter().any(|w| w == "Edit quantities…"), "{words:?}");
        });
    }

    /// "Order labels" is Left until somebody picks Right, which is kept,
    /// and the charts are told at once; picking what it is tells nobody.
    #[test]
    fn the_order_labels_are_left_until_moved_and_moving_them_is_kept() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().unwrap());
            let panel = panel(&store, Rc::new(Desk::new(Registry::of(vec![Arc::new(FakeBroker::default())]))));
            let told = Rc::new(std::cell::Cell::new(0));
            let counted = told.clone();
            panel.set_labels_moved(Rc::new(move || counted.set(counted.get() + 1)));
            assert_eq!(order_labels(&store), LabelSide::Left);
            let combo = |panel: &Panel| {
                let group = panel.groups.borrow().iter().find(|group| group.title() == "Chart").cloned().unwrap();
                fn find(widget: &gtk::Widget) -> Option<adw::ComboRow> {
                    let mut next = widget.first_child();
                    while let Some(child) = next {
                        if let Ok(row) = child.clone().downcast::<adw::ComboRow>() {
                            return Some(row);
                        }
                        if let Some(row) = find(&child) {
                            return Some(row);
                        }
                        next = child.next_sibling();
                    }
                    None
                }
                find(group.upcast_ref()).expect("the Order labels row")
            };
            let row = combo(&panel);
            assert_eq!(row.title(), "Order labels");
            assert!(row.subtitle().is_none_or(|s| s.is_empty()), "no hint under it");
            assert_eq!(row.selected(), 0, "Left");
            row.set_selected(1);
            assert_eq!(order_labels(&store), LabelSide::Right);
            assert_eq!(told.get(), 1, "the charts are told");
            rebuild(&panel);
            assert_eq!(combo(&panel).selected(), 1, "kept");
            assert_eq!(told.get(), 1, "drawing the page again moves nothing");
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
            let titles: Vec<String> = groups.iter().map(|group| group.title().to_string()).collect();
            assert_eq!(titles, ["Trading accounts", "Order entry", "Chart"]);
            assert!(words_under(groups[0].upcast_ref()).iter().any(|w| w == "Add a trading account…"));
            assert!(words_under(groups[1].upcast_ref()).iter().any(|w| w == "Edit quantities…"));
            let rows = login_rows(&panel);
            assert_eq!(rows.len(), 2, "two logins");
            assert_eq!(rows[0].title(), "Lucid");
            assert_eq!(rows[0].subtitle(), "Rithmic Test · not signed in · data");
            assert_eq!(rows[1].subtitle(), "Rithmic Test · not signed in");
            let lucid_words = words_under(rows[0].upcast_ref());
            for word in ["1234", "USD · $50,210", "Default", "1235", "Make default", "Sign in", "Remove"] {
                assert!(lucid_words.iter().any(|w| w == word), "{word} missing: {lucid_words:?}");
            }
            assert!(!lucid_words.iter().any(|w| w == "Use for data"), "{lucid_words:?}");
            let test_words = words_under(rows[1].upcast_ref());
            assert!(test_words.iter().any(|w| w == "Use for data"), "{test_words:?}");
            assert!(test_words.iter().any(|w| w == "Sign in…"), "{test_words:?}");
            assert!(page_words(&panel).iter().any(|w| w == "Edit quantities…"), "no way to the sizes with a login");

            // Removing the data login hands data to the other, and says so.
            let (_, body) = removal_words("Lucid", "Rithmic", &trading::removal(&store, &lucid));
            assert_eq!(body.as_deref(), Some("Charts' Rithmic data will come through Rithmic Test."));
            remove_login(&panel, &lucid, true);
            assert_eq!(login_rows(&panel)[0].subtitle(), "Rithmic Test · not signed in · data");
        });
    }

    /// A broker with no data feed, which nothing above the seam names, is
    /// offered by "Add a trading account…" because the registry lists it,
    /// signs in from this page, lists its accounts here, and trades: a market order
    /// fills, with its fill, and is held on the desk's book. Its login is
    /// never the data login, and the data feeds are left as they were.
    #[test]
    fn a_second_broker_signs_in_and_trades_without_being_named() {
        crate::ui::on_gtk(|| {
            crate::features::force(crate::features::Feature::Trading, true);
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
            signed_in(&store, &desk, &id);
            assert!(desk.live(&id), "signed in through the registry");
            let rows = login_rows(&panel);
            assert_eq!(rows[0].title(), "Practice");
            assert_eq!(rows[0].subtitle(), "live");
            let words = words_under(rows[0].upcast_ref());
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
            crate::features::force(crate::features::Feature::Trading, true);
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
            signed_in(&store, &desk, &id);
            assert!(desk.live(&id));
            // A broker that says the user ID back in its refusal.
            log_words::say(log_words::signed_in("Practice", Err(&format!("user {USER} is locked"))));
            sign_out(&panel, &id);

            let said: Vec<String> =
                log.since(before).0.into_iter().filter(|e| e.source == "Practice").map(|e| e.text).collect();
            assert!(said.iter().any(|text| text == "Signed in · 1 account"), "{said:?}");
            let detail = log.since(before).0.into_iter().find(|e| e.text == "Signed in · 1 account").and_then(|e| e.detail);
            assert_eq!(detail.as_deref(), Some("User: …"), "who signed in, concealed, in the entry's detail");
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
        assert_eq!(words(trading::Removal::Login), ("Remove Lucid?".to_string(), None));
        assert_eq!(
            words(trading::Removal::HandOver { to: "Tradeify".into() }).1.as_deref(),
            Some("Charts' Rithmic data will come through Tradeify.")
        );
        assert_eq!(words(trading::Removal::Ask).0, "Remove the Rithmic data feed too?");
        assert_eq!(words(trading::Removal::Ask).1.as_deref(), Some("Kept, it stays on the Data page, not signed in."));
        assert_eq!(
            words(trading::Removal::KeepFeed).1.as_deref(),
            Some("The Rithmic data feed stays on the Data page, not signed in: it is the last one.")
        );
    }

    use std::sync::Arc;
}
