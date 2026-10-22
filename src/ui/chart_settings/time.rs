//! The editors for the indicators of time: Sessions, Time Window and Session
//! Levels.
//!
//! Every time on these pages is written in the chart's time zone and the
//! app's time format, the same as the axis beneath them, so a time read off
//! the chart is the time typed in here. The one exception says so: a window
//! that keeps a market's own clock — London's open, at eight in London — shows
//! its hours in that clock and what they come to on the chart beside them.

use chrono_tz::Tz;
use omacharts_engine::clock::{format_minutes, zone_label, TimeFormat, ZONES};
use omacharts_engine::exchanges::{self, Part, Schedule};
use omacharts_engine::indicators::time::{MAX_SHADE_ALPHA, MIN_SHADE_ALPHA};
use omacharts_engine::indicators::{Level, LevelSource, Preset, Separators, Window, PRESETS};

use super::*;

/// The groups an indicator of time is edited with, top to bottom.
pub(super) fn groups(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    match &indicator.params {
        Params::Sessions { .. } => sessions(owner, refresh, colour, indicator),
        Params::TimeWindow { .. } => window(owner, refresh, colour, indicator),
        Params::SessionLevels { .. } => levels(owner, refresh, colour, indicator),
        _ => Vec::new(),
    }
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

/// A row with a switch, which is how every on-or-off here is asked.
fn switch_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    title: &str,
    subtitle: Option<&str>,
    on: bool,
    apply: impl Fn(&mut Indicator, bool) + 'static,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::new();
    row.set_title(title);
    if let Some(subtitle) = subtitle {
        row.set_subtitle(subtitle);
    }
    row.set_active(on);
    let owner = owner.clone();
    let refresh = refresh.clone();
    row.connect_active_notify(move |row| {
        let on = row.is_active();
        update(&owner, id, |indicator| apply(indicator, on));
        refresh.run();
    });
    row
}

/// A choice from a short list of labels.
fn combo_row(title: &str, labels: &[String], selected: usize) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title(title);
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    row.set_model(Some(&gtk::StringList::new(&names)));
    row.set_selected(selected as u32);
    row
}

/// The indicator's own line — colour, weight, pattern — as rows.
fn line_rows(owner: &Rc<Owner>, refresh: &Refresh, colour: &str, indicator: &Indicator) -> Vec<gtk::Widget> {
    let id = indicator.id;
    let ink = {
        let owner = owner.clone();
        Ink::new(move || drawn_colour(&owner, &owner.indicators(), id))
    };
    let mut rows: Vec<gtk::Widget> = vec![colour_row(
        owner,
        refresh,
        &ink,
        "Colour",
        colour,
        indicator.color.clone(),
        move |indicator, choice| indicator.color = choice,
        id,
    )
    .upcast()];
    rows.extend(
        stroke_rows(owner, refresh, &ink, id, "", indicator.stroke, {
            move |indicator: &mut Indicator, stroke: Stroke| indicator.stroke = stroke
        })
        .into_iter()
        .map(|row| row.upcast()),
    );
    rows
}

/// Those rows in a group of their own, titled for what the line is on this
/// indicator.
fn line_group(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
    title: &str,
    description: Option<&str>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    group.set_title(title);
    group.set_description(description);
    for row in line_rows(owner, refresh, colour, indicator) {
        group.add(&row);
    }
    group
}

/// How much of the colour a shading gets, over the range the engine allows.
fn shading_row(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    id: u32,
    alpha: f64,
    apply: impl Fn(&mut Indicator, f64) + 'static,
) -> adw::ActionRow {
    percent_row(owner, refresh, id, "Shading %", alpha, MIN_SHADE_ALPHA, MAX_SHADE_ALPHA, apply)
}

// -- Sessions -----------------------------------------------------------------

