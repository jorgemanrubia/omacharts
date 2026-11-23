//! A change to how an indicator looks repaints the chart, and nothing more.
//!
//! The author saw the whole chart redraw for a colour picked in a volume
//! profile's settings: the change charted the symbol again, which read the
//! bars back, asked the feed for them and worked every indicator out from
//! scratch. These hold the chart to going straight from one finished
//! picture to the next.

use std::rc::Rc;

use gtk::glib;
use gtk::prelude::*;
use omacharts_engine::indicators::{profile, Output, Params};
use omacharts_engine::theme::ColorChoice;
use omacharts_engine::{Indicator, IndicatorKind};

use super::trade::tests::window;
use super::{ChartPane, Window, SETTING_WORKSPACE};
use crate::store::Store;

fn settle(done: impl Fn() -> bool) -> bool {
    let context = glib::MainContext::default();
    for _ in 0..600 {
        if done() {
            return true;
        }
        context.iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    done()
}

/// Let whatever the last change queued run out.
fn drain() {
    let context = glib::MainContext::default();
    for _ in 0..40 {
        context.iteration(false);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn red() -> Option<ColorChoice> {
    Some(ColorChoice::Fixed { hex: "#c0392b".into() })
}

/// What the chart draws for indicator `id`, with its colour.
fn drawn(pane: &ChartPane, id: u32) -> Option<(Output, String, Indicator)> {
    pane.view.with_indicators(|drawn| {
        drawn.iter().find(|d| d.indicator.id == id).map(|d| (d.output.clone(), d.color.clone(), d.indicator.clone()))
    })
}

fn profiles(pane: &ChartPane, id: u32) -> usize {
    match drawn(pane, id).map(|(output, ..)| output) {
        Some(Output::Profiles(profiles)) => profiles.len(),
        _ => 0,
    }
}

/// A chart charting its bars with a volume profile and a moving average,
/// all of it worked out.
fn charted(store: &Rc<Store>) -> (Rc<Window>, Rc<ChartPane>) {
    let window = window(store);
    let pane = window.focused_pane();
    assert!(settle(|| pane.view.with_bars(|bars| bars.len() > 50)), "bars on the chart");
    window.set_indicators(vec![Indicator::new(1, IndicatorKind::VolumeProfile), Indicator::new(2, IndicatorKind::Sma)]);
    assert!(settle(|| profiles(&pane, 1) > 0), "profiles on the chart");
    drain();
    (window, pane)
}

/// The one change, done the way the settings do it: the whole set, edited.
fn edit(window: &Rc<Window>, id: u32, change: impl Fn(&mut Indicator)) {
    let mut indicators = window.indicators();
    let indicator = indicators.iter_mut().find(|i| i.id == id).expect("the indicator");
    change(indicator);
    window.set_indicators(indicators);
}

/// A profile's colour, its point of control's and its transparency, and a
/// line's colour and weight: each one is on the chart as soon as it is
/// set, over the same bars and the same profiles, and nothing is worked
/// out again, then or after.
#[test]
fn a_change_of_colour_repaints_and_computes_nothing() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let (window, pane) = charted(&store);
        let computed = pane.computed.get();
        let bars = pane.view.with_bars(|bars| bars.to_vec());
        let memo = window.series.borrow().len();
        let (before, ..) = drawn(&pane, 1).expect("the profile");
        let legend = pane.indicator_legend.first_child().expect("the profile's line in the legend");

        edit(&window, 1, |i| i.color = red());
        // Straight away, in the same turn: there is no frame between with
        // the profile missing or drawn from anything else.
        let (output, color, _) = drawn(&pane, 1).expect("the profile, still");
        assert_eq!(output, before, "the same profiles");
        assert_eq!(color, "#c0392b", "in its new colour");
        edit(&window, 1, |i| {
            if let Params::VolumeProfile { poc_color, alpha, style, align, .. } = &mut i.params {
                *poc_color = red();
                *alpha = 0.8;
                *style = profile::Style::Outline;
                *align = profile::Align::Right;
            }
        });
        let (output, _, indicator) = drawn(&pane, 1).expect("the profile, still");
        assert_eq!(output, before, "the same profiles");
        assert!(matches!(indicator.params, Params::VolumeProfile { alpha, .. } if alpha == 0.8), "drawn as set");
        edit(&window, 2, |i| {
            i.color = red();
            i.stroke.width = 4.0;
        });
        assert_eq!(drawn(&pane, 2).map(|(_, color, i)| (color, i.stroke.width)), Some(("#c0392b".into(), 4.0)));

        assert_eq!(pane.indicator_legend.first_child(), Some(legend), "the legend's lines kept, not built again");
        assert_eq!(window.legend_colour(&pane, 1).as_deref(), Some("#c0392b"), "its dot repainted");

        drain();
        assert_eq!(pane.computed.get(), computed, "nothing worked out again");
        assert!(!pane.view.loading(), "nothing loading");
        assert_eq!(pane.view.with_bars(|bars| bars.to_vec()), bars, "the same bars");
        assert_eq!(window.series.borrow().len(), memo, "the bars kept in memory kept");
        assert_eq!(drawn(&pane, 1).map(|(output, ..)| output), Some(before), "and nothing else drawn after");
    });
}

