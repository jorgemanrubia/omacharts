//! Each plant's session, as Rithmic numbers it: the `unique_user_id` a
//! login response carries. Rithmic's conformance review asks for these,
//! with when the session ran, to look from its side at what the app did.
//!
//! Nothing in this crate logs, so a session is told to whoever asked with
//! [`on_session`], and the app keeps the record.

use std::sync::OnceLock;
use std::time::SystemTime;

use rithmic_rs::RithmicResponse;

use crate::Credentials;
use rithmic_rs::rti::messages::RithmicMessage;

/// One plant signed in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// "ticker", "history", "order" or "pnl".
    pub plant: &'static str,
    /// Rithmic's `unique_user_id` for this session.
    pub id: String,
    /// The system signed in to, such as "Rithmic Test".
    pub system: String,
    /// The gateway's WebSocket URL.
    pub gateway: String,
    pub at: SystemTime,
}

type Heard = Box<dyn Fn(&Session) + Send + Sync>;

static HEARD: OnceLock<Heard> = OnceLock::new();

/// Be told of every plant's session as it signs in. Once per process; a
/// second call is ignored.
pub fn on_session(heard: impl Fn(&Session) + Send + Sync + 'static) {
    let _ = HEARD.set(Box::new(heard));
}

/// The session a login response names, if it names one.
fn of(plant: &'static str, credentials: &Credentials, message: &RithmicMessage) -> Option<Session> {
    let RithmicMessage::ResponseLogin(login) = message else { return None };
    let id = login.unique_user_id.as_deref().filter(|id| !id.is_empty())?;
    Some(Session {
        plant,
        id: id.to_string(),
        system: credentials.system.clone(),
        gateway: credentials.url.clone(),
        at: SystemTime::now(),
    })
}

/// Tell whoever asked that `plant` signed in.
pub(crate) fn heard(plant: &'static str, credentials: &Credentials, response: &RithmicResponse) {
    if let (Some(heard), Some(session)) = (HEARD.get(), of(plant, credentials, &response.message)) {
        heard(&session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rithmic_rs::rti::ResponseLogin;

    fn test_login() -> Credentials {
        Credentials {
            user: "trader".into(),
            password: "secret".into(),
            system: "Rithmic Test".into(),
            url: crate::TEST_URL.into(),
        }
    }

    #[test]
    fn a_login_response_names_its_session() {
        let mut login = ResponseLogin::default();
        login.unique_user_id = Some("8f2c-11".into());
        let session = of("order", &test_login(), &RithmicMessage::ResponseLogin(login)).unwrap();
        assert_eq!((session.plant, session.id.as_str(), session.system.as_str()), ("order", "8f2c-11", "Rithmic Test"));
        assert_eq!(session.gateway, crate::TEST_URL);
        assert!(!format!("{session:?}").contains("secret"), "no password in a session");

        let mut nameless = ResponseLogin::default();
        nameless.unique_user_id = Some(String::new());
        assert_eq!(of("order", &test_login(), &RithmicMessage::ResponseLogin(nameless)), None);
    }
}