fn sessions(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    let id = indicator.id;
    let Params::Sessions { shade, shade_color, shade_alpha, separators } = &indicator.params else {
        return Vec::new();
    };
    let mut groups = Vec::new();
    let (zone, format) = clock(owner);
    let market = market(owner);

    // What the shading and the lines will follow on this chart, said before
    // anything is asked about them.
    if let Some((schedule, _)) = market {
        let group = adw::PreferencesGroup::new();
        let row = adw::ActionRow::new();
        row.set_title(&schedule.exchange.name);
        row.set_subtitle(&describe_day(schedule, zone, format));
        row.add_css_class("property");
        group.add(&row);
        groups.push(group);
    }

    let extended = adw::PreferencesGroup::new();
    extended.set_title("Extended hours");
    // A switch that does nothing on this chart says why rather than sitting
    // there looking broken.
    let why_not = match market {
        Some((schedule, _)) if !schedule.has_regular() => Some("Trades around the clock, so there is nothing to shade"),
        Some((_, Session::Regular)) => Some("This chart shows regular hours only"),
        _ => None,
    };
    let shaded = switch_row(
        owner,
        refresh,
        id,
        "Shaded",
        Some(why_not.unwrap_or("Pre-market, post-market and overnight")),
        *shade,
        |indicator, on| {
            if let Params::Sessions { shade, .. } = &mut indicator.params {
                *shade = on;
            }
        },
    );
    extended.add(&shaded);
    let alpha = shading_row(owner, refresh, id, *shade_alpha, |indicator, share| {
        if let Params::Sessions { shade_alpha, .. } = &mut indicator.params {
            *shade_alpha = share;
        }
    });
    shaded.bind_property("active", &alpha, "sensitive").sync_create().build();
    extended.add(&alpha);
    let shown = shade_color
        .as_ref()
        .map(|choice| choice.resolve(&owner.theme()))
        .unwrap_or_else(|| owner.theme().ui.text_muted.clone());
    let fill = colour_row(
        owner,
        refresh,
        &Ink::new(String::new),
        "Colour",
        &shown,
        shade_color.clone(),
        |indicator, choice| {
            if let Params::Sessions { shade_color, .. } = &mut indicator.params {
                *shade_color = choice;
            }
        },
        id,
    );
    shaded.bind_property("active", &fill, "sensitive").sync_create().build();
    extended.add(&fill);
    groups.push(extended);

    // Which lines first, then what they look like.
    let lines = adw::PreferencesGroup::new();
    lines.set_title("Separators");
    let labels: Vec<String> = Separators::ALL.iter().map(|s| s.label().to_string()).collect();
    let every = combo_row(
        "Every",
        &labels,
        Separators::ALL.iter().position(|s| s == separators).unwrap_or(0),
    );
    every.set_subtitle("Promoted automatically when the chart is too coarse for it.");
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        every.connect_selected_notify(move |row| {
            let Some(chosen) = Separators::ALL.get(row.selected() as usize).copied() else { return };
            update(&owner, id, |indicator| {
                if let Params::Sessions { separators, .. } = &mut indicator.params {
                    *separators = chosen;
                }
            });
            refresh.run();
        });
    }
    lines.add(&every);
    for row in line_rows(owner, refresh, colour, indicator) {
        lines.add(&row);
    }
    groups.push(lines);
    groups
}

/// One trading day of this exchange, in the chart's clock: "Trading
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
    if !schedule.has_regular() {
        return format!("Trades around the clock; each day starts at {}, {place}", at(day.start));
    }
    let regular: Vec<&(Part, i64, i64)> = day.segments.iter().filter(|(p, ..)| *p == Part::Regular).collect();
    let (Some(open), Some(close)) = (regular.first().map(|s| s.1), regular.last().map(|s| s.2)) else {
        return place;
    };
    let first = day.segments.first().map(|s| s.1).unwrap_or(open);
    let last = day.segments.last().map(|s| s.2).unwrap_or(close);
    match first < open || last > close {
        // The whole day, then the part of it that is regular: two ranges a
        // person can check against the axis, rather than the extended hours
        // spelled out in pieces either side of the session.
        true => format!(
            "Trading {}–{}, regular {}–{}, {place}",
            at(first),
            at(last),
            at(open),
            at(close)
        ),
        false => format!("Regular {}–{}, {place}", at(open), at(close)),
    }
}

