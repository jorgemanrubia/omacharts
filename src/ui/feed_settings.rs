//! Choosing a data feed, and signing in to one that needs it.
//!
//! A page of its own rather than a row in the settings, because picking a
//! feed is not like picking a colour: one of them charts your brokerage
//! account, which means a sign-in, a browser, a session that expires, and
//! four different things that can be true of it. A combo box could hold the
//! choice and would have nowhere to say any of that.
//!
//! What the page is built from comes from the feeds themselves — the list,
//! what each one serves, how fresh it is (asked of the provider, so it stops
//! saying "real time" the moment that stops being true), and whether it
//! needs signing in to. Nothing about brokerages or browsers is written
//! here, so the third feed arrives as a folder in the engine and this file
//! does not change.
//!
//! The sign-in runs on a thread of its own and reports back over a channel
//! the GTK main loop polls, because it waits for a person: a browser opens,
//! they type a password and a code off their phone, and the ten minutes that
//! may take must not be ten minutes of frozen window. That is the same shape
//! the loader uses for fetching bars, for the same reason.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::providers::{self, Access, Listed};

use crate::feeds;
use crate::store::Store;

/// What the sign-in thread sends back.
enum Progress {
    /// One line from the browser login, as it happens.
    Line(String),
    /// Finished. `Err` carries what went wrong, in the client's words.
    Done(Result<(), String>),
}

struct Panel {
    store: Rc<Store>,
    /// Redraws the window's provider row and anything else the choice
    /// shows. Called when the stored feed changes, and nowhere else.
    on_change: Rc<dyn Fn()>,
    page: adw::PreferencesPage,
    banner: adw::Banner,
    /// The group describing the chosen feed's sign-in, rebuilt whenever
    /// the choice or the session state changes. Held so it can be taken off
    /// the page again — a feed that needs nothing must leave nothing behind.
    setup: RefCell<Option<adw::PreferencesGroup>>,
    /// True while a browser login is outstanding, so a second press of the
    /// button cannot open a second browser onto the same profile.
    ///
    /// A bare `Cell`: the panel itself is behind an `Rc`, and everything
    /// that reads this holds a clone of that one.
    signing_in: std::cell::Cell<bool>,
}

/// Opens the feed page over the settings dialog.
pub fn push(
    dialog: &adw::PreferencesDialog,
    store: Rc<Store>,
    on_change: Rc<dyn Fn()>,
) {
    let page = adw::PreferencesPage::new();
    let banner = adw::Banner::new("");

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    // Under the header, over the page: where libadwaita puts a sentence that
    // is about the whole page rather than about one row.
    toolbar.add_top_bar(&banner);
    toolbar.set_content(Some(&page));

    let panel = Rc::new(Panel {
        store,
        on_change,
        page,
        banner,
        setup: RefCell::new(None),
        signing_in: std::cell::Cell::new(false),
    });

    panel.page.add(&feeds_group(&panel));
    rebuild_setup(&panel);
    show_restart_note(&panel);

    // Nothing packed into the header: a navigation page draws its own back
    // button, and every choice here applies as it is made, so a Done button
    // beside it would be a second way to do nothing.
    dialog.push_subpage(&adw::NavigationPage::new(&toolbar, "Provider"));
}

/// The feeds, in the order the engine offers them: the first is the default.
fn feeds_group(panel: &Rc<Panel>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Data feed");
    group.set_description(Some(
        "Where prices come from. The choice applies the next time Omacharts starts.",
    ));

    let chosen = feeds::stored(&panel.store).id;
    let mut first: Option<gtk::CheckButton> = None;

    for feed in providers::LISTED {
        let row = adw::ActionRow::new();
        row.set_title(feed.label);
        row.set_subtitle(&providers::described(feed));

        let tick = gtk::CheckButton::new();
        tick.set_valign(gtk::Align::Center);
        match &first {
            // Radio buttons, by sharing the first one's group — one feed at
            // a time, and the group is what makes clicking one release the
            // others.
            Some(leader) => tick.set_group(Some(leader)),
            None => first = Some(tick.clone()),
        }
        tick.set_active(feed.id == chosen);

        // A word, not a warning. The feed works; what it rests on is a web
        // session and an undocumented gateway, so it can be taken away by
        // somebody else's release. Said quietly, in the row's own small
        // type, because this is a thing to know when choosing rather than a
        // thing to be stopped by.
        if feed.experimental {
            let tag = gtk::Label::new(Some("Experimental"));
            tag.add_css_class("caption");
            tag.add_css_class("dim-label");
            tag.set_valign(gtk::Align::Center);
            row.add_suffix(&tag);
        }

        let panel_for_tick = panel.clone();
        tick.connect_toggled(move |tick| {
            if !tick.is_active() {
                return;
            }
            choose(&panel_for_tick, feed);
        });

        row.add_prefix(&tick);
        row.set_activatable_widget(Some(&tick));
        group.add(&row);
    }

    group
}

