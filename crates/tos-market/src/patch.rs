//! RFC-6902 application keyed by `service_id_ver`, mirroring the SPA's cache.
//!
//! The gateway sends a `snapshot` first and `patch` frames afterwards; every
//! patch applies to the last document seen for the same request key. The
//! patches are ordinary RFC-6902, so they are applied here rather than by a
//! crate: the whole of what the gateway can send is below, and a patch that
//! fails costs the document and a resync either way, so none of the rollback
//! a general implementation does would ever be observed.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::Deserialize;
use serde_json::Value;

use crate::protocol::{RawResponseItem, Response, ResponseType};

#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: HashMap<String, Value>,
    /// Request ids whose stream needs a fresh snapshot, not yet handed out.
    needs_resync: BTreeSet<String>,
    /// Ids handed out by [`DocumentStore::take_needs_resync`] and still
    /// waiting for their snapshot; further broken patches on them are
    /// dropped silently rather than requested again.
    awaiting_snapshot: HashSet<String>,
}

impl DocumentStore {
    fn key(item: &RawResponseItem) -> String {
        format!(
            "{}_{}_{}",
            item.header.service, item.header.id, item.header.ver
        )
    }

    /// Request ids whose document was lost (a patch with no base snapshot,
    /// or one that failed to apply). The caller re-issues the subscription;
    /// each id is returned once until its snapshot arrives.
    pub fn take_needs_resync(&mut self) -> Vec<String> {
        let ids: Vec<String> = std::mem::take(&mut self.needs_resync).into_iter().collect();
        self.awaiting_snapshot.extend(ids.iter().cloned());
        ids
    }

    fn flag_resync(&mut self, id: &str) {
        if !self.awaiting_snapshot.contains(id) {
            self.needs_resync.insert(id.to_string());
        }
    }

    /// Materializes one raw frame into a full-document [`Response`]. Returns
    /// `None` for frame types we do not model and for patches that cannot be
    /// applied — those drop the document and flag the id for a resync instead
    /// of emitting an empty or stale one.
    pub fn apply(&mut self, item: RawResponseItem) -> Option<Response> {
        let kind = item.kind();
        let key = Self::key(&item);
        let RawResponseItem { header, body } = item;
        let body = match kind {
            ResponseType::Snapshot => {
                self.awaiting_snapshot.remove(&header.id);
                self.needs_resync.remove(&header.id);
                self.docs.insert(key, body.clone());
                body
            }
            ResponseType::Patch => {
                let Some(mut doc) = self.docs.remove(&key) else {
                    eprintln!("patch on {key} without a snapshot; dropping it and resyncing");
                    self.flag_resync(&header.id);
                    return None;
                };
                let patches = match body.get("patches") {
                    None => Vec::new(),
                    Some(raw) => match Vec::<Operation>::deserialize(raw) {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!(
                                "undecodable patches on {key}: {e}; dropping the document and resyncing"
                            );
                            self.flag_resync(&header.id);
                            return None;
                        }
                    },
                };
                if let Err(e) = apply(&mut doc, &patches) {
                    eprintln!(
                        "patch on {key} failed: {e}; dropping the document and resyncing"
                    );
                    self.flag_resync(&header.id);
                    return None;
                }
                self.docs.insert(key, doc.clone());
                doc
            }
            ResponseType::Error => body,
            ResponseType::Other => {
                eprintln!("unknown message type {:?} on {key}", header.kind);
                return None;
            }
        };
        Some(Response {
            service: header.service,
            id: header.id,
            ver: header.ver,
            kind,
            body,
        })
    }
}

/// One RFC-6902 operation, tagged on `op` with lowercase names — the shape
/// the RFC specifies and the gateway sends. Members the operation does not
/// name are ignored; an `op` that is not one of these six fails to decode,
/// because skipping an operation would leave a document that looks whole and
/// is wrong.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Operation {
    Add { path: String, value: Value },
    Remove { path: String },
    Replace { path: String, value: Value },
    Move { from: String, path: String },
    Copy { from: String, path: String },
    Test { path: String, value: Value },
}

