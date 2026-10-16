//! The main window.
//!
//! A chart, a watchlist that can be hidden outright, and the window's own
//! controls floating over its top-right corner. The chart is the main
//! character: there is no header bar, and nothing but the drawing takes room.
//!
//! Nothing here waits on the network. Opening a symbol paints whatever the
//! cache holds immediately, asks a worker for the gap, and repaints when the
//! answer arrives.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use adw::prelude::*;
use gtk::glib;
use omacharts_engine::providers;
use omacharts_engine::refresh;
use omacharts_engine::{
    resample, BarColouring, BarStyle, Delivery, FetchFailure, Grid, Indicator, Instrument, Params, Provider, Reset,
    SearchIndex, Session,
    Timeframe,
};

use crate::at_price::AtPrice;
use crate::live::{Change, Live, Series, Watching, Written};
use crate::loader::{Loader, Request, Response, BACKFILL, BACKGROUND, FOREGROUND, REFRESH};
use crate::book_templates;
use crate::store::{Store, DEFAULT_WATCHLIST};
use crate::chart_defaults::ChartDefaults;
use crate::theming::Theming;
use crate::ui::chart::{Drawn, Echo};
use crate::ui::pane::{self, ChartPane, Hideable, LadderSource, Node};
use crate::ui::drop_line;
use crate::ui::screenshot;
use omacharts_engine::link;
use omacharts_engine::theme::StoredColouring;
use omacharts_engine::LinkMode;
use crate::ui::colors;
use crate::ui::preferences::Preferences;
use crate::ui::pick_frame::Over;
use crate::ui::search::SymbolSearch;
use crate::ui::shortcuts;
use crate::ui::watchlist::{Quote, Watchlist, DEFAULTS};
use gtk::gio;

mod log;
mod marks;
mod trade;
mod updates;
mod about;
use trade::Trade;

/// How often the rail's missing prices are looked for.
///
/// Not a pacing knob — the provider owns pacing, and this never fetches. It is
/// how long a symbol added from a terminal, or a section just expanded, waits
/// to be noticed, and how long after a 429 the backfill offers itself again.
/// A minute is below anyone's patience for a dash filling in and far above the
/// cost of the `coverage` queries it takes to find out there is nothing to do.
const BACKFILL_POLL_SECONDS: u32 = 60;

/// How far before its first bar a stitched chart scrolled back reads at a
/// time: a quarter's month and some, intraday; five years, daily.
const OLDER_INTRADAY_DAYS: i64 = 100;
const OLDER_DAILY_DAYS: i64 = 5 * 365;

/// Most batches of closes one rail refresh sends: two hundred rows at
/// Yahoo's twenty a request. Not a budget — the loader paces them like any
/// other work nobody is waiting on — but a list longer than this is split,
/// and a split list keeps falling due in halves a minute apart.
const RAIL_MAX_BATCHES: usize = 10;

/// How often we ask which charts on screen have gone stale.
///
/// Not the refresh period — that belongs to each chart and comes from its
/// resolution, see [`omacharts_engine::refresh::period`]. This is only how
/// finely the question is asked, and it is half the shortest period any chart
/// can have so that a one-minute chart is not held to two minutes by the
/// timer's own phase. A tick with nothing to do is a handful of `coverage`
/// queries against a local, indexed, write-ahead-logged table.
const REFRESH_TICK_SECONDS: u32 = 30;

/// Does a provider that delivers bars this way need the refresh timer at all?
///
/// A provider that streams has nothing to poll, so there is nothing to time:
/// not a tick every thirty seconds, not the `coverage` queries a tick makes
/// to find out it has nothing to do. Asked once, when the window is built,
/// because how a provider delivers bars does not change while it is open.
/// [`omacharts_engine::refresh::due`] refuses a streamed chart too, so a
/// caller that reaches it anyway still gets the right answer; this is the
/// line that keeps the question from being asked in the first place.
fn wants_refresh_timer(delivery: Delivery) -> bool {
    delivery == Delivery::Polled
}

/// Is a rail row worth fetching daily bars for again, given how the provider
/// delivers and whether the cache already holds any?
///
/// On a polled feed, always: "stale" is a thing a polled feed has, and the
/// rail's own refresh and the bar widget's are how a row keeps up — whether
/// it is due is theirs to decide, see [`crate::cli::plan_refresh`]. On a
/// streamed feed, only a
/// row with nothing cached at all, once — the stream keeps the charted
/// symbols current, and a row that is not charted keeps the quote it was
/// first fetched with. Fetching it again on the widget's timer would be
/// polling the provider behind a path that claims not to, which is the one
/// thing a streamed feed must never do.
fn worth_warming(delivery: Delivery, cached: bool) -> bool {
    delivery == Delivery::Polled || !cached
}

/// What a chart's header says about where its bars come from: the feed
/// serving it, then each it was passed over and why.
/// How often we look for a desktop theme change. Cheap enough to be invisible,
/// often enough to feel immediate.
const THEME_POLL_SECONDS: u32 = 2;

/// How long a wake from a stream waits for a frame before the series is
/// brought up to date without one.
///
/// Longer than any frame interval, so a window on screen always drains on
/// its frame clock and this never fires there; short enough that a window
/// nobody is looking at is never more than this behind the feed. Not a
/// timer in its own right: it is armed by a tick and runs once.
const HIDDEN_DRAIN_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

/// Read the stored resolution strip.
///
/// A strip that was never set gets the presets; one that was set to nothing
/// stays set to nothing. Clearing it is a choice — typing a resolution works
/// whether or not it is on the strip — and a list that refills itself the next
/// time the app starts is a setting that does not hold.
fn parse_timeframes(stored: Option<&str>) -> Vec<Timeframe> {
    let Some(stored) = stored else {
        return Timeframe::PRESETS.to_vec();
    };
    let mut listed: Vec<Timeframe> =
        stored.split(',').filter_map(|k| Timeframe::parse(k.trim())).collect();
    listed.sort_by_key(|t| t.seconds());
    listed.dedup();
    listed
}

fn format_timeframes(listed: &[Timeframe]) -> String {
    listed.iter().map(|t| t.key()).collect::<Vec<_>>().join(",")
}

/// Every shortcut the sheet lists, grouped the way it lists them.
///
/// Out here rather than inside the sheet so that the filtering the box at
/// the top does can be tested against the rows people actually read. The key
/// text is written the way the sheet prints it, glyphs and all; what somebody
/// types instead of a glyph is `key_synonyms`' problem.
const SHORTCUT_SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Finding things",
        &[
            ("Type a letter", "Find a symbol"),
            ("Type a number", "Set the resolution"),
            ("Ctrl+K", "Find a symbol"),
            ("Ctrl+I", "Indicators"),
            ("Ctrl+Shift+I", "Add an indicator"),
            ("Ctrl+Shift+S", "Chart settings"),
            ("Ctrl+,", "Preferences"),
            ("Ctrl+Shift+M", "Show or hide the Log"),
            ("F10", "Main menu"),
            ("? · Ctrl+?", "This list"),
            ("Ctrl+Q", "Quit"),
        ],
    ),
    (
        "Chartbooks",
        &[
            ("Ctrl+N", "New chartbook"),
            ("Ctrl+Shift+R", "Rename this chartbook"),
            ("Ctrl+Shift+Alt+X", "Remove this chartbook"),
            ("Ctrl+Alt+O", "Screenshot this chartbook"),
            ("Ctrl+Alt+← →", "Previous or next chartbook"),
            ("Double-click a tab", "Rename it"),
            ("Right-click a tab", "Rename, duplicate or remove it, or its chartbook template"),
        ],
    ),
    (
        "Watchlist",
        &[
            ("Ctrl+Shift+B", "Show or hide the watchlist"),
            ("↑ ↓", "Next or previous symbol"),
            ("Ctrl+↑ ↓", "Next or previous section"),
            ("Enter", "Fold or unfold a section"),
            // The one binding in this grid that is not global, so the
            // row says where it works: pressed over a chart it does
            // nothing, and nothing is hard to ask a question about.
            ("Ctrl+Alt+Shift+↑ ↓", "Previous or next watchlist, in the sidebar"),
            ("Ctrl+Shift+1 … 9", "Watchlist N, in the switcher's order"),
            ("Ctrl+N", "Add a symbol, in the sidebar"),
            ("Ctrl+Shift+N", "Add a section, in the sidebar"),
            ("Delete", "Remove the symbol"),
        ],
    ),
    (
        "Charts",
        &[
            ("Ctrl+Alt+H", "Split horizontally"),
            ("Ctrl+Alt+V", "Split vertically"),
            ("Ctrl+Alt+X", "Close this chart"),
            ("Ctrl+M", "Give this chart the window, or put it back"),
            ("Ctrl+L  Ctrl+Shift+L", "Next or previous link mode"),
            ("Ctrl+O", "Screenshot this chart"),
            ("Alt+← →", "Focus the previous or next chart, row by row"),
            ("Alt+↑ ↓", "Focus the chart above or below"),
            ("Ctrl+Alt+Shift+← → ↑ ↓", "Resize this chart"),
        ],
    ),
    (
        "Chart",
        &[
            ("Ctrl+Shift+← →", "Previous or next resolution"),
            ("↑ ↓ · Ctrl+Alt+↑ ↓", "Previous or next symbol"),
            ("← →", "Pan"),
            ("+ −", "Zoom"),
            ("End", "Jump to the latest bar"),
            ("Ctrl+Esc", "Reset the view"),
            ("Ctrl+G", "Go to a stretch of time: 6m, 3y, Y, 2Y, 2025, all"),
            ("Ctrl+Shift+G", "Show or hide the gridlines"),
            ("Ctrl+H", "Show or hide the drawings"),
            ("Ctrl+Shift+H", "Show or hide the indicators"),
            ("Ctrl+Alt+Shift+H", "Hide the bars, or show them as they were"),
            ("F2", "Edit the chart title, with no drawing selected"),
            ("Ctrl+Alt+Shift+1 … 9", "Chart template N, in the order they are listed"),
            ("Ctrl+Alt+Shift+0", "No chart template"),
            ("Ctrl+Alt+Shift+PgUp PgDn", "Previous or next chart template"),
            ("Esc", "Back to the chart"),
        ],
    ),
    (
        "Drawing",
        &[
            ("Alt+L", "Draw a line: click where it starts, then where it ends"),
            ("Alt+H", "Draw a horizontal line: a level, held flat"),
            ("Alt+A", "Draw an arrow"),
            ("Alt+Z", "Draw a zig-zag: click each corner, double-click or Enter to end it"),
            ("Ctrl+click a tool", "Keep it in hand for drawing after drawing; Esc puts it down"),
            ("Ctrl+X / Ctrl+C / Ctrl+V", "Cut or copy the selected drawings or indicator, and put them down again"),
            ("Alt+R", "Draw a rectangle over a run of bars"),
            ("Alt+C", "Draw a circle, dragged to any shape"),
            ("Alt+T", "Write on the chart: click, then type"),
            ("Alt+V", "Draw a volume profile of the bars dragged across"),
            ("Alt+W", "Anchor a VWAP: click the bar it starts from"),
            ("Alt+F", "Draw a Fibonacci retracement: drag from the swing's start to its end"),
            ("Alt+M", "Measure: click the start, then the end"),
            ("Ctrl+Shift+D", "Show or hide the drawing tools"),
            ("Alt+1 … 9", "Configuration N, for the selected drawing or the one about to be drawn"),
            ("Enter / F2", "Put the caret in the selected drawing's text"),
            ("Alt+Enter", "The selected drawing's or indicator's properties"),
            ("Double-click", "Put the caret in a drawing and type on it"),
            ("Ctrl++ / Ctrl+−", "Grow or shrink the selected drawing's text"),
            ("Shift+Enter", "A new line, while typing on the chart"),
            ("Ctrl+B / Ctrl+I", "Bold or italic, over what is selected while typing"),
            ("Alt+Shift+← → ↑ ↓", "Put a selected drawing's label against that edge"),
            ("Alt+Shift+.", "Put it back in the middle"),
            ("Alt+Shift+A", "Step the selected drawing's arrowhead: none, filled, open, barb"),
            ("Alt+Shift++ / Alt+Shift+−", "Thicken or thin the selected drawings' lines or edges, or the selected indicator's line"),
            ("Enter", "The selected drawing's properties"),
            ("← → ↑ ↓", "Nudge the selected drawing a pixel; ten with Shift"),
            ("Ctrl+Shift+↑ ↓", "Bring the selected drawing to the front, or send it to the back"),
            ("Ctrl+Z · Ctrl+Y", "Undo and redo, on this chart's drawings"),
            ("Click a drawing", "Select it; drag an end, or the whole thing"),
            ("Shift+click a drawing", "Add it to the selection, or take it out; Ctrl does the same"),
            ("Shift+drag the chart", "Select every drawing the box touches"),
            ("Ctrl+A", "Select every drawing on the plot"),
            ("Right-click a drawing", "Its colour and thickness, or delete it"),
            ("Delete", "Delete the selected drawings"),
            ("Esc", "Put the tool down, or let go of the selection"),
        ],
    ),
];

/// The trading keys, which the sheet lists only once there is a trading
/// login: with none they do nothing.
const TRADING_SHORTCUTS: (&str, &[(&str, &str)]) = (
    "Trading",
    &[
        ("Ctrl+B", "Buy the chart's quantity at market"),
        ("Ctrl+S", "Sell the chart's quantity at market"),
        ("Ctrl+F", "Flatten the chart's contract and cancel its orders"),
        ("Ctrl+1 … 9", "Pick the quantity, 1st to 9th"),
        ("Ctrl+Shift+A", "Show or hide the accounts"),
        ("Ctrl+Shift+P", "Show or hide the positions"),
        ("Ctrl+Shift+O", "Show or hide the orders"),
    ],
);

/// Whether one shortcut row answers what was typed into the sheet's box.
///
/// Both halves of a row are searched, because people arrive holding either
/// one: somebody after the split types "split", and somebody who half
/// remembers the chord types "ctrl shift o". The section's title counts as
/// part of every row in it, so "watchlist" finds the rail's keys on the rows
/// whose own description never says the word.
///
/// Every word typed has to be found somewhere in the row, in any order:
/// "shift ctrl o" and "Ctrl+Shift+O" are the same question asked twice. A word
/// matches a key it is the start of — "del" finds Delete — or a word of the
/// description it is the start of, and from three characters anywhere inside
/// the description as well, which is what makes "list" find a watchlist.
///
/// A single character is only ever a key. "ctrl shift o" would otherwise drag
/// in every row whose description contains the word "or", which is most of the
/// rows that begin with Ctrl+Shift — the exact pile the chord was typed to get
/// out of.
fn shortcut_matches(section: &str, keys: &str, what: &str, query: &str) -> bool {
    let typed = chord_parts(query);
    if typed.is_empty() {
        return true;
    }
    let keys = chord_parts(keys);
    let described = format!("{what} {section}").to_lowercase();
    typed.iter().all(|word| {
        let key_side = keys.iter().any(|key| {
            key.starts_with(word)
                || key_synonyms(key).iter().any(|name| name.starts_with(word.as_str()))
        });
        let letters = word.chars().count();
        key_side
            || (letters > 1
                && (described.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(word))
                    || (letters >= 3 && described.contains(word))))
    })
}

/// A chord in the pieces somebody might type, however they write it.
///
/// "Ctrl+Shift+O", "ctrl shift o" and "Ctrl-Shift-O" all come out the same, so
/// the sheet does not insist on the one spelling it happens to print.
///
/// A chunk that is nothing but separators is a key rather than punctuation —
/// the zoom keys are "+" and "−" — and splitting those apart would leave those
/// rows with no key to find at all. The typographic minus is folded onto the
/// hyphen for the same reason: it is not on anybody's keyboard.
fn chord_parts(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for chunk in text.split(|c: char| c.is_whitespace() || c == '·') {
        let chunk = chunk.to_lowercase();
        if chunk.is_empty() {
            continue;
        }
        let pieces: Vec<&str> = chunk
            .split(['+', '-', '−'])
            .filter(|piece| !piece.is_empty())
            .collect();
        if pieces.is_empty() {
            parts.push(chunk.replace('−', "-"));
        } else {
            parts.extend(pieces.into_iter().map(str::to_string));
        }
    }
    parts
}

/// What somebody types for a key the sheet draws as a glyph or an
/// abbreviation.
///
/// The arrows are the ones that matter: "↑" cannot be typed, and a sheet whose
/// arrow keys are unfindable is a sheet that is missing half the chart's keys.
fn key_synonyms(part: &str) -> &'static [&'static str] {
    match part {
        "←" => &["left", "arrow"],
        "→" => &["right", "arrow"],
        "↑" => &["up", "arrow"],
        "↓" => &["down", "arrow"],
        "ctrl" => &["control"],
        "esc" => &["escape"],
        "+" => &["plus"],
        "-" => &["minus"],
        _ => &[],
    }
}

thread_local! {
    /// Built once, the first time each is wanted, like the symbol picker.
    static RANGE_CARD: std::cell::OnceCell<Rc<crate::ui::range_card::RangeCard>> = const { std::cell::OnceCell::new() };
    static RESOLUTION_CARD: std::cell::OnceCell<Rc<crate::ui::range_card::ResolutionCard>> =
        const { std::cell::OnceCell::new() };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chart of the regular session out of hours: the newest trade is one
    /// it does not draw, and the last price follows it all the same; in
    /// hours, on extended hours or on a daily chart, the last bar has it.
    #[test]
    fn the_last_price_is_the_feed_s_whatever_the_session_draws() {
        use chrono::TimeZone;
        let at = |h: u32| chrono_tz::America::New_York.with_ymd_and_hms(2024, 3, 13, h, 0, 0).single().unwrap().timestamp();
        let bar = |ts: i64, close: f64| omacharts_engine::Bar { ts, open: close, high: close, low: close, close, volume: 1.0 };
        let stock = Instrument {
            symbol: "X".into(),
            name: "X".into(),
            kind: omacharts_engine::InstrumentKind::Equity,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        };
        let (regular, extended) = (omacharts_engine::Session::Regular, omacharts_engine::Session::Extended);
        let hour = Timeframe::hours(1);
        let evening = vec![bar(at(15), 100.0), bar(at(18), 96.5)];
        assert_eq!(hidden_latest(&evening, regular, &stock, hour), Some(96.5), "out of hours");
        assert_eq!(hidden_latest(&evening, extended, &stock, hour), None, "drawn already");
        assert_eq!(hidden_latest(&evening, regular, &stock, Timeframe::days(1)), None, "a day is a session");
        let mut ticked = evening.clone();
        ticked.push(bar(at(19), 95.25));
        assert_eq!(hidden_latest(&ticked, regular, &stock, hour), Some(95.25), "and follows the next trade");
        assert_eq!(hidden_latest(&evening[..1], regular, &stock, hour), None, "in hours");
        assert_eq!(hidden_latest(&[], regular, &stock, hour), None);
    }

    /// The timer is GTK's and cannot be watched from here, so the decision
    /// that installs it is a function of its own and this holds that. A
    /// provider that streams gets no timer at all — nothing to poll, nothing
    /// to time — and the one that is polled gets it without anybody asking.
    /// The feed somebody chose comes back with the chart after a restart,
    /// and a chart that follows the order writes nothing for it, so every
    /// arrangement stored before there was a choice reads as following it.
    #[test]
    fn a_chosen_feed_survives_a_restart_and_none_is_written_for_automatic() {
        let store = Store::memory().expect("an in-memory store");
        let mut pinned = stored_pane(1, "ES", LinkMode::Focus);
        pinned.source = Some("rithmic".into());
        pinned.account = Some("1/951".into());
        let two = crate::trading::Quantity::new(2).unwrap();
        pinned.trading_size = Some(crate::trading::sizes::Picked { template: "Futures".into(), size: two, contract: Some("ES".into()) });
        let book = book_of("Futures", vec![pinned, stored_pane(2, "NQ", LinkMode::Focus)]);
        let json = serde_json::to_string(&Workspace { books: vec![book], active: 0 }).unwrap();
        assert_eq!(json.matches("\"source\"").count(), 1, "only the chosen one is written: {json}");
        store.set_setting(SETTING_WORKSPACE, &json);

        let back = parse_workspace(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap();
        let sources: Vec<Option<&str>> = back.books[0].panes.iter().map(|p| p.source.as_deref()).collect();
        assert_eq!(sources, vec![Some("rithmic"), None]);
        let old = parse_workspace(ONE_ARRANGEMENT).unwrap();
        assert!(old.books[0].panes.iter().all(|stored| stored.source.is_none()));
        // The account the same way: only a chosen one is written.
        assert_eq!(json.matches("\"account\"").count(), 1, "{json}");
        let accounts: Vec<Option<&str>> = back.books[0].panes.iter().map(|p| p.account.as_deref()).collect();
        assert_eq!(accounts, vec![Some("1/951"), None]);
        assert!(old.books[0].panes.iter().all(|pane| pane.account.is_none()));
        // And the trading size the same way.
        assert_eq!(json.matches("\"trading_size\"").count(), 1, "{json}");
        assert_eq!(back.books[0].panes[0].trading_size.as_ref().map(|picked| picked.size), Some(two));
        assert!(back.books[0].panes[1].trading_size.is_none());
        assert!(old.books[0].panes.iter().all(|pane| pane.trading_size.is_none()));
    }

    #[test]
    fn a_streamed_provider_gets_no_refresh_timer() {
        assert!(!wants_refresh_timer(Delivery::Streamed));
        assert!(wants_refresh_timer(Delivery::Polled));
    }

    /// The bar widget asks every half minute to two minutes. On a polled
    /// feed that is how rows keep up; on a streamed feed nothing may be
    /// asked for again because it is old — only a row with nothing, once.
    #[test]
    fn a_streamed_feed_warms_a_rail_row_once_and_never_because_it_is_old() {
        assert!(worth_warming(Delivery::Polled, true));
        assert!(worth_warming(Delivery::Polled, false));
        assert!(!worth_warming(Delivery::Streamed, true));
        assert!(worth_warming(Delivery::Streamed, false));
    }

    #[test]
    fn the_resolution_strip_is_ordered_by_length() {
        let listed = parse_timeframes(Some("1D,3m,4h,15"));
        assert_eq!(
            listed.iter().map(|t| t.key()).collect::<Vec<_>>(),
            vec!["3m", "15m", "4h", "1D"]
        );
    }

    #[test]
    fn a_strip_nobody_has_set_falls_back_to_the_presets() {
        assert_eq!(parse_timeframes(None), Timeframe::PRESETS.to_vec());
    }

    /// Emptying the strip is allowed and has to stick. Typing a resolution
    /// works whether or not it is listed, so a chart with no strip at all is a
    /// usable chart — and a list that refilled itself on the next launch would
    /// be a setting that does not hold.
    #[test]
    fn a_strip_emptied_on_purpose_stays_empty() {
        assert!(parse_timeframes(Some("")).is_empty());
        assert!(parse_timeframes(Some("banana,,")).is_empty());
    }

    #[test]
    fn the_strip_round_trips_and_does_not_repeat_itself() {
        let listed = parse_timeframes(Some("3m,3m,60,1h"));
        // "60" and "1h" are the same resolution.
        assert_eq!(listed.iter().map(|t| t.key()).collect::<Vec<_>>(), vec!["3m", "1h"]);
        assert_eq!(parse_timeframes(Some(&format_timeframes(&listed))), listed);
    }

    /// What the setting held before there were chartbooks: one arrangement,
    /// with no name and nothing around it.
    const ONE_ARRANGEMENT: &str = r#"{
        "layout": {"split": {"horizontal": true, "ratio": 0.4,
                             "first": {"leaf": 1}, "second": {"leaf": 2}}},
        "focused": 2,
        "panes": [
            {"id": 1, "symbol": "SPY", "suffix": null, "timeframe": "1D",
             "indicators": [], "bar_style": "candle", "session": "extended",
             "show_grid": true, "linked": true},
            {"id": 2, "symbol": "QQQ", "suffix": null, "timeframe": "5m",
             "indicators": [], "bar_style": "candle", "session": "extended",
             "show_grid": true, "linked": false}
        ]
    }"#;

    /// What a chart written with link groups becomes, read through the
    /// store the way a window or a command reads it.
    fn upgraded(json: &str, store: &Store) -> Workspace {
        let mut value: serde_json::Value = serde_json::from_str(json).expect("valid JSON");
        upgrade_link_groups(&mut value, store);
        parse_workspace(&value.to_string()).expect("the upgraded shape should parse")
    }

    fn modes(book: &Chartbook) -> Vec<LinkMode> {
        book.panes.iter().map(|pane| pane.link).collect()
    }

    /// Every chart stored before there were groups says `"linked": true`, and
    /// out of the box the default watchlist drove what that meant, so a
    /// linked chart beside it is a Follow chart and an unlinked one Focus.
    #[test]
    fn a_chart_linked_before_there_were_groups_follows_the_watchlist() {
        let store = Store::memory().expect("an in-memory store");
        let workspace = upgraded(ONE_ARRANGEMENT, &store);
        assert_eq!(modes(&workspace.books[0]), vec![LinkMode::Follow, LinkMode::Focus]);
    }

    /// Out of the box every chart was in group 1 and only the default list
    /// drove it, so every click in it moved every chart — and still does. A
    /// book beside another list was moved by none of its clicks, so its
    /// charts go back to taking one while they are focused.
    #[test]
    fn the_out_of_the_box_groups_become_follow_only_beside_the_list_that_drove_them() {
        let store = Store::memory().expect("an in-memory store");
        let energy = store.add_watchlist("Energy").expect("made");
        let pane = |id: u32| {
            format!(
                r#"{{"id": {id}, "symbol": "SPY", "timeframe": "1D", "indicators": [],
                    "bar_style": "candle", "session": "extended", "show_grid": true,
                    "linked": 1}}"#
            )
        };
        let json = format!(
            r#"{{"active": 0, "books": [
                {{"name": "Main", "layout": {{"leaf": 1}}, "focused": 1, "watchlist": 1,
                  "panes": [{}, {}]}},
                {{"name": "Energy", "layout": {{"leaf": 3}}, "focused": 3, "watchlist": {energy},
                  "panes": [{}]}}]}}"#,
            pane(1),
            pane(2),
            pane(3)
        );
        let workspace = upgraded(&json, &store);
        assert_eq!(modes(&workspace.books[0]), vec![LinkMode::Follow, LinkMode::Follow]);
        assert_eq!(modes(&workspace.books[1]), vec![LinkMode::Focus]);
    }

    /// The whole table, per book: in the group its list drives is Follow,
    /// in another group or none is Focus, and a pin is a pin.
    #[test]
    fn each_book_upgrades_against_the_group_its_own_list_drove() {
        let store = Store::memory().expect("an in-memory store");
        let fx = store.add_watchlist("FX").expect("made");
        let quiet = store.add_watchlist("Quiet").expect("made");
        store.set_setting(&format!("watchlist_link_{fx}"), "2");
        store.set_setting(&format!("watchlist_link_{}", DEFAULT_WATCHLIST), "0");
        let pane = |id: u32, linked: &str| {
            format!(
                r#"{{"id": {id}, "symbol": "EURUSD", "timeframe": "1D", "indicators": [],
                    "bar_style": "candle", "session": "extended", "show_grid": true,
                    "linked": {linked}}}"#
            )
        };
        let json = format!(
            r#"{{"active": 0, "books": [
                {{"layout": {{"leaf": 1}}, "focused": 1, "watchlist": {fx},
                  "panes": [{}, {}, {}]}},
                {{"layout": {{"leaf": 5}}, "focused": 5, "watchlist": {quiet},
                  "panes": [{}]}},
                {{"layout": {{"leaf": 6}}, "focused": 6, "watchlist": 1,
                  "panes": [{}]}}]}}"#,
            pane(1, "2"),
            pane(2, "3"),
            pane(3, "0"),
            pane(5, "2"),
            pane(6, "true"),
        );
        let workspace = upgraded(&json, &store);
        let (follow, focus) = (LinkMode::Follow, LinkMode::Focus);
        assert_eq!(modes(&workspace.books[0]), vec![follow, focus, focus]);
        assert_eq!(modes(&workspace.books[1]), vec![focus], "Quiet drove no group");
        assert_eq!(modes(&workspace.books[2]), vec![focus], "the default list was unlinked");
    }

    /// Two lists could claim one group in a database from before that was
    /// refused, and the earlier list in display order held it.
    #[test]
    fn a_group_two_lists_claimed_was_driven_by_the_earlier_one() {
        let store = Store::memory().expect("an in-memory store");
        let first = store.add_watchlist("Energy").expect("made");
        let second = store.add_watchlist("Metals").expect("made");
        store.set_setting(&format!("watchlist_link_{first}"), "4");
        store.set_setting(&format!("watchlist_link_{second}"), "4");
        let book = |list: i64, id: u32| {
            format!(
                r#"{{"layout": {{"leaf": {id}}}, "focused": {id}, "watchlist": {list},
                    "panes": [{{"id": {id}, "symbol": "CL", "timeframe": "1D",
                    "indicators": [], "bar_style": "candle", "session": "extended",
                    "show_grid": true, "linked": 4}}]}}"#
            )
        };
        let json =
            format!(r#"{{"active": 0, "books": [{}, {}]}}"#, book(first, 1), book(second, 2));
        let workspace = upgraded(&json, &store);
        assert_eq!(modes(&workspace.books[0]), vec![LinkMode::Follow]);
        assert_eq!(modes(&workspace.books[1]), vec![LinkMode::Focus]);
    }

    /// The upgrade writes the mode and drops the groups, so it happens once:
    /// a chart that has a mode is read as that mode whatever the lists'
    /// rows say later.
    #[test]
    fn an_upgraded_chart_is_not_upgraded_again() {
        let store = Store::memory().expect("an in-memory store");
        let mut value: serde_json::Value = serde_json::from_str(ONE_ARRANGEMENT).unwrap();
        assert!(upgrade_link_groups(&mut value, &store));
        assert!(value["panes"][0].get("linked").is_none(), "{value}");
        assert_eq!(value["panes"][0]["link"], "follow");
        store.set_setting(&format!("watchlist_link_{}", DEFAULT_WATCHLIST), "0");
        assert!(!upgrade_link_groups(&mut value, &store), "nothing left to upgrade");
        assert_eq!(value["panes"][0]["link"], "follow");
    }

    #[test]
    fn a_mode_survives_being_written_down() {
        for mode in omacharts_engine::link::MODES {
            let json = serde_json::to_string(&stored_pane(1, "SPY", mode)).unwrap();
            assert!(json.contains(&format!(r#""link":"{}""#, mode.key())), "{json}");
            assert!(!json.contains(r#""linked""#), "{json}");
            let back: StoredPane = serde_json::from_str(&json).unwrap();
            assert_eq!(back.link, mode);
        }
    }

    /// A click in the bar widget hands back a canonical ticker and a suffix,
    /// and looks for a list holding that pair. Matching on the ticker alone
    /// would find SAP in Frankfurt from a click on SAP in New York.
    #[test]
    fn a_list_holds_a_symbol_only_with_the_venue_it_was_stored_under() {
        let store = Store::memory().expect("an in-memory store");
        store.add_to_root("SAP", Some("DE"));

        assert!(list_holds(&store, DEFAULT_WATCHLIST, "SAP", Some("DE")));
        assert!(!list_holds(&store, DEFAULT_WATCHLIST, "SAP", None), "a different listing");
        assert!(!list_holds(&store, DEFAULT_WATCHLIST, "SAP.DE", None), "not the display form");
    }

    #[test]
    fn a_workspace_written_before_chartbooks_comes_back_as_one() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert_eq!(workspace.books.len(), 1);
        assert_eq!(workspace.active, 0);

        let book = &workspace.books[0];
        assert_eq!(book.focused, 2, "which chart had the keyboard survives");
        assert_eq!(book.panes.len(), 2);
        assert_eq!(book.name, "", "nothing named it, so it is still unnamed");
        // The shape, dividers and all: a layout that comes back centred is
        // not the layout anybody left.
        assert_eq!(book.layout.leaves(), vec![1, 2]);
        assert!(matches!(book.layout, Node::Split { ratio, .. } if (ratio - 0.4).abs() < 1e-9));
    }

    #[test]
    fn several_books_round_trip_through_the_setting() {
        let one = parse_workspace(ONE_ARRANGEMENT).unwrap().books.remove(0);
        let two = Chartbook {
            name: "Energy".to_string(),
            layout: Node::leaf(7),
            focused: 7,
            panes: vec![StoredPane {
                id: 7,
                title: None,
                symbol: "CL".to_string(),
                suffix: None,
                timeframe: "1h".to_string(),
                indicators: Vec::new(),
                template: None,
                bar_style: "candle".to_string(),
                bars_hidden: false,
                colouring: None,
                swapped: None,
                hollow_up: None,
                ohlc_thickness: None,
                session: "extended".to_string(),
                continuous: None,
                show_grid: true,
                link: LinkMode::Focus,
                shows_drawings: true,
                sends_drawings: true,
                shows_legend: false,
                drawing_uid: String::new(),
                drawings: Vec::new(),
                profile_merges: Vec::new(),
                source: None,
                account: None,
                trading_size: None,
                trading_symbol: None,
                trading_suffix: None,
                auto_scale: false,
                price_window: None,
                strip_windows: Default::default(),
                log_scale: true,
                indicators_shown: true,
                drawings_shown: true,
            }],
            watchlist: Some(4),
            sidebar_shown: Some(false),
            sidebar_width: Some(330),
            maximized: None,
            book_template: None,
        };
        let json =
            serde_json::to_string(&Workspace { books: vec![one, two], active: 1 }).unwrap();

        let back = parse_workspace(&json).expect("the new shape should parse");
        assert_eq!(back.active, 1);
        assert_eq!(back.books.len(), 2);
        assert_eq!(back.books[0].panes.len(), 2, "the first book kept its split");
        assert_eq!(back.books[1].name, "Energy");
        assert_eq!(back.books[1].layout.leaves(), vec![7]);
        assert!(!back.books[1].panes[0].auto_scale, "a held price scale is still held");
        assert!(back.books[1].panes[0].log_scale, "a log axis is still log");
        assert!(!back.books[0].panes[0].log_scale);
        assert!(!back.books[1].panes[0].shows_legend, "a chart with its legend off keeps it off");
    }

    /// Every chart written before the legend could be turned off drew it.
    #[test]
    fn a_chart_written_before_the_legend_switch_draws_its_legend() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert!(workspace.books[0].panes.iter().all(|pane| pane.shows_legend));
    }

    /// One chart's legend turned off, written through the store and read
    /// back as a restart reads it, while its neighbour keeps its own.
    #[test]
    fn a_legend_turned_off_survives_a_restart() {
        let store = Store::memory().expect("an in-memory store");
        let mut bare = stored_pane(1, "SPY", LinkMode::Focus);
        bare.shows_legend = false;
        let book = book_of("Macro", vec![bare, stored_pane(2, "QQQ", LinkMode::Focus)]);
        let json = serde_json::to_string(&Workspace { books: vec![book], active: 0 }).unwrap();
        store.set_setting(SETTING_WORKSPACE, &json);

        let back = parse_workspace(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap();
        let legends: Vec<bool> = back.books[0].panes.iter().map(|p| p.shows_legend).collect();
        assert_eq!(legends, vec![false, true]);
    }

    /// Every chart written before the axis could be logarithmic was linear.
    #[test]
    fn a_chart_written_before_log_scales_is_linear() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert!(workspace.books[0].panes.iter().all(|pane| !pane.log_scale));
    }

    /// Every chart written before the price scale was remembered was fitting
    /// itself — the only way off automatic was a drag that nothing stored —
    /// so that is what each of them comes back doing.
    #[test]
    fn a_chart_written_before_the_price_scale_was_remembered_fits_itself() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert!(workspace.books[0].panes.iter().all(|pane| pane.auto_scale));
    }

    /// Through the store rather than through serde alone: the setting is one
    /// string in one row, and the row is what a restart reads.
    #[test]
    fn a_held_price_scale_survives_a_restart() {
        let store = Store::memory().expect("an in-memory store");
        let mut held = stored_pane(1, "SPY", LinkMode::Focus);
        held.auto_scale = false;
        let book = book_of("Macro", vec![held, stored_pane(2, "QQQ", LinkMode::Focus)]);
        let json = serde_json::to_string(&Workspace { books: vec![book], active: 0 }).unwrap();
        store.set_setting(SETTING_WORKSPACE, &json);

        let back = parse_workspace(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap();
        let scales: Vec<bool> = back.books[0].panes.iter().map(|p| p.auto_scale).collect();
        assert_eq!(scales, vec![false, true], "each chart keeps its own answer");
    }

    /// The prices a held scale shows are written down with it and read back
    /// through the store; a chart that fits itself writes none, and one
    /// written before they were remembered has none.
    #[test]
    fn a_held_price_window_survives_a_restart() {
        let store = Store::memory().expect("an in-memory store");
        let mut held = stored_pane(1, "SPY", LinkMode::Focus);
        held.auto_scale = false;
        held.price_window = Some([412.5, 431.25]);
        let book = book_of("Macro", vec![held, stored_pane(2, "QQQ", LinkMode::Focus)]);
        let json = serde_json::to_string(&Workspace { books: vec![book], active: 0 }).unwrap();
        assert_eq!(json.matches("price_window").count(), 1, "only the held chart writes one");
        store.set_setting(SETTING_WORKSPACE, &json);

        let back = parse_workspace(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap();
        let windows: Vec<_> = back.books[0].panes.iter().map(|p| p.price_window).collect();
        assert_eq!(windows, vec![Some([412.5, 431.25]), None]);

        let old = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert!(old.books[0].panes.iter().all(|pane| pane.price_window.is_none()));
    }

    /// A chart, as a book that is not on screen holds one.
    fn stored_pane(id: u32, symbol: &str, link: LinkMode) -> StoredPane {
        StoredPane {
            id,
            title: None,
            symbol: symbol.to_string(),
            suffix: None,
            timeframe: "1D".to_string(),
            indicators: Vec::new(),
            template: None,
            bar_style: "candle".to_string(),
            bars_hidden: false,
            colouring: None,
            swapped: None,
            hollow_up: None,
            ohlc_thickness: None,
            session: "extended".to_string(),
            continuous: None,
            show_grid: true,
            link,
            shows_drawings: true,
            sends_drawings: true,
            shows_legend: true,
            drawing_uid: String::new(),
            drawings: Vec::new(),
            profile_merges: Vec::new(),
            source: None,
            account: None,
            trading_size: None,
            trading_symbol: None,
            trading_suffix: None,
            auto_scale: true,
            price_window: None,
            strip_windows: Default::default(),
            log_scale: false,
            indicators_shown: true,
            drawings_shown: true,
        }
    }

    fn book_of(name: &str, panes: Vec<StoredPane>) -> Chartbook {
        let first = panes.first().map(|pane| pane.id).unwrap_or(1);
        Chartbook {
            name: name.to_string(),
            layout: Node::leaf(first),
            focused: first,
            panes,
            watchlist: None,
            sidebar_shown: None,
            sidebar_width: None,
            maximized: None,
            book_template: None,
        }
    }

    /// The number row by position: 1 to 9 then 0, whatever it types.
    #[test]
    fn the_number_row_is_read_by_where_a_key_is() {
        assert_eq!(number_row(10), Some(1));
        assert_eq!(number_row(18), Some(9));
        assert_eq!(number_row(19), Some(0));
        assert_eq!(number_row(20), None, "the key after 0 is minus");
        assert_eq!(number_row(9), None, "escape");
    }

    /// Ctrl+Shift and the number row, as each kind of keyboard delivers it.
    #[test]
    fn a_shifted_digit_is_read_by_where_it_is_then_by_what_it_types() {
        use gtk::gdk::Key;
        // A US desk keyboard: Shift turns the row into symbols, and the
        // keycode is what says which.
        assert_eq!(shifted_number(Key::exclam, 10), Some(1));
        assert_eq!(shifted_number(Key::at, 11), Some(2));
        assert_eq!(shifted_number(Key::parenleft, 18), Some(9));
        // AZERTY, which types the digit only with Shift: both agree.
        assert_eq!(shifted_number(Key::_1, 10), Some(1));
        // Spanish, whose Shift+3 is a middle dot: the position still says 3.
        assert_eq!(shifted_number(Key::periodcentered, 12), Some(3));
        // A virtual keyboard puts every key at one code off the row; the
        // digit it typed is all there is to go on.
        assert_eq!(shifted_number(Key::_4, 9), Some(4));
        // Neither: a letter off the row is no number at all.
        assert_eq!(shifted_number(Key::B, 56), None);
        assert_eq!(shifted_number(Key::exclam, 9), None);
    }

    /// None and two templates: stepping goes round both ways.
    #[test]
    fn stepping_through_templates_wraps_at_either_end() {
        assert_eq!(step_round(0, 2, 1), 1);
        assert_eq!(step_round(2, 2, 1), 0, "past the last is None");
        assert_eq!(step_round(0, 2, -1), 2, "before None is the last");
        assert_eq!(step_round(0, 0, 1), 0, "with no templates, None stays None");
    }

    /// A chart written down before there were templates keeps its own
    /// indicators: nothing on screen changes for anybody upgrading.
    #[test]
    fn a_chart_written_before_templates_is_on_none_and_one_since_keeps_its_template() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert!(workspace.books[0].panes.iter().all(|pane| pane.template.is_none()));

        let store = Store::memory().expect("an in-memory store");
        let mut held = stored_pane(1, "SPY", LinkMode::Focus);
        held.template = Some(3);
        let book = book_of("Macro", vec![held]);
        let json = serde_json::to_string(&Workspace { books: vec![book], active: 0 }).unwrap();
        store.set_setting(SETTING_WORKSPACE, &json);
        let back = parse_workspace(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap();
        assert_eq!(back.books[0].panes[0].template, Some(3));
    }

    /// A book written before chartbook templates is on none, and a book on
    /// none is written down without the field at all, so it is stored
    /// exactly as it was before there were any.
    #[test]
    fn a_book_written_before_chartbook_templates_is_on_none_and_stays_unchanged() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert_eq!(workspace.books[0].book_template, None);
        let json = serde_json::to_string(&workspace.books[0]).unwrap();
        assert!(!json.contains("book_template"), "{json}");

        let mut linked = book_of("Macro", vec![stored_pane(1, "SPY", LinkMode::Focus)]);
        linked.book_template = Some(4);
        let back: Chartbook = serde_json::from_str(&serde_json::to_string(&linked).unwrap()).unwrap();
        assert_eq!(back.book_template, Some(4));
    }

    /// What the window does as it writes down a change to the book on
    /// screen: the books it holds as data, the ones not on screen, take the
    /// change and read back as books.
    #[test]
    fn a_change_to_the_book_on_screen_reaches_the_books_that_are_not() {
        let store = Store::memory().expect("an in-memory store");
        let mut open = book_of(
            "Open",
            vec![stored_pane(1, "SPY", LinkMode::Focus), stored_pane(2, "QQQ", LinkMode::Follow)],
        );
        open.layout = Node::leaf(1).split(1, 2, true);
        let mut closed = book_of(
            "Closed",
            vec![stored_pane(7, "CL", LinkMode::Pinned), stored_pane(8, "NG", LinkMode::Focus)],
        );
        closed.layout = Node::leaf(7).split(7, 8, true);
        let shape = book_templates::capture(&serde_json::to_value(&open).unwrap()).unwrap();
        let id = book_templates::create(&store, "Pair", shape).id;
        open.book_template = Some(id);
        closed.book_template = Some(id);

        // A divider dragged, and an RSI added to the second chart.
        let before = open.layout.leaves();
        open.layout = open.layout.with_ratio(&[], 0.3);
        open.panes[1].indicators = vec![Indicator::new(1, omacharts_engine::IndicatorKind::Rsi)];
        open.panes[1].timeframe = "4h".to_string();
        let mut books: Vec<serde_json::Value> =
            [&open, &closed].iter().map(|b| serde_json::to_value(b).unwrap()).collect();
        assert_eq!(book_templates::carry(&store, &mut books, 0, &before), 1);

        let back: Vec<Chartbook> =
            books.into_iter().map(|b| serde_json::from_value(b).unwrap()).collect();
        let closed = &back[1];
        assert!(matches!(closed.layout, Node::Split { ratio, .. } if (ratio - 0.3).abs() < 1e-9));
        let second = closed.panes.iter().find(|p| p.id == 8).unwrap();
        assert_eq!(second.indicators.len(), 1);
        assert_eq!(second.timeframe, "4h", "the resolution did not reach the book off screen");
        assert_eq!(second.symbol, "NG", "a symbol is the book's own");
        assert_eq!(second.link, LinkMode::Focus, "a link mode is the book's own");
        assert_eq!(closed.panes.iter().find(|p| p.id == 7).unwrap().link, LinkMode::Pinned);
    }

    /// Something may well have been filling the window when one of these was
    /// written, but nothing wrote it down, so nothing is.
    #[test]
    fn a_book_written_before_the_fill_was_remembered_comes_back_showing_everything() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        assert_eq!(workspace.books[0].maximized, None);
    }

    /// Which chart fills the window belongs to the book it is in, so two
    /// books each keep their own answer and a restart finds both.
    #[test]
    fn the_chart_filling_the_window_survives_a_restart_with_its_book() {
        let store = Store::memory().expect("an in-memory store");
        let mut macro_book = book_of(
            "Macro",
            vec![stored_pane(1, "SPY", LinkMode::Focus), stored_pane(2, "QQQ", LinkMode::Focus)],
        );
        macro_book.layout = Node::leaf(1).split(1, 2, true);
        macro_book.maximized = Some(2);
        let energy = book_of("Energy", vec![stored_pane(3, "CL", LinkMode::Focus)]);
        let workspace = Workspace { books: vec![macro_book, energy], active: 0 };
        store.set_setting(SETTING_WORKSPACE, &serde_json::to_string(&workspace).unwrap());

        let back = parse_workspace(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap();
        assert_eq!(back.books[0].maximized, Some(2));
        assert_eq!(back.books[1].maximized, None, "the other book is left as it was");
    }

    /// Whatever a test wants to spread. Only the ticker and the suffix are read
    /// by anything here; the rest is what the inventory would have filled in.
    fn any_instrument(symbol: &str, suffix: Option<&str>) -> Instrument {
        Instrument {
            symbol: symbol.to_string(),
            name: symbol.to_string(),
            kind: omacharts_engine::InstrumentKind::Equity,
            suffix: suffix.map(str::to_string),
            currency: Some("USD".to_string()),
            tier: 2,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    /// A stored chart naming a listing outside the curated half, saved again
    /// before the long tail has loaded, has to come back out as itself.
    ///
    /// This is the data loss: the window is built on the curated half, putting
    /// the saved charts back is the first thing it does, and the catalogue
    /// cannot arrive until the constructor has returned — so restoration never
    /// resolves one of these. Writing it down as nothing then replaced
    /// somebody's chart with an empty symbol on the next save, and `chart list`
    /// flushes before it reads, so asking what was on screen was enough.
    #[test]
    fn a_symbol_the_curated_half_cannot_name_survives_being_saved() {
        let stored = stored_pane(1, "FOXF", LinkMode::Follow);
        let curated = crate::inventory::Inventory::curated();
        assert!(
            curated.find(&stored.symbol, stored.suffix.as_deref()).is_none(),
            "FOXF is in the generated listings, which is the premise of this test"
        );

        // What the chart is left holding when the index cannot name it.
        let pending = Some((stored.symbol.clone(), stored.suffix.clone()));

        assert_eq!(
            stored_symbol(None, pending.as_ref()),
            ("FOXF".to_string(), None),
            "the saved symbol was written over with an empty one"
        );
    }

    /// The venue travels with the ticker, or a listing abroad comes back as
    /// the one in New York that shares its name.
    #[test]
    fn the_venue_is_remembered_with_the_symbol() {
        let pending = Some(("SAP".to_string(), Some("DE".to_string())));
        assert_eq!(
            stored_symbol(None, pending.as_ref()),
            ("SAP".to_string(), Some("DE".to_string()))
        );
    }

    /// Showing a chart something clears what it was waiting for, so a chart
    /// somebody pointed elsewhere while the listings were loading is stored as
    /// what it shows. A chart that never had a symbol at all still stores as
    /// none — that is the one case an empty symbol is the truth.
    #[test]
    fn what_a_chart_shows_beats_what_it_was_waiting_for() {
        let pending = Some(("FOXF".to_string(), None));
        assert_eq!(
            stored_symbol(Some(&any_instrument("NVDA", None)), pending.as_ref()),
            ("NVDA".to_string(), None)
        );
        assert_eq!(stored_symbol(None, None), (String::new(), None));
    }

    /// The first thing anybody ever sees. It used to be a hardcoded index that
    /// was not in the watchlist beside it, so a brand new install opened on a
    /// symbol its own rail never mentioned.
    #[test]
    fn a_first_launch_opens_on_the_top_of_the_watchlist() {
        let store = Store::memory().expect("an in-memory store");
        store.seed_watchlist_if_empty(crate::ui::watchlist::DEFAULTS);

        let wanted = opening_candidates(None, &store.watchlist());

        assert_eq!(wanted.first().map(|(s, _)| s.as_str()), Some("SPY"));
    }

    /// The root section sorts before the named ones, so a symbol sitting
    /// outside any section is what the rail draws at the top — and that is what
    /// "the first symbol" has to mean, or the app opens on something other than
    /// the row the user is looking at.
    #[test]
    fn the_first_symbol_is_the_one_the_rail_draws_first() {
        let store = Store::memory().expect("an in-memory store");
        store.seed_watchlist_if_empty(crate::ui::watchlist::DEFAULTS);
        store.add_to_root("MU", None);

        let wanted = opening_candidates(None, &store.watchlist());

        assert_eq!(
            wanted.first().map(|(s, _)| s.as_str()),
            Some("MU"),
            "the root's entries come before any section's"
        );
    }

    /// Restoring the last state is what every launch but the first does, and
    /// nothing about seeding a first one is allowed to disturb it.
    #[test]
    fn the_symbol_last_looked_at_wins_over_the_watchlist() {
        let store = Store::memory().expect("an in-memory store");
        store.seed_watchlist_if_empty(crate::ui::watchlist::DEFAULTS);

        let stored = Some(("SAP".to_string(), Some("DE".to_string())));
        let wanted = opening_candidates(stored, &store.watchlist());

        assert_eq!(wanted[0], ("SAP".to_string(), Some("DE".to_string())));
        assert_eq!(wanted[1].0, "SPY", "and the watchlist is what it falls back to");
    }

    /// A watchlist can be emptied — somebody can delete every row — and a
    /// first launch into a blank window reads as an app that failed to start.
    #[test]
    fn an_empty_watchlist_still_leaves_something_to_open_on() {
        let store = Store::memory().expect("an in-memory store");

        let wanted = opening_candidates(None, &store.watchlist());

        assert_eq!(wanted, vec![(LAST_RESORT_SYMBOL.to_string(), None)]);
    }

    /// Every candidate can fail to resolve: an inventory changes under people,
    /// and a row naming a listing that has gone must be stepped over rather
    /// than end the search.
    #[test]
    fn a_symbol_that_no_longer_resolves_is_stepped_over_rather_than_fatal() {
        let store = Store::memory().expect("an in-memory store");
        store.seed_watchlist_if_empty(crate::ui::watchlist::DEFAULTS);

        let stored = Some(("NOSUCHTHING".to_string(), None));
        let wanted = opening_candidates(stored, &store.watchlist());
        let index = crate::inventory::Inventory::curated();
        let found = wanted
            .iter()
            .find_map(|(symbol, suffix)| index.find(symbol, suffix.as_deref()));

        assert_eq!(found.map(|i| i.symbol), Some("SPY".to_string()));
    }

    /// A book is what you were looking at, and half of that is the list
    /// beside the charts. Coming back to an arrangement of energy charts
    /// next to yesterday's list is the same surprise as coming back to the
    /// wrong charts.
    #[test]
    fn a_book_remembers_the_rail_it_was_left_with() {
        let one = parse_workspace(ONE_ARRANGEMENT).unwrap().books.remove(0);
        let json = serde_json::to_string(&Workspace { books: vec![one], active: 0 }).unwrap();
        let mut book = parse_workspace(&json).unwrap().books.remove(0);
        book.watchlist = Some(4);
        book.sidebar_shown = Some(false);
        book.sidebar_width = Some(330);

        let json = serde_json::to_string(&Workspace { books: vec![book], active: 0 }).unwrap();
        let back = &parse_workspace(&json).expect("a book with a rail should parse").books[0];
        assert_eq!(back.watchlist, Some(4), "which list it was showing");
        assert_eq!(back.sidebar_shown, Some(false), "whether it was open at all");
        assert_eq!(back.sidebar_width, Some(330), "and how wide");
    }

    /// Every book written before the rail belonged to one says nothing about
    /// it, and has to come back saying nothing rather than failing to come
    /// back — a missing field is not a reason to lose somebody's charts.
    #[test]
    fn a_book_written_before_the_rail_belonged_to_it_still_loads() {
        let workspace = parse_workspace(ONE_ARRANGEMENT).expect("the old shape should parse");
        let book = &workspace.books[0];
        assert_eq!(book.panes.len(), 2, "the charts come back either way");
        // None rather than a default, so the fallback can be the window's own
        // settings rather than a number invented here.
        assert_eq!(book.watchlist, None);
        assert_eq!(book.sidebar_shown, None);
        assert_eq!(book.sidebar_width, None);
    }

    /// The rail stops at its ceiling on a wide window, at what leaves the
    /// charts their floor on a narrow one, and at its own floor on one too
    /// narrow for both.
    #[test]
    fn the_rail_cannot_take_the_window_from_the_charts() {
        assert_eq!(sidebar_room(1914), MAX_SIDEBAR_WIDTH);
        assert_eq!(sidebar_room(957), 957 - MIN_CHARTS_WIDTH);
        assert_eq!(sidebar_room(500), MIN_SIDEBAR_WIDTH);
    }

    /// The two shapes have to be told apart by what is in them, because both
    /// arrive as the same string from the same setting.
    #[test]
    fn neither_stored_shape_can_be_read_as_the_other() {
        let new = serde_json::to_string(&Workspace {
            books: vec![parse_workspace(ONE_ARRANGEMENT).unwrap().books.remove(0)],
            active: 0,
        })
        .unwrap();
        assert!(serde_json::from_str::<Chartbook>(&new).is_err(), "no layout at the top");
        assert!(serde_json::from_str::<Workspace>(ONE_ARRANGEMENT).is_err(), "no books");
    }

    /// A window with nothing written down, and one with something unreadable,
    /// both have to end up with a chart rather than with half of one.
    /// What this build cannot read of a stored chart or book is left out,
    /// and only that: a field, an entry of a list or a map in it, or the
    /// book holding a chart it cannot be rebuilt without. A field it has
    /// never heard of is passed over.
    #[test]
    fn what_will_not_read_is_left_out_and_only_that() {
        let mut value: serde_json::Value = serde_json::from_str(ONE_ARRANGEMENT).unwrap();
        let mut second = value.clone();
        let charts = value["panes"].as_array_mut().unwrap();
        charts[0]["strip_windows"] = serde_json::json!({ "7": "x", "8": { "kind": "cvd", "window": [1.0, 2.0] }, "9": [3.0, 4.0] });
        charts[0]["log_scale"] = serde_json::json!("yes");
        charts[0]["indicators"] = serde_json::json!([Indicator::new(4, omacharts_engine::IndicatorKind::Rsi), { "kind": "from-the-future" }]);
        charts[0]["a_field_from_a_later_build"] = serde_json::json!({ "anything": [1, 2] });
        charts[1]["strip_windows"] = serde_json::json!("garbage");
        second["panes"][1]["symbol"] = serde_json::json!(5);
        second["name"] = serde_json::json!("Broken");
        let workspace = serde_json::json!({ "books": [value, second], "active": 0 });

        let read = parse_workspace(&workspace.to_string()).expect("read but for what will not");
        assert_eq!(read.books.len(), 1, "the book with a chart it cannot rebuild goes, and only it");
        let [one, two] = &read.books[0].panes[..] else { panic!("both charts") };
        assert_eq!((one.symbol.as_str(), two.symbol.as_str()), ("SPY", "QQQ"));
        assert_eq!(
            one.strip_windows.iter().map(|(id, strip)| (*id, strip.clone())).collect::<Vec<_>>(),
            vec![
                (8, StoredStrip { kind: Some(omacharts_engine::IndicatorKind::Cvd), window: [1.0, 2.0] }),
                (9, StoredStrip { kind: None, window: [3.0, 4.0] }),
            ]
        );
        assert!(!one.log_scale, "read as one written without it");
        assert_eq!(one.indicators.iter().map(|indicator| indicator.id).collect::<Vec<_>>(), vec![4]);
        assert!(two.strip_windows.is_empty());
        assert_eq!(two.timeframe, "5m", "the rest of the chart read");
    }

    #[test]
    fn nonsense_in_the_setting_restores_nothing() {
        assert!(parse_workspace("").is_none());
        assert!(parse_workspace("{}").is_none());
        assert!(parse_workspace(r#"{"books": [], "active": 0}"#).is_none());
    }

    /// A chartbook with nothing in it but the list beside it: enough for the
    /// questions about who is using which watchlist.
    fn book(watchlist: Option<i64>) -> Chartbook {
        Chartbook {
            name: String::new(),
            layout: Node::leaf(1),
            focused: 1,
            panes: Vec::new(),
            watchlist,
            sidebar_shown: None,
            sidebar_width: None,
            maximized: None,
            book_template: None,
        }
    }

    /// A watchlist belongs to one chartbook, and the switcher has to be able to
    /// say which — a row that only refuses leaves nothing to understand.
    #[test]
    fn a_watchlist_another_chartbook_is_using_is_spoken_for() {
        let books = vec![book(Some(7)), book(Some(8))];

        assert_eq!(book_using(&books, 0, 8), Some(1), "the second book has it");
        assert_eq!(book_using(&books, 1, 7), Some(0), "and the first has the other");
        assert_eq!(book_using(&books, 0, 9), None, "a list nobody is using is free");
    }

    /// The list the rail is showing is the active chartbook's by definition, so
    /// the book on screen is never told its own list is taken — not even where
    /// a second book claims it as well, which nothing stops a hand-edited
    /// workspace from saying.
    #[test]
    fn a_chartbook_is_never_told_its_own_list_is_in_use() {
        let books = vec![book(Some(7)), book(Some(7))];

        assert_eq!(book_using(&books, 0, 7), Some(1), "somebody else does claim it");
        assert_eq!(book_using(&books, 1, 7), Some(0), "whichever way round you ask");
    }

    /// The default list is the one that cannot be deleted and the one every
    /// fallback lands on. Letting one chartbook keep it would leave a second
    /// book with nowhere at all to be.
    #[test]
    fn the_default_watchlist_is_every_chartbooks_to_show() {
        let books = vec![book(Some(DEFAULT_WATCHLIST)), book(Some(DEFAULT_WATCHLIST))];

        assert_eq!(book_using(&books, 1, DEFAULT_WATCHLIST), None);
        assert_eq!(book_using(&books, 0, DEFAULT_WATCHLIST), None);
    }

    /// Dragging a tab must not switch chartbooks: the charts on screen stay
    /// where they are, and only the order of the strip changes.
    #[test]
    fn the_chartbook_on_screen_travels_with_its_tab() {
        // Three books, looking at the first, dragged to the end.
        assert_eq!(shifted(0, 0, 2), 2, "the one you dragged is the one you are looking at");
        // Looking at the last, dragging the first past it.
        assert_eq!(shifted(2, 0, 2), 1, "it moved up one as the other left");
        // Looking at the middle, dragging the last to the front.
        assert_eq!(shifted(1, 2, 0), 2);
        // Dragging something that was already past you, and stays past you.
        assert_eq!(shifted(0, 1, 2), 0, "nothing to do with you");
    }

    /// Every book has to come out of a reorder somewhere, exactly once: a
    /// mapping that collided would put two tabs on one chartbook.
    #[test]
    fn a_reorder_lands_every_chartbook_in_its_own_place() {
        for count in 1..=5 {
            for from in 0..count {
                for to in 0..count {
                    let mut landed: Vec<usize> =
                        (0..count).map(|active| shifted(active, from, to)).collect();
                    landed.sort();
                    assert_eq!(
                        landed,
                        (0..count).collect::<Vec<_>>(),
                        "{count} books, {from} -> {to}"
                    );
                }
            }
        }
    }

    /// Or the switcher slowly fills with rows held by chartbooks nobody can
    /// find. The books are the only record of who is using what, so removing
    /// one is all it takes.
    #[test]
    fn removing_a_chartbook_frees_the_watchlist_it_was_using() {
        let mut books = vec![book(Some(7)), book(Some(8))];
        assert_eq!(book_using(&books, 0, 8), Some(1));

        books.remove(1);
        assert_eq!(book_using(&books, 0, 8), None);
    }

    /// Every row the sheet would show for what was typed, as "keys — what".
    fn found(query: &str) -> Vec<String> {
        SHORTCUT_SECTIONS
            .iter()
            .flat_map(|(section, rows)| {
                rows.iter()
                    .filter(move |(keys, what)| shortcut_matches(section, keys, what, query))
                    .map(|(keys, what)| format!("{keys} — {what}"))
            })
            .collect()
    }

    /// The sheet is a reference, and a reference that hides rows the moment it
    /// is opened is a reference with a bug in it.
    #[test]
    fn an_empty_box_shows_every_row() {
        let rows: usize = SHORTCUT_SECTIONS.iter().map(|(_, rows)| rows.len()).sum();
        assert_eq!(found("").len(), rows);
        assert_eq!(found("   ").len(), rows);
    }

    #[test]
    fn what_a_key_does_finds_the_key_that_does_it() {
        assert_eq!(
            found("split"),
            vec!["Ctrl+Alt+H — Split horizontally", "Ctrl+Alt+V — Split vertically"]
        );
        assert_eq!(found("SPLIT HORI"), vec!["Ctrl+Alt+H — Split horizontally"]);
    }

    /// Nobody writes a chord the way the next person does, and somebody
    /// reaching for this box is already unsure of it.
    #[test]
    fn a_chord_is_found_however_it_is_written() {
        let one = vec!["Ctrl+Alt+O — Screenshot this chartbook"];
        let ways = ["Ctrl+Alt+O", "ctrl alt o", "alt ctrl o", "ctrl-alt-o", "CTRL+ALT+O"];
        for query in ways {
            assert_eq!(found(query), one, "{query}");
        }
    }

    /// The half-remembered chord above is the test. A lone letter read as part
    /// of a description buries that row: "o" is a word of "Previous or next
    /// resolution" and of half the other Ctrl+Shift rows.
    #[test]
    fn a_single_letter_is_only_ever_a_key() {
        assert_eq!(
            found("o"),
            vec!["Ctrl+Alt+O — Screenshot this chartbook", "Ctrl+O — Screenshot this chart"]
        );
        // Two is enough to mean a word again, and three to mean one somebody
        // only half typed — and then it finds every row that says it.
        assert_eq!(
            found("hori"),
            vec![
                "Ctrl+Alt+H — Split horizontally",
                "Alt+H — Draw a horizontal line: a level, held flat",
            ]
        );
    }

    /// Half the chart's keys are arrows, and an arrow cannot be typed.
    #[test]
    fn the_keys_drawn_as_glyphs_are_found_by_their_names() {
        assert!(found("down arrow").iter().any(|row| row.contains("Ctrl+↑ ↓")));
        assert!(found("alt left").iter().any(|row| row.contains("Alt+← →")));
        assert!(found("alt up").iter().any(|row| row.contains("Alt+↑ ↓")));
        assert_eq!(found("plus"), vec!["+ − — Zoom"]);
        assert_eq!(found("-"), vec!["+ − — Zoom"]);
        assert!(found("escape").iter().any(|row| row.contains("Esc")));
        assert!(found("control q").iter().any(|row| row.contains("Ctrl+Q")));
    }

    /// Punctuation that is the key rather than the glue between keys: these
    /// rows have nothing else to find them by.
    #[test]
    fn a_key_that_is_punctuation_is_still_searchable() {
        assert_eq!(found("?"), vec!["? · Ctrl+? — This list"]);
        assert_eq!(found(","), vec!["Ctrl+, — Preferences"]);
    }

    /// Where a key works is in the heading more often than in the row, so the
    /// heading is part of what the row says.
    #[test]
    fn a_heading_counts_as_part_of_the_rows_under_it() {
        let rail = found("watchlist");
        assert!(rail.iter().any(|row| row.contains("Next or previous symbol")));
        assert!(rail.iter().any(|row| row.contains("Ctrl+Shift+B")));
    }

    /// The gridlines have a key of their own, and the sheet is where somebody
    /// who half remembers it goes looking.
    #[test]
    fn the_gridlines_key_is_on_the_sheet() {
        let row = vec!["Ctrl+Shift+G — Show or hide the gridlines"];
        assert_eq!(found("gridlines"), row);
        assert_eq!(found("ctrl shift g"), row);
    }

    #[test]
    fn a_key_nobody_has_matches_nothing_at_all() {
        assert!(found("xyzzy").is_empty());
        // Every word has to be found, so a right query and a wrong one
        // together are still wrong.
        assert!(found("split xyzzy").is_empty());
    }
}

#[cfg(test)]
mod strip_window_tests {
    use super::*;
    use crate::ui::window::trade::tests::window;

    /// A cumulative delta's strip zoomed on its own scale comes back after a
    /// restart as it was left, and is written down again with the chart;
    /// a strip whose indicator is gone is not; and resetting the view lets
    /// go of it, so nothing is written down for it after.
    #[test]
    fn a_zoomed_strip_survives_a_restart_and_a_reset_lets_go_of_it() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let first = window(&store);
            first.save_workspace();
            let stored = || -> serde_json::Value { serde_json::from_str(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap() };
            let mut workspace = stored();
            let chart = &mut workspace["books"][0]["panes"][0];
            assert!(chart.get("strip_windows").is_none(), "nothing zoomed, nothing written");
            let cvd = Indicator::new(7, omacharts_engine::IndicatorKind::Cvd);
            chart["indicators"] = serde_json::json!([cvd]);
            chart["indicators"] = serde_json::json!([cvd, Indicator::new(8, omacharts_engine::IndicatorKind::Rsi)]);
            // The RSI's is one held for a cumulative delta, whose id a
            // template has given it: it fits itself.
            chart["strip_windows"] = serde_json::json!({
                "7": { "kind": "cvd", "window": [-1500.0, 2500.0] },
                "8": { "kind": "cvd", "window": [-1500.0, 2500.0] },
                "99": { "window": [1.0, 2.0] },
            });
            store.set_setting(SETTING_WORKSPACE, &workspace.to_string());
            drop(first);

            let again = window(&store);
            let pane = again.focused_pane();
            assert_eq!(pane.view.held_strip_windows().get(&7), Some(&(-1500.0, 2500.0)), "held as it was left");
            assert_eq!(pane.view.held_strip_windows().get(&8), None, "an RSI took a cumulative delta's range");
            again.save_workspace();
            let written = stored()["books"][0]["panes"][0]["strip_windows"].clone();
            let kept = serde_json::json!({ "7": { "kind": "cvd", "window": [-1500.0, 2500.0] } });
            assert_eq!(written, kept, "only the strips the chart has");

            // Other trading hours are another series: let go of, and so
            // written down.
            let other = match again.session() {
                Session::Regular => Session::Extended,
                _ => Session::Regular,
            };
            again.set_session(other);
            assert!(pane.view.held_strip_windows().is_empty(), "held across other hours");
            assert!(stored()["books"][0]["panes"][0].get("strip_windows").is_none(), "written down held");
            pane.view.hold_strip_window(7, Some(omacharts_engine::IndicatorKind::Cvd), -1500.0, 2500.0);
            again.save_workspace();
            assert_eq!(stored()["books"][0]["panes"][0]["strip_windows"], kept);

            // Another symbol: let go of, and written down so.
            let nq = again.index.find("NQ", None).expect("NQ");
            again.show_in(&pane, nq);
            let context = glib::MainContext::default();
            for _ in 0..400 {
                if pane.view.held_strip_windows().is_empty() {
                    break;
                }
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(pane.view.held_strip_windows().is_empty(), "held across another symbol");
            assert!(stored()["books"][0]["panes"][0].get("strip_windows").is_none(), "written down held");
            pane.view.hold_strip_window(7, Some(omacharts_engine::IndicatorKind::Cvd), -1500.0, 2500.0);

            let actions = again.trade.chart_actions.borrow().clone().expect("the chart's actions");
            actions.activate_action("reset-view", None);
            assert!(pane.view.held_strip_windows().is_empty(), "the reset let go of it");
            assert!(stored()["books"][0]["panes"][0].get("strip_windows").is_none(), "and nothing is written for it");
        });
    }

    /// A strip written down as a bare bottom and top, before its kind was
    /// kept, comes back held, for the kind of the indicator it is on, and is
    /// written again with it.
    #[test]
    fn a_strip_written_without_its_kind_comes_back_for_the_indicator_it_is_on() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let first = window(&store);
            first.save_workspace();
            let stored = || -> serde_json::Value { serde_json::from_str(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap() };
            let mut workspace = stored();
            let chart = &mut workspace["books"][0]["panes"][0];
            chart["indicators"] = serde_json::json!([Indicator::new(7, omacharts_engine::IndicatorKind::Cvd)]);
            chart["strip_windows"] = serde_json::json!({ "7": [-1500.0, 2500.0] });
            store.set_setting(SETTING_WORKSPACE, &workspace.to_string());
            drop(first);

            let again = window(&store);
            let pane = again.focused_pane();
            assert_eq!(pane.view.held_strip_windows().get(&7), Some(&(-1500.0, 2500.0)), "the old shape not read");
            assert_eq!(pane.view.held_strip_kinds().get(&7), Some(&omacharts_engine::IndicatorKind::Cvd), "held for no kind");
            again.save_workspace();
            let written = stored()["books"][0]["panes"][0]["strip_windows"].clone();
            assert_eq!(written, serde_json::json!({ "7": { "kind": "cvd", "window": [-1500.0, 2500.0] } }));
        });
    }

    /// What a chart's field this build cannot read leaves out is that field,
    /// not the chartbooks: the window opens on the arrangement written down.
    #[test]
    fn a_chart_field_that_will_not_read_leaves_the_chartbooks_open() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let first = window(&store);
            first.save_workspace();
            let stored = || -> serde_json::Value { serde_json::from_str(&store.setting(SETTING_WORKSPACE).unwrap()).unwrap() };
            let mut workspace = stored();
            workspace["books"][0]["name"] = serde_json::json!("Energy");
            let chart = &mut workspace["books"][0]["panes"][0];
            chart["symbol"] = serde_json::json!("NQ");
            chart["strip_windows"] = serde_json::json!("garbage");
            store.set_setting(SETTING_WORKSPACE, &workspace.to_string());
            drop(first);

            let again = window(&store);
            assert_eq!(again.books.borrow()[0].name, "Energy", "opened on a new chartbook");
            let pane = again.focused_pane();
            assert_eq!(pane.instrument.borrow().as_ref().map(|i| i.symbol.clone()).as_deref(), Some("NQ"));
            assert!(pane.view.held_strip_windows().is_empty());
        });
    }

    /// Resetting a chart onto other trading hours lets go of its strips held
    /// on their own scale, as choosing the hours does. Applying the defaults
    /// to every chart never touches what is kept with its indicators, so it
    /// keeps them: on screen and in a book that is not.
    #[test]
    fn resetting_a_chart_onto_other_hours_lets_go_of_its_strips_and_applying_to_all_keeps_them() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let pane = window.focused_pane();
            // A strip held is one of the chart's indicators': redrawn, a
            // window held for none of them is dropped.
            window.set_indicators_of(&pane, vec![Indicator::new(7, omacharts_engine::IndicatorKind::Cvd)]);
            window.save_workspace();
            let same = window.session();
            let other = match same {
                Session::Regular => Session::Extended,
                _ => Session::Regular,
            };
            let mut defaults = ChartDefaults::load(&store);
            defaults.template = None;
            pane.view.hold_strip_window(7, Some(omacharts_engine::IndicatorKind::Cvd), -1500.0, 2500.0);
            defaults.session = same;
            window.adopt_looks(&pane, &defaults);
            assert_eq!(pane.view.held_strip_windows().get(&7), Some(&(-1500.0, 2500.0)), "let go of on the same hours");
            defaults.session = other;
            window.adopt_looks(&pane, &defaults);
            assert_eq!(window.session(), other);
            assert!(pane.view.held_strip_windows().is_empty(), "held across other hours");

            // Apply to all: the strips stay held, on screen and where a book
            // not on screen is written.
            let elsewhere = {
                let mut book = window.books.borrow()[0].clone();
                book.name = "Elsewhere".into();
                book.panes[0].session = same.key().to_string();
                book.panes[0].strip_windows.insert(7, StoredStrip { kind: None, window: [-1.0, 1.0] });
                book
            };
            window.books.borrow_mut().push(elsewhere);
            pane.view.hold_strip_window(7, Some(omacharts_engine::IndicatorKind::Cvd), -1500.0, 2500.0);
            defaults.session = same;
            window.apply_chart_defaults_now(&defaults);
            assert_eq!(window.session(), same, "the hours are applied");
            assert_eq!(pane.view.held_strip_windows().get(&7), Some(&(-1500.0, 2500.0)), "kept on screen");
            let books = window.books.borrow();
            assert_eq!(books[1].panes[0].session, same.key(), "the hours are applied where it is written");
            assert_eq!(books[1].panes[0].strip_windows.get(&7).map(|s| s.window), Some([-1.0, 1.0]), "kept where it is written");
        });
    }

    /// A root on a feed that stitches its months is asked for joined as its
    /// chart says, which is another series to the cache: switching asks
    /// afresh, the choice comes back after a restart, a new chart takes the
    /// default, and a dated contract and a feed with nothing to choose are
    /// asked for as they always were.
    #[test]
    fn a_chart_s_choice_of_how_months_are_joined_is_what_it_asks_its_feed_for() {
        use omacharts_engine::continuous::{fake::Months, Adjustment, Continuous};
        crate::ui::on_gtk(|| {
            let _ = adw::init();
            let store = Rc::new(Store::memory().expect("a store"));
            let open = |store: &Rc<Store>| {
                let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
                let feed: Arc<dyn Provider> = Arc::new(Continuous::new(Box::new(Months::all())));
                Window::build_with(&app, store.clone(), vec![feed])
            };
            let window = open(&store);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES");
            window.show_in(&pane, es.clone());
            let key = |window: &Rc<Window>, instrument: &Instrument| {
                let pane = window.focused_pane();
                window.route_pane(&pane, instrument).expect("routed").key()
            };
            assert!(window.offers_continuous());
            assert_eq!(window.continuous(), Adjustment::BackAdjusted, "the default");
            assert_eq!(key(&window, &es), "months:ES~ESZ26~back-adjusted");
            window.set_continuous(Adjustment::AsTraded);
            assert_eq!(key(&window, &es), "months:ES~ESZ26~as-traded", "another series");
            let dated = es.in_month(omacharts_engine::contracts::Month::new(2026, 9).unwrap()).unwrap();
            assert_eq!(key(&window, &dated), "months:ESU6", "a dated contract is itself either way");
            window.save_workspace();
            drop(window);

            let again = open(&store);
            assert_eq!(again.continuous(), Adjustment::AsTraded, "kept with the chart");
            let mut defaults = ChartDefaults::load(&store);
            defaults.continuous = Adjustment::AsTraded;
            defaults.save(&store);
            let fresh = again.new_default_pane(Timeframe::days(1), Vec::new());
            assert_eq!(fresh.continuous.get(), Adjustment::AsTraded, "a new chart takes the default");
            defaults.continuous = Adjustment::BackAdjusted;
            again.adopt_looks(&again.focused_pane(), &defaults);
            assert_eq!(again.continuous(), Adjustment::BackAdjusted, "and is put back on it");

            // A feed whose own series is all there is offers nothing.
            let yahoo = window_on(&store, Arc::new(omacharts_engine::providers::Yahoo::new()));
            yahoo.show_in(&yahoo.focused_pane(), es.clone());
            assert!(!yahoo.offers_continuous());
            yahoo.set_continuous(Adjustment::AsTraded);
            assert_eq!(key(&yahoo, &es), "yahoo:ES=F", "its own series, as it is");
        });
    }

    fn window_on(store: &Rc<Store>, feed: Arc<dyn Provider>) -> Rc<Window> {
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        Window::build_with(&app, store.clone(), vec![feed])
    }
}

#[cfg(test)]
mod legend_switch_tests {
    use super::*;
    use crate::ui::window::trade::tests::window;

    /// How many indicators the chart's legend names.
    fn named(pane: &ChartPane) -> usize {
        let mut count = 0;
        let mut child = pane.indicator_legend.first_child();
        while let Some(row) = child {
            count += 1;
            child = row.next_sibling();
        }
        count
    }

    /// Three indicators, one of them left out of the legend by its own switch.
    fn three(window: &Rc<Window>) -> Rc<ChartPane> {
        let pane = window.focused_pane();
        let mut quiet = Indicator::new(3, omacharts_engine::IndicatorKind::Sma);
        quiet.in_legend = false;
        window.set_indicators_of(
            &pane,
            vec![
                Indicator::new(1, omacharts_engine::IndicatorKind::Sma),
                Indicator::new(2, omacharts_engine::IndicatorKind::Rsi),
                quiet,
            ],
        );
        pane
    }

    /// On, each indicator's own switch decides; off, the legend names none.
    #[test]
    fn turning_the_legend_off_hides_all_of_it_and_on_leaves_each_its_own_say() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let pane = three(&window);
            assert!(window.shows_legend(), "on to begin with");
            assert_eq!(named(&pane), 2, "the one out of the legend is left out");
            window.set_shows_legend(false);
            assert_eq!(named(&pane), 0, "nothing named with the legend off");
            window.set_shows_legend(true);
            assert_eq!(named(&pane), 2);
        });
    }

    /// The preferences say what a new chart opens with; a chart that says
    /// otherwise keeps its own answer, across a restart too.
    #[test]
    fn a_chart_s_own_legend_wins_over_the_default_and_survives_a_restart() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let mut defaults = ChartDefaults::load(&store);
            defaults.shows_legend = false;
            defaults.save(&store);

            let first = window(&store);
            let fresh = first.new_default_pane(Timeframe::days(1), Vec::new());
            assert!(!fresh.shows_legend.get(), "a new chart opens on the default");

            let pane = three(&first);
            first.set_shows_legend(true);
            assert_eq!(named(&pane), 2, "this chart's own answer, not the default's");
            first.save_workspace();
            drop(first);

            let again = window(&store);
            let pane = again.focused_pane();
            assert!(pane.shows_legend.get(), "the chart's own answer came back");
            assert_eq!(named(&pane), 2);

            again.set_shows_legend(false);
            again.save_workspace();
            drop(again);
            let off = window(&store);
            assert!(!off.focused_pane().shows_legend.get(), "and off came back off");
            assert_eq!(named(&off.focused_pane()), 0);
        });
    }

    /// Applying the defaults and resetting a chart put its legend back to
    /// the default, as they do its gridlines: on screen and in a book that is
    /// not.
    #[test]
    fn the_defaults_put_a_chart_s_legend_back() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            window.save_workspace();
            let pane = three(&window);
            let mut defaults = ChartDefaults::load(&store);
            defaults.template = None;
            defaults.shows_legend = false;
            window.adopt_looks(&pane, &defaults);
            assert!(!window.shows_legend());
            assert_eq!(named(&pane), 0);
            defaults.shows_legend = true;
            window.adopt_looks(&pane, &defaults);
            assert_eq!(named(&pane), 2);

            let elsewhere = {
                let mut book = window.books.borrow()[0].clone();
                book.name = "Elsewhere".into();
                book.panes[0].shows_legend = true;
                book
            };
            window.books.borrow_mut().push(elsewhere);
            defaults.shows_legend = false;
            window.apply_chart_defaults_now(&defaults);
            assert!(!window.shows_legend(), "the chart on screen");
            assert!(!window.books.borrow()[1].panes[0].shows_legend, "the book written down");
        });
    }
}

#[cfg(test)]
mod apply_to_all_tests {
    use super::*;
    use crate::ui::window::trade::tests::window;
    use omacharts_engine::IndicatorKind;

    /// Indicators of a chart's own, each with settings of its own.
    fn own() -> Vec<Indicator> {
        let mut hidden = Indicator::new(1, IndicatorKind::Sma);
        hidden.visible = false;
        let mut quiet = Indicator::new(2, IndicatorKind::Rsi);
        quiet.in_legend = false;
        vec![hidden, quiet, Indicator::new(3, IndicatorKind::Cvd)]
    }

    /// Every widget under `root`.
    fn widgets(root: gtk::Widget) -> Vec<gtk::Widget> {
        let (mut found, mut stack) = (Vec::new(), vec![root]);
        while let Some(widget) = stack.pop() {
            let mut child = widget.first_child();
            while let Some(next) = child {
                child = next.next_sibling();
                stack.push(next);
            }
            found.push(widget);
        }
        found
    }

    /// Apply to all, from Preferences, applies at once with nothing asked:
    /// every chart takes the defaults but keeps its template and its
    /// indicators as they are, on screen and in a chartbook that is not.
    #[test]
    fn applying_to_all_keeps_every_chart_s_template_and_indicators_and_asks_nothing() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let mine = own();
            let on_a = crate::templates::create_from(&store, "Momentum", vec![Indicator::new(1, IndicatorKind::Macd)]);
            let default = crate::templates::create_from(&store, "Swing", vec![Indicator::new(1, IndicatorKind::Ema)]);

            // On screen: a chart with indicators of its own.
            let pane = window.focused_pane();
            window.set_indicators_of(&pane, mine.clone());
            pane.view.hold_strip_window(3, Some(IndicatorKind::Cvd), -1500.0, 2500.0);
            window.save_workspace();

            // Not on screen: one chart on a template, one with its own.
            let elsewhere = {
                let mut book = window.books.borrow()[0].clone();
                book.name = "Elsewhere".into();
                let mut templated = book.panes[0].clone();
                templated.id += 100;
                templated.template = Some(on_a.id);
                templated.indicators = on_a.indicators.clone();
                book.panes.push(templated);
                book
            };
            window.books.borrow_mut().push(elsewhere);

            // Defaults that differ from every chart, the template among them.
            let mut defaults = ChartDefaults::load(&store);
            defaults.template = Some(default.id);
            defaults.bar_style = BarStyle::Ohlc;
            defaults.show_grid = !pane.show_grid.get();
            defaults.log_scale = !pane.view.price_log();
            defaults.shows_legend = !pane.shows_legend.get();
            defaults.save(&store);

            window.open_preferences_at(Some("defaults"));
            let preferences = window.window.visible_dialog().expect("the preferences");
            let apply = widgets(preferences.clone().upcast())
                .into_iter()
                .find_map(|widget| {
                    widget.downcast::<gtk::Button>().ok().filter(|b| b.label().as_deref() == Some("Apply to all charts now"))
                })
                .expect("the Apply to all button");
            apply.emit_clicked();

            let shown = window.window.visible_dialog().expect("the preferences, still");
            assert!(shown.downcast_ref::<adw::AlertDialog>().is_none(), "nothing is asked");
            assert_eq!(shown, preferences, "nothing over the preferences");

            // The chart on screen: its own indicators, settings and all, and
            // no template; the rest of the defaults applied.
            assert_eq!(pane.template.get(), None, "kept off the default template");
            assert_eq!(*pane.indicators.borrow(), mine, "its own indicators, as they were");
            assert_eq!(pane.view.held_strip_windows().get(&3), Some(&(-1500.0, 2500.0)), "its strip as held");
            assert_eq!(pane.bar_style.get(), BarStyle::Ohlc);
            assert_eq!(pane.show_grid.get(), defaults.show_grid);
            assert_eq!(pane.view.price_log(), defaults.log_scale);
            assert_eq!(pane.shows_legend.get(), defaults.shows_legend);

            // The chartbook not on screen, where it is written.
            let books = window.books.borrow();
            let (theirs, templated) = (&books[1].panes[0], &books[1].panes[1]);
            assert_eq!(theirs.template, None);
            assert_eq!(theirs.indicators, mine, "its own indicators, as they were");
            assert_eq!(templated.template, Some(on_a.id), "kept on its own template");
            assert_eq!(templated.indicators, on_a.indicators);
            for stored in [theirs, templated] {
                assert_eq!(stored.bar_style, BarStyle::Ohlc.key());
                assert_eq!(stored.show_grid, defaults.show_grid);
                assert_eq!(stored.log_scale, defaults.log_scale);
                assert_eq!(stored.shows_legend, defaults.shows_legend);
            }
            drop(books);
            preferences.force_close();

            // A chart on screen on a template stays on it too.
            window.set_template(Some(on_a.id));
            window.apply_chart_defaults();
            assert_eq!(pane.template.get(), Some(on_a.id), "kept on its own template");
            assert_eq!(*pane.indicators.borrow(), on_a.indicators);
        });
    }
}

#[cfg(test)]
mod tools_toggle_tests {
    use super::*;
    use crate::ui::window::trade::tests::window;

    /// How far in each chart's header starts.
    fn header_starts(window: &Window) -> Vec<i32> {
        window
            .panes
            .borrow()
            .iter()
            .map(|pane| pane.header_row.parent().expect("the legend").margin_start())
            .collect()
    }

    /// The charts that leave the start of their time axis to the toggle.
    fn taken(window: &Window) -> Vec<u32> {
        window.panes.borrow().iter().filter(|pane| pane.view.axis_start_taken() > 0.0).map(|pane| pane.id).collect()
    }

    /// Where a toggle stands in the window: the overlay it is laid in, its
    /// alignment and its margins from that overlay's edges, which are the
    /// window's own. The same answer is the same pixels on screen.
    fn place(toggle: &gtk::ToggleButton) -> (Option<gtk::Widget>, gtk::Align, gtk::Align, i32, i32, i32) {
        (
            toggle.parent(),
            toggle.halign(),
            toggle.valign(),
            toggle.margin_start(),
            toggle.margin_end(),
            toggle.margin_bottom(),
        )
    }

    /// The drawing tools' toggle stands in the window's bottom-left corner,
    /// level with the dates and centred under the column of tools, and does
    /// not move when the tools come out or go away: only its lit state
    /// changes. Put away, it is over the bottom-left chart, and that chart
    /// alone leaves the start of its dates to it; out, it is at the foot of
    /// the bar and no chart leaves anything. Every chart's header keeps its
    /// own inset either way, with one chart or four.
    #[test]
    fn the_tools_toggle_stays_still_bottom_left_and_the_headers_start_at_the_edge() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let bar = window.drawing_bar.borrow().clone().expect("the bar");
            let inset = crate::ui::pane::LEGEND_MARGIN;
            assert!(!bar.is_shown());
            assert_eq!((bar.toggle.halign(), bar.toggle.valign()), (gtk::Align::Start, gtk::Align::End));
            let overlay = bar.toggle.parent().and_downcast::<gtk::Overlay>().expect("over the bar and the charts");
            assert!(bar.root.is_ancestor(&overlay) && window.chart_host.is_ancestor(&overlay));
            assert!(window.split.is_ancestor(&overlay), "over the rail too, the window's own corners");
            let width = tools_toggle_width(&bar.toggle);
            assert_eq!(
                bar.toggle.margin_start() + width / 2,
                crate::ui::drawing_bar::COLUMN_CENTRE,
                "centred under the column of tools"
            );
            assert_eq!(header_starts(&window), vec![inset]);
            assert_eq!(taken(&window), vec![window.focused.get()]);
            assert!(bar.toggle.has_css_class("tools-toggle") && !bar.toggle.is_active());
            let shut = place(&bar.toggle);

            window.toggle_drawing_tools();
            assert!(bar.toggle.is_active(), "lit while the tools are out");
            assert_eq!(place(&bar.toggle), shut, "the tools coming out moved the toggle");
            assert_eq!(header_starts(&window), vec![inset]);
            assert!(taken(&window).is_empty(), "nothing to leave while it is over the bar");
            window.toggle_watchlist();
            assert_eq!(place(&bar.toggle), shut, "the rail moved the toggle");
            window.toggle_watchlist();
            window.toggle_drawing_tools();
            assert_eq!(place(&bar.toggle), shut, "the tools going away moved the toggle");

            // Four charts, two by two.
            let first = window.focused.get();
            window.split_focused(true);
            window.focus(first);
            window.split_focused(false);
            let right = window.layout.borrow().leaves()[2];
            window.focus(right);
            window.split_focused(false);
            assert_eq!(header_starts(&window), vec![inset; 4]);
            let leaves = window.layout.borrow().leaves();
            assert_eq!(taken(&window), vec![leaves[1]], "the bottom-left chart alone: {leaves:?}");
            assert_eq!(place(&bar.toggle), shut, "four charts moved the toggle");
            window.toggle_drawing_tools();
            assert_eq!(place(&bar.toggle), shut, "the tools out over four charts moved the toggle");
            assert_eq!(header_starts(&window), vec![inset; 4]);
            assert!(taken(&window).is_empty());
        });
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    use crate::ui::window::trade::tests::bare_window;

    /// Two charts on top and three below, by id: `[top left, top right]`
    /// and `[bottom left, bottom middle, bottom right]`.
    fn uneven(window: &Rc<Window>) -> ([u32; 2], [u32; 3]) {
        let top_left = window.focused.get();
        window.split_focused(false);
        let bottom_left = window.focused.get();
        window.split_focused(true);
        let bottom_middle = window.focused.get();
        window.split_focused(true);
        let bottom_right = window.focused.get();
        window.focus(top_left);
        window.split_focused(true);
        let top_right = window.focused.get();
        // Laid out at a real size, so the spatial keys have widgets to measure.
        window.chart_host.allocate(1200, 800, -1, None);
        ([top_left, top_right], [bottom_left, bottom_middle, bottom_right])
    }

    fn pressed(window: &Rc<Window>, from: u32, step: i32) -> u32 {
        window.focus(from);
        window.focus_along(step);
        window.focused.get()
    }

    /// Alt+Right walks every chart in reading order, whatever the rows: off
    /// the end of the top row into the bottom one, and from the last chart
    /// back to the first. Alt+Left is the same walk backwards.
    #[test]
    fn alt_left_and_right_reach_every_chart_in_reading_order() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let ([tl, tr], [bl, bm, br]) = uneven(&window);
            let order = [tl, tr, bl, bm, br];
            assert_eq!(reading_order(&window.layout_rects()), order);

            assert_eq!(pressed(&window, tr, 1), bl, "off the end of the top row");
            assert_eq!(pressed(&window, br, 1), tl, "the last wraps to the first");
            assert_eq!(pressed(&window, bl, -1), tr, "back up into the top row");
            assert_eq!(pressed(&window, tl, -1), br, "the first wraps to the last");

            window.focus(tl);
            let mut walked = vec![];
            for _ in 0..order.len() {
                window.focus_along(1);
                walked.push(window.focused.get());
            }
            assert_eq!(walked, [tr, bl, bm, br, tl]);
            for _ in 0..order.len() {
                window.focus_along(-1);
                walked.push(window.focused.get());
            }
            assert_eq!(walked[5..], [br, bm, bl, tr, tl]);
        });
    }

    /// Alt+Up and Down are still the chart that way on screen.
    #[test]
    fn alt_up_and_down_stay_spatial() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let ([tl, tr], [bl, _, br]) = uneven(&window);
            let towards = |from: u32, dy: i32| {
                window.focus(from);
                window.focus_towards(0, dy);
                window.focused.get()
            };
            assert_eq!(towards(tl, 1), bl);
            assert_eq!(towards(br, -1), tr);
            assert_eq!(towards(tl, -1), tl, "nothing above the top row");
            assert_eq!(towards(bl, 1), bl, "nothing below the bottom row");
        });
    }

    /// Maximized, the window goes with the focus: Alt+Left and Right cycle
    /// through every chart one window at a time, and Up and Down still go
    /// the way they point.
    #[test]
    fn maximized_the_window_follows_the_focus() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let ([tl, tr], [bl, bm, br]) = uneven(&window);
            window.focus(tr);
            window.toggle_maximized();
            assert_eq!(window.maximized.get(), Some(tr));
            for (step, reached) in [(1, bl), (1, bm), (1, br), (1, tl), (-1, br)] {
                window.focus_along(step);
                assert_eq!(window.focused.get(), reached);
                assert_eq!(window.maximized.get(), Some(reached), "the window stayed behind");
            }
            window.focus(tl);
            window.focus_towards(0, 1);
            assert_eq!(window.maximized.get(), Some(bl), "down, maximized");
        });
    }

    /// Rows are read by where the charts start: a tall chart belongs to the
    /// row it starts in, and two columns split at nearly the same height
    /// still read as two rows rather than a staircase.
    #[test]
    fn reading_order_reads_rows_off_the_screen() {
        // A tall chart on the left, two stacked on the right.
        let tall = HashMap::from([(1, (0.0, 0.0, 600.0, 800.0)), (2, (600.0, 0.0, 600.0, 400.0)), (3, (600.0, 400.0, 600.0, 400.0))]);
        assert_eq!(reading_order(&tall), [1, 2, 3]);
        // Columns first, each split near the middle: still row by row.
        let columns = HashMap::from([
            (1, (0.0, 0.0, 600.0, 400.0)),
            (2, (0.0, 400.0, 600.0, 400.0)),
            (3, (600.0, 0.0, 600.0, 380.0)),
            (4, (600.0, 380.0, 600.0, 420.0)),
        ]);
        assert_eq!(reading_order(&columns), [1, 3, 2, 4]);
    }
}

#[cfg(test)]
mod sidebar_tests {
    use super::*;
    use crate::ui::window::trade::tests::window;

    /// The Sidebars submenu's rows, as (label, action).
    fn sidebars(model: &gio::MenuModel) -> Option<Vec<(String, String)>> {
        for at in 0..model.n_items() {
            let label = model.item_attribute_value(at, "label", None).and_then(|v| v.str().map(str::to_string));
            if label.as_deref() == Some("Sidebars")
                && let Some(menu) = model.item_link(at, "submenu")
            {
                return Some(
                    (0..menu.n_items())
                        .map(|row| {
                            let text = |name| {
                                menu.item_attribute_value(row, name, None)
                                    .and_then(|v| v.str().map(str::to_string))
                                    .unwrap_or_default()
                            };
                            (text("label"), text("action"))
                        })
                        .collect(),
                );
            }
            if let Some(found) = model.item_link(at, "section").and_then(|inner| sidebars(&inner)) {
                return Some(found);
            }
        }
        None
    }

    fn state(window: &Window, action: &str) -> bool {
        window.window.action_state(action).and_then(|v| v.get::<bool>()).expect("a stateful action")
    }

    /// Under the split options, a submenu ticks each sidebar while it is
    /// open, however it was opened or closed: the menu, the key, the
    /// corner button.
    #[test]
    fn the_chart_menu_ticks_each_sidebar_while_it_is_open() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let rows = sidebars(window.chart_menu_model(100.0).upcast_ref()).expect("a Sidebars submenu");
            assert_eq!(
                rows,
                vec![
                    ("Drawing tools".to_string(), "win.drawing-tools".to_string()),
                    ("Watchlist".to_string(), "win.watchlist".to_string()),
                ]
            );

            let bar = window.drawing_bar.borrow().clone().expect("the bar");
            let tools = bar.is_shown();
            assert_eq!(state(&window, "drawing-tools"), tools);
            gtk::prelude::ActionGroupExt::activate_action(&window.window, "drawing-tools", None);
            assert_eq!(bar.is_shown(), !tools);
            assert_eq!(state(&window, "drawing-tools"), !tools);
            bar.toggle.set_active(tools);
            assert_eq!(bar.is_shown(), tools, "the corner button");
            assert_eq!(state(&window, "drawing-tools"), tools);

            let list = window.shows_sidebar();
            assert_eq!(state(&window, "watchlist"), list);
            gtk::prelude::ActionGroupExt::activate_action(&window.window, "watchlist", None);
            assert_eq!(window.shows_sidebar(), !list);
            assert_eq!(state(&window, "watchlist"), !list);
            window.watchlist_toggle.borrow().as_ref().expect("the toggle").set_active(list);
            assert_eq!(window.shows_sidebar(), list, "the corner button");
            assert_eq!(state(&window, "watchlist"), list);
        });
    }
}

#[cfg(test)]
mod outside_change_tests {
    use super::*;
    use crate::ui::window::trade::tests::bare_window;
    use omacharts_engine::Range;

    /// Run a command against the window, as a terminal would.
    fn command(window: &Rc<Window>, words: &[&str]) -> String {
        let args: Vec<String> = std::iter::once("omacharts").chain(words.iter().copied()).map(String::from).collect();
        let live: &dyn crate::cli::Live = window;
        let outcome = crate::cli::run(&args, &window.store, Some(live), &crate::cli::Here);
        assert_eq!(outcome.code, 0, "{words:?}: {}", outcome.err);
        outcome.out
    }

    fn at(window: &Window, position: usize) -> Rc<ChartPane> {
        let id = window.layout.borrow().leaves()[position];
        window.pane(id).expect("a chart there")
    }

    /// Every row of a kind under `widget`, in the order they are read.
    fn rows<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
        let mut found = Vec::new();
        let mut next = widget.first_child();
        while let Some(child) = next {
            if let Ok(row) = child.clone().downcast::<T>() {
                found.push(row);
            }
            found.extend(rows::<T>(&child));
            next = child.next_sibling();
        }
        found
    }

    fn titled<T: IsA<gtk::Widget> + IsA<adw::PreferencesRow>>(window: &Window, title: &str) -> Vec<T> {
        rows::<T>(window.window.upcast_ref()).into_iter().filter(|row| row.title() == title).collect()
    }

    /// A range set on one chart by a command stays on it when another
    /// command moves the focus, which rebuilds every chart from what was
    /// written down, and when the focus comes back.
    #[test]
    fn a_range_set_by_a_command_survives_the_focus_moving() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "split", "horizontal"]);
            command(&window, &["chart", "focus", "pos:0"]);
            command(&window, &["chart", "range", "2025"]);
            let held = at(&window, 0).view.held_range();
            assert!(matches!(held, Some(Range::Year(2025))), "{held:?}");

            command(&window, &["chart", "focus", "pos:1"]);
            assert_eq!(at(&window, 0).view.held_range(), held, "moving the focus away let the range go");
            assert_eq!(at(&window, 1).view.held_range(), None, "the other chart took it on");
            command(&window, &["chart", "focus", "pos:0"]);
            assert_eq!(at(&window, 0).view.held_range(), held, "moving the focus back let the range go");
        });
    }

    /// The rows of a menu, as (label, action), and whether each opens a
    /// section rather than being a row.
    fn menu_rows(model: &gio::MenuModel) -> Vec<(String, String, bool)> {
        (0..model.n_items())
            .map(|at| {
                let text = |name| {
                    model
                        .item_attribute_value(at, name, None)
                        .and_then(|v| v.str().map(str::to_string))
                        .unwrap_or_default()
                };
                (text("label"), text("action"), model.item_link(at, "section").is_some())
            })
            .collect()
    }

    /// Right-clicking the stripe offers the header's editor as the line
    /// beside it does; the chart's menu ends its first section with Edit
    /// chart title…, under the three switches.
    #[test]
    fn the_stripe_offers_the_header_and_the_chart_menu_offers_a_title() {
        crate::ui::on_gtk(|| {
            let rows: Vec<(String, String)> =
                menu_rows(strip_menu_model().upcast_ref()).into_iter().map(|(label, action, _)| (label, action)).collect();
            assert_eq!(
                rows,
                vec![
                    ("Edit resolutions…".to_string(), "chart.edit-resolutions".to_string()),
                    ("Edit header…".to_string(), "chart.edit-header".to_string()),
                ]
            );

            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let rows = menu_rows(window.chart_menu_model(100.0).upcast_ref());
            let at = rows.iter().position(|(label, ..)| label == "Edit chart title…").expect("an Edit chart title… row");
            assert_eq!(rows[at].1, "chart.edit-title");
            assert_eq!(rows[at - 1].0, "Drawings");
            assert!(rows.get(at + 1).is_some_and(|(.., section)| *section), "not the end of its section");
            assert!(rows[..at].iter().all(|(.., section)| !section), "not the first section");
        });
    }

    /// Clicked, the Title part is an entry where it stands: Enter keeps
    /// what was typed, trimmed; Escape what was there; and blank takes the
    /// title away, leaving the slot to say Add title under the pointer. What
    /// is kept is the chart's, in what a command lists.
    #[test]
    fn a_title_is_typed_where_it_stands_in_the_header() {
        use crate::ui::pane::{TITLE_PLACEHOLDER, TITLE_UNTITLED};
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            assert!(pane.shows(crate::header::Part::Title), "the title is not in the standard header");
            assert!(pane.title_part.parent().is_some_and(|parent| parent == pane.header_center));
            assert!(pane.title_button.has_css_class(TITLE_UNTITLED));
            let words = pane.title_button.child().and_downcast::<gtk::Label>().expect("the title's words");
            assert_eq!(words.text(), TITLE_PLACEHOLDER);

            pane.title_button.emit_clicked();
            assert!(pane.editing_title());
            assert_eq!(pane.title_part.visible_child_name().as_deref(), Some("editing"));
            pane.title_entry.set_text("  Rates   desk ");
            pane.title_entry.emit_activate();
            assert!(!pane.editing_title());
            assert_eq!(pane.title_part.visible_child_name().as_deref(), Some("shown"));
            assert_eq!(window.chart_title(), "Rates desk");
            assert_eq!(words.text(), "Rates desk");
            assert!(!pane.title_button.has_css_class(TITLE_UNTITLED));
            let listed: serde_json::Value =
                serde_json::from_str(&command(&window, &["chart", "list", "--json"])).expect("JSON");
            assert_eq!(listed["charts"][0]["title"], "Rates desk");

            pane.title_button.emit_clicked();
            assert_eq!(pane.title_entry.text(), "Rates desk");
            pane.title_entry.set_text("Something else");
            pane.finish_title_edit(false);
            assert_eq!(window.chart_title(), "Rates desk", "Escape kept what was typed");

            pane.title_button.emit_clicked();
            pane.title_entry.set_text("   ");
            pane.title_entry.emit_activate();
            assert_eq!(window.chart_title(), "");
            assert!(pane.title_button.has_css_class(TITLE_UNTITLED));
            assert_eq!(words.text(), TITLE_PLACEHOLDER);
            let listed: serde_json::Value =
                serde_json::from_str(&command(&window, &["chart", "list", "--json"])).expect("JSON");
            assert_eq!(listed["charts"][0]["title"], serde_json::Value::Null);
        });
    }

    /// F2 with a drawing selected puts the caret in its text, as it always
    /// has, and leaves the chart title alone; with nothing selected, it is
    /// the chart title's.
    #[test]
    fn f2_is_a_selected_drawing_s_text_and_otherwise_the_chart_title() {
        use omacharts_engine::drawings::{Anchor, Drawing, Kind as DrawingKind, Text};
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            let mut note = Drawing::new(DrawingKind::Text, Anchor::new(1, 1.0), Anchor::new(1, 1.0));
            note.text = Text::plain("Earnings");
            pane.view.select_only(note);

            window.press_f2();
            assert!(pane.view.is_typing_text(), "F2 left the selected drawing's text alone");
            assert!(!pane.editing_title(), "F2 took the chart title from a selected drawing");
            assert!(window.window.visible_dialog().is_none());
        });
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            assert!(!pane.view.has_selection());
            window.press_f2();
            assert!(pane.editing_title(), "F2 with nothing selected did not edit the chart title");
            assert!(!pane.view.is_typing_text());
        });
    }

    /// F2 edits the chart title where the header shows it, and in the
    /// dialog where it does not: left out of the header, or with no room for
    /// the row it is in. The menu row says F2.
    #[test]
    fn f2_edits_the_chart_title_in_the_header_or_else_in_the_dialog() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            let rows = menu_rows(window.chart_menu_model(100.0).upcast_ref());
            assert!(rows.iter().any(|(label, ..)| label == "Edit chart title…"));
            assert_eq!(shortcuts::accel("chart.edit-title"), Some("F2"));

            window.edit_chart_title();
            assert!(pane.editing_title(), "F2 did not edit in the header");
            assert!(window.window.visible_dialog().is_none());
            pane.finish_title_edit(false);

            pane.header_center.set_child_visible(false);
            window.edit_chart_title();
            assert!(!pane.editing_title());
            let dialog = window.window.visible_dialog().and_downcast::<adw::AlertDialog>().expect("the dialog");
            assert_eq!(dialog.heading().as_deref(), Some("Chart title"));
            dialog.force_close();
            pane.header_center.set_child_visible(true);

            *pane.header.borrow_mut() = Header { center: Vec::new(), ..Header::standard() };
            pane.arrange_header();
            assert!(!pane.shows(Part::Title));
            window.edit_chart_title();
            assert!(!pane.editing_title());
            window.window.visible_dialog().and_downcast::<adw::AlertDialog>().expect("the dialog").force_close();
        });
    }

    /// Untitled, the title is the whole top of the chart between the two
    /// sides, so the pointer finds Add title anywhere there; titled, it is
    /// as wide as its words.
    #[test]
    fn an_untitled_chart_offers_its_title_across_the_top() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            let lay_out = || {
                pane.root.measure(gtk::Orientation::Horizontal, -1);
                pane.root.measure(gtk::Orientation::Vertical, 1000);
                pane.root.size_allocate(&gtk::Allocation::new(0, 0, 1000, 600), -1);
            };
            lay_out();
            let wide = pane.title_button.width();
            assert!(wide > 600, "untitled, it is only {wide} wide of 1000");
            assert_eq!(pane.title_button.cursor().and_then(|c| c.name()).as_deref(), Some("pointer"));

            window.set_chart_title("Rates");
            lay_out();
            let narrow = pane.title_button.width();
            assert!(narrow > 0 && narrow < 200, "titled, it is {narrow} wide");
        });
    }

    /// A screenshot draws the chart title where the header shows it, and an
    /// untitled chart's empty slot, which is only the Add title placeholder
    /// waiting for a hover, leaves nothing there.
    #[test]
    fn a_screenshot_of_a_titled_chart_shows_its_title_in_the_header() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            let pane = window.focused_pane();
            // On screen, since a widget that is not shown draws nothing.
            window.window.set_default_size(1000, 600);
            window.window.present();
            let settle = || {
                let context = gtk::glib::MainContext::default();
                for _ in 0..30 {
                    while context.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            };
            let pixels = |texture: &gtk::gdk::Texture| {
                let stride = texture.width() as usize * 4;
                let mut bytes = vec![0; stride * texture.height() as usize];
                texture.download(&mut bytes, stride);
                (bytes, stride)
            };
            window.set_chart_title("Semis desk");
            settle();
            assert!(pane.root.is_mapped(), "the chart never came on screen");
            let at = pane.title_button.compute_bounds(&pane.root).expect("the title has a place");
            let titled = screenshot::chart(&pane, &window.theme()).expect("a screenshot").texture;
            window.set_chart_title("");
            settle();
            let untitled = screenshot::chart(&pane, &window.theme()).expect("a screenshot").texture;
            window.window.close();

            let scale = titled.width() as f32 / pane.root.width() as f32;
            let ((titled, stride), (untitled, _)) = (pixels(&titled), pixels(&untitled));
            let rows = (at.y() * scale) as usize..((at.y() + at.height()) * scale) as usize;
            let columns = (at.x() * scale) as usize * 4..((at.x() + at.width()) * scale) as usize * 4;
            let region = |bytes: &[u8]| -> Vec<u8> {
                rows.clone().flat_map(|row| bytes[row * stride..][columns.clone()].to_vec()).collect()
            };
            let (titled, untitled) = (region(&titled), region(&untitled));
            let ground = untitled[..4].to_vec();
            assert!(
                untitled.chunks(4).all(|pixel| pixel == ground),
                "an untitled chart's empty slot drew something into the screenshot"
            );
            assert!(
                titled.chunks(4).any(|pixel| pixel != ground),
                "the title is not in the screenshot where the header shows it"
            );
        });
    }

    /// Edit chart title… asks in a dialog, offering the title the chart has, and
    /// an empty answer is one: the chart is untitled again.
    #[test]
    fn edit_title_asks_for_the_chart_s_title_in_a_dialog() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            window.set_chart_title("Macro");
            let answer = |text: &str| {
                gtk::prelude::WidgetExt::activate_action(&window.window, "chart.edit-title", None)
                    .expect("an Edit chart title… action");
                let dialog =
                    window.window.visible_dialog().and_downcast::<adw::AlertDialog>().expect("a dialog");
                assert_eq!(dialog.heading().as_deref(), Some("Chart title"));
                assert_eq!(dialog.response_label("ok"), "Save");
                let field = rows::<adw::EntryRow>(dialog.upcast_ref()).into_iter().next().expect("a field");
                assert_eq!(field.title(), "Title");
                assert_eq!(field.text(), window.chart_title());
                field.set_text(text);
                assert!(dialog.is_response_enabled("ok"), "{text:?} was refused");
                dialog.emit_by_name::<()>("response", &[&"ok"]);
                dialog.force_close();
            };
            answer("Rates");
            assert_eq!(window.chart_title(), "Rates");
            assert_eq!(window.focused_pane().title.borrow().as_str(), "Rates");
            answer("");
            assert_eq!(window.chart_title(), "");
        });
    }

    /// The Title row heads a chart's settings: typed into, it titles the
    /// chart as it is typed; and it reads a title given by a command, or
    /// taken away by one.
    #[test]
    fn the_title_row_titles_the_chart_and_follows_a_command() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            crate::ui::chart_settings::ChartSettings::present(&window);
            let row = titled::<adw::EntryRow>(&window, crate::ui::chart_settings::TITLE_LABEL)
                .into_iter()
                .next()
                .expect("a Title row");
            assert_eq!(row.text(), "");
            row.set_text("Rates ");
            assert_eq!(window.chart_title(), "Rates");
            assert_eq!(row.text(), "Rates ", "the row's own typing was written back over it");

            command(&window, &["chart", "set", "--title", "Yields"]);
            assert_eq!(row.text(), "Yields");
            assert_eq!(window.focused_pane().title.borrow().as_str(), "Yields");
            command(&window, &["chart", "set", "--title", ""]);
            assert_eq!(row.text(), "");
            assert_eq!(window.chart_title(), "");
        });
    }

    /// The Thickness row is offered under Style only while the bars are
    /// OHLC bars, follows a command that changes them, and moves them as it
    /// is moved.
    #[test]
    fn the_thickness_row_is_there_for_ohlc_bars_only_and_follows_a_command() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            crate::ui::chart_settings::ChartSettings::present(&window);
            let row = titled::<adw::ActionRow>(&window, crate::ui::chart_settings::THICKNESS_LABEL)
                .into_iter()
                .next()
                .expect("a Thickness row");
            assert!(!row.get_visible(), "offered for candles");
            let slider = rows::<gtk::Scale>(row.upcast_ref()).into_iter().next().expect("a slider");

            command(&window, &["chart", "set", "--style", "ohlc", "--thickness", "3"]);
            assert!(row.get_visible(), "not offered for OHLC bars");
            assert_eq!(slider.value(), 3.0);
            assert_eq!(window.ohlc_thickness(), 3);

            slider.set_value(1.0);
            assert_eq!(window.ohlc_thickness(), 1, "the slider did not reach the chart");
            command(&window, &["chart", "set", "--style", "candles"]);
            assert!(!row.get_visible());
        });
    }

    /// The volume pane's Opacity sits right under its Colouring, starts at
    /// 85%, reaches the pane from its slider and follows a command.
    #[test]
    fn the_volume_opacity_row_sits_under_its_colouring_and_follows_a_command() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            // A new chart opens on no template, with no volume pane of its own.
            command(&window, &["chart", "indicator", "add", "volume"]);
            let volume = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::Volume)
                .map(|i| i.id)
                .expect("a volume pane added");
            let opacity = || {
                let indicators = window.indicators();
                crate::ui::chart::volume_opacity(indicators.iter().find(|i| i.id == volume).unwrap())
            };
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, volume);
            let row = || {
                titled::<adw::ActionRow>(&window, crate::ui::chart_settings::VOLUME_OPACITY_LABEL)
                    .into_iter()
                    .next()
                    .expect("an Opacity row")
            };
            let above = row().prev_sibling().and_then(|w| w.downcast::<adw::ComboRow>().ok());
            assert_eq!(above.map(|r| r.title()).as_deref(), Some("Colouring"), "Opacity is not under Colouring");
            let slider = |row: adw::ActionRow| rows::<gtk::Scale>(row.upcast_ref()).into_iter().next().expect("a slider");
            assert_eq!(slider(row()).value(), 85.0);
            assert_eq!(opacity(), 0.85);

            slider(row()).set_value(100.0);
            assert_eq!(opacity(), 1.0, "the slider did not reach the pane");

            command(&window, &["chart", "indicator", "set", "volume", "--opacity", "0.5"]);
            assert_eq!(slider(row()).value(), 50.0, "the row kept the old opacity");
            assert_eq!(opacity(), 0.5);
        });
    }

    /// A volume profile's settings offer a colouring right under its
    /// calculation method while it measures the delta, and none for the
    /// total; picking one paints the delta in it.
    #[test]
    fn a_delta_profile_offers_a_colouring_under_its_method() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "indicator", "add", "volume_profile"]);
            let profile = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::VolumeProfile)
                .map(|i| i.id)
                .expect("a volume profile added");
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, profile);
            // Read again after each change: a change can rebuild the page.
            let rows = || {
                let method = titled::<adw::ComboRow>(&window, "Calculation method").pop().expect("the method row");
                let colouring = titled::<adw::ComboRow>(&window, "Colouring").pop().expect("the colouring row");
                (method, colouring)
            };
            // Its own switch rather than whether it is drawn: the dialog is
            // never shown here.
            let shown = |row: &adw::ComboRow| row.property::<bool>("visible");
            let (method, row) = rows();
            assert!(!shown(&row), "a total has no colouring");
            assert_eq!(method.next_sibling().as_ref(), Some(row.upcast_ref()), "right under the method");

            method.set_selected(1);
            let (method, row) = rows();
            assert_eq!(method.selected(), 1);
            assert!(shown(&row), "a delta has one");
            assert_eq!(method.next_sibling().as_ref(), Some(row.upcast_ref()), "right under the method");
            assert_eq!(row.selected(), 0, "the default, to begin with");
            let names: Vec<String> = (0..row.model().map_or(0, |m| m.n_items()))
                .filter_map(|at| row.model()?.item(at)?.downcast::<gtk::StringObject>().ok().map(|s| s.string().to_string()))
                .collect();
            assert_eq!(names, ["Default", "Vivid", "Soft", "Quiet"]);

            row.set_selected(3);
            let stored = window.indicators();
            let params = &stored.iter().find(|i| i.id == profile).expect("the profile").params;
            assert!(
                matches!(params, omacharts_engine::indicators::Params::VolumeProfile { colouring: Some(BarColouring::Quiet), .. }),
                "{params:?}"
            );
            let (method, row) = rows();
            assert_eq!(row.selected(), 3, "shown as picked");
            method.set_selected(0);
            let (_, row) = rows();
            assert!(!shown(&row), "hidden again for the total");
        });
    }

    /// A volume profile's settings show it in its last period only behind a
    /// switch under its reset period, off to begin with, on either method.
    #[test]
    fn a_profile_is_shown_in_its_last_period_only_from_its_settings() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "indicator", "add", "volume_profile"]);
            let profile = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::VolumeProfile)
                .map(|i| i.id)
                .expect("a volume profile added");
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, profile);
            let label = crate::ui::chart_settings::PROFILE_LAST_ONLY_LABEL;
            let switch = || titled::<adw::SwitchRow>(&window, label).pop().expect("the switch");
            let reset = || titled::<adw::ComboRow>(&window, "Reset every").pop().expect("the reset");
            let last_only = || {
                let stored = window.indicators();
                let params = &stored.iter().find(|i| i.id == profile).expect("the profile").params;
                matches!(params, omacharts_engine::indicators::Params::VolumeProfile { last_only: true, .. })
            };
            assert!(!switch().is_active() && !last_only(), "off to begin with");
            assert_eq!(reset().next_sibling().as_ref(), Some(switch().upcast_ref()), "under the reset");

            for method in ["total", "delta"] {
                command(&window, &["chart", "indicator", "set", "volume_profile", "--profile-method", method]);
                switch().set_active(true);
                assert!(last_only(), "on on {method}");
                switch().set_active(false);
                assert!(!last_only(), "off again on {method}");
            }
        });
    }

    /// A volume profile's settings draw its point of control behind a switch
    /// that is on to begin with; off greys its colour and thickness and
    /// stops it being drawn, whatever the method.
    #[test]
    fn a_profile_s_point_of_control_is_switched_from_its_settings() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "indicator", "add", "volume_profile"]);
            let profile = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::VolumeProfile)
                .map(|i| i.id)
                .expect("a volume profile added");
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, profile);
            let label = crate::ui::chart_settings::PROFILE_POC_LABEL;
            let switch = || titled::<adw::SwitchRow>(&window, label).pop().expect("the switch");
            let colour = || titled::<adw::ActionRow>(&window, "Point of control").pop().expect("its colour");
            let width = || titled::<adw::SpinRow>(&window, "Point of control thickness").pop().expect("its thickness");
            let drawn = || {
                let stored = window.indicators();
                let params = &stored.iter().find(|i| i.id == profile).expect("the profile").params;
                matches!(params, omacharts_engine::indicators::Params::VolumeProfile { poc: true, .. })
            };
            assert!(switch().is_active() && drawn(), "on to begin with");
            assert!(colour().is_sensitive() && width().is_sensitive());
            assert_eq!(switch().next_sibling().as_ref(), Some(colour().upcast_ref()), "above its colour");

            for method in ["total", "delta"] {
                command(&window, &["chart", "indicator", "set", "volume_profile", "--profile-method", method]);
                switch().set_active(false);
                assert!(!drawn(), "off on {method}");
                assert!(!colour().is_sensitive() && !width().is_sensitive(), "greyed on {method}");
                switch().set_active(true);
                assert!(drawn(), "on again on {method}");
                assert!(colour().is_sensitive() && width().is_sensitive());
            }
        });
    }

    /// A chart's settings, open while a command changes its colouring, its
    /// switches and its volume pane's palette, show what the command did —
    /// and showing it changes nothing more.
    #[test]
    fn open_chart_settings_follow_a_command() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            // A new chart opens on no template, with no volume pane of its own.
            command(&window, &["chart", "indicator", "add", "volume"]);
            let volume = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::Volume)
                .map(|i| i.id)
                .expect("a volume pane added");
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, volume);
            let colourings = titled::<adw::ComboRow>(&window, "Colouring");
            assert_eq!(colourings.len(), 2, "the chart's and the volume pane's");
            let (bars, strip) = (&colourings[0], &colourings[1]);
            let hollow = titled::<adw::SwitchRow>(&window, crate::ui::chart_settings::HOLLOW_LABEL);
            let swapped = titled::<adw::SwitchRow>(&window, crate::ui::chart_settings::SWAP_LABEL);
            assert_eq!(bars.selected(), 0);
            assert!(!hollow[0].is_active() && !swapped[0].is_active());

            command(&window, &["chart", "set", "--colouring", "theme-quiet", "--hollow-up", "on", "--swap-colours", "on"]);
            assert_eq!(window.bar_colouring(), BarColouring::Quiet);
            let quiet = BarColouring::ALL.iter().position(|c| *c == BarColouring::Quiet).unwrap() as u32;
            assert_eq!(bars.selected(), quiet, "the Colouring row kept the old palette");
            assert!(hollow[0].is_active(), "the hollow switch stayed off");
            assert!(swapped[0].is_active(), "the swap switch stayed off");

            command(&window, &["chart", "set", "--colouring", "theme-mono"]);
            assert!(!swapped[0].is_sensitive(), "nothing to swap on Monochrome");
            assert!(swapped[0].is_active(), "greyed, not turned off");
            assert_eq!(window.bar_colouring(), BarColouring::Monochrome);
            assert!(window.bars_swapped() && window.bars_hollow_up(), "reading the change made another");

            command(&window, &["chart", "indicator", "set", "volume", "--colouring", "theme-soft"]);
            let strip = titled::<adw::ComboRow>(&window, "Colouring").into_iter().nth(1).unwrap_or(strip.clone());
            let soft = BarColouring::ALL.iter().position(|c| *c == BarColouring::Soft).unwrap() as u32;
            assert_eq!(strip.selected(), soft + 1, "the volume pane's Colouring row kept the old palette");
            let shown = window.indicators();
            let params = &shown.iter().find(|i| i.id == volume).expect("the volume pane").params;
            assert!(
                matches!(params, omacharts_engine::indicators::Params::Volume { colouring: Some(BarColouring::Soft), .. }),
                "{params:?}"
            );
        });
    }

    /// The names a combo row offers, in order.
    fn offered(row: &adw::ComboRow) -> Vec<String> {
        (0..row.model().map_or(0, |m| m.n_items()))
            .filter_map(|at| row.model()?.item(at)?.downcast::<gtk::StringObject>().ok().map(|s| s.string().to_string()))
            .collect()
    }

    /// A volume pane's Colouring is the delta profile's: Default first, which
    /// is the chart's own as it always was, then the three that tell a rise
    /// from a fall.
    #[test]
    fn a_volume_pane_offers_the_delta_profiles_colourings() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            command(&window, &["chart", "indicator", "add", "volume"]);
            let volume = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::Volume)
                .map(|i| i.id)
                .expect("a volume pane added");
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, volume);
            let strip = titled::<adw::ComboRow>(&window, "Colouring").pop().expect("the volume pane's");
            assert_eq!(offered(&strip), ["Default", "Vivid", "Soft", "Quiet"]);
            assert_eq!(strip.selected(), 0, "the chart's own, to begin with");
        });
    }

    /// A cumulative delta's settings: when it resets, with Never last; candles
    /// or a line, the line's own rows offered only for a line, by a command
    /// too; its colouring;
    /// and, on a feed with no bid and ask volume, one line saying it is
    /// estimated.
    #[test]
    fn a_cumulative_delta_is_set_up_from_its_settings() {
        use omacharts_engine::indicators::{cvd, Params};
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = bare_window(&store);
            // Intraday, so the session is a period it can be drawn by.
            command(&window, &["chart", "set", "--resolution", "5m"]);
            command(&window, &["chart", "indicator", "add", "cvd"]);
            let cvd = window
                .indicators()
                .iter()
                .find(|i| i.kind == omacharts_engine::indicators::Kind::Cvd)
                .map(|i| i.id)
                .expect("a cumulative delta added");
            let params = || window.indicators().iter().find(|i| i.id == cvd).expect("the delta").params.clone();
            crate::ui::chart_settings::ChartSettings::present_indicator(&window, cvd);
            // Read again after each change: a change can rebuild the page.
            let reset = || titled::<adw::ComboRow>(&window, "Reset every").pop().expect("the reset");
            let style = || {
                titled::<adw::ComboRow>(&window, "Style")
                    .into_iter()
                    .filter(|row| offered(row) == ["Candles", "Line"])
                    .last()
                    .expect("the style row")
            };
            let colouring = || titled::<adw::ComboRow>(&window, "Colouring").pop().expect("the colouring");

            let names = offered(&reset());
            assert_eq!(names.last().map(String::as_str), Some(cvd::NEVER));
            assert_eq!(names[reset().selected() as usize], "Session", "the session, to begin with");
            assert_eq!(style().selected(), 0);
            assert_eq!(offered(&colouring()), ["Default", "Vivid", "Soft", "Quiet"]);

            let group = reset().ancestor(adw::PreferencesGroup::static_type()).and_downcast::<adw::PreferencesGroup>();
            let said = group.and_then(|g| g.description()).map(|d| d.to_string());
            assert_eq!(said.as_deref(), Some(crate::ui::chart_settings::CVD_ESTIMATED), "a chart with no ladders");

            let line_shown = || {
                rows::<adw::PreferencesGroup>(window.window.upcast_ref())
                    .into_iter()
                    .find(|group| group.title() == "Line")
                    .expect("the line's group")
                    .property::<bool>("visible")
            };
            assert!(!line_shown(), "candles have no line to set");

            let never = reset().model().map_or(0, |m| m.n_items()) - 1;
            reset().set_selected(never);
            assert!(matches!(params(), Params::Cvd { reset: None, .. }), "{:?}", params());
            style().set_selected(1);
            assert!(matches!(params(), Params::Cvd { style: cvd::Style::Line, .. }), "{:?}", params());
            assert!(line_shown(), "a line's own rows");
            colouring().set_selected(2);
            assert!(matches!(params(), Params::Cvd { colouring: Some(BarColouring::Soft), .. }), "{:?}", params());

            // The line's rows follow a command that draws it as candles again.
            command(&window, &["chart", "indicator", "set", "cvd", "--cvd-style", "candles"]);
            assert!(!line_shown(), "the line's rows stayed for candles");
        });
    }
}

/// The Reset every row of an indicator's menu: the periods, the one being
/// drawn ringed, and with `never` a last button for never starting again,
/// which only a cumulative delta has.
fn reset_strip(
    pane: &ChartPane,
    presets: &gio::Menu,
    rows: &mut Vec<(String, gtk::Widget)>,
    current: Option<Reset>,
    never: bool,
) {
    let item = gio::MenuItem::new(Some("Reset every"), None);
    item.set_attribute_value("custom", Some(&"reset".to_variant()));
    presets.append_item(&item);
    // Ringed is the period being drawn: a chosen one the chart is
    // too coarse for is drawn as another, and that is the one in use.
    let timeframe = pane.timeframe.get();
    let in_use = current.map(|reset| reset.effective_for(timeframe));
    let mut choices: Vec<(String, glib::Variant, bool)> =
        Reset::ALL.iter().map(|r| (r.label().to_string(), r.key().to_variant(), Some(*r) == in_use)).collect();
    if never {
        choices.push((omacharts_engine::indicators::cvd::NEVER.to_string(), "never".to_variant(), in_use.is_none()));
    }
    let strip = choice_strip(&choices, "chart.indicator-reset", "Reset every");
    // A period the chart is too coarse for would be promoted to
    // another the moment it was picked: greyed, and saying why,
    // rather than a button that seems to do nothing.
    let mut child = strip.first_child();
    for period in Reset::ALL {
        let Some(button) = child else { break };
        if !period.is_meaningful_for(timeframe) {
            button.set_sensitive(false);
            button.set_tooltip_text(Some(&format!(
                "Too few {} bars in a {} to draw it",
                timeframe.label(),
                period.label().to_lowercase()
            )));
        }
        child = button.next_sibling();
    }
    rows.push(("reset".to_string(), strip));
}

/// Does this watchlist hold that symbol?
///
/// Matched on the canonical ticker and the exchange suffix together, never on
/// what either of them is displayed as: BRK.B is one symbol with a dot in it
/// and SAN.MC is a symbol and a suffix, and a comparison that cannot tell
/// those apart finds the wrong one abroad.
fn list_holds(store: &Store, watchlist: i64, symbol: &str, suffix: Option<&str>) -> bool {
    store.watchlist_sections(watchlist).iter().any(|section| {
        section
            .entries
            .iter()
            .any(|entry| entry.symbol == symbol && entry.suffix.as_deref() == suffix)
    })
}

/// Which digit of the number row a hardware keycode is, whatever the layout
/// makes it type.
///
/// Evdev's codes, which are what GDK hands over on Linux, offset by the eight
/// X has always added: the row runs 1 to 9 from code 10, and 0 is after them.
fn number_row(keycode: u32) -> Option<usize> {
    match keycode {
        10..=18 => Some(keycode as usize - 9),
        19 => Some(0),
        _ => None,
    }
}

/// Which digit a key with Shift held means: by where it is, or, when the
/// keycode is not on the number row, by the digit it typed.
///
/// Where it is comes first, because Shift makes the row type "!" and "@" on
/// one layout and "!" and "\"" on another. The digit is for the keyboards
/// whose keycodes say nothing about position — a virtual one builds its
/// keymap per run and puts every key at the same code — and for a layout
/// that types digits only with Shift held, where both agree anyway.
fn shifted_number(key: gtk::gdk::Key, keycode: u32) -> Option<usize> {
    number_row(keycode).or_else(|| key.to_unicode().and_then(|c| c.to_digit(10)).map(|n| n as usize))
}

/// The chart action a trading key reaches, and its target: Ctrl+B, Ctrl+S
/// and Ctrl+F by letter, Ctrl and a digit by the digit or the number row.
fn trading_key(key: gtk::gdk::Key, keycode: u32) -> Option<(&'static str, Option<glib::Variant>)> {
    use gtk::gdk::Key;
    // The digit typed, or where the key is when the layout types something
    // else there without Shift, as AZERTY's row does.
    let digit = key.to_unicode().and_then(|c| c.to_digit(10)).map(|n| n as usize);
    if let Some(n) = digit.or_else(|| number_row(keycode)).filter(|n| *n > 0) {
        return Some((trade::TRADING_SIZE, Some((n as i32).to_variant())));
    }
    match key {
        Key::b | Key::B => Some((trade::BUY_MARKET, None)),
        Key::s | Key::S => Some((trade::SELL_MARKET, None)),
        Key::f | Key::F => Some((trade::GO_FLAT, None)),
        _ => None,
    }
}

/// Where stepping `by` from `current` lands among None and the templates,
/// wrapping at either end. Positions, with 0 for None.
fn step_round(current: usize, count: usize, by: i32) -> usize {
    let len = count as i32 + 1;
    (current as i32 + by).rem_euclid(len) as usize
}

/// The key that picks the template at `n` in the list, or none for 0.
/// The charts in reading order: by row from the top, and left to right
/// within a row, which is the order `chart list` gives a chartbook built row
/// by row.
///
/// A row is the charts whose tops are level, give or take a twentieth of the
/// area's height: two columns split at slightly different heights still read
/// as one row below the other, not as a staircase. A chart taller than its
/// neighbours belongs to the row it starts in.
fn reading_order(rects: &HashMap<u32, (f64, f64, f64, f64)>) -> Vec<u32> {
    let mut charts: Vec<(u32, f64, f64)> = rects.iter().map(|(id, r)| (*id, r.0, r.1)).collect();
    charts.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.1.total_cmp(&b.1)).then(a.0.cmp(&b.0)));
    let top = rects.values().map(|r| r.1).fold(f64::INFINITY, f64::min);
    let bottom = rects.values().map(|r| r.1 + r.3).fold(f64::NEG_INFINITY, f64::max);
    let level = (bottom - top) / 20.0;
    let mut rows: Vec<Vec<(u32, f64, f64)>> = Vec::new();
    for chart in charts {
        match rows.last_mut() {
            Some(row) if chart.2 - row[0].2 <= level => row.push(chart),
            _ => rows.push(vec![chart]),
        }
    }
    rows.into_iter()
        .flat_map(|mut row| {
            row.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            row.into_iter().map(|(id, _, _)| id)
        })
        .collect()
}

fn template_accel(n: usize) -> String {
    format!("<Ctrl><Alt><Shift>{n}")
}

/// What was last copied: drawings, or one indicator, never both, since only
/// one of them can be selected at a time.
#[derive(Clone)]
enum Clipboard {
    Empty,
    Drawings(Vec<omacharts_engine::Drawing>),
    Indicator(Indicator),
}

/// What a legend row is named by, so the one for a selected indicator can be
/// found among them.
fn legend_row_name(id: u32) -> String {
    format!("legend-indicator-{id}")
}

/// The bar style choice that hides the bars, among the styles that draw
/// them.
pub const NO_BARS: &str = "none";
pub const NO_BARS_LABEL: &str = "None";

/// What the Bar style menu marks: the style, or None while the bars are
/// hidden.
fn bar_style_state(pane: &ChartPane) -> &'static str {
    if pane.bars_hidden.get() { NO_BARS } else { pane.bar_style.get().key() }
}

/// A chart template as a menu action's target: its id, or `none`, spelled
/// the way the default for new charts is written down.
fn template_state(template: Option<u32>) -> glib::Variant {
    match template {
        Some(id) => id.to_string().to_variant(),
        None => crate::chart_defaults::NO_TEMPLATE.to_variant(),
    }
}

/// The periods a moving average is usually drawn at, as the indicator
/// menu offers them.
const AVERAGE_PERIODS: [usize; 5] = [10, 20, 50, 100, 200];

/// A row of a menu holding a few answers as buttons, the current one ringed:
/// each activates `action` with its target and closes the menu.
fn choice_strip(choices: &[(String, glib::Variant, bool)], action: &str, what: &str) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    row.add_css_class("drawing-head-row");
    row.set_homogeneous(true);
    for (label, target, current) in choices {
        let button = gtk::Button::with_label(label);
        button.add_css_class("flat");
        button.add_css_class("menu-choice");
        button.set_tooltip_text(Some(&format!("{what}: {label}")));
        if *current {
            button.add_css_class("drawing-preview-current");
        }
        button.set_action_name(Some(action));
        button.set_action_target_value(Some(target));
        button.connect_clicked(|button| {
            if let Some(popover) = button.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>() {
                popover.popdown();
            }
        });
        row.append(&button);
    }
    row.upcast()
}

fn popup_menu(
    model: &gio::Menu,
    over: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
) -> gtk::PopoverMenu {
    popup_menu_with(model, over, x, y, Vec::new())
}

/// The same, with widgets of our own standing in for the items that name
/// them in a `custom` attribute: a row that shows something rather than
/// says it.
fn popup_menu_with(
    model: &gio::Menu,
    over: &impl IsA<gtk::Widget>,
    x: f64,
    y: f64,
    children: Vec<(String, gtk::Widget)>,
) -> gtk::PopoverMenu {
    // Sliding submenus, which is GTK's default and not what this used.
    //
    // `NESTED` puts each submenu in a popover of its own, and a popover of
    // its own takes the pointer grab: with one open, a click on the chart
    // dismissed the submenu and went no further, so the menu behind it sat
    // there. Hovering `Bar style` and then clicking away left a menu that
    // would not close. The same flag is why a custom widget inside a
    // submenu drew nothing — it is a slot a nested popover never fills,
    // which is how the Configuration submenu came to open empty.
    //
    // Sliding keeps the whole menu in one popover: one grab, one click to
    // dismiss, and widgets of our own render wherever they are put.
    let popover = gtk::PopoverMenu::from_model(Some(model));
    for (id, child) in &children {
        popover.add_child(child, id);
    }
    popover.set_parent(over);
    popover.set_has_arrow(false);
    popover.set_halign(gtk::Align::Start);
    popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    // Clicking an item closes the popover and *then* activates its action, so
    // unparenting on close would pull the action context out from under the
    // click. Let the activation happen first.
    popover.connect_closed(|popover| {
        let popover = popover.clone();
        glib::idle_add_local_once(move || popover.unparent());
    });
    // Popped from an idle, not here. GtkPopoverMenu inserts its section
    // separators from an idle after this callback returns, and a popup surface
    // is sized once, when it is first shown — GTK never re-presents a mapped
    // popover for a resize that started inside it. Showing it now measures a
    // menu two separators short of itself, and the scroller inside every
    // GtkPopoverMenu absorbs the difference by scrolling, which is why the last
    // item sat under the bottom corner. The separator sync runs at a higher
    // idle priority than this one, so by the time it pops it is whole.
    let shown = popover.clone();
    glib::idle_add_local_once(move || shown.popup());
    popover
}

/// How many symbols either side of the selection to fetch ahead.
///
/// Small on purpose. Every request is one someone may never want, and the
/// window moves with the selection — arrow up and the two above you are
/// already being fetched. Filling an entire watchlist up front is fifty
/// requests in a burst for a list most of which never gets looked at.
/// One data feed as the window holds it: the provider, its own loader and,
/// for a provider that streams, its own registry of live series.
///
/// A loader each, because pacing and cooldowns are a provider's: Yahoo's rate
/// limit must not hold back a feed that has none. With Yahoo alone there is
/// one of these and the window is what it was with one feed.
struct Feed {
    provider: Arc<dyn Provider>,
    loader: Loader,
    /// The one source for everything this feed streams. `None` for a feed
    /// that does not, which then pays nothing: no registry, no writer
    /// thread, no wake.
    live: Option<Rc<Live>>,
    /// Its volume at price, for a feed whose provider has ladders. `None`
    /// for every other, which then pays nothing: no thread, no query.
    at_price: Option<Rc<AtPrice>>,
    /// Can it be charted from with what is saved? Read from disk when the
    /// list is built and when a sign-in or sign-out finishes, never by
    /// connecting. A feed that cannot is passed over by [`Window::route`].
    usable: Cell<bool>,
    /// Why it cannot, for the header: "not signed in", "session expired".
    unusable_because: Cell<&'static str>,
    /// Set when this feed has just refused speculative work. Until it passes,
    /// the backfill offers this feed nothing — it would only turn it down,
    /// and asking is how a cooldown becomes a longer one.
    backfill_quiet_until: Cell<Option<Instant>>,
    /// The same, for refreshing charts, and separate from it on purpose. The
    /// two ask for different things and one being refused says nothing about
    /// the other: a backfill paused for five minutes must not also stop the
    /// chart somebody is watching from keeping itself current.
    refresh_quiet_until: Cell<Option<Instant>>,
    /// Taken off the order while the rest of it stayed: whatever its worker
    /// still sends is for charts that have moved on.
    retired: Cell<bool>,
}

impl Feed {
    fn new(provider: Arc<dyn Provider>, store: &Store, sender: async_channel::Sender<Response>) -> Feed {
        let loader = Loader::new(provider.clone(), sender);
        let live = provider
            .stream()
            .is_some()
            .then(|| Rc::new(Live::new(provider.clone(), store.path().map(Path::to_path_buf))));
        let at_price = AtPrice::new(provider.clone(), store.path().map(Path::to_path_buf)).map(Rc::new);
        let feed = Feed {
            provider,
            loader,
            live,
            at_price,
            usable: Cell::new(true),
            unusable_because: Cell::new(""),
            backfill_quiet_until: Cell::new(None),
            refresh_quiet_until: Cell::new(None),
            retired: Cell::new(false),
        };
        feed.read_access();
        feed
    }

    /// Read again whether this feed can be used. True when that changed.
    fn read_access(&self) -> bool {
        let access = providers::access(self.provider.id());
        let usable = access.as_ref().is_none_or(|access| access.ready());
        self.unusable_because.set(match access {
            Some(providers::Access::Expired(_)) => "session expired",
            _ => "not signed in",
        });
        self.usable.replace(usable) != usable
    }

    /// Is `key` one of this feed's? Cache keys are `provider:symbol`, which is
    /// what keeps one feed's bars from ever being drawn as another's.
    fn owns(&self, key: &str) -> bool {
        key.strip_prefix(self.provider.id()).is_some_and(|rest| rest.starts_with(':'))
    }

    /// Clear a pause that has run out; true while one still holds.
    fn quiet(cell: &Cell<Option<Instant>>) -> bool {
        match cell.get() {
            Some(until) if until > Instant::now() => true,
            Some(_) => {
                cell.set(None);
                false
            }
            None => false,
        }
    }
}

/// Which feed serves a chart, as [`Window::route`] decided it.
struct Routed {
    feed: Rc<Feed>,
    /// The instrument in that feed's spelling.
    symbol: String,
    /// Feeds above it that would have served it and cannot be used.
    passed: Vec<Rc<Feed>>,
}

impl Routed {
    /// The cache key, `provider:symbol`.
    fn key(&self) -> String {
        format!("{}:{}", self.feed.provider.id(), self.symbol)
    }
}

const PREFETCH_WINDOW: usize = 3;

const LAST_SYMBOL: &str = "last_symbol";
const LAST_SUFFIX: &str = "last_suffix";
const LAST_TIMEFRAME: &str = "last_timeframe";
const SHOW_WATCHLIST: &str = "show_watchlist";
/// How wide the rail was left, in pixels.
const SETTING_SIDEBAR_WIDTH: &str = "sidebar_width";
/// Whether the bar widget has been put in the bar once already.
const SETTING_WIDGET_OFFERED: &str = "bar_widget_offered";
const DEFAULT_SIDEBAR_WIDTH: i32 = 280;
/// How narrow and how wide the rail may be dragged. Below the first its
/// columns stop fitting; above the second it is taking room from the chart
/// without showing anything more.
const MIN_SIDEBAR_WIDTH: i32 = 160;
const MAX_SIDEBAR_WIDTH: i32 = 900;
/// How narrow the rail may push the charts. The rail is a list of symbols;
/// it should not be able to take the window from the charts of them.
const MIN_CHARTS_WIDTH: i32 = 480;

/// The widest the rail may be in a split this wide: its own ceiling, or
/// what leaves the charts their floor, and never below its own floor.
fn sidebar_room(total: i32) -> i32 {
    (total - MIN_CHARTS_WIDTH).clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
}
const SETTING_TIMEFRAMES: &str = "timeframes";
/// Why [`Window::show_named`] put nothing on a chart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotShown {
    /// No watchlist holds the symbol, or not the one asked for.
    Unlisted,
    /// The list holds it and no chart took it: the focused chart is Pinned
    /// and none follows the watchlist.
    Refused,
}

/// The whole arrangement of charts, as one stored value.
const SETTING_WORKSPACE: &str = "workspace";
/// Whether the drawing tools are on screen.
const SHOW_DRAWING_TOOLS: &str = "show_drawing_tools";
/// Whether a pointer on a chart draws a line on the other charts of its
/// symbol. One switch for the app, in the preferences.
pub const SETTING_SYNC_CROSSHAIR: &str = "sync_crosshair";

/// One chart, as it is written down.
///
/// Stored together with the layout rather than as separate settings, because
/// they only mean anything together: a tree of pane ids and a list of panes
/// that disagree is a window that cannot be rebuilt.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct StoredPane {
    id: u32,
    /// The chart title. Missing for a chart nobody titled, which is
    /// every chart written before charts could be.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    symbol: String,
    #[serde(default)]
    suffix: Option<String>,
    timeframe: String,
    /// What the chart shows. For a chart on a template, a copy of the
    /// template's, so that a command reading the arrangement reads what is on
    /// screen and a chart whose template has gone still has something to show.
    indicators: Vec<Indicator>,
    /// The chart template it shows, by id. Missing from a chart written
    /// before there were templates, and every one of those had its own.
    #[serde(default)]
    template: Option<u32>,
    bar_style: String,
    /// Whether the bars are hidden, which leaves `bar_style` as the style
    /// showing them again brings back. Missing from a chart written before
    /// they could be, and every one of those was drawing them.
    #[serde(default)]
    bars_hidden: bool,
    /// How the theme's candle colours are painted. Missing from a chart
    /// written before charts were painted one by one.
    #[serde(default)]
    colouring: Option<String>,
    /// The two switches worn over the colouring: up and down exchanged, and
    /// a rise drawn as an outline. Missing from a chart written before they
    /// were switches: one whose `colouring` is a scheme an older version
    /// wrote for the look (`theme-red-up`, `theme-hollow`) wore it, and any
    /// other chart with a colouring of its own did not.
    #[serde(default)]
    swapped: Option<bool>,
    #[serde(default)]
    hollow_up: Option<bool>,
    /// How many pixels wide its OHLC bars are drawn. Missing from a chart
    /// written before they could be thicker, which takes the default.
    #[serde(default)]
    ohlc_thickness: Option<u8>,
    session: String,
    /// How a futures root's months are joined at the rolls: `back-adjusted`
    /// or `as-traded`. Missing from a chart written before there was a
    /// choice, which takes the default.
    #[serde(default)]
    continuous: Option<String>,
    show_grid: bool,
    /// What the rail does to this chart. Missing from a chart written with
    /// link groups instead, which [`upgrade_link_groups`] turns into this
    /// before the chart is read.
    #[serde(default)]
    link: LinkMode,
    /// Whether the price axis fits itself to the visible bars, or has been
    /// taken off automatic to be dragged. Missing from a chart written before
    /// this was remembered, and every one of those was fitting itself.
    #[serde(default = "fits_itself")]
    auto_scale: bool,
    /// The prices a held price axis shows, bottom and top, so a restart
    /// brings back the zoom and the shift it was left at rather than a fresh
    /// fit of the live edge. Prices, not a zoom on the bars, because the
    /// bars on screen after a restart are not the ones that were: the prices
    /// somebody zoomed to are what they were looking at. Missing on a chart
    /// that fits itself, and on one written before this was remembered,
    /// which holds the fit it is first drawn with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    price_window: Option<[f64; 2]>,
    /// The strips zoomed or dragged on their own scale, by the indicator
    /// they draw, each with the values its bottom and top show, so a restart
    /// brings them back as the price's held window does. A strip not named
    /// fits itself; missing on a chart none of whose strips was zoomed, and
    /// on one written before they could be.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    strip_windows: std::collections::BTreeMap<u32, StoredStrip>,
    /// Whether the price axis is logarithmic. Missing from a chart written
    /// before it could be, and every one of those was linear.
    #[serde(default)]
    log_scale: bool,
    /// Whether the chart draws its indicators, or hides them all for a look
    /// at the bars. Missing from a chart written before they could all be
    /// hidden at once, and every one of those drew them.
    #[serde(default = "yes")]
    indicators_shown: bool,
    /// Whether the chart draws its drawings, its own and the shared, or
    /// hides them all. Missing from a chart written before they could be
    /// hidden, and every one of those drew them.
    #[serde(default = "yes")]
    drawings_shown: bool,
    /// Whether the chart shows what other charts of its symbol draw, and
    /// whether it sends them what is drawn here. Both on by default, which
    /// is what a book written before either existed gets — and what the
    /// drawing groups these replaced did for every chart that had not been
    /// put in one.
    #[serde(default = "yes")]
    shows_drawings: bool,
    #[serde(default = "yes")]
    sends_drawings: bool,
    /// Whether the chart draws its indicator legend at all. Missing from a
    /// chart written before the legend could be turned off, and every one
    /// of those drew it.
    #[serde(default = "yes")]
    shows_legend: bool,
    /// What this chart is called among the drawings it has made. Empty for
    /// a chart written down before drawings remembered who drew them, which
    /// simply means none of them name it.
    #[serde(default)]
    drawing_uid: String,
    /// The drawings that are this chart's alone.
    #[serde(default)]
    drawings: Vec<omacharts_engine::Drawing>,
    /// Volume profile periods merged into composites on this chart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    profile_merges: Vec<omacharts_engine::indicators::profile::Merge>,
    /// The data feed somebody chose for this chart, by id. Missing for a
    /// chart that follows the order of feeds, which is every chart until
    /// somebody chooses and every chart written before anybody could.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    /// The trading account somebody chose for this chart, as
    /// `<login id>/<account number>`. Missing for a chart on the default
    /// account; one whose login was removed is dropped and the chart is on
    /// the default again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    /// The trading size somebody picked for this chart, and the size
    /// template it was picked from. Missing for a chart on the first size
    /// of its template, which is every chart until somebody picks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trading_size: Option<crate::trading::sizes::Picked>,
    /// What this chart's orders go to when it is not what it charts, by
    /// symbol and suffix, as `symbol` and `suffix` name what it charts.
    /// Missing for a chart that trades what it charts, which is every chart
    /// until somebody sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trading_symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trading_suffix: Option<String>,
}

impl StoredPane {
    /// A chart with nothing in it, written down: what stands in for a
    /// field of a stored one while the others are tried. See [`mend`].
    fn blank() -> StoredPane {
        StoredPane {
            id: 0,
            title: None,
            symbol: String::new(),
            suffix: None,
            timeframe: String::new(),
            indicators: Vec::new(),
            template: None,
            bar_style: String::new(),
            bars_hidden: false,
            colouring: None,
            swapped: None,
            hollow_up: None,
            ohlc_thickness: None,
            session: String::new(),
            continuous: None,
            show_grid: true,
            link: LinkMode::default(),
            auto_scale: true,
            price_window: None,
            strip_windows: Default::default(),
            log_scale: false,
            indicators_shown: true,
            drawings_shown: true,
            shows_drawings: true,
            sends_drawings: true,
            shows_legend: true,
            drawing_uid: String::new(),
            drawings: Vec::new(),
            profile_merges: Vec::new(),
            source: None,
            account: None,
            trading_size: None,
            trading_symbol: None,
            trading_suffix: None,
        }
    }
}

/// A strip held on its own scale, as written down: the values its bottom
/// and top show, and the kind of indicator it was held for, so that another
/// kind given its id later fits itself rather than take it on.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(from = "StripShape")]
struct StoredStrip {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<omacharts_engine::IndicatorKind>,
    window: [f64; 2],
}

/// A held strip as either shape has written it: with its kind, or as the
/// bare bottom and top a build before kinds were kept wrote, whose kind is
/// then the indicator's it is brought back on.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum StripShape {
    Held {
        #[serde(default)]
        kind: Option<omacharts_engine::IndicatorKind>,
        window: [f64; 2],
    },
    Window([f64; 2]),
}

impl From<StripShape> for StoredStrip {
    fn from(shape: StripShape) -> StoredStrip {
        match shape {
            StripShape::Held { kind, window } => StoredStrip { kind, window },
            StripShape::Window(window) => StoredStrip { kind: None, window },
        }
    }
}

fn yes() -> bool {
    true
}

/// What right-clicking the resolutions stripe offers: its list, and the
/// header's own item, since the stripe is in the header and a menu that
/// left it out would read as though this part of it were set elsewhere.
fn strip_menu_model() -> gio::Menu {
    let model = gio::Menu::new();
    model.append(Some("Edit resolutions…"), Some("chart.edit-resolutions"));
    model.append(Some("Edit header…"), Some("chart.edit-header"));
    model
}

/// What a chart's price scale does until somebody takes hold of it.
fn fits_itself() -> bool {
    true
}

/// A chart's title as it is kept: trimmed, on one line, and no longer than
/// the header's entry lets one be typed, wherever it was typed.
pub(crate) fn clean_title(title: &str) -> String {
    let one_line: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(crate::ui::pane::TITLE_MAX_LENGTH as usize).collect()
}

/// One arrangement of charts, saved under a name.
///
/// A chartbook is the unit you switch between: a tree, the charts in it, and
/// which of them had the keyboard. Everything about a chart that is worth
/// keeping is already in `StoredPane`, so a book is that list plus the shape
/// they were in.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Chartbook {
    #[serde(default)]
    name: String,
    layout: Node,
    focused: u32,
    panes: Vec<StoredPane>,
    /// The rail, as this book left it: which watchlist it was showing,
    /// whether it was open at all, and how wide.
    ///
    /// Part of the book rather than of the window, because a book is what you
    /// were looking at and the list beside the charts is half of that — an
    /// arrangement of energy charts wants the energy list, and finding
    /// yesterday's list beside it is the same surprise as finding the wrong
    /// charts.
    ///
    /// `None` in a book written before the rail belonged to one. Those fall
    /// back to the window's settings, which is where the choice used to live
    /// and is still where a brand new book takes its from.
    #[serde(default)]
    watchlist: Option<i64>,
    #[serde(default)]
    sidebar_shown: Option<bool>,
    #[serde(default)]
    sidebar_width: Option<i32>,
    /// The chart that had the window to itself, by the same id `focused`
    /// uses. `None` when none did — and in every book written before this
    /// was remembered, which come back showing the whole arrangement.
    #[serde(default)]
    maximized: Option<u32>,
    /// The chartbook template it was made from or last saved as, by id. See
    /// [`crate::book_templates`]. Left out when it is on none, which is every
    /// book written before there were chartbook templates, so a book that
    /// never takes one is written down exactly as it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    book_template: Option<u32>,
}

impl Chartbook {
    /// A book with nothing in it, written down. See [`mend`].
    fn blank() -> Chartbook {
        Chartbook {
            name: String::new(),
            layout: Node::leaf(0),
            focused: 0,
            panes: Vec::new(),
            watchlist: None,
            sidebar_shown: None,
            sidebar_width: None,
            maximized: None,
            book_template: None,
        }
    }
}

/// Everything the window had open, as one stored value.
///
/// `Workspace` used to be a single arrangement, which is what `Chartbook` is
/// now. A window holds several and shows one.
#[derive(serde::Serialize, serde::Deserialize)]
struct Workspace {
    books: Vec<Chartbook>,
    active: usize,
}

/// A move as the header writes it, in whole hundredths of a percent.
fn hundredths(change_pct: f64) -> i64 {
    (change_pct * 100.0).round() as i64
}

/// What a chart's bars are painted in: the theme's candle colours, the way
/// this chart paints them.
/// The feed's latest trade when the chart's session leaves it out of the
/// bars drawn, a chart of the regular session out of hours: the price is
/// the price whatever candles are shown, so the last price follows it.
/// `None` when the last bar is one the chart draws. One bar through the
/// session's filter, so nothing here grows with the series.
fn hidden_latest(
    bars: &[omacharts_engine::Bar],
    session: omacharts_engine::Session,
    instrument: &Instrument,
    timeframe: Timeframe,
) -> Option<f64> {
    let newest = bars.last()?;
    omacharts_engine::session::filter(std::slice::from_ref(newest), session, instrument, timeframe.is_intraday())
        .is_empty()
        .then_some(newest.close)
}

fn pane_scheme(pane: &ChartPane, theme: &omacharts_engine::Theme) -> omacharts_engine::BarScheme {
    pane.colouring.get().scheme(theme).worn(pane.swapped.get(), pane.hollow_up.get())
}

/// How a stored chart paints its bars: the palette and the switches over it.
///
/// A chart written before it had a colouring of its own was wearing the one
/// every chart wore, which is the default's, switches and all. One written
/// before the looks were switches says so by its colouring: a scheme an older
/// version wrote for a hollow rise or for red up is Vivid with that switch
/// on, and any other colouring of its own was worn plain.
fn stored_colouring(stored: &StoredPane, defaults: &ChartDefaults) -> StoredColouring {
    match stored.colouring.as_deref().and_then(BarColouring::from_stored) {
        Some(written) => StoredColouring {
            colouring: written.colouring,
            swapped: stored.swapped.unwrap_or(written.swapped),
            hollow_up: stored.hollow_up.unwrap_or(written.hollow_up),
        },
        None => StoredColouring {
            colouring: defaults.colouring,
            swapped: stored.swapped.unwrap_or(defaults.swapped),
            hollow_up: stored.hollow_up.unwrap_or(defaults.hollow_up),
        },
    }
}

/// A chart's colouring and the switches it wears, as a split copies them.
fn pane_look(pane: &ChartPane) -> StoredColouring {
    StoredColouring {
        colouring: pane.colouring.get(),
        swapped: pane.swapped.get(),
        hollow_up: pane.hollow_up.get(),
    }
}

/// A chart's local drawings get negative ids, below everything the store
/// hands out, so one number names a drawing wherever it lives.
fn next_local_id(locals: &[omacharts_engine::Drawing]) -> i64 {
    locals.iter().map(|d| d.id).min().unwrap_or(0).min(0) - 1
}

/// The last choice in the New chartbook dialog's watchlist picker: no list
/// that exists, but an empty one named after the chartbook.
const NEW_WATCHLIST: &str = "Create a new watchlist";

/// What a chartbook nobody has renamed is called.
///
/// By where it sits rather than when it was made, so the names in the strip
/// always read 1, 2, 3 and the arrow keys walk them in the order they are
/// written. A book keeps an empty name until somebody types one.
fn default_book_name(ordinal: usize) -> String {
    format!("Chartbook {ordinal}")
}

/// Which chartbook is using `watchlist`, other than the one at `besides`.
///
/// A watchlist belongs to one chartbook: two books showing one list would
/// fight over it, because which list the rail shows is written down against
/// whichever book was saved last.
///
/// A scan of the books rather than an index beside them. There are a handful,
/// and a second record of who is using what is a second record that can come
/// to disagree with the books themselves.
///
/// The default watchlist is nobody's. It is the one that cannot be deleted and
/// the one every fallback lands on, so a rule that let one book keep it would
/// leave a second book with nowhere to be.
fn book_using(books: &[Chartbook], besides: usize, watchlist: i64) -> Option<usize> {
    if watchlist == DEFAULT_WATCHLIST {
        return None;
    }
    books
        .iter()
        .enumerate()
        .find(|(at, book)| *at != besides && book.watchlist == Some(watchlist))
        .map(|(at, _)| at)
}

/// Where the chartbook at `active` ends up when the one at `from` is taken out
/// of the list and put back at `to`.
///
/// Written down rather than worked out at the call site, because `active` is a
/// position in `books` and not a book: a reorder that did not carry it along
/// would switch chartbooks as a side effect of dragging a tab.
fn shifted(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        return to;
    }
    let lifted = if active > from { active - 1 } else { active };
    if lifted >= to {
        lifted + 1
    } else {
        lifted
    }
}

/// Read what was written down, in whichever shape it was written.
///
/// The setting held a single arrangement before there were chartbooks to hold
/// several, and anybody upgrading has one of those in their database. Reading
/// it as the one book it describes is the difference between keeping the
/// layout they left and being handed an empty window.
///
/// The two shapes cannot be confused for each other: the old one has no
/// `books`, and the new one has no `layout`.
///
/// A part this build cannot read is left out and said in the Log, not the
/// whole of it: a field of a chart or a book written by another build, or
/// one entry of a list or a map in it, goes, and the rest is read. Only a
/// chart that cannot be read without what was left out — its symbol, its
/// id — takes its book with it, since the book's layout names it. Leaving
/// the whole arrangement unread instead would open the window on one new
/// chart, and its next save would write that over every chartbook.
fn parse_workspace(json: &str) -> Option<Workspace> {
    let mut value: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut left_out = Vec::new();
    mend_workspace(&mut value, &mut left_out);
    for part in left_out {
        omacharts_engine::log::global()
            .add(omacharts_engine::log::Entry::warning("Chartbooks", format!("Left out of the saved chartbooks, as it could not be read: {part}")));
    }
    serde_json::from_value::<Workspace>(value.clone())
        .ok()
        .filter(|workspace| !workspace.books.is_empty())
        .or_else(|| {
            let book = serde_json::from_value::<Chartbook>(value).ok()?;
            Some(Workspace { books: vec![book], active: 0 })
        })
}

/// Take out of a stored arrangement, in place, what this build cannot read
/// of it, each part left out named in `left_out`. See [`parse_workspace`].
fn mend_workspace(value: &mut serde_json::Value, left_out: &mut Vec<String>) {
    // The arrangement from before chartbooks is one book at the top level.
    if value.get("books").is_none() {
        mend_book(value, left_out);
        return;
    }
    if let Some(books) = value["books"].as_array_mut() {
        books.retain_mut(|book| mend_book(book, left_out));
    }
    let blank = serde_json::json!({ "books": [], "active": 0 });
    mend::<Workspace>(value, &blank, "the chartbooks", left_out);
}

/// [`mend_workspace`] for one book: `false` when it cannot be read even so.
fn mend_book(book: &mut serde_json::Value, left_out: &mut Vec<String>) -> bool {
    let name = book.get("name").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let named = match name.is_empty() {
        true => "a chartbook".to_string(),
        false => format!("chartbook \u{201c}{name}\u{201d}"),
    };
    let blank_pane = serde_json::to_value(StoredPane::blank()).expect("a chart is written down");
    if let Some(panes) = book.get_mut("panes").and_then(serde_json::Value::as_array_mut) {
        for pane in panes {
            let symbol = pane.get("symbol").and_then(serde_json::Value::as_str).unwrap_or("?").to_string();
            if !mend::<StoredPane>(pane, &blank_pane, &format!("the {symbol} chart in {named}"), left_out) {
                left_out.push(format!("{named}, whose {symbol} chart it cannot be rebuilt without"));
                return false;
            }
        }
    }
    let blank = serde_json::to_value(Chartbook::blank()).expect("a chartbook is written down");
    let read = mend::<Chartbook>(book, &blank, &named, left_out);
    if !read {
        left_out.push(named);
    }
    read
}

/// Make `value` read as a `T`, leaving out what of it does not: each field
/// that will not read is taken out, so it is read as one written without it,
/// and a list or a map that will not is kept but for the entries that will
/// not. `blank`, a `T` written down, stands in for what a field needs while
/// the others are tried. `false` when it does not read even so: a field it
/// cannot do without is missing or would not read, or it is not an object.
fn mend<T: serde::de::DeserializeOwned>(value: &mut serde_json::Value, blank: &serde_json::Value, what: &str, left_out: &mut Vec<String>) -> bool {
    use serde_json::{Map, Value};
    if serde_json::from_value::<T>(value.clone()).is_ok() {
        return true;
    }
    let (Some(stored), Some(blank)) = (value.as_object(), blank.as_object()) else { return false };
    let reads = |object: &Map<String, Value>| serde_json::from_value::<T>(Value::Object(object.clone())).is_ok();
    let mut mended = blank.clone();
    let mut bad = Vec::new();
    for (key, field) in stored {
        let was = mended.insert(key.clone(), field.clone());
        if reads(&mended) {
            continue;
        }
        // A list or a map read entry by entry, where an empty one reads.
        let mut parts = Vec::new();
        let piecewise = match field {
            Value::Array(entries) => {
                mended.insert(key.clone(), Value::Array(Vec::new()));
                reads(&mended).then(|| {
                    for (at, entry) in entries.iter().enumerate() {
                        let Some(Value::Array(kept)) = mended.get_mut(key) else { unreachable!() };
                        kept.push(entry.clone());
                        if !reads(&mended) {
                            let Some(Value::Array(kept)) = mended.get_mut(key) else { unreachable!() };
                            kept.pop();
                            parts.push(format!("{key}[{at}]"));
                        }
                    }
                })
            }
            Value::Object(entries) => {
                mended.insert(key.clone(), Value::Object(Map::new()));
                reads(&mended).then(|| {
                    for (name, entry) in entries {
                        let Some(Value::Object(kept)) = mended.get_mut(key) else { unreachable!() };
                        kept.insert(name.clone(), entry.clone());
                        if !reads(&mended) {
                            let Some(Value::Object(kept)) = mended.get_mut(key) else { unreachable!() };
                            kept.remove(name);
                            parts.push(format!("{key}.{name}"));
                        }
                    }
                })
            }
            _ => None,
        };
        match piecewise {
            Some(()) => bad.extend(parts),
            None => {
                match was {
                    Some(stand_in) => mended.insert(key.clone(), stand_in),
                    None => mended.remove(key),
                };
                bad.push(key.clone());
            }
        }
    }
    // What was left out, and what it was written without, are read as a
    // field missing is; one it cannot do without leaves it unread.
    for key in blank.keys() {
        let missing = !stored.contains_key(key) || bad.contains(key);
        if missing && mended.remove(key).is_some() && !reads(&mended) {
            return false;
        }
    }
    if !reads(&mended) {
        return false;
    }
    left_out.extend(bad.into_iter().map(|part| format!("{part} of {what}")));
    *value = Value::Object(mended);
    true
}

/// Turn the link groups a stored arrangement may still carry into modes, in
/// place. `true` when there was anything to turn.
///
/// Charts were written down with one of nine link groups, and the watchlist
/// beside them chose which group its clicks moved. Each book is read against
/// the group its own list drove, from the rows those lists kept it in: a
/// chart in that group moved on every click, so it is Follow, and every
/// other chart is Focus. See [`LinkMode::from_group`].
///
/// On the stored value rather than on [`StoredPane`], so that a window and a
/// command reading the same row with no window open arrive at the same modes,
/// and so the old fields do not have to live on in the struct. The groups are
/// dropped as the mode is written, so the next save stores the mode and the
/// rows are never asked again; they are left where they are.
pub(crate) fn upgrade_link_groups(workspace: &mut serde_json::Value, store: &Store) -> bool {
    use serde_json::Value;

    let legacy = |pane: &Value| pane.get("linked").is_some();
    // The arrangement from before chartbooks is one book at the top level.
    let mut books: Vec<&mut Value> = match workspace.get("books").is_some() {
        true => match workspace["books"].as_array_mut() {
            Some(books) => books.iter_mut().collect(),
            None => return false,
        },
        false => vec![workspace],
    };
    let any = books.iter().any(|book| {
        book["panes"].as_array().is_some_and(|panes| panes.iter().any(legacy))
    });
    if !any {
        return false;
    }

    // Which group each list drove: what its row says, the default list group
    // 1 when nothing was written, and a group two lists claimed stays with
    // the earlier in display order, as the rail decided it.
    let mut driven: Vec<(i64, u8)> = Vec::new();
    for (id, _) in store.watchlists() {
        let group = match store.setting(&format!("watchlist_link_{id}")) {
            Some(value) => value.parse::<u8>().ok().filter(|n| (1..=9).contains(n)),
            None => (id == crate::store::DEFAULT_WATCHLIST).then_some(1),
        };
        if let Some(group) = group
            && !driven.iter().any(|(_, taken)| *taken == group)
        {
            driven.push((id, group));
        }
    }
    // A book written before it kept a list of its own showed the rail's.
    let rail = store
        .setting(crate::ui::watchlist::SETTING_ACTIVE)
        .and_then(|id| id.parse::<i64>().ok())
        .unwrap_or(crate::store::DEFAULT_WATCHLIST);

    for book in books.iter_mut() {
        let list = book["watchlist"].as_i64().unwrap_or(rail);
        let drives = driven.iter().find(|(id, _)| *id == list).map(|(_, group)| *group);
        let Some(panes) = book.get_mut("panes").and_then(Value::as_array_mut) else { continue };
        for pane in panes.iter_mut().filter(|p| legacy(p)) {
            let Some(fields) = pane.as_object_mut() else { continue };
            let group = match fields.remove("linked") {
                Some(Value::Bool(linked)) => u8::from(linked),
                Some(Value::Number(n)) => n
                    .as_f64()
                    .filter(|n| n.fract() == 0.0 && (1.0..=9.0).contains(n))
                    .map_or(0, |n| n as u8),
                _ => 0,
            };
            if !fields.contains_key("link") {
                let mode = LinkMode::from_group(group, drives);
                fields.insert("link".to_string(), Value::from(mode.key()));
            }
        }
    }
    true
}

/// The symbol and venue a chart is written down under.
///
/// The instrument on screen is the answer whenever there is one. Failing that
/// it is whatever the chart was asked for and the inventory could not name
/// yet — because the alternative is an empty symbol, and an empty symbol saved
/// over a stored one is somebody's chart gone. Only a chart that genuinely
/// never had a symbol stores as empty.
///
/// Free-standing so that the choice can be tested without a window, which is
/// where the loss happened: the save runs long before anybody looks.
fn stored_symbol(
    instrument: Option<&Instrument>,
    pending: Option<&(String, Option<String>)>,
) -> (String, Option<String>) {
    match (instrument, pending) {
        (Some(instrument), _) => (instrument.symbol.clone(), instrument.suffix.clone()),
        (None, Some((symbol, suffix))) => (symbol.clone(), suffix.clone()),
        (None, None) => (String::new(), None),
    }
}

/// What a chart opens on when there is nothing else at all: no symbol stored,
/// and a watchlist whose every row has been deleted or no longer names anything
/// the inventory knows.
///
/// The broadest thing there is, and the point of it is only that it is never
/// nothing: a blank window on a first launch reads as an app that failed to
/// start.
const LAST_RESORT_SYMBOL: &str = "GSPC";

/// Which symbols a window with no arrangement to restore tries, best first.
///
/// The one last looked at, then the default watchlist from the top in display
/// order, then [`LAST_RESORT_SYMBOL`].
///
/// The watchlist is in there because the first thing anybody ever sees should
/// be something they can find in their own list. A hardcoded index was a symbol
/// that is not in it, so the app opened on something the rail beside it did not
/// mention.
///
/// A list rather than one answer because every candidate can fail to resolve.
/// An inventory changes under people, and a stored symbol or a watchlist row
/// that no longer names an instrument has to be stepped over rather than end
/// the search.
fn opening_candidates(
    stored: Option<(String, Option<String>)>,
    watchlist: &[crate::store::Section],
) -> Vec<(String, Option<String>)> {
    let mut wanted: Vec<(String, Option<String>)> = stored.into_iter().collect();
    wanted.extend(
        watchlist
            .iter()
            .flat_map(|section| section.entries.iter())
            .map(|entry| (entry.symbol.clone(), entry.suffix.clone())),
    );
    wanted.push((LAST_RESORT_SYMBOL.to_string(), None));
    wanted
}

/// How wide the window's corner controls are: the price axis's width, plus
/// the ring every pane carries inside its edge, so that over a chart the
/// corner stands exactly on the axis's column and its left edge is the
/// axis's rule. Held to it, not left to the buttons, which is what keeps the
/// rule straight.
pub(crate) const CORNER_WIDTH: i32 =
    crate::ui::chart::PRICE_AXIS_W as i32 + omacharts_engine::frame::RING_WIDTH as i32;

/// How tall the window's corner is before it has been laid out: its
/// buttons' top, a 22-pixel button with 2 of padding each side, and the 7
/// of padding under it in `.window-corner`.
const CORNER_HEIGHT: i32 = crate::theming::CORNER_BUTTON_TOP + 26 + 7;

/// Where the rail's + stands from the window's right edge: against the
/// toggle, the way the toggle stands against the menu.
pub(crate) const RAIL_ADD_END: i32 = CORNER_WIDTH - crate::theming::CORNER_PAD_START;

/// The drawing tools' toggle's 22-pixel glyph button, until it is laid out.
const TOOLS_TOGGLE_SIZE: i32 = 22;

/// How wide the drawing tools' toggle is, its padding in and its margins
/// out: what a widget measures includes the margins, and the toggle's start
/// margin is worked out from its width, so counting it crept the toggle a
/// pixel to the right on every pass.
fn tools_toggle_width(toggle: &gtk::ToggleButton) -> i32 {
    match toggle.measure(gtk::Orientation::Horizontal, -1).1 - toggle.margin_start() - toggle.margin_end() {
        width if width > 0 => width,
        _ => TOOLS_TOGGLE_SIZE,
    }
}

/// Bars already folded to a resolution, keyed by cache key and that
/// resolution, and shared with every pane drawing them.
type SeriesCache = Rc<RefCell<HashMap<(String, Timeframe), Rc<Vec<omacharts_engine::Bar>>>>>;

/// A dialog's "draw yourself again", handed to the rows it rebuilds. Optional
/// because it cannot exist until the rows it refers to do.
type Rebuild = Rc<RefCell<Option<Box<dyn Fn()>>>>;

pub struct Window {
    pub window: adw::ApplicationWindow,
    /// Every chart on screen. One of them is focused, and that is the one the
    /// keyboard, the menus and the symbol search act on.
    panes: RefCell<Vec<Rc<ChartPane>>>,
    /// How they are arranged. Splitting divides the focused pane; closing one
    /// hands its space to its sibling.
    layout: RefCell<Node>,
    focused: Cell<u32>,
    /// The chart that has the window to itself, if one does.
    ///
    /// Kept beside the tree rather than in it: maximizing is a way of looking
    /// at an arrangement, not a change to it, so the arrangement it came from
    /// is still there to go back to exactly. It is written down with the
    /// chartbook all the same, because a chart you left filling the window
    /// is the chart you expect to find filling it — and the corner it is put
    /// back from says there is more behind it.
    maximized: Cell<Option<u32>>,
    /// Where the pointer last was, in the window's own coordinates.
    pointer_at: Cell<Option<(f64, f64)>>,
    /// Where the pointer was when the keyboard last moved the focus, while
    /// it has not moved since. The charts' hover furniture — crosshair,
    /// link, gear, maximize — belongs to a hand on the mouse, and a pointer
    /// left resting over some chart is not one.
    hushed_at: Cell<Option<(f64, f64)>>,
    /// Every chartbook, as it is written down, including the one on screen.
    ///
    /// Only one book is ever built into widgets: `panes` and `layout` above
    /// are the live form of whichever is active, and the rest sit here as
    /// data. Switching writes the live one back into this list and builds the
    /// next, which is the same work `restore_workspace` does on launch and so
    /// has only one way to be wrong.
    books: RefCell<Vec<Chartbook>>,
    active: Cell<usize>,
    next_pane: Cell<u32>,
    /// The charts on screen as the chartbook template they are on last saw
    /// them, in layout order, so a change can be carried to the other books
    /// on it the way it was made. Empty for a book on none.
    template_leaves: RefCell<Vec<u32>>,
    /// What an open chart settings dialog reads off the focused chart, asked
    /// to read it again when something else changed it. Each says whether
    /// it is still on screen, and goes once it is not.
    chart_watchers: RefCell<Vec<Box<dyn Fn() -> bool>>>,
    /// A save is already queued, so a drag does not queue one per pixel.
    save_pending: Cell<bool>,
    /// The chart menu's stateful actions, so they can be re-pointed at
    /// whichever chart is focused.
    linked_action: RefCell<Option<gio::SimpleAction>>,
    bar_style_action: RefCell<Option<gio::SimpleAction>>,
    session_action: RefCell<Option<gio::SimpleAction>>,
    template_action: RefCell<Option<gio::SimpleAction>>,
    /// The chart menu's "Create new template from chart…", turned off while
    /// the chart it opens on has no indicators.
    template_from_chart_action: RefCell<Option<gio::SimpleAction>>,
    grid_action: RefCell<Option<gio::SimpleAction>>,
    bars_action: RefCell<Option<gio::SimpleAction>>,
    /// The indicator the open indicator menu is about.
    menu_indicator: RefCell<Option<u32>>,
    /// The profile the indicator menu was opened over, for merging it.
    menu_profile: RefCell<Option<crate::ui::chart::ProfileSpot>>,
    indicators_action: RefCell<Option<gio::SimpleAction>>,
    drawings_action: RefCell<Option<gio::SimpleAction>>,
    auto_scale_action: RefCell<Option<gio::SimpleAction>>,
    log_scale_action: RefCell<Option<gio::SimpleAction>>,
    drawing_config_action: RefCell<Option<gio::SimpleAction>>,
    drawing_settings_action: RefCell<Option<gio::SimpleAction>>,
    /// What was last copied, waiting to be put down somewhere.
    ///
    /// The window's rather than the system's: a drawing is a moment, a
    /// price and a look, and the only thing that can read one back is this
    /// application. Being the window's also means a copy carries between
    /// charts and chartbooks, which is most of what copying a drawing is
    /// for. An indicator is copied the same way, with its settings.
    clipboard: RefCell<Clipboard>,
    /// The drawing tools, on the left.
    drawing_bar: RefCell<Option<Rc<crate::ui::drawing_bar::DrawingBar>>>,
    /// Where the tree of charts is mounted, rebuilt whenever it changes.
    chart_host: gtk::Box,
    /// The row of chartbook tabs under the charts, empty and hidden until
    /// there is a second book to switch to.
    book_strip: gtk::Box,
    /// The window's own controls, floating over the top-right corner. Held on
    /// to because a chart's maximize button is in that corner too, and has to
    /// step aside when the rail is closed and the two would be in one place.
    corner: RefCell<Option<gtk::WindowHandle>>,
    /// The corner's watchlist button. Held because Ctrl+Shift+B decides what to do
    /// and the button is what does it: showing the rail through the button is
    /// what keeps its pressed state honest and the corner clearance correct.
    watchlist_toggle: RefCell<Option<gtk::ToggleButton>>,
    /// How wide the rail should be.
    ///
    /// Kept here rather than measured off the handle when it is wanted,
    /// because the handle only means anything once the window has been laid
    /// out — and a chartbook gets written down at moments when it has not.
    sidebar_width: Cell<i32>,
    /// A width has been put back, so what the handle says from here on is
    /// somebody's choice rather than a guess from a window with no size yet.
    sidebar_placed: Cell<bool>,
    /// How wide the rail was last put, which is narrower than it wants when
    /// the window has no room for the whole of it. The handle reading back
    /// this width is the window resizing; any other is somebody dragging.
    sidebar_laid: Cell<i32>,
    /// The clock settings, read once: every streamed tick asks for them, and
    /// an unset format is the desktop's, which is a GSettings lookup. Read
    /// again when a preference or a command changes them.
    clock_settings: Cell<Option<(omacharts_engine::clock::ChartZone, omacharts_engine::clock::TimeFormat)>>,
    store: Rc<Store>,
    index: crate::inventory::Inventory,
    theming: Rc<RefCell<Theming>>,
    search: Rc<SymbolSearch>,
    watchlist: RefCell<Option<Rc<Watchlist>>>,
    /// The feeds, top first, the default always last, and each with
    /// everything built around it. A cell because the order can change while
    /// the window is open — see [`Window::switch_feeds`], the one place that
    /// replaces it, in order. Which of them serves a chart is
    /// [`Window::route`]'s to say.
    feeds: RefCell<Vec<Rc<Feed>>>,
    /// Every trading service login's trading session, at most one each, kept for the
    /// life of the window.
    desk: Rc<crate::trading::Desk>,
    /// The logins and accounts, as Trading last stored them, for the
    /// headers to draw from. Read when the window opens and whenever Trading
    /// changes.
    trading: RefCell<crate::trading::View>,
    /// Everything trading puts in the window, none of it built until there
    /// is a login.
    trade: Trade,
    /// The Log, the corner notice and the main menu's dot.
    messages: log::Messages,
    /// The About dialog, while it is open.
    about: RefCell<Option<Rc<about::About>>>,
    /// Which feed a loader reply belongs to. Bumped by a switch, so a reply
    /// the old worker had already sent is recognised and dropped rather
    /// than painted onto a chart that has moved on.
    feed_generation: Cell<u64>,
    /// The refresh timer is installed, so a switch to another polled feed
    /// does not install a second one.
    refresh_timer_on: Cell<bool>,
    /// A drain is already on the frame clock, so a second wake before it
    /// runs has nothing to add.
    drain_scheduled: Cell<bool>,
    /// The one timer for asking a lost series again, at the earliest retry
    /// due. Replaced rather than added to when a loss is due sooner.
    retry_timer: RefCell<Option<glib::SourceId>>,
    /// Folded series, keyed by cache key and the resolution shown.
    ///
    /// The bars are the chart — there is nothing else to preload — so once a
    /// symbol has been fetched, switching to it is a read and a draw. This
    /// removes even the read, which is what makes arrowing back and forth over
    /// the same few symbols feel like nothing is happening at all.
    series: SeriesCache,
    /// The split, so the keyboard can show and hide the rail.
    ///
    /// A GtkPaned rather than an AdwOverlaySplitView: the rail is a table of
    /// numbers whose useful width depends on how many columns you have shown,
    /// and a split view has no handle to drag. Hiding it is hiding the child,
    /// which takes the handle with it.
    split: gtk::Paned,
    /// The resolution strip and what is on it.
    timeframes: RefCell<Vec<Timeframe>>,
}

impl Window {
    pub fn build(app: &adw::Application, store: Rc<Store>) -> Rc<Window> {
        // One feed for the life of the window: the loader's queue is paced to
        // its rules and the price cache is keyed by its id, so this is read
        // once here and nowhere else asks again.
        // The process holding the window is the one that may open a session
        // a login may have only one of; any other passes such a feed over
        // rather than open a second. A flag: no feed is built by setting it.
        omacharts_engine::providers::serve_from_here();
        // The flags first: a feed behind one, Sierra Chart's, is no feed at
        // all to the order until they are read, and the window would open on
        // the feeds under it.
        crate::features::adopt(&store);
        let feeds = crate::feeds::ordered(&store).into_iter().map(Arc::from).collect();
        Window::build_with(app, store, feeds)
    }

    /// The window on feeds of the caller's choosing, top first.
    ///
    /// Apart from [`Window::build`] so that something other than the stored
    /// setting can decide the feeds: `examples/live_window.rs` opens the real
    /// window on the synthetic streaming provider, which is not listed and
    /// cannot be chosen any other way.
    pub fn build_with(
        app: &adw::Application,
        store: Rc<Store>,
        providers: Vec<Arc<dyn Provider>>,
    ) -> Rc<Window> {
        store.seed_watchlist_if_empty(DEFAULTS);
        if let Some(top) = providers.first() {
            crate::feeds::note_running(top.id());
        }
        crate::zoom::adopt(&store);
        crate::features::adopt(&store);

        let index = crate::inventory::Inventory::curated();
        let theming = Rc::new(RefCell::new(Theming::load(&store)));
        theming.borrow_mut().carry_over_candle_colours(&store);
        theming.borrow_mut().apply();

        // A feed that streams gets the registry that shares, coalesces and
        // writes behind for every chart; one that does not gets nothing at
        // all, and no line below this one asks which it was.
        let mut receivers = Vec::with_capacity(providers.len());
        let feeds: Vec<Rc<Feed>> = providers
            .into_iter()
            .map(|provider| {
                let (sender, receiver) = async_channel::unbounded::<Response>();
                let feed = Rc::new(Feed::new(provider, &store, sender));
                receivers.push((Rc::downgrade(&feed), receiver));
                feed
            })
            .collect();

        let window = adw::ApplicationWindow::new(app);
        // Just the name. Which symbol you are looking at is written over each
        // chart, and with several of them the title could only ever name one.
        window.set_title(Some("Omacharts"));
        window.set_default_size(1280, 800);

        let split = gtk::Paned::new(gtk::Orientation::Horizontal);
        let chart_host = gtk::Box::new(gtk::Orientation::Vertical, 0);
        chart_host.set_hexpand(true);
        chart_host.set_vexpand(true);

        // Its own widget rather than something inside `chart_host`, which is
        // emptied and refilled every time the tree changes.
        let book_strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        book_strip.add_css_class("chartbook-strip");
        book_strip.set_visible(false);

        let this = Rc::new(Window {
            window: window.clone(),
            panes: RefCell::new(Vec::new()),
            layout: RefCell::new(Node::leaf(1)),
            focused: Cell::new(1),
            maximized: Cell::new(None),
            pointer_at: Cell::new(None),
            hushed_at: Cell::new(None),
            books: RefCell::new(Vec::new()),
            active: Cell::new(0),
            next_pane: Cell::new(1),
            template_leaves: RefCell::new(Vec::new()),
            chart_watchers: RefCell::new(Vec::new()),
            save_pending: Cell::new(false),
            linked_action: RefCell::new(None),
            bar_style_action: RefCell::new(None),
            session_action: RefCell::new(None),
            template_action: RefCell::new(None),
            template_from_chart_action: RefCell::new(None),
            grid_action: RefCell::new(None),
            bars_action: RefCell::new(None),
            menu_indicator: RefCell::new(None),
            menu_profile: RefCell::new(None),
            indicators_action: RefCell::new(None),
            drawings_action: RefCell::new(None),
            auto_scale_action: RefCell::new(None),
            log_scale_action: RefCell::new(None),
            drawing_config_action: RefCell::new(None),
            drawing_settings_action: RefCell::new(None),
            clipboard: RefCell::new(Clipboard::Empty),
            drawing_bar: RefCell::new(None),
            chart_host: chart_host.clone(),
            book_strip: book_strip.clone(),
            corner: RefCell::new(None),
            watchlist_toggle: RefCell::new(None),
            sidebar_width: Cell::new(DEFAULT_SIDEBAR_WIDTH),
            sidebar_placed: Cell::new(false),
            sidebar_laid: Cell::new(0),
            clock_settings: Cell::new(None),
            store: store.clone(),
            index: index.clone(),
            theming: theming.clone(),
            search: SymbolSearch::new(index.clone()),
            watchlist: RefCell::new(None),
            feeds: RefCell::new(feeds),
            desk: Rc::new(crate::trading::Desk::new(crate::trading::registry())),
            trading: RefCell::new(crate::trading::View::load(&store)),
            trade: Trade::default(),
            messages: log::Messages::new(omacharts_engine::log::global()),
            about: RefCell::new(None),
            feed_generation: Cell::new(0),
            refresh_timer_on: Cell::new(false),
            drain_scheduled: Cell::new(false),
            retry_timer: RefCell::new(None),
            series: Rc::new(RefCell::new(HashMap::new())),
            split: split.clone(),
            timeframes: RefCell::new(parse_timeframes(
                store.setting(SETTING_TIMEFRAMES).as_deref(),
            )),
        });

        // A root opened in the picker asks the feed serving it what it
        // lists, through the order the window charts from.
        let ordered = store.clone();
        this.search.list_contracts_through(move |root, done| {
            crate::feeds::list_contracts(crate::feeds::order(&ordered), root, done);
        });
        this.watch_trading();
        this.watch_log();
        this.sign_in_after_first_frame();
        this.update_after_first_frame();
        let watchlist = this.build_watchlist();
        // Which list the rail is showing is part of the chartbook, so every
        // way of changing it has to write the book down. One hook, because a
        // second route to the same state is a second route that can forget.
        let saver = this.clone();
        watchlist.connect_switched(move || {
            // The list it is now showing follows the Follow charts rather than
            // leading them. Being shown a list is not picking a symbol, and a
            // rail that led here would move charts for a glance at a list.
            saver.rail_follows_follow_charts();
            saver.save_soon();
        });
        *this.watchlist.borrow_mut() = Some(watchlist.clone());

        // Which chartbook has a watchlist is a question only the window can
        // answer, and the switcher is where it gets asked.
        let books = this.clone();
        watchlist.connect_chartbook_using(move |id| books.book_using_watchlist(id));

        let split = &this.split;
        split.set_end_child(Some(&watchlist.widget));
        // The chart takes the room a wider window gives; the rail keeps the
        // width it was left at, because its columns do not get more useful.
        split.set_resize_start_child(true);
        split.set_resize_end_child(false);
        split.set_shrink_end_child(false);
        watchlist.widget.set_visible(store.setting_bool(SHOW_WATCHLIST, true));

        // The tools stand beside the charts and push them over, the way the
        // rail does on the right: laid over them, they hid the first bars and
        // the strips' names under a chart down the left edge. They are not
        // part of the split, so the divider between charts and rail is still
        // the only divider.
        let bar = crate::ui::drawing_bar::DrawingBar::new(
            {
                let this = this.clone();
                move |kind, sticky| {
                    let pane = this.focused_pane();
                    pane.view.arm_sticky(kind, sticky);
                    pane.view.area.grab_focus();
                    this.sync_drawing_bar();
                }
            },
            {
                let this = this.clone();
                move |kind| this.open_drawing_configurations(kind)
            },
            {
                let this = this.clone();
                move || this.toggle_drawing_tools()
            },
            {
                let this = this.clone();
                move || this.arm_ruler()
            },
        );
        let with_tools = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        with_tools.append(&bar.root);
        with_tools.append(&chart_host);
        bar.toggle.add_css_class("tools-toggle");
        bar.toggle.set_halign(gtk::Align::Start);
        bar.toggle.set_valign(gtk::Align::End);
        bar.set_shown(store.setting_bool(SHOW_DRAWING_TOOLS, false));
        *this.drawing_bar.borrow_mut() = Some(bar.clone());
        split.set_start_child(Some(&with_tools));

        // No header bar: the window's controls float over its top-right
        // corner, and the chart gets the row the bar used to take.
        let corner = this.build_corner(&this.split);
        *this.corner.borrow_mut() = Some(corner.clone());
        // The window's bottom corners, at the time axis's height: the
        // drawing tools' toggle on the left and, with a login, the trading
        // panel's on the right. Laid over the charts, the tools and the
        // rail together, so each stands at one place in the window whether
        // the bar or the rail beside it is out or put away, and only its lit
        // state changes; what it stands over moves around it.
        let corners = gtk::Overlay::new();
        corners.set_child(Some(split));
        corners.add_overlay(&bar.toggle);
        *this.trade.corners.borrow_mut() = Some(corners.clone());
        // The strip runs under the whole window, rail included. A row of tabs
        // stopping where the rail began would read as part of the chart area,
        // and a chartbook is not: the rail belongs to one as much as the
        // charts do.
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        corners.set_vexpand(true);
        root.append(&corners);
        root.append(&book_strip);

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&root));
        overlay.add_overlay(&corner);
        *this.trade.overlay.borrow_mut() = Some(overlay.clone());
        *this.trade.root.borrow_mut() = Some(root.clone());
        this.watch_sidebar_width();
        window.set_content(Some(&overlay));

        // The first chart. Everything else is a split of this one.
        let first = this.new_default_pane(
            store
                .setting(LAST_TIMEFRAME)
                .and_then(|k| Timeframe::parse(&k))
                .unwrap_or(Timeframe::days(1)),
            store.indicators(),
        );
        *this.layout.borrow_mut() = Node::leaf(first.id);
        this.focused.set(first.id);
        this.rebuild_layout();
        // After the first pane: the chart menu's radio items are built from
        // the focused chart, and until now there was not one.
        this.install_chart_actions();
        // Trading's furniture, if there is a login to trade through.
        this.sync_trading();

        this.wire_shortcuts();
        this.wire_hush();
        for (feed, receiver) in receivers {
            this.wire_responses(feed, receiver);
        }
        this.wire_theme_polling();
        this.wire_backfill();
        this.wire_refresh();
        this.wire_live();
        this.wire_entitlements();
        this.wire_rolls();
        this.wire_close();

        // The widget is part of the app, so it arrives with it rather than
        // waiting to be discovered in the settings.
        let installer = this.clone();
        glib::idle_add_local_once(move || installer.offer_bar_widget());

        // The long tail of listings arrives on a thread once the window is up:
        // the names you are most likely to type are already in the curated half.
        //
        // Which means the restoration below runs against the curated half and
        // cannot name a listing outside it — the swap happens on the main loop,
        // and the main loop is waiting for this constructor to return. So the
        // charts that could not be resolved are filled in from here, once —
        // and the rail, which left out its symbols outside the curated half.
        let waiting = this.clone();
        crate::inventory::load_in_background(this.index.clone(), move |_| {
            waiting.resolve_pending();
            let rail = waiting.watchlist.borrow().as_ref().cloned();
            if let Some(rail) = rail {
                rail.rebuild();
            }
        });

        if !this.restore_workspace() {
            this.restore_last_symbol();
        }
        // A window that restored nothing still has a chartbook: the one it
        // just built. Everything below here can assume there is always one.
        if this.books.borrow().is_empty() {
            let book = this.capture_book(String::new());
            this.books.borrow_mut().push(book);
            this.active.set(0);
        }
        this.rebuild_book_strip();
        // Either way. The legend was only built on the path that did not
        // restore anything, so a window that came back with its charts came
        // back without the names of what was drawn on them.
        this.rebuild_indicator_legend();
        this
    }

    // -- panes -------------------------------------------------------------

    /// A chart as a new one opens: the defaults from the preferences, and the
    /// template they name in place of `indicators` when they name one.
    fn new_default_pane(
        self: &Rc<Self>,
        timeframe: Timeframe,
        indicators: Vec<Indicator>,
    ) -> Rc<ChartPane> {
        let defaults = ChartDefaults::load(&self.store);
        let template = defaults.template.and_then(|id| crate::templates::get(&self.store, id));
        let pane = self.new_pane(
            timeframe,
            template.as_ref().map(|t| t.indicators.clone()).unwrap_or(indicators),
            defaults.bar_style,
            defaults.look(),
            defaults.session,
            defaults.show_grid,
            LinkMode::default(),
        );
        pane.template.set(template.map(|t| t.id));
        self.show_header_on(&pane);
        pane.continuous.set(defaults.continuous);
        pane.view.set_price_log(defaults.log_scale);
        pane.set_ohlc_thickness(defaults.ohlc_thickness);
        pane.shows_drawings.set(defaults.shows_drawings);
        pane.sends_drawings.set(defaults.sends_drawings);
        pane.view.set_sends(defaults.sends_drawings);
        pane.shows_legend.set(defaults.shows_legend);
        pane
    }

    /// Build a chart and wire it to this window. Not placed in the layout:
    /// the caller decides where it goes.
    #[allow(clippy::too_many_arguments)]
    fn new_pane(
        self: &Rc<Self>,
        timeframe: Timeframe,
        indicators: Vec<Indicator>,
        bar_style: BarStyle,
        look: StoredColouring,
        session: Session,
        show_grid: bool,
        mode: LinkMode,
    ) -> Rc<ChartPane> {
        let id = self.next_pane.get();
        self.next_pane.set(id + 1);

        let theme = self.theming.borrow().theme();
        let scheme = look.colouring.scheme(&theme).worn(look.swapped, look.hollow_up);
        let pane = ChartPane::new(
            id, theme, scheme, timeframe, indicators, bar_style, session, show_grid, mode,
        );
        pane.colouring.set(look.colouring);
        pane.swapped.set(look.swapped);
        pane.hollow_up.set(look.hollow_up);
        let (house, swapped) = self.house_look();
        pane.view.set_house_look(house, swapped);
        pane.view.set_bar_style(bar_style);
        pane.view.set_show_grid(show_grid);

        let searcher = self.clone();
        pane.symbol_button.connect_clicked(move |_| {
            searcher.focus(id);
            searcher.open_search();
        });

        let opener = self.clone();
        pane.gear.connect_clicked(move |_| {
            opener.focus(id);
            opener.open_chart_settings();
        });

        // A hidden thing's glyph means that chart's: the focus moves there
        // first, the way the gear's does, and then it is shown.
        for (what, button) in &pane.hidden_glyphs {
            let (shower, what) = (self.clone(), *what);
            button.connect_clicked(move |_| {
                shower.focus(id);
                match what {
                    Hideable::Drawings => shower.set_drawings_shown(true),
                    Hideable::Indicators => shower.set_show_indicators(true),
                    Hideable::Bars => shower.set_bars_hidden(false),
                }
            });
        }

        // Clicking another chart's corner means that chart: the focus moves
        // there first, and then it grows.
        let expander = self.clone();
        pane.expand.connect_clicked(move |_| {
            expander.focus(id);
            expander.toggle_maximized();
        });

        // The account opens the accounts, the feed's dot the feeds that
        // could serve this chart, and the quantity the template's
        // quantities: native menus, each built afresh each time it opens,
        // since which there are, which can be used and what the chart is
        // of all change underneath them. A choice is a stateful action, so
        // GTK draws its mark.
        self.add_menu_actions(&pane);
        let builder = Rc::downgrade(self);
        pane.account_menu.set_create_popup_func(move |button| {
            if let Some(this) = builder.upgrade() {
                crate::ui::menus::set_model(button, &this.account_menu(id));
            }
        });
        let builder = Rc::downgrade(self);
        pane.set_feed_menu(Rc::new(move || match builder.upgrade() {
            Some(this) => this.feed_menu(id),
            None => gio::Menu::new(),
        }));
        // On a right click the quantity also offers to edit the templates,
        // as the strip does its list.
        let builder = Rc::downgrade(self);
        pane.size_menu.set_create_popup_func(move |button| {
            if let Some(this) = builder.upgrade() {
                crate::ui::menus::set_model(button, &this.size_menu(id));
            }
        });
        let editor = self.clone();
        let size_menu = gtk::GestureClick::new();
        size_menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let (part, anchor) = (pane.size_menu.clone(), pane.header_row.clone());
        size_menu.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            editor.focus(id);
            // Over the line it is in, since the button has a popover of its
            // own already.
            let at = part.compute_point(&anchor, &gtk::graphene::Point::new(x as f32, y as f32));
            let (x, y) = at.map_or((x, y), |at| (at.x() as f64, at.y() as f64));
            popup_menu(&trade::size_part_menu(), &anchor, x, y);
        });
        pane.size_menu.add_controller(size_menu);
        // Buy and Sell are the keys' orders, for this chart.
        for (button, side) in [(&pane.buy_button, crate::trading::Side::Buy), (&pane.sell_button, crate::trading::Side::Sell)] {
            let this = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                let Some(this) = this.upgrade() else { return };
                this.focus(id);
                if let Some(pane) = this.pane(id) {
                    this.market_on(&pane, side);
                }
            });
        }

        // The chain opens the three modes rather than toggling one: "off" is
        // two answers, taking the click when focused and never taking it.
        // The same three rows as the chart menu's Watchlist submenu. By id
        // rather than by holding the pane: the pane owns the button that
        // owns this menu.
        let marked = Rc::downgrade(self);
        let choosing = Rc::downgrade(self);
        crate::ui::menus::choice(
            &pane.link,
            "link",
            link::MODES.iter().map(|mode| (mode.label().to_string(), mode.key().to_string())).collect(),
            move || {
                let mode = marked.upgrade().and_then(|window| window.pane(id)).map(|pane| pane.mode()).unwrap_or_default();
                mode.key().to_string()
            },
            move |key| {
                let (Some(window), Some(mode)) = (choosing.upgrade(), LinkMode::from_key(key)) else { return };
                window.focus(id);
                window.set_pane_mode(id, mode);
            },
        );
        pane.set_mode(mode, mode.colour(&self.theming.borrow().theme()));

        // Right-clicking a strip offers to edit the list, rather than throwing
        // a modal at you for a click you may not have meant. The list is one
        // list for the window: every chart offers the same resolutions, and
        // picks its own from them.
        let editor = self.clone();
        let strip_menu = gtk::GestureClick::new();
        strip_menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let strip = pane.strip.clone();
        strip_menu.connect_pressed(move |gesture, _, x, y| {
            // Its own menu, not the header's as well, when it is in the
            // header's line.
            gesture.set_state(gtk::EventSequenceState::Claimed);
            editor.focus(id);
            popup_menu(&strip_menu_model(), &strip, x, y);
        });
        pane.strip.add_controller(strip_menu);

        // The title is typed where it stands: clicked, it is an entry, and
        // what is typed there is kept like a title given in the menu.
        let titler = Rc::downgrade(self);
        pane.title_button.connect_clicked(move |_| {
            let Some(window) = titler.upgrade() else { return };
            window.focus(id);
            if let Some(pane) = window.pane(id) {
                pane.begin_title_edit();
            }
        });
        let keeper = Rc::downgrade(self);
        pane.set_title_handler(move |title| {
            if let Some(window) = keeper.upgrade() {
                window.set_title_of(id, &title);
            }
        });

        // And the header, the same way, for what it shows: one line for every
        // chart, set in the preferences, and reached from here because here
        // is where somebody is looking when they want it changed. Either of
        // its sections.
        for header in [pane.header_row.clone(), pane.header_center.clone()] {
            let editor = self.clone();
            let header_menu = gtk::GestureClick::new();
            header_menu.set_button(gtk::gdk::BUTTON_SECONDARY);
            let anchor = header.clone();
            header_menu.connect_pressed(move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                editor.focus(id);
                let model = gio::Menu::new();
                model.append(Some("Edit header…"), Some("chart.edit-header"));
                popup_menu(&model, &anchor, x, y);
            });
            header.add_controller(header_menu);
        }

        let axis_owner = self.clone();
        pane.view.set_axis_menu_handler(move |x, y| {
            axis_owner.focus(id);
            axis_owner.price_axis_menu(x, y);
        });

        let menu_owner = self.clone();
        pane.view.set_context_menu_handler(move |x, y| {
            menu_owner.focus(id);
            menu_owner.chart_menu(x, y);
        });

        // What the hand drew is written down by the symbol it was drawn on,
        // and every chart showing that symbol is told.
        let keeper = self.clone();
        pane.view.set_drawing_handler(move |event| keeper.record_drawing(id, event));
        // What the hand did to the orders and the position on it, which the
        // window sends, never the chart.
        let trader = self.clone();
        pane.view.set_mark_handler(move |event| {
            if let Some(event) = marks::trade_event(event) {
                trader.trade_from_chart(id, event);
            }
        });

        let menu_owner = self.clone();
        pane.view.set_drawing_menu_handler(move |x, y| {
            menu_owner.focus(id);
            menu_owner.drawing_menu(x, y);
        });
        let opener = self.clone();
        {
            // While a caret is in a label, Ctrl+B is bold and Ctrl+I is
            // italic. Those chords belong to the rail and the indicators the
            // rest of the time, and an application accelerator is owned
            // everywhere — GTK activates it at the window before the editor
            // is offered the key — so the editor can only have them by the
            // accelerators standing down while it is open.
            let window = self.window.clone();
            pane.view.set_typing_handler(move |typing| {
                if let Some(app) = window.application().and_downcast::<adw::Application>() {
                    shortcuts::lend_to_text_editor(&app, typing);
                }
            });
        }

        let picker = self.clone();
        pane.view.set_indicator_handler(move |event| picker.indicator_event(id, event));

        pane.view.set_drawing_properties_handler(move || {
            opener.focus(id);
            opener.open_drawing_settings();
        });
        let shower = self.clone();
        pane.view.set_tool_handler(move || {
            if shower.focused.get() == id {
                shower.sync_drawing_bar();
                // A tool picked up on a chart with its drawings hidden shows
                // them again, and that is stored like the key would store it.
                // The menu's tick still says hidden until it is told.
                let ticked = shower.drawings_action.borrow().as_ref().and_then(|a| a.state());
                let was_hidden = ticked.and_then(|v| v.get::<bool>()) == Some(false);
                if was_hidden && shower.drawings_shown() {
                    let pane = shower.focused_pane();
                    shower.sync_chart_actions(&pane);
                    shower.save_soon();
                }
            }
        });

        // A drag or a wheel on the price axis takes the scale off automatic,
        // or moves a held one, inside the chart, where nothing is written
        // down. It is a setting like the gridlines, so it is stored the way
        // they are, once the wheel or the drag stops.
        let scaler = self.clone();
        pane.view.set_price_auto_handler(move |auto| scaler.price_auto_changed(id, auto));

        // Scrolled back toward its first bar, a stitched root brings the
        // months before it.
        let filler = self.clone();
        pane.view.set_history_handler(move |first| filler.fill_older(id, first));

        // Dragging a pane's edge changes the indicator, not just the drawing,
        // so the new height is stored the way any other setting of its would be.
        let resizer = self.clone();
        pane.view.set_pane_resize_handler(move |indicator_id, share| {
            let Some(pane) = resizer.pane(id) else { return };
            let mut indicators = pane.indicators.borrow().clone();
            if let Some(indicator) = indicators.iter_mut().find(|i| i.id == indicator_id) {
                match &mut indicator.params {
                    omacharts_engine::Params::Volume { height, .. }
                    | omacharts_engine::Params::Rsi { height, .. }
                    | omacharts_engine::Params::Macd { height, .. }
                    | omacharts_engine::Params::Atr { height, .. }
                    | omacharts_engine::Params::Stochastic { height, .. } => *height = share,
                    _ => return,
                }
                // On a template, the template takes the height, and the
                // charts on it keep one shape.
                resizer.change_indicators_of(&pane, indicators);
            }
        });

        // The arrows in a strip's corner move it through the strips on its
        // side of the price. The order lives with the indicators, which is the
        // same order the settings list drags around — one order, two ways to
        // change it.
        let mover = self.clone();
        pane.view.set_pane_move_handler(move |indicator_id, direction| {
            let Some(pane) = mover.pane(id) else { return };
            let mut indicators = pane.indicators.borrow().clone();
            if omacharts_engine::indicators::move_strip(&mut indicators, indicator_id, direction) {
                mover.change_indicators_of(&pane, indicators);
            }
        });

        // The × in a strip's corner takes that indicator off this chart.
        let closer = self.clone();
        pane.view.set_pane_close_handler(move |indicator_id| {
            let Some(pane) = closer.pane(id) else { return };
            let kept: Vec<Indicator> = pane
                .indicators
                .borrow()
                .iter()
                .filter(|i| i.id != indicator_id)
                .cloned()
                .collect();
            closer.change_indicators_of(&pane, kept);
        });

        // The pointer on a chart draws a crosshair on the other charts of its
        // symbol, so you can read the same moment and the same price on all
        // of them at once. Both halves travel as values rather than as
        // pixels: charts at different resolutions share no bar index, and
        // charts at different zooms share no y.
        let echoer = self.clone();
        pane.view.set_hover_handler(move |hover| {
            echoer.echo_crosshair(id, hover.map(|h| Echo { ts: h.bar.ts, price: h.price }));
        });

        // Clicking anywhere on a chart focuses it, which is what makes the
        // next keystroke land where you are looking.
        //
        // The keyboard has to come with it, and GTK does not move it for a
        // drawing area the way it would for a button. Without this the ring
        // moved to the chart you clicked while the keyboard stayed wherever
        // it was — so the next key went to the rail you had just clicked
        // away from, not to the chart the ring was on.
        //
        // Except on the text editor. This runs on the way *down*, before the
        // widget under the pointer sees the press at all, so a click meant
        // to put the caret somewhere in a label was taking the keyboard off
        // the editor first — which commits the edit and takes the editor off
        // the chart, so the press that followed landed on bare chart. Every
        // pointer gesture inside a label died that way: no caret placement,
        // no drag to select, no double-click to take a word.
        let focuser = self.clone();
        let click = gtk::GestureClick::new();
        click.set_button(0);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let root_for_pick = pane.root.clone();
        click.connect_pressed(move |_, _, x, y| {
            focuser.focus(id);
            if root_for_pick
                .pick(x, y, gtk::PickFlags::DEFAULT)
                .is_some_and(|under| Window::is_editor(&under))
            {
                return;
            }
            if let Some(pane) = focuser.pane(id) {
                pane.view.area.grab_focus();
            }
        });
        pane.root.add_controller(click);

        self.panes.borrow_mut().push(pane.clone());
        self.rebuild_strip_of(&pane);
        self.rebuild_legend_of(&pane);
        pane
    }

    /// Whether the point is on a chart's open text editor.
    ///
    /// Asked of the widget actually under the pointer rather than of a
    /// rectangle worked out by hand, so it stays true however the editor is
    /// placed. Its class is the only thing it has to be recognised by, which
    /// is enough: there is one editor and it wears it.
    fn is_editor(widget: &gtk::Widget) -> bool {
        let mut at = Some(widget.clone());
        while let Some(widget) = at {
            if widget.has_css_class(crate::ui::chart::TEXT_EDITOR_CLASS) {
                return true;
            }
            at = widget.parent();
        }
        false
    }

    pub fn focused_pane(&self) -> Rc<ChartPane> {
        let id = self.focused.get();
        let panes = self.panes.borrow();
        panes
            .iter()
            .find(|p| p.id == id)
            .or_else(|| panes.first())
            .cloned()
            .expect("at least one chart")
    }

    fn pane(&self, id: u32) -> Option<Rc<ChartPane>> {
        self.panes.borrow().iter().find(|p| p.id == id).cloned()
    }

    /// Bring the bars before `first`, chart `id`'s first bar, now that its
    /// view has come near it: for a stitched root, whose first load reaches
    /// back only so far, the stretch before it, read off the window's thread
    /// and stitched as the chart's own are, moved by every roll after it.
    /// A feed with nothing older brings nothing, and is not asked again
    /// until the chart has another first bar. The bars come in before the
    /// ones shown, which stay where they are, and are kept with them.
    fn fill_older(self: &Rc<Self>, id: u32, first: i64) {
        let Some(pane) = self.pane(id) else { return };
        let Some(key) = pane.served.borrow().clone() else { return };
        let Some((_, symbol)) = key.split_once(':') else { return };
        if omacharts_engine::continuous::Spelled::parse(symbol).is_none() {
            return;
        }
        let Some(feed) = self.feed_of(&key) else { return };
        let native = pane.timeframe.get().native();
        let reach = match native.is_intraday() {
            true => OLDER_INTRADAY_DAYS * 86_400,
            false => OLDER_DAILY_DAYS * 86_400,
        };
        let (provider, symbol) = (feed.provider.clone(), symbol.to_string());
        let (sender, older) = async_channel::bounded(1);
        let read = std::thread::Builder::new().name("omacharts older bars".into()).spawn(move || {
            let _ = sender.send_blocking(provider.bars_between(&symbol, native, first.saturating_sub(reach), first));
        });
        if read.is_err() {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let Ok(Ok(older)) = older.recv().await else { return };
            this.take_older(&key, native, first, older);
        });
    }

    /// Put `older`, bars from before `first`, under the series `key` at
    /// `native` and on every chart of it.
    fn take_older(self: &Rc<Self>, key: &str, native: Timeframe, first: i64, older: Vec<omacharts_engine::Bar>) {
        let older: Vec<_> = older.into_iter().filter(|bar| bar.ts < first).collect();
        if older.is_empty() {
            return;
        }
        let Some(feed) = self.feed_of(key) else { return };
        if !crate::loader::drawn_short(key, native) {
            self.store.merge_bars(key, native, &older);
        }
        let series = Series { key: key.to_string(), native };
        if let Some(live) = feed.live.as_ref()
            && let Some(bars) = live.with_bars(&series, |bars| bars.to_vec())
        {
            if bars.first().is_some_and(|bar| bar.ts == first) {
                let mut joined = older;
                joined.extend(bars);
                if let Some(change) = live.merged(&series, joined) {
                    self.apply_live_change(live, change);
                }
            }
            return;
        }
        let bars = self.store.load_bars(key, native);
        let panes: Vec<_> = self.panes.borrow().clone();
        for pane in panes {
            if pane.served.borrow().as_deref() != Some(key) || pane.timeframe.get().native() != native {
                continue;
            }
            let instrument = pane.instrument.borrow().clone();
            let Some(instrument) = instrument else { continue };
            self.paint(&pane, key, &instrument, pane.timeframe.get(), bars.clone());
        }
    }

    /// Make `id` the chart the keyboard and the menus act on.
    pub fn focus(self: &Rc<Self>, id: u32) {
        if self.focused.get() == id && self.panes.borrow().iter().any(|p| p.id == id) {
            return;
        }
        // The tool in hand goes with the focus, so every chart is drawn on
        // with the one the bar shows.
        if let (Some(from), Some(to)) = (self.pane(self.focused.get()), self.pane(id)) {
            from.view.hand_over(&to.view);
        }
        self.focused.set(id);
        // Maximized, the focus keys still walk the arrangement — they just
        // walk it one chart at a time, with the window showing whichever one
        // the focus has reached. Stepping out of the maximized chart to look
        // at its neighbour is the whole reason to press them while one chart
        // is filling the window.
        if self.maximized.get().is_some_and(|open| open != id) && self.pane(id).is_some() {
            self.maximized.set(Some(id));
            self.rebuild_layout();
        }
        for pane in self.panes.borrow().iter() {
            pane.set_focused(pane.id == id);
        }
        // What is left in the header describes the focused chart, as do the
        // chart menu's radio items.
        self.sync_header();
    }

    /// The chart menu's radio items describe the focused chart, so they have
    /// to be re-pointed when the focus moves.
    fn sync_chart_actions(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        if let Some(action) = self.linked_action.borrow().as_ref() {
            action.set_state(&pane.mode().key().to_variant());
        }
        if let Some(action) = self.bar_style_action.borrow().as_ref() {
            action.set_state(&bar_style_state(pane).to_variant());
        }
        if let Some(action) = self.bars_action.borrow().as_ref() {
            action.set_state(&(!pane.bars_hidden.get()).to_variant());
        }
        if let Some(action) = self.session_action.borrow().as_ref() {
            action.set_state(&pane.session.get().key().to_variant());
        }
        if let Some(action) = self.template_action.borrow().as_ref() {
            action.set_state(&template_state(pane.template.get()));
        }
        if let Some(action) = self.grid_action.borrow().as_ref() {
            action.set_state(&pane.show_grid.get().to_variant());
        }
        if let Some(action) = self.indicators_action.borrow().as_ref() {
            action.set_state(&pane.view.show_indicators().to_variant());
        }
        if let Some(action) = self.drawings_action.borrow().as_ref() {
            action.set_state(&pane.view.show_drawings().to_variant());
        }
        if let Some(action) = self.auto_scale_action.borrow().as_ref() {
            action.set_state(&pane.view.price_auto().to_variant());
        }
        if let Some(action) = self.log_scale_action.borrow().as_ref() {
            action.set_state(&pane.view.price_log().to_variant());
        }
    }

    /// A gesture on chart `id` just changed whether its price scale is
    /// automatic.
    ///
    /// Written down on the same delay as a divider drag, because that is
    /// usually what it is: the hand has just closed on the axis and is about
    /// to pull it.
    fn price_auto_changed(self: &Rc<Self>, id: u32, auto: bool) {
        if self.focused.get() == id
            && let Some(action) = self.auto_scale_action.borrow().as_ref()
        {
            action.set_state(&auto.to_variant());
        }
        self.save_soon();
    }

    /// Point the header and the rail at the focused chart.
    fn sync_header(self: &Rc<Self>) {
        let pane = self.focused_pane();
        pane.sync_strip();
        self.sync_chart_actions(&pane);
        self.sync_drawing_bar();
        if let (Some(watchlist), Some(instrument)) =
            (self.watchlist.borrow().as_ref(), pane.instrument.borrow().as_ref())
        {
            watchlist.highlight(instrument);
        }
    }

    /// Divide the focused chart in two, the new one showing the same thing.
    ///
    /// Copying the chart you split is what makes splitting useful: you get two
    /// of what you were looking at and change one of them.
    pub fn split_focused(self: &Rc<Self>, horizontal: bool) {
        // Dividing a chart that is filling the window has to show you what it
        // divided into, so this is where maximizing ends.
        self.maximized.set(None);
        let from = self.focused_pane();
        let added = self.new_pane(
            from.timeframe.get(),
            from.indicators.borrow().clone(),
            from.bar_style.get(),
            pane_look(&from),
            from.session.get(),
            from.show_grid.get(),
            from.mode(),
        );
        added.template.set(from.template.get());
        self.show_header_on(&added);
        added.continuous.set(from.continuous.get());
        added.view.set_price_auto(from.view.price_auto());
        added.view.set_price_log(from.view.price_log());
        added.bars_hidden.set(from.bars_hidden.get());
        added.view.set_bars_hidden(from.bars_hidden.get());
        added.set_ohlc_thickness(from.ohlc_thickness.get());
        added.view.set_show_indicators(from.view.show_indicators());
        added.view.set_show_drawings(from.view.show_drawings());
        added.shows_legend.set(from.shows_legend.get());
        let next = self.layout.borrow().split(from.id, added.id, horizontal);
        *self.layout.borrow_mut() = next;
        self.rebuild_layout();
        if let Some(instrument) = from.instrument.borrow().clone() {
            self.show_in(&added, instrument);
        }
        self.focus(added.id);
        self.save_workspace();
    }

    /// Close the focused chart. The last one stays: a window with no chart in
    /// it is not a state worth being able to reach.
    pub fn close_focused(self: &Rc<Self>) {
        let id = self.focused.get();
        let Some(next) = self.layout.borrow().remove(id) else { return };
        if self.maximized.get() == Some(id) {
            self.maximized.set(None);
        }
        let leaves = next.leaves();
        *self.layout.borrow_mut() = next;
        self.panes.borrow_mut().retain(|p| p.id != id);
        self.release_pane(id);
        self.rebuild_layout();
        if let Some(first) = leaves.first() {
            self.focus(*first);
        }
        self.save_workspace();
    }

    /// Give the focused chart the whole window, or hand the window back.
    ///
    /// The tree is left alone: what changes is how much of it is mounted. So
    /// restoring is exact, down to where every divider was.
    pub fn toggle_maximized(self: &Rc<Self>) {
        if self.layout.borrow().leaves().len() < 2 {
            return;
        }
        let id = self.focused.get();
        let next = (self.maximized.get() != Some(id)).then_some(id);
        self.maximized.set(next);
        self.rebuild_layout();
    }

    /// Walk the focus to the next or previous chart in reading order — top
    /// to bottom, and left to right within a row — wrapping at either end.
    ///
    /// This is Alt+Left and Alt+Right, and they are not spatial on purpose.
    /// A spatial step left or right has nothing to go to at the end of a row,
    /// so in a chartbook of uneven rows the key did nothing, and a chart in
    /// the other row was out of reach of it. Reading order goes everywhere:
    /// in a plain row it is the same step a spatial one would take, and at
    /// the end of a row it carries on into the next. Taking the spatial step
    /// whenever there is one, and reading order only when there is not, would
    /// look kinder and is not: beside a tall chart, the spatial step can jump
    /// past a chart that the fallback then never lands on either.
    ///
    /// Maximized, `focus` hands the window to the chart reached, so the keys
    /// cycle through the charts one window at a time.
    pub fn focus_along(self: &Rc<Self>, step: i32) {
        let order = reading_order(&self.layout_rects());
        let Some(at) = order.iter().position(|id| *id == self.focused.get()) else { return };
        if order.len() < 2 {
            return;
        }
        let next = (at as i64 + step as i64).rem_euclid(order.len() as i64) as usize;
        self.focus(order[next]);
        self.hush_pointer();
    }

    /// Move the focus to the nearest chart in a direction.
    ///
    /// By where the charts are on screen, not by where they sit in the tree.
    /// Walking the tree means the pane to the right of this one is whichever
    /// leaf happens to come next in a depth-first order — from the top left of
    /// a column beside a tall chart, that is the pane *below*, and pressing
    /// right moved down. Geometry is what the arrow key is asking about.
    pub fn focus_towards(self: &Rc<Self>, dx: i32, dy: i32) {
        // Maximized, every chart but one is off the widget tree and has no
        // allocation to ask about — so the arrangement is measured from the
        // arrangement itself. Otherwise the real allocations are the truth,
        // gutters and all.
        let rects = self.maximized.get().map(|_| self.layout_rects());
        let rect = |id: u32| match &rects {
            Some(rects) => rects.get(&id).copied(),
            None => self.pane_rect(id),
        };
        let Some(from) = rect(self.focused.get()) else { return };
        let (fx, fy) = (from.0 + from.2 / 2.0, from.1 + from.3 / 2.0);

        let mut best: Option<(f64, u32)> = None;
        for pane in self.panes.borrow().iter() {
            if pane.id == self.focused.get() {
                continue;
            }
            let Some(rect) = rect(pane.id) else { continue };
            let (cx, cy) = (rect.0 + rect.2 / 2.0, rect.1 + rect.3 / 2.0);
            let (along, across) = if dx != 0 {
                ((cx - fx) * dx as f64, (cy - fy).abs())
            } else {
                ((cy - fy) * dy as f64, (cx - fx).abs())
            };
            // Strictly in the direction asked for, and then the nearest one,
            // with ties broken by how well the two charts line up.
            if along <= 1.0 {
                continue;
            }
            let score = along + across * 2.0;
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, pane.id));
            }
        }
        if let Some((_, id)) = best {
            self.focus(id);
            self.hush_pointer();
        }
    }

    /// Put the charts' hover furniture away until the pointer moves: the
    /// keyboard is driving, and a crosshair and three buttons wherever the
    /// pointer happens to rest — on the chart just left, or, maximized, on
    /// the one just swapped in under it — are only in the way.
    fn hush_pointer(&self) {
        let Some(at) = self.pointer_at.get() else { return };
        self.hushed_at.set(Some(at));
        self.chart_host.add_css_class("hushed");
        for pane in self.panes.borrow().iter() {
            pane.view.drop_pointer();
        }
    }

    /// Wire the hush's way out. Motion is held back from the charts while
    /// the pointer stands where it was hushed: GTK repeats the pointer's
    /// position to whatever is newly under it after a rebuild, and a chart
    /// would take that for a hand. A real move, or a press, lets it through.
    fn wire_hush(self: &Rc<Self>) {
        let legacy = gtk::EventControllerLegacy::new();
        legacy.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        legacy.connect_event(move |_, event| {
            use gtk::gdk::EventType;
            match event.event_type() {
                EventType::MotionNotify => {
                    let Some(at) = event.position() else { return glib::Propagation::Proceed };
                    this.pointer_at.set(Some(at));
                    match this.hushed_at.get() {
                        Some((x, y)) if (at.0 - x).abs() < 1.0 && (at.1 - y).abs() < 1.0 => {
                            glib::Propagation::Stop
                        }
                        Some(_) => {
                            this.unhush();
                            glib::Propagation::Proceed
                        }
                        None => glib::Propagation::Proceed,
                    }
                }
                EventType::ButtonPress | EventType::Scroll | EventType::TouchBegin => {
                    this.unhush();
                    glib::Propagation::Proceed
                }
                EventType::LeaveNotify => {
                    this.pointer_at.set(None);
                    glib::Propagation::Proceed
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.window.add_controller(legacy);
    }

    fn unhush(&self) {
        if self.hushed_at.take().is_some() {
            self.chart_host.remove_css_class("hushed");
        }
    }

    /// Where a chart sits, in the coordinates of the area the charts share.
    fn pane_rect(&self, id: u32) -> Option<(f64, f64, f64, f64)> {
        let pane = self.panes.borrow().iter().find(|p| p.id == id).cloned()?;
        let (x, y) = pane.root.translate_coordinates(&self.chart_host, 0.0, 0.0)?;
        Some((x, y, pane.root.width() as f64, pane.root.height() as f64))
    }

    /// Where every chart would sit, worked out from the tree rather than from
    /// the widgets — which is the only way to ask while one of them is
    /// filling the window and the rest are off the screen entirely.
    fn layout_rects(&self) -> HashMap<u32, (f64, f64, f64, f64)> {
        fn fill(node: &Node, x: f64, y: f64, w: f64, h: f64, out: &mut HashMap<u32, (f64, f64, f64, f64)>) {
            match node {
                Node::Leaf(id) => {
                    out.insert(*id, (x, y, w, h));
                }
                Node::Split { horizontal, ratio, first, second } => {
                    let ratio = ratio.clamp(0.05, 0.95);
                    if *horizontal {
                        fill(first, x, y, w * ratio, h, out);
                        fill(second, x + w * ratio, y, w * (1.0 - ratio), h, out);
                    } else {
                        fill(first, x, y, w, h * ratio, out);
                        fill(second, x, y + h * ratio, w, h * (1.0 - ratio), out);
                    }
                }
            }
        }
        let mut out = HashMap::new();
        let width = self.chart_host.width().max(1) as f64;
        let height = self.chart_host.height().max(1) as f64;
        fill(&self.layout.borrow(), 0.0, 0.0, width, height, &mut out);
        out
    }

    /// What is actually mounted: the whole tree, or the one chart that has
    /// been given the window.
    fn mounted(&self) -> Node {
        match self.filled() {
            Some(id) => Node::Leaf(id),
            None => self.layout.borrow().clone(),
        }
    }

    /// The chart filling the window, if one is and still can: a chart that
    /// has been closed cannot, and one chart alone has nothing to fill the
    /// window away from. What is mounted and what is written down both ask
    /// this, so they cannot disagree.
    fn filled(&self) -> Option<u32> {
        self.maximized
            .get()
            .filter(|id| self.pane(*id).is_some() && self.layout.borrow().leaves().len() > 1)
    }

    /// Mount the tree. Rebuilt whole rather than patched: a handful of panes,
    /// and a tree that is half old and half new is a bug nobody can see.
    fn rebuild_layout(self: &Rc<Self>) {
        while let Some(child) = self.chart_host.first_child() {
            self.chart_host.remove(&child);
        }
        for pane in self.panes.borrow().iter() {
            if let Some(parent) = pane.root.parent()
                && let Some(paned) = parent.downcast_ref::<gtk::Paned>()
            {
                if paned.start_child().as_ref() == Some(pane.root.upcast_ref()) {
                    paned.set_start_child(None::<&gtk::Widget>);
                } else {
                    paned.set_end_child(None::<&gtk::Widget>);
                }
            }
        }
        let layout = self.mounted();
        let widget = self.build_node(&layout, &[]);
        self.chart_host.append(&widget);
        let focused = self.focused.get();
        // One chart has nothing to be maximized away from, so the corner is
        // not offered at all until there is a second — and nothing is drawn
        // as maximized either, which is what `mounted` has already decided.
        let several = self.layout.borrow().leaves().len() > 1;
        let maximized = several.then(|| self.maximized.get()).flatten();
        for pane in self.panes.borrow().iter() {
            pane.set_focused(pane.id == focused);
            pane.set_expandable(several);
            pane.set_maximized(maximized == Some(pane.id));
        }
        self.sync_corner_clearance();
    }

    /// Keep a chart's maximize button out from under the window's own corner.
    ///
    /// They want the same few pixels, and only sometimes: the window's
    /// controls sit over the rail while the rail is open, and drop onto the
    /// top-right chart when it is closed. So the chart under them steps its
    /// button aside by exactly their width, and every other chart keeps the
    /// corner it had.
    fn sync_corner_clearance(self: &Rc<Self>) {
        // Through the toggle, not the rail's own visibility: gtk4-rs answers
        // that about the whole ancestor chain, so during launch it says the
        // rail is hidden and the cluster wears its backdrop over the rail —
        // until you toggle the sidebar and this runs again with the window on
        // screen, which is what made it look like a repaint bug.
        let railed = self.shows_sidebar();
        let clearance = match railed {
            true => 0,
            false => self
                .corner
                .borrow()
                .as_ref()
                .map(|corner| corner.width())
                .filter(|width| *width > 0)
                .unwrap_or(CORNER_WIDTH),
        };
        // Over a chart the cluster needs a ground of its own; over the rail it
        // has nothing behind it to be told apart from. Only the window knows
        // which of the two it is sitting on.
        if let Some(cluster) = self.corner.borrow().as_ref().and_then(|corner| corner.child()) {
            match railed {
                true => cluster.remove_css_class("over-chart"),
                false => cluster.add_css_class("over-chart"),
            }
        }

        // The rects tile the area exactly, so the chart under the window's
        // corner is the one — the only one — holding its top right pixel.
        // Compared within a millionth of the width rather than half a pixel:
        // before the window has a size the area is one pixel wide, and half
        // of it would take the second chart of an even split for the first.
        let rects = self.layout_rects();
        let right = self.chart_host.width().max(1) as f64;
        let at = |a: f64, b: f64| (a - b).abs() <= right * 1e-6;
        // A maximized chart is under it wherever it sits in the tree.
        let topmost = self.filled().or_else(|| {
            self.layout.borrow().leaves().into_iter().find(|id| {
                rects.get(id).is_some_and(|(x, y, w, _)| at(*y, 0.0) && at(x + w, right))
            })
        });
        // And how far down it stands, so the chart can keep its prices out
        // from under it. Measured from the top of the charts' area rather
        // than the chart's own: this runs as the charts are remounted, before
        // a pane that has just moved, or been maximized, is laid out again,
        // and its stale place put the corner hundreds of pixels above it.
        // The stylesheet height stands in until the corner is laid out.
        let cover = match railed {
            true => 0.0,
            false => self.corner.borrow().as_ref().map_or(0.0, |corner| {
                let low = corner.height() as f32;
                match corner.compute_point(&self.chart_host, &gtk::graphene::Point::new(0.0, low)) {
                    Some(point) if low > 0.0 => point.y().max(0.0) as f64,
                    _ => CORNER_HEIGHT as f64,
                }
            }),
        };
        for pane in self.panes.borrow().iter() {
            let under = Some(pane.id) == topmost;
            pane.set_corner_clearance(if under { clearance } else { 0 });
            pane.view.set_covered_axis_top(if under { cover } else { 0.0 });
        }

        // The drawing tools' toggle, in the window's bottom-left corner at
        // the time axis's height, centred under the column of tools, and at
        // that one place whether the tools are out or not: only its lit
        // state says which. Out, it stands at the foot of the bar. Put away,
        // it stands over the bottom-left chart, and that chart leaves the
        // start of its time axis to it. The headers keep their own inset.
        let ring = omacharts_engine::frame::RING_WIDTH as i32;
        let bar = self.drawing_bar.borrow().clone();
        let mut taken = 0.0;
        if let Some(bar) = &bar {
            let tooled = bar.is_shown();
            let toggle = tools_toggle_width(&bar.toggle);
            let start = crate::ui::drawing_bar::COLUMN_CENTRE - toggle / 2;
            let height = crate::ui::chart::TIME_AXIS_H as i32;
            bar.toggle.set_margin_start(start.max(0));
            bar.toggle.set_margin_bottom(ring + (height - TOOLS_TOGGLE_SIZE) / 2);
            if !tooled {
                // Its right edge in the chart's own pixels, and a few more
                // of air before a date may start.
                taken = (start - ring + toggle + 4) as f64;
            }
        }
        let bottom = self.chart_host.height().max(1) as f64;
        let lowest_left = self.filled().or_else(|| {
            self.layout.borrow().leaves().into_iter().find(|id| {
                rects.get(id).is_some_and(|(x, y, _, h)| at(*x, 0.0) && at(y + h, bottom))
            })
        });
        for pane in self.panes.borrow().iter() {
            pane.view.set_axis_start_taken(if Some(pane.id) == lowest_left { taken } else { 0.0 });
        }

        // And the trading panel's toggle, in the window's bottom-right
        // corner under the prices, there only with a login and one
        // chartbook, and at that one place whether the rail is out or not.
        // Put away, it is over the bottom-right chart, which leaves the
        // corner empty for it; out, the rail leaves its own foot to it, the
        // time axis's height.
        let toggled = self.trading_toggle_over_charts();
        if let Some(rail) = self.watchlist.borrow().as_ref() {
            rail.leave_foot(if toggled { ring + crate::ui::chart::TIME_AXIS_H as i32 } else { 0 });
        }
        let toggled = toggled && !railed;
        let lowest = self.filled().or_else(|| {
            self.layout.borrow().leaves().into_iter().find(|id| {
                rects.get(id).is_some_and(|(x, y, w, h)| at(x + w, right) && at(y + h, bottom))
            })
        });
        for pane in self.panes.borrow().iter() {
            pane.view.set_axis_corner_taken(toggled && Some(pane.id) == lowest);
        }
    }

    fn build_node(self: &Rc<Self>, node: &Node, path: &[bool]) -> gtk::Widget {
        match node {
            Node::Leaf(id) => match self.pane(*id) {
                Some(pane) => pane.root.clone().upcast(),
                None => gtk::Box::new(gtk::Orientation::Vertical, 0).upcast(),
            },
            Node::Split { horizontal, ratio, first, second } => {
                let paned = gtk::Paned::new(if *horizontal {
                    gtk::Orientation::Horizontal
                } else {
                    gtk::Orientation::Vertical
                });
                paned.add_css_class("chart-split");
                let mut down = path.to_vec();
                down.push(true);
                paned.set_start_child(Some(&self.build_node(first, &down)));
                let mut down = path.to_vec();
                down.push(false);
                paned.set_end_child(Some(&self.build_node(second, &down)));
                // Both halves grow with the window, so a split stays the split
                // you made rather than drifting as the window is resized.
                paned.set_resize_start_child(true);
                paned.set_resize_end_child(true);
                paned.set_shrink_start_child(false);
                paned.set_shrink_end_child(false);
                paned.set_wide_handle(true);

                // Put the divider back where it was left, once the split has a
                // size to measure it against — a position set before the first
                // allocation is a position against zero.
                let ratio = ratio.clamp(0.05, 0.95);
                let placed = Rc::new(Cell::new(false));
                let paned_for_place = paned.clone();
                let placed_for_map = placed.clone();
                let place = move || {
                    if placed_for_map.get() {
                        return;
                    }
                    let length = if paned_for_place.orientation() == gtk::Orientation::Horizontal {
                        paned_for_place.width()
                    } else {
                        paned_for_place.height()
                    };
                    if length > 0 {
                        placed_for_map.set(true);
                        paned_for_place.set_position((length as f64 * ratio).round() as i32);
                    }
                };
                let place_on_map = place.clone();
                paned.connect_map(move |_| place_on_map());

                let this = self.clone();
                let here = path.to_vec();
                let placed_for_move = placed.clone();
                paned.connect_position_notify(move |paned| {
                    // Only once it has been put back, or the first notify —
                    // which fires while the split still has no size — would
                    // record a ratio of nothing.
                    if !placed_for_move.get() {
                        place();
                        return;
                    }
                    let length = if paned.orientation() == gtk::Orientation::Horizontal {
                        paned.width()
                    } else {
                        paned.height()
                    };
                    if length > 0 {
                        this.record_ratio(&here, paned.position() as f64 / length as f64);
                    }
                });

                paned.upcast()
            }
        }
    }

    /// Change what the rail does to a chart — and where it becomes Follow,
    /// make what it is showing the Follow charts' symbol.
    ///
    /// The chart leads rather than follows. You made *this* chart follow, so
    /// this is the symbol you meant the Follow charts to be on.
    ///
    /// Three changes deliberately write nothing, each of which would otherwise
    /// move charts somebody did not ask about:
    ///
    /// - **Leaving Follow.** The charts still following keep what they had.
    /// - **Re-picking the mode it is already in.** The chain's popover and the
    ///   chart menu's radio items both re-apply their own state, so a mode
    ///   that rewrote four charts every time its menu opened would be unusable.
    /// - **Following from a chart with nothing on it.** Follow charts that are
    ///   showing something keep it; leading with nothing would blank them.
    pub fn set_pane_mode(self: &Rc<Self>, id: u32, mode: LinkMode) {
        let Some(pane) = self.pane(id) else { return };
        let before = pane.mode();
        pane.set_mode(mode, mode.colour(&self.theming.borrow().theme()));
        self.sync_chart_actions(&pane);
        let showing = pane.instrument.borrow().clone();
        if mode == LinkMode::Follow && before != mode && let Some(instrument) = showing {
            self.spread_to_followers(&instrument, mode, Some(id));
        }
        self.save_workspace();
    }

    /// Move a chart on to its next link mode, or back to the one before.
    ///
    /// Arriving at Follow, it joins the Follow charts rather than leading
    /// them: it was pinned or focused to hold something they were not showing,
    /// so leading would put that on every one of them, which is not what a key
    /// pressed on one chart asks for.
    fn cycle_mode(self: &Rc<Self>, pane: &Rc<ChartPane>, back: bool) {
        let mode = pane.mode().cycled(back);
        let joining = (mode == LinkMode::Follow)
            .then(|| {
                self.panes
                    .borrow()
                    .iter()
                    .filter(|p| p.id != pane.id && p.mode() == LinkMode::Follow)
                    .find_map(|p| p.instrument.borrow().clone())
            })
            .flatten();
        let Some(instrument) = joining else {
            self.set_pane_mode(pane.id, mode);
            return;
        };
        pane.set_mode(mode, mode.colour(&self.theming.borrow().theme()));
        self.sync_chart_actions(pane);
        self.show_in(pane, instrument);
        self.save_workspace();
    }

    /// The three header menus' choices on `pane`: an account by its key,
    /// a feed by its id, a quantity by its place in the template. Each
    /// is what the row it marks would do when picked.
    fn add_menu_actions(self: &Rc<Self>, pane: &ChartPane) {
        let id = pane.id;
        let account = gio::SimpleAction::new_stateful("account", Some(glib::VariantTy::STRING), &"".to_variant());
        let this = Rc::downgrade(self);
        account.connect_activate(move |action, value| {
            let (Some(this), Some(key)) = (this.upgrade(), value.and_then(|value| value.str()).map(str::to_string)) else {
                return;
            };
            action.set_state(&key.to_variant());
            this.focus(id);
            this.pick_account(id, &key);
        });
        pane.menu_actions.add_action(&account);
        let feed = gio::SimpleAction::new_stateful("feed", Some(glib::VariantTy::STRING), &"".to_variant());
        let this = Rc::downgrade(self);
        feed.connect_activate(move |action, value| {
            let (Some(this), Some(own)) = (this.upgrade(), value.and_then(|value| value.str()).map(str::to_string)) else {
                return;
            };
            action.set_state(&own.to_variant());
            this.focus(id);
            this.pick_feed(id, &own);
        });
        pane.menu_actions.add_action(&feed);
        let quantity = gio::SimpleAction::new_stateful("quantity", Some(glib::VariantTy::INT32), &0i32.to_variant());
        let this = Rc::downgrade(self);
        quantity.connect_activate(move |action, value| {
            let (Some(this), Some(n)) = (this.upgrade(), value.and_then(|value| value.get::<i32>())) else { return };
            action.set_state(&n.to_variant());
            this.focus(id);
            if let Ok(n) = usize::try_from(n) {
                this.pick_size(id, n);
            }
        });
        pane.menu_actions.add_action(&quantity);
    }

    /// Mark `value` as the chart's choice in its menu named `name`.
    fn mark_choice(pane: &ChartPane, name: &str, value: glib::Variant) {
        if let Some(action) = pane.menu_actions.lookup_action(name).and_downcast::<gio::SimpleAction>() {
            action.set_state(&value);
        }
    }

    /// The Account part's menu, for the chart `id` as it is now: each
    /// trading service, its login, a heading with its trouble after its
    /// name, only when it has some, and its accounts by number under it,
    /// a mark on the chart's own. Picking an account picks its service.
    fn account_menu(self: &Rc<Self>, id: u32) -> gio::Menu {
        let menu = gio::Menu::new();
        let Some(pane) = self.pane(id) else { return menu };
        let view = self.trading.borrow().clone();
        let account = view.account_of(pane.account.borrow().as_deref()).cloned();
        Self::mark_choice(&pane, "account", account.map(|account| account.key()).unwrap_or_default().to_variant());
        for login in &view.logins {
            let theirs: Vec<&crate::trading::Account> =
                view.accounts.iter().filter(|listed| listed.login == login.id).collect();
            let Some(first) = theirs.first() else { continue };
            let heading = match view.trouble(&self.desk, first) {
                Some(trouble) => format!("{} · {trouble}", login.name),
                None => login.name.clone(),
            };
            let section = gio::Menu::new();
            for listed in theirs {
                let item = gio::MenuItem::new(Some(&listed.id), None);
                item.set_action_and_target_value(Some("pane.account"), Some(&listed.key().to_variant()));
                section.append_item(&item);
            }
            menu.append_section(Some(&heading), &section);
        }
        menu
    }

    /// The chart `id` on the account with the key `key`. Choosing the
    /// default stores nothing, so the chart keeps following it; the chart's
    /// feed is left as it was.
    fn pick_account(self: &Rc<Self>, id: u32, key: &str) {
        let Some(pane) = self.pane(id) else { return };
        let (current, default_account) = {
            let view = self.trading.borrow();
            (
                view.account_of(pane.account.borrow().as_deref()).map(crate::trading::Account::key),
                view.default_account().map(crate::trading::Account::key),
            )
        };
        if current.as_deref() == Some(key) {
            return;
        }
        let stored = (default_account.as_deref() != Some(key)).then(|| key.to_string());
        self.choose_account(id, stored);
    }

    /// The feeds that could serve `pane`'s chart of `instrument`, the
    /// default first and the rest in the order; the default's id; and the
    /// id of the one in force, chosen or fallen back to.
    fn feeds_serving(&self, pane: &ChartPane, instrument: &Instrument) -> (Vec<Rc<Feed>>, Option<&'static str>, Option<&'static str>) {
        let feeds = self.feeds();
        let view = self.trading.borrow();
        let account = view.account_of(pane.account.borrow().as_deref());
        let mut serving: Vec<Rc<Feed>> =
            providers::serving(feeds.iter().map(|feed| feed.provider.as_ref()), instrument)
                .into_iter()
                .filter_map(|at| feeds.get(at).cloned())
                .collect();
        let default = crate::trading::default_feed(
            &view,
            account,
            feeds.iter().map(|feed| feed.provider.as_ref() as &dyn Provider),
            instrument,
        );
        drop(view);
        // The default first, as the one nobody has to choose; the rest in
        // the order.
        serving.sort_by_key(|feed| Some(feed.provider.id()) != default);
        let in_force = match self.route_chosen(pane, instrument) {
            Some(Ok(routed) | Err(routed)) => Some(routed.feed.provider.id()),
            None => self.missing_from(pane, instrument).map(|feed| feed.provider.id()),
        };
        (serving, default, in_force)
    }

    /// The Data feed part's menu, for the chart `id` as it is now: the
    /// feeds that serve the chart's instrument, by name, the default first
    /// and a mark on the one in force, under how the feed stands, in the
    /// dot's own words, only when the dot is red.
    fn feed_menu(self: &Rc<Self>, id: u32) -> gio::Menu {
        let menu = gio::Menu::new();
        let Some(pane) = self.pane(id) else { return menu };
        let section = gio::Menu::new();
        let instrument = pane.instrument.borrow().clone();
        let mut credited = Vec::new();
        if let Some(instrument) = instrument {
            let (serving, _, in_force) = self.feeds_serving(&pane, &instrument);
            credited = serving.iter().map(|feed| feed.provider.id()).collect();
            Self::mark_choice(&pane, "feed", in_force.unwrap_or_default().to_variant());
            for feed in &serving {
                let own = feed.provider.id();
                let item = gio::MenuItem::new(Some(feed.provider.label()), None);
                item.set_action_and_target_value(Some("pane.feed"), Some(&own.to_variant()));
                section.append_item(&item);
            }
        }
        let status = pane.feed_in_trouble().then(|| pane.feed_status_words()).flatten();
        menu.append_section(status.as_deref(), &section);
        // The artwork a feed on offer has to show, under the feeds; the dot
        // fills the room left for it as the menu opens.
        crate::ui::sign_in_dialogs::leave_room_for_credits(&menu, &credited);
        menu
    }

    /// The chart `id` on the feed `own`. The default is no choice at all:
    /// the chart goes on following it.
    fn pick_feed(self: &Rc<Self>, id: u32, own: &str) {
        let Some(pane) = self.pane(id) else { return };
        let instrument = pane.instrument.borrow().clone();
        let Some(instrument) = instrument else { return };
        let (_, default, _) = self.feeds_serving(&pane, &instrument);
        let chosen = (Some(own) != default).then(|| own.to_string());
        self.choose_feed(id, chosen);
    }

    /// Move the divider the focused chart sits against, by a nudge.
    ///
    /// Through the handle on screen rather than through the tree, so that one
    /// press is exactly a very short drag: the position notify already writes
    /// the ratio down, already clamps it to what the charts will allow, and
    /// already waits for the pressing to stop before it saves — which is what a
    /// held key needs, for the same reason dragging did.
    ///
    /// `false` when there is nothing to move, so the key can go on to whatever
    /// else wants it.
    fn resize_focused(self: &Rc<Self>, how: pane::Resize) -> bool {
        // One chart filling the window has no divider on screen: the rest of
        // the tree is not mounted, and moving a divider nobody can see would
        // only show itself on restore.
        if self.maximized.get().is_some() {
            return false;
        }
        // Read the tree and let go of it before touching the handle. Setting a
        // position emits the notify that records the ratio, and that wants the
        // layout mutably while this borrow would still be alive.
        let divider = self.layout.borrow().resize(self.focused.get(), how);
        let Some((path, ratio)) = divider else { return false };
        let Some(paned) = self.paned_at(&path) else { return false };
        let length = if paned.orientation() == gtk::Orientation::Horizontal {
            paned.width()
        } else {
            paned.height()
        };
        if length <= 0 {
            return false;
        }
        paned.set_position((length as f64 * ratio).round() as i32);
        true
    }

    /// The handle at `path`, counting down from the one the charts are mounted
    /// in — the same path `build_node` handed out on the way down.
    fn paned_at(&self, path: &[bool]) -> Option<gtk::Paned> {
        let mut widget = self.chart_host.first_child()?;
        for half in path {
            let paned: gtk::Paned = widget.downcast().ok()?;
            widget = if *half { paned.start_child()? } else { paned.end_child()? };
        }
        widget.downcast().ok()
    }

    /// Remember where a divider was dragged to.
    fn record_ratio(self: &Rc<Self>, path: &[bool], ratio: f64) {
        let next = self.layout.borrow().with_ratio(path, ratio);
        *self.layout.borrow_mut() = next;
        self.save_soon();
    }

    /// Save once the dragging stops.
    ///
    /// A handle being dragged emits a position for every pixel it crosses, and
    /// a database write per pixel is a database write per pixel. The delay is
    /// longer than the gap between two of those and shorter than anyone's
    /// patience.
    fn save_soon(self: &Rc<Self>) {
        if self.save_pending.replace(true) {
            return;
        }
        let this = self.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(400), move || {
            this.save_pending.set(false);
            this.save_workspace();
        });
    }

    /// The charts on screen, written down as a chartbook.
    ///
    /// The name is the one thing nothing on screen carries, so it is handed
    /// in by whoever already knows it. Everything else, the rail included, is
    /// read off the window as it stands.
    fn capture_book(&self, name: String) -> Chartbook {
        let panes: Vec<StoredPane> = self
            .panes
            .borrow()
            .iter()
            .map(|pane| {
                let instrument = pane.instrument.borrow();
                let pending = pane.pending.borrow();
                let (symbol, suffix) = stored_symbol(instrument.as_ref(), pending.as_ref());
                StoredPane {
                    id: pane.id,
                    title: Some(pane.title.borrow().clone()).filter(|title| !title.is_empty()),
                    symbol,
                    suffix,
                    timeframe: pane.timeframe.get().key(),
                    indicators: pane.indicators.borrow().clone(),
                    template: pane.template.get(),
                    bar_style: pane.bar_style.get().key().to_string(),
                    bars_hidden: pane.bars_hidden.get(),
                    colouring: Some(pane.colouring.get().key().to_string()),
                    swapped: Some(pane.swapped.get()),
                    hollow_up: Some(pane.hollow_up.get()),
                    ohlc_thickness: Some(pane.ohlc_thickness.get()),
                    session: pane.session.get().key().to_string(),
                    continuous: Some(pane.continuous.get().key().to_string()),
                    show_grid: pane.show_grid.get(),
                    link: pane.mode(),
                    shows_drawings: pane.shows_drawings.get(),
                    sends_drawings: pane.sends_drawings.get(),
                    shows_legend: pane.shows_legend.get(),
                    drawing_uid: pane.uid.borrow().clone(),
                    drawings: pane.local_drawings.borrow().clone(),
                    profile_merges: pane.profile_merges.borrow().clone(),
                    source: pane.feed.borrow().clone(),
                    account: pane.account.borrow().clone(),
                    trading_size: pane.trading_size.borrow().clone(),
                    // One still to be found is written back as it was kept.
                    trading_symbol: pane
                        .trading_symbol
                        .borrow()
                        .as_ref()
                        .map(|traded| traded.symbol.clone())
                        .or_else(|| pane.trading_pending.borrow().as_ref().map(|(symbol, _)| symbol.clone())),
                    trading_suffix: match pane.trading_symbol.borrow().as_ref() {
                        Some(traded) => traded.suffix.clone(),
                        None => pane.trading_pending.borrow().as_ref().and_then(|(_, suffix)| suffix.clone()),
                    },
                    auto_scale: pane.view.price_auto(),
                    price_window: pane.view.held_price_window().map(|(low, high)| [low, high]),
                    // Only the strips of indicators the chart still has: one
                    // removed, or gone with a template, takes its zoom along.
                    strip_windows: {
                        let kinds = pane.view.held_strip_kinds();
                        let indicators = pane.indicators.borrow();
                        pane.view
                            .held_strip_windows()
                            .into_iter()
                            .filter_map(|(id, (low, high))| {
                                let kind = indicators.iter().find(|indicator| indicator.id == id)?.kind;
                                // Held for another kind than the chart has under its id now.
                                if kinds.get(&id).is_some_and(|held| *held != kind) {
                                    return None;
                                }
                                Some((id, StoredStrip { kind: Some(kind), window: [low, high] }))
                            })
                            .collect()
                    },
                    log_scale: pane.view.price_log(),
                    indicators_shown: pane.view.show_indicators(),
                    drawings_shown: pane.view.show_drawings(),
                }
            })
            .collect();
        Chartbook {
            name,
            layout: self.layout.borrow().clone(),
            focused: self.focused.get(),
            panes,
            watchlist: self.watchlist.borrow().as_ref().map(|rail| rail.active_watchlist()),
            sidebar_shown: Some(self.shows_sidebar()),
            sidebar_width: Some(self.sidebar_width.get()),
            maximized: self.filled(),
            book_template: None,
        }
    }

    /// Write every chartbook down, so a window comes back as it was left.
    ///
    /// The book on screen is the only one that can have changed, so it is
    /// folded back into the list first. Everything else is already data.
    ///
    /// Public because a command typed in a terminal reads and writes the same
    /// stored arrangement, and what is on screen is only in memory until this
    /// runs — see [`crate::cli::Live`].
    pub fn save_workspace(self: &Rc<Self>) {
        {
            let mut books = self.books.borrow_mut();
            let active = self.active.get().min(books.len().saturating_sub(1));
            let name = books.get(active).map(|book| book.name.clone()).unwrap_or_default();
            let mut live = self.capture_book(name);
            // Nothing on screen carries it either.
            live.book_template = books.get(active).and_then(|book| book.book_template);
            match books.get_mut(active) {
                Some(book) => *book = live,
                None => books.push(live),
            }
            self.active.set(active);
        }
        self.carry_book_template();
        let workspace =
            Workspace { books: self.books.borrow().clone(), active: self.active.get() };
        if let Ok(json) = serde_json::to_string(&workspace) {
            self.store.set_setting(SETTING_WORKSPACE, &json);
        }
    }

    /// Put the chartbooks back. `false` when there was nothing written down,
    /// or when what was written cannot be rebuilt.
    fn restore_workspace(self: &Rc<Self>) -> bool {
        let Some(mut json) = self.store.setting(SETTING_WORKSPACE) else { return false };
        if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&json)
            && upgrade_link_groups(&mut value, &self.store)
        {
            json = value.to_string();
        }
        let Some(workspace) = parse_workspace(&json) else { return false };
        let active = workspace.active.min(workspace.books.len() - 1);
        *self.books.borrow_mut() = workspace.books;
        self.active.set(active);
        if !self.materialise_book(active) {
            // Nothing partial: a window that could not rebuild what it was
            // given starts over with one chart rather than some of one.
            self.books.borrow_mut().clear();
            return false;
        }
        true
    }

    /// Read the arrangement back and rebuild from it, after something outside
    /// the window changed it — a command typed in a terminal.
    ///
    /// Everything the window holds comes back from what was written down,
    /// which chart had the window to itself included: the command ran after
    /// `save_workspace` put that into the book, and `materialise_book` reads
    /// it back out by the chart's new id. Nothing is carried across by hand.
    pub fn reload_workspace(self: &Rc<Self>) {
        let books = self.books.borrow().clone();
        let active = self.active.get();
        // The one thing on screen that is not written down and still has to
        // survive a rebuild: a range a chart was told to keep showing, by
        // `chart range` among others. Without it, the next command that
        // rebuilds the arrangement — focusing another chart is one — puts
        // every chart back at rest. Matched by the chart's own name, which a
        // rebuild keeps and a copy is given afresh, rather than by its id,
        // which a rebuild hands out again.
        let held: HashMap<String, omacharts_engine::Range> = self
            .panes
            .borrow()
            .iter()
            .filter_map(|pane| Some((pane.uid.borrow().clone(), pane.view.held_range()?)))
            .collect();

        if !self.restore_workspace() {
            // An arrangement that cannot be rebuilt leaves the charts alone:
            // `materialise_book` refuses before it touches a widget, so what is
            // on screen is still what the window had. Only the list of books it
            // emptied on the way out has to be put back.
            *self.books.borrow_mut() = books;
            self.active.set(active);
            return;
        }

        if !held.is_empty() {
            for pane in self.panes.borrow().iter() {
                if let Some(range) = held.get(pane.uid.borrow().as_str()) {
                    pane.view.show_range(*range);
                }
            }
        }

        // A command can add or remove a whole chartbook, which is the one
        // change that shows outside the charts themselves.
        self.rebuild_book_strip();
        self.chart_changed_elsewhere();
    }

    /// Have `watch` run whenever the focused chart may have been changed by
    /// something other than the dialog showing it: a command, a theme.
    /// It answers whether it is still wanted.
    pub fn watch_chart(&self, watch: impl Fn() -> bool + 'static) {
        self.chart_watchers.borrow_mut().push(Box::new(watch));
    }

    /// Tell whatever shows the focused chart's settings to read them again.
    /// Taken out while they run, because one may rebuild rows that register
    /// a watcher of their own.
    pub fn chart_changed_elsewhere(&self) {
        let mut watchers = std::mem::take(&mut *self.chart_watchers.borrow_mut());
        watchers.retain(|watch| watch());
        let mut kept = self.chart_watchers.borrow_mut();
        watchers.append(&mut kept);
        *kept = watchers;
    }

    /// Draw the rail again, after something outside the window changed a
    /// watchlist.
    pub fn reload_watchlists(self: &Rc<Self>) {
        let rail = self.watchlist.borrow().as_ref().cloned();
        if let Some(rail) = rail {
            rail.reload();
        }
        // A watchlist that just changed is the likeliest moment for there to
        // be a row with no price — somebody has just added one. Waiting for
        // the timer would mean watching a dash for up to a minute after
        // typing the symbol.
        self.backfill();
    }

    /// Whether the focused chart's feed has volume at price for it: bid and
    /// ask volume a cumulative delta can be read from rather than estimated.
    pub fn focused_pane_has_ladders(&self) -> bool {
        self.focused_pane().ladders.borrow().is_some()
    }

    /// The colouring chosen in Preferences, and whether it is worn the other
    /// way round: what a delta profile or a cumulative delta with no
    /// colouring of its own is painted in, on every chart.
    pub fn house_look(&self) -> (BarColouring, bool) {
        let theming = self.theming.borrow();
        (theming.colouring(), theming.swapped())
    }

    /// Adopt a theme or bar scheme chosen outside this window, and repaint.
    ///
    /// `restyle` paints from the `Theming` this process is holding, so without
    /// re-reading the choice first it would faithfully repaint the theme the
    /// window already had.
    pub fn adopt_theming(self: &Rc<Self>) {
        self.theming.borrow_mut().reload_selection(&self.store);
        self.restyle();
    }

    /// Show the settings a backup just put back: everything the window holds
    /// in memory that came out of them, read again.
    ///
    /// The same things a restore typed in a terminal asks for through
    /// [`crate::cli::Live`], in the same order. The arrangement goes last and
    /// without being saved first — what is on screen is exactly what was
    /// replaced, and saving it would put it back.
    pub fn adopt_restored(self: &Rc<Self>) {
        self.adopt_theming();
        self.adopt_feed();
        self.adopt_zoom();
        self.adopt_pnl_display();
        self.reload_watchlists();
        self.reload_workspace();
    }

    /// Build a chartbook into widgets, replacing whatever is on screen.
    ///
    /// `false` when it cannot be rebuilt — a layout naming panes that are not
    /// there is worse than starting over with one chart.
    fn materialise_book(self: &Rc<Self>, index: usize) -> bool {
        let Some(book) = self.books.borrow().get(index).cloned() else { return false };
        if book.panes.is_empty() {
            return false;
        }
        // A build before this one wrote arrangements whose every quadrant named
        // the same pane, so a stored tree is not trusted to be one. Repeated
        // leaves come out before anything is built from them, which turns an
        // unmountable grid back into the chart it actually described.
        let Some(book_layout) = book.layout.deduped() else { return false };
        // Taken before the panes are consumed below, and applied after the
        // charts are up: showing the rail moves a chart's own corner out from
        // under the window's controls, which needs the charts to exist.
        let rail = (book.watchlist, book.sidebar_shown, book.sidebar_width);
        let wanted = book_layout.leaves();
        if wanted.is_empty() || wanted.iter().any(|id| !book.panes.iter().any(|p| p.id == *id)) {
            return false;
        }

        self.release_all_panes();
        self.panes.borrow_mut().clear();
        let mut restored: Vec<(Rc<ChartPane>, StoredPane)> = Vec::new();
        let defaults = ChartDefaults::load(&self.store);
        let templates = crate::templates::all(&self.store);
        for stored in book.panes {
            if !wanted.contains(&stored.id) {
                continue;
            }
            // Read off the template rather than the copy, which a template
            // changed while this book was not on screen has left behind. A
            // template that has gone leaves the chart the copy, as its own.
            let template = stored.template.and_then(|id| templates.iter().find(|t| t.id == id));
            let pane = self.new_pane(
                Timeframe::parse(&stored.timeframe).unwrap_or(Timeframe::days(1)),
                template.map(|t| t.indicators.clone()).unwrap_or_else(|| stored.indicators.clone()),
                BarStyle::from_key(&stored.bar_style).unwrap_or_default(),
                stored_colouring(&stored, &defaults),
                Session::from_key(&stored.session).unwrap_or_default(),
                stored.show_grid,
                stored.link,
            );
            pane.template.set(template.map(|t| t.id));
            self.show_header_on(&pane);
            pane.continuous.set(
                stored
                    .continuous
                    .as_deref()
                    .and_then(omacharts_engine::continuous::Adjustment::from_key)
                    .unwrap_or(defaults.continuous),
            );
            pane.shows_drawings.set(stored.shows_drawings);
            pane.sends_drawings.set(stored.sends_drawings);
            pane.view.set_sends(stored.sends_drawings);
            pane.shows_legend.set(stored.shows_legend);
            // Keep the name it was written down under, so the drawings that
            // name it go on naming it. A chart that has none keeps the one
            // it was just minted.
            if !stored.drawing_uid.is_empty() {
                *pane.uid.borrow_mut() = stored.drawing_uid.clone();
            }
            *pane.local_drawings.borrow_mut() = stored.drawings.clone();
            *pane.profile_merges.borrow_mut() = stored.profile_merges.clone();
            *pane.feed.borrow_mut() = stored.source.clone();
            *pane.account.borrow_mut() = stored.account.clone();
            *pane.trading_size.borrow_mut() = stored.trading_size.clone();
            // The window opens on the curated half, which may not name it:
            // then it waits for the rest, as the chart's own symbol does,
            // and nothing is traded meanwhile.
            if let Some(symbol) = stored.trading_symbol.as_deref() {
                match self.index.find(symbol, stored.trading_suffix.as_deref()) {
                    Some(traded) => *pane.trading_symbol.borrow_mut() = Some(traded),
                    None => *pane.trading_pending.borrow_mut() = Some((symbol.to_string(), stored.trading_suffix.clone())),
                }
            }
            pane.view.set_price_auto(stored.auto_scale);
            if let (false, Some([low, high])) = (stored.auto_scale, stored.price_window) {
                pane.view.hold_price_window(low, high);
            }
            for (id, strip) in &stored.strip_windows {
                // Only for the kind of indicator it was held for, should a
                // template have put another under its id.
                let now = pane.indicators.borrow().iter().find(|indicator| indicator.id == *id).map(|indicator| indicator.kind);
                if strip.kind.is_some_and(|kind| now != Some(kind)) {
                    continue;
                }
                let [low, high] = strip.window;
                pane.view.hold_strip_window(*id, strip.kind, low, high);
            }
            pane.view.set_price_log(stored.log_scale);
            pane.bars_hidden.set(stored.bars_hidden);
            pane.view.set_bars_hidden(stored.bars_hidden);
            pane.set_ohlc_thickness(stored.ohlc_thickness.unwrap_or(defaults.ohlc_thickness));
            pane.write_title(stored.title.as_deref().unwrap_or(""));
            pane.view.set_show_indicators(stored.indicators_shown);
            pane.view.set_show_drawings(stored.drawings_shown);
            restored.push((pane, stored));
        }

        // Ids are handed out afresh in layout order, so the tree is rewritten
        // to match rather than trusting ids from another run to still be free.
        // The counter is never wound back, because the books that are not on
        // screen still name panes by the ids they were built with — which is
        // exactly why the whole tree is relabelled at once. The new ids overlap
        // the stored ones, so renaming them one at a time would rename leaves
        // an earlier step had just written.
        let ids: HashMap<u32, u32> =
            restored.iter().map(|(pane, stored)| (stored.id, pane.id)).collect();
        let mut focused = restored.first().map(|(pane, _)| pane.id).unwrap_or(1);
        for (pane, stored) in &restored {
            if stored.id == book.focused {
                focused = pane.id;
            }
        }
        *self.layout.borrow_mut() = book_layout.relabel(&ids);
        self.focused.set(focused);
        // Which chart fills the window is the book's to remember, so it comes
        // back with the book — by its new id, the way the focus does. Each
        // book keeps its own answer, which is what makes switching away and
        // back find the window as it was left. A chart that is no longer in
        // the tree cannot fill it, and `filled` says so when it is asked.
        self.maximized.set(book.maximized.and_then(|old| ids.get(&old).copied()));
        self.rebuild_layout();

        for (pane, stored) in &restored {
            if let Some(instrument) = self.index.find(&stored.symbol, stored.suffix.as_deref()) {
                self.show_in(pane, instrument.clone());
            } else if !stored.symbol.is_empty() {
                // The window opens on the curated half of the inventory, so a
                // chart showing anything from the long tail cannot be resolved
                // yet. The chart keeps what it was asked for: a save before the
                // catalogue lands writes that symbol rather than an empty one,
                // and the chart fills itself in when the rest arrives.
                *pane.pending.borrow_mut() = Some((stored.symbol.clone(), stored.suffix.clone()));
            }
        }
        // Only a book on a template has anything to compare against, so one on
        // none pays nothing for it.
        *self.template_leaves.borrow_mut() = match book.book_template {
            Some(_) => self.layout.borrow().leaves(),
            None => Vec::new(),
        };
        self.sync_header();
        self.rebuild_indicator_legend();
        self.apply_sidebar(rail.0, rail.1, rail.2);
        true
    }

    /// Chart whatever was asked for before the catalogue could name it.
    ///
    /// The window is built on the curated half and the long tail lands a
    /// moment later, so this is the second and last look: a chart restored
    /// from a saved arrangement gets its symbol here or not at all.
    ///
    /// Only charts still holding what they were asked for and still showing
    /// nothing. That is the whole of the cancellation — [`Window::show_in`]
    /// clears the pending symbol, so a chart somebody pointed somewhere else
    /// while the listings were loading is skipped, and the books that are not
    /// on screen have no charts at all for this to reach.
    fn resolve_pending(self: &Rc<Self>) {
        // Collected first: charting one reaches back into the window, and a
        // borrow on the charts held across that is a panic.
        let waiting: Vec<(Rc<ChartPane>, String, Option<String>)> = self
            .panes
            .borrow()
            .iter()
            .filter(|pane| pane.instrument.borrow().is_none())
            .filter_map(|pane| {
                let (symbol, suffix) = pane.pending.borrow().clone()?;
                Some((pane.clone(), symbol, suffix))
            })
            .collect();
        let mut charted = false;
        // Trading symbols first, so that a chart charted below is judged
        // against the one it trades.
        let trading: Vec<Rc<ChartPane>> =
            self.panes.borrow().iter().filter(|pane| pane.trading_pending.borrow().is_some()).cloned().collect();
        for pane in trading {
            let Some((symbol, suffix)) = pane.trading_pending.borrow().clone() else { continue };
            let Some(traded) = self.index.find(&symbol, suffix.as_deref()) else { continue };
            pane.trading_pending.replace(None);
            let charted_now = pane.instrument.borrow().clone();
            match charted_now {
                Some(instrument) if !instrument.tracks(&traded) => {}
                _ => *pane.trading_symbol.borrow_mut() = Some(traded),
            }
            pane.write_readout();
            self.apply_marks_to(&pane);
            charted = true;
        }
        for (pane, symbol, suffix) in waiting {
            if let Some(instrument) = self.index.find(&symbol, suffix.as_deref()) {
                self.show_in(&pane, instrument);
                charted = true;
            }
        }
        if charted {
            self.sync_header();
            self.rebuild_indicator_legend();
        }
    }

    /// Put the rail back the way a chartbook left it.
    ///
    /// A book that says nothing about the rail was written before the rail
    /// belonged to one, and takes the window's settings — the same ones a
    /// window with nothing written down at all opens with.
    fn apply_sidebar(
        self: &Rc<Self>,
        watchlist: Option<i64>,
        shown: Option<bool>,
        width: Option<i32>,
    ) {
        let rail = self.watchlist.borrow().as_ref().cloned();
        if let Some(rail) = rail {
            // A book that names no list keeps the one in front of you, which is
            // what the window's settings left there — but it is still asked for,
            // so that two books cannot end up on one list by both saying
            // nothing about it.
            let wanted = watchlist.unwrap_or_else(|| rail.active_watchlist());
            rail.set_active_watchlist(self.rail_for_book(wanted));
        }
        self.set_rail_shown(shown.unwrap_or_else(|| self.store.setting_bool(SHOW_WATCHLIST, true)));
        self.want_sidebar_width(width.unwrap_or_else(|| self.stored_sidebar_width()));
    }

    // -- chartbooks --------------------------------------------------------

    /// What a book is called in the strip. Unnamed books are called by where
    /// they sit, so the row always reads in order.
    fn book_label(&self, index: usize) -> String {
        match self.books.borrow().get(index) {
            Some(book) if !book.name.is_empty() => book.name.clone(),
            _ => default_book_name(index + 1),
        }
    }

    /// The chartbook using this watchlist, named, if one other than the book on
    /// screen is.
    ///
    /// The book on screen is left out by its position rather than by what it
    /// says it is using: its stored field is only written when the workspace is
    /// saved, so between a switch and the next save it still names the list the
    /// rail has already moved off.
    fn book_using_watchlist(&self, watchlist: i64) -> Option<String> {
        let at = book_using(&self.books.borrow(), self.active.get(), watchlist)?;
        Some(self.book_label(at))
    }

    /// Which watchlist every chartbook is using, the one on screen included.
    ///
    /// For the one question `book_using_watchlist` cannot answer: what a
    /// chartbook that does not exist yet may be handed. The book on screen is
    /// read from the rail, for the same reason the other reads it from the
    /// books — what is written down against it is a save behind.
    fn watchlists_in_use(&self) -> Vec<i64> {
        let live = self.watchlist.borrow().as_ref().map(|rail| rail.active_watchlist());
        let active = self.active.get();
        self.books
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(at, book)| if at == active { live } else { book.watchlist })
            .collect()
    }

    /// Which list a chartbook ends up showing, given the one it asked for.
    ///
    /// Two ways of not getting it: the list has been deleted since the book was
    /// written down, or another chartbook is using it. Both land on the default,
    /// which is the list that is always there and is never anybody's.
    fn rail_for_book(&self, wanted: i64) -> i64 {
        let available =
            self.store.watchlist_exists(wanted) && self.book_using_watchlist(wanted).is_none();
        if available {
            wanted
        } else {
            DEFAULT_WATCHLIST
        }
    }

    /// Throw away the charts on screen and start again with one.
    ///
    /// For a brand new chartbook, and for the one case a stored book cannot
    /// be rebuilt: either way the window has to end up with a chart in it.
    fn mount_single_chart(self: &Rc<Self>, instrument: Option<Instrument>) {
        // At the resolution you were looking at, so a new book does not jump
        // back to daily; everything else is what a new chart opens with.
        let from = self.panes.borrow().iter().find(|p| p.id == self.focused.get()).cloned();
        let store = &self.store;
        let timeframe = from.as_ref().map(|p| p.timeframe.get()).unwrap_or_else(|| {
            store
                .setting(LAST_TIMEFRAME)
                .and_then(|key| Timeframe::parse(&key))
                .unwrap_or(Timeframe::days(1))
        });
        self.release_all_panes();
        self.panes.borrow_mut().clear();
        self.maximized.set(None);
        // One chart put up in place of a book that would not come up is not
        // a change anybody made to its chartbook template: carried, it would
        // take every other book on the template down to one chart too. So the
        // book lets go of it instead.
        if let Some(book) = self.books.borrow_mut().get_mut(self.active.get()) {
            book.book_template = None;
        }
        self.template_leaves.borrow_mut().clear();
        // A new chart carries nothing. Splitting copies the chart you split,
        // because two of what you were looking at is the point of splitting —
        // but a new chartbook is a fresh start, and inheriting whatever the
        // last chart happened to be carrying is how a VWAP you set up once
        // follows you around for the rest of the session.
        let pane = self.new_default_pane(timeframe, Vec::new());
        *self.layout.borrow_mut() = Node::leaf(pane.id);
        self.focused.set(pane.id);
        self.rebuild_layout();
        if let Some(instrument) = instrument {
            self.show_in(&pane, instrument);
        }
        self.sync_header();
        self.rebuild_indicator_legend();
    }

    /// Open a new chartbook, showing one chart on whatever you were looking
    /// at.
    ///
    /// One chart rather than a copy of the arrangement you were in: splitting
    /// is the thing that duplicates a chart in this app, and a new book you
    /// have to dismantle before you can use it is not a new book.
    /// Ask what the new chartbook is called, and which list sits beside it.
    ///
    /// A dialog rather than the inline tab the strip uses for renaming,
    /// because the strip is not there yet: going from one book to two is the
    /// moment it appears, and a caret arriving in a twenty-one pixel row at
    /// the bottom edge of the window is easy to miss entirely.
    pub fn new_chartbook(self: &Rc<Self>) {
        let proposed = default_book_name(self.books.borrow().len() + 1);
        let all = self
            .watchlist
            .borrow()
            .as_ref()
            .map(|rail| rail.watchlists())
            .unwrap_or_default();
        // A watchlist is one chartbook's, so the ones already spoken for are not
        // on offer here either — offering one would be offering to take it off
        // the book that has it.
        let taken = self.watchlists_in_use();
        let lists: Vec<(i64, String)> = all
            .iter()
            .filter(|(id, _)| *id == DEFAULT_WATCHLIST || !taken.contains(id))
            .cloned()
            .collect();

        let name = adw::EntryRow::new();
        name.set_title("Name");
        name.set_text(&proposed);

        let chosen = adw::ComboRow::new();
        chosen.set_title("Watchlist");
        // Said rather than left to be noticed: a list somebody can see in the
        // rail and cannot find in this menu needs a reason given.
        if lists.len() < all.len() {
            chosen.set_subtitle("Lists other chartbooks are using are not shown");
        }
        // The lists there are, and under a line the one there is not yet: an
        // empty list named after the chartbook, made when it is created.
        let names: Vec<&str> = lists
            .iter()
            .map(|(_, name)| name.as_str())
            .chain(std::iter::once(NEW_WATCHLIST))
            .collect();
        chosen.set_model(Some(&gtk::StringList::new(&names)));
        chosen.set_list_factory(Some(&crate::ui::preferences::separated_list(vec![lists.len()])));
        let at = lists.iter().position(|(id, _)| *id == DEFAULT_WATCHLIST).unwrap_or(0);
        chosen.set_selected(at as u32);

        // What the book is built from, after its name: None, or a chartbook
        // template it starts on and stays on. Starting on None, the book
        // this dialog has always made. Not offered at all while there are no
        // templates, so the dialog is what it was until somebody saves one.
        let templates = self.book_templates();
        let template = adw::ComboRow::new();
        template.set_title("Template");
        let template_names: Vec<&str> = std::iter::once("None")
            .chain(templates.iter().map(|t| t.name.as_str()))
            .collect();
        template.set_model(Some(&gtk::StringList::new(&template_names)));
        template.set_selected(0);

        let rows = adw::PreferencesGroup::new();
        rows.add(&name);
        if !templates.is_empty() {
            rows.add(&template);
        }
        rows.add(&chosen);

        let field = name.clone();
        let dialog = crate::ui::dialogs::card("New chartbook", None, Some(rows.upcast_ref()));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("create", "Create");
        dialog.set_response_appearance("create", adw::ResponseAppearance::Suggested);
        // Creating is not destroying, so Return may finish it — unlike the
        // alerts that delete something, which default to Cancel on purpose.
        dialog.set_default_response(Some("create"));
        dialog.set_close_response("cancel");

        // A chartbook with no name is one you cannot tell from its neighbours
        // in the strip, so blank is not a thing to fall back from — it is a
        // thing to wait for. The field arrives filled, so clearing it is
        // deliberate, and the only answer is to stop offering Create.
        let gate = dialog.clone();
        let blank_blocks = move |field: &adw::EntryRow| {
            gate.set_response_enabled("create", !field.text().trim().is_empty());
        };
        blank_blocks(&name);
        let gating = blank_blocks.clone();
        name.connect_changed(move |field| gating(field));

        // Finishing from the keyboard, by the one route that creates a
        // chartbook. The binding has no emitter for this signal, so the signal
        // is emitted by name — and the dialog is then closed by hand, because
        // only a real response does that for us. A button press arrives at the
        // same handler having already closed itself.
        let finish: Rc<dyn Fn()> = {
            let dialog = dialog.clone();
            let field = name.clone();
            Rc::new(move || {
                // Return reaches here whether or not the button is offered, so
                // the gate has to be asked again rather than assumed.
                if field.text().trim().is_empty() {
                    return;
                }
                dialog.emit_by_name::<()>("response", &[&"create"]);
                dialog.close();
            })
        };

        let committing = finish.clone();
        crate::ui::dialogs::commit_on_ctrl_enter(&dialog, move || committing());

        // An EntryRow keeps Return for itself and emits this instead, so the
        // dialog's default response never hears the key that was meant for
        // it. Typing a name and pressing Return is one gesture and should
        // finish one thing.
        let typing = finish.clone();
        name.connect_entry_activated(move |_| typing());

        let this = self.clone();
        // Three ways in and one chartbook out: the key paths emit the response
        // and then close, and a close carries its own response behind it.
        let done = Cell::new(false);
        dialog.connect_response(None, move |_, response| {
            if response != "create" || done.replace(true) {
                return;
            }
            let label = name.text().trim().to_string();
            if label.is_empty() {
                return;
            }
            let watchlist = if chosen.selected() as usize == lists.len() {
                // Made only now, so cancelling leaves no list behind that
                // nothing points at.
                match this.store.add_watchlist(&label) {
                    Some(id) => Some(id),
                    None => return,
                }
            } else {
                lists.get(chosen.selected() as usize).map(|(id, _)| *id)
            };
            let from = (template.selected() as usize)
                .checked_sub(1)
                .and_then(|at| templates.get(at))
                .cloned();
            this.open_chartbook(&label, watchlist, from);
        });
        dialog.present(Some(&self.window));

        // The name is the only thing most people change, and the proposed one
        // is a number nobody wants to keep — so it comes up chosen, and typing
        // replaces it. After presenting, because the row is not realised until
        // the frame the dialog arrives in and cannot hold the keyboard before
        // then.
        glib::idle_add_local_once(move || {
            field.grab_focus();
            field.select_region(0, -1);
        });
    }

    fn open_chartbook(
        self: &Rc<Self>,
        name: &str,
        watchlist: Option<i64>,
        template: Option<book_templates::BookTemplate>,
    ) {
        self.save_workspace();
        let instrument = self.focused_pane().instrument.borrow().clone();
        let at = self.books.borrow().len();
        // A placeholder, so that `active` names a book that exists before
        // anything else looks. Saving at the end writes the real one.
        self.books.borrow_mut().push(Chartbook {
            name: if name == default_book_name(at + 1) { String::new() } else { name.to_string() },
            layout: Node::leaf(0),
            focused: 0,
            panes: Vec::new(),
            watchlist,
            sidebar_shown: None,
            sidebar_width: None,
            maximized: None,
            book_template: None,
        });
        self.active.set(at);
        self.mount_single_chart(instrument);
        // Through the one path a book's rail always arrives by, so a list
        // chosen here and a list restored later cannot disagree.
        self.apply_sidebar(watchlist, None, None);
        self.rebuild_book_strip();
        self.save_workspace();
        // Built from the one chart, the way taking a template from the menu
        // builds from the charts there are — with nothing of its own to lose,
        // so nothing to ask.
        if let Some(template) = template {
            self.take_book_template(&template);
        }
    }

    /// Ask before taking a chartbook and its charts away.
    ///
    /// The only one does not go: there would be nothing left to show, the
    /// same guard closing the last chart already keeps.
    pub fn close_chartbook(self: &Rc<Self>) {
        if self.books.borrow().len() < 2 {
            return;
        }
        let index = self.active.get();
        let charts = self.layout.borrow().leaves().len();
        let body = format!(
            "{} and its {} will be removed. This cannot be undone.",
            self.book_label(index),
            match charts {
                1 => "chart".to_string(),
                n => format!("{n} charts"),
            },
        );

        let dialog = crate::ui::dialogs::card("Remove chartbook", Some(&body), None);
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("remove", "Remove");
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "remove" {
                this.remove_chartbook(index);
            }
        });
        dialog.present(Some(&self.window));
    }

    fn remove_chartbook(self: &Rc<Self>, index: usize) {
        if self.books.borrow().len() < 2 {
            return;
        }
        // The book that was asked about may not be the one on screen any more.
        if index != self.active.get() {
            self.activate_book(index);
        }
        let index = self.active.get();
        self.books.borrow_mut().remove(index);
        // The one that slid into its place, or the new last one.
        let next = index.min(self.books.borrow().len() - 1);
        self.active.set(next);
        if !self.materialise_book(next) {
            self.mount_single_chart(None);
        }
        self.rebuild_book_strip();
        self.save_workspace();
    }

    /// Show a different chartbook, writing the one on screen back first.
    pub fn activate_book(self: &Rc<Self>, index: usize) {
        if index >= self.books.borrow().len() || index == self.active.get() {
            return;
        }
        self.save_workspace();
        self.active.set(index);
        if !self.materialise_book(index) {
            self.mount_single_chart(None);
        }
        self.rebuild_book_strip();
        self.save_workspace();
        // The keyboard to the chart that was focused in this book, once its
        // panes are on screen. Left where GTK put it, it lands on the first
        // button of the new book's header, which wears a focus ring and takes
        // the next key meant for the chart.
        let this = self.clone();
        glib::idle_add_local_once(move || {
            this.focused_pane().view.area.grab_focus();
        });
    }

    /// Walk to the chartbook before or after this one, round the ends.
    pub fn step_chartbook(self: &Rc<Self>, delta: i32) {
        let count = self.books.borrow().len();
        if count < 2 {
            return;
        }
        let next = (self.active.get() as i32 + delta).rem_euclid(count as i32) as usize;
        self.activate_book(next);
    }

    /// Give a chartbook a name of its own, or take it back to its number.
    fn rename_book(self: &Rc<Self>, index: usize, name: &str) {
        let name = name.trim();
        {
            let mut books = self.books.borrow_mut();
            let Some(book) = books.get_mut(index) else { return };
            // Typing the name it already shows is not naming it, so an
            // emptied box hands it back to its position rather than leaving
            // a book with a name that stops matching where it sits.
            book.name = if name.is_empty() || name == default_book_name(index + 1) {
                String::new()
            } else {
                name.to_string()
            };
        }
        self.rebuild_book_strip();
        self.save_workspace();
    }

    /// Put a chartbook somewhere else in the strip.
    ///
    /// The order of `books` is three things at once: the strip, what
    /// Ctrl+Alt+Left and Right walk, and what is written down. Moving one entry
    /// moves all three, which is the point — there is one order, and dragging a
    /// tab is a second way to change it.
    fn move_book(self: &Rc<Self>, from: usize, to: usize) {
        {
            let mut books = self.books.borrow_mut();
            if from >= books.len() || to >= books.len() || from == to {
                return;
            }
            // Taken out before the destination is used, so a tab dropped to the
            // right lands after the tab it was dropped on and one dropped to
            // the left lands in its place — which is where the pointer was
            // either way.
            let book = books.remove(from);
            books.insert(to, book);
        }
        self.active.set(shifted(self.active.get(), from, to));
        self.rebuild_book_strip();
        // The charts on screen have not changed, only the order they are
        // written in, so this can wait with the dragged dividers.
        self.save_soon();
    }

    /// Make a copy of the chartbook on screen, put it beside it, open it, and
    /// offer its name to be typed over.
    ///
    /// Everything about the charts comes along: the layout, the symbols,
    /// resolutions, indicators, chart templates, link modes and each chart's
    /// own settings and drawings, and the chartbook template it is on. Each
    /// chart gets a name of its own among the drawings, the way splitting one
    /// does, so turning sending off on a copy takes back only what the copy
    /// drew. The watchlist does not come: a list is one chartbook's, so the
    /// copy shows the default one unless that is what the original shows too.
    pub fn duplicate_chartbook(self: &Rc<Self>) {
        self.save_workspace();
        let at = self.active.get();
        let Some(mut copy) = self.books.borrow().get(at).cloned() else { return };
        let mut next = self
            .books
            .borrow()
            .iter()
            .flat_map(|book| book.panes.iter().map(|pane| pane.id))
            .max()
            .unwrap_or(0)
            .max(self.next_pane.get())
            + 1;
        let mut ids = HashMap::new();
        for pane in &mut copy.panes {
            ids.insert(pane.id, next);
            pane.id = next;
            pane.drawing_uid = glib::uuid_string_random().to_string();
            next += 1;
        }
        self.next_pane.set(next);
        copy.layout = copy.layout.relabel(&ids);
        copy.focused = ids.get(&copy.focused).copied().unwrap_or(copy.focused);
        copy.maximized = copy.maximized.and_then(|id| ids.get(&id).copied());
        copy.name = format!("{} copy", self.book_label(at));
        if copy.watchlist.is_some_and(|list| list != DEFAULT_WATCHLIST) {
            copy.watchlist = Some(DEFAULT_WATCHLIST);
        }
        self.books.borrow_mut().insert(at + 1, copy);
        self.activate_book(at + 1);
        // Straight into the name, the way a double-click on the tab would:
        // "copy" is nobody's name for it.
        self.begin_rename(at + 1);
    }

    // -- chartbook templates -----------------------------------------------

    pub fn book_templates(&self) -> Vec<book_templates::BookTemplate> {
        book_templates::all(&self.store)
    }

    /// The chartbook template the book on screen is on, while it exists.
    pub fn active_book_template(&self) -> Option<book_templates::BookTemplate> {
        let id = self.books.borrow().get(self.active.get()).and_then(|book| book.book_template)?;
        book_templates::get(&self.store, id)
    }

    /// How many chartbooks, the one on screen included, are on `template`.
    pub fn books_on_book_template(&self, template: u32) -> usize {
        self.books.borrow().iter().filter(|book| book.book_template == Some(template)).count()
    }

    /// Whether the book on screen is on `template`.
    pub fn on_book_template(&self, template: u32) -> bool {
        self.books.borrow().get(self.active.get()).and_then(|book| book.book_template)
            == Some(template)
    }

    /// Every chartbook as it is stored, the one on screen written down first.
    fn stored_books(self: &Rc<Self>) -> Vec<serde_json::Value> {
        self.save_workspace();
        self.books.borrow().iter().filter_map(|book| serde_json::to_value(book).ok()).collect()
    }

    /// Take back books changed as stored values. Nothing, when one of them
    /// will not read back as a book: half a list is worse than the old one.
    fn adopt_books(&self, values: Vec<serde_json::Value>) -> bool {
        let Ok(books) = values
            .into_iter()
            .map(serde_json::from_value::<Chartbook>)
            .collect::<Result<Vec<_>, _>>()
        else {
            return false;
        };
        // The window's ids stay clear of any a template handed out.
        let highest = books.iter().flat_map(|book| book.panes.iter().map(|pane| pane.id)).max();
        if let Some(highest) = highest {
            self.next_pane.set(self.next_pane.get().max(highest + 1));
        }
        *self.books.borrow_mut() = books;
        true
    }

    /// Carry a change to the chartbook on screen into the template it is on,
    /// and into every other chartbook on it.
    ///
    /// Run as the arrangement is written down, which is what every change to
    /// it already ends in: a split or a close at once, a dragged divider once
    /// the drag stops (see [`Window::save_soon`]), an indicator as it lands.
    /// A book on no template stops at the first line, and one whose
    /// arrangement is still its template's — a new symbol, a new resolution —
    /// at the comparison. The other books are not on screen, so they are
    /// rewritten as data and come up changed when they are opened.
    fn carry_book_template(&self) {
        let active = self.active.get();
        let Some(id) = self.books.borrow().get(active).and_then(|book| book.book_template) else {
            return;
        };
        // The book on screen alone first: most saves are a symbol or a
        // resolution, which leave it its template's, and the other books need
        // not be written out as data to find that out.
        let unchanged = {
            let books = self.books.borrow();
            let shape = books
                .get(active)
                .and_then(|book| serde_json::to_value(book).ok())
                .and_then(|book| book_templates::capture(&book));
            match (shape, book_templates::get(&self.store, id)) {
                (Some(shape), Some(template)) => shape.same(&template.shape),
                _ => true,
            }
        };
        if unchanged {
            *self.template_leaves.borrow_mut() = self.layout.borrow().leaves();
            return;
        }
        let mut books: Vec<serde_json::Value> =
            self.books.borrow().iter().filter_map(|book| serde_json::to_value(book).ok()).collect();
        let before = self.template_leaves.borrow().clone();
        if book_templates::carry(&self.store, &mut books, active, &before) > 0 {
            self.adopt_books(books);
        }
        *self.template_leaves.borrow_mut() = self.layout.borrow().leaves();
    }

    /// Put the chartbook on screen on a template, or on none.
    ///
    /// None lets go of the template and keeps everything: a chartbook with
    /// nothing in it is not a choice anybody makes. A template replaces the
    /// arrangement, asking first when that would lose one no template holds.
    pub fn choose_book_template(self: &Rc<Self>, template: Option<u32>) {
        let Some(id) = template else {
            let at = self.active.get();
            if let Some(book) = self.books.borrow_mut().get_mut(at) {
                book.book_template = None;
            }
            self.template_leaves.borrow_mut().clear();
            self.save_workspace();
            return;
        };
        let Some(chosen) = book_templates::get(&self.store, id) else { return };
        let books = self.stored_books();
        let lost = books
            .get(self.active.get())
            .is_some_and(|book| !book_templates::kept(book, &self.book_templates()));
        if !lost {
            self.take_book_template(&chosen);
            return;
        }
        // Charts past the template's last go, drawings and all, and saying
        // only that the rest keep their symbols would hide that.
        let had = books.get(self.active.get()).map_or(0, |book| book_templates::leaves(book).len());
        let fewer = match had.saturating_sub(chosen.shape.panes.len()) {
            0 => String::new(),
            1 => " The chart past its last one is removed, with its drawings.".to_string(),
            n => format!(" The {n} charts past its last one are removed, with their drawings."),
        };
        let body = format!(
            "This chartbook\u{2019}s layout and indicators will be replaced by \
             \u{201c}{}\u{201d}. Its charts keep their symbols.{fewer}",
            chosen.name
        );
        let dialog = adw::AlertDialog::new(Some("Use Chartbook Template?"), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("replace", "Replace Layout");
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "replace" {
                this.take_book_template(&chosen);
            }
        });
        dialog.present(Some(&self.window));
    }

    fn take_book_template(self: &Rc<Self>, chosen: &book_templates::BookTemplate) {
        let mut books = self.stored_books();
        let at = self.active.get();
        let charts = crate::templates::all(&self.store);
        let mut next = book_templates::next_pane(&books);
        let Some(book) = books.get_mut(at) else { return };
        if !book_templates::apply(book, &chosen.shape, &charts, &mut next, None) {
            return;
        }
        book_templates::set_link(book, Some(chosen.id));
        if !self.adopt_books(books) {
            return;
        }
        if !self.materialise_book(at) {
            self.mount_single_chart(None);
        }
        self.save_workspace();
    }

    /// Save the chartbook on screen as a new template called `name`, and put
    /// it on it.
    pub fn save_as_new_book_template(self: &Rc<Self>, name: &str) {
        let books = self.stored_books();
        let Some(shape) = books.get(self.active.get()).and_then(book_templates::capture) else {
            return;
        };
        let made = book_templates::create(&self.store, name, shape);
        self.link_active_book(made.id);
    }

    fn link_active_book(self: &Rc<Self>, id: u32) {
        let at = self.active.get();
        if let Some(book) = self.books.borrow_mut().get_mut(at) {
            book.book_template = Some(id);
        }
        *self.template_leaves.borrow_mut() = self.layout.borrow().leaves();
        self.save_workspace();
    }

    pub fn rename_book_template(&self, id: u32, name: &str) {
        book_templates::rename(&self.store, id, name);
    }

    /// Delete a template. The chartbooks on it keep what they have.
    pub fn delete_book_template(self: &Rc<Self>, id: u32) {
        book_templates::delete(&self.store, id);
        if self.on_book_template(id) {
            self.template_leaves.borrow_mut().clear();
        }
        for book in self.books.borrow_mut().iter_mut() {
            if book.book_template == Some(id) {
                book.book_template = None;
            }
        }
        self.save_workspace();
    }

    /// Draw the row of chartbook tabs, or no row at all.
    fn refill_book_strip(self: &Rc<Self>) {
        while let Some(child) = self.book_strip.first_child() {
            self.book_strip.remove(&child);
        }
        let count = self.books.borrow().len();
        // One chartbook is how the window has always looked, and a strip
        // naming the only thing there is would be a row of furniture saying
        // nothing. It arrives with the second book and leaves with it.
        self.book_strip.set_visible(count > 1);
        if count < 2 {
            return;
        }
        for index in 0..count {
            self.book_strip.append(&self.build_book_tab(index));
        }
    }

    /// The strip anew, and the trading toggle in the home it now calls for:
    /// at the strip's right end while there is one, in the charts' corner
    /// while there is not.
    fn rebuild_book_strip(self: &Rc<Self>) {
        self.refill_book_strip();
        let toggle = self.trade.toggle.borrow().clone();
        if let Some(toggle) = toggle {
            self.place_trading_toggle(&toggle);
        }
    }

    /// One tab: click to switch, double-click to rename.
    fn build_book_tab(self: &Rc<Self>, index: usize) -> gtk::Widget {
        let tab = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tab.add_css_class("chartbook-tab");
        if index == self.active.get() {
            tab.add_css_class("active");
        }
        let label = gtk::Label::new(Some(&self.book_label(index)));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(18);
        tab.append(&label);

        let click = gtk::GestureClick::new();
        let renamer = self.clone();
        click.connect_pressed(move |_, presses, _, _| {
            if presses >= 2 {
                renamer.begin_rename(index);
            }
        });
        // Switching happens on release rather than on press, and not at all
        // once a drag has started, because switching rebuilds the strip — which
        // would pull the dragged tab out from under the drag. The release that
        // precedes a rename is harmless: it switches to the book you are about
        // to rename, which is where you wanted to be anyway.
        let dragging = Rc::new(Cell::new(false));
        let switcher = self.clone();
        let held = dragging.clone();
        click.connect_released(move |_, presses, _, _| {
            if presses < 2 && !held.get() {
                switcher.activate_book(index);
            }
        });
        tab.add_controller(click);

        // Drag a tab onto another to put it there, the same gesture the
        // watchlist and the indicator list already use for an order.
        //
        // The position travels as the payload, which it can here because
        // nothing rebuilds the strip while a drag is in flight — a tab that is
        // being dragged has not switched chartbooks.
        let source = gtk::DragSource::new();
        source.set_actions(gtk::gdk::DragAction::MOVE);
        let payload = index.to_string();
        source.connect_prepare(move |_, _, _| {
            Some(gtk::gdk::ContentProvider::for_value(&payload.to_value()))
        });
        let started = dragging.clone();
        source.connect_drag_begin(move |_, _| started.set(true));
        let finished = dragging.clone();
        source.connect_drag_end(move |_, _, _| finished.set(false));
        tab.add_controller(source);

        let from = |value: &glib::Value| value.get::<String>().ok()?.parse::<usize>().ok();
        let mover = self.clone();
        let target = drop_line::target(
            &tab,
            move |value| drop_line::edge(from(value)?, index),
            move |value| {
                let Some(from) = from(value) else { return false };
                // A tab dropped on itself is a click that travelled a few
                // pixels too far, and means nothing.
                if from == index {
                    return false;
                }
                mover.move_book(from, index);
                true
            },
        );
        tab.add_controller(target);

        // Right-clicking a tab switches to it first, then offers the menu for
        // it. The app already settles this ambiguity the same way — clicking
        // another chart's corner focuses that chart before maximizing it —
        // and it is what keeps the accelerators the rows advertise honest,
        // since both of them act on the book that is open.
        let menu = gtk::GestureClick::new();
        menu.set_button(gtk::gdk::BUTTON_SECONDARY);
        let this = self.clone();
        menu.connect_pressed(move |_, _, x, y| {
            this.activate_book(index);
            // Found in the strip rather than captured: switching books redrew
            // it, and the tab this gesture is attached to is no longer the
            // tab on screen.
            let Some(tab) = this.book_tab(this.active.get()) else { return };
            let model = gio::Menu::new();
            let book = gio::Menu::new();
            shortcuts::append(&book, "Rename", "win.rename-chartbook");
            shortcuts::append(&book, "Duplicate", "win.duplicate-chartbook");
            shortcuts::append(&book, "Remove", "win.close-chartbook");
            shortcuts::append(&book, "Take screenshot", "win.screenshot");
            model.append_section(None, &book);
            let template = gio::Menu::new();
            template.append_submenu(Some("Template"), &this.book_template_menu());
            model.append_section(None, &template);
            popup_menu(&model, &tab, x, y);
        });
        tab.add_controller(menu);
        tab.upcast()
    }

    /// The tab menu's Template submenu: None and the chartbook templates, with
    /// the one this book is on marked, Save as New Template… after them, and
    /// Manage Templates… on its own.
    ///
    /// Built when the menu opens, from the templates as they are then, so one
    /// made or renamed a moment ago — here or from a terminal — is in it.
    fn book_template_menu(&self) -> gio::Menu {
        let current = self.active_book_template().map(|t| t.id);
        if let Some(action) =
            self.window.lookup_action("book-template").and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&template_state(current));
        }
        let menu = gio::Menu::new();
        // None heads the same run as the templates: one choice among them.
        // And Save as New Template… closes it, as the way to one more.
        let made = gio::Menu::new();
        let none = gio::MenuItem::new(Some("None"), None);
        none.set_action_and_target_value(Some("win.book-template"), Some(&template_state(None)));
        made.append_item(&none);
        for template in self.book_templates() {
            let item = gio::MenuItem::new(Some(&template.name), None);
            item.set_action_and_target_value(
                Some("win.book-template"),
                Some(&template_state(Some(template.id))),
            );
            made.append_item(&item);
        }
        made.append(Some("Save as New Template\u{2026}"), Some("win.save-new-book-template"));
        menu.append_section(None, &made);
        let tend = gio::Menu::new();
        tend.append(Some("Manage Templates\u{2026}"), Some("win.manage-book-templates"));
        menu.append_section(None, &tend);
        menu
    }

    /// The tab sitting at `index` in the strip right now.
    fn book_tab(&self, index: usize) -> Option<gtk::Box> {
        let mut child = self.book_strip.first_child()?;
        for _ in 0..index {
            child = child.next_sibling()?;
        }
        child.downcast::<gtk::Box>().ok()
    }

    /// Put the chartbook on screen into the same editable tab a double-click
    /// opens, so there is one rename rather than two that have to agree.
    ///
    /// Which book that is, is read now rather than captured when the action
    /// was wired: the active one moves, and this area has already produced
    /// one bug from holding on to something that had stopped being true.
    ///
    /// A single chartbook has no strip and therefore no tab, and this does
    /// nothing — the same bargain the window already strikes with Maximize
    /// and Close chart. Offering a dialog for that one case would make it a
    /// different interaction from the ordinary one, which is the thing
    /// sharing the inline edit is for.
    fn rename_active_book(self: &Rc<Self>) {
        self.begin_rename(self.active.get());
    }

    /// Swap a tab's name for a box to type a new one in.
    ///
    /// Found in the strip rather than handed in, because the press that opens
    /// a rename has already switched books, and switching redraws the strip —
    /// the tab the gesture was attached to is not the tab on screen.
    fn begin_rename(self: &Rc<Self>, index: usize) {
        let Some(tab) = self.book_tab(index) else { return };
        let Some(label) = tab.first_child().and_downcast::<gtk::Label>() else { return };
        let entry = gtk::Entry::new();
        entry.add_css_class("chartbook-rename");
        entry.set_text(&self.book_label(index));
        entry.set_width_chars(12);
        entry.set_max_width_chars(18);
        tab.remove(&label);
        tab.append(&entry);
        entry.grab_focus();
        entry.select_region(0, -1);

        // Enter commits, and so does clicking away — but only one of them
        // does, because committing twice would rebuild the strip underneath
        // the box that is still handing in its text.
        let done = Rc::new(Cell::new(false));
        let commit = {
            let this = self.clone();
            let done = done.clone();
            move |entry: &gtk::Entry| {
                if done.replace(true) {
                    return;
                }
                this.rename_book(index, &entry.text());
            }
        };

        let on_activate = commit.clone();
        entry.connect_activate(move |entry| on_activate(entry));

        let focus = gtk::EventControllerFocus::new();
        let entry_weak = entry.downgrade();
        let on_leave = commit.clone();
        focus.connect_leave(move |_| {
            if let Some(entry) = entry_weak.upgrade() {
                on_leave(&entry);
            }
        });
        entry.add_controller(focus);

        // Escape puts the name back, which means leaving the book alone and
        // drawing the strip again.
        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        let done_on_escape = done.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gtk::gdk::Key::Escape {
                return glib::Propagation::Proceed;
            }
            done_on_escape.set(true);
            this.rebuild_book_strip();
            glib::Propagation::Stop
        });
        entry.add_controller(keys);
    }

    /// Draw where the pointer on `from` is on every other chart showing its
    /// symbol, and on no other chart.
    ///
    /// Whatever their link mode or resolution: what decides it is the
    /// symbol, since a price and a moment only mean the same thing on two
    /// charts of the same one. With the setting off nothing is drawn, and a
    /// line still standing from before it was turned off goes as soon as the
    /// pointer moves.
    fn echo_crosshair(self: &Rc<Self>, from: u32, echo: Option<Echo>) {
        let symbol = |pane: &ChartPane| {
            pane.instrument.borrow().as_ref().map(|i| (i.symbol.clone(), i.suffix.clone()))
        };
        let sent = self
            .pane(from)
            .and_then(|source| symbol(&source))
            .filter(|_| self.store.setting_bool(SETTING_SYNC_CROSSHAIR, true));
        for pane in self.panes.borrow().iter() {
            if pane.id == from {
                continue;
            }
            let show = sent.is_some() && symbol(pane) == sent;
            pane.view.set_echo(if show { echo } else { None });
        }
    }

    /// Stop every echo, for when the setting is switched off or the panes
    /// change under it.
    pub fn clear_echoes(self: &Rc<Self>) {
        for pane in self.panes.borrow().iter() {
            pane.view.set_echo(None);
        }
    }

    /// What the Follow charts in the open chartbook are already showing.
    fn follow_instrument(&self) -> Option<Instrument> {
        self.panes
            .borrow()
            .iter()
            .filter(|p| p.mode() == LinkMode::Follow)
            .find_map(|p| p.instrument.borrow().clone())
    }

    /// Point the rail at `instrument`, where `mode` is Follow — the charts
    /// every list drives.
    ///
    /// Through [`highlight`](Watchlist::highlight), which selects the row and
    /// tells nobody. That is what stops this coming back round: the rail's loud
    /// door is `pick`, whose `row-selected` handler is the one path that
    /// reaches `spread` again, and `highlight` brackets its selection in the
    /// rail's `quiet` flag so that handler returns at once.
    ///
    /// A list that does not hold the symbol keeps the row it was on. There is
    /// nothing to select, and clearing the selection would lose somebody's
    /// place in a list of forty over a symbol that was never in it.
    fn rail_follows(&self, instrument: &Instrument, mode: LinkMode) {
        let rail = self.watchlist.borrow().as_ref().cloned();
        let Some(rail) = rail.filter(|_| mode == LinkMode::Follow) else { return };
        rail.highlight(instrument);
    }

    /// Point the rail at whatever the Follow charts are showing.
    ///
    /// The answer to the rail being one widget showing one list: a list that
    /// is not on screen cannot be pointed at anything when the Follow charts
    /// move. There is nothing to write down to make up for that, because their
    /// symbol is already written in the charts — so the list is pointed at it
    /// the moment it is shown instead, which cannot go stale.
    fn rail_follows_follow_charts(self: &Rc<Self>) {
        let rail = self.watchlist.borrow().as_ref().cloned();
        let Some(rail) = rail else { return };
        if let Some(instrument) = self.follow_instrument() {
            rail.highlight(&instrument);
        }
    }

    /// The window's own controls, over its top-right corner: the watchlist
    /// toggle and the main menu.
    ///
    /// There is no header bar. It was a band across the whole window holding
    /// an app name, two buttons and the desktop's close button — which on a
    /// tiling desktop is furniture and everywhere else is Alt+F4 — and it cost
    /// every chart in the top row forty-seven pixels. The cluster sits over
    /// the rail when it is open, which is where the HIG puts a sidebar's menu,
    /// and over the top-right chart when it is not.
    ///
    /// A `WindowHandle`, so the cluster is also what the window is dragged by,
    /// double-clicked to maximise and right-clicked for the window menu —
    /// everything a header bar's blank space did.
    fn build_corner(self: &Rc<Self>, split: &gtk::Paned) -> gtk::WindowHandle {
        let menu = gio::Menu::new();
        let books = gio::Menu::new();
        shortcuts::append(&books, "New chartbook", "win.new-chartbook");
        menu.append_section(None, &books);
        shortcuts::append(&menu, "Preferences", "win.preferences");
        shortcuts::append(&menu, "Log", "win.log");
        shortcuts::append(&menu, "Keyboard Shortcuts", "win.shortcuts");
        // A section of its own, so the menu draws its own separator above it.
        let about = gio::Menu::new();
        shortcuts::append(&about, "About Omacharts", "win.about");
        menu.append_section(None, &about);
        let menu_button = gtk::MenuButton::new();
        menu_button.set_icon_name("open-menu-symbolic");
        crate::ui::menus::set_model(&menu_button, &menu);
        let tip = shortcuts::tooltip_with_key("Main Menu", shortcuts::MAIN_MENU);
        menu_button.set_tooltip_text(Some(&tip));
        // F10 opens the primary menu, and the header bar used to be the only
        // thing saying this menu was the primary one.
        menu_button.set_primary(true);
        menu_button.add_css_class("flat");
        *self.messages.menu_button.borrow_mut() = Some(menu_button.clone());

        let toggle = gtk::ToggleButton::new();
        toggle.set_icon_name("sidebar-show-right-symbolic");
        toggle.set_tooltip_text(Some(&shortcuts::tooltip("Watchlist", "win.watchlist")));
        toggle.add_css_class("flat");
        toggle.set_active(split.end_child().map(|rail| rail.is_visible()).unwrap_or(false));
        let split_weak = split.downgrade();
        let store = self.store.clone();
        let this = self.clone();
        toggle.connect_toggled(move |toggle| {
            if let Some(rail) = split_weak.upgrade().and_then(|split| split.end_child()) {
                rail.set_visible(toggle.is_active());
            }
            // Also the window's own, which is what seeds the next chartbook
            // and what a window with nothing written down opens with.
            store.set_setting_bool(SHOW_WATCHLIST, toggle.is_active());
            this.mark_sidebar("watchlist", toggle.is_active());
            // Closing the rail drops these controls onto the top-right chart,
            // where its own corner is.
            this.sync_corner_clearance();
            this.save_soon();
        });

        *self.watchlist_toggle.borrow_mut() = Some(toggle.clone());

        // The drawing tools' toggle has a corner of its own, the bottom-left.
        // Stateful, so the chart menu's Sidebars rows are ticks that say
        // whether each is open, however it was opened.
        let shown = self.drawing_bar.borrow().as_ref().is_some_and(|bar| bar.is_shown());
        let action = gio::SimpleAction::new_stateful("drawing-tools", None, &shown.to_variant());
        let this = self.clone();
        action.connect_activate(move |_, _| this.toggle_drawing_tools());
        self.window.add_action(&action);

        // Ctrl+Shift+B shows the rail with the keyboard in it, and hides it with the
        // keyboard back on the chart. A plain toggle: it used to focus a rail
        // that was open but not focused, so after a click on a chart it took
        // two presses to close what was plainly open.
        let action = gio::SimpleAction::new_stateful("watchlist", None, &toggle.is_active().to_variant());
        let this = self.clone();
        action.connect_activate(move |_, _| this.toggle_watchlist());
        self.window.add_action(&action);

        let cluster = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        cluster.add_css_class("window-corner");
        // The axis's width: with the ring's width of margin standing it off
        // the window's edge (see the stylesheet), it stands on the axis.
        cluster.set_size_request(crate::ui::chart::PRICE_AXIS_W as i32, -1);
        cluster.append(&toggle);
        cluster.append(&menu_button);

        let handle = gtk::WindowHandle::new();
        handle.set_child(Some(&cluster));
        handle.set_halign(gtk::Align::End);
        handle.set_valign(gtk::Align::Start);
        handle
    }

    /// Fill the resolution strip from the list.
    /// Build every chart's resolution strip. Called when the list of
    /// resolutions changes, which is the only time the buttons differ.
    fn rebuild_timeframes(self: &Rc<Self>) {
        let panes = self.panes.borrow().clone();
        for pane in panes {
            self.rebuild_strip_of(&pane);
        }
    }

    fn rebuild_strip_of(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        while let Some(child) = pane.strip.first_child() {
            pane.strip.remove(&child);
        }
        let current = pane.timeframe.get();
        let mut first: Option<gtk::ToggleButton> = None;
        let mut buttons = Vec::new();

        for timeframe in self.timeframes.borrow().iter().copied() {
            let button = gtk::ToggleButton::with_label(&timeframe.label());
            button.add_css_class("flat");
            button.set_tooltip_text(Some(&timeframe.description()));
            // A click picks a resolution; it does not move the keyboard. Taking
            // the focus left a ring on the button that outlived the click, and
            // the keys meant for the chart went to the strip instead.
            button.set_focus_on_click(false);
            match &first {
                Some(anchor) => button.set_group(Some(anchor)),
                None => first = Some(button.clone()),
            }
            button.set_active(timeframe == current);

            // The strip belongs to its own chart, so clicking one focuses that
            // chart first: changing a resolution is not a reason to leave the
            // keyboard pointing somewhere else.
            let this = self.clone();
            let owner = pane.id;
            button.connect_toggled(move |button| {
                if button.is_active() {
                    this.focus(owner);
                    this.set_timeframe(timeframe);
                }
            });
            pane.strip.append(&button);
            buttons.push((timeframe, button));
        }
        *pane.buttons.borrow_mut() = buttons;

        // The same list behind the resolution beside the symbol, which stays
        // when the strip has no room — the way a narrow TradingView chart
        // folds its intervals into one dropdown.
        let timeframes: Vec<Timeframe> = self.timeframes.borrow().clone();
        let owner = pane.id;
        let marked = Rc::downgrade(self);
        let choosing = Rc::downgrade(self);
        crate::ui::menus::choice(
            &pane.timeframe_menu,
            "resolution",
            timeframes.iter().map(|timeframe| (timeframe.description(), timeframe.key())).collect(),
            move || {
                marked
                    .upgrade()
                    .and_then(|window| window.pane(owner))
                    .map(|pane| pane.timeframe.get().key())
                    .unwrap_or_default()
            },
            move |key| {
                let Some(window) = choosing.upgrade() else { return };
                let Some(timeframe) = timeframes.iter().copied().find(|timeframe| timeframe.key() == key) else { return };
                window.focus(owner);
                window.set_timeframe(timeframe);
            },
        );
    }

    /// Put a resolution on the strip, in the place its length earns it.
    fn remember_timeframe(self: &Rc<Self>, timeframe: Timeframe) {
        {
            let mut timeframes = self.timeframes.borrow_mut();
            if timeframes.contains(&timeframe) {
                return;
            }
            timeframes.push(timeframe);
            timeframes.sort_by_key(|t| t.seconds());
        }
        self.store
            .set_setting(SETTING_TIMEFRAMES, &format_timeframes(&self.timeframes.borrow()));
        self.rebuild_timeframes();
    }

    /// The modal for editing the strip.
    fn edit_timeframes(self: &Rc<Self>) {
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();

        let rebuild: Rebuild = Rc::new(RefCell::new(None));

        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("3m, 45m, 2h, 1D"));
        entry.set_valign(gtk::Align::Center);
        let add_row = adw::ActionRow::new();
        add_row.set_title("Add");
        add_row.add_suffix(&entry);
        group.add(&add_row);

        let rows: Rc<RefCell<Vec<adw::ActionRow>>> = Rc::new(RefCell::new(Vec::new()));
        let fill = {
            let this = self.clone();
            let group = group.clone();
            let rows = rows.clone();
            let rebuild = rebuild.clone();
            move || {
                for row in rows.borrow_mut().drain(..) {
                    group.remove(&row);
                }
                let listed: Vec<Timeframe> = this.timeframes.borrow().clone();
                for timeframe in listed {
                    let row = adw::ActionRow::new();
                    row.set_title(&timeframe.label());
                    row.set_subtitle(&timeframe.description());

                    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
                    remove.add_css_class("flat");
                    remove.set_valign(gtk::Align::Center);
                    // The last one cannot go: a strip with nothing on it is a
                    // chart with no way back to a resolution.
                    // Every one of them can go. A chart with no strip is a
                    // usable chart: typing a resolution works whether or not
                    // it is listed.
                    remove.set_sensitive(true);

                    let this = this.clone();
                    let rebuild = rebuild.clone();
                    remove.connect_clicked(move |_| {
                        this.timeframes.borrow_mut().retain(|t| *t != timeframe);
                        this.store.set_setting(
                            SETTING_TIMEFRAMES,
                            &format_timeframes(&this.timeframes.borrow()),
                        );
                        this.rebuild_timeframes();
                        if let Some(rebuild) = rebuild.borrow().as_ref() {
                            rebuild();
                        }
                    });
                    row.add_suffix(&remove);
                    group.add(&row);
                    rows.borrow_mut().push(row);
                }
            }
        };
        fill();
        *rebuild.borrow_mut() = Some(Box::new(fill));

        let this = self.clone();
        let rebuild_on_add = rebuild.clone();
        entry.connect_activate(move |entry| {
            let Some(timeframe) = Timeframe::parse(&entry.text()) else { return };
            this.remember_timeframe(timeframe);
            entry.set_text("");
            if let Some(rebuild) = rebuild_on_add.borrow().as_ref() {
                rebuild();
            }
        });

        page.add(&group);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&page));

        let dialog = adw::Dialog::new();
        dialog.set_title("Resolutions");
        dialog.set_content_width(420);
        dialog.set_content_height(520);
        dialog.set_child(Some(&toolbar));
        dialog.present(Some(&self.window));
    }

    fn build_watchlist(self: &Rc<Self>) -> Rc<Watchlist> {
        let store = self.store.clone();
        // The feed serving a row as it stands when a quote is asked for, not
        // the one there was when the rail was built: a feed switch has to
        // reach the rail's numbers too, and the rail outlives the switch.
        let window = Rc::downgrade(self);
        // Quotes come from the cache alone. A rail that fetches is a rail that
        // gets you throttled.
        let quote: crate::ui::watchlist::QuoteLookup = Rc::new(move |instrument: &Instrument| {
            let routed = window.upgrade()?.route(instrument)?;
            let quote = crate::cli::quote(&store, routed.feed.provider.as_ref(), instrument)?;
            Some(Quote { last: quote.last, change: quote.change, change_pct: quote.change_pct })
        });

        let this = self.clone();
        Watchlist::new(
            self.store.clone(),
            self.index.clone(),
            self.search.clone(),
            quote,
            move |instrument| this.show_from_rail(instrument),
        )
    }



    fn wire_shortcuts(self: &Rc<Self>) {
        // Everything the application can own outright, which is also
        // what makes the menus draw the key beside the row.
        if let Some(app) = self.window.application().and_downcast::<adw::Application>() {
            shortcuts::install(&app);
        }

        let preferences = gio::SimpleAction::new("preferences", None);
        let this = self.clone();
        preferences.connect_activate(move |_, _| this.open_preferences());
        self.window.add_action(&preferences);

        let shortcuts = gio::SimpleAction::new("shortcuts", None);
        let this = self.clone();
        shortcuts.connect_activate(move |_, _| this.show_shortcuts());
        self.window.add_action(&shortcuts);

        let log = gio::SimpleAction::new("log", None);
        let this = self.clone();
        log.connect_activate(move |_, _| this.toggle_log());
        self.window.add_action(&log);

        let about = gio::SimpleAction::new("about", None);
        let this = self.clone();
        about.connect_activate(move |_, _| this.show_about());
        self.window.add_action(&about);

        let find = gio::SimpleAction::new("find", None);
        let this = self.clone();
        find.connect_activate(move |_, _| this.open_search());
        self.window.add_action(&find);

        let new_book = gio::SimpleAction::new("new-chartbook", None);
        let this = self.clone();
        new_book.connect_activate(move |_, _| {
            // Ctrl+N in the rail means a new symbol. The key is one
            // accelerator and an accelerator is offered above whatever has the
            // keyboard, so the rail cannot take it first — the action asks it
            // instead, and only makes a chartbook when the answer is no.
            let rail = this.watchlist.borrow().as_ref().cloned();
            if rail.is_some_and(|rail| rail.add_symbol_if_focused()) {
                return;
            }
            this.new_chartbook();
        });
        self.window.add_action(&new_book);

        let rename_book = gio::SimpleAction::new("rename-chartbook", None);
        let this = self.clone();
        rename_book.connect_activate(move |_, _| this.rename_active_book());
        self.window.add_action(&rename_book);

        let duplicate_book = gio::SimpleAction::new("duplicate-chartbook", None);
        let this = self.clone();
        duplicate_book.connect_activate(move |_, _| this.duplicate_chartbook());
        self.window.add_action(&duplicate_book);

        // Stateful, so the menu marks the template the book is on. The state
        // is set as the menu opens rather than kept up to date, because
        // nothing else shows it.
        let book_template = gio::SimpleAction::new_stateful(
            "book-template",
            Some(glib::VariantTy::STRING),
            &template_state(None),
        );
        let this = self.clone();
        book_template.connect_activate(move |_, value| {
            let Some(key) = value.and_then(|v| v.get::<String>()) else { return };
            this.choose_book_template(key.parse().ok());
        });
        self.window.add_action(&book_template);

        let save_new = gio::SimpleAction::new("save-new-book-template", None);
        let this = self.clone();
        save_new.connect_activate(move |_, _| crate::ui::book_templates::save_new(&this));
        self.window.add_action(&save_new);

        let manage = gio::SimpleAction::new("manage-book-templates", None);
        let this = self.clone();
        manage.connect_activate(move |_, _| crate::ui::book_templates::manage(&this));
        self.window.add_action(&manage);

        let close_book = gio::SimpleAction::new("close-chartbook", None);
        let this = self.clone();
        close_book.connect_activate(move |_, _| this.close_chartbook());
        self.window.add_action(&close_book);

        let shot = gio::SimpleAction::new("screenshot", None);
        let this = self.clone();
        shot.connect_activate(move |_, _| {
            screenshot::take_chartbook(
                &this.window,
                &this.store,
                &this.chart_host,
                &this.book_label(this.active.get()),
                &this.theming.borrow().theme(),
            );
        });
        self.window.add_action(&shot);

        let keys = gtk::EventControllerKey::new();
        let this = self.clone();
        keys.connect_key_pressed(move |_, key, keycode, state| {
            use gtk::gdk::Key;
            let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
            let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
            let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);

            // Ctrl+Alt+Shift and the number row picks the focused chart's
            // template: 1 to 9 the templates in the order they are listed, 0
            // none. By where the key is rather than what it types, because
            // with Shift held the row types "!" and "\"" on one layout, "!"
            // and "@" on another and digits only on a third — the trap the
            // note on chart settings in `shortcuts` describes.
            if ctrl && alt && shift {
                if let Some(n) = shifted_number(key, keycode) {
                    this.pick_template(n);
                    return glib::Propagation::Stop;
                }
                // Page up and down rather than the arrows, which with these
                // three resize the chart and rotate the rail's watchlists.
                let step = match key {
                    Key::Page_Up | Key::KP_Page_Up => Some(-1),
                    Key::Page_Down | Key::KP_Page_Down => Some(1),
                    _ => None,
                };
                if let Some(step) = step {
                    this.step_template(step);
                    return glib::Propagation::Stop;
                }
            }

            // Ctrl+Shift and a digit shows that watchlist, counting down the
            // switcher from 1. From anywhere, rail open or not: nothing is
            // typed with Ctrl held. Bare digits are the resolution's, Alt
            // with one a drawing configuration's and Ctrl with one the
            // trading size's. By where the key is, as the templates' are,
            // since Shift changes what the row types.
            if ctrl
                && !alt
                && shift
                && let Some(n) = shifted_number(key, keycode).filter(|n| *n > 0)
                && let Some(rail) = this.watchlist.borrow().clone()
                && rail.show_numbered(n)
            {
                return glib::Propagation::Stop;
            }

            // The trading keys, only with a trading account and never while
            // something is being typed: Ctrl+B buys and Ctrl+S sells the
            // focused chart's size at the market, Ctrl+F goes flat in its
            // contract, Ctrl and a digit picks its size. With no account
            // there are no actions for them, and they do nothing at all.
            if ctrl && !alt && !shift && !this.is_typing() && this.on_trading_key(key, keycode) {
                return glib::Propagation::Stop;
            }

            match key {
                // Ctrl+G: go to a stretch of time. Only off a box: in one,
                // Ctrl+G is the box's.
                Key::g | Key::G if ctrl && !alt && !shift && !this.is_typing() => {
                    this.prompt_range();
                    return glib::Propagation::Stop;
                }
                // Alt+Left and Right walk every chart in reading order; Up and
                // Down go to the chart that way. See `focus_along`.
                Key::Left if alt && !ctrl => {
                    this.focus_along(-1);
                    return glib::Propagation::Stop;
                }
                Key::Right if alt && !ctrl => {
                    this.focus_along(1);
                    return glib::Propagation::Stop;
                }
                Key::Up if alt && !ctrl => {
                    this.focus_towards(0, -1);
                    return glib::Propagation::Stop;
                }
                Key::Down if alt && !ctrl => {
                    this.focus_towards(0, 1);
                    return glib::Propagation::Stop;
                }
                // Ctrl+L: the focused chart's next link mode, and with Shift
                // the one before. A letter, so it is the same key on every
                // layout, and L for link; Alt+L is the line tool, a different
                // register.
                Key::l | Key::L if ctrl && !alt => {
                    this.cycle_mode(&this.focused_pane(), shift);
                    return glib::Propagation::Stop;
                }
                // Back to the chart, from wherever the keyboard ended up —
                // unless a dialog is open, which is then what Escape closes.
                //
                // A dialog normally closes itself and this never runs: the
                // shortcut that does it sits on the sheet the dialog is drawn
                // in, so it only ever sees a key on its way up from whatever
                // inside the dialog holds the keyboard, and the key never
                // reaches the window at all. Arriving here means the dialog
                // has not got the keyboard — it never took it, or something
                // took it back — and the sheet will never hear the key. This
                // controller is the last thing above it, so the closing is
                // done here.
                Key::Escape => {
                    if let Some(dialog) = this.window.visible_dialog() {
                        // A pushed page pops rather than taking the dialog
                        // with it, which is what Escape means inside one.
                        if let Some(prefs) = dialog.downcast_ref::<adw::PreferencesDialog>()
                            && prefs.pop_subpage()
                        {
                            return glib::Propagation::Stop;
                        }
                        dialog.close();
                        return glib::Propagation::Stop;
                    }
                    // Whatever the chart was in the middle of — a tool in
                    // hand, a drawing selected — Escape ends it, wherever
                    // the keyboard was.
                    this.focused_pane().view.cancel();
                    this.focused_pane().view.area.grab_focus();
                    return glib::Propagation::Stop;
                }
                // Ctrl+Shift+I skips the list and opens the picker, because
                // adding one is what you usually came for. It has to be tested
                // before plain Ctrl+I or that arm swallows it.
                Key::i | Key::I if ctrl && shift => {
                    this.add_indicator();
                    return glib::Propagation::Stop;
                }
                // Bare "?" as well as Ctrl+?, because it is what people try
                // first and it collides with nothing: the type-to-search path
                // only takes letters and digits.
                Key::question => {
                    this.show_shortcuts();
                    return glib::Propagation::Stop;
                }
                // F2: a selected drawing's text, or, with none selected, the
                // chart title. A box being typed in keeps it.
                Key::F2 if !ctrl && !alt && !shift && !this.is_typing() => {
                    this.press_f2();
                    return glib::Propagation::Stop;
                }
                // Ctrl+W closes the chartbook, and closes the window when
                // that was the only one — which is what it has always done,
                // and what every tabbed application does with the last tab.
                // Ctrl+Q skips the question and quits.
                Key::q if ctrl => {
                    this.window.close();
                    return glib::Propagation::Stop;
                }
                // Ctrl+Alt+Shift and an arrow resizes the focused chart,
                // growing it the way the arrow points by moving the divider it
                // sits against. Up and down are the rail's watchlist rotation
                // as well, so that is tried first and this falls through rather
                // than swallowing — two things on one key, separated by what
                // has the keyboard, the way Ctrl+N already is.
                Key::Left | Key::Right | Key::Up | Key::Down if ctrl && alt && shift => {
                    if matches!(key, Key::Up | Key::Down)
                        && this.rotate_watchlist(if key == Key::Up { -1 } else { 1 })
                    {
                        return glib::Propagation::Stop;
                    }
                    let how = match key {
                        Key::Left => pane::Resize::Left,
                        Key::Right => pane::Resize::Right,
                        Key::Up => pane::Resize::Up,
                        _ => pane::Resize::Down,
                    };
                    if !this.resize_focused(how) {
                        return glib::Propagation::Proceed;
                    }
                    return glib::Propagation::Stop;
                }
                // Walk the chart without leaving it: chartbooks sideways with
                // Ctrl+Alt, which is the larger of the things sideways could
                // mean — past the edge of this arrangement and into the next —
                // and symbols up and down, whichever pane has the keyboard.
                Key::Left if ctrl && alt => {
                    this.step_chartbook(-1);
                    return glib::Propagation::Stop;
                }
                Key::Right if ctrl && alt => {
                    this.step_chartbook(1);
                    return glib::Propagation::Stop;
                }
                Key::Up if ctrl && alt => {
                    this.step_symbol(-1);
                    return glib::Propagation::Stop;
                }
                Key::Down if ctrl && alt => {
                    this.step_symbol(1);
                    return glib::Propagation::Stop;
                }
                // Resolutions sideways with Ctrl+Shift, which nothing else
                // claims. Alt is not in it because Ctrl+Alt+Shift and an arrow
                // resizes the chart now. It stays out of the accelerator table
                // for the same reason Ctrl+V does: in an entry these two keys
                // select a word, and the entry has to have them first.
                Key::Left if ctrl && shift => {
                    this.step_timeframe(-1);
                    return glib::Propagation::Stop;
                }
                Key::Right if ctrl && shift => {
                    this.step_timeframe(1);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }

            if alt {
                return glib::Propagation::Proceed;
            }

            // The rail owns the arrows while the keyboard is on it.
            let on_watchlist = this
                .watchlist
                .borrow()
                .as_ref()
                .map(|w| w.has_focus())
                .unwrap_or(false);

            // Alt with a digit is a drawing configuration — for the tool in
            // hand or the selected drawing — and never a resolution, even
            // when the keyboard is not on the chart itself.
            if state.contains(gtk::gdk::ModifierType::ALT_MASK) {
                if let Some(n) = key.to_unicode().and_then(|c| c.to_digit(10)) {
                    this.focused_pane().view.apply_configuration(n as u8);
                    return glib::Propagation::Stop;
                }
            }

            match (key, ctrl) {
                (Key::slash, false) => {
                    this.open_search();
                    glib::Propagation::Stop
                }
                // Left and right are the chart's wherever the keyboard is: the
                // rail has nothing sideways to move through that is worth
                // taking them from the chart for.
                (Key::Left, false) => {
                    this.focused_pane().view.pan_bars(-5);
                    glib::Propagation::Stop
                }
                (Key::Right, false) => {
                    this.focused_pane().view.pan_bars(5);
                    glib::Propagation::Stop
                }
                // Up and down walk the watchlist from the chart, the way they
                // do on the rail, so a list can be read without going to it.
                // A selected drawing has had them first, as its nudge, and a
                // box being typed in keeps them for itself.
                (Key::Up, false) if !on_watchlist && !this.is_typing() => {
                    this.step_symbol(-1);
                    glib::Propagation::Stop
                }
                (Key::Down, false) if !on_watchlist && !this.is_typing() => {
                    this.step_symbol(1);
                    glib::Propagation::Stop
                }
                (Key::plus | Key::equal | Key::KP_Add, false) => {
                    this.focused_pane().view.zoom(1.0 / 1.25);
                    glib::Propagation::Stop
                }
                (Key::minus | Key::KP_Subtract, false) => {
                    this.focused_pane().view.zoom(1.25);
                    glib::Propagation::Stop
                }
                // With Ctrl held the same two keys are the text size of
                // whatever is selected, which is the pair every application
                // grows type with. Bare, they are the chart's zoom and
                // always have been — the modifier is what keeps a key that
                // means "bigger" meaning it about two different things
                // without either taking the other's.
                (Key::plus | Key::equal, true) | (Key::KP_Add, true) => {
                    let by = omacharts_engine::drawings::TEXT_SIZE_STEP;
                    match this.focused_pane().view.step_text_size(by) {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                (Key::minus, true) | (Key::KP_Subtract, true) => {
                    let by = -omacharts_engine::drawings::TEXT_SIZE_STEP;
                    match this.focused_pane().view.step_text_size(by) {
                        true => glib::Propagation::Stop,
                        false => glib::Propagation::Proceed,
                    }
                }
                (Key::End, false) if !on_watchlist => {
                    this.focused_pane().view.go_to_latest();
                    glib::Propagation::Stop
                }
                (_, true) => glib::Propagation::Proceed,
                _ => {
                    // Start typing and the chart does what every charting tool
                    // does: letters look for a symbol, digits set the
                    // resolution. The keystroke carries into the box.
                    match key.to_unicode() {
                        Some(c) if c.is_ascii_alphabetic() => {
                            this.open_search_with(&c.to_string());
                            glib::Propagation::Stop
                        }
                        Some(c) if c.is_ascii_digit() => {
                            this.prompt_resolution(&c.to_string());
                            glib::Propagation::Stop
                        }
                        _ => glib::Propagation::Proceed,
                    }
                }
            }
        });
        self.window.add_controller(keys);

        // Ctrl+C and Ctrl+V are copy and paste, and GTK claims both before
        // a controller on the window ever sees them — which is why the
        // vertical split did nothing here while the horizontal one worked,
        // back when those were the split's keys. A drawing's clipboard has
        // to be caught on the way down for the same reason.
        //
        // Only when the focus is not in something you can type into, or
        // copying a symbol out of a box would copy a drawing instead.
        let capture = gtk::EventControllerKey::new();
        capture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        capture.connect_key_pressed(move |_, key, _, state| {
            use gtk::gdk::Key;
            if !state.contains(gtk::gdk::ModifierType::CONTROL_MASK) || this.is_typing() {
                return glib::Propagation::Proceed;
            }
            // Only the bare Ctrl combos. Catching a key here means the
            // accelerator for it never runs — which is how Ctrl+Alt+V, the
            // vertical split, came to be eaten as a paste that had nothing
            // to paste.
            let other = gtk::gdk::ModifierType::SHIFT_MASK | gtk::gdk::ModifierType::ALT_MASK;
            if state.intersects(other) {
                return glib::Propagation::Proceed;
            }
            match key {
                // Copy and paste, outright. They used to share Ctrl+V with
                // the vertical split, which is a key that can only ever
                // mean one thing to the hand that presses it; the split is
                // on Ctrl+Alt+V now.
                Key::x | Key::X => this.cut(),
                Key::c | Key::C => this.copy(),
                Key::v | Key::V => {
                    this.paste();
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.window.add_controller(capture);
    }

    /// Fold arriving bars into the chart. Runs on the main thread.
    ///
    /// One of these per feed, holding its feed weakly: the feed owns the
    /// loader whose worker owns the sender, so a strong hold here would keep
    /// a retired feed's thread alive for as long as it sent nothing.
    fn wire_responses(
        self: &Rc<Self>,
        feed: std::rc::Weak<Feed>,
        receiver: async_channel::Receiver<Response>,
    ) {
        let this = self.clone();
        let generation = self.feed_generation.get();
        glib::spawn_future_local(async move {
            while let Ok(response) = receiver.recv().await {
                // The feed this reply came from has been retired. Whatever
                // the worker had already sent is for charts that no longer
                // exist in that world, and this task ends with it.
                if this.feed_generation.get() != generation {
                    break;
                }
                let Some(feed) = feed.upgrade() else { break };
                if feed.retired.get() {
                    break;
                }
                match response {
                    // The rail reads the newer of a row's bars and its closes,
                    // so a batch the widget asked for moves its prices too.
                    Response::Closes { keys, failure } => {
                        if !keys.is_empty()
                            && let Some(rail) = this.watchlist.borrow().as_ref()
                        {
                            rail.refresh_quotes();
                            this.write_changes();
                        }
                        // Any failure, not only a refusal: a batch that came
                        // back with nothing leaves every row in it due, and
                        // the next tick would send it again a minute later.
                        if let Some(failure) = failure {
                            this.pause_backfill(&feed);
                            this.say(crate::log_words::closes_failed(feed.provider.label(), failure));
                        }
                    }
                    Response::Bars { key, timeframe, bars } => {
                        this.present(&key, timeframe, bars, None);
                        // One at a time, each asked for as the last lands.
                        feed.loader.keep_warming(&key, timeframe, |tf| {
                            this.store.coverage(&key, tf).is_some()
                        });
                    }
                    Response::Failed { key, timeframe, bars, failure, unasked } => {
                        // The one place a failed fetch is taken in, so the
                        // one place it is logged.
                        let symbol = key.split_once(':').map_or(key.as_str(), |(_, symbol)| symbol);
                        this.say(crate::log_words::fetch_failed(feed.provider.label(), symbol, timeframe, failure));
                        // A failure shows the same chart and says what went
                        // wrong. Never an empty pane, and never a pane that
                        // blames the symbol for a request that never arrived.
                        //
                        // Unless nobody asked. A refresh that was refused
                        // leaves the chart exactly as it was, because the one
                        // thing this feature must never do is put "rate
                        // limited" in the corner of a chart somebody is
                        // reading happily, about a request they did not make
                        // and cannot act on.
                        if !unasked {
                            this.present(&key, timeframe, bars, Some(failure));
                        }
                        // Three more requests is the worst possible answer to
                        // a provider that has just refused one.
                        feed.loader.stop_warming(&key);
                        if unasked {
                            this.pause_refresh(&feed);
                        }
                        // Being throttled is the one failure worth acting on
                        // rather than just showing: carrying on would spend a
                        // queue of speculative requests on certain refusals
                        // and keep the cooldown climbing.
                        if failure == FetchFailure::RateLimited {
                            this.pause_backfill(&feed);
                        }
                    }
                }
            }
        });
    }

    /// Put the bar widget in the bar, the first time this runs on a desktop
    /// that has one.
    ///
    /// Once, and remembered. Taking the widget out is a choice, and a choice
    /// that undid itself on the next launch would not be one — so the question
    /// is recorded as asked, rather than inferred from whether the widget
    /// happens to be there.
    ///
    /// Only from the default store, like the refresh at launch: a test
    /// window on a throwaway store has never been asked, and would put the
    /// widget back in a bar somebody took it out of.
    fn offer_bar_widget(self: &Rc<Self>) {
        let home = crate::store::home();
        if self.store.setting_bool(SETTING_WIDGET_OFFERED, false)
            || !crate::bar_plugin::launch_owns_widget(&home)
            || !crate::bar_plugin::available(&home)
        {
            return;
        }
        self.store.set_setting_bool(SETTING_WIDGET_OFFERED, true);
        if let Err(error) = crate::bar_plugin::install(&home) {
            eprintln!("omacharts: bar widget not installed: {error}");
        }
    }

    fn wire_theme_polling(self: &Rc<Self>) {
        let this = self.clone();
        glib::timeout_add_seconds_local(THEME_POLL_SECONDS, move || {
            if this.theming.borrow_mut().poll_omarchy() {
                this.restyle();
            }
            glib::ControlFlow::Continue
        });
    }

    /// Repaint with the current theme and bar scheme.
    pub fn restyle(self: &Rc<Self>) {
        let theme = {
            let mut theming = self.theming.borrow_mut();
            theming.apply();
            theming.theme()
        };
        if let Some(bar) = self.drawing_bar.borrow().as_ref() {
            bar.restyle();
        }
        let (house, swapped) = self.house_look();
        for pane in self.panes.borrow().iter() {
            pane.view.restyle(theme.clone(), pane_scheme(pane, &theme));
            pane.view.set_house_look(house, swapped);
            self.apply_marks_to(pane);
            // The colour is the theme's answer about the mode, which has just
            // changed.
            let mode = pane.mode();
            pane.set_mode(mode, mode.colour(&theme));
        }
        // Indicators without a colour of their own take one from the theme's
        // palette, and that answer just changed. Repainting without asking
        // again leaves them wearing the old theme's colours.
        self.rebuild_indicator_legend();
        self.redraw_current();
        if let Some(watchlist) = self.watchlist.borrow().as_ref() {
            watchlist.rebuild();
        }
        // Whether a colouring has two tones to swap is the theme's answer.
        self.chart_changed_elsewhere();
    }

    fn open_search(self: &Rc<Self>) {
        self.open_search_with("");
    }

    /// The symbol picker, for what chart `id` trades instead of what it
    /// charts.
    pub fn find_trading_symbol(self: &Rc<Self>, id: u32) {
        let this = self.clone();
        // Over the chart settings it is asked from, which hold the keyboard.
        let from: gtk::Widget = self.window.visible_dialog().map_or_else(|| self.window.clone().upcast(), |dialog| dialog.upcast());
        let over = Over::window(&from, crate::ui::chart_settings::TRADING_SYMBOL_LABEL);
        // What a trading service takes an order on, futures and their months,
        // whether or not one is signed in to say so yet.
        let signed_in = self.trading.borrow().logins.iter().any(|login| self.desk.session(&login.id).is_some());
        let note = (!signed_in).then_some("No trading service is signed in");
        // Only what tracks what it charts: its own root's months, its micro
        // or its full-size contract.
        let charted = self.pane(id).and_then(|pane| pane.instrument.borrow().clone());
        self.search.present_futures(over, note, charted, move |instrument| this.set_trading_symbol(id, Some(instrument)));
    }

    /// Over the focused chart, which is the one a pick changes.
    fn open_search_with(self: &Rc<Self>, query: &str) {
        let this = self.clone();
        self.search.present_with(self.over_focused(), query, move |instrument| this.show(instrument));
    }

    /// Where a picker opens: over the focused chart, which is the one a pick
    /// changes.
    fn over_focused(&self) -> Over {
        let pane = self.focused_pane();
        let panes = self.panes.borrow();
        Over::chart(&pane, panes.iter().map(|pane| pane.as_ref()))
    }

    /// The resolution box: type "3", "15", "4h", "1D".
    ///
    /// A bare number means minutes, so the digit that opened this is already
    /// the start of an answer.
    fn prompt_resolution(self: &Rc<Self>, start: &str) {
        let card = RESOLUTION_CARD.with(|card| card.get_or_init(crate::ui::range_card::ResolutionCard::new).clone());
        card.open(&self.over_focused(), self.range_context(), start);
    }

    /// Ctrl+G: how much of the past the focused chart shows. Type `6m`,
    /// `Y`, `2025`, `2025:`, or Tab into the days it lands on and set them;
    /// Space on an empty box lists the common ones.
    fn prompt_range(self: &Rc<Self>) {
        let card = RANGE_CARD.with(|card| card.get_or_init(crate::ui::range_card::RangeCard::new).clone());
        card.open(&self.over_focused(), self.range_context());
    }

    /// What the range and resolution cards need from the focused chart.
    fn range_context(self: &Rc<Self>) -> crate::ui::range_card::Context {
        let pane = self.focused_pane();
        let today = chrono::Local::now().date_naive();
        let showing = pane.view.days_shown().unwrap_or((today, today));
        let first = pane.view.first_day().unwrap_or(showing.0);
        let mut ladder = self.timeframes.borrow().clone();
        ladder.sort_by_key(|tf| tf.seconds());
        ladder.dedup();
        let weak = Rc::downgrade(self);
        let plan = move |range, forced| match weak.upgrade() {
            Some(this) => this.plan_range(range, forced),
            None => crate::ui::range_card::Plan {
                timeframe: forced.unwrap_or(Timeframe::days(1)),
                auto: false,
                fits: true,
            },
        };
        let weak = Rc::downgrade(self);
        let apply = move |range, forced| {
            if let Some(this) = weak.upgrade() {
                this.show_range_at(range, forced);
            }
        };
        let weak = Rc::downgrade(self);
        let resolution = move |timeframe| {
            if let Some(this) = weak.upgrade() {
                this.apply_timeframe(timeframe);
            }
        };
        crate::ui::range_card::Context {
            today,
            showing,
            lasting: {
                let weak = Rc::downgrade(self);
                Rc::new(move |range| match weak.upgrade() {
                    Some(this) => this.as_shown(range),
                    None => range,
                })
            },
            first,
            current: pane.timeframe.get(),
            ladder,
            plan: Rc::new(plan),
            apply: Rc::new(apply),
            resolution: Rc::new(resolution),
        }
    }

    pub fn open_chart_settings(self: &Rc<Self>) {
        crate::ui::chart_settings::ChartSettings::present(self);
    }

    pub fn open_indicators(self: &Rc<Self>) {
        crate::ui::chart_settings::ChartSettings::present_indicators(self);
    }

    /// The indicator picker, centred like every other picker in the app.
    pub fn add_indicator(self: &Rc<Self>) {
        crate::ui::chart_settings::add_indicator(self);
    }

    /// Re-fold and repaint what is on screen, after something that changes
    /// how the bars are read rather than which bars they are.
    fn redraw_current(self: &Rc<Self>) {
        self.series.borrow_mut().clear();
        let panes = self.panes.borrow().clone();
        for pane in panes {
            let instrument = pane.instrument.borrow().clone();
            if let Some(instrument) = instrument {
                self.show_in(&pane, instrument);
            }
        }
    }

    fn open_preferences(self: &Rc<Self>) {
        self.open_preferences_at(None);
    }

    /// The preferences at the tab that edits what every chart's header
    /// shows, from the header itself.
    pub fn edit_header(self: &Rc<Self>) {
        self.open_preferences_at(Some("header"));
    }

    fn open_preferences_at(self: &Rc<Self>, charts_tab: Option<&str>) {
        let this = self.clone();
        let editor = self.clone();
        let restorer = self.clone();
        Preferences::present(
            &self.window,
            self.store.clone(),
            self.theming.clone(),
            // A change on the panel is a change to the theme or to the feed,
            // and both are read back from the store rather than passed in.
            Rc::new(move || {
                this.restyle();
                this.adopt_feed();
            }),
            Rc::new(move || editor.edit_timeframes()),
            Rc::new(move || restorer.adopt_restored()),
            self.clone(),
            charts_tab,
        );
    }

    /// The shortcuts, grouped and aligned, over a box that filters them.
    ///
    /// A list of rows rather than a block of text: an alert dialog centres
    /// whatever it is given, which turns two columns into a ragged mess.
    ///
    /// The box has the keyboard from the moment the sheet arrives, because
    /// somebody who opened this came to look something up — and the only thing
    /// the sheet does with a keystroke is look it up.
    fn show_shortcuts(self: &Rc<Self>) {
        let page = adw::PreferencesPage::new();

        // The rows are kept with the strings they were built from, so
        // filtering asks the table rather than reading titles back off GTK.
        type Row = (adw::ActionRow, &'static str, &'static str, &'static str);
        let mut listed: Vec<(adw::PreferencesGroup, Vec<Row>)> = Vec::new();

        // Trading's keys only for somebody who trades: with no login they
        // do nothing, and the sheet does not list them.
        let trading = self.trading.borrow().trades().then_some(TRADING_SHORTCUTS);
        for (title, shortcuts) in SHORTCUT_SECTIONS.iter().chain(trading.iter()) {
            let group = adw::PreferencesGroup::new();
            group.set_title(title);
            let mut rows = Vec::new();
            for (keys, what) in *shortcuts {
                let row = adw::ActionRow::new();
                row.set_title(what);
                let label = gtk::Label::new(Some(keys));
                label.add_css_class("keycap");
                label.set_valign(gtk::Align::Center);
                row.add_suffix(&label);
                group.add(&row);
                rows.push((row, *title, *keys, *what));
            }
            page.add(&group);
            listed.push((group, rows));
        }

        // Said rather than left to be guessed at. With every group hidden the
        // sheet is an empty box, which reads as broken rather than as an
        // answer — and "not here" is an answer worth having.
        let nothing = adw::PreferencesGroup::new();
        let sentence = gtk::Label::new(Some("No shortcut matches that"));
        sentence.add_css_class("dim-label");
        nothing.add(&sentence);
        nothing.set_visible(false);
        page.add(&nothing);

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search"));
        search.set_hexpand(true);
        let field = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        field.set_margin_top(2);
        field.set_margin_bottom(10);
        field.set_margin_start(12);
        field.set_margin_end(12);
        field.append(&search);

        // Headings with nothing under them are worse than a shorter sheet, so a
        // group whose every row is filtered away goes too.
        let filter = move |query: &str| {
            let mut found = 0usize;
            for (group, rows) in &listed {
                let mut showing = 0usize;
                for (row, section, keys, what) in rows {
                    let hit = shortcut_matches(section, keys, what, query);
                    row.set_visible(hit);
                    showing += usize::from(hit);
                }
                group.set_visible(showing > 0);
                found += showing;
            }
            nothing.set_visible(found == 0);
        };
        search.connect_search_changed(move |entry| filter(&entry.text()));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        // A bar of its own under the header rather than a widget in it: the
        // sheet keeps its title, and the box stays put while the list scrolls.
        toolbar.add_top_bar(&field);
        toolbar.set_content(Some(&page));

        let dialog = adw::Dialog::new();
        dialog.set_title("Keyboard shortcuts");
        dialog.set_content_width(480);
        dialog.set_content_height(620);
        dialog.set_child(Some(&toolbar));

        // Escape in a search box is otherwise dead; here it means the same
        // thing it means everywhere else in the sheet.
        crate::ui::dialogs::close_on_search_escape(&search, &dialog);
        crate::ui::dialogs::close_on_click_outside(&dialog);

        // The keyboard lands in the box, so the first thing typed is the
        // search. Asked of the dialog rather than grabbed here, because the
        // sheet is not on screen yet and focuses whatever this names two
        // frames later, when it is.
        dialog.set_focus(Some(&search));
        dialog.present(Some(&self.window));
    }

    /// Move along the resolution strip.
    fn step_timeframe(self: &Rc<Self>, delta: i32) {
        let listed = self.timeframes.borrow().clone();
        if listed.is_empty() {
            return;
        }
        let current = self.focused_pane().timeframe.get();
        // A resolution typed but not on the strip has no neighbours; start
        // from the nearest thing that is.
        let at = listed
            .iter()
            .position(|t| *t == current)
            .unwrap_or_else(|| {
                listed
                    .iter()
                    .position(|t| t.seconds() >= current.seconds())
                    .unwrap_or(listed.len() - 1)
            }) as i32;
        let next = (at + delta).clamp(0, listed.len() as i32 - 1) as usize;
        self.apply_timeframe(listed[next]);
    }

    /// Move through the watchlist without going to it.
    ///
    /// On a Pinned chart this is a rail click, not a change to the chart: the
    /// walk starts from the rail's highlight rather than from the chart's
    /// symbol, the Follow charts take each step, and with none of them the
    /// pin lights and nothing moves.
    fn step_symbol(self: &Rc<Self>, delta: i32) {
        let Some(watchlist) = self.watchlist.borrow().as_ref().cloned() else { return };
        let pinned = self.focused_pane().mode() == LinkMode::Pinned;
        let current = match pinned {
            true => None,
            false => self.focused_pane().instrument.borrow().clone(),
        };
        let (order, from) = watchlist.step_order(current.as_ref());
        if order.is_empty() {
            return;
        }
        // Nothing to step from, not even a highlight: start at whichever end
        // the step points away from.
        let at = from
            .map(|at| at as i32)
            .unwrap_or(if delta > 0 { -1 } else { order.len() as i32 });
        let next = (at + delta).clamp(0, order.len() as i32 - 1) as usize;
        let landed = &order[next];
        if pinned {
            if self.show_from_rail(landed.clone()) {
                watchlist.highlight_walked(next);
            }
            return;
        }
        let showing = current
            .as_ref()
            .is_some_and(|c| c.symbol == landed.symbol && c.suffix == landed.suffix);
        if !showing {
            self.show(landed.clone());
        }
        watchlist.highlight_walked(next);
    }

    /// Switch to a resolution that may not be on the strip yet.
    fn apply_timeframe(self: &Rc<Self>, timeframe: Timeframe) {
        // A resolution you typed belongs on the strip: you asked for it once,
        // you will ask for it again.
        self.remember_timeframe(timeframe);
        self.set_timeframe(timeframe);
    }

    /// The resolution belongs to the focused chart. Splitting a chart to put
    /// the same symbol on two resolutions is most of why you would split one.
    fn set_timeframe(self: &Rc<Self>, timeframe: Timeframe) {
        let pane = self.focused_pane();
        pane.timeframe.set(timeframe);
        // Every way of changing the resolution ends here, so the strip
        // follows whichever one it was.
        pane.sync_strip();
        self.store.set_setting(LAST_TIMEFRAME, &timeframe.key());
        // A promoted reset period changes with the resolution, so the legend
        // has to be rewritten when the resolution does.
        self.rebuild_indicator_legend();
        let instrument = pane.instrument.borrow().clone();
        if let Some(instrument) = instrument {
            self.show_in(&pane, instrument);
        }
        self.save_workspace();
    }

    /// Chart an instrument on the focused chart, and on every Follow chart
    /// when it is one.
    ///
    /// The Follow charts show one symbol: the watchlist drives them, and a
    /// symbol picked on any of them moves the rest. A Focus or Pinned chart
    /// moves alone — leaving one showing something while you go looking
    /// elsewhere is most of the reason to have split it.
    pub fn show(self: &Rc<Self>, instrument: Instrument) {
        let focused = self.focused_pane();
        self.spread(instrument, focused.mode(), Some(focused.id));
    }

    /// Chart what the rail just picked.
    ///
    /// The Follow charts take it, and so does the focused chart if it is a
    /// Focus chart — which is what makes "split, make the new chart Focus,
    /// then click" put the symbol on that chart and leave the old one alone.
    /// A Pinned chart takes nothing, focused or not.
    ///
    /// A click that moves nothing at all — a Pinned chart in front of you and
    /// no Follow charts — lights the pin rather than doing nothing silently,
    /// because a rail whose click did nothing would read as broken rather
    /// than as pinned. The rail's highlight goes back to what that chart is
    /// showing, so the list does not claim a symbol no chart shows, and the
    /// answer is `false`, so a command that clicked for somebody can say so.
    pub fn show_from_rail(self: &Rc<Self>, instrument: Instrument) -> bool {
        let focused = self.focused_pane();
        let seed = (focused.mode() == LinkMode::Focus).then_some(focused.id);
        let following = self.panes.borrow().iter().any(|p| p.mode() == LinkMode::Follow);
        if seed.is_none() && !following {
            focused.flag_pinned();
            let showing = focused.instrument.borrow().clone();
            let rail = self.watchlist.borrow().as_ref().cloned();
            if let Some(rail) = rail {
                rail.put_back(showing.as_ref());
            }
            return false;
        }
        self.spread(instrument, LinkMode::Follow, seed);
        true
    }

    /// Put `instrument` on `seed`, and on every Follow chart when `mode` is
    /// Follow.
    ///
    /// The open chartbook only. A chartbook you come back to is as you left
    /// it: each shows its own watchlist, so following the watchlist means
    /// this book's list, not a list in a book that is away.
    fn spread(self: &Rc<Self>, instrument: Instrument, mode: LinkMode, seed: Option<u32>) {
        if let Some(pane) = seed.and_then(|id| self.pane(id)) {
            self.show_in(&pane, instrument.clone());
        }
        self.spread_to_followers(&instrument, mode, seed);
        self.save_workspace();
    }

    /// Put `instrument` on every Follow chart but `except`, and point the
    /// rail at it, when `mode` is Follow.
    ///
    /// `except` is the one already showing it — the chart the symbol came from.
    /// Skipped rather than painted again, so a chart leading the Follow charts
    /// does not refetch what it is already displaying.
    ///
    /// Nothing happens for Focus or Pinned. Neither is a pool to agree with,
    /// which is what makes leaving Follow a change that moves no other chart.
    fn spread_to_followers(
        self: &Rc<Self>,
        instrument: &Instrument,
        mode: LinkMode,
        except: Option<u32>,
    ) {
        if mode != LinkMode::Follow {
            return;
        }
        let others: Vec<Rc<ChartPane>> = self
            .panes
            .borrow()
            .iter()
            .filter(|p| Some(p.id) != except && p.mode() == LinkMode::Follow)
            .cloned()
            .collect();
        for pane in others {
            self.show_in(&pane, instrument.clone());
        }
        self.rail_follows(instrument, mode);
    }

    /// Chart an instrument on one chart: paint from cache now, fetch the gap
    /// in the background.
    fn show_in(self: &Rc<Self>, pane: &Rc<ChartPane>, instrument: Instrument) {
        let timeframe = pane.timeframe.get();
        Self::own_size(pane);
        // What it trades instead goes with what it charted, unless it
        // tracks the new one too: NQ charted where ES was must not go on
        // trading MES at NQ's prices.
        if self.drop_trading_symbol_unless(pane, &instrument) {
            self.save_soon();
        }
        // A chart moved to something its chosen feed has no name for, or
        // whose feed has gone from the order, follows the order again.
        if self.settle_feed(pane, &instrument) {
            self.save_soon();
        }
        // An order from it goes to a contract whichever feed charts it.
        for login in &self.trading.borrow().logins {
            if let Some(session) = self.desk.session(&login.id) {
                let _ = session.contract_for(&instrument);
            }
        }
        let routed = match self.route_chosen(pane, &instrument) {
            Some(Ok(routed)) => routed,
            Some(Err(refused)) => {
                self.show_refused(pane, instrument, refused);
                return;
            }
            None => {
                match self.missing_from(pane, &instrument) {
                    Some(feed) => self.show_missing(pane, instrument, &feed),
                    None => self.show_unserved(pane, instrument),
                }
                return;
            }
        };
        let key = routed.key();
        pane.served.replace(Some(key.clone()));
        let Routed { feed, symbol, passed } = routed;
        let native = timeframe.native();
        // Whatever this chart streamed from another feed, it is this one's now.
        self.release_pane_elsewhere(pane.id, &feed);
        // Where its volume at price would come from: only a feed with
        // ladders of this contract, and nothing is asked until one of its
        // indicators wants them.
        let source = feed
            .at_price
            .as_ref()
            .filter(|at_price| at_price.serves(&symbol, &instrument))
            .map(|at_price| LadderSource { at_price: at_price.clone(), key: key.clone(), symbol: symbol.clone() });
        let was = pane.ladders.replace(source);
        if let Some(was) = was
            && pane.ladders.borrow().as_ref().is_none_or(|now| now.key != was.key)
        {
            was.at_price.unwatch(pane.id);
        }

        *pane.instrument.borrow_mut() = Some(instrument.clone());
        // After the instrument, which is what says whether another feed
        // could be chosen. Connecting from here until bars come, or do not.
        self.write_feed_of(pane, Some(&feed), &passed);
        pane.set_feed_phase(pane::FeedPhase::Connecting);
        self.apply_marks_to(pane);
        // Whatever this chart was waiting for, it is not waiting any more. A
        // chart somebody has since pointed somewhere else must not be moved
        // back when the catalogue lands.
        *pane.pending.borrow_mut() = None;
        pane.write_readout();
        self.write_header_of(pane);
        if pane.id == self.focused.get() {
            self.store.set_setting(LAST_SYMBOL, &instrument.symbol);
            self.store
                .set_setting(LAST_SUFFIX, instrument.suffix.as_deref().unwrap_or(""));
        }

        // The live series first, if this is being streamed already: it is
        // newer than anything the memo or the cache holds. Then the memo,
        // then the cache, so the chart is on screen before any request
        // leaves.
        let series = Series { key: key.clone(), native };
        let streamed = feed.live.as_ref().and_then(|live| live.with_bars(&series, |bars| bars.to_vec()));
        let memo = match streamed {
            Some(_) => None,
            None => self.series.borrow().get(&(key.clone(), timeframe)).cloned(),
        };
        let cached_was_empty = match (streamed, memo) {
            (Some(bars), _) => {
                let empty = bars.is_empty();
                self.paint(pane, &key, &instrument, timeframe, bars);
                empty
            }
            (None, Some(bars)) => {
                let empty = bars.is_empty();
                // Indicators have to be recomputed here too. Skipping it left
                // the previous symbol's VWAP on screen until the network reply
                // arrived and forced a repaint — which looked like a slow
                // indicator and was a missing call.
                self.recompute_indicators(pane, &instrument, timeframe, &bars);
                pane.view.set_series(instrument.clone(), timeframe, (*bars).clone());
                self.apply_marks_to(pane);
                empty
            }
            (None, None) => {
                let cached = self.store.load_bars(&key, native);
                let empty = cached.is_empty();
                self.paint(pane, &key, &instrument, timeframe, cached);
                empty
            }
        };
        pane.view.set_trouble(None);
        pane.view.set_loading(cached_was_empty);
        pane.view.set_configurations(self.store.drawing_configurations());
        let shared = self.store.drawings(&instrument.symbol, instrument.suffix.as_deref());
        pane.view.set_drawings(self.drawings_for(pane, &shared));

        // Only the focused chart drives the rail and the prefetch window: the
        // others are not where the next keystroke is going.
        if pane.id == self.focused.get() {
            if let Some(watchlist) = self.watchlist.borrow().as_ref() {
                watchlist.highlight(&instrument);
            }
            // The chart on screen overtakes everything queued behind it, and
            // the old neighbourhood is forgotten: those symbols are no longer
            // the ones a keypress away.
            for feed in self.feeds.borrow().iter() {
                feed.loader.drop_prefetches();
            }
            // Fill in the rest of the strip behind this chart, so the first
            // click on 15m is a repaint rather than a wait. Nothing is asked
            // for until this very load lands, which is what keeps a walk down
            // the rail free.
            feed.loader.warm_strip(&key, &symbol, &self.timeframes.borrow(), timeframe, |tf| {
                self.store.coverage(&key, tf).is_some()
            });
        }
        let load = omacharts_engine::progress::name(&symbol, native);
        match &feed.live {
            // The subscription is the fetch: its snapshot is the history, and
            // nothing on a streamed path asks for bars it is about to be
            // pushed. A chart already streaming this series joins it.
            Some(live) => {
                let since = self.store.coverage(&key, native).map(|coverage| coverage.last_ts);
                match live.watch(pane.id, series, &symbol, since) {
                    Watching::Showing => pane.set_feed_phase(pane::FeedPhase::Connected),
                    Watching::Opening => {}
                    Watching::Refused(failure) => {
                        pane.view.set_loading(false);
                        pane.view.set_trouble(Some(failure));
                        pane.set_feed_phase(pane::FeedPhase::Lost(failure));
                        self.say(crate::log_words::fetch_failed(feed.provider.label(), &symbol, native, failure));
                    }
                }
            }
            None => feed
                .loader
                .fetch(Request { key, symbol, timeframe, speculative: false }, FOREGROUND),
        }
        pane.follow_load(load);
        if pane.id == self.focused.get() {
            self.prefetch_neighbours(&instrument, timeframe);
        }
    }

    /// Chart an instrument the current feed has no name for.
    ///
    /// Only some feeds serve everything. Yahoo charts every instrument in the
    /// index; thinkorswim charts US listings and leaves a Taipei or Madrid
    /// line unmapped, and a watchlist holds whatever somebody put in it.
    ///
    /// So this is a real outcome rather than an impossible one, and it used
    /// to return early — before `pane.instrument` was set, which meant
    /// clicking such a symbol did nothing whatsoever: no bars, no message,
    /// the chart you were already looking at still on screen, and a rail
    /// selection pointing at something the chart was not showing. Nothing
    /// said no, so it read as a click that had been dropped.
    ///
    /// Now the chart moves to the symbol like any other and says what is
    /// true: this feed cannot chart it. Nothing is fetched, because there is
    /// nothing to ask for.
    fn show_unserved(self: &Rc<Self>, pane: &Rc<ChartPane>, instrument: Instrument) {
        let timeframe = pane.timeframe.get();

        *pane.instrument.borrow_mut() = Some(instrument.clone());
        self.apply_marks_to(pane);
        *pane.pending.borrow_mut() = None;
        pane.write_readout();
        self.write_feed_of(pane, None, &[]);
        pane.served.replace(None);
        // Whatever this chart was streaming, it is not showing it now.
        self.release_pane(pane.id);
        if pane.id == self.focused.get() {
            self.store.set_setting(LAST_SYMBOL, &instrument.symbol);
            self.store
                .set_setting(LAST_SUFFIX, instrument.suffix.as_deref().unwrap_or(""));
            if let Some(watchlist) = self.watchlist.borrow().as_ref() {
                watchlist.highlight(&instrument);
            }
        }

        // No bars, and none coming: the previous symbol's must go, or the
        // chart would be drawing one instrument's prices under another's
        // name.
        pane.view.set_series(instrument, timeframe, Vec::new());
        pane.view.set_loading(false);
        pane.view.set_trouble(Some(FetchFailure::Unserved));
        pane.set_feed_phase(pane::FeedPhase::Lost(FetchFailure::Unserved));
    }

    /// Chart an instrument on the feed somebody chose for it, which could
    /// chart it and has nothing of it: Sierra Chart chosen for MES, with no
    /// files of MES. The choice stays, and the chart says plainly why it is
    /// empty, rather than moving to another feed nobody chose.
    fn show_missing(self: &Rc<Self>, pane: &Rc<ChartPane>, instrument: Instrument, feed: &Rc<Feed>) {
        let timeframe = pane.timeframe.get();
        *pane.instrument.borrow_mut() = Some(instrument.clone());
        self.apply_marks_to(pane);
        *pane.pending.borrow_mut() = None;
        pane.write_readout();
        self.write_header_of(pane);
        self.write_account_of(pane);
        let part = self.feed_part_of(pane, |part| {
            part.label = Some(feed.provider.label().to_string());
            part.refused = Some(format!("no {} in its data", instrument.symbol));
        });
        pane.write_feed(part);
        pane.served.replace(None);
        self.release_pane(pane.id);
        if pane.id == self.focused.get() {
            self.store.set_setting(LAST_SYMBOL, &instrument.symbol);
            self.store
                .set_setting(LAST_SUFFIX, instrument.suffix.as_deref().unwrap_or(""));
            if let Some(watchlist) = self.watchlist.borrow().as_ref() {
                watchlist.highlight(&instrument);
            }
        }
        let words = format!("{} isn't in {}'s data", instrument.symbol, feed.provider.label());
        pane.view.set_series(instrument, timeframe, Vec::new());
        pane.view.set_loading(false);
        pane.view.set_trouble_saying(FetchFailure::Unserved, words);
        pane.set_feed_phase(pane::FeedPhase::Lost(FetchFailure::Unserved));
    }

    /// Chart an instrument on the feed somebody chose for it, which cannot
    /// be used: not signed in, or its session expired.
    ///
    /// The chart is not moved to the next feed down, which is what the order
    /// does with a feed it passes over. It was pointed at this one, so it
    /// shows what this feed's cache already holds, asks for nothing, and says
    /// why in its header and over its bars.
    fn show_refused(self: &Rc<Self>, pane: &Rc<ChartPane>, instrument: Instrument, refused: Routed) {
        let timeframe = pane.timeframe.get();
        let key = refused.key();
        *pane.instrument.borrow_mut() = Some(instrument.clone());
        self.apply_marks_to(pane);
        *pane.pending.borrow_mut() = None;
        pane.write_readout();
        self.write_header_of(pane);
        self.write_refused_feed(pane, &refused.feed);
        pane.served.replace(None);
        self.release_pane(pane.id);
        if pane.id == self.focused.get() {
            self.store.set_setting(LAST_SYMBOL, &instrument.symbol);
            self.store
                .set_setting(LAST_SUFFIX, instrument.suffix.as_deref().unwrap_or(""));
            if let Some(watchlist) = self.watchlist.borrow().as_ref() {
                watchlist.highlight(&instrument);
            }
        }
        let cached = self.store.load_bars(&key, timeframe.native());
        self.paint(pane, &key, &instrument, timeframe, cached);
        pane.view.set_loading(false);
        pane.view.set_trouble(Some(FetchFailure::NeedsSignIn));
        pane.set_feed_phase(pane::FeedPhase::Lost(FetchFailure::NeedsSignIn));
        pane.view.set_configurations(self.store.drawing_configurations());
        let shared = self.store.drawings(&instrument.symbol, instrument.suffix.as_deref());
        pane.view.set_drawings(self.drawings_for(pane, &shared));
    }

    /// Fetch ahead around the selection, nearest first.
    ///
    /// This is what makes arrowing through the rail feel instant: by the time
    /// you press the key the next one is already cached, and moving shifts the
    /// window so the symbols now a keypress away are the ones being fetched.
    /// Everything here is speculative, so the loader paces it apart and
    /// drops it entirely if Yahoo starts refusing.
    fn prefetch_neighbours(self: &Rc<Self>, current: &Instrument, timeframe: Timeframe) {
        let Some(watchlist) = self.watchlist.borrow().as_ref().cloned() else {
            return;
        };
        let order = watchlist.flat_order();
        let Some(position) = order
            .iter()
            .position(|i| i.symbol == current.symbol && i.suffix == current.suffix)
        else {
            return;
        };

        let first = position.saturating_sub(PREFETCH_WINDOW);
        let last = (position + PREFETCH_WINDOW).min(order.len().saturating_sub(1));

        // `index` is a position in the rail, not just a subscript: it is what
        // says which neighbour to skip and how far away the rest are, so
        // enumerate() would hand back the same number by a longer route.
        #[allow(clippy::needless_range_loop)]
        for index in first..=last {
            if index == position {
                continue;
            }
            let instrument = &order[index];
            let Some(routed) = self.route(instrument) else { continue };
            let key = routed.key();
            let Routed { feed, symbol, .. } = routed;
            let distance = index.abs_diff(position) as u32;

            if self.store.coverage(&key, timeframe.native()).is_none() {
                feed.loader.fetch(
                    Request {
                        key: key.clone(),
                        symbol: symbol.clone(),
                        timeframe,
                        speculative: true,
                    },
                    distance,
                );
            }
            // The rail's change column reads daily bars, so a neighbour needs
            // them even if you never open it — but after its chart data.
            let daily = Timeframe::days(1);
            if timeframe.native() != daily && self.store.coverage(&key, daily).is_none() {
                feed.loader.fetch(
                    Request { key, symbol, timeframe: daily, speculative: true },
                    distance + BACKGROUND,
                );
            }
        }
    }

    /// Queue a paced daily fetch for instruments the rail has no price for,
    /// in the order given, and return at once.
    ///
    /// The one place anything outside the chart asks for data, and the reason
    /// it exists is a rule worth stating plainly: **the GTK main loop never
    /// waits on the network.** A command forwarded from a terminal or from the
    /// bar widget runs *inside this process, on this thread* — so a command
    /// that fetched would freeze the window for as long as the provider's
    /// pacing took, which was six seconds every two minutes. Queueing instead
    /// costs a mutex, and the loader already holds one throttle across every
    /// request it has ever made, so a 429 is remembered rather than rediscovered
    /// by each caller.
    ///
    /// Queues what it is given, in the order given — deciding what is worth
    /// asking for belongs to whoever knows why they are asking. The loader
    /// drops a duplicate of something already queued.
    pub fn warm(self: &Rc<Self>, instruments: &[Instrument]) {
        let daily = Timeframe::days(1);
        for (rank, instrument) in instruments.iter().enumerate() {
            let Some(routed) = self.route(instrument) else { continue };
            let key = routed.key();
            let Routed { feed, symbol, .. } = routed;
            let delivery = feed.provider.delivery();
            if !worth_warming(delivery, self.store.coverage(&key, daily).is_some()) {
                continue;
            }
            feed.loader.fetch(
                Request { key, symbol, timeframe: daily, speculative: true },
                BACKFILL.saturating_add(rank as u32),
            );
        }
    }

    /// Queue batches of closes for the bar widget, on this window's loaders
    /// so that they share their pacing and their cooldowns with everything
    /// else. A batch is one feed's, and its keys say which.
    pub fn warm_closes(self: &Rc<Self>, batches: Vec<Vec<crate::loader::Listing>>) {
        for listings in batches {
            let Some(feed) = listings.first().and_then(|listing| self.feed_of(&listing.key)) else {
                continue;
            };
            feed.loader.fetch_closes(listings, BACKFILL);
        }
    }

    /// Keep asking, on a slow timer, whether the rail has rows with no price.
    ///
    /// A timer rather than a hook on every way a watchlist can change: symbols
    /// arrive from a terminal as well as from the window, sections are expanded
    /// and collapsed, and the question — "is anything on screen showing a dash"
    /// — is the same one however it came to be true. It is also what lets the
    /// backfill offer itself again after the provider has refused it.
    fn wire_backfill(self: &Rc<Self>) {
        // Behind the first frame, with the rest of the housekeeping: a dash
        // filling in a moment later is not worth delaying the window for.
        let first = self.clone();
        glib::idle_add_local_once(move || first.backfill());
        let this = self.clone();
        glib::timeout_add_seconds_local(BACKFILL_POLL_SECONDS, move || {
            this.backfill();
            glib::ControlFlow::Continue
        });
    }

    /// Ask for what the rail's visible rows are missing: daily bars for the
    /// rows it cannot price yet, and fresh closes for the rows that have gone
    /// stale.
    ///
    /// The rail paints from the cache and never fetches — that is its contract
    /// and this does not change it. What changes is that a symbol nobody has
    /// opened no longer waits to be opened, neither for its first price nor
    /// for a current one: the window, which already fetches on the rail's
    /// behalf around the selection, covers the rest of the list too. Until it
    /// did, a row's price was whatever it was when it was last charted, and a
    /// list nobody clicked through showed Monday's prices on Thursday.
    ///
    /// Four things keep it from costing anybody their rate limit. Every
    /// request is speculative, so the loader holds them [two seconds
    /// apart](crate::loader) and refuses them outright while the provider is
    /// throttling — the same pacing that already fills the neighbourhood,
    /// not a second mechanism. A row with nothing cached is asked for once,
    /// so that part drains and goes quiet. A stale row is asked for in a
    /// batch with the others that are due, twenty to a request on Yahoo: a
    /// quarter of an hour apart while its market trades, the rule a chart on
    /// screen keeps to, and once a day while it is shut, see
    /// [`crate::cli::plan_refresh`]. And only
    /// visible rows, in a window on screen: a collapsed section's symbols are
    /// not showing a price to anybody, and get picked up when it opens.
    fn backfill(self: &Rc<Self>) {
        let feeds = self.feeds();
        // A feed that has just refused is offered nothing until its pause
        // runs out; the others carry on.
        let quiet: Vec<bool> = feeds.iter().map(|feed| Feed::quiet(&feed.backfill_quiet_until)).collect();
        if quiet.iter().all(|quiet| *quiet) {
            return;
        }
        let asking = |routed: &Routed| {
            feeds.iter().zip(&quiet).any(|(feed, quiet)| !quiet && Rc::ptr_eq(feed, &routed.feed))
        };
        let Some(rail) = self.watchlist.borrow().as_ref().cloned() else { return };
        let daily = Timeframe::days(1);
        let missing: Vec<Instrument> = rail
            .flat_order()
            .into_iter()
            .filter(|instrument| {
                self.route(instrument)
                    .map(|routed| asking(&routed) && self.store.coverage(&routed.key(), daily).is_none())
                    .unwrap_or(false)
            })
            .collect();
        // Nothing missing is the steady state, and it has to be silent.
        if !missing.is_empty() {
            self.warm(&missing);
        }
        if self.on_screen() {
            for (feed, quiet) in feeds.iter().zip(&quiet) {
                if !quiet {
                    self.refresh_rail(feed, &rail.flat_order());
                }
            }
        }
    }

    /// Bring the stale rows among `rows` that `feed` serves up to date, most
    /// of them in batches.
    ///
    /// The same plan the bar widget makes, so the two never disagree about
    /// when a price is old and share every batch either of them fetches: a
    /// row in both lists is asked for once. A row with nothing cached is the
    /// backfill's above, and asking for it twice queues it once. A batch is
    /// one feed's request, so the plan is made a feed at a time.
    fn refresh_rail(self: &Rc<Self>, feed: &Rc<Feed>, rows: &[Instrument]) {
        let alone = self.feeds.borrow().len() == 1;
        let served: std::borrow::Cow<[Instrument]> = match alone {
            // Everything routes to the one feed there is, or to none.
            true => std::borrow::Cow::Borrowed(rows),
            false => rows
                .iter()
                .filter(|row| self.route(row).is_some_and(|routed| Rc::ptr_eq(&routed.feed, feed)))
                .cloned()
                .collect(),
        };
        let now = chrono::Utc::now().timestamp();
        let plan = crate::cli::plan_refresh(
            &self.store,
            feed.provider.as_ref(),
            &served,
            now,
            RAIL_MAX_BATCHES,
        );
        if !plan.bars.is_empty() {
            self.warm(&plan.bars);
        }
        self.warm_closes(plan.closes);
    }

    /// Stop offering `feed` backfill work for a while, because it just
    /// refused some.
    ///
    /// Deliberately longer than the provider's own first cooldown, so the
    /// window is never the thing that walks into it. The provider climbs its
    /// own from a minute to fifteen; this only has to stay out of the way.
    fn pause_backfill(self: &Rc<Self>, feed: &Feed) {
        feed.loader.drop_backfill();
        feed.backfill_quiet_until
            .set(Some(std::time::Instant::now() + std::time::Duration::from_secs(300)));
    }

    /// Keep the charts on screen from quietly going stale.
    ///
    /// Whether a chart needs this at all is the provider's to say, through
    /// [`Provider::delivery`]: one that streams has nothing to be asked for,
    /// and one that does not — Yahoo, which has no stream and nothing cheaper
    /// to poll than the series itself — has to be asked again or the chart
    /// stops being true with nothing on screen to say so. Refreshing a chart
    /// means refetching its tail, which the loader already does in one small
    /// request from a few bars before where the cache ends.
    ///
    /// There is no setting beside the provider's answer, and deliberately
    /// not: the off position was a chart that looked current and was not. An
    /// `auto_refresh` row left in an older settings table is simply ignored.
    ///
    /// Which leaves the only question worth engineering: how rarely this can
    /// get away with asking. That answer belongs to
    /// [`omacharts_engine::refresh`], one chart at a time, and most ticks of
    /// this timer queue nothing at all.
    fn wire_refresh(self: &Rc<Self>) {
        if !self.any_polled() || self.refresh_timer_on.get() {
            return;
        }
        self.refresh_timer_on.set(true);
        // No first pass on idle, unlike the backfill: every chart on screen
        // has just been fetched by the thing that put it there, so there is
        // nothing a tick at startup could usefully do.
        let this = self.clone();
        glib::timeout_add_seconds_local(REFRESH_TICK_SECONDS, move || {
            // A switch to feeds that all stream: the timer stands down, and
            // a later switch back installs it again.
            if !this.any_polled() {
                this.refresh_timer_on.set(false);
                return glib::ControlFlow::Break;
            }
            this.refresh_charts();
            glib::ControlFlow::Continue
        });
    }

    /// Is any feed one that has to be asked again to stay current?
    fn any_polled(&self) -> bool {
        self.feeds.borrow().iter().any(|feed| wants_refresh_timer(feed.provider.delivery()))
    }

    /// Offer a tail fetch for each chart on screen that has gone stale.
    ///
    /// Deliberately asks [`refresh::due`] about every chart rather than
    /// second-guessing any of it here, so that there is exactly one place
    /// where the decision lives and exactly one place to read to find out
    /// what this will and will not do.
    fn refresh_charts(self: &Rc<Self>) {
        let feeds = self.feeds();
        let quiet: Vec<bool> = feeds.iter().map(|feed| Feed::quiet(&feed.refresh_quiet_until)).collect();
        if quiet.iter().all(|quiet| *quiet) {
            return;
        }

        let visible = self.on_screen();
        let now = chrono::Utc::now().timestamp();

        // Only the charts actually mounted. `panes` keeps the ones hidden
        // behind a maximized chart so that they are right when they come
        // back, and until they do, nobody is looking at them.
        for id in self.mounted().leaves() {
            let Some(pane) = self.pane(id) else { continue };
            let instrument = pane.instrument.borrow().clone();
            let Some(instrument) = instrument else { continue };
            let Some(routed) = self.route_pane(&pane, &instrument) else { continue };
            // A feed that has just refused is left alone until its pause
            // runs out, and its charts with it.
            if feeds.iter().zip(&quiet).any(|(feed, quiet)| *quiet && Rc::ptr_eq(feed, &routed.feed)) {
                continue;
            }
            let key = routed.key();
            let Routed { feed, symbol, .. } = routed;
            let timeframe = pane.timeframe.get();
            let candidate = refresh::Candidate {
                delivery: feed.provider.delivery(),
                visible,
                at_latest: pane.view.at_latest(),
                timeframe,
                instrument: &instrument,
                fetched_at: self
                    .store
                    .coverage(&key, timeframe.native())
                    .map(|coverage| coverage.fetched_at),
            };
            if refresh::due(&candidate, now).is_err() {
                continue;
            }
            // Speculative, which is the whole of how this stays polite: the
            // loader holds it two seconds behind anything somebody is
            // waiting for and refuses it outright while the provider is
            // throttling. A chart keeping itself current must never be the
            // reason another one loads slowly.
            feed.loader.fetch(Request { key, symbol, timeframe, speculative: true }, REFRESH);
        }
    }

    /// Is the window on screen at all?
    ///
    /// Mapped, which is GTK's own answer and catches a window that has been
    /// closed to nothing or never shown. Deliberately **not** focus: a chart
    /// is a thing you glance at while working in another window, and one that
    /// stopped keeping itself current the moment it lost the keyboard would be
    /// stale at precisely the instant somebody looked at it.
    ///
    /// This is the weakest of the refusals, and honestly so. The signal that
    /// would really answer the question is `GtkWindow:suspended` — the
    /// compositor saying the surface is not being displayed, which is what a
    /// window on another workspace is — and it needs GTK 4.12, while this
    /// builds against 4.10. Turning that on surfaces deprecations across
    /// unrelated code, so it is left for whoever raises the floor; the day
    /// they do, this is the one line that changes. Until then a window on
    /// another workspace counts as on screen, which is no worse than the
    /// behaviour this replaced, and the refusals that do the real work here
    /// are the market being shut and the period not having elapsed.
    fn on_screen(&self) -> bool {
        self.window.is_mapped()
    }

    /// Stop refreshing charts for a while, because a refresh was just refused.
    ///
    /// The refusal stops this rather than being retried, which is the rule
    /// that matters: a failed fetch writes no bars, so the chart stays due,
    /// and without a pause the timer would offer the same request every
    /// thirty seconds for as long as whatever went wrong lasted. Any failure
    /// counts, not only a 429 — the provider and the network are shared by
    /// every chart, so one refusal really is news about all of them.
    ///
    /// Five minutes, as for the backfill: comfortably past the provider's
    /// first cooldown, so the window is never the thing that walks into one,
    /// and invisible against a cadence whose slowest setting is a quarter of
    /// an hour.
    fn pause_refresh(self: &Rc<Self>, feed: &Feed) {
        feed.refresh_quiet_until
            .set(Some(std::time::Instant::now() + std::time::Duration::from_secs(300)));
    }

    /// Apply bars that arrived for `key`, ignoring a reply for a chart the user
    /// has already navigated away from.
    fn present(
        self: &Rc<Self>,
        key: &str,
        native: Timeframe,
        bars: Vec<omacharts_engine::Bar>,
        trouble: Option<FetchFailure>,
    ) {
        // A series being streamed is newer in memory than any reply: the
        // reply still feeds the rail, and paints nothing.
        let streamed = self
            .feed_of(key)
            .and_then(|feed| feed.live.clone())
            .is_some_and(|live| live.is_showing(&Series { key: key.to_string(), native }));
        // One reply can belong to several charts: the same symbol at the same
        // resolution in two panes is one fetch and two repaints.
        let waiting: Vec<Rc<ChartPane>> = self
            .panes
            .borrow()
            .iter()
            .filter(|pane| {
                !streamed
                    && pane.timeframe.get().native() == native
                    && pane
                        .instrument
                        .borrow()
                        .as_ref()
                        .and_then(|i| self.route_pane(pane, i))
                        .map(|routed| routed.key() == key)
                        .unwrap_or(false)
            })
            .cloned()
            .collect();

        if waiting.is_empty() {
            // Something we prefetched. Not on any chart, but the rail's change
            // column may now have numbers it did not have a moment ago.
            if let Some(watchlist) = self.watchlist.borrow().as_ref() {
                watchlist.refresh_quotes();
                self.write_changes();
                watchlist.set_trouble(trouble);
            }
            return;
        }

        for pane in waiting {
            let instrument = pane.instrument.borrow().clone();
            let Some(instrument) = instrument else { continue };
            self.paint(&pane, key, &instrument, pane.timeframe.get(), bars.clone());
            pane.view.set_trouble(trouble);
            pane.view.set_loading(false);
            pane.set_feed_phase(match trouble {
                Some(why) => pane::FeedPhase::Lost(why),
                None => pane::FeedPhase::Connected,
            });
        }
        if let Some(watchlist) = self.watchlist.borrow().as_ref() {
            watchlist.refresh_quotes();
            self.write_changes();
            watchlist.set_trouble(trouble);
        }
    }

    /// Fold to the shown resolution, memoise, and draw.
    fn paint(
        self: &Rc<Self>,
        pane: &Rc<ChartPane>,
        key: &str,
        instrument: &Instrument,
        timeframe: Timeframe,
        bars: Vec<omacharts_engine::Bar>,
    ) {
        // A market that never closes opens where it left off. Nothing here is
        // specific to a provider: the repair only fires when the series it is
        // given has no bodies, which is true of Yahoo's currency pairs and
        // not of its crypto.
        let mut bars = if instrument.kind.is_continuous() {
            omacharts_engine::repair_continuous_opens(&bars)
        } else {
            bars
        };
        // Then what nobody could have traded, left out or cut back: here,
        // on the way to the screen, and never where the bars are kept.
        omacharts_engine::drawable(&mut bars, timeframe, instrument.kind);
        let latest = hidden_latest(&bars, pane.session.get(), instrument, timeframe);
        let bars = omacharts_engine::session::filter(
            &bars,
            pane.session.get(),
            instrument,
            timeframe.is_intraday(),
        );
        let bars = if timeframe.is_derived() {
            resample(&bars, timeframe, &Grid::new(instrument, pane.session.get()))
        } else {
            bars
        };
        self.recompute_indicators(pane, instrument, timeframe, &bars);
        let bars = Rc::new(bars);
        {
            let mut series = self.series.borrow_mut();
            // A watchlist's worth of series is a couple of megabytes; past that
            // this is holding onto symbols nobody is going back to.
            if series.len() > 96 {
                series.clear();
            }
            series.insert((key.to_string(), timeframe), bars.clone());
        }
        let held = !pane.view.held_strip_windows().is_empty();
        pane.view.set_series(instrument.clone(), timeframe, (*bars).clone());
        // Another symbol or timeframe lets go of the strips held on their own
        // scale: written down so, rather than brought back after a restart.
        if held && pane.view.held_strip_windows().is_empty() {
            self.save_workspace();
        }
        pane.view.set_latest(latest);
        // Bars from a feed are when it has found the contract they are
        // from, which is what an order on the chart goes to: the marks can
        // be drawn from here, without waiting for the book to move.
        self.apply_marks_to(pane);
        self.chart_moved(pane);
    }

    /// Run the indicators over the bars now on screen and hand them to the
    /// chart, coloured.
    ///
    /// Colour comes from the theme's palette by slot, so a set of indicators
    /// is distinguishable without anyone choosing anything — and follows the
    /// theme when it changes.
    fn recompute_indicators(
        self: &Rc<Self>,
        pane: &Rc<ChartPane>,
        instrument: &Instrument,
        timeframe: Timeframe,
        bars: &[omacharts_engine::Bar],
    ) {
        // The clock goes with them: every way a chart is handed a series
        // comes through here, the memo included, and a chart painted from
        // the memo was left reading the default clock.
        pane.view.set_clock(self.clock_for(instrument));
        let drawn = self.drawn_indicators(pane, instrument, timeframe, bars);
        pane.view.set_indicators(drawn);
    }

    /// This chart's indicators computed over `bars`, ready to draw.
    ///
    /// Apart from putting them on the chart, because the live tail computes
    /// them over the series the chart already holds, borrowed from it — and
    /// a chart cannot be handed its indicators while it is lending its bars.
    fn drawn_indicators(
        self: &Rc<Self>,
        pane: &Rc<ChartPane>,
        instrument: &Instrument,
        timeframe: Timeframe,
        bars: &[omacharts_engine::Bar],
    ) -> Vec<Drawn> {
        let theme = self.theming.borrow().theme();
        let all = pane.indicators.borrow().clone();
        let colors = omacharts_engine::palette_colors(&all, &theme);
        let context = omacharts_engine::indicators::Context::of(
            instrument,
            timeframe,
            self.clock_for(instrument).zone,
        )
        .with_memos(Some(&pane.cvd_memos));
        let ids: Vec<u32> = all.iter().map(|indicator| indicator.id).collect();
        pane.cvd_memos.keep(&ids);
        let drawn = self.compute_drawn(pane, instrument, timeframe, bars, &all, colors, &context);
        // The legend marks a cumulative delta that is estimated anywhere,
        // and is built again only when which ones are changes — after this
        // computation, since the chart may be lending its bars to it.
        let estimated: Vec<u32> =
            drawn.iter().filter(|d| d.output.is_estimated()).map(|d| d.indicator.id).collect();
        if *pane.estimated.borrow() != estimated {
            *pane.estimated.borrow_mut() = estimated;
            let (this, pane) = (Rc::downgrade(self), Rc::downgrade(pane));
            glib::idle_add_local_once(move || {
                if let (Some(this), Some(pane)) = (this.upgrade(), pane.upgrade()) {
                    this.rebuild_legend_of(&pane);
                }
            });
        }
        drawn
    }

    /// [`Self::drawn_indicators`]'s computation, on the context it made.
    #[allow(clippy::too_many_arguments)]
    fn compute_drawn(
        &self,
        pane: &Rc<ChartPane>,
        instrument: &Instrument,
        timeframe: Timeframe,
        bars: &[omacharts_engine::Bar],
        all: &[Indicator],
        colors: Vec<String>,
        context: &omacharts_engine::indicators::Context,
    ) -> Vec<Drawn> {
        let context = *context;
        let merges = pane.profile_merges.borrow();
        let compute = |context: &omacharts_engine::indicators::Context| -> Vec<Drawn> {
            all.iter()
                .zip(colors.iter().cloned())
                .map(|(indicator, color)| {
                    let mut indicator = indicator.clone();
                    // The chart's own composites, put into the profile only to
                    // be drawn: the indicator itself may be a template's.
                    if let omacharts_engine::Params::VolumeProfile { merged, reset, .. } = &mut indicator.params {
                        let reset = reset.effective_for(timeframe);
                        if let Some(merge) = merges.iter().find(|m| m.is(indicator.id, instrument, reset)) {
                            *merged = merge.starts.clone();
                        }
                    }
                    Drawn {
                        color,
                        output: omacharts_engine::indicators::compute_in(&indicator, bars, context),
                        indicator,
                    }
                })
                .collect()
        };
        // Volume at price, asked once per computation and never per bar: a
        // chart whose feed has no ladders has no source, and goes straight
        // to the bars.
        let source = pane.ladders.borrow().clone();
        let Some(source) = source else { return compute(&context) };
        if !all.iter().any(|indicator| indicator.kind.uses_volume_at_price()) {
            source.at_price.unwatch(pane.id);
            return compute(&context);
        }
        if let Some(first) = bars.first() {
            source.at_price.want(pane.id, &source.key, &source.symbol, instrument, first.ts);
        }
        let drawn = source.at_price.with_held(&source.key, |held| compute(&context.with_ladders(held)));
        // Days a period cut and whose minutes are not in memory yet: read,
        // and the chart drawn again when they are.
        source.at_price.load_wanted(&source.key);
        drawn
    }

    /// The clock a chart of this instrument reads, as the settings say.
    pub fn clock_for(&self, instrument: &Instrument) -> crate::ui::chart::Clock {
        let (zone, format) = match self.clock_settings.get() {
            Some(settings) => settings,
            None => {
                let read = (crate::clock::zone(&self.store), crate::clock::format(&self.store));
                self.clock_settings.set(Some(read));
                read
            }
        };
        crate::ui::chart::Clock {
            zone: zone.for_instrument(Some(instrument)),
            format,
            named: zone == omacharts_engine::clock::ChartZone::Exchange,
        }
    }

    /// Every chart on screen in the time zone and the format the settings now
    /// name: the axis relabelled, and the indicators of time worked out again,
    /// since a window typed in the chart's clock moves when the clock does.
    pub fn adopt_clock(self: &Rc<Self>) {
        self.clock_settings.set(None);
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            let instrument = pane.instrument.borrow().clone();
            let Some(instrument) = instrument else { continue };
            pane.view.set_clock(self.clock_for(&instrument));
            let timeframe = pane.timeframe.get();
            let drawn =
                pane.view.with_bars(|bars| self.drawn_indicators(pane, &instrument, timeframe, bars));
            pane.view.set_indicators(drawn);
        }
    }

    // -- streaming ---------------------------------------------------------

    /// Hook the registry up to the main loop: wakes drain on the frame
    /// clock, the writer's answers paint and refresh the rail, and the
    /// window closing writes everything down first.
    ///
    /// Nothing here runs while nothing arrives. There is no timer; a tick
    /// sends one wake, the wake schedules one drain on the next frame, and
    /// the frame clock stops again when the drain has run.
    fn wire_live(self: &Rc<Self>) {
        for feed in self.feeds() {
            if let Some(live) = &feed.live {
                self.wire_live_of(live.clone());
            }
            if let Some(at_price) = &feed.at_price {
                self.wire_at_price(at_price.clone());
            }
        }
    }

    /// Re-route the charts when a feed's session says what its login may
    /// chart, or that changes: a root on an exchange the login is not
    /// entitled to goes to the next feed. Where every chart is served
    /// from is noted before the new set is taken on, so only the charts
    /// whose feed changes are shown again. Nothing listens until a
    /// session exists to say anything.
    fn wire_entitlements(self: &Rc<Self>) {
        let (said, heard) = async_channel::unbounded::<()>();
        providers::on_serving_changed(move || {
            let _ = said.try_send(());
        });
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while heard.recv().await.is_ok() {
                let Some(this) = this.upgrade() else { return };
                if providers::adopt_serving() {
                    this.say(crate::log_words::serving_changed());
                    this.reroute();
                }
            }
        });
    }

    /// Re-route the charts when a stitched root followed live rolls: the
    /// month its chart is drawn up to is in its symbol, so a chart of it is
    /// shown again as the month that is front now — a series, a stream and
    /// ladders of its own — and every other chart keeps what it has.
    fn wire_rolls(self: &Rc<Self>) {
        let (said, heard) = async_channel::unbounded::<()>();
        // Heard for as long as the window is open: once it has gone, so has
        // the receiving end, and the engine lets this go.
        omacharts_engine::continuous::on_rolled(move || said.try_send(()).is_ok());
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while heard.recv().await.is_ok() {
                let Some(this) = this.upgrade() else { return };
                this.reroute();
            }
        });
    }

    /// The same for a feed's volume at price: its live wakes drain on the
    /// frame clock with the streams', and what its thread loads is put in
    /// place and drawn by the charts that wanted it.
    fn wire_at_price(self: &Rc<Self>, at_price: Rc<AtPrice>) {
        let this = self.clone();
        let nudges = at_price.nudges();
        glib::spawn_future_local(async move {
            while nudges.recv().await.is_ok() {
                this.schedule_drain();
            }
        });
        let this = self.clone();
        let done = at_price.done();
        glib::spawn_future_local(async move {
            while let Ok(first) = done.recv().await {
                // Whatever else is back by now goes in before the charts
                // are worked out again, once: a scroll back asks a day of
                // pieces at a time.
                let mut panes = at_price.finished(first);
                while let Ok(more) = done.try_recv() {
                    panes.extend(at_price.finished(more));
                }
                this.recompute_panes(&panes);
            }
        });
    }

    /// Work out the indicators of the charts `ids` again, over the bars
    /// they hold.
    fn recompute_panes(self: &Rc<Self>, ids: &[u32]) {
        let mut seen: Vec<u32> = Vec::with_capacity(ids.len());
        for &id in ids {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let Some(pane) = self.pane(id) else { continue };
            let instrument = pane.instrument.borrow().clone();
            let Some(instrument) = instrument else { continue };
            let timeframe = pane.timeframe.get();
            let drawn = pane.view.with_bars(|bars| self.drawn_indicators(&pane, &instrument, timeframe, bars));
            pane.view.set_indicators(drawn);
        }
    }

    /// The same, for one feed's registry.
    fn wire_live_of(self: &Rc<Self>, live: Rc<Live>) {
        let this = self.clone();
        let nudges = live.nudges();
        glib::spawn_future_local(async move {
            while nudges.recv().await.is_ok() {
                this.schedule_drain();
            }
        });

        let this = self.clone();
        let written = live.written();
        let registry = live.clone();
        glib::spawn_future_local(async move {
            while let Ok(written) = written.recv().await {
                match written {
                    Written::Merged { series, bars } => {
                        if let Some(change) = registry.merged(&series, bars) {
                            this.apply_live_change(&registry, change);
                        }
                        // Ticks that waited for the merge are applied on
                        // top of it in the same breath.
                        this.drain_live();
                    }
                    Written::Flushed { .. } => {
                        if let Some(watchlist) = this.watchlist.borrow().as_ref() {
                            watchlist.refresh_quotes();
                            this.write_changes();
                        }
                    }
                }
            }
        });

    }

    /// The window closing is the one exit nothing else covers: ten seconds
    /// of ticks are in memory and nowhere else, and a feed's threads are
    /// still running. The same retirement a feed switch does, with nothing
    /// installed after it. Every trading session is signed out too, so a
    /// closed app leaves nothing signed in at the trading service, waiting no longer
    /// than the data session does.
    fn wire_close(self: &Rc<Self>) {
        let this = self.clone();
        self.window.connect_close_request(move |_| {
            this.retire_feeds();
            this.desk.sign_out_all(crate::trading::SIGN_OUT_WAIT);
            glib::Propagation::Proceed
        });
    }

    // -- the feed ----------------------------------------------------------

    /// The feed on top of the order. A clone of the handle, which is what
    /// every caller wants: nothing holds the cell across a call that might
    /// switch.
    pub fn provider(&self) -> Arc<dyn Provider> {
        match self.feeds.borrow().first() {
            Some(feed) => feed.provider.clone(),
            None => Arc::from(providers::built_or_default(providers::DEFAULT)),
        }
    }

    /// The feeds, top first. A copy of the list, so nothing holds the cell
    /// across a call that might switch.
    fn feeds(&self) -> Vec<Rc<Feed>> {
        self.feeds.borrow().clone()
    }

    /// Which feed serves `instrument`: the first in the order that can be
    /// used and has a name for it.
    ///
    /// The one place that decides, and it decides only on what is known
    /// before asking anybody for anything — whether a feed is signed in, and
    /// whether it serves the instrument. A feed that fails while running
    /// keeps its charts: they show the trouble dot they always showed, and
    /// the provider owns getting back. Somebody watching a chart must never
    /// find it moved to another feed because of a dropped connection.
    fn route(&self, instrument: &Instrument) -> Option<Routed> {
        let feeds = self.feeds.borrow();
        let route = providers::route(
            feeds.iter().map(|feed| (feed.provider.as_ref() as &dyn Provider, feed.usable.get())),
            instrument,
        )?;
        let feed = feeds.get(route.at)?.clone();
        let passed = route.passed.iter().filter_map(|at| feeds.get(*at).cloned()).collect();
        Some(Routed { feed, symbol: route.symbol, passed })
    }

    /// Which feed serves `pane`'s chart of `instrument`: the one somebody
    /// chose for it, or the order's answer when nobody did.
    ///
    /// `None` when nothing serves it, and when the chosen feed cannot be
    /// used — a chosen feed is never passed over, so nothing is fetched and
    /// [`Window::show_in`] says why. Everything that asks for a chart's bars
    /// asks this rather than [`Window::route`].
    fn route_pane(&self, pane: &ChartPane, instrument: &Instrument) -> Option<Routed> {
        self.route_chosen(pane, instrument)?.ok()
    }

    /// [`Window::route_pane`], keeping a refusal: `Err` is the chosen feed
    /// that cannot be used, with the instrument in its spelling.
    ///
    /// The feed is the one [`crate::trading::feed_of`] points the chart at
    /// — chosen for it, or its account's own — else the order's.
    ///
    /// `None` too when the chosen feed has nothing of the instrument, which
    /// [`Window::missing_from`] names.
    ///
    /// A futures root is asked for joined at the rolls as the chart says, on
    /// a feed with that choice: see [`omacharts_engine::continuous::routed`].
    fn route_chosen(&self, pane: &ChartPane, instrument: &Instrument) -> Option<Result<Routed, Routed>> {
        let adjust = |routed: Routed| {
            let symbol = omacharts_engine::continuous::routed(
                routed.feed.provider.as_ref(),
                instrument,
                &routed.symbol,
                pane.continuous.get(),
            );
            Routed { symbol, ..routed }
        };
        Some(match self.route_chosen_as_stored(pane, instrument)? {
            Ok(routed) => Ok(adjust(routed)),
            Err(refused) => Err(refused),
        })
    }

    /// [`Window::route_chosen`] in the feed's own spelling, before the
    /// chart's choice of how a root's months are joined.
    fn route_chosen_as_stored(&self, pane: &ChartPane, instrument: &Instrument) -> Option<Result<Routed, Routed>> {
        let Some(chosen) = self.chosen_route(pane, instrument) else {
            return self.route(instrument).map(Ok);
        };
        let feeds = self.feeds.borrow();
        match chosen? {
            providers::Chosen::Route(route) => {
                let feed = feeds.get(route.at)?.clone();
                let passed = route.passed.iter().filter_map(|at| feeds.get(*at).cloned()).collect();
                Some(Ok(Routed { feed, symbol: route.symbol, passed }))
            }
            providers::Chosen::Refused { at, symbol } => {
                Some(Err(Routed { feed: feeds.get(at)?.clone(), symbol, passed: Vec::new() }))
            }
            providers::Chosen::Missing { .. } => None,
        }
    }

    /// The feed chosen for `pane` that could chart `instrument` and has
    /// nothing of it: Sierra Chart chosen for MES with no files of MES.
    fn missing_from(&self, pane: &ChartPane, instrument: &Instrument) -> Option<Rc<Feed>> {
        match self.chosen_route(pane, instrument)?? {
            providers::Chosen::Missing { at } => self.feeds.borrow().get(at).cloned(),
            _ => None,
        }
    }

    /// The engine's answer for `pane`'s chart of `instrument` on the feed
    /// [`crate::trading::feed_of`] points it at; `None` when it points it
    /// at none and the order decides.
    fn chosen_route(&self, pane: &ChartPane, instrument: &Instrument) -> Option<Option<providers::Chosen>> {
        let chosen_feed = pane.feed.borrow();
        let feeds = self.feeds.borrow();
        let usable = || feeds.iter().map(|feed| (feed.provider.as_ref() as &dyn Provider, feed.usable.get()));
        let view = self.trading.borrow();
        let account = view.account_of(pane.account.borrow().as_deref());
        let chosen = crate::trading::feed_of(&view, account, chosen_feed.as_deref(), usable(), instrument)?;
        Some(providers::route_chosen(usable(), Some(chosen), instrument))
    }

    /// Drop a chart's chosen feed if it can no longer be chosen for
    /// `instrument`: taken off the order, or with no name for it. The chart
    /// goes back to following the order. True when it did.
    fn settle_feed(&self, pane: &ChartPane, instrument: &Instrument) -> bool {
        let stale = match pane.feed.borrow().as_deref() {
            None => false,
            Some(id) => {
                let feeds = self.feeds.borrow();
                !providers::choosable(feeds.iter().map(|feed| feed.provider.as_ref()), id, instrument)
            }
        };
        if stale {
            pane.feed.replace(None);
        }
        stale
    }

    /// Point one chart at a feed, or back at the order with `None`, and
    /// show it again from there. Only this chart: the others keep what they
    /// are showing.
    fn choose_feed(self: &Rc<Self>, id: u32, feed: Option<String>) {
        let Some(pane) = self.pane(id) else { return };
        if *pane.feed.borrow() == feed {
            return;
        }
        pane.feed.replace(feed);
        let instrument = pane.instrument.borrow().clone();
        if let Some(instrument) = instrument {
            self.show_in(&pane, instrument);
        }
        self.save_workspace();
    }

    /// Put one chart on an account, or back on the default with `None`,
    /// and say so in its header. A feed chosen for the chart stays: what a
    /// chart is drawn from and where its orders go are chosen apart, a
    /// Sierra Chart feed under a Rithmic account as well as either alone. A
    /// chart on no chosen feed follows the new account's default.
    fn choose_account(self: &Rc<Self>, id: u32, account: Option<String>) {
        let Some(pane) = self.pane(id) else { return };
        if *pane.account.borrow() == account {
            return;
        }
        pane.account.replace(account);
        self.apply_marks_to(&pane);
        self.reroute();
        self.save_workspace();
    }

    /// What chart `id` trades instead of what it charts, if anything.
    pub fn trading_symbol_of(&self, id: u32) -> Option<Instrument> {
        self.pane(id).and_then(|pane| pane.trading_symbol.borrow().clone())
    }

    /// The trading symbol of chart `id` as its settings name it: the one
    /// set, or the one kept with it that is still to be found.
    pub fn trading_symbol_named(&self, id: u32) -> Option<String> {
        let pane = self.pane(id)?;
        let set = pane.trading_symbol.borrow().as_ref().map(|traded| traded.display_symbol());
        set.or_else(|| pane.trading_pending.borrow().as_ref().map(|(symbol, _)| symbol.clone()))
    }

    /// A size picked before picks named their contract is the one `pane`
    /// trades now, about to change: it is named so, and stays that
    /// contract's rather than going on to the next, 10 MES never 10 ES.
    fn own_size(pane: &ChartPane) {
        let Some(traded) = pane.traded() else { return };
        if let Some(picked) = pane.trading_size.borrow_mut().as_mut().filter(|picked| picked.contract.is_none()) {
            picked.contract = Some(traded.root().to_string());
        }
    }

    /// Forget what `pane` trades instead when it does not track
    /// `instrument`, which it is about to chart. True when it did.
    fn drop_trading_symbol_unless(&self, pane: &ChartPane, instrument: &Instrument) -> bool {
        let traded = pane.trading_symbol.borrow().clone();
        let stale = traded.is_some_and(|traded| !instrument.tracks(&traded));
        // One still to be found is judged by its root, as written.
        let waiting = pane.trading_pending.borrow().as_ref().is_some_and(|(symbol, _)| {
            instrument.kind != omacharts_engine::InstrumentKind::FutureRoot
                || !omacharts_engine::specs::track_together(instrument.root(), omacharts_engine::specs::root_of(symbol))
        });
        if stale {
            pane.trading_symbol.replace(None);
        }
        if waiting {
            pane.trading_pending.replace(None);
        }
        stale || waiting
    }

    /// Have chart `id` trade `traded` rather than what it charts, or what
    /// it charts again for `None`: its header names both at once, and its
    /// orders, position, marks and P&L follow the trading contract.
    pub fn set_trading_symbol(self: &Rc<Self>, id: u32, traded: Option<Instrument>) {
        let Some(pane) = self.pane(id) else { return };
        // Only what tracks what it charts, at its prices: MES for ES, never
        // NQ. The picker offers nothing else.
        let charted = pane.instrument.borrow().clone();
        if let Some(traded) = &traded
            && !charted.is_some_and(|charted| charted.tracks(traded))
        {
            return;
        }
        // Set or cleared by hand: whatever was waiting to be found is
        // settled.
        let waited = pane.trading_pending.replace(None).is_some();
        let same = |a: &Instrument, b: &Instrument| a.symbol == b.symbol && a.suffix == b.suffix;
        let unchanged = match (pane.trading_symbol.borrow().as_ref(), traded.as_ref()) {
            (None, None) => true,
            (Some(was), Some(now)) => same(was, now),
            _ => false,
        };
        if unchanged && !waited {
            return;
        }
        Self::own_size(&pane);
        pane.trading_symbol.replace(traded);
        pane.write_readout();
        self.apply_marks_to(&pane);
        self.mark_to_market();
        self.save_workspace();
        self.chart_changed_elsewhere();
    }

    /// Drop a chart's account if it is no longer listed, say because its
    /// login was removed: it is on the default again, and on that
    /// account's default feed. True when it did.
    fn settle_account(&self, pane: &ChartPane) -> bool {
        // Trading switched off knows of no account, which does not make the
        // one a chart is on stale: it is there again when trading is.
        if !crate::features::trading() {
            return false;
        }
        let stale = crate::trading::stale(&self.trading.borrow().accounts, pane.account.borrow().as_deref());
        if stale {
            pane.account.replace(None);
            pane.feed.replace(None);
        }
        stale
    }

    /// The trading sessions, which the Trading page opens and the headers
    /// read.
    pub fn desk(&self) -> Rc<crate::trading::Desk> {
        self.desk.clone()
    }

    /// Read Trading again — the logins, their accounts, the default — and
    /// redraw every chart's account part from it.
    pub fn adopt_trading(self: &Rc<Self>) {
        let mut view = crate::trading::View::load(&self.store);
        view.book = self.marked_book(&view.logins);
        *self.trading.borrow_mut() = view;
        // A chart whose account's feed changed with the default is shown
        // again.
        self.reroute();
        self.sync_trading();
    }

    /// Keep the cache and the headers in step with the trading sessions: a
    /// balance arriving, or a session ending by itself, is noted on the
    /// main loop and every chart's part redrawn. Pushed by the session's
    /// thread, never looked for.
    fn watch_trading(self: &Rc<Self>) {
        // Told by the Trading page after every change it makes, and by the
        // loop below after every push: a login added or removed, an order
        // moving. The window reads Trading again either way.
        let weak = Rc::downgrade(self);
        let adopt: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(this) = weak.upgrade() {
                this.adopt_trading();
            }
        });
        self.desk.listen(&adopt);
        *self.trade.listener.borrow_mut() = Some(adopt);

        let woken = self.desk.woken();
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(wake) = woken.recv().await {
                let Some(this) = weak.upgrade() else { return };
                if matches!(wake, crate::trading::Wake::Priced) {
                    this.mark_to_market();
                    continue;
                }
                // A data feed signed in with its login changes which feeds
                // can be used.
                let fed = matches!(&wake, crate::trading::Wake::Opened(_, _, Some(_), _));
                let opened = match &wake {
                    crate::trading::Wake::Opened(login, Ok(_), ..) => Some(login.id.clone()),
                    _ => None,
                };
                this.desk.take(wake);
                if let Some(login) = opened {
                    this.find_contracts(&login);
                }
                this.desk.note(&this.store);
                if fed {
                    this.adopt_feed();
                }
                this.desk.tell();
            }
        });
    }

    /// Ask `login`'s session which contract every chart on screen trades
    /// as, so that its menu offers Buy and Sell by the time it is opened,
    /// whichever feed charts it. Never waits: what is not known yet is
    /// asked for, and arrives as a change.
    fn find_contracts(&self, login: &str) {
        let Some(session) = self.desk.session(login) else { return };
        for pane in self.panes.borrow().iter() {
            if let Some(instrument) = pane.instrument.borrow().as_ref() {
                let _ = session.contract_for(instrument);
            }
        }
    }

    /// Sign the saved trading logins in, once, after the window's first
    /// frame is on screen and behind whatever the charts asked for on the
    /// way: an idle at low priority, registered when that frame has been
    /// painted. Each login signs in and is refreshed on a thread of its own
    /// ([`crate::trading::Desk::sign_in_saved`]), and the panel, the
    /// headers and the chart lines fill in as each arrives. Nothing here
    /// waits on a network or a keyring. A window that is never shown, as
    /// in a test, signs nobody in.
    fn sign_in_after_first_frame(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let mapped = Rc::new(Cell::new(false));
        self.window.connect_map(move |window| {
            if mapped.replace(true) {
                return;
            }
            let Some(clock) = window.frame_clock() else { return };
            let painted: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
            let (weak, once) = (weak.clone(), painted.clone());
            let handler = clock.connect_after_paint(move |clock| {
                if let Some(handler) = once.borrow_mut().take() {
                    clock.disconnect(handler);
                }
                let weak = weak.clone();
                glib::idle_add_local_full(glib::Priority::LOW, move || {
                    if let Some(this) = weak.upgrade() {
                        let logins = this.trading.borrow().logins.clone();
                        this.desk.sign_in_saved(&logins);
                        this.desk.tell();
                    }
                    glib::ControlFlow::Break
                });
            });
            *painted.borrow_mut() = Some(handler);
        });
    }

    /// The feed whose cache key this is.
    fn feed_of(&self, key: &str) -> Option<Rc<Feed>> {
        self.feeds.borrow().iter().find(|feed| feed.owns(key)).cloned()
    }

    /// Put trading in or take it out, in place, as the flag now says: the
    /// same as the first login arriving or the last going. Switched off,
    /// every session on an order plant signs out; switched on, the saved
    /// logins sign in as they do at startup. The charts, the arrangement
    /// and the focus stay as they are.
    pub fn adopt_features(self: &Rc<Self>) {
        let on = crate::features::trading();
        if !on {
            for login in crate::trading::logins(&self.store) {
                self.desk.end(&login.id);
            }
        }
        self.adopt_trading();
        if on {
            let logins = self.trading.borrow().logins.clone();
            self.desk.sign_in_saved(&logins);
        }
        self.desk.tell();
        // A feed behind a flag comes or goes with it, and its charts with it.
        self.adopt_feed();
    }

    /// Zoom every chart at the sensitivity the settings now name. Read by the
    /// charts on each scroll, so nothing on screen needs redrawing.
    pub fn adopt_zoom(&self) {
        crate::zoom::adopt(&self.store);
    }

    /// Chart from whatever feeds the settings now name, if they are not the
    /// ones on screen; and read again which of them can be used, which a
    /// sign-in or a sign-out has just changed. What the panel and
    /// `config set provider` call.
    pub fn adopt_feed(self: &Rc<Self>) {
        crate::feeds::remember_logins(&self.store);
        let wanted = providers::ordered(&crate::feeds::stored_order(&self.store));
        let same = {
            let feeds = self.feeds.borrow();
            feeds.len() == wanted.len()
                && feeds.iter().zip(&wanted).all(|(feed, want)| feed.provider.id() == want.id())
        };
        if !same {
            self.regroup_feeds(wanted.into_iter().map(Arc::from).collect());
        }
        for feed in self.feeds() {
            feed.read_access();
        }
        // Whatever changed — the feeds, which of them can be used, or what
        // one of them has, a folder set up that has ES now — every chart is
        // held against the feed it was shown from, and the ones it moves
        // are shown again, at once.
        self.reroute();
    }

    /// Chart from `providers`, top first, keeping the feeds that stay on
    /// the order as they are: their loaders, their streams and their
    /// sessions. Only the feeds taken off are retired, and only the new
    /// ones are built. What [`Window::adopt_feed`] does when the Data page
    /// adds, removes or reorders a feed; [`Window::switch_feeds`] is the
    /// whole replacement.
    fn regroup_feeds(self: &Rc<Self>, providers: Vec<Arc<dyn Provider>>) {
        let old = self.feeds();
        let mut receivers = Vec::new();
        let mut lives = Vec::new();
        let mut at_prices = Vec::new();
        let feeds: Vec<Rc<Feed>> = providers
            .into_iter()
            .map(|provider| {
                if let Some(kept) = old.iter().find(|feed| feed.provider.id() == provider.id()) {
                    kept.read_access();
                    return kept.clone();
                }
                let (sender, receiver) = async_channel::unbounded::<Response>();
                let feed = Rc::new(Feed::new(provider, &self.store, sender));
                receivers.push((Rc::downgrade(&feed), receiver));
                lives.extend(feed.live.clone());
                at_prices.extend(feed.at_price.clone());
                feed
            })
            .collect();
        for gone in old.iter().filter(|feed| !feeds.iter().any(|kept| Rc::ptr_eq(kept, feed))) {
            Window::retire_feed(gone);
        }
        if let Some(top) = feeds.first() {
            crate::feeds::note_running(top.provider.id());
        }
        *self.feeds.borrow_mut() = feeds;
        for (feed, receiver) in receivers {
            self.wire_responses(feed, receiver);
        }
        for live in lives {
            self.wire_live_of(live);
        }
        for at_price in at_prices {
            self.wire_at_price(at_price);
        }
        self.wire_refresh();
    }

    /// Show again the charts whose feed is no longer the one they were
    /// shown from, and only rewrite the header of the rest: a chart still
    /// on the feed it was on keeps its series and its stream as they are.
    fn reroute(self: &Rc<Self>) {
        let panes: Vec<Rc<ChartPane>> = self.panes.borrow().clone();
        let mut settled = false;
        for pane in panes {
            let instrument = pane.instrument.borrow().clone();
            let Some(instrument) = instrument else { continue };
            let was = pane.served.borrow().clone();
            // A chart whose chosen feed has left the order follows it again,
            // and one whose account has gone is on the default.
            settled |= self.settle_feed(&pane, &instrument);
            settled |= self.settle_account(&pane);
            match self.route_pane(&pane, &instrument) {
                Some(routed) if was.as_deref() == Some(routed.key().as_str()) => {
                    self.write_feed_of(&pane, Some(&routed.feed), &routed.passed);
                }
                _ => self.show_in(&pane, instrument),
            }
        }
        if settled {
            self.save_workspace();
        }
        if let Some(watchlist) = self.watchlist.borrow().as_ref() {
            watchlist.refresh_quotes();
            self.write_changes();
        }
    }

    /// Chart from `providers`, top first, from now on.
    ///
    /// One ordered sequence, in one place, because scattered as reactions
    /// it would be a race that only a second click finds. The order is the
    /// argument: nothing may be built on the new feeds while anything of
    /// the old ones can still deliver, and nothing of the old ones may be
    /// told to stop while a chart could still ask them for more.
    ///
    /// 1. Retire the outgoing feeds — [`Window::retire_feeds`]: their
    ///    replies are stale from here, their subscriptions are closed and
    ///    flushed, their queues are dropped, and the workers end on their
    ///    own.
    /// 2. Install the new providers, lazily: constructed, held, connected to
    ///    nothing until a chart asks.
    /// 3. Build what each needs — a loader, and a registry if it streams —
    ///    and wire their replies to the window.
    /// 4. Re-arm liveness for what the new feeds are: the refresh timer if
    ///    any polls, which the timer itself gives up if none does later.
    /// 5. Reload what is on screen, and only that: each chart is routed
    ///    again, paints from its feed's cache at once if it has one, keeps
    ///    what it shows until the new bars land if it does not, and says so
    ///    through its own trouble dot if no feed can carry its symbol. The
    ///    bar cache is not touched: it is keyed by feed, so nothing mixes
    ///    and switching back is instant.
    ///
    /// Nothing narrates any of this. The person ticks the other feed and
    /// the charts are from the other feed.
    pub fn switch_feeds(self: &Rc<Self>, providers: Vec<Arc<dyn Provider>>) {
        self.retire_feeds();

        if let Some(top) = providers.first() {
            crate::feeds::note_running(top.id());
        }
        let mut receivers = Vec::with_capacity(providers.len());
        let feeds: Vec<Rc<Feed>> = providers
            .into_iter()
            .map(|provider| {
                let (sender, receiver) = async_channel::unbounded::<Response>();
                let feed = Rc::new(Feed::new(provider, &self.store, sender));
                receivers.push((Rc::downgrade(&feed), receiver));
                feed
            })
            .collect();
        *self.feeds.borrow_mut() = feeds;
        for (feed, receiver) in receivers {
            self.wire_responses(feed, receiver);
        }
        self.wire_live();
        self.wire_refresh();
        self.reshow_all();
    }

    /// Show every chart on screen again, routed afresh.
    fn reshow_all(self: &Rc<Self>) {
        let panes: Vec<Rc<ChartPane>> = self.panes.borrow().clone();
        for pane in panes {
            let instrument = pane.instrument.borrow().clone();
            if let Some(instrument) = instrument {
                self.show_in(&pane, instrument);
            }
        }
        if let Some(watchlist) = self.watchlist.borrow().as_ref() {
            watchlist.refresh_quotes();
            self.write_changes();
        }
    }

    /// Stop the feeds on screen, in the order that leaves nothing of them
    /// able to reach a chart.
    ///
    /// First the generation moves on, so a reply already on its way from a
    /// worker is dropped on arrival rather than painted. Then every stream
    /// subscription is closed and what it held is written, so the
    /// providers' threads end and the cache is current. Then the queues are
    /// emptied, each worker finishes whatever it is sending now, finds
    /// nothing to send and the window gone, and ends. Last each provider is
    /// told it is retired, which is where a feed that holds a connection
    /// closes it. The cache keeps every bar: it is keyed by feed, and the
    /// feed may be back.
    fn retire_feeds(self: &Rc<Self>) {
        self.feed_generation.set(self.feed_generation.get() + 1);
        for feed in self.feeds() {
            Window::retire_feed(&feed);
        }
    }

    /// Stop one feed, as [`Window::retire_feeds`] stops them all.
    fn retire_feed(feed: &Feed) {
        feed.retired.set(true);
        if let Some(live) = &feed.live {
            live.shutdown();
        }
        if let Some(at_price) = &feed.at_price {
            at_price.shutdown();
        }
        feed.loader.drop_prefetches();
        feed.loader.drop_backfill();
        // Whatever the provider holds beyond its value: a connection, a
        // session on a gateway. The worker still finishing a request on
        // it gets a failure it already knows how to report, and drops it.
        feed.provider.retire();
    }

    /// Drain on the next frame, once, however many wakes arrive before it.
    ///
    /// The frame clock rather than an idle: the expensive half of a tick —
    /// the indicators — then runs once per frame whatever the tick rate, and
    /// a `queue_draw` from inside the update phase is painted in that same
    /// frame. A frozen clock (the window on another workspace) holds the
    /// drain, and the mailboxes hold the newest bars, until it runs again.
    fn schedule_drain(self: &Rc<Self>) {
        if self.drain_scheduled.replace(true) {
            return;
        }
        let this = self.clone();
        self.window.add_tick_callback(move |_, _| {
            this.drain_live();
            glib::ControlFlow::Break
        });
        // A window on another workspace gets no frames, and a drain that
        // only ever ran on one would leave the series, the cache and
        // `provider status` where they were when the window was last seen.
        // So if no frame has come for the wake, the drain runs anyway, a
        // tenth of a second later: the series stays current and the writer
        // keeps writing, while painting stays the frame clock's. On a
        // window that is on screen the frame comes first and this finds
        // nothing to do.
        let this = self.clone();
        glib::timeout_add_local_once(HIDDEN_DRAIN_WAIT, move || {
            if this.drain_scheduled.get() {
                this.drain_live();
            }
        });
    }

    /// Fold everything that arrived into the series in memory and redraw
    /// what changed.
    fn drain_live(self: &Rc<Self>) {
        self.drain_scheduled.set(false);
        let mut retry = false;
        for feed in self.feeds() {
            if let Some(at_price) = &feed.at_price {
                let touched = at_price.drain();
                self.recompute_panes(&touched);
            }
            let Some(live) = &feed.live else { continue };
            let drained = live.drain();
            for change in drained.changes {
                self.apply_live_change(live, change);
            }
            if drained.schedule_flush {
                let registry = live.clone();
                glib::timeout_add_local_once(crate::live::WRITE_BEHIND, move || registry.flush());
            }
            retry |= drained.schedule_retry;
        }
        if retry {
            self.schedule_retry();
        }
    }

    /// The registries of the feeds that stream.
    fn lives(&self) -> Vec<Rc<Live>> {
        self.feeds.borrow().iter().filter_map(|feed| feed.live.clone()).collect()
    }

    /// One change to one series of `live`'s, on every chart watching it.
    fn apply_live_change(self: &Rc<Self>, live: &Rc<Live>, change: Change) {
        match change {
            // The whole series: the ordinary paint, which folds, filters and
            // recomputes, as a loader reply would.
            Change::Snapshot(series) => {
                if self.messages.lost.borrow_mut().remove(&series) {
                    self.say_of_stream(&series, None);
                }
                let bars = live.with_bars(&series, |bars| bars.to_vec()).unwrap_or_default();
                for id in live.watchers(&series) {
                    let Some(pane) = self.pane(id) else { continue };
                    let instrument = pane.instrument.borrow().clone();
                    let Some(instrument) = instrument else { continue };
                    self.paint(&pane, &series.key, &instrument, pane.timeframe.get(), bars.clone());
                    pane.view.set_trouble(None);
                    pane.view.set_loading(false);
                    pane.set_feed_phase(pane::FeedPhase::Connected);
                }
                if let Some(watchlist) = self.watchlist.borrow().as_ref() {
                    watchlist.refresh_quotes();
                    self.write_changes();
                }
            }
            // The end of the series: each chart's view of the bars from the
            // touched bucket on, put in place of what it had.
            Change::Tail { series, from } => {
                for id in live.watchers(&series) {
                    let Some(pane) = self.pane(id) else { continue };
                    let instrument = pane.instrument.borrow().clone();
                    let Some(instrument) = instrument else { continue };
                    let timeframe = pane.timeframe.get();
                    let (tail, latest) = live
                        .with_bars(&series, |native| {
                            let grid = Grid::new(&instrument, pane.session.get());
                            let start = omacharts_engine::stream::tail_start(native, from, timeframe, &grid);
                            // Left out and cut back as `paint` did, or a tick
                            // refolding this week, month or year would put back
                            // what the chart had left out of it.
                            let drawn =
                                omacharts_engine::drawable_from(native, start, timeframe, instrument.kind);
                            let tail = omacharts_engine::session::filter(
                                &drawn,
                                pane.session.get(),
                                &instrument,
                                timeframe.is_intraday(),
                            );
                            // The trade the session leaves out of the bars,
                            // which the last price still follows.
                            let latest = native.last().filter(|newest| {
                                hidden_latest(std::slice::from_ref(*newest), pane.session.get(), &instrument, timeframe)
                                    .is_some()
                            });
                            let tail = if timeframe.is_derived() { resample(&tail, timeframe, &grid) } else { tail };
                            (tail, latest.copied())
                        })
                        .unwrap_or_default();
                    pane.view.set_latest(latest.map(|newest| newest.close));
                    if let Some(newest) = latest
                        && pane.shows(crate::header::Part::Change)
                    {
                        self.write_live_change_of(&pane, &newest);
                    }
                    if tail.is_empty() {
                        continue;
                    }
                    pane.view.apply_tail(&tail);
                    if let Some(newest) = tail.last()
                        && latest.is_none()
                        && pane.shows(crate::header::Part::Change)
                    {
                        self.write_live_change_of(&pane, newest);
                    }
                    // The memo this chart was painted from has to agree with
                    // the chart, or the next switch back paints yesterday.
                    let memo = {
                        let mut memo = self.series.borrow_mut();
                        if let Some(entry) = memo.get_mut(&(series.key.clone(), timeframe)) {
                            let bars = Rc::make_mut(entry);
                            for bar in &tail {
                                omacharts_engine::stream::upsert(bars, *bar);
                            }
                        }
                        memo.get(&(series.key.clone(), timeframe)).cloned()
                    };
                    let drawn = match memo {
                        Some(bars) => self.drawn_indicators(&pane, &instrument, timeframe, &bars),
                        None => pane
                            .view
                            .with_bars(|bars| self.drawn_indicators(&pane, &instrument, timeframe, bars)),
                    };
                    pane.view.set_indicators(drawn);
                    self.chart_moved(&pane);
                }
            }
            // The bars stay; the corner says why nothing newer is coming.
            Change::Lost { series, why } => {
                self.messages.lost.borrow_mut().insert(series.clone());
                self.say_of_stream(&series, Some(why));
                for id in live.watchers(&series) {
                    let Some(pane) = self.pane(id) else { continue };
                    pane.view.set_loading(false);
                    pane.view.set_trouble(Some(why));
                    pane.set_feed_phase(pane::FeedPhase::Lost(why));
                }
            }
        }
    }

    /// Keep one timer at the earliest retry due, replacing whatever was
    /// timed before. No lost series, no timer.
    fn schedule_retry(self: &Rc<Self>) {
        let lives = self.lives();
        if lives.is_empty() {
            return;
        }
        if let Some(old) = self.retry_timer.borrow_mut().take() {
            old.remove();
        }
        let Some(at) = lives.iter().filter_map(|live| live.next_retry_at()).min() else { return };
        let this = self.clone();
        let wait = at.saturating_duration_since(Instant::now());
        let source = glib::timeout_add_local_once(wait, move || {
            *this.retry_timer.borrow_mut() = None;
            let now = Instant::now();
            let mut again = false;
            for live in &lives {
                again |= live.retry_due(now);
            }
            if again {
                this.schedule_retry();
            }
        });
        *self.retry_timer.borrow_mut() = Some(source);
    }

    /// Chart `id` stopped showing what it was streaming, on any feed.
    fn release_pane(self: &Rc<Self>, id: u32) {
        for live in self.lives() {
            if live.unwatch(id) {
                Window::schedule_reap(live);
            }
        }
        if let Some(pane) = self.pane(id) {
            pane.ladders.borrow_mut().take();
        }
        for feed in self.feeds() {
            if let Some(at_price) = &feed.at_price {
                at_price.unwatch(id);
            }
        }
    }

    /// Chart `id` is showing something from `serving` now, so whatever it
    /// streamed from any other feed it is not showing any more.
    fn release_pane_elsewhere(self: &Rc<Self>, id: u32, serving: &Rc<Feed>) {
        for feed in self.feeds() {
            if Rc::ptr_eq(&feed, serving) {
                continue;
            }
            if let Some(live) = &feed.live
                && live.unwatch(id)
            {
                Window::schedule_reap(live.clone());
            }
            if let Some(at_price) = &feed.at_price {
                at_price.unwatch(id);
            }
        }
    }

    /// Every chart stopped: a chartbook is coming down.
    fn release_all_panes(self: &Rc<Self>) {
        for live in self.lives() {
            if live.unwatch_all() {
                Window::schedule_reap(live);
            }
        }
        for pane in self.panes.borrow().iter() {
            pane.ladders.borrow_mut().take();
        }
        for feed in self.feeds() {
            if let Some(at_price) = &feed.at_price {
                at_price.unwatch_all();
            }
        }
    }

    /// Tear idle series down on idle rather than now, so the chart that
    /// comes straight back — the next chartbook showing the same symbol —
    /// finds its subscription where it left it.
    fn schedule_reap(live: Rc<Live>) {
        glib::idle_add_local_once(move || live.reap());
    }

    /// What is being streamed right now, for `provider status`.
    pub fn streaming(&self) -> Vec<crate::live::Report> {
        self.lives().iter().flat_map(|live| live.report()).collect()
    }

    /// Replace the set of indicators and redraw.
    /// Indicators belong to one chart, so this is the focused one's.
    pub fn set_indicators(self: &Rc<Self>, indicators: Vec<Indicator>) {
        let pane = self.focused_pane();
        self.change_indicators_of(&pane, indicators);
    }

    /// Change what a chart shows, from the chart: on a template, the
    /// template's indicators — and so every chart on it — and otherwise the
    /// chart's own.
    ///
    /// One rule for every control on the chart — the legend's eye and bin, a
    /// strip's corner, its settings — so that two buttons side by side cannot
    /// mean "every chart" and "this chart" between them. A chart is taken off
    /// its template by choosing None for it, not by touching an indicator.
    fn change_indicators_of(self: &Rc<Self>, pane: &Rc<ChartPane>, indicators: Vec<Indicator>) {
        match pane.template.get() {
            Some(template) => self.set_template_indicators(template, indicators),
            None => self.set_indicators_of(pane, indicators),
        }
    }

    /// Give one chart these indicators as its own, taking it off any
    /// template first and keeping what it showed.
    fn set_indicators_of(self: &Rc<Self>, pane: &Rc<ChartPane>, indicators: Vec<Indicator>) {
        if pane.template.take().is_some() {
            self.write_header_of(pane);
            if pane.id == self.focused.get() {
                self.sync_chart_actions(pane);
            }
        }
        // Still written to the one stored set, which is what a brand new chart
        // starts from: the next split should look like the last thing you set
        // up rather than like the defaults.
        self.store.set_indicators(&indicators);
        self.show_indicators(pane, indicators);
        self.save_workspace();
    }

    /// Draw a chart with these indicators, writing nothing down.
    fn show_indicators(self: &Rc<Self>, pane: &Rc<ChartPane>, indicators: Vec<Indicator>) {
        *pane.indicators.borrow_mut() = indicators;
        self.rebuild_legend_of(pane);
        let instrument = pane.instrument.borrow().clone();
        if let Some(instrument) = instrument {
            self.series.borrow_mut().clear();
            self.show_in(pane, instrument);
        }
    }

    // -- chart templates ---------------------------------------------------

    /// Every chart template, in the order they were made.
    pub fn templates(&self) -> Vec<crate::templates::Template> {
        crate::templates::all(&self.store)
    }

    /// The template the focused chart is on, if it is on one.
    pub fn focused_template(&self) -> Option<u32> {
        self.focused_pane().template.get()
    }

    /// Put the focused chart on a template, or on none.
    ///
    /// Onto one, the chart shows the template's indicators in place of its
    /// own, which go. Onto none, it shows no indicators at all: None is a
    /// choice of indicators like the others, the empty one.
    pub fn set_template(self: &Rc<Self>, template: Option<u32>) {
        let pane = self.focused_pane();
        self.put_on_template(&pane, template);
        self.sync_chart_actions(&pane);
        self.save_workspace();
    }

    /// The template at `n` in the order they are listed, counting from 1, or
    /// none for 0. A number with no template behind it does nothing.
    fn pick_template(self: &Rc<Self>, n: usize) {
        let chosen = match n {
            0 => None,
            n => match self.templates().get(n - 1) {
                Some(template) => Some(template.id),
                None => return,
            },
        };
        // None again still clears a chart showing indicators of its own,
        // which is the one thing pressing it there can mean.
        if chosen.is_none() || chosen != self.focused_template() {
            self.choose_template(chosen, || {});
        }
    }

    /// The next or previous of None and the templates, round and round.
    fn step_template(self: &Rc<Self>, by: i32) {
        let templates = self.templates();
        let at = self
            .focused_template()
            .and_then(|id| templates.iter().position(|t| t.id == id))
            .map_or(0, |at| at + 1);
        let next = step_round(at, templates.len(), by);
        let chosen = next.checked_sub(1).and_then(|at| templates.get(at)).map(|t| t.id);
        self.choose_template(chosen, || {});
    }

    /// Put the focused chart on a template or on none, asking first when that
    /// would drop indicators of its own, and run `done` once it is settled
    /// either way.
    ///
    /// Only then: a chart already on a template has nothing of its own to
    /// lose, one with nothing on it has nothing to lose at all, and one
    /// showing exactly what some template holds can have it back by choosing
    /// that template.
    pub fn choose_template(self: &Rc<Self>, template: Option<u32>, done: impl Fn() + 'static) {
        let pane = self.focused_pane();
        let chosen = template.and_then(|id| crate::templates::get(&self.store, id));
        let own = pane.indicators.borrow().len();
        let kept_elsewhere =
            self.templates().iter().any(|t| *pane.indicators.borrow() == t.indicators);
        let loses = pane.template.get().is_none() && own > 0 && !kept_elsewhere;
        // Choosing what it already shows loses nothing either.
        let same = chosen.as_ref().is_some_and(|t| *pane.indicators.borrow() == t.indicators);
        if !loses || same {
            self.set_template(template);
            done();
            return;
        }
        let count = match own {
            1 => "one".to_string(),
            n => n.to_string(),
        };
        let (title, body, verb) = match &chosen {
            Some(chosen) => (
                "Use Chart Template?",
                format!(
                    "This chart will show the indicators of \u{201c}{}\u{201d} in place of its own {count}.",
                    chosen.name
                ),
                "Replace Indicators",
            ),
            None => (
                "Remove Indicators?",
                format!(
                    "This chart will show no indicators, and its own {count} will be removed."
                ),
                "Remove Indicators",
            ),
        };
        let dialog = adw::AlertDialog::new(Some(title), Some(&body));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("replace", verb);
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "replace" {
                this.set_template(template);
            } else {
                // Put the menu's mark back on what the chart is still on.
                this.sync_chart_actions(&this.focused_pane());
            }
            done();
        });
        dialog.present(Some(&self.window));
    }

    fn put_on_template(self: &Rc<Self>, pane: &Rc<ChartPane>, template: Option<u32>) {
        let found = template.and_then(|id| crate::templates::get(&self.store, id));
        pane.template.set(found.as_ref().map(|t| t.id));
        self.write_header_of(pane);
        let indicators = found.map(|t| t.indicators).unwrap_or_default();
        if *pane.indicators.borrow() != indicators {
            self.show_indicators(pane, indicators);
        }
    }

    /// Change what a template holds, and every chart on it with it.
    pub fn set_template_indicators(self: &Rc<Self>, template: u32, indicators: Vec<Indicator>) {
        crate::templates::set_indicators(&self.store, template, indicators);
        self.follow_templates();
    }

    /// Ask for a name, and keep what the focused chart shows as a new
    /// template under it, putting the chart on it.
    ///
    /// On it, because that is what keeping it was for: the chart already
    /// shows exactly what the template holds, so nothing on screen changes,
    /// and the next change to its indicators is one the template takes too.
    pub fn new_template_from_chart(self: &Rc<Self>) {
        let pane = self.focused_pane();
        let indicators = pane.indicators.borrow().clone();
        if indicators.is_empty() {
            return;
        }
        let this = self.clone();
        crate::ui::preferences::ask_name(
            &self.window,
            "New Chart Template",
            "",
            "Create",
            move |name| {
                let id = crate::templates::create_from(&this.store, &name, indicators.clone()).id;
                // The chart it was asked on, wherever the focus went meanwhile.
                if this.panes.borrow().iter().any(|p| Rc::ptr_eq(p, &pane)) {
                    this.put_on_template(&pane, Some(id));
                    this.sync_chart_actions(&this.focused_pane());
                    this.save_workspace();
                }
            },
        );
    }

    pub fn create_template(self: &Rc<Self>, name: &str) -> u32 {
        crate::templates::create(&self.store, name).id
    }

    pub fn rename_template(self: &Rc<Self>, template: u32, name: &str) {
        crate::templates::rename(&self.store, template, name);
        // The menus and the chart's own settings read the name when they are
        // opened; a header showing it is the one place it is on screen.
        let panes = self.panes.borrow().clone();
        for pane in panes.iter().filter(|pane| pane.template.get() == Some(template)) {
            self.write_header_of(pane);
        }
    }

    // -- the header ------------------------------------------------------

    /// The header as it is set now, on every chart on screen: what follows
    /// a change to it, from the preferences or a command.
    ///
    /// The charts of the other chartbooks are not built until their book is
    /// open, and they read the setting then.
    pub fn apply_header(self: &Rc<Self>) {
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            self.show_header_on(pane);
        }
    }

    /// One chart's header, as the setting says.
    fn show_header_on(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        *pane.header.borrow_mut() = crate::header::load(&self.store);
        pane.arrange_header();
        self.write_header_of(pane);
    }

    /// Fill in the parts of a chart's header only the window knows: the
    /// template's name, and the day's move. Each only when the header shows
    /// it, so a chart that does not ask for the move never reads a quote.
    fn write_header_of(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        use crate::header::Part;
        if pane.shows(Part::Template) {
            let name = pane.template.get().and_then(|id| crate::templates::get(&self.store, id));
            pane.write_template(name.as_ref().map(|t| t.name.as_str()));
        }
        if pane.shows(Part::Change) {
            self.write_change_of(pane);
        }
    }

    /// Say which account and feed a chart is on. The Account part names
    /// the account, "Lucid 1234", with its trouble after it, while there
    /// is more than one account to be on. The Data feed part's dot is
    /// `serving`'s, told what the order passed over to reach it, while
    /// there is more than Yahoo to be on or connected to. With one account
    /// and Yahoo alone each says nothing and draws nothing.
    fn write_feed_of(&self, pane: &ChartPane, serving: Option<&Rc<Feed>>, passed: &[Rc<Feed>]) {
        self.write_account_of(pane);
        let part = self.feed_part_of(pane, |part| {
            part.label = serving.map(|feed| feed.provider.label().to_string());
            part.passed = passed
                .iter()
                .map(|feed| format!("{} {}", feed.provider.label(), feed.unusable_because.get()))
                .collect();
        });
        pane.write_feed(part);
    }

    /// Say that a chart's chosen feed cannot serve it, and why: the dot
    /// red, "Rithmic · Not signed in" on it.
    fn write_refused_feed(&self, pane: &ChartPane, feed: &Rc<Feed>) {
        self.write_account_of(pane);
        let part = self.feed_part_of(pane, |part| {
            part.label = Some(feed.provider.label().to_string());
            part.refused = Some(feed.unusable_because.get().to_string());
        });
        pane.write_feed(part);
    }

    /// The Account part's words for `pane`: the account with its trouble
    /// after it, "1234 signed out", while there is more than one account
    /// across the trading services; nothing otherwise.
    fn write_account_of(&self, pane: &ChartPane) {
        let view = self.trading.borrow();
        let text = match view.accounts.len() > 1 {
            true => view.account_of(pane.account.borrow().as_deref()).map(|account| {
                let trouble = view.trouble(&self.desk, account);
                crate::trading::account_text(&view.account_label(account), trouble.as_deref())
            }),
            false => None,
        };
        pane.write_account(text.as_deref());
    }

    /// The Data feed part for `pane`, when the dot is drawn at all: with
    /// more than Yahoo on the order, or a feed that connects to something.
    /// `fill` says which feed; this says whether another can be chosen.
    fn feed_part_of(&self, pane: &ChartPane, fill: impl FnOnce(&mut pane::FeedPart)) -> Option<pane::FeedPart> {
        let feeds = self.feeds.borrow();
        if !crate::feeds::worth_a_dot(feeds.iter().map(|feed| feed.provider.id())) {
            return None;
        }
        let instrument = pane.instrument.borrow();
        let serving = instrument.as_ref().map_or(0, |instrument| {
            providers::serving(feeds.iter().map(|feed| feed.provider.as_ref()), instrument).len()
        });
        let mut part = pane::FeedPart { clickable: serving > 1, ..pane::FeedPart::default() };
        fill(&mut part);
        Some(part)
    }

    /// Every chart's day's move, after quotes have moved.
    ///
    /// Out of the cache the rail reads, through the same lookup, so the
    /// header and the rail cannot disagree and nothing is fetched for it.
    /// Called where the rail's own numbers are refreshed — never from a draw.
    fn write_changes(self: &Rc<Self>) {
        let panes = self.panes.borrow().clone();
        for pane in panes.iter().filter(|pane| pane.shows(crate::header::Part::Change)) {
            self.write_change_of(pane);
        }
    }

    fn write_change_of(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        let instrument = pane.instrument.borrow().clone();
        let quote = instrument.and_then(|instrument| {
            let routed = self.route_pane(pane, &instrument)?;
            crate::cli::quote(&self.store, routed.feed.provider.as_ref(), &instrument)
        });
        // In the theme's green and red, a chart whose rises are red saying
        // so here too; see `header::change_colour`.
        match quote {
            Some(quote) => {
                pane.change_basis.set(Some((quote.last - quote.change, crate::cli::session_day(quote.as_of))));
                pane.change_shown.set(Some(hundredths(quote.change_pct)));
                let colour = crate::header::change_colour(&self.theme(), pane.swapped.get(), quote.change);
                pane.write_change(Some((quote.change_pct, colour.as_deref())));
            }
            None => {
                pane.change_basis.set(None);
                pane.change_shown.set(None);
                pane.write_change(None);
            }
        }
    }

    /// The day's move again, from a bar that just arrived: on the frame the
    /// trade is drawn on, rather than when the cache next catches up. Only
    /// a bar of the session the move was last read for, and only written
    /// when the figure shown changes.
    fn write_live_change_of(self: &Rc<Self>, pane: &Rc<ChartPane>, newest: &omacharts_engine::Bar) {
        let Some((previous, day)) = pane.change_basis.get() else { return };
        if previous == 0.0 || crate::cli::session_day(newest.ts) != day {
            return;
        }
        let change = newest.close - previous;
        let change_pct = change / previous * 100.0;
        if pane.change_shown.replace(Some(hundredths(change_pct))) == Some(hundredths(change_pct)) {
            return;
        }
        let colour = crate::header::change_colour(&self.theme(), pane.swapped.get(), change);
        pane.write_change(Some((change_pct, colour.as_deref())));
    }

    /// Delete a template. Its charts keep what it was showing them.
    pub fn delete_template(self: &Rc<Self>, template: u32) {
        crate::templates::delete(&self.store, template);
        self.follow_templates();
    }

    /// How many charts, in every chartbook, are on `template`.
    pub fn charts_on_template(&self, template: u32) -> usize {
        let active = self.active.get();
        let on_screen =
            self.panes.borrow().iter().filter(|pane| pane.template.get() == Some(template)).count();
        let elsewhere: usize = self
            .books
            .borrow()
            .iter()
            .enumerate()
            .filter(|(at, _)| *at != active)
            .map(|(_, book)| book.panes.iter().filter(|p| p.template == Some(template)).count())
            .sum();
        on_screen + elsewhere
    }

    /// Bring every chart on a template up to what the template now holds.
    ///
    /// The charts on screen are redrawn, and only the ones whose indicators
    /// actually changed; the books that are not on screen are only written
    /// down, and read the template again when they are next built anyway. A
    /// chart whose template has gone keeps what it was showing, as its own.
    fn follow_templates(self: &Rc<Self>) {
        let templates = crate::templates::all(&self.store);
        let held = |id: u32| templates.iter().find(|t| t.id == id).map(|t| &t.indicators);
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            let Some(id) = pane.template.get() else { continue };
            match held(id) {
                Some(indicators) if *pane.indicators.borrow() != *indicators => {
                    self.show_indicators(pane, indicators.clone());
                }
                Some(_) => {}
                None => {
                    pane.template.set(None);
                    self.write_header_of(pane);
                }
            }
        }
        let active = self.active.get();
        for (at, book) in self.books.borrow_mut().iter_mut().enumerate() {
            if at == active {
                continue;
            }
            for stored in &mut book.panes {
                let Some(id) = stored.template else { continue };
                match held(id) {
                    Some(indicators) => stored.indicators = indicators.clone(),
                    None => stored.template = None,
                }
            }
        }
        self.sync_chart_actions(&self.focused_pane());
        self.save_workspace();
    }

    /// The indicator rows under the legend: a dot, a name, and the two things
    /// you reach for without opening anything — hide it, or go to its settings.
    ///
    /// Deliberately faint. These sit over the drawing, and the drawing is the
    /// point; they come up to full strength when the pointer is near.
    fn rebuild_indicator_legend(self: &Rc<Self>) {
        let panes = self.panes.borrow().clone();
        for pane in panes {
            self.rebuild_legend_of(&pane);
        }
    }

    fn rebuild_legend_of(self: &Rc<Self>, pane: &Rc<ChartPane>) {
        while let Some(child) = pane.indicator_legend.first_child() {
            pane.indicator_legend.remove(&child);
        }
        pane.write_readout();
        // Read once for the whole legend: off, nothing is named, whatever
        // each indicator's own `in_legend` says.
        if !pane.shows_legend.get() {
            return;
        }
        let theme = self.theming.borrow().theme();

        let timeframe = pane.timeframe.get();
        let all = pane.indicators.borrow().clone();
        let colors = omacharts_engine::palette_colors(&all, &theme);
        for (indicator, colour) in all.iter().zip(colors).filter(|(i, _)| i.in_legend) {
            let id = indicator.id;

            // The dot in its colour, ringed in it too while the indicator
            // is selected on the chart. Drawn in room the ring already has,
            // so selecting one moves nothing in the row.
            let dot = gtk::DrawingArea::new();
            dot.set_size_request(crate::ui::pane::LEGEND_DOT, crate::ui::pane::LEGEND_DOT);
            dot.set_valign(gtk::Align::Center);
            let dot_colour = colour.clone();
            let dot_pane = Rc::downgrade(pane);
            dot.set_draw_func(move |_, cr, width, height| {
                let (x, y) = (width as f64 / 2.0, height as f64 / 2.0);
                colors::set_source(cr, &dot_colour);
                cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                if dot_pane.upgrade().is_some_and(|p| p.view.picked() == Some(id)) {
                    cr.set_line_width(1.5);
                    cr.arc(x, y, (width.min(height) as f64) / 2.0 - 0.75, 0.0, std::f64::consts::TAU);
                    let _ = cr.stroke();
                }
            });

            let estimated = pane.estimated.borrow().contains(&id);
            let label = gtk::Label::new(Some(&indicator.legend_for(timeframe, estimated)));
            label.add_css_class("legend-indicator");
            label.set_xalign(0.0);
            if !indicator.visible || !pane.view.show_indicators() {
                label.add_css_class("legend-indicator-hidden");
            }

            // A click on the name selects the indicator on the chart, as a
            // click on its line does. Double-clicking opens its settings, the
            // same as the gear beside it: the name is the bigger target and
            // the one you are already looking at.
            let open_settings = gtk::GestureClick::new();
            let this = self.clone();
            let picker = pane.clone();
            open_settings.connect_pressed(move |_, presses, _, _| {
                this.focus(picker.id);
                if presses < 2 {
                    picker.view.pick(Some(id));
                    return;
                }
                crate::ui::chart_settings::ChartSettings::present_indicator(&this, id);
            });
            label.add_controller(open_settings);


            let toggle = gtk::Button::from_icon_name(if indicator.visible {
                "view-reveal-symbolic"
            } else {
                "view-conceal-symbolic"
            });
            toggle.add_css_class("flat");
            toggle.add_css_class("legend-button");
            toggle.set_tooltip_text(Some(if indicator.visible { "Hide" } else { "Show" }));
            let this = self.clone();
            let shown = pane.clone();
            toggle.connect_clicked(move |_| {
                let mut indicators = shown.indicators.borrow().clone();
                if let Some(found) = indicators.iter_mut().find(|i| i.id == id) {
                    found.visible = !found.visible;
                }
                this.change_indicators_of(&shown, indicators);
            });

            let settings = gtk::Button::from_icon_name("emblem-system-symbolic");
            settings.add_css_class("flat");
            settings.add_css_class("legend-button");
            settings.set_tooltip_text(Some("Settings"));
            let this = self.clone();
            settings.connect_clicked(move |_| {
                crate::ui::chart_settings::ChartSettings::present_indicator(&this, id);
            });

            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.add_css_class("legend-button");
            remove.set_tooltip_text(Some("Remove"));
            let this = self.clone();
            let owner = pane.clone();
            remove.connect_clicked(move |_| {
                let kept: Vec<Indicator> =
                    owner.indicators.borrow().iter().filter(|i| i.id != id).cloned().collect();
                this.change_indicators_of(&owner, kept);
            });

            let row = crate::ui::pane::legend_row(&dot, &label);
            row.set_widget_name(&legend_row_name(id));
            if pane.view.picked() == Some(id) {
                row.add_css_class("selected");
            }
            row.append(&toggle);
            row.append(&settings);
            row.append(&remove);
            pane.indicator_legend.append(&row);
        }
    }

    /// Mark the legend row of the indicator selected on a chart, and only
    /// that one.
    fn show_picked_in_legend(&self, pane: &ChartPane, picked: Option<u32>) {
        let mut child = pane.indicator_legend.first_child();
        while let Some(row) = child {
            let selected = picked.is_some_and(|id| row.widget_name() == legend_row_name(id));
            if selected {
                row.add_css_class("selected");
            } else {
                row.remove_css_class("selected");
            }
            if let Some(dot) = row.first_child() {
                dot.queue_draw();
            }
            child = row.next_sibling();
        }
    }

    /// What one indicator wants doing, from the chart: its menu, its
    /// properties, its removal, or the legend to say it is selected.
    fn indicator_event(self: &Rc<Self>, id: u32, event: crate::ui::chart::IndicatorEvent) {
        use crate::ui::chart::IndicatorEvent;
        let Some(pane) = self.panes.borrow().iter().find(|p| p.id == id).cloned() else { return };
        match event {
            IndicatorEvent::Picked(picked) => self.show_picked_in_legend(&pane, picked),
            IndicatorEvent::Menu(indicator, x, y) => {
                self.focus(id);
                self.indicator_menu(indicator, x, y);
            }
            IndicatorEvent::Properties(indicator) => {
                self.focus(id);
                crate::ui::chart_settings::ChartSettings::present_indicator(self, indicator);
            }
            IndicatorEvent::Remove(indicator) => self.remove_indicator(&pane, indicator),
            IndicatorEvent::StepWidth(indicator, thicker) => {
                let mut indicators = pane.indicators.borrow().clone();
                let stepped = indicators
                    .iter_mut()
                    .find(|i| i.id == indicator)
                    .is_some_and(|i| i.step_width(thicker));
                if stepped {
                    self.change_indicators_of(&pane, indicators);
                }
            }
        }
    }

    /// Take one indicator off a chart, or off the template it is on.
    fn remove_indicator(self: &Rc<Self>, pane: &Rc<ChartPane>, id: u32) {
        let kept: Vec<Indicator> = pane.indicators.borrow().iter().filter(|i| i.id != id).cloned().collect();
        self.change_indicators_of(pane, kept);
    }

    /// Cut, Copy, Paste and Remove, in the order and the one section GTK's
    /// own context menus give Cut, Copy, Paste and Delete. Paste only while there
    /// is something to put down. Remove is this selection's, and not every
    /// drawing or indicator: clearing the chart is a thing done to the chart,
    /// in the chart's own menu. Plain words, since the menu is already about
    /// what is selected.
    fn clipboard_section(&self, remove: &str) -> gio::Menu {
        let section = gio::Menu::new();
        shortcuts::append_with_key(&section, "Cut", "chart.cut", "<Ctrl>x");
        shortcuts::append_with_key(&section, "Copy", "chart.copy", "<Ctrl>c");
        if self.has_copied() {
            shortcuts::append_with_key(&section, "Paste", "chart.paste", "<Ctrl>v");
        }
        shortcuts::append_with_key(&section, "Remove", remove, "Delete");
        section
    }

    /// The menu of the indicator selected on the focused chart, laid out as a
    /// drawing's is: copying and removing it, then its properties, last and
    /// alone. What only some indicators can do goes above, as a drawing's own
    /// errands do.
    fn indicator_menu(self: &Rc<Self>, indicator: u32, x: f64, y: f64) {
        let pane = self.focused_pane();
        *self.menu_indicator.borrow_mut() = Some(indicator);
        let params = pane.indicators.borrow().iter().find(|i| i.id == indicator).map(|i| i.params.clone());

        let menu = gio::Menu::new();
        let mut rows: Vec<(String, gtk::Widget)> = Vec::new();

        // The value most often changed, as a row of the usual answers: one
        // click, where the properties are a window and a field.
        let presets = gio::Menu::new();
        match &params {
            Some(Params::MovingAverage { period }) => {
                let item = gio::MenuItem::new(Some("Period"), None);
                item.set_attribute_value("custom", Some(&"period".to_variant()));
                presets.append_item(&item);
                let choices = AVERAGE_PERIODS.map(|n| (n.to_string(), (n as i32).to_variant(), n == *period));
                rows.push(("period".to_string(), choice_strip(&choices, "chart.indicator-period", "Period")));
            }
            Some(Params::Vwap { reset, .. } | Params::VolumeProfile { reset, .. }) => {
                reset_strip(&pane, &presets, &mut rows, Some(*reset), false);
            }
            Some(Params::Cvd { reset, .. }) => reset_strip(&pane, &presets, &mut rows, *reset, true),
            _ => {}
        }
        if !rows.is_empty() {
            menu.append_section(None, &presets);
        }

        // A composite is made one profile at a time, from the one under the
        // pointer back into the one before it.
        if matches!(params, Some(Params::VolumeProfile { .. })) {
            let spot = pane.view.profile_at(indicator, x);
            let profile = gio::Menu::new();
            if spot.as_ref().is_some_and(|s| s.has_previous) {
                profile.append(Some("Merge with previous profile"), Some("chart.profile-merge"));
            }
            if spot.as_ref().is_some_and(|s| s.parts.len() > 1) {
                profile.append(Some("Split composite profile"), Some("chart.profile-split"));
            }
            if profile.n_items() > 0 {
                menu.append_section(None, &profile);
            }
            *self.menu_profile.borrow_mut() = spot;
        }

        menu.append_section(None, &self.clipboard_section("chart.indicator-delete"));

        let properties = gio::Menu::new();
        shortcuts::append_with_key(&properties, "Properties…", "chart.indicator-settings", "<Alt>Return");
        menu.append_section(None, &properties);

        let area = pane.view.area.clone();
        let (wx, wy) = area.translate_coordinates(&self.window, x, y).unwrap_or((x, y));
        popup_menu_with(&menu, &self.window, wx, wy, rows);
    }

    /// Change one indicator of the focused chart, wherever its indicators
    /// are kept.
    fn edit_indicator(self: &Rc<Self>, id: u32, edit: impl FnOnce(&mut Params)) {
        let pane = self.focused_pane();
        let mut all = pane.indicators.borrow().clone();
        let Some(indicator) = all.iter_mut().find(|i| i.id == id) else { return };
        edit(&mut indicator.params);
        self.change_indicators_of(&pane, all);
    }

    /// Merge the profile the menu was opened over into the one before it, or
    /// split the composite it is back into its periods. The chart's alone,
    /// and for the symbol it is showing.
    fn change_profile_merges(self: &Rc<Self>, id: u32, spot: &crate::ui::chart::ProfileSpot, merge: bool) {
        let pane = self.focused_pane();
        let Some(instrument) = pane.instrument.borrow().clone() else { return };
        let reset = pane.indicators.borrow().iter().find(|i| i.id == id).and_then(|i| match i.params {
            Params::VolumeProfile { reset, .. } => Some(reset.effective_for(pane.timeframe.get())),
            _ => None,
        });
        let Some(reset) = reset else { return };
        {
            let mut merges = pane.profile_merges.borrow_mut();
            let at = match merges.iter().position(|m| m.is(id, &instrument, reset)) {
                Some(at) => at,
                None => {
                    merges.push(omacharts_engine::indicators::profile::Merge {
                        indicator: id,
                        symbol: instrument.symbol.clone(),
                        suffix: instrument.suffix.clone(),
                        reset,
                        starts: Vec::new(),
                    });
                    merges.len() - 1
                }
            };
            let starts = &mut merges[at].starts;
            match merge {
                true => {
                    if let Some(first) = spot.parts.first()
                        && !starts.contains(first)
                    {
                        starts.push(*first);
                        starts.sort_unstable();
                    }
                }
                false => starts.retain(|start| !spot.parts[1..].contains(start)),
            }
            merges.retain(|m| !m.starts.is_empty());
        }
        let instrument = pane.instrument.borrow().clone();
        if let Some(instrument) = instrument {
            let timeframe = pane.timeframe.get();
            let drawn = pane.view.with_bars(|bars| self.drawn_indicators(&pane, &instrument, timeframe, bars));
            pane.view.set_indicators(drawn);
        }
        self.save_workspace();
    }

    /// The active theme, for anything that needs to resolve a colour.
    pub fn theme(&self) -> omacharts_engine::Theme {
        self.theming.borrow().theme()
    }

    /// The bar scheme the focused chart is painted in.
    pub fn bar_scheme(&self) -> omacharts_engine::BarScheme {
        pane_scheme(&self.focused_pane(), &self.theme())
    }

    /// The focused chart's trading hours.
    pub fn session(&self) -> Session {
        self.focused_pane().session.get()
    }

    pub fn set_session(self: &Rc<Self>, session: Session) {
        let pane = self.focused_pane();
        // Other hours are another series to a strip: a cumulative delta
        // counted over them is nowhere near the range it was held at.
        if pane.session.replace(session) != session {
            pane.view.free_strip_windows();
        }
        self.save_workspace();
    }

    /// How the focused chart joins a futures root's months at the rolls.
    pub fn continuous(&self) -> omacharts_engine::continuous::Adjustment {
        self.focused_pane().continuous.get()
    }

    /// Join the focused chart's months back-adjusted or as traded: another
    /// series, asked for afresh, so the chart is drawn again from it.
    pub fn set_continuous(self: &Rc<Self>, adjustment: omacharts_engine::continuous::Adjustment) {
        let pane = self.focused_pane();
        if pane.continuous.replace(adjustment) == adjustment {
            return;
        }
        pane.view.free_strip_windows();
        self.save_workspace();
        self.redraw_current();
    }

    /// Is there a choice of how the focused chart's months are joined: a
    /// futures root on a feed that stitches its months, or that has its own
    /// series both ways?
    pub fn offers_continuous(&self) -> bool {
        let pane = self.focused_pane();
        let Some(instrument) = pane.instrument.borrow().clone() else { return false };
        self.route_pane(&pane, &instrument)
            .is_some_and(|routed| omacharts_engine::continuous::offers_choice(routed.feed.provider.as_ref(), &instrument))
    }

    /// Whether the focused chart is drawing gridlines.
    ///
    /// Read off the chart rather than off the setting, because the setting is
    /// only what a chart opens with: two charts in a split are entitled to
    /// disagree, and the one being asked about is the focused one.
    pub fn show_grid(&self) -> bool {
        self.focused_pane().show_grid.get()
    }

    /// The one way the gridlines are turned on and off, so the key, the chart
    /// menu and the settings dialog cannot come to different conclusions.
    pub fn set_show_grid(self: &Rc<Self>, show: bool) {
        let pane = self.focused_pane();
        pane.show_grid.set(show);
        pane.view.set_show_grid(show);
        // The menu's tick, for when the answer came from the key or from the
        // settings dialog rather than from the menu itself.
        self.sync_chart_actions(&pane);
        // Written down with the rest of the arrangement, or the chart comes
        // back tomorrow with the gridlines it had yesterday.
        self.save_workspace();
    }

    /// Whether the focused chart draws its indicators.
    pub fn indicators_shown(&self) -> bool {
        self.focused_pane().view.show_indicators()
    }

    /// Hide every indicator on the focused chart, or bring them back. This
    /// chart only, and not what the next one opens with: a chart that came
    /// up bare would read as one that lost its indicators.
    pub fn set_show_indicators(self: &Rc<Self>, show: bool) {
        let pane = self.focused_pane();
        pane.view.set_show_indicators(show);
        // The legend keeps their names, faded, so the chart still says what
        // it would draw.
        self.rebuild_legend_of(&pane);
        self.sync_chart_actions(&pane);
        self.save_workspace();
    }

    /// Whether the focused chart draws its indicator legend.
    pub fn shows_legend(&self) -> bool {
        self.focused_pane().shows_legend.get()
    }

    /// The one way the focused chart's legend is turned on and off from the
    /// window. This chart only: the preferences say what the next one opens
    /// with.
    pub fn set_shows_legend(self: &Rc<Self>, show: bool) {
        let pane = self.focused_pane();
        if pane.shows_legend.replace(show) == show {
            return;
        }
        self.rebuild_legend_of(&pane);
        self.save_workspace();
    }

    /// Whether the focused chart draws its drawings.
    pub fn drawings_shown(&self) -> bool {
        self.focused_pane().view.show_drawings()
    }

    /// Hide every drawing on the focused chart, or bring them back: this
    /// chart only, like its indicators. Nothing is removed, here or on the
    /// other charts of the symbol.
    pub fn set_drawings_shown(self: &Rc<Self>, show: bool) {
        let pane = self.focused_pane();
        pane.view.set_show_drawings(show);
        self.sync_chart_actions(&pane);
        self.sync_drawing_bar();
        self.save_workspace();
    }

    pub fn auto_scale(&self) -> bool {
        self.focused_pane().view.price_auto()
    }

    /// The one way the price scale is put on or off automatic from outside
    /// the chart — the axis menu, the settings dialog — so they cannot come
    /// to different conclusions. A drag on the axis itself arrives through
    /// `price_auto_changed` instead.
    pub fn set_auto_scale(self: &Rc<Self>, auto: bool) {
        let pane = self.focused_pane();
        pane.view.set_price_auto(auto);
        self.sync_chart_actions(&pane);
        self.save_workspace();
    }

    pub fn log_scale(&self) -> bool {
        self.focused_pane().view.price_log()
    }

    /// The one way the price axis is made logarithmic or linear from inside
    /// the window — the axis menu, the settings dialog.
    pub fn set_log_scale(self: &Rc<Self>, log: bool) {
        let pane = self.focused_pane();
        pane.view.set_price_log(log);
        self.sync_chart_actions(&pane);
        self.save_workspace();
    }

    /// How many rows the profile with this id is drawing right now.
    pub fn profile_rows(&self, id: u32) -> Option<usize> {
        self.focused_pane().view.profile_rows(id)
    }

    pub fn bar_style(&self) -> BarStyle {
        self.focused_pane().bar_style.get()
    }

    /// Whether the focused chart's bars are hidden.
    pub fn bars_hidden(&self) -> bool {
        self.focused_pane().bars_hidden.get()
    }

    /// The one way the bars are hidden and shown, so the key, the chart menu
    /// and the settings dialog cannot come to different conclusions. The
    /// style is left alone: showing them again is showing them as they were.
    pub fn set_bars_hidden(self: &Rc<Self>, hidden: bool) {
        let pane = self.focused_pane();
        pane.bars_hidden.set(hidden);
        pane.view.set_bars_hidden(hidden);
        self.sync_chart_actions(&pane);
        self.save_workspace();
    }

    /// Choosing a style is choosing to see it, so it shows hidden bars.
    pub fn set_bar_style(self: &Rc<Self>, style: BarStyle) {
        let pane = self.focused_pane();
        pane.bar_style.set(style);
        pane.view.set_bar_style(style);
        pane.bars_hidden.set(false);
        pane.view.set_bars_hidden(false);
        self.sync_chart_actions(&pane);
        let theme = self.theme();
        pane.view.restyle(theme.clone(), pane_scheme(&pane, &theme));
        self.save_workspace();
    }

    pub fn bar_colouring(&self) -> BarColouring {
        self.focused_pane().colouring.get()
    }

    pub fn set_bar_colouring(self: &Rc<Self>, colouring: BarColouring) {
        let pane = self.focused_pane();
        pane.colouring.set(colouring);
        let theme = self.theme();
        pane.view.restyle(theme.clone(), pane_scheme(&pane, &theme));
        self.save_workspace();
    }

    /// Whether the focused chart wears its colouring the other way round.
    pub fn bars_swapped(&self) -> bool {
        self.focused_pane().swapped.get()
    }

    pub fn set_bars_swapped(self: &Rc<Self>, swapped: bool) {
        let pane = self.focused_pane();
        pane.swapped.set(swapped);
        let theme = self.theme();
        pane.view.restyle(theme.clone(), pane_scheme(&pane, &theme));
        // Profit and loss turn round with the candles.
        self.apply_marks_to(&pane);
        self.save_workspace();
    }

    /// The focused chart's title, empty for none.
    pub fn chart_title(&self) -> String {
        self.focused_pane().title.borrow().clone()
    }

    /// Title the focused chart, or take its title away with a blank one.
    pub fn set_chart_title(self: &Rc<Self>, title: &str) {
        let id = self.focused_pane().id;
        self.set_title_of(id, title);
    }

    /// Title chart `id`, on its header and in the arrangement, and tell its
    /// settings if they are open.
    pub fn set_title_of(self: &Rc<Self>, id: u32, title: &str) {
        let Some(pane) = self.pane(id) else { return };
        let title = clean_title(title);
        if *pane.title.borrow() == title {
            return;
        }
        pane.write_title(&title);
        self.save_workspace();
        self.chart_changed_elsewhere();
    }

    /// F2, on the focused chart: with a drawing selected, its text, as it
    /// always was (the chart caught that first when it had the keyboard;
    /// this is the same from anywhere else); with none, the chart title.
    pub fn press_f2(self: &Rc<Self>) {
        let view = self.focused_pane().view.clone();
        match view.has_selection() {
            true => {
                view.edit_selected_text();
            }
            false => self.edit_chart_title(),
        }
    }

    /// The focused chart's title typed where the header shows it, or
    /// asked for in the dialog when the header leaves it out or has no room.
    pub fn edit_chart_title(self: &Rc<Self>) {
        let pane = self.focused_pane();
        match pane.title_in_view() {
            true => pane.begin_title_edit(),
            false => self.ask_chart_title(),
        }
    }

    /// Edit chart title…: the focused chart's title, asked for in a dialog the
    /// way a chartbook template's name is, where blank takes the title away.
    pub fn ask_chart_title(self: &Rc<Self>) {
        let id = self.focused_pane().id;
        let this = self.clone();
        crate::ui::preferences::ask_title(&self.window, &self.chart_title(), move |chosen| {
            this.set_title_of(id, &chosen)
        });
    }

    /// How many pixels wide the focused chart draws its OHLC bars.
    pub fn ohlc_thickness(&self) -> u8 {
        self.focused_pane().ohlc_thickness.get()
    }

    pub fn set_ohlc_thickness(self: &Rc<Self>, thickness: u8) {
        self.focused_pane().set_ohlc_thickness(thickness);
        self.save_workspace();
    }

    /// Whether the focused chart draws a rising body as an outline.
    pub fn bars_hollow_up(&self) -> bool {
        self.focused_pane().hollow_up.get()
    }

    pub fn set_bars_hollow_up(self: &Rc<Self>, hollow_up: bool) {
        let pane = self.focused_pane();
        pane.hollow_up.set(hollow_up);
        let theme = self.theme();
        pane.view.restyle(theme.clone(), pane_scheme(&pane, &theme));
        self.save_workspace();
    }

    /// Put the focused chart back to what a new chart opens with.
    /// The template is chosen the way any other choice of it is, asking first
    /// when that would drop indicators of the chart's own; `done` runs once
    /// that is settled either way.
    pub fn reset_chart_to_defaults(self: &Rc<Self>, done: impl Fn() + 'static) {
        let defaults = ChartDefaults::load(&self.store);
        let pane = self.focused_pane();
        self.adopt_looks(&pane, &defaults);
        self.sync_chart_actions(&pane);
        self.redraw_current();
        self.save_workspace();
        self.choose_template(defaults.template, done);
    }

    /// Every chart, in every chartbook, on the defaults, at once: there is
    /// nothing to ask, since it never touches a chart's indicators.
    ///
    /// Everything of the defaults but the template. A chart's template, its
    /// indicators and all that is kept with them, down to the strips each is
    /// held at, are its own and stay as they are; the template a new chart
    /// opens on is for new charts only.
    pub fn apply_chart_defaults(self: &Rc<Self>) {
        let defaults = ChartDefaults::load(&self.store);
        self.apply_chart_defaults_now(&defaults);
    }

    fn apply_chart_defaults_now(self: &Rc<Self>, defaults: &ChartDefaults) {
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            self.adopt_chart_settings(pane, defaults);
        }
        // The books not on screen are only written down, so they are changed
        // where they are written.
        let active = self.active.get();
        for (at, book) in self.books.borrow_mut().iter_mut().enumerate() {
            if at == active {
                continue;
            }
            for stored in &mut book.panes {
                stored.bar_style = defaults.bar_style.key().to_string();
                stored.bars_hidden = false;
                stored.colouring = Some(defaults.colouring.key().to_string());
                stored.swapped = Some(defaults.swapped);
                stored.hollow_up = Some(defaults.hollow_up);
                stored.ohlc_thickness = Some(defaults.ohlc_thickness);
                stored.session = defaults.session.key().to_string();
                stored.continuous = Some(defaults.continuous.key().to_string());
                stored.show_grid = defaults.show_grid;
                stored.log_scale = defaults.log_scale;
                stored.shows_drawings = defaults.shows_drawings;
                stored.shows_legend = defaults.shows_legend;
                // Not sending: turning it on or off moves a chart's drawings
                // between it and its symbol, which only a chart on screen
                // can do. It takes the default when its book is next open
                // and somebody applies again.
            }
        }
        self.sync_chart_actions(&self.focused_pane());
        self.redraw_current();
        self.save_workspace();
    }

    /// Everything of the defaults but the template, for a chart being put
    /// back on what a new chart opens with: other hours let go of the strips
    /// held, as `set_session` does.
    fn adopt_looks(self: &Rc<Self>, pane: &Rc<ChartPane>, defaults: &ChartDefaults) {
        let other_hours =
            pane.session.get() != defaults.session || pane.continuous.get() != defaults.continuous;
        self.adopt_chart_settings(pane, defaults);
        if other_hours {
            pane.view.free_strip_windows();
        }
    }

    /// A chart's own settings from the defaults, drawn as it now is, and
    /// nothing of its indicators: the legend's switch is the chart's, read
    /// over them. Drawings move the way the switches in its own settings
    /// move them.
    fn adopt_chart_settings(self: &Rc<Self>, pane: &Rc<ChartPane>, defaults: &ChartDefaults) {
        pane.bar_style.set(defaults.bar_style);
        pane.view.set_bar_style(defaults.bar_style);
        pane.bars_hidden.set(false);
        pane.view.set_bars_hidden(false);
        pane.colouring.set(defaults.colouring);
        pane.swapped.set(defaults.swapped);
        pane.hollow_up.set(defaults.hollow_up);
        pane.set_ohlc_thickness(defaults.ohlc_thickness);
        let theme = self.theme();
        pane.view.restyle(theme.clone(), pane_scheme(pane, &theme));
        self.apply_marks_to(pane);
        pane.session.set(defaults.session);
        pane.continuous.set(defaults.continuous);
        pane.show_grid.set(defaults.show_grid);
        pane.view.set_show_grid(defaults.show_grid);
        pane.view.set_price_log(defaults.log_scale);
        self.set_pane_shows_drawings(pane, defaults.shows_drawings);
        self.set_pane_sends_drawings(pane, defaults.sends_drawings);
        if pane.shows_legend.replace(defaults.shows_legend) != defaults.shows_legend {
            self.rebuild_legend_of(pane);
        }
    }

    pub fn measurement_zone(&self) -> chrono_tz::Tz {
        let pane = self.focused_pane();
        crate::clock::zone(&self.store).for_instrument(pane.instrument.borrow().as_ref())
    }

    /// The focused chart's volume profiles, for a command: the chart they
    /// are on, and each profile with whether it is on screen.
    pub fn profiles_on_focused(&self, id: Option<u32>) -> Result<(String, Vec<(omacharts_engine::indicators::Profile, bool)>), String> {
        let pane = self.focused_pane();
        let label = self.chart_named(&pane);
        let indicator = self.profile_indicator(&pane, &label, id)?;
        let profiles = pane.view.profiles(indicator).unwrap_or_default();
        Ok((label, profiles))
    }

    /// The one volume profile on a chart, or the one `id` names.
    fn profile_indicator(&self, pane: &ChartPane, label: &str, id: Option<u32>) -> Result<u32, String> {
        let profiles: Vec<u32> = pane
            .indicators
            .borrow()
            .iter()
            .filter(|i| matches!(i.params, Params::VolumeProfile { .. }))
            .filter(|i| id.is_none_or(|id| i.id == id))
            .map(|i| i.id)
            .collect();
        match profiles.as_slice() {
            [one] => Ok(*one),
            [] => Err(format!("{label} has no volume profile")),
            many => {
                let ids: Vec<String> = many.iter().map(|id| id.to_string()).collect();
                Err(format!("{label} has {} volume profiles; say which with --id {}", many.len(), ids.join(" or ")))
            }
        }
    }

    /// Merge the focused chart's volume profile holding `ts` into the one
    /// before it, or split the composite holding it, for a command. The
    /// window's to answer, since only it has the bars that say where a
    /// period begins.
    pub fn change_profile_at(self: &Rc<Self>, id: Option<u32>, ts: i64, merge: bool) -> Result<String, String> {
        let pane = self.focused_pane();
        let label = self.chart_named(&pane);
        let indicator = self.profile_indicator(&pane, &label, id)?;
        let Some(spot) = pane.view.profile_of(indicator, ts) else {
            return Err(format!("{label} draws no profile at that moment"));
        };
        match merge {
            true if !spot.has_previous => return Err(format!("that is the first profile on {label}; there is none before it")),
            false if spot.parts.len() < 2 => return Err(format!("that profile on {label} is not a composite")),
            _ => {}
        }
        self.change_profile_merges(indicator, &spot, merge);
        Ok(label)
    }

    /// A temporary measurement belongs to the live focused chart, never the store.
    pub fn measure(
        &self,
        anchors: Option<(omacharts_engine::Anchor, omacharts_engine::Anchor)>,
    ) -> (String, Option<omacharts_engine::measurement::Measurement>) {
        let pane = self.focused_pane();
        let label = self.chart_named(&pane);
        let result = pane.view.measure(anchors);
        self.sync_drawing_bar();
        (label, result)
    }

    /// Right-clicking the chart offers the things you change most, and a way
    /// to everything else.
    /// A picture of what is on screen, for a command.
    ///
    /// A window method because only a window has pixels: the stored
    /// arrangement describes charts, and a screenshot is of the ones being
    /// drawn. Always the focused chart, or the whole of the open chartbook —
    /// a chart in a book that is not on screen has nothing to photograph.
    pub fn screenshot(
        self: &Rc<Self>,
        whole_book: bool,
        into: Option<&std::path::Path>,
        clipboard: bool,
    ) -> Result<(String, std::path::PathBuf), String> {
        let theme = self.theming.borrow().theme();
        let book = self.book_label(self.active.get());
        if whole_book {
            let charts = self.panes.borrow().len();
            let path = screenshot::write_chartbook(
                &self.store,
                &self.chart_host,
                &book,
                &theme,
                into,
                clipboard,
            )?;
            return Ok((format!("chartbook \"{book}\", {charts} charts"), path));
        }
        let pane = self.focused_pane();
        let symbol = pane
            .instrument
            .borrow()
            .as_ref()
            .map(|instrument| instrument.display_symbol())
            .unwrap_or_default();
        let at = self.layout.borrow().leaves().iter().position(|leaf| *leaf == pane.id);
        let path = screenshot::write_chart(&self.store, &pane, &theme, into, clipboard)?;
        let what = format!(
            "pos:{} {symbol} {} in \"{book}\"",
            at.unwrap_or(0),
            pane.timeframe.get().label()
        );
        Ok((what, path))
    }

    /// Show `range` on the focused chart, at `forced` when it is given, else
    /// at its resolution when the range fits it and at the nearest on the
    /// strip that does when not. A day on its own keeps the length of time
    /// on screen, and the resolution with it. Answers for a command, in the
    /// manual's words: which of the three things it was, the days it comes
    /// to, the chart it acted on, and the resolution when that moved:
    /// `span: year to date on pos:1 NVDA 5m in "Manual", now 1D`.
    pub fn show_range_at(self: &Rc<Self>, range: omacharts_engine::Range, forced: Option<Timeframe>) -> String {
        use omacharts_engine::Range;
        let pane = self.focused_pane();
        let today = chrono::Local::now().date_naive();
        let range = self.as_shown(range).pinned(today);
        let current = pane.timeframe.get();
        let target = self.plan_range(range, forced).timeframe;
        // The chart as it was, so the answer names what the command found.
        let acted_on = self.chart_named(&pane);
        // Held before the resolution changes, so the series that arrives for
        // the new one is laid across the range rather than at rest.
        pane.view.show_range(range);
        if target != current {
            self.set_timeframe(target);
        }
        let (kind, kept) = match range {
            Range::Day(_) | Range::Lasting(..) => ("date", " (window kept)"),
            Range::Slice(..) => ("custom range", ""),
            _ => ("span", ""),
        };
        let moved = match target != current {
            true => format!(", now {}", target.label()),
            false => String::new(),
        };
        format!("{kind}: {}{kept} on {acted_on}{moved}", range.spelled(today))
    }

    /// `range` as the focused chart would show it: a day on its own lasts
    /// as long as the chart shows now, from the start of that day or up to
    /// now when that comes first, and anything else is as it says.
    pub fn as_shown(&self, range: omacharts_engine::Range) -> omacharts_engine::Range {
        let view = &self.focused_pane().view;
        match view.seconds_shown().zip(view.now()) {
            Some((seconds, now)) => range.lasting(seconds, chrono::Local::now().date_naive(), now),
            None => range,
        }
    }

    /// The resolution `range` would show at on the focused chart — `forced`,
    /// or the window's pick — and how many bars that makes.
    pub fn plan_range(&self, range: omacharts_engine::Range, forced: Option<Timeframe>) -> crate::ui::range_card::Plan {
        use omacharts_engine::range::{bars_in, resolution_for, Fit, AIM_BARS};
        let pane = self.focused_pane();
        let today = chrono::Local::now().date_naive();
        let range = self.as_shown(range);
        let current = pane.timeframe.get();
        let instrument = pane.instrument.borrow().clone();
        let Some(instrument) = instrument else {
            return crate::ui::range_card::Plan {
                timeframe: forced.unwrap_or(current),
                auto: forced.is_none(),
                fits: true,
            };
        };
        let provider = match self.route_pane(&pane, &instrument) {
            Some(routed) => routed.feed.provider.clone(),
            None => self.provider(),
        };
        let (intraday, daily) = omacharts_engine::session::week_share(&instrument, omacharts_engine::Session::Regular);
        // What the feed actually delivers beats the guess, when the chart
        // already holds intraday bars to measure it on.
        let measured = current.is_intraday().then(|| {
            pane.view.with_bars(|bars| match (bars.first(), bars.last()) {
                (Some(first), Some(last)) if bars.len() > 50 => {
                    let span = (last.ts - first.ts + current.seconds()) as f64;
                    Some(bars.len() as f64 * current.seconds() as f64 / span)
                }
                _ => None,
            })
        });
        let share = (measured.flatten().unwrap_or(intraday), daily);
        let ladder = self.timeframes.borrow().clone();
        // Every feed charts every symbol at every resolution on the strip.
        let offers = |_: Timeframe| true;
        let reach = |tf: Timeframe| {
            let native = tf.native();
            provider.capabilities().iter().find(|c| c.timeframe == native).and_then(|c| c.history_days)
        };
        let fit = Fit { ladder: &ladder, bars: AIM_BARS, share, offers: &offers, reach: &reach };
        // A day on its own keeps the resolution, as it keeps the length of
        // time on screen, unless the feed does not reach back to the day
        // there: the day, not the start of a window moved earlier to end at
        // now, so that moving it never changes the resolution.
        let keeps = match range {
            omacharts_engine::Range::Lasting(day, ..) => {
                match (reach(current), omacharts_engine::Range::Day(day).reach(today)) {
                    (Some(limit), Some(back)) => i64::from(limit) >= back,
                    _ => true,
                }
            }
            _ => false,
        };
        let timeframe = forced.unwrap_or_else(|| match keeps {
            true => current,
            false => resolution_for(range, today, current, &fit),
        });
        // A range from the first bar counts from the first bar the chart has.
        let counted = match range.start(today) {
            Some(_) => range,
            None => match pane.view.first_day() {
                Some(first) => omacharts_engine::Range::Slice(
                    Some(omacharts_engine::range::End::Day(first)),
                    range.end(today).map(omacharts_engine::range::End::Day),
                ),
                None => range,
            },
        };
        let bars = bars_in(counted, today, timeframe, share);
        let reaches = match (reach(timeframe), counted.reach(today)) {
            (Some(limit), Some(back)) => i64::from(limit) >= back,
            _ => true,
        };
        let (fewest, most) = crate::ui::chart::VISIBLE_BARS;
        let readable = bars.is_none_or(|bars| (fewest as f64..=most as f64).contains(&bars.round()));
        crate::ui::range_card::Plan {
            timeframe,
            auto: forced.is_none(),
            fits: offers(timeframe) && reaches && readable,
        }
    }

    /// A chart named the way a command's answer names it:
    /// `pos:0 MES 1D in "Macro"`.
    fn chart_named(&self, pane: &ChartPane) -> String {
        let symbol = pane.instrument.borrow().as_ref().map(|i| i.display_symbol()).unwrap_or_default();
        let at = self.layout.borrow().leaves().iter().position(|leaf| *leaf == pane.id);
        format!(
            "pos:{} {symbol} {} in \"{}\"",
            at.unwrap_or(0),
            pane.timeframe.get().label(),
            self.book_label(self.active.get())
        )
    }

    /// Write a drawing's change down, and hand every chart of the symbol
    /// what is drawn on it now — the one it came from included, which is
    /// how a new drawing learns its id. A local drawing lives with its
    /// chart; a shared one lives in the store under the symbol.
    fn record_drawing(self: &Rc<Self>, pane_id: u32, event: crate::ui::chart::DrawingEvent) {
        use crate::ui::chart::DrawingEvent;
        let Some(pane) = self.pane(pane_id) else { return };
        let instrument = pane.instrument.borrow().clone();
        let Some(instrument) = instrument else { return };
        let suffix = instrument.suffix.as_deref();
        match event {
            DrawingEvent::Moving(drawing) => {
                // Shown to the other charts of the symbol as it moves, and
                // written down by nobody: the release does that.
                if drawing.is_local() {
                    return;
                }
                for other in self.panes.borrow().iter() {
                    let same = other
                        .instrument
                        .borrow()
                        .as_ref()
                        .map(|i| i.symbol == instrument.symbol && i.suffix.as_deref() == suffix)
                        .unwrap_or(false);
                    if same && other.id != pane_id {
                        other.view.follow_moving(&drawing);
                    }
                }
                return;
            }
            DrawingEvent::Added(mut drawing) => {
                // Stamped with the chart that made it, so the chart can
                // take it back if it stops sending.
                drawing.origin = Some(pane.uid.borrow().clone());
                if drawing.is_local() {
                    drawing.id = next_local_id(&pane.local_drawings.borrow());
                    pane.local_drawings.borrow_mut().push(drawing);
                } else {
                    self.store.add_drawing(&instrument.symbol, suffix, &drawing);
                }
            }
            DrawingEvent::Changed(drawing) => {
                // A drawing that changed scope moves house.
                let was_local = drawing.id < 0;
                match (was_local, drawing.is_local()) {
                    (true, true) => {
                        let mut locals = pane.local_drawings.borrow_mut();
                        if let Some(slot) = locals.iter_mut().find(|d| d.id == drawing.id) {
                            *slot = drawing;
                        }
                    }
                    (false, false) => self.store.update_drawing(&drawing),
                    (true, false) => {
                        pane.local_drawings.borrow_mut().retain(|d| d.id != drawing.id);
                        let mut moved = drawing;
                        moved.id = 0;
                        self.store.add_drawing(&instrument.symbol, suffix, &moved);
                    }
                    (false, true) => {
                        self.store.remove_drawing(drawing.id);
                        let mut moved = drawing;
                        moved.id = next_local_id(&pane.local_drawings.borrow());
                        pane.local_drawings.borrow_mut().push(moved);
                    }
                }
            }
            DrawingEvent::Removed(id) => {
                if id < 0 {
                    pane.local_drawings.borrow_mut().retain(|d| d.id != id);
                } else {
                    self.store.remove_drawing(id);
                }
            }
            DrawingEvent::Replaced(list) => {
                // Whatever this chart could see that is not in the list is
                // gone; whatever is in the list is back, under its own id.
                let (locals, shared): (Vec<_>, Vec<_>) = list.into_iter().partition(|d| d.id < 0);
                *pane.local_drawings.borrow_mut() = locals;
                let shows = pane.shows_drawings.get();
                let keep: std::collections::HashSet<i64> = shared.iter().map(|d| d.id).collect();
                for d in self.store.drawings(&instrument.symbol, suffix) {
                    // Only what this chart could see is this chart's to
                    // undo: a chart that is not shown the symbol's drawings
                    // cannot have removed one.
                    if shows && !keep.contains(&d.id) {
                        self.store.remove_drawing(d.id);
                    }
                }
                for d in &shared {
                    self.store.put_drawing(&instrument.symbol, suffix, d);
                }
            }
        }
        self.save_soon();
        self.reload_drawings(&instrument.symbol, suffix);
    }

    /// Show every chart of `symbol` what it should see drawn on it: the
    /// shared drawings its sharing lets through, and its own.
    pub fn reload_drawings(&self, symbol: &str, suffix: Option<&str>) {
        let shared = self.store.drawings(symbol, suffix);
        for pane in self.panes.borrow().iter() {
            let same = pane
                .instrument
                .borrow()
                .as_ref()
                .map(|i| i.symbol == symbol && i.suffix.as_deref() == suffix)
                .unwrap_or(false);
            if same {
                pane.view.set_drawings(self.drawings_for(pane, &shared));
            }
        }
    }

    /// What one chart sees, out of what is drawn on its symbol.
    fn drawings_for(&self, pane: &ChartPane, shared: &[omacharts_engine::Drawing]) -> Vec<omacharts_engine::Drawing> {
        // The symbol's, when this chart is shown what others send; and its
        // own either way, which is the half a toggle never takes away.
        let mut all: Vec<omacharts_engine::Drawing> = match pane.shows_drawings.get() {
            true => shared.to_vec(),
            false => Vec::new(),
        };
        all.extend(pane.local_drawings.borrow().iter().cloned());
        omacharts_engine::drawings::sort_for_painting(&mut all);
        all
    }

    /// The nine configurations of each kind, as stored.
    pub fn drawing_configurations(&self) -> omacharts_engine::Configurations {
        self.store.drawing_configurations()
    }

    /// Write the configurations down and show every chart the new ones, so
    /// a drawing that follows configuration N changes with it, on every
    /// chart, as it is edited.
    pub fn set_drawing_configurations(&self, store: &Store, configs: omacharts_engine::Configurations) {
        store.set_drawing_configurations(&configs);
        for pane in self.panes.borrow().iter() {
            pane.view.set_configurations(configs.clone());
        }
    }

    /// Whether the focused chart is shown what other charts of its symbol
    /// draw. It is shown its drawings again, since the answer to "which do
    /// I see" just changed.
    pub fn set_shows_drawings(self: &Rc<Self>, shows: bool) {
        self.set_pane_shows_drawings(&self.focused_pane(), shows);
        self.save_workspace();
    }

    /// The same for any chart, written down by the caller.
    fn set_pane_shows_drawings(self: &Rc<Self>, pane: &Rc<ChartPane>, shows: bool) {
        pane.shows_drawings.set(shows);
        let instrument = pane.instrument.borrow().clone();
        if let Some(instrument) = instrument {
            self.reload_drawings(&instrument.symbol, instrument.suffix.as_deref());
        }
    }

    /// Whether what is drawn on the focused chart goes to the symbol.
    ///
    /// Live, not only for what is drawn next: turning it off takes back the
    /// drawings this chart sent, and turning it on sends what it has been
    /// keeping. A switch that only governed the future would read as doing
    /// nothing at all — the drawing you were looking at when you flicked it
    /// would sit there on the other chart.
    ///
    /// Only this chart's own move. Each drawing remembers which chart made
    /// it, so another chart's work is never withdrawn by a switch on this
    /// one — and a drawing made before charts had names belongs to none of
    /// them and stays where it is.
    pub fn set_sends_drawings(self: &Rc<Self>, sends: bool) {
        self.set_pane_sends_drawings(&self.focused_pane(), sends);
        self.save_workspace();
    }

    /// The same for any chart, written down by the caller.
    fn set_pane_sends_drawings(self: &Rc<Self>, pane: &Rc<ChartPane>, sends: bool) {
        if pane.sends_drawings.get() == sends {
            return;
        }
        pane.sends_drawings.set(sends);
        pane.view.set_sends(sends);
        let instrument = pane.instrument.borrow().clone();
        if let Some(instrument) = instrument {
            let (symbol, suffix) = (instrument.symbol.clone(), instrument.suffix.clone());
            let suffix = suffix.as_deref();
            let mine = pane.uid.borrow().clone();
            match sends {
                // Out of this chart's keeping and on to the symbol.
                true => {
                    let keeping: Vec<omacharts_engine::Drawing> =
                        pane.local_drawings.borrow_mut().drain(..).collect();
                    for mut drawing in keeping {
                        drawing.id = 0;
                        drawing.scope = omacharts_engine::Scope::Shared;
                        drawing.origin = Some(mine.clone());
                        self.store.add_drawing(&symbol, suffix, &drawing);
                    }
                }
                // Back off the symbol and into this chart's keeping.
                false => {
                    let sent: Vec<omacharts_engine::Drawing> = self
                        .store
                        .drawings(&symbol, suffix)
                        .into_iter()
                        .filter(|d| d.origin.as_deref() == Some(mine.as_str()))
                        .collect();
                    for mut drawing in sent {
                        self.store.remove_drawing(drawing.id);
                        drawing.id = next_local_id(&pane.local_drawings.borrow());
                        drawing.scope = omacharts_engine::Scope::Local;
                        pane.local_drawings.borrow_mut().push(drawing);
                    }
                }
            }
            self.reload_drawings(&symbol, suffix);
        }
    }

    /// Show or hide the drawing tools. Ctrl+Shift+D, and the toggle
    /// bottom-left.
    pub fn toggle_drawing_tools(self: &Rc<Self>) {
        let Some(bar) = self.drawing_bar.borrow().clone() else { return };
        let shown = !bar.is_shown();
        bar.set_shown(shown);
        self.store.set_setting_bool(SHOW_DRAWING_TOOLS, shown);
        self.mark_sidebar("drawing-tools", shown);
        // Putting the tools away drops their toggle onto the bottom-left
        // chart, beside its first date.
        self.sync_corner_clearance();
    }

    /// Tick or clear a sidebar's row in the chart menu: the window's action
    /// for it holds whether it is open.
    fn mark_sidebar(&self, action: &str, shown: bool) {
        if let Some(action) = self.window.lookup_action(action).and_downcast::<gio::SimpleAction>() {
            action.set_state(&shown.to_variant());
        }
    }

    /// Light the tool in hand on the bar, after the chart was told by a key
    /// or a menu rather than by the bar itself.
    fn sync_drawing_bar(&self) {
        if let Some(bar) = self.drawing_bar.borrow().as_ref() {
            let view = &self.focused_pane().view;
            bar.show_armed(view.armed(), view.next_config(), view.is_sticky());
            bar.show_ruler(view.ruler_armed());
        }
    }

    fn open_drawing_configurations(self: &Rc<Self>, kind: omacharts_engine::DrawingKind) {
        crate::ui::drawing_settings::present_configurations(self, &self.store, kind);
    }

    /// Arm a drawing tool on the focused chart. The key is not a toggle:
    /// Alt+R twice is still the rectangle, so a hand that is not sure
    /// whether it pressed can press again. Escape is the way back.
    /// Pick up the ruler on the focused chart, from its key or its button.
    /// Like a drawing tool's key, pressing it again keeps it in hand.
    fn arm_ruler(&self) {
        let pane = self.focused_pane();
        pane.view.arm_ruler();
        pane.view.area.grab_focus();
        self.sync_drawing_bar();
    }

    fn arm_drawing(&self, kind: omacharts_engine::DrawingKind) {
        let pane = self.focused_pane();
        pane.view.arm(Some(kind));
        pane.view.area.grab_focus();
        self.sync_drawing_bar();
    }

    /// Take a copy of whatever is selected on the focused chart: the
    /// indicator, or the drawings. Nothing selected leaves the clipboard as
    /// it was.
    pub fn copy(self: &Rc<Self>) {
        let pane = self.focused_pane();
        if let Some(id) = pane.view.picked() {
            if let Some(indicator) = pane.indicators.borrow().iter().find(|i| i.id == id) {
                *self.clipboard.borrow_mut() = Clipboard::Indicator(indicator.clone());
            }
            return;
        }
        let copied = pane.view.copy_selected();
        if !copied.is_empty() {
            *self.clipboard.borrow_mut() = Clipboard::Drawings(copied);
        }
    }

    /// Copy what is selected, and take it off the chart: the indicator, from
    /// the template on a chart on one, or the drawings, as one step back.
    pub fn cut(self: &Rc<Self>) {
        let pane = self.focused_pane();
        let picked = pane.view.picked();
        let drawings = !pane.view.selected_drawings().is_empty();
        if picked.is_none() && !drawings {
            return;
        }
        self.copy();
        match picked {
            Some(id) => self.remove_indicator(&pane, id),
            None => pane.view.delete_selected(),
        }
    }

    /// Put the copy down on the focused chart. Says whether there was one.
    pub fn paste(self: &Rc<Self>) -> bool {
        let copied = self.clipboard.borrow().clone();
        match copied {
            Clipboard::Empty => false,
            Clipboard::Drawings(drawings) => self.paste_drawings(&drawings),
            Clipboard::Indicator(indicator) => {
                self.paste_indicator(indicator);
                true
            }
        }
    }

    /// Another of the indicator copied, on the focused chart, or on the
    /// template it is on, as adding one there does. Selected once it is
    /// down, so a paste can be followed at once by its properties.
    fn paste_indicator(self: &Rc<Self>, mut indicator: Indicator) {
        let pane = self.focused_pane();
        let mut indicators = pane.indicators.borrow().clone();
        indicator.id = indicators.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let id = indicator.id;
        indicators.push(indicator);
        self.change_indicators_of(&pane, indicators);
        pane.view.area.grab_focus();
        pane.view.pick_when_shown(id);
    }

    /// Put copied drawings down on the focused chart.
    fn paste_drawings(self: &Rc<Self>, copied: &[omacharts_engine::Drawing]) -> bool {
        let pane = self.focused_pane();
        // Offset only where it would otherwise land exactly on what it was
        // copied from. Pasted onto another chart — another symbol, another
        // resolution — the moment and the price are the whole point, and
        // nudging them would be the one thing a paste must not do.
        let same = pane
            .view
            .drawings()
            .iter()
            .any(|there| copied.iter().any(|copy| there.from == copy.from && there.to == copy.to));
        pane.view.paste(copied, same)
    }

    fn has_copied(&self) -> bool {
        !matches!(*self.clipboard.borrow(), Clipboard::Empty)
    }

    fn open_drawing_settings(self: &Rc<Self>) {
        crate::ui::drawing_settings::present(self, &self.store, &self.focused_pane());
    }

    /// The menu a drawing gets when it is right-clicked: about the drawing
    /// and nothing else. The chart's own menu is a right-click on the chart.
    fn drawing_menu(self: &Rc<Self>, x: f64, y: f64) {
        let pane = self.focused_pane();
        let Some(drawing) = pane.view.selected_drawing() else { return };
        // Properties and configurations are a kind's: with lines and
        // rectangles selected together, neither can be asked for.
        let one_kind = pane.view.selection_kind().is_some();
        if let Some(action) = self.drawing_settings_action.borrow().as_ref() {
            action.set_enabled(one_kind);
        }
        if let Some(action) = self.drawing_config_action.borrow().as_ref() {
            action.set_enabled(one_kind);
            action.set_state(&(drawing.config.unwrap_or(0) as i32).to_variant());
        }

        let menu = gio::Menu::new();
        let edit = gio::Menu::new();
        // Above the properties, because it is the more common errand; Enter
        // and F2 do the same.
        // Labelled for what is there — one that already says something is
        // being changed, one that says nothing is being given words.
        let words = match drawing.has_text() {
            true => "Edit text",
            false => "Add text",
        };
        if drawing.kind.takes_text() {
            shortcuts::append_with_key(&edit, words, "chart.drawing-text", "Return");
            menu.append_section(None, &edit);
        }
        menu.append_section(None, &self.clipboard_section("chart.drawing-delete"));

        // The arrowhead, as the four pictures it is, on one row: the three
        // shapes and the one that is no shape. A row of four rather than a
        // submenu of four, because a submenu's rows cannot carry a picture —
        // a custom widget nested inside one is a slot GtkPopoverMenu leaves
        // empty, which is how the Configuration submenu came to open on
        // nothing. At the top level it renders, so this is where it goes.
        let mut rows: Vec<(String, gtk::Widget)> = Vec::new();
        let kind = pane.view.selection_kind();
        // Three rows of pictures in one section — the configuration, the
        // thickness, the arrowhead — each answering "what does this look
        // like" by looking. The last two appear only for the kinds that
        // have the property; the first is on every kind, because every kind
        // follows a configuration.
        let pictures = gio::Menu::new();
        // The first four configurations, as pictures of this kind in each.
        // Above the thickness, because which configuration a drawing
        // follows is the larger choice — it carries the colour, the
        // thickness and everything else — and the row below it is one
        // property of the look it lands on. The other five are Alt+5 to
        // Alt+9 and the picker in the properties; four is what a menu row
        // holds at a size where the picture is still a picture.
        if kind.is_some() {
            let item = gio::MenuItem::new(Some("Configuration"), None);
            item.set_attribute_value("custom", Some(&"configs".to_variant()));
            pictures.append_item(&item);
            rows.push(("configs".to_string(), self.configurations_row(&pane)));
        }
        if kind.is_some_and(|kind| !kind.is_text()) {
            let item = gio::MenuItem::new(Some("Thickness"), None);
            item.set_attribute_value("custom", Some(&"width".to_variant()));
            pictures.append_item(&item);
            rows.push(("width".to_string(), self.width_row(&pane)));
        }
        if kind.is_some_and(|kind| kind.is_line()) {
            let item = gio::MenuItem::new(Some("Arrowhead"), None);
            item.set_attribute_value("custom", Some(&"arrowhead".to_variant()));
            pictures.append_item(&item);
            rows.push(("arrowhead".to_string(), self.arrowhead_row(&pane)));
        }
        if !rows.is_empty() {
            menu.append_section(None, &pictures);
        }

        let edit = gio::Menu::new();
        // No Configuration submenu. Its nine rows were widgets of our own,
        // because a menu item cannot carry a picture, and a custom widget in
        // a *nested* submenu is a slot `GtkPopoverMenu` does not fill — so
        // the submenu opened empty.
        //
        // Not worth fixing, because it was the third way to the same choice
        // and the worst of the three: Alt+1 to Alt+9 does it without a menu
        // at all, and the properties dialog offers the nine as live pictures
        // of this drawing in each, which is more than a swatch in a row
        // could ever have shown.
        //
        // And no "Shown on" either. Where a drawing goes is the chart's
        // business now, in two switches at the foot of its settings, rather
        // than a question asked again of every drawing.
        menu.append_section(None, &edit);

        let order = gio::Menu::new();
        shortcuts::append_with_key(&order, "Bring to front", "chart.drawing-front", "<Ctrl><Shift>Up");
        shortcuts::append_with_key(&order, "Send to back", "chart.drawing-back", "<Ctrl><Shift>Down");
        menu.append_section(None, &order);


        // Last, and alone. Everything above is a thing to do to the drawing;
        // this opens a window about it, which is the heavier choice and the
        // one that should not be sitting under the pointer when the menu
        // opens. A section of its own puts a rule above it that says so.
        let properties = gio::Menu::new();
        shortcuts::append_with_key(&properties, "Properties…", "chart.drawing-settings", "<Alt>Return");
        menu.append_section(None, &properties);

        let area = pane.view.area.clone();
        let (wx, wy) = area.translate_coordinates(&self.window, x, y).unwrap_or((x, y));
        popup_menu_with(&menu, &self.window, wx, wy, rows);
    }

    /// The configurations row: the first four, each drawn as this kind
    /// would look following it, with the one in use ringed.
    ///
    /// The same shape as the two rows below it. A configuration is a thing
    /// you recognise rather than a thing you read — which is why the old
    /// submenu of nine named rows went, and why this is four pictures.
    fn configurations_row(self: &Rc<Self>, pane: &Rc<ChartPane>) -> gtk::Widget {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        row.add_css_class("drawing-head-row");
        row.set_homogeneous(true);
        let theme = self.theme();
        let all = self.drawing_configurations();
        let Some(kind) = pane.view.selection_kind() else { return row.upcast() };
        let following = pane.view.selected_drawing().and_then(|d| d.config);

        for n in 1..=4u8 {
            let picture = crate::ui::drawing_settings::swatch(&theme, kind, all.of(kind, n));
            picture.set_size_request(-1, 20);
            picture.set_hexpand(true);
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("drawing-head-cell");
            button.set_child(Some(&picture));
            button.set_tooltip_text(Some(&format!("Configuration {n} (Alt+{n})")));
            if following == Some(n) {
                button.add_css_class("drawing-preview-current");
            }
            button.set_hexpand(true);
            button.set_action_name(Some("chart.drawing-config"));
            button.set_action_target_value(Some(&(n as i32).to_variant()));
            button.connect_clicked(|button| {
                if let Some(popover) =
                    button.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>()
                {
                    popover.popdown();
                }
            });
            row.append(&button);
        }
        row.upcast()
    }

    /// The thickness row: four strokes, the one in use ringed, and the keys
    /// that step between them off to the side.
    ///
    /// The same shape as the arrowhead row below it, because it answers the
    /// same sort of question — one you settle by looking rather than by
    /// reading a number. The number is still in the properties for anybody
    /// who wants one this row does not offer.
    fn width_row(self: &Rc<Self>, pane: &Rc<ChartPane>) -> gtk::Widget {
        // Homogeneous and expanding, so the four pictures share the row's
        // whole width instead of huddling at the left end of it.
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        row.add_css_class("drawing-head-row");
        row.set_homogeneous(true);
        let current = pane.view.selected_width();

        for width in omacharts_engine::drawings::WIDTHS {
            let picture = gtk::DrawingArea::new();
            picture.set_content_height(20);
            picture.set_hexpand(true);
            picture.set_draw_func(move |area, cr, w, h| {
                let fg = area.color();
                let (r, g, b, a) =
                    (fg.red() as f64, fg.green() as f64, fg.blue() as f64, fg.alpha() as f64);
                cr.set_source_rgba(r, g, b, a);
                cr.set_line_width(width);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                // On the half-pixel for odd widths, on the pixel for even,
                // so a hairline is a hairline rather than two grey rows.
                let y = (h as f64 / 2.0).round()
                    + if (width.round() as i64) % 2 == 1 { 0.5 } else { 0.0 };
                cr.move_to(5.0, y);
                cr.line_to(w as f64 - 5.0, y);
                let _ = cr.stroke();
            });
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("drawing-head-cell");
            button.set_child(Some(&picture));
            button.set_tooltip_text(Some(&format!("{width}px")));
            if current == Some(width) {
                button.add_css_class("drawing-preview-current");
            }
            button.set_hexpand(true);
            button.set_action_name(Some("chart.drawing-width"));
            button.set_action_target_value(Some(&width.to_variant()));
            button.connect_clicked(|button| {
                if let Some(popover) =
                    button.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>()
                {
                    popover.popdown();
                }
            });
            row.append(&button);
        }

        row.upcast()
    }

    /// The arrowhead row: four pictures, the one in use ringed, and the key
    /// that steps between them off to the side.
    fn arrowhead_row(self: &Rc<Self>, pane: &Rc<ChartPane>) -> gtk::Widget {
        use omacharts_engine::{Arrow, ArrowHead};
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        row.add_css_class("drawing-head-row");
        row.set_homogeneous(true);
        let current = pane.view.selected_head();

        for shape in [None, Some(ArrowHead::ALL[0]), Some(ArrowHead::ALL[1]), Some(ArrowHead::ALL[2])] {
            let picture = gtk::DrawingArea::new();
            picture.set_content_height(20);
            picture.set_hexpand(true);
            picture.set_draw_func(move |area, cr, w, h| {
                let fg = area.color();
                let (r, g, b, a) =
                    (fg.red() as f64, fg.green() as f64, fg.blue() as f64, fg.alpha() as f64);
                cr.set_source_rgba(r, g, b, a);
                cr.set_line_width(1.4);
                cr.set_line_cap(gtk::cairo::LineCap::Round);
                let y = (h as f64 / 2.0).round() + 0.5;
                let (tail, tip) = ((5.0, y), (w as f64 - 6.0, y));
                cr.move_to(tail.0, tail.1);
                cr.line_to(tip.0, tip.1);
                let _ = cr.stroke();
                if let Some(shape) = shape {
                    crate::ui::drawing_settings::paint_head(cr, tail, tip, 1.4, 7.0, shape);
                }
            });
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("drawing-head-cell");
            button.set_child(Some(&picture));
            button.set_tooltip_text(Some(match shape {
                None => "No arrowhead",
                Some(shape) => shape.label(),
            }));
            // Ringed rather than ticked: there is no room for a mark beside
            // a picture this size, and the ring is what the configuration
            // tiles already use for "this is the one".
            let chosen = match (current, shape) {
                (Some((Arrow::None, _)), None) => true,
                (Some((arrow, head)), Some(shape)) => arrow != Arrow::None && head == shape,
                _ => false,
            };
            if chosen {
                button.add_css_class("drawing-preview-current");
            }
            button.set_hexpand(true);
            button.set_action_name(Some("chart.drawing-head"));
            button.set_action_target_value(Some(
                &shape.map(|s| s.key()).unwrap_or("none").to_variant(),
            ));
            button.connect_clicked(|button| {
                if let Some(popover) =
                    button.ancestor(gtk::Popover::static_type()).and_downcast::<gtk::Popover>()
                {
                    popover.popdown();
                }
            });
            row.append(&button);
        }

        row.upcast()
    }

    fn chart_menu(self: &Rc<Self>, x: f64, y: f64) {
        let menu = self.chart_menu_model(y);

        // Hung off the window rather than the chart it was opened on: a menu
        // parented to one pane of a split has only that pane's height to fit
        // in, and GTK answers a menu that does not fit by making it scroll.
        let area = self.focused_pane().view.area.clone();
        let (wx, wy) = area
            .translate_coordinates(&self.window, x, y)
            .unwrap_or((x, y));
        let popover = popup_menu(&menu, &self.window, wx, wy);
        self.number_row_picks_templates(&popover);
    }

    /// What the chart's menu holds, opened at height `y` of the focused
    /// chart.
    fn chart_menu_model(self: &Rc<Self>, y: f64) -> gio::Menu {
        let menu = gio::Menu::new();

        // Paste first: it is the one thing in this menu that puts something
        // on the chart rather than changing how the chart is drawn, and it
        // is only here while there is something to put down.
        if self.has_copied() {
            let paste = gio::Menu::new();
            shortcuts::append_with_key(&paste, "Paste", "chart.paste", "<Ctrl>v");
            menu.append_section(None, &paste);
        }

        // Orders at the price under the pointer, with a login to send them
        // through; with none, not a trace.
        let orders = self.order_section(y);
        if let Some(section) = &orders {
            menu.append_section(None, section);
        }
        // Under orders, how the chart is drawn is a section of its own, so
        // a line keeps it apart from them; with none, the menu is as it was.
        let outer = menu.clone();
        let menu = if orders.is_none() { outer.clone() } else { gio::Menu::new() };

        // Not the tools. They were here when the only way to reach them was
        // a key, and there is a bar of them on the chart's own edge now —
        // four more rows naming what four buttons already show is a menu
        // you have to read past to get to what is only here. Clearing is
        // not a tool, and is further down with the rest of the things done
        // to the chart rather than at the head of the menu.

        // Choices live behind a named item rather than loose in the menu: a
        // flat list of radio buttons makes you read every option to find out
        // what the menu is even about.
        let bars = gio::Menu::new();
        for style in BarStyle::ALL {
            let item = gio::MenuItem::new(Some(style.label()), None);
            item.set_action_and_target_value(
                Some("chart.bar-style"),
                Some(&style.key().to_variant()),
            );
            bars.append_item(&item);
        }
        // Last, and carrying the key: the key goes between this and the
        // style above, so it is the row that says what the key does.
        let none = gio::MenuItem::new(Some(NO_BARS_LABEL), None);
        none.set_action_and_target_value(Some("chart.bar-style"), Some(&NO_BARS.to_variant()));
        if let Some(accel) = shortcuts::accel("chart.bars") {
            none.set_attribute_value("accel", Some(&accel.to_variant()));
        }
        bars.append_item(&none);
        menu.append_submenu(Some("Bar style"), &bars);

        let sessions = gio::Menu::new();
        for session in Session::ALL {
            let item = gio::MenuItem::new(Some(session.label()), None);
            item.set_action_and_target_value(
                Some("chart.session"),
                Some(&session.key().to_variant()),
            );
            sessions.append_item(&item);
        }
        menu.append_submenu(Some("Session"), &sessions);

        // Built from the templates as they are now, every time the menu
        // opens, so one made or renamed a moment ago is already here.
        //
        // Each says its key, Ctrl+Alt+Shift and its place in the list, down to
        // the ninth. The keys themselves are taken by where they are on the
        // keyboard, by the window and, while it is open, by the menu below.
        let templates = gio::Menu::new();
        let none = gio::MenuItem::new(Some("None"), None);
        none.set_action_and_target_value(Some("chart.template"), Some(&template_state(None)));
        none.set_attribute_value("accel", Some(&template_accel(0).to_variant()));
        templates.append_item(&none);
        let made = gio::Menu::new();
        for (at, template) in self.templates().into_iter().enumerate() {
            let item = gio::MenuItem::new(Some(&template.name), None);
            item.set_action_and_target_value(
                Some("chart.template"),
                Some(&template_state(Some(template.id))),
            );
            if at < 9 {
                item.set_attribute_value("accel", Some(&template_accel(at + 1).to_variant()));
            }
            made.append_item(&item);
        }
        templates.append_section(None, &made);
        // Below a rule, as the way to one more: what this chart shows, kept
        // under a name. Greyed rather than gone on a chart with nothing to
        // keep, so the menu does not change shape from one chart to the next.
        let more = gio::Menu::new();
        more.append(Some("Create new template from chart\u{2026}"), Some("chart.template-from-chart"));
        templates.append_section(None, &more);
        if let Some(action) = self.template_from_chart_action.borrow().as_ref() {
            action.set_enabled(!self.focused_pane().indicators.borrow().is_empty());
        }
        menu.append_submenu(Some("Template"), &templates);

        // With the other two choices about how the chart is drawn, and drawn
        // as a tick: "Gridlines" as a plain row could not say whether this
        // chart has any.
        shortcuts::append(&menu, "Gridlines", "chart.grid");
        shortcuts::append(&menu, "Indicators", "chart.show-indicators");
        shortcuts::append(&menu, "Drawings", "chart.show-drawings");
        shortcuts::append(&menu, "Edit chart title\u{2026}", "chart.edit-title");
        if orders.is_some() {
            outer.append_section(None, &menu);
        }
        let menu = outer;

        // The layout, in its own section: splitting and closing are about the
        // arrangement rather than about what this chart draws.
        let layout = gio::Menu::new();
        shortcuts::append(&layout, "Split horizontally", "chart.split-h");
        shortcuts::append(&layout, "Split vertically", "chart.split-v");
        // The window's two sidebars, ticked while each is open, with their keys.
        let sidebars = gio::Menu::new();
        shortcuts::append(&sidebars, "Drawing tools", "win.drawing-tools");
        shortcuts::append(&sidebars, "Watchlist", "win.watchlist");
        layout.append_submenu(Some("Sidebars"), &sidebars);
        // Both only mean anything with something else on screen: there is
        // nothing to close down to, and nothing to grow over.
        if self.panes.borrow().len() > 1 {
            let open = self.maximized.get() == Some(self.focused.get());
            let what = if open { pane::RESTORE } else { pane::MAXIMIZE };
            shortcuts::append(&layout, what, "chart.maximize");
            shortcuts::append(&layout, "Close chart", "chart.close");
        }
        menu.append_section(None, &layout);

        let rest = gio::Menu::new();
        // The gear is a header part now, and a header without it still
        // needs a way in: this row, only then, so a header with the gear is
        // not offered the same thing twice.
        if !self.focused_pane().shows(crate::header::Part::Settings) {
            shortcuts::append(&rest, "Chart settings", "chart.settings");
        }
        let modes = gio::Menu::new();
        for mode in link::MODES {
            let item = gio::MenuItem::new(Some(mode.label()), None);
            item.set_action_and_target_value(Some("chart.link-mode"), Some(&mode.key().to_variant()));
            modes.append_item(&item);
        }
        rest.append_submenu(Some("Watchlist"), &modes);
        shortcuts::append(&rest, "Take screenshot", "chart.screenshot");
        if !self.focused_pane().view.drawings().is_empty() {
            shortcuts::append(&rest, "Remove all drawings", "chart.drawing-clear");
        }
        menu.append_section(None, &rest);

        // Last, and together, below a rule: everything above is something
        // done to this chart, and these two open a window about it.
        let settings = gio::Menu::new();
        shortcuts::append(&settings, "Indicators…", "chart.indicators");
        shortcuts::append(&settings, "Chart settings…", "chart.settings");
        menu.append_section(None, &settings);
        menu
    }

    fn number_row_picks_templates(self: &Rc<Self>, popover: &gtk::PopoverMenu) {
        // The template rows' keys are drawn, and GTK also binds what they
        // draw while the menu is open — by what the key types, which on a
        // layout whose number row types other digits with Shift held picks a
        // different template than the same key does with the menu closed.
        // So the menu takes the number row first, by where the key is, the
        // way the window does.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = self.clone();
        let closing = popover.downgrade();
        keys.connect_key_pressed(move |_, _, keycode, state| {
            use gtk::gdk::ModifierType;
            let chord = ModifierType::CONTROL_MASK | ModifierType::ALT_MASK | ModifierType::SHIFT_MASK;
            let Some(n) = number_row(keycode).filter(|_| state.contains(chord)) else {
                return glib::Propagation::Proceed;
            };
            if let Some(popover) = closing.upgrade() {
                popover.popdown();
            }
            this.pick_template(n);
            // Closing hands the keyboard to the window's first button; back
            // to the chart the menu was about, once it has.
            let area = this.focused_pane().view.area.clone();
            glib::idle_add_local_once(move || {
                area.grab_focus();
            });
            glib::Propagation::Stop
        });
        popover.add_controller(keys);
    }

    /// The price axis has its own menu, and its own questions: whether the
    /// scale fits itself to what is on screen, and whether it is logarithmic.
    ///
    /// Shown as a state rather than offered as an action, because it is on
    /// unless you have dragged the axis — "Auto scale price" as a plain item
    /// looked broken, since clicking it usually asked for what was already
    /// happening. Turning it off is what lets the chart be dragged up and down.
    fn price_axis_menu(self: &Rc<Self>, x: f64, y: f64) {
        if let Some(action) = self.auto_scale_action.borrow().as_ref() {
            action.set_state(&self.focused_pane().view.price_auto().to_variant());
        }
        if let Some(action) = self.log_scale_action.borrow().as_ref() {
            action.set_state(&self.focused_pane().view.price_log().to_variant());
        }
        let menu = gio::Menu::new();
        menu.append(Some("Auto scale price"), Some("chart.auto-scale"));
        menu.append(Some("Logarithmic scale"), Some("chart.log-scale"));
        let rest = gio::Menu::new();
        shortcuts::append(&rest, "Reset chart", "chart.reset-view");
        menu.append_section(None, &rest);

        let area = self.focused_pane().view.area.clone();
        let (wx, wy) = area.translate_coordinates(&self.window, x, y).unwrap_or((x, y));
        popup_menu(&menu, &self.window, wx, wy);
    }

    /// The actions the chart's menus drive.
    ///
    /// Stateful actions rather than plain ones, so the menu draws the current
    /// choice as a selected radio item without anybody building ticks by hand.
    fn install_chart_actions(self: &Rc<Self>) {
        let actions = gio::SimpleActionGroup::new();

        let bar_style = gio::SimpleAction::new_stateful(
            "bar-style",
            Some(glib::VariantTy::STRING),
            &bar_style_state(&self.focused_pane()).to_variant(),
        );
        let this = self.clone();
        bar_style.connect_activate(move |_, value| {
            let Some(key) = value.and_then(|v| v.str().map(str::to_string)) else { return };
            if key == NO_BARS {
                this.set_bars_hidden(true);
            } else if let Some(style) = BarStyle::from_key(&key) {
                this.set_bar_style(style);
            }
        });
        actions.add_action(&bar_style);
        *self.bar_style_action.borrow_mut() = Some(bar_style);

        // The indicator menu's two rows, about the indicator it was opened on.
        let delete = gio::SimpleAction::new("indicator-delete", None);
        let this = self.clone();
        delete.connect_activate(move |_, _| {
            if let Some(id) = this.menu_indicator.borrow_mut().take() {
                this.remove_indicator(&this.focused_pane(), id);
            }
        });
        actions.add_action(&delete);
        let settings = gio::SimpleAction::new("indicator-settings", None);
        let this = self.clone();
        settings.connect_activate(move |_, _| {
            if let Some(id) = this.menu_indicator.borrow_mut().take() {
                crate::ui::chart_settings::ChartSettings::present_indicator(&this, id);
            }
        });
        actions.add_action(&settings);

        // The indicator menu's presets and a profile's composites.
        let period = gio::SimpleAction::new("indicator-period", Some(glib::VariantTy::INT32));
        let this = self.clone();
        period.connect_activate(move |_, value| {
            let Some(chosen) = value.and_then(|v| v.get::<i32>()) else { return };
            if let Some(id) = this.menu_indicator.borrow_mut().take() {
                this.edit_indicator(id, |params| {
                    if let Params::MovingAverage { period } = params {
                        *period = chosen as usize;
                    }
                });
            }
        });
        actions.add_action(&period);
        let reset = gio::SimpleAction::new("indicator-reset", Some(glib::VariantTy::STRING));
        let this = self.clone();
        reset.connect_activate(move |_, value| {
            let Some(key) = value.and_then(|v| v.get::<String>()) else { return };
            // `never` is a cumulative delta's alone: one running total.
            let chosen = match key.as_str() {
                "never" => None,
                key => match Reset::from_key(key) {
                    Some(reset) => Some(reset),
                    None => return,
                },
            };
            if let Some(id) = this.menu_indicator.borrow_mut().take() {
                this.edit_indicator(id, |params| match params {
                    Params::Vwap { reset, .. } | Params::VolumeProfile { reset, .. } => {
                        if let Some(chosen) = chosen {
                            *reset = chosen;
                        }
                    }
                    Params::Cvd { reset, .. } => *reset = chosen,
                    _ => {}
                });
            }
        });
        actions.add_action(&reset);
        for (name, merge) in [("profile-merge", true), ("profile-split", false)] {
            let action = gio::SimpleAction::new(name, None);
            let this = self.clone();
            action.connect_activate(move |_, _| {
                let id = this.menu_indicator.borrow_mut().take();
                let spot = this.menu_profile.borrow_mut().take();
                if let (Some(id), Some(spot)) = (id, spot) {
                    this.change_profile_merges(id, &spot, merge);
                }
            });
            actions.add_action(&action);
        }

        // What Ctrl+Alt+Shift+H flips: None and the style the bars had, which
        // hiding them never forgets.
        let bars = gio::SimpleAction::new_stateful("bars", None, &(!self.bars_hidden()).to_variant());
        let this = self.clone();
        bars.connect_activate(move |_, _| this.set_bars_hidden(!this.bars_hidden()));
        actions.add_action(&bars);
        *self.bars_action.borrow_mut() = Some(bars);

        let session = gio::SimpleAction::new_stateful(
            "session",
            Some(glib::VariantTy::STRING),
            &self.session().key().to_variant(),
        );
        let this = self.clone();
        session.connect_activate(move |action, value| {
            let Some(key) = value.and_then(|v| v.str().map(str::to_string)) else { return };
            let Some(chosen) = Session::from_key(&key) else { return };
            action.set_state(&key.to_variant());
            this.set_session(chosen);
            this.redraw_current();
        });
        actions.add_action(&session);
        *self.session_action.borrow_mut() = Some(session);

        // The same choice as the chart's Indicators page offers, asked the
        // same way. The state follows what was actually done, which is not
        // what was clicked when the question is answered with Cancel.
        let template = gio::SimpleAction::new_stateful(
            "template",
            Some(glib::VariantTy::STRING),
            &template_state(self.focused_template()),
        );
        let this = self.clone();
        template.connect_activate(move |_, value| {
            let Some(key) = value.and_then(|v| v.str().map(str::to_string)) else { return };
            this.choose_template(key.parse().ok(), || {});
        });
        actions.add_action(&template);
        *self.template_action.borrow_mut() = Some(template);

        // Stateful over a boolean rather than taking one, because a key has
        // nothing to hand it: Ctrl+Shift+G means the other way round from
        // however this chart is drawn now.
        let grid = gio::SimpleAction::new_stateful("grid", None, &self.show_grid().to_variant());
        let this = self.clone();
        grid.connect_activate(move |_, _| this.set_show_grid(!this.show_grid()));
        actions.add_action(&grid);
        *self.grid_action.borrow_mut() = Some(grid);

        let shown = gio::SimpleAction::new_stateful("show-indicators", None, &self.indicators_shown().to_variant());
        let this = self.clone();
        shown.connect_activate(move |_, _| this.set_show_indicators(!this.indicators_shown()));
        actions.add_action(&shown);
        *self.indicators_action.borrow_mut() = Some(shown);

        let shown = gio::SimpleAction::new_stateful("show-drawings", None, &self.drawings_shown().to_variant());
        let this = self.clone();
        shown.connect_activate(move |_, _| this.set_drawings_shown(!this.drawings_shown()));
        actions.add_action(&shown);
        *self.drawings_action.borrow_mut() = Some(shown);

        let split_h = gio::SimpleAction::new("split-h", None);
        let this = self.clone();
        split_h.connect_activate(move |_, _| this.split_focused(true));
        actions.add_action(&split_h);

        let split_v = gio::SimpleAction::new("split-v", None);
        let this = self.clone();
        split_v.connect_activate(move |_, _| this.split_focused(false));
        actions.add_action(&split_v);

        let maximize = gio::SimpleAction::new("maximize", None);
        let this = self.clone();
        maximize.connect_activate(move |_, _| this.toggle_maximized());
        actions.add_action(&maximize);

        let close = gio::SimpleAction::new("close", None);
        let this = self.clone();
        close.connect_activate(move |_, _| this.close_focused());
        actions.add_action(&close);

        // Stateful over the mode's key, so the menu draws a radio mark beside
        // the one this chart is in.
        let linked = gio::SimpleAction::new_stateful(
            "link-mode",
            Some(glib::VariantTy::STRING),
            &self.focused_pane().mode().key().to_variant(),
        );
        let this = self.clone();
        linked.connect_activate(move |action, target| {
            let Some(mode) = target.and_then(|t| t.get::<String>()).and_then(|k| LinkMode::from_key(&k))
            else {
                return;
            };
            action.set_state(&mode.key().to_variant());
            this.set_pane_mode(this.focused.get(), mode);
        });
        actions.add_action(&linked);
        *self.linked_action.borrow_mut() = Some(linked);

        let auto_scale =
            gio::SimpleAction::new_stateful("auto-scale", None, &self.auto_scale().to_variant());
        let this = self.clone();
        auto_scale.connect_activate(move |_, _| this.set_auto_scale(!this.auto_scale()));
        actions.add_action(&auto_scale);
        *self.auto_scale_action.borrow_mut() = Some(auto_scale);

        let log_scale =
            gio::SimpleAction::new_stateful("log-scale", None, &self.log_scale().to_variant());
        let this = self.clone();
        log_scale.connect_activate(move |_, _| this.set_log_scale(!this.log_scale()));
        actions.add_action(&log_scale);
        *self.log_scale_action.borrow_mut() = Some(log_scale);

        let reset_view = gio::SimpleAction::new("reset-view", None);
        let this = self.clone();
        reset_view.connect_activate(move |_, _| {
            let pane = this.focused_pane();
            pane.view.reset_view();
            // Resetting puts the price scale back on automatic, and that much
            // of it is written down with the chart.
            this.sync_chart_actions(&pane);
            this.save_workspace();
        });
        actions.add_action(&reset_view);

        let settings = gio::SimpleAction::new("settings", None);
        let this = self.clone();
        settings.connect_activate(move |_, _| this.open_chart_settings());
        actions.add_action(&settings);

        let indicators = gio::SimpleAction::new("indicators", None);
        let this = self.clone();
        indicators.connect_activate(move |_, _| this.open_indicators());
        actions.add_action(&indicators);

        let shot = gio::SimpleAction::new("screenshot", None);
        let this = self.clone();
        shot.connect_activate(move |_, _| {
            screenshot::take_chart(
                &this.window,
                &this.store,
                &this.focused_pane(),
                &this.theming.borrow().theme(),
            );
        });
        actions.add_action(&shot);

        let resolutions = gio::SimpleAction::new("edit-resolutions", None);
        let this = self.clone();
        resolutions.connect_activate(move |_, _| this.edit_timeframes());
        actions.add_action(&resolutions);

        let header = gio::SimpleAction::new("edit-header", None);
        let this = self.clone();
        header.connect_activate(move |_, _| this.edit_header());
        actions.add_action(&header);

        let title = gio::SimpleAction::new("edit-title", None);
        let this = self.clone();
        title.connect_activate(move |_, _| this.ask_chart_title());
        actions.add_action(&title);

        // Offered only while the chart has indicators: a template made from
        // none would be an empty one, which the preferences already make.
        let from_chart = gio::SimpleAction::new("template-from-chart", None);
        let this = self.clone();
        from_chart.connect_activate(move |_, _| this.new_template_from_chart());
        actions.add_action(&from_chart);
        *self.template_from_chart_action.borrow_mut() = Some(from_chart);

        let draw_line = gio::SimpleAction::new("draw-line", None);
        let this = self.clone();
        draw_line.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Line)
        });
        actions.add_action(&draw_line);

        let draw_rect = gio::SimpleAction::new("draw-rect", None);
        let this = self.clone();
        draw_rect.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Rect)
        });
        actions.add_action(&draw_rect);

        let draw_hline = gio::SimpleAction::new("draw-hline", None);
        let this = self.clone();
        draw_hline.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Horizontal)
        });
        actions.add_action(&draw_hline);

        let draw_zigzag = gio::SimpleAction::new("draw-zigzag", None);
        let this = self.clone();
        draw_zigzag.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Zigzag)
        });
        actions.add_action(&draw_zigzag);

        let draw_arrow = gio::SimpleAction::new("draw-arrow", None);
        let this = self.clone();
        draw_arrow.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Arrow)
        });
        actions.add_action(&draw_arrow);

        let draw_ellipse = gio::SimpleAction::new("draw-ellipse", None);
        let this = self.clone();
        draw_ellipse.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Ellipse)
        });
        actions.add_action(&draw_ellipse);

        let draw_text = gio::SimpleAction::new("draw-text", None);
        let this = self.clone();
        draw_text.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Text)
        });
        actions.add_action(&draw_text);

        let draw_profile = gio::SimpleAction::new("draw-profile", None);
        let this = self.clone();
        draw_profile.connect_activate(move |_, _| {
            this.arm_drawing(omacharts_engine::DrawingKind::Profile)
        });
        actions.add_action(&draw_profile);

        let draw_fib = gio::SimpleAction::new("draw-fib", None);
        let this = self.clone();
        draw_fib.connect_activate(move |_, _| this.arm_drawing(omacharts_engine::DrawingKind::Fib));
        actions.add_action(&draw_fib);

        let draw_avwap = gio::SimpleAction::new("draw-avwap", None);
        let this = self.clone();
        draw_avwap.connect_activate(move |_, _| this.arm_drawing(omacharts_engine::DrawingKind::Vwap));
        actions.add_action(&draw_avwap);

        let measure = gio::SimpleAction::new("measure", None);
        let this = self.clone();
        measure.connect_activate(move |_, _| this.arm_ruler());
        actions.add_action(&measure);

        let drawing_text = gio::SimpleAction::new("drawing-text", None);
        let this = self.clone();
        drawing_text.connect_activate(move |_, _| {
            this.focused_pane().view.edit_selected_text();
        });
        actions.add_action(&drawing_text);

        let drawing_width = gio::SimpleAction::new("drawing-width", Some(glib::VariantTy::DOUBLE));
        let this = self.clone();
        drawing_width.connect_activate(move |_, target| {
            let Some(width) = target.and_then(|t| t.get::<f64>()) else { return };
            this.focused_pane().view.set_width(width);
        });
        actions.add_action(&drawing_width);

        let drawing_head = gio::SimpleAction::new("drawing-head", Some(glib::VariantTy::STRING));
        let this = self.clone();
        drawing_head.connect_activate(move |_, target| {
            let Some(key) = target.and_then(|t| t.str()) else { return };
            let shape = match key {
                "none" => None,
                key => match omacharts_engine::ArrowHead::from_key(key) {
                    Some(shape) => Some(shape),
                    None => return,
                },
            };
            this.focused_pane().view.set_head(shape);
        });
        actions.add_action(&drawing_head);

        let cut = gio::SimpleAction::new("cut", None);
        let this = self.clone();
        cut.connect_activate(move |_, _| this.cut());
        actions.add_action(&cut);

        let copy = gio::SimpleAction::new("copy", None);
        let this = self.clone();
        copy.connect_activate(move |_, _| this.copy());
        actions.add_action(&copy);

        let paste = gio::SimpleAction::new("paste", None);
        let this = self.clone();
        paste.connect_activate(move |_, _| {
            this.paste();
        });
        actions.add_action(&paste);

        let drawing_settings = gio::SimpleAction::new("drawing-settings", None);
        let this = self.clone();
        drawing_settings.connect_activate(move |_, _| this.open_drawing_settings());
        actions.add_action(&drawing_settings);
        *self.drawing_settings_action.borrow_mut() = Some(drawing_settings);

        let drawing_clear = gio::SimpleAction::new("drawing-clear", None);
        let this = self.clone();
        drawing_clear.connect_activate(move |_, _| this.focused_pane().view.delete_all());
        actions.add_action(&drawing_clear);

        let drawing_delete = gio::SimpleAction::new("drawing-delete", None);
        let this = self.clone();
        drawing_delete.connect_activate(move |_, _| this.focused_pane().view.delete_selected());
        actions.add_action(&drawing_delete);

        // Stateful over the number, so the menu marks the configuration the
        // drawing follows; zero is a drawing with a look of its own.
        let drawing_config =
            gio::SimpleAction::new_stateful("drawing-config", Some(glib::VariantTy::INT32), &0i32.to_variant());
        let this = self.clone();
        drawing_config.connect_activate(move |action, target| {
            let Some(number) = target.and_then(|t| t.get::<i32>()) else { return };
            if this.focused_pane().view.apply_configuration(number.clamp(0, 9) as u8) {
                action.set_state(&number.to_variant());
            }
        });
        actions.add_action(&drawing_config);
        *self.drawing_config_action.borrow_mut() = Some(drawing_config);

        let drawing_front = gio::SimpleAction::new("drawing-front", None);
        let this = self.clone();
        drawing_front.connect_activate(move |_, _| this.focused_pane().view.restack_selected(true));
        actions.add_action(&drawing_front);

        let drawing_back = gio::SimpleAction::new("drawing-back", None);
        let this = self.clone();
        drawing_back.connect_activate(move |_, _| this.focused_pane().view.restack_selected(false));
        actions.add_action(&drawing_back);

        self.window.insert_action_group("chart", Some(&actions));
        *self.trade.chart_actions.borrow_mut() = Some(actions);
    }

    /// One key for the whole watchlist, doing the obvious next thing.
    ///
    /// Closed, open it and put the keyboard on it — the reason to open a
    /// watchlist is almost always to move through it. Open but not focused,
    /// focus it. Open and focused, you are done with it, so close it.
    /// Is the keyboard going into a box somebody is typing in?
    fn is_typing(&self) -> bool {
        gtk::prelude::GtkWindowExt::focus(&self.window)
            .map(|widget| widget.is::<gtk::Editable>() || widget.is::<gtk::TextView>())
            .unwrap_or(false)
    }

    /// Is the rail on screen?
    ///
    /// Asked of the corner button rather than of the rail itself, because a
    /// widget reports itself invisible until every ancestor is on screen —
    /// and a chartbook is written down before the window has been shown,
    /// which recorded the rail as closed on every launch. The button knows
    /// from the moment it is built, and it is already the one thing that
    /// opens and closes the rail.
    fn shows_sidebar(&self) -> bool {
        match self.watchlist_toggle.borrow().as_ref() {
            Some(toggle) => toggle.is_active(),
            None => self.split.end_child().map(|rail| rail.is_visible()).unwrap_or(false),
        }
    }

    fn set_show_sidebar(&self, show: bool) {
        if let Some(rail) = self.split.end_child() {
            rail.set_visible(show);
        }
    }

    /// Show or hide the rail the way a click does.
    ///
    /// Through the corner button rather than around it: the button's handler
    /// is what remembers the choice and what moves a chart's own corner out
    /// from under the window's controls. Setting the rail directly would leave
    /// the button looking unpressed over an open rail.
    fn set_rail_shown(&self, shown: bool) {
        match self.watchlist_toggle.borrow().as_ref() {
            Some(toggle) => toggle.set_active(shown),
            None => self.set_show_sidebar(shown),
        }
    }

    /// The width the window was last left at, for a book that has no opinion
    /// and for a window with nothing written down at all.
    fn stored_sidebar_width(&self) -> i32 {
        self.store
            .setting(SETTING_SIDEBAR_WIDTH)
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(DEFAULT_SIDEBAR_WIDTH)
            .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
    }

    /// Ask for a rail this wide — now if the window has a size, and otherwise
    /// the moment it gets one.
    fn want_sidebar_width(self: &Rc<Self>, width: i32) {
        self.sidebar_width.set(width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH));
        self.place_sidebar();
    }

    /// Move the handle to where the wanted width puts it.
    ///
    /// A width rather than a handle position, because the position is
    /// measured from the other side and would move the rail every time the
    /// window was resized.
    fn place_sidebar(&self) {
        let total = self.split.width();
        let width = self.sidebar_width.get().min(sidebar_room(total));
        if total > width + 200 {
            self.sidebar_placed.set(true);
            self.sidebar_laid.set(width);
            self.split.set_position(total - width);
        }
    }

    /// Follow the handle, and keep what it is dragged to.
    ///
    /// Nothing is recorded until a width has been put back once. A window
    /// that has not been laid out reports a position measured against no
    /// width at all, and recording that overwrote the rail every launch with
    /// whatever the default happened to produce.
    fn watch_sidebar_width(self: &Rc<Self>) {
        self.sidebar_width.set(self.stored_sidebar_width());
        self.place_sidebar();

        let this = self.clone();
        self.split.connect_map(move |_| {
            if !this.sidebar_placed.get() {
                this.place_sidebar();
            }
        });

        let this = self.clone();
        self.split.connect_position_notify(move |paned| {
            if !this.sidebar_placed.get() {
                this.place_sidebar();
                return;
            }
            let total = paned.width();
            if total <= 0 || paned.position() <= 0 {
                return;
            }
            let raw = total - paned.position();
            // The rail keeps its width when the window resizes, so a handle
            // still at the width it was put at means the window moved, not
            // the hand: put back what the rail wants, as far as there is room
            // for it, and remember nothing.
            if raw == this.sidebar_laid.get() {
                if raw != this.sidebar_width.get().min(sidebar_room(total)) {
                    this.place_sidebar();
                }
                return;
            }
            // Dragged: held between the bounds, which is the handle stopping
            // at them rather than the stored width merely being clamped.
            let width = raw.clamp(MIN_SIDEBAR_WIDTH, sidebar_room(total));
            this.sidebar_laid.set(width);
            if width != raw {
                paned.set_position(total - width);
            }
            if width == this.sidebar_width.get() {
                return;
            }
            this.sidebar_width.set(width);
            // Also the window's own, which is what seeds the next chartbook
            // and what a window with nothing written down opens at.
            this.store.set_setting(SETTING_SIDEBAR_WIDTH, &width.to_string());
            this.save_soon();
        });
    }

    /// Walk the rail to the watchlist before or after the one it is showing.
    ///
    /// `false` when the keyboard is not in the rail, so the key falls through
    /// to whatever else wants it. Matching and then doing nothing would eat
    /// Ctrl+Alt+Shift and the arrows everywhere in the app, which is worse
    /// than not having the binding at all.
    fn rotate_watchlist(self: &Rc<Self>, delta: i32) -> bool {
        let rail = self.watchlist.borrow().as_ref().cloned();
        let Some(rail) = rail.filter(|rail| rail.has_focus()) else { return false };
        rail.rotate_watchlist(delta);
        true
    }

    pub fn toggle_watchlist(self: &Rc<Self>) {
        if self.shows_sidebar() {
            self.set_rail_shown(false);
            self.focused_pane().view.area.grab_focus();
            return;
        }
        self.set_rail_shown(true);
        if let Some(watchlist) = self.watchlist.borrow().as_ref().cloned() {
            // The sidebar is not realised until the frame after it is
            // revealed, so focus has to wait for it.
            glib::idle_add_local_once(move || watchlist.grab_focus());
        }
    }



    pub fn search(&self) -> Rc<SymbolSearch> {
        self.search.clone()
    }

    /// The window's own symbol index, once it names everything.
    ///
    /// Lent out so a command running on the main loop can resolve a stored
    /// symbol against the whole catalogue without building a second index of
    /// eleven thousand rows to do it — the window has one already, and
    /// rebuilding it is milliseconds the frame cannot spare.
    ///
    /// `None` until the long tail has been swapped in, because the curated
    /// half would answer the same question with a different and much shorter
    /// truth, and a caller silently resolving against it is the bug this
    /// exists to avoid.
    pub fn symbols(&self) -> Option<Rc<SearchIndex>> {
        self.index.complete().then(|| self.index.get())
    }

    /// Re-fold and repaint, after something that changes how the bars are read.
    pub fn refresh(self: &Rc<Self>) {
        self.redraw_current();
    }

    /// Show a symbol named on the command line, if we know it.
    ///
    /// Takes the canonical symbol and the exchange suffix separately, because
    /// a display symbol cannot be split back apart: BRK.B is one symbol with a
    /// dot in it, SAN.MC is a symbol and a suffix.
    /// Selected in the first chartbook whose watchlist holds it, rather than
    /// forced onto whichever chart happens to be focused: the bar widget is a
    /// way back into what you were already watching, and a click there that
    /// rewrote the chart in front of you would be a different feature.
    ///
    /// [`NotShown::Unlisted`] when no list holds it, which leaves the caller
    /// to do the one thing that is still right — put the window up.
    ///
    /// With `watchlist`, the window moves to that list whatever else holds
    /// the symbol, and the symbol is picked only there: the bar widget shows
    /// the default list, and a click there is a click in that list, not in
    /// whichever one happened to be on screen. `Unlisted` then means that
    /// list does not hold it, and nothing was searched for instead — a list
    /// that was asked for and quietly swapped for another is the answer a
    /// script cannot tell from the right one.
    ///
    /// [`NotShown::Refused`] when the list holds it and no chart took it,
    /// as [`show_from_rail`](Self::show_from_rail) refuses a click.
    pub fn show_named(
        self: &Rc<Self>,
        symbol: &str,
        suffix: Option<&str>,
        watchlist: Option<i64>,
    ) -> Result<(), NotShown> {
        if let Some(wanted) = watchlist {
            self.show_watchlist(wanted);
            if !self.list_holds(wanted, symbol, suffix) {
                return Err(NotShown::Unlisted);
            }
            let instrument = self.index.find(symbol, suffix).ok_or(NotShown::Unlisted)?;
            return self.pick_in_rail(&instrument);
        }

        let Some(instrument) = self.index.find(symbol, suffix) else {
            return Err(NotShown::Unlisted);
        };

        // The book you are in wins any tie. "The first where it is available"
        // says nothing about two books holding the same symbol, and being
        // moved out of what you were looking at because another book sorts
        // earlier would be the wrong half of that silence.
        let here = self.watchlist.borrow().as_ref().map(|rail| rail.active_watchlist());
        if here.is_some_and(|id| self.list_holds(id, symbol, suffix)) {
            return self.pick_in_rail(&instrument);
        }

        let elsewhere = {
            let books = self.books.borrow();
            books.iter().enumerate().find_map(|(index, book)| {
                let id = book.watchlist?;
                self.list_holds(id, symbol, suffix).then_some(index)
            })
        };
        let Some(index) = elsewhere else { return Err(NotShown::Unlisted) };
        self.activate_book(index);
        // After the switch, never before: selecting looks through the rows the
        // rail is showing, and until it has been rebuilt those are the last
        // book's rows.
        self.pick_in_rail(&instrument)
    }

    /// Put `watchlist` in the rail: by staying, when it is already there; by
    /// moving to another chartbook showing it, when one is; and otherwise by
    /// pointing the book on screen at it.
    ///
    /// Not [`book_using`]: that one says the default list is nobody's, which is
    /// right for handing a list out and wrong here. Several books can show the
    /// default, and moving to one of them is gentler than taking the book you
    /// are in off its own list. A list no other book shows is free to take, so
    /// it is handed straight to the rail rather than through `rail_for_book`.
    pub fn show_watchlist(self: &Rc<Self>, watchlist: i64) {
        let rail = self.watchlist.borrow().as_ref().cloned();
        let Some(rail) = rail else { return };
        if rail.active_watchlist() == watchlist {
            return;
        }
        let active = self.active.get();
        let showing = self
            .books
            .borrow()
            .iter()
            .enumerate()
            .position(|(at, book)| at != active && book.watchlist == Some(watchlist));
        match showing {
            Some(index) => self.activate_book(index),
            None => rail.set_active_watchlist(watchlist),
        }
    }

    /// The database this window reads, for a launch that has to resolve
    /// something against it.
    pub fn store(&self) -> &Store {
        &self.store
    }

    fn list_holds(&self, watchlist: i64, symbol: &str, suffix: Option<&str>) -> bool {
        list_holds(&self.store, watchlist, symbol, suffix)
    }

    fn pick_in_rail(self: &Rc<Self>, instrument: &Instrument) -> Result<(), NotShown> {
        let rail = self.watchlist.borrow().as_ref().cloned();
        match rail.and_then(|rail| rail.pick(instrument)) {
            None => Err(NotShown::Unlisted),
            Some(false) => Err(NotShown::Refused),
            Some(true) => Ok(()),
        }
    }

    pub fn indicators(&self) -> Vec<Indicator> {
        self.focused_pane().indicators.borrow().clone()
    }

    /// What the focused chart is of, and whether it is keeping regular hours
    /// only: what an editor needs to say what an indicator of time will do
    /// on it.
    pub fn focused_market(&self) -> Option<(Instrument, omacharts_engine::Session)> {
        let pane = self.focused_pane();
        let instrument = pane.instrument.borrow().clone()?;
        Some((instrument, pane.session.get()))
    }

    /// Open the chart on the symbol last looked at, or — the first time ever —
    /// on the top of the watchlist.
    ///
    /// Reached only where there was no arrangement to restore, which is a fresh
    /// install and nothing else.
    fn restore_last_symbol(self: &Rc<Self>) {
        let stored = self
            .store
            .setting(LAST_SYMBOL)
            .filter(|symbol| !symbol.is_empty())
            .map(|symbol| (symbol, self.store.setting(LAST_SUFFIX).filter(|s| !s.is_empty())));
        // The store rather than the rail. The rail is built by now, but asking
        // it would make the first thing anybody sees depend on the order two
        // things happen in during startup — and the ordering that matters here,
        // the root section's entries before the named sections', is the store's
        // own answer anyway.
        let wanted = opening_candidates(stored, &self.store.watchlist());
        let found = wanted
            .iter()
            .find_map(|(symbol, suffix)| self.index.find(symbol, suffix.as_deref()));
        if let Some(instrument) = found {
            self.show(instrument);
        }
    }
}