/// Store the choice, and show what follows from it.
fn choose(panel: &Rc<Panel>, feed: &'static Listed) {
    panel.store.set_setting(feeds::SETTING, feed.id);
    (panel.on_change)();
    rebuild_setup(panel);
    show_restart_note(panel);
}

/// The feed is read when the process starts, so a change made here is not a
/// change to what is on screen. Saying so is the whole of the honesty
/// available: the alternative is a panel that looks like it switched the
/// feed and charts that keep coming from the old one.
fn show_restart_note(panel: &Rc<Panel>) {
    let stored = feeds::stored(&panel.store);
    let running = feeds::in_use(&panel.store);
    if stored.id == running.id {
        panel.banner.set_revealed(false);
        return;
    }
    panel.banner.set_title(&format!(
        "Charts are still coming from {}. {} starts with the next Omacharts.",
        running.label, stored.label
    ));
    panel.banner.set_revealed(true);
}

/// Replace the sign-in group with the chosen feed's, or with nothing.
fn rebuild_setup(panel: &Rc<Panel>) {
    if let Some(old) = panel.setup.borrow_mut().take() {
        panel.page.remove(&old);
    }
    let feed = feeds::stored(&panel.store);
    if !feed.needs_sign_in() {
        return;
    }
    let group = setup_group(panel, feed);
    panel.page.add(&group);
    *panel.setup.borrow_mut() = Some(group);
}

/// The chosen feed's sign-in, which is one row: the group's title names the
/// feed, and the row is the whole of what there is to do about it.
fn setup_group(panel: &Rc<Panel>, feed: &'static Listed) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(feed.label);

    let access = providers::access(feed.id).unwrap_or(Access::Missing);

    // One action, and which one it is says where somebody stands: an offer
    // to sign in means there is nothing usable saved, and an offer to sign
    // out means there is. A row spelling the same fact out in a sentence
    // beside them would only be a second thing to keep in agreement.
    if access.ready() {
        group.add(&sign_out_row(panel, feed));
    } else {
        group.add(&sign_in_row(panel, feed, &access));
    }

    group
}

/// The sign-in offer, and the reason it is not a button when it cannot work.
fn sign_in_row(panel: &Rc<Panel>, feed: &'static Listed, access: &Access) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title(match access {
        // A session that went bad is not a fresh install, and "again" is the
        // whole of the difference worth saying.
        Access::Expired(_) | Access::Refused(_) => "Sign in again",
        _ => "Sign in",
    });
    // Plain text, and room to wrap. The subtitle is empty until the button
    // is pressed, and then it carries the client's own lines, which can be
    // long and can hold an ampersand that must not take the row out.
    row.set_use_markup(false);
    row.set_subtitle_lines(0);

    match providers::can_sign_in(feed.id) {
        Ok(_) => {
            let spinner = gtk::Spinner::new();
            spinner.set_valign(gtk::Align::Center);
            spinner.set_visible(false);
            row.add_suffix(&spinner);

            let button = gtk::Button::with_label("Sign in…");
            button.set_valign(gtk::Align::Center);
            button.add_css_class("suggested-action");
            let panel_for_click = panel.clone();
            let progress = row.clone();
            let spinning = spinner.clone();
            let pressed = button.clone();
            button.connect_clicked(move |_| {
                start_sign_in(&panel_for_click, feed, &progress, &spinning, &pressed);
            });
            row.add_suffix(&button);
        }
        // A machine with no Chromium-family browser cannot do this at all.
        // An enabled button that fails teaches somebody nothing; the row
        // says what to install instead.
        Err(why) => {
            row.set_subtitle(&why);
            row.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
            row.set_sensitive(false);
        }
    }

    row
}

/// The way out of a session that works, and the only row shown while one
/// does.
fn sign_out_row(panel: &Rc<Panel>, feed: &'static Listed) -> adw::ActionRow {
    let row = adw::ActionRow::new();
    row.set_title("Sign out");

    let button = gtk::Button::with_label("Sign out");
    button.add_css_class("destructive-action");
    button.set_valign(gtk::Align::Center);
    let panel_for_click = panel.clone();
    button.connect_clicked(move |_| {
        if let Err(error) = providers::sign_out(feed.id) {
            eprintln!("omacharts: {error}");
        }
        rebuild_setup(&panel_for_click);
    });
    row.add_suffix(&button);

    row
}

