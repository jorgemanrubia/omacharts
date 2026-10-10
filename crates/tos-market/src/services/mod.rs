//! Chart candles, and the login that opens the socket.
//!
//! And the list of what else this crate may ask the gateway for, which is
//! nothing. [`ALLOWED_SERVICES`] is the whole of it.

pub mod chart;
pub mod login;

use crate::error::{Error, Result};

/// Every service this crate will send a request for.
///
/// The gateway behind a thinkorswim session is the one the platform's own
/// order entry talks to, so "this crate charts and places no orders" has to
/// be a property of the code rather than a promise about it. This is where it
/// becomes one: [`assert_allowed`] is called on the way into
/// [`crate::client::Client::route`] and again inside the single function that
/// writes a frame, so a request for `order` — or for anything else somebody
/// adds later without meaning to — is refused before it reaches the socket
/// instead of being refused by nobody having written it yet.
pub const ALLOWED_SERVICES: &[&str] = &["chart", "login", "login/schwab"];

/// Whether a service is one this crate sends.
pub fn is_allowed(service: &str) -> bool {
    ALLOWED_SERVICES.contains(&service)
}

/// Refuses a service that is not on [`ALLOWED_SERVICES`].
pub fn assert_allowed(service: &str) -> Result<()> {
    if is_allowed(service) {
        return Ok(());
    }
    Err(Error::ForbiddenService(service.to_string()))
}

/// A chart patch with no base document is safe to ask for again.
pub const REPLAYABLE_SERVICES: &[&str] = &["chart"];

pub fn is_replayable(service: &str) -> bool {
    REPLAYABLE_SERVICES.contains(&service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::chart::{chart_request, ChartParams};
    use crate::services::login::{login_request, schwab_login_request};

    /// What the list is for, said as a test: the services that place and
    /// manage orders are not on it, and a chart feed has no use for them.
    #[test]
    fn nothing_that_touches_an_order_is_on_the_list() {
        for service in [
            "order",
            "trade",
            "orders",
            "order/place",
            "positions",
            "balances",
            "alerts",
            "",
        ] {
            assert!(!is_allowed(service), "{service}");
            assert!(matches!(
                assert_allowed(service),
                Err(Error::ForbiddenService(ref s)) if s == service
            ));
        }
        // Three, and adding a fourth has to come through this test.
        assert_eq!(ALLOWED_SERVICES, &["chart", "login", "login/schwab"]);
    }

    /// Every request this crate knows how to build asks for a service on the
    /// list — so the list cannot quietly stop describing the code.
    #[test]
    fn every_request_this_crate_builds_asks_for_an_allowed_service() {
        for request in [
            chart_request(&ChartParams::new("/ES", "MIN5", "DAY1")),
            login_request("token"),
            schwab_login_request("code"),
        ] {
            assert!(
                is_allowed(request.service()),
                "{} is not on the allowlist",
                request.service()
            );
            assert!(assert_allowed(request.service()).is_ok());
        }
    }

    /// And the constructors tested above are all of them: every
    /// `Request::new` in this module names its service as a literal, and
    /// every one of those literals is on the list. A constructor added
    /// without a thought about the list fails here.
    #[test]
    fn no_request_in_this_module_names_a_service_off_the_list() {
        let sources = [
            ("chart.rs", include_str!("chart.rs")),
            ("login.rs", include_str!("login.rs")),
        ];
        let mut found = 0;
        for (name, source) in sources {
            for tail in source.split("Request::new(").skip(1) {
                let service = tail
                    .split('"')
                    .nth(1)
                    .unwrap_or_else(|| panic!("{name}: a Request::new with no literal service"));
                assert!(is_allowed(service), "{name}: {service} is not allowed");
                found += 1;
            }
        }
        assert_eq!(found, 3, "the three constructors this crate has");
    }
}
