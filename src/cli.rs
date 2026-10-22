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

use omacharts_engine::providers;
use omacharts_engine::{Instrument, Provider, SearchIndex, Timeframe};

use crate::loader::{Listing, Loader, Request, BACKFILL};
use crate::store::{Store, ROOT_SECTION};

pub mod charts;
pub mod completions;
pub mod exec;
pub mod parser;
pub mod skill;
pub mod spec;
pub mod transfer;

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

/// A file named on the command line, made absolute where it was typed.
///
/// With a window open, the command runs in the window's process, whose
/// working directory is not the terminal's; `backup restore ./omacharts.db`
/// has to mean the file beside the person typing it, the way any command
/// would. So the path is settled here, before it travels. A name that is not
/// a file here is left alone: `backup restore settings-20261021-140307` names
/// a backup.
///
/// Wherever the command sits, since the line handed to a window still has the
/// launch options in front of it.
pub fn caller_paths(mut args: Vec<String>) -> Vec<String> {
    let at = args.windows(2).position(|pair| pair[0] == "backup" && pair[1] == "restore");
    if let Some(named) = at.and_then(|at| args.get_mut(at + 2)) {
        let path = std::path::Path::new(named.as_str());
        if !path.is_absolute() && path.is_file() {
            if let Ok(full) = std::path::absolute(path) {
                *named = full.to_string_lossy().into_owned();
            }
        }
    }
    args
}

/// Whoever typed the command, for what a command reads from them rather than
/// from the database: a file they named, or what they piped in.
///
/// A command inside the window runs in another process from the terminal that
/// typed it, with another working directory and no stdin of its own. Reading
/// `./watchlists.json` there would read the window's file, not theirs.
pub trait Caller {
    /// The text of `path` as the caller means it: relative to where they
    /// were, or their stdin for `-`.
    fn read(&self, path: &str) -> Result<String, Fault>;

    /// The home directory a command that writes outside the database writes
    /// under — `plugin install` and `uninstall` edit the shell's configuration
    /// there. Whoever is running, unless a test says otherwise.
    fn home(&self) -> std::path::PathBuf {
        crate::store::home()
    }
}

/// This process, which is the caller whenever there is no window.
pub struct Here;

impl Caller for Here {
    fn read(&self, path: &str) -> Result<String, Fault> {
        use std::io::Read;
        let mut text = String::new();
        let read = match path {
            "-" => std::io::stdin().read_to_string(&mut text).map(|_| text),
            path => std::fs::read_to_string(path),
        };
        read.map_err(|error| unreadable(path, error))
    }
}

/// A file that could not be read, said the same way wherever it was read.
pub fn unreadable(path: &str, error: impl std::fmt::Display) -> Fault {
    match path {
        "-" => Fault::new(EXIT_ERROR, format!("could not read stdin: {error}")),
        path => Fault::not_found(format!("could not read {path:?}: {error}")),
    }
}

/// A `-` asking for stdin when stdin is the terminal itself.
///
/// Checked in the process that was typed in, before anything is handed on:
/// the window can read a caller's stdin but cannot tell a pipe from a
/// keyboard, and a read from a keyboard waits for an end of input that never
/// comes — on the window's own main loop, so the whole app with it.
pub fn stdin_is_not_piped(args: &[String]) -> Option<Outcome> {
    use std::io::IsTerminal;
    (is_command(args) && args.iter().any(|arg| arg == "-") && std::io::stdin().is_terminal())
        .then(|| {
            Outcome::failed(Fault::usage(
                "`-` reads what is piped in, and nothing is; pipe a file in with `< FILE`".into(),
            ))
        })
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
    /// Chart from the feed the settings now name, if it is not the one on
    /// screen. Nothing to do without a window.
    fn adopt_feed(&self) {}
    /// Show the header the settings now name on every chart on screen.
    /// Nothing to do without a window.
    fn adopt_header(&self) {}
    /// Draw every chart on screen in the time zone and the time format the
    /// settings now name. Nothing to do without a window.
    fn adopt_clock(&self) {}

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
    /// Queue batches of closes on the window's loader, the same bargain as
    /// [`Self::warm`] for the rows a batch can cover.
    fn warm_closes(&self, batches: Vec<Vec<Listing>>);
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

    /// What the window holds a stream subscription to, for a provider that
    /// streams. Empty for one that does not, and with no window at all.
    fn streaming(&self) -> Vec<crate::live::Report> {
        Vec::new()
    }
}

/// A command line with its launch options taken off, and what they said.
#[derive(Debug)]
pub struct Launched {
    /// What is left, including `argv[0]`, for the command dispatch and the
    /// symbol to open.
    pub args: Vec<String>,
    /// The feed `--provider` named, already checked against the catalogue.
    pub provider: Option<&'static omacharts_engine::providers::Listed>,
    /// The watchlist `--watchlist` named, as typed. Resolved against the
    /// store by [`watchlist_named`] where there is one: peeling happens
    /// before any database is open.
    pub watchlist: Option<String>,
}

