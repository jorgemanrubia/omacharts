//! The feeds charts come from, in the order they are tried.
//!
//! A list rather than a choice: each chart comes from the first feed that
//! has its symbol and is signed in, so the order is the setting. Yahoo is a
//! feed like the others, moved and removed the same way; only the last
//! feed left cannot be removed, since with none no chart could draw. A
//! chart of something no remaining feed has says so.
//!
//! What each row says comes from the feeds themselves: what a feed
//! covers, how fresh it is (asked of the provider), whether it needs
//! signing in, how, and how that stands. A feed whose login is a broker
//! connection signs in through a login in Preferences → Trading, the data
//! login, so while there is one its row says so and leaves signing in, out
//! and removing to that page; adding it with no login is adding a broker
//! there. With trading switched off ([`crate::features`]) there is no
//! Trading page, and the broker's feed is a feed like the others: its
//! login is typed in the broker's own dialog from its row, kept as the data
//! login, and signs in its data alone, never an order plant. A feed with a dialog of its own is signed in to here, in that
//! dialog, found in `sign_in_dialogs`; one that signs in in a browser opens
//! one.
//!
//! Signing in runs on a thread of its own and reports back over a channel
//! the main loop awaits, because it waits on somebody: a browser and a
//! code off a phone, or a gateway.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::provider::Delivery;
use omacharts_engine::providers::{self, Access, How, Listed, Verbs};

use crate::feeds;
use crate::store::Store;
use crate::ui::drop_line;
use crate::ui::sign_in_dialogs;

/// What the sign-in thread sends back.
enum Progress {
    /// One line of how it is going, as it happens.
    Line(String),
    /// Finished. `Err` carries what went wrong, in the feed's words.
    Done(Result<(), String>),
}

/// Opens the login dialog of the broker with this id, from Trading, over
/// the widget given.
pub type AddBroker = Rc<dyn Fn(&str, &gtk::Widget)>;

struct Panel {
    store: Rc<Store>,
    /// Adding a broker connection's feed with no login is adding a broker
    /// in Trading.
    add_broker: Option<AddBroker>,
    /// Has the window chart from the order now stored, and read again
    /// which feeds are signed in. Called after every change here.
    on_change: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    /// The feeds, rebuilt whenever one changes.
    group: RefCell<Option<adw::PreferencesGroup>>,
    /// The artwork and notices the feeds set up have to show, under the
    /// feeds.
    credits: RefCell<Vec<adw::PreferencesGroup>>,
    /// What the page shows below the feeds, kept last.
    after: Vec<adw::PreferencesGroup>,
    /// The feed a sign-in is running for, so a second press cannot start
    /// a second one.
    signing_in: Cell<Option<&'static str>>,
    /// The latest word from a sign-in, by feed, until the next one:
    /// progress while it runs, and why it failed when that is not a state
    /// the feed keeps itself.
    notes: RefCell<HashMap<&'static str, String>>,
}

/// Fills the settings' data section: the feeds, and the groups in
/// `after` below them. Returns what draws the feeds again, for Trading to
/// call when a login changes.
pub fn build(
    page: &adw::PreferencesPage,
    store: Rc<Store>,
    on_change: Rc<dyn Fn()>,
    after: Vec<adw::PreferencesGroup>,
    add_broker: AddBroker,
) -> Rc<dyn Fn()> {
    feeds::remember_logins(&store);
    let panel = Panel::new(page.clone(), store, Some(add_broker), on_change, after);
    rebuild(&panel);
    Rc::new(move || rebuild(&panel))
}

impl Panel {
    fn new(
        page: adw::PreferencesPage,
        store: Rc<Store>,
        add_broker: Option<AddBroker>,
        on_change: Rc<dyn Fn()>,
        after: Vec<adw::PreferencesGroup>,
    ) -> Rc<Panel> {
        Rc::new(Panel {
            store,
            add_broker,
            on_change,
            page,
            group: RefCell::new(None),
            credits: RefCell::new(Vec::new()),
            after,
            signing_in: Cell::new(None),
            notes: RefCell::new(HashMap::new()),
        })
    }

