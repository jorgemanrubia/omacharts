//! Chartbook templates: a chartbook's arrangement, saved under a name for
//! other chartbooks to take.
//!
//! What a template holds is the shape of a book and what each chart in it
//! draws — the tree of splits, every divider as a share of its split rather
//! than in pixels, so it fits a window of any size, and for each chart its
//! resolution and either its chart template or its own indicators. What is on
//! the charts is not in it: a chartbook keeps its own symbols and link modes,
//! and taking a template gives them to the charts by where they sit.
//!
//! A chartbook remembers the template it was made from or saved as, and stays
//! on it. A change to its arrangement goes into the template and from there to
//! every other book on it, the way a change to a chart template reaches every
//! chart on it — see [`carry`].
//!
//! Kept as one JSON setting, for the reasons [`crate::templates`] gives: no
//! schema step, read whole, carried by a backup. The ids are handed out once,
//! so a book left naming a template that has gone cannot start following a
//! new one that happened to get its number.
//!
//! Everything here that touches a chartbook works on it as stored — as
//! [`serde_json::Value`] — because a command edits the arrangement that way
//! and must not drop a field it was not taught about. The window turns its
//! own books into values for the moment it needs these, so there is one way
//! of applying a template rather than two that can disagree.

use std::collections::HashMap;

use omacharts_engine::Indicator;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::store::Store;
use crate::templates::Template;
use crate::ui::pane::Node;

/// Where every chartbook template is kept.
pub const SETTING: &str = "chartbook_templates";

/// The field on a stored chartbook naming the template it is on.
pub const BOOK_FIELD: &str = "book_template";

/// What a chartbook template holds: the arrangement, and what each chart in
/// it draws. Leaves of the tree are positions in `panes`, counting from 0 in
/// layout order.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Shape {
    pub layout: Node,
    #[serde(default)]
    pub panes: Vec<Slot>,
}

/// One chart's place in a template: its resolution and what it draws.
///
/// A chart on a chart template is written down with the template's id and a
/// copy of what it held, so a book taking this after the chart template has
/// been deleted still gets the indicators it was saved with.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Slot {
    #[serde(default)]
    pub template: Option<u32>,
    #[serde(default)]
    pub indicators: Vec<Indicator>,
    /// The resolution, as a chart stores it. Missing from a template saved
    /// before resolutions were part of one, which leaves each chart's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeframe: Option<String>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct BookTemplate {
    pub id: u32,
    pub name: String,
    #[serde(flatten)]
    pub shape: Shape,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    next_id: u32,
    #[serde(default)]
    templates: Vec<BookTemplate>,
}

fn read(store: &Store) -> Stored {
    let mut stored: Stored = store
        .setting(SETTING)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let highest = stored.templates.iter().map(|t| t.id).max().unwrap_or(0);
    stored.next_id = stored.next_id.max(highest.saturating_add(1));
    stored
}

fn write(store: &Store, stored: &Stored) {
    if let Ok(json) = serde_json::to_string(stored) {
        store.set_setting(SETTING, &json);
    }
}

/// Every chartbook template, in the order they were made.
pub fn all(store: &Store) -> Vec<BookTemplate> {
    read(store).templates
}

pub fn get(store: &Store, id: u32) -> Option<BookTemplate> {
    all(store).into_iter().find(|t| t.id == id)
}

pub fn create(store: &Store, name: &str, shape: Shape) -> BookTemplate {
    let mut stored = read(store);
    let template = BookTemplate { id: stored.next_id, name: name.to_string(), shape };
    stored.next_id = stored.next_id.saturating_add(1);
    stored.templates.push(template.clone());
    write(store, &stored);
    template
}

/// Put a new arrangement in a template that exists. `false` when it does not.
pub fn replace(store: &Store, id: u32, shape: Shape) -> bool {
    change(store, id, move |t| t.shape = shape.clone())
}

/// `false` when there is no template with that id.
pub fn rename(store: &Store, id: u32, name: &str) -> bool {
    change(store, id, |t| t.name = name.to_string())
}

fn change(store: &Store, id: u32, edit: impl Fn(&mut BookTemplate)) -> bool {
    let mut stored = read(store);
    let Some(found) = stored.templates.iter_mut().find(|t| t.id == id) else { return false };
    edit(found);
    write(store, &stored);
    true
}

/// Take a template out. The books on it are the caller's to let go of, with
/// [`unlink`]: they live in the arrangement.
pub fn delete(store: &Store, id: u32) -> Option<BookTemplate> {
    let mut stored = read(store);
    let at = stored.templates.iter().position(|t| t.id == id)?;
    let gone = stored.templates.remove(at);
    write(store, &stored);
    Some(gone)
}

/// The template a selector names: a name, case-insensitive, or `id:N`.
pub fn find<'a>(templates: &'a [BookTemplate], selector: &str) -> Lookup<'a> {
    if let Some(rest) = selector.strip_prefix("id:") {
        return match rest.parse::<u32>().ok().and_then(|id| templates.iter().find(|t| t.id == id)) {
            Some(found) => Lookup::Found(found),
            None => Lookup::Missing,
        };
    }
    let wanted = selector.trim().to_lowercase();
    let hits: Vec<&BookTemplate> =
        templates.iter().filter(|t| t.name.to_lowercase() == wanted).collect();
    match hits.as_slice() {
        [one] => Lookup::Found(one),
        [] => Lookup::Missing,
        many => Lookup::Ambiguous(many.iter().map(|t| t.id).collect()),
    }
}