/// Takes the options in [`spec::LAUNCH`] off a command line.
///
/// Before anything else looks at the arguments, because a launch option is
/// not a command's argument and not a symbol: `omacharts --provider tos NVDA`
/// has to leave `NVDA` looking like the ticker it is, and
/// `omacharts --provider tos watchlist list` has to still be a `watchlist`
/// command. Taking them off here is also what keeps [`is_command`] and
/// [`runs_in_the_caller`] reading `args[1]` — they ask what was typed, and a
/// launch option is not part of the answer.
///
/// Refuses rather than ignores. A name that is not a feed we have is a
/// mistake worth stopping for: carrying on with Yahoo would chart from the
/// wrong source for an hour before anybody noticed, and a script that
/// misspells a feed wants to hear about it.
pub fn peel_launch(args: &[String]) -> Result<Launched, Fault> {
    let mut kept: Vec<String> = Vec::with_capacity(args.len());
    let mut provider = None;
    let mut watchlist = None;
    let mut rest = args.iter();

    while let Some(arg) = rest.next() {
        let Some((long, inline)) = arg.strip_prefix("--").map(split_option) else {
            kept.push(arg.clone());
            continue;
        };
        let Some(option) = spec::LAUNCH.iter().find(|option| option.long == long) else {
            kept.push(arg.clone());
            continue;
        };
        let value = match inline {
            Some(value) => value.to_string(),
            None => rest
                .next()
                .cloned()
                .ok_or_else(|| Fault::usage(format!("--{long} takes a {}", option.value)))?,
        };
        match option.long {
            "provider" => provider = Some(feed_named(&value)?),
            "watchlist" => watchlist = Some(value),
            // Unreachable while every entry in LAUNCH has an arm, and the
            // compiler cannot know that. A new launch option lands here.
            other => return Err(Fault::usage(format!("--{other} is not handled"))),
        }
    }

    // A list to open things in means nothing to a command, and a command
    // that ran as if it had been scoped to one is worse than one that was
    // refused: `--watchlist Tech chart set --symbol NVDA` would act on the
    // focused chart wherever it is, and exit 0.
    if watchlist.is_some() && is_command(&kept) {
        return Err(Fault::usage(
            "--watchlist says which list to open a symbol in; it does not apply to commands"
                .into(),
        ));
    }

    Ok(Launched { args: kept, provider, watchlist })
}

/// The watchlist `--watchlist` named, a name or `id:N`, as its id and name.
pub fn watchlist_named(store: &Store, selector: &str) -> Result<(i64, String), Fault> {
    exec::find_list(store, selector)
}

/// `name=value` as a long option's two halves.
fn split_option(rest: &str) -> (&str, Option<&str>) {
    match rest.split_once('=') {
        Some((long, value)) => (long, Some(value)),
        None => (rest, None),
    }
}

/// The feed with this name, or a usage error listing the ones there are.
fn feed_named(name: &str) -> Result<&'static omacharts_engine::providers::Listed, Fault> {
    providers::listed(name).ok_or_else(|| {
        Fault::usage(format!(
            "no such data feed: {name:?}; the feeds are {}",
            spec::launch_values("provider").join(", ")
        ))
    })
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

/// Does this command have to run in the process it was typed in?
///
/// The whole noun, not the verbs that happen to write something: `--help` and
/// a misspelled flag have to be answered by the process whose environment the
/// answer describes, or the help is right about somebody else's machine. See
/// [`spec::Noun::local`] for which nouns those are and why.
pub fn runs_in_the_caller(args: &[String]) -> bool {
    let Some(first) = args.get(1).map(String::as_str) else { return false };
    if spec::SURFACE.iter().any(|noun| noun.name == first && noun.local) {
        return true;
    }
    // And the handful of verbs whose own reason is not their noun's. See
    // [`spec::IN_THE_CALLER`]: a sign-in waits on a person, and waiting on a
    // person inside the window's main loop is a frozen window.
    let Some(second) = args.get(2).map(String::as_str) else { return false };
    spec::IN_THE_CALLER.contains(&(first, second))
}