/// Applies `ops` in order. A failure part-way through leaves the document
/// part-patched, so the caller must discard it — which is what
/// [`DocumentStore::apply`] does with the one it was given.
pub fn apply(doc: &mut Value, ops: &[Operation]) -> Result<(), String> {
    for op in ops {
        match op {
            Operation::Add { path, value } => add(doc, &tokens(path)?, value.clone())?,
            Operation::Remove { path } => {
                take(doc, &tokens(path)?)?;
            }
            Operation::Replace { path, value } => {
                *at_mut(doc, &tokens(path)?)? = value.clone();
            }
            Operation::Test { path, value } => {
                if at(doc, &tokens(path)?)? != value {
                    return Err(format!("test on {path} does not hold"));
                }
            }
            Operation::Move { from, path } => {
                let (from, path) = (tokens(from)?, tokens(path)?);
                // A value moved under itself would have nowhere left to go.
                if path.starts_with(&from[..]) {
                    return Err(format!("{from:?} cannot move inside itself"));
                }
                let value = take(doc, &from)?;
                add(doc, &path, value)?;
            }
            Operation::Copy { from, path } => {
                let value = at(doc, &tokens(from)?)?.clone();
                add(doc, &tokens(path)?, value)?;
            }
        }
    }
    Ok(())
}

/// A JSON Pointer (RFC 6901) as its unescaped tokens. The empty pointer names
/// the document itself and has none. `~1` is unescaped before `~0`, so that
/// `~01` reads as the literal `~1` it encodes rather than as a `/`.
fn tokens(pointer: &str) -> Result<Vec<String>, String> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let rest = pointer
        .strip_prefix('/')
        .ok_or_else(|| format!("pointer {pointer} does not start with /"))?;
    Ok(rest
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect())
}

/// An array index, refusing what the RFC does not allow in that position.
/// `len` itself is addressable only where a value is being inserted.
fn index(token: &str, len: usize, inserting: bool) -> Result<usize, String> {
    if token.len() > 1 && token.starts_with('0') {
        return Err(format!("array index {token} has a leading zero"));
    }
    let i: usize = token
        .parse()
        .map_err(|_| format!("array index {token} is not a number"))?;
    if i > len || (!inserting && i == len) {
        return Err(format!("array index {i} is past the end of {len}"));
    }
    Ok(i)
}

fn at<'v>(doc: &'v Value, path: &[String]) -> Result<&'v Value, String> {
    let mut here = doc;
    for token in path {
        here = match here {
            Value::Object(map) => map.get(token).ok_or_else(|| format!("no member {token}"))?,
            Value::Array(items) => &items[index(token, items.len(), false)?],
            _ => return Err(format!("{token} names nothing inside a leaf")),
        };
    }
    Ok(here)
}

fn at_mut<'v>(doc: &'v mut Value, path: &[String]) -> Result<&'v mut Value, String> {
    let mut here = doc;
    for token in path {
        here = match here {
            Value::Object(map) => map
                .get_mut(token)
                .ok_or_else(|| format!("no member {token}"))?,
            Value::Array(items) => {
                let i = index(token, items.len(), false)?;
                &mut items[i]
            }
            _ => return Err(format!("{token} names nothing inside a leaf")),
        };
    }
    Ok(here)
}

/// Sets a member of an object or inserts into an array. The empty pointer
/// replaces the whole document, and `-` on an array appends to it.
fn add(doc: &mut Value, path: &[String], value: Value) -> Result<(), String> {
    let Some((last, parent)) = path.split_last() else {
        *doc = value;
        return Ok(());
    };
    match at_mut(doc, parent)? {
        Value::Object(map) => {
            map.insert(last.clone(), value);
        }
        Value::Array(items) => {
            let i = if last == "-" {
                items.len()
            } else {
                index(last, items.len(), true)?
            };
            items.insert(i, value);
        }
        _ => return Err(format!("cannot add {last} to a leaf")),
    }
    Ok(())
}

