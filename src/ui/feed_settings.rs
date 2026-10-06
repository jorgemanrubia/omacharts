//! Choosing a data feed, and signing in to one that needs it.
//!
//! A page of its own rather than a row in the settings, because picking a
//! feed is not like picking a colour: one of them charts your brokerage
//! account, which means a sign-in, a browser, a session that expires, and
//! four different things that can be true of it. A combo box could hold the
//! choice and would have nowhere to say any of that.
//!
//! What the page is built from comes from the feeds themselves — the list,
//! each one's summary, and for a feed that needs an account, its own
//! instructions. Nothing about brokerages or browsers is written here, so
//! the third feed arrives as a folder in the engine and this file does not
//! change.
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
    signing_in: Rc<std::cell::Cell<bool>>,
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
        signing_in: Rc::new(std::cell::Cell::new(false)),
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
        row.set_subtitle(feed.summary);

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
    let Some(setup) = feed.setup else {
        return;
    };
    let group = setup_group(panel, feed, setup);
    panel.page.add(&group);
    *panel.setup.borrow_mut() = Some(group);
}

fn setup_group(
    panel: &Rc<Panel>,
    feed: &'static Listed,
    setup: &'static providers::Setup,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(feed.label);
    group.set_description(Some(setup.explain));

    let access = providers::access(feed.id).unwrap_or(Access::Missing);

    // What is true right now, first. A static paragraph about signing in is
    // no use to somebody who wants to know whether they are signed in.
    let state = adw::ActionRow::new();
    state.set_title("Session");
    // Plain text: these lines carry paths and a brokerage's own wording, and
    // an ampersand in one must not take the row out.
    state.set_use_markup(false);
    state.set_subtitle(&access.line());
    state.set_subtitle_lines(0);
    let icon = gtk::Image::from_icon_name(match access {
        Access::Signed(_) => "emblem-ok-symbolic",
        Access::Missing => "dialog-information-symbolic",
        _ => "dialog-warning-symbolic",
    });
    // Amber for the two states that are wrong, and not for the one that is
    // merely not done yet: a fresh install is not a warning, and colouring
    // it like one teaches somebody to ignore the colour.
    if matches!(access, Access::Expired(_) | Access::Refused(_)) {
        icon.add_css_class("warning");
    }
    state.add_prefix(&icon);

    let spinner = gtk::Spinner::new();
    spinner.set_valign(gtk::Align::Center);
    spinner.set_visible(false);
    state.add_suffix(&spinner);
    group.add(&state);

    // The button, and the reason it is not a button when it cannot work.
    let action = adw::ActionRow::new();
    action.set_title(match access {
        Access::Signed(_) => "Sign in again",
        Access::Expired(_) => "Sign in again",
        _ => "Sign in",
    });

    match providers::can_sign_in(feed.id) {
        Ok(browser) => {
            action.set_subtitle(&format!("Opens {browser}"));
            let button = gtk::Button::with_label("Sign in…");
            button.set_valign(gtk::Align::Center);
            if !access.ready() {
                button.add_css_class("suggested-action");
            }
            let panel_for_click = panel.clone();
            let row = state.clone();
            let spinning = spinner.clone();
            let pressed = button.clone();
            button.connect_clicked(move |_| {
                start_sign_in(&panel_for_click, feed, &row, &spinning, &pressed);
            });
            action.add_suffix(&button);
        }
        // A machine with no Chromium-family browser cannot do this at all.
        // An enabled button that fails teaches somebody nothing; the row
        // says what to install instead.
        Err(why) => {
            action.set_subtitle(&why);
            action.set_subtitle_lines(0);
            action.add_prefix(&gtk::Image::from_icon_name("dialog-warning-symbolic"));
            action.set_sensitive(false);
        }
    }
    group.add(&action);

    // Only offered when there is something to forget.
    if !matches!(access, Access::Missing) {
        let out = adw::ActionRow::new();
        out.set_title("Sign out");
        out.set_subtitle("Forgets the saved session. The browser stays signed in.");
        out.set_subtitle_lines(0);
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
        out.add_suffix(&button);
        group.add(&out);
    }

    // One row, both lines. A row each, all titled "Needs", reads as a
    // stutter rather than as a list.
    let needs = adw::ActionRow::new();
    needs.set_title("Needs");
    needs.set_subtitle(&setup.requires.join("\n"));
    needs.set_subtitle_lines(0);
    group.add(&needs);

    for (what, where_it_is) in providers::places(feed.id) {
        let row = adw::ActionRow::new();
        row.set_title(what);
        row.set_subtitle(&home_relative(&where_it_is));
        row.set_subtitle_lines(0);
        row.add_css_class("property");
        group.add(&row);
    }

    let scope = adw::ActionRow::new();
    scope.set_title("What it does");
    scope.set_subtitle(setup.scope);
    scope.set_subtitle_lines(0);
    group.add(&scope);

    group
}

/// Run the browser login off the main loop, showing what it says.
fn start_sign_in(
    panel: &Rc<Panel>,
    feed: &'static Listed,
    state: &adw::ActionRow,
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
    state.set_subtitle("Opening a browser. Sign in there — this waits for you.");

    let (sender, receiver) = async_channel::unbounded::<Progress>();
    let lines = sender.clone();
    std::thread::spawn(move || {
        let outcome = providers::sign_in(feed.id, move |line| {
            let _ = lines.send_blocking(Progress::Line(line.to_string()));
        });
        let _ = sender.send_blocking(Progress::Done(outcome));
    });

    let panel = panel.clone();
    let state = state.clone();
    let spinner = spinner.clone();
    let button = button.clone();
    glib::spawn_future_local(async move {
        while let Ok(progress) = receiver.recv().await {
            match progress {
                // The browser's own commentary — which page it is on, that
                // it is waiting for a login. Shown as it arrives, because
                // ten silent minutes look like a hang.
                Progress::Line(line) => state.set_subtitle(&line),
                Progress::Done(outcome) => {
                    spinner.stop();
                    spinner.set_visible(false);
                    button.set_sensitive(true);
                    panel.signing_in.set(false);
                    if let Err(error) = outcome {
                        state.set_subtitle(&format!("Sign-in failed · {error}"));
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

/// A path as somebody would say it.
fn home_relative(path: &str) -> String {
    let home = crate::store::home();
    match std::path::Path::new(path).strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.to_string(),
    }
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
            assert!(!feed.summary.is_empty(), "{} has no summary", feed.id);
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
            signing_in: Rc::new(std::cell::Cell::new(false)),
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
