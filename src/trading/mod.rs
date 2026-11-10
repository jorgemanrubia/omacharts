//! Trading: the trading service logins on this machine, the accounts inside them, and
//! which account a chart is about.
//!
//! Three things, kept apart so the trading service an order goes to and the feed a
//! chart is drawn from can be combined freely:
//!
//! - **A login** is a trading service connection: a name the trader chooses, the
//!   trading service it is with, its user ID and whatever else the trading service's dialog
//!   asks, and the password in the trading service's keyring under the login's id.
//!   Any number of them, in [`LOGINS`], `Local` because a login belongs to
//!   the machine it was typed on.
//! - **An account** is one of the account numbers inside a login, listed by
//!   the order plant once it signs in. They are cached in [`ACCOUNTS`] as
//!   bookkeeping, never in a backup, so the settings page and the header
//!   draw without connecting and account numbers stay on this machine.
//! - **A data feed** is a row on the Data page. Exactly one login is the
//!   data login, [`DATA_LOGIN`], the first added whose trading service has a data
//!   feed, unless somebody picks another: that feed signs in through it, and
//!   the others sign in for orders and P&L only. Several logins streaming quotes could count
//!   as several data sessions under CME's two-session rule.
//!
//! Each login has at most one trading session, because a second sign-in on
//! the same login forces the first one out: [`Desk`] holds them by login and
//! refuses to open a second.
//!
//! Which account a chart is about is decided in one place, [`account_of`],
//! for the header and the orders alike, so the two can never disagree.
//!
//! The files, so that each kind of thing has one place:
//!
//! - `domain`: the neutral types every trading service is mapped into.
//! - `rules`: what follows from them, with no store and no trading service.
//! - `linking`: a stop and a target linked with whatever the trading service has,
//!   and the order of the steps that keeps the position safe meanwhile.
//! - `words`: what the menu, the toasts and the pills say.
//! - `logins`: what is stored about logins and their accounts.
//! - `session`: the seam for trading services, [`TradingService`] and [`Session`], and the
//!   [`Desk`] that holds every login's session.
//! - `services`: the trading services themselves, and [`registry`], the one place
//!   that lists them.
//! - `fake`: a trading service in memory, for the tests, and in the registry of a
//!   build with the `fakes` feature.
//! - `sizes`: trading sizes and their templates.

pub mod services;
mod domain;
#[cfg(any(test, feature = "fakes"))]
pub mod fake;
mod linking;
mod logins;
mod rules;
mod session;
pub mod sizes;
mod words;

pub use services::*;
pub use domain::*;
pub use linking::*;
pub use logins::*;
pub use rules::*;
pub use session::*;
pub use words::*;
