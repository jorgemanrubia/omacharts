//! Chartbooks and the charts in them, from outside the window.
//!
//! These live as one JSON value in a settings row rather than as rows of
//! their own, because a tree of pane ids and a list of panes that disagree is
//! a window that cannot be rebuilt — so they are written together or not at
//! all.
//!
//! Everything here edits that value as [`serde_json::Value`] rather than
//! through the window's own structs. That is deliberate: the CLI must not
//! drop a field it has not been taught about. Parsing into a struct and
//! writing it back would silently discard anything added since, and the field
//! it discarded would be somebody's layout.

use serde_json::{json, Map, Value};

use crate::store::Store;

use super::{Fault, EXIT_ERROR, EXIT_NOT_FOUND};

/// Where the arrangement is kept. The same key the window writes.
const SETTING: &str = "workspace";

pub struct Workspace {
    pub value: Value,
}

impl Workspace {
    /// Read what the window last wrote, or an empty arrangement.
    pub fn load(store: &Store) -> Workspace {
        let value = store
            .setting(SETTING)
            .and_then(|json| serde_json::from_str::<Value>(&json).ok())
            .filter(|value| value.get("books").is_some_and(Value::is_array))
            .unwrap_or_else(|| json!({ "books": [], "active": 0 }));
        Workspace { value }
    }

    pub fn save(&self, store: &Store) {
        store.set_setting(SETTING, &self.value.to_string());
    }

    pub fn books(&self) -> &[Value] {
        self.value["books"].as_array().map(|b| b.as_slice()).unwrap_or(&[])
    }

    /// The books, to change.
    ///
    /// [`load`](Self::load) only accepts a value whose `books` is an array, and
    /// this puts an empty one back rather than trusting that — a panic here
    /// runs inside the window, where it aborts the process instead of
    /// unwinding, and a command is not worth somebody's arrangement.
    fn books_mut(&mut self) -> &mut Vec<Value> {
        if !self.value["books"].is_array() {
            self.value["books"] = json!([]);
        }
        self.value["books"].as_array_mut().expect("just made it an array")
    }

    pub fn active(&self) -> usize {
        self.value["active"].as_u64().unwrap_or(0) as usize
    }

    pub fn set_active(&mut self, index: usize) {
        self.value["active"] = json!(index);
    }

    /// What a book with no name of its own is called.
    ///
    /// By where it sits rather than when it was made, matching the strip: the
    /// names people read have to be the names the CLI answers to.
    pub fn name_of(&self, index: usize) -> String {
        let named = self.books().get(index).and_then(|b| b["name"].as_str()).unwrap_or("");
        match named.is_empty() {
            true => format!("Chartbook {}", index + 1),
            false => named.to_string(),
        }
    }

