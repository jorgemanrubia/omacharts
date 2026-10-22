//! The editor for Session Times: its ranges, one group each, and a way to
//! add another.
//!
//! Every time on this page is written in the chart's time zone and the app's
//! time format, the same as the axis beneath it, so a time read off the chart
//! is the time typed in here. The one exception says so: a range that keeps a
//! market's own clock — London, at eight in London — shows its hours in that
//! clock and what they come to on the chart beside them.

use chrono_tz::Tz;
use omacharts_engine::clock::{format_minutes, zone_label, TimeFormat, ZONES};
use omacharts_engine::exchanges::{self, Part, Schedule};
use omacharts_engine::indicators::time::{MAX_ALPHA, MIN_ALPHA};
use omacharts_engine::indicators::{Hours, Preset, TimeRange};

use super::*;

/// The groups Session Times is edited with, top to bottom, on `page`.
///
/// Added to the page here rather than handed back, because adding or taking
/// away a range changes how many groups there are: they are rebuilt in place,
/// and only this knows which of the page's groups are its own.
pub(super) fn fill(owner: &Rc<Owner>, refresh: &Refresh, page: &adw::PreferencesPage, id: u32) {
    let groups: Rc<RefCell<Vec<glib::WeakRef<adw::PreferencesGroup>>>> = Rc::new(RefCell::new(Vec::new()));
    // The groups it builds are what keep it alive: their handlers hold it, to
    // rebuild again. Everything it holds in turn — itself, the page, the
    // groups — it holds weakly, so the panel closing lets the whole of it go.
    let rebuild: Rc<Box<dyn Fn()>> = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let page = page.downgrade();
        Rc::new_cyclic(move |me: &std::rc::Weak<Box<dyn Fn()>>| {
            let me = me.clone();
            Box::new(move || {
                // Held for the length of this call, since the groups about to
                // be dropped may be the last of its owners.
                let (Some(this), Some(page)) = (me.upgrade(), page.upgrade()) else { return };
                for group in groups.borrow_mut().drain(..).filter_map(|g| g.upgrade()) {
                    page.remove(&group);
                }
                let again: Rc<dyn Fn()> = Rc::new(move || this());
                for group in build(&owner, &refresh, id, again) {
                    page.add(&group);
                    groups.borrow_mut().push(group.downgrade());
                }
            }) as Box<dyn Fn()>
        })
    };
    rebuild();
}

/// The ranges as they are stored now.
fn stored(owner: &Owner, id: u32) -> Vec<TimeRange> {
    owner
        .indicators()
        .into_iter()
        .find(|i| i.id == id)
        .and_then(|i| match i.params {
            Params::SessionTimes { ranges } => Some(ranges),
            _ => None,
        })
        .unwrap_or_default()
}

/// Change the ranges and redraw.
fn change(owner: &Rc<Owner>, refresh: &Refresh, id: u32, edit: impl Fn(&mut Vec<TimeRange>)) {
    update(owner, id, |indicator| {
        if let Params::SessionTimes { ranges } = &mut indicator.params {
            edit(ranges);
        }
    });
    refresh.run();
}

/// Change one range, by its place in the list.
fn change_range(owner: &Rc<Owner>, refresh: &Refresh, id: u32, at: usize, edit: impl Fn(&mut TimeRange)) {
    change(owner, refresh, id, |ranges| {
        if let Some(range) = ranges.get_mut(at) {
            edit(range);
        }
    });
}

/// The app's clock, as the focused chart reads it. A template's editor has no
/// chart, so it reads the setting with no exchange to fall back on.
fn clock(owner: &Owner) -> (Tz, TimeFormat) {
    let store = owner.app.store();
    let instrument = match owner.template {
        None => owner.app.focused_market().map(|(instrument, _)| instrument),
        Some(_) => None,
    };
    (crate::clock::zone(store).for_instrument(instrument.as_ref()), crate::clock::format(store))
}

