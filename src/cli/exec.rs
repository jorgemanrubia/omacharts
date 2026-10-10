//! Carrying out a command.
//!
//! One arm per verb in [`super::spec`], and nothing here decides what the
//! surface is — the table does. Adding a command means adding it there and
//! here, and the test at the bottom of `parser` fails if only one of the two
//! happened.
//!
//! Every arm returns text rather than printing, because the same code answers
//! a terminal in this process and a terminal in somebody else's. See
//! [`super::Outcome`].

use serde_json::{json, Value};

use omacharts_engine::indicators::{
    LineStyle, Stroke, MAX_FILL_ALPHA, MAX_PANE_SHARE, MIN_FILL_ALPHA, MIN_PANE_SHARE,
};
use omacharts_engine::providers;
use omacharts_engine::theme::{
    ColorChoice, SWATCH_NAMES, THEME_BARS_ID, THEME_MONO_ID, THEME_RED_UP_ID,
};
use omacharts_engine::{
    link, BarStyle, Indicator, IndicatorKind, LinkGroup, Reset, Session, Timeframe,
};

use super::charts::{self, Workspace};
use super::{parser, Caller, Fault, Live, Outcome, EXIT_USAGE};
use crate::store::{Entry, Section, Store, DEFAULT_WATCHLIST};

/// Parse and run.
pub fn dispatch(
    args: &[String],
    store: &Store,
    live: Option<&dyn Live>,
    caller: &dyn Caller,
) -> Outcome {
    // `omacharts watchlist [--refresh]` was the whole command line once, and
    // it is what the bar widget installed on people's desktops still runs on a
    // timer. `watchlist` grew subcommands around it; this keeps the bare form
    // meaning what it has always meant rather than answering a widget with a
    // usage error.
    if args.get(1).map(String::as_str) == Some("watchlist")
        && args.iter().skip(2).all(|arg| arg == "--refresh")
    {
        return Outcome::ok(format!(
            "{}\n",
            super::watchlist_json(store, args.iter().any(|arg| arg == "--refresh"), live)
        ));
    }

    let parsed = match parser::command().try_get_matches_from(args) {
        Ok(parsed) => parsed,
        // Clap's own rendering, which is where "did you mean" and the usage
        // line come from. A help or version request is a success that happens
        // to arrive as an error.
        Err(error) => {
            let text = error.render().to_string();
            return match error.use_stderr() {
                false => Outcome::ok(text),
                true => Outcome { code: EXIT_USAGE, out: String::new(), err: text },
            };
        }
    };

    let Some((noun, rest)) = parsed.subcommand() else {
        return Outcome::ok(parser::command().render_help().to_string());
    };

    if noun == "help" {
        return help_for(rest.get_many::<String>("COMMAND").map(|c| c.cloned().collect()));
    }
    if noun == "surface" {
        return surface(rest);
    }

    let Some((verb, m)) = rest.subcommand() else {
        return Outcome::ok(parser::command().render_help().to_string());
    };
    let Some(spec) = super::spec::verb(noun, verb) else {
        return Outcome::failed(Fault::usage(format!("no such command: {noun} {verb}")));
    };

    // A command that reads the arrangement has to see what is on screen, not
    // what was saved a moment ago; one that writes it must not be overwritten
    // by the next save. Both are the same hand-off, so both happen here
    // rather than in every arm that could forget.
    if let Some(live) = live.filter(|_| spec.workspace) {
        live.flush_workspace();
    }

    let json = flag(m, "json");

    // Nothing to do with the database or the window: these reach into an
    // agent's configuration, and only ever because somebody typed them. See
    // `super::skill` for why that is the whole design.
    //
    // Answered here rather than in the table of arms below because a run over
    // two agents can half work — installed for one, refused for the other —
    // and a `Result<String, Fault>` has nowhere to put both halves.
    if noun == "skill" {
        return super::skill::run(verb, &chosen_agents(m), json);
    }

    let result = match (noun, verb) {
        ("symbol", "search") => symbol_search(m, json),
        ("symbol", "show") => symbol_show(m, json),

        ("watchlist", "list") => watchlist_list(store, json),
        ("watchlist", "show") => watchlist_show(store, m, json),
        ("watchlist", "create") => watchlist_create(store, m, json),
        ("watchlist", "rename") => watchlist_rename(store, m, json),
        ("watchlist", "delete") => watchlist_delete(store, m, json),
        ("watchlist", "add") => watchlist_add(store, m, json, true),
        ("watchlist", "remove") => watchlist_add(store, m, json, false),
        ("watchlist", "move") => watchlist_move(store, m, json),
        ("watchlist", "link") => watchlist_link(store, m, json),
        ("watchlist", "feed") => Ok(super::watchlist_json(store, flag(m, "refresh"), live)),
        ("watchlist", "export") => watchlist_export(store, m),
        ("watchlist", "import") => watchlist_import(store, m, json, caller),

        ("section", "list") => section_list(store, m, json),
        ("section", "create") => section_create(store, m, json),
        ("section", "rename") => section_rename(store, m, json),
        ("section", "delete") => section_delete(store, m, json),
        ("section", "promote") => section_promote(store, m, json),
        ("section", "order") => section_order(store, m, json),

        ("status", "show") => status(store, live.is_some(), json),
        // Before the arm below, which answers out of the stored arrangement.
        // These two cannot: there are no pixels in a saved layout.
        ("chart", "screenshot") => screenshot(live, m, json, false),
        ("chartbook", "screenshot") => screenshot(live, m, json, true),
        ("chart", "crosshair") => crosshair(store, m, json),
        ("chartbook", _) | ("chart", _) => {
            // "The one I am looking at" needs something to be looking at. The
            // stored arrangement is what the window had when it last closed,
            // and an agent cannot tell that from what is on screen now — so a
            // command with no target is refused rather than answered from it.
            if live.is_none() && !names_a_target(noun, verb, m) {
                return Outcome::failed(Fault::no_window(match noun {
                    "chartbook" => "open chartbook",
                    _ => "focused chart",
                }));
            }
            charts_verb(store, noun, verb, m, json)
        }

        ("provider", "list") => provider_list(store, json),
        ("provider", "status") => provider_status(store, live, json),
        ("provider", "login") => provider_login(store, m, json),
        ("provider", "logout") => provider_logout(store, m, json),

        ("config", "list") => config_list(store, json),
        ("config", "get") => config_get(store, m, json),
        ("config", "set") => {
            let outcome = config_set(store, m, json);
            // The feed is the one setting a window acts on the moment it
            // changes: the charts switch, with nothing to restart.
            if outcome.is_ok()
                && required(m, "KEY").is_ok_and(|key| key == crate::feeds::SETTING)
                && let Some(live) = live
            {
                live.adopt_feed();
            }
            outcome
        }
        ("config", "bars") => config_bars(store, m, json, live),

        ("plugin", "status") => plugin_status(json),
        ("plugin", "install") => plugin_install(json),
        ("plugin", "uninstall") => plugin_uninstall(json),

        ("cache", "status") => cache_status(store, json),
        ("cache", "clear") => cache_clear(store, json),
        ("cache", "limit") => cache_limit(store, m, json),

        _ => Err(Fault::usage(format!("no such command: {noun} {verb}"))),
    };

    match result {
        Err(fault) => Outcome::failed(fault),
        Ok(out) => {
            // Only after it worked, and only what it touched. Rebuilding the
            // window because a setting changed would be the kind of thing
            // that is fine with five symbols and stutters with five hundred.
            if let Some(live) = live.filter(|_| spec.writes) {
                match spec.workspace {
                    true => live.reload_workspace(),
                    false => live.reload_watchlists(),
                }
            }
            Outcome::ok(out)
        }
    }
}

/// Did the command say which chartbook or chart it meant?
///
/// Verbs that take one as a required argument always have; the rest default
/// to what is on screen, and that default only exists while something is.
fn names_a_target(noun: &str, verb: &str, m: &clap::ArgMatches) -> bool {
    if arg(m, "BOOK").is_some() || arg(m, "book").is_some() {
        return true;
    }
    // Creating one invents its own target, and listing them all needs none.
    // The drawing configurations are about every chart rather than one.
    matches!((noun, verb), ("chartbook", "create") | ("chartbook", "list"))
        || (noun == "chart"
            && verb == "drawing"
            && matches!(
                arg(m, "ACTION").map(String::as_str),
                Some("configs" | "configure" | "reset-configs")
            ))
}

fn help_for(path: Option<Vec<String>>) -> Outcome {
    let mut cmd = parser::command();
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return Outcome::ok(cmd.render_help().to_string());
    };
    for step in &path {
        let found = cmd.get_subcommands().find(|c| c.get_name() == step).cloned();
        match found {
            Some(next) => cmd = next,
            None => {
                return Outcome::failed(Fault::usage(format!(
                    "no such command: {}; try `omacharts --help`",
                    path.join(" ")
                )))
            }
        }
    }
    Outcome::ok(cmd.render_help().to_string())
}

fn surface(m: &clap::ArgMatches) -> Outcome {
    if let Some(shell) = arg(m, "completions") {
        return Outcome::ok(completions(shell));
    }
    if flag(m, "man") {
        return Outcome::ok(man_page());
    }
    Outcome::ok(format!("{}\n", parser::surface_json()))
}

// -- symbols --------------------------------------------------------------

