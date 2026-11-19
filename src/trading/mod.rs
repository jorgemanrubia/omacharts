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
//! - **A data feed** is a row on the Data page. Each login whose trading
//!   service has a data feed has one of its own, named after it, "Rithmic
//!   (Acme Futures)", signed in with that login alone ([`data_feed_of`]).
//!   Its data session opens only once a chart is drawn from it, so a login
//!   no chart uses signs in for orders and P&L only, and none has a second
//!   data session beside its own, which CME would count as a professional's.
//!   A chart about an account is drawn from that account's login's feed
//!   unless somebody picked another for it ([`rules::feed_of`]).
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
//! - `chart_pnl`: whether the charts write P&L as money or in R.
//! - `executions`: an account's fills in a contract, paired into round trips.
//! - `brackets`: an entry's target and stop as typed, turned into the
//!   distances a bracket order is sent with.
//! - `events`: what changed between two books, said one line each.
//! - `order_templates`: named brackets an order is sent with, and how
//!   their exits become sizes and distances in ticks.
//! - `reading`: a chart's bars, indicator values and levels, in numbers.

pub mod services;
pub mod brackets;
pub mod chart_pnl;
pub mod events;
pub mod executions;
mod domain;
#[cfg(any(test, feature = "fakes"))]
pub mod fake;
mod linking;
mod logins;
pub mod order_templates;
pub mod reading;
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