/// Why `name` cannot be a chartbook template's, or `None` when it can: a
/// name another template has, in any case, would make a selector name two,
/// and "none" or `id:N` already mean something to a selector. `except` is the
/// template being renamed, which may keep its own name.
pub fn refused_name(templates: &[BookTemplate], name: &str, except: Option<u32>) -> Option<String> {
    let wanted = name.trim().to_lowercase();
    if wanted.is_empty() {
        return Some("a chartbook template needs a name".into());
    }
    if wanted == crate::chart_defaults::NO_TEMPLATE.to_lowercase() || wanted.starts_with("id:") {
        return Some(format!("{:?} is how a chartbook template is picked, not a name for one", name.trim()));
    }
    templates
        .iter()
        .find(|t| Some(t.id) != except && t.name.trim().to_lowercase() == wanted)
        .map(|t| format!("there is already a chartbook template called {:?}", t.name))
}

pub enum Lookup<'a> {
    Found(&'a BookTemplate),
    Missing,
    /// Two or more share the name; these are their ids.
    Ambiguous(Vec<u32>),
}

// -- chartbooks, as stored --------------------------------------------------

/// The template a stored chartbook is on, if it is on one.
pub fn linked(book: &Value) -> Option<u32> {
    book.get(BOOK_FIELD).and_then(Value::as_u64).map(|id| id as u32)
}

pub fn set_link(book: &mut Value, template: Option<u32>) {
    match (template, book.as_object_mut()) {
        (Some(id), Some(fields)) => {
            fields.insert(BOOK_FIELD.to_string(), json!(id));
        }
        (None, Some(fields)) => {
            fields.remove(BOOK_FIELD);
        }
        (_, None) => {}
    }
}

/// How many of `books` are on template `id`.
pub fn usage(books: &[Value], id: u32) -> usize {
    books.iter().filter(|book| linked(book) == Some(id)).count()
}

/// A stored chartbook's charts in layout order, with the tree they are in.
fn in_order(book: &Value) -> Option<(Node, Vec<&Value>)> {
    let layout: Node = serde_json::from_value::<Node>(book.get("layout")?.clone()).ok()?.deduped()?;
    let panes = book.get("panes")?.as_array()?;
    let ordered = layout
        .leaves()
        .into_iter()
        .map(|id| panes.iter().find(|p| p["id"].as_u64() == Some(u64::from(id))))
        .collect::<Option<Vec<_>>>()?;
    Some((layout, ordered))
}

/// What a stored chartbook would be saved as. `None` for a book whose tree
/// names a chart it does not have, which is not an arrangement to keep.
pub fn capture(book: &Value) -> Option<Shape> {
    let (layout, ordered) = in_order(book)?;
    let mut positions = HashMap::new();
    let mut panes = Vec::with_capacity(ordered.len());
    for (at, pane) in ordered.iter().enumerate() {
        positions.insert(pane["id"].as_u64()? as u32, at as u32);
        panes.push(Slot {
            template: pane["template"].as_u64().map(|id| id as u32),
            indicators: serde_json::from_value(pane["indicators"].clone()).unwrap_or_default(),
            timeframe: pane["timeframe"].as_str().map(str::to_string),
        });
    }
    Some(Shape { layout: layout.relabel(&positions), panes })
}

/// How far apart two dividers may be and still be the same divider.
///
/// A divider is put back in whole pixels, and the window writes down the
/// share the pixel it landed on gives, so a template opened in a window of
/// another size comes back a thousandth or two off what it was saved with.
/// That is not a change anybody made, and carrying it to every other book on
/// the template would be a write each time one is opened.
const SAME_DIVIDER: f64 = 0.005;

impl Shape {
    /// Whether this is the arrangement a template `held`: dividers within
    /// [`SAME_DIVIDER`] of each other, and a resolution the template says
    /// nothing about — one saved before resolutions were in templates — no
    /// difference at all.
    pub fn same(&self, held: &Shape) -> bool {
        fn alike(a: &Node, b: &Node) -> bool {
            match (a, b) {
                (Node::Leaf(a), Node::Leaf(b)) => a == b,
                (
                    Node::Split { horizontal: ha, ratio: ra, first: fa, second: sa },
                    Node::Split { horizontal: hb, ratio: rb, first: fb, second: sb },
                ) => ha == hb && (ra - rb).abs() <= SAME_DIVIDER && alike(fa, fb) && alike(sa, sb),
                _ => false,
            }
        }
        let slots = self.panes.len() == held.panes.len()
            && self.panes.iter().zip(&held.panes).all(|(now, held)| {
                now.template == held.template
                    && now.indicators == held.indicators
                    && (held.timeframe.is_none() || now.timeframe == held.timeframe)
            });
        slots && alike(&self.layout, &held.layout)
    }
}

/// A shape whose tree names exactly the positions it has a slot for.
fn sound(shape: &Shape) -> bool {
    let mut leaves = shape.layout.leaves();
    leaves.sort_unstable();
    leaves.len() == shape.panes.len() && leaves.iter().enumerate().all(|(at, id)| *id as usize == at)
}

