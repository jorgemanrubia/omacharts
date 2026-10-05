//! The command line.
//!
//! Everything the window can do, this can do — that is the contract, and
//! `AGENTS.md` states it as one. A feature that cannot be driven from a
//! terminal is a feature somebody has to click, which rules out scripting it
//! and rules out an agent doing it at all.
//!
//! Two things follow from that and shape the whole module.
//!
//! The first is that a command has to take effect in a window that is already
//! open. GTK hands a second invocation's arguments to the instance already
//! running, so that instance is the one that executes it — the terminal gets
//! the output and the exit status back across the same hand-off. Nothing
//! polls and nothing watches a file: the app is told, and it refreshes the
//! part that changed.
//!
//! The second is that output is built rather than printed. The same code runs
//! in a process with a real stdout and in a window answering for somebody
//! else's terminal, so an [`Outcome`] is returned and whoever called decides
//! where it goes.

use std::rc::Rc;

use omacharts_engine::providers::Yahoo;
use omacharts_engine::{Instrument, Provider, SearchIndex, Timeframe};

use crate::loader::{Loader, Request, BACKFILL};
use crate::store::{Store, ROOT_SECTION};

pub mod charts;
pub mod completions;
pub mod exec;
pub mod parser;
pub mod skill;
pub mod spec;

/// What the shell is told. Documented in `doc/cli.md` and in the JSON
/// surface, because an agent cannot read a message — only this.
pub const EXIT_OK: u8 = 0;
/// Something went wrong that none of the others describe.
pub const EXIT_ERROR: u8 = 1;
/// The command was not spelled in a way the parser accepts.
pub const EXIT_USAGE: u8 = 2;
/// The watchlist, section, chartbook, chart or symbol named does not exist.
pub const EXIT_NOT_FOUND: u8 = 3;
/// The name given fits more than one thing; say which with `id:N`.
pub const EXIT_AMBIGUOUS: u8 = 4;
/// Understood, and refused: deleting the last chartbook, or the one
/// watchlist that cannot be deleted.
pub const EXIT_REFUSED: u8 = 5;
/// The command meant "the chart I am looking at", and nothing is open to be
/// looking at. Its own code because the fix is different from every other
/// failure: start the app, or name a chart.
pub const EXIT_NO_WINDOW: u8 = 6;
/// The command hit a bug in Omacharts and was abandoned part-way.
///
/// Its own code because it means something no other code does: nothing is
/// wrong with the command, and running it again will do the same thing. Worth
/// reporting, and worth a caller treating differently from a refusal.
pub const EXIT_BUG: u8 = 7;

pub const EXIT_CODES: &[(u8, &str)] = &[
    (EXIT_OK, "the command did what it says"),
    (EXIT_ERROR, "something went wrong that none of the others describe"),
    (EXIT_USAGE, "the command was not spelled in a way the parser accepts"),
    (EXIT_NOT_FOUND, "what was named does not exist"),
    (EXIT_AMBIGUOUS, "the name fits more than one thing; say which with id:N"),
    (EXIT_REFUSED, "understood, and refused"),
    (EXIT_NO_WINDOW, "the command meant the chart you are looking at, and no window is open"),
    (EXIT_BUG, "the command hit a bug in Omacharts and did not finish"),
];

/// A command that could not be carried out, and why.
#[derive(Debug)]
pub struct Fault {
    pub code: u8,
    pub message: String,
}

impl Fault {
    pub fn new(code: u8, message: String) -> Fault {
        Fault { code, message }
    }
    pub fn usage(message: String) -> Fault {
        Fault { code: EXIT_USAGE, message }
    }
    pub fn not_found(message: String) -> Fault {
        Fault { code: EXIT_NOT_FOUND, message }
    }
    pub fn ambiguous(message: String) -> Fault {
        Fault { code: EXIT_AMBIGUOUS, message }
    }
    pub fn refused(message: String) -> Fault {
        Fault { code: EXIT_REFUSED, message }
    }
    /// Nothing is open, so there is no "the one I am looking at".
    ///
    /// Never answered from what was stored when the window last closed: an
    /// agent cannot tell last week's arrangement from this one, and would act
    /// on it.
    pub fn no_window(what: &str) -> Fault {
        Fault {
            code: EXIT_NO_WINDOW,
            message: format!(
                "no window is open, so there is no {what} to act on; \
                 name one, or start Omacharts"
            ),
        }
    }
}

