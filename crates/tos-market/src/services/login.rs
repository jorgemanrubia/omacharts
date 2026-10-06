use serde::Deserialize;
use serde_json::json;

use crate::protocol::Request;

/// Token login: `login` with an access token captured from the browser.
pub fn login_request(access_token: &str) -> Request {
    Request::new(
        "login",
        "login",
        0,
        json!({
            "token": access_token,
            "domain": "TOS",
            "platform": "PROD",
            "tag": "TOSWeb",
        }),
    )
}

/// Code login: `login/schwab` with the one-time code from `getAuthCode`.
/// The SPA sends this shape for both the OAuth redirect and a trading-system
/// switch (`clientId` is `zt.lmsApiKey`, which is `"TOSWeb"`).
pub fn schwab_login_request(auth_code: &str) -> Request {
    Request::new(
        "login/schwab",
        "login/schwab",
        0,
        json!({
            "clientId": "TOSWeb",
            "tag": "TOSWeb",
            "redirectUri": "https://trade.thinkorswim.com/oauth",
            "authCode": auth_code,
        }),
    )
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccessTokenInfo {
    pub refresh_token: Option<String>,
}

/// Body of a `login` response: the fields the session keeps.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LoginBody {
    pub message: Option<String>,
    pub authentication_status: String,
    pub authenticated: bool,
    pub user_code: String,
    pub token: String,
    pub access_token_info: Option<AccessTokenInfo>,
}

impl LoginBody {
    /// `login` reports `authenticationStatus: "OK"`; `login/schwab` reports
    /// `authenticated: true`. Either means a usable session.
    pub fn successful(&self) -> bool {
        self.authentication_status == "OK" || self.authenticated
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.access_token_info
            .as_ref()
            .and_then(|i| i.refresh_token.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schwab_login_matches_the_spa_switch() {
        let req = schwab_login_request("code-1");
        assert_eq!(req.service(), "login/schwab");
        assert_eq!(req.id(), "login/schwab");
        assert_eq!(req.header().ver, 0);
        assert_eq!(
            req.params(),
            &json!({
                "clientId": "TOSWeb",
                "tag": "TOSWeb",
                "redirectUri": "https://trade.thinkorswim.com/oauth",
                "authCode": "code-1",
            })
        );
    }
}
