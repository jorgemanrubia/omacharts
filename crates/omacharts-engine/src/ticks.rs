//! Trades, in memory only: the current session's prints for one contract.
//!
//! The buffer, the folds into tick, volume and range bars, and the live
//! tape are `omacharts-ticks`, re-exported here whole.

pub use omacharts_ticks::ticks::*;