/// The exchange the focused chart's instrument trades on, and whether the
/// chart keeps regular hours only. Nothing for a template, which is no chart.
fn market(owner: &Owner) -> Option<(Schedule<'static>, Session)> {
    if owner.template.is_some() {
        return None;
    }
    let (instrument, session) = owner.app.focused_market()?;
    Some((exchanges::registry().for_instrument(&instrument)?, session))
}

fn build(owner: &Rc<Owner>, refresh: &Refresh, id: u32, rebuild: Rc<dyn Fn()>) -> Vec<adw::PreferencesGroup> {
    let ranges = stored(owner, id);
    let mut groups: Vec<adw::PreferencesGroup> = ranges
        .iter()
        .enumerate()
        .map(|(at, range)| range_group(owner, refresh, id, at, range, rebuild.clone()))
        .collect();
    groups.push(add_group(owner, refresh, id, &ranges, rebuild));
    // Last, as on every indicator's page; rebuilt with the rest so it stays
    // below the ranges however many there are.
    if let Some(indicator) = owner.indicators().into_iter().find(|i| i.id == id) {
        groups.push(legend_group(owner, refresh, &indicator));
    }
    groups
}

/// One range: what it covers, and how it is shaded.
fn range_group(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    at: usize,
    range: &TimeRange,
    rebuild: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let (chart_zone, format) = clock(owner);
    let group = adw::PreferencesGroup::new();
    group.set_title(match range.name.trim() {
        "" => "Range",
        name => name,
    });
    group.set_description(describe(owner, &range.hours, chart_zone, format).as_deref());

    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_valign(gtk::Align::Center);
    remove.set_tooltip_text(Some("Remove this range"));
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        remove.connect_clicked(move |_| {
            change(&owner, &refresh, id, |ranges| {
                if at < ranges.len() {
                    ranges.remove(at);
                }
            });
            let rebuild = rebuild.clone();
            glib::idle_add_local_once(move || rebuild());
        });
    }
    group.set_header_suffix(Some(&remove));

    if let Hours::Clock { from, to, zone } = &range.hours {
        let name = adw::EntryRow::new();
        name.set_title("Name");
        name.set_text(&range.name);
        {
            let owner = owner.clone();
            let refresh = refresh.clone();
            let group = group.downgrade();
            name.connect_changed(move |row| {
                let typed = row.text().to_string();
                change_range(&owner, &refresh, id, at, |range| range.name = typed.clone());
                if let Some(group) = group.upgrade() {
                    group.set_title(match typed.trim() {
                        "" => "Range",
                        name => name,
                    });
                }
            });
        }
        group.add(&name);

        for (title, minutes, start) in [("Starts", *from, true), ("Ends", *to, false)] {
            let owner = owner.clone();
            let refresh = refresh.clone();
            group.add(&controls::time_row(title, minutes, format, move |minutes| {
                change_range(&owner, &refresh, id, at, |range| {
                    if let Hours::Clock { from, to, .. } = &mut range.hours {
                        match start {
                            true => *from = minutes,
                            false => *to = minutes,
                        }
                    }
                });
            }));
        }
        group.add(&zone_row(owner, refresh, id, at, (*from, *to), zone.clone(), (chart_zone, format)));
    }

    // Unset shows as what it paints with: the ground lifted or darkened.
    let theme = owner.theme();
    let (shown, _) =
        omacharts_engine::indicators::time::shade(range.color.as_ref(), range.alpha, range.hours.ground(), &theme);
    group.add(&colour_row(
        owner,
        refresh,
        &Ink::new(String::new),
        "Colour",
        &shown,
        range.color.clone(),
        move |indicator, choice| {
            if let Params::SessionTimes { ranges } = &mut indicator.params
                && let Some(range) = ranges.get_mut(at)
            {
                range.color = choice;
            }
        },
        id,
    ));
    group.add(&percent_row(owner, refresh, id, "Shading %", range.alpha, MIN_ALPHA, MAX_ALPHA, move |indicator, share| {
        if let Params::SessionTimes { ranges } = &mut indicator.params
            && let Some(range) = ranges.get_mut(at)
        {
            range.alpha = share;
        }
    }));
    group
}

