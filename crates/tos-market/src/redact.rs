//! Trace-log redaction for the login services, whose frames carry the access
//! token and the refresh token. Those dumps are `tracing::trace`, with
//! `token` and `accessTokenInfo` replaced, so `RUST_LOG=debug` does not print
//! them. This crate logs no bearer token or API key.

use serde_json::Value;

use crate::protocol::Request;

const REDACTED: &str = "<redacted>";
const PARAM_SECRETS: &[&str] = &["token", "authCode"];
const BODY_SECRETS: &[&str] = &["token", "accessTokenInfo"];

pub(crate) fn is_login_service(service: &str) -> bool {
    service == "login" || service == "login/schwab"
}

fn scrub(obj: &mut Value, keys: &[&str]) {
    if let Some(map) = obj.as_object_mut() {
        for k in keys {
            if map.contains_key(*k) {
                map.insert((*k).to_string(), Value::String(REDACTED.into()));
            }
        }
    }
}

/// The outbound frame as it would be logged, with login secrets replaced.
pub(crate) fn outbound(request: &Request, text: &str) -> String {
    if !is_login_service(request.service()) {
        return text.to_string();
    }
    let mut copy = request.clone();
    for item in &mut copy.payload {
        scrub(&mut item.params, PARAM_SECRETS);
    }
    serde_json::to_string(&copy).unwrap_or_else(|_| REDACTED.to_string())
}

/// The inbound frame as it would be logged. Non-payload frames and payloads
/// without a login item pass through untouched.
pub(crate) fn inbound(text: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(text) else {
        return text.to_string();
    };
    let Some(items) = value.get_mut("payload").and_then(Value::as_array_mut) else {
        return text.to_string();
    };
    let mut touched = false;
    for item in items {
        let service = item
            .get("header")
            .and_then(|h| h.get("service"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if is_login_service(service) {
            if let Some(body) = item.get_mut("body") {
                scrub(body, BODY_SECRETS);
                touched = true;
            }
        }
    }
    if touched {
        value.to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::login::{login_request, schwab_login_request};

    #[test]
    fn scrubs_login_request_params() {
        let req = login_request("SECRET-TOKEN");
        let text = serde_json::to_string(&req).unwrap();
        let logged = outbound(&req, &text);
        assert!(!logged.contains("SECRET-TOKEN"), "{logged}");
        assert!(logged.contains(r#""token":"<redacted>""#));
        assert!(logged.contains(r#""domain":"TOS""#));
    }

    #[test]
    fn scrubs_the_schwab_auth_code() {
        let req = schwab_login_request("SECRET-CODE");
        let text = serde_json::to_string(&req).unwrap();
        let logged = outbound(&req, &text);
        assert!(!logged.contains("SECRET-CODE"), "{logged}");
        assert!(logged.contains(r#""authCode":"<redacted>""#));
        assert!(logged.contains(r#""clientId":"TOSWeb""#));
    }

    #[test]
    fn leaves_other_requests_alone() {
        let req = Request::new(
            "quotes",
            "q",
            0,
            serde_json::json!({"token": "not-a-secret"}),
        );
        let text = serde_json::to_string(&req).unwrap();
        assert_eq!(outbound(&req, &text), text);
    }

    #[test]
    fn scrubs_login_response_bodies() {
        let text = r#"{"payload":[{"header":{"service":"login","id":"login","ver":0,"type":"snapshot"},"body":{"authenticationStatus":"OK","token":"SECRET-TOKEN","accessTokenInfo":{"refreshToken":"SECRET-REFRESH"},"userCode":"u1"}}]}"#;
        let logged = inbound(text);
        assert!(!logged.contains("SECRET-TOKEN"), "{logged}");
        assert!(!logged.contains("SECRET-REFRESH"), "{logged}");
        assert!(logged.contains(r#""userCode":"u1""#));

        let schwab = text.replace(r#""service":"login""#, r#""service":"login/schwab""#);
        assert!(!inbound(&schwab).contains("SECRET"));
    }

    #[test]
    fn passes_other_frames_through_verbatim() {
        for text in [
            r#"{"heartbeat":1}"#,
            r#"{"payload":[{"header":{"service":"quotes","id":"q","ver":0,"type":"snapshot"},"body":{"token":"x"}}]}"#,
            "not json",
        ] {
            assert_eq!(inbound(text), text);
        }
    }
}