/// Run one command.
///
/// `live` is the window when there is one. Its absence is not an error: the
/// same commands work with nothing running, which is what makes this usable
/// over ssh and out of a script.
pub fn run(args: &[String], store: &Store, live: Option<&dyn Live>, caller: &dyn Caller) -> Outcome {
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
    let attempt = std::panic::AssertUnwindSafe(|| exec::dispatch(args, store, live, caller));
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

/// How long a row whose market is shut goes before a `--refresh` run fetches
/// it anyway.
///
/// When a row is due is otherwise the window's own rule,
/// [`omacharts_engine::refresh::due`]: once a quarter of an hour while its
/// market trades, never while it is shut. Never is right for a chart on
/// screen, which was fetched while the market was open, and wrong for a
/// widget nobody ran since Thursday: on Saturday it would go on showing
/// Thursday's close until Monday. Once a day is enough to catch that, and
/// costs a weekend two requests.
const CLOSED_REFRESH_SECONDS: i64 = 86_400;

/// Most symbols one invocation will fetch bars for, one request each.
///
/// Only rows with no daily bars at all are fetched this way, or every row
/// on a provider that cannot batch. The bar calls this on a timer, and a run
/// that tried forty would spend minutes being paced apart and overlap the
/// next run. Whatever is stalest goes first, so a few runs cover everything.
const MAX_REFRESH: usize = 6;

/// Most batches of closes one widget run will send. Sixty rows at Yahoo's
/// twenty a request; a longer list takes its stalest first and the rest on
/// the next run. A run is a process of its own, which has to be done before
/// the next one starts.
const MAX_BATCHES: usize = 3;

/// How far ahead of UTC a session's own day runs.
///
/// A daily bar is stamped at its session's open, and a few sessions open on
/// their own date while it is still the evening before in UTC: FX at London
/// midnight, 23:00 UTC in summer, Sydney at 23:00 and Wellington at 21:00.
/// Everything else opens between midnight and 15:00 UTC. Three hours on
/// puts every one of them on its own date.
const SESSION_LEAD_SECONDS: i64 = 3 * 3_600;

/// The day the session at `ts` belongs to, as days since the epoch.
///
/// What a daily stamp means, rather than the UTC date it happens to fall
/// on: EURUSD's Friday is stamped Thursday 23:00 in summer and Friday 00:00
/// in winter, and is Friday's either way.
pub fn session_day(ts: i64) -> i64 {
    (ts + SESSION_LEAD_SECONDS).div_euclid(86_400)
}

pub struct Quote {
    pub last: f64,
    pub change: f64,
    pub change_pct: f64,
    pub as_of: i64,
}

/// Read an instrument's move from the cache.
pub fn quote(store: &Store, provider: &dyn Provider, instrument: &Instrument) -> Option<Quote> {
    let key = cache_key(provider, instrument)?;
    quote_from(store, &key, &daily_bars(store, &key, instrument.kind))
}

/// The daily bars cached under `key`, as a chart draws them.
pub fn daily_bars(
    store: &Store,
    key: &str,
    kind: omacharts_engine::InstrumentKind,
) -> Vec<omacharts_engine::Bar> {
    let mut bars = store.load_bars(key, Timeframe::days(1));
    omacharts_engine::drawable(&mut bars, Timeframe::days(1), kind);
    bars
}

/// The move of the series cached under `key`: its latest close against the
/// one before, from whichever of the two places a close is kept is newer.
///
/// The daily bars, which a chart keeps current, or the closes a batch
/// fetched for the bar widget. Newer means the later session first, then the
/// later fetch: a chart refreshed a minute after the widget's batch has the
/// fresher close for the same day.
#[cfg(test)]
fn quote_at(store: &Store, key: &str) -> Option<Quote> {
    quote_from(store, key, &store.load_bars(key, Timeframe::days(1)))
}

/// The same, from daily bars already loaded: [`daily_bars`], so that what a
/// chart would not draw is not quoted either.
pub fn quote_from(store: &Store, key: &str, bars: &[omacharts_engine::Bar]) -> Option<Quote> {
    let daily = Timeframe::days(1);
    let from_bars = match bars {
        [.., previous, last] => {
            let fetched_at = store.coverage(key, daily).map_or(0, |c| c.fetched_at);
            Some((last.ts, fetched_at, last.close, previous.close))
        }
        _ => None,
    };
    let from_closes = store.load_closes(key).map(|c| {
        // A batch now and then comes back with a session missing, and its
        // previous close is then two sessions old: the move shown would be
        // yesterday's and today's together. When the bars hold a session in
        // the gap, the latest of them is the previous close.
        let day = session_day;
        let previous = bars
            .iter()
            .rev()
            .find(|bar| day(bar.ts) > day(c.previous_at) && day(bar.ts) < day(c.at))
            .map_or(c.previous, |bar| bar.close);
        (c.at, c.fetched_at, c.last, previous)
    });
    let newer = |(at, fetched_at, ..): &(i64, i64, f64, f64)| (session_day(*at), *fetched_at);
    let (as_of, _, last, previous) = [from_bars, from_closes].into_iter().flatten().max_by_key(newer)?;
    let change = last - previous;
    Some(Quote {
        last,
        change,
        change_pct: if previous == 0.0 { 0.0 } else { change / previous * 100.0 },
        as_of,
    })
}

pub fn cache_key(provider: &dyn Provider, instrument: &Instrument) -> Option<String> {
    provider.symbol_for(instrument).map(|symbol| format!("{}:{symbol}", provider.id()))
}

/// What one refresh of the widget's rows asks for.
#[derive(Debug, Default, PartialEq)]
pub struct Refresh {
    /// Rows to fetch daily bars for, one request each: those with none
    /// cached, whose sparkline needs a history that closes alone cannot give,
    /// and any row on a provider that cannot batch.
    pub bars: Vec<Instrument>,
    /// The rest, one request per batch, when any of them is due.
    pub closes: Vec<Vec<Listing>>,
}

/// Whether a row with something cached is worth asking about again.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Due {
    Now,
    /// Not yet, but its market trades and it is over halfway there: worth
    /// a place in a batch that is going anyway, see [`plan_refresh`].
    Soon,
    Later,
}

/// Is a row with something cached worth asking about again at `now`?
///
/// By its age and its market alone, as if every feed were polled. Whether a
/// streamed feed may be polled at all is the sender's to decide: the window
/// keeps a streamed row to what the stream gives it ([`Live::warm`]), while
/// a widget process with no window has no stream running, and a quote it
/// never refetched would stay at its first price for good.
fn row_due(instrument: &Instrument, fetched_at: i64, now: i64) -> Due {
    use omacharts_engine::refresh::{self, Held};
    let timeframe = Timeframe::days(1);
    let candidate = refresh::Candidate {
        delivery: omacharts_engine::Delivery::Polled,
        visible: true,
        at_latest: true,
        timeframe,
        instrument,
        fetched_at: Some(fetched_at),
    };
    match refresh::due(&candidate, now) {
        Ok(()) => Due::Now,
        Err(Held::Closed) if now - fetched_at > CLOSED_REFRESH_SECONDS => Due::Now,
        Err(Held::Fresh) if now - fetched_at >= refresh::period(timeframe) / 2 => Due::Soon,
        Err(_) => Due::Later,
    }
}

