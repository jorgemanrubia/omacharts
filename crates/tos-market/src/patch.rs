//! RFC-6902 application keyed by `service_id_ver`, mirroring the SPA's cache.
//!
//! The gateway sends a `snapshot` first and `patch` frames afterwards; every
//! patch applies to the last document seen for the same request key.

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
                    Some(raw) => match Vec::<json_patch::PatchOperation>::deserialize(raw) {
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
                if let Err(e) = json_patch::patch(&mut doc, &patches) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{decode_inbound, Inbound};

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