    /// Resolve `a name, or id:N` to a position in the list.
    pub fn find(&self, selector: &str) -> Result<usize, Fault> {
        if let Some(rest) = selector.strip_prefix("id:") {
            let index: usize = rest
                .parse()
                .map_err(|_| Fault::usage(format!("{selector:?} is not a chartbook id")))?;
            return match index < self.books().len() {
                true => Ok(index),
                false => Err(Fault::new(
                    EXIT_NOT_FOUND,
                    format!("no chartbook with id {index}"),
                )),
            };
        }
        let wanted = selector.to_lowercase();
        let hits: Vec<usize> = (0..self.books().len())
            .filter(|i| self.name_of(*i).to_lowercase() == wanted)
            .collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(Fault::new(
                EXIT_NOT_FOUND,
                format!("no chartbook called {selector:?}"),
            )),
            many => Err(Fault::ambiguous(format!(
                "{} chartbooks are called {selector:?}; use {}",
                many.len(),
                many.iter().map(|i| format!("id:{i}")).collect::<Vec<_>>().join(" or ")
            ))),
        }
    }

    /// The book a command acts on when none was named: the open one.
    pub fn resolve(&self, selector: Option<&str>) -> Result<usize, Fault> {
        match selector {
            Some(name) => self.find(name),
            None if self.books().is_empty() => Err(Fault::new(
                EXIT_NOT_FOUND,
                "there are no chartbooks yet; open the app once, or create one".to_string(),
            )),
            None => Ok(self.active().min(self.books().len() - 1)),
        }
    }

    /// The next pane id nobody is using.
    ///
    /// Across every book, not just the one being changed: a book that is not
    /// on screen still names its panes by the ids it was built with, and two
    /// books sharing an id is the bug that collapsed a layout once already.
    pub fn next_pane(&self) -> u32 {
        self.books()
            .iter()
            .flat_map(|book| book["panes"].as_array().map(|p| p.as_slice()).unwrap_or(&[]))
            .filter_map(|pane| pane["id"].as_u64())
            .max()
            .map(|id| id as u32 + 1)
            .unwrap_or(1)
    }

    pub fn book(&self, index: usize) -> Result<&Value, Fault> {
        self.books().get(index).ok_or_else(|| {
            Fault::new(EXIT_NOT_FOUND, format!("no chartbook at position {index}"))
        })
    }

    /// A book to change, by position.
    ///
    /// A failure rather than an index, because every position here came from
    /// somebody's command and reaching past the end would take the window with
    /// it.
    pub fn book_mut(&mut self, index: usize) -> Result<&mut Value, Fault> {
        let books = self.books_mut();
        match index < books.len() {
            true => Ok(&mut books[index]),
            false => Err(Fault::new(
                EXIT_NOT_FOUND,
                format!("no chartbook at position {index}"),
            )),
        }
    }

    /// Put `symbol` on every chart in `group`, everywhere but on the chart
    /// that is leading it.
    ///
    /// Across every chartbook, not only the one that is open. A group reaches
    /// across books, so stopping at the open one would make a group mean "the
    /// charts in this group I can currently see" — and the book you switched
    /// away from would come back showing a symbol from before the link. The
    /// open book is included for the same reason the rest are: a command writes
    /// the whole arrangement back and the window rebuilds from it.
    ///
    /// Answers how many charts actually moved, so a command that changed four
    /// of them can say so. A chart already on the symbol is not one of them.
    pub fn lead_link_group(
        &mut self,
        group: u8,
        symbol: &str,
        suffix: Option<&str>,
        at: usize,
        leader: u32,
    ) -> usize {
        if group == 0 {
            return 0;
        }
        let mut moved = 0;
        for (index, book) in self.books_mut().iter_mut().enumerate() {
            let Some(panes) = book["panes"].as_array_mut() else { continue };
            for pane in panes {
                let is_leader = index == at && pane["id"].as_u64() == Some(u64::from(leader));
                if is_leader || pane["linked"].as_u64() != Some(u64::from(group)) {
                    continue;
                }
                if pane["symbol"].as_str() == Some(symbol) && pane["suffix"].as_str() == suffix {
                    continue;
                }
                pane["symbol"] = json!(symbol);
                pane["suffix"] = json!(suffix);
                moved += 1;
            }
        }
        moved
    }

    pub fn push(&mut self, book: Value) -> usize {
        self.books_mut().push(book);
        self.books().len() - 1
    }

    pub fn remove(&mut self, index: usize) {
        self.books_mut().remove(index);
        let active = self.active();
        let last = self.books().len().saturating_sub(1);
        self.set_active(if active >= index { active.saturating_sub(1).min(last) } else { active });
    }
}

/// A chart, as the CLI reports it.
pub fn panes(book: &Value) -> &[Value] {
    book["panes"].as_array().map(|p| p.as_slice()).unwrap_or(&[])
}

/// Which chart a command acts on: the one named, or the focused one.
///
/// `pos:N` counts along the arrangement in layout order and is the form worth
/// using. Rebuilding a chartbook hands out fresh pane ids, so an id read
/// before one names a different chart afterwards; a position names the same
/// place on the screen either way.
pub fn resolve_pane(book: &Value, wanted: Option<&str>) -> Result<u32, Fault> {
    let order = leaves(&book["layout"]);
    let ids: Vec<u32> = panes(book).iter().filter_map(|p| p["id"].as_u64()).map(|i| i as u32).collect();
    let Some(text) = wanted else {
        return book["focused"]
            .as_u64()
            .map(|id| id as u32)
            .filter(|id| ids.contains(id))
            .or_else(|| order.first().copied())
            .ok_or_else(|| Fault::new(EXIT_NOT_FOUND, "this chartbook has no charts".to_string()));
    };
    if let Some(rest) = text.strip_prefix("pos:") {
        let at: usize = rest
            .parse()
            .map_err(|_| Fault::usage(format!("{text:?} is not a position")))?;
        return order.get(at).copied().ok_or_else(|| {
            Fault::new(
                EXIT_NOT_FOUND,
                format!(
                    "this chartbook has {} charts, so there is no pos:{at}",
                    order.len()
                ),
            )
        });
    }
    let id: u32 = text
        .parse()
        .map_err(|_| Fault::usage(format!("{text:?} is not a chart; use pos:N or an id")))?;
    match ids.contains(&id) {
        true => Ok(id),
        false => Err(Fault::new(
            EXIT_NOT_FOUND,
            format!("this chartbook has no chart {id}; try `omacharts chart list`"),
        )),
    }
}

/// Where a chart sits in the arrangement, which is how it is named back.
pub fn position_of(book: &Value, id: u32) -> Option<usize> {
    leaves(&book["layout"]).iter().position(|leaf| *leaf == id)
}