/// Decide what a refresh of `instruments` asks for at `now`.
///
/// Once any row that can be batched is due, the due rows go, and with them
/// the rows that would fall due within the next half period: the request is
/// one whether it names three symbols or twenty, and rows fetched together
/// stay together, rather than each falling due on its own minute and going
/// in a request of its own. A row whose market is shut is due only once a
/// day, so a list of shares and one coin asks for the coin over the
/// weekend, not for the shares beside it.
///
/// A row the batch left out last time it was asked has its bars fetched
/// instead, as a chart's would be, and is tried in a batch again a day
/// later. Without that it sits on the price it had, asked for again and
/// again in a request that never answers for it.
///
/// At most `max_batches` batches, stalest rows first. A list longer than
/// that is split across runs, and the split repeats every cycle: the rows
/// left over fall due a run after the rest, too late to join their batch.
/// So the cap is for a caller that must be quick, not a budget.
///
/// Who sends it depends on where we are running, which is the distinction
/// [`Live::warm`] exists to make.
pub fn plan_refresh(
    store: &Store,
    provider: &dyn Provider,
    instruments: &[Instrument],
    now: i64,
    max_batches: usize,
) -> Refresh {
    let daily = Timeframe::days(1);
    let batch = provider.max_batch();
    let mut bars: Vec<(i64, &Instrument)> = Vec::new();
    let mut batchable: Vec<(i64, Listing)> = Vec::new();
    let mut any_due = false;
    let mut seen = std::collections::HashSet::new();
    for instrument in instruments {
        let Some(symbol) = provider.symbol_for(instrument) else { continue };
        let key = format!("{}:{symbol}", provider.id());
        // Listed twice, in two sections or under two labels: asked for once.
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(coverage) = store.coverage(&key, daily) else {
            bars.push((0, instrument));
            continue;
        };
        let asked = store.closes_fetched_at(&key);
        let left_out = asked.is_some_and(|at| now - at < CLOSED_REFRESH_SECONDS)
            && store.load_closes(&key).is_none();
        let fetched_at =
            if left_out { coverage.fetched_at } else { coverage.fetched_at.max(asked.unwrap_or(0)) };
        let due = row_due(instrument, fetched_at, now);
        if batch == 0 || left_out {
            if due == Due::Now {
                bars.push((fetched_at, instrument));
            }
            continue;
        }
        any_due |= due == Due::Now;
        if due != Due::Later {
            batchable.push((fetched_at, Listing { key, symbol }));
        }
    }

    bars.sort_by_key(|(fetched_at, _)| *fetched_at);
    let mut plan = Refresh {
        bars: bars.into_iter().take(MAX_REFRESH).map(|(_, i)| i.clone()).collect(),
        closes: Vec::new(),
    };
    if any_due {
        batchable.sort_by_key(|(fetched_at, _)| *fetched_at);
        let listings: Vec<Listing> =
            batchable.into_iter().take(batch.saturating_mul(max_batches)).map(|(_, l)| l).collect();
        plan.closes = listings.chunks(batch).map(<[Listing]>::to_vec).collect();
    }
    plan
}