/// Whether some template already holds this book's arrangement exactly, so
/// replacing it loses nothing that cannot be had back.
pub fn kept(book: &Value, templates: &[BookTemplate]) -> bool {
    capture(book).is_some_and(|shape| templates.iter().any(|t| shape.same(&t.shape)))
}

/// The next pane id no book is using, for charts a template adds.
pub fn next_pane(books: &[Value]) -> u32 {
    books
        .iter()
        .flat_map(|book| book["panes"].as_array().map(|p| p.as_slice()).unwrap_or(&[]))
        .filter_map(|pane| pane["id"].as_u64())
        .max()
        .map_or(1, |id| id as u32 + 1)
}

/// Where a chart of a changed arrangement came from in the one before it.
///
/// What lets the other books on a template change the way the one being
/// edited did. Splitting the first of two charts puts the new one second in
/// layout order; read by position alone, every other book would see its
/// second chart move into the new pane and a copy appear where it was.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Origin {
    /// The chart that was at this position.
    Was(usize),
    /// A new chart, split off the one that was at this position.
    Beside(usize),
    /// A new chart nothing says anything about.
    New,
}

/// Where each chart of `layout` came from, given the charts that were there
/// before, in layout order.
///
/// A chart that was there is found by its id. One that was not was split off
/// whichever chart shares its split, when that chart was there before — which
/// is what splitting does, from the window and from a command alike.
pub fn origins(before: &[u32], layout: &Node) -> Vec<Origin> {
    fn walk(node: &Node, before: &[u32], out: &mut Vec<Origin>) {
        match node {
            Node::Leaf(id) => out.push(was(before, *id)),
            Node::Split { first, second, .. } => match (first.as_ref(), second.as_ref()) {
                (Node::Leaf(a), Node::Leaf(b)) => {
                    let (a, b) = (was(before, *a), was(before, *b));
                    out.push(beside(a, b));
                    out.push(beside(b, a));
                }
                _ => {
                    walk(first, before, out);
                    walk(second, before, out);
                }
            },
        }
    }
    fn was(before: &[u32], id: u32) -> Origin {
        before.iter().position(|old| *old == id).map_or(Origin::New, Origin::Was)
    }
    fn beside(this: Origin, sibling: Origin) -> Origin {
        match (this, sibling) {
            (Origin::New, Origin::Was(at)) => Origin::Beside(at),
            _ => this,
        }
    }
    let mut out = Vec::new();
    walk(layout, before, &mut out);
    out
}

/// Give a stored chartbook a template's arrangement, in place.
///
/// The book keeps what is on its charts. Each chart keeps its symbol, link
/// mode and the rest of its own settings: by `origins` when they are given,
/// and otherwise by position. A chart the book had nothing for
/// is a copy of the chart it was split off, or of the focused one, with no
/// drawings of its own. Charts left over go. Its resolution is the template's,
/// when the template has one, and so is what it draws: its chart template when
/// that still exists, read as it is now, and otherwise the indicators the
/// template was saved with.
///
/// Leaves the link alone — taking a template and being on it are the caller's
/// to put together. `false`, with the book untouched, when either side is not
/// an arrangement.
pub fn apply(
    book: &mut Value,
    shape: &Shape,
    charts: &[Template],
    next_id: &mut u32,
    origins: Option<&[Origin]>,
) -> bool {
    if !sound(shape) {
        return false;
    }
    let Some((_, ordered)) = in_order(book) else { return false };
    let had: Vec<Value> = ordered.into_iter().cloned().collect();
    if had.is_empty() {
        return false;
    }
    let focused = book["focused"].as_u64();
    let focus_at = had.iter().position(|p| p["id"].as_u64() == focused).unwrap_or(0);

    let from = |at: usize| match origins.and_then(|o| o.get(at)) {
        Some(Origin::Was(old)) if *old < had.len() => Origin::Was(*old),
        Some(Origin::Beside(old)) if *old < had.len() => Origin::Beside(*old),
        Some(_) => Origin::New,
        None if at < had.len() => Origin::Was(at),
        None => Origin::New,
    };
    let mut taken = vec![false; had.len()];
    let mut ids = HashMap::new();
    let mut panes = Vec::with_capacity(shape.panes.len());
    for (at, slot) in shape.panes.iter().enumerate() {
        let (old, keep) = match from(at) {
            Origin::Was(old) => (old, !taken[old]),
            Origin::Beside(old) => (old, false),
            Origin::New => (focus_at, false),
        };
        let mut pane = had[old].clone();
        if keep {
            taken[old] = true;
        } else {
            pane["id"] = json!(*next_id);
            *next_id += 1;
            // Its own name among the drawings, and none of the original's
            // drawings: see `cli::charts::copy_pane`.
            pane["drawing_uid"] = json!(gtk::glib::uuid_string_random().to_string());
            pane["drawings"] = json!([]);
        }
        match slot.template.and_then(|id| charts.iter().find(|t| t.id == id)) {
            Some(template) => {
                pane["template"] = json!(template.id);
                pane["indicators"] = serde_json::to_value(&template.indicators).unwrap_or(json!([]));
            }
            None => {
                pane["template"] = Value::Null;
                pane["indicators"] = serde_json::to_value(&slot.indicators).unwrap_or(json!([]));
            }
        }
        if let Some(timeframe) = &slot.timeframe {
            pane["timeframe"] = json!(timeframe);
        }
        let Some(id) = pane["id"].as_u64() else { return false };
        ids.insert(at as u32, id as u32);
        panes.push(pane);
    }
    let layout = shape.layout.relabel(&ids);
    let Ok(layout) = serde_json::to_value(&layout) else { return false };
    // The focus stays on the chart that had it, wherever that went.
    let focus = had[focus_at]["id"].as_u64().map(|id| id as u32);
    let kept_focus = panes.iter().any(|p| p["id"].as_u64().map(|id| id as u32) == focus);
    let focus = match (kept_focus, focus) {
        (true, Some(id)) => id,
        _ => ids.get(&(focus_at.min(panes.len() - 1) as u32)).copied().unwrap_or(0),
    };
    book["layout"] = layout;
    book["panes"] = Value::Array(panes);
    book["focused"] = json!(focus);
    // Which chart filled the window was a chart of the old arrangement.
    book["maximized"] = Value::Null;
    true
}