/// The charts of a book, to change.
///
/// A failure rather than an `expect`: the arrangement is read back out of a
/// settings row that `config set workspace` can write anything into, and a
/// panic in a command aborts the window it is running inside.
pub fn panes_mut(book: &mut Value) -> Result<&mut Vec<Value>, Fault> {
    book["panes"].as_array_mut().ok_or_else(|| {
        Fault::new(EXIT_ERROR, "this chartbook has no list of charts in it".to_string())
    })
}

pub fn pane_mut(book: &mut Value, id: u32) -> Option<&mut Value> {
    book["panes"]
        .as_array_mut()?
        .iter_mut()
        .find(|p| p["id"].as_u64() == Some(id as u64))
}

/// A fresh chart, carrying the defaults the window would give it.
pub fn new_pane(id: u32, symbol: &str, suffix: Option<&str>) -> Value {
    json!({
        "id": id,
        "symbol": symbol,
        "suffix": suffix,
        "timeframe": "1D",
        "indicators": [],
        "bar_style": "candles",
        "session": "extended",
        "show_grid": true,
        "linked": 0,
        "shows_drawings": true,
        "sends_drawings": true,
        // What the chart is called among the drawings it makes, so it can
        // take them back if it stops sending.
        "drawing_uid": gtk::glib::uuid_string_random().to_string(),
        "drawings": [],
        "auto_scale": true,
    })
}

/// A chart copied from another, which is what splitting one means.
pub fn copy_pane(from: &Value, id: u32) -> Value {
    let mut copy = from.clone();
    copy["id"] = json!(id);
    // Its own name among the drawings, not the one it was copied from.
    // Two charts answering to the same name means a switch on either takes
    // back what both of them drew — which is how splitting a chart and
    // turning sending off on the half made the other half's work vanish
    // as well.
    copy["drawing_uid"] = json!(gtk::glib::uuid_string_random().to_string());
    copy
}

// -- the layout tree ------------------------------------------------------
//
// Serialised by the window as `{"leaf": 3}` or `{"split": {..}}`. Walked here
// rather than deserialised into the window's own enum, for the same reason as
// everything else in this file: a field this does not know about is a field
// it must not drop.

pub fn leaves(node: &Value) -> Vec<u32> {
    let mut out = Vec::new();
    collect(node, &mut out);
    out
}

fn collect(node: &Value, out: &mut Vec<u32>) {
    if let Some(id) = node.get("leaf").and_then(Value::as_u64) {
        out.push(id as u32);
    } else if let Some(split) = node.get("split") {
        collect(&split["first"], out);
        collect(&split["second"], out);
    }
}

/// Replace one leaf with a split holding it and a new chart.
pub fn split_leaf(node: &Value, id: u32, added: u32, horizontal: bool) -> Value {
    if node.get("leaf").and_then(Value::as_u64) == Some(id as u64) {
        return json!({"split": {
            "horizontal": horizontal,
            "ratio": 0.5,
            "first": {"leaf": id},
            "second": {"leaf": added},
        }});
    }
    match node.get("split") {
        None => node.clone(),
        Some(split) => {
            let mut inner = split.as_object().cloned().unwrap_or_default();
            inner.insert("first".into(), split_leaf(&split["first"], id, added, horizontal));
            inner.insert("second".into(), split_leaf(&split["second"], id, added, horizontal));
            Value::Object(Map::from_iter([("split".to_string(), Value::Object(inner))]))
        }
    }
}

