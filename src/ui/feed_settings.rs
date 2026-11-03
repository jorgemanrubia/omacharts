//! The sources charts come from, in the order they are tried.
//!
//! A list rather than a choice: each chart comes from the first source that
//! has its symbol and is signed in, so the order is the setting. Yahoo is a
//! source like the others, moved and removed the same way; only the last
//! source left cannot be removed, since with none no chart could draw. A
//! chart of something no remaining source has says so.
//!
//! What each row says comes from the feeds themselves: what a source
//! covers, how fresh it is (asked of the provider), whether it needs
//! signing in, how, and how that stands. A feed whose login is a broker
//! connection signs in through a login in Preferences → Trading, the data
//! login, so while there is one its row says so and leaves signing in, out
//! and removing to that page; adding it with no login is adding a broker
//! there. A feed with a form of its own is typed into here, in the form it
//! declares; one that signs in in a browser opens one.
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
use omacharts_engine::providers::{self, Access, How, Listed};

use crate::feeds;
use crate::store::Store;
use crate::ui::drop_line;
use crate::ui::login_form::{self, Form};

/// What the sign-in thread sends back.
enum Progress {
    /// One line of how it is going, as it happens.
    Line(String),
    /// Finished. `Err` carries what went wrong, in the source's words.
    Done(Result<(), String>),
}

/// Opens the broker login dialog, from Trading, over the widget given.
pub type AddBroker = Rc<dyn Fn(&gtk::Widget)>;

struct Panel {
    store: Rc<Store>,
    /// Adding a broker connection's feed with no login is adding a broker
    /// in Trading.
    add_broker: Option<AddBroker>,
    /// Has the window chart from the order now stored, and read again
    /// which sources are signed in. Called after every change here.
    on_change: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    /// The sources, rebuilt whenever one changes.
    group: RefCell<Option<adw::PreferencesGroup>>,
    /// What the page shows below the sources, kept last.
    after: Vec<adw::PreferencesGroup>,
    /// The source a sign-in is running for, so a second press cannot start
    /// a second one.
    signing_in: Cell<Option<&'static str>>,
    /// The latest word from a sign-in, by source, until the next one:
    /// progress while it runs, and why it failed when that is not a state
    /// the source keeps itself.
    notes: RefCell<HashMap<&'static str, String>>,
}

/// Fills the settings' data section: the sources, and the groups in
/// `after` below them. Returns what draws the sources again, for Trading to
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
            after,
            signing_in: Cell::new(None),
            notes: RefCell::new(HashMap::new()),
        })
    }

    /// The sources added, top first, as the catalogue has them.
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

/// Replace the sources group with one drawn from what is stored now.
fn rebuild(panel: &Rc<Panel>) {
    if let Some(old) = panel.group.borrow_mut().take() {
        panel.page.remove(&old);
    }
    let group = sources_group(panel);
    panel.page.add(&group);
    *panel.group.borrow_mut() = Some(group);
    for after in &panel.after {
        if after.parent().is_some() {
            panel.page.remove(after);
        }
        panel.page.add(after);
    }
}

fn sources_group(panel: &Rc<Panel>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Data feeds");
    group.set_description(Some(
        "Each chart comes from the first data feed in this list that has its symbol.",
    ));

    let added = panel.added();
    for (at, feed) in added.iter().enumerate() {
        group.add(&source_row(panel, feed, at, added.len() > 1));
    }

    let missing: Vec<&'static Listed> =
        providers::LISTED.iter().filter(|feed| !added.iter().any(|known| known.id == feed.id)).collect();
    if !missing.is_empty() {
        group.add(&add_button(panel, missing));
    }
    group
}