// -- Time Window --------------------------------------------------------------

fn window(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    let Params::TimeWindow { shade, shade_alpha, mark, range, .. } = &indicator.params else {
        return Vec::new();
    };
    let id = indicator.id;

    // Rebuilt in place whenever a choice changes which rows it has — a
    // preset, clock times against a session's open — rather than keeping
    // two sets of rows and hiding one. Every row then shows what is stored.
    //
    // The rows it builds are what keep it alive: their handlers hold it, to
    // rebuild again. Everything it holds in turn — itself, the group, the
    // rows — it holds weakly, so the panel closing lets the whole of it go.
    let hours = adw::PreferencesGroup::new();
    hours.set_title("Window");
    let rows: Rc<RefCell<Vec<glib::WeakRef<gtk::Widget>>>> = Rc::new(RefCell::new(Vec::new()));
    let rebuild: Rc<Box<dyn Fn()>> = {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let group = hours.downgrade();
        Rc::new_cyclic(move |me: &std::rc::Weak<Box<dyn Fn()>>| {
            let me = me.clone();
            Box::new(move || {
                // Held for the length of this call, since the rows about to
                // be dropped may be the last of its owners.
                let (Some(this), Some(group)) = (me.upgrade(), group.upgrade()) else { return };
                for row in rows.borrow_mut().drain(..).filter_map(|row| row.upgrade()) {
                    group.remove(&row);
                }
                let again: Rc<dyn Fn()> = Rc::new(move || this());
                for row in window_rows(&owner, &refresh, id, again) {
                    group.add(&row);
                    rows.borrow_mut().push(row.downgrade());
                }
            }) as Box<dyn Fn()>
        })
    };
    rebuild();

    let show = adw::PreferencesGroup::new();
    show.set_title("Show");
    let shaded = switch_row(owner, refresh, id, "Shaded", None, *shade, |indicator, on| {
        if let Params::TimeWindow { shade, .. } = &mut indicator.params {
            *shade = on;
        }
    });
    show.add(&shaded);
    let alpha = shading_row(owner, refresh, id, *shade_alpha, |indicator, share| {
        if let Params::TimeWindow { shade_alpha, .. } = &mut indicator.params {
            *shade_alpha = share;
        }
    });
    shaded.bind_property("active", &alpha, "sensitive").sync_create().build();
    show.add(&alpha);
    show.add(&switch_row(owner, refresh, id, "Line at the start", None, *mark, |indicator, on| {
        if let Params::TimeWindow { mark, .. } = &mut indicator.params {
            *mark = on;
        }
    }));
    show.add(&switch_row(
        owner,
        refresh,
        id,
        "High and low",
        Some("Held until the window comes round again"),
        *range,
        |indicator, on| {
            if let Params::TimeWindow { range, .. } = &mut indicator.params {
                *range = on;
            }
        },
    ));

    let lines = line_group(
        owner,
        refresh,
        colour,
        indicator,
        "Lines",
        Some("The shading, the line at the start, and the high and low"),
    );
    vec![hours, show, lines]
}

/// The current window, read back off the chart: what the rows show.
fn stored_window(owner: &Owner, id: u32) -> Option<(String, Window, Option<String>, bool)> {
    owner.indicators().into_iter().find(|i| i.id == id).and_then(|i| match i.params {
        Params::TimeWindow { name, window, zone, weekdays, .. } => Some((name, window, zone, weekdays)),
        _ => None,
    })
}

fn set_window(owner: &Rc<Owner>, id: u32, change: impl Fn(&mut String, &mut Window, &mut Option<String>)) {
    update(owner, id, |indicator| {
        if let Params::TimeWindow { name, window, zone, .. } = &mut indicator.params {
            change(name, window, zone);
        }
    });
}