    /// The feeds added, top first, as the catalogue has them.
    fn added(&self) -> Vec<&'static Listed> {
        feeds::stored_order(&self.store)
            .iter()
            .filter_map(|id| providers::listed(id))
            .fold(Vec::new(), |mut added, feed| {
                if !added.iter().any(|known: &&Listed| known.id == feed.id) {
                    added.push(feed);
                }
                added
            })
    }

    /// Write `order` down, have the window chart from it, and redraw.
    fn reorder(self: &Rc<Self>, order: Vec<&'static Listed>) {
        let ids: Vec<String> = order.iter().map(|feed| feed.id.to_string()).collect();
        feeds::save_order(&self.store, &ids);
        self.changed();
    }

    fn changed(self: &Rc<Self>) {
        (self.on_change)();
        rebuild(self);
    }
}

/// Replace the feeds group with one drawn from what is stored now.
fn rebuild(panel: &Rc<Panel>) {
    if let Some(old) = panel.group.borrow_mut().take() {
        panel.page.remove(&old);
    }
    for old in panel.credits.borrow_mut().drain(..) {
        panel.page.remove(&old);
    }
    let group = feeds_group(panel);
    panel.page.add(&group);
    *panel.group.borrow_mut() = Some(group);
    for credits in set_up(panel).into_iter().filter_map(sign_in_dialogs::credits) {
        panel.page.add(&credits);
        panel.credits.borrow_mut().push(credits);
    }
    for after in &panel.after {
        if after.parent().is_some() {
            panel.page.remove(after);
        }
        panel.page.add(after);
    }
}

/// The feeds set up: those added, and those with a login in Trading. A
/// feed that has artwork and notices to show has them on the page while
/// it is set up.
fn set_up(panel: &Panel) -> Vec<&'static Listed> {
    let logins = crate::trading::logins(&panel.store);
    let mut feeds = panel.added();
    for feed in providers::offered() {
        let logged_in = logins.iter().any(|login| login.broker.eq_ignore_ascii_case(feed.id));
        if logged_in && !feeds.iter().any(|known| known.id == feed.id) {
            feeds.push(feed);
        }
    }
    feeds
}

fn feeds_group(panel: &Rc<Panel>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Data feeds");
    group.set_description(Some(
        "Symbols use the first data feed that offers them.",
    ));

    let added = panel.added();
    for (at, feed) in added.iter().enumerate() {
        group.add(&feed_row(panel, feed, at, added.len() > 1));
    }

    let missing: Vec<&'static Listed> =
        providers::offered().filter(|feed| !added.iter().any(|known| known.id == feed.id)).collect();
    if !missing.is_empty() {
        group.add(&add_button(panel, missing));
    }
    group
}

/// How a feed stands, in a word or two: how fresh it is while it can be
/// used, otherwise why it cannot.
fn standing(panel: &Panel, feed: &'static Listed) -> String {
    if let Some(note) = panel.notes.borrow().get(feed.id) {
        return note.clone();
    }
    match providers::access(feed.id) {
        None | Some(Access::Signed(_)) => fresh(feed.id).into(),
        Some(Access::Missing) => verbs(feed).missing.into(),
        Some(Access::Expired(_)) => {
            feed.setup.and_then(|setup| setup.refusal()).unwrap_or_else(|| "session expired".into())
        }
    }
}

/// "live", "delayed", or "refetched", from how the provider delivers.
fn fresh(id: &str) -> &'static str {
    let provider = providers::built_or_default(id);
    if provider.delivery() == Delivery::Streamed {
        return "live";
    }
    match providers::freshness(id).starts_with("delayed") {
        true => "delayed",
        false => "refetched",
    }
}

fn subtitle(panel: &Panel, feed: &'static Listed) -> String {
    match through_trading(panel, feed) {
        Some(login) => format!("{} · {} · through {}", feed.covers, standing(panel, feed), login.name),
        None => format!("{} · {}", feed.covers, standing(panel, feed)),
    }
}

/// The login a feed signs in through, when that is Trading's business: a
/// broker connection's feed, while there is a login in Trading.
fn through_trading(panel: &Panel, feed: &'static Listed) -> Option<crate::trading::Login> {
    if !crate::features::trading() {
        return None;
    }
    crate::trading::data_login(&panel.store).filter(|login| trades(feed) && login.broker.eq_ignore_ascii_case(feed.id))
}

/// Is `feed`'s login a broker connection, kept in Trading?
fn trades(feed: &'static Listed) -> bool {
    feed.setup.is_some_and(|setup| setup.trades())
}