/// What a command produced, kept apart from where it is written.
///
/// A command runs either in a short-lived process of its own or inside the
/// window, answering a terminal in another process entirely. Returning the
/// text rather than printing it is what lets one implementation serve both.
#[derive(Default)]
pub struct Outcome {
    pub code: u8,
    pub out: String,
    pub err: String,
}

impl Outcome {
    pub fn ok(out: String) -> Outcome {
        Outcome { code: EXIT_OK, out, err: String::new() }
    }
    pub fn failed(fault: Fault) -> Outcome {
        Outcome {
            code: fault.code,
            out: String::new(),
            err: format!("omacharts: {}\n", fault.message),
        }
    }
}

/// The window, to a command typed in a terminal.
///
/// Three methods, and all three exist because of one thing: the arrangement
/// of charts is held in memory and written out on a debounce, so a command
/// that read the stored copy would see what was true a moment ago, and one
/// that wrote it would be overwritten by the next save.
///
/// So the window flushes before a command reads, and reloads after one
/// writes. Everything else — watchlists, sections, settings — is rows, which
/// a command can change directly; the window only has to be told to show them
/// again.
pub trait Live {
    /// Write the arrangement out now, so a command about to read or change it
    /// sees what is on screen.
    fn flush_workspace(&self);
    /// Read the arrangement back and rebuild from it.
    fn reload_workspace(&self);
    /// Draw the rail again, after a command changed a watchlist.
    fn reload_watchlists(&self);
    /// Re-read the theme and bar scheme, and repaint.
    fn adopt_theming(&self);

    /// Save a picture of the focused chart, or of the whole open chartbook.
    ///
    /// The one chart verb that cannot be answered without a window at all, not
    /// even from a selector: a screenshot is pixels, and a stored arrangement
    /// has none. Answers with what it photographed and where it went, because
    /// an implicit target has to be named back.
    fn screenshot(
        &self,
        whole_book: bool,
        into: Option<&std::path::Path>,
        clipboard: bool,
    ) -> Result<(String, std::path::PathBuf), Fault>;
    /// Queue a paced background fetch of these instruments' daily bars, and
    /// return immediately.
    ///
    /// This is the whole reason the trait exists for anything but redrawing:
    /// a command running inside the window runs *on its main loop*, so a
    /// command that fetches freezes the app for as long as the provider's
    /// pacing takes. The window already owns a loader thread that fetches
    /// nearest-wanted-first and shares one throttle across every request it
    /// has ever made, which is strictly the better place to do it.
    fn warm(&self, instruments: &[Instrument]);
    /// The window's symbol index, if it already names every listing.
    ///
    /// Same bargain as `warm`: the window has spent the milliseconds once, so
    /// a command on its main loop borrows the result instead of spending them
    /// again on every tick of a bar widget's timer.
    ///
    /// `None` while the window is still showing the curated half, a few
    /// hundred milliseconds after it opens. A caller that needs the whole
    /// catalogue has to build one then, and had better say so.
    fn symbols(&self) -> Option<Rc<SearchIndex>>;
}

/// Is this argument list a command rather than a symbol to open?
///
/// Checked against the table rather than by looking for a leading dash,
/// because `omacharts NVDA` and `omacharts watchlist add` arrive the same
/// way and mean entirely different things.
pub fn is_command(args: &[String]) -> bool {
    let Some(first) = args.get(1).map(String::as_str) else { return false };
    matches!(first, "help" | "surface") || spec::SURFACE.iter().any(|n| n.name == first)
}