/// Bring every book on template `id` but the one at `except` to `shape`. How
/// many it changed.
pub fn follow(
    books: &mut [Value],
    id: u32,
    shape: &Shape,
    charts: &[Template],
    except: Option<usize>,
    origins: Option<&[Origin]>,
) -> usize {
    let mut next_id = next_pane(books);
    let mut changed = 0;
    for (at, book) in books.iter_mut().enumerate() {
        if Some(at) == except || linked(book) != Some(id) {
            continue;
        }
        // Following is not taking: a chart that filled the window, and is
        // still there, still fills it when the book is opened.
        let maximized = book["maximized"].clone();
        if apply(book, shape, charts, &mut next_id, origins) {
            if leaves(book).iter().any(|id| maximized.as_u64() == Some(u64::from(*id))) {
                book["maximized"] = maximized;
            }
            changed += 1;
        }
    }
    changed
}

/// Carry a change to the book at `at` into the template it is on, and from
/// there into every other book on it. How many other books changed.
///
/// Nothing at all when the book is on no template, or its arrangement is
/// still exactly the template's — which is every save that changed a symbol,
/// a resolution or a link mode. `before` is the book's charts as they were,
/// in layout order, so the others change the way it did.
///
/// The window calls this as it writes its arrangement down, and a command
/// as it writes its own, so a chart split from either reaches the same books
/// the same way.
pub fn carry(store: &Store, books: &mut [Value], at: usize, before: &[u32]) -> usize {
    let Some(id) = books.get(at).and_then(linked) else { return 0 };
    let Some(template) = get(store, id) else { return 0 };
    let Some(book) = books.get(at) else { return 0 };
    let Some(shape) = capture(book) else { return 0 };
    if shape.same(&template.shape) {
        return 0;
    }
    // With nothing to say what the book was — one just made, or one whose
    // tree could not be read — the change is read by position. Read by ids
    // that match nothing, every chart would be new, and every other book
    // would lose its charts to copies of its focused one.
    let origins = match before.is_empty() {
        true => None,
        false => in_order(book).map(|(layout, _)| origins(before, &layout)),
    };
    replace(store, id, shape.clone());
    let charts = crate::templates::all(store);
    follow(books, id, &shape, &charts, Some(at), origins.as_deref())
}

/// The ids of a stored book's charts, in layout order.
pub fn leaves(book: &Value) -> Vec<u32> {
    in_order(book).map(|(layout, _)| layout.leaves()).unwrap_or_default()
}