/// A feed's row. `others` is whether any other feed is on the list:
/// the last one left offers no Remove, and has nothing to be dragged past.
fn feed_row(panel: &Rc<Panel>, feed: &'static Listed, at: usize, others: bool) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(feed.label);
    row.set_use_markup(false);
    row.set_subtitle(&subtitle(panel, feed));
    row.set_subtitle_lines(0);

    if others {
        let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        handle.add_css_class("dim-label");
        row.add_prefix(&handle);
    }

    // A word, not a warning: the feed works, and rests on a web session
    // somebody else's release can take away.
    if feed.experimental {
        let tag = gtk::Label::new(Some("Experimental"));
        tag.add_css_class("caption");
        tag.add_css_class("dim-label");
        tag.set_valign(gtk::Align::Center);
        row.add_suffix(&tag);
    }

    // Signed in, out and removed in Trading, with the login it is.
    if through_trading(panel, feed).is_some() {
        wire_reorder(panel, &row, feed, at);
        return row;
    }

    if feed.needs_sign_in() {
        if panel.signing_in.get() == Some(feed.id) {
            let spinner = gtk::Spinner::new();
            spinner.set_valign(gtk::Align::Center);
            spinner.start();
            row.add_suffix(&spinner);
        } else {
            if let Some(button) = action_button(panel, feed, &row) {
                row.add_suffix(&button);
            }
        }
    }

    if others {
        let remove = gtk::Button::with_label("Remove");
        remove.add_css_class("flat");
        remove.set_valign(gtk::Align::Center);
        let panel_for_remove = panel.clone();
        remove.connect_clicked(move |_| remove_feed(&panel_for_remove, feed));
        row.add_suffix(&remove);
    }

    wire_reorder(panel, &row, feed, at);
    row
}

/// The one thing to do about a feed's sign-in, which says where it
/// stands: Sign in… with nothing usable saved, after a refusal or after an
/// expiry, and Sign out while it works.
fn action_button(panel: &Rc<Panel>, feed: &'static Listed, row: &adw::ActionRow) -> Option<gtk::Button> {
    let access = providers::access(feed.id).unwrap_or(Access::Missing);
    let verbs = verbs(feed);
    let label = match &access {
        Access::Signed(_) => verbs.change.or(verbs.end)?,
        Access::Expired(_) | Access::Missing => verbs.start,
    };
    let button = gtk::Button::with_label(label);
    button.set_valign(gtk::Align::Center);
    let panel = panel.clone();
    if access.ready() {
        let anchor = row.clone();
        match verbs.change {
            Some(_) => button.connect_clicked(move |_| ask_dialog(&panel, feed, &anchor)),
            None => button.connect_clicked(move |_| sign_out(&panel, feed)),
        };
        return Some(button);
    }
    button.add_css_class("suggested-action");
    // A sign-in that cannot start says why on the row, rather than being a
    // button that fails. A dialog always can.
    if !feed.asks_in_a_dialog()
        && let Err(why) = providers::can_sign_in(feed.id)
    {
        button.set_sensitive(false);
        row.set_subtitle(&format!("{} · {why}", feed.covers));
        return Some(button);
    }
    let anchor = row.clone();
    button.connect_clicked(move |_| match (feed.asks_in_a_dialog(), trades(feed)) {
        (true, true) if !crate::features::trading() => ask_login(&panel, feed, &anchor),
        (true, true) => ask_broker(&panel, feed, &anchor),
        (true, false) => ask_dialog(&panel, feed, &anchor),
        (false, _) => start_sign_in(&panel, feed),
    });
    Some(button)
}

/// What `feed`'s row offers and says, in the feed's words.
fn verbs(feed: &'static Listed) -> Verbs {
    feed.setup.map_or(Verbs::SIGN_IN, |setup| setup.verbs())
}

/// Dragged by id, because the rows are rebuilt under the drag. A drop is
/// taken out of the order and put back at the place of the row it lands
/// on, as the header's part list does.
fn wire_reorder(panel: &Rc<Panel>, row: &adw::ActionRow, feed: &'static Listed, onto: usize) {
    let source = gtk::DragSource::new();
    source.set_actions(gtk::gdk::DragAction::MOVE);
    let payload = feed.id.to_string();
    source.connect_prepare(move |_, _, _| Some(gtk::gdk::ContentProvider::for_value(&payload.to_value())));
    source.connect_drag_begin(|source, _| {
        let Some(row) = source.widget() else { return };
        let ghost = gtk::WidgetPaintable::new(Some(&row));
        source.set_icon(Some(&ghost), row.width() / 2, row.height() / 2);
    });
    row.add_controller(source);

    let from = {
        let panel = panel.clone();
        move |value: &glib::Value| {
            let id = value.get::<String>().ok()?;
            panel.added().iter().position(|feed| feed.id == id)
        }
    };
    let landing = from.clone();
    let panel = panel.clone();
    let target = drop_line::target(
        row,
        move |value| landing(value).and_then(|from| drop_line::edge(from, onto)),
        move |value| {
            let Some(from) = from(value) else { return false };
            match moved(&panel.added(), from, onto) {
                Some(order) => {
                    panel.reorder(order);
                    true
                }
                None => false,
            }
        },
    );
    row.add_controller(target);
}