/// How a source stands, in a word or two: how fresh it is while it can be
/// used, otherwise why it cannot.
fn standing(panel: &Panel, feed: &'static Listed) -> String {
    if let Some(note) = panel.notes.borrow().get(feed.id) {
        return note.clone();
    }
    match providers::access(feed.id) {
        None | Some(Access::Signed(_)) => fresh(feed.id).into(),
        Some(Access::Missing) => "not signed in".into(),
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
        Some(login) => format!("{} · {} · signs in through {} in Trading", feed.covers, standing(panel, feed), login.name),
        None => format!("{} · {}", feed.covers, standing(panel, feed)),
    }
}

/// The login a source signs in through, when that is Trading's business: a
/// broker connection's feed, while there is a login in Trading.
fn through_trading(panel: &Panel, feed: &'static Listed) -> Option<crate::trading::Login> {
    trades(feed).then(|| crate::trading::data_login(&panel.store)).flatten()
}

/// Is `feed`'s login a broker connection, kept in Trading?
fn trades(feed: &'static Listed) -> bool {
    feed.setup.is_some_and(|setup| setup.trades())
}

/// A source's row. `others` is whether any other source is on the list:
/// the last one left offers no Remove, and has nothing to be dragged past.
fn source_row(panel: &Rc<Panel>, feed: &'static Listed, at: usize, others: bool) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(feed.label);
    row.set_use_markup(false);
    row.set_subtitle(&subtitle(panel, feed));
    row.set_subtitle_lines(0);

    if others {
        let handle = gtk::Image::from_icon_name("list-drag-handle-symbolic");
        handle.add_css_class("dim-label");
        handle.set_tooltip_text(Some("Drag to reorder"));
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
            row.add_suffix(&action_button(panel, feed, &row));
        }
    }

    if others {
        let remove = gtk::Button::with_label("Remove");
        remove.add_css_class("flat");
        remove.set_valign(gtk::Align::Center);
        let panel_for_remove = panel.clone();
        remove.connect_clicked(move |_| remove_source(&panel_for_remove, feed));
        row.add_suffix(&remove);
    }

    wire_reorder(panel, &row, feed, at);
    row
}

/// The one thing to do about a source's sign-in, which says where it
/// stands: Sign in… with nothing usable saved, Sign in again after a
/// refusal or an expiry, Sign out while it works.
fn action_button(panel: &Rc<Panel>, feed: &'static Listed, row: &adw::ActionRow) -> gtk::Button {
    let access = providers::access(feed.id).unwrap_or(Access::Missing);
    let label = match &access {
        Access::Signed(_) => "Sign out",
        Access::Expired(_) => "Sign in again",
        Access::Missing => "Sign in…",
    };
    let button = gtk::Button::with_label(label);
    button.set_valign(gtk::Align::Center);
    let panel = panel.clone();
    if access.ready() {
        button.connect_clicked(move |_| sign_out(&panel, feed));
        return button;
    }
    button.add_css_class("suggested-action");
    // A sign-in that cannot start says why on the row, rather than being a
    // button that fails. A form is a dialog, which always can.
    if feed.form().is_none()
        && let Err(why) = providers::can_sign_in(feed.id)
    {
        button.set_sensitive(false);
        row.set_subtitle(&format!("{} · {why}", feed.covers));
        return button;
    }
    let anchor = row.clone();
    button.connect_clicked(move |_| match (feed.form().is_some(), trades(feed)) {
        (true, true) => ask_broker(&panel, &anchor),
        (true, false) => ask_form(&panel, feed, &anchor),
        (false, _) => start_sign_in(&panel, feed),
    });
    button
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
/// `None` for a move that changes nothing or names no source.
fn moved<T: Copy>(order: &[T], from: usize, onto: usize) -> Option<Vec<T>> {
    if from == onto || from >= order.len() || onto >= order.len() {
        return None;
    }
    let mut order = order.to_vec();
    let lifted = order.remove(from);
    order.insert(onto, lifted);
    Some(order)
}

/// The sources not added yet, one press each.
fn add_button(panel: &Rc<Panel>, missing: Vec<&'static Listed>) -> gtk::MenuButton {
    let add = gtk::MenuButton::new();
    add.set_label("Add a data feed…");
    add.set_halign(gtk::Align::Start);
    add.set_margin_top(12);
    let choices = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    for feed in missing {
        let button = gtk::Button::with_label(feed.label);
        button.add_css_class("flat");
        if let Some(label) = button.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
        }
        let panel = panel.clone();
        let shut = popover.clone();
        let anchor = add.clone();
        button.connect_clicked(move |_| {
            shut.popdown();
            add_source(&panel, feed, &anchor);
        });
        choices.append(&button);
    }
    popover.set_child(Some(&choices));
    add.set_popover(Some(&popover));
    add
}

