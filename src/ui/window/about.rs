//! About Omacharts, from the main menu: the name, the version, the website,
//! and a check for a newer release while it is open.
//!
//! An `adw::Dialog` laid out as `adw::AboutDialog` lays itself out — the
//! same widgets in the same order under the same style classes, so the
//! theme draws it as one — rather than the real thing, because the real
//! thing has nowhere to put anything of its own. Its version is a fixed
//! button that copies itself, with no room for a spinner beside it, and
//! nothing can go under it but its own rows. The check needs both.
//!
//! The check is [`crate::update::look`], on a thread of its own: the
//! dialog is on screen before anything is asked, and asks the same server,
//! for the same release, under the same rules as the window's hourly check.
//! "Update" is [`crate::update::fetch`], the download that check makes, and
//! "Restart to update" the corner notice's restart.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::Window;
use crate::update::{self, Offer, Shelf, Version};

/// Where the website row goes.
pub const WEBSITE: &str = "https://omacharts.org";

/// What the dialog shows under its name, besides the version.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shows {
    /// The version and nothing else: up to date, a check that failed, or
    /// one that never ran.
    Version,
    /// The spinner beside the version, while a release is asked for.
    Checking,
    /// **Update**, which downloads `Version`.
    Update(Version),
    /// The download under way, on the button that started it.
    Downloading(Version),
    /// **Restart to update**, which brings `Version` in.
    Restart(Version),
}

pub struct About {
    pub dialog: adw::Dialog,
    #[cfg_attr(not(test), allow(dead_code))]
    version: gtk::Button,
    spinner: gtk::Spinner,
    button: gtk::Button,
    button_label: gtk::Label,
    button_spinner: gtk::Spinner,
    trouble: gtk::Label,
    shows: Cell<Shows>,
}

impl About {
    /// `update` downloads the version offered, and `restart` quits into
    /// the one downloaded.
    pub fn new(update: impl Fn(&Rc<About>, Version) + 'static, restart: impl Fn() + 'static) -> Rc<About> {
        let icon = gtk::Image::from_icon_name(crate::APP_ID);
        icon.set_pixel_size(128);
        icon.add_css_class("icon-dropshadow");
        icon.set_accessible_role(gtk::AccessibleRole::Presentation);

        let name = gtk::Label::new(Some("Omacharts"));
        name.set_wrap(true);
        name.set_justify(gtk::Justification::Center);
        name.add_css_class("title-1");

        // As `adw::AboutDialog` has it: a button that copies the version,
        // and says so in a toast.
        let version = gtk::Button::with_label(crate::launch::VERSION);
        version.add_css_class("app-version");
        version.set_valign(gtk::Align::Center);
        let toasts = adw::ToastOverlay::new();
        let copied = toasts.downgrade();
        version.connect_clicked(move |button| {
            button.clipboard().set_text(crate::launch::VERSION);
            if let Some(toasts) = copied.upgrade() {
                toasts.add_toast(adw::Toast::new("Copied to clipboard"));
            }
        });
        // The spinner sits beside the version without pushing it off
        // centre: an empty square of the spinner's size on the other side.
        let spinner = gtk::Spinner::new();
        spinner.set_size_request(16, 16);
        spinner.set_valign(gtk::Align::Center);
        spinner.set_tooltip_text(Some("Checking for updates"));
        let balance = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        balance.set_size_request(16, 16);
        let version_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        version_row.set_halign(gtk::Align::Center);
        version_row.append(&balance);
        version_row.append(&version);
        version_row.append(&spinner);

        let button_label = gtk::Label::new(None);
        let button_spinner = gtk::Spinner::new();
        button_spinner.set_visible(false);
        let button_content = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        button_content.set_halign(gtk::Align::Center);
        button_content.append(&button_spinner);
        button_content.append(&button_label);
        let button = gtk::Button::new();
        button.set_child(Some(&button_content));
        button.set_halign(gtk::Align::Center);
        button.add_css_class("pill");
        button.add_css_class("suggested-action");
        button.set_visible(false);

        let trouble = gtk::Label::new(None);
        trouble.set_wrap(true);
        trouble.set_justify(gtk::Justification::Center);
        trouble.add_css_class("dim-label");
        trouble.add_css_class("caption");
        trouble.set_visible(false);

        let website = adw::ActionRow::new();
        website.set_title("omacharts.org");
        website.set_activatable(true);
        website.set_tooltip_text(Some(WEBSITE));
        website.add_suffix(&gtk::Image::from_icon_name("adw-external-link-symbolic"));
        let links = adw::PreferencesGroup::new();
        links.add(&website);

        let below = gtk::Box::new(gtk::Orientation::Vertical, 0);
        below.append(&button);
        below.append(&trouble);
        below.append(&links);

        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&icon);
        page.append(&name);
        page.append(&version_row);
        page.append(&below);
        let clamp = adw::Clamp::new();
        clamp.set_child(Some(&page));
        // `adw::AboutDialog` scrolls here; this is short enough not to, and
        // a scrolled window would keep the height it opened at when the
        // button appears. The viewport is what its style is written for.
        let viewport = gtk::Viewport::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
        viewport.set_child(Some(&clamp));
        let scrolled = gtk::Box::new(gtk::Orientation::Vertical, 0);
        scrolled.add_css_class("main-page");
        scrolled.append(&viewport);