/// `order` with the entry at `from` taken out and put back at `onto`, or
/// `None` for a move that changes nothing or names no feed.
fn moved<T: Copy>(order: &[T], from: usize, onto: usize) -> Option<Vec<T>> {
    if from == onto || from >= order.len() || onto >= order.len() {
        return None;
    }
    let mut order = order.to_vec();
    let lifted = order.remove(from);
    order.insert(onto, lifted);
    Some(order)
}

/// The feeds not added yet, one press each.
fn add_button(panel: &Rc<Panel>, missing: Vec<&'static Listed>) -> gtk::MenuButton {
    let add = gtk::MenuButton::new();
    add.set_label("Add a data feed…");
    add.set_halign(gtk::Align::Start);
    add.set_margin_top(12);
    let items = missing
        .into_iter()
        .map(|feed| {
            let panel = panel.clone();
            let anchor = add.downgrade();
            crate::ui::menus::Item::new(feed.label, move || {
                if let Some(anchor) = anchor.upgrade() {
                    add_feed(&panel, feed, &anchor);
                }
            })
        })
        .collect();
    crate::ui::menus::actions(&add, "feed", items);
    add
}

/// A broker connection's feed is added by adding a broker login, in
/// Trading, unless there is one already to sign in through. Any other is
/// added at once, and signed in to from its row.
fn add_feed(panel: &Rc<Panel>, feed: &'static Listed, anchor: &impl IsA<gtk::Widget>) {
    match trades(feed) && through_trading(panel, feed).is_none() {
        true if !crate::features::trading() => ask_login(panel, feed, anchor),
        true => ask_broker(panel, feed, anchor),
        false => {
            feeds::add_feed(&panel.store, feed.id);
            panel.changed();
            // A feed set up in a dialog of its own is set up as it is added.
            if feed.asks_in_a_dialog() && !trades(feed) && !providers::usable(feed.id) {
                ask_dialog(panel, feed, anchor);
            }
        }
    }
}

fn remove_feed(panel: &Rc<Panel>, feed: &'static Listed) {
    panel.notes.borrow_mut().remove(feed.id);
    feeds::remove_feed(&panel.store, feed.id);
    panel.changed();
}

fn sign_out(panel: &Rc<Panel>, feed: &'static Listed) {
    panel.notes.borrow_mut().remove(feed.id);
    if let Err(error) = providers::sign_out(feed.id) {
        eprintln!("omacharts: {error}");
    }
    feeds::forget_login(&panel.store, feed.id);
    panel.changed();
}

/// A broker connection's feed with no login: a broker is added in Trading,
/// which puts the feed on top of the feeds and signs it in.
fn ask_broker(panel: &Rc<Panel>, feed: &'static Listed, anchor: &impl IsA<gtk::Widget>) {
    if let Some(add_broker) = &panel.add_broker {
        add_broker(feed.id, anchor.upcast_ref());
    }
}

/// A broker connection's feed with trading switched off: its login typed
/// in the broker's own dialog — the data login again, prefilled, when
/// there is one — kept, and its data signed in from this row, as any other
/// feed's. Nothing signs in to the broker's order plant.
fn ask_login(panel: &Rc<Panel>, feed: &'static Listed, anchor: &impl IsA<gtk::Widget>) {
    let Some(dialog) = sign_in_dialogs::login_dialog(feed.id) else { return };
    let known = crate::trading::data_login(&panel.store).filter(|login| login.broker.eq_ignore_ascii_case(feed.id));
    let panel = panel.clone();
    let id = known.as_ref().map(|login| login.id.clone());
    dialog(
        anchor.upcast_ref(),
        known.as_ref(),
        Box::new(move |typed| {
            let secrets = crate::ui::trading_settings::keyring(&typed.broker);
            let kept = match &id {
                Some(id) => crate::trading::retype_login(&panel.store, &secrets, id, &typed),
                None => crate::trading::add_login(&panel.store, &secrets, &typed).map(|_| ()),
            };
            if let Err(error) = kept {
                panel.notes.borrow_mut().insert(feed.id, format!("not saved · {error}"));
                panel.changed();
                return;
            }
            feeds::add_feed(&panel.store, feed.id);
            feeds::remember_logins(&panel.store);
            panel.changed();
            start_sign_in(&panel, feed);
        }),
    );
}

