//! Watchlists as a file, for carrying them from one machine to another.
//!
//! `watchlist export` writes every list — sections, order, symbols and the
//! link group each drives — and `watchlist import` brings them in on the
//! other side. Importing adds what is missing and touches nothing else, so
//! the same file can be imported again and again, on every machine, as the
//! lists grow: a second run changes nothing. `--replace` is the one way to
//! make a list match the file exactly, and even that never removes a list the
//! file does not name.
//!
//! The file is the lists and nothing about this machine: no row ids, which
//! mean nothing anywhere else, and the default list marked as the default
//! rather than by name, because it can be renamed.

use std::collections::{HashMap, HashSet};

use omacharts_engine::{link, LinkGroup};
use serde::{Deserialize, Serialize};

use super::exec::link_setting;
use super::Fault;
use crate::store::{Entry, Store, DEFAULT_WATCHLIST};

/// What the file says it is, so a chartbook or a config file handed to
/// `import` by mistake is refused rather than read as an empty export.
const KIND: &str = "watchlists";
/// Raised when the format changes in a way an older version cannot read.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct Export {
    pub omacharts: String,
    pub version: u32,
    pub watchlists: Vec<List>,
}

#[derive(Serialize, Deserialize)]
pub struct List {
    pub name: String,
    /// The list every install has. Matched to that one whatever either side
    /// calls it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default: bool,
    /// The link group it drives, 0 for none.
    #[serde(default)]
    pub link: u8,
    pub sections: Vec<Part>,
}

/// A section. The nameless one holds what is in no section.
#[derive(Serialize, Deserialize)]
pub struct Part {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    pub symbols: Vec<Symbol>,
}

/// A ticker, written as just the ticker unless it has a venue suffix, which
/// is how nearly all of them are.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
pub enum Symbol {
    Plain(String),
    Listed { symbol: String, suffix: String },
}

fn is_false(value: &bool) -> bool {
    !value
}

impl Symbol {
    fn of(entry: &Entry) -> Symbol {
        match &entry.suffix {
            Some(suffix) => Symbol::Listed { symbol: entry.symbol.clone(), suffix: suffix.clone() },
            None => Symbol::Plain(entry.symbol.clone()),
        }
    }

    fn entry(&self) -> Entry {
        let tidy = |text: &str| text.trim().to_uppercase();
        match self {
            Symbol::Plain(symbol) => Entry { symbol: tidy(symbol), suffix: None },
            Symbol::Listed { symbol, suffix } => Entry {
                symbol: tidy(symbol),
                suffix: Some(tidy(suffix)).filter(|s| !s.is_empty()),
            },
        }
    }
}

/// These watchlists, as the file `export` writes.
pub fn export(store: &Store, lists: &[(i64, String)]) -> Export {
    // Who drives what, read once: asking per list rescans every list each time.
    let links: HashMap<i64, u8> = crate::ui::watchlist::group_owners(store)
        .into_iter()
        .map(|(group, id, _)| (id, group))
        .collect();
    let watchlists = lists
        .iter()
        .map(|(id, name)| List {
            name: name.clone(),
            default: *id == DEFAULT_WATCHLIST,
            link: links.get(id).copied().unwrap_or(0),
            sections: store
                .watchlist_sections(*id)
                .iter()
                .map(|section| Part {
                    name: section.name.clone(),
                    collapsed: section.collapsed,
                    symbols: section.entries.iter().map(Symbol::of).collect(),
                })
                .collect(),
        })
        .collect();
    Export { omacharts: KIND.to_string(), version: VERSION, watchlists }
}

/// Read a file `export` wrote, refusing anything that is not one before a
/// single row is touched.
pub fn parse(text: &str) -> Result<Export, Fault> {
    let file: Export = serde_json::from_str(text)
        .map_err(|error| Fault::usage(format!("not a watchlist export: {error}")))?;
    if file.omacharts != KIND {
        return Err(Fault::usage(format!(
            "this is an omacharts {:?} file, not a watchlist export",
            file.omacharts
        )));
    }
    if file.version > VERSION {
        return Err(Fault::usage(format!(
            "this export was made by a newer Omacharts (format {}); update this one first",
            file.version
        )));
    }
    let mut names = HashSet::new();
    for list in &file.watchlists {
        let name = list.name.trim();
        if name.is_empty() {
            return Err(Fault::usage("a watchlist in the file has no name".into()));
        }
        if !names.insert(name.to_lowercase()) {
            return Err(Fault::usage(format!("the file has two watchlists called {name:?}")));
        }
        if list.link > link::GROUP_COUNT {
            return Err(Fault::usage(format!("{name:?} drives link group {}, which is not one", list.link)));
        }
        if list.sections.iter().flat_map(|part| &part.symbols).any(|s| s.entry().symbol.is_empty()) {
            return Err(Fault::usage(format!("{name:?} has an empty symbol")));
        }
    }
    if file.watchlists.iter().filter(|list| list.default).count() > 1 {
        return Err(Fault::usage("the file marks more than one watchlist as the default".into()));
    }
    Ok(file)
}

/// What an import did to one watchlist.
#[derive(Clone, Copy, PartialEq)]
pub enum Action {
    Created,
    Merged,
    Replaced,
    Unchanged,
}

impl Action {
    pub fn key(self) -> &'static str {
        match self {
            Action::Created => "created",
            Action::Merged => "merged",
            Action::Replaced => "replaced",
            Action::Unchanged => "unchanged",
        }
    }
}

/// What importing did to one watchlist.
pub struct Imported {
    pub name: String,
    pub action: Action,
    pub symbols: usize,
    pub sections: usize,
    /// A link group the file asked for and another list already drives.
    pub link_kept_by: Option<(u8, String)>,
}

