//! What every chart draws over its bars: the header across its top, and
//! the lines across it. One answer for all of them, in every chartbook,
//! rather than a switch per chart that only decides what the next chart
//! opens with.
//!
//! Built wherever it is wanted, as Appearance is: in Preferences, and
//! beside a chart's own settings.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::chart_defaults::{chart_switch, SETTING_PRICE_LINE, SETTING_SHOW_GRID};
use crate::store::Store;
use crate::ui::window::{Window, SETTING_SYNC_CROSSHAIR};

/// An Overlays switch on screen, the setting it is, and where that is stored.
type Switch = (glib::WeakRef<adw::SwitchRow>, &'static str, Rc<Store>);

thread_local! {
    /// Every Overlays switch on screen, so a key or a command that changes a
    /// setting moves its switch with it.
    static SWITCHES: RefCell<Vec<Switch>> = const { RefCell::new(Vec::new()) };
}

/// The Overlays page: the header's sections, the way back to the header as
/// it ships, and the lines. A change to the header is put on every chart at
/// once, and so is a line.
pub fn page(window: Rc<Window>, store: Rc<Store>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let store_for_header = store.clone();
    let window_for_header = window.clone();
    // The account, Buy and Sell, the quantity and the day's P&L are
    // listed and offered while trading is switched on, with or without
    // a login, like every trading setting: they draw nothing on a chart
    // until there is an account to trade on. Switched off, they are
    // neither listed nor offered, and stay where they were.
    let (sections, restore_header) = header_sections(crate::header::load(&store), move |header| {
        crate::header::save(&store_for_header, header);
        window_for_header.apply_header();
    });
    for section in &sections {
        page.add(&section.group);
    }
    let restore_group = adw::PreferencesGroup::new();
    let restore = gtk::Button::with_label("Restore defaults");
    restore.set_halign(gtk::Align::Start);
    restore.connect_clicked(move |_| restore_header());
    restore_group.add(&restore);
    page.add(&restore_group);
    page.add(&lines_group(store, Rc::new(move || window.adopt_chart_look())));
    page
}

/// A line changed from somewhere else — the key, the chart menu,
/// a command — so every switch on screen reads what is stored now.
pub fn charts_changed() {
    let live: Vec<(adw::SwitchRow, &'static str, Rc<Store>)> = SWITCHES.with(|switches| {
        let mut switches = switches.borrow_mut();
        switches.retain(|(row, ..)| row.upgrade().is_some());
        switches.iter().filter_map(|(row, key, store)| Some((row.upgrade()?, *key, store.clone()))).collect()
    });
    for (row, key, store) in live {
        let on = chart_switch(&store, key);
        // Only when it differs: setting it runs the handler, which stores it
        // and repaints the charts again.
        if row.is_active() != on {
            row.set_active(on);
        }
    }
}

/// The lines every chart draws, or does not, stored as they change and put
/// on the charts with `on_charts`.
fn lines_group(store: Rc<Store>, on_charts: Rc<dyn Fn()>) -> adw::PreferencesGroup {
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
        row.set_active(chart_switch(&store, key));
        let (store_for_row, on_charts) = (store.clone(), on_charts.clone());
        row.connect_active_notify(move |row| {
            store_for_row.set_setting_bool(key, row.is_active());
            on_charts();
        });
        group.add(&row);
        SWITCHES.with(|switches| switches.borrow_mut().push((row.downgrade(), key, store.clone())));
    }
    group
}