/// A feed with a dialog of its own: the dialog keeps what is typed, and the
/// feed is signed in with it.
///
/// It is handed what the feed was last set up with, and hands back what it
/// is set up with now, which is kept before the sign-in starts.
fn ask_dialog(panel: &Rc<Panel>, feed: &'static Listed, anchor: &impl IsA<gtk::Widget>) {
    let Some(dialog) = sign_in_dialogs::feed_dialog(feed) else { return };
    let saved = feeds::setup_of(&panel.store, feed.id);
    let panel = panel.clone();
    let kept = move |saved: providers::Saved| {
        feeds::set_up(&panel.store, feed, saved);
        start_sign_in(&panel, feed);
    };
    dialog(saved, Box::new(kept)).present(Some(anchor));
}

/// Run a sign-in off the main loop, showing on the row how it goes, then
/// have the window read the feeds again.
fn start_sign_in(panel: &Rc<Panel>, feed: &'static Listed) {
    if panel.signing_in.get().is_some() {
        return;
    }
    panel.signing_in.set(Some(feed.id));
    let opening = match feed.setup.map(|setup| setup.how()) {
        Some(How::Browser) => "opening a browser…",
        _ => "signing in",
    };
    panel.notes.borrow_mut().insert(feed.id, opening.into());
    rebuild(panel);

    let (sender, receiver) = async_channel::unbounded::<Progress>();
    let lines = sender.clone();
    std::thread::spawn(move || {
        let outcome = feeds::sign_in_now(feed.id, move |line| {
            let _ = lines.send_blocking(Progress::Line(line.to_string()));
        });
        let _ = sender.send_blocking(Progress::Done(outcome));
    });

    let panel = panel.clone();
    glib::spawn_future_local(async move {
        while let Ok(progress) = receiver.recv().await {
            match progress {
                Progress::Line(line) => {
                    panel.notes.borrow_mut().insert(feed.id, line);
                    rebuild(&panel);
                }
                Progress::Done(outcome) => {
                    panel.signing_in.set(None);
                    let mut notes = panel.notes.borrow_mut();
                    notes.remove(feed.id);
                    // A refusal is a state the feed keeps, and the row
                    // reads it back in the provider's words; anything else
                    // is said once, here.
                    if let Err(error) = outcome
                        && !matches!(providers::access(feed.id), Some(Access::Expired(_)))
                    {
                        notes.insert(feed.id, format!("not signed in · {error}"));
                    }
                    drop(notes);
                    panel.changed();
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const RITHMIC: &str = "rithmic";

    use omacharts_engine::providers::rithmic::session;

    fn panel(store: &Rc<Store>) -> Rc<Panel> {
        Panel::new(adw::PreferencesPage::new(), store.clone(), None, Rc::new(|| {}), Vec::new())
    }

    /// Every [`adw::ActionRow`] under `widget`, in the order somebody reads
    /// them.
    fn action_rows(widget: &gtk::Widget) -> Vec<adw::ActionRow> {
        let mut found = Vec::new();
        let mut next = widget.first_child();
        while let Some(child) = next {
            if let Ok(row) = child.clone().downcast::<adw::ActionRow>() {
                found.push(row);
            }
            found.extend(action_rows(&child));
            next = child.next_sibling();
        }
        found
    }

    /// The words of every label and button under `widget`.
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

    fn titles(panel: &Panel) -> Vec<String> {
        let group = panel.group.borrow().clone().expect("the feeds are drawn");
        action_rows(group.upcast_ref()).iter().map(|row| row.title().to_string()).collect()
    }

    /// With nothing configured, one row, Yahoo, and the button to add
    /// another. No brokerage, no "Experimental", no sign-in.
    #[test]
    fn nothing_configured_is_one_yahoo_row() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("an empty database"));
            let panel = panel(&store);
            rebuild(&panel);
            assert_eq!(titles(&panel), vec!["Yahoo Finance"]);
            let group = panel.group.borrow().clone().unwrap();
            let words = words_under(group.upcast_ref());
            assert!(words.iter().any(|word| word == "Add a data feed…"), "{words:?}");
            // The feeds still to add are in the button's menu, not on the page.
            let yahoo = &action_rows(group.upcast_ref())[0];
            let words = words_under(yahoo.upcast_ref());
            for absent in ["Experimental", "Sign in…", "Sign out", "Remove"] {
                assert!(!words.iter().any(|word| word == absent), "{absent} is on the page: {words:?}");
            }
            assert!(yahoo.subtitle().unwrap_or_default().starts_with("Stocks, ETFs"), "{:?}", yahoo.subtitle());
        });
    }

    /// An added feed gets a row above Yahoo, with Remove, and
    /// thinkorswim's says Experimental. The order written is the order
    /// drawn, and moving one writes the new order.
    #[test]
    fn the_order_is_stored_and_drawn() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("an empty database"));
            let panel = panel(&store);
            let tos = providers::listed("tos").unwrap();
            let rithmic = providers::listed(RITHMIC).unwrap();
            feeds::add_feed(&store, RITHMIC);
            feeds::add_feed(&store, "tos");
            rebuild(&panel);
            assert_eq!(titles(&panel), vec!["Rithmic", "thinkorswim", "Yahoo Finance"]);

            let group = panel.group.borrow().clone().unwrap();
            let rows = action_rows(group.upcast_ref());
            let tos_words = words_under(rows[1].upcast_ref());
            assert!(tos_words.iter().any(|w| w == "Experimental"), "{tos_words:?}");
            assert!(tos_words.iter().any(|w| w == "Remove"), "{tos_words:?}");
            let rithmic_words = words_under(rows[0].upcast_ref());
            assert!(!rithmic_words.iter().any(|w| w == "Experimental"), "{rithmic_words:?}");

            let order = moved(&panel.added(), 1, 0).expect("a move");
            panel.reorder(order);
            assert_eq!(store.setting(feeds::SETTING).as_deref(), Some(r#"["tos","rithmic","yahoo"]"#));
            assert_eq!(titles(&panel), vec!["thinkorswim", "Rithmic", "Yahoo Finance"]);
            assert_eq!(panel.added().iter().map(|f| f.id).collect::<Vec<_>>(), vec![tos.id, rithmic.id, "yahoo"]);
        });
    }

    /// Yahoo moves like any other feed, and can be removed and added back
    /// through the button; the last feed left offers no Remove.
    #[test]
    fn yahoo_moves_and_goes_like_any_other_but_the_last_stays() {
        let order = ["rithmic", "tos", "yahoo"];
        assert_eq!(moved(&order, 1, 1), None, "dropped on itself");
        assert_eq!(moved(&order, 3, 0), None, "no such feed");
        assert_eq!(moved(&order, 2, 0), Some(vec!["yahoo", "rithmic", "tos"]));
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("an empty database"));
            let panel = panel(&store);
            feeds::add_feed(&store, "tos");
            rebuild(&panel);
            assert_eq!(titles(&panel), vec!["thinkorswim", "Yahoo Finance"]);
            let yahoo = providers::listed("yahoo").unwrap();
            remove_feed(&panel, yahoo);
            assert_eq!(titles(&panel), vec!["thinkorswim"]);
            let group = panel.group.borrow().clone().unwrap();
            let words = words_under(action_rows(group.upcast_ref())[0].upcast_ref());
            assert!(!words.iter().any(|w| w == "Remove"), "the last feed: {words:?}");
            add_feed(&panel, yahoo, &gtk::Button::new());
            assert_eq!(titles(&panel), vec!["thinkorswim", "Yahoo Finance"]);
            let order = moved(&panel.added(), 1, 0).expect("a move");
            panel.reorder(order);
            assert_eq!(store.setting(feeds::SETTING).as_deref(), Some(r#"["yahoo","tos"]"#));
            assert_eq!(titles(&panel), vec!["Yahoo Finance", "thinkorswim"]);
        });
    }

    /// Remove takes the feed off the order; the last one removed leaves
    /// no setting. While Rithmic signs in through a login in Trading its row
    /// says so and offers neither Remove nor a sign-in of its own.
    #[test]
    fn remove_clears_the_setting_and_trading_owns_rithmics_login() {
        if session::development() {
            return;
        }
        crate::ui::on_gtk(|| {
            crate::features::force(crate::features::Feature::Trading, true);
            let store = Rc::new(Store::memory().expect("an empty database"));
            let panel = panel(&store);
            feeds::add_feed(&store, RITHMIC);
            rebuild(&panel);
            let group = panel.group.borrow().clone().unwrap();
            let words = words_under(action_rows(group.upcast_ref())[0].upcast_ref());
            assert!(words.iter().any(|w| w == "Remove"), "{words:?}");
            remove_feed(&panel, providers::listed(RITHMIC).unwrap());
            assert_eq!(store.setting(feeds::SETTING), None);
            assert_eq!(titles(&panel), vec!["Yahoo Finance"]);

            let kept = crate::trading::fake::Kept::default();
            let typed = crate::trading::Typed {
                broker: "rithmic".into(),
                name: "Lucid".into(),
                user: "trader".into(),
                password: "PLANTED-PASSWORD".into(),
                values: vec![("system".into(), "Rithmic Test".into())],
            };
            crate::trading::add_login(&store, &kept, &typed).unwrap();
            rebuild(&panel);
            assert_eq!(titles(&panel), vec!["Rithmic", "Yahoo Finance"]);
            let group = panel.group.borrow().clone().unwrap();
            let row = &action_rows(group.upcast_ref())[0];
            assert!(row.subtitle().unwrap_or_default().ends_with("· through Lucid"), "{:?}", row.subtitle());
            let words = words_under(row.upcast_ref());
            for absent in ["Remove", "Sign in…", "Sign out"] {
                assert!(!words.iter().any(|w| w == absent), "{absent}: {words:?}");
            }
        });
    }

    /// With trading switched off there is no Trading page to leave Rithmic's
    /// row to: a login kept for its data is signed in, out and removed from
    /// the row, like any other feed's, and nothing says "through" it.
    #[test]
    fn with_trading_off_rithmic_is_a_feed_like_the_others() {
        if session::development() {
            return;
        }
        crate::ui::on_gtk(|| {
            assert!(!crate::features::trading(), "off unless switched on");
            let store = Rc::new(Store::memory().expect("an empty database"));
            let panel = panel(&store);
            let kept = crate::trading::fake::Kept::default();
            let typed = crate::trading::Typed {
                broker: "rithmic".into(),
                name: "Lucid".into(),
                user: "trader".into(),
                password: "PLANTED-PASSWORD".into(),
                values: vec![("system".into(), "Rithmic Test".into())],
            };
            crate::trading::add_login(&store, &kept, &typed).unwrap();
            rebuild(&panel);
            assert_eq!(titles(&panel), vec!["Rithmic", "Yahoo Finance"]);
            let group = panel.group.borrow().clone().unwrap();
            let row = &action_rows(group.upcast_ref())[0];
            assert!(!row.subtitle().unwrap_or_default().contains("through"), "{:?}", row.subtitle());
            let words = words_under(row.upcast_ref());
            assert!(words.iter().any(|w| w == "Remove"), "{words:?}");
            assert!(words.iter().any(|w| w == "Sign in…" || w == "Sign out"), "{words:?}");
        });
    }

    /// A feed's row offers one action, and which one follows how it
    /// stands. Rithmic with nobody saved is not signed in.
    #[test]
    fn a_feed_offers_the_action_its_state_calls_for() {
        if session::development() {
            return;
        }
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("an empty database"));
            feeds::remember_logins(&store);
            let panel = panel(&store);
            feeds::add_feed(&store, RITHMIC);
            rebuild(&panel);
            let group = panel.group.borrow().clone().unwrap();
            let row = &action_rows(group.upcast_ref())[0];
            assert_eq!(row.subtitle().as_deref(), Some("CME futures, front month · not signed in"));
            let words = words_under(row.upcast_ref());
            assert!(words.iter().any(|w| w == "Sign in…"), "{words:?}");
            assert!(!words.iter().any(|w| w == "Sign out"), "{words:?}");
        });
    }

    /// A feed no code here names gets its row and its sign-in from what it
    /// declares, its dialog from the registry, and charts routed to it: the
    /// fake rich feed, which is in no list a person is offered.
    #[test]
    fn a_feed_nobody_names_gets_its_row_its_dialog_and_its_charts() {
        crate::ui::on_gtk(|| {
            let fake = providers::listed(omacharts_engine::providers::fake::LISTED.id).expect("in a test build");
            let store = Rc::new(Store::memory().expect("an empty database"));
            let panel = panel(&store);
            add_feed(&panel, fake, &gtk::Button::new());
            assert_eq!(titles(&panel), vec![fake.label, "Yahoo Finance"]);
            let group = panel.group.borrow().clone().unwrap();
            let row = action_rows(group.upcast_ref())[0].clone();
            assert_eq!(row.subtitle().unwrap_or_default(), format!("{} · not signed in", fake.covers));
            let words = words_under(row.upcast_ref());
            assert!(words.iter().any(|w| w == "Sign in…"), "{words:?}");
            assert!(words.iter().any(|w| w == "Remove"), "{words:?}");

            // Its dialog is its own, found by what it declares; what it keeps
            // signs it in, and the row says it is live.
            assert!(feed_settings_opens_a_dialog(fake));
            let done = Rc::new(RefCell::new(None));
            let signed = done.clone();
            let known = providers::Saved::default().with("desk", "North");
            let dialog = sign_in_dialogs::feed_dialog(fake).expect("a dialog")(
                known,
                Box::new(move |saved| *signed.borrow_mut() = Some(saved)),
            );
            let list = dialog.extra_child().and_downcast::<gtk::ListBox>().expect("its rows");
            list.first_child().and_downcast::<adw::EntryRow>().expect("a member number").set_text("member");
            dialog.emit_by_name::<()>("response", &[&sign_in_dialogs::fake::SIGN_IN]);
            let saved = done.borrow_mut().take().expect("the page is handed what it was set up with");
            assert_eq!((saved.user.as_deref(), saved.value("desk")), (Some("member"), Some("North")));
            // Kept by the app, and handed back to the feed from there.
            feeds::set_up(&store, fake, saved);
            assert_eq!(feeds::setup_of(&store, fake.id).user.as_deref(), Some("member"));
            feeds::remember_logins(&store);
            let log = omacharts_engine::log::global();
            let before = log.revision();
            assert!(feeds::sign_in_now(fake.id, |_| {}).is_ok());
            let said = |before| {
                log.since(before).0.into_iter().filter(|e| e.source == fake.label).map(|e| e.text).collect::<Vec<_>>()
            };
            assert_eq!(said(before).last().map(String::as_str), Some("Signed in"), "the Log has the sign-in");
            rebuild(&panel);
            let group = panel.group.borrow().clone().unwrap();
            let row = action_rows(group.upcast_ref())[0].clone();
            assert_eq!(row.subtitle().unwrap_or_default(), format!("{} · live", fake.covers));
            assert!(words_under(row.upcast_ref()).iter().any(|w| w == "Sign out"));

            // And charts go to it for what it serves, and to Yahoo for the rest.
            let routed = feeds::Feeds::load(&store);
            let instrument = |symbol: &str, kind| omacharts_engine::Instrument {
                symbol: symbol.into(),
                name: String::new(),
                kind,
                suffix: None,
                currency: None,
                tier: 0,
                session_origin: 0,
                overrides: Vec::new(),
                exchange: None,
                popularity: 0,
                local_name: None,
            };
            let future = instrument("ES", omacharts_engine::InstrumentKind::FutureRoot);
            let stock = instrument("AAPL", omacharts_engine::InstrumentKind::Equity);
            assert_eq!(routed.route(&future).map(|(provider, _)| provider.id()), Some(fake.id));
            assert_eq!(routed.route(&stock).map(|(provider, _)| provider.id()), Some(providers::DEFAULT));

            // Signed out from its row, it cannot sign in until the dialog again.
            sign_out(&panel, fake);
            let before = log.revision();
            assert!(feeds::sign_in_now(fake.id, |_| {}).is_err());
            let refused = said(before);
            assert!(refused.last().is_some_and(|text| text.starts_with("Not signed in · ")), "{refused:?}");
        });
    }

    /// A feed is signed in to through a dialog of the app's only when the
    /// registry has one for it.
    fn feed_settings_opens_a_dialog(feed: &'static Listed) -> bool {
        feed.asks_in_a_dialog() && !trades(feed) && sign_in_dialogs::feed_dialog(feed).is_some()
    }

    /// Nothing on this page may reach the network, because it is drawn the
    /// moment somebody opens Preferences.
    #[test]
    fn reading_the_session_state_connects_to_nothing() {
        for feed in providers::LISTED {
            let _ = providers::access(feed.id);
            assert!(!providers::connected(feed.id), "{} connected", feed.id);
        }
    }

    /// Every feed has a few words for its row.
    #[test]
    fn every_feed_says_what_it_covers() {
        for feed in providers::LISTED {
            assert!(!feed.covers.is_empty(), "{} has nothing to say", feed.id);
        }
    }
}
