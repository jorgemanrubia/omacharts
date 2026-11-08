//! The Log: a window of its own, beside the charts, listing what the feeds,
//! the brokers and the app have said, and the corner notice and the dot on
//! the main menu that bring an error to somebody who is not reading it.
//!
//! Everything said goes into the engine's ring ([`omacharts_engine::log`])
//! from whatever thread said it, and wakes the window. The window asks what
//! changed since it last looked and puts each entry where [`surface`] says:
//! nowhere else, the corner notice, or a toast. While the Log is open it
//! appends the same entries to its list; while it is closed it does not
//! exist.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};
use omacharts_engine::log::{Entry, Level, Log};

use super::Window;
use crate::ui::notice::{surface, Notice, State, Surface};

/// What the window keeps of the Log: where it last read up to, the notice
/// and the dot, and the Log window while it is open.
pub(super) struct Messages {
    /// The app's log; a test's own, in a test.
    pub log: Cell<&'static Log>,
    /// The log's revision this window has surfaced up to.
    revision: Cell<u64>,
    notice: RefCell<Option<Rc<Notice>>>,
    /// The errors not yet seen in the Log, oldest first, by id.
    unseen: RefCell<Vec<u64>>,
    /// The main menu, which wears a dot while an error dismissed from the
    /// notice is unread.
    pub menu_button: RefCell<Option<gtk::MenuButton>>,
    window: RefCell<Option<Rc<LogWindow>>>,
    /// The series whose stream was said to be lost, to say when it is back.
    pub lost: RefCell<std::collections::HashSet<crate::live::Series>>,
    /// Each login's trouble as last said, so that it is said once.
    pub troubled: RefCell<std::collections::HashMap<String, String>>,
    /// The notice that an update is ready, while it is showing.
    pub update: RefCell<Option<Rc<crate::ui::notice::UpdateNotice>>>,
    /// The version it offers.
    pub update_shown: Cell<Option<crate::update::Version>>,
    /// The version whose notice was dismissed: not offered again this run.
    pub update_dismissed: Cell<Option<crate::update::Version>>,
}

impl Messages {
    pub fn new(log: &'static Log) -> Messages {
        Messages {
            log: Cell::new(log),
            revision: Cell::new(0),
            notice: RefCell::new(None),
            unseen: RefCell::new(Vec::new()),
            menu_button: RefCell::new(None),
            window: RefCell::new(None),
            lost: RefCell::new(Default::default()),
            troubled: RefCell::new(Default::default()),
            update: RefCell::new(None),
            update_shown: Cell::new(None),
            update_dismissed: Cell::new(None),
        }
    }
}

/// The class the main menu wears while an error is unread.
const UNREAD: &str = "unread";

impl Window {
    /// Put `entry` in this window's log.
    pub(super) fn say(&self, entry: Entry) {
        self.messages.log.get().add(entry);
    }