/// A change to what a profile computes works it out again — once, over the
/// bars the chart has, with the profiles on the chart throughout.
#[test]
fn a_change_of_rows_computes_once_over_the_bars_on_the_chart() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let (window, pane) = charted(&store);
        let computed = pane.computed.get();
        let bars = pane.view.with_bars(|bars| bars.to_vec());
        let (before, ..) = drawn(&pane, 1).expect("the profile");

        edit(&window, 1, |i| {
            if let Params::VolumeProfile { rows, .. } = &mut i.params {
                *rows = Some(12);
            }
        });
        let (output, ..) = drawn(&pane, 1).expect("the profile, still");
        assert_ne!(output, before, "worked out again, in the same turn");
        assert!(profiles(&pane, 1) > 0, "and drawn");

        drain();
        assert_eq!(pane.computed.get(), computed + 1, "once");
        assert!(!pane.view.loading(), "nothing loading");
        assert_eq!(pane.view.with_bars(|bars| bars.to_vec()), bars, "the same bars");
    });
}

/// A chart on a template takes a colour changed on it the same way, and
/// so does every other chart on the template.
#[test]
fn a_colour_changed_on_a_template_repaints_its_charts() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let (window, pane) = charted(&store);
        let template = window.create_template("Profiles");
        window.set_template_indicators(template, window.indicators());
        window.set_template(Some(template));
        drain();
        let computed = pane.computed.get();
        let (before, ..) = drawn(&pane, 1).expect("the profile");

        let mut indicators = window.indicators();
        indicators[0].color = red();
        window.set_template_indicators(template, indicators);

        let (output, color, _) = drawn(&pane, 1).expect("the profile, still");
        assert_eq!((output, color.as_str()), (before, "#c0392b"));
        drain();
        assert_eq!(pane.computed.get(), computed, "nothing worked out again");
    });
}

/// A colour set from a terminal is drawn on the chart already on screen,
/// rather than on one built again in its place.
#[test]
fn a_colour_set_by_a_command_repaints_the_chart_on_screen() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let (window, pane) = charted(&store);
        window.save_workspace();
        let computed = pane.computed.get();
        let (before, ..) = drawn(&pane, 1).expect("the profile");

        let mut workspace: serde_json::Value =
            serde_json::from_str(&store.setting(SETTING_WORKSPACE).expect("written")).expect("json");
        workspace["books"][0]["panes"][0]["indicators"][0]["color"] = serde_json::json!({"kind": "fixed", "hex": "#c0392b"});
        store.set_setting(SETTING_WORKSPACE, &workspace.to_string());
        window.adopt_indicators();

        assert!(Rc::ptr_eq(&window.focused_pane(), &pane), "the same chart");
        let (output, color, _) = drawn(&pane, 1).expect("the profile, still");
        assert_eq!((output, color.as_str()), (before, "#c0392b"));
        drain();
        assert_eq!(pane.computed.get(), computed, "nothing worked out again");
        // And written down as it is now, the colour kept.
        window.save_workspace();
        let written: serde_json::Value =
            serde_json::from_str(&store.setting(SETTING_WORKSPACE).expect("written")).expect("json");
        assert_eq!(written["books"][0]["panes"][0]["indicators"][0]["color"], serde_json::json!({"kind": "fixed", "hex": "#c0392b"}));
    });
}

/// What the chart draws of a market profile, by period.
fn market_profiles(
    pane: &ChartPane,
    id: u32,
) -> Vec<std::sync::Arc<omacharts_engine::indicators::MarketProfile>> {
    match drawn(pane, id).map(|(output, ..)| output) {
        Some(Output::MarketProfiles(profiles)) => profiles,
        _ => Vec::new(),
    }
}

