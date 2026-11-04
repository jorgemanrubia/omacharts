//! What every chart draws over its bars: the header across its top, and
//! the lines across it. One answer for all of them, in every chartbook,
//! rather than a switch per chart that only decides what the next chart
//! opens with.
//!
//! Built wherever it is wanted, as Appearance is: in Preferences, and
//! beside a chart's own settings.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib;

use crate::chart_defaults::{chart_switch, SETTING_PRICE_LINE, SETTING_SHOW_GRID};
use crate::chart_look::LookEdit;
use crate::header::{Header, Part, Section};
use crate::store::Store;
use crate::ui::part_list::PartList;
use crate::ui::scope_rule;
use crate::ui::window::{Window, SETTING_SYNC_CROSSHAIR};

/// A Lines switch on screen, the setting it is, and the window whose look
/// and store it reads.
type Switch = (glib::WeakRef<adw::SwitchRow>, &'static str, Rc<Window>, Rc<Store>);

thread_local! {
    /// Every Lines switch on screen, so a key or a command that changes a
    /// setting moves its switch with it.
    static SWITCHES: RefCell<Vec<Switch>> = const { RefCell::new(Vec::new()) };
    /// Every header editor on screen, so a command or a change of scope
    /// reaches it. More than one: Preferences and a chart's settings can
    /// both be open.
    static HEADERS: RefCell<Vec<Weak<HeaderLists>>> = const { RefCell::new(Vec::new()) };
}

/// The Overlays page. The header and the lines are the look, changed where
/// the rule says; `on_crosshair` puts the crosshair switch, which is every
/// chart's, on the charts once it is stored.
pub fn page(window: Rc<Window>, store: Rc<Store>, on_crosshair: Rc<dyn Fn()>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let header = HeaderLists::new(window.clone());
    for section in &header.sections {
        page.add(&section.group);
    }
    let restore_group = adw::PreferencesGroup::new();
    let restore = gtk::Button::with_label("Restore defaults");
    restore.set_halign(gtk::Align::Start);
    let restoring = header.clone();
    restore.connect_clicked(move |_| restoring.restore());
    restore_group.add(&restore);
    page.add(&restore_group);
    // Kept by the page's groups, which let go when the dialog is disposed.
    let kept = header.clone();
    restore_group.connect_destroy(move |_| {
        let _ = &kept;
    });
    page.add(&lines_group(window, store, on_crosshair));
    page
}

/// The header or the scope changed — a command, the rule over the sidebar,
/// the focused chart — so every header editor on screen shows the header in
/// scope now.
pub fn header_changed() {
    let live: Vec<Rc<HeaderLists>> = HEADERS.with(|headers| {
        let mut headers = headers.borrow_mut();
        headers.retain(|header| header.upgrade().is_some());
        headers.iter().filter_map(Weak::upgrade).collect()
    });
    for header in live {
        header.reload();
    }
}

/// The header's sections, a list each, changing the header in scope: every
/// chart's, or under Local the focused chart's.
///
/// A part is in the header once, so a part in one section is not offered by
/// the others' +, and is offered again the moment it is taken out. The
/// account, Buy and Sell, the quantity and the day's P&L are listed and
/// offered while trading is switched on, with or without a login, like every
/// trading setting: they draw nothing on a chart until there is an account
/// to trade on. Switched off, they are neither listed nor offered, and stay
/// where they were.
struct HeaderLists {
    window: Rc<Window>,
    sections: Vec<Rc<PartList<Part>>>,
}

impl HeaderLists {
    fn new(window: Rc<Window>) -> Rc<HeaderLists> {
        let header = scope_rule::look(&window).header;
        let this = Rc::new_cyclic(|me: &Weak<HeaderLists>| {
            let sections = Section::ALL
                .into_iter()
                .map(|section| {
                    let me = me.clone();
                    PartList::new(
                        section.title(),
                        &format!("Add to the {}", section.title().to_lowercase()),
                        header.shown(section, crate::features::trading()),
                        move |parts: Vec<Part>| {
                            let Some(me) = me.upgrade() else { return };
                            scope_rule::change(&me.window, LookEdit::Header(section, parts));
                            me.reload();
                        },
                    )
                })
                .collect();
            HeaderLists { window, sections }
        });
        this.offer(&header);
        HEADERS.with(|headers| headers.borrow_mut().push(Rc::downgrade(&this)));
        this
    }

    /// Each list offers what no other section holds.
    fn offer(&self, header: &Header) {
        for (section, list) in Section::ALL.into_iter().zip(&self.sections) {
            let elsewhere = Section::ALL
                .into_iter()
                .filter(|other| *other != section)
                .flat_map(|other| header.section(other).to_vec())
                .collect();
            list.set_elsewhere(elsewhere);
        }
    }

    /// Show the header in scope as it is now.
    fn reload(&self) {
        let header = scope_rule::look(&self.window).header;
        for (section, list) in Section::ALL.into_iter().zip(&self.sections) {
            let shown = header.shown(section, crate::features::trading());
            if list.parts() != shown {
                list.replace(shown);
            }
        }
        self.offer(&header);
    }

    /// Every section back as it ships, at once.
    fn restore(&self) {
        let standard = Header::standard();
        for section in Section::ALL {
            scope_rule::change(&self.window, LookEdit::Header(section, standard.section(section).to_vec()));
        }
        self.reload();
    }
}

/// A line changed from somewhere else — the key, the chart menu, a
/// command — so every switch on screen reads what is stored now.
pub fn charts_changed() {
    let live: Vec<(adw::SwitchRow, &'static str, Rc<Window>, Rc<Store>)> = SWITCHES.with(|switches| {
        let mut switches = switches.borrow_mut();
        switches.retain(|(row, ..)| row.upgrade().is_some());
        switches
            .iter()
            .filter_map(|(row, key, window, store)| Some((row.upgrade()?, *key, window.clone(), store.clone())))
            .collect()
    });
    for (row, key, window, store) in live {
        let on = line_on(&window, &store, key);
        // Only when it differs: setting it runs the handler, which stores it
        // and repaints the charts again.
        if row.is_active() != on {
            row.set_active(on);
        }
    }
}

