//! Wire format of the wsjson gateway.
//!
//! * Handshake: a bare `{"ver","fmt","heartbeat"}` object right after open; the
//!   gateway answers with a bare `{"session","build","ver"}`.
//! * Requests: `{"payload":[{"header":{service,id,ver},"params":{…}}]}`.
//! * Responses: `{"payload":[{"header":{service,id,ver,type},"body":{…}}]}`
//!   where `type` is `snapshot` | `patch` | `error`.
//! * Heartbeats: bare `{"heartbeat":<ms>}` every ~2 s.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONNECTION_REQUEST_MESSAGE: &str =
    r#"{"ver":"27.*.*","fmt":"json-patches-structured","heartbeat":"2s"}"#;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestHeader {
    pub service: String,
    pub id: String,
    pub ver: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestItem {
    pub header: RequestHeader,
    pub params: Value,
}

/// One outbound envelope. Always a single item — the SPA never batches.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Request {
    pub(crate) payload: [RequestItem; 1],
}

impl Request {
    pub fn new(service: impl Into<String>, id: impl Into<String>, ver: i64, params: Value) -> Self {
        Request {
            payload: [RequestItem {
                header: RequestHeader {
                    service: service.into(),
                    id: id.into(),
                    ver,
                },
                params,
            }],
        }
    }

    pub fn header(&self) -> &RequestHeader {
        &self.payload[0].header
    }

    pub fn service(&self) -> &str {
        &self.header().service
    }

    pub fn id(&self) -> &str {
        &self.header().id
    }