/// A streamed feed ticking into a chart with a market profile, beside a
/// volume profile: the forming session's profile moves with the ticks, and
/// each tick works out that session alone, every one before it kept.
#[test]
fn a_streamed_tick_works_out_the_forming_market_profile_alone() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gtk::gio::ApplicationFlags::NON_UNIQUE);
        let feed: std::sync::Arc<dyn omacharts_engine::Provider> = std::sync::Arc::new(
            omacharts_engine::providers::Synthetic::new(Some(std::time::Duration::from_millis(15))),
        );
        let window = Window::build_with(&app, store.clone(), vec![feed]);
        let pane = window.focused_pane();
        assert!(
            settle(|| pane.view.with_bars(|bars| bars.len() > 50)),
            "bars on the chart"
        );
        window.set_indicators(vec![
            Indicator::new(1, IndicatorKind::VolumeProfile),
            Indicator::new(2, IndicatorKind::MarketProfile),
        ]);
        assert!(
            settle(|| market_profiles(&pane, 2).len() > 1),
            "market profiles on the chart"
        );
        let first = market_profiles(&pane, 2);
        let computed = pane.computed.get();

        // Ticks arrive, and the chart is worked out again for them.
        assert!(
            settle(|| pane.computed.get() > computed + 3
                && market_profiles(&pane, 2).last() != first.last()
                // Not the tick that dropped the oldest bars, which moves
                // every index and so works every session out again.
                && pane.memos.market.with(2, |memo| memo.worked) <= 2),
            "the forming profile moved with the ticks"
        );
        let now = market_profiles(&pane, 2);
        let worked = pane.memos.market.with(2, |memo| memo.worked);
        assert!(
            worked <= 2,
            "a tick works out the forming session, not all {}: {worked}",
            now.len()
        );
        // The chart keeps a bounded run of bars, so the oldest may have
        // gone meanwhile and every index moved with them: a session is
        // known by its start, and its rows and letters are as they were.
        // The oldest left may have lost bars, and the newest then went on.
        let mut compared = 0;
        for is in now.iter().skip(1) {
            let Some(was) = first[..first.len() - 1]
                .iter()
                .skip(1)
                .find(|was| was.profile.start_ts == is.profile.start_ts)
            else {
                continue;
            };
            assert_eq!(
                (
                    &was.profile.rows,
                    &was.letters,
                    &was.starts,
                    &was.columns,
                    was.open_row,
                    was.close_row
                ),
                (
                    &is.profile.rows,
                    &is.letters,
                    &is.starts,
                    &is.columns,
                    is.open_row,
                    is.close_row
                ),
                "the session from {} as it was",
                is.profile.start_ts
            );
            compared += 1;
        }
        assert!(compared > 10, "sessions before it to hold to: {compared}");
        assert_eq!(
            now.last().map(|m| m.profile.last_bar),
            pane.view.with_bars(|bars| bars.len().checked_sub(1))
        );
        // Its volume profile beside it, over the same forming session.
        let volume = match drawn(&pane, 1).map(|(output, ..)| output) {
            Some(Output::Profiles(profiles)) => profiles,
            _ => Vec::new(),
        };
        assert_eq!(
            volume.last().map(|p| (p.start_ts, p.last_bar)),
            now.last().map(|m| (m.profile.start_ts, m.profile.last_bar)),
            "the pair grows together"
        );
        window.window.destroy();
    });
}