/// Fetch what `plan` asks for, here and now.
///
/// Only correct with no window open, where "here" is a short-lived process
/// that exists to answer one question and has nothing to block. Inside the
/// window this would run on the GTK main loop — see [`Live::warm`].
///
/// Through the loader rather than the provider directly, because the loader
/// is the one place that paces requests. Speculative, so they go two seconds
/// apart and are refused outright while the provider is throttling: a bar
/// widget must never cost the app its rate limit.
fn refresh(store: &Store, provider: &dyn Provider, plan: Refresh) {
    let (sender, receiver) = async_channel::unbounded();
    let loader = Loader::new(std::sync::Arc::from(crate::feeds::selected(store)), sender);
    let mut outstanding = 0;
    // The batches first: a request each for the whole list, against one each
    // for the rows that need a history.
    for listings in plan.closes {
        loader.fetch_closes(listings, BACKFILL);
        outstanding += 1;
    }
    for (rank, instrument) in plan.bars.iter().enumerate() {
        let Some(symbol) = provider.symbol_for(instrument) else { continue };
        let Some(key) = cache_key(provider, instrument) else { continue };
        let request = Request { key, symbol, timeframe: Timeframe::days(1), speculative: true };
        loader.fetch(request, BACKFILL.saturating_add(1 + rank as u32));
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
    let provider = crate::feeds::selected(store);

    let sections = store.watchlist();
    if refresh_first {
        let instruments: Vec<Instrument> = sections
            .iter()
            .flat_map(|section| section.entries.iter())
            .filter_map(|entry| index.find(&entry.symbol, entry.suffix.as_deref()).cloned())
            .collect();
        let now = chrono::Utc::now().timestamp();
        let plan = plan_refresh(store, provider.as_ref(), &instruments, now, MAX_BATCHES);
        match live {
            // Inside the window, on its main loop. Hand the work to the
            // loader thread and answer from the cache, which is all the
            // widget draws anyway: a quote that arrives on the next tick is
            // not worth six seconds of frozen application.
            Some(live) => {
                live.warm(&plan.bars);
                live.warm_closes(plan.closes);
            }
            // A process of its own, with no window and nothing to block.
            None => refresh(store, provider.as_ref(), plan),
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
            out.push_str(&entry_json(store, provider.as_ref(), instrument));
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

/// The colours the widget draws in, from the app's own palette.
///
/// Sent with the data rather than worked out in the widget, so there is one
/// palette and one mapping from the desktop theme to it: the bar and the
/// window cannot disagree about what up looks like, a bar scheme picked in
/// the window — red up, monochrome, a custom one — reaches the bar too, and
/// changing the desktop theme moves both. Every colour is the palette's ink,
/// already checked as text against the panel and a hovered row, and the change
/// pill's fill and label are the chart's own last-price tag.
///
/// Cheap enough for a command polled every couple of minutes: two small
/// files and a handful of settings rows, the same reads the window makes when
/// it starts.
fn colors_json(store: &Store) -> String {
    let ink = crate::theming::Theming::load(store).bar_widget_ink();
    let tag = |tag: &omacharts_engine::Tag| {
        format!("{{\"fill\":{},\"label\":{}}}", json_string(&tag.fill), json_string(&tag.label))
    };
    format!(
        "{{\"text\":{},\"muted\":{},\"up\":{},\"down\":{},\"flat\":{},\"hover\":{},\"tags\":{{\"up\":{},\"down\":{},\"flat\":{}}}}}",
        json_string(&ink.text),
        json_string(&ink.muted),
        json_string(&ink.up),
        json_string(&ink.down),
        json_string(&ink.flat),
        json_string(&ink.hover),
        tag(&ink.up_tag),
        tag(&ink.down_tag),
        tag(&ink.flat_tag),
    )
}

fn entry_json(store: &Store, provider: &dyn Provider, instrument: &Instrument) -> String {
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
    let quote = quote(store, provider, instrument);
    if let Some(key) = cache_key(provider, instrument) {
        let bars = daily_bars(store, &key, instrument.kind);
        let mut closes: Vec<(i64, f64)> = bars.iter().map(|bar| (bar.ts, bar.close)).collect();
        // The line ends where the price beside it does. A batch of closes is
        // usually newer than the bars: its close replaces the day it shares
        // with them, or follows them for a day they do not have yet.
        if let Some(q) = &quote {
            let day = session_day(q.as_of);
            match closes.last_mut() {
                Some(last) if session_day(last.0) == day => last.1 = q.last,
                Some(last) if session_day(last.0) < day => closes.push((q.as_of, q.last)),
                _ => {}
            }
        }
        let tail = &closes[closes.len().saturating_sub(SPARK_POINTS)..];
        if tail.len() >= 2 {
            let points: Vec<String> =
                tail.iter().map(|(_, close)| format!("{close:.6}")).collect();
            fields.push_str(&format!(",\"spark\":[{}]", points.join(",")));
        }
    }
    match quote {
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

    fn adopt_feed(&self) {
        crate::ui::Window::adopt_feed(self);
    }

    fn adopt_header(&self) {
        crate::ui::Window::apply_header(self);
    }

    fn adopt_clock(&self) {
        crate::ui::Window::adopt_clock(self);
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

    fn warm_closes(&self, batches: Vec<Vec<Listing>>) {
        crate::ui::Window::warm_closes(self, batches);
    }

    fn symbols(&self) -> Option<Rc<SearchIndex>> {
        crate::ui::Window::symbols(self)
    }

    fn streaming(&self) -> Vec<crate::live::Report> {
        crate::ui::Window::streaming(self)
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
    use omacharts_engine::providers::Yahoo;

    fn line(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn a_launch_option_comes_off_and_leaves_the_rest_alone() {
        let peeled = peel_launch(&line(&["omacharts", "--provider", "tos", "NVDA"])).unwrap();
        assert_eq!(peeled.args, line(&["omacharts", "NVDA"]));
        assert_eq!(peeled.provider.map(|feed| feed.id), Some("tos"));

        // The feed's name must not survive as a symbol — this is the bug the
        // peeling exists to prevent, because the symbol filter only drops
        // arguments that start with a dash.
        assert!(!peeled.args.iter().any(|arg| arg == "tos"));
    }

    #[test]
    fn it_is_written_either_way_round() {
        for written in [
            line(&["omacharts", "--provider=tos"]),
            line(&["omacharts", "--provider", "tos"]),
        ] {
            assert_eq!(peel_launch(&written).unwrap().provider.map(|f| f.id), Some("tos"));
        }
    }

    /// A command keeps being a command with a launch option in front of it,
    /// which is what `is_command` and `runs_in_the_caller` read `args[1]`
    /// for.
    #[test]
    fn a_command_behind_a_launch_option_is_still_that_command() {
        let peeled = peel_launch(&line(&["omacharts", "--provider", "tos", "watchlist", "list"]))
            .unwrap();
        assert!(is_command(&peeled.args));
        let local = peel_launch(&line(&["omacharts", "--provider", "yahoo", "skill", "status"]))
            .unwrap();
        assert!(runs_in_the_caller(&local.args));
    }

    #[test]
    fn nothing_to_peel_is_the_line_unchanged() {
        let given = line(&["omacharts", "chart", "symbol", "AAPL"]);
        let peeled = peel_launch(&given).unwrap();
        assert_eq!(peeled.args, given);
        assert!(peeled.provider.is_none());
    }

    /// The bar widget names the list it shows, so a click moves the window to
    /// it. Kept as typed: there is no database to resolve it against yet.
    #[test]
    fn a_watchlist_comes_off_as_typed_and_leaves_the_symbol() {
        let peeled =
            peel_launch(&line(&["omacharts", "--watchlist", "id:1", "SAP", "DE"])).unwrap();
        assert_eq!(peeled.args, line(&["omacharts", "SAP", "DE"]));
        assert_eq!(peeled.watchlist.as_deref(), Some("id:1"));
        assert!(!is_command(&peeled.args));
    }

    /// A command run as if it were scoped to a list, when it was not, is the
    /// answer an agent cannot tell from the right one.
    #[test]
    fn a_watchlist_in_front_of_a_command_is_refused() {
        let fault = peel_launch(&line(&["omacharts", "--watchlist", "Tech", "watchlist", "list"]))
            .unwrap_err();
        assert_eq!(fault.code, EXIT_USAGE);
        assert!(fault.message.contains("--watchlist"), "{}", fault.message);
    }

    #[test]
    fn a_watchlist_is_found_by_name_or_id_and_refused_when_missing() {
        let store = Store::memory().expect("an in-memory store");
        let id = store.add_watchlist("Tech").expect("a watchlist");
        assert_eq!(watchlist_named(&store, "tech").unwrap(), (id, "Tech".into()));
        assert_eq!(watchlist_named(&store, &format!("id:{id}")).unwrap().0, id);
        assert_eq!(watchlist_named(&store, "id:1").unwrap().0, crate::store::DEFAULT_WATCHLIST);
        assert_eq!(watchlist_named(&store, "Nowhere").unwrap_err().code, EXIT_NOT_FOUND);
    }

    /// Charting from the wrong source because a name was misspelled is worse
    /// than refusing to start, and a script that typos a feed wants to know.
    #[test]
    fn a_feed_we_do_not_have_is_a_usage_error_naming_the_ones_we_do() {
        let fault = peel_launch(&line(&["omacharts", "--provider", "bloomberg"])).unwrap_err();
        assert_eq!(fault.code, EXIT_USAGE);
        assert!(fault.message.contains("bloomberg"), "{}", fault.message);
        assert!(fault.message.contains("yahoo"), "{}", fault.message);
        assert!(fault.message.contains("tos"), "{}", fault.message);
    }

    #[test]
    fn an_option_with_nothing_after_it_says_what_it_wanted() {
        let fault = peel_launch(&line(&["omacharts", "--provider"])).unwrap_err();
        assert_eq!(fault.code, EXIT_USAGE);
        assert!(fault.message.contains("NAME"), "{}", fault.message);
    }

    /// Everything else that looks like an option is left where it was, for
    /// `peel_off` and clap to have their say — a launch option list is not a
    /// licence to swallow arguments.
    #[test]
    fn another_option_is_not_ours_to_take() {
        let given = line(&["omacharts", "--nonsense", "--version"]);
        assert_eq!(peel_launch(&given).unwrap().args, given);
    }

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
        fn warm_closes(&self, _batches: Vec<Vec<Listing>>) {}
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
        let outcome = run(&args, &store, Some(&Breaks), &Here);
        assert_eq!(outcome.code, EXIT_BUG);
        assert!(outcome.err.contains("status show"), "it has to say which: {}", outcome.err);
    }

    /// `main` asks this to decide where a command runs, so the answer has to
    /// come out of the table rather than from a noun's name written down a
    /// second time there — and it is the whole noun either way. A `--help` or
    /// a misspelled flag answered by the window would be right about the
    /// window's machine and wrong about the one somebody typed on.
    #[test]
    fn a_caller_local_noun_runs_in_the_caller_however_it_is_typed() {
        for noun in spec::SURFACE {
            for tail in ["", " status", " install --to", " --help", " nonsense"] {
                let typed = format!("omacharts {}{tail}", noun.name);
                let args: Vec<String> = typed.split_whitespace().map(String::from).collect();
                assert_eq!(runs_in_the_caller(&args), noun.local, "{typed}");
            }
        }
    }

    /// The bug this is here for: with a window open, `skill` was handed to it,
    /// so `CODEX_HOME` came from the window's environment and a relative
    /// `--to` resolved against the window's working directory — never the
    /// directory the person was standing in.
    /// A relative path to a file is made absolute where it was typed, in
    /// front of launch options too; a backup's name is left as it is.
    #[test]
    fn a_relative_file_to_restore_is_settled_where_it_was_typed() {
        let dir = std::env::temp_dir().join(format!("omacharts-caller-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("theirs.db");
        std::fs::write(&file, b"x").unwrap();
        let here = std::env::current_dir().unwrap();
        let relative = pathdiff(&file, &here);
        let line = |tail: &[&str]| {
            let mut args = vec!["omacharts".to_string(), "--provider".into(), "yahoo".into()];
            args.extend(tail.iter().map(|s| s.to_string()));
            args
        };
        let settled = caller_paths(line(&["backup", "restore", &relative]));
        let settled = std::path::Path::new(&settled[5]);
        assert!(settled.is_absolute(), "{}", settled.display());
        assert_eq!(settled.canonicalize().unwrap(), file.canonicalize().unwrap());
        let named = caller_paths(line(&["backup", "restore", "settings-20261021-140307"]));
        assert_eq!(named[5], "settings-20261021-140307");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `to`, relative to `from`, with as many `..` as it takes.
    fn pathdiff(to: &std::path::Path, from: &std::path::Path) -> String {
        let up = from.components().count();
        let mut out = "../".repeat(up);
        out.push_str(to.strip_prefix("/").unwrap().to_str().unwrap());
        out
    }

    #[test]
    fn skill_is_a_caller_local_noun() {
        let skill = spec::SURFACE.iter().find(|noun| noun.name == "skill").expect("a skill noun");
        assert!(skill.local);
    }

    #[test]
    fn a_symbol_or_a_bare_option_is_not_a_caller_local_command() {
        for typed in ["omacharts", "omacharts NVDA", "omacharts --help", "omacharts --version"] {
            let args: Vec<String> = typed.split_whitespace().map(String::from).collect();
            assert!(!runs_in_the_caller(&args), "{typed}");
        }
    }

    #[test]
    fn the_payload_carries_the_palettes_ink() {
        let store = Store::memory().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
        for key in ["text", "muted", "up", "down", "flat", "hover"] {
            let value = parsed[key].as_str().unwrap_or_default();
            assert!(value.starts_with('#') && value.len() >= 7, "{key}: {value:?}");
        }
        assert_ne!(parsed["up"], parsed["down"], "up and down must differ");
        for way in ["up", "down", "flat"] {
            for part in ["fill", "label"] {
                let value = parsed["tags"][way][part].as_str().unwrap_or_default();
                assert!(value.starts_with('#') && value.len() >= 7, "tags.{way}.{part}: {value:?}");
            }
        }

        // The bar reads a rise the way the window does.
        store.set_setting(crate::theming::SETTING_BARS, omacharts_engine::theme::THEME_RED_UP_ID);
        let red: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
        assert_eq!((&red["up"], &red["down"]), (&parsed["down"], &parsed["up"]));
        assert_eq!(
            (&red["tags"]["up"], &red["tags"]["down"]),
            (&parsed["tags"]["down"], &parsed["tags"]["up"])
        );
    }

    /// The scheme chosen in the window reaches the bar: somebody who reads a
    /// rise as red reads it that way in the bar too.
    #[test]
    fn the_payload_follows_whichever_bar_scheme_was_chosen() {
        let store = Store::memory().unwrap();
        let theme: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
        store.set_setting(crate::theming::SETTING_BARS, omacharts_engine::theme::THEME_RED_UP_ID);
        let chosen: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
        assert_ne!(chosen["up"], theme["up"]);
        assert_ne!(chosen["down"], theme["down"]);
        // Surfaces and text are the theme's, whatever the candles wear.
        assert_eq!(chosen["text"], theme["text"]);
        assert_eq!(chosen["hover"], theme["hover"]);
    }

    /// Every scheme on offer — filled, hollow, red up and monochrome — reaches
    /// the widget whole: the change text, the sparklines (which
    /// are drawn in the same up and down) and the pill all come from the
    /// chosen scheme's colours, by the same rules the chart applies to them.
    #[test]
    fn every_scheme_on_offer_reaches_the_widget() {
        use omacharts_engine::{ink::legible, Tag};
        let store = Store::memory().unwrap();
        let theming = crate::theming::Theming::load(&store);
        let schemes = omacharts_engine::theme::bar_schemes(&theming.theme());
        assert_eq!(schemes.len(), 4);
        for scheme in schemes {
            store.set_setting(crate::theming::SETTING_BARS, &scheme.id);
            let sent: serde_json::Value = serde_json::from_str(&colors_json(&store)).unwrap();
            let theme = crate::theming::Theming::load(&store);
            let ui = theme.theme().ui;
            let hover = sent["hover"].as_str().unwrap().to_string();
            let grounds = [ui.background.as_str(), ui.surface.as_str(), hover.as_str()];
            for (way, colour) in [("up", &scheme.up), ("down", &scheme.down)] {
                assert_eq!(sent[way], legible(colour, &grounds).as_str(), "{}: {way}", scheme.id);
                let tag = Tag::on(colour);
                assert_eq!(sent["tags"][way]["fill"], tag.fill.as_str(), "{}: {way} pill", scheme.id);
                assert_eq!(sent["tags"][way]["label"], tag.label.as_str(), "{}: {way} label", scheme.id);
            }
        }
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

    // -- refreshing the widget's rows ---------------------------------------

    /// Wednesday 2026-10-14, 11:00 in New York: US shares trading.
    const OPEN: i64 = 1_791_990_000;
    /// Saturday 2026-10-17, noon in New York: nothing but crypto trading.
    const SATURDAY: i64 = 1_792_252_800;

    fn listed(symbol: &str) -> Instrument {
        let index = SearchIndex::new(omacharts_engine::symbols::seed());
        let mut instrument = index.find("AAPL", None).unwrap().clone();
        instrument.symbol = symbol.to_string();
        instrument
    }

    /// `symbol` with two daily bars cached, last fetched at `fetched_at`.
    fn cached(store: &Store, provider: &dyn Provider, symbol: &str, fetched_at: i64) -> Instrument {
        let instrument = listed(symbol);
        let key = cache_key(provider, &instrument).unwrap();
        let bar = |ts| omacharts_engine::Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        store.write_bars(&key, Timeframe::days(1), &[bar(86_400), bar(172_800)]);
        store.backdate(&key, Timeframe::days(1), fetched_at);
        instrument
    }

    fn batched(plan: &Refresh) -> Vec<Vec<&str>> {
        plan.closes.iter().map(|b| b.iter().map(|l| l.symbol.as_str()).collect()).collect()
    }

    /// One row due brings the rows over halfway to due with it, in the one
    /// request it costs anyway, so that they fall due together from then on.
    /// A row fetched a minute ago stays out, whether by its bars or a batch.
    #[test]
    fn one_stale_row_brings_the_rows_nearly_due_in_one_request() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let rows = [
            cached(&store, &provider, "FRESH", OPEN - 60),
            cached(&store, &provider, "BATCHED", OPEN - 3_600),
            cached(&store, &provider, "HALFWAY", OPEN - 600),
            cached(&store, &provider, "STALE", OPEN - 3_600),
        ];
        let closes = omacharts_engine::Closes {
            symbol: "BATCHED".into(), last: 2.0, previous: 1.0, at: OPEN, previous_at: OPEN - 86_400,
        };
        store.write_closes("yahoo:BATCHED", &closes);
        store.backdate_closes("yahoo:BATCHED", OPEN - 60);
        let plan = plan_refresh(&store, &provider, &rows, OPEN, MAX_BATCHES);
        assert!(plan.bars.is_empty());
        assert_eq!(batched(&plan), vec![vec!["STALE", "HALFWAY"]], "stalest first, one request");
    }

    /// Yahoo takes twenty to a request, and a run sends at most three.
    #[test]
    fn a_long_list_is_cut_into_batches_the_provider_takes() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let rows: Vec<Instrument> =
            (0..70).map(|i| cached(&store, &provider, &format!("S{i:02}"), OPEN - 3_600 - i)).collect();
        let plan = plan_refresh(&store, &provider, &rows, OPEN, MAX_BATCHES);
        let sizes: Vec<usize> = plan.closes.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![20, 20, 20]);
        assert_eq!(plan.closes[0][0].symbol, "S69", "the stalest go first; the rest next run");
    }

    /// Saturday: a shut market is asked for nothing while its close is
    /// recent and once a day otherwise, so a widget left off since Thursday
    /// does not show Thursday all weekend. A coin trades through it, and
    /// brings no shut row with it that is not due.
    #[test]
    fn a_shut_market_is_asked_once_a_day_at_most() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let friday = cached(&store, &provider, "FRI", SATURDAY - 3 * 3_600);
        let thursday = cached(&store, &provider, "THU", SATURDAY - 2 * 86_400);
        let coin = Instrument { kind: omacharts_engine::InstrumentKind::Crypto, ..listed("BTC") };
        let key = cache_key(&provider, &coin).unwrap();
        let bar = |ts| omacharts_engine::Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        store.write_bars(&key, Timeframe::days(1), &[bar(86_400), bar(172_800)]);
        store.backdate(&key, Timeframe::days(1), SATURDAY - 3_600);

        let plan = plan_refresh(&store, &provider, &[friday, thursday, coin], SATURDAY, MAX_BATCHES);
        assert_eq!(batched(&plan), vec![vec!["THU", "BTC-USD"]]);
    }

    /// thinkorswim streams, but not into a widget process with no window: a
    /// stale row there is fetched by its bars, as it always was, or its
    /// price would stop at the first one fetched.
    #[test]
    fn a_streamed_feed_still_plans_its_stale_rows() {
        let store = Store::memory().unwrap();
        let provider = omacharts_engine::providers::Tos::new();
        let row = cached(&store, &provider, "AAPL", OPEN - 3_600);
        let plan = plan_refresh(&store, &provider, std::slice::from_ref(&row), OPEN, MAX_BATCHES);
        assert_eq!((plan.bars.len(), plan.closes.len()), (1, 0));
    }

    /// A row the batch left out goes to its bars, at the cadence a chart
    /// keeps, rather than sitting on the price it had for good; a day later
    /// the batch gets another try.
    #[test]
    fn a_row_the_batch_left_out_gets_its_bars() {
        let store = Store::memory().unwrap();
        let provider = Yahoo::new();
        let row = cached(&store, &provider, "GONE", OPEN - 3_600);
        let key = cache_key(&provider, &row).unwrap();
        store.mark_closes_asked(&key);
        store.backdate_closes(&key, OPEN - 60);
        let plan = plan_refresh(&store, &provider, std::slice::from_ref(&row), OPEN, MAX_BATCHES);
        assert_eq!(plan.bars.len(), 1, "due by its bars, fetched an hour ago");
        assert!(plan.closes.is_empty());

        store.backdate_closes(&key, OPEN - 2 * 86_400);
        let plan = plan_refresh(&store, &provider, &[row], OPEN, MAX_BATCHES);
        assert_eq!(batched(&plan), vec![vec!["GONE"]]);
    }

    /// EURUSD on Thursday 2026-10-15 at 10:57 UTC. The bars stamp its days
    /// at London midnight, 23:00 UTC the evening before, today's included;
    /// the batch's closes, taken from Yahoo's own answer, must land on the
    /// same day rather than a day after it, or the move is measured against
    /// today's own price and the line draws today twice.
    #[test]
    fn an_fx_batch_and_its_bars_agree_on_the_day() {
        let store = Store::memory().unwrap();
        let key = "yahoo:EURUSD=X";
        let bar = |ts, close| omacharts_engine::Bar { ts, open: close, high: close, low: close, close, volume: 0.0 };
        let (tuesday, wednesday, thursday) = (1_791_846_000, 1_791_932_400, 1_792_018_800);
        store.write_bars(key, Timeframe::days(1), &[bar(tuesday, 1.116), bar(wednesday, 1.1204), bar(thursday, 1.1210)]);
        store.backdate(key, Timeframe::days(1), thursday + 3_600);
        let body = serde_json::json!({ "EURUSD=X": { "symbol": "EURUSD=X",
            "timestamp": [tuesday, wednesday, 1_792_061_850],
            "close": [1.116, 1.1204, 1.1217] } });
        let closes = omacharts_engine::providers::yahoo::parse_spark(&body).unwrap();
        store.write_closes(key, &closes[0]);

        let q = quote_at(&store, key).unwrap();
        assert_eq!(q.last, 1.1217);
        assert!((q.change - 0.0013).abs() < 1e-9, "against Wednesday, not today: {}", q.change);
        assert_eq!(q.as_of, thursday);

        let eurusd = Instrument { kind: omacharts_engine::InstrumentKind::Fx, ..listed("EURUSD") };
        let parsed: serde_json::Value =
            serde_json::from_str(&entry_json(&store, &Yahoo::new(), &eurusd)).unwrap();
        let spark: Vec<f64> =
            parsed["spark"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(spark, vec![1.116, 1.1204, 1.1217], "today once, at the batch's price");

        // A chart fetched after the batch has the fresher price for the day.
        let fresher = [bar(tuesday, 1.116), bar(wednesday, 1.1204), bar(thursday, 1.1220)];
        store.write_bars(key, Timeframe::days(1), &fresher);
        store.backdate(key, Timeframe::days(1), thursday + 7_200);
        store.backdate_closes(key, thursday + 60);
        assert_eq!(quote_at(&store, key).unwrap().last, 1.1220);
    }

    /// FX's session opens at London midnight: 23:00 UTC in summer, 00:00 in
    /// winter. A stamp from either side of the clock change is the same
    /// session, where its UTC date would put the two a day apart.
    #[test]
    fn a_session_keeps_its_day_across_the_clock_change() {
        let (thursday_summer, thursday_winter) = (1_792_018_800, 1_792_022_400);
        assert_eq!(session_day(thursday_summer), session_day(thursday_winter));
        assert_ne!(thursday_summer.div_euclid(86_400), thursday_winter.div_euclid(86_400));
    }

    /// A batch that skipped a session: the bars' close in the gap is the
    /// previous one, not the batch's, which is two sessions old.
    #[test]
    fn a_batch_missing_a_session_takes_the_previous_close_from_the_bars() {
        let store = Store::memory().unwrap();
        let key = "yahoo:BTC-USD";
        let bar = |day: i64, close| omacharts_engine::Bar { ts: day * 86_400, open: close, high: close, low: close, close, volume: 1.0 };
        store.write_bars(key, Timeframe::days(1), &[bar(1, 100.0), bar(2, 110.0)]);
        store.write_closes(key, &omacharts_engine::Closes {
            symbol: "BTC-USD".into(), last: 111.0, previous: 100.0, at: 3 * 86_400, previous_at: 86_400,
        });
        let q = quote_at(&store, key).unwrap();
        assert_eq!((q.last, q.change), (111.0, 1.0), "against day 2's close, not day 1's");

        // With no gap the batch's own previous close stands, though the bars
        // were fetched mid-session and hold an earlier price for that day.
        store.write_closes(key, &omacharts_engine::Closes {
            symbol: "BTC-USD".into(), last: 111.0, previous: 109.0, at: 3 * 86_400, previous_at: 2 * 86_400,
        });
        assert_eq!(quote_at(&store, key).unwrap().change, 2.0);
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