/// The header's sections, a list each, handing every change to `changed` as
/// the whole header.
///
/// A part is in the header once, so a part in one section is not offered by
/// the other's +, and is offered again the moment it is taken out.
///
/// Comes with the way back to the standard header, every section at once.
#[allow(clippy::type_complexity)]
fn header_sections(
    current: crate::header::Header,
    changed: impl Fn(&crate::header::Header) + 'static,
) -> (Vec<Rc<crate::ui::part_list::PartList<crate::header::Part>>>, Rc<dyn Fn()>) {
    use crate::header::Section;
    use crate::ui::part_list::PartList;
    type Lists = Rc<RefCell<Vec<(Section, std::rc::Weak<PartList<crate::header::Part>>)>>>;
    let header = Rc::new(RefCell::new(current));
    let changed = Rc::new(changed);
    let lists: Lists = Rc::new(RefCell::new(Vec::new()));
    let elsewhere = Rc::new(move |header: &crate::header::Header, section: Section| -> Vec<crate::header::Part> {
        Section::ALL
            .into_iter()
            .filter(|other| *other != section)
            .flat_map(|other| header.section(other).to_vec())
            .collect()
    });
    let built: Vec<_> = Section::ALL
        .into_iter()
        .map(|section| {
            let (edited, changed, others) = (header.clone(), changed.clone(), lists.clone());
            let elsewhere_now = elsewhere.clone();
            // Trading's parts while it is switched off are neither listed
            // nor lost: they stay where they were for when it is on.
            let current = header.borrow().shown(section, crate::features::trading());
            let list = PartList::new(
                section.title(),
                &format!("Add to the {}", section.title().to_lowercase()),
                current,
                move |parts: Vec<crate::header::Part>| {
                    edited.borrow_mut().set_shown(section, parts, crate::features::trading());
                    let now = edited.borrow().clone();
                    for (other, list) in others.borrow().iter().filter(|(other, _)| *other != section) {
                        if let Some(list) = list.upgrade() {
                            list.set_elsewhere(elsewhere_now(&now, *other));
                        }
                    }
                    changed(&now);
                },
            );
            list.set_elsewhere(elsewhere(&header.borrow(), section));
            lists.borrow_mut().push((section, Rc::downgrade(&list)));
            list
        })
        .collect();
    // Kept by their groups, which let go when the dialog is disposed: a
    // section with no rows and nothing to offer holds nothing that would.
    for list in &built {
        let kept = built.clone();
        list.group.connect_destroy(move |_| {
            let _ = &kept;
        });
    }
    let shown: Vec<_> = built.iter().map(Rc::downgrade).collect();
    let restore = Rc::new(move || {
        let standard = crate::header::Header::standard();
        *header.borrow_mut() = standard.clone();
        for (section, list) in Section::ALL.into_iter().zip(&shown) {
            if let Some(list) = list.upgrade() {
                list.replace(standard.shown(section, crate::features::trading()));
                list.set_elsewhere(elsewhere(&standard, section));
            }
        }
        changed(&standard);
    });
    (built, restore)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part in one section is not offered by the others, and is offered
    /// again once it is taken out; every change is the whole header.
    #[test]
    fn a_part_in_one_section_is_not_offered_in_the_others() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            crate::features::force(crate::features::Feature::Trading, true);
            let saved: Rc<RefCell<Vec<Header>>> = Rc::default();
            let seen = saved.clone();
            let (sections, _) =
                header_sections(Header::standard(), move |header| seen.borrow_mut().push(header.clone()));
            let [left, center, right] = sections.as_slice() else { panic!("three sections") };
            assert_eq!(left.group.title(), "Left");
            assert_eq!(center.group.title(), "Center");
            assert_eq!(right.group.title(), "Right");
            assert_eq!(left.parts(), vec![Part::Feed, Part::Symbol, Part::Resolution, Part::Change]);
            assert_eq!(center.parts(), vec![Part::Title]);
            assert_eq!(right.parts(), vec![Part::Account, Part::DayPnl, Part::Trade, Part::Size, Part::Position, Part::Settings, Part::Maximize, Part::Link]);
            assert_eq!(left.offered(), vec![Part::Template, Part::Stripe]);
            assert_eq!(center.offered(), vec![Part::Template, Part::Stripe]);
            assert_eq!(right.offered(), vec![Part::Template, Part::Stripe]);

            // Out of the center, the title is offered by all three.
            center.set(Vec::new());
            assert_eq!(left.offered(), vec![Part::Template, Part::Title, Part::Stripe]);
            assert_eq!(center.offered(), vec![Part::Template, Part::Title, Part::Stripe]);
            assert_eq!(right.offered(), vec![Part::Template, Part::Title, Part::Stripe]);
            // The stripe into the left is offered by none.
            left.set(vec![Part::Symbol, Part::Stripe]);
            assert_eq!(
                center.offered(),
                vec![Part::Resolution, Part::Change, Part::Template, Part::Title, Part::Feed]
            );
            assert!(!left.offered().contains(&Part::Stripe));
            assert!(!right.offered().contains(&Part::Stripe));
            assert_eq!(
                saved.borrow().last(),
                Some(&Header {
                    left: vec![Part::Symbol, Part::Stripe],
                    center: Vec::new(),
                    right: vec![Part::Account, Part::DayPnl, Part::Trade, Part::Size, Part::Position, Part::Settings, Part::Maximize, Part::Link]
                })
            );
            // The corner's controls move like any other part: the gear out
            // of the right is offered by the left, and put there is offered
            // by neither until it is taken out again.
            right.set(vec![Part::Maximize, Part::Link]);
            assert!(left.offered().contains(&Part::Settings));
            left.set(vec![Part::Symbol, Part::Settings]);
            assert!(!right.offered().contains(&Part::Settings));
            assert_eq!(saved.borrow().last().map(|header| header.right.clone()), Some(vec![Part::Maximize, Part::Link]));
            left.set(vec![Part::Symbol]);
            assert!(right.offered().contains(&Part::Settings));
        });
    }

    /// Restoring puts every section back as it ships, at once, lists and
    /// offers included, and hands the whole header on once.
    #[test]
    fn restoring_the_header_puts_every_section_back() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            crate::features::force(crate::features::Feature::Trading, true);
            let saved: Rc<RefCell<Vec<Header>>> = Rc::default();
            let seen = saved.clone();
            let moved = Header {
                left: vec![Part::Change, Part::Settings, Part::Size],
                center: Vec::new(),
                right: vec![Part::Symbol, Part::Stripe],
            };
            let (sections, restore) =
                header_sections(moved, move |header| seen.borrow_mut().push(header.clone()));
            let [left, center, right] = sections.as_slice() else { panic!("three sections") };
            restore();
            assert_eq!(left.parts(), vec![Part::Feed, Part::Symbol, Part::Resolution, Part::Change]);
            assert_eq!(center.parts(), vec![Part::Title]);
            assert_eq!(right.parts(), vec![Part::Account, Part::DayPnl, Part::Trade, Part::Size, Part::Position, Part::Settings, Part::Maximize, Part::Link]);
            assert_eq!(left.offered(), vec![Part::Template, Part::Stripe]);
            assert_eq!(right.offered(), vec![Part::Template, Part::Stripe]);
            assert_eq!(saved.borrow().as_slice(), &[Header::standard()]);
            // And the lists go on from there.
            center.set(Vec::new());
            assert_eq!(saved.borrow().last().map(|header| header.left.clone()), Some(Header::standard().left));
        });
    }
}