    /// Catch up with the log whenever it is added to. Nothing runs while
    /// nothing is said.
    pub(super) fn watch_log(self: &Rc<Self>) {
        let woken = self.messages.log.get().woken();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while woken.recv().await.is_ok() {
                let Some(this) = weak.upgrade() else { return };
                this.catch_up_log();
            }
        });
    }

    /// Surface what was said since this last ran, and show it in the Log if
    /// it is open.
    pub(super) fn catch_up_log(self: &Rc<Self>) {
        let (fresh, revision) = self.messages.log.get().since(self.messages.revision.get());
        self.messages.revision.set(revision);
        if fresh.is_empty() {
            return;
        }
        let open = self.messages.window.borrow().clone();
        let state = State { log_open: open.is_some() };
        for entry in &fresh {
            match surface(entry, &state) {
                Surface::Quiet => {}
                Surface::Toast => self.toast(&entry.line()),
                Surface::Notice => self.notice(entry),
            }
        }
        if let Some(open) = open {
            open.catch_up(&fresh);
        }
    }

    /// Say `entry` in the corner notice, building it the first time.
    fn notice(self: &Rc<Self>, entry: &Entry) {
        {
            let mut unseen = self.messages.unseen.borrow_mut();
            if !unseen.contains(&entry.id) {
                unseen.push(entry.id);
            }
        }
        let others = self.messages.unseen.borrow().len().saturating_sub(1);
        let built = self.messages.notice.borrow().clone();
        let notice = match built {
            Some(notice) => notice,
            None => {
                let Some(overlay) = self.trade.overlay.borrow().clone() else { return };
                let show = Rc::downgrade(self);
                let dismiss = Rc::downgrade(self);
                let notice = Notice::new(
                    move || {
                        if let Some(this) = show.upgrade() {
                            this.open_log();
                        }
                    },
                    move || {
                        if let Some(this) = dismiss.upgrade() {
                            this.dismiss_notice();
                        }
                    },
                );
                notice.revealer.set_margin_start(12);
                // Clear of the chartbook strip when it is showing, and of
                // the trading panel, which runs under the charts too.
                let strip = if self.book_strip.is_visible() { self.book_strip.height() } else { 0 };
                notice.revealer.set_margin_bottom(12 + strip + self.trading_clearance());
                overlay.add_overlay(&notice.revealer);
                *self.messages.notice.borrow_mut() = Some(notice.clone());
                notice
            }
        };
        notice.say(entry, others);
        self.place_update_notice();
    }

    /// How tall the error notice is, while there is one.
    pub(super) fn notice_height(&self) -> Option<i32> {
        let notice = self.messages.notice.borrow().clone()?;
        let (_, natural, _, _) = notice.revealer.measure(gtk::Orientation::Vertical, -1);
        Some(natural)
    }

    /// ✕ on the notice: gone, and the main menu wears a dot until the Log
    /// is opened.
    fn dismiss_notice(&self) {
        self.remove_notice();
        if let Some(button) = self.messages.menu_button.borrow().as_ref() {
            button.add_css_class(UNREAD);
        }
    }

    fn remove_notice(&self) {
        let notice = self.messages.notice.borrow_mut().take();
        if let (Some(notice), Some(overlay)) = (notice, self.trade.overlay.borrow().as_ref()) {
            overlay.remove_overlay(&notice.revealer);
        }
        self.place_update_notice();
    }

    /// The Log, opened from the main menu or the notice: the notice and the
    /// dot go, and the oldest error not yet seen is selected.
    /// Ctrl+Shift+M and the main menu's Log: open it, or close it if it is
    /// open.
    pub(super) fn toggle_log(self: &Rc<Self>) {
        let open = self.messages.window.borrow().clone();
        match open {
            Some(log) => log.window.close(),
            None => self.open_log(),
        }
    }

    pub(super) fn open_log(self: &Rc<Self>) {
        self.remove_notice();
        if let Some(button) = self.messages.menu_button.borrow().as_ref() {
            button.remove_css_class(UNREAD);
        }
        let first_unseen = self.messages.unseen.borrow().first().copied();
        self.messages.unseen.borrow_mut().clear();

        let open = self.messages.window.borrow().clone();
        let log = match open {
            Some(log) => log,
            None => {
                // Built from everything the log holds, so anything said since
                // the last wake is in the list rather than surfaced.
                self.messages.revision.set(self.messages.log.get().revision());
                let weak = Rc::downgrade(self);
                let log = LogWindow::new(self.messages.log.get(), move || {
                    if let Some(this) = weak.upgrade() {
                        this.messages.window.borrow_mut().take();
                    }
                });
                *self.messages.window.borrow_mut() = Some(log.clone());
                log
            }
        };
        log.window.present();
        if let Some(id) = first_unseen {
            log.select(id);
        }
    }

    #[cfg(test)]
    pub(super) fn log_window(&self) -> Option<Rc<LogWindow>> {
        self.messages.window.borrow().clone()
    }

    #[cfg(test)]
    pub(super) fn notice_shown(&self) -> Option<Rc<Notice>> {
        self.messages.notice.borrow().clone()
    }

    #[cfg(test)]
    pub(super) fn menu_unread(&self) -> bool {
        self.messages.menu_button.borrow().as_ref().is_some_and(|button| button.has_css_class(UNREAD))
    }

    /// A stream lost for `why`, or back when there is none.
    pub(super) fn say_of_stream(&self, series: &crate::live::Series, why: Option<omacharts_engine::FetchFailure>) {
        let Some(feed) = self.feed_of(&series.key) else { return };
        let feed = feed.provider.label();
        let symbol = series.key.split_once(':').map_or(series.key.as_str(), |(_, symbol)| symbol);
        self.say(match why {
            Some(why) => crate::log_words::stream_lost(feed, symbol, series.native, why),
            None => crate::log_words::stream_back(feed, symbol, series.native),
        });
    }

    /// Say a trading session's trouble once, when it starts.
    pub(super) fn say_session_trouble(&self) {
        let logins = self.trading.borrow().logins.clone();
        let mut troubled = self.messages.troubled.borrow_mut();
        for login in &logins {
            let why = self.desk.session(&login.id).and_then(|session| session.trouble());
            match why {
                Some(why) if troubled.get(&login.id) != Some(&why) => {
                    self.say(crate::log_words::session_ended(&login.name, &why));
                    troubled.insert(login.id.clone(), why);
                }
                Some(_) => {}
                None => {
                    troubled.remove(&login.id);
                }
            }
        }
    }
}