/// A market profile expanded by hand from the chart, its TPOs laid out
/// over time, and collapsed again: one profile at a time, kept with the
/// chart and found there after a restart, and the forming one, while the
/// study expands the last, left to the study.
#[test]
fn a_market_profile_expands_and_collapses_one_profile_at_a_time() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        let pane = window.focused_pane();
        assert!(
            settle(|| pane.view.with_bars(|bars| bars.len() > 50)),
            "bars on the chart"
        );
        window.set_indicators(vec![Indicator::new(1, IndicatorKind::MarketProfile)]);
        assert!(
            settle(|| market_profiles(&pane, 1).len() > 2),
            "market profiles on the chart"
        );
        let profiles = market_profiles(&pane, 1);
        let earlier = &profiles[profiles.len() - 2].profile;
        let inside = (earlier.start_ts + earlier.end_ts) / 2;
        let expanded =
            |pane: &ChartPane| match drawn(pane, 1).map(|(_, _, indicator)| indicator.params) {
                Some(Params::MarketProfile { expanded, .. }) => expanded,
                _ => Vec::new(),
            };

        assert_eq!(
            pane.view
                .profile_of(1, inside)
                .and_then(|spot| spot.expanded),
            Some(false),
            "offered to expand"
        );
        window
            .expand_profile_at(None, inside, true)
            .expect("expanded");
        // Kept by the period it was cut from, as a merge is, not its first
        // bar.
        let period = earlier.parts[0];
        assert_eq!(expanded(&pane), vec![period], "drawn expanded, that one alone");
        // A change of paint keeps it so: drawn again, not worked out again.
        let mut indicators = window.indicators();
        indicators[0].color = red();
        window.set_indicators(indicators);
        assert_eq!(expanded(&pane), vec![period], "still expanded, repainted");
        assert_eq!(
            pane.view
                .profile_of(1, inside)
                .and_then(|spot| spot.expanded),
            Some(true),
            "offered to collapse"
        );
        assert!(
            window.expand_profile_at(None, inside, true).is_err(),
            "expanded already"
        );

        // Kept with the chart, and there after a restart.
        let written: serde_json::Value =
            serde_json::from_str(&store.setting(SETTING_WORKSPACE).expect("written"))
                .expect("json");
        let kept = &written["books"][0]["panes"][0]["profile_expanded"][0];
        assert_eq!(kept["indicator"], 1, "{kept}");
        assert_eq!(kept["starts"], serde_json::json!([period]));
        window.window.destroy();
        let again = super::trade::tests::window(&store);
        let pane = again.focused_pane();
        assert!(
            settle(|| market_profiles(&pane, 1).len() > 2),
            "market profiles on the chart again"
        );
        assert_eq!(expanded(&pane), vec![period], "expanded after a restart");

        again
            .expand_profile_at(None, inside, false)
            .expect("collapsed");
        assert!(expanded(&pane).is_empty(), "stacked again");
        assert!(
            again.expand_profile_at(None, inside, false).is_err(),
            "not expanded"
        );

        // The forming one, while the study expands the last: the study's.
        let mut indicators = again.indicators();
        if let Params::MarketProfile { expand_last, .. } = &mut indicators[0].params {
            *expand_last = true;
        }
        again.set_indicators(indicators);
        let forming = market_profiles(&pane, 1)
            .last()
            .map(|m| m.profile.end_ts)
            .expect("a forming profile");
        assert_eq!(
            pane.view.profile_of(1, forming).map(|spot| spot.expanded),
            Some(None),
            "nothing to offer"
        );
        assert!(again.expand_profile_at(None, forming, false).is_err());
        again.window.destroy();
    });
}