impl Imported {
    pub fn describe(&self) -> String {
        let counted = |n: usize, one: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {one}s"),
        };
        let (name, symbols) = (&self.name, counted(self.symbols, "symbol"));
        let mut text = match self.action {
            Action::Unchanged => format!("{name}: already up to date"),
            Action::Merged if self.sections == 0 => format!("{name}: added {symbols}"),
            Action::Merged => {
                format!("{name}: added {symbols} and {}", counted(self.sections, "section"))
            }
            action => format!(
                "{name}: {}, {symbols} in {}",
                action.key(),
                counted(self.sections, "section")
            ),
        };
        if let Some((group, holder)) = &self.link_kept_by {
            text.push_str(&format!(" (link group {group} stays with {holder:?})"));
        }
        text
    }
}

/// Bring the file's watchlists in, all of them or none.
///
/// Each is matched to a local list by name, or to the default list when the
/// file says it is the default. A match gains whatever sections and symbols it
/// lacks, at the end, and nothing it already has is moved; with `replace` it
/// becomes exactly what the file says instead. A list with no match is
/// created.
pub fn import(store: &Store, file: &Export, replace: bool) -> Result<Vec<Imported>, Fault> {
    // Every list is matched before anything is written, so a name that is
    // ambiguous here stops the import rather than half of it.
    let local = store.watchlists();
    let mut targets: Vec<Option<&(i64, String)>> = Vec::new();
    for list in &file.watchlists {
        let wanted = list.name.trim().to_lowercase();
        let named: Vec<&(i64, String)> =
            local.iter().filter(|(_, name)| name.to_lowercase() == wanted).collect();
        let target = match (list.default, named.as_slice()) {
            (true, _) => local.iter().find(|(id, _)| *id == DEFAULT_WATCHLIST),
            (false, []) => None,
            (false, [one]) => Some(*one),
            (false, _) => {
                return Err(Fault::ambiguous(format!(
                    "{} watchlists here are called {:?}; rename one before importing",
                    named.len(),
                    list.name.trim()
                )));
            }
        };
        if let Some((_, here)) = target.filter(|_| targets.contains(&target)) {
            return Err(Fault::usage(format!(
                "two watchlists in the file would both go into {here:?}"
            )));
        }
        targets.push(target);
    }

    store
        .atomically(|| {
            file.watchlists
                .iter()
                .zip(targets)
                .map(|(list, target)| bring_in(store, list, target, replace))
                .collect()
        })
        .ok_or_else(|| {
            Fault::new(super::EXIT_ERROR, "nothing was imported: the database refused a write".into())
        })
}

/// One list, into `target` when it matched one here and into a new list
/// otherwise. `None` is a write the store refused, which rolls the whole
/// import back.
fn bring_in(
    store: &Store,
    list: &List,
    target: Option<&(i64, String)>,
    replace: bool,
) -> Option<Imported> {
    // A list that matched keeps its own name: the file's name was there to
    // match on, and the default matched by being the default. So the default
    // list stays what it is called here, whatever it was renamed to there.
    let (id, name, action) = match target {
        None => {
            let name = list.name.trim().to_string();
            (store.add_watchlist(&name)?, name, Action::Created)
        }
        Some((id, name)) if replace => {
            clear(store, *id);
            (*id, name.clone(), Action::Replaced)
        }
        Some((id, name)) => (*id, name.clone(), Action::Merged),
    };

    // What the list held before, in any section: a symbol already here is not
    // added again somewhere else, so a merge never duplicates one that was
    // moved on the other machine. The file's own symbols are taken as they
    // are, because a list may hold one symbol in two sections on purpose.
    let here = store.watchlist_sections(id);
    let before: HashSet<&Entry> = here.iter().flat_map(|s| &s.entries).collect();
    let mut named: HashMap<String, i64> =
        here.iter().filter(|s| !s.root).map(|s| (s.name.to_lowercase(), s.id)).collect();
    let (mut symbols, mut sections) = (0, 0);
    for part in &list.sections {
        let section = match part.name.trim() {
            "" => store.root_section(id),
            wanted => match named.get(&wanted.to_lowercase()) {
                Some(existing) => *existing,
                None => {
                    let made = store.add_section(id, wanted)?;
                    store.set_section_collapsed(made, part.collapsed);
                    named.insert(wanted.to_lowercase(), made);
                    sections += 1;
                    made
                }
            },
        };
        for symbol in &part.symbols {
            let entry = symbol.entry();
            if !before.contains(&entry) {
                store.add_to_section(section, &entry.symbol, entry.suffix.as_deref());
                symbols += 1;
            }
        }
    }

    // A group is the file's to set only on a list it is writing whole. On a
    // merge the list keeps whatever group it drives here.
    let mut link_kept_by = None;
    if action != Action::Merged {
        match crate::ui::watchlist::group_held_by(store, LinkGroup::numbered(list.link), id) {
            Some((_, holder)) => link_kept_by = Some((list.link, holder)),
            None if list.link > 0 || action == Action::Replaced => {
                store.set_setting(&link_setting(id), &list.link.to_string());
            }
            None => {}
        }
    }

    let action = match (action, symbols + sections) {
        (Action::Merged, 0) => Action::Unchanged,
        (action, _) => action,
    };
    Some(Imported { name, action, symbols, sections, link_kept_by })
}

/// Empty a watchlist, keeping the list itself and its nameless section.
fn clear(store: &Store, id: i64) {
    for section in store.watchlist_sections(id) {
        match section.root {
            true => {
                for entry in &section.entries {
                    store.remove_from_section(section.id, &entry.symbol, entry.suffix.as_deref());
                }
            }
            false => store.remove_section(section.id),
        }
    }
}