/// Run the browser login off the main loop, showing what it says.
fn start_sign_in(
    panel: &Rc<Panel>,
    feed: &'static Listed,
    row: &adw::ActionRow,
    spinner: &gtk::Spinner,
    button: &gtk::Button,
) {
    // One browser at a time. Two logins would fight over the same profile
    // directory, and the second one loses in a way that looks like a bug.
    if panel.signing_in.replace(true) {
        return;
    }

    button.set_sensitive(false);
    spinner.set_visible(true);
    spinner.start();
    row.set_subtitle("Opening a browser. Sign in there — this waits for you.");

    let (sender, receiver) = async_channel::unbounded::<Progress>();
    let lines = sender.clone();
    std::thread::spawn(move || {
        let outcome = providers::sign_in(feed.id, move |line| {
            let _ = lines.send_blocking(Progress::Line(line.to_string()));
        });
        let _ = sender.send_blocking(Progress::Done(outcome));
    });

    let panel = panel.clone();
    let row = row.clone();
    let spinner = spinner.clone();
    let button = button.clone();
    glib::spawn_future_local(async move {
        while let Ok(progress) = receiver.recv().await {
            match progress {
                // The browser's own commentary — which page it is on, that
                // it is waiting for a login. Shown as it arrives, because
                // ten silent minutes look like a hang.
                Progress::Line(line) => row.set_subtitle(&line),
                Progress::Done(outcome) => {
                    spinner.stop();
                    spinner.set_visible(false);
                    button.set_sensitive(true);
                    panel.signing_in.set(false);
                    if let Err(error) = outcome {
                        row.set_subtitle(&format!("Sign-in failed · {error}"));
                        return;
                    }
                    // Rebuilt rather than relabelled: a session that now
                    // exists changes which rows belong here.
                    rebuild_setup(&panel);
                    (panel.on_change)();
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yahoo first, because the first feed in the list is the default and
    /// the panel must not be the place that disagrees about which that is.
    #[test]
    fn the_first_feed_offered_is_the_default_one() {
        assert_eq!(providers::LISTED[0].id, providers::DEFAULT);
        assert_eq!(providers::LISTED[0].id, "yahoo");
    }

    /// Every feed the panel lists has a line under it, and the one that
    /// needs signing in has instructions for doing so. A feed added to the
    /// catalogue without either would render as a bare name.
    #[test]
    fn every_feed_has_something_to_say_for_itself() {
        for feed in providers::LISTED {
            assert!(!feed.serves.is_empty(), "{} has nothing to say", feed.id);
            match feed.setup {
                None => assert!(providers::access(feed.id).is_none(), "{}", feed.id),
                Some(setup) => {
                    assert!(!setup.explain.is_empty(), "{}", feed.id);
                    assert!(!setup.requires.is_empty(), "{}", feed.id);
                    assert!(!setup.scope.is_empty(), "{}", feed.id);
                    assert!(providers::access(feed.id).is_some(), "{}", feed.id);
                }
            }
        }
    }

    /// The page is built from widgets, so this needs a display. What it
    /// checks is the thing that is easy to get wrong and invisible in a
    /// screenshot: that choosing a feed writes the setting, and that the
    /// rows for the feed that needs a sign-in appear and disappear with it.
    #[test]
    fn choosing_a_feed_stores_it_and_brings_its_setup_with_it() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let page = adw::PreferencesPage::new();
        let panel = Rc::new(Panel {
            store: store.clone(),
            on_change: Rc::new(|| {}),
            page,
            banner: adw::Banner::new(""),
            setup: RefCell::new(None),
            signing_in: std::cell::Cell::new(false),
        });
        rebuild_setup(&panel);
        assert!(panel.setup.borrow().is_none(), "Yahoo needs no sign-in rows");

        let tos = providers::listed("tos").expect("thinkorswim is listed");
        choose(&panel, tos);
        assert_eq!(store.setting(feeds::SETTING).as_deref(), Some("tos"));
        assert!(panel.setup.borrow().is_some(), "thinkorswim's sign-in rows are missing");

        let yahoo = providers::listed("yahoo").expect("yahoo is listed");
        choose(&panel, yahoo);
        assert_eq!(store.setting(feeds::SETTING).as_deref(), Some("yahoo"));
        assert!(panel.setup.borrow().is_none(), "rows for a feed nobody chose");
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

    /// The same rows as title and subtitle, which is what most of these
    /// tests are asking about.
    fn rows_under(widget: &gtk::Widget) -> Vec<(String, String)> {
        action_rows(widget)
            .into_iter()
            .map(|row| (row.title().to_string(), row.subtitle().unwrap_or_default().to_string()))
            .collect()
    }

    /// The words of every label under `widget`. A badge is a label the row
    /// carries, not a property of the row, so there is no other way to ask
    /// what one says.
    fn labels_under(widget: &gtk::Widget) -> Vec<String> {
        let mut found = Vec::new();
        let mut next = widget.first_child();
        while let Some(child) = next {
            if let Ok(label) = child.clone().downcast::<gtk::Label>() {
                found.push(label.label().to_string());
            }
            found.extend(labels_under(&child));
            next = child.next_sibling();
        }
        found
    }

    /// One row, and which one it is is the whole of what the group says
    /// about the session: sign in while there is nothing usable saved, sign
    /// out while there is. Nothing describes the state in words, and the row
    /// on screen is the row that says which browser a login opens, because
    /// there is no other row left to say it.
    #[test]
    fn the_group_offers_one_action_and_it_follows_the_session() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = Rc::new(Panel {
            store,
            on_change: Rc::new(|| {}),
            page: adw::PreferencesPage::new(),
            banner: adw::Banner::new(""),
            setup: RefCell::new(None),
            signing_in: std::cell::Cell::new(false),
        });
        let tos = providers::listed("tos").expect("thinkorswim is listed");
        assert!(tos.needs_sign_in(), "thinkorswim needs signing in to");
        let group = setup_group(&panel, tos);
        let rows = rows_under(group.upcast_ref::<gtk::Widget>());

        assert_eq!(rows.len(), 1, "one action and nothing else: {rows:?}");
        let (title, subtitle) = &rows[0];

        // Whichever state this machine's saved session is in, the row must
        // agree with it: sign out while it is good, sign in while it is not.
        let access = providers::access(tos.id).expect("thinkorswim keeps a session");
        if access.ready() {
            assert_eq!(title, "Sign out", "{}", access.line());
        } else {
            assert!(title.starts_with("Sign in"), "{} offers {title}", access.line());
        }

        // A title and a button, and no sentence under either. The one
        // exception is a machine with no browser to open: that row is a
        // refusal rather than an offer, and has to say what to install.
        let expected = match access.ready() {
            true => String::new(),
            false => providers::can_sign_in(tos.id).err().unwrap_or_default(),
        };
        assert_eq!(subtitle, &expected, "{title} has something to say for itself");
    }

    /// The feed that rests on somebody else's web client says so where it
    /// is chosen, and the feed that rests on nothing of the sort says
    /// nothing. The word is the engine's, so the row cannot be the place
    /// that disagrees about which feed it belongs to.
    #[test]
    fn the_experimental_feed_is_the_only_one_that_says_so() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = Rc::new(Panel {
            store,
            on_change: Rc::new(|| {}),
            page: adw::PreferencesPage::new(),
            banner: adw::Banner::new(""),
            setup: RefCell::new(None),
            signing_in: std::cell::Cell::new(false),
        });
        let group = feeds_group(&panel);

        let rows = action_rows(group.upcast_ref::<gtk::Widget>());
        assert_eq!(rows.len(), providers::LISTED.len(), "a row per feed");
        for row in rows {
            let feed = providers::LISTED
                .iter()
                .find(|feed| feed.label == row.title())
                .expect("every row is a feed");
            let said = labels_under(row.upcast_ref::<gtk::Widget>())
                .iter()
                .any(|text| text == "Experimental");
            assert_eq!(said, feed.experimental, "{} is labelled wrong", feed.id);
        }

        assert!(providers::listed("tos").expect("thinkorswim is listed").experimental);
        assert!(!providers::listed("yahoo").expect("yahoo is listed").experimental);
    }

    /// The group is one action row. Every sentence that used to sit beside
    /// it — the session state, the prerequisites, the file paths, the small
    /// print — is somewhere a person can read it, and none of those places
    /// is this panel.
    #[test]
    fn nothing_in_the_group_describes_itself() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("an empty database"));
        let panel = Rc::new(Panel {
            store,
            on_change: Rc::new(|| {}),
            page: adw::PreferencesPage::new(),
            banner: adw::Banner::new(""),
            setup: RefCell::new(None),
            signing_in: std::cell::Cell::new(false),
        });
        let tos = providers::listed("tos").expect("thinkorswim is listed");
        let group = setup_group(&panel, tos);

        assert!(
            group.description().is_none_or(|text| text.is_empty()),
            "the group explains itself again"
        );
        for (title, _) in rows_under(group.upcast_ref::<gtk::Widget>()) {
            assert!(
                !matches!(title.as_str(), "Session" | "Needs" | "Browser profile" | "What it does"),
                "{title} is back"
            );
        }
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
}
