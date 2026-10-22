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
    bollinger::MAX_DEVIATIONS, LineStyle, Stroke, MAX_FILL_ALPHA, MAX_PANE_SHARE, MIN_FILL_ALPHA,
    MIN_PANE_SHARE,
};
use omacharts_engine::providers;
use omacharts_engine::theme::{
    ColorChoice, SWATCH_NAMES, THEME_MONO_ID, THEME_RED_UP_ID,
};
use omacharts_engine::{
    BarStyle, Indicator, IndicatorKind, LinkMode, Reset, Session, Timeframe,
};

use super::charts::{self, Workspace};
use super::{parser, Caller, Fault, Live, Outcome, EXIT_USAGE};
use crate::chart_defaults::ChartDefaults;
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

    let parsed = match parser::command_for(args).try_get_matches_from(args) {
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
        ("watchlist", "sort") => watchlist_sort(store, m, json),
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

        ("template", _) => {
            let outcome = template_verb(store, verb, m, json);
            // A new name touches no chart, only the header of one showing it,
            // so the window is told that rather than rebuilt.
            if outcome.is_ok()
                && verb == "rename"
                && let Some(live) = live
            {
                live.adopt_header();
            }
            outcome
        }

        ("provider", "list") => provider_list(store, json),
        ("provider", "status") => provider_status(store, live, json),
        ("provider", "login") => provider_login(store, m, json),
        ("provider", "logout") => provider_logout(store, m, json),

        ("config", "list") => config_list(store, json),
        ("config", "get") => config_get(store, m, json),
        ("config", "set") => {
            let outcome = config_set(store, m, json);
            // The settings a window acts on the moment they change: the feed,
            // whose charts switch with nothing to restart, and the header,
            // which every chart on screen shows.
            if outcome.is_ok()
                && let Some(live) = live
            {
                match required(m, "KEY") {
                    Ok(key) if key == crate::feeds::SETTING => live.adopt_feed(),
                    Ok(key) if key == crate::header::SETTING => live.adopt_header(),
                    _ => {}
                }
            }
            outcome
        }
        ("config", "bars") => config_bars(store, m, json, live),

        ("plugin", "status") => plugin_status(&caller.home(), json),
        ("plugin", "install") => plugin_install(&caller.home(), json),
        ("plugin", "uninstall") => plugin_uninstall(&caller.home(), json),

        ("cache", "status") => cache_status(store, json),
        ("cache", "clear") => cache_clear(store, json),
        ("cache", "limit") => cache_limit(store, m, json),

        ("backup", "list") => backup_list(store, json),
        ("backup", "create") => backup_create(store, json),
        ("backup", "restore") => backup_restore(store, m, json, live),

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
        // A chartbook template is named on the line; only saving and applying
        // are about a chartbook.
        || (noun == "chartbook"
            && verb == "template"
            && matches!(arg(m, "ACTION").map(String::as_str), Some("list" | "rename" | "delete")))
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
pub(super) fn find_list(store: &Store, selector: &str) -> Result<(i64, String), Fault> {
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

/// What the rail does to a chart, as a command spells it.
fn link_mode(text: &str) -> Result<LinkMode, Fault> {
    LinkMode::from_key(text).ok_or_else(|| {
        Fault::usage(format!("{text:?} is not a link mode; use follow, focus or pinned"))
    })
}

fn watchlist_sort(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    use crate::ui::watchlist::{store_sort, stored_sort, Column, Sort};
    let (id, name) = find_list(store, required(m, "LIST")?)?;
    let Some(wanted) = arg(m, "COLUMN") else {
        let sort = stored_sort(store, id).map(Sort::key);
        let text = sort.clone().unwrap_or_else(|| "none".to_string());
        return said(as_json, json!({"id": id, "name": name, "sort": sort}), text);
    };
    // `none` is no column, which is the list's own order; the parser has
    // already refused anything that is neither.
    let descending = flag(m, "descending");
    let sort = Column::from_key(wanted).map(|column| Sort { column, descending });
    store_sort(store, id, sort);
    let text = match sort {
        None => format!("{name:?} is in its own order"),
        Some(sort) => format!(
            "{name:?} sorted by {}, {}",
            sort.column.key(),
            if sort.descending { "high to low" } else { "low to high" }
        ),
    };
    said(as_json, json!({"id": id, "name": name, "sort": sort.map(Sort::key)}), text)
}

/// Every watchlist.
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
                    "sort": crate::ui::watchlist::stored_sort(store, *id)
                        .map(crate::ui::watchlist::Sort::key),
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
            format!("{id:<4} {name:<24} {:>4} symbols  {tag}", counted(*id))
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
                                stored_mode(pane).key(),
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
            let template = arg(m, "template").map(|t| find_book_template(store, t)).transpose()?;
            let mut book = json!({
                "name": name,
                "layout": {"leaf": id},
                "focused": id,
                "panes": [fresh_pane(store, id, &symbol)],
                "watchlist": watchlist,
            });
            // Built from its one chart, as taking the template would be.
            if let Some(template) = &template {
                let charts = crate::templates::all(store);
                let mut next = id + 1;
                if !crate::book_templates::apply(&mut book, &template.shape, &charts, &mut next, None) {
                    return Err(Fault::new(
                        super::EXIT_ERROR,
                        format!("chartbook template {:?} is not an arrangement", template.name),
                    ));
                }
                crate::book_templates::set_link(&mut book, Some(template.id));
            }
            let at = workspace.push(book);
            if flag(m, "switch") {
                workspace.set_active(at);
            }
            match template {
                Some(template) => format!(
                    "created chartbook {name:?} (id:{at}) showing {symbol}, on chartbook template {:?}: {}",
                    template.name,
                    charts_counted(template.shape.panes.len())
                ),
                None => format!("created chartbook {name:?} (id:{at}) showing {symbol}"),
            }
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
        ("chartbook", "duplicate") => {
            let at = workspace.resolve(selector.map(String::as_str))?;
            let was = workspace.name_of(at);
            let name = match arg(m, "name") {
                Some(name) => name.trim().to_string(),
                None => format!("{was} copy"),
            };
            if name.is_empty() {
                return Err(Fault::usage("a chartbook needs a name".into()));
            }
            let mut copy = workspace.book(at)?.clone();
            let mut next = workspace.next_pane();
            let mut ids = std::collections::HashMap::new();
            for pane in charts::panes_mut(&mut copy)? {
                let Some(old) = pane["id"].as_u64() else { continue };
                ids.insert(old as u32, next);
                pane["id"] = json!(next);
                // Its own name among the drawings: see `charts::copy_pane`.
                pane["drawing_uid"] = json!(gtk::glib::uuid_string_random().to_string());
                next += 1;
            }
            let renamed = |id: &Value| {
                id.as_u64().and_then(|id| ids.get(&(id as u32))).map_or(Value::Null, |id| json!(id))
            };
            copy["layout"] = charts::relabel(&copy["layout"], &ids);
            copy["focused"] = renamed(&copy["focused"]);
            copy["maximized"] = renamed(&copy["maximized"]);
            copy["name"] = json!(name);
            // A watchlist is one chartbook's.
            if copy["watchlist"].as_i64().is_some_and(|list| list != DEFAULT_WATCHLIST) {
                copy["watchlist"] = json!(DEFAULT_WATCHLIST);
            }
            let placed = workspace.insert(at + 1, copy);
            if flag(m, "switch") {
                workspace.set_active(placed);
            }
            format!("duplicated chartbook {was:?} as {name:?} (id:{placed})")
        }
        ("chartbook", "template") => {
            return book_template_verb(store, m, as_json, &mut workspace, selector.map(String::as_str));
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
                .unwrap_or_else(|| fresh_pane(store, added, "SPY"));
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
            Fault::usage(format!("{text:?} is not a resolution; try 5m, 1h, 1D, 1W, 1M or 1Y"))
        })?),
    };
    let style = match arg(m, "style") {
        None => None,
        Some(text) => Some(
            BarStyle::from_key(text)
                .ok_or_else(|| Fault::usage(format!("{text:?} is not a bar style")))?,
        ),
    };
    let colouring = match arg(m, "colouring") {
        None => None,
        Some(text) => Some(
            omacharts_engine::BarColouring::from_key(text)
                .ok_or_else(|| Fault::usage(format!("{text:?} is not a bar colouring")))?,
        ),
    };
    let session = match arg(m, "session") {
        None => None,
        Some(text) => Some(
            Session::from_key(text)
                .ok_or_else(|| Fault::usage(format!("{text:?} is not a session")))?,
        ),
    };
    let mode = match arg(m, "link") {
        None => None,
        Some(text) => Some(link_mode(text)?),
    };
    let switch = |id: &str| -> Result<Option<bool>, Fault> {
        match arg(m, id).map(String::as_str) {
            None => Ok(None),
            Some("on") => Ok(Some(true)),
            Some("off") => Ok(Some(false)),
            Some(other) => Err(Fault::usage(format!("--{id} is on or off, not {other:?}"))),
        }
    };
    let shows_drawings = switch("show-drawings")?;
    let sends_drawings = switch("send-drawings")?;
    let template = match arg(m, "template") {
        None => None,
        Some(text) if text.eq_ignore_ascii_case("none") => Some(None),
        Some(text) => Some(Some(find_template(store, text)?)),
    };

    // Which mode it was in, read before anything is written: only becoming
    // Follow leads the Follow charts. Re-stating the mode a chart is already
    // in is not somebody asking for four charts to move.
    let was = mode_of(workspace.book(at)?, pane);

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
    if let Some(colouring) = colouring {
        chart["colouring"] = json!(colouring.key());
        changed.push(format!("colouring {}", colouring.key()));
    }
    if let Some(session) = session {
        chart["session"] = json!(session.key());
        changed.push(format!("session {}", session.key()));
    }
    if let Some(mode) = mode {
        chart["link"] = json!(mode.key());
        changed.push(format!("link {}", mode.key()));
    }
    if let Some(grid) = arg(m, "grid") {
        chart["show_grid"] = json!(grid == "on");
        changed.push(format!("grid {grid}"));
    }
    if let Some(shows) = shows_drawings {
        chart["shows_drawings"] = json!(shows);
        changed.push(format!("shows other charts' drawings {}", on_off(shows)));
    }
    if let Some(sends) = sends_drawings {
        let was = chart["sends_drawings"].as_bool().unwrap_or(true);
        chart["sends_drawings"] = json!(sends);
        if was != sends {
            move_this_chart_s_drawings(store, chart, sends);
        }
        changed.push(format!("sends drawings to other charts {}", on_off(sends)));
    }
    if let Some(auto) = arg(m, "auto-scale") {
        chart["auto_scale"] = json!(auto == "on");
        changed.push(format!("auto-scale {auto}"));
    }
    if let Some(log) = arg(m, "log-scale") {
        chart["log_scale"] = json!(log == "on");
        changed.push(format!("log scale {log}"));
    }
    match template {
        None => {}
        // The template's indicators replace the chart's own, which go: the
        // chart shows what the template holds from now on.
        Some(Some(template)) => {
            chart["template"] = json!(template.id);
            chart["indicators"] = to_value(&template.indicators)?;
            changed.push(format!("chart template {:?}", template.name));
        }
        // None is the empty set: the chart shows no indicators.
        Some(None) => {
            chart["template"] = Value::Null;
            chart["indicators"] = json!([]);
            changed.push("no chart template, and no indicators".to_string());
        }
    }
    // What the chart ends up showing, which is what it leads its new group
    // with. Read after the writes above, so `--symbol X --link N` leads with X
    // rather than with whatever the chart had before.
    let leading = chart["symbol"].as_str().filter(|s| !s.is_empty()).map(String::from);
    let leading_suffix = chart["suffix"].as_str().map(String::from);

    if changed.is_empty() {
        return Err(Fault::usage(
            "nothing to change; pass --symbol, --resolution, --style, --session, --link, --grid, \
             --auto-scale, --log-scale, --show-drawings, --send-drawings or --template"
                .into(),
        ));
    }

    // A chart made Follow leads the Follow charts. Three things write
    // nothing: leaving Follow — the charts still following keep what they had;
    // re-stating the mode the chart is already in; and leading from a chart
    // with no symbol, which would blank the charts it just joined.
    let joined = mode.filter(|mode| *mode == LinkMode::Follow && *mode != was);
    let followers = match (joined, &leading) {
        (Some(_), Some(symbol)) => {
            workspace.lead_follow_charts(symbol, leading_suffix.as_deref(), at, pane)
        }
        _ => 0,
    };

    workspace.save(store);
    let mut text = format!("chart {pane}: {}", changed.join(", "));
    if followers > 0 {
        let named = spell(leading.as_deref().unwrap_or("?"), leading_suffix.as_deref());
        text.push_str(&match followers {
            1 => format!("; 1 other Follow chart now shows {named}"),
            n => format!("; {n} other Follow charts now show {named}"),
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
        return Ok(list_indicators(&listed, json!({"chart": pane}), as_json));
    }

    // Everything is read and checked before anything is written, so a command
    // carrying one bad value leaves the chart exactly as it was.
    let kind = indicator_kind(m, action)?;
    let edits = Edits::read(m)?;

    let target = workspace.book_mut(at)?;
    let Some(chart) = charts::pane_mut(target, pane) else {
        return Err(Fault::not_found(format!("no chart {pane}")));
    };
    // A chart on a template shows the template's indicators, and changing one
    // of them here would change every chart on it. So the chart lets go of
    // the template first, keeping what it showed, and says it did — the same
    // as adding one from the window does. The template itself is changed
    // with `template indicator`.
    let left = match chart["template"].as_u64() {
        Some(id) => {
            chart["template"] = Value::Null;
            Some(
                crate::templates::get(store, id as u32)
                    .map(|t| t.name)
                    .unwrap_or_else(|| format!("id:{id}")),
            )
        }
        None => None,
    };
    let indicators = chart["indicators"].as_array_mut().ok_or_else(|| {
        Fault::new(super::EXIT_ERROR, "this chart's indicators are not a list".into())
    })?;
    let text = edit_indicators(m, action, kind, &edits, indicators, "this chart")?;

    workspace.save(store);

    // What it actually hit, not just that it worked. A command that named no
    // chart has to say which one it found, or nobody can catch it reaching
    // the wrong one.
    let mut where_at = format!(
        "{text} on {} in {:?}",
        chart_label(workspace.book(at)?, pane),
        workspace.name_of(at)
    );
    if let Some(name) = left {
        where_at.push_str(&format!(
            "; it no longer uses chart template {name:?}, and keeps the indicators it had"
        ));
    }
    said(as_json, json!({"chart": pane, "message": where_at}), where_at)
}

/// A chart as a new one opens: the defaults, and the template they name.
fn fresh_pane(store: &Store, id: u32, symbol: &str) -> Value {
    let defaults = ChartDefaults::load(store);
    let mut pane = charts::new_pane(id, symbol, None, &defaults);
    if let Some(template) = defaults.template.and_then(|id| crate::templates::get(store, id)) {
        pane["template"] = json!(template.id);
        pane["indicators"] = serde_json::to_value(&template.indicators).unwrap_or_else(|_| json!([]));
    }
    pane
}

/// The template a selector names, or why there is none.
fn find_template(store: &Store, selector: &str) -> Result<crate::templates::Template, Fault> {
    use crate::templates::Lookup;
    let all = crate::templates::all(store);
    match crate::templates::find(&all, selector) {
        Lookup::Found(found) => Ok(found.clone()),
        Lookup::Missing => Err(Fault::not_found(format!(
            "no chart template called {selector:?}; see `omacharts template list`"
        ))),
        Lookup::Ambiguous(ids) => Err(Fault::ambiguous(format!(
            "{} chart templates are called {selector:?}; use {}",
            ids.len(),
            ids.iter().map(|id| format!("id:{id}")).collect::<Vec<_>>().join(" or ")
        ))),
    }
}

/// Indicators as `indicator list` prints them, for a chart or a template.
fn list_indicators(listed: &[Value], whose: Value, as_json: bool) -> String {
    if as_json {
        let rows: Vec<Value> = listed
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
            })
            .collect();
        let mut out = whose;
        out["indicators"] = Value::Array(rows);
        return format!("{out}\n");
    }
    listed.iter().map(describe_indicator).collect::<Vec<_>>().join("\n") + "\n"
}