        let header = adw::HeaderBar::new();
        header.set_show_title(false);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scrolled));
        toasts.set_child(Some(&toolbar));

        let dialog = adw::Dialog::new();
        dialog.set_title("About Omacharts");
        dialog.set_content_width(360);
        dialog.set_width_request(360);
        dialog.add_css_class("about");
        dialog.set_child(Some(&toasts));

        let about = Rc::new(About {
            dialog,
            version,
            spinner,
            button,
            button_label,
            button_spinner,
            trouble,
            shows: Cell::new(Shows::Version),
        });
        about.show(Shows::Version);

        let weak = Rc::downgrade(&about);
        about.button.connect_clicked(move |_| {
            let Some(about) = weak.upgrade() else { return };
            match about.shows.get() {
                Shows::Update(version) => update(&about, version),
                Shows::Restart(_) => restart(),
                _ => {}
            }
        });
        let weak = Rc::downgrade(&about);
        website.connect_activated(move |_| {
            let parent = weak.upgrade().and_then(|about| about.dialog.root()).and_downcast::<gtk::Window>();
            gtk::UriLauncher::new(WEBSITE).launch(parent.as_ref(), gio::Cancellable::NONE, |_| {});
        });
        about
    }

    pub fn show(&self, shows: Shows) {
        self.shows.set(shows);
        let checking = shows == Shows::Checking;
        self.spinner.set_spinning(checking);
        self.spinner.set_opacity(if checking { 1.0 } else { 0.0 });
        let (label, downloading) = match shows {
            Shows::Version | Shows::Checking => {
                self.button.set_visible(false);
                return;
            }
            Shows::Update(_) => ("Update", false),
            Shows::Downloading(_) => ("Downloading", true),
            Shows::Restart(_) => ("Restart to update", false),
        };
        self.button_label.set_label(label);
        self.button_spinner.set_visible(downloading);
        self.button_spinner.set_spinning(downloading);
        // Dimmed while it works, solid again when there is something to do.
        self.button.set_sensitive(!downloading);
        self.button.set_visible(true);
        if !downloading {
            self.say(None);
        }
    }

    /// A line under the button saying why the last download did not finish.
    pub fn say(&self, trouble: Option<&str>) {
        self.trouble.set_label(trouble.unwrap_or_default());
        self.trouble.set_visible(trouble.is_some());
    }
}

/// What the tests read back.
#[cfg(test)]
impl About {
    pub fn shows(&self) -> Shows {
        self.shows.get()
    }

    pub fn version_text(&self) -> String {
        self.version.label().unwrap_or_default().to_string()
    }

    pub fn spinning(&self) -> bool {
        self.spinner.is_spinning()
    }

    pub fn button_text(&self) -> Option<String> {
        self.button.is_visible().then(|| self.button_label.label().to_string())
    }

    pub fn trouble_text(&self) -> Option<String> {
        self.trouble.is_visible().then(|| self.trouble.label().to_string())
    }

    pub fn press(&self) {
        self.button.emit_clicked();
    }
}

impl Window {
    /// The About dialog, checking for a newer release as it opens.
    pub(super) fn show_about(self: &Rc<Self>) {
        let fetcher = Rc::downgrade(self);
        let restarter = Rc::downgrade(self);
        let about = About::new(
            move |about, version| {
                if let Some(this) = fetcher.upgrade() {
                    this.fetch_from_about(about, version);
                }
            },
            move || {
                if let Some(this) = restarter.upgrade() {
                    this.restart_to_update();
                }
            },
        );
        about.dialog.present(Some(&self.window));
        let closer = Rc::downgrade(self);
        about.dialog.connect_closed(move |_| {
            if let Some(this) = closer.upgrade() {
                this.about.take();
            }
        });
        self.look_from_about(&about);
        *self.about.borrow_mut() = Some(about);
    }