fn symbol_search(m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let query = required(m, "QUERY")?;
    let limit: usize = m
        .get_one::<String>("limit")
        .map(|l| l.parse().unwrap_or(10))
        .unwrap_or(10);
    let index = crate::inventory::everything();
    let hits: Vec<&omacharts_engine::Instrument> =
        index.search(query, limit).iter().filter_map(|h| index.get(h.index)).collect();
    if hits.is_empty() {
        return Err(Fault::not_found(format!("nothing matches {query:?}")));
    }
    if as_json {
        return Ok(wrap_list("symbols", hits.iter().map(|i| instrument_json(i)).collect()));
    }
    Ok(hits
        .iter()
        .map(|i| {
            format!(
                "{:<10} {:<34} {:<8} {}",
                i.display_symbol(),
                truncate(&i.full_name(), 34),
                i.kind.label(),
                i.exchange_label().unwrap_or("")
            )
            .trim_end()
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn symbol_show(m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let index = crate::inventory::everything();
    let (symbol, suffix) =
        listing(&index, required(m, "SYMBOL")?, arg(m, "SUFFIX").map(|s| s.to_uppercase()));
    let found = index.find(&symbol, suffix.as_deref()).ok_or_else(|| {
        Fault::not_found(format!(
            "no instrument called {}; try `omacharts symbol search {symbol}`",
            spell(&symbol, suffix.as_deref())
        ))
    })?;
    if as_json {
        return Ok(format!("{}\n", instrument_json(found)));
    }
    Ok(format!(
        "{}\n{}\nkind      {}\nexchange  {}\ncurrency  {}\n",
        found.display_symbol(),
        found.full_name(),
        found.kind.label(),
        found.exchange_label().unwrap_or("unknown"),
        found.currency.as_deref().unwrap_or("unknown"),
    ))
}

fn instrument_json(i: &omacharts_engine::Instrument) -> String {
    json!({
        "symbol": i.symbol,
        "suffix": i.suffix,
        "display": i.display_symbol(),
        "name": i.name,
        "local_name": i.local_name,
        "kind": i.kind.label(),
        "exchange": i.exchange_label(),
        "currency": i.currency,
    })
    .to_string()
}

// -- watchlists -----------------------------------------------------------

/// Resolve `a name, or id:N` against the watchlists.
fn find_list(store: &Store, selector: &str) -> Result<(i64, String), Fault> {
    let lists = store.watchlists();
    if let Some(rest) = selector.strip_prefix("id:") {
        let id: i64 = rest
            .parse()
            .map_err(|_| Fault::usage(format!("{selector:?} is not a watchlist id")))?;
        return lists
            .iter()
            .find(|(i, _)| *i == id)
            .cloned()
            .ok_or_else(|| Fault::not_found(format!("no watchlist with id {id}")));
    }
    let wanted = selector.to_lowercase();
    let hits: Vec<&(i64, String)> =
        lists.iter().filter(|(_, name)| name.to_lowercase() == wanted).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(Fault::not_found(format!(
            "no watchlist called {selector:?}; try `omacharts watchlist list`"
        ))),
        many => Err(Fault::ambiguous(format!(
            "{} watchlists are called {selector:?}; use {}",
            many.len(),
            many.iter().map(|(i, _)| format!("id:{i}")).collect::<Vec<_>>().join(" or ")
        ))),
    }
}

/// The watchlist a command acts on when none was named.
fn list_or_default(store: &Store, m: &clap::ArgMatches) -> Result<(i64, String), Fault> {
    match arg(m, "LIST") {
        Some(selector) => find_list(store, selector),
        None => find_list(store, &format!("id:{DEFAULT_WATCHLIST}")),
    }
}

/// A link group as a chart and a watchlist both spell one: `1`-`9`, or `none`.
fn link_group(text: &str) -> Result<u8, Fault> {
    if text.eq_ignore_ascii_case("none") {
        return Ok(0);
    }
    text.parse::<u8>()
        .ok()
        .filter(|group| (1..=link::GROUP_COUNT).contains(group))
        .ok_or_else(|| {
            Fault::usage(format!(
                "{text:?} is not a link group; use 1-{} or none",
                link::GROUP_COUNT
            ))
        })
}

/// Where a watchlist's link group is written down.
///
/// Against the watchlist's id, because the group belongs to the list rather
/// than to the rail: the same list driving group 3 under one chartbook drives
/// group 3 under the next. The rail's own helper for this key is private to
/// the window, so the spelling is here as well — and
/// `the_group_a_command_sets_is_the_one_the_rail_reads` writes through this
/// one and reads back through that one, so the two cannot drift apart
/// unnoticed.
pub(super) fn link_setting(watchlist: i64) -> String {
    format!("watchlist_link_{watchlist}")
}

/// The group a watchlist drives, as the rail would report it.
///
/// Asked of the rail's own scan rather than of the setting, so both of its
/// rules are applied here too: the default watchlist drives group 1 until
/// something says otherwise, and where two lists claim one group the earlier
/// in display order keeps it. Zero is no group.
fn link_group_of(store: &Store, watchlist: i64) -> u8 {
    crate::ui::watchlist::group_owners(store)
        .into_iter()
        .find(|(_, id, _)| *id == watchlist)
        .map(|(group, ..)| group)
        .unwrap_or(0)
}

/// Read, set or clear the group a watchlist drives.
///
/// A group drives one list, so one already taken is refused rather than
/// stolen — the rail greys that row out instead of offering it, and a command
/// that quietly moved it would change which charts a list drives without
/// saying so.
fn watchlist_link(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (id, name) = find_list(store, required(m, "LIST")?)?;
    let Some(wanted) = arg(m, "GROUP") else {
        let group = link_group_of(store, id);
        return match as_json {
            true => Ok(format!("{}\n", json!({"id": id, "name": name, "group": group}))),
            false => Ok(format!("{}\n", spell_group(group))),
        };
    };
    let group = link_group(wanted)?;
    if let Some((_, holder)) =
        crate::ui::watchlist::group_held_by(store, LinkGroup::numbered(group), id)
    {
        return Err(Fault::refused(format!(
            "link group {group} already drives {holder:?}; take it off that list first"
        )));
    }
    store.set_setting(&link_setting(id), &group.to_string());
    let text = match group {
        0 => format!("{name:?} drives no link group"),
        group => format!("{name:?} now drives link group {group}"),
    };
    said(as_json, json!({"id": id, "name": name, "group": group}), text)
}

/// A group where somebody reads it. Zero is not a group.
fn spell_group(group: u8) -> String {
    match group {
        0 => "none".to_string(),
        group => group.to_string(),
    }
}

/// Every watchlist, with the group it drives.
///
/// The group is in the listing rather than only in `watchlist link`, because
/// "which list drives which group" is one question and answering it a list at
/// a time invites nine commands and a wrong answer in the middle.
fn watchlist_list(store: &Store, as_json: bool) -> Result<String, Fault> {
    let lists = store.watchlists();
    let counted = |id: i64| -> usize {
        store.watchlist_sections(id).iter().map(|s| s.entries.len()).sum()
    };
    if as_json {
        let rows = lists
            .iter()
            .map(|(id, name)| {
                json!({
                    "id": id,
                    "name": name,
                    "symbols": counted(*id),
                    "isDefault": *id == DEFAULT_WATCHLIST,
                    "link": link_group_of(store, *id),
                })
                .to_string()
            })
            .collect();
        return Ok(wrap_list("watchlists", rows));
    }
    Ok(lists
        .iter()
        .map(|(id, name)| {
            let tag = if *id == DEFAULT_WATCHLIST { "(default)" } else { "" };
            let link = match link_group_of(store, *id) {
                0 => String::new(),
                group => format!("link {group}"),
            };
            format!("{id:<4} {name:<24} {:>4} symbols  {link:<8}{tag}", counted(*id))
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn watchlist_show(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (id, name) = list_or_default(store, m)?;
    let sections = store.watchlist_sections(id);
    if as_json {
        return Ok(format!(
            "{}\n",
            json!({ "id": id, "name": name, "sections": sections_json(&sections) })
        ));
    }
    let mut out = format!("{name}\n");
    for section in &sections {
        out.push_str(&format!("\n  {}\n", section_title(section)));
        for entry in &section.entries {
            out.push_str(&format!("    {}\n", spell(&entry.symbol, entry.suffix.as_deref())));
        }
    }
    Ok(out)
}

fn sections_json(sections: &[Section]) -> Value {
    Value::Array(
        sections
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "name": s.name,
                    "root": s.root,
                    "collapsed": s.collapsed,
                    "symbols": s.entries.iter().map(|e| json!({
                        "symbol": e.symbol,
                        "suffix": e.suffix,
                        "display": spell(&e.symbol, e.suffix.as_deref()),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

/// Every watchlist, or the ones named, as a file `watchlist import` reads.
fn watchlist_export(store: &Store, m: &clap::ArgMatches) -> Result<String, Fault> {
    let lists = match m.get_many::<String>("LIST") {
        None => store.watchlists(),
        Some(named) => named.map(|selector| find_list(store, selector)).collect::<Result<_, _>>()?,
    };
    let file = super::transfer::export(store, &lists);
    let text = serde_json::to_string_pretty(&file)
        .map_err(|error| Fault::new(super::EXIT_ERROR, error.to_string()))?;
    Ok(format!("{text}\n"))
}

/// Bring in a file from `watchlist export`, adding what is missing.
fn watchlist_import(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    caller: &dyn Caller,
) -> Result<String, Fault> {
    let file = super::transfer::parse(&caller.read(required(m, "FILE")?)?)?;
    let imported = super::transfer::import(store, &file, flag(m, "replace"))?;
    if as_json {
        let rows = imported
            .iter()
            .map(|done| {
                json!({
                    "name": done.name,
                    "action": done.action.key(),
                    "symbols": done.symbols,
                    "sections": done.sections,
                    "linkKeptBy": done.link_kept_by.as_ref().map(|(_, holder)| holder),
                })
                .to_string()
            })
            .collect();
        return Ok(wrap_list("watchlists", rows));
    }
    match imported.is_empty() {
        true => Ok("the file holds no watchlists\n".to_string()),
        false => Ok(imported.iter().map(|done| done.describe() + "\n").collect()),
    }
}

fn watchlist_create(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let name = required(m, "NAME")?;
    let id = store
        .add_watchlist(name)
        .ok_or_else(|| Fault::new(super::EXIT_ERROR, "could not create the watchlist".into()))?;
    said(as_json, json!({"id": id, "name": name}), format!("created watchlist {name:?} (id {id})"))
}

fn watchlist_rename(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (id, was) = find_list(store, required(m, "LIST")?)?;
    let name = required(m, "NAME")?;
    store.rename_watchlist(id, name);
    said(as_json, json!({"id": id, "name": name}), format!("renamed {was:?} to {name:?}"))
}

fn watchlist_delete(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (id, name) = find_list(store, required(m, "LIST")?)?;
    if id == DEFAULT_WATCHLIST {
        return Err(Fault::refused(format!(
            "{name:?} is the watchlist the bar widget shows and cannot be deleted; rename it instead"
        )));
    }
    store.remove_watchlist(id);
    said(as_json, json!({"id": id, "name": name}), format!("deleted watchlist {name:?}"))
}

/// Add symbols, or take them out. One function because they differ in a verb
/// and nothing else, and two would drift.
fn watchlist_add(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    adding: bool,
) -> Result<String, Fault> {
    let (id, name) = find_list(store, required(m, "LIST")?)?;
    let suffix = arg(m, "suffix").map(|s| s.to_uppercase());
    let symbols: Vec<String> = m
        .get_many::<String>("SYMBOL")
        .map(|given| given.map(|s| s.to_uppercase()).collect())
        .unwrap_or_default();
    if symbols.is_empty() {
        return Err(Fault::usage("name at least one symbol".into()));
    }

    let section = match arg(m, "section") {
        None => store.root_section(id),
        Some(wanted) => find_section(store, id, wanted)?.id,
    };

    let index = crate::inventory::everything();
    let mut done: Vec<String> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for typed in &symbols {
        // Each one on its own, so `2330.TW 6488.TWO` can share a command.
        let (symbol, suffix) = listing(&index, typed, suffix.clone());
        if index.find(&symbol, suffix.as_deref()).is_none() {
            unknown.push(spell(&symbol, suffix.as_deref()));
            continue;
        }
        match adding {
            true => store.add_to_section(section, &symbol, suffix.as_deref()),
            false => store.remove_from_section(section, &symbol, suffix.as_deref()),
        }
        done.push(spell(&symbol, suffix.as_deref()));
    }

    if done.is_empty() {
        return Err(Fault::not_found(format!(
            "none of those are instruments this knows: {}",
            unknown.join(", ")
        )));
    }
    let verb = if adding { "added" } else { "removed" };
    let preposition = if adding { "to" } else { "from" };
    let mut text = format!("{verb} {} {preposition} {name:?}: {}", done.len(), done.join(" "));
    if !unknown.is_empty() {
        text.push_str(&format!("\nskipped, not instruments: {}", unknown.join(" ")));
    }
    said(
        as_json,
        json!({"watchlist": name, "changed": done, "skipped": unknown}),
        text,
    )
}

/// Move one symbol: into another section, to another place in the one it is
/// in, or both at once.
///
/// What dragging a row in the rail does. The drag knows which copy of the
/// symbol was picked up because the pointer was on it; a command has no
/// pointer, which is the only thing `--from` is for — the same symbol sitting
/// in two sections of one watchlist.
fn watchlist_move(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (list, name) = find_list(store, required(m, "LIST")?)?;
    let moving = Entry {
        symbol: required(m, "SYMBOL")?.to_uppercase(),
        suffix: arg(m, "suffix").map(|s| s.to_uppercase()),
    };
    let shown = spell(&moving.symbol, moving.suffix.as_deref());

    let from = match arg(m, "from") {
        Some(wanted) => find_section(store, list, wanted)?,
        None => holder_of(store, list, &moving, &name, &shown)?,
    };
    if !from.entries.contains(&moving) {
        return Err(Fault::not_found(format!(
            "{shown} is not in {}",
            section_title(&from)
        )));
    }

    let to = match arg(m, "section") {
        Some(wanted) => find_section(store, list, wanted)?,
        None => from.clone(),
    };
    let before = match arg(m, "before") {
        None => None,
        Some(wanted) => Some(landing_before(&to, &wanted.to_uppercase(), &moving)?),
    };
    if to.id == from.id && before.is_none() {
        return Err(Fault::usage(
            "nowhere to move it to: name a --section, a --before, or both".into(),
        ));
    }

    store.move_entry_to_section(from.id, to.id, &moving, before.as_ref());

    let landed = match &before {
        Some(before) => {
            format!("in front of {}", spell(&before.symbol, before.suffix.as_deref()))
        }
        None => "at the end".to_string(),
    };
    let text = match from.id == to.id {
        true => format!("moved {shown} in {} of {name:?}, {landed}", section_title(&to)),
        false => format!(
            "moved {shown} from {} to {} in {name:?}, {landed}",
            section_title(&from),
            section_title(&to)
        ),
    };
    said(
        as_json,
        json!({
            "watchlist": name,
            "symbol": shown,
            "from": {"id": from.id, "name": from.name},
            "to": {"id": to.id, "name": to.name},
            "before": before.map(|b| spell(&b.symbol, b.suffix.as_deref())),
        }),
        text,
    )
}

/// The section a symbol is in, when the command did not say.
///
/// One watchlist can hold the same symbol in two sections, and there is no
/// pointer here to say which one was meant — so that is refused rather than
/// resolved by taking the first and moving a symbol the caller was not
/// looking at.
fn holder_of(
    store: &Store,
    list: i64,
    moving: &Entry,
    name: &str,
    shown: &str,
) -> Result<Section, Fault> {
    let holding: Vec<Section> = all_sections(store, list)
        .into_iter()
        .filter(|section| section.entries.contains(moving))
        .collect();
    match holding.as_slice() {
        [_] => Ok(holding.into_iter().next().expect("one")),
        [] => Err(Fault::not_found(format!("{shown} is not in {name:?}"))),
        // Each alternative repeats the flag, so one of them can be pasted
        // straight back onto the line that failed.
        many => Err(Fault::ambiguous(format!(
            "{shown} is in {} sections of {name:?}; say which with --from {}",
            many.len(),
            many.iter().map(|s| format!("id:{}", s.id)).collect::<Vec<_>>().join(" or --from ")
        ))),
    }
}

/// The entry a move lands in front of.
///
/// Named by symbol alone, without its venue suffix, because the symbol being
/// moved already carries `--suffix` and asking for a second one to point at a
/// neighbour is a flag nobody would guess.
fn landing_before(to: &Section, wanted: &str, moving: &Entry) -> Result<Entry, Fault> {
    if wanted == moving.symbol {
        return Err(Fault::usage(format!(
            "{} cannot land in front of itself",
            spell(&moving.symbol, moving.suffix.as_deref())
        )));
    }
    to.entries
        .iter()
        .find(|entry| entry.symbol == wanted)
        .cloned()
        .ok_or_else(|| Fault::not_found(format!("{wanted} is not in {}", section_title(to))))
}

// -- sections -------------------------------------------------------------

/// Every section of a watchlist, the nameless root included even when it is
/// empty.
///
/// `watchlist_sections` leaves an empty root out because the rail has nothing
/// to draw for it. A command does have something to do with it: `id:N` is the
/// only way to name the root, and a symbol has to be able to move back out of
/// a section into one that is currently empty.
fn all_sections(store: &Store, list: i64) -> Vec<Section> {
    let mut sections = store.watchlist_sections(list);
    if !sections.iter().any(|section| section.root) {
        sections.insert(
            0,
            Section {
                id: store.root_section(list),
                name: String::new(),
                collapsed: false,
                root: true,
                entries: Vec::new(),
            },
        );
    }
    sections
}

/// What a section is called where somebody reads it. The root has no name to
/// print, and an empty string in the middle of a sentence reads as a bug.
fn section_title(section: &Section) -> &str {
    match section.root {
        true => "(no section)",
        false => &section.name,
    }
}

fn find_section(store: &Store, list: i64, selector: &str) -> Result<Section, Fault> {
    let sections = all_sections(store, list);
    if let Some(rest) = selector.strip_prefix("id:") {
        let id: i64 = rest
            .parse()
            .map_err(|_| Fault::usage(format!("{selector:?} is not a section id")))?;
        return sections
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| Fault::not_found(format!("no section with id {id}")));
    }
    let wanted = selector.to_lowercase();
    let hits: Vec<Section> =
        sections.into_iter().filter(|s| s.name.to_lowercase() == wanted).collect();
    match hits.len() {
        1 => Ok(hits.into_iter().next().expect("one")),
        0 => Err(Fault::not_found(format!("no section called {selector:?}"))),
        n => Err(Fault::ambiguous(format!(
            "{n} sections are called {selector:?}; use {}",
            hits.iter().map(|s| format!("id:{}", s.id)).collect::<Vec<_>>().join(" or ")
        ))),
    }
}

fn section_list(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (id, name) = list_or_default(store, m)?;
    let sections = store.watchlist_sections(id);
    if as_json {
        return Ok(format!("{}\n", json!({"watchlist": name, "sections": sections_json(&sections)})));
    }
    Ok(sections
        .iter()
        .map(|s| {
            format!("{:<5} {:<24} {:>3} symbols", s.id, section_title(s), s.entries.len())
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn section_create(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (list, list_name) = find_list(store, required(m, "LIST")?)?;
    let name = required(m, "NAME")?;
    let id = store
        .add_section(list, name)
        .ok_or_else(|| Fault::new(super::EXIT_ERROR, "could not create the section".into()))?;
    said(
        as_json,
        json!({"id": id, "name": name, "watchlist": list_name}),
        format!("created section {name:?} in {list_name:?}"),
    )
}

fn section_rename(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (list, _) = find_list(store, required(m, "LIST")?)?;
    let section = find_section(store, list, required(m, "SECTION")?)?;
    let name = required(m, "NAME")?;
    store.rename_section(section.id, name);
    said(
        as_json,
        json!({"id": section.id, "name": name}),
        format!("renamed section {:?} to {name:?}", section.name),
    )
}

fn section_delete(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (list, _) = find_list(store, required(m, "LIST")?)?;
    let section = find_section(store, list, required(m, "SECTION")?)?;
    if section.root {
        return Err(Fault::refused(
            "that is where symbols outside a section live, and cannot be removed".into(),
        ));
    }
    store.remove_section(section.id);
    said(
        as_json,
        json!({"id": section.id, "name": section.name, "symbols": section.entries.len()}),
        format!("deleted section {:?} and its {} symbols", section.name, section.entries.len()),
    )
}

/// Turn a section into a watchlist of its own.
///
/// The symbols are copied across before the section goes, so a run that dies
/// in the middle leaves them in both places and never in neither.
fn section_promote(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (list, _) = find_list(store, required(m, "LIST")?)?;
    let section = find_section(store, list, required(m, "SECTION")?)?;
    if section.root {
        return Err(Fault::refused(
            "symbols outside a section have no name to give a watchlist".into(),
        ));
    }
    let id = store.add_watchlist(&section.name).ok_or_else(|| {
        Fault::new(super::EXIT_ERROR, "could not create the watchlist".into())
    })?;
    let root = store.root_section(id);
    for entry in &section.entries {
        store.add_to_section(root, &entry.symbol, entry.suffix.as_deref());
    }
    store.remove_section(section.id);
    said(
        as_json,
        json!({"id": id, "name": section.name, "symbols": section.entries.len()}),
        format!(
            "turned section {:?} into a watchlist with its {} symbols",
            section.name,
            section.entries.len()
        ),
    )
}

/// Put a watchlist's sections in the order given.
///
/// Sections left out keep the order they had, behind the ones named, so "put
/// Energy first" is one section long rather than the whole list spelled out.
///
/// The symbols come with their section rather than being moved: they are
/// positioned inside it, which is what makes it impossible for two sections'
/// symbols to end up interleaved however the sections are shuffled.
fn section_order(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let (list, name) = find_list(store, required(m, "LIST")?)?;
    let wanted: Vec<String> =
        m.get_many::<String>("SECTION").map(|given| given.cloned().collect()).unwrap_or_default();
    if wanted.is_empty() {
        return Err(Fault::usage("name at least one section".into()));
    }

    let mut ordered: Vec<i64> = Vec::new();
    for selector in &wanted {
        let section = find_section(store, list, selector)?;
        if section.root {
            return Err(Fault::refused(
                "symbols outside a section are always first and cannot be ordered".into(),
            ));
        }
        if ordered.contains(&section.id) {
            return Err(Fault::usage(format!("{selector:?} is named twice")));
        }
        ordered.push(section.id);
    }
    // Positions are written straight from this list, so it has to be the whole
    // order: a section left out would keep a position one of these has taken.
    for id in store.section_order(list) {
        if !ordered.contains(&id) {
            ordered.push(id);
        }
    }
    store.reorder_sections(list, &ordered);

    let sections = store.watchlist_sections(list);
    let named: Vec<&Section> = sections.iter().filter(|section| !section.root).collect();
    said(
        as_json,
        json!({
            "watchlist": name,
            "sections": named
                .iter()
                .map(|s| json!({"id": s.id, "name": s.name}))
                .collect::<Vec<_>>(),
        }),
        format!(
            "sections of {name:?}: {}",
            named.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
        ),
    )
}

// -- chartbooks and charts ------------------------------------------------

/// A picture of what is on screen, written to a file and never to a dialog.
///
/// A command has to be able to run in a script and in an agent, so this reaches
/// the writing path directly rather than the one the keyboard uses: the
/// "save automatically" setting is not consulted here, and the file chooser it
/// can open is not reachable from this call at all.
fn screenshot(
    live: Option<&dyn Live>,
    m: &clap::ArgMatches,
    as_json: bool,
    whole_book: bool,
) -> Result<String, Fault> {
    let live = live.ok_or_else(|| {
        Fault::new(
            crate::cli::EXIT_NO_WINDOW,
            format!(
                "no window is open, and a screenshot is of what is on screen; \
                 start Omacharts, then take a picture of {}",
                if whole_book { "its chartbook" } else { "its chart" }
            ),
        )
    })?;
    let into = arg(m, "output").map(std::path::PathBuf::from);
    let clipboard = flag(m, "clipboard");
    let (of, path) = live.screenshot(whole_book, into.as_deref(), clipboard)?;
    Ok(match as_json {
        true => format!(
            "{}\n",
            json!({ "of": of, "path": path.display().to_string(), "clipboard": clipboard })
        ),
        false if clipboard => format!("saved {of} to {}, and copied it\n", path.display()),
        false => format!("saved {of} to {}\n", path.display()),
    })
}

fn charts_verb(
    store: &Store,
    noun: &str,
    verb: &str,
    m: &clap::ArgMatches,
    as_json: bool,
) -> Result<String, Fault> {
    let mut workspace = Workspace::load(store);
    let selector = arg(m, "BOOK").or_else(|| arg(m, "book"));

    let text = match (noun, verb) {
        ("chartbook", "list") => {
            let rows: Vec<String> = (0..workspace.books().len())
                .map(|i| {
                    let book = &workspace.books()[i];
                    json!({
                        "id": i,
                        "name": workspace.name_of(i),
                        "charts": charts::panes(book).len(),
                        "watchlist": book["watchlist"],
                        "active": i == workspace.active(),
                    })
                    .to_string()
                })
                .collect();
            if !as_json && workspace.books().is_empty() {
                return Ok("no chartbooks yet; open the app once, or create one\n".to_string());
            }
            return Ok(match as_json {
                true => wrap_list("chartbooks", rows),
                false => (0..workspace.books().len())
                    .map(|i| {
                        format!(
                            "{}{:<4} {:<24} {:>2} charts  {}",
                            if i == workspace.active() { "* " } else { "  " },
                            format!("id:{i}"),
                            workspace.name_of(i),
                            charts::panes(&workspace.books()[i]).len(),
                            rail_label(store, &workspace.books()[i]),
                        )
                        .trim_end()
                        .to_string()
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n",
            });
        }
        ("chartbook", "show") | ("chart", "list") => {
            let at = workspace.resolve(selector.map(String::as_str))?;
            let book = workspace.book(at)?;
            let rows: Vec<String> = charts::panes(book).iter().map(pane_json).collect();
            return Ok(match as_json {
                true => format!(
                    "{{\"chartbook\":{},\"watchlist\":{},\"charts\":[{}]}}\n",
                    super::json_string(&workspace.name_of(at)),
                    book["watchlist"],
                    rows.join(",")
                ),
                false => {
                    let focused = book["focused"].as_u64().unwrap_or(0);
                    // Which list a book shows and which group a chart is in
                    // are half of what either one is for, so they belong in
                    // the listing a person reads and not only in the JSON.
                    let shows = rail_label(store, book);
                    let mut out = match shows.is_empty() {
                        true => workspace.name_of(at),
                        false => format!("{} — {shows}", workspace.name_of(at)),
                    };
                    out.push('\n');
                    for pane in charts::panes(book) {
                        let id = pane["id"].as_u64().unwrap_or(0);
                        out.push_str(
                            format!(
                                "{} {:<4} {:<12} {:<6} {:<8} {}\n",
                                if id == focused { "*" } else { " " },
                                id,
                                spell(
                                    pane["symbol"].as_str().unwrap_or("?"),
                                    pane["suffix"].as_str()
                                ),
                                pane["timeframe"].as_str().unwrap_or(""),
                                pane["bar_style"].as_str().unwrap_or(""),
                                match pane["linked"].as_u64().unwrap_or(0) {
                                    0 => "unlinked".to_string(),
                                    group => format!("link {group}"),
                                },
                            )
                            .trim_end(),
                        );
                        out.push('\n');
                    }
                    out
                }
            });
        }
        ("chartbook", "create") => {
            let name = required(m, "NAME")?;
            let symbol = m
                .get_one::<String>("symbol")
                .map(|s| s.to_uppercase())
                .unwrap_or_else(|| "SPY".to_string());
            let watchlist = match arg(m, "watchlist") {
                Some(selector) => {
                    let (id, name) = find_list(store, selector)?;
                    // The book being made is not in the list yet, so nothing
                    // is excepted: any book holding this list is another one.
                    if let Some(holder) = book_using(&workspace, usize::MAX, id) {
                        return Err(Fault::refused(format!(
                            "chartbook {holder:?} already shows {name:?}; a watchlist belongs to one"
                        )));
                    }
                    Some(id)
                }
                None => None,
            };
            let id = workspace.next_pane();
            let book = json!({
                "name": name,
                "layout": {"leaf": id},
                "focused": id,
                "panes": [charts::new_pane(id, &symbol, None)],
                "watchlist": watchlist,
            });
            let at = workspace.push(book);
            if flag(m, "switch") {
                workspace.set_active(at);
            }
            format!("created chartbook {name:?} (id:{at}) showing {symbol}")
        }
        ("chartbook", "rename") => {
            let at = workspace.find(required(m, "BOOK")?)?;
            let was = workspace.name_of(at);
            let name = required(m, "NAME")?;
            workspace.book_mut(at)?["name"] = json!(name);
            format!("renamed chartbook {was:?} to {name:?}")
        }
        ("chartbook", "delete") => {
            let at = workspace.find(required(m, "BOOK")?)?;
            if workspace.books().len() < 2 {
                return Err(Fault::refused(
                    "this is the only chartbook; there has to be one".into(),
                ));
            }
            let name = workspace.name_of(at);
            let charts = charts::panes(workspace.book(at)?).len();
            workspace.remove(at);
            format!("deleted chartbook {name:?} and its {charts} charts")
        }
        ("chartbook", "switch") => {
            let at = workspace.find(required(m, "BOOK")?)?;
            workspace.set_active(at);
            format!("switched to chartbook {:?}", workspace.name_of(at))
        }
        ("chartbook", "watchlist") => {
            let at = workspace.find(required(m, "BOOK")?)?;
            let (id, name) = find_list(store, required(m, "LIST")?)?;
            if let Some(holder) = book_using(&workspace, at, id) {
                return Err(Fault::refused(format!(
                    "chartbook {holder:?} already shows {name:?}; a watchlist belongs to one"
                )));
            }
            workspace.book_mut(at)?["watchlist"] = json!(id);
            format!("chartbook {:?} now shows watchlist {name:?}", workspace.name_of(at))
        }
        ("chart", _) => return chart_verb(store, verb, m, as_json, &mut workspace),
        _ => return Err(Fault::usage(format!("no such command: {noun} {verb}"))),
    };

    workspace.save(store);
    said(as_json, json!({"message": text}), text)
}

fn chart_verb(
    store: &Store,
    verb: &str,
    m: &clap::ArgMatches,
    as_json: bool,
    workspace: &mut Workspace,
) -> Result<String, Fault> {
    let at = workspace.resolve(arg(m, "book").map(String::as_str))?;
    let wanted = arg(m, "chart").map(String::as_str);
    let book = workspace.book(at)?.clone();
    let pane = charts::resolve_pane(&book, wanted)?;

    let text = match verb {
        "split" => {
            let horizontal =
                arg(m, "DIRECTION").map(|d| d == "horizontal").unwrap_or(true);
            let added = workspace.next_pane();
            let from = charts::panes(&book)
                .iter()
                .find(|p| p["id"].as_u64() == Some(pane as u64))
                .cloned()
                .unwrap_or_else(|| charts::new_pane(added, "SPY", None));
            let target = workspace.book_mut(at)?;
            target["layout"] = charts::split_leaf(&target["layout"], pane, added, horizontal);
            charts::panes_mut(target)?.push(charts::copy_pane(&from, added));
            target["focused"] = json!(added);
            format!(
                "split chart {pane} {}; the new chart is {added}",
                if horizontal { "horizontally" } else { "vertically" }
            )
        }
        "close" => {
            let Some(pruned) = charts::remove_leaf(&book["layout"], pane) else {
                return Err(Fault::refused(
                    "this is the only chart in the chartbook; there has to be one".into(),
                ));
            };
            let kept = charts::leaves(&pruned);
            let target = workspace.book_mut(at)?;
            target["layout"] = pruned;
            charts::panes_mut(target)?
                .retain(|p| p["id"].as_u64().is_some_and(|id| kept.contains(&(id as u32))));
            target["focused"] = json!(kept.first().copied().unwrap_or(0));
            // A chart that is gone cannot be filling the window. The window
            // would ignore a stale one, but what is stored should not need
            // ignoring.
            if target["maximized"].as_u64() == Some(u64::from(pane)) {
                target["maximized"] = Value::Null;
            }
            format!("closed chart {pane}")
        }
        "focus" => {
            // Through the same resolver as every other verb, so `pos:N` works
            // here too. It is the form worth using, and a verb that took only
            // a raw id would be the one place that is not true.
            let wanted = required(m, "CHART")?;
            let id = charts::resolve_pane(&book, Some(wanted))?;
            workspace.book_mut(at)?["focused"] = json!(id);
            format!("focused {}", chart_label(&book, id))
        }
        "set" => return chart_set(store, m, as_json, workspace, at, pane),
        "indicator" => return chart_indicator(store, m, as_json, workspace, at, pane),
        "drawing" => return chart_drawing(store, m, as_json, workspace, at, pane),
        _ => return Err(Fault::usage(format!("no such command: chart {verb}"))),
    };

    workspace.save(store);
    said(as_json, json!({"message": text, "chart": pane}), text)
}

fn chart_set(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    workspace: &mut Workspace,
    at: usize,
    pane: u32,
) -> Result<String, Fault> {
    let given = arg(m, "suffix").map(|s| s.to_uppercase());
    let mut changed: Vec<String> = Vec::new();

    // Everything is checked before anything is written, so a command with one
    // bad value does not leave the chart half-changed.
    let (symbol, suffix) = match arg(m, "symbol") {
        None => (None, given),
        Some(text) => {
            let index = crate::inventory::everything();
            let (symbol, suffix) = listing(&index, text, given);
            if index.find(&symbol, suffix.as_deref()).is_none() {
                return Err(Fault::not_found(format!(
                    "no instrument called {}; try `omacharts symbol search {symbol}`",
                    spell(&symbol, suffix.as_deref())
                )));
            }
            (Some(symbol), suffix)
        }
    };
    let timeframe = match arg(m, "resolution") {
        None => None,
        Some(text) => Some(Timeframe::parse(text).ok_or_else(|| {
            Fault::usage(format!("{text:?} is not a resolution; try 5m, 1h, 1D or 1W"))
        })?),
    };
    let style = match arg(m, "style") {
        None => None,
        Some(text) => Some(
            BarStyle::from_key(text)
                .ok_or_else(|| Fault::usage(format!("{text:?} is not a bar style")))?,
        ),
    };
    let session = match arg(m, "session") {
        None => None,
        Some(text) => Some(
            Session::from_key(text)
                .ok_or_else(|| Fault::usage(format!("{text:?} is not a session")))?,
        ),
    };
    let link = match arg(m, "link") {
        None => None,
        Some(text) => Some(link_group(text)?),
    };
    let drawing_sharing = match arg(m, "drawing-sharing") {
        None => None,
        Some(text) => Some(omacharts_engine::Sharing::from_key(text).ok_or_else(|| {
            Fault::usage(format!("{text:?} is not a drawing group; try global, group-1 to group-9, or off"))
        })?),
    };

    // Which group it was in, read before anything is written: only an actual
    // change leads the group. Re-stating the group a chart is already in is
    // not somebody asking for four charts to move.
    let was = group_of(workspace.book(at)?, pane);

    let target = workspace.book_mut(at)?;
    let Some(chart) = charts::pane_mut(target, pane) else {
        return Err(Fault::not_found(format!("no chart {pane}")));
    };
    if let Some(symbol) = symbol {
        chart["symbol"] = json!(symbol);
        chart["suffix"] = json!(suffix);
        changed.push(format!("symbol {}", spell(&symbol, suffix.as_deref())));
    }
    if let Some(timeframe) = timeframe {
        chart["timeframe"] = json!(timeframe.key());
        changed.push(format!("resolution {}", timeframe.key()));
    }
    if let Some(style) = style {
        chart["bar_style"] = json!(style.key());
        changed.push(format!("style {}", style.key()));
    }
    if let Some(session) = session {
        chart["session"] = json!(session.key());
        changed.push(format!("session {}", session.key()));
    }
    if let Some(link) = link {
        chart["linked"] = json!(link);
        changed.push(match link {
            0 => "unlinked".to_string(),
            group => format!("link group {group}"),
        });
    }
    if let Some(grid) = arg(m, "grid") {
        chart["show_grid"] = json!(grid == "on");
        changed.push(format!("grid {grid}"));
    }
    if let Some(sharing) = drawing_sharing {
        chart["drawing_sharing"] = json!(sharing.key());
        changed.push(format!("drawing group {}", sharing.key()));
    }
    if let Some(auto) = arg(m, "auto-scale") {
        chart["auto_scale"] = json!(auto == "on");
        changed.push(format!("auto-scale {auto}"));
    }
    // What the chart ends up showing, which is what it leads its new group
    // with. Read after the writes above, so `--symbol X --link N` leads with X
    // rather than with whatever the chart had before.
    let leading = chart["symbol"].as_str().filter(|s| !s.is_empty()).map(String::from);
    let leading_suffix = chart["suffix"].as_str().map(String::from);

    if changed.is_empty() {
        return Err(Fault::usage(
            "nothing to change; pass --symbol, --resolution, --style, --session, --link, --grid, \
             --auto-scale or --drawing-sharing"
                .into(),
        ));
    }

    // A chart put in a group leads it. Three things write nothing: leaving a
    // group — the charts still in it keep what they had; re-stating the group
    // the chart is already in; and leading from a chart with no symbol, which
    // would blank the group it just joined.
    let joined = link.filter(|group| *group > 0 && *group != was);
    let followers = match (joined, &leading) {
        (Some(group), Some(symbol)) => {
            workspace.lead_link_group(group, symbol, leading_suffix.as_deref(), at, pane)
        }
        _ => 0,
    };

    workspace.save(store);
    let mut text = format!("chart {pane}: {}", changed.join(", "));
    if followers > 0 {
        let group = joined.unwrap_or(0);
        let named = spell(leading.as_deref().unwrap_or("?"), leading_suffix.as_deref());
        text.push_str(&match followers {
            1 => format!("; 1 other chart in group {group} now shows {named}"),
            n => format!("; {n} other charts in group {group} now show {named}"),
        });
    }
    said(
        as_json,
        json!({"chart": pane, "changed": changed, "followers": followers}),
        text,
    )
}

fn chart_indicator(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    workspace: &mut Workspace,
    at: usize,
    pane: u32,
) -> Result<String, Fault> {
    let action = required(m, "ACTION")?.as_str();
    let book = workspace.book(at)?.clone();
    let chart = charts::panes(&book)
        .iter()
        .find(|p| p["id"].as_u64() == Some(pane as u64))
        .cloned()
        .unwrap_or_else(|| json!({}));

    if action == "list" {
        let listed = chart["indicators"].as_array().cloned().unwrap_or_default();
        let rows: Vec<String> = listed
            .iter()
            .map(|i| {
                json!({
                    "id": i["id"],
                    "kind": i["kind"],
                    "params": i["params"],
                    "color": i["color"],
                    "stroke": i["stroke"],
                    "visible": i["visible"],
                })
                .to_string()
            })
            .collect();
        return match as_json {
            true => Ok(format!("{{\"chart\":{pane},\"indicators\":[{}]}}\n", rows.join(","))),
            false => Ok(listed
                .iter()
                .map(describe_indicator)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"),
        };
    }

    let wanted = arg(m, "KIND")
        .ok_or_else(|| Fault::usage(format!("`indicator {action}` needs an indicator to {action}")))?
        .to_lowercase();
    let kind = IndicatorKind::ALL
        .into_iter()
        .find(|k| k.key() == wanted)
        .ok_or_else(|| {
            Fault::usage(format!(
                "{wanted:?} is not an indicator; try {}",
                IndicatorKind::ALL.map(|k| k.key()).join(", ")
            ))
        })?;

    // Everything is read and checked before anything is written, so a command
    // carrying one bad value leaves the chart exactly as it was.
    let edits = Edits::read(m)?;

    let target = workspace.book_mut(at)?;
    let Some(chart) = charts::pane_mut(target, pane) else {
        return Err(Fault::not_found(format!("no chart {pane}")));
    };
    let indicators = chart["indicators"].as_array_mut().ok_or_else(|| {
        Fault::new(super::EXIT_ERROR, "this chart's indicators are not a list".into())
    })?;

    let text = match action {
        "add" => {
            let next = indicators
                .iter()
                .filter_map(|i| i["id"].as_u64())
                .max()
                .map(|id| id as u32 + 1)
                .unwrap_or(1);
            let mut added = serde_json::to_value(Indicator::new(next, kind))
                .map_err(|e| Fault::new(super::EXIT_ERROR, e.to_string()))?;
            edits.apply(&mut added, kind)?;
            let described = describe_indicator(&added);
            indicators.push(added);
            format!("added {described}")
        }
        "remove" => {
            let before = indicators.len();
            indicators.retain(|i| !is_kind(i, kind));
            if indicators.len() == before {
                return Err(Fault::not_found(format!(
                    "this chart has no {}",
                    kind.short_name()
                )));
            }
            format!("removed {}", kind.short_name())
        }
        "set" => {
            let wanted_id = match arg(m, "id") {
                None => None,
                Some(text) => Some(
                    text.parse::<u64>()
                        .map_err(|_| Fault::usage(format!("{text:?} is not an indicator id")))?,
                ),
            };
            let matching: Vec<usize> = indicators
                .iter()
                .enumerate()
                .filter(|(_, i)| is_kind(i, kind))
                .filter(|(_, i)| wanted_id.is_none_or(|id| i["id"].as_u64() == Some(id)))
                .map(|(at, _)| at)
                .collect();
            let which = match matching.as_slice() {
                [one] => *one,
                [] => {
                    return Err(Fault::not_found(format!(
                        "this chart has no {}",
                        kind.short_name()
                    )))
                }
                many => {
                    return Err(Fault::ambiguous(format!(
                        "this chart has {} of them; say which with --id {}",
                        many.len(),
                        many.iter()
                            .filter_map(|at| indicators[*at]["id"].as_u64())
                            .map(|id| id.to_string())
                            .collect::<Vec<_>>()
                            .join(" or ")
                    )))
                }
            };
            if edits.is_empty() {
                return Err(Fault::usage(
                    "nothing to change; pass --period, --anchor, --rows, --color and so on \
                     (see `omacharts chart indicator --help`)"
                        .into(),
                ));
            }
            edits.apply(&mut indicators[which], kind)?;
            format!("set {}", describe_indicator(&indicators[which]))
        }
        other => return Err(Fault::usage(format!("{other:?} is not list, add, remove or set"))),
    };

    workspace.save(store);

    // What it actually hit, not just that it worked. A command that named no
    // chart has to say which one it found, or nobody can catch it reaching
    // the wrong one.
    let where_at = format!(
        "{text} on {} in {:?}",
        chart_label(workspace.book(at)?, pane),
        workspace.name_of(at)
    );
    said(as_json, json!({"chart": pane, "message": where_at}), where_at)
}

/// What is drawn on a chart's symbol, from a terminal.
///
/// Drawings belong to the symbol rather than the chart, so the chart named
/// here is mostly the way to say which symbol: a line added through `pos:0`
/// showing AAPL is on every chart of AAPL that shares it, as one drawn by
/// hand would be. The exception is a local drawing, which stays with the
/// chart it was made on and is kept in the chart's own record. Anchors are a
/// moment and a price, the way the chart stores them; a look is either a
/// configuration number or properties given by hand, and colours are the
/// nine presets or a hex.
fn chart_drawing(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    workspace: &mut Workspace,
    at: usize,
    pane: u32,
) -> Result<String, Fault> {
    use omacharts_engine::drawings::{Drawing, Kind, Scope, Sharing, CONFIGURATIONS};

    let action = required(m, "ACTION")?.as_str();
    let kind = match arg(m, "KIND") {
        None => None,
        Some(text) => Some(
            Kind::from_key(text).ok_or_else(|| Fault::usage(format!("{text:?} is not line or rect")))?,
        ),
    };

    // The configurations are about every chart, so they come before the
    // chart is even looked at.
    if let Some(answer) = configurations_verb(store, m, as_json, action, kind)? {
        return Ok(answer);
    }

    let book = workspace.book(at)?.clone();
    let chart = charts::panes(&book)
        .iter()
        .find(|p| p["id"].as_u64() == Some(pane as u64))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let symbol = chart["symbol"].as_str().unwrap_or("").to_string();
    let suffix = chart["suffix"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    if symbol.is_empty() {
        return Err(Fault::refused("that chart has no symbol to draw on".into()));
    }
    let sharing = chart["drawing_sharing"]
        .as_str()
        .and_then(Sharing::from_key)
        .unwrap_or_default();
    let configs = store.drawing_configurations();

    // What this chart shows: the shared drawings its sharing lets through,
    // and its own.
    let locals: Vec<Drawing> = chart["drawings"]
        .as_array()
        .map(|list| list.iter().filter_map(|d| serde_json::from_value(d.clone()).ok()).collect())
        .unwrap_or_default();
    let shown = |store: &Store, locals: &[Drawing]| -> Vec<Drawing> {
        let mut all: Vec<Drawing> = store
            .drawings(&symbol, suffix.as_deref())
            .into_iter()
            .filter(|d| sharing.shows(d.scope))
            .collect();
        all.extend(locals.iter().cloned());
        omacharts_engine::drawings::sort_for_painting(&mut all);
        all
    };

    if action == "list" {
        let listed = shown(store, &locals);
        return match as_json {
            true => Ok(format!(
                "{{\"chart\":{pane},\"symbol\":{},\"sharing\":{},\"drawings\":[{}]}}\n",
                json!(spell(&symbol, suffix.as_deref())),
                json!(sharing.key()),
                listed.iter().map(|d| drawing_json(d, &configs).to_string()).collect::<Vec<_>>().join(",")
            )),
            false if listed.is_empty() => Ok(format!("nothing drawn on {}\n", spell(&symbol, suffix.as_deref()))),
            false => Ok(listed.iter().map(|d| describe_drawing(d, &configs)).collect::<Vec<_>>().join("\n") + "\n"),
        };
    }

    // Every flag is read and checked before anything is written, so one bad
    // anchor leaves the symbol's drawings exactly as they were.
    let from = anchor(m, "from")?;
    let to = anchor(m, "to")?;
    let config = match arg(m, "config") {
        None => None,
        Some(text) => Some(
            text.parse::<u8>()
                .ok()
                .filter(|n| (1..=CONFIGURATIONS).contains(n))
                .ok_or_else(|| Fault::usage(format!("--config is a number from 1 to {CONFIGURATIONS}, not {text:?}")))?,
        ),
    };
    let style = StyleEdits::read(m)?;
    let scope = match arg(m, "scope") {
        None => None,
        Some(text) => Some(Scope::from_key(text).ok_or_else(|| {
            Fault::usage(format!("{text:?} is not a scope; try local, global or group-1 to group-9"))
        })?),
    };
    let order = match arg(m, "order").map(String::as_str) {
        None => None,
        Some("front") => Some(true),
        Some("back") => Some(false),
        Some(other) => return Err(Fault::usage(format!("{other:?} is not front or back"))),
    };
    let id = match arg(m, "id") {
        None => None,
        Some(text) => Some(
            text.parse::<i64>()
                .map_err(|_| Fault::usage(format!("--id takes a number from `drawing list`, not {text:?}")))?,
        ),
    };

    // Where a drawing with this scope is kept, and the id it gets there.
    let mut locals = locals;
    let text = match action {
        "add" => {
            let kind = kind.ok_or_else(|| Fault::usage("`drawing add` needs a kind: line or rect".into()))?;
            let (Some(from), Some(to)) = (from, to) else {
                return Err(Fault::usage("`drawing add` needs --from and --to, each WHEN,PRICE".into()));
            };
            let mut drawing = Drawing::new(kind, from, to);
            drawing.follow(config.unwrap_or(1));
            if !style.is_empty() {
                drawing.edit_style(&configs, |s| style.apply(s));
            }
            drawing.scope = scope.unwrap_or_else(|| sharing.scope_for_new());
            drawing.order = shown(store, &locals).iter().map(|d| d.order).max().unwrap_or(0);
            if drawing.is_local() {
                drawing.id = next_local_id(&locals);
                locals.push(drawing.clone());
                write_locals(workspace, at, pane, &locals)?;
            } else {
                let Some(id) = store.add_drawing(&symbol, suffix.as_deref(), &drawing) else {
                    return Err(Fault::new(super::EXIT_ERROR, "could not write the drawing down".into()));
                };
                drawing.id = id;
            }
            format!("drew {}", describe_drawing(&drawing, &configs))
        }
        "set" => {
            let Some(id) = id else {
                return Err(Fault::usage("`drawing set` needs --id, from `drawing list`".into()));
            };
            if from.is_none() && to.is_none() && config.is_none() && style.is_empty() && scope.is_none() && order.is_none() {
                return Err(Fault::usage(
                    "nothing to set: give --from, --to, --config, --scope, --order, or a property".into(),
                ));
            }
            let mut drawing = find_drawing(store, &symbol, suffix.as_deref(), &locals, id)?;
            let was_local = drawing.is_local();
            if let Some(from) = from {
                drawing.from = from;
            }
            if let Some(to) = to {
                drawing.to = to;
            }
            if let Some(n) = config {
                drawing.follow(n);
            }
            if !style.is_empty() {
                drawing.edit_style(&configs, |s| style.apply(s));
            }
            if let Some(scope) = scope {
                drawing.scope = scope;
            }
            if let Some(front) = order {
                let others = shown(store, &locals);
                let (lo, hi) = others.iter().fold((0i64, 0i64), |(lo, hi), d| (lo.min(d.order), hi.max(d.order)));
                drawing.order = if front { hi + 1 } else { lo - 1 };
            }
            // A drawing that changed sides moves house: out of where it was,
            // into where its scope now says.
            match (was_local, drawing.is_local()) {
                (true, true) => {
                    if let Some(slot) = locals.iter_mut().find(|d| d.id == id) {
                        *slot = drawing.clone();
                    }
                    write_locals(workspace, at, pane, &locals)?;
                }
                (false, false) => store.update_drawing(&drawing),
                (true, false) => {
                    locals.retain(|d| d.id != id);
                    write_locals(workspace, at, pane, &locals)?;
                    drawing.id = 0;
                    drawing.id = store
                        .add_drawing(&symbol, suffix.as_deref(), &drawing)
                        .ok_or_else(|| Fault::new(super::EXIT_ERROR, "could not write the drawing down".into()))?;
                }
                (false, true) => {
                    store.remove_drawing(id);
                    drawing.id = next_local_id(&locals);
                    locals.push(drawing.clone());
                    write_locals(workspace, at, pane, &locals)?;
                }
            }
            format!("set {}", describe_drawing(&drawing, &configs))
        }
        "remove" => {
            let Some(id) = id else {
                return Err(Fault::usage("`drawing remove` needs --id, from `drawing list`".into()));
            };
            let found = find_drawing(store, &symbol, suffix.as_deref(), &locals, id)?;
            if found.is_local() {
                locals.retain(|d| d.id != id);
                write_locals(workspace, at, pane, &locals)?;
            } else {
                store.remove_drawing(id);
            }
            format!("removed {}", describe_drawing(&found, &configs))
        }
        "clear" => {
            let count = shown(store, &locals).len();
            store.clear_drawings(&symbol, suffix.as_deref());
            locals.clear();
            write_locals(workspace, at, pane, &locals)?;
            format!("removed {count} drawing{}", if count == 1 { "" } else { "s" })
        }
        other => {
            return Err(Fault::usage(format!(
                "{other:?} is not list, add, set, remove, clear, configs, configure or reset-configs"
            )))
        }
    };

    workspace.save(store);
    let where_at = format!(
        "{text} on {} in {:?}",
        chart_label(workspace.book(at)?, pane),
        workspace.name_of(at)
    );
    said(as_json, json!({"chart": pane, "message": where_at}), where_at)
}

/// The three actions about configurations rather than drawings. `None` when
/// the action is about drawings.
fn configurations_verb(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    action: &str,
    kind: Option<omacharts_engine::DrawingKind>,
) -> Result<Option<String>, Fault> {
    use omacharts_engine::drawings::{Kind, CONFIGURATIONS};
    let kinds: Vec<Kind> = kind.map(|k| vec![k]).unwrap_or_else(|| Kind::ALL.to_vec());
    match action {
        "configs" => {
            let configs = store.drawing_configurations();
            if as_json {
                let rows: Vec<String> = kinds
                    .iter()
                    .flat_map(|kind| {
                        let configs = &configs;
                        (1..=CONFIGURATIONS).map(move |n| {
                            json!({
                                "kind": kind.key(),
                                "config": n,
                                "style": style_json(configs.of(*kind, n)),
                                "default": configs.is_default(*kind, n),
                            })
                            .to_string()
                        })
                    })
                    .collect();
                return Ok(Some(wrap_list("configurations", rows)));
            }
            let mut out = String::new();
            for kind in &kinds {
                for n in 1..=CONFIGURATIONS {
                    let tag = if configs.is_default(*kind, n) { "" } else { "  (edited)" };
                    out.push_str(&format!("{} {n}  {}{tag}\n", kind.key(), describe_style(*kind, configs.of(*kind, n))));
                }
            }
            Ok(Some(out))
        }
        "configure" => {
            let kind = kind.ok_or_else(|| Fault::usage("`drawing configure` needs a kind: line or rect".into()))?;
            let n = arg(m, "config")
                .ok_or_else(|| Fault::usage("`drawing configure` needs --config N".into()))?
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=CONFIGURATIONS).contains(n))
                .ok_or_else(|| Fault::usage(format!("--config is a number from 1 to {CONFIGURATIONS}")))?;
            let edits = StyleEdits::read(m)?;
            if edits.is_empty() {
                return Err(Fault::usage("nothing to configure: give --color, --width, --arrow, --head, --border, --fill or --alpha".into()));
            }
            let mut configs = store.drawing_configurations();
            let mut style = configs.of(kind, n).clone();
            edits.apply(&mut style);
            configs.set(kind, n, style.clone());
            store.set_drawing_configurations(&configs);
            let text = format!("{} configuration {n} is now {}", kind.label(), describe_style(kind, &style));
            Ok(Some(said(as_json, json!({"kind": kind.key(), "config": n, "style": style_json(&style), "message": text}), text)?))
        }
        "reset-configs" => {
            let mut configs = store.drawing_configurations();
            for kind in &kinds {
                configs.reset(*kind);
            }
            store.set_drawing_configurations(&configs);
            let what = kinds.iter().map(|k| k.label().to_lowercase()).collect::<Vec<_>>().join(" and ");
            let text = format!("restored the default {what} configurations");
            Ok(Some(said(as_json, json!({"message": text}), text)?))
        }
        _ => Ok(None),
    }
}

/// A chart's local drawings get negative ids, below everything the store
/// hands out, so one number names a drawing wherever it lives.
fn next_local_id(locals: &[omacharts_engine::Drawing]) -> i64 {
    locals.iter().map(|d| d.id).min().unwrap_or(0).min(0) - 1
}

/// Put a chart's local drawings back in its record.
fn write_locals(
    workspace: &mut Workspace,
    at: usize,
    pane: u32,
    locals: &[omacharts_engine::Drawing],
) -> Result<(), Fault> {
    let book = workspace.book_mut(at)?;
    let Some(target) = charts::pane_mut(book, pane) else {
        return Err(Fault::not_found(format!("chart {pane} is not in this chartbook")));
    };
    target["drawings"] = serde_json::to_value(locals).unwrap_or_else(|_| json!([]));
    Ok(())
}

/// A drawing by id, from the store or the chart's own.
fn find_drawing(
    store: &Store,
    symbol: &str,
    suffix: Option<&str>,
    locals: &[omacharts_engine::Drawing],
    id: i64,
) -> Result<omacharts_engine::Drawing, Fault> {
    let found = if id < 0 {
        locals.iter().find(|d| d.id == id).cloned()
    } else {
        store.drawings(symbol, suffix).into_iter().find(|d| d.id == id)
    };
    found.ok_or_else(|| Fault::not_found(format!("no drawing {id} on {}", spell(symbol, suffix))))
}

fn drawing_json(d: &omacharts_engine::Drawing, configs: &omacharts_engine::Configurations) -> Value {
    json!({
        "id": d.id,
        "kind": d.kind.key(),
        "from": {"ts": d.from.ts, "when": spell_moment(d.from.ts), "price": d.from.price},
        "to": {"ts": d.to.ts, "when": spell_moment(d.to.ts), "price": d.to.price},
        "config": d.config,
        "style": style_json(d.style(configs)),
        "scope": d.scope.key(),
        "order": d.order,
    })
}

fn style_json(style: &omacharts_engine::Style) -> Value {
    json!({
        "color": style.colour.spell(),
        "width": style.width,
        "arrow": style.arrow.key(),
        "head": style.head.key(),
        "border": style.border,
        "fill": style.fill.spell(),
        "alpha": style.alpha,
    })
}

/// A drawing, said back in one line:
/// `#3 line  2026-09-01 180.50 → 2026-09-19 192.00  configuration 4 · global`.
fn describe_drawing(drawing: &omacharts_engine::Drawing, configs: &omacharts_engine::Configurations) -> String {
    let look = match drawing.config {
        Some(n) => format!("configuration {n}"),
        None => describe_style(drawing.kind, drawing.style(configs)),
    };
    format!(
        "#{} {}  {} {:.2} → {} {:.2}  {look} · {}",
        drawing.id,
        drawing.kind.key(),
        spell_moment(drawing.from.ts),
        drawing.from.price,
        spell_moment(drawing.to.ts),
        drawing.to.price,
        drawing.scope.key()
    )
}

/// A look in words: `amber, 2.5px, arrow at the end` for a line; `fill amber
/// at 0.16, edge amber 1px` for a rectangle.
fn describe_style(kind: omacharts_engine::DrawingKind, style: &omacharts_engine::Style) -> String {
    use omacharts_engine::drawings::{Arrow, ArrowHead, Kind};
    match kind {
        Kind::Line => {
            let arrow = match (style.arrow, style.head) {
                (Arrow::None, _) => String::new(),
                (arrow, ArrowHead::Filled) => format!(", arrow {}", arrow.label().to_lowercase()),
                (arrow, head) => {
                    format!(", {} arrow {}", head.label().to_lowercase(), arrow.label().to_lowercase())
                }
            };
            format!("{}, {}px{arrow}", style.colour.spell(), style.width)
        }
        Kind::Rect => {
            let edge = match style.border {
                true => format!(", edge {} {}px", style.colour.spell(), style.width),
                false => ", no edge".to_string(),
            };
            format!("fill {} at {:.2}{edge}", style.fill.spell(), style.alpha)
        }
    }
}

/// The properties a command offered for a drawing or a configuration, read
/// and checked before any is written.
struct StyleEdits {
    colour: Option<omacharts_engine::Paint>,
    width: Option<f64>,
    arrow: Option<omacharts_engine::Arrow>,
    head: Option<omacharts_engine::ArrowHead>,
    border: Option<bool>,
    fill: Option<omacharts_engine::Paint>,
    alpha: Option<f64>,
}

impl StyleEdits {
    fn read(m: &clap::ArgMatches) -> Result<StyleEdits, Fault> {
        use omacharts_engine::drawings::{Arrow, ArrowHead, Paint};
        let paint = |id: &str| -> Result<Option<Paint>, Fault> {
            let Some(text) = arg(m, id) else { return Ok(None) };
            Paint::parse(text).map(Some).ok_or_else(|| {
                Fault::usage(format!(
                    "{text:?} is not a drawing colour; try up, down, blue, amber, violet, teal, orange, cyan, ink — or #rrggbb"
                ))
            })
        };
        let width = match number(m, "width")? {
            None => None,
            Some(w) if (0.5..=12.0).contains(&w) => Some(w),
            Some(w) => return Err(Fault::usage(format!("--width is {w}, which is outside 0.5 to 12"))),
        };
        let arrow = match arg(m, "arrow") {
            None => None,
            Some(text) => Some(
                Arrow::from_key(text)
                    .ok_or_else(|| Fault::usage(format!("{text:?} is not an arrow; try none, end, start or both")))?,
            ),
        };
        let head = match arg(m, "head") {
            None => None,
            Some(text) => Some(
                ArrowHead::from_key(text)
                    .ok_or_else(|| Fault::usage(format!("{text:?} is not an arrowhead; try filled, open or barb")))?,
            ),
        };
        let border = match arg(m, "border").map(String::as_str) {
            None => None,
            Some("on") => Some(true),
            Some("off") => Some(false),
            Some(other) => return Err(Fault::usage(format!("--border is on or off, not {other:?}"))),
        };
        Ok(StyleEdits {
            colour: paint("color")?,
            width,
            arrow,
            head,
            border,
            fill: paint("fill")?,
            alpha: fraction(m, "alpha", 0.0, 1.0)?,
        })
    }

    fn is_empty(&self) -> bool {
        self.colour.is_none()
            && self.width.is_none()
            && self.arrow.is_none()
            && self.head.is_none()
            && self.border.is_none()
            && self.fill.is_none()
            && self.alpha.is_none()
    }

    fn apply(&self, style: &mut omacharts_engine::Style) {
        if let Some(colour) = &self.colour {
            style.colour = colour.clone();
        }
        if let Some(width) = self.width {
            style.width = width;
        }
        if let Some(arrow) = self.arrow {
            style.arrow = arrow;
        }
        if let Some(head) = self.head {
            style.head = head;
        }
        if let Some(border) = self.border {
            style.border = border;
        }
        if let Some(fill) = &self.fill {
            style.fill = fill.clone();
        }
        if let Some(alpha) = self.alpha {
            style.alpha = alpha;
        }
    }
}

/// A moment as a person would write it: the date alone when it is midnight
/// local time, the minute otherwise.
fn spell_moment(ts: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(ts, 0).single() {
        Some(at) if at.format("%H:%M").to_string() == "00:00" => at.format("%Y-%m-%d").to_string(),
        Some(at) => at.format("%Y-%m-%dT%H:%M").to_string(),
        None => ts.to_string(),
    }
}

/// `WHEN,PRICE` as an anchor. The moment is a local date, a local date and
/// time, or unix seconds; the price is a number.
fn anchor(m: &clap::ArgMatches, id: &str) -> Result<Option<omacharts_engine::Anchor>, Fault> {
    use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
    let Some(text) = arg(m, id) else { return Ok(None) };
    let bad = || Fault::usage(format!("--{id} takes WHEN,PRICE — like 2026-09-01,180.5 or 2026-09-01T14:30,180.5 — not {text:?}"));
    let (when, price) = text.rsplit_once(',').ok_or_else(bad)?;
    let price: f64 = price.trim().parse().map_err(|_| bad())?;
    let when = when.trim();
    let ts = if let Ok(seconds) = when.parse::<i64>() {
        seconds
    } else if let Ok(at) = NaiveDateTime::parse_from_str(when, "%Y-%m-%dT%H:%M") {
        Local.from_local_datetime(&at).single().ok_or_else(bad)?.timestamp()
    } else if let Ok(day) = NaiveDate::parse_from_str(when, "%Y-%m-%d") {
        let at = day.and_hms_opt(0, 0, 0).ok_or_else(bad)?;
        Local.from_local_datetime(&at).single().ok_or_else(bad)?.timestamp()
    } else {
        return Err(bad());
    };
    Ok(Some(omacharts_engine::Anchor::new(ts, price)))
}

/// Does this stored indicator have that kind?
///
/// `Params` is tagged by kind and the kind is also its own field, and a value
/// written by an older build may carry only one of them.
fn is_kind(stored: &Value, kind: IndicatorKind) -> bool {
    stored["kind"].as_str() == Some(kind.key())
        || stored["params"]["kind"].as_str() == Some(kind.key())
}

/// How a chart is named back to somebody who did not name it.
fn chart_label(book: &Value, pane: u32) -> String {
    let Some(found) = charts::panes(book).iter().find(|p| p["id"].as_u64() == Some(pane as u64))
    else {
        return format!("chart {pane}");
    };
    let at = charts::position_of(book, pane).map(|at| format!("pos:{at} ")).unwrap_or_default();
    format!(
        "{at}{} {}",
        spell(found["symbol"].as_str().unwrap_or("?"), found["suffix"].as_str()),
        found["timeframe"].as_str().unwrap_or("")
    )
}

/// Which group a chart is in, as a number, with zero for none.
///
/// Wanted before a change rather than after it, because joining a group leads
/// it and re-stating the group a chart is already in must not.
fn group_of(book: &Value, pane: u32) -> u8 {
    charts::panes(book)
        .iter()
        .find(|p| p["id"].as_u64() == Some(u64::from(pane)))
        .and_then(|p| p["linked"].as_u64())
        .and_then(|group| u8::try_from(group).ok())
        .unwrap_or(0)
}

/// The chartbook already showing `watchlist`, if it is not the one at `besides`.
///
/// A list belongs to one book. Which list the rail shows is written down
/// against whichever book was saved last, so two books claiming one would
/// fight over it — the window heals that by falling the shown book back to the
/// default, and a command that could cause it in the first place is worth
/// refusing instead.
///
/// The default watchlist is nobody's: it is the one that cannot be deleted and
/// where every fallback lands, so a rule that let one book keep it would leave
/// a second book with nowhere to be.
fn book_using(workspace: &Workspace, besides: usize, watchlist: i64) -> Option<String> {
    if watchlist == DEFAULT_WATCHLIST {
        return None;
    }
    (0..workspace.books().len())
        .filter(|at| *at != besides)
        .find(|at| workspace.books()[*at]["watchlist"].as_i64() == Some(watchlist))
        .map(|at| workspace.name_of(at))
}

/// Which watchlist a chartbook shows, where a person reads it.
///
/// Empty for a book written before the rail belonged to one: those fall back
/// to whatever the window was last showing, and naming a list they do not
/// actually hold would be a worse answer than naming none.
fn rail_label(store: &Store, book: &Value) -> String {
    let Some(id) = book["watchlist"].as_i64() else { return String::new() };
    match store.watchlists().into_iter().find(|(found, _)| *found == id) {
        Some((_, name)) => format!("watchlist {name:?}"),
        None => String::new(),
    }
}

fn describe_indicator(stored: &Value) -> String {
    let kind = stored["kind"].as_str().unwrap_or("?");
    let mut text = IndicatorKind::ALL
        .into_iter()
        .find(|k| k.key() == kind)
        .map(|k| k.short_name().to_string())
        .unwrap_or_else(|| kind.to_string());
    let lengths: Vec<String> = ["period", "k_smooth", "d_period"]
        .iter()
        .filter_map(|key| stored["params"][key].as_u64().map(|n| n.to_string()))
        .collect();
    if !lengths.is_empty() {
        text.push_str(&format!("({})", lengths.join(",")));
    }
    if let Some(reset) = stored["params"]["reset"].as_str() {
        text.push_str(&format!(" · {reset}"));
    }
    text
}

/// Every parameter a command offered, read and checked before one is written.
struct Edits {
    period: Option<u64>,
    k_smooth: Option<u64>,
    d_period: Option<u64>,
    d_color: Option<ColorChoice>,
    anchor: Option<Reset>,
    rows: Option<Option<u64>>,
    value_area: Option<f64>,
    poc_color: Option<ColorChoice>,
    color: Option<ColorChoice>,
    width: Option<f64>,
    style: Option<LineStyle>,
    height: Option<f64>,
    overbought: Option<f64>,
    oversold: Option<f64>,
    bands: Option<Vec<usize>>,
    band_alpha: Option<f64>,
    visible: Option<bool>,
}

impl Edits {
    fn read(m: &clap::ArgMatches) -> Result<Edits, Fault> {
        Ok(Edits {
            period: number(m, "period")?.map(|n| n as u64),
            k_smooth: number(m, "k-smooth")?.map(|n| n as u64),
            d_period: number(m, "d-period")?.map(|n| n as u64),
            d_color: colour(m, "d-color")?,
            anchor: match arg(m, "anchor") {
                None => None,
                Some(text) => Some(Reset::from_key(text).ok_or_else(|| {
                    Fault::usage(format!("{text:?} is not an anchor; try session or week"))
                })?),
            },
            rows: match arg(m, "rows") {
                None => None,
                Some(text) if text.eq_ignore_ascii_case("auto") => Some(None),
                Some(text) => Some(Some(text.parse::<u64>().map_err(|_| {
                    Fault::usage(format!("{text:?} is not a row count; use a number or `auto`"))
                })?)),
            },
            value_area: bounded(m, "value-area", 0.0, 1.0)?,
            poc_color: colour(m, "poc-color")?,
            color: colour(m, "color")?,
            width: number(m, "width")?,
            style: match arg(m, "style") {
                None => None,
                Some(text) => Some(
                    LineStyle::ALL
                        .into_iter()
                        .find(|s| serde_json::to_value(s).ok().as_ref().and_then(Value::as_str) == Some(text.as_str()))
                        .ok_or_else(|| Fault::usage(format!("{text:?} is not a line style")))?,
                ),
            },
            // The range the engine clamps to rather than a wider one of our own:
            // a height it quietly brings back reads as the command having
            // worked, and the number it was given is not the one on screen.
            height: bounded(m, "height", MIN_PANE_SHARE, MAX_PANE_SHARE)?,
            // The strip runs 0 to 100 and a level is drawn across it; one
            // past either end is a line nowhere on the chart.
            overbought: bounded(m, "overbought", 0.0, 100.0)?,
            oversold: bounded(m, "oversold", 0.0, 100.0)?,
            bands: match arg(m, "bands") {
                None => None,
                Some(text) if text.eq_ignore_ascii_case("none") => Some(Vec::new()),
                Some(text) => Some(
                    text.split(',')
                        .map(|part| {
                            part.trim().parse::<usize>().ok().filter(|n| (1..=3).contains(n)).map(|n| n - 1)
                        })
                        .collect::<Option<Vec<usize>>>()
                        .ok_or_else(|| {
                            Fault::usage(format!("{text:?} is not a band list; try 1,2 or none"))
                        })?,
                ),
            },
            // The engine's own ends rather than a narrower pair of our own,
            // for the same reason as the height above: a figure it would
            // quietly bring back reads as the command having worked.
            band_alpha: bounded(m, "band-alpha", MIN_FILL_ALPHA, MAX_FILL_ALPHA)?,
            visible: arg(m, "visible").map(|v| v == "on"),
        })
    }

    fn is_empty(&self) -> bool {
        self.period.is_none()
            && self.k_smooth.is_none()
            && self.d_period.is_none()
            && self.d_color.is_none()
            && self.anchor.is_none()
            && self.rows.is_none()
            && self.value_area.is_none()
            && self.poc_color.is_none()
            && self.color.is_none()
            && self.width.is_none()
            && self.style.is_none()
            && self.height.is_none()
            && self.overbought.is_none()
            && self.oversold.is_none()
            && self.bands.is_none()
            && self.band_alpha.is_none()
            && self.visible.is_none()
    }

    /// Write what was given, and refuse what this indicator has no use for.
    ///
    /// Silently ignoring `--period` on a volume profile would read as the
    /// command having worked, which is the one thing a caller that cannot see
    /// the screen must never be told.
    fn apply(&self, stored: &mut Value, kind: IndicatorKind) -> Result<(), Fault> {
        let params = &mut stored["params"];
        let refuse = |what: &str| {
            Fault::usage(format!("{} has no {what}", kind.short_name()))
        };

        if let Some(period) = self.period {
            match params.get("period").is_some() {
                true => params["period"] = json!(period.max(1)),
                false => return Err(refuse("period")),
            }
        }
        if let Some(bars) = self.k_smooth {
            match params.get("k_smooth").is_some() {
                true => params["k_smooth"] = json!(bars.max(1)),
                false => return Err(refuse("%K smoothing")),
            }
        }
        if let Some(bars) = self.d_period {
            match params.get("d_period").is_some() {
                true => params["d_period"] = json!(bars.max(1)),
                false => return Err(refuse("%D smoothing")),
            }
        }
        if let Some(colour) = &self.d_color {
            match kind == IndicatorKind::Stochastic {
                true => params["d_color"] = to_value(colour)?,
                false => return Err(refuse("%D line")),
            }
        }
        if let Some(anchor) = self.anchor {
            match params.get("reset").is_some() {
                true => params["reset"] = json!(anchor.key()),
                false => return Err(refuse("anchor")),
            }
        }
        if let Some(rows) = self.rows {
            match kind == IndicatorKind::VolumeProfile {
                true => params["rows"] = json!(rows),
                false => return Err(refuse("rows")),
            }
        }
        if let Some(share) = self.value_area {
            match kind == IndicatorKind::VolumeProfile {
                true => params["value_area"] = json!(share),
                false => return Err(refuse("value area")),
            }
        }
        if let Some(colour) = &self.poc_color {
            match kind == IndicatorKind::VolumeProfile {
                true => params["poc_color"] = to_value(colour)?,
                false => return Err(refuse("point of control")),
            }
        }
        // The pair may not cross: a band whose floor is above its ceiling is
        // not a band. Whichever of the two the command left out is the one
        // already on the chart, and the one given has to clear it. Checked
        // before either is written, so a refusal leaves the chart as it was.
        if self.overbought.is_some() || self.oversold.is_some() {
            let ceiling = self.overbought.or(params["overbought"].as_f64());
            let floor = self.oversold.or(params["oversold"].as_f64());
            if let (Some(ceiling), Some(floor)) = (ceiling, floor)
                && floor >= ceiling
            {
                return Err(Fault::usage(format!(
                    "oversold {floor} is not below overbought {ceiling}"
                )));
            }
        }
        if let Some(level) = self.overbought {
            match params.get("overbought").is_some() {
                true => params["overbought"] = json!(level),
                false => return Err(refuse("overbought level")),
            }
        }
        if let Some(level) = self.oversold {
            match params.get("oversold").is_some() {
                true => params["oversold"] = json!(level),
                false => return Err(refuse("oversold level")),
            }
        }
        if let Some(height) = self.height {
            match params.get("height").is_some() {
                true => params["height"] = json!(height),
                false => return Err(refuse("pane height")),
            }
        }
        if self.bands.is_some() || self.band_alpha.is_some() {
            if kind != IndicatorKind::Vwap {
                return Err(refuse("bands"));
            }
            let Some(bands) = params["bands"].as_array_mut() else {
                return Err(refuse("bands"));
            };
            for (at, band) in bands.iter_mut().enumerate() {
                if let Some(wanted) = &self.bands {
                    band["enabled"] = json!(wanted.contains(&at));
                }
                if let Some(alpha) = self.band_alpha {
                    band["fill_alpha"] = json!(alpha);
                }
            }
        }

        if let Some(colour) = &self.color {
            stored["color"] = to_value(colour)?;
        }
        if let Some(width) = self.width {
            stored["stroke"]["width"] = json!(width.max(0.0));
        }
        if let Some(style) = self.style {
            stored["stroke"]["style"] = to_value(&style)?;
        }
        if stored["stroke"].is_null() {
            stored["stroke"] = to_value(&Stroke::default())?;
        }
        if let Some(visible) = self.visible {
            stored["visible"] = json!(visible);
        }
        Ok(())
    }
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, Fault> {
    serde_json::to_value(value).map_err(|e| Fault::new(super::EXIT_ERROR, e.to_string()))
}

fn number(m: &clap::ArgMatches, id: &str) -> Result<Option<f64>, Fault> {
    match arg(m, id) {
        None => Ok(None),
        Some(text) => text
            .parse::<f64>()
            .map(Some)
            .map_err(|_| Fault::usage(format!("--{id} takes a number, not {text:?}"))),
    }
}

fn bounded(m: &clap::ArgMatches, id: &str, low: f64, high: f64) -> Result<Option<f64>, Fault> {
    match number(m, id)? {
        None => Ok(None),
        Some(value) if (low..=high).contains(&value) => Ok(Some(value)),
        Some(value) => Err(Fault::usage(format!(
            "--{id} is {value}, which is outside {low} to {high}"
        ))),
    }
}

/// A palette name, or a hex the theme will never touch.
///
/// Both, because the app keeps them apart: a swatch is re-resolved whenever
/// the theme changes and a hex is not, so taking only hex would opt every
/// scripted indicator out of following the desktop.
fn colour(m: &clap::ArgMatches, id: &str) -> Result<Option<ColorChoice>, Fault> {
    let Some(text) = arg(m, id) else { return Ok(None) };
    if let Some(hex) = text.strip_prefix('#') {
        let valid = matches!(hex.len(), 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
        return match valid {
            true => Ok(Some(ColorChoice::Fixed { hex: format!("#{}", hex.to_lowercase()) })),
            false => Err(Fault::usage(format!("{text:?} is not a colour; try #rrggbb"))),
        };
    }
    SWATCH_NAMES
        .iter()
        .find(|name| name.eq_ignore_ascii_case(text))
        .map(|name| Some(ColorChoice::swatch(name)))
        .ok_or_else(|| {
            Fault::usage(format!(
                "{text:?} is not a palette colour; try {} — or #rrggbb for one the theme will not change",
                SWATCH_NAMES.join(", ")
            ))
        })
}

fn pane_json(pane: &Value) -> String {
    json!({
        "id": pane["id"],
        "symbol": pane["symbol"],
        "suffix": pane["suffix"],
        "display": spell(pane["symbol"].as_str().unwrap_or(""), pane["suffix"].as_str()),
        "resolution": pane["timeframe"],
        "style": pane["bar_style"],
        "session": pane["session"],
        "grid": pane["show_grid"],
        // Missing from a chart stored before it was remembered, and every one
        // of those was fitting itself.
        "auto_scale": pane["auto_scale"].as_bool().unwrap_or(true),
        "link": pane["linked"],
        "drawing_sharing": pane["drawing_sharing"].as_str().unwrap_or("global"),
        "indicators": pane["indicators"]
            .as_array()
            .map(|all| all.iter().filter_map(|i| i["kind"].as_str()).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
    .to_string()
}

// -- what is open ---------------------------------------------------------

/// The live picture, in one call.
///
/// One command rather than `chartbook current` and `chart current`, so the
/// answer cannot disagree with itself because something moved between two
/// reads. Everything it names is spelled the way the other verbs accept it,
/// so a snapshot composes into the next command without translation.
fn status(store: &Store, window_open: bool, as_json: bool) -> Result<String, Fault> {
    let workspace = Workspace::load(store);
    let at = workspace.active().min(workspace.books().len().saturating_sub(1));
    let book = workspace.books().get(at).cloned().unwrap_or_else(|| json!({}));
    let focused = book["focused"].as_u64().unwrap_or(0) as u32;
    let order = charts::leaves(&book["layout"]);
    let position = order.iter().position(|id| *id == focused);

    let charts_json: Vec<String> = order
        .iter()
        .enumerate()
        .filter_map(|(i, id)| {
            let pane = charts::panes(&book).iter().find(|p| p["id"].as_u64() == Some(*id as u64))?;
            Some(format!(
                "{{\"position\":{i},\"target\":\"pos:{i}\",\"focused\":{},\"chart\":{}}}",
                Some(*id) == Some(focused),
                pane_json(pane)
            ))
        })
        .collect();

    if as_json {
        return Ok(format!(
            "{{\"windowOpen\":{window_open},\"chartbook\":{},\"target\":{},\"watchlist\":{},\
             \"focusedPosition\":{},\"charts\":[{}]}}\n",
            super::json_string(&workspace.name_of(at)),
            super::json_string(&format!("id:{at}")),
            book["watchlist"],
            match position {
                Some(at) => at.to_string(),
                None => "null".to_string(),
            },
            charts_json.join(",")
        ));
    }

    let mut out = match window_open {
        true => String::new(),
        false => "nothing is open; this is what was stored when the window last closed\n\n"
            .to_string(),
    };
    // Nothing arranged at all is not a chartbook with nothing in it, and
    // naming one that does not exist would be an answer a caller acts on.
    if workspace.books().is_empty() {
        out.push_str("no chartbooks yet; open the app once, or create one\n");
        return Ok(out);
    }
    out.push_str(&format!("chartbook  {} (id:{at})\n", workspace.name_of(at)));
    for (i, id) in order.iter().enumerate() {
        let Some(pane) = charts::panes(&book).iter().find(|p| p["id"].as_u64() == Some(*id as u64))
        else {
            continue;
        };
        out.push_str(&format!(
            "{} pos:{i}  {:<10} {:<5} {}\n",
            if *id == focused { "*" } else { " " },
            spell(pane["symbol"].as_str().unwrap_or("?"), pane["suffix"].as_str()),
            pane["timeframe"].as_str().unwrap_or(""),
            match pane["linked"].as_u64().unwrap_or(0) {
                0 => "unlinked".to_string(),
                group => format!("link {group}"),
            },
        ));
    }
    Ok(out)
}

/// Whether a pointer on one chart draws a line on the ones linked to it.
///
/// One setting for the whole window rather than one per chart, which is why
/// it reads and writes a setting rather than a pane.
fn crosshair(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    const KEY: &str = "sync_crosshair";
    let Some(state) = arg(m, "STATE") else {
        let on = store.setting_bool(KEY, true);
        return match as_json {
            true => Ok(format!("{}\n", json!({"crosshairSync": on}))),
            false => Ok(format!("{}\n", if on { "on" } else { "off" })),
        };
    };
    let on = state == "on";
    store.set_setting_bool(KEY, on);
    said(
        as_json,
        json!({"crosshairSync": on}),
        format!("crosshair syncing across linked charts is {state}"),
    )
}

// -- settings and cache ---------------------------------------------------

/// Which feeds there are, which is stored, and which this process is using.
///
/// The three can differ: a launch given `--provider` is using one while
/// another is stored, and that is worth seeing at a glance rather than
/// being a thing somebody discovers from a chart.
fn provider_list(store: &Store, as_json: bool) -> Result<String, Fault> {
    let stored = crate::feeds::stored(store).id;
    let in_use = crate::feeds::in_use(store).id;
    if as_json {
        let rows = providers::LISTED
            .iter()
            .map(|feed| {
                json!({
                    "id": feed.id,
                    "label": feed.label,
                    "serves": feed.serves,
                    "freshness": providers::freshness(feed.id),
                    "delivery": providers::selected(Some(feed.id)).delivery().word(),
                    "stored": feed.id == stored,
                    "inUse": feed.id == in_use,
                    "needsSignIn": feed.needs_sign_in(),
                    "experimental": feed.experimental,
                    "session": providers::access(feed.id).map(|access| access.line()),
                })
                .to_string()
            })
            .collect();
        return Ok(wrap_list("providers", rows));
    }
    Ok(providers::LISTED
        .iter()
        .map(|feed| {
            let mut note = match (feed.id == stored, feed.id == in_use) {
                (true, true) => "in use".to_string(),
                (true, false) => "stored, not in use".to_string(),
                (false, true) => "in use, not stored".to_string(),
                (false, false) => String::new(),
            };
            if let Some(access) = providers::access(feed.id) {
                if !note.is_empty() {
                    note.push_str(" · ");
                }
                note.push_str(&access.line().to_lowercase());
            }
            // The word rides with what the feed serves rather than with
            // the note beside it: it is a fact about the feed, true whether
            // or not this machine is using it.
            let mut serves = providers::described(feed);
            if feed.experimental {
                serves.push_str(" · experimental");
            }
            format!("{:<8} {} · {}{}", feed.id, feed.label, serves, suffixed(&note))
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn suffixed(note: &str) -> String {
    match note.is_empty() {
        true => String::new(),
        false => format!("   ({note})"),
    }
}

/// The chosen feed in detail: is it ready, if not what is missing, how it
/// delivers bars, and what is being streamed right now.
///
/// The settings panel says none of this in words — it offers one action,
/// and which one it is says whether a session exists — so this is where the
/// session, the files it lives in and the state of every subscription are
/// spelled out for somebody who needs them.
fn provider_status(store: &Store, live: Option<&dyn Live>, as_json: bool) -> Result<String, Fault> {
    let stored = crate::feeds::stored(store);
    let in_use = crate::feeds::in_use(store);
    let access = providers::access(in_use.id);
    let browser = providers::can_sign_in(in_use.id);
    let places = providers::places(in_use.id);
    let delivery = providers::selected(Some(in_use.id)).delivery();
    let streaming = live.map(|live| live.streaming());

    if as_json {
        return Ok(format!(
            "{}\n",
            json!({
                "id": in_use.id,
                "label": in_use.label,
                "serves": in_use.serves,
                "freshness": providers::freshness(in_use.id),
                "delivery": delivery.word(),
                "subscriptions": streaming.as_deref().map(|reports| {
                    reports.iter().map(subscription_json).collect::<Vec<_>>()
                }),
                "stored": stored.id,
                "forThisLaunch": crate::feeds::for_this_launch().map(|feed| feed.id),
                "needsSignIn": in_use.needs_sign_in(),
                "ready": access.as_ref().map(|access| access.ready()).unwrap_or(true),
                "session": access.as_ref().map(|access| access.line()),
                "browser": browser.as_ref().ok(),
                "cannotSignIn": browser.as_ref().err(),
                "files": places
                    .iter()
                    .map(|(what, path)| json!({"what": what, "path": path}))
                    .collect::<Vec<_>>(),
            })
        ));
    }

    let mut out = format!("{} · {}\n", in_use.label, providers::described(in_use));
    if stored.id != in_use.id {
        out.push_str(&format!(
            "in use in the open window; {} is stored\n",
            stored.label
        ));
    }
    match &access {
        None => out.push_str("nothing to sign in to; it works as it is\n"),
        Some(access) => {
            out.push_str(&format!("{}\n", access.line()));
            if !access.ready() {
                out.push_str("charts will be empty until you sign in: omacharts provider login\n");
            }
        }
    }
    if let Err(why) = &browser
        && in_use.needs_sign_in()
    {
        out.push_str(&format!("{why}\n"));
    }
    for (what, path) in places {
        out.push_str(&format!("{}: {path}\n", what.to_lowercase()));
    }
    for line in streaming_lines(delivery, streaming.as_deref()) {
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// How bars reach a chart, and — for a feed that streams — what the window
/// holds a subscription to right now.
///
/// Honest about what has and has not arrived. A subscription that is open
/// with nothing ticked says so, rather than implying data is flowing: with
/// the market shut that is the normal state of a live chart, and the one
/// thing somebody checking whether streaming works needs to be told.
fn streaming_lines(delivery: omacharts_engine::Delivery, streaming: Option<&[crate::live::Report]>) -> Vec<String> {
    use omacharts_engine::Delivery;
    let mut lines = Vec::new();
    match delivery {
        Delivery::Polled => {
            lines.push("delivery: polled — charts are refetched on a timer".into());
            return lines;
        }
        Delivery::Streamed => {
            lines.push(
                "delivery: streamed — bars arrive as they print, and nothing fetches on a timer"
                    .into(),
            );
        }
    }
    match streaming {
        None => lines.push("no window is open, so nothing is subscribed".into()),
        Some([]) => lines.push("nothing is being streamed: no chart is showing this feed".into()),
        Some(reports) => {
            lines.push(format!(
                "streaming {} series:",
                reports.len()
            ));
            for report in reports {
                lines.push(format!("  {}", subscription_line(report)));
            }
        }
    }
    lines
}

/// One subscription, in one line.
fn subscription_line(report: &crate::live::Report) -> String {
    use crate::live::ReportState;

    let charts = match report.charts {
        1 => "1 chart".to_string(),
        n => format!("{n} charts"),
    };
    let mut line = format!("{} {} · {charts}", report.symbol, report.native.key());
    match &report.state {
        ReportState::Opening => line.push_str(" · waiting for the first snapshot"),
        ReportState::Live => {
            line.push_str(&format!(" · {} bars", report.bars));
            if let Some(ts) = report.last_bar {
                line.push_str(&format!(" · last bar {}", when(ts)));
            }
            match report.updated_ago {
                Some(ago) => line.push_str(&format!(" · updated {} ago", ago_text(ago))),
                None => line.push_str(" · nothing has arrived since the snapshot"),
            }
        }
        ReportState::Lost { why, retrying } => {
            line.push_str(&format!(" · lost: {}", why.message()));
            line.push_str(if *retrying { " · retrying" } else { " · not retrying" });
        }
    }
    line
}

fn subscription_json(report: &crate::live::Report) -> serde_json::Value {
    use crate::live::ReportState;

    let (state, why, retrying) = match &report.state {
        ReportState::Opening => ("opening", None, None),
        ReportState::Live => ("live", None, None),
        ReportState::Lost { why, retrying } => ("lost", Some(why.message()), Some(*retrying)),
    };
    json!({
        "symbol": report.symbol,
        "resolution": report.native.key(),
        "charts": report.charts,
        "bars": report.bars,
        "lastBar": report.last_bar,
        "updatedSecondsAgo": report.updated_ago.map(|ago| ago.as_secs()),
        "state": state,
        "why": why,
        "retrying": retrying,
    })
}

/// A unix second as a clock time in the local zone, which is where the
/// person reading it is sitting.
fn when(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|utc| chrono::DateTime::<chrono::Local>::from(utc).format("%H:%M").to_string())
        .unwrap_or_else(|| ts.to_string())
}

fn ago_text(ago: std::time::Duration) -> String {
    let secs = ago.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 3_600 {
        format!("{} min", secs / 60)
    } else {
        format!("{} h", secs / 3_600)
    }
}

/// Sign in to a feed, in a browser the person drives themselves.
///
/// Runs in the terminal it was typed in — see `spec::IN_THE_CALLER` — and
/// blocks for as long as the sign-in takes, printing what the browser is
/// doing to stderr as it goes. The lines go to stderr rather than into the
/// result because they are progress rather than an answer: `--json` stays
/// one object on stdout, which is what something parsing this needs.
fn provider_login(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let feed = named_or_chosen(store, m)?;
    if !feed.needs_sign_in() {
        return Err(Fault::refused(format!(
            "{} needs no signing in; it works as it is",
            feed.label
        )));
    }
    if let Err(why) = providers::can_sign_in(feed.id) {
        return Err(Fault::new(super::EXIT_ERROR, why));
    }

    eprintln!("a browser is opening at {}; sign in there", feed.label);
    providers::sign_in(feed.id, |line| eprintln!("{line}"))
        .map_err(|error| Fault::new(super::EXIT_ERROR, error))?;

    let access = providers::access(feed.id).map(|access| access.line());
    said(
        as_json,
        json!({"id": feed.id, "session": access}),
        format!("signed in to {}", feed.label),
    )
}

fn provider_logout(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let feed = named_or_chosen(store, m)?;
    if !feed.needs_sign_in() {
        return Err(Fault::refused(format!(
            "{} has nothing saved to forget",
            feed.label
        )));
    }
    // Saying what was there beats "ok": somebody signing out twice should
    // be able to tell the difference.
    let had = providers::access(feed.id).is_some_and(|access| !matches!(access, providers::Access::Missing));
    providers::sign_out(feed.id).map_err(|error| Fault::new(super::EXIT_ERROR, error))?;
    said(
        as_json,
        json!({"id": feed.id, "forgotten": had}),
        match had {
            true => format!("forgot the saved {} session", feed.label),
            false => format!("no saved {} session to forget", feed.label),
        },
    )
}

/// The feed a command named, or the one in use.
fn named_or_chosen(
    store: &Store,
    m: &clap::ArgMatches,
) -> Result<&'static providers::Listed, Fault> {
    match arg(m, "NAME") {
        Some(name) => providers::listed(name).ok_or_else(|| {
            Fault::not_found(format!(
                "no such data feed: {name:?}; the feeds are {}",
                feed_names()
            ))
        }),
        None => Ok(crate::feeds::in_use(store)),
    }
}

fn feed_names() -> String {
    providers::LISTED.iter().map(|feed| feed.id).collect::<Vec<_>>().join(", ")
}

fn config_list(store: &Store, as_json: bool) -> Result<String, Fault> {
    let settings = store.settings();
    if as_json {
        let rows = settings
            .iter()
            .map(|(k, v)| json!({"key": k, "value": v}).to_string())
            .collect();
        return Ok(wrap_list("settings", rows));
    }
    Ok(settings
        .iter()
        .map(|(k, v)| format!("{k:<24} {}", truncate(v, 60)))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

fn config_get(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let key = required(m, "KEY")?;
    let value = store
        .setting(key)
        .ok_or_else(|| Fault::not_found(format!("{key:?} has never been set")))?;
    match as_json {
        true => Ok(format!("{}\n", json!({"key": key, "value": value}))),
        false => Ok(format!("{value}\n")),
    }
}

fn config_set(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let key = required(m, "KEY")?;
    let value = required(m, "VALUE")?;

    // `config set` writes any setting by name, and that is the point of it.
    // The feed is the one value naming something from a closed set, where a
    // misspelling is not stored nonsense but a silent fall back to Yahoo —
    // somebody who typed `thinkorswimm` would chart from the wrong source
    // and be told it worked. So this one is checked, and stored canonically
    // so that `TOS` and `tos` are the same choice rather than two.
    let value = match key == crate::feeds::SETTING {
        false => value.to_string(),
        true => providers::listed(value)
            .ok_or_else(|| {
                Fault::usage(format!(
                    "no such data feed: {value:?}; the feeds are {}",
                    feed_names()
                ))
            })?
            .id
            .to_string(),
    };

    store.set_setting(key, &value);
    said(as_json, json!({"key": key, "value": value}), format!("set {key} to {value}"))
}

/// Where the scheme to go back to when the colour is put back is remembered.
///
/// The same key the preferences dialog writes, whose own constant is private
/// to it — `the_scheme_this_remembers_is_the_one_the_dialog_remembers` pins
/// the spelling. Monochrome is a scheme like any other, so choosing it without
/// writing this down would throw away a palette somebody had picked and leave
/// them on the default when they changed their mind.
const COLOURED_BARS: &str = "coloured_bar_scheme";

/// Colour or no colour, and which way round, asked the way people arrive at
/// it.
///
/// `config set bar_scheme theme-mono` reaches the same scheme, and is not the
/// same command: it does not remember what to go back to, so putting the
/// colour back lands on the default rather than on the scheme that was in use.
/// This is the question the preferences dialog asks, so the terminal asks it
/// too.
fn config_bars(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    live: Option<&dyn Live>,
) -> Result<String, Fault> {
    let current =
        store.setting(crate::theming::SETTING_BARS).unwrap_or_else(|| THEME_BARS_ID.to_string());
    let Some(state) = arg(m, "STATE") else {
        return match as_json {
            true => Ok(format!("{}\n", json!({"bars": spell_bars(&current), "scheme": current}))),
            false => Ok(format!("{}\n", spell_bars(&current))),
        };
    };
    let state = state.as_str();

    let scheme = match state {
        "monochrome" => THEME_MONO_ID.to_string(),
        "red-up" => THEME_RED_UP_ID.to_string(),
        // "coloured": stay on the palette in use if it already carries the
        // direction, and otherwise go back to the one it was left from.
        _ if spell_bars(&current) == "coloured" => current.clone(),
        _ => store.setting(COLOURED_BARS).unwrap_or_else(|| THEME_BARS_ID.to_string()),
    };
    // Leaving a palette somebody picked: remember it to come back to.
    if scheme != current && spell_bars(&current) == "coloured" {
        store.set_setting(COLOURED_BARS, &current);
    }
    store.set_setting(crate::theming::SETTING_BARS, &scheme);
    // A candle has no row to reload: an open window paints from the scheme it
    // read, so it is told to read it again.
    if let Some(live) = live {
        live.adopt_theming();
    }
    let text = match (state, scheme == current) {
        // Said rather than silently done, because "it was already monochrome"
        // and "it is monochrome now" are different answers to somebody
        // checking their own work.
        (_, true) => format!("bars were already {state}"),
        ("monochrome", _) => "bars are monochrome; the colours are remembered".to_string(),
        ("red-up", _) => "rising bars are red and falling bars green".to_string(),
        _ => format!("bars carry their direction again, in the {scheme:?} scheme"),
    };
    said(as_json, json!({"bars": state, "scheme": scheme}), text)
}

/// Which of the three answers a scheme is.
fn spell_bars(scheme: &str) -> &'static str {
    match scheme {
        THEME_MONO_ID => "monochrome",
        THEME_RED_UP_ID => "red-up",
        _ => "coloured",
    }
}

/// The widget in the Omarchy bar.
///
/// These three reach into `~/.config/omarchy` rather than the database or the
/// arrangement, which is why they neither flush nor refresh a window that is
/// open, and why a desktop with no bar is reported rather than refused: having
/// no Omarchy shell is a fact about the machine, not a command that went
/// wrong. The switch in Preferences writes the same two things.
fn plugin_status(as_json: bool) -> Result<String, Fault> {
    let home = crate::store::home();
    let where_at = crate::bar_plugin::location(&home);
    let at = where_at.display();
    let (available, installed) =
        (crate::bar_plugin::available(&home), crate::bar_plugin::installed(&home));
    match as_json {
        true => Ok(format!(
            "{}\n",
            json!({
                "id": crate::bar_plugin::PLUGIN_ID,
                "omarchy": available,
                "installed": installed,
                "path": where_at,
            })
        )),
        false => Ok(match (available, installed) {
            (false, _) => format!("no Omarchy bar on this desktop; nothing in {at}\n"),
            (true, true) => format!("in the bar, from {at}\n"),
            (true, false) => format!("not in the bar; it would go in {at}\n"),
        }),
    }
}

fn plugin_install(as_json: bool) -> Result<String, Fault> {
    let home = crate::store::home();
    if !crate::bar_plugin::available(&home) {
        return said(
            as_json,
            json!({"omarchy": false, "installed": false}),
            "no Omarchy bar on this desktop, so there is nowhere to put the widget".to_string(),
        );
    }
    // Already in the bar means bring it up to date rather than write it again:
    // a widget installed once and never touched runs whichever version shipped
    // the day the switch was flicked.
    let already = crate::bar_plugin::installed(&home);
    let done = match already {
        true => crate::bar_plugin::refresh(&home),
        false => crate::bar_plugin::install(&home),
    };
    done.map_err(|e| Fault::new(super::EXIT_ERROR, e))?;
    let at = crate::bar_plugin::location(&home);
    said(
        as_json,
        json!({"omarchy": true, "installed": true, "updated": already, "path": at}),
        match already {
            true => format!("already in the bar; brought it up to date in {}", at.display()),
            false => format!("installed in the bar, in {}", at.display()),
        },
    )
}

fn plugin_uninstall(as_json: bool) -> Result<String, Fault> {
    let home = crate::store::home();
    // Idempotent on purpose: a script that takes the widget out should not
    // have to ask first, and "it was already gone" is the outcome it wanted.
    if !crate::bar_plugin::installed(&home) {
        return said(
            as_json,
            json!({"installed": false, "removed": false}),
            "not in the bar, so there was nothing to remove".to_string(),
        );
    }
    crate::bar_plugin::remove(&home).map_err(|e| Fault::new(super::EXIT_ERROR, e))?;
    said(as_json, json!({"installed": false, "removed": true}), "taken out of the bar".to_string())
}

fn cache_status(store: &Store, as_json: bool) -> Result<String, Fault> {
    let (used, limit, series) = (store.used_bytes(), store.cache_limit(), store.cached_series());
    let share = if limit > 0 { used as f64 / limit as f64 * 100.0 } else { 0.0 };
    match as_json {
        true => Ok(format!(
            "{}\n",
            json!({"series": series, "usedBytes": used, "limitBytes": limit, "percent": share.round()})
        )),
        false => Ok(format!(
            "{series} series · {} of {} · {}%\n",
            bytes(used),
            bytes(limit),
            share.round()
        )),
    }
}

fn cache_clear(store: &Store, as_json: bool) -> Result<String, Fault> {
    let series = store.cached_series();
    store
        .clear_market_data()
        .map_err(|e| Fault::new(super::EXIT_ERROR, e.to_string()))?;
    said(as_json, json!({"cleared": series}), format!("cleared {series} cached series"))
}

fn cache_limit(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let Some(text) = arg(m, "SIZE") else {
        let limit = store.cache_limit();
        return match as_json {
            true => Ok(format!("{}\n", json!({"limitBytes": limit}))),
            false => Ok(format!("{}\n", bytes(limit))),
        };
    };
    let parsed = parse_bytes(text)
        .ok_or_else(|| Fault::usage(format!("{text:?} is not a size; try 512MB or 2GB")))?;
    store.set_cache_limit(parsed);
    said(
        as_json,
        json!({"limitBytes": parsed}),
        format!("the cache will be pruned back under {}", bytes(parsed)),
    )
}

// -- shared -------------------------------------------------------------

/// Say what happened, in whichever form was asked for.
///
/// A mutation that printed nothing would leave a caller that cannot look at
/// the screen with no way to tell a success from a no-op but to go and ask.
fn said(as_json: bool, value: Value, text: String) -> Result<String, Fault> {
    match as_json {
        true => Ok(format!("{value}\n")),
        false => Ok(format!("{text}\n")),
    }
}

fn wrap_list(name: &str, rows: Vec<String>) -> String {
    format!("{{\"{name}\":[{}]}}\n", rows.join(","))
}

/// Read an argument that this verb may not have.
///
/// Clap panics when asked for an id the subcommand never defined, and the
/// arms below are shared between verbs that take a chartbook and verbs that
/// do not. Asking politely is the difference between a missing flag and a
/// crash.
fn arg<'a>(m: &'a clap::ArgMatches, id: &str) -> Option<&'a String> {
    m.try_get_one::<String>(id).ok().flatten()
}

/// An argument the table marks required, as a failure rather than a crash.
///
/// Clap has already refused a command that left one out, so nothing here
/// means this arm and the table disagree about the argument's name — which is
/// how `chart focus` shipped looking for an `ID` the table calls `CHART`. That
/// was an `expect`, and a panic in a command runs inside the window, where it
/// crosses GLib's C trampoline and aborts the process rather than unwinding:
/// the arrangement on screen is gone, from a bug in a command that could have
/// just failed.
fn required<'a>(m: &'a clap::ArgMatches, id: &str) -> Result<&'a String, Fault> {
    arg(m, id).ok_or_else(|| {
        Fault::new(
            super::EXIT_ERROR,
            format!("this build cannot read its own {id} argument, which is a bug in it"),
        )
    })
}

fn flag(m: &clap::ArgMatches, id: &str) -> bool {
    m.try_get_one::<bool>(id).ok().flatten().copied().unwrap_or(false)
}

/// A ticker as typed, and the venue suffix it was given or spelled with.
///
/// `--suffix TW` and `2330.TW` say the same thing, and people type the second
/// because it is what every chart and every quote page shows. A dot that is
/// not a venue — `BRK.B` — is part of the ticker, and so is one the inventory
/// lists whole: `FTSEMIB.MI` is an index, not a Milan listing.
fn listing(
    index: &omacharts_engine::SearchIndex,
    symbol: &str,
    suffix: Option<String>,
) -> (String, Option<String>) {
    let symbol = symbol.to_uppercase();
    if suffix.is_some() || index.find(&symbol, None).is_some() {
        return (symbol, suffix);
    }
    match omacharts_engine::symbols::split_suffix(&symbol) {
        (ticker, Some(venue)) => (ticker.to_string(), Some(venue.to_string())),
        _ => (symbol, None),
    }
}

/// How an instrument is written where a person reads it.
pub fn spell(symbol: &str, suffix: Option<&str>) -> String {
    match suffix.filter(|s| !s.is_empty()) {
        Some(suffix) => format!("{symbol}.{suffix}"),
        None => symbol.to_string(),
    }
}

fn truncate(text: &str, width: usize) -> String {
    match text.chars().count() > width {
        false => text.to_string(),
        true => text.chars().take(width - 1).collect::<String>() + "…",
    }
}

fn bytes(count: i64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let n = count as f64;
    if n >= GB {
        format!("{:.1} GB", n / GB)
    } else {
        format!("{:.0} MB", n / MB)
    }
}

fn parse_bytes(text: &str) -> Option<i64> {
    let cleaned = text.trim().to_uppercase().replace(' ', "");
    let (number, scale) = if let Some(rest) = cleaned.strip_suffix("GB") {
        (rest, 1024.0 * 1024.0 * 1024.0)
    } else if let Some(rest) = cleaned.strip_suffix("MB") {
        (rest, 1024.0 * 1024.0)
    } else if let Some(rest) = cleaned.strip_suffix('B') {
        (rest, 1.0)
    } else {
        (cleaned.as_str(), 1.0)
    };
    let value: f64 = number.parse().ok()?;
    (value > 0.0).then_some((value * scale) as i64)
}

fn completions(shell: &str) -> String {
    super::completions::script(shell)
}

fn man_page() -> String {
    super::completions::man()
}

/// The agents, or the one directory, a `skill` verb acts on.
///
/// Reads the selection off the command line and leaves every decision about
/// what it means to `super::skill`, which owns the list of agents.
fn chosen_agents(m: &clap::ArgMatches) -> Vec<super::skill::Agent> {
    let selected: Vec<&str> = super::skill::KNOWN
        .iter()
        .filter(|known| flag(m, known.flag))
        .map(|known| known.flag)
        .collect();
    super::skill::chosen(&selected, arg(m, "to").map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(line: &str, store: &Store) -> Outcome {
        let args: Vec<String> =
            std::iter::once("omacharts".to_string()).chain(line.split_whitespace().map(String::from)).collect();
        dispatch(&args, store, None, &super::super::Here)
    }

    /// A caller who piped this text in, whatever path they named.
    struct Piped(String);

    impl Caller for Piped {
        fn read(&self, _path: &str) -> Result<String, Fault> {
            Ok(self.0.clone())
        }
    }

    fn import(file: &str, flags: &str, store: &Store) -> Outcome {
        let args: Vec<String> = ["omacharts", "watchlist", "import", "-"]
            .into_iter()
            .map(String::from)
            .chain(flags.split_whitespace().map(String::from))
            .collect();
        dispatch(&args, store, None, &Piped(file.to_string()))
    }

    /// A machine with a bit of everything an export has to carry.
    fn exporting() -> Store {
        let store = Store::memory().unwrap();
        run("watchlist add Default SPY QQQ", &store);
        run("watchlist rename Default Mine", &store);
        run("watchlist create Semis", &store);
        run("watchlist add Semis NVDA AMD", &store);
        run("section create Semis Memory", &store);
        run("watchlist add Semis MU --section Memory", &store);
        run("watchlist add Semis 2330 --suffix TW --section Memory", &store);
        run("watchlist link Semis 3", &store);
        // One symbol in two sections, which a list may hold on purpose.
        run("watchlist add Semis NVDA --section Memory", &store);
        store
    }

    #[test]
    fn an_export_imported_on_another_machine_comes_back_the_same() {
        let file = run("watchlist export", &exporting()).out;
        let elsewhere = Store::memory().unwrap();
        let out = import(&file, "", &elsewhere);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("Semis: created, 5 symbols in 1 section"), "{}", out.out);
        // Into the default list, whatever either side calls it, and under the
        // name it has here — even when the file replaces it.
        assert!(out.out.contains("Default: added 2 symbols"), "{}", out.out);
        assert_eq!(run("watchlist export", &elsewhere).out.replace("Default", "Mine"), file);
        assert!(import(&file, "--replace", &elsewhere).out.contains("Default: replaced"));
        assert!(run("watchlist list", &elsewhere).out.contains("Default"));
        assert_eq!(run("watchlist export", &elsewhere).out.replace("Default", "Mine"), file);
    }

    #[test]
    fn importing_the_same_file_twice_changes_nothing_the_second_time() {
        let file = run("watchlist export", &exporting()).out;
        let elsewhere = Store::memory().unwrap();
        import(&file, "", &elsewhere);
        let before = run("watchlist export", &elsewhere).out;
        let again = import(&file, "", &elsewhere);
        assert!(again.out.lines().all(|line| line.ends_with("already up to date")), "{}", again.out);
        assert_eq!(run("watchlist export", &elsewhere).out, before);
    }

    #[test]
    fn a_merge_only_adds_and_a_replace_makes_it_match() {
        let file = run("watchlist export Semis", &exporting()).out;
        let elsewhere = Store::memory().unwrap();
        run("watchlist create Semis", &elsewhere);
        run("watchlist add Semis INTC NVDA", &elsewhere);
        run("watchlist create Other", &elsewhere);

        let merged = import(&file, "", &elsewhere);
        // NVDA was already here, so it is not added to Memory as well.
        assert!(merged.out.contains("Semis: added 3 symbols and 1 section"), "{}", merged.out);
        let semis = run("watchlist show Semis", &elsewhere).out;
        // What was here stays, first; what was missing goes after it.
        assert!(semis.find("INTC").unwrap() < semis.find("AMD").unwrap(), "{semis}");

        let replaced = import(&file, "--replace", &elsewhere);
        assert!(replaced.out.contains("Semis: replaced"), "{}", replaced.out);
        assert!(!run("watchlist show Semis", &elsewhere).out.contains("INTC"));
        // A list the file does not name is never touched.
        assert!(run("watchlist list", &elsewhere).out.contains("Other"));
    }

    #[test]
    fn a_file_that_is_not_an_export_changes_nothing() {
        let store = exporting();
        let before = run("watchlist export", &store).out;
        for bad in [
            "not json",
            r#"{"omacharts": "chartbooks", "version": 1, "watchlists": []}"#,
            r#"{"omacharts": "watchlists", "version": 99, "watchlists": []}"#,
            r#"{"omacharts": "watchlists", "version": 1, "watchlists": [
                {"name": "A", "sections": []}, {"name": "a", "sections": []}]}"#,
        ] {
            let out = import(bad, "", &store);
            assert_eq!(out.code, super::super::EXIT_USAGE, "{bad}: {}", out.err);
        }
        assert_eq!(run("watchlist export", &store).out, before);
    }

    #[test]
    fn a_link_group_already_driven_here_is_left_where_it_is() {
        let file = run("watchlist export Semis", &exporting()).out;
        let elsewhere = Store::memory().unwrap();
        run("watchlist create Macro", &elsewhere);
        run("watchlist link Macro 3", &elsewhere);
        let out = import(&file, "", &elsewhere);
        assert!(out.out.contains("link group 3 stays with \"Macro\""), "{}", out.out);
        assert_eq!(run("watchlist link Macro", &elsewhere).out, "3\n");
        assert_eq!(run("watchlist link Semis", &elsewhere).out, "none\n");
    }

    #[test]
    fn a_watchlist_can_be_made_filled_and_read_back() {
        let store = Store::memory().unwrap();
        assert_eq!(run("watchlist create Semis", &store).code, 0);
        let added = run("watchlist add Semis NVDA AMD AVGO", &store);
        assert_eq!(added.code, 0, "{}", added.err);
        assert!(added.out.contains("added 3"), "{}", added.out);

        let shown = run("watchlist show Semis --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        let symbols = parsed["sections"][0]["symbols"].as_array().unwrap();
        assert_eq!(symbols.len(), 3);
    }

    /// The order the sections come back in, as the rail would draw them.
    fn order_of(store: &Store, list: &str) -> Vec<String> {
        let shown = run(&format!("watchlist show {list} --json"), store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        parsed["sections"]
            .as_array()
            .unwrap()
            .iter()
            .map(|section| match section["root"].as_bool() {
                Some(true) => "(no section)".to_string(),
                _ => section["name"].as_str().unwrap().to_string(),
            })
            .collect()
    }

    /// Dragging a section header in the rail, from a terminal. The symbols are
    /// positioned inside their section, so they go where it goes.
    #[test]
    fn sections_can_be_reordered_and_take_their_symbols_with_them() {
        let store = Store::memory().unwrap();
        run("section create Default Energy", &store);
        run("section create Default Metals", &store);
        run("watchlist add Default CL --section Energy", &store);
        run("watchlist add Default GC --section Metals", &store);
        assert_eq!(order_of(&store, "Default"), vec!["Energy", "Metals"]);

        let moved = run("section order Default Metals Energy", &store);
        assert_eq!(moved.code, 0, "{}", moved.err);
        assert!(moved.out.contains("Metals, Energy"), "{}", moved.out);
        assert_eq!(order_of(&store, "Default"), vec!["Metals", "Energy"]);

        let shown = run("watchlist show Default --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        assert_eq!(parsed["sections"][0]["symbols"][0]["symbol"], "GC");
        assert_eq!(parsed["sections"][1]["symbols"][0]["symbol"], "CL");
    }

    /// Naming one section is the whole gesture of "put this one first", so the
    /// rest have to keep the order they were in rather than fall into whatever
    /// order the ids happen to be.
    #[test]
    fn sections_left_out_of_an_order_keep_theirs_behind_the_ones_named() {
        let store = Store::memory().unwrap();
        for name in ["Energy", "Metals", "Crypto"] {
            run(&format!("section create Default {name}"), &store);
        }

        assert_eq!(run("section order Default Crypto", &store).code, 0);
        assert_eq!(order_of(&store, "Default"), vec!["Crypto", "Energy", "Metals"]);
    }

    /// The nameless bucket is always the first row in the rail, and there is
    /// no header to drag it by. A command that quietly gave it a position
    /// would put the loose symbols somewhere no gesture can.
    #[test]
    fn the_section_holding_loose_symbols_cannot_be_ordered() {
        let store = Store::memory().unwrap();
        run("watchlist add Default SPY", &store);
        run("section create Default Energy", &store);
        let root = run("section list Default --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&root.out).unwrap();
        let root_id = parsed["sections"][0]["id"].as_i64().unwrap();

        let refused = run(&format!("section order Default id:{root_id}"), &store);
        assert_eq!(refused.code, super::super::EXIT_REFUSED, "{}", refused.out);
        assert_eq!(order_of(&store, "Default"), vec!["(no section)", "Energy"]);
    }

    /// Naming a section twice is a line that cannot mean one thing, and
    /// guessing which of the two places was meant is worse than refusing.
    #[test]
    fn an_order_that_names_a_section_twice_is_refused() {
        let store = Store::memory().unwrap();
        run("section create Default Energy", &store);
        run("section create Default Metals", &store);
        assert_eq!(
            run("section order Default Energy Energy", &store).code,
            super::super::EXIT_USAGE
        );
        assert_eq!(run("section order Default Nope", &store).code, super::super::EXIT_NOT_FOUND);
    }

    /// Dragging a symbol between sections, from a terminal.
    #[test]
    fn a_symbol_moves_between_sections_and_says_where_it_went() {
        let store = Store::memory().unwrap();
        run("section create Default Energy", &store);
        run("watchlist add Default CL NG --section Energy", &store);
        run("watchlist add Default SPY", &store);

        let moved = run("watchlist move Default SPY --section Energy", &store);
        assert_eq!(moved.code, 0, "{}", moved.err);
        assert!(moved.out.contains("(no section)"), "{}", moved.out);
        assert!(moved.out.contains("Energy"), "{}", moved.out);

        let shown = run("watchlist show Default --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        let energy = &parsed["sections"][0];
        assert_eq!(energy["name"], "Energy", "the root is empty now and is not shown");
        let symbols: Vec<&str> = energy["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["symbol"].as_str().unwrap())
            .collect();
        assert_eq!(symbols, vec!["CL", "NG", "SPY"], "landed at the end");
    }

    /// Reordering inside one section, which is the other half of what dragging
    /// a row does and needs no destination section at all.
    #[test]
    fn a_symbol_moves_in_front_of_another_in_its_own_section() {
        let store = Store::memory().unwrap();
        run("section create Default Energy", &store);
        run("watchlist add Default CL NG BZ --section Energy", &store);

        let moved = run("watchlist move Default BZ --before CL", &store);
        assert_eq!(moved.code, 0, "{}", moved.err);

        let shown = run("watchlist show Default --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        let symbols: Vec<&str> = parsed["sections"][0]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["symbol"].as_str().unwrap())
            .collect();
        assert_eq!(symbols, vec!["BZ", "CL", "NG"]);
    }

    /// A move needs somewhere to go. Without a section or a neighbour the
    /// command would be a no-op reported as a success, which is the one answer
    /// a caller cannot act on.
    #[test]
    fn moving_a_symbol_nowhere_is_a_usage_error() {
        let store = Store::memory().unwrap();
        run("watchlist add Default SPY", &store);
        assert_eq!(run("watchlist move Default SPY", &store).code, super::super::EXIT_USAGE);
        assert_eq!(
            run("watchlist move Default NVDA --before SPY", &store).code,
            super::super::EXIT_NOT_FOUND
        );
    }

    /// One watchlist can hold the same symbol in two sections, and nothing on
    /// the command line says which one the caller was looking at. The rail has
    /// a pointer; this has to ask.
    #[test]
    fn a_symbol_in_two_sections_has_to_be_taken_out_of_the_one_named() {
        let store = Store::memory().unwrap();
        run("section create Default Energy", &store);
        run("section create Default Majors", &store);
        run("watchlist add Default CL --section Energy", &store);
        run("watchlist add Default CL --section Majors", &store);

        let refused = run("watchlist move Default CL --section Majors", &store);
        assert_eq!(refused.code, super::super::EXIT_AMBIGUOUS, "{}", refused.err);
        assert!(refused.err.contains("--from"), "{}", refused.err);

        let moved = run("watchlist move Default CL --from Energy --section Majors", &store);
        assert_eq!(moved.code, 0, "{}", moved.err);
        let shown = run("watchlist show Default --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        assert_eq!(parsed["sections"][0]["name"], "Energy");
        assert!(parsed["sections"][0]["symbols"].as_array().unwrap().is_empty(), "taken out of it");
        assert_eq!(parsed["sections"][1]["symbols"][0]["symbol"], "CL", "and still in the other");
    }

    /// An agent reads the code, not the sentence. A name that does not exist
    /// has to be distinguishable from a command that was spelled wrongly.
    #[test]
    fn naming_something_that_does_not_exist_says_so_in_the_exit_code() {
        let store = Store::memory().unwrap();
        assert_eq!(run("watchlist show Nope", &store).code, super::super::EXIT_NOT_FOUND);
        assert_eq!(run("watchlist frobnicate", &store).code, super::super::EXIT_USAGE);
    }

    #[test]
    fn the_watchlist_the_bar_widget_shows_cannot_be_deleted() {
        let store = Store::memory().unwrap();
        let refused = run("watchlist delete id:1", &store);
        assert_eq!(refused.code, super::super::EXIT_REFUSED);
        assert!(refused.err.contains("rename"), "{}", refused.err);
    }

    #[test]
    fn adding_the_same_symbol_twice_is_not_an_error() {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        assert_eq!(run("watchlist add Semis NVDA", &store).code, 0);
        assert_eq!(run("watchlist add Semis NVDA", &store).code, 0);
        let shown = run("watchlist show Semis --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        assert_eq!(parsed["sections"][0]["symbols"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_taiwan_listing_can_be_named_with_its_venue_or_with_the_flag() {
        let store = Store::memory().unwrap();
        run("watchlist create Taiwan", &store);
        let out = run("watchlist add Taiwan 2330.TW 6488.TWO", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert_eq!(run("watchlist add Taiwan 2454 --suffix TW", &store).code, 0);

        let shown = run("watchlist show Taiwan --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        let stored: Vec<(String, String)> = parsed["sections"][0]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| (e["symbol"].as_str().unwrap().into(), e["suffix"].as_str().unwrap().into()))
            .collect();
        assert_eq!(
            stored,
            [("2330", "TW"), ("6488", "TWO"), ("2454", "TW")]
                .map(|(a, b)| (a.to_string(), b.to_string())),
            "the ticker and the venue are stored apart, however they were typed"
        );
    }

    #[test]
    fn a_taiwan_listing_shows_both_its_names() {
        let store = Store::memory().unwrap();
        let out = run("symbol show 2330.TW --json", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let parsed: serde_json::Value = serde_json::from_str(&out.out).unwrap();
        assert_eq!(parsed["local_name"], "台積電");
        assert_eq!(parsed["exchange"], "TWSE");
        assert_eq!(parsed["currency"], "TWD");

        let found = run("symbol search 台積電 --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&found.out).unwrap();
        assert_eq!(parsed["symbols"][0]["display"], "2330.TW", "{}", found.out);
    }

    #[test]
    fn a_dot_that_is_not_a_venue_stays_in_the_ticker() {
        let store = Store::memory().unwrap();
        // FTSEMIB.MI is an index whose canonical symbol has the dot in it.
        let out = run("symbol show FTSEMIB.MI --json", &store);
        assert_eq!(out.code, 0, "{}", out.err);
    }

    #[test]
    fn a_ticker_nobody_lists_is_refused_rather_than_stored() {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        let out = run("watchlist add Semis NOTATICKER", &store);
        assert_eq!(out.code, super::super::EXIT_NOT_FOUND);
    }

    #[test]
    fn a_section_can_become_a_watchlist_carrying_its_symbols() {
        let store = Store::memory().unwrap();
        run("section create id:1 Energy", &store);
        run("watchlist add id:1 CL --section Energy", &store);
        let out = run("section promote id:1 Energy", &store);
        assert_eq!(out.code, 0, "{}", out.err);

        let listed = run("watchlist list --json", &store);
        assert!(listed.out.contains("Energy"), "{}", listed.out);
        assert!(!run("section list id:1 --json", &store).out.contains("Energy"));
    }

    #[test]
    fn a_chartbook_can_be_created_split_and_read_back() {
        let store = Store::memory().unwrap();
        assert_eq!(run("chartbook create Macro --symbol SPY --switch", &store).code, 0);
        let split = run("chart split horizontal --book Macro", &store);
        assert_eq!(split.code, 0, "{}", split.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn the_last_chart_in_a_chartbook_cannot_be_closed() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        assert_eq!(run("chart close --book Macro", &store).code, super::super::EXIT_REFUSED);
    }

    #[test]
    fn a_charts_symbol_and_resolution_can_be_set_together() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let set = run("chart set --book Macro --symbol NVDA --resolution 1h --style ohlc", &store);
        assert_eq!(set.code, 0, "{}", set.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA");
        assert_eq!(parsed["charts"][0]["resolution"], "1h");
    }

    /// A command that half-worked is worse than one that refused: the caller
    /// has no way to tell which half.
    #[test]
    fn one_bad_value_leaves_the_chart_exactly_as_it_was() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol SPY --switch", &store);
        let out = run("chart set --book Macro --symbol NVDA --resolution nonsense", &store);
        assert_eq!(out.code, super::super::EXIT_USAGE);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "SPY", "the symbol must not have moved");
    }

    /// Whether the price axis fits itself is stored with the chart, like the
    /// gridlines, and a command can hold it still or let it go.
    #[test]
    fn a_charts_price_scale_can_be_held_and_released_from_a_command() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol SPY --switch", &store);
        let charts = |store: &Store| -> serde_json::Value {
            serde_json::from_str(&run("chart list --book Macro --json", store).out).unwrap()
        };
        assert_eq!(charts(&store)["charts"][0]["auto_scale"], true, "fits itself to begin with");

        let held = run("chart set --book Macro --auto-scale off", &store);
        assert_eq!(held.code, 0, "{}", held.err);
        assert!(held.out.contains("auto-scale off"), "{}", held.out);
        assert_eq!(charts(&store)["charts"][0]["auto_scale"], false);

        run("chart set --book Macro --auto-scale on", &store);
        assert_eq!(charts(&store)["charts"][0]["auto_scale"], true);
    }

    /// A chart stored before the price scale was remembered says nothing
    /// about it, and is reported as fitting itself rather than as `null`.
    #[test]
    fn a_chart_stored_before_the_price_scale_was_remembered_is_reported_as_fitting_itself() {
        let store = Store::memory().unwrap();
        store.set_setting(
            "workspace",
            r#"{"books":[{"name":"Old","layout":{"leaf":1},"focused":1,
                "panes":[{"id":1,"symbol":"SPY","timeframe":"1D","indicators":[],
                          "bar_style":"candles","session":"extended","show_grid":true,"linked":0}]}],
               "active":0}"#,
        );
        let listed = run("chart list --book Old --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["auto_scale"], true);
    }

    /// The window writes down which chart fills it. Closing that chart from
    /// a terminal takes the note with it; closing any other leaves it alone.
    #[test]
    fn closing_the_chart_that_filled_the_window_forgets_the_fill() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol SPY --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart split horizontal --book Macro", &store);
        let stored = |store: &Store| -> serde_json::Value {
            serde_json::from_str(&store.setting("workspace").unwrap()).unwrap()
        };
        let mut workspace = stored(&store);
        workspace["books"][0]["maximized"] = serde_json::json!(2);
        store.set_setting("workspace", &workspace.to_string());

        run("chart close --book Macro --chart 3", &store);
        assert_eq!(stored(&store)["books"][0]["maximized"], 2, "another chart went, not this one");

        run("chart close --book Macro --chart 2", &store);
        assert!(stored(&store)["books"][0]["maximized"].is_null());
    }

    /// The widget on somebody's bar is running the old spelling right now,
    /// and it updates on a timer. Growing subcommands under `watchlist` must
    /// not answer it with a usage error.
    #[test]
    fn the_bar_widgets_own_command_still_means_what_it_did() {
        let store = Store::memory().unwrap();
        // Without `--refresh`, which would go to the network: this is about
        // the shape of the answer, not about fetching one.
        let out = run("watchlist", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let parsed: serde_json::Value = serde_json::from_str(&out.out).unwrap();
        assert!(parsed["sections"].is_array(), "the bare form must answer with the feed");
    }

    /// The whole point of the implicit default is "the chart in front of me".
    /// With nothing in front of anybody, answering from what was stored when
    /// the window last closed would be indistinguishable from answering
    /// correctly — and an agent would act on it.
    #[test]
    fn asking_for_the_focused_chart_with_nothing_open_says_so() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add rsi", &store);
        assert_eq!(out.code, super::super::EXIT_NO_WINDOW);
        assert!(out.err.contains("name one, or start Omacharts"), "{}", out.err);
    }

    #[test]
    fn an_indicator_carries_the_parameters_it_was_given() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let out = run(
            "chart indicator add sma --book Macro --period 200 --color Amber --style dashed --width 2",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let sma = &parsed["indicators"][0];
        assert_eq!(sma["params"]["period"], 200);
        assert_eq!(sma["color"]["name"], "Amber", "a swatch, so it follows the theme");
        assert_eq!(sma["stroke"]["style"], "dashed");
        assert_eq!(sma["stroke"]["width"], 2.0);
    }

    /// A hex is the colour somebody picked and is never re-resolved; a name is
    /// the theme's and is. Both have to arrive as what they are.
    #[test]
    fn a_colour_is_either_the_themes_or_the_users_and_the_two_stay_apart() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add ema --book Macro --color #ff8800", &store);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["indicators"][0]["color"]["hex"], "#ff8800");

        let bad = run("chart indicator add rsi --book Macro --color Chartreuse", &store);
        assert_eq!(bad.code, super::super::EXIT_USAGE);
        assert!(bad.err.contains("Blue"), "the error has to name the palette: {}", bad.err);
    }

    #[test]
    fn the_volume_profile_takes_auto_rows_and_its_own_two_colours() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let out = run(
            "chart indicator add volume_profile --book Macro --rows auto --anchor week \
             --color Violet --poc-color Amber",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let vp = &parsed["indicators"][0];
        assert!(vp["params"]["rows"].is_null(), "auto is stored as no number at all");
        assert_eq!(vp["params"]["reset"], "week");
        assert_eq!(vp["params"]["poc_color"]["name"], "Amber");
    }

    /// Quietly ignoring a parameter the indicator has no use for would read
    /// as the command having worked.
    #[test]
    fn a_parameter_an_indicator_has_no_use_for_is_refused() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add volume --book Macro --period 50", &store);
        assert_eq!(out.code, super::super::EXIT_USAGE);
        assert!(out.err.contains("no period"), "{}", out.err);
    }

    #[test]
    fn an_indicator_already_on_a_chart_can_be_reconfigured() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add rsi --book Macro", &store);
        let out = run("chart indicator set rsi --book Macro --period 21 --overbought 80", &store);
        assert_eq!(out.code, 0, "{}", out.err);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["indicators"][0]["params"]["period"], 21);
        assert_eq!(parsed["indicators"][0]["params"]["overbought"], 80.0);
    }

    #[test]
    fn a_stochastic_takes_its_smoothings_and_a_d_colour_and_nothing_else_does() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add stochastic --book Macro", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("Stoch(14,3,3)"), "{}", out.out);

        let out = run(
            "chart indicator set stochastic --book Macro --period 5 --k-smooth 1 \
             --d-period 5 --overbought 85 --d-color Amber",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let stoch = &parsed["indicators"][0]["params"];
        assert_eq!(stoch["period"], 5);
        assert_eq!(stoch["k_smooth"], 1);
        assert_eq!(stoch["d_period"], 5);
        assert_eq!(stoch["overbought"], 85.0);
        assert_eq!(stoch["d_color"]["name"], "Amber");

        run("chart indicator add rsi --book Macro", &store);
        let refused = run("chart indicator set rsi --book Macro --d-period 5", &store);
        assert_ne!(refused.code, 0);
    }

    /// A level off the strip, or a floor at or above its ceiling, is a band
    /// the chart cannot draw. The command says so and stores nothing, on an
    /// indicator being added as much as on one being changed.
    #[test]
    fn a_level_off_the_strip_or_a_crossed_pair_is_refused() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add rsi --book Macro", &store);
        for args in [
            "--overbought 120",
            "--oversold -5",
            "--oversold 75",
            "--overbought 25",
            "--overbought 60 --oversold 60",
        ] {
            let out = run(&format!("chart indicator set rsi --book Macro {args}"), &store);
            assert_eq!(out.code, super::super::EXIT_USAGE, "{args}: {}", out.err);
        }
        let out = run("chart indicator add stochastic --book Macro --overbought 20 --oversold 80", &store);
        assert_eq!(out.code, super::super::EXIT_USAGE, "{}", out.err);
        assert!(out.err.contains("not below"), "{}", out.err);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let indicators = parsed["indicators"].as_array().unwrap();
        assert_eq!(indicators.len(), 1, "the refused stochastic was not added");
        assert_eq!(indicators[0]["params"]["overbought"], 70.0);
        assert_eq!(indicators[0]["params"]["oversold"], 30.0);

        // Moving both at once past where either was is fine: only the pair
        // the chart ends up with is judged.
        let out = run("chart indicator set rsi --book Macro --overbought 25 --oversold 10", &store);
        assert_eq!(out.code, 0, "{}", out.err);
    }

    #[test]
    fn vwap_bands_can_be_turned_on_and_shaded() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add vwap --book Macro --anchor month --bands 1,2 --band-alpha 0.3", &store);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let bands = parsed["indicators"][0]["params"]["bands"].as_array().unwrap();
        assert_eq!(bands[0]["enabled"], true);
        assert_eq!(bands[1]["enabled"], true);
        assert_eq!(bands[2]["enabled"], false);
        assert_eq!(bands[0]["fill_alpha"], 0.3);
    }

    /// The flag takes the whole of what an alpha is, and the figure it is
    /// given is the figure stored — the help says 0-1 and means it.
    #[test]
    fn shading_can_be_asked_for_clear_or_solid() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add vwap --book Macro --bands 1", &store);

        for wanted in ["0", "1"] {
            let out = run(
                &format!("chart indicator set vwap --book Macro --band-alpha {wanted}"),
                &store,
            );
            assert_eq!(out.code, 0, "{}", out.err);

            let listed = run("chart indicator list --book Macro --json", &store);
            let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
            let bands = parsed["indicators"][0]["params"]["bands"].as_array().unwrap();
            assert_eq!(bands[0]["fill_alpha"], wanted.parse::<f64>().unwrap());
        }

        // Wider is not anything-goes: past either end, and anything that is
        // not a figure at all, is still refused rather than quietly brought
        // back to the nearest end.
        for refused in ["1.2", "-0.1", "nan", "inf"] {
            let out = run(
                &format!("chart indicator set vwap --book Macro --band-alpha {refused}"),
                &store,
            );
            assert_ne!(out.code, 0, "--band-alpha {refused} was accepted");
        }
    }

    /// A command that resolved its own target has to say which one it found,
    /// or nobody can catch it reaching the wrong chart.
    #[test]
    fn a_command_names_the_chart_it_acted_on() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        let out = run("chart indicator add rsi --book Macro", &store);
        assert!(out.out.contains("AAPL"), "{}", out.out);
        assert!(out.out.contains("Macro"), "{}", out.out);
    }

    #[test]
    fn status_says_whether_anything_is_actually_open() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        let out = run("status show --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&out.out).unwrap();
        assert_eq!(parsed["windowOpen"], false, "nothing is running in a test");
        assert_eq!(parsed["chartbook"], "Macro");
        assert_eq!(parsed["charts"][0]["target"], "pos:0", "spelled as the other verbs take it");
    }

    #[test]
    fn crosshair_syncing_can_be_read_and_set() {
        let store = Store::memory().unwrap();
        assert_eq!(run("chart crosshair off", &store).code, 0);
        assert_eq!(run("chart crosshair", &store).out.trim(), "off");
        assert_eq!(run("chart crosshair on", &store).code, 0);
        assert_eq!(run("chart crosshair", &store).out.trim(), "on");
    }

    #[test]
    fn sizes_are_read_the_way_people_write_them() {
        assert_eq!(parse_bytes("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_bytes("512 MB"), Some(512 * 1024 * 1024));
        assert_eq!(parse_bytes("nonsense"), None);
        assert_eq!(parse_bytes("0"), None, "a cache of nothing is not a limit");
    }

    #[test]
    fn a_watchlists_group_can_be_read_set_and_cleared() {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        assert_eq!(run("watchlist link Semis", &store).out.trim(), "none");

        let set = run("watchlist link Semis 3", &store);
        assert_eq!(set.code, 0, "{}", set.err);
        assert_eq!(run("watchlist link Semis", &store).out.trim(), "3");
        assert!(run("watchlist list", &store).out.contains("link 3"));

        assert_eq!(run("watchlist link Semis none", &store).code, 0);
        assert_eq!(run("watchlist link Semis", &store).out.trim(), "none");
    }

    /// A group drives one list. Taking one off another list is a thing to do
    /// deliberately, so a command that wanted it has to be told no first.
    #[test]
    fn a_group_another_watchlist_drives_is_refused_rather_than_taken() {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        run("watchlist create Energy", &store);
        run("watchlist link Semis 5", &store);

        let refused = run("watchlist link Energy 5", &store);
        assert_eq!(refused.code, super::super::EXIT_REFUSED);
        assert!(refused.err.contains("Semis"), "it has to name the holder: {}", refused.err);
        assert_eq!(run("watchlist link Semis", &store).out.trim(), "5", "and leave it where it was");
    }

    /// The rail and this command have to be writing down the same thing. The
    /// window's own helper for the key is private to it, so this writes through
    /// the command and reads back through the window's own reader: spell the
    /// key differently in either place and this fails.
    #[test]
    fn the_group_a_command_sets_is_the_one_the_rail_reads() {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        run("watchlist link Semis 4", &store);
        let owners = crate::ui::watchlist::group_owners(&store);
        assert!(
            owners.iter().any(|(group, _, name)| *group == 4 && name == "Semis"),
            "the rail reads {owners:?}"
        );
    }

    /// The default watchlist drives group 1 out of the box, and the listing has
    /// to say so — it is the one group whose holder nobody chose.
    #[test]
    fn the_default_watchlist_is_reported_driving_the_group_it_starts_in() {
        let store = Store::memory().unwrap();
        assert_eq!(run("watchlist link id:1", &store).out.trim(), "1");
    }

    #[test]
    fn bars_can_turn_red_up_and_get_the_same_scheme_back() {
        let store = Store::memory().unwrap();
        run("config set bar_scheme hollow", &store);
        assert_eq!(run("config bars red-up", &store).code, 0);
        assert_eq!(run("config bars", &store).out.trim(), "red-up");
        assert_eq!(run("config get bar_scheme", &store).out.trim(), "theme-red-up");

        // From one of the two answers to the other, and back: the palette
        // remembered is still the one somebody picked.
        assert_eq!(run("config bars monochrome", &store).code, 0);
        assert_eq!(run("config bars coloured", &store).code, 0);
        assert_eq!(run("config get bar_scheme", &store).out.trim(), "hollow");
    }

    #[test]
    fn bars_can_lose_their_colour_and_get_the_same_scheme_back() {
        let store = Store::memory().unwrap();
        run("config set bar_scheme hollow", &store);
        assert_eq!(run("config bars", &store).out.trim(), "coloured");

        assert_eq!(run("config bars monochrome", &store).code, 0);
        assert_eq!(run("config bars", &store).out.trim(), "monochrome");
        assert_eq!(run("config get bar_scheme", &store).out.trim(), "theme-mono");

        assert_eq!(run("config bars coloured", &store).code, 0);
        assert_eq!(
            run("config get bar_scheme", &store).out.trim(),
            "hollow",
            "the scheme that was in use has to come back, not the default"
        );
    }

    /// The preferences dialog remembers the coloured scheme under this key and
    /// its own constant is private to it, so a rename there would leave the two
    /// quietly disagreeing: the dialog would put back a scheme the terminal
    /// never wrote.
    #[test]
    fn the_scheme_this_remembers_is_the_one_the_dialog_remembers() {
        let dialog = include_str!("../ui/preferences.rs");
        assert!(
            dialog.contains(&format!("\"{COLOURED_BARS}\"")),
            "the preferences dialog does not mention {COLOURED_BARS}"
        );
    }

    #[test]
    fn a_chart_is_focused_by_its_position_as_well_as_by_its_id() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:1 --symbol NVDA", &store);

        let out = run("chart focus pos:0 --book Macro", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("pos:0"), "it has to name what it focused: {}", out.out);
        assert!(out.out.contains("AAPL"), "{}", out.out);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["id"], 1);
    }

    /// Joining a group leads it. You linked *this* chart, so what it is showing
    /// is the symbol you meant the group to be on — and the chain and the
    /// canvas agree either way round, which is the state the window goes out of
    /// its way to keep.
    #[test]
    fn a_chart_joining_a_group_leads_it_onto_what_that_chart_is_showing() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link 2", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD", &store);

        let joined = run("chart set --book Macro --chart pos:1 --link 2", &store);
        assert_eq!(joined.code, 0, "{}", joined.err);
        assert!(joined.out.contains("AMD"), "it has to say what followed: {}", joined.out);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "AMD", "the chart already in it follows");
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
    }

    /// A symbol named in the same command is what the chart ends up showing,
    /// so it is also what the group is led onto — read after the write rather
    /// than before it.
    #[test]
    fn a_symbol_named_alongside_a_group_is_the_one_the_group_follows() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link 2", &store);

        run("chart set --book Macro --chart pos:1 --symbol AMD --link 2", &store);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
        assert_eq!(parsed["charts"][0]["symbol"], "AMD", "and the group follows the new symbol");
    }

    /// Only an actual change leads the group. A radio menu makes re-picking the
    /// group you are already in easy to do by accident, and a popover re-states
    /// its own state every time it opens — either of those quietly rewriting
    /// four charts would make the whole control unusable.
    #[test]
    fn re_stating_the_group_a_chart_is_already_in_moves_nobody() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link 2", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD --link 2", &store);
        // Both on AMD now. Put one of them somewhere else by hand, then re-pick
        // the group it has been in all along.
        run("chart set --book Macro --chart pos:0 --symbol NVDA", &store);

        let again = run("chart set --book Macro --chart pos:1 --link 2", &store);
        assert_eq!(again.code, 0, "{}", again.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA", "nothing was asked for, so nothing moved");
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
    }

    /// Leaving a group changes nothing about the group. The chart stops
    /// following and stops leading; the charts still in it keep what they were
    /// showing, because unlinking is a statement about one chart.
    #[test]
    fn leaving_a_group_leaves_the_charts_still_in_it_alone() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link 2", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD --link 2", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD", &store);

        let left = run("chart set --book Macro --chart pos:1 --link none", &store);
        assert_eq!(left.code, 0, "{}", left.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "AMD", "group 2 keeps what it had");
        assert_eq!(parsed["charts"][1]["link"], 0, "and this one is out of it");
    }

    /// Moving between two groups writes to the one being joined and not to the
    /// one being left, which are two different sets of charts.
    #[test]
    fn moving_from_one_group_to_another_leads_only_the_group_it_joins() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart split horizontal --book Macro --chart pos:1", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link 2", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD --link 5", &store);
        run("chart set --book Macro --chart pos:2 --symbol TSM --link 2", &store);
        // Joining group 2 just led it onto TSM. Put group 2 back on NVDA, so
        // that what the move below does to it — nothing — is visible.
        run("chart set --book Macro --chart pos:0 --symbol NVDA", &store);

        run("chart set --book Macro --chart pos:2 --link 5", &store);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][1]["symbol"], "TSM", "group 5 follows what joined it");
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA", "group 2 was only left, not written to");
    }

    /// A chart with nothing on it leads with nothing. Blanking a group because
    /// an empty chart joined it would be the one way this feature could destroy
    /// what somebody was looking at.
    #[test]
    fn a_chart_with_no_symbol_does_not_blank_the_group_it_joins() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link 2", &store);
        // Nothing in the command surface can empty a chart, so this reaches
        // past it — which is the only way the stored arrangement gets into this
        // state, and it is reachable: `config set workspace` writes it whole.
        let mut workspace = charts::Workspace::load(&store);
        let book = workspace.book_mut(0).unwrap();
        charts::pane_mut(book, 2).unwrap()["symbol"] = json!("");
        workspace.save(&store);

        let joined = run("chart set --book Macro --chart pos:1 --link 2", &store);
        assert_eq!(joined.code, 0, "{}", joined.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA", "the group keeps what it was showing");
    }

    /// A group reaches across chartbooks, so joining one has to write into the
    /// books that are not open. Stopping at the open one would make a group
    /// mean "the charts in this group I can currently see", and the book you
    /// switched back to would show a symbol from before the link.
    #[test]
    fn joining_a_group_leads_the_charts_in_the_books_that_are_away() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chartbook create Chips --symbol MU", &store);
        run("chart set --book Chips --chart pos:0 --link 4", &store);
        run("chart set --book Chips --chart pos:0 --symbol MU", &store);

        let joined = run("chart set --book Macro --chart pos:0 --symbol NVDA --link 4", &store);
        assert_eq!(joined.code, 0, "{}", joined.err);
        assert!(joined.out.contains("group 4"), "it has to say so: {}", joined.out);

        let listed = run("chart list --book Chips --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA", "the book that is away followed");
    }

    /// Which list the rail shows is written down against whichever book was
    /// saved last, so two books claiming one would fight over it. The window
    /// heals that on display; refusing it is better than relying on the heal.
    #[test]
    fn a_watchlist_one_chartbook_shows_is_not_handed_to_a_second() {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        run("chartbook create Chips --watchlist Semis --switch", &store);
        run("chartbook create Macro --symbol AAPL", &store);

        let refused = run("chartbook watchlist Macro Semis", &store);
        assert_eq!(refused.code, super::super::EXIT_REFUSED);
        assert!(refused.err.contains("Chips"), "it has to name the holder: {}", refused.err);

        let creating = run("chartbook create Again --watchlist Semis", &store);
        assert_eq!(creating.code, super::super::EXIT_REFUSED, "{}", creating.out);

        assert_eq!(
            run("chartbook watchlist Chips Semis", &store).code,
            0,
            "the book that already shows it is not another book"
        );
    }

    /// The default watchlist is where every fallback lands, so it is nobody's
    /// and every book may show it.
    #[test]
    fn the_default_watchlist_is_not_spoken_for_by_the_book_that_shows_it() {
        let store = Store::memory().unwrap();
        run("chartbook create Chips --watchlist Default --switch", &store);
        run("chartbook create Macro --symbol AAPL", &store);
        assert_eq!(run("chartbook watchlist Macro Default", &store).code, 0);
    }

    /// `config set workspace` can put anything at all in the row the
    /// arrangement is read back out of, and a command that found something
    /// unexpected there used to abort the window rather than fail.
    #[test]
    fn an_arrangement_that_is_not_one_fails_the_command_and_nothing_else() {
        let store = Store::memory().unwrap();
        store.set_setting(
            "workspace",
            r#"{"books":[{"name":"Odd","layout":{"leaf":1},"focused":1}],"active":0}"#,
        );
        for line in ["chart split horizontal --book Odd", "chart close --book Odd", "chart list --book Odd"] {
            let outcome = run(line, &store);
            assert_ne!(outcome.code, super::super::EXIT_BUG, "{line}: {}", outcome.err);
        }
    }

    /// Where a window would be, so the verbs that default to the focused chart
    /// run rather than refusing. Nothing to flush or redraw: the store is the
    /// only thing a test has.
    struct NoWindow;

    impl Live for NoWindow {
        fn flush_workspace(&self) {}
        fn reload_workspace(&self) {}
        fn reload_watchlists(&self) {}
        fn adopt_theming(&self) {}
        fn warm(&self, _instruments: &[omacharts_engine::Instrument]) {}

        /// No window means no index to borrow, so whoever needs one builds it.
        fn symbols(&self) -> Option<std::rc::Rc<omacharts_engine::SearchIndex>> {
            None
        }

        /// A test has no pixels. The other methods stand in for a window; this
        /// one stands in for there being nothing on screen to photograph,
        /// which is a refusal and not a usage error.
        fn screenshot(
            &self,
            _whole_book: bool,
            _into: Option<&std::path::Path>,
            _clipboard: bool,
        ) -> Result<(String, std::path::PathBuf), Fault> {
            Err(Fault::new(crate::cli::EXIT_ERROR, "a test has nothing on screen".to_string()))
        }
    }

    /// Every example in the table is a command that runs.
    ///
    /// Examples are what an agent copies, so one that cannot be parsed is
    /// worse than no example at all — it is an instruction to try something
    /// that can only fail. A sibling test checks each example names its own
    /// command; this one runs it, which is the only way to catch an argument
    /// the table describes and the arm never reads.
    ///
    /// Against a seeded store with a window standing in, so the implicit
    /// "the chart I am looking at" resolves instead of refusing. A usage error
    /// is the failure: anything else is this store not happening to hold what
    /// the example names, which is not what is being tested.
    #[test]
    fn every_example_in_the_table_is_a_command_that_runs() {
        for noun in super::super::spec::SURFACE {
            for verb in noun.verbs {
                // The one example that would go to the network. Its shape is
                // covered by `the_bar_widgets_own_command_still_means_what_it_did`.
                if (noun.name, verb.name) == ("watchlist", "feed") {
                    continue;
                }
                let store = seeded();
                let mut args: Vec<String> =
                    verb.example.split_whitespace().map(String::from).collect();
                // A verb that writes outside the database is told where to
                // write. `skill install` is the case: running the tests must
                // not install a skill into the configuration of whoever is
                // running them, and the example is run rather than skipped
                // because what this test is for — an argument the table
                // describes and the arm never reads — is exactly as worth
                // catching there as anywhere else.
                // Only `skill`'s `--to` is a place on disk; a drawing's
                // `--to` is the other end of a line.
                if noun.name == "skill" && verb.flags.iter().any(|flag| flag.long == "to") {
                    let scratch = std::env::temp_dir()
                        .join(format!("omacharts-example-{}", std::process::id()))
                        .join(verb.name);
                    let _ = std::fs::remove_dir_all(&scratch);
                    args.push("--to".to_string());
                    args.push(scratch.to_string_lossy().into_owned());
                }
                let outcome = dispatch(&args, &store, Some(&NoWindow), &super::super::Here);
                assert_ne!(
                    outcome.code,
                    super::super::EXIT_USAGE,
                    "{:?} is not a command this parser accepts: {}",
                    verb.example,
                    outcome.err
                );
            }
        }
    }

    /// A drawing belongs to the symbol: added through one chart, it is listed
    /// through any chart showing that symbol that shares it, and absent from
    /// a chart that shows another.
    #[test]
    fn a_drawing_is_on_the_symbol_not_the_chart() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split vertical --book Macro", &store);
        run("chart set --book Macro --chart pos:1 --symbol NVDA", &store);

        let out = run(
            "chart drawing add line --book Macro --chart pos:0 --from 2026-09-01,180.5 --to 2026-09-19,192 --config 4",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("drew #1 line"), "{}", out.out);
        assert!(out.out.contains("configuration 4"), "{}", out.out);
        assert!(out.out.contains("pos:0 AAPL"), "{}", out.out);

        let on_aapl = run("chart drawing list --book Macro --chart pos:0 --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&on_aapl.out).unwrap();
        assert_eq!(parsed["drawings"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["drawings"][0]["kind"], "line");
        assert_eq!(parsed["drawings"][0]["config"], 4);
        assert_eq!(parsed["drawings"][0]["style"]["color"], "amber");
        assert_eq!(parsed["drawings"][0]["scope"], "global");
        assert_eq!(parsed["drawings"][0]["from"]["price"], 180.5);
        assert_eq!(parsed["drawings"][0]["from"]["when"], "2026-09-01");

        let on_nvda = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&on_nvda.out).unwrap();
        assert!(parsed["drawings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_drawing_can_be_moved_restyled_and_removed() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart drawing add rect --book Macro --from 2026-09-01,180 --to 2026-09-19,192", &store);
        run("chart drawing add line --book Macro --from 2026-09-01,100 --to 2026-09-19,110", &store);

        // A property set by hand takes the drawing off its configuration.
        let out = run("chart drawing set --book Macro --id 1 --fill ink --alpha 0.3 --to 2026-09-30T14:30,195", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["drawings"][0]["config"], serde_json::Value::Null);
        assert_eq!(parsed["drawings"][0]["style"]["fill"], "ink");
        assert_eq!(parsed["drawings"][0]["style"]["alpha"], 0.3);
        assert_eq!(parsed["drawings"][0]["to"]["price"], 195.0);
        assert_eq!(parsed["drawings"][0]["to"]["when"], "2026-09-30T14:30");

        // And a configuration puts it back on one.
        run("chart drawing set --book Macro --id 1 --config 2", &store);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["drawings"][0]["config"], 2);
        assert_eq!(parsed["drawings"][0]["style"]["fill"], "down");

        let out = run("chart drawing remove --book Macro --id 1", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["drawings"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["drawings"][0]["id"], 2);

        let out = run("chart drawing clear --book Macro", &store);
        assert!(out.out.contains("removed 1 drawing"), "{}", out.out);
        let listed = run("chart drawing list --book Macro", &store);
        assert!(listed.out.starts_with("nothing drawn on AAPL"), "{}", listed.out);
    }

    /// A chart's sharing decides what it sees and what it draws into; a
    /// local drawing lives with the chart and has a negative id.
    #[test]
    fn sharing_decides_what_a_chart_sees() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split vertical --book Macro", &store);
        run("chart set --book Macro --chart pos:1 --symbol AAPL --drawing-sharing group-2", &store);

        run("chart drawing add line --book Macro --chart pos:0 --from 2026-09-01,100 --to 2026-09-19,110", &store);
        run("chart drawing add line --book Macro --chart pos:1 --from 2026-09-01,120 --to 2026-09-19,130", &store);
        run("chart drawing add rect --book Macro --chart pos:1 --from 2026-09-01,140 --to 2026-09-19,150 --scope local", &store);

        let global = run("chart drawing list --book Macro --chart pos:0 --json", &store);
        let global: serde_json::Value = serde_json::from_str(&global.out).unwrap();
        assert_eq!(global["sharing"], "global");
        let scopes: Vec<&str> = global["drawings"].as_array().unwrap().iter().map(|d| d["scope"].as_str().unwrap()).collect();
        assert_eq!(scopes, vec!["global"], "a global chart sees global drawings only");

        let grouped = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let grouped: serde_json::Value = serde_json::from_str(&grouped.out).unwrap();
        let mut scopes: Vec<&str> = grouped["drawings"].as_array().unwrap().iter().map(|d| d["scope"].as_str().unwrap()).collect();
        scopes.sort();
        assert_eq!(scopes, vec!["global", "group-2", "local"]);
        let local = grouped["drawings"].as_array().unwrap().iter().find(|d| d["scope"] == "local").unwrap();
        assert!(local["id"].as_i64().unwrap() < 0, "a local drawing is numbered below the store's");

        // A local drawing is removed through its chart, like any other.
        let id = local["id"].as_i64().unwrap();
        let out = run(&format!("chart drawing remove --book Macro --chart pos:1 --id {id}"), &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let grouped = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let grouped: serde_json::Value = serde_json::from_str(&grouped.out).unwrap();
        assert_eq!(grouped["drawings"].as_array().unwrap().len(), 2);
    }

    /// Editing a configuration changes every drawing that follows it and
    /// none that has a look of its own; restoring the defaults puts the nine
    /// back.
    #[test]
    fn a_configuration_moves_the_drawings_that_follow_it() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart drawing add line --book Macro --from 2026-09-01,100 --to 2026-09-19,110 --config 3", &store);
        run("chart drawing add line --book Macro --from 2026-09-01,120 --to 2026-09-19,130 --config 3 --width 4", &store);

        let out = run("chart drawing configure line --config 3 --color teal --arrow end", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let follows = &parsed["drawings"][0];
        let own = &parsed["drawings"][1];
        assert_eq!(follows["style"]["color"], "teal");
        assert_eq!(follows["style"]["arrow"], "end");
        assert_eq!(own["style"]["color"], "blue", "a look of its own is not the configuration's");
        assert_eq!(own["style"]["width"], 4.0);

        let configs = run("chart drawing configs line --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&configs.out).unwrap();
        let third = &parsed["configurations"][2];
        assert_eq!(third["config"], 3);
        assert_eq!(third["default"], false);

        run("chart drawing reset-configs line", &store);
        let configs = run("chart drawing configs line --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&configs.out).unwrap();
        assert_eq!(parsed["configurations"][2]["default"], true);
        assert_eq!(parsed["configurations"][2]["style"]["color"], "blue");
        assert_eq!(parsed["configurations"].as_array().unwrap().len(), 9);
    }

    /// One bad value refuses the whole command, and names what was wrong.
    #[test]
    fn a_bad_anchor_or_colour_leaves_the_drawings_as_they_were() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        for bad in [
            "chart drawing add line --book Macro --from yesterday,180 --to 2026-09-19,192",
            "chart drawing add line --book Macro --from 2026-09-01,180 --to 2026-09-19,192 --color red",
            "chart drawing add line --book Macro --from 2026-09-01,180 --to 2026-09-19,192 --config 12",
            "chart drawing add line --book Macro --from 2026-09-01,180 --to 2026-09-19,192 --scope group-0",
            "chart drawing add line --book Macro --from 2026-09-01,180",
            "chart drawing add --book Macro --from 2026-09-01,180 --to 2026-09-19,192",
            "chart drawing set --book Macro --id 1",
            "chart drawing remove --book Macro",
            "chart drawing configure --config 1 --color teal",
        ] {
            let out = run(bad, &store);
            assert_eq!(out.code, super::super::EXIT_USAGE, "{bad}: {}", out.err);
        }
        let out = run("chart drawing remove --book Macro --id 9", &store);
        assert_eq!(out.code, super::super::EXIT_NOT_FOUND, "{}", out.err);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert!(parsed["drawings"].as_array().unwrap().is_empty());
    }

    /// A store holding what the examples name.
    fn seeded() -> Store {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        run("watchlist add Semis NVDA AMD AVGO TSM MU", &store);
        run("section create Default Energy", &store);
        run("section create Default Metals", &store);
        run("section create Semis Energy", &store);
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chartbook create Semis --watchlist Semis --symbol NVDA", &store);
        run("chart indicator add rsi --book Macro", &store);
        store
    }

    /// Every command in the agent skill is a command that runs.
    ///
    /// This is the condition the skill ships on. `agents/skills/omacharts`
    /// is prose, and prose about a generated surface rots — a skill that has
    /// drifted is worse than no skill, because it is confidently wrong and the
    /// agent reading it has no way to tell. The surface has
    /// `every_example_in_the_table_is_a_command_that_runs` holding it honest,
    /// so the skill gets the same treatment, out of the same file the plugin
    /// installs.
    ///
    /// Held to a higher bar than the table's own examples: each fenced block is
    /// run as a block, in order, against one store, and every line has to
    /// succeed outright rather than merely parse. A workflow is the thing the
    /// skill is for, and "these six commands in this order" is a claim that can
    /// be false while all six parse — `chart set --chart pos:1` after a split
    /// that never made a `pos:1` is exactly that.
    #[test]
    fn every_command_in_the_agent_skill_is_a_command_that_runs() {
        const SKILL: &str = include_str!("../../agents/skills/omacharts/SKILL.md");

        let blocks = omacharts_commands_in(SKILL);
        assert!(blocks.len() >= 4, "no commands found in the skill: is it still a markdown file?");

        for block in &blocks {
            // One store per block. Each is a recipe somebody starts from
            // wherever they are, not a continuation of the one above it.
            let store = skill_seed();
            for line in block {
                let args: Vec<String> = std::iter::once("omacharts".to_string())
                    .chain(line.split_whitespace().map(String::from))
                    .collect();
                let outcome = dispatch(&args, &store, Some(&NoWindow), &super::super::Here);
                assert_eq!(
                    outcome.code,
                    super::super::EXIT_OK,
                    "the skill says {line:?}, and that does not work: {}{}",
                    outcome.err,
                    outcome.out
                );
            }
        }
    }

    /// The `omacharts` lines of each fenced code block, block by block.
    ///
    /// Prose around them is left alone: the skill is written for somebody to
    /// read, and only what it puts in a fence is something it is telling an
    /// agent to type.
    fn omacharts_commands_in(markdown: &str) -> Vec<Vec<String>> {
        let mut blocks: Vec<Vec<String>> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut fenced = false;
        for line in markdown.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
                if !fenced && !current.is_empty() {
                    blocks.push(std::mem::take(&mut current));
                }
                continue;
            }
            if let Some(rest) = line.trim().strip_prefix("omacharts ").filter(|_| fenced) {
                current.push(rest.to_string());
            }
        }
        blocks
    }

    /// A store the skill's own workflows start from: one chartbook of two
    /// charts, and a watchlist no chartbook has claimed.
    ///
    /// Deliberately not `seeded()`. That one hands the Semis watchlist to a
    /// Semis chartbook, and a watchlist belongs to one chartbook — so the
    /// skill's "build a chartbook around this list" would be refused for a
    /// reason that is about the fixture rather than about the skill.
    fn skill_seed() -> Store {
        let store = Store::memory().unwrap();
        run("watchlist create Semis", &store);
        run("watchlist add Semis NVDA AMD AVGO TSM MU", &store);
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        store
    }
}