/// `chart profile list` reads a market profile's TPO structure as the
/// chart draws it: each row's letters, the point of control and value
/// area, the open and the close with their rows, each TPO period, whether
/// it is expanded, and the volume profile it is drawn beside.
#[test]
fn a_market_profiles_structure_reads_from_the_command_line_as_it_is_drawn() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        let pane = window.focused_pane();
        window.set_timeframe(omacharts_engine::Timeframe::minutes(5));
        assert!(settle(|| pane.view.with_bars(|bars| bars.len() > 200)), "bars on the chart");
        window.set_indicators(vec![
            Indicator::new(1, IndicatorKind::VolumeProfile),
            Indicator::new(2, IndicatorKind::MarketProfile),
        ]);
        assert!(settle(|| market_profiles(&pane, 2).len() > 2), "market profiles on the chart");
        let drawn = market_profiles(&pane, 2);
        let earlier = &drawn[drawn.len() - 2].profile;
        window.expand_profile_at(Some(2), (earlier.end_ts + earlier.start_ts) / 2, true).expect("expanded");
        let drawn = market_profiles(&pane, 2);
        let bars = pane.view.with_bars(|bars| bars.to_vec());

        let live: &dyn crate::cli::Live = &window;
        let list = |tail: &str| -> serde_json::Value {
            let args: Vec<String> =
                ["omacharts", "chart", "profile", "list"].into_iter().chain(tail.split_whitespace()).map(str::to_string).collect();
            let out = crate::cli::run(&args, &store, Some(live), &crate::cli::Here);
            assert_eq!(out.code, 0, "{}", out.err);
            serde_json::from_str(&out.out).expect("json")
        };
        let read = list("--id 2 --tpos --json");
        assert_eq!(read["kind"], "market_profile");
        assert_eq!(read["indicator"], 2);
        assert_eq!(read["tpo_minutes"], 30);
        assert_eq!(read["paired_with"], serde_json::json!([1]));
        let near = |value: &serde_json::Value, price: f64| (value.as_f64().expect("a price") - price).abs() < 1e-3;
        let listed = read["profiles"].as_array().expect("profiles");
        assert_eq!(listed.len(), drawn.len(), "every profile");
        let mut expanded = 0;
        for (json, market) in listed.iter().zip(&drawn) {
            let profile = &market.profile;
            assert_eq!(json["periods"], serde_json::json!(profile.parts));
            let rows = json["rows"].as_array().expect("rows");
            assert_eq!(rows.len(), profile.rows.len(), "the same rows");
            for (at, row) in rows.iter().enumerate() {
                let letters: String =
                    market.letters_of(at).iter().map(|l| omacharts_engine::indicators::market::letter(*l)).collect();
                let row_price = profile.rows[at].low + market.tick_size / 2.0;
                assert!(near(&row[0], row_price), "named by its lowest tick: {} vs {row_price}", row[0]);
                assert_eq!(row[1], letters.len(), "its TPOs counted");
                assert_eq!(row[2], letters, "its letters");
            }
            // On ticks, as the rows are named: the point of control by its
            // row's lowest tick, the value area from its lowest row's lowest
            // tick to its highest row's highest.
            let half = market.tick_size / 2.0;
            assert!(near(&json["poc"], profile.rows[profile.poc].low + half), "the point of control's row");
            assert!(rows.iter().any(|row| row[0] == json["poc"]), "a row of its own");
            let (low, high) = profile.value_area_bounds();
            assert!(near(&json["value_area_low"], low + half) && near(&json["value_area_high"], high - half));
            let centre = |row: usize| profile.rows[row].low + market.tick_size / 2.0;
            assert!(near(&json["open"]["row"], centre(market.open_row)), "the open's row");
            assert!(near(&json["close"]["row"], centre(market.close_row)), "the close's row");
            // The arrows' rows: the open's beside the first letter of its
            // row, the first TPO period's; the close's beside the last of
            // its row.
            assert_eq!(market.letters_of(market.open_row).first(), Some(&0), "the open in A");
            // To the tick, as every price the command gives.
            let on_tick = |value: &serde_json::Value, price: f64| (value.as_f64().expect("a price") - price).abs() <= market.tick_size / 2.0 + 1e-9;
            assert!(on_tick(&json["open"]["price"], bars[profile.first_bar].open), "its first bar's open");
            assert!(on_tick(&json["close"]["price"], bars[profile.last_bar].close), "its last bar's close");
            assert!(near(&json["row_height"], market.tick_size * market.row_ticks as f64));
            let periods = json["tpo_periods"].as_array().expect("TPO periods");
            assert_eq!(periods.len(), market.column_bars.len(), "a period for each column");
            assert_eq!(periods[0]["letter"], "A");
            assert_eq!(periods[0]["start"], bars[profile.first_bar].ts, "from the first bar");
            assert_eq!(json["paired_with"][0]["indicator"], 1, "its volume profile beside it");
            expanded += usize::from(json["expanded"] == true);
        }
        assert_eq!(expanded, 1, "the one expanded");
        assert_eq!(listed[listed.len() - 2]["expanded"], true);

        let volume = list("--id 1 --json");
        assert_eq!(volume["kind"], "volume_profile");
        assert_eq!(volume["paired_with"], serde_json::json!([2]));
        assert!(volume["profiles"][0].get("rows").is_none() && volume["profiles"][0].get("open").is_none(), "a volume profile's, as it was");
        window.window.destroy();
    });
}

