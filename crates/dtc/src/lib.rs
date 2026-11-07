//! A DTC Protocol client for Sierra Chart's own DTC server, for what it is
//! good for here: the accounts Sierra is connected to, what they hold and
//! have working, placing and managing orders through whatever service
//! Sierra itself trades on (simulated or live), and reference data — each
//! contract's tick, its worth, its expiry.
//!
//! Not market data. Sierra's DTC server refuses CME, CBOT, NYMEX and COMEX
//! data ("Market data request not allowed"; its historical port says
//! "Request is not authorized."), so charts are drawn from Sierra's own
//! files and this crate never needs to be fast.
//!
//! The server is turned on in Sierra's "Sierra Chart Server Settings"
//! window (from Global Settings), tab "[DTC Protocol Server]": "Enable DTC
//! Protocol Server", its "Listening Port" (11099; the historical data port
//! is 11098), and "Allow Trading", without which no trading message is
//! answered. "Require Authentication" makes the logon need Sierra's own
//! username and password, and is forced on when "Allowed Incoming IPs" is
//! "Any IP".
//!
//! **JSON, not binary.** A connection opens with an ENCODING_REQUEST in
//! binary, whatever it asks for, and from the ENCODING_RESPONSE on speaks
//! what was agreed. This asks for JSON: null-terminated objects whose
//! fields are named as DTCProtocol.h names the struct members. The traffic
//! is a handful of trading messages a second at the very most, so what an
//! encoding costs to write is beside the point. What matters is that named
//! fields survive the protocol changing under them: the binary structs are
//! laid out under `#pragma pack(8)`, with some under `pack(1)`, and grow
//! fields at their ends between versions, so a struct read with the wrong
//! layout reads wrong silently, where a JSON field that moved is still found
//! by name and one that is new is ignored. Sierra's own server shows why it
//! matters: it leaves out zero fields in a definition, writes an unset
//! price as `null`, and sends heartbeat fields the shipped header does not
//! have — all harmless in JSON. And a captured session reads as text: the
//! fixtures under `tests/fixtures` are two, recorded on this machine with
//! the market open and closed. One rule JSON does impose: Sierra reads
//! `Type` and then the fields after it, so a field written before `Type` is
//! dropped without a word, and every message here is written `Type` first.
//!
//! **One connection, one thread.** A [`Trading`] session holds one
//! connection, read by one thread that also sends the heartbeat when it is
//! due and notices a server silent for twice the interval. Requests write on
//! the same socket and wait, with a timeout, for the thread to fold the
//! answers into the session's [`State`]. [`probe`], [`definitions`],
//! [`quote`] and [`history`] open a short-lived connection of their own and
//! close it, with no thread.
//!
//! **Reconnecting.** A dropped session says so ([`Pulse::Lost`]),
//! reconnects after a wait that doubles from half a second up to thirty,
//! asks for every account's balance, positions, working orders and fills
//! again, and then says [`Pulse::Back`]. Nothing placed is ever resent. A
//! logon the server refuses is never retried, nor is a session it logs off
//! saying not to reconnect: the same login would only be refused again.
//!
//! **What Sierra's DTC server does not do**, from its documentation, its
//! header and this machine's server:
//!
//! - No CME-group market data, live or historical (above).
//! - The order types are market, limit, stop and stop-limit (with market-
//!   and limit-if-touched in the header besides); time in force day, good
//!   till cancelled, good till a date, IOC, all-or-none, fill-or-kill.
//! - OCO pairs and brackets exist only as placed together
//!   (SUBMIT_NEW_OCO_ORDER, with a parent for a bracket), which
//!   [`Trading::place_pair`] sends for a pair with no parent. There is no
//!   message that links orders already working — CANCEL_REPLACE_ORDER
//!   changes prices, quantity and time in force, never a link — so "make
//!   these two OCO" after the fact cannot be done. A pair's partial fills
//!   come off the other (`PartialFillHandling`); there is no reduce-only,
//!   and `OpenOrClose` "is not used for futures". The logon says whether
//!   OCO is supported (`OCOOrdersSupported`, [`Server::oco`]); this
//!   machine's Sierra says it is.
//! - CANCEL_REPLACE_ORDER moves `Price1` of a stop-limit only: Sierra moves
//!   `Price2`, the limit, by the same amount, keeping the distance.
//! - Every price on a trading message is in the service's own units: for
//!   CME on Sierra's futures service, hundredths, with the contract's
//!   `DisplayPriceMultiplier` (0.01) saying so. This crate converts both
//!   ways; tick sizes (`MinPriceIncrement`) are already real.
//! - Sierra's simulated accounts ("Sim1") have no balance over DTC; a
//!   broker's account has cash, open and daily P&L, and its daily loss
//!   limit. Positions carry `OpenProfitLoss` when the server works it out.
//! - Fills history needs an account (it refuses "TradeAccount is not set").
//! - The server ignores `Exchange` and names a contract in full,
//!   "ESZ26_FUT_CME"; it answers nothing at all for a symbol it does not
//!   know, so such a request ends in [`Error::Timeout`]. Listing a root's
//!   contracts needs a security type, and leaves out their expiries.
//! - Market data, and a closed market, push no feed or trading status
//!   messages: a quiet session is heartbeats and nothing else, which is not
//!   a drop.
//!
//! **Nobody's password leaves here except to the server.** [`Endpoint`]'s
//! `Debug` leaves it out, and no [`Error`] is built from it.