    /// Ask for a newer release, unless this process is one that never does:
    /// a build in `target/`, `OMACHARTS_NO_UPDATE`, root.
    fn look_from_about(self: &Rc<Self>, about: &Rc<About>) {
        let Some(origin) = update::checks_here() else {
            about.show(Shows::Version);
            return;
        };
        let running = Version::running();
        // The corner notice already knows of a download: no need to ask.
        if let Some(version) =
            self.messages.update_shown.get().or(self.messages.update_dismissed.get()).filter(|version| *version > running)
        {
            about.show(Shows::Restart(version));
            return;
        }
        about.show(Shows::Checking);
        let (send, receive) = async_channel::bounded::<Option<Offer>>(1);
        std::thread::spawn(move || {
            let shelf = Shelf::new(update::shelf_dir());
            let installed = || update::installed_version(origin);
            let looked = update::look(&update::GitHub::new(), &shelf, running, &installed);
            if let Err(failure) = &looked {
                eprintln!("omacharts: update: {failure}");
            }
            let _ = send.send_blocking(looked.ok());
        });
        let about = Rc::downgrade(about);
        glib::spawn_future_local(async move {
            let offer = receive.recv().await.ok().flatten();
            if let Some(about) = about.upgrade() {
                about.show(shows_for(offer));
            }
        });
    }

    /// "Update": download `version` as the window's own check would. Once
    /// it is ready the corner notice offers it too, so closing the dialog
    /// meanwhile loses nothing.
    fn fetch_from_about(self: &Rc<Self>, about: &Rc<About>, version: Version) {
        let Some(origin) = update::checks_here() else { return };
        about.show(Shows::Downloading(version));
        about.say(None);
        let (send, receive) = async_channel::bounded::<Result<Option<Version>, String>>(1);
        std::thread::spawn(move || {
            let shelf = Shelf::new(update::shelf_dir());
            let installed = || update::installed_version(origin);
            let fetched = update::fetch(&update::GitHub::new(), &shelf, Version::running(), &installed)
                .map_err(|failure| failure.to_string());
            if let Err(why) = &fetched {
                eprintln!("omacharts: update: {why}");
            }
            let _ = send.send_blocking(fetched);
        });
        let (this, about) = (Rc::downgrade(self), Rc::downgrade(about));
        glib::spawn_future_local(async move {
            let fetched = receive.recv().await.unwrap_or_else(|_| Err("the download stopped".into()));
            if let (Ok(Some(ready)), Some(this)) = (&fetched, this.upgrade()) {
                this.offer_restart(*ready);
            }
            let Some(about) = about.upgrade() else { return };
            about.show_fetched(version, fetched);
        });
    }

    #[cfg(test)]
    pub(super) fn about(&self) -> Option<Rc<About>> {
        self.about.borrow().clone()
    }
}

impl About {
    /// What a download from the button came to.
    pub fn show_fetched(&self, version: Version, fetched: Result<Option<Version>, String>) {
        match fetched {
            Ok(Some(ready)) => self.show(Shows::Restart(ready)),
            // pacman installed it meanwhile: nothing left to offer.
            Ok(None) => self.show(Shows::Version),
            Err(why) => {
                self.show(Shows::Update(version));
                self.say(Some(&format!("Could not update: {why}")));
            }
        }
    }
}