/// A chart's merges and the profiles expanded on it are the chart's to
/// keep, both kept by where each profile starts: through a change of
/// resolution its sessions outlive, a change to the studies, a template
/// taken off and put back, a switch of chartbook and back, and a restart.
/// A composite expands as one, and splitting it leaves the expanded state
/// with its first part, which collapses as any other.
#[test]
fn merged_and_expanded_profiles_outlive_everything_the_chart_goes_through() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        let pane = window.focused_pane();
        window.set_timeframe(omacharts_engine::Timeframe::minutes(5));
        assert!(settle(|| pane.view.with_bars(|bars| bars.len() > 200)), "bars on the chart");
        let studies = vec![
            Indicator::new(1, IndicatorKind::VolumeProfile),
            Indicator::new(2, IndicatorKind::MarketProfile),
        ];
        window.set_indicators(studies.clone());
        assert!(settle(|| market_profiles(&pane, 2).len() > 3), "market profiles on the chart");
        let now = market_profiles(&pane, 2);
        let (first, second) = (now[now.len() - 3].profile.clone(), now[now.len() - 2].profile.clone());
        let inside = |p: &profile::Profile| (p.start_ts + p.end_ts) / 2;
        let (a, b) = (first.parts[0], second.parts[0]);

        // Merged and expanded, and drawn so: the pair merged together.
        window.change_profile_at(Some(2), inside(&second), true).expect("merged");
        window.expand_profile_at(Some(2), inside(&second), true).expect("the composite expanded");
        let held = |pane: &ChartPane| {
            let of = |id: u32| match drawn(pane, id).map(|(_, _, indicator)| indicator.params) {
                Some(Params::MarketProfile { merged, expanded, .. }) => (merged, expanded),
                Some(Params::VolumeProfile { merged, .. }) => (merged, Vec::new()),
                _ => (Vec::new(), Vec::new()),
            };
            (of(1), of(2))
        };
        let composite = |pane: &ChartPane| {
            market_profiles(pane, 2).iter().any(|m| m.profile.parts == vec![a, b])
                && match drawn(pane, 1).map(|(output, ..)| output) {
                    Some(Output::Profiles(profiles)) => profiles.iter().any(|p| p.parts == vec![a, b]),
                    _ => false,
                }
        };
        let kept = ((vec![b], vec![]), (vec![b], vec![a]));
        let both_back = |pane: &ChartPane, after: &str| {
            assert!(settle(|| market_profiles(pane, 2).len() > 2), "profiles {after}");
            assert_eq!(held(pane), kept, "merged and expanded {after}");
            assert!(composite(pane), "the composite drawn {after}, both studies");
            assert!(
                market_profiles(pane, 2).iter().any(|m| m.profile.parts == vec![a, b]
                    && omacharts_engine::indicators::market::by_hand(&[a], &m.profile)),
                "the composite drawn expanded {after}"
            );
        };
        both_back(&pane, "at first");

        // A resolution its sessions outlive, and back.
        window.set_timeframe(omacharts_engine::Timeframe::minutes(15));
        assert!(settle(|| held(&pane) == kept && composite(&pane)), "{:?} on 15m", held(&pane));
        both_back(&pane, "on 15m");
        window.set_timeframe(omacharts_engine::Timeframe::minutes(5));
        assert!(settle(|| held(&pane) == kept && composite(&pane)), "back on 5m");

        // A change to a study, and a template taken off and put back.
        let mut changed = window.indicators();
        changed[1].color = red();
        window.set_indicators(changed.clone());
        both_back(&pane, "after a change of look");
        let template = crate::templates::create_from(&store, "Profiles", changed);
        window.set_template(Some(template.id));
        both_back(&pane, "on a template");
        window.set_template(None);
        assert!(market_profiles(&pane, 2).is_empty(), "no studies off the template");
        window.set_template(Some(template.id));
        assert!(settle(|| market_profiles(&pane, 2).len() > 2), "the template's studies back");
        both_back(&pane, "with the template put back");

        // Another chartbook, and back to this one.
        window.duplicate_chartbook();
        assert_eq!(window.active.get(), 1);
        window.activate_book(0);
        let pane = window.focused_pane();
        both_back(&pane, "after a switch of chartbook");

        // A restart.
        window.window.destroy();
        let again = super::trade::tests::window(&store);
        let pane = again.focused_pane();
        both_back(&pane, "after a restart");

        // Split: the expanded state stays with the first part alone, which
        // then collapses as any profile does.
        again.change_profile_at(Some(2), inside(&second), false).expect("split");
        assert!(
            settle(|| market_profiles(&pane, 2).iter().any(|m| m.profile.parts == vec![b])),
            "split"
        );
        let expanded_of = |pane: &ChartPane, start: i64| {
            market_profiles(pane, 2)
                .iter()
                .find(|m| m.profile.parts == vec![start])
                .map(|m| omacharts_engine::indicators::market::by_hand(&[a], &m.profile))
        };
        assert_eq!(expanded_of(&pane, a), Some(true), "the first part still expanded");
        assert_eq!(expanded_of(&pane, b), Some(false), "the second part stacked");
        assert_eq!(pane.view.profile_of(2, inside(&first)).and_then(|s| s.expanded), Some(true));
        again.expand_profile_at(Some(2), inside(&first), false).expect("collapsed after the split");
        assert_eq!(held(&pane), ((vec![], vec![]), (vec![], vec![])), "nothing merged or expanded");
        again.window.destroy();
        let third = super::trade::tests::window(&store);
        let pane = third.focused_pane();
        assert!(settle(|| market_profiles(&pane, 2).len() > 2), "profiles after another restart");
        assert_eq!(held(&pane), ((vec![], vec![]), (vec![], vec![])), "still nothing after a restart");
    });
}

