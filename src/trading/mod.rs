//! Trading: the broker logins on this machine, the accounts inside them, and
//! which account a chart is about.
//!
//! Three things, kept apart so the broker an order goes to and the feed a
//! chart is drawn from can be combined freely:
//!
//! - **A login** is a broker connection: a name the trader chooses, the
//!   Rithmic user ID, system and gateway, and the password in the keyring
//!   under the login's id. Any number of them, in [`LOGINS`], `Local`
//!   because a login belongs to the machine it was typed on.
//! - **An account** is one of the account numbers inside a login, listed by
//!   the order plant once it signs in. They are cached in [`ACCOUNTS`] as
//!   bookkeeping, never in a backup, so the settings page and the header
//!   draw without connecting and account numbers stay on this machine.
//! - **A data feed** is a source on the Data page. Exactly one login is the
//!   data login, [`DATA_LOGIN`], the first added unless somebody picks
//!   another: Rithmic's data source signs in through it, and the others sign
//!   in for orders and P&L only. Several logins streaming quotes could count
//!   as several data sessions under CME's two-session rule.
//!
//! Each login has at most one trading session, because a second sign-in on
//! the same login forces the first one out: [`Desk`] holds them by login and
//! refuses to open a second.
//!
//! Which account a chart is about is decided in one place, [`account_of`],
//! for the header now and for the order ticket later, so the two can never
//! disagree.
//!
//! The files, so that each kind of thing has one place:
//!
//! - `domain`: the neutral types every broker is mapped into.
//! - `rules`: what follows from them, with no store and no broker.
//! - `words`: what the menu, the toasts and the pills say.
//! - `logins`: what is stored about logins and their accounts.
//! - `session`: the seam for brokers, [`Broker`] and [`Session`], and the
//!   [`Desk`] that holds every login's session.
//! - `adapters`: the brokers themselves, the one place that names them.
//! - `fake`: a broker in memory, for the tests.
//! - `sizes`: trading sizes and their templates.

pub mod adapters;
mod domain;
#[cfg(any(test, feature = "fakes"))]
pub mod fake;
mod logins;
mod rules;
mod session;
pub mod sizes;
mod words;

pub use adapters::*;
pub use domain::*;
pub use logins::*;
pub use rules::*;
pub use session::*;
pub use words::*;