/// A broker connection's feed is added by adding a broker login, in
/// Trading, unless there is one already to sign in through. Any other is
/// added at once, and signed in to from its row.
fn add_source(panel: &Rc<Panel>, feed: &'static Listed, anchor: &impl IsA<gtk::Widget>) {
    match trades(feed) && through_trading(panel, feed).is_none() {
        true => ask_broker(panel, anchor),
        false => {
            feeds::add_source(&panel.store, feed.id);
            panel.changed();
        }
    }
}

fn remove_source(panel: &Rc<Panel>, feed: &'static Listed) {
    panel.notes.borrow_mut().remove(feed.id);
    feeds::remove_source(&panel.store, feed.id);
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
/// which puts the feed on top of the sources and signs it in.
fn ask_broker(panel: &Rc<Panel>, anchor: &impl IsA<gtk::Widget>) {
    if let Some(add_broker) = &panel.add_broker {
        add_broker(anchor.upcast_ref());
    }
}

/// A feed with a form of its own: what is typed is kept, and the feed signed
/// in with it.
fn ask_form(panel: &Rc<Panel>, feed: &'static Listed, anchor: &impl IsA<gtk::Widget>) {
    let Some(form) = Form::for_feed(feed) else { return };
    let panel = panel.clone();
    login_form::ask(anchor, form, move |typed| {
        match feeds::keep_login(&panel.store, feed, &typed.values()) {
            Ok(()) => start_sign_in(&panel, feed),
            Err(error) => {
                panel.notes.borrow_mut().insert(feed.id, format!("not saved · {error}"));
                panel.changed();
            }
        }
    });
}

/// Run a sign-in off the main loop, showing on the row how it goes, then
/// have the window read the sources again.
fn start_sign_in(panel: &Rc<Panel>, feed: &'static Listed) {
    if panel.signing_in.get().is_some() {
        return;
    }
    panel.signing_in.set(Some(feed.id));
    let opening = match feed.setup.map(|setup| setup.how()) {
        Some(How::Browser) => "opening a browser; sign in there",
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
                    // A refusal is a state the source keeps, and the row
                    // reads it back in the provider's words; anything else
                    // is said once, here.
                    if let Err(error) = outcome
                        && !matches!(providers::access(feed.id), Some(Access::Expired(_)))
                    {
                        notes.insert(feed.id, format!("sign-in failed · {error}"));
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

    /// A developer's login file stands in for the broker connection's form.
    fn stood_in() -> bool {
        providers::connections().filter_map(|feed| feed.setup).any(|setup| setup.stood_in())
    }

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
        let group = panel.group.borrow().clone().expect("the sources are drawn");
        action_rows(group.upcast_ref()).iter().map(|row| row.title().to_string()).collect()
    }

    /// With nothing configured, one row, Yahoo, and the button to add
    /// another. No brokerage, no "Experimental", no sign-in.
    #[test]
    fn nothing_configured_is_one_yahoo_row() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = panel(&store);
        rebuild(&panel);
        assert_eq!(titles(&panel), vec!["Yahoo Finance"]);
        let group = panel.group.borrow().clone().unwrap();
        let words = words_under(group.upcast_ref());
        assert!(words.iter().any(|word| word == "Add a data feed…"), "{words:?}");
        // The sources still to add are in the button's menu, not on the page.
        let yahoo = &action_rows(group.upcast_ref())[0];
        let words = words_under(yahoo.upcast_ref());
        for absent in ["Experimental", "Sign in…", "Sign out", "Remove"] {
            assert!(!words.iter().any(|word| word == absent), "{absent} is on the page: {words:?}");
        }
        assert!(yahoo.subtitle().unwrap_or_default().starts_with("Stocks, ETFs"), "{:?}", yahoo.subtitle());
    }

    /// An added source gets a row above Yahoo, with Remove, and
    /// thinkorswim's says Experimental. The order written is the order
    /// drawn, and moving one writes the new order.
    #[test]
    fn the_order_is_stored_and_drawn() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = panel(&store);
        let tos = providers::listed("tos").unwrap();
        let rithmic = providers::listed(RITHMIC).unwrap();
        feeds::add_source(&store, RITHMIC);
        feeds::add_source(&store, "tos");
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
    }

    /// Yahoo moves like any other source, and can be removed and added back
    /// through the button; the last source left offers no Remove.
    #[test]
    fn yahoo_moves_and_goes_like_any_other_but_the_last_stays() {
        let order = ["rithmic", "tos", "yahoo"];
        assert_eq!(moved(&order, 1, 1), None, "dropped on itself");
        assert_eq!(moved(&order, 3, 0), None, "no such source");
        assert_eq!(moved(&order, 2, 0), Some(vec!["yahoo", "rithmic", "tos"]));
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = panel(&store);
        feeds::add_source(&store, "tos");
        rebuild(&panel);
        assert_eq!(titles(&panel), vec!["thinkorswim", "Yahoo Finance"]);
        let yahoo = providers::listed("yahoo").unwrap();
        remove_source(&panel, yahoo);
        assert_eq!(titles(&panel), vec!["thinkorswim"]);
        let group = panel.group.borrow().clone().unwrap();
        let words = words_under(action_rows(group.upcast_ref())[0].upcast_ref());
        assert!(!words.iter().any(|w| w == "Remove"), "the last source: {words:?}");
        add_source(&panel, yahoo, &gtk::Button::new());
        assert_eq!(titles(&panel), vec!["thinkorswim", "Yahoo Finance"]);
        let order = moved(&panel.added(), 1, 0).expect("a move");
        panel.reorder(order);
        assert_eq!(store.setting(feeds::SETTING).as_deref(), Some(r#"["yahoo","tos"]"#));
        assert_eq!(titles(&panel), vec!["Yahoo Finance", "thinkorswim"]);
    }

    /// Remove takes the source off the order; the last one removed leaves
    /// no setting. While Rithmic signs in through a login in Trading its row
    /// says so and offers neither Remove nor a sign-in of its own.
    #[test]
    fn remove_clears_the_setting_and_trading_owns_rithmics_login() {
        if !crate::ui::gtk_ready() || stood_in() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = panel(&store);
        feeds::add_source(&store, RITHMIC);
        rebuild(&panel);
        let group = panel.group.borrow().clone().unwrap();
        let words = words_under(action_rows(group.upcast_ref())[0].upcast_ref());
        assert!(words.iter().any(|w| w == "Remove"), "{words:?}");
        remove_source(&panel, providers::listed(RITHMIC).unwrap());
        assert_eq!(store.setting(feeds::SETTING), None);
        assert_eq!(titles(&panel), vec!["Yahoo Finance"]);

        let kept = crate::trading::tests::Kept::default();
        let typed = crate::trading::Typed {
            name: "Lucid".into(),
            user: "trader".into(),
            password: "PLANTED-PASSWORD".into(),
            system: String::new(),
            gateway: String::new(),
        };
        crate::trading::add_login(&store, &kept, &typed).unwrap();
        rebuild(&panel);
        assert_eq!(titles(&panel), vec!["Rithmic", "Yahoo Finance"]);
        let group = panel.group.borrow().clone().unwrap();
        let row = &action_rows(group.upcast_ref())[0];
        assert!(row.subtitle().unwrap_or_default().ends_with("signs in through Lucid in Trading"), "{:?}", row.subtitle());
        let words = words_under(row.upcast_ref());
        for absent in ["Remove", "Sign in…", "Sign out"] {
            assert!(!words.iter().any(|w| w == absent), "{absent}: {words:?}");
        }
    }

    /// A source's row offers one action, and which one follows how it
    /// stands. Rithmic with nobody saved is not signed in.
    #[test]
    fn a_source_offers_the_action_its_state_calls_for() {
        if !crate::ui::gtk_ready() || stood_in() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        feeds::remember_logins(&store);
        let panel = panel(&store);
        feeds::add_source(&store, RITHMIC);
        rebuild(&panel);
        let group = panel.group.borrow().clone().unwrap();
        let row = &action_rows(group.upcast_ref())[0];
        assert_eq!(row.subtitle().as_deref(), Some("CME futures, front month · not signed in"));
        let words = words_under(row.upcast_ref());
        assert!(words.iter().any(|w| w == "Sign in…"), "{words:?}");
        assert!(!words.iter().any(|w| w == "Sign out"), "{words:?}");
    }

    /// A feed no code here names gets its row, its form and its sign-in
    /// from what it declares, and charts are routed to it: the fake rich
    /// feed, which is in no list a person is offered.
    #[test]
    fn a_feed_nobody_names_gets_its_row_its_form_and_its_charts() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let fake = providers::listed(omacharts_engine::providers::fake::LISTED.id).expect("in a test build");
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = panel(&store);
        add_source(&panel, fake, &gtk::Button::new());
        assert_eq!(titles(&panel), vec![fake.label, "Yahoo Finance"]);
        let group = panel.group.borrow().clone().unwrap();
        let row = action_rows(group.upcast_ref())[0].clone();
        assert_eq!(row.subtitle().unwrap_or_default(), format!("{} · not signed in", fake.covers));
        let words = words_under(row.upcast_ref());
        assert!(words.iter().any(|w| w == "Sign in…"), "{words:?}");
        assert!(words.iter().any(|w| w == "Remove"), "{words:?}");

        // Its form is the fields it declares, under its name.
        let form = Form::for_feed(fake).expect("a form");
        assert_eq!(form.heading, format!("Sign in to {}", fake.label));
        let declared: Vec<&str> = fake.form().unwrap().iter().map(|field| field.title).collect();
        assert_eq!(login_form::tests::titles(&login_form::dialog(form, |_| {})), declared);

        // What the form hands over is kept, the secret in the feed's own
        // keyring and never in a setting, and the row says it is live.
        let typed: Vec<(&'static str, String)> = fake
            .form()
            .unwrap()
            .iter()
            .map(|field| match field.kind {
                providers::FieldKind::Secret => (field.key, "PLANTED-SECRET".to_string()),
                _ => (field.key, format!("typed {}", field.key)),
            })
            .collect();
        feeds::keep_login(&store, fake, &typed).unwrap();
        assert!(!store.setting(feeds::LOGINS).unwrap_or_default().contains("PLANTED-SECRET"));
        assert!(feeds::sign_in_now(fake.id, |_| {}).is_ok());
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

        // Signed out from its row, it keeps what was typed but the user.
        sign_out(&panel, fake);
        assert_eq!(providers::access(fake.id), Some(Access::Missing));
        assert!(store.setting(feeds::LOGINS).unwrap_or_default().contains("typed server"));
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

    /// Every source has a few words for its row.
    #[test]
    fn every_source_says_what_it_covers() {
        for feed in providers::LISTED {
            assert!(!feed.covers.is_empty(), "{} has nothing to say", feed.id);
        }
    }
}
