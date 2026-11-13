//! The trading services, and the one place above the feeds that lists them: the
//! window signs in through [`registry`], everything else finds a login's
//! trading service with [`service_for`], and nothing else in the app knows which
//! trading service it is talking to.

use std::sync::Arc;

use super::{TradingService, Registry};

mod rithmic;
mod sierra;

pub use rithmic::SIGN_OUT_WAIT;

/// Every trading service this build has, in the order "Add a trading service…" offers
/// them.
/// A build with the `fakes` feature has the fake one too, which is offered
/// like any other.
pub fn registry() -> Registry {
    #[allow(unused_mut)]
    let mut services: Vec<Arc<dyn TradingService>> = vec![Arc::new(rithmic::Rithmic), Arc::new(sierra::Sierra)];
    #[cfg(any(test, feature = "fakes"))]
    {
        let fake = super::fake::FakeService::listed();
        // A build with the feature has a day of orders to look at; the
        // tests start from nothing.
        #[cfg(not(test))]
        fake.held.lock().unwrap().insert(String::new(), super::fake::day_of_orders());
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