/// Run one command.
///
/// `live` is the window when there is one. Its absence is not an error: the
/// same commands work with nothing running, which is what makes this usable
/// over ssh and out of a script.
pub fn run(args: &[String], store: &Store, live: Option<&dyn Live>) -> Outcome {
    // A command runs inside the window when there is one, and a panic there
    // does not merely fail: GLib calls us from C, so unwinding through that
    // frame aborts the process. The window goes, and the arrangement somebody
    // had on screen goes with it — because of a bug reached by typing a
    // command. Nothing a command can do is worth that, so whatever happens in
    // here comes back as a failed command and the window survives it.
    //
    // `AssertUnwindSafe` because neither a `Store` nor the window is unwind
    // safe by the type system's reckoning, and the question the marker really
    // asks is whether a half-finished command can leave either in a state the
    // next one reads as valid. It cannot: a watchlist change is one SQL
    // statement, and the arrangement is written as one value at the end of the
    // verb that changed it, so an abandoned command leaves the last complete
    // version of both.
    let attempt = std::panic::AssertUnwindSafe(|| exec::dispatch(args, store, live));
    std::panic::catch_unwind(attempt).unwrap_or_else(|_| {
        Outcome::failed(Fault::new(
            EXIT_BUG,
            format!(
                "{:?} hit a bug in Omacharts and did not finish; nothing else was affected",
                args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ")
            ),
        ))
    })
}

/// How many closes the row's sparkline gets.
///
/// Enough to show a shape, few enough that a watchlist of forty stays a small
/// payload: the widget re-reads this every couple of minutes.
const SPARK_POINTS: usize = 30;

/// How stale a quote may be before a `--refresh` run goes and gets it.
///
/// The very number a chart in the window holds itself to at its slowest, and
/// for the same reason — it is the worst delay the provider admits to. Taken
/// from there rather than respelled here, because the bar widget and the
/// window answering "how old is too old" differently would be a disagreement
/// nobody could see and everybody would feel.
const STALE_AFTER_SECONDS: i64 = omacharts_engine::refresh::CEILING_SECONDS;

/// Most symbols one invocation will fetch.
///
/// The bar calls this on a timer, and a run that tries to refresh forty
/// symbols would spend minutes being paced apart and overlap the next run.
/// Whatever is stalest goes first, so a few runs cover everything.
const MAX_REFRESH: usize = 6;

pub struct Quote {
    pub last: f64,
    pub change: f64,
    pub change_pct: f64,
    pub as_of: i64,
}

/// Read the cached daily bars for an instrument and work out its move.
pub fn quote(store: &Store, provider: &Yahoo, instrument: &Instrument) -> Option<Quote> {
    let key = cache_key(provider, instrument)?;
    let bars = store.load_bars(&key, Timeframe::days(1));
    let (previous, last) = (bars.get(bars.len().checked_sub(2)?)?, bars.last()?);
    let change = last.close - previous.close;
    Some(Quote {
        last: last.close,
        change,
        change_pct: if previous.close == 0.0 { 0.0 } else { change / previous.close * 100.0 },
        as_of: last.ts,
    })
}

pub fn cache_key(provider: &Yahoo, instrument: &Instrument) -> Option<String> {
    provider.symbol_for(instrument).map(|symbol| format!("{}:{symbol}", provider.id()))
}

/// The symbols whose daily bars are old enough to be worth fetching, stalest
/// first.
///
/// Who acts on it depends on where we are running, which is the distinction
/// [`Live::warm`] exists to make.
fn stale_daily(store: &Store, provider: &Yahoo, instruments: &[Instrument]) -> Vec<Instrument> {
    let now = chrono::Utc::now().timestamp();
    let daily = Timeframe::days(1);

    let mut candidates: Vec<(i64, &Instrument)> = instruments
        .iter()
        .filter_map(|instrument| {
            let key = cache_key(provider, instrument)?;
            let fetched_at = store.coverage(&key, daily).map(|c| c.fetched_at).unwrap_or(0);
            (now - fetched_at > STALE_AFTER_SECONDS).then_some((fetched_at, instrument))
        })
        .collect();
    candidates.sort_by_key(|(fetched_at, _)| *fetched_at);
    candidates.into_iter().map(|(_, instrument)| instrument.clone()).collect()
}