/// How many bubbles large orders indicator `id` draws over the whole chart.
fn bubbles(pane: &ChartPane, id: u32) -> Option<Vec<(i64, u64)>> {
    let shown = pane.view.with_bars(|bars| 0..bars.len());
    let (orders, _) = pane.view.read_large_orders(id, shown)?;
    Some(orders.iter().map(|o| (o.order.ts_nanos, o.order.size)).collect())
}

/// [`bubbles`] once the feed's days have all come in: they come newest
/// first, a book handed over after each.
fn all_bubbles(pane: &ChartPane, id: u32) -> Vec<(i64, u64)> {
    let mut last = None;
    for _ in 0..100 {
        assert!(settle(|| bubbles(pane, id).is_some_and(|b| !b.is_empty())), "bubbles on the chart");
        for _ in 0..60 {
            glib::MainContext::default().iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let now = bubbles(pane, id);
        if now == last {
            return now.unwrap();
        }
        last = now;
    }
    panic!("the bubbles never settled");
}

fn frozen() -> i64 {
    super::trade::tests::FROZEN_AT
}

/// The author's report: the thresholds of a chart's large orders changed in
/// its settings, every bubble went, and putting the old ones back left none
/// either. A threshold is paint, so the chart is drawn again and nothing
/// worked out; it is still asked of the feed's large orders, whose book
/// comes back and is drawn with no other change to the chart. A change of
/// resolution, which works the chart out again, used to be the only way.
#[test]
fn thresholds_changed_in_the_settings_draw_their_bubbles_once_built() {
    use crate::large_orders::tests::{es, scratch, Asked, Fake};
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        let pane = window.focused_pane();
        assert!(settle(|| pane.view.with_bars(|bars| bars.len() > 50)), "bars on the chart");
        let path = scratch("window-thresholds");
        let provider: std::sync::Arc<dyn omacharts_engine::Provider> =
            std::sync::Arc::new(Fake(std::sync::Arc::new(std::sync::Mutex::new(Asked::default()))));
        let at_price = Rc::new(crate::at_price::AtPrice::new(provider.clone(), Some(path.clone())).unwrap());
        at_price.set_clock(frozen);
        let large = Rc::new(crate::large_orders::LargeOrders::new(provider, Some(path)).unwrap());
        large.set_clock(frozen);
        window.wire_at_price(at_price.clone());
        window.wire_large_orders(large.clone());
        *pane.instrument.borrow_mut() = Some(es());
        *pane.ladders.borrow_mut() = Some(crate::ui::pane::LadderSource {
            at_price: at_price.clone(),
            large_orders: Some(large.clone()),
            key: "fake:ES".into(),
            symbol: "ES".into(),
        });
        let set = |regular: u64, extended: u64| {
            move |i: &mut Indicator| {
                if let Params::LargeOrders { regular: r, extended: e, .. } = &mut i.params {
                    (*r, *e) = (regular, extended);
                }
            }
        };
        let mut indicator = Indicator::new(1, IndicatorKind::LargeOrders);
        set(5, 5)(&mut indicator);
        window.set_indicators(vec![indicator]);
        let at_five = all_bubbles(&pane, 1);
        let computed = pane.computed.get();

        // The overnight threshold up: its book is built and drawn, a
        // subset, with nothing else done to the chart.
        edit(&window, 1, set(5, 50));
        let fewer = all_bubbles(&pane, 1);
        assert!(!fewer.is_empty() && fewer.len() < at_five.len(), "{} of {}", fewer.len(), at_five.len());
        assert!(fewer.iter().all(|order| at_five.contains(order)));

        // Back: drawn in the same turn, from the book kept.
        edit(&window, 1, set(5, 5));
        assert_eq!(bubbles(&pane, 1), Some(at_five.clone()), "every bubble back at once");

        // The day's threshold up too.
        edit(&window, 1, set(50, 5));
        let fewer = all_bubbles(&pane, 1);
        assert!(fewer.len() < at_five.len() && fewer.iter().all(|order| at_five.contains(order)));
        assert_eq!(pane.computed.get(), computed, "a repaint each time, nothing worked out");
        at_price.shutdown();
        large.shutdown();
    });
}