/// The indicator kind an `indicator` verb names.
fn indicator_kind(m: &clap::ArgMatches, action: &str) -> Result<IndicatorKind, Fault> {
    let wanted = arg(m, "KIND")
        .ok_or_else(|| Fault::usage(format!("`indicator {action}` needs an indicator to {action}")))?
        .to_lowercase();
    IndicatorKind::ALL.into_iter().find(|k| k.key() == wanted).ok_or_else(|| {
        Fault::usage(format!(
            "{wanted:?} is not an indicator; try {}",
            IndicatorKind::ALL.map(|k| k.key()).join(", ")
        ))
    })
}

/// Add, remove or reconfigure one indicator in a list of them.
///
/// One list, whoever it belongs to: a chart's and a template's are the same
/// shape and take the same flags, so they are changed by the same code and
/// cannot come to read a flag differently. `whose` is how a refusal names
/// the list — "this chart has no RSI".
fn edit_indicators(
    m: &clap::ArgMatches,
    action: &str,
    kind: IndicatorKind,
    edits: &Edits,
    indicators: &mut Vec<Value>,
    whose: &str,
) -> Result<String, Fault> {
    Ok(match action {
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
                return Err(Fault::not_found(format!("{whose} has no {}", kind.short_name())));
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
                    return Err(Fault::not_found(format!("{whose} has no {}", kind.short_name())))
                }
                many => {
                    return Err(Fault::ambiguous(format!(
                        "{whose} has {} of them; say which with --id {}",
                        many.len(),
                        many.iter()
                            .filter_map(|at| indicators.get(*at).and_then(|i| i["id"].as_u64()))
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
            let Some(chosen) = indicators.get_mut(which) else {
                return Err(Fault::not_found(format!("{whose} has no {}", kind.short_name())));
            };
            edits.apply(chosen, kind)?;
            format!("set {}", describe_indicator(chosen))
        }
        other => return Err(Fault::usage(format!("{other:?} is not list, add, remove or set"))),
    })
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
    use omacharts_engine::drawings::{Drawing, Kind, Scope, CONFIGURATIONS};

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
    let shows = chart["shows_drawings"].as_bool().unwrap_or(true);
    let sends = chart["sends_drawings"].as_bool().unwrap_or(true);
    let configs = store.drawing_configurations();

    // What this chart shows: the symbol's, when it is shown what other
    // charts send, and its own either way.
    let locals: Vec<Drawing> = chart["drawings"]
        .as_array()
        .map(|list| list.iter().filter_map(|d| serde_json::from_value(d.clone()).ok()).collect())
        .unwrap_or_default();
    let shown = |store: &Store, locals: &[Drawing]| -> Vec<Drawing> {
        let mut all: Vec<Drawing> = match shows {
            true => store.drawings(&symbol, suffix.as_deref()),
            false => Vec::new(),
        };
        all.extend(locals.iter().cloned());
        omacharts_engine::drawings::sort_for_painting(&mut all);
        all
    };

    if action == "list" {
        let listed = shown(store, &locals);
        return match as_json {
            true => Ok(format!(
                "{{\"chart\":{pane},\"symbol\":{},\"shows_drawings\":{shows},\"sends_drawings\":{sends},\"drawings\":[{}]}}\n",
                json!(spell(&symbol, suffix.as_deref())),
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
    let corners = anchors(m, "point")?;
    if corners.len() == 1 {
        return Err(Fault::usage(
            "a zig-zag needs at least two --point corners; one corner is not a drawing".into(),
        ));
    }
    // What a kind that has no corners of its own should hear, rather than
    // having them quietly dropped: four points on a rectangle is somebody
    // expecting a polygon.
    let not_a_path = || {
        Fault::usage("--point is a zig-zag's corners; every other kind is --from and --to".into())
    };
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
    let words = TextEdits::read(m)?;
    let scope = match arg(m, "scope") {
        None => None,
        Some(text) => Some(Scope::from_key(text).ok_or_else(|| {
            Fault::usage(format!("{text:?} is not a scope; try shared or local"))
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
            let kind = kind.ok_or_else(|| {
                Fault::usage("`drawing add` needs a kind: line, rect, ellipse or text".into())
            })?;
            if !corners.is_empty() && !kind.is_path() {
                return Err(not_a_path());
            }
            // Words sit at a point, so a text drawing takes one anchor and
            // uses it for both. A zig-zag given its corners has its ends
            // among them and needs no --from and --to of its own. Everything
            // else spans two.
            let (from, to) = match (kind.is_text(), corners.first(), corners.last()) {
                (_, Some(first), Some(last)) => (*first, *last),
                (true, ..) => {
                    let Some(at) = from.or(to) else {
                        return Err(Fault::usage(
                            "`drawing add text` needs --from WHEN,PRICE: where the words go".into(),
                        ));
                    };
                    (at, at)
                }
                (false, ..) => match (from, to) {
                    (Some(from), Some(to)) => (from, to),
                    _ if kind.is_path() => {
                        return Err(Fault::usage(
                            "`drawing add zigzag` needs --point WHEN,PRICE for each corner, or --from and --to for a single leg".into(),
                        ))
                    }
                    _ => {
                        return Err(Fault::usage(
                            "`drawing add` needs --from and --to, each WHEN,PRICE".into(),
                        ))
                    }
                },
            };
            if kind.is_text() && words.words.as_deref().unwrap_or_default().is_empty() {
                return Err(Fault::usage(
                    "`drawing add text` needs --text: a text drawing with nothing to say draws nothing".into(),
                ));
            }
            let mut drawing = Drawing::new(kind, from, to);
            if !corners.is_empty() {
                drawing.set_corners(corners.clone());
            }
            drawing.origin = chart["drawing_uid"].as_str().map(str::to_string).filter(|u| !u.is_empty());
            drawing.follow(config.unwrap_or(1));
            if !style.is_empty() {
                drawing.edit_style(&configs, |s| style.apply(s));
            }
            if !words.is_empty() {
                words.apply(&mut drawing);
            }
            drawing.scope = scope.unwrap_or(match sends {
                true => Scope::Shared,
                false => Scope::Local,
            });
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
            if from.is_none()
                && to.is_none()
                && corners.is_empty()
                && config.is_none()
                && style.is_empty()
                && words.is_empty()
                && scope.is_none()
                && order.is_none()
            {
                return Err(Fault::usage(
                    "nothing to set: give --from, --to, --point, --config, --scope, --order, --text, or a property".into(),
                ));
            }
            let mut drawing = find_drawing(store, &symbol, suffix.as_deref(), &locals, id)?;
            let was_local = drawing.is_local();
            // Through `move_grip` rather than straight onto the field, so a
            // text drawing's two anchors stay the one point they are.
            if let Some(from) = from {
                drawing.move_grip(omacharts_engine::Grip::From, from);
            }
            if let Some(to) = to {
                drawing.move_grip(omacharts_engine::Grip::To, to);
            }
            // Last, so that the path's own corners settle the ends rather
            // than the other way about.
            if !corners.is_empty() && !drawing.set_corners(corners.clone()) {
                return Err(not_a_path());
            }
            if let Some(n) = config {
                drawing.follow(n);
            }
            if !style.is_empty() {
                drawing.edit_style(&configs, |s| style.apply(s));
            }
            if !words.is_empty() {
                words.apply(&mut drawing);
            }
            if drawing.kind.is_text() && !drawing.has_text() {
                return Err(Fault::usage(
                    "a text drawing with nothing to say draws nothing; `drawing remove` is the way to take it off".into(),
                ));
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
/// Take back what this chart sent, or send what it has been keeping.
///
/// The window does the same thing when the switch is flicked there. Only
/// this chart's own move: each drawing remembers which chart made it, so a
/// switch here never withdraws another chart's work, and a drawing made
/// before charts had names belongs to none of them and stays put.
fn move_this_chart_s_drawings(store: &Store, chart: &mut Value, sends: bool) {
    use omacharts_engine::{Drawing, Scope};
    let symbol = chart["symbol"].as_str().unwrap_or("").to_string();
    if symbol.is_empty() {
        return;
    }
    let suffix = chart["suffix"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let suffix = suffix.as_deref();
    let mine = chart["drawing_uid"].as_str().unwrap_or("").to_string();
    let mut locals: Vec<Drawing> = chart["drawings"]
        .as_array()
        .map(|list| list.iter().filter_map(|d| serde_json::from_value(d.clone()).ok()).collect())
        .unwrap_or_default();

    match sends {
        true => {
            for mut drawing in locals.drain(..) {
                drawing.id = 0;
                drawing.scope = Scope::Shared;
                if !mine.is_empty() {
                    drawing.origin = Some(mine.clone());
                }
                store.add_drawing(&symbol, suffix, &drawing);
            }
        }
        false => {
            if mine.is_empty() {
                return;
            }
            let sent: Vec<Drawing> = store
                .drawings(&symbol, suffix)
                .into_iter()
                .filter(|d| d.origin.as_deref() == Some(mine.as_str()))
                .collect();
            for mut drawing in sent {
                store.remove_drawing(drawing.id);
                drawing.id = next_local_id(&locals);
                drawing.scope = Scope::Local;
                locals.push(drawing);
            }
        }
    }
    chart["drawings"] = json!(locals);
}

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
        // Only a zig-zag has corners of its own; for every other kind they
        // would be `from` and `to` said a second time.
        "corners": d.kind.is_path().then(|| d.corners().iter().map(|a| json!({
            "ts": a.ts, "when": spell_moment(a.ts), "price": a.price,
        })).collect::<Vec<_>>()),
        "config": d.config,
        // Where a look of its own came from, for a drawing that has one.
        "started_from": d.started_from,
        "style": style_json(d.style(configs)),
        "scope": d.scope.key(),
        "order": d.order,
        "text": d.has_text().then(|| json!({
            "words": d.text.plain_text(),
            // The same words as `--text` would take to make them, emphasis
            // and all, so a label can be read off one drawing and put on
            // another without going through the runs by hand.
            "markup": d.text.markup(),
            "at": d.text.at.key(),
            // The runs, so a reader can see where the bold is rather than
            // only that the characters are there. A plain label is one run.
            "spans": d.text.spans.iter().map(|span| json!({
                "text": span.text,
                "bold": span.bold,
                "italic": span.italic,
            })).collect::<Vec<_>>(),
        })),
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
        "text_color": style.text.colour.spell(),
        // Null for the desktop's own font, which is a role rather than a
        // family and resolves differently on another machine.
        "font": style.text.family_name(),
        "font_size": style.text.clamped_size(),
    })
}

/// A drawing, said back in one line:
/// `#3 line  2026-09-01 180.50 → 2026-09-19 192.00  configuration 4 · global`.
fn describe_drawing(drawing: &omacharts_engine::Drawing, configs: &omacharts_engine::Configurations) -> String {
    let look = match drawing.config {
        Some(n) => format!("configuration {n}"),
        None => describe_style(drawing.kind, drawing.style(configs)),
    };
    // What it says, in quotes, since a label is the thing somebody is
    // looking for in a list of six boxes. Trimmed to a line: the whole of a
    // paragraph belongs in `--json`, not in a line of a listing.
    let words = match drawing.has_text() {
        false => String::new(),
        true => format!("  {:?}", shorten(&drawing.text.markup().replace('\n', " "), 40)),
    };
    // A word is at one point, so saying the same moment twice would be noise.
    let span = match drawing.kind.is_text() {
        true => format!("{} {:.2}", spell_moment(drawing.from.ts), drawing.from.price),
        false => format!(
            "{} {:.2} → {} {:.2}",
            spell_moment(drawing.from.ts),
            drawing.from.price,
            spell_moment(drawing.to.ts),
            drawing.to.price
        ),
    };
    format!(
        "#{} {}  {span}{words}  {look} · {}",
        drawing.id,
        drawing.kind.key(),
        drawing.scope.key()
    )
}

/// A string cut to `most` characters, with an ellipsis standing for the rest.
fn shorten(text: &str, most: usize) -> String {
    match text.chars().count() > most {
        false => text.to_string(),
        true => text.chars().take(most.saturating_sub(1)).collect::<String>() + "…",
    }
}

/// A look in words: `amber, 2.5px, arrow at the end` for a line; `fill amber
/// at 0.16, edge amber 1px` for a rectangle.
fn describe_style(kind: omacharts_engine::DrawingKind, style: &omacharts_engine::Style) -> String {
    use omacharts_engine::drawings::{Arrow, ArrowHead, Kind};
    match kind {
        Kind::Line | Kind::Horizontal | Kind::Arrow | Kind::Zigzag => {
            let arrow = match (style.arrow, style.head) {
                (Arrow::None, _) => String::new(),
                (arrow, ArrowHead::Filled) => format!(", arrow {}", arrow.label().to_lowercase()),
                (arrow, head) => {
                    format!(", {} arrow {}", head.label().to_lowercase(), arrow.label().to_lowercase())
                }
            };
            format!("{}, {}px{arrow}", style.colour.spell(), style.width)
        }
        Kind::Rect | Kind::Ellipse => {
            let edge = match style.border {
                true => format!(", edge {} {}px", style.colour.spell(), style.width),
                false => ", no edge".to_string(),
            };
            format!("fill {} at {:.2}{edge}", style.fill.spell(), style.alpha)
        }
        Kind::Text => describe_text_style(style),
    }
}

/// How a configuration sets words: the ink, the size, and the face when it is
/// not the desktop's.
fn describe_text_style(style: &omacharts_engine::Style) -> String {
    let face = match style.text.family_name() {
        Some(family) => format!(", {family}"),
        None => String::new(),
    };
    format!("{} {:.0}px{face}", style.text.colour.spell(), style.text.clamped_size())
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
    text_colour: Option<omacharts_engine::Paint>,
    /// The face: `Some(Some(family))` for one by name, `Some(None)` for the
    /// desktop's own, which `--font system` is the way to ask for.
    font: Option<Option<String>>,
    font_size: Option<f64>,
}

impl StyleEdits {
    fn read(m: &clap::ArgMatches) -> Result<StyleEdits, Fault> {
        use omacharts_engine::drawings::{Arrow, ArrowHead, Paint, MAX_WIDTH, MIN_WIDTH};
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
            Some(w) if (MIN_WIDTH..=MAX_WIDTH).contains(&w) => Some(w),
            Some(w) => {
                return Err(Fault::usage(format!(
                    "--width is {w}, which is outside {MIN_WIDTH} to {MAX_WIDTH}"
                )))
            }
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
        let font = arg(m, "font").map(|family| match family.eq_ignore_ascii_case("system") {
            true => None,
            false => Some(family.to_string()),
        });
        Ok(StyleEdits {
            colour: paint("color")?,
            width,
            arrow,
            head,
            border,
            fill: paint("fill")?,
            alpha: bounded(m, "alpha", MIN_FILL_ALPHA, MAX_FILL_ALPHA)?,
            text_colour: paint("text-color")?,
            font,
            font_size: bounded(
                m,
                "font-size",
                omacharts_engine::drawings::MIN_TEXT_SIZE,
                omacharts_engine::drawings::MAX_TEXT_SIZE,
            )?,
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
            && self.text_colour.is_none()
            && self.font.is_none()
            && self.font_size.is_none()
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
        if let Some(colour) = &self.text_colour {
            style.text.colour = colour.clone();
        }
        if let Some(font) = &self.font {
            style.text.family = font.clone();
        }
        if let Some(size) = self.font_size {
            style.text.size = size;
        }
    }
}

/// What a command said about the words themselves, as opposed to how they are
/// set.
///
/// Apart from [`StyleEdits`] because they are not a style: a configuration
/// says how words are set and never which words, so `--text` edits the
/// drawing and must not take it off the configuration it follows the way
/// `--text-color` does.
struct TextEdits {
    words: Option<String>,
    at: Option<omacharts_engine::Place>,
}

impl TextEdits {
    fn read(m: &clap::ArgMatches) -> Result<TextEdits, Fault> {
        let at = match arg(m, "text-at") {
            None => None,
            Some(text) => Some(omacharts_engine::Place::from_key(text).ok_or_else(|| {
                Fault::usage(format!(
                    "{text:?} is not a place; try center, top, bottom, left, right, or a corner like top-left"
                ))
            })?),
        };
        Ok(TextEdits { words: arg(m, "text").cloned(), at })
    }

    fn is_empty(&self) -> bool {
        self.words.is_none() && self.at.is_none()
    }

    /// Put them on the drawing. The runs are read back out of the one string
    /// a terminal can hand over: `**bold**`, `*italic*`, `***both***`, which
    /// is how emphasis survives a flag.
    fn apply(&self, drawing: &mut omacharts_engine::Drawing) {
        let mut text = drawing.text.clone();
        if let Some(words) = &self.words {
            text.spans = omacharts_engine::drawings::Text::from_markup(words).spans;
        }
        if let Some(at) = self.at {
            text.at = at;
        }
        drawing.set_text(text);
    }
}

/// A switch, as a command line says it back.
fn on_off(on: bool) -> &'static str {
    match on {
        true => "on",
        false => "off",
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
    match arg(m, id) {
        None => Ok(None),
        Some(text) => read_anchor(text, id).map(Some),
    }
}

/// The same, for a flag that may be given more than once: a zig-zag's
/// corners, in the order they were typed.
fn anchors(m: &clap::ArgMatches, id: &str) -> Result<Vec<omacharts_engine::Anchor>, Fault> {
    m.get_many::<String>(id)
        .into_iter()
        .flatten()
        .map(|text| read_anchor(text, id))
        .collect()
}

fn read_anchor(text: &str, id: &str) -> Result<omacharts_engine::Anchor, Fault> {
    use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
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
    Ok(omacharts_engine::Anchor::new(ts, price))
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

/// Which mode a chart is in.
///
/// Wanted before a change rather than after it, because becoming Follow leads
/// the Follow charts and re-stating the mode a chart is already in must not.
fn mode_of(book: &Value, pane: u32) -> LinkMode {
    charts::panes(book)
        .iter()
        .find(|p| p["id"].as_u64() == Some(u64::from(pane)))
        .map(stored_mode)
        .unwrap_or_default()
}

/// A stored chart's mode, read the way the window reads it: anything it
/// does not know is Focus. A chart still written with a link group has been
/// given its mode by [`charts::Workspace::load`] before this sees it.
fn stored_mode(pane: &Value) -> LinkMode {
    serde_json::from_value::<LinkMode>(pane["link"].clone()).unwrap_or_default()
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
    let mut lengths: Vec<String> = ["period", "fast", "slow", "signal", "k_smooth", "d_period"]
        .iter()
        .filter_map(|key| stored["params"][key].as_u64().map(|n| n.to_string()))
        .collect();
    // A Bollinger's multiple says as much about it as its period does, so it
    // goes in the same brackets. Only a Bollinger's: a VWAP's three bands sit
    // at three different multiples and none of them is the indicator's.
    if kind == IndicatorKind::Bollinger.key()
        && let Some(deviations) = stored["params"]["bands"]
            .as_array()
            .and_then(|bands| bands.first())
            .and_then(|band| band["deviations"].as_f64())
    {
        lengths.push(deviations.to_string());
    }
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
    deviations: Option<f64>,
    fast: Option<u64>,
    slow: Option<u64>,
    signal: Option<u64>,
    signal_color: Option<ColorChoice>,
    k_smooth: Option<u64>,
    d_period: Option<u64>,
    d_color: Option<ColorChoice>,
    anchor: Option<Reset>,
    rows: Option<Option<u64>>,
    value_area: Option<f64>,
    poc_color: Option<ColorChoice>,
    up_color: Option<ColorChoice>,
    down_color: Option<ColorChoice>,
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
            period: count(m, "period")?,
            // Nought is a band on the line, which is a real if useless answer,
            // so the floor is nought and not the engine's own. The ceiling is
            // the engine's, as the height's and the shading's are.
            //
            // Adding nought turns a negative nought into a positive one.
            // `-0.0` is inside the range and is the same band as `0.0`, but it
            // is written down as it was given and comes back out in the
            // indicator's own name, so a chart ends up labelled `BB(20,-0)`.
            deviations: bounded(m, "deviations", 0.0, MAX_DEVIATIONS)?.map(|wanted| wanted + 0.0),
            fast: count(m, "fast")?,
            slow: count(m, "slow")?,
            signal: count(m, "signal")?,
            signal_color: colour(m, "signal-color")?,
            k_smooth: count(m, "k-smooth")?,
            d_period: count(m, "d-period")?,
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
            up_color: colour(m, "up-color")?,
            down_color: colour(m, "down-color")?,
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
            && self.deviations.is_none()
            && self.fast.is_none()
            && self.slow.is_none()
            && self.signal.is_none()
            && self.signal_color.is_none()
            && self.k_smooth.is_none()
            && self.d_period.is_none()
            && self.d_color.is_none()
            && self.anchor.is_none()
            && self.rows.is_none()
            && self.value_area.is_none()
            && self.poc_color.is_none()
            && self.up_color.is_none()
            && self.down_color.is_none()
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
                true => params["period"] = json!(period),
                false => return Err(refuse("period")),
            }
        }
        // The quick average has to stay quicker than the slow one: equal
        // lengths measure nothing, and the wrong way round is the line upside
        // down. Whichever of the two the command left out is the one already
        // on the chart. Checked before either is written, so a refusal leaves
        // the chart as it was.
        if self.fast.is_some() || self.slow.is_some() {
            let quick = self.fast.or_else(|| params["fast"].as_u64());
            let slower = self.slow.or_else(|| params["slow"].as_u64());
            if let (Some(quick), Some(slower)) = (quick, slower)
                && quick >= slower
            {
                return Err(Fault::usage(format!(
                    "--fast {quick} is not below --slow {slower}"
                )));
            }
        }
        if let Some(bars) = self.fast {
            match params.get("fast").is_some() {
                true => params["fast"] = json!(bars),
                false => return Err(refuse("fast average")),
            }
        }
        if let Some(bars) = self.slow {
            match params.get("slow").is_some() {
                true => params["slow"] = json!(bars),
                false => return Err(refuse("slow average")),
            }
        }
        if let Some(bars) = self.signal {
            match params.get("signal").is_some() {
                true => params["signal"] = json!(bars),
                false => return Err(refuse("signal line")),
            }
        }
        if let Some(colour) = &self.signal_color {
            match kind == IndicatorKind::Macd {
                true => params["signal_color"] = to_value(colour)?,
                false => return Err(refuse("signal line")),
            }
        }
        if let Some(bars) = self.k_smooth {
            match params.get("k_smooth").is_some() {
                true => params["k_smooth"] = json!(bars),
                false => return Err(refuse("%K smoothing")),
            }
        }
        if let Some(bars) = self.d_period {
            match params.get("d_period").is_some() {
                true => params["d_period"] = json!(bars),
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
        for (colour, key, what) in [
            (&self.up_color, "up_color", "rising volume colour"),
            (&self.down_color, "down_color", "falling volume colour"),
        ] {
            if let Some(colour) = colour {
                match kind == IndicatorKind::Volume {
                    true => params[key] = to_value(colour)?,
                    false => return Err(refuse(what)),
                }
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
        // A VWAP and a Bollinger both carry bands, in the same shape, so both
        // answer to the same flags. Anything else has none.
        if self.bands.is_some() || self.band_alpha.is_some() || self.deviations.is_some() {
            let what = match self.deviations.is_some() {
                true => "deviations",
                false => "bands",
            };
            let Some(bands) = params["bands"].as_array_mut() else {
                return Err(refuse(what));
            };
            // Naming a band this indicator has not got would otherwise read as
            // having switched something on: a Bollinger has one, and `--bands
            // 2` on it would quietly switch its only band off.
            if let Some(wanted) = &self.bands
                && let Some(past) = wanted.iter().find(|at| **at >= bands.len())
            {
                return Err(Fault::usage(format!(
                    "{} has {} band{}, so {} is not one of them",
                    kind.short_name(),
                    bands.len(),
                    if bands.len() == 1 { "" } else { "s" },
                    past + 1
                )));
            }
            // One multiple for one band. A VWAP's three sit at different ones
            // on purpose, and a single figure cannot say which it means.
            if self.deviations.is_some() && kind != IndicatorKind::Bollinger {
                return Err(Fault::usage(format!(
                    "--deviations sets the one band a Bollinger has; {} has {} of them",
                    kind.short_name(),
                    bands.len()
                )));
            }
            for (at, band) in bands.iter_mut().enumerate() {
                if let Some(wanted) = &self.bands {
                    band["enabled"] = json!(wanted.contains(&at));
                }
                if let Some(alpha) = self.band_alpha {
                    band["fill_alpha"] = json!(alpha);
                }
                if let Some(deviations) = self.deviations {
                    band["deviations"] = json!(deviations);
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

/// A length in bars: a whole number of them, and at least one.
///
/// Not `number(...) as u64`, which is how `--period -5` used to come out as a
/// period of one — a float cast to an unsigned integer saturates at nought,
/// and the floor that followed turned it into a figure nobody asked for. A
/// caller that cannot see the screen has to be told instead.
fn count(m: &clap::ArgMatches, id: &str) -> Result<Option<u64>, Fault> {
    let Some(text) = arg(m, id) else { return Ok(None) };
    match text.parse::<u64>() {
        Ok(bars) if bars >= 1 => Ok(Some(bars)),
        _ => Err(Fault::usage(format!(
            "--{id} takes a whole number of bars, one or more, not {text:?}"
        ))),
    }
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
        "colouring": pane["colouring"],
        "session": pane["session"],
        "grid": pane["show_grid"],
        // Missing from a chart stored before it was remembered, and every one
        // of those was fitting itself.
        "auto_scale": pane["auto_scale"].as_bool().unwrap_or(true),
        // Likewise linear, before an axis could be anything else.
        "log_scale": pane["log_scale"].as_bool().unwrap_or(false),
        "link": stored_mode(pane).key(),
        "shows_drawings": pane["shows_drawings"].as_bool().unwrap_or(true),
        "sends_drawings": pane["sends_drawings"].as_bool().unwrap_or(true),
        "indicators": pane["indicators"]
            .as_array()
            .map(|all| all.iter().filter_map(|i| i["kind"].as_str()).collect::<Vec<_>>())
            .unwrap_or_default(),
        // The chart template by id, as `template list` names it; none before
        // there were templates.
        "template": pane["template"],
    })
    .to_string()
}

// -- chart templates ------------------------------------------------------

/// Named sets of indicators that charts share. See [`crate::templates`].
///
/// A template is a setting and the charts using it are in the arrangement, so
/// a verb that changes one writes both: the charts on a template keep a copy
/// of what it holds, which is what every other command reading a chart's
/// indicators reads, and what a chart keeps when it lets go.
fn template_verb(
    store: &Store,
    verb: &str,
    m: &clap::ArgMatches,
    as_json: bool,
) -> Result<String, Fault> {
    let text = match verb {
        "list" => return template_list(store, &Workspace::load(store), as_json),
        "indicator" => return template_indicator(store, m, as_json, &mut Workspace::load(store)),
        "default" => return template_default(store, m, as_json),
        "create" => {
            let name = template_name(m, "NAME")?;
            let made = crate::templates::create(store, &name);
            format!("created chart template {name:?} (id:{})", made.id)
        }
        "rename" => {
            let template = find_template(store, required(m, "TEMPLATE")?)?;
            let name = template_name(m, "NAME")?;
            crate::templates::rename(store, template.id, &name);
            format!("renamed chart template {:?} to {name:?}", template.name)
        }
        "delete" => {
            let template = find_template(store, required(m, "TEMPLATE")?)?;
            crate::templates::delete(store, template.id);
            // Nothing disappears from a chart: each one keeps what the
            // template was showing it, as its own.
            let mut workspace = Workspace::load(store);
            let kept = follow_template(&mut workspace, template.id, None);
            workspace.save(store);
            match kept {
                0 => format!("deleted chart template {:?}", template.name),
                1 => format!(
                    "deleted chart template {:?}; the chart using it keeps its indicators as its own",
                    template.name
                ),
                n => format!(
                    "deleted chart template {:?}; the {n} charts using it keep their indicators as their own",
                    template.name
                ),
            }
        }
        other => return Err(Fault::usage(format!("no such command: template {other}"))),
    };
    said(as_json, json!({"message": text}), text)
}

/// Chartbook templates, from `chartbook template`. See [`crate::book_templates`].
///
/// A chartbook on one carries every change to its arrangement into it as it
/// is written down — [`Workspace::save`] does that for every command — so
/// there is nothing here for saving into the template a book is already on,
/// and `save` only ever makes a new one.
fn book_template_verb(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    workspace: &mut Workspace,
    selector: Option<&str>,
) -> Result<String, Fault> {
    use crate::book_templates as bt;
    let action = required(m, "ACTION")?.as_str();
    let templates = bt::all(store);
    let named = || {
        arg(m, "TEMPLATE")
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| Fault::usage(format!("`chartbook template {action}` needs a TEMPLATE")))
    };
    let found = |selector: &str| find_book_template(store, selector);

    let (text, value) = match action {
        "list" => return Ok(book_template_list(&templates, workspace, as_json)),
        "save" => {
            let at = workspace.resolve(selector)?;
            let book = workspace.name_of(at);
            let shape = bt::capture(workspace.book(at)?).ok_or_else(|| {
                Fault::new(super::EXIT_ERROR, format!("chartbook {book:?} names a chart it does not have"))
            })?;
            let wanted = named()?;
            // Only ever a new one: replacing a template would change every
            // chartbook on it, which a word like "save" does not say.
            if let Some(why) = bt::refused_name(&templates, wanted, None) {
                return Err(Fault::usage(why));
            }
            let id = bt::create(store, wanted, shape.clone()).id;
            bt::set_link(workspace.book_mut(at)?, Some(id));
            (
                format!(
                    "saved chartbook {book:?} as new chartbook template {wanted:?} ({})",
                    charts_counted(shape.panes.len()),
                ),
                json!({"template": id, "chartbook": book}),
            )
        }
        "apply" => {
            let at = workspace.resolve(selector)?;
            let book = workspace.name_of(at);
            let wanted = named()?;
            if wanted.eq_ignore_ascii_case(crate::chart_defaults::NO_TEMPLATE) {
                bt::set_link(workspace.book_mut(at)?, None);
                (
                    format!("chartbook {book:?} is on no chartbook template; it keeps its layout and indicators"),
                    json!({"template": null, "chartbook": book}),
                )
            } else {
                let chosen = found(wanted)?;
                let charts = crate::templates::all(store);
                let mut next = workspace.next_pane();
                let target = workspace.book_mut(at)?;
                if !bt::apply(target, &chosen.shape, &charts, &mut next, None) {
                    return Err(Fault::new(
                        super::EXIT_ERROR,
                        format!("chartbook template {:?} or chartbook {book:?} is not an arrangement", chosen.name),
                    ));
                }
                bt::set_link(target, Some(chosen.id));
                (
                    format!(
                        "applied chartbook template {:?} to chartbook {book:?}: {}",
                        chosen.name,
                        charts_counted(chosen.shape.panes.len())
                    ),
                    json!({"template": chosen.id, "chartbook": book}),
                )
            }
        }
        "rename" => {
            let template = found(named()?)?;
            let name = arg(m, "NAME").map(|n| n.trim()).filter(|n| !n.is_empty()).ok_or_else(|| {
                Fault::usage("`chartbook template rename` needs a TEMPLATE and a NAME".into())
            })?;
            if let Some(why) = bt::refused_name(&templates, name, Some(template.id)) {
                return Err(Fault::usage(why));
            }
            bt::rename(store, template.id, name);
            (
                format!("renamed chartbook template {:?} to {name:?}", template.name),
                json!({"template": template.id, "name": name}),
            )
        }
        "delete" => {
            let template = found(named()?)?;
            bt::delete(store, template.id);
            let kept = bt::unlink(workspace.books_mut(), template.id);
            let tail = match kept {
                0 => String::new(),
                1 => "; the chartbook on it keeps its layout and indicators".to_string(),
                n => format!("; the {n} chartbooks on it keep their layout and indicators"),
            };
            (
                format!("deleted chartbook template {:?}{tail}", template.name),
                json!({"template": template.id, "chartbooks": kept}),
            )
        }
        other => return Err(Fault::usage(format!("no such action: chartbook template {other}"))),
    };
    workspace.save(store);
    let mut value = value;
    value["message"] = json!(text);
    said(as_json, value, text)
}

fn charts_counted(count: usize) -> String {
    match count {
        1 => "1 chart".to_string(),
        n => format!("{n} charts"),
    }
}

/// The chartbook template a selector names, or why there is none.
fn find_book_template(store: &Store, selector: &str) -> Result<crate::book_templates::BookTemplate, Fault> {
    use crate::book_templates::{self as bt, Lookup};
    let all = bt::all(store);
    match bt::find(&all, selector) {
        Lookup::Found(found) => Ok(found.clone()),
        Lookup::Missing => Err(Fault::not_found(format!(
            "no chartbook template called {selector:?}; see `omacharts chartbook template list`"
        ))),
        Lookup::Ambiguous(ids) => Err(Fault::ambiguous(format!(
            "{} chartbook templates are called {selector:?}; use {}",
            ids.len(),
            ids.iter().map(|id| format!("id:{id}")).collect::<Vec<_>>().join(" or ")
        ))),
    }
}

/// Every chartbook template, how many charts it lays out and how many
/// chartbooks are on it; * marks the open chartbook's.
fn book_template_list(
    templates: &[crate::book_templates::BookTemplate],
    workspace: &Workspace,
    as_json: bool,
) -> String {
    use crate::book_templates as bt;
    let open = workspace.books().get(workspace.active()).and_then(bt::linked);
    if as_json {
        let rows = templates
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "charts": t.shape.panes.len(),
                    "chartbooks": bt::usage(workspace.books(), t.id),
                    "open": open == Some(t.id),
                })
                .to_string()
            })
            .collect();
        return wrap_list("book_templates", rows);
    }
    if templates.is_empty() {
        return "no chartbook templates; save one with `omacharts chartbook template save NAME`\n"
            .into();
    }
    templates
        .iter()
        .map(|t| {
            let books = match bt::usage(workspace.books(), t.id) {
                1 => "1 chartbook".to_string(),
                n => format!("{n} chartbooks"),
            };
            format!(
                "{}{:<5} {:<20} {:<9} {books}",
                if open == Some(t.id) { "* " } else { "  " },
                format!("id:{}", t.id),
                t.name,
                charts_counted(t.shape.panes.len()),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// A name as typed, refusing a blank one: a template nobody can name is one
/// nobody can pick again.
fn template_name(m: &clap::ArgMatches, id: &str) -> Result<String, Fault> {
    let name = required(m, id)?.trim();
    if name.is_empty() {
        return Err(Fault::usage("a chart template needs a name".into()));
    }
    Ok(name.to_string())
}

/// Bring every chart on template `id` up to date: showing `indicators`, or,
/// with none, letting go of a template that has gone. How many there were.
fn follow_template(workspace: &mut Workspace, id: u32, indicators: Option<&Value>) -> usize {
    let mut count = 0;
    workspace.each_pane_mut(|pane| {
        if pane["template"].as_u64() != Some(u64::from(id)) {
            return;
        }
        count += 1;
        match indicators {
            Some(indicators) => pane["indicators"] = indicators.clone(),
            None => pane["template"] = Value::Null,
        }
    });
    count
}

/// How many charts, in every chartbook, are on template `id`.
fn charts_on(workspace: &Workspace, id: u32) -> usize {
    workspace
        .books()
        .iter()
        .flat_map(charts::panes)
        .filter(|pane| pane["template"].as_u64() == Some(u64::from(id)))
        .count()
}

fn template_list(store: &Store, workspace: &Workspace, as_json: bool) -> Result<String, Fault> {
    let default = ChartDefaults::load(store).template;
    let templates = crate::templates::all(store);
    if as_json {
        let rows = templates
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "name": t.name,
                    "indicators": t.indicators.iter().map(Indicator::label).collect::<Vec<_>>(),
                    "charts": charts_on(workspace, t.id),
                    "default": default == Some(t.id),
                })
                .to_string()
            })
            .collect();
        return Ok(wrap_list("templates", rows));
    }
    if templates.is_empty() {
        return Ok("no chart templates; make one with `omacharts template create NAME`\n".into());
    }
    Ok(templates
        .iter()
        .map(|t| {
            let shows = match t.indicators.is_empty() {
                true => "no indicators".to_string(),
                false => t.indicators.iter().map(Indicator::label).collect::<Vec<_>>().join(", "),
            };
            let charts = match charts_on(workspace, t.id) {
                1 => "1 chart".to_string(),
                n => format!("{n} charts"),
            };
            format!(
                "{}{:<5} {:<20} {:<9} {shows}",
                if default == Some(t.id) { "* " } else { "  " },
                format!("id:{}", t.id),
                t.name,
                charts,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

/// `chart indicator`, for a template: the same actions and flags, and every
/// chart on the template follows.
fn template_indicator(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    workspace: &mut Workspace,
) -> Result<String, Fault> {
    let action = required(m, "ACTION")?.as_str();
    let template = find_template(store, required(m, "TEMPLATE")?)?;
    let mut list: Vec<Value> =
        template.indicators.iter().map(to_value).collect::<Result<_, _>>()?;
    if action == "list" {
        return Ok(list_indicators(&list, json!({"template": template.name}), as_json));
    }

    let kind = indicator_kind(m, action)?;
    let edits = Edits::read(m)?;
    let text = edit_indicators(m, action, kind, &edits, &mut list, "this chart template")?;
    let indicators: Vec<Indicator> = serde_json::from_value(Value::Array(list))
        .map_err(|e| Fault::new(super::EXIT_ERROR, e.to_string()))?;
    crate::templates::set_indicators(store, template.id, indicators.clone());
    let following = follow_template(workspace, template.id, Some(&to_value(&indicators)?));
    workspace.save(store);

    let text = match following {
        0 => format!("{text} in chart template {:?}", template.name),
        1 => format!("{text} in chart template {:?}, and on the chart using it", template.name),
        n => format!("{text} in chart template {:?}, and on the {n} charts using it", template.name),
    };
    said(as_json, json!({"template": template.id, "message": text, "charts": following}), text)
}

/// What a new chart opens with, read or chosen.
fn template_default(store: &Store, m: &clap::ArgMatches, as_json: bool) -> Result<String, Fault> {
    let Some(selector) = arg(m, "TEMPLATE") else {
        let current = ChartDefaults::load(store)
            .template
            .and_then(|id| crate::templates::get(store, id));
        let text = match &current {
            Some(t) => format!("new charts open with chart template {:?}", t.name),
            None => "new charts open with no chart template".to_string(),
        };
        return said(as_json, json!({"template": current.map(|t| t.name)}), text);
    };
    let (value, text) = default_template(store, selector)?;
    store.set_setting(crate::chart_defaults::SETTING_TEMPLATE, &value);
    said(as_json, json!({"template": value}), text)
}

/// What to store for "new charts open with `selector`", and how to say so.
///
/// Shared by `template default` and `config set chart_template`, so that the
/// setting only ever holds an id that names a template, or `none`: a name
/// stored there would read as no template at all, and look as though it
/// worked.
fn default_template(store: &Store, selector: &str) -> Result<(String, String), Fault> {
    if selector.eq_ignore_ascii_case(crate::chart_defaults::NO_TEMPLATE) {
        return Ok((
            crate::chart_defaults::NO_TEMPLATE.to_string(),
            "new charts open with no chart template".to_string(),
        ));
    }
    let template = find_template(store, selector)?;
    Ok((
        template.id.to_string(),
        format!("new charts open with chart template {:?}", template.name),
    ))
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
            stored_mode(pane).key(),
        ));
    }
    Ok(out)
}

/// Whether a pointer on a Follow chart draws a line on the other Follow charts.
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
        format!("crosshair syncing across Follow charts is {state}"),
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
    // The header too, so a typo is refused here rather than read back later
    // as a header missing a part.
    if key == crate::header::SETTING {
        let parts = crate::header::parse(value).map_err(Fault::usage)?;
        crate::header::save(store, &parts);
        let value = crate::header::spell(&parts);
        let text = format!("every chart's header shows {value}");
        return said(as_json, json!({"key": key, "value": value}), text);
    }
    // The templates themselves are a document `template` keeps, with the
    // charts on them holding copies; written raw, the charts would not follow.
    if key == crate::templates::SETTING {
        return Err(Fault::usage(
            "chart templates are changed with `omacharts template …`, not `config set`".into(),
        ));
    }
    if key == crate::chart_defaults::SETTING_TEMPLATE {
        let (value, text) = default_template(store, value)?;
        store.set_setting(key, &value);
        return said(as_json, json!({"key": key, "value": value}), text);
    }
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
/// Monochrome is a scheme like any other, so choosing it without
/// writing this down would throw away a palette somebody had picked and leave
/// them on the default when they changed their mind.
const COLOURED_BARS: &str = "coloured_bar_scheme";

/// Colour or no colour, and which way round, asked the way people arrive at
/// it.
///
/// `config set bar_scheme theme-mono` reaches the same scheme, and is not the
/// same command: it does not remember what to go back to, so putting the
/// colour back lands on the default rather than on the scheme that was in use.
fn config_bars(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    live: Option<&dyn Live>,
) -> Result<String, Fault> {
    // Spelled as a colouring, whatever an older version wrote: a scheme id
    // that is not one names colours that now live in a theme.
    let colouring = |id: Option<String>| {
        omacharts_engine::BarColouring::from_key(id.as_deref().unwrap_or(""))
            .unwrap_or_default()
            .key()
            .to_string()
    };
    let current = colouring(store.setting(crate::theming::SETTING_BARS));
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
        _ => colouring(store.setting(COLOURED_BARS)),
    };
    // Leaving a palette somebody picked: remember it to come back to.
    if scheme != current && spell_bars(&current) == "coloured" {
        store.set_setting(COLOURED_BARS, &current);
    }
    store.set_setting(crate::theming::SETTING_BARS, &scheme);
    // Charts are painted one by one now, so the question asked of the whole
    // app is answered on every chart as well as for the next one — what
    // "Apply to all charts" does with the same choice in the preferences.
    let mut workspace = charts::Workspace::load(store);
    workspace.set_on_every_pane("colouring", json!(scheme));
    workspace.save(store);
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
///
/// Each moves a widget installed under the old id first, so that what they
/// report on or change is the widget as it is now. `install` and `uninstall`
/// report a move that could not be made, as `bar_plugin::install`, `refresh`
/// and `remove` do; `status` answers regardless, because a move that could
/// not be made is no reason not to say what is there.
fn plugin_status(home: &std::path::Path, as_json: bool) -> Result<String, Fault> {
    let _ = crate::bar_plugin::migrate_in_full(home);
    let where_at = crate::bar_plugin::location(home);
    let at = where_at.display();
    let (available, installed) =
        (crate::bar_plugin::available(home), crate::bar_plugin::installed(home));
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

fn plugin_install(home: &std::path::Path, as_json: bool) -> Result<String, Fault> {
    if !crate::bar_plugin::available(home) {
        return said(
            as_json,
            json!({"omarchy": false, "installed": false}),
            "no Omarchy bar on this desktop, so there is nowhere to put the widget".to_string(),
        );
    }
    // Already in the bar means bring it up to date rather than write it again:
    // a widget installed once and never touched runs whichever version shipped
    // the day the switch was flicked. Judged after an old install has been
    // moved, or the answer would be "installed" for a widget that was in the
    // bar all along under its old id. `install` and `refresh` move it again
    // themselves, which finds nothing to do.
    crate::bar_plugin::migrate_in_full(home).map_err(|e| Fault::new(super::EXIT_ERROR, e))?;
    let already = crate::bar_plugin::installed(home);
    let done = match already {
        true => crate::bar_plugin::refresh(home),
        false => crate::bar_plugin::install(home),
    };
    done.map_err(|e| Fault::new(super::EXIT_ERROR, e))?;
    let at = crate::bar_plugin::location(home);
    said(
        as_json,
        json!({"omarchy": true, "installed": true, "updated": already, "path": at}),
        match already {
            true => format!("already in the bar; brought it up to date in {}", at.display()),
            false => format!("installed in the bar, in {}", at.display()),
        },
    )
}

fn plugin_uninstall(home: &std::path::Path, as_json: bool) -> Result<String, Fault> {
    // Idempotent on purpose: a script that takes the widget out should not
    // have to ask first, and "it was already gone" is the outcome it wanted.
    // No bar at all is the same outcome, said before `remove` goes looking
    // for the shell.json it would have edited.
    if !crate::bar_plugin::available(home) {
        return said(
            as_json,
            json!({"omarchy": false, "installed": false, "removed": false}),
            "no Omarchy bar on this desktop, so there was nothing to remove".to_string(),
        );
    }
    let removed = crate::bar_plugin::remove(home).map_err(|e| Fault::new(super::EXIT_ERROR, e))?;
    // The same keys whatever happened, so a script reads one shape.
    said(
        as_json,
        json!({"omarchy": true, "installed": false, "removed": removed}),
        match removed {
            true => "taken out of the bar".to_string(),
            false => "not in the bar, so there was nothing to remove".to_string(),
        },
    )
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

fn backup_fault(error: crate::backup::Error) -> Fault {
    Fault::new(super::EXIT_ERROR, error.to_string())
}

fn backup_json(backup: &crate::backup::Backup) -> Value {
    json!({
        "name": backup.name,
        "takenAt": backup.taken.to_rfc3339(),
        "bytes": backup.bytes,
        "schema": backup.schema,
    })
}

fn backup_list(store: &Store, as_json: bool) -> Result<String, Fault> {
    let settings = store.path().ok_or_else(|| backup_fault(crate::backup::Error::NoFile))?;
    let backups = crate::backup::list(settings);
    if as_json {
        return Ok(format!(
            "{}\n",
            json!({
                "folder": crate::backup::dir(settings).to_string_lossy(),
                "backups": backups.iter().map(backup_json).collect::<Vec<_>>(),
            })
        ));
    }
    if backups.is_empty() {
        return Ok("no backups yet; one is taken when the window opens\n".to_string());
    }
    let mut out = String::new();
    for backup in &backups {
        out.push_str(&format!(
            "{}  {}  {:>8}  schema {}\n",
            backup.name,
            crate::backup::local_stamp(backup.taken),
            crate::ui::preferences::describe_bytes(backup.bytes as i64),
            backup.schema.map(|v| v.to_string()).unwrap_or_else(|| "?".to_string()),
        ));
    }
    Ok(out)
}

fn backup_create(store: &Store, as_json: bool) -> Result<String, Fault> {
    let (backup, fresh) = crate::backup::take(store).map_err(backup_fault)?;
    let mut value = backup_json(&backup);
    value["new"] = json!(fresh);
    said(
        as_json,
        value,
        match fresh {
            true => format!(
                "backed up the settings to {} ({})",
                backup.name,
                crate::ui::preferences::describe_bytes(backup.bytes as i64)
            ),
            false => format!("nothing changed since {}, so nothing new was kept", backup.name),
        },
    )
}

fn backup_restore(
    store: &Store,
    m: &clap::ArgMatches,
    as_json: bool,
    live: Option<&dyn Live>,
) -> Result<String, Fault> {
    let name = required(m, "NAME")?;
    // A file is a file brought from elsewhere; anything else names a backup.
    // A relative path was made absolute where it was typed — see
    // `cli::caller_paths` — so by here it is either a full path or a name.
    let path = match name.strip_prefix("~/") {
        Some(rest) => crate::store::home().join(rest),
        None => std::path::PathBuf::from(name),
    };
    // One of this machine's own backups, however it was named, is restored
    // as that backup: everything in it, this machine's settings included.
    let own = store
        .path()
        .map(crate::backup::dir)
        .zip(path.parent())
        .is_some_and(|(folder, parent)| {
            folder.canonicalize().ok() == parent.canonicalize().ok() && parent.exists()
        });
    let restored = match (path.is_file() || name.contains('/')) && !own {
        true => crate::backup::restore_file(store, &path),
        false => crate::backup::restore(
            store,
            path.file_name().and_then(|n| n.to_str()).unwrap_or(name),
        ),
    }
    .map_err(backup_fault)?;
    // The window holds the theme, the feed and the rail in memory. The
    // arrangement is reloaded after this returns, like any command that
    // writes it.
    if let Some(live) = live {
        live.adopt_theming();
        live.adopt_feed();
        live.reload_watchlists();
    }
    said(
        as_json,
        json!({"restored": backup_json(&restored.from), "kept": backup_json(&restored.kept)}),
        format!(
            "restored the settings from {} ({}); what they replaced is kept as {}",
            restored.from.name,
            crate::backup::local_stamp(restored.from.taken),
            restored.kept.name,
        ),
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

    /// A store whose new charts open with nothing on them.
    ///
    /// A fresh profile's charts open on the "Default" chart template, holding
    /// volume — which the template tests below are about. Everything else
    /// here counts indicators from an empty chart, and was written when that
    /// is what a new chart was.
    fn bare() -> Store {
        let store = Store::memory().unwrap();
        store.set_setting(
            crate::chart_defaults::SETTING_TEMPLATE,
            crate::chart_defaults::NO_TEMPLATE,
        );
        store
    }

    fn json_of(out: &Outcome) -> Value {
        assert_eq!(out.code, 0, "{}", out.err);
        serde_json::from_str(&out.out).unwrap()
    }

    #[test]
    fn a_new_chart_opens_on_the_default_template() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        let listed = json_of(&run("chart indicator list --book Macro --json", &store));
        assert_eq!(listed["indicators"][0]["kind"], "volume");
        let charts = Workspace::load(&store);
        let default = crate::templates::all(&store)[0].id;
        assert_eq!(charts::panes(&charts.books()[0])[0]["template"], json!(default));
    }

    #[test]
    fn templates_are_listed_made_renamed_and_deleted() {
        let store = Store::memory().unwrap();
        assert_eq!(run("template create Swing", &store).code, 0);
        assert_eq!(run("template rename swing Swing2", &store).code, 0);
        let listed = json_of(&run("template list --json", &store));
        let names: Vec<&str> =
            listed["templates"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names, ["Default", "Swing2"]);
        assert_eq!(listed["templates"][0]["default"], true);
        assert_eq!(listed["templates"][0]["indicators"][0], "Volume");

        assert_eq!(run("template delete Swing2", &store).code, 0);
        assert_eq!(run("template delete Swing2", &store).code, super::super::EXIT_NOT_FOUND);
        assert_ne!(run("template create", &store).code, 0, "a template with no name");
    }

    /// Changing a template changes what every chart on it shows, in every
    /// chartbook, and leaves a chart on no template alone.
    #[test]
    fn a_template_s_indicators_are_what_its_charts_show() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chartbook create Semis", &store);
        run("chart set --book Semis --template none", &store);

        let out = run("template indicator add Default rsi --period 9", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("2 charts"), "{}", out.out);

        let kinds = |book: &str| -> Vec<Vec<String>> {
            let workspace = Workspace::load(&store);
            let at = workspace.find(book).unwrap();
            charts::panes(&workspace.books()[at])
                .iter()
                .map(|p| {
                    p["indicators"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|i| i["kind"].as_str().unwrap().to_string())
                        .collect()
                })
                .collect()
        };
        assert_eq!(kinds("Macro"), vec![vec!["volume", "rsi"], vec!["volume", "rsi"]]);
        assert_eq!(kinds("Semis"), vec![Vec::<String>::new()], "a chart on none followed anyway");

        run("template indicator set Default rsi --period 21", &store);
        run("template indicator remove Default volume", &store);
        let held = json_of(&run("template indicator list Default --json", &store));
        assert_eq!(held["indicators"][0]["params"]["period"], 21);
        assert_eq!(kinds("Macro"), vec![vec!["rsi"], vec!["rsi"]]);
    }

    #[test]
    fn a_chart_switches_onto_a_template_and_onto_none() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add sma --book Macro", &store);

        // Onto it: the chart's own indicators give way to the template's.
        let out = run("chart set --book Macro --template Default", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = json_of(&run("chart indicator list --book Macro --json", &store));
        assert_eq!(listed["indicators"].as_array().unwrap().len(), 1);
        assert_eq!(listed["indicators"][0]["kind"], "volume");

        // Onto none: the empty set, and no longer following.
        let out = run("chart set --book Macro --template none", &store);
        assert!(out.out.contains("no indicators"), "{}", out.out);
        run("template indicator add Default rsi", &store);
        let listed = json_of(&run("chart indicator list --book Macro --json", &store));
        assert_eq!(listed["indicators"].as_array().unwrap().len(), 0, "{listed}");

        assert_eq!(
            run("chart set --book Macro --template Nothing", &store).code,
            super::super::EXIT_NOT_FOUND
        );
    }

    /// Adding to a chart on a template lets go of the template first, so the
    /// template — and every other chart on it — is left as it was.
    #[test]
    fn adding_to_a_chart_on_a_template_leaves_the_template_alone() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("chart split horizontal --book Macro", &store);
        let out = run("chart indicator add sma --book Macro --chart pos:0", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("no longer uses chart template \"Default\""), "{}", out.out);

        let workspace = Workspace::load(&store);
        let panes = charts::panes(&workspace.books()[0]);
        assert_eq!(panes[0]["template"], Value::Null);
        assert_eq!(panes[0]["indicators"].as_array().unwrap().len(), 2, "kept volume, added sma");
        assert_eq!(panes[1]["indicators"].as_array().unwrap().len(), 1);
        assert_eq!(crate::templates::all(&store)[0].indicators.len(), 1);
    }

    /// Nothing disappears from a chart when its template goes.
    #[test]
    fn deleting_a_template_leaves_its_charts_a_copy() {
        let store = Store::memory().unwrap();
        run("chartbook create Macro --switch", &store);
        run("template indicator add Default ema", &store);
        let out = run("template delete Default", &store);
        assert!(out.out.contains("keeps its indicators"), "{}", out.out);

        let workspace = Workspace::load(&store);
        let pane = &charts::panes(&workspace.books()[0])[0];
        assert_eq!(pane["template"], Value::Null);
        assert_eq!(pane["indicators"].as_array().unwrap().len(), 2);
        assert_eq!(ChartDefaults::load(&store).template, None);
    }

    #[test]
    fn the_header_is_one_setting_and_a_typo_is_refused() {
        let store = bare();
        assert_eq!(crate::header::load(&store), crate::header::standard());
        let out = run("config set chart_header change,symbol,template", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert_eq!(out.out, "every chart's header shows change,symbol,template\n");
        assert_eq!(run("config set chart_header symbol,price", &store).code, EXIT_USAGE);
        use crate::header::Part;
        assert_eq!(crate::header::load(&store), vec![Part::Change, Part::Symbol, Part::Template]);
        // Not a chart's to have: there is no flag for it any more.
        assert_eq!(run("chart set --header symbol", &store).code, EXIT_USAGE);
    }

    #[test]
    fn the_default_template_is_read_and_chosen_by_name() {
        let store = Store::memory().unwrap();
        run("template create Swing", &store);
        assert!(run("template default", &store).out.contains("\"Default\""));
        assert_eq!(run("template default swing", &store).code, 0);
        let swing = crate::templates::all(&store)[1].id;
        assert_eq!(ChartDefaults::load(&store).template, Some(swing));

        // `config set` reaches the same setting, and stores an id or none.
        assert_eq!(run("config set chart_template none", &store).code, 0);
        assert_eq!(store.setting("chart_template").as_deref(), Some("none"));
        assert_eq!(run("config set chart_template Default", &store).code, 0);
        assert_eq!(ChartDefaults::load(&store).template, Some(crate::templates::all(&store)[0].id));
        assert_eq!(
            run("config set chart_template Nope", &store).code,
            super::super::EXIT_NOT_FOUND,
            "a name that is no template was stored"
        );
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
        let store = bare();
        run("watchlist add Default SPY QQQ", &store);
        run("watchlist rename Default Mine", &store);
        run("watchlist create Semis", &store);
        run("watchlist add Semis NVDA AMD", &store);
        run("section create Semis Memory", &store);
        run("watchlist add Semis MU --section Memory", &store);
        run("watchlist add Semis 2330 --suffix TW --section Memory", &store);
        // One symbol in two sections, which a list may hold on purpose.
        run("watchlist add Semis NVDA --section Memory", &store);
        store
    }

    #[test]
    fn an_export_imported_on_another_machine_comes_back_the_same() {
        let file = run("watchlist export", &exporting()).out;
        let elsewhere = bare();
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
        let elsewhere = bare();
        import(&file, "", &elsewhere);
        let before = run("watchlist export", &elsewhere).out;
        let again = import(&file, "", &elsewhere);
        assert!(again.out.lines().all(|line| line.ends_with("already up to date")), "{}", again.out);
        assert_eq!(run("watchlist export", &elsewhere).out, before);
    }

    #[test]
    fn a_merge_only_adds_and_a_replace_makes_it_match() {
        let file = run("watchlist export Semis", &exporting()).out;
        let elsewhere = bare();
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

    /// A file exported while lists drove link groups says which one each
    /// drove. Lists drive no group now, so the field is read past rather than
    /// refused, and nothing is written for it.
    #[test]
    fn a_file_that_names_a_link_group_imports_as_one_that_does_not() {
        let file = r#"{"omacharts": "watchlists", "version": 1, "watchlists": [
            {"name": "FX", "link": 3, "sections": [{"symbols": ["SPY"]}]}]}"#;
        let store = bare();
        let out = import(file, "", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(!out.out.contains("link"), "{}", out.out);
        let (fx, _) = super::find_list(&store, "FX").unwrap();
        assert_eq!(store.setting(&format!("watchlist_link_{fx}")), None);
        assert!(!run("watchlist export FX", &store).out.contains("link"));
    }

    /// Commands read a chart stored with a link group as the mode the window
    /// gives it, so a terminal and the window agree about one row.
    #[test]
    fn a_chart_stored_with_a_link_group_lists_as_its_mode() {
        let store = bare();
        let pane = |id: u32, linked: &str| {
            format!(
                r#"{{"id":{id},"symbol":"SPY","timeframe":"1D","indicators":[],
                    "bar_style":"candles","session":"extended","show_grid":true,
                    "linked":{linked}}}"#
            )
        };
        store.set_setting(
            "workspace",
            &format!(
                r#"{{"books":[{{"name":"Old","layout":{{"split":{{"horizontal":true,"ratio":0.5,
                    "first":{{"leaf":1}},"second":{{"leaf":2}}}}}},"focused":1,"watchlist":1,
                    "panes":[{},{}]}}],"active":0}}"#,
                pane(1, "1"),
                pane(2, "4")
            ),
        );
        let listed = run("chart list --book Old --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["link"], "follow", "{}", listed.out);
        assert_eq!(parsed["charts"][1]["link"], "focus", "{}", listed.out);
    }

    #[test]
    fn a_watchlist_can_be_made_filled_and_read_back() {
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
        assert_eq!(run("watchlist show Nope", &store).code, super::super::EXIT_NOT_FOUND);
        assert_eq!(run("watchlist frobnicate", &store).code, super::super::EXIT_USAGE);
    }

    #[test]
    fn the_watchlist_the_bar_widget_shows_cannot_be_deleted() {
        let store = bare();
        let refused = run("watchlist delete id:1", &store);
        assert_eq!(refused.code, super::super::EXIT_REFUSED);
        assert!(refused.err.contains("rename"), "{}", refused.err);
    }

    #[test]
    fn adding_the_same_symbol_twice_is_not_an_error() {
        let store = bare();
        run("watchlist create Semis", &store);
        assert_eq!(run("watchlist add Semis NVDA", &store).code, 0);
        assert_eq!(run("watchlist add Semis NVDA", &store).code, 0);
        let shown = run("watchlist show Semis --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&shown.out).unwrap();
        assert_eq!(parsed["sections"][0]["symbols"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_taiwan_listing_can_be_named_with_its_venue_or_with_the_flag() {
        let store = bare();
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
        let store = bare();
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
        let store = bare();
        // FTSEMIB.MI is an index whose canonical symbol has the dot in it.
        let out = run("symbol show FTSEMIB.MI --json", &store);
        assert_eq!(out.code, 0, "{}", out.err);
    }

    #[test]
    fn a_ticker_nobody_lists_is_refused_rather_than_stored() {
        let store = bare();
        run("watchlist create Semis", &store);
        let out = run("watchlist add Semis NOTATICKER", &store);
        assert_eq!(out.code, super::super::EXIT_NOT_FOUND);
    }

    #[test]
    fn a_section_can_become_a_watchlist_carrying_its_symbols() {
        let store = bare();
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
        let store = bare();
        assert_eq!(run("chartbook create Macro --symbol SPY --switch", &store).code, 0);
        let split = run("chart split horizontal --book Macro", &store);
        assert_eq!(split.code, 0, "{}", split.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"].as_array().unwrap().len(), 2);
    }

    fn ok(line: &str, store: &Store) -> String {
        let out = run(line, store);
        assert_eq!(out.code, 0, "{line}: {}", out.err);
        out.out
    }

    /// A book's charts, in layout order.
    fn panes_of(store: &Store, book: &str) -> Vec<Value> {
        let workspace = Workspace::load(store);
        let at = workspace.find(book).unwrap();
        let book = &workspace.books()[at];
        charts::leaves(&book["layout"])
            .iter()
            .map(|id| {
                charts::panes(book).iter().find(|p| p["id"].as_u64() == Some(u64::from(*id))).unwrap().clone()
            })
            .collect()
    }

    fn symbols_of(store: &Store, book: &str) -> Vec<String> {
        panes_of(store, book).iter().map(|p| p["symbol"].as_str().unwrap().to_string()).collect()
    }

    fn swing(store: &Store) -> Value {
        json!(find_template(store, "Swing").unwrap().id)
    }

    /// Two by two, with an RSI on the first chart and a chart template on
    /// the second: the mixed book the chartbook templates are for.
    fn grid_book(store: &Store, name: &str, symbol: &str) {
        ok(&format!("chartbook create {name} --symbol {symbol}"), store);
        ok(&format!("chart split horizontal --book {name} --chart pos:0"), store);
        ok(&format!("chart split vertical --book {name} --chart pos:0"), store);
        ok(&format!("chart split vertical --book {name} --chart pos:2"), store);
        ok(&format!("chart indicator add rsi --book {name} --chart pos:0"), store);
        ok("template create Swing", store);
        ok("template indicator add Swing macd", store);
        ok(&format!("chart set --book {name} --chart pos:1 --template Swing"), store);
        ok(&format!("chart set --book {name} --chart pos:2 --resolution 1h"), store);
    }

    #[test]
    fn a_chartbook_is_saved_as_a_template_and_applied_to_another() {
        let store = bare();
        grid_book(&store, "Grid", "SPY");
        ok("chartbook create Other --symbol QQQ", &store);
        assert!(ok("chartbook template save Corners --book Grid", &store).contains("4 charts"));

        let listed = json_of(&run("chartbook template list --json", &store));
        assert_eq!(listed["book_templates"][0]["chartbooks"], 1);
        assert_eq!(listed["book_templates"][0]["charts"], 4);

        ok("chartbook template apply Corners --book Other", &store);
        assert_eq!(symbols_of(&store, "Other"), ["QQQ"; 4], "the chartbook lost its symbol");
        assert_eq!(panes_of(&store, "Other")[1]["template"], swing(&store));
        assert_eq!(panes_of(&store, "Other")[2]["timeframe"], "1h", "the resolution was not taken");
        assert_eq!(panes_of(&store, "Other")[0]["timeframe"], "1D");
        let first = json_of(&run("chart indicator list --book Other --chart pos:0 --json", &store));
        assert_eq!(first["indicators"][0]["kind"], "rsi");
        let listed = json_of(&run("chartbook template list --json", &store));
        assert_eq!(listed["book_templates"][0]["chartbooks"], 2);
    }

    /// A change to a chartbook on a template reaches the template and every
    /// other chartbook on it, whether or not it is the one open.
    #[test]
    fn a_change_to_a_chartbook_on_a_template_reaches_the_others_on_it() {
        let store = bare();
        grid_book(&store, "Grid", "SPY");
        ok("chartbook template save Corners --book Grid", &store);
        ok("chartbook create Other --symbol QQQ", &store);
        ok("chartbook template apply Corners --book Other", &store);
        ok("chartbook create Loose --symbol IWM", &store);
        ok("chartbook switch Grid", &store);

        // The open book closes a chart; the closed one follows, by position.
        ok("chart close --book Grid --chart pos:3", &store);
        assert_eq!(symbols_of(&store, "Other").len(), 3);
        // A closed book is changed in turn, and the open one follows.
        ok("chart indicator add atr --book Other --chart pos:2", &store);
        let atr = json_of(&run("chart indicator list --book Grid --chart pos:2 --json", &store));
        assert_eq!(atr["indicators"][0]["kind"], "atr");
        let templates = crate::book_templates::all(&store);
        assert_eq!(templates[0].shape.panes.len(), 3);
        assert_eq!(templates[0].shape.panes[2].indicators[0].kind, IndicatorKind::Atr);
        // A resolution goes to the same position, from the open book and to it.
        ok("chart set --book Grid --chart pos:1 --resolution 15m", &store);
        assert_eq!(panes_of(&store, "Other")[1]["timeframe"], "15m");
        ok("chart set --book Other --chart pos:0 --resolution 1W", &store);
        assert_eq!(panes_of(&store, "Grid")[0]["timeframe"], "1W");
        assert_eq!(panes_of(&store, "Loose")[0]["timeframe"], "1D");
        // A symbol is the book's own.
        ok("chart set --book Grid --chart pos:0 --symbol AAPL", &store);
        assert_eq!(symbols_of(&store, "Other")[0], "QQQ");
        // And a book on none is left alone.
        assert_eq!(symbols_of(&store, "Loose"), ["IWM"]);
    }

    #[test]
    fn a_chartbook_can_be_made_on_a_template() {
        let store = bare();
        grid_book(&store, "Grid", "SPY");
        ok("chartbook template save Corners --book Grid", &store);
        let made = ok("chartbook create Oil --symbol CL --template corners", &store);
        assert!(made.contains("on chartbook template \"Corners\": 4 charts"), "{made}");
        assert_eq!(symbols_of(&store, "Oil"), ["CL"; 4]);
        assert_eq!(panes_of(&store, "Oil")[1]["template"], swing(&store));
        assert_eq!(panes_of(&store, "Oil")[2]["timeframe"], "1h");
        let listed = json_of(&run("chartbook template list --json", &store));
        assert_eq!(listed["book_templates"][0]["chartbooks"], 2);
        // And it is on it: a change there reaches the book it came from.
        ok("chart close --book Oil --chart pos:3", &store);
        assert_eq!(symbols_of(&store, "Grid").len(), 3);

        let missing = run("chartbook create Gas --template Nope", &store);
        assert_eq!(missing.code, super::super::EXIT_NOT_FOUND);
        assert!(Workspace::load(&store).find("Gas").is_err(), "a refused create made a book");
    }

    #[test]
    fn saving_only_ever_makes_a_new_template() {
        let store = bare();
        grid_book(&store, "Grid", "SPY");
        ok("chartbook create One --symbol QQQ", &store);
        ok("chartbook template save Solo --book One", &store);
        ok("chartbook create Two --symbol IWM", &store);
        ok("chartbook template apply Solo --book Two", &store);

        for line in [
            "chartbook template save solo --book Grid",
            "chartbook template save Solo --book One",
            "chartbook template save id:1 --book Grid",
            "chartbook template save None --book Grid",
        ] {
            let refused = run(line, &store);
            assert_eq!(refused.code, EXIT_USAGE, "{line}: {}", refused.err);
        }
        assert!(run("chartbook template save SOLO --book Grid", &store).err.contains("already a chartbook template"));
        assert_eq!(symbols_of(&store, "Two").len(), 1, "a refused save changed a book on the template");
        assert_eq!(crate::book_templates::all(&store)[0].shape.panes.len(), 1);

        assert!(ok("chartbook template save Corners --book Grid", &store).contains("new chartbook template"));
        assert_eq!(crate::book_templates::all(&store).len(), 2);
        assert_eq!(symbols_of(&store, "Two").len(), 1);
    }

    #[test]
    fn a_chartbook_template_is_renamed_let_go_of_and_deleted() {
        let store = bare();
        grid_book(&store, "Grid", "SPY");
        ok("chartbook template save Corners --book Grid", &store);
        ok("chartbook create Other --symbol QQQ", &store);
        ok("chartbook template apply Corners --book Other", &store);

        // A name another template has, or one a selector reads as something
        // else, is refused; a template's own name in another case is not.
        ok("chartbook create Spare --symbol IWM", &store);
        ok("chartbook template save Solo --book Spare", &store);
        for line in ["chartbook template rename Corners solo", "chartbook template rename Corners none"] {
            assert_eq!(run(line, &store).code, EXIT_USAGE, "{line}");
        }
        ok("chartbook template rename Corners CORNERS", &store);
        ok("chartbook template delete Solo", &store);
        ok("chartbook delete Spare", &store);
        ok("chartbook template rename Corners Quarters", &store);
        assert!(ok("chartbook template list", &store).contains("Quarters"));
        assert_eq!(run("chartbook template rename Corners X", &store).code, super::super::EXIT_NOT_FOUND);
        assert_eq!(run("chartbook template apply id:99 --book Grid", &store).code, super::super::EXIT_NOT_FOUND);

        // None lets go and keeps everything; a change stops travelling.
        ok("chartbook template apply none --book Other", &store);
        assert_eq!(symbols_of(&store, "Other").len(), 4);
        ok("chart close --book Other --chart pos:0", &store);
        assert_eq!(symbols_of(&store, "Grid").len(), 4);

        let gone = ok("chartbook template delete Quarters", &store);
        assert!(gone.contains("the chartbook on it keeps"), "{gone}");
        assert!(crate::book_templates::all(&store).is_empty());
        assert_eq!(symbols_of(&store, "Grid").len(), 4);
        ok("chart close --book Grid --chart pos:0", &store);
        assert_eq!(symbols_of(&store, "Other").len(), 3);
        assert!(Workspace::load(&store).books().iter().all(|b| crate::book_templates::linked(b).is_none()));
    }

    #[test]
    fn a_duplicate_is_a_copy_beside_the_original_on_the_same_template() {
        let store = bare();
        ok("watchlist create Semis", &store);
        grid_book(&store, "Grid", "SPY");
        ok("chartbook watchlist Grid Semis", &store);
        ok("chartbook create Last --symbol IWM", &store);
        ok("chartbook template save Corners --book Grid", &store);
        ok("chartbook duplicate Grid --name Copy --switch", &store);

        let workspace = Workspace::load(&store);
        assert_eq!(workspace.name_of(1), "Copy", "the copy is not beside the original");
        assert_eq!(workspace.active(), 1);
        assert_eq!(symbols_of(&store, "Copy"), symbols_of(&store, "Grid"));
        assert_eq!(panes_of(&store, "Copy")[1]["template"], swing(&store));
        assert_eq!(panes_of(&store, "Copy")[0]["indicators"][0]["kind"], "rsi");
        assert_eq!(workspace.books()[1]["watchlist"], json!(DEFAULT_WATCHLIST), "two books on one watchlist");
        assert_eq!(crate::book_templates::linked(&workspace.books()[1]), crate::book_templates::linked(&workspace.books()[0]));
        // Its charts are its own.
        let ids = |at: usize| charts::leaves(&workspace.books()[at]["layout"]);
        assert!(ids(1).iter().all(|id| !ids(0).contains(id)));
        let uids = |at: usize| -> Vec<Value> { charts::panes(&workspace.books()[at]).iter().map(|p| p["drawing_uid"].clone()).collect() };
        assert!(uids(1).iter().all(|uid| !uids(0).contains(uid)));
        // And being on the template, a change to it travels.
        ok("chart close --book Copy --chart pos:0", &store);
        assert_eq!(symbols_of(&store, "Grid").len(), 3);
        // The default name.
        ok("chartbook duplicate Last", &store);
        let workspace = Workspace::load(&store);
        assert_eq!(workspace.find("Last copy").unwrap(), workspace.find("Last").unwrap() + 1);
    }

    /// A chartbook made from a terminal starts the way one made in the window
    /// does: its chart on Follow, so picking in the rail moves it.
    #[test]
    fn a_new_chartbooks_chart_starts_on_follow() {
        let store = bare();
        run("chartbook create Macro --symbol SPY --switch", &store);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["link"], "follow", "{}", listed.out);
    }

    #[test]
    fn the_last_chart_in_a_chartbook_cannot_be_closed() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        assert_eq!(run("chart close --book Macro", &store).code, super::super::EXIT_REFUSED);
    }

    #[test]
    fn a_charts_symbol_and_resolution_can_be_set_together() {
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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

    /// A log axis is stored with the chart and reported back, and a chart
    /// stored before there was one is linear.
    #[test]
    fn a_charts_price_axis_can_be_made_logarithmic_from_a_command() {
        let store = bare();
        run("chartbook create Macro --symbol SPY --switch", &store);
        let charts = |store: &Store| -> serde_json::Value {
            serde_json::from_str(&run("chart list --book Macro --json", store).out).unwrap()
        };
        assert_eq!(charts(&store)["charts"][0]["log_scale"], false, "linear to begin with");

        let log = run("chart set --book Macro --log-scale on", &store);
        assert_eq!(log.code, 0, "{}", log.err);
        assert!(log.out.contains("log scale on"), "{}", log.out);
        assert_eq!(charts(&store)["charts"][0]["log_scale"], true);
        assert_eq!(charts(&store)["charts"][0]["auto_scale"], true, "and still fits itself");

        run("chart set --book Macro --log-scale off", &store);
        assert_eq!(charts(&store)["charts"][0]["log_scale"], false);
    }

    /// A chart stored before the price scale was remembered says nothing
    /// about it, and is reported as fitting itself rather than as `null`.
    #[test]
    fn a_chart_stored_before_the_price_scale_was_remembered_is_reported_as_fitting_itself() {
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add rsi", &store);
        assert_eq!(out.code, super::super::EXIT_NO_WINDOW);
        assert!(out.err.contains("name one, or start Omacharts"), "{}", out.err);
    }

    #[test]
    fn an_indicator_carries_the_parameters_it_was_given() {
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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


    #[test]
    fn a_bollinger_takes_a_period_and_a_multiple() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add bollinger --book Macro", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("BB(20,2)"), "{}", out.out);

        let out = run(
            "chart indicator set bollinger --book Macro --period 10 --deviations 2.5 \
             --band-alpha 0.3",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let bollinger = &parsed["indicators"][0]["params"];
        assert_eq!(bollinger["period"], 10);
        assert_eq!(bollinger["bands"][0]["deviations"], 2.5);
        assert_eq!(bollinger["bands"][0]["fill_alpha"], 0.3);
        assert_eq!(bollinger["bands"][0]["enabled"], true, "a band is what it is for");
    }

    /// A VWAP's three bands and a Bollinger's one are the same thing, so the
    /// same flags reach both — and naming a band an indicator has not got is
    /// refused rather than quietly switching off the one it has.
    #[test]
    fn the_band_flags_reach_both_indicators_that_have_bands_and_nothing_else() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add bollinger --book Macro", &store);
        run("chart indicator add vwap --book Macro", &store);
        run("chart indicator add rsi --book Macro", &store);

        assert_eq!(run("chart indicator set vwap --book Macro --bands 1,3", &store).code, 0);
        assert_eq!(run("chart indicator set bollinger --book Macro --bands none", &store).code, 0);

        let refused = run("chart indicator set bollinger --book Macro --bands 2", &store);
        assert_eq!(refused.code, super::super::EXIT_USAGE);
        assert!(refused.err.contains("has 1 band"), "{}", refused.err);

        // One figure cannot say which of three bands it means.
        let refused = run("chart indicator set vwap --book Macro --deviations 2", &store);
        assert_eq!(refused.code, super::super::EXIT_USAGE);
        assert!(refused.err.contains("has 3 of them"), "{}", refused.err);

        let refused = run("chart indicator set rsi --book Macro --bands 1", &store);
        assert_eq!(refused.code, super::super::EXIT_USAGE);
        assert!(refused.err.contains("no bands"), "{}", refused.err);

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let vwap = &parsed["indicators"][1]["params"]["bands"];
        assert_eq!(vwap[0]["enabled"], true);
        assert_eq!(vwap[1]["enabled"], false);
        assert_eq!(vwap[2]["enabled"], true);
        assert_eq!(parsed["indicators"][0]["params"]["bands"][0]["enabled"], false);
    }

    #[test]
    fn a_macd_takes_three_lengths_and_a_colour_for_its_signal() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add macd --book Macro", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("MACD(12,26,9)"), "{}", out.out);

        let out = run(
            "chart indicator set macd --book Macro --fast 5 --slow 35 --signal 5 \
             --signal-color Amber --height 0.3",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let macd = &parsed["indicators"][0]["params"];
        assert_eq!(macd["fast"], 5);
        assert_eq!(macd["slow"], 35);
        assert_eq!(macd["signal"], 5);
        assert_eq!(macd["height"], 0.3);
        assert_eq!(macd["signal_color"]["name"], "Amber");

        run("chart indicator add rsi --book Macro", &store);
        assert_ne!(run("chart indicator set rsi --book Macro --fast 5", &store).code, 0);
        assert_ne!(run("chart indicator set rsi --book Macro --signal-color Amber", &store).code, 0);
    }

    /// A quick average no quicker than the slow one measures nothing, or
    /// measures it upside down. Either way the chart is left as it was.
    #[test]
    fn a_macd_whose_averages_are_the_wrong_way_round_is_refused() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add macd --book Macro", &store);

        for args in ["--fast 26", "--slow 12", "--fast 30 --slow 10", "--fast 9 --slow 9"] {
            let out = run(&format!("chart indicator set macd --book Macro {args}"), &store);
            assert_eq!(out.code, super::super::EXIT_USAGE, "{args}: {}", out.err);
            assert!(out.err.contains("not below"), "{args}: {}", out.err);
        }

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["indicators"][0]["params"]["fast"], 12, "nothing was written");
        assert_eq!(parsed["indicators"][0]["params"]["slow"], 26);

        // Moving both at once past where either was is fine, as it is for the
        // overbought and oversold pair: only the pair it ends up with is
        // judged.
        let out = run("chart indicator set macd --book Macro --fast 30 --slow 60", &store);
        assert_eq!(out.code, 0, "{}", out.err);
    }

    /// A length is a whole number of bars and at least one of them. A float
    /// cast to an unsigned integer saturates at nought, so `--period -5` used
    /// to be floored into a period of one and reported as having worked.
    #[test]
    fn a_length_that_is_not_a_number_of_bars_is_refused_rather_than_floored() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add bollinger --book Macro", &store);
        run("chart indicator add macd --book Macro", &store);
        run("chart indicator add sma --book Macro", &store);

        for (kind, flag) in [
            ("bollinger", "--period"),
            ("sma", "--period"),
            ("macd", "--fast"),
            ("macd", "--slow"),
            ("macd", "--signal"),
        ] {
            for value in ["0", "-5", "1.5", "lots"] {
                let out =
                    run(&format!("chart indicator set {kind} --book Macro {flag} {value}"), &store);
                assert_eq!(out.code, super::super::EXIT_USAGE, "{flag} {value}: {}", out.err);
                assert!(out.err.contains("whole number of bars"), "{flag} {value}: {}", out.err);
            }
        }

        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["indicators"][0]["params"]["period"], 20, "nothing was written");
        assert_eq!(parsed["indicators"][2]["params"]["period"], 50);
    }

    /// Nought deviations is a band on the line, which is useless and defined;
    /// a negative one and one past the engine's own end are neither.
    #[test]
    fn a_band_width_outside_what_a_band_can_be_is_refused() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        run("chart indicator add bollinger --book Macro", &store);

        assert_eq!(run("chart indicator set bollinger --book Macro --deviations 0", &store).code, 0);

        // A negative nought is the same band as a nought and is inside the
        // range, so it is not refused — but it is written down as it was
        // given, and the indicator's own name is built from what is written
        // down. "BB(20,-0)" is a chart labelled with a figure nobody typed.
        let out = run("chart indicator set bollinger --book Macro --deviations -0.0", &store);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("BB(20,0)"), "{}", out.out);
        let listed = run("chart indicator list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let stored = &parsed["indicators"][0]["params"]["bands"][0]["deviations"];
        assert_eq!(stored.as_f64().map(f64::is_sign_positive), Some(true), "{stored}");

        for value in ["-2", "51", "wide"] {
            let out =
                run(&format!("chart indicator set bollinger --book Macro --deviations {value}"), &store);
            assert_eq!(out.code, super::super::EXIT_USAGE, "{value}: {}", out.err);
        }
    }

    /// Quietly ignoring a parameter the indicator has no use for would read
    /// as the command having worked.
    #[test]
    fn a_parameter_an_indicator_has_no_use_for_is_refused() {
        let store = bare();
        run("chartbook create Macro --switch", &store);
        let out = run("chart indicator add volume --book Macro --period 50", &store);
        assert_eq!(out.code, super::super::EXIT_USAGE);
        assert!(out.err.contains("no period"), "{}", out.err);
    }

    #[test]
    fn an_indicator_already_on_a_chart_can_be_reconfigured() {
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
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
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        let out = run("chart indicator add rsi --book Macro", &store);
        assert!(out.out.contains("AAPL"), "{}", out.out);
        assert!(out.out.contains("Macro"), "{}", out.out);
    }

    #[test]
    fn status_says_whether_anything_is_actually_open() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        let out = run("status show --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&out.out).unwrap();
        assert_eq!(parsed["windowOpen"], false, "nothing is running in a test");
        assert_eq!(parsed["chartbook"], "Macro");
        assert_eq!(parsed["charts"][0]["target"], "pos:0", "spelled as the other verbs take it");
    }

    #[test]
    fn crosshair_syncing_can_be_read_and_set() {
        let store = bare();
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
    fn bars_can_turn_red_up_and_get_the_same_scheme_back() {
        let store = bare();
        run("config set bar_scheme hollow", &store);
        assert_eq!(run("config bars red-up", &store).code, 0);
        assert_eq!(run("config bars", &store).out.trim(), "red-up");
        assert_eq!(run("config get bar_scheme", &store).out.trim(), "theme-red-up");

        // From one of the two answers to the other, and back: the palette
        // remembered is still the one somebody picked.
        assert_eq!(run("config bars monochrome", &store).code, 0);
        assert_eq!(run("config bars coloured", &store).code, 0);
        assert_eq!(run("config get bar_scheme", &store).out.trim(), "theme-hollow");
    }

    #[test]
    fn bars_can_lose_their_colour_and_get_the_same_scheme_back() {
        let store = bare();
        run("config set bar_scheme hollow", &store);
        assert_eq!(run("config bars", &store).out.trim(), "coloured");

        assert_eq!(run("config bars monochrome", &store).code, 0);
        assert_eq!(run("config bars", &store).out.trim(), "monochrome");
        assert_eq!(run("config get bar_scheme", &store).out.trim(), "theme-mono");

        assert_eq!(run("config bars coloured", &store).code, 0);
        assert_eq!(
            run("config get bar_scheme", &store).out.trim(),
            "theme-hollow",
            "the scheme that was in use has to come back, not the default"
        );
    }

    #[test]
    fn a_chart_is_focused_by_its_position_as_well_as_by_its_id() {
        let store = bare();
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

    /// Becoming Follow leads the Follow charts. You made *this* chart follow,
    /// so what it is showing is the symbol you meant them to be on — and the
    /// chain and the canvas agree either way round, which is the state the
    /// window goes out of its way to keep.
    #[test]
    fn a_chart_made_follow_leads_the_others_onto_what_it_is_showing() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        // Split charts start on Follow; the second is taken out to join.
        run("chart set --book Macro --chart pos:1 --link focus", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link follow", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD", &store);

        let joined = run("chart set --book Macro --chart pos:1 --link follow", &store);
        assert_eq!(joined.code, 0, "{}", joined.err);
        assert!(joined.out.contains("AMD"), "it has to say what followed: {}", joined.out);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "AMD", "the chart already following follows");
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
    }

    /// A symbol named in the same command is what the chart ends up showing,
    /// so it is also what the Follow charts are led onto — read after the
    /// write rather than before it.
    #[test]
    fn a_symbol_named_alongside_follow_is_the_one_the_others_take() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        // Split charts start on Follow; the second is taken out to join.
        run("chart set --book Macro --chart pos:1 --link focus", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link follow", &store);

        run("chart set --book Macro --chart pos:1 --symbol AMD --link follow", &store);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
        assert_eq!(parsed["charts"][0]["symbol"], "AMD", "and the Follow charts take the new symbol");
    }

    /// Only an actual change leads. A radio menu makes re-picking the mode
    /// you are already in easy to do by accident, and a popover re-states its
    /// own state every time it opens — either of those quietly rewriting four
    /// charts would make the whole control unusable.
    #[test]
    fn re_stating_follow_on_a_follow_chart_moves_nobody() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        // Split charts start on Follow; the second is taken out to join.
        run("chart set --book Macro --chart pos:1 --link focus", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link follow", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD --link follow", &store);
        // Both on AMD now. Put one of them somewhere else by hand, then re-pick
        // the mode it has been in all along.
        run("chart set --book Macro --chart pos:0 --symbol NVDA", &store);

        let again = run("chart set --book Macro --chart pos:1 --link follow", &store);
        assert_eq!(again.code, 0, "{}", again.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA", "nothing was asked for, so nothing moved");
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
    }

    /// Leaving Follow changes nothing about the other Follow charts. They keep
    /// what they were showing, because leaving is a statement about one chart.
    #[test]
    fn leaving_follow_leaves_the_charts_still_following_alone() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        // Split charts start on Follow; the second is taken out to join.
        run("chart set --book Macro --chart pos:1 --link focus", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link follow", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD --link follow", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD", &store);

        let left = run("chart set --book Macro --chart pos:1 --link focus", &store);
        assert_eq!(left.code, 0, "{}", left.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "AMD", "the Follow chart keeps what it had");
        assert_eq!(parsed["charts"][1]["link"], "focus", "and this one is out of it");
    }

    /// A Pinned chart is not a Follow chart: leading the Follow charts passes
    /// it by, and so does a chart in Focus.
    #[test]
    fn leading_the_follow_charts_passes_pinned_and_focus_charts_by() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart split horizontal --book Macro --chart pos:1", &store);
        run("chart set --book Macro --chart pos:0 --symbol SPY --link pinned", &store);
        run("chart set --book Macro --chart pos:1 --symbol AMD --link focus", &store);

        run("chart set --book Macro --chart pos:2 --symbol TSM --link follow", &store);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "SPY");
        assert_eq!(parsed["charts"][0]["link"], "pinned");
        assert_eq!(parsed["charts"][1]["symbol"], "AMD");
    }

    /// Every mode a command sets is the one it reads back.
    #[test]
    fn a_chart_s_mode_can_be_set_and_read_back() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        for mode in ["follow", "pinned", "focus"] {
            let set = run(&format!("chart set --book Macro --chart pos:0 --link {mode}"), &store);
            assert_eq!(set.code, 0, "{}", set.err);
            assert!(set.out.contains(&format!("link {mode}")), "{}", set.out);
            let listed = run("chart list --book Macro", &store);
            assert!(listed.out.contains(mode), "{}", listed.out);
        }
        assert_ne!(run("chart set --book Macro --chart pos:0 --link 2", &store).code, 0);
    }

    /// A chart with nothing on it leads with nothing. Blanking the Follow
    /// charts because an empty chart joined them would be the one way this
    /// feature could destroy what somebody was looking at.
    #[test]
    fn a_chart_with_no_symbol_does_not_blank_the_follow_charts() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        run("chart set --book Macro --chart pos:0 --symbol NVDA --link follow", &store);
        // Nothing in the command surface can empty a chart, so this reaches
        // past it — which is the only way the stored arrangement gets into this
        // state, and it is reachable: `config set workspace` writes it whole.
        let mut workspace = charts::Workspace::load(&store);
        let book = workspace.book_mut(0).unwrap();
        charts::pane_mut(book, 2).unwrap()["symbol"] = json!("");
        workspace.save(&store);

        let joined = run("chart set --book Macro --chart pos:1 --link follow", &store);
        assert_eq!(joined.code, 0, "{}", joined.err);

        let listed = run("chart list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "NVDA", "the Follow chart keeps what it was showing");
    }

    /// Following the watchlist means this chartbook's list. A chartbook you
    /// come back to is as you left it, so becoming Follow in one book leaves
    /// the Follow charts of another alone.
    #[test]
    fn becoming_follow_leaves_the_charts_in_other_books_alone() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chartbook create Chips --symbol MU", &store);
        run("chart set --book Chips --chart pos:0 --link follow", &store);

        let joined = run("chart set --book Macro --chart pos:0 --symbol NVDA --link follow", &store);
        assert_eq!(joined.code, 0, "{}", joined.err);

        let listed = run("chart list --book Chips --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["charts"][0]["symbol"], "MU", "the book that is away kept its chart");
    }

    /// Which list the rail shows is written down against whichever book was
    /// saved last, so two books claiming one would fight over it. The window
    /// heals that on display; refusing it is better than relying on the heal.
    #[test]
    fn a_watchlist_one_chartbook_shows_is_not_handed_to_a_second() {
        let store = bare();
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
        let store = bare();
        run("chartbook create Chips --watchlist Default --switch", &store);
        run("chartbook create Macro --symbol AAPL", &store);
        assert_eq!(run("chartbook watchlist Macro Default", &store).code, 0);
    }

    /// `config set workspace` can put anything at all in the row the
    /// arrangement is read back out of, and a command that found something
    /// unexpected there used to abort the window rather than fail.
    #[test]
    fn an_arrangement_that_is_not_one_fails_the_command_and_nothing_else() {
        let store = bare();
        store.set_setting(
            "workspace",
            r#"{"books":[{"name":"Odd","layout":{"leaf":1},"focused":1}],"active":0}"#,
        );
        for line in ["chart split horizontal --book Odd", "chart close --book Odd", "chart list --book Odd"] {
            let outcome = run(line, &store);
            assert_ne!(outcome.code, super::super::EXIT_BUG, "{line}: {}", outcome.err);
        }
    }

    /// A caller whose home is a fresh directory of its own.
    ///
    /// For running commands as written: `plugin install` and `uninstall` edit
    /// `~/.config/omarchy`, and against the real home a test run took the
    /// widget out of the bar of whoever ran the suite.
    struct ScratchHome(std::path::PathBuf);

    impl ScratchHome {
        fn new() -> ScratchHome {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let at = std::env::temp_dir().join(format!(
                "omacharts-home-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            // Drop clears it, but a run that was killed never drops, and a pid
            // comes round again. A widget left there would make `install` an
            // update, and an update asks the live shell to reload its plugins.
            let _ = std::fs::remove_dir_all(&at);
            ScratchHome(at)
        }
    }

    impl Caller for ScratchHome {
        fn read(&self, path: &str) -> Result<String, Fault> {
            super::super::Here.read(path)
        }
        fn home(&self) -> std::path::PathBuf {
            self.0.clone()
        }
    }

    impl Drop for ScratchHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The widget goes in, and comes out, of the home the caller names and no
    /// other — which is what lets the examples run without reaching the bar
    /// of whoever is running them.
    #[test]
    fn the_plugin_is_installed_under_the_callers_home() {
        let home = ScratchHome::new();
        let config = home.0.join(".config/omarchy");
        std::fs::create_dir_all(config.join("plugins")).unwrap();
        std::fs::write(config.join("shell.json"), r#"{"bar": {"layout": {"right": []}}}"#)
            .unwrap();
        let store = bare();
        let plugin = |verb: &str| {
            let args: Vec<String> =
                ["omacharts", "plugin", verb].into_iter().map(String::from).collect();
            dispatch(&args, &store, None, &home)
        };
        let widget = crate::bar_plugin::location(&home.0);

        assert_eq!(plugin("install").code, super::super::EXIT_OK);
        assert!(widget.join("Service.qml").is_file(), "written under the scratch home");
        let shell = std::fs::read_to_string(config.join("shell.json")).unwrap();
        assert!(shell.contains(crate::bar_plugin::PLUGIN_ID), "{shell}");

        assert_eq!(plugin("uninstall").code, super::super::EXIT_OK);
        assert!(!widget.exists(), "and taken out of it again");
    }

    /// Every outcome of `uninstall --json` has the keys `install` and `status`
    /// answer with, `omarchy` among them, so a script reads one shape whether
    /// the widget was there, was already gone, or there is no bar at all.
    #[test]
    fn uninstall_json_has_the_same_keys_whatever_it_found() {
        let home = ScratchHome::new();
        let config = home.0.join(".config/omarchy");
        std::fs::create_dir_all(config.join("plugins")).unwrap();
        std::fs::write(config.join("shell.json"), r#"{"bar": {"layout": {"right": []}}}"#)
            .unwrap();
        let store = bare();
        let plugin = |verb: &str| {
            let args: Vec<String> =
                ["omacharts", "plugin", verb, "--json"].into_iter().map(String::from).collect();
            let out = dispatch(&args, &store, None, &home);
            assert_eq!(out.code, super::super::EXIT_OK, "{}", out.err);
            serde_json::from_str::<Value>(&out.out).unwrap()
        };

        plugin("install");
        assert_eq!(
            plugin("uninstall"),
            json!({"omarchy": true, "installed": false, "removed": true})
        );
        assert_eq!(
            plugin("uninstall"),
            json!({"omarchy": true, "installed": false, "removed": false})
        );
    }

    /// Having no Omarchy shell is a fact about the machine, and `uninstall`
    /// reports it the way `install` and `status` do — not as a shell.json it
    /// could not open. A home over ssh, or a machine that is not Omarchy at
    /// all, has none.
    #[test]
    fn uninstalling_where_there_is_no_bar_is_nothing_to_remove() {
        let home = ScratchHome::new();
        let store = bare();
        let uninstall = |extra: &[&str]| {
            let args: Vec<String> = ["omacharts", "plugin", "uninstall"]
                .into_iter()
                .chain(extra.iter().copied())
                .map(String::from)
                .collect();
            dispatch(&args, &store, None, &home)
        };

        let out = uninstall(&[]);
        assert_eq!(out.code, super::super::EXIT_OK, "{}", out.err);
        assert_eq!(out.out, "no Omarchy bar on this desktop, so there was nothing to remove\n");

        let out = uninstall(&["--json"]);
        assert_eq!(out.code, super::super::EXIT_OK, "{}", out.err);
        let said: Value = serde_json::from_str(&out.out).unwrap();
        assert_eq!(said, json!({"omarchy": false, "installed": false, "removed": false}));
    }

    /// A widget an older version put in the bar is already in the bar, so
    /// `install` brings it up to date rather than installing it — which is
    /// what it says, and what it does: moved, with its settings, not added
    /// beside itself.
    #[test]
    fn installing_over_an_old_install_is_an_update() {
        let home = ScratchHome::new();
        let config = home.0.join(".config/omarchy");
        let old = config.join("plugins").join(crate::bar_plugin::OLD_PLUGIN_ID);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("manifest.json"), r#"{"id": "jorgemanrubia.omacharts"}"#).unwrap();
        std::fs::write(
            config.join("shell.json"),
            json!({ "bar": { "layout": { "right": [
                { "id": "omarchy.tray" },
                { "id": crate::bar_plugin::OLD_PLUGIN_ID, "refreshIntervalSec": 300 },
            ] } } })
            .to_string(),
        )
        .unwrap();
        let store = bare();
        let args: Vec<String> =
            ["omacharts", "plugin", "install", "--json"].into_iter().map(String::from).collect();

        let out = dispatch(&args, &store, None, &home);
        assert_eq!(out.code, super::super::EXIT_OK, "{}", out.err);
        let said: Value = serde_json::from_str(&out.out).unwrap();
        assert_eq!(said["updated"], true, "{said}");
        assert!(!old.exists(), "moved");
        let shell: Value =
            serde_json::from_str(&std::fs::read_to_string(config.join("shell.json")).unwrap())
                .unwrap();
        assert_eq!(
            shell["bar"]["layout"]["right"],
            json!([
                { "id": "omarchy.tray" },
                { "id": crate::bar_plugin::PLUGIN_ID, "refreshIntervalSec": 300 },
            ]),
            "renamed in place, not added a second time"
        );
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
        fn warm_closes(&self, _batches: Vec<Vec<crate::loader::Listing>>) {}

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
        // `plugin install` and `uninstall` write under the caller's home, and
        // `skill install` is pointed into it below.
        let home = ScratchHome::new();
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
                    args.push("--to".to_string());
                    args.push(home.0.join("skills").join(verb.name).to_string_lossy().into_owned());
                }
                let outcome = dispatch(&args, &store, Some(&NoWindow), &home);
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
        let store = bare();
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
        assert_eq!(parsed["drawings"][0]["scope"], "shared");
        assert_eq!(parsed["drawings"][0]["from"]["price"], 180.5);
        assert_eq!(parsed["drawings"][0]["from"]["when"], "2026-09-01");

        let on_nvda = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&on_nvda.out).unwrap();
        assert!(parsed["drawings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_drawing_can_be_moved_restyled_and_removed() {
        let store = bare();
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

    /// Turning sending off takes back what this chart sent, and turning it
    /// on sends back what it kept — its own either way, never another
    /// chart's.
    #[test]
    fn sending_takes_back_only_this_chart_s_own() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split vertical --book Macro", &store);
        run("chart set --book Macro --chart pos:1 --symbol AAPL", &store);

        run("chart drawing add line --book Macro --chart pos:0 --from 2026-09-01,100 --to 2026-09-19,110", &store);
        run("chart drawing add rect --book Macro --chart pos:1 --from 2026-09-01,120 --to 2026-09-19,130", &store);

        let seen = |chart: &str| -> Vec<String> {
            let out = run(&format!("chart drawing list --book Macro --chart {chart} --json"), &store);
            let parsed: serde_json::Value = serde_json::from_str(&out.out).unwrap();
            parsed["drawings"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["kind"].as_str().unwrap().to_string())
                .collect()
        };

        // Both send, so both see both.
        assert_eq!(seen("pos:0"), vec!["line", "rect"]);
        assert_eq!(seen("pos:1"), vec!["line", "rect"]);

        // The second stops sending: it keeps its rectangle and the first
        // stops seeing it, while the first's line is untouched.
        run("chart set --book Macro --chart pos:1 --send-drawings off", &store);
        assert_eq!(seen("pos:0"), vec!["line"], "the first chart kept seeing what the second took back");
        assert_eq!(seen("pos:1"), vec!["line", "rect"], "the second lost its own");

        // And sending again puts it back.
        run("chart set --book Macro --chart pos:1 --send-drawings on", &store);
        assert_eq!(seen("pos:0"), vec!["line", "rect"]);
        assert_eq!(seen("pos:1"), vec!["line", "rect"]);
    }

    /// The two switches decide what a chart sees and where what is drawn
    /// on it goes; a drawing that is not sent lives with the chart and has
    /// a negative id.
    #[test]
    fn the_two_switches_decide_what_a_chart_sees_and_sends() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split vertical --book Macro", &store);
        run("chart set --book Macro --chart pos:1 --symbol AAPL --send-drawings off", &store);

        run("chart drawing add line --book Macro --chart pos:0 --from 2026-09-01,100 --to 2026-09-19,110", &store);
        run("chart drawing add line --book Macro --chart pos:1 --from 2026-09-01,120 --to 2026-09-19,130", &store);

        // The first chart sends, so its line is the symbol's; the second
        // does not, so its line is its own.
        let sending = run("chart drawing list --book Macro --chart pos:0 --json", &store);
        let sending: serde_json::Value = serde_json::from_str(&sending.out).unwrap();
        assert_eq!(sending["shows_drawings"], true);
        assert_eq!(sending["sends_drawings"], true);
        let scopes: Vec<&str> =
            sending["drawings"].as_array().unwrap().iter().map(|d| d["scope"].as_str().unwrap()).collect();
        assert_eq!(scopes, vec!["shared"], "a chart sees the symbol's, not another chart's own");

        // The second sees the symbol's as well as its own, since it still
        // shows what others send.
        let keeping = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let keeping: serde_json::Value = serde_json::from_str(&keeping.out).unwrap();
        assert_eq!(keeping["sends_drawings"], false);
        let mut scopes: Vec<&str> =
            keeping["drawings"].as_array().unwrap().iter().map(|d| d["scope"].as_str().unwrap()).collect();
        scopes.sort();
        assert_eq!(scopes, vec!["local", "shared"]);
        let local =
            keeping["drawings"].as_array().unwrap().iter().find(|d| d["scope"] == "local").unwrap();
        assert!(local["id"].as_i64().unwrap() < 0, "a drawing a chart kept is numbered below the store's");

        // Stop showing what others send and only its own is left.
        run("chart set --book Macro --chart pos:1 --show-drawings off", &store);
        let alone = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let alone: serde_json::Value = serde_json::from_str(&alone.out).unwrap();
        assert_eq!(alone["shows_drawings"], false);
        let scopes: Vec<&str> =
            alone["drawings"].as_array().unwrap().iter().map(|d| d["scope"].as_str().unwrap()).collect();
        assert_eq!(scopes, vec!["local"]);

        // A drawing a chart kept is removed through it, like any other.
        let id = local["id"].as_i64().unwrap();
        let out = run(&format!("chart drawing remove --book Macro --chart pos:1 --id {id}"), &store);
        assert_eq!(out.code, 0, "{}", out.err);
        let alone = run("chart drawing list --book Macro --chart pos:1 --json", &store);
        let alone: serde_json::Value = serde_json::from_str(&alone.out).unwrap();
        assert!(alone["drawings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn a_configuration_moves_the_drawings_that_follow_it() {
        let store = bare();
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

    /// A zig-zag is the one kind that is not two anchors, so it is the one
    /// kind `--from` and `--to` cannot describe.
    #[test]
    fn a_zigzag_is_drawn_through_its_corners() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        let out = run(
            "chart drawing add zigzag --book Macro --point 2026-09-01,100 --point 2026-09-05,120 --point 2026-09-09,110",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);

        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let corners = parsed["drawings"][0]["corners"].as_array().unwrap();
        assert_eq!(corners.len(), 3);
        assert_eq!(corners[1]["price"], 120.0);
        // Its ends are the first and the last of them, which is what
        // everything that only wants a drawing's span reads.
        assert_eq!(parsed["drawings"][0]["from"]["price"], 100.0);
        assert_eq!(parsed["drawings"][0]["to"]["price"], 110.0);

        let out = run(
            "chart drawing set --book Macro --id 1 --point 2026-09-01,100 --point 2026-09-05,130 --point 2026-09-09,105 --point 2026-09-12,140",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["drawings"][0]["corners"].as_array().unwrap().len(), 4);
        assert_eq!(parsed["drawings"][0]["to"]["price"], 140.0);

        // A kind with no corners of its own says so rather than dropping
        // them, and a single corner is not a drawing.
        for bad in [
            "chart drawing add rect --book Macro --point 2026-09-01,100 --point 2026-09-05,120",
            "chart drawing add zigzag --book Macro --point 2026-09-01,100",
            "chart drawing set --book Macro --id 1 --point oops,100 --point 2026-09-05,120",
        ] {
            let out = run(bad, &store);
            assert_eq!(out.code, super::super::EXIT_USAGE, "{bad}: {}", out.out);
        }
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert_eq!(parsed["drawings"].as_array().unwrap().len(), 1, "nothing else was drawn");
        assert_eq!(parsed["drawings"][0]["corners"].as_array().unwrap().len(), 4, "and nothing moved");
        assert!(parsed["drawings"][0]["corners"].is_array());

        // Every other kind has none, rather than a copy of its two ends.
        run("chart drawing add line --book Macro --from 2026-09-01,100 --to 2026-09-19,110", &store);
        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        assert!(parsed["drawings"][1]["corners"].is_null(), "{}", listed.out);
    }

    /// Emphasis has to arrive as characters, because a terminal has no
    /// Ctrl+B — and it goes back out the same way, so a label can be read off
    /// one drawing and put on another without going through the runs by hand.
    #[test]
    fn a_label_keeps_its_bold_through_a_terminal() {
        let store = bare();
        run("chartbook create Macro --symbol AAPL --switch", &store);
        let out = run(
            "chart drawing add rect --book Macro --from 2026-09-01,180 --to 2026-09-19,192 --text buy-**the**-dip --text-at top",
            &store,
        );
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(out.out.contains("buy-**the**-dip"), "{}", out.out);

        let listed = run("chart drawing list --book Macro --json", &store);
        let parsed: serde_json::Value = serde_json::from_str(&listed.out).unwrap();
        let text = &parsed["drawings"][0]["text"];
        assert_eq!(text["words"], "buy-the-dip", "the characters, with the asterisks gone");
        assert_eq!(text["markup"], "buy-**the**-dip");
        assert_eq!(text["at"], "top");
        let spans = text["spans"].as_array().unwrap();
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[1]["text"], "the");
        assert_eq!(spans[1]["bold"], true);
        assert_eq!(spans[1]["italic"], false);

        // What is in a configuration is how words are set, never which
        // words, so a label is not carried off onto one.
        let out = run("chart drawing configure --config 1 rect --text nope", &store);
        assert_eq!(out.code, super::super::EXIT_USAGE, "{}", out.out);
    }

    /// One bad value refuses the whole command, and names what was wrong.
    #[test]
    fn a_bad_anchor_or_colour_leaves_the_drawings_as_they_were() {
        let store = bare();
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
        let store = bare();
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

    /// A command line split the way a shell splits it: on spaces, except
    /// inside quotes.
    ///
    /// `split_whitespace` was enough while every value was one word, and
    /// stopped being enough the moment a drawing could carry a label —
    /// `--text "gap fills here"` is three arguments to a naive splitter and
    /// fails as one. What a skill shows has to be what somebody can paste,
    /// so the test reads it the way their shell will.
    fn words_of(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quote: Option<char> = None;
        let mut started = false;
        for c in line.chars() {
            match (quote, c) {
                (Some(open), c) if c == open => quote = None,
                (Some(_), c) => word.push(c),
                (None, '"') | (None, '\'') => {
                    quote = Some(c);
                    // An empty pair of quotes is an empty argument, which is
                    // how a label is taken off again.
                    started = true;
                }
                (None, c) if c.is_whitespace() => {
                    if started || !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                        started = false;
                    }
                }
                (None, c) => word.push(c),
            }
        }
        if started || !word.is_empty() {
            words.push(word);
        }
        words
    }

    #[test]
    fn a_quoted_value_is_one_argument() {
        assert_eq!(words_of("a b"), vec!["a", "b"]);
        assert_eq!(words_of("--text \"two words\""), vec!["--text", "two words"]);
        assert_eq!(words_of("--text \"\""), vec!["--text", ""]);
        assert_eq!(words_of("  spaced   out  "), vec!["spaced", "out"]);
        assert_eq!(words_of("--text 'single quoted'"), vec!["--text", "single quoted"]);
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

        // A workflow that ever puts the widget in the bar does it here.
        let home = ScratchHome::new();
        for block in &blocks {
            // One store per block. Each is a recipe somebody starts from
            // wherever they are, not a continuation of the one above it.
            let store = skill_seed();
            for line in block {
                let args: Vec<String> = std::iter::once("omacharts".to_string())
                    .chain(words_of(line))
                    .collect();
                let outcome = dispatch(&args, &store, Some(&NoWindow), &home);
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
        let store = bare();
        run("watchlist create Semis", &store);
        run("watchlist add Semis NVDA AMD AVGO TSM MU", &store);
        run("chartbook create Macro --symbol AAPL --switch", &store);
        run("chart split horizontal --book Macro", &store);
        store
    }

    /// A window that writes down what it was asked to do.
    #[derive(Default)]
    struct Watching(std::cell::RefCell<Vec<&'static str>>);

    impl Live for Watching {
        fn flush_workspace(&self) {
            self.0.borrow_mut().push("flush");
        }
        fn reload_workspace(&self) {
            self.0.borrow_mut().push("workspace");
        }
        fn reload_watchlists(&self) {
            self.0.borrow_mut().push("watchlists");
        }
        fn adopt_theming(&self) {
            self.0.borrow_mut().push("theming");
        }
        fn adopt_feed(&self) {
            self.0.borrow_mut().push("feed");
        }
        fn warm(&self, _instruments: &[omacharts_engine::Instrument]) {}
        fn warm_closes(&self, _batches: Vec<Vec<crate::loader::Listing>>) {}
        fn symbols(&self) -> Option<std::rc::Rc<omacharts_engine::SearchIndex>> {
            None
        }
        fn screenshot(
            &self,
            _whole_book: bool,
            _into: Option<&std::path::Path>,
            _clipboard: bool,
        ) -> Result<(String, std::path::PathBuf), Fault> {
            Err(Fault::new(crate::cli::EXIT_ERROR, "nothing on screen".to_string()))
        }
    }

    /// A restore typed at a terminal while the window is open lands in the
    /// window at once: what is on screen is saved first, so it is in the
    /// backup the restore keeps, and everything read out of the settings is
    /// read again afterwards.
    #[test]
    fn a_restore_reaches_the_window_that_is_open() {
        let dir = std::env::temp_dir().join(format!("omacharts-backup-cli-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_at(&dir.join("omacharts.db")).unwrap();

        assert_eq!(run("backup create", &store).code, 0);
        let listed: Value = serde_json::from_str(&run("backup list --json", &store).out).unwrap();
        let name = listed["backups"][0]["name"].as_str().unwrap().to_string();
        assert_eq!(listed["backups"][0]["schema"], json!(crate::migrations::LATEST));
        // Out of the way of the backup the restore takes, which would
        // otherwise land in the same second and replace it.
        let backups = dir.join("backups");
        std::fs::rename(backups.join(&name), backups.join("settings-20000101-000000.db")).unwrap();
        run("watchlist create Later", &store);

        let window = Watching::default();
        let args: Vec<String> = ["omacharts", "backup", "restore", "settings-20000101-000000"]
            .map(String::from)
            .to_vec();
        let outcome = dispatch(&args, &store, Some(&window), &super::super::Here);
        assert_eq!(outcome.code, 0, "{}", outcome.err);
        assert!(outcome.out.contains("what they replaced is kept as settings-"), "{}", outcome.out);
        assert_eq!(*window.0.borrow(), vec!["flush", "theming", "feed", "watchlists", "workspace"]);
        assert!(!run("watchlist list", &store).out.contains("Later"));

        let unknown = run("backup restore settings-nope", &store);
        assert_eq!(unknown.code, crate::cli::EXIT_ERROR);
        assert!(unknown.err.contains("backup list"), "{}", unknown.err);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