/// Which entries the filter shows: everything, or a level and above.
const FILTERS: [(&str, Level); 3] = [("All", Level::Info), ("Warnings", Level::Warning), ("Errors", Level::Error)];

/// The Log window.
pub(super) struct LogWindow {
    pub window: adw::Window,
    log: &'static Log,
    entries: gio::ListStore,
    shown: gtk::FilterListModel,
    selection: gtk::SingleSelection,
    list: gtk::ListView,
    scrolled: gtk::ScrolledWindow,
    /// Kept for the tests to choose from.
    #[cfg_attr(not(test), allow(dead_code))]
    filter: gtk::DropDown,
}

fn entry_of(object: &glib::Object) -> Option<std::cell::Ref<'_, Entry>> {
    object.downcast_ref::<glib::BoxedAnyObject>().map(|boxed| boxed.borrow::<Entry>())
}

impl LogWindow {
    fn new(log: &'static Log, closed: impl Fn() + 'static) -> Rc<LogWindow> {
        let entries = gio::ListStore::new::<glib::BoxedAnyObject>();
        for entry in log.entries() {
            entries.append(&glib::BoxedAnyObject::new(entry));
        }

        let least = Rc::new(Cell::new(Level::Info));
        let wanted = least.clone();
        let level_filter = gtk::CustomFilter::new(move |object| {
            entry_of(object).is_some_and(|entry| entry.level >= wanted.get())
        });
        let shown = gtk::FilterListModel::new(Some(entries.clone()), Some(level_filter.clone()));
        let selection = gtk::SingleSelection::new(Some(shown.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);

        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            item.set_child(Some(&Row::new().root));
        });
        factory.connect_bind(|_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let (Some(child), Some(object)) = (item.child(), item.item()) else { return };
            if let Some(entry) = entry_of(&object) {
                Row::bind(&child, &entry);
            }
        });
        let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
        list.add_css_class("log-list");
        let scrolled = gtk::ScrolledWindow::new();
        scrolled.set_hscrollbar_policy(gtk::PolicyType::Never);
        scrolled.set_vexpand(true);
        scrolled.set_child(Some(&list));

        let filter = gtk::DropDown::from_strings(&FILTERS.map(|(name, _)| name));
        {
            let level_filter = level_filter.clone();
            filter.connect_selected_notify(move |filter| {
                let level = FILTERS.get(filter.selected() as usize).map_or(Level::Info, |(_, level)| *level);
                least.set(level);
                level_filter.changed(gtk::FilterChange::Different);
            });
        }
        let copy = gtk::Button::with_label("Copy");
        let clear = gtk::Button::with_label("Clear");

        let header = adw::HeaderBar::new();
        header.pack_start(&filter);
        header.pack_end(&clear);
        header.pack_end(&copy);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scrolled));

        let window = adw::Window::new();
        window.set_title(Some("Log"));
        window.set_default_size(720, 480);
        window.set_content(Some(&toolbar));
        let escape = gtk::ShortcutController::new();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("Escape"),
            Some(gtk::NamedAction::new("window.close")),
        ));
        // The key that opened it closes it, from the Log itself too: the
        // main window's action is not reachable while the Log has the
        // keyboard.
        if let Some(accel) = crate::ui::shortcuts::accel("win.log") {
            escape.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(accel),
                Some(gtk::NamedAction::new("window.close")),
            ));
        }
        window.add_controller(escape);
        window.connect_close_request(move |_| {
            closed();
            glib::Propagation::Proceed
        });

        let this = Rc::new(LogWindow { window, log, entries, shown, selection, list, scrolled, filter });
        let weak = Rc::downgrade(&this);
        copy.connect_clicked(move |button| {
            if let Some(this) = weak.upgrade() {
                button.clipboard().set_text(&this.text());
            }
        });
        let weak = Rc::downgrade(&this);
        clear.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.clear();
            }
        });
        let weak = Rc::downgrade(&this);
        glib::idle_add_local_once(move || {
            if let Some(this) = weak.upgrade() {
                this.scroll_to_end();
            }
        });
        this
    }

    /// Put what the log says changed in the list: a repeat in place of the
    /// entry it repeats, anything new on the end. The list follows the end
    /// only when it was already there.
    fn catch_up(&self, fresh: &[Entry]) {
        let following = self.at_end();
        for entry in fresh {
            let count = self.entries.n_items();
            let last_id =
                count.checked_sub(1).and_then(|at| self.entries.item(at)).and_then(|o| entry_of(&o).map(|e| e.id));
            let object = glib::BoxedAnyObject::new(entry.clone());
            match last_id {
                Some(id) if id == entry.id => self.entries.splice(count - 1, 1, &[object]),
                _ => self.entries.append(&object),
            }
        }
        if following {
            self.scroll_to_end();
        }
    }

    fn at_end(&self) -> bool {
        let adjustment = self.scrolled.vadjustment();
        adjustment.value() + adjustment.page_size() >= adjustment.upper() - 4.0
    }

    fn scroll_to_end(&self) {
        if let Some(last) = self.shown.n_items().checked_sub(1) {
            let _ = self.list.activate_action("list.scroll-to-item", Some(&last.to_variant()));
        }
    }

    /// Select the entry `id`, and bring it into view.
    fn select(&self, id: u64) {
        let at = (0..self.shown.n_items())
            .find(|&at| self.shown.item(at).and_then(|o| entry_of(&o).map(|e| e.id)) == Some(id));
        if let Some(at) = at {
            self.selection.set_selected(at);
            let _ = self.list.activate_action("list.scroll-to-item", Some(&at.to_variant()));
        }
    }

    /// Forget everything said so far, here and in the log.
    fn clear(&self) {
        self.log.clear();
        self.entries.remove_all();
    }

    /// What the filter shows, a line each, details indented under it.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for at in 0..self.shown.n_items() {
            let Some(object) = self.shown.item(at) else { continue };
            let Some(entry) = entry_of(&object) else { continue };
            text.push_str(&format!("{} {} {}\n", entry.at.format("%H:%M:%S"), entry.level.word(), entry.line()));
            if let Some(detail) = &entry.detail {
                for line in detail.lines() {
                    text.push_str(&format!("    {line}\n"));
                }
            }
        }
        text
    }

    /// How many entries the list holds, whatever the filter.
    #[cfg(test)]
    pub fn len(&self) -> u32 {
        self.entries.n_items()
    }

    #[cfg(test)]
    pub fn shown(&self) -> u32 {
        self.shown.n_items()
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<u64> {
        let object = self.selection.selected_item()?;
        entry_of(&object).map(|entry| entry.id)
    }

    /// The list's own objects, to tell a rebuild from an append.
    #[cfg(test)]
    pub fn object(&self, at: u32) -> Option<glib::Object> {
        self.entries.item(at)
    }
}