/// The author's report: after a restart a session volume profile and the
/// large orders beside it drew nothing for a long while, though the
/// ladders were kept from the run before, and a market profile, drawn from
/// the bars, was there at once. The ladders' load filled the store's holes
/// from the provider before it answered with anything, and the large
/// orders wait for the ladders: both waited on a history plant slow to
/// answer. With what the store kept and the provider still holding back,
/// the profile and the bubbles are drawn, with nothing done to the chart.
#[test]
fn a_chart_restored_draws_its_profile_and_large_orders_from_the_store_before_the_feed_answers() {
    use crate::large_orders::tests::{es, scratch, Asked, Fake};
    use omacharts_engine::ladders::{Days, Minute, Side};
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        let pane = window.focused_pane();
        assert!(settle(|| pane.view.with_bars(|bars| bars.len() > 50)), "bars on the chart");
        // The run before kept the ladders of the newest bars' hours; the
        // older ones are a hole the provider is asked about, slowly.
        let path = scratch("window-restored");
        let last = pane.view.with_bars(|bars| bars.last().map(|bar| bar.ts)).unwrap();
        let (a, b) = ((last - 3 * 3_600).div_euclid(60) * 60, last.div_euclid(60) * 60);
        let minutes: Vec<Minute> = (a..b)
            .step_by(60)
            .map(|ts| {
                let mut minute = Minute::opening(ts, 7_625.0);
                minute.add_trade(30_500, 7_625.0, 3, Side::Buy, 1);
                minute
            })
            .collect();
        Store::open_at(&path).unwrap().save_ladder_history("fake:ES", 0.25, &Days::new(&es()), a, b, &minutes).unwrap();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let mut asked = Asked::default();
        asked.ladders_held = Some(held);
        let provider: std::sync::Arc<dyn omacharts_engine::Provider> =
            std::sync::Arc::new(Fake(std::sync::Arc::new(std::sync::Mutex::new(asked))));
        let at_price = Rc::new(crate::at_price::AtPrice::new(provider.clone(), Some(path.clone())).unwrap());
        at_price.set_clock(frozen);
        let large = Rc::new(crate::large_orders::LargeOrders::new(provider, Some(path)).unwrap());
        large.set_clock(frozen);
        window.wire_at_price(at_price.clone());
        window.wire_large_orders(large.clone());
        *pane.instrument.borrow_mut() = Some(es());
        *pane.ladders.borrow_mut() = Some(crate::ui::pane::LadderSource {
            at_price: at_price.clone(),
            large_orders: Some(large.clone()),
            key: "fake:ES".into(),
            symbol: "ES".into(),
        });
        let mut orders = Indicator::new(2, IndicatorKind::LargeOrders);
        if let Params::LargeOrders { regular, extended, .. } = &mut orders.params {
            (*regular, *extended) = (5, 5);
        }
        window.set_indicators(vec![Indicator::new(1, IndicatorKind::VolumeProfile), orders]);

        assert!(settle(|| profiles(&pane, 1) > 0), "the profile drawn from the store");
        assert!(at_price.with_held("fake:ES", |held| held.is_some()), "the ladders the store kept");
        assert!(!all_bubbles(&pane, 2).is_empty(), "the bubbles, the ladders' feed still asked");
        drop(release);
        at_price.shutdown();
        large.shutdown();
    });
}

/// A chart draws one footprint. The author's chart had two, the second with
/// its volume profile on and never drawn: pasting a footprint onto a chart
/// with one changes the one there, in its place, instead of adding another.
#[test]
fn a_footprint_pasted_onto_a_chart_with_one_takes_its_place() {
    crate::ui::on_gtk(|| {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        window.set_indicators(vec![Indicator::new(1, IndicatorKind::Footprint), Indicator::new(2, IndicatorKind::Sma)]);
        let mut copied = Indicator::new(9, IndicatorKind::Footprint);
        if let Params::Footprint { profile, .. } = &mut copied.params {
            profile.on = true;
        }
        *window.clipboard.borrow_mut() = super::Clipboard::Indicator(copied.clone());
        assert!(window.paste());
        let after = window.indicators();
        assert_eq!(after.iter().map(|i| (i.id, i.kind)).collect::<Vec<_>>(), [(1, IndicatorKind::Footprint), (2, IndicatorKind::Sma)]);
        assert_eq!(after[0].params, copied.params, "the pasted settings, in the one there");

        // Anything else is added beside what is there, as before.
        *window.clipboard.borrow_mut() = super::Clipboard::Indicator(Indicator::new(9, IndicatorKind::Sma));
        assert!(window.paste());
        assert_eq!(window.indicators().len(), 3);
    });
}
