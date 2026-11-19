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