/// Removes what `path` names and hands it back, which is what a move needs.
fn take(doc: &mut Value, path: &[String]) -> Result<Value, String> {
    let Some((last, parent)) = path.split_last() else {
        return Err("the whole document is not removable".into());
    };
    match at_mut(doc, parent)? {
        Value::Object(map) => map
            .remove(last)
            .ok_or_else(|| format!("no member {last} to remove")),
        Value::Array(items) => {
            let i = index(last, items.len(), false)?;
            Ok(items.remove(i))
        }
        _ => Err(format!("cannot remove {last} from a leaf")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{decode_inbound, Inbound};
    use serde_json::json;

    fn items(text: &str) -> Vec<RawResponseItem> {
        match decode_inbound(text).unwrap() {
            Inbound::Payload(items) => items,
            other => panic!("{other:?}"),
        }
    }

    fn one(text: &str) -> RawResponseItem {
        items(text).into_iter().next().unwrap()
    }

    const SNAP: &str = r#"{"payload":[{"header":{"service":"positions","id":"positions","ver":0,"type":"snapshot"},"body":{"items":[{"symbol":"/ESU26","values":{"QUANTITY":1}}]}}]}"#;
    const PATCH: &str = r#"{"payload":[{"header":{"service":"positions","id":"positions","ver":0,"type":"patch"},"body":{"patches":[{"op":"replace","path":"/items/0/values/QUANTITY","value":2}]}}]}"#;

    #[test]
    fn applies_patches_to_the_snapshot() {
        let mut store = DocumentStore::default();
        let r = store.apply(one(SNAP)).unwrap();
        assert_eq!(r.kind, ResponseType::Snapshot);
        let r = store.apply(one(PATCH)).unwrap();
        assert_eq!(r.kind, ResponseType::Patch);
        assert_eq!(r.body["items"][0]["values"]["QUANTITY"], 2);
    }

    #[test]
    fn patch_without_a_snapshot_is_dropped_and_flags_a_resync() {
        let mut store = DocumentStore::default();
        assert!(store.apply(one(PATCH)).is_none());
        assert_eq!(store.take_needs_resync(), vec!["positions".to_string()]);
        // Until the snapshot arrives, more broken patches do not re-request.
        assert!(store.apply(one(PATCH)).is_none());
        assert!(store.take_needs_resync().is_empty());
        // The snapshot restores the stream and later patches apply normally.
        let r = store.apply(one(SNAP)).unwrap();
        assert_eq!(r.kind, ResponseType::Snapshot);
        let r = store.apply(one(PATCH)).unwrap();
        assert_eq!(r.body["items"][0]["values"]["QUANTITY"], 2);
        assert!(store.take_needs_resync().is_empty());
    }

    #[test]
    fn undecodable_patches_resync_instead_of_re_emitting_the_stale_doc() {
        let mut store = DocumentStore::default();
        store.apply(one(SNAP)).unwrap();
        let bad = one(
            r#"{"payload":[{"header":{"service":"positions","id":"positions","ver":0,"type":"patch"},"body":{"patches":"not-a-list"}}]}"#,
        );
        assert!(store.apply(bad).is_none());
        assert_eq!(store.take_needs_resync(), vec!["positions".to_string()]);
    }

    #[test]
    fn failed_patch_drops_the_document_instead_of_emitting_an_empty_one() {
        let mut store = DocumentStore::default();
        store.apply(one(SNAP)).unwrap();
        let bad = one(
            r#"{"payload":[{"header":{"service":"positions","id":"positions","ver":0,"type":"patch"},"body":{"patches":[{"op":"replace","path":"/items/9/values/QUANTITY","value":2}]}}]}"#,
        );
        assert!(store.apply(bad).is_none());
        assert_eq!(store.take_needs_resync(), vec!["positions".to_string()]);
        // The stale document is gone: a good patch has no base either now.
        assert!(store.apply(one(PATCH)).is_none());
        store.apply(one(SNAP)).unwrap();
        assert_eq!(
            store.apply(one(PATCH)).unwrap().body["items"][0]["values"]["QUANTITY"],
            2
        );
    }

    fn ops(value: serde_json::Value) -> Vec<Operation> {
        serde_json::from_value(value).expect("operations")
    }

    /// One patched document per operation the RFC defines, because the
    /// gateway is free to send any of them and a wrong one here would show
    /// up as a chart that is subtly out of date rather than as a failure.
    #[test]
    fn every_operation_the_rfc_defines_does_what_it_says() {
        let mut doc = json!({"a": {"b": 1}, "list": [10, 20, 30]});
        apply(
            &mut doc,
            &ops(json!([
                {"op": "test", "path": "/a/b", "value": 1},
                {"op": "replace", "path": "/a/b", "value": 2},
                {"op": "add", "path": "/a/c", "value": "new"},
                {"op": "add", "path": "/list/1", "value": 15},
                {"op": "add", "path": "/list/-", "value": 40},
                {"op": "remove", "path": "/list/0"},
                {"op": "copy", "from": "/a/c", "path": "/a/d"},
                {"op": "move", "from": "/a/d", "path": "/a/e"},
            ])),
        )
        .unwrap();
        assert_eq!(
            doc,
            json!({"a": {"b": 2, "c": "new", "e": "new"}, "list": [15, 20, 30, 40]})
        );
    }

    #[test]
    fn the_empty_pointer_names_the_document_itself() {
        let mut doc = json!({"a": 1});
        apply(&mut doc, &ops(json!([{"op": "replace", "path": "", "value": [1]}]))).unwrap();
        assert_eq!(doc, json!([1]));
        assert!(apply(&mut doc, &ops(json!([{"op": "remove", "path": ""}]))).is_err());
    }

    /// `~1` is a slash and `~0` a tilde, and unescaping them in that order is
    /// what keeps `~01` the literal `~1` the gateway meant.
    #[test]
    fn escaped_pointer_tokens_name_the_key_they_encode() {
        let mut doc = json!({"a/b": 1, "c~1": 2});
        apply(
            &mut doc,
            &ops(json!([
                {"op": "replace", "path": "/a~1b", "value": 9},
                {"op": "replace", "path": "/c~01", "value": 8},
            ])),
        )
        .unwrap();
        assert_eq!(doc, json!({"a/b": 9, "c~1": 8}));
    }

    /// Every way of naming a location that is not there, because each one
    /// has to fail rather than grow the document somewhere unintended.
    #[test]
    fn an_operation_on_a_location_that_is_not_there_fails() {
        let doc = json!({"list": [1], "leaf": 5});
        for op in [
            json!({"op": "replace", "path": "/list/1", "value": 0}),
            json!({"op": "replace", "path": "/nope", "value": 0}),
            json!({"op": "remove", "path": "/nope"}),
            json!({"op": "remove", "path": "/list/1"}),
            json!({"op": "add", "path": "/list/2", "value": 0}),
            json!({"op": "add", "path": "/list/01", "value": 0}),
            json!({"op": "add", "path": "/list/x", "value": 0}),
            json!({"op": "add", "path": "/leaf/x", "value": 0}),
            json!({"op": "test", "path": "/leaf", "value": 6}),
            json!({"op": "copy", "from": "/nope", "path": "/x"}),
            json!({"op": "move", "from": "/list", "path": "/list/0"}),
            json!({"op": "replace", "path": "no-slash", "value": 0}),
        ] {
            let mut copy = doc.clone();
            assert!(
                apply(&mut copy, &ops(json!([op.clone()]))).is_err(),
                "{op}"
            );
        }
    }

    /// An operation nobody implements must not be read as one that is: the
    /// frame is refused and the stream resyncs.
    #[test]
    fn an_operation_this_does_not_know_is_refused_rather_than_skipped() {
        assert!(serde_json::from_value::<Vec<Operation>>(
            json!([{"op": "increment", "path": "/a", "value": 1}])
        )
        .is_err());
        // Members the operation does not name are no reason to refuse it.
        apply(
            &mut json!({"a": 1}),
            &ops(json!([{"op": "replace", "path": "/a", "value": 2, "extra": true}])),
        )
        .unwrap();
    }

    #[test]
    fn surfaces_error_frames() {
        let mut store = DocumentStore::default();
        let err = items(
            r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":0,"type":"error"},"body":{"message":"Symbol not found"}}]}"#,
        );
        let r = store.apply(err.into_iter().next().unwrap()).unwrap();
        assert!(r.is_error());
        assert_eq!(r.id, "chart-/ES-MIN5");
        assert_eq!(r.error_message(), "Symbol not found");
        assert!(r.into_result().is_err());
    }
}