    pub fn params(&self) -> &Value {
        &self.payload[0].params
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseType {
    Snapshot,
    Patch,
    Error,
    Other,
}

impl ResponseType {
    fn parse(s: &str) -> Self {
        match s {
            "snapshot" => ResponseType::Snapshot,
            "patch" => ResponseType::Patch,
            "error" => ResponseType::Error,
            _ => ResponseType::Other,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RawResponseHeader {
    /// Terminal errors such as `session_expired` omit this.
    pub service: String,
    pub id: String,
    pub ver: i64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawResponseItem {
    pub header: RawResponseHeader,
    #[serde(default)]
    pub body: Value,
}

/// A fully materialized response: for `patch` frames the body is the patched
/// document, not the patch list (see [`crate::patch::DocumentStore`]).
///
/// The body is shared rather than owned. A chart document is twenty sessions
/// of candles, and one arrives every three seconds per subscription; the
/// store keeps it to apply the next patch to, and every route for the id is
/// handed it as well. Cloning it for each of those was the one real cost on
/// the socket thread, and it bought nothing — nobody writes to a body they
/// were handed.
#[derive(Debug, Clone)]
pub struct Response {
    pub service: String,
    pub id: String,
    pub ver: i64,
    pub kind: ResponseType,
    pub body: Arc<Value>,
    /// For a patch, the JSON pointers its operations named, in order. What a
    /// subscriber reads to find the candles a tick changed without walking
    /// the arrays; empty for a snapshot, which changed everything.
    pub touched: Arc<[String]>,
}

impl Response {
    pub fn is_error(&self) -> bool {
        self.kind == ResponseType::Error
    }

    /// Did this frame carry the whole document — a snapshot, or a patch that
    /// replaced the root? The gateway answers a changed range on a live id
    /// with the latter, and to a subscriber the two are the same news.
    pub fn is_whole_document(&self) -> bool {
        self.kind == ResponseType::Snapshot || self.touched.iter().any(|path| path.is_empty())
    }

    /// `body.message` of an error frame, or a generic text.
    pub fn error_message(&self) -> String {
        message_of(&self.body)
    }

    pub fn into_result(self) -> crate::Result<Response> {
        if self.is_error() {
            Err(crate::Error::Gateway {
                service: self.service.clone(),
                id: self.id.clone(),
                message: self.error_message(),
            })
        } else {
            Ok(self)
        }
    }
}

/// Everything the gateway can send, classified.
#[derive(Debug, Clone)]
pub enum Inbound {
    Heartbeat(i64),
    Connection {
        session: String,
        build: String,
        ver: String,
    },
    Payload(Vec<RawResponseItem>),
    Unknown(Value),
}

pub fn decode_inbound(text: &str) -> serde_json::Result<Inbound> {
    let mut obj = match serde_json::from_str(text)? {
        Value::Object(obj) => obj,
        other => return Ok(Inbound::Unknown(other)),
    };
    if let Some(hb) = obj.get("heartbeat").and_then(Value::as_i64) {
        if !obj.contains_key("payload") && !obj.contains_key("session") {
            return Ok(Inbound::Heartbeat(hb));
        }
    }
    if obj.contains_key("session") && obj.contains_key("build") && obj.contains_key("ver") {
        let s = |k: &str| {
            obj.get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        return Ok(Inbound::Connection {
            session: s("session"),
            build: s("build"),
            ver: s("ver"),
        });
    }
    // Moved out, not cloned: this runs for every frame the gateway sends.
    if let Some(payload) = obj.remove("payload") {
        return Ok(Inbound::Payload(serde_json::from_value(payload)?));
    }
    Ok(Inbound::Unknown(Value::Object(obj)))
}

/// `body.message` of an error frame, or a generic text.
pub(crate) fn message_of(body: &Value) -> String {
    body.get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown gateway error")
        .to_string()
}

impl RawResponseItem {
    pub fn kind(&self) -> ResponseType {
        ResponseType::parse(&self.header.kind)
    }

    /// Gateway told us this session is dead (`session_expired`, or any
    /// error with `isTerminal`).
    pub fn is_terminal_session_error(&self) -> bool {
        if self.kind() != ResponseType::Error {
            return false;
        }
        if self.header.id == "session_expired" {
            return true;
        }
        self.body
            .get("isTerminal")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_frames() {
        assert!(matches!(
            decode_inbound(r#"{"heartbeat":1}"#).unwrap(),
            Inbound::Heartbeat(1)
        ));
        assert!(matches!(
            decode_inbound(r#"{"session":"s","build":"b","ver":"v"}"#).unwrap(),
            Inbound::Connection { .. }
        ));
        match decode_inbound(
            r#"{"payload":[{"header":{"service":"quotes","id":"q","ver":0,"type":"snapshot"},"body":{"items":[]}}]}"#,
        )
        .unwrap()
        {
            Inbound::Payload(items) => {
                assert_eq!(items[0].header.service, "quotes");
                assert_eq!(items[0].kind(), ResponseType::Snapshot);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            decode_inbound(r#"{"x":1}"#).unwrap(),
            Inbound::Unknown(_)
        ));
    }

    #[test]
    fn session_expired_without_service_is_a_terminal_error() {
        let inbound = decode_inbound(
            r#"{"payload":[{"header":{"id":"session_expired","ver":0,"type":"error"},"body":{"message":"Session has expired. Please log in again.","isTerminal":true}}]}"#,
        )
        .unwrap();
        match inbound {
            Inbound::Payload(items) => {
                assert!(items[0].is_terminal_session_error());
                assert_eq!(items[0].header.service, "");
                assert_eq!(items[0].header.id, "session_expired");
            }
            other => panic!("{other:?}"),
        }
    }

    /// An error frame reaches the caller as the gateway's own sentence and
    /// nothing else from the body. Worth pinning: [`crate::Error::Gateway`]
    /// ends up in a log line and on a chart, and an error frame is one of the
    /// shapes that can arrive carrying a token beside the message.
    #[test]
    fn a_gateway_error_carries_the_message_and_none_of_the_rest_of_the_body() {
        let body = json!({
            "message": "No such symbol: ZZZZ",
            "token": "SECRET-TOKEN",
            "accessTokenInfo": {"refreshToken": "SECRET-REFRESH"},
        });
        assert_eq!(message_of(&body), "No such symbol: ZZZZ");
        let response = Response {
            service: "chart".into(),
            id: "chart-ZZZZ-MIN5".into(),
            ver: 1,
            kind: ResponseType::Error,
            body: Arc::new(body),
            touched: Vec::new().into(),
        };
        let error = response.into_result().expect_err("an error frame");
        let said = error.to_string();
        assert!(said.contains("No such symbol"), "{said}");
        assert!(!said.contains("SECRET"), "{said}");
    }

    #[test]
    fn request_serializes_as_single_payload_item() {
        let req = Request::new("login", "login", 0, json!({"token": "t"}));
        let text = serde_json::to_string(&req).unwrap();
        assert_eq!(
            text,
            r#"{"payload":[{"header":{"service":"login","id":"login","ver":0},"params":{"token":"t"}}]}"#
        );
    }
}
