//! The brokers, and the one place above the feeds that names them: the
//! window signs in through what is listed here, and nothing else in the app
//! knows which broker it is talking to.

mod rithmic;

pub use rithmic::{Rithmic, SIGN_OUT_WAIT, TEST_GATEWAY, TEST_SYSTEM, data_refusal};

/// The feed a login's market data comes through: the data login's, the one
/// a chart on one of its accounts is drawn from unless somebody chose
/// another.
pub const LOGIN_FEED: &str = "rithmic";
