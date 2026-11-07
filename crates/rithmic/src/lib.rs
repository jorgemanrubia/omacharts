//! A Rithmic login, as a charting app needs one: signed in once, asked for
//! bars, and watched for trades.
//!
//! Rithmic is reached over R | Protocol, protocol buffers on a WebSocket,
//! which `rithmic-rs` speaks. Its plants are separate logins: the ticker
//! plant for live prices and reference data, the history plant for bars.
//! Everything here is the protocol's business, so the app above it needs
//! neither tokio nor anything from `rithmic-rs`: it calls plain blocking
//! functions and is handed plain values.
//!
//! **One connection per login.** Rithmic signs a session out when the same
//! login signs in again, so whatever a login is wanted for goes through the
//! one [`Connection`] that [`connection`] keeps for it, by the login's id.
//! It owns a tokio runtime on two threads of its own, built the first time
//! anything asks and not before, and lends two halves of itself: the data
//! half ([`Client`]: ticker and history plants) and the trading half
//! ([`Trading`]: order and PnL plants). Opening one does not open the
//! other's plants. Only one login supplies market data — CME bills a
//! non-professional subscriber as a professional after two concurrent data
//! sessions — so every chart shares its one data half; any number of logins
//! can trade. [`shutdown`] closes the data half, [`Trading::sign_out`] the
//! trading half, [`close`] both, and with both closed the connection's
//! threads stop.
//!
//! **It looks after its own connection.** A drop is reported to everything
//! watching ([`Live::Lost`], [`Pulse::Lost`]), the half reconnects with a
//! doubling wait both halves share, subscribes again to what is still
//! watched, and says so ([`Live::Watching`] again, [`Pulse::Back`]), which
//! is a caller's cue to load a fresh snapshot or reconcile its orders. A
//! login Rithmic refuses is not retried: the same password would only be
//! refused again, and a storm of refused logins is exactly what Rithmic's
//! conformance review looks for. Nor is a forced logout, which is what
//! Rithmic does to a session when the same login signs in somewhere else:
//! it stops both halves, because signing back in would log the other one
//! out, and the two would take turns for ever.
//!
//! **Nobody's password leaves here except to the gateway.** [`Credentials`]'
//! `Debug` leaves it out, no [`Error`] is built from one, and nothing in this
//! crate logs.

mod client;
mod connection;
pub mod keyring;
mod login;
mod trading;

pub use client::{
    choose_front_month_with, entitled_exchanges, on_entitlements, on_front_month, on_reference, price_limits, Client,
    Expiry, Reference, Watch,
};
pub use connection::{close, connection, open_data, running, shutdown, Connection, SIGN_OUT_WAIT};
pub use login::{Login, DEV_LOGIN};
pub use trading::{
    listing, Account, AccountPnl, Book, Fill, Kind, Limits, ListReply, Listing, Order, Position, Pulse, Status, Trading,
    WorkingOrder,
};

/// The app signing in, as Rithmic is told it: its name and its version.
///
/// The caller's, never this crate's. Rithmic Test takes any name;
/// production takes only the one Rithmic registered for the app at
/// conformance, and the version is the app's release, which a library's own
/// version is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct App {
    pub name: &'static str,
    pub version: &'static str,
}

/// Rithmic's test system, which every developer kit login can use.
pub const TEST_SYSTEM: &str = "Rithmic Test";
pub const TEST_URL: &str = "wss://rituz00100.rithmic.com:443";

/// Who to sign in as, and where.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub user: String,
    pub password: String,
    /// The Rithmic system, such as "Rithmic Test" or a prop firm's own.
    pub system: String,
    /// The gateway's WebSocket URL.
    pub url: String,
}

impl std::fmt::Debug for Credentials {
    /// Everything but the password, which must never reach a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("user", &self.user)
            .field("system", &self.system)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// What went wrong, in words a person can be shown.
///
/// Built from what Rithmic said and from this crate's own sentences, never
/// from a login: nothing here can carry a password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// There is nothing to sign in with: no user saved, or no password for it
    /// in the keyring.
    NoLogin(String),
    /// Rithmic turned the login down, in its own words ("permission
    /// denied"). Signing in again with something else is the only fix.
    Refused(String),
    /// The gateway could not be reached, or the connection dropped.
    Connection(String),
    /// Rithmic did not answer in time.
    Timeout,
    /// Rithmic answered and said no to the request, in its own words.
    Rejected(String),
    /// Rithmic has nothing by this name.
    NotFound(String),
    /// The session was signed out while this was waiting.
    Closed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoLogin(why) => write!(f, "{why}"),
            Error::Refused(why) => write!(f, "Rithmic refused the login: {why}"),
            Error::Connection(why) => write!(f, "could not reach Rithmic: {why}"),
            Error::Timeout => write!(f, "Rithmic did not answer in time"),
            Error::Rejected(why) => write!(f, "Rithmic refused the request: {why}"),
            Error::NotFound(what) => write!(f, "Rithmic has no {what}"),
            Error::Closed => write!(f, "the Rithmic session was signed out"),
        }
    }
}

impl std::error::Error for Error {}

/// How long each bar of a request lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Minutes(u32),
    Days(u32),
}

/// One bar, as the history plant sends it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// When it opened, in unix seconds. A daily bar opens at midnight UTC of
    /// the trading date it is named for.
    pub start: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
}

/// One minute of volume at price, as the history plant sends it: its bar,
/// and for each price that traded the volume and trade count on each side.
/// The arrays run side by side, one entry per price.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LadderMinute {
    /// When it opened, in unix seconds.
    pub start: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
    pub prices: Vec<f64>,
    /// Sell-aggressor volume: sellers hitting the bid.
    pub bid: Vec<u64>,
    /// Buy-aggressor volume: buyers lifting the offer.
    pub ask: Vec<u64>,
    /// Volume with no known aggressor.
    pub unknown: Vec<u64>,
    pub bid_trades: Vec<u32>,
    pub ask_trades: Vec<u32>,
    pub unknown_trades: Vec<u32>,
}

/// Which side started a trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// A buyer lifted the offer.
    Buy,
    /// A seller hit the bid.
    Sell,
}

/// One trade on a watched contract.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trade {
    pub price: f64,
    pub size: u64,
    /// `None` when the exchange did not say.
    pub aggressor: Option<Side>,
    /// When the exchange matched it, in nanoseconds since 1970.
    pub at_nanos: i64,
}

/// What a watch hears, in the order the session hears it.
#[derive(Clone, Debug, PartialEq)]
pub enum Live {
    /// Trades on `contract` follow from here. Sent when the watch starts and
    /// again after every reconnect, and after trades were missed, so it is
    /// also the cue to load a snapshot: anything from before it may be
    /// incomplete.
    Watching { contract: String },
    Trade(Trade),
    /// The last trade before the watch began, which a subscription opens
    /// with: the market's price until it trades again. A finished bar from
    /// the history plant already counts it; the bar forming now, which the
    /// plant does not give, does not.
    Last(Trade),
    /// Nothing more until the next [`Live::Watching`], and this is why. The
    /// session is reconnecting by itself unless the error says it cannot
    /// ([`Error::Refused`], [`Error::NoLogin`], [`Error::Closed`]).
    Lost(Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_never_in_what_credentials_print() {
        let credentials = Credentials {
            user: "trader".into(),
            password: "SECRET".into(),
            system: TEST_SYSTEM.into(),
            url: TEST_URL.into(),
        };
        let printed = format!("{credentials:?}");
        assert!(!printed.contains("SECRET"), "{printed}");
        assert!(printed.contains("trader"), "{printed}");
    }
}
