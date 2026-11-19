//! Prints, and what traded at each price: the tick and volume-at-price
//! model on its own.
//!
//! [`ticks`] holds the prints and folds them into tick, volume and range
//! bars; [`ladders`] holds minute ladders — open, high, low, close, volume
//! and, at each price, the volume that hit the bid and the volume that
//! lifted the offer — and answers ranges of them. Neither knows where the
//! prints come from or where a trading day begins: a provider feeds them,
//! and a calendar is handed in as a [`ladders::TradingDays`].
//!
//! A [`ladders::LadderBar`] is an [`omacharts_studies::AtPrice`], so the
//! studies of volume at price step over it directly.
//!
//! It depends on nothing but the dependency-free indicator arithmetic, so
//! anything that folds trades can link it without building the application
//! around it.

pub mod ladders;
pub mod ticks;
