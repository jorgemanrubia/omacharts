//! What may be said about a frame, and what may not.
//!
//! Frame dumps are off unless `TOS_TRACE` asks for them, and when they are on
//! every member named like a secret is replaced first, in both directions and
//! at any depth. A frame that is not JSON is not quoted at all — if nothing
//! here can read it, nothing here can say which of its bytes are a token.
//!
//! The same rule covers what is said about a frame that failed to decode:
//! `serde_json::Error`'s own words quote the value it did not expect, and one
//! of the documents this crate decodes is the login reply that carries the
//! access token. [`decode_failure`] is the only thing that may be printed
//! about one.

use std::sync::OnceLock;

use serde_json::Value;

const REDACTED: &str = "<redacted>";

/// Members replaced wherever they appear.
///
/// The login frames carry `token`, `accessTokenInfo.refreshToken` and the
/// one-time `authCode`. The list is matched at every depth and for every
/// service rather than only inside a login body, because a dump is read by
/// somebody debugging the gateway and the gateway is free to move a field: a
/// token under a key this crate has never seen is still a token.
const SECRET_KEYS: &[&str] = &[
    "token",
    "accessToken",
    "accessTokenInfo",
    "refreshToken",
    "authCode",
];

/// Whether `TOS_TRACE` asks for every frame the socket carries on stderr.
///
/// Read once, and off by default: a frame dump is for somebody debugging the
/// gateway by hand, and the redaction below is what makes leaving it reachable
/// safe at all.
pub(crate) fn frames_on_stderr() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TOS_TRACE").is_some_and(|v| !v.is_empty()))
}

pub(crate) fn is_login_service(service: &str) -> bool {
    service == "login" || service == "login/schwab"
}

/// Replaces every member named like a secret, at any depth.
fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, member) in map.iter_mut() {
                if SECRET_KEYS.contains(&key.as_str()) {
                    *member = Value::String(REDACTED.into());
                } else {
                    scrub(member);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}

/// A frame as it may be written to stderr, whichever way it was going.
pub(crate) fn frame(text: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        // Unreadable here means unreadable: there is no way to tell which
        // part of it would be a secret, so none of it is repeated.
        return format!("<unreadable frame of {} bytes>", text.len());
    };
    scrub(&mut value);
    value.to_string()
}

/// What a JSON decode failure may be said about, with the document left out.
///
/// `serde_json::Error`'s `Display` is not safe to print: a data error quotes
/// the value it did not expect — `invalid type: string "…"` — and the value
/// it did not expect comes from the frame. The classification and the
/// position are as much as debugging needs and carry no content.
pub(crate) fn decode_failure(error: &serde_json::Error) -> String {
    format!(
        "{:?} at line {} column {}",
        error.classify(),
        error.line(),
        error.column()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{RawResponseItem, Request};
    use crate::services::login::{login_request, schwab_login_request};

    fn logged(request: &Request) -> String {
        frame(&serde_json::to_string(request).unwrap())
    }

    #[test]
    fn scrubs_login_request_params() {
        let text = logged(&login_request("SECRET-TOKEN"));
        assert!(!text.contains("SECRET-TOKEN"), "{text}");
        assert!(text.contains(r#""token":"<redacted>""#), "{text}");
        assert!(text.contains(r#""domain":"TOS""#), "{text}");
    }

    #[test]
    fn scrubs_the_schwab_auth_code() {
        let text = logged(&schwab_login_request("SECRET-CODE"));
        assert!(!text.contains("SECRET-CODE"), "{text}");
        assert!(text.contains(r#""authCode":"<redacted>""#), "{text}");
        assert!(text.contains(r#""clientId":"TOSWeb""#), "{text}");
    }

    #[test]
    fn scrubs_login_response_bodies() {
        let text = r#"{"payload":[{"header":{"service":"login","id":"login","ver":0,"type":"snapshot"},"body":{"authenticationStatus":"OK","token":"SECRET-TOKEN","accessTokenInfo":{"refreshToken":"SECRET-REFRESH"},"userCode":"u1"}}]}"#;
        let logged = frame(text);
        assert!(!logged.contains("SECRET-TOKEN"), "{logged}");
        assert!(!logged.contains("SECRET-REFRESH"), "{logged}");
        assert!(logged.contains(r#""userCode":"u1""#), "{logged}");

        let schwab = text.replace(r#""service":"login""#, r#""service":"login/schwab""#);
        assert!(!frame(&schwab).contains("SECRET"));
    }

    /// A token is a token wherever the gateway puts it: under another
    /// service's header, at the root, inside an array, nested two objects
    /// down. Scrubbing only the login bodies left every one of these
    /// printable.
    #[test]
    fn a_secret_is_scrubbed_wherever_it_sits() {
        for text in [
            r#"{"token":"SECRET"}"#,
            r#"{"payload":[{"header":{"service":"quotes","id":"q","ver":0,"type":"snapshot"},"body":{"token":"SECRET"}}]}"#,
            r#"{"payload":[{"body":{"session":{"accessTokenInfo":{"refreshToken":"SECRET"}}}}]}"#,
            r#"{"items":[{"accessToken":"SECRET"},{"authCode":"SECRET"}]}"#,
            r#"{"a":{"b":{"c":{"token":"SECRET"}}}}"#,
        ] {
            let logged = frame(text);
            assert!(!logged.contains("SECRET"), "{text} -> {logged}");
            assert!(logged.contains(REDACTED), "{text} -> {logged}");
        }
    }

    #[test]
    fn a_frame_that_cannot_be_read_is_never_quoted() {
        let logged = frame("not json SECRET");
        assert!(!logged.contains("SECRET"), "{logged}");
        assert_eq!(logged, "<unreadable frame of 15 bytes>");
        // A frame with nothing secret in it is still itself.
        assert_eq!(frame(r#"{"heartbeat":1}"#), r#"{"heartbeat":1}"#);
    }

    /// The premise of [`decode_failure`], checked rather than assumed: serde
    /// really does quote the input in a data error, so the error this crate
    /// prints about an undecodable frame cannot be serde's own words.
    #[test]
    fn what_is_said_about_an_undecodable_frame_carries_none_of_it() {
        let error = serde_json::from_str::<Vec<RawResponseItem>>(r#"["SECRET-TOKEN"]"#)
            .expect_err("a string is not a response item");
        assert!(
            error.to_string().contains("SECRET-TOKEN"),
            "serde no longer quotes the input: {error}"
        );
        let said = decode_failure(&error);
        assert!(!said.contains("SECRET"), "{said}");
        assert!(said.contains("Data"), "{said}");
        assert!(said.contains("line 1"), "{said}");
    }
}