mod link;
mod prices;
mod probe;
mod reference;
mod trading;
pub mod wire;

#[cfg(any(test, feature = "fake"))]
pub mod fake;

#[cfg(test)]
mod tests;

pub use prices::{HistoryRecord, Interval, history};
pub use probe::{Probe, Server, probe};
pub use reference::{Definition, Quote, SecurityKind, contract_month, definition, definitions, quote};
pub use trading::{
    Account, Balance, Book, Fill, NewOrder, Order, OrderKind, Position, Pulse, Side, State, Status, TimeInForce, Timing, Trading,
};
pub use wire::Encoding;

/// The client name a [`probe`] logs on as.
const PROBE_NAME: &str = "omacharts probe";

/// The client name the short-lived reference and history connections log
/// on as.
const REFERENCE_NAME: &str = "omacharts reference";

/// Where Sierra's DTC server is, and who to log on as when it asks.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    /// Only when the server has "Require Authentication" on: Sierra's own
    /// username.
    pub user: Option<String>,
    pub password: Option<String>,
}

impl Default for Endpoint {
    /// Sierra's own default: this computer, port 11099, no credentials.
    fn default() -> Endpoint {
        Endpoint { host: "127.0.0.1".into(), port: 11099, user: None, password: None }
    }
}

impl Endpoint {
    /// `host:port`, for words.
    pub fn place(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl std::fmt::Debug for Endpoint {
    /// Everything but the password, which must never reach a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "…"))
            .finish()
    }
}

/// What went wrong, in words a person can be shown. Built from what the
/// server said and from this crate's own sentences, never from a password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Nothing answered, or the connection dropped.
    Unreachable(String),
    /// The server turned the logon down, or will not trade, in its words or
    /// ours. Signing in again as things stand would only be refused again.
    Refused(String),
    /// The server said no to a request or an order, in its words.
    Rejected(String),
    /// The server did not answer in time.
    Timeout,
    /// The session was signed out while this was waiting.
    Closed,
    /// Something answered that does not speak DTC as expected.
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreachable(why) => write!(f, "could not reach Sierra Chart: {why}"),
            Error::Refused(why) => write!(f, "Sierra Chart refused: {why}"),
            Error::Rejected(why) => write!(f, "Sierra Chart rejected it: {why}"),
            Error::Timeout => write!(f, "Sierra Chart did not answer in time"),
            Error::Closed => write!(f, "the Sierra Chart session was signed out"),
            Error::Protocol(why) => write!(f, "Sierra Chart's DTC server said something unexpected: {why}"),
        }
    }
}

impl std::error::Error for Error {}

/// Now, in unix seconds.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn a_password_is_never_in_what_an_endpoint_prints() {
        let endpoint = Endpoint { user: Some("trader".into()), password: Some("SECRET".into()), ..Endpoint::default() };
        let printed = format!("{endpoint:?}");
        assert!(!printed.contains("SECRET"), "{printed}");
        assert!(printed.contains("trader"), "{printed}");
    }

    #[test]
    fn the_default_endpoint_is_sierra_s_own_default() {
        assert_eq!(Endpoint::default().place(), "127.0.0.1:11099");
    }
}