/// Fetch daily bars for the stalest symbols that need them, here and now.
///
/// Only correct with no window open, where "here" is a short-lived process
/// that exists to answer one question and has nothing to block. Inside the
/// window this would run on the GTK main loop — see [`Live::warm`].
///
/// Through the loader rather than the provider directly, because the loader
/// is the one place that paces requests. Speculative, so they go two seconds
/// apart and are refused outright while the provider is throttling: a bar
/// widget must never cost the app its rate limit.
fn refresh(store: &Store, provider: &Yahoo, instruments: &[Instrument]) {
    let (sender, receiver) = async_channel::unbounded();
    let loader = Loader::new(Yahoo::new(), sender);
    let mut outstanding = 0;
    let stale = stale_daily(store, provider, instruments);
    for (rank, instrument) in stale.iter().take(MAX_REFRESH).enumerate() {
        let Some(symbol) = provider.symbol_for(instrument) else { continue };
        let Some(key) = cache_key(provider, instrument) else { continue };
        let request = Request { key, symbol, timeframe: Timeframe::days(1), speculative: true };
        loader.fetch(request, BACKFILL.saturating_add(rank as u32));
        outstanding += 1;
    }
    // The loader writes whatever arrives to the cache itself. All that is
    // left is to wait until it has.
    for _ in 0..outstanding {
        if receiver.recv_blocking().is_err() {
            break;
        }
    }
}

/// The watchlist as JSON, for the bar widget.
pub fn watchlist_json(store: &Store, refresh_first: bool, live: Option<&dyn Live>) -> String {
    let index = resolver(live);
    let provider = Yahoo::new();

    let sections = store.watchlist();
    if refresh_first {
        let instruments: Vec<Instrument> = sections
            .iter()
            .flat_map(|section| section.entries.iter())
            .filter_map(|entry| index.find(&entry.symbol, entry.suffix.as_deref()).cloned())
            .collect();
        match live {
            // Inside the window, on its main loop. Hand the work to the
            // loader thread and answer from the cache, which is all the
            // widget draws anyway: a quote that arrives on the next tick is
            // not worth six seconds of frozen application.
            Some(live) => live.warm(&stale_daily(store, &provider, &instruments)),
            // A process of its own, with no window and nothing to block.
            None => refresh(store, &provider, &instruments),
        }
    }

    let mut out = String::from("{\"sections\":[");
    for (s, section) in sections.iter().enumerate() {
        if s > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"name\":{},\"root\":{},\"entries\":[",
            json_string(&section.name),
            section.id == ROOT_SECTION
        ));
        // Resolve first, then count. The separator has to be decided over the
        // entries that made it into the array, not over the stored rows: a row
        // that resolves to nothing and is skipped anyway used to spend its turn
        // on a comma, which put one before the first entry or two between a
        // pair — and a bar widget parsing the answer got nothing at all.
        let resolved = section
            .entries
            .iter()
            .filter_map(|entry| index.find(&entry.symbol, entry.suffix.as_deref()));
        for (e, instrument) in resolved.enumerate() {
            if e > 0 {
                out.push(',');
            }
            out.push_str(&entry_json(store, &provider, instrument));
        }
        out.push_str("]}");
    }
    out.push_str(&format!(
        "],\"colors\":{},\"updatedAt\":{}}}",
        colors_json(store),
        chrono::Utc::now().timestamp()
    ));
    out
}

/// The index the feed resolves stored entries against.
///
/// It has to be the whole catalogue, because `watchlist add` admits the whole
/// catalogue: resolving against the curated few hundred left most of what the
/// app lets somebody watchlist out of the feed entirely.
///
/// Where it comes from is the only question. Inside the window there is one
/// already, so borrow it — building eleven thousand rows on the main loop
/// every time a bar widget ticks is milliseconds the frame has no business
/// paying. Outside, in the short-lived process that has no window and no
/// index, build it: that is exactly what `everything` is for.
///
/// The window's first few hundred milliseconds are the one case where
/// borrowing is not on offer, and there the cost gets paid. It is a bad moment
/// for it and still the right call: the alternative is a feed that quietly
/// drops half a watchlist, and the widget asks again only every two minutes by
/// default, so a short answer would sit on the bar until it did.
fn resolver(live: Option<&dyn Live>) -> Rc<SearchIndex> {
    live.and_then(|live| live.symbols())
        .unwrap_or_else(|| Rc::new(crate::inventory::everything()))
}

