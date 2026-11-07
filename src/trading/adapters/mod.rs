//! The brokers, and the one place above the feeds that lists them: the
//! window signs in through [`registry`], everything else finds a login's
//! broker with [`broker_for`], and nothing else in the app knows which
//! broker it is talking to.

use std::sync::Arc;

use super::{Broker, Registry};

mod rithmic;
mod sierra;

pub use rithmic::SIGN_OUT_WAIT;

/// Every broker this build has, in the order "Add a trading account…" offers
/// them.
/// A build with the `fakes` feature has the fake one too, which is offered
/// like any other.
pub fn registry() -> Registry {
    #[allow(unused_mut)]
    let mut brokers: Vec<Arc<dyn Broker>> = vec![Arc::new(rithmic::Rithmic), Arc::new(sierra::Sierra)];
    #[cfg(any(test, feature = "fakes"))]
    {
        let fake = super::fake::FakeBroker::listed();
        // A build with the feature has a day of orders to look at; the
        // tests start from nothing.
        #[cfg(not(test))]
        fake.held.lock().unwrap().insert(String::new(), super::fake::day_of_orders());
        brokers.push(Arc::new(fake));
    }
    Registry::of(brokers)
}

/// The broker logins call `id`.
pub fn broker_for(id: &str) -> Option<Arc<dyn Broker>> {
    registry().get(id)
}