/// What a range covers on this chart, said under its name.
///
/// ETH and RTH are the exchange's, so they say whose and when — or why there
/// is nothing to shade, rather than leaving a range that draws nothing
/// looking broken.
fn describe(owner: &Owner, hours: &Hours, zone: Tz, format: TimeFormat) -> Option<String> {
    let regular = match hours {
        Hours::Clock { .. } => return None,
        Hours::Rth => true,
        Hours::Eth => false,
    };
    let what = match regular {
        true => "The regular session",
        false => "Pre-market, post-market and overnight",
    };
    Some(match market(owner) {
        None => format!("{what}, by each chart's exchange"),
        Some((schedule, _)) if !schedule.has_regular() => {
            "Trades around the clock, so there are no extended hours to tell apart".to_string()
        }
        Some((_, Session::Regular)) if !regular => {
            "This chart shows regular hours only, so there is nothing to shade".to_string()
        }
        Some((schedule, _)) => format!(
            "{what}, as {} keeps them: {}",
            schedule.exchange.name,
            describe_day(schedule, zone, format)
        ),
    })
}

/// One trading day of this exchange, in the chart's clock: "trading
/// 18:00–17:00, regular 09:30–16:00, New York time".
fn describe_day(schedule: Schedule, zone: Tz, format: TimeFormat) -> String {
    // A weekday a little ahead, so the hours are the ones in force now rather
    // than across a clock change months ago.
    let mut date = chrono::Utc::now().date_naive();
    let day = loop {
        if let Some(day) = schedule.day(date) {
            break day;
        }
        date = date.succ_opt().unwrap_or(date);
    };
    let at = |ts: i64| exchanges::wall(zone, ts).map(|(_, m)| format_minutes(m, format)).unwrap_or_default();
    let place = format!("{} time", zone_label(zone));
    let regular: Vec<&(Part, i64, i64)> = day.segments.iter().filter(|(p, ..)| *p == Part::Regular).collect();
    let (Some(open), Some(close)) = (regular.first().map(|s| s.1), regular.last().map(|s| s.2)) else {
        return place;
    };
    let first = day.segments.first().map(|s| s.1).unwrap_or(open);
    let last = day.segments.last().map(|s| s.2).unwrap_or(close);
    match first < open || last > close {
        // The whole day, then the part of it that is regular: two ranges a
        // person can check against the axis.
        true => format!("trading {}–{}, regular {}–{}, {place}", at(first), at(last), at(open), at(close)),
        false => format!("regular {}–{}, {place}", at(open), at(close)),
    }
}

/// The clock a range of wall-clock hours keeps.
fn zone_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    at: usize,
    (from, to): (u32, u32),
    zone: Option<String>,
    (chart_zone, format): (Tz, TimeFormat),
) -> adw::ComboRow {
    // The chart's own clock first, then the markets' in the order they open;
    // a zone set from the command line that is not among them is offered
    // too, so the row can show it.
    let mut zones: Vec<Option<String>> =
        std::iter::once(None).chain(ZONES.iter().map(|z| Some(z.to_string()))).collect();
    if !zones.contains(&zone) {
        zones.push(zone.clone());
    }
    let labels: Vec<&str> = zones
        .iter()
        .map(|z| match z.as_deref().and_then(|z| z.parse::<Tz>().ok()) {
            None => "Chart's time zone",
            Some(tz) => tz.name().rsplit('/').next().unwrap_or("UTC"),
        })
        .collect();
    let labels: Vec<String> = labels.iter().map(|l| l.replace('_', " ")).collect();
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let row = adw::ComboRow::new();
    row.set_title("Time zone");
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(zones.iter().position(|z| *z == zone).unwrap_or(0) as u32);
    row.set_enable_search(true);
    let own: Option<Tz> = zone.as_deref().and_then(|z| z.parse().ok());
    row.set_subtitle(&conversion(from, to, own.unwrap_or(chart_zone), chart_zone, own.is_none(), format));
    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_selected_notify(move |row| {
        let Some(chosen) = zones.get(row.selected() as usize).cloned() else { return };
        change_range(&owner, &refresh, id, at, |range| {
            if let Hours::Clock { zone, .. } = &mut range.hours {
                *zone = chosen.clone();
            }
        });
        let own: Option<Tz> = chosen.as_deref().and_then(|z| z.parse().ok());
        let (from, to) = stored(&owner, id)
            .get(at)
            .and_then(|range| match range.hours {
                Hours::Clock { from, to, .. } => Some((from, to)),
                _ => None,
            })
            .unwrap_or((from, to));
        row.set_subtitle(&conversion(from, to, own.unwrap_or(chart_zone), chart_zone, own.is_none(), format));
    });
    row
}