/// The direction colours, derived exactly as the app derives them.
///
/// Sent with the data rather than hardcoded in the widget, so the bar and the
/// window cannot disagree about what up looks like — and so changing the
/// desktop theme moves both.
fn colors_json(store: &Store) -> String {
    let home = crate::store::home();
    let theme = omacharts_engine::omarchy::current(&home)
        .unwrap_or_else(|| omacharts_engine::theme::builtin_themes()[0].clone());
    // The theme's colours rather than the chosen scheme's, but the right way
    // round: a red-up window beside a green-up bar would be two answers to
    // which way the market went.
    let red_up = store.setting(crate::theming::SETTING_BARS).as_deref()
        == Some(omacharts_engine::theme::THEME_RED_UP_ID);
    let bars = match red_up {
        true => omacharts_engine::theme::theme_red_up_bars(&theme),
        false => omacharts_engine::theme_bars(&theme),
    };
    use omacharts_engine::Direction;
    format!(
        "{{\"up\":{},\"down\":{},\"flat\":{},\"foreground\":{}}}",
        json_string(bars.outline(Direction::Up)),
        json_string(bars.outline(Direction::Down)),
        json_string(bars.outline(Direction::Flat)),
        json_string(&theme.ui.text),
    )
}

fn entry_json(store: &Store, provider: &Yahoo, instrument: &Instrument) -> String {
    let mut fields = format!(
        "{{\"symbol\":{},\"suffix\":{},\"display\":{},\"name\":{},\"kind\":{}",
        json_string(&instrument.symbol),
        match &instrument.suffix {
            Some(suffix) => json_string(suffix),
            None => "null".to_string(),
        },
        json_string(&instrument.display_symbol()),
        json_string(&instrument.name),
        json_string(instrument.kind.label()),
    );
    if let Some(key) = cache_key(provider, instrument) {
        let bars = store.load_bars(&key, Timeframe::days(1));
        let tail = &bars[bars.len().saturating_sub(SPARK_POINTS)..];
        if tail.len() >= 2 {
            let points: Vec<String> =
                tail.iter().map(|bar| format!("{:.6}", bar.close)).collect();
            fields.push_str(&format!(",\"spark\":[{}]", points.join(",")));
        }
    }
    match quote(store, provider, instrument) {
        // A quote we do not have is absent rather than zero. Zero is a price.
        Some(q) => fields.push_str(&format!(
            ",\"last\":{:.6},\"change\":{:.6},\"changePct\":{:.4},\"asOf\":{}",
            q.last, q.change, q.change_pct, q.as_of
        )),
        None => fields.push_str(",\"last\":null,\"change\":null,\"changePct\":null,\"asOf\":null"),
    }
    fields.push('}');
    fields
}

/// Minimal JSON string escaping — enough for symbols and section names.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The old top-level help, kept as the one-line summary `--help` opens with.
///
/// The real help is rendered by the parser from `spec`, so this is only what
/// a caller that wants a string rather than a printed page gets.
pub fn usage() -> String {
    parser::command().render_help().to_string()
}

/// Implemented for `Rc<Window>` rather than for `Window`, so that `&self` is
/// already the `Rc` the window's own methods are written against — rebuilding
/// an arrangement builds widgets that hold the window, and a method that does
/// that cannot be handed a bare reference. The alternative, recovering an `Rc`
/// from a `&Window`, is a use-after-free waiting to happen.
impl Live for Rc<crate::ui::Window> {
    fn flush_workspace(&self) {
        crate::ui::Window::save_workspace(self);
    }

    fn reload_workspace(&self) {
        crate::ui::Window::reload_workspace(self);
    }

    fn reload_watchlists(&self) {
        crate::ui::Window::reload_watchlists(self);
    }

    fn adopt_theming(&self) {
        crate::ui::Window::adopt_theming(self);
    }

    fn screenshot(
        &self,
        whole_book: bool,
        into: Option<&std::path::Path>,
        clipboard: bool,
    ) -> Result<(String, std::path::PathBuf), Fault> {
        crate::ui::Window::screenshot(self, whole_book, into, clipboard)
            .map_err(|error| Fault::new(EXIT_ERROR, error))
    }