/// Drop a leaf, collapsing the split above it into its sibling.
///
/// `None` when the tree was only that leaf: a chartbook with no charts in it
/// is not an arrangement, so the caller refuses rather than writing one down.
pub fn remove_leaf(node: &Value, id: u32) -> Option<Value> {
    if node.get("leaf").and_then(Value::as_u64) == Some(id as u64) {
        return None;
    }
    let Some(split) = node.get("split") else { return Some(node.clone()) };
    match (remove_leaf(&split["first"], id), remove_leaf(&split["second"], id)) {
        (None, None) => None,
        (None, Some(kept)) | (Some(kept), None) => Some(kept),
        (Some(first), Some(second)) => {
            let mut inner = split.as_object().cloned().unwrap_or_default();
            inner.insert("first".into(), first);
            inner.insert("second".into(), second);
            Some(Value::Object(Map::from_iter([("split".to_string(), Value::Object(inner))])))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(panes: Vec<u32>, layout: Value) -> Value {
        json!({
            "name": "",
            "layout": layout,
            "focused": panes.first().copied().unwrap_or(1),
            "panes": panes.iter().map(|id| new_pane(*id, "SPY", None)).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn splitting_a_leaf_leaves_both_charts_in_the_tree() {
        let tree = json!({"leaf": 1});
        let split = split_leaf(&tree, 1, 2, true);
        assert_eq!(leaves(&split), vec![1, 2]);
        assert_eq!(split["split"]["horizontal"], json!(true));
    }

    #[test]
    fn splitting_a_leaf_inside_a_split_leaves_the_rest_alone() {
        let tree = split_leaf(&json!({"leaf": 1}), 1, 2, true);
        let deeper = split_leaf(&tree, 2, 3, false);
        assert_eq!(leaves(&deeper), vec![1, 2, 3]);
    }

    #[test]
    fn closing_a_chart_hands_its_space_to_its_sibling() {
        let tree = split_leaf(&json!({"leaf": 1}), 1, 2, true);
        assert_eq!(leaves(&remove_leaf(&tree, 2).unwrap()), vec![1]);
    }

    #[test]
    fn closing_the_only_chart_leaves_nothing_to_write_down() {
        assert!(remove_leaf(&json!({"leaf": 1}), 1).is_none());
    }

    /// A split's ratio is where somebody dragged the divider. Closing a chart
    /// elsewhere in the tree must not quietly centre it again.
    #[test]
    fn a_divider_somebody_moved_stays_where_they_put_it() {
        let mut tree = split_leaf(&json!({"leaf": 1}), 1, 2, true);
        tree["split"]["ratio"] = json!(0.3);
        let deeper = split_leaf(&tree, 2, 3, false);
        let pruned = remove_leaf(&deeper, 3).unwrap();
        assert_eq!(pruned["split"]["ratio"], json!(0.3));
    }

    /// Two books naming the same pane is what collapsed a layout before, so
    /// ids are handed out from across every book rather than from one.
    #[test]
    fn a_new_chart_never_takes_an_id_another_chartbook_is_using() {
        let workspace = Workspace {
            value: json!({
                "books": [book(vec![1, 2], json!({"leaf": 1})), book(vec![7], json!({"leaf": 7}))],
                "active": 0,
            }),
        };
        assert_eq!(workspace.next_pane(), 8);
    }

    /// Ids are handed out afresh every time an arrangement is rebuilt, so the
    /// only thing that still names the same chart afterwards is where it sits.
    #[test]
    fn a_chart_is_named_by_where_it_sits_as_well_as_by_its_id() {
        let tree = split_leaf(&json!({"leaf": 4}), 4, 9, true);
        let book = json!({
            "layout": tree,
            "focused": 9,
            "panes": [new_pane(4, "SPY", None), new_pane(9, "QQQ", None)],
        });
        assert_eq!(resolve_pane(&book, Some("pos:0")).unwrap(), 4);
        assert_eq!(resolve_pane(&book, Some("pos:1")).unwrap(), 9);
        assert_eq!(resolve_pane(&book, Some("9")).unwrap(), 9);
        assert_eq!(resolve_pane(&book, None).unwrap(), 9, "no target means the focused one");
        assert_eq!(position_of(&book, 9), Some(1));
        assert_eq!(resolve_pane(&book, Some("pos:7")).unwrap_err().code, EXIT_NOT_FOUND);
    }

    #[test]
    fn a_chartbook_nobody_renamed_answers_to_its_position() {
        let workspace = Workspace {
            value: json!({"books": [book(vec![1], json!({"leaf": 1}))], "active": 0}),
        };
        assert_eq!(workspace.name_of(0), "Chartbook 1");
        assert_eq!(workspace.find("chartbook 1").unwrap(), 0);
    }

    #[test]
    fn two_chartbooks_with_one_name_have_to_be_told_apart_by_id() {
        let mut named = book(vec![1], json!({"leaf": 1}));
        named["name"] = json!("Macro");
        let workspace = Workspace {
            value: json!({"books": [named.clone(), named], "active": 0}),
        };
        let fault = workspace.find("Macro").unwrap_err();
        assert!(fault.message.contains("id:0"), "{}", fault.message);
    }

    /// The window gains fields over time, and the CLI is not rebuilt in step
    /// with somebody's database. Editing a book must carry through whatever
    /// it did not recognise.
    #[test]
    fn editing_a_chartbook_keeps_fields_this_version_knows_nothing_about() {
        let store = Store::memory().unwrap();
        store.set_setting(
            SETTING,
            &json!({
                "books": [{
                    "name": "Macro",
                    "layout": {"leaf": 1},
                    "focused": 1,
                    "panes": [{"id": 1, "symbol": "SPY", "something_new": 42}],
                    "a_field_from_the_future": true,
                }],
                "active": 0,
            })
            .to_string(),
        );

        let mut workspace = Workspace::load(&store);
        workspace.book_mut(0).unwrap()["name"] = json!("Rates");
        workspace.save(&store);

        let back = Workspace::load(&store);
        assert_eq!(back.book(0).unwrap()["a_field_from_the_future"], json!(true));
        assert_eq!(back.book(0).unwrap()["panes"][0]["something_new"], json!(42));
        assert_eq!(back.name_of(0), "Rates");
    }
}