/// What a finished check shows. A check that failed shows what one that
/// found nothing does: the version, and nothing to say about it.
pub fn shows_for(offer: Option<Offer>) -> Shows {
    match offer {
        Some(Offer::Update(version)) => Shows::Update(version),
        Some(Offer::Restart(version)) => Shows::Restart(version),
        Some(Offer::Nothing) | None => Shows::Version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dialog, what its Update asked for, and whether it restarted.
    type Watched = (Rc<About>, Rc<Cell<Option<Version>>>, Rc<Cell<bool>>);

    fn about() -> Watched {
        let (asked, restarted) = (Rc::new(Cell::new(None)), Rc::new(Cell::new(false)));
        let (a, r) = (asked.clone(), restarted.clone());
        let about = About::new(move |_, version| a.set(Some(version)), move || r.set(true));
        (about, asked, restarted)
    }

    #[test]
    fn the_version_shown_is_the_one_version_says() {
        crate::ui::on_gtk(|| {
            let (about, _, _) = about();
            let line = crate::launch::version_line();
            assert_eq!(Some(about.version_text().as_str()), line.strip_prefix("omacharts "));
            assert_eq!(about.version_text(), Version::running().to_string());
        });
    }

    #[test]
    fn checking_then_up_to_date_shows_the_spinner_then_nothing_more() {
        crate::ui::on_gtk(|| {
            let (about, _, _) = about();
            about.show(Shows::Checking);
            assert!(about.spinning());
            assert_eq!(about.button_text(), None);
            about.show(shows_for(Some(Offer::Nothing)));
            assert!(!about.spinning());
            assert_eq!(about.button_text(), None);
            assert_eq!(about.trouble_text(), None, "nothing said about being up to date");
        });
    }

    #[test]
    fn checking_then_a_newer_release_offers_update_which_downloads_and_offers_the_restart() {
        crate::ui::on_gtk(|| {
            let (about, asked, restarted) = about();
            about.show(Shows::Checking);
            about.show(shows_for(Some(Offer::Update(Version(0, 2, 20)))));
            assert!(!about.spinning());
            assert_eq!(about.button_text().as_deref(), Some("Update"));
            about.press();
            assert_eq!(asked.get(), Some(Version(0, 2, 20)));

            about.show(Shows::Downloading(Version(0, 2, 20)));
            assert_eq!(about.button_text().as_deref(), Some("Downloading"));
            assert!(!about.button.is_sensitive(), "pending while it downloads");
            asked.set(None);
            about.press();
            assert_eq!(asked.get(), None, "not twice");

            about.show_fetched(Version(0, 2, 20), Ok(Some(Version(0, 2, 20))));
            assert_eq!(about.button_text().as_deref(), Some("Restart to update"));
            assert!(about.button.is_sensitive());
            about.press();
            assert!(restarted.get());
        });
    }

    #[test]
    fn a_download_that_fails_goes_back_to_update_and_says_why() {
        crate::ui::on_gtk(|| {
            let (about, _, _) = about();
            about.show(Shows::Downloading(Version(0, 2, 20)));
            about.show_fetched(Version(0, 2, 20), Err("releases not reachable: offline".into()));
            assert_eq!(about.button_text().as_deref(), Some("Update"));
            assert_eq!(about.trouble_text().as_deref(), Some("Could not update: releases not reachable: offline"));
            about.show(Shows::Downloading(Version(0, 2, 20)));
            about.show_fetched(Version(0, 2, 20), Ok(Some(Version(0, 2, 20))));
            assert_eq!(about.trouble_text(), None, "said once, until it works");
        });
    }

    #[test]
    fn a_download_already_made_offers_the_restart_straight_away() {
        crate::ui::on_gtk(|| {
            let (about, _, _) = about();
            about.show(shows_for(Some(Offer::Restart(Version(0, 2, 20)))));
            assert!(!about.spinning());
            assert_eq!(about.button_text().as_deref(), Some("Restart to update"));
        });
    }

    #[test]
    fn a_check_that_fails_stops_the_spinner_and_says_nothing() {
        crate::ui::on_gtk(|| {
            let (about, _, _) = about();
            about.show(Shows::Checking);
            about.show(shows_for(None));
            assert!(!about.spinning());
            assert_eq!(about.button_text(), None);
            assert_eq!(about.trouble_text(), None);
        });
    }

    #[test]
    fn where_updates_never_apply_the_dialog_shows_the_version_and_asks_nothing() {
        crate::ui::on_gtk(|| {
            // A test is not an installed omacharts: no origin, as for a
            // build in `target/`, and as `OMACHARTS_NO_UPDATE` or root.
            assert!(update::checks_here().is_none());
            let store = Rc::new(crate::store::Store::memory().expect("a store"));
            let window = crate::ui::window::trade::tests::bare_window(&store);
            window.show_about();
            let about = window.about().expect("the dialog");
            assert_eq!(about.shows(), Shows::Version);
            assert!(!about.spinning());
            assert_eq!(about.button_text(), None);
            assert_eq!(about.version_text(), crate::launch::VERSION);
            about.dialog.force_close();
        });
    }

    #[test]
    fn the_main_menu_ends_with_about_in_a_section_of_its_own() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(crate::store::Store::memory().expect("a store"));
            let window = crate::ui::window::trade::tests::bare_window(&store);
            let button = window.messages.menu_button.borrow().clone().expect("the main menu");
            let menu = button.menu_model().expect("its model");
            let last = menu.n_items() - 1;
            let section = menu.item_link(last, "section").expect("the last item is a section");
            assert_eq!(section.n_items(), 1, "About alone");
            let label = section.item_attribute_value(0, "label", None).and_then(|v| v.get::<String>());
            let action = section.item_attribute_value(0, "action", None).and_then(|v| v.get::<String>());
            assert_eq!(label.as_deref(), Some("About Omacharts"));
            assert_eq!(action.as_deref(), Some("win.about"));
            assert!(window.window.lookup_action("about").is_some());
            assert!(crate::ui::shortcuts::accel("win.about").is_none(), "no key, so not on the shortcuts sheet");
        });
    }
}