/// The rows of the Window group, for what is stored now.
fn window_rows(owner: &Rc<Owner>, refresh: &Refresh, id: u32, rebuild: Rc<dyn Fn()>) -> Vec<gtk::Widget> {
    let Some((name, window, zone, weekdays)) = stored_window(owner, id) else { return Vec::new() };
    let (chart_zone, format) = clock(owner);
    let mut rows: Vec<gtk::Widget> = Vec::new();

    let name_row = adw::EntryRow::new();
    name_row.set_title("Name");
    name_row.set_text(&name);
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        name_row.connect_changed(move |row| {
            let typed = row.text().to_string();
            set_window(&owner, id, |name, _, _| *name = typed.clone());
            retitle(row, &typed);
            refresh.run();
        });
    }
    rows.push(name_row.upcast());

    // Shown as chosen while the settings are a preset's, and as Custom once
    // they are not: a preset is a way to fill the rows in, not a mode the
    // window is in.
    let mut labels: Vec<String> = PRESETS.iter().map(|p| p.name.to_string()).collect();
    labels.push("Custom".to_string());
    let matching = Preset::matching(&window, zone.as_deref())
        .and_then(|found| PRESETS.iter().position(|p| p == found))
        .unwrap_or(PRESETS.len());
    let preset = combo_row("Preset", &labels, matching);
    // Set while the row is put back in step with an edit elsewhere, which is
    // the row agreeing with the settings rather than somebody choosing.
    let syncing = Rc::new(std::cell::Cell::new(false));
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let rebuild = rebuild.clone();
        let syncing = syncing.clone();
        preset.connect_selected_notify(move |row| {
            if syncing.get() {
                return;
            }
            // Custom is what the row says once the settings stop being a
            // preset's, not something to choose: chosen while they still are
            // one, the row goes back to saying which.
            let Some(chosen) = PRESETS.get(row.selected() as usize) else {
                let matching = stored_window(&owner, id)
                    .and_then(|(_, window, zone, _)| Preset::matching(&window, zone.as_deref()))
                    .and_then(|found| PRESETS.iter().position(|p| p == found));
                if let Some(at) = matching {
                    syncing.set(true);
                    row.set_selected(at as u32);
                    syncing.set(false);
                }
                return;
            };
            let (picked, picked_zone) = chosen.parts();
            // The name follows the preset unless somebody typed one of their
            // own: "London" stays "London", "My London" is theirs.
            let was = stored_window(&owner, id).map(|(name, ..)| name).unwrap_or_default();
            let named_by_preset = was.trim().is_empty() || PRESETS.iter().any(|p| p.name == was);
            set_window(&owner, id, |name, window, zone| {
                *window = picked.clone();
                *zone = picked_zone.clone();
                if named_by_preset {
                    *name = chosen.name.to_string();
                }
            });
            if named_by_preset {
                retitle(row, chosen.name);
            }
            refresh.run();
            let rebuild = rebuild.clone();
            glib::idle_add_local_once(move || rebuild());
        });
    }
    rows.push(preset.clone().upcast());

    let kinds = ["Clock times".to_string(), "After a session opens".to_string()];
    let is_clock = matches!(window, Window::Clock { .. });
    let times = combo_row("Times", &kinds, if is_clock { 0 } else { 1 });
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        let rebuild = rebuild.clone();
        times.connect_selected_notify(move |row| {
            let clock = row.selected() == 0;
            set_window(&owner, id, |_, window, zone| {
                *window = match (clock, &*window) {
                    (true, Window::Clock { .. }) | (false, Window::Session { .. }) => return,
                    // A session's open put in clock terms: nine thirty to ten
                    // is the opening range on most of what gets charted.
                    (true, _) => Window::Clock { from: 9 * 60 + 30, to: 10 * 60 },
                    (false, _) => Window::Session { part: Part::Regular, offset: 0, minutes: 30 },
                };
                *zone = None;
            });
            refresh.run();
            let rebuild = rebuild.clone();
            glib::idle_add_local_once(move || rebuild());
        });
    }
    rows.push(times.upcast());

    // Each edit puts the preset row back in step with what is now stored.
    let sync_preset = {
        let owner = owner.clone();
        let preset = preset.downgrade();
        Rc::new(move || {
            let (Some(preset), Some((_, window, zone, _))) = (preset.upgrade(), stored_window(&owner, id))
            else {
                return;
            };
            let at = Preset::matching(&window, zone.as_deref())
                .and_then(|found| PRESETS.iter().position(|p| p == found))
                .unwrap_or(PRESETS.len());
            if preset.selected() as usize != at {
                syncing.set(true);
                preset.set_selected(at as u32);
                syncing.set(false);
            }
        })
    };

    match window {
        Window::Clock { from, to } => {
            let shown_zone: Tz = zone.as_deref().and_then(|z| z.parse().ok()).unwrap_or(chart_zone);
            for (title, minutes, start) in [("Starts", from, true), ("Ends", to, false)] {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let sync = sync_preset.clone();
                let row = controls::time_row(title, minutes, format, move |minutes| {
                    set_window(&owner, id, |_, window, _| {
                        if let Window::Clock { from, to } = window {
                            match start {
                                true => *from = minutes,
                                false => *to = minutes,
                            }
                        }
                    });
                    sync();
                    refresh.run();
                });
                rows.push(row.upcast());
            }

            // The chart's own clock first, then the markets' in the order
            // they open; a zone set from the command line that is not among
            // them is offered too, so the row can show it.
            let mut zones: Vec<Option<String>> =
                std::iter::once(None).chain(ZONES.iter().map(|z| Some(z.to_string()))).collect();
            if !zones.contains(&zone) {
                zones.push(zone.clone());
            }
            let labels: Vec<String> = zones
                .iter()
                .map(|z| match z.as_deref().and_then(|z| z.parse::<Tz>().ok()) {
                    None => "Chart's time zone".to_string(),
                    Some(tz) => zone_label(tz),
                })
                .collect();
            let zone_row = combo_row(
                "Time zone",
                &labels,
                zones.iter().position(|z| *z == zone).unwrap_or(0),
            );
            zone_row.set_enable_search(true);
            zone_row.set_subtitle(&conversion(from, to, shown_zone, chart_zone, zone.is_none(), format));
            {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let rebuild = rebuild.clone();
                zone_row.connect_selected_notify(move |row| {
                    let Some(chosen) = zones.get(row.selected() as usize).cloned() else { return };
                    set_window(&owner, id, |_, _, zone| *zone = chosen.clone());
                    refresh.run();
                    let rebuild = rebuild.clone();
                    glib::idle_add_local_once(move || rebuild());
                });
            }
            rows.push(zone_row.upcast());
        }
        Window::Session { part, offset, minutes } => {
            let parts = [Part::Regular, Part::Overnight, Part::Pre, Part::Post];
            let labels: Vec<String> = parts.iter().map(|p| p.label().to_string()).collect();
            let session = combo_row("Session", &labels, parts.iter().position(|p| *p == part).unwrap_or(0));
            {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let sync = sync_preset.clone();
                session.connect_selected_notify(move |row| {
                    let Some(chosen) = parts.get(row.selected() as usize).copied() else { return };
                    set_window(&owner, id, |_, window, _| {
                        if let Window::Session { part, .. } = window {
                            *part = chosen;
                        }
                    });
                    sync();
                    refresh.run();
                });
            }
            rows.push(session.upcast());
            for (title, value, lower, is_offset) in
                [("Starts after (min)", offset, 0.0, true), ("Lasts (min)", minutes, 1.0, false)]
            {
                let owner = owner.clone();
                let refresh = refresh.clone();
                let sync = sync_preset.clone();
                let row = adw::SpinRow::with_range(lower, 24.0 * 60.0, 5.0);
                row.set_title(title);
                row.set_value(value as f64);
                row.connect_value_notify(move |row| {
                    let value = row.value().round() as u32;
                    set_window(&owner, id, |_, window, _| {
                        if let Window::Session { offset, minutes, .. } = window {
                            match is_offset {
                                true => *offset = value,
                                false => *minutes = value.max(1),
                            }
                        }
                    });
                    sync();
                    refresh.run();
                });
                rows.push(row.upcast());
            }
        }
    }

    rows.push(
        switch_row(
            owner,
            refresh,
            id,
            "Weekdays only",
            Some("Skips Saturday and Sunday"),
            weekdays,
            |indicator, on| {
                if let Params::TimeWindow { weekdays, .. } = &mut indicator.params {
                    *weekdays = on;
                }
            },
        )
        .upcast(),
    );
    rows
}