/// Let every book on template `id` go of it, keeping what it has. How many.
pub fn unlink(books: &mut [Value], id: u32) -> usize {
    let mut count = 0;
    for book in books.iter_mut().filter(|book| linked(book) == Some(id)) {
        set_link(book, None);
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use omacharts_engine::IndicatorKind;

    fn pane(id: u32, symbol: &str, indicators: Value, template: Option<u32>) -> Value {
        json!({
            "id": id, "symbol": symbol, "timeframe": "1D", "indicators": indicators,
            "template": template, "bar_style": "candles", "session": "regular",
            "show_grid": true, "link": "focus", "drawing_uid": format!("uid-{id}"),
            "drawings": [{"marker": id}],
        })
    }

    fn rsi() -> Value {
        serde_json::to_value(vec![Indicator::new(1, IndicatorKind::Rsi)]).unwrap()
    }

    /// A two-by-two: two columns, each split into two rows, dividers moved.
    fn grid(symbols: [&str; 4]) -> Value {
        json!({
            "name": "Grid",
            "layout": {"split": {"horizontal": true, "ratio": 0.3,
                "first": {"split": {"horizontal": false, "ratio": 0.7,
                    "first": {"leaf": 11}, "second": {"leaf": 12}}},
                "second": {"split": {"horizontal": false, "ratio": 0.4,
                    "first": {"leaf": 13}, "second": {"leaf": 14}}}}},
            "focused": 13,
            "panes": [
                pane(14, symbols[3], json!([]), None),
                pane(11, symbols[0], rsi(), None),
                pane(12, symbols[1], json!([]), Some(1)),
                pane(13, symbols[2], json!([]), None),
            ],
        })
    }

    fn single(symbol: &str) -> Value {
        single_at(5, symbol)
    }

    fn single_at(id: u32, symbol: &str) -> Value {
        json!({
            "name": "One", "layout": {"leaf": id}, "focused": id,
            "panes": [pane(id, symbol, json!([]), None)],
            "maximized": id,
        })
    }

    fn symbols(book: &Value) -> Vec<String> {
        let (_, ordered) = in_order(book).unwrap();
        ordered.iter().map(|p| p["symbol"].as_str().unwrap().to_string()).collect()
    }

    fn volume_template() -> Template {
        Template { id: 1, name: "Default".into(), indicators: vec![Indicator::new(1, IndicatorKind::Volume)] }
    }

    /// Saved from one book and taken by another, the arrangement comes back
    /// with every divider where it was, as a share rather than in pixels, and
    /// each chart drawing what the same position drew.
    #[test]
    fn an_arrangement_survives_being_saved_and_taken() {
        let saved = capture(&grid(["A", "B", "C", "D"])).unwrap();
        assert_eq!(saved.layout.leaves(), vec![0, 1, 2, 3]);
        assert_eq!(saved.panes[0].indicators[0].kind, IndicatorKind::Rsi);
        assert_eq!(saved.panes[1].template, Some(1));

        let mut book = single("SPY");
        let mut next = 100;
        assert!(apply(&mut book, &saved, &[volume_template()], &mut next, None));
        assert_eq!(capture(&book).unwrap(), Shape {
            // Position 1 is on the chart template, and draws it as it is now.
            panes: vec![
                saved.panes[0].clone(),
                Slot { template: Some(1), indicators: volume_template().indicators, timeframe: Some("1D".into()) },
                saved.panes[2].clone(),
                saved.panes[3].clone(),
            ],
            layout: saved.layout.clone(),
        });
        let json = book["layout"].to_string();
        assert!(json.contains("0.3") && json.contains("0.7") && json.contains("0.4"), "{json}");
        // The book keeps its symbol where it was, and fills the rest from it.
        assert_eq!(symbols(&book), ["SPY", "SPY", "SPY", "SPY"]);
        assert!(book["maximized"].is_null(), "a chart of the old arrangement still fills the window");
    }

    /// A book taking a template keeps its charts by where they sit.
    #[test]
    fn the_charts_on_screen_keep_their_symbols_by_position() {
        let saved = capture(&grid(["A", "B", "C", "D"])).unwrap();
        let mut book = grid(["W", "X", "Y", "Z"]);
        let mut next = 100;
        assert!(apply(&mut book, &saved, &[], &mut next, None));
        assert_eq!(symbols(&book), ["W", "X", "Y", "Z"]);

        // Fewer positions: the charts past the end go, and the focus stays
        // in range.
        let two = capture(&json!({
            "layout": {"split": {"horizontal": true, "ratio": 0.5,
                "first": {"leaf": 1}, "second": {"leaf": 2}}},
            "focused": 1,
            "panes": [pane(1, "A", json!([]), None), pane(2, "B", json!([]), None)],
        }))
        .unwrap();
        assert!(apply(&mut book, &two, &[], &mut next, None));
        assert_eq!(symbols(&book), ["W", "X"]);
        let focused = book["focused"].as_u64().unwrap() as u32;
        assert!(in_order(&book).unwrap().0.leaves().contains(&focused));
    }

    /// A chart a template adds is a copy of the focused one, with its own
    /// name among the drawings and none of the focused chart's drawings.
    #[test]
    fn a_chart_a_template_adds_copies_the_focused_one_without_its_drawings() {
        let saved = capture(&grid(["A", "B", "C", "D"])).unwrap();
        let mut book = json!({
            "layout": {"split": {"horizontal": true, "ratio": 0.5,
                "first": {"leaf": 1}, "second": {"leaf": 2}}},
            "focused": 2,
            "panes": [pane(1, "AAPL", json!([]), None), pane(2, "MSFT", json!([]), None)],
        });
        let mut next = 50;
        assert!(apply(&mut book, &saved, &[], &mut next, None));
        assert_eq!(symbols(&book), ["AAPL", "MSFT", "MSFT", "MSFT"]);
        let (_, ordered) = in_order(&book).unwrap();
        assert_eq!(ordered[1]["drawings"], json!([{"marker": 2}]), "a kept chart lost its drawings");
        assert_eq!(ordered[2]["drawings"], json!([]));
        assert_ne!(ordered[2]["drawing_uid"], ordered[1]["drawing_uid"]);
        assert_eq!(next, 52, "two charts were added");
    }

    /// A chart template deleted since the chartbook template was saved
    /// leaves the chart the indicators it was saved with, as its own.
    #[test]
    fn a_chart_template_that_has_gone_leaves_the_indicators_it_was_saved_with() {
        let mut saved = capture(&single("A")).unwrap();
        saved.panes[0] = Slot { template: Some(7), indicators: vec![Indicator::new(1, IndicatorKind::Macd)], timeframe: None };
        let mut book = single("SPY");
        assert!(apply(&mut book, &saved, &[volume_template()], &mut 10, None));
        let pane = &book["panes"][0];
        assert!(pane["template"].is_null());
        assert_eq!(pane["indicators"][0]["kind"], json!("macd"));
    }

    /// A tree naming a position the template has no chart for, or a book
    /// naming a chart it does not have, changes nothing.
    #[test]
    fn an_arrangement_that_is_not_one_changes_nothing() {
        let broken = Shape { layout: Node::leaf(3), panes: vec![Slot { template: None, indicators: vec![], timeframe: None }] };
        let mut book = single("SPY");
        let before = book.clone();
        assert!(!apply(&mut book, &broken, &[], &mut 10, None));
        assert_eq!(book, before);

        let mut orphan = json!({"layout": {"leaf": 9}, "focused": 9, "panes": [pane(1, "A", json!([]), None)]});
        assert!(capture(&orphan).is_none());
        let good = capture(&single("A")).unwrap();
        assert!(!apply(&mut orphan, &good, &[], &mut 10, None));
    }

    #[test]
    fn templates_are_made_replaced_renamed_and_deleted() {
        let store = Store::memory().unwrap();
        let made = create(&store, "Grid", capture(&grid(["A", "B", "C", "D"])).unwrap());
        assert!(replace(&store, made.id, capture(&single("A")).unwrap()));
        assert_eq!(get(&store, made.id).unwrap().shape.panes.len(), 1, "overwriting kept the old arrangement");
        assert!(rename(&store, made.id, "Solo"));
        assert_eq!(get(&store, made.id).unwrap().name, "Solo");
        assert_eq!(delete(&store, made.id).map(|t| t.name), Some("Solo".into()));
        assert!(get(&store, made.id).is_none());
        assert!(!rename(&store, made.id, "Gone"));
        assert!(!replace(&store, made.id, capture(&single("A")).unwrap()));
        assert!(delete(&store, made.id).is_none());

        let again = create(&store, "Again", capture(&single("A")).unwrap());
        assert_ne!(again.id, made.id, "an id was handed out twice");
    }

    #[test]
    fn a_selector_is_a_name_or_an_id() {
        let store = Store::memory().unwrap();
        let shape = capture(&single("A")).unwrap();
        let grid = create(&store, "Grid", shape.clone());
        create(&store, "Twin", shape.clone());
        create(&store, "twin", shape);
        let templates = all(&store);
        assert!(matches!(find(&templates, "grid"), Lookup::Found(t) if t.id == grid.id));
        assert!(matches!(find(&templates, &format!("id:{}", grid.id)), Lookup::Found(_)));
        assert!(matches!(find(&templates, "Twin"), Lookup::Ambiguous(ids) if ids.len() == 2));
        assert!(matches!(find(&templates, "nope"), Lookup::Missing));
        assert!(matches!(find(&templates, "id:99"), Lookup::Missing));
    }

    /// A profile from before there were chartbook templates has no setting,
    /// and its books name none; nonsense in the setting reads as none.
    #[test]
    fn old_settings_and_books_read_as_no_templates() {
        let store = Store::memory().unwrap();
        assert!(all(&store).is_empty());
        assert_eq!(linked(&single("A")), None);
        store.set_setting(SETTING, "{ not json");
        assert!(all(&store).is_empty());
        assert_eq!(create(&store, "Fresh", capture(&single("A")).unwrap()).name, "Fresh");
    }

    /// Saving into a template brings every other book on it along, and a
    /// book on another template or none is left alone.
    #[test]
    fn the_books_on_a_template_follow_it_and_no_others() {
        let shape = capture(&grid(["A", "B", "C", "D"])).unwrap();
        let mut books = vec![single_at(1, "ONE"), single_at(2, "TWO"), single_at(3, "THREE"), single_at(4, "FOUR")];
        set_link(&mut books[0], Some(3));
        set_link(&mut books[1], Some(3));
        set_link(&mut books[2], Some(4));
        assert_eq!(usage(&books, 3), 2);

        let changed = follow(&mut books, 3, &shape, &[], Some(0), None);
        assert_eq!(changed, 1);
        assert_eq!(symbols(&books[0]), ["ONE"], "the book saved from was changed");
        assert_eq!(symbols(&books[1]), ["TWO"; 4]);
        assert_eq!(symbols(&books[2]), ["THREE"]);
        assert_eq!(symbols(&books[3]), ["FOUR"]);

        // No two charts anywhere share an id.
        let mut ids: Vec<u64> =
            books.iter().flat_map(|b| b["panes"].as_array().unwrap().iter()).map(|p| p["id"].as_u64().unwrap()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);

        assert_eq!(unlink(&mut books, 3), 2);
        assert_eq!(usage(&books, 3), 0);
        assert_eq!(symbols(&books[1]), ["TWO"; 4], "unlinking took something off the book");
    }

    #[test]
    fn a_book_whose_arrangement_some_template_holds_has_nothing_to_lose() {
        let book = grid(["A", "B", "C", "D"]);
        let store = Store::memory().unwrap();
        assert!(!kept(&book, &all(&store)));
        create(&store, "Grid", capture(&book).unwrap());
        assert!(kept(&grid(["W", "X", "Y", "Z"]), &all(&store)), "symbols are not the arrangement");
    }

    fn pair(first: (u32, &str), second: (u32, &str)) -> Value {
        json!({
            "layout": {"split": {"horizontal": true, "ratio": 0.5,
                "first": {"leaf": first.0}, "second": {"leaf": second.0}}},
            "focused": first.0,
            "panes": [pane(first.0, first.1, json!([]), None), pane(second.0, second.1, json!([]), None)],
        })
    }

    /// Splitting the first of two charts puts the new one second in layout
    /// order. The other books split their first chart too, rather than see
    /// their second chart move into the new pane.
    #[test]
    fn the_other_books_change_the_way_the_edited_one_did() {
        let store = Store::memory().unwrap();
        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        let template = create(&store, "Pair", capture(&books[0]).unwrap());
        set_link(&mut books[0], Some(template.id));
        set_link(&mut books[1], Some(template.id));

        // Chart 1 split, the way the window and `chart split` both do it.
        let before = leaves(&books[0]);
        books[0]["layout"] = json!({"split": {"horizontal": true, "ratio": 0.5,
            "first": {"split": {"horizontal": false, "ratio": 0.5,
                "first": {"leaf": 1}, "second": {"leaf": 9}}},
            "second": {"leaf": 2}}});
        books[0]["panes"].as_array_mut().unwrap().push(pane(9, "A", json!([]), None));
        assert_eq!(origins(&before, &in_order(&books[0]).unwrap().0), [Origin::Was(0), Origin::Beside(0), Origin::Was(1)]);

        assert_eq!(carry(&store, &mut books, 0, &before), 1);
        assert_eq!(symbols(&books[1]), ["X", "X", "Y"]);
        assert_eq!(get(&store, template.id).unwrap().shape.panes.len(), 3, "the template did not take the change");

        // Closing the first chart: the second book loses its first, not its last.
        let before = leaves(&books[0]);
        books[0]["layout"] = json!({"split": {"horizontal": true, "ratio": 0.5,
            "first": {"leaf": 9}, "second": {"leaf": 2}}});
        books[0]["panes"].as_array_mut().unwrap().retain(|p| p["id"] != json!(1));
        assert_eq!(carry(&store, &mut books, 0, &before), 1);
        assert_eq!(symbols(&books[1]), ["X", "Y"]);

        // A divider dragged, and an indicator added: same charts, new shape.
        let before = leaves(&books[0]);
        books[0]["layout"]["split"]["ratio"] = json!(0.25);
        let second = books[0]["panes"].as_array_mut().unwrap().iter_mut().find(|p| p["id"] == json!(2)).unwrap();
        second["indicators"] = rsi();
        assert_eq!(carry(&store, &mut books, 0, &before), 1);
        assert_eq!(books[1]["layout"]["split"]["ratio"], json!(0.25));
        assert_eq!(symbols(&books[1]), ["X", "Y"]);
        let (_, ordered) = in_order(&books[1]).unwrap();
        assert_eq!(ordered[1]["indicators"][0]["kind"], json!("rsi"));

        // A new symbol is not an arrangement: nothing to carry.
        books[0]["panes"][0]["symbol"] = json!("QQQ");
        let before = leaves(&books[0]);
        assert_eq!(carry(&store, &mut books, 0, &before), 0);
        assert_eq!(symbols(&books[1]), ["X", "Y"]);
    }

    /// A divider put back a pixel off in a window of another size is the
    /// same divider, and opening the book writes nothing anywhere.
    #[test]
    fn a_divider_a_pixel_off_is_not_a_change() {
        let store = Store::memory().unwrap();
        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        books[0]["layout"]["split"]["ratio"] = json!(0.4);
        let template = create(&store, "Pair", capture(&books[0]).unwrap());
        set_link(&mut books[0], Some(template.id));
        set_link(&mut books[1], Some(template.id));
        books[0]["layout"]["split"]["ratio"] = json!(0.4012);
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 0);
        assert_eq!(get(&store, template.id).unwrap().shape, template.shape);
        books[0]["layout"]["split"]["ratio"] = json!(0.43);
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 1);
    }

    fn timeframes(book: &Value) -> Vec<String> {
        let (_, ordered) = in_order(book).unwrap();
        ordered.iter().map(|p| p["timeframe"].as_str().unwrap().to_string()).collect()
    }

    /// Each chart's resolution is part of the arrangement: saved, taken, and
    /// carried to the same position in every other book on the template.
    #[test]
    fn a_resolution_is_saved_taken_and_carried_by_position() {
        let store = Store::memory().unwrap();
        let mut saved = grid(["A", "B", "C", "D"]);
        for pane in saved["panes"].as_array_mut().unwrap() {
            if pane["id"] == json!(12) {
                pane["timeframe"] = json!("1h");
            }
        }
        let shape = capture(&saved).unwrap();
        assert_eq!(shape.panes[1].timeframe.as_deref(), Some("1h"));
        let back: Shape = serde_json::from_str(&serde_json::to_string(&shape).unwrap()).unwrap();
        assert_eq!(back, shape);

        let mut books = [pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        let template = create(&store, "Pair", capture(&books[0]).unwrap());
        set_link(&mut books[0], Some(template.id));
        set_link(&mut books[1], Some(template.id));
        assert!(apply(&mut books[1], &shape, &[], &mut 50, None));
        assert_eq!(timeframes(&books[1]), ["1D", "1h", "1D", "1D"]);

        // A resolution changed on the open book reaches the closed one at the
        // same position, and its symbols stay.
        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        set_link(&mut books[0], Some(template.id));
        set_link(&mut books[1], Some(template.id));
        books[0]["panes"][1]["timeframe"] = json!("15m");
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 1);
        assert_eq!(timeframes(&books[1]), ["1D", "15m"]);
        assert_eq!(symbols(&books[1]), ["X", "Y"]);
        // And the other way, from what was the closed one.
        books[1]["panes"][0]["timeframe"] = json!("1W");
        assert_eq!(carry(&store, &mut books, 1, &[3, 4]), 1);
        assert_eq!(timeframes(&books[0]), ["1W", "15m"]);
        // Carried once: the book it reached has nothing left to carry back.
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 0);
    }

    /// A template saved before resolutions were part of one leaves every
    /// chart's resolution as it is, and is not rewritten just for being
    /// opened.
    #[test]
    fn a_template_saved_without_resolutions_leaves_them_alone() {
        let store = Store::memory().unwrap();
        let old = r#"{"next_id":2,"templates":[{"id":1,"name":"Old",
            "layout":{"split":{"horizontal":true,"ratio":0.5,"first":{"leaf":0},"second":{"leaf":1}}},
            "panes":[{"template":null,"indicators":[]},{"template":null,"indicators":[]}]}]}"#;
        store.set_setting(SETTING, old);
        let template = get(&store, 1).unwrap();
        assert!(template.shape.panes.iter().all(|slot| slot.timeframe.is_none()));

        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        books[0]["panes"][0]["timeframe"] = json!("1h");
        assert!(apply(&mut books[0], &template.shape, &[], &mut 50, None));
        assert_eq!(timeframes(&books[0]), ["1h", "1D"]);

        set_link(&mut books[0], Some(1));
        set_link(&mut books[1], Some(1));
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 0, "opening an old one rewrote the others");
        assert_eq!(timeframes(&books[1]), ["1D", "1D"]);
    }

    /// A book on no template, or on one that has gone, carries nothing.
    #[test]
    fn a_book_on_no_template_carries_nothing() {
        let store = Store::memory().unwrap();
        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        books[0]["layout"]["split"]["ratio"] = json!(0.2);
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 0);
        set_link(&mut books[0], Some(42));
        set_link(&mut books[1], Some(42));
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 0);
        assert_eq!(books[1]["layout"]["split"]["ratio"], json!(0.5));
    }

    /// A change from a book nothing says the earlier shape of — one just
    /// made on the template — is read by position, so the other books keep
    /// their charts rather than each becoming copies of its focused one.
    #[test]
    fn a_change_with_no_earlier_shape_is_carried_by_position() {
        let store = Store::memory().unwrap();
        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        let template = create(&store, "Pair", capture(&books[0]).unwrap());
        set_link(&mut books[0], Some(template.id));
        set_link(&mut books[1], Some(template.id));
        books[0]["panes"][1]["indicators"] = rsi();
        assert_eq!(carry(&store, &mut books, 0, &[]), 1);
        assert_eq!(symbols(&books[1]), ["X", "Y"]);
        let (_, ordered) = in_order(&books[1]).unwrap();
        assert_eq!(ordered.iter().map(|p| p["id"].clone()).collect::<Vec<_>>(), [json!(3), json!(4)]);
        assert_eq!(ordered[0]["drawings"], json!([{"marker": 3}]), "a chart lost its drawings");
        assert_eq!(ordered[1]["indicators"][0]["kind"], json!("rsi"));
    }

    /// A book that follows a change keeps the chart that filled its window
    /// while that chart is still there, and lets it go once it is not.
    #[test]
    fn a_following_book_keeps_its_maximized_chart() {
        let store = Store::memory().unwrap();
        let mut books = vec![pair((1, "A"), (2, "B")), pair((3, "X"), (4, "Y"))];
        let template = create(&store, "Pair", capture(&books[0]).unwrap());
        set_link(&mut books[0], Some(template.id));
        set_link(&mut books[1], Some(template.id));
        books[1]["maximized"] = json!(4);
        books[0]["layout"]["split"]["ratio"] = json!(0.3);
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 1);
        assert_eq!(books[1]["maximized"], json!(4));

        books[0]["layout"] = json!({"leaf": 1});
        books[0]["panes"].as_array_mut().unwrap().retain(|p| p["id"] == json!(1));
        assert_eq!(carry(&store, &mut books, 0, &[1, 2]), 1);
        assert!(books[1]["maximized"].is_null());
    }

    /// No name two templates would share, in any case, and none a selector
    /// already reads as something else.
    #[test]
    fn a_name_is_refused_when_a_selector_could_not_tell_it_apart() {
        let store = Store::memory().unwrap();
        let shape = capture(&single("SPY")).unwrap();
        let corners = create(&store, "Corners", shape.clone());
        create(&store, "Pair", shape);
        let all = all(&store);
        assert!(refused_name(&all, "Grid", None).is_none());
        assert!(refused_name(&all, " pair ", None).is_some());
        assert!(refused_name(&all, "Pair", Some(corners.id)).is_some());
        assert!(refused_name(&all, "CORNERS", Some(corners.id)).is_none());
        for name in ["None", "id:4", "  "] {
            assert!(refused_name(&all, name, None).is_some(), "{name}");
        }
    }
}