    fn warm(&self, instruments: &[Instrument]) {
        crate::ui::Window::warm(self, instruments);
    }

    fn symbols(&self) -> Option<Rc<SearchIndex>> {
        crate::ui::Window::symbols(self)
    }
}

/// The window as something a command can refresh.
///
/// An `Option` because the same function is asked in a process that has no
/// window — there, a command still runs against the database and still answers
/// the terminal, and there is simply nothing on screen to catch up.
pub fn live(window: &Rc<crate::ui::Window>) -> Option<Box<dyn Live>> {
    Some(Box::new(window.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window that goes wrong. `flush_workspace` is the first thing a command
    /// touching the arrangement calls, so panicking there stands in for a bug
    /// anywhere inside one.
    struct Breaks;

    impl Live for Breaks {
        fn flush_workspace(&self) {
            panic!("a bug somewhere inside the window")
        }
        fn reload_workspace(&self) {}
        fn reload_watchlists(&self) {}
        fn adopt_theming(&self) {}
        fn warm(&self, _instruments: &[Instrument]) {}
        fn symbols(&self) -> Option<Rc<SearchIndex>> {
            None
        }
        fn screenshot(
            &self,
            _whole_book: bool,
            _into: Option<&std::path::Path>,
            _clipboard: bool,
        ) -> Result<(String, std::path::PathBuf), Fault> {
            panic!("a bug somewhere inside the window")
        }
    }

    /// Unwinding out of a command would cross the C frame GLib called it from,
    /// which aborts rather than unwinds — so a bug reached by typing a command
    /// would cost somebody the arrangement they had on screen. The command
    /// fails; the window does not.
    #[test]
    fn a_command_that_hits_a_bug_fails_rather_than_taking_the_window_with_it() {
        let store = Store::memory().unwrap();
        let args: Vec<String> =
            ["omacharts", "status", "show"].iter().map(|a| a.to_string()).collect();
        let outcome = run(&args, &store, Some(&Breaks));
        assert_eq!(outcome.code, EXIT_BUG);
        assert!(outcome.err.contains("status show"), "it has to say which: {}", outcome.err);
    }

    #[test]
    fn the_payload_carries_the_themes_direction_colours() {
        let store = Store::memory().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
        for key in ["up", "down", "flat", "foreground"] {
            let value = parsed[key].as_str().unwrap_or_default();
            assert!(value.starts_with('#') && value.len() >= 7, "{key}: {value:?}");
        }
        assert_ne!(parsed["up"], parsed["down"], "up and down must differ");

        // The bar reads a rise the way the window does.
        store.set_setting(crate::theming::SETTING_BARS, omacharts_engine::theme::THEME_RED_UP_ID);
        let red: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
        assert_eq!((&red["up"], &red["down"]), (&parsed["down"], &parsed["up"]));
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(json_string("plain"), "\"plain\"");
        assert_eq!(json_string("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(json_string("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(json_string("two\nlines"), "\"two\\nlines\"");
        assert_eq!(json_string("bell\u{7}"), "\"bell\\u0007\"");
    }

    #[test]
    fn a_section_name_with_quotes_does_not_break_the_json() {
        let store = Store::memory().unwrap();
        let id = store
            .add_section(crate::store::DEFAULT_WATCHLIST, "My \"best\" picks")
            .unwrap();
        store.add_to_section(id, "ES", None);

        let name = json_string(&store.watchlist()[0].name);
        let parsed: serde_json::Value = serde_json::from_str(&name).unwrap();
        assert_eq!(parsed.as_str().unwrap(), "My \"best\" picks");
    }

    #[test]
    fn an_entry_without_bars_reports_no_quote_rather_than_zero() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let instrument = index.find("ES", None).unwrap();

        let json = entry_json(&store, &provider, instrument);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["last"].is_null(), "a missing price must not read as 0");
        assert_eq!(parsed["symbol"], "ES");
        assert_eq!(parsed["kind"], "Futures");
    }

    /// `watchlist add` takes anything in the catalogue, so the feed has to be
    /// able to name anything in the catalogue. It used to resolve against the
    /// curated few hundred instead, which dropped every mid-cap somebody had
    /// put on their bar — and dropped it mid-array, so the comma that row had
    /// already been counted for went out with nothing in front of it. FOXF
    /// first, because that is the position that makes the JSON unparseable.
    #[test]
    fn the_feed_names_every_symbol_a_watchlist_can_hold() {
        let store = Store::memory().unwrap();
        store.add_to_section(ROOT_SECTION, "FOXF", None);
        store.add_to_section(ROOT_SECTION, "AAPL", None);

        let json = watchlist_json(&store, false, None);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}: {json}"));

        let entries = parsed["sections"][0]["entries"].as_array().expect("entries");
        let symbols: Vec<&str> = entries.iter().filter_map(|e| e["symbol"].as_str()).collect();
        assert_eq!(symbols, ["FOXF", "AAPL"], "a long-tail listing belongs on the bar");
    }

    /// The widget hands these three fields straight back as `omacharts SAP DE`
    /// when somebody clicks the row, and `SearchIndex::find` matches on the
    /// ticker and the suffix as two things. Emitting `SAP.DE` as the symbol
    /// would read as a ticker nobody lists and quietly open nothing.
    #[test]
    fn a_listing_abroad_carries_its_suffix_apart_from_its_ticker() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let instrument = index.find("SAP", Some("DE")).unwrap();

        let parsed: serde_json::Value =
            serde_json::from_str(&entry_json(&store, &provider, instrument)).unwrap();
        assert_eq!(parsed["symbol"], "SAP", "the ticker alone, for looking it up again");
        assert_eq!(parsed["suffix"], "DE");
        assert_eq!(parsed["display"], "SAP.DE", "and the spelling people read");
    }

    #[test]
    fn a_row_carries_a_short_series_for_its_sparkline() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let instrument = index.find("ES", None).unwrap();
        let key = cache_key(&provider, instrument).unwrap();

        // More history than the sparkline wants, so it has to take the tail.
        let bars: Vec<omacharts_engine::Bar> = (0..100)
            .map(|i| omacharts_engine::Bar {
                ts: (i + 1) * 86_400,
                open: i as f64,
                high: i as f64,
                low: i as f64,
                close: i as f64,
                volume: 1.0,
            })
            .collect();
        store.write_bars(&key, Timeframe::days(1), &bars);

        let parsed: serde_json::Value =
            serde_json::from_str(&entry_json(&store, &provider, instrument)).unwrap();
        let spark = parsed["spark"].as_array().unwrap();
        assert_eq!(spark.len(), SPARK_POINTS);
        assert_eq!(spark.last().unwrap().as_f64().unwrap(), 99.0, "the tail, not the start");
    }

    #[test]
    fn a_row_with_nothing_to_draw_has_no_series() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let instrument = index.find("ES", None).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&entry_json(&store, &provider, instrument)).unwrap();
        assert!(parsed["spark"].is_null(), "an absent series beats an empty one");
    }

    #[test]
    fn an_entry_with_bars_reports_its_move() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let instrument = index.find("ES", None).unwrap();
        let key = cache_key(&provider, instrument).unwrap();

        let bar = |ts, close| omacharts_engine::Bar {
            ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
        };
        store.write_bars(&key, Timeframe::days(1), &[bar(86_400, 100.0), bar(172_800, 110.0)]);

        let parsed: serde_json::Value =
            serde_json::from_str(&entry_json(&store, &provider, instrument)).unwrap();
        assert_eq!(parsed["last"].as_f64().unwrap(), 110.0);
        assert_eq!(parsed["change"].as_f64().unwrap(), 10.0);
        assert!((parsed["changePct"].as_f64().unwrap() - 10.0).abs() < 1e-6);
    }

    #[test]
    fn one_bar_is_not_enough_for_a_change() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let instrument = index.find("ES", None).unwrap();
        let key = cache_key(&provider, instrument).unwrap();
        store.write_bars(
            &key,
            Timeframe::days(1),
            &[omacharts_engine::Bar {
                ts: 86_400,
                open: 100.0,
                high: 100.0,
                low: 100.0,
                close: 100.0,
                volume: 1.0,
            }],
        );
        assert!(quote(&store, &provider, instrument).is_none());
    }
}