/// Name the panel this row is on after the window, as it is typed: the title
/// is what the window is called, and a panel still titled "Opening range"
/// over a window renamed London is describing something else.
fn retitle(row: &impl IsA<gtk::Widget>, name: &str) {
    let title = match name.trim() {
        "" => "Window",
        name => name,
    };
    if let Some(dialog) = row.ancestor(adw::Dialog::static_type()).and_downcast::<adw::Dialog>() {
        dialog.set_title(title);
    }
    if let Some(page) =
        row.ancestor(adw::NavigationPage::static_type()).and_downcast::<adw::NavigationPage>()
    {
        page.set_title(title);
    }
}

/// What a window's hours come to on the chart: "Times are in the chart's time
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

// -- Session Levels -----------------------------------------------------------

fn levels(
    owner: &Rc<Owner>,
    refresh: &Refresh,
    colour: &str,
    indicator: &Indicator,
) -> Vec<adw::PreferencesGroup> {
    let Params::SessionLevels { source, levels } = &indicator.params else { return Vec::new() };
    let id = indicator.id;
    let group = adw::PreferencesGroup::new();
    group.set_title("Levels");
    group.set_description(Some("Each day's, drawn across the day after it"));

    let labels: Vec<String> = LevelSource::ALL.iter().map(|s| s.label().to_string()).collect();
    let from = combo_row("From", &labels, LevelSource::ALL.iter().position(|s| s == source).unwrap_or(0));
    if market(owner).is_some_and(|(schedule, _)| !schedule.has_regular()) {
        from.set_subtitle("Trades around the clock, so every choice is the whole day");
    }
    {
        let owner = owner.clone();
        let refresh = refresh.clone();
        from.connect_selected_notify(move |row| {
            let Some(chosen) = LevelSource::ALL.get(row.selected() as usize).copied() else { return };
            update(&owner, id, |indicator| {
                if let Params::SessionLevels { source, .. } = &mut indicator.params {
                    *source = chosen;
                }
            });
            refresh.run();
        });
    }
    group.add(&from);

    // High and low go together: one without the other is half a range.
    let wanted: &[(&str, &[Level])] = &[
        ("High and low", &[Level::High, Level::Low]),
        ("Close", &[Level::Close]),
        ("Open", &[Level::Open]),
        ("Midpoint", &[Level::Mid]),
    ];
    for (title, which) in wanted {
        let which: Vec<Level> = which.to_vec();
        let on = which.iter().all(|level| levels.contains(level));
        group.add(&switch_row(owner, refresh, id, title, None, on, move |indicator, on| {
            if let Params::SessionLevels { levels, .. } = &mut indicator.params {
                levels.retain(|level| !which.contains(level));
                if on {
                    levels.extend(which.iter().copied());
                }
                // Kept in one order, so the list reads the same however it
                // was switched on.
                levels.sort_by_key(|level| Level::ALL.iter().position(|l| l == level));
            }
        }));
    }

    vec![group, line_group(owner, refresh, colour, indicator, "Line", None)]
}

#[cfg(test)]
mod tests {
    use chrono_tz::America::New_York;
    use chrono_tz::Europe::London;

    use super::*;

    #[test]
    fn a_window_in_the_charts_clock_says_so() {
        let said = conversion(8 * 60, 16 * 60 + 30, New_York, New_York, true, TimeFormat::H24);
        assert_eq!(said, "Times are in the chart's time zone, New York");
    }

    #[test]
    fn a_window_in_its_own_clock_says_what_it_comes_to() {
        let said = conversion(8 * 60, 16 * 60 + 30, London, New_York, false, TimeFormat::H24);
        assert!(said.ends_with("New York time today"), "{said}");
        // Four or five hours behind, depending on the week.
        assert!(said.starts_with("03:00–11:30") || said.starts_with("04:00–12:30"), "{said}");
    }
}
