//! The trading services, and the one place above the feeds that lists them: the
//! window signs in through [`registry`], everything else finds a login's
//! trading service with [`service_for`], and nothing else in the app knows which
//! trading service it is talking to.

use std::sync::Arc;

use super::{TradingService, Registry};

mod rithmic;
mod rithmic_sessions;
pub mod rithmic_systems;
mod sierra;

pub use rithmic::SIGN_OUT_WAIT;

/// Start keeping the record each trading service's terms ask for of the
/// sessions it opens: for Rithmic, every plant's session id
/// ([`rithmic_sessions`]). Once, at startup.
pub fn keep_session_records() {
    rithmic_sessions::keep();
}

/// Every trading service this build has, in the order "Add a trading service…" offers
/// them.
/// A build with the `fakes` feature has the fake one too, which is offered
/// like any other.
pub fn registry() -> Registry {
    #[allow(unused_mut)]
    let mut services: Vec<Arc<dyn TradingService>> = vec![Arc::new(rithmic::Rithmic), Arc::new(sierra::Sierra)];
    #[cfg(any(test, feature = "fakes"))]
    {
        #[allow(unused_mut)]
        let mut fake = super::fake::FakeService::listed();
        // A build with the feature has a day of orders to look at, a
        // different one for its second login, on an account numbered after
        // the user typed; the tests start from nothing, on FAKE-1.
        #[cfg(not(test))]
        {
            fake.numbered_by_user = true;
            fake.in_turn = vec![super::fake::day_of_orders(), super::fake::another_day()];
            fake.held.lock().unwrap().insert(String::new(), super::fake::day_of_orders());
        }
        #[cfg(not(test))]
        if let Some(path) = std::env::var_os(super::fake::PAST_FILLS_ENV) {
            fake.past.lock().unwrap().insert(String::new(), super::fake::past_fills(std::path::Path::new(&path)));
        }
        services.push(Arc::new(fake));
    }
    Registry::of(services)
}

/// The trading service logins call `id`.
pub fn service_for(id: &str) -> Option<Arc<dyn TradingService>> {
    registry().get(id)
}