/// What a range's hours come to on the chart: "Times are in the chart's time
/// zone, New York", or "03:00–11:30 New York time today" for London's eight
/// to half past four — today, because the two move apart for a few weeks a
/// year when their clocks change on different Sundays.
fn conversion(from: u32, to: u32, zone: Tz, chart: Tz, follows: bool, format: TimeFormat) -> String {
    if follows || zone == chart {
        return format!("Times are in the chart's time zone, {}", zone_label(chart));
    }
    let today = chrono::Utc::now().date_naive();
    let on_chart = |minutes: u32| {
        let ts = exchanges::instant(zone, today, minutes);
        exchanges::wall(chart, ts).map(|(_, m)| format_minutes(m, format)).unwrap_or_default()
    };
    format!("{}–{} {} time today", on_chart(from), on_chart(to), zone_label(chart))
}

/// The way to another range: a menu of the ready-made ones and Custom.
fn add_group(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    ranges: &[TimeRange],
    rebuild: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    if ranges.is_empty() {
        group.set_description(Some("No ranges yet: add one to shade it behind the bars."));
    }
    let row = adw::ActionRow::new();
    row.set_title("Add a range");

    let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let popover = gtk::Popover::new();
    popover.set_child(Some(&menu));
    for preset in Preset::ALL {
        let item = gtk::Button::with_label(preset.label());
        item.add_css_class("flat");
        // ETH twice is the same bars shaded twice, which only reads as the
        // shading being too dark.
        if preset.is_among(ranges) {
            item.set_sensitive(false);
            item.set_tooltip_text(Some("Already shaded"));
        }
        if let Some(label) = item.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
        }
        let owner = owner.clone();
        let refresh = refresh.clone();
        let rebuild = rebuild.clone();
        let popover = popover.downgrade();
        item.connect_clicked(move |_| {
            change(&owner, &refresh, id, |ranges| ranges.push(preset.range()));
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
            }
            let rebuild = rebuild.clone();
            glib::idle_add_local_once(move || rebuild());
        });
        menu.append(&item);
    }

    let button = gtk::MenuButton::new();
    button.set_icon_name("list-add-symbolic");
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Add a range"));
    button.set_popover(Some(&popover));
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    group.add(&row);
    group
}

#[cfg(test)]
mod tests {
    use chrono_tz::America::New_York;
    use chrono_tz::Europe::London;

    use super::*;

    #[test]
    fn a_range_in_the_charts_clock_says_so() {
        let said = conversion(8 * 60, 16 * 60 + 30, New_York, New_York, true, TimeFormat::H24);
        assert_eq!(said, "Times are in the chart's time zone, New York");
    }

    #[test]
    fn a_range_in_its_own_clock_says_what_it_comes_to() {
        let said = conversion(8 * 60, 16 * 60 + 30, London, New_York, false, TimeFormat::H24);
        assert!(said.ends_with("New York time today"), "{said}");
        // Four or five hours behind, depending on the week.
        assert!(said.starts_with("03:00–11:30") || said.starts_with("04:00–12:30"), "{said}");
    }
}