/// Whether the line `key` is drawn: in the look in scope, or for the
/// crosshair, which is every chart's, in the settings.
fn line_on(window: &Window, store: &Store, key: &str) -> bool {
    match key {
        SETTING_SHOW_GRID => scope_rule::look(window).grid,
        SETTING_PRICE_LINE => scope_rule::look(window).price_line,
        _ => chart_switch(store, key),
    }
}

fn lines_group(window: Rc<Window>, store: Rc<Store>, on_crosshair: Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title("Lines");

    let rows = [
        ("Gridlines", SETTING_SHOW_GRID),
        ("Last price line", SETTING_PRICE_LINE),
        ("Crosshair across Follow charts", SETTING_SYNC_CROSSHAIR),
    ];
    for (title, key) in rows {
        let row = adw::SwitchRow::new();
        row.set_title(title);
        row.set_active(line_on(&window, &store, key));
        // Between charts by nature, so never one chart's own.
        if key == SETTING_SYNC_CROSSHAIR {
            row.set_widget_name(crate::ui::scope_rule::GLOBAL_ONLY);
        }
        let (window_for_row, store_for_row, on_crosshair) = (window.clone(), store.clone(), on_crosshair.clone());
        row.connect_active_notify(move |row| {
            let on = row.is_active();
            // Moved to what is already there, by a change of scope: not an
            // edit, and under Local one would hand the chart a copy.
            if on == line_on(&window_for_row, &store_for_row, key) {
                return;
            }
            match key {
                SETTING_SHOW_GRID => scope_rule::change(&window_for_row, LookEdit::Grid(on)),
                SETTING_PRICE_LINE => scope_rule::change(&window_for_row, LookEdit::PriceLine(on)),
                _ => {
                    store_for_row.set_setting_bool(key, on);
                    on_crosshair();
                }
            }
        });
        group.add(&row);
        SWITCHES.with(|switches| switches.borrow_mut().push((row.downgrade(), key, window.clone(), store.clone())));
    }
    group
}