/// One row of the list: the level's icon, the time, who said it and what,
/// and the details under an expander when there are any.
struct Row {
    root: gtk::Box,
}

impl Row {
    fn new() -> Row {
        let icon = gtk::Image::new();
        icon.set_valign(gtk::Align::Start);
        let time = gtk::Label::new(None);
        time.add_css_class("dim-label");
        time.add_css_class("numeric");
        time.set_valign(gtk::Align::Start);
        let words = gtk::Label::new(None);
        words.set_wrap(true);
        words.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        words.set_xalign(0.0);
        words.set_hexpand(true);
        words.set_selectable(false);
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        line.append(&icon);
        line.append(&time);
        line.append(&words);

        let detail = gtk::Label::new(None);
        detail.set_wrap(true);
        detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        detail.set_xalign(0.0);
        detail.set_selectable(true);
        detail.add_css_class("monospace");
        detail.add_css_class("dim-label");
        let details = gtk::Expander::new(Some("Details"));
        details.set_child(Some(&detail));
        details.set_margin_start(24);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 2);
        root.add_css_class("log-row");
        root.append(&line);
        root.append(&details);
        Row { root }
    }

    /// Fill a row built by [`Row::new`] with `entry`.
    fn bind(root: &gtk::Widget, entry: &Entry) {
        let Some(line) = root.first_child() else { return };
        let Some(details) = line.next_sibling().and_then(|w| w.downcast::<gtk::Expander>().ok()) else { return };
        let Some(icon) = line.first_child().and_then(|w| w.downcast::<gtk::Image>().ok()) else { return };
        let Some(time) = icon.next_sibling().and_then(|w| w.downcast::<gtk::Label>().ok()) else { return };
        let Some(words) = time.next_sibling().and_then(|w| w.downcast::<gtk::Label>().ok()) else { return };
        let (name, class) = match entry.level {
            Level::Info => ("dialog-information-symbolic", "success"),
            Level::Warning => ("dialog-warning-symbolic", "warning"),
            Level::Error => ("dialog-error-symbolic", "error"),
        };
        icon.set_icon_name(Some(name));
        for other in ["success", "warning", "error"] {
            icon.remove_css_class(other);
        }
        icon.add_css_class(class);
        icon.set_tooltip_text(Some(entry.level.word()));
        time.set_label(&entry.at.format("%H:%M:%S").to_string());
        words.set_label(&entry.line());
        details.set_visible(entry.detail.is_some());
        details.set_expanded(false);
        if let Some(detail) = details.child().and_then(|w| w.downcast::<gtk::Label>().ok()) {
            detail.set_label(entry.detail.as_deref().unwrap_or_default());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::Store;
    use crate::ui::window::trade::tests::window;

    /// The window's log, swapped for one of the test's own, so that nothing
    /// another test says reaches it.
    pub(crate) fn private(window: &Window) -> &'static Log {
        let log: &'static Log = Box::leak(Box::new(Log::new(omacharts_engine::log::CAPACITY)));
        window.messages.log.set(log);
        window.messages.revision.set(0);
        log
    }

    fn overlays(window: &Window) -> usize {
        let overlay = window.trade.overlay.borrow().clone().expect("an overlay");
        let mut count = 0;
        let mut child = overlay.first_child();
        while let Some(widget) = child {
            count += 1;
            child = widget.next_sibling();
        }
        count
    }

    /// Nothing until an error; then one notice, its words the newest
    /// error's and the rest counted, never a second card. ✕ takes it out of
    /// the window and puts a dot on the menu; opening the Log clears both
    /// and selects the oldest error not yet seen.
    #[test]
    fn an_error_is_noticed_once_and_the_rest_counted() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let log = private(&window);
            let bare = overlays(&window);

            window.say(Entry::info("Yahoo Finance", "AAPL 1m back"));
            window.say(Entry::warning("Yahoo Finance", "AAPL 1D · Rate limited by the data feed."));
            window.catch_up_log();
            assert!(window.notice_shown().is_none(), "a notice for a warning");
            assert_eq!(overlays(&window), bare, "built with nothing to say");

            window.say(Entry::error("Lucid", "Not flattened · nothing held"));
            window.catch_up_log();
            let notice = window.notice_shown().expect("a notice");
            assert_eq!(notice.text(), "Lucid · Not flattened · nothing held");
            assert_eq!(notice.more(), None);
            let first = log.entries().last().unwrap().id;

            window.say(Entry::error("Lucid", "Not moved · filled meanwhile"));
            window.say(Entry::error("Rithmic Test", "Signed out · forced logout"));
            window.say(Entry::error("Rithmic Test", "Signed out · forced logout"));
            window.catch_up_log();
            let notice = window.notice_shown().expect("a notice");
            assert_eq!(notice.text(), "Rithmic Test · Signed out · forced logout (×2)");
            assert_eq!(notice.more().as_deref(), Some("· 2 more"), "a repeat is not one more");
            assert_eq!(overlays(&window), bare + 1, "a second card");

            window.dismiss_notice();
            assert!(window.notice_shown().is_none());
            assert_eq!(overlays(&window), bare, "dismissed and still in the window");
            assert!(window.menu_unread(), "no dot on the menu");

            window.open_log();
            assert!(!window.menu_unread(), "the dot outlived opening the Log");
            assert!(window.notice_shown().is_none());
            let open = window.log_window().expect("the Log");
            assert_eq!(open.selected(), Some(first), "the oldest unseen error is selected");

            // With the Log open, an error is in it and nowhere else.
            let before = open.len();
            window.say(Entry::error("Lucid", "Not placed: price out of band"));
            window.catch_up_log();
            assert!(window.notice_shown().is_none(), "a notice over the open Log");
            assert_eq!(open.len(), before + 1);

            open.window.close();
            assert!(window.log_window().is_none(), "the Log kept once closed");
        });
    }

    /// The Log's key opens it, and closes it again.
    #[test]
    fn the_logs_key_shows_and_hides_it() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            assert!(window.log_window().is_none());
            gio::prelude::ActionGroupExt::activate_action(&window.window, "log", None);
            assert!(window.log_window().is_some(), "not opened");
            gio::prelude::ActionGroupExt::activate_action(&window.window, "log", None);
            assert!(window.log_window().is_none(), "not closed");
            gio::prelude::ActionGroupExt::activate_action(&window.window, "log", None);
            let open = window.log_window().expect("opened again");
            open.window.close();
        });
    }

    /// The open Log takes what is said onto the end of the list it has,
    /// counts a repeat in place, filters by level, copies what it shows with
    /// the details, and clears.
    #[test]
    fn the_log_appends_without_rebuilding_and_filters_by_level() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let log = private(&window);
            window.say(Entry::info("Lucid", "Buy Market 1 ESZ6"));
            window.say(Entry::warning("Yahoo Finance", "Lost ES 1m · The data feed is not answering."));
            window.say(Entry::error("Settings", "Settings not backed up").detail("disk I/O error (code 10)"));
            window.open_log();
            let open = window.log_window().expect("the Log");
            assert_eq!(open.len(), 3);
            let kept = open.object(0).expect("a row");

            window.say(Entry::info("Lucid", "Filled 1 ESZ6 at 7,612.25").confirming());
            window.catch_up_log();
            assert_eq!(open.len(), 4);
            assert_eq!(open.object(0).as_ref(), Some(&kept), "the list was rebuilt");
            window.say(Entry::info("Lucid", "Filled 1 ESZ6 at 7,612.25").confirming());
            window.catch_up_log();
            assert_eq!(open.len(), 4, "a repeat added a row");
            assert!(open.text().contains("(×2)"), "{}", open.text());

            open.filter.set_selected(1);
            assert_eq!(open.shown(), 2, "warnings and above");
            open.filter.set_selected(2);
            assert_eq!(open.shown(), 1, "errors alone");
            let copied = open.text();
            assert!(copied.contains("Error Settings · Settings not backed up\n    disk I/O error (code 10)\n"), "{copied}");
            assert!(!copied.contains("Buy Market"), "copied what the filter hides: {copied}");

            open.clear();
            assert_eq!(open.len(), 0);
            assert!(log.entries().is_empty());
            open.window.close();
        });
    }
}
