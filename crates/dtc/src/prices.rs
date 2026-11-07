//! Bars and ticks from Sierra's historical data server, its second port
//! (11098 by default), which takes one request per connection and closes.
//!
//! Sierra serves what it is entitled to serve, and that excludes CME: asked
//! for ES it answers "Request is not authorized." on this machine, local
//! file or not, and fetches nothing — a contract with no file stays without
//! one. So this is for what a service does let through, and an app reads
//! CME bars from Sierra's own files instead. Compression is not asked for;
//! Sierra's logon here says "Compression not supported." regardless.

use std::io;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::link;
use crate::wire::{self, Message};
use crate::{Endpoint, Error};

/// What each record spans: `HistoricalDataIntervalEnum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interval {
    /// Every trade.
    Tick,
    /// Bars of this many seconds: 1, 2, 4, 5, 10, 30, 60, 300, 600, 900,
    /// 1800, 3600, 7200, 86400 or 604800, the values the header names.
    Seconds(u32),
}

impl Interval {
    fn code(self) -> u32 {
        match self {
            Interval::Tick => 0,
            Interval::Seconds(seconds) => seconds,
        }
    }
}

/// One record, as the server sends it. Prices as they travel: multiply by
/// the contract's [`Definition::price_multiplier`](crate::Definition).
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryRecord {
    Bar {
        /// When it opened, in unix seconds with any fraction the server gave.
        start: f64,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        /// `NumTrades`, which shares its field with `OpenInterest`.
        trades: u32,
        bid_volume: f64,
        ask_volume: f64,
    },
    Tick {
        /// Unix seconds, to the millisecond.
        at: f64,
        price: f64,
        volume: f64,
        /// `AtBidOrAsk`: 1 at the bid, 2 at the ask, 0 unknown.
        at_bid_or_ask: u8,
    },
}

/// `StartDateTime` is microseconds since 1970 or, from an older server,
/// seconds: past this it can only be microseconds.
const MICROSECONDS: i64 = 100_000_000_000;

/// Ask for `symbol` from `from` to `to` (unix seconds; `to` 0 for now) in
/// `interval` records, over a connection of its own.
pub fn history(endpoint: &Endpoint, symbol: &str, interval: Interval, from: i64, to: i64, timeout: Duration) -> Result<Vec<HistoryRecord>, Error> {
    let mut link = link::open(endpoint, crate::REFERENCE_NAME, Duration::from_secs(60), timeout)?;
    let request = Message::new(
        wire::HISTORICAL_PRICE_DATA_REQUEST,
        json!({
            "RequestID": 1,
            "Symbol": symbol,
            "Exchange": "",
            "RecordInterval": interval.code(),
            "StartDateTime": from,
            "EndDateTime": to,
            "MaxDaysToReturn": 0,
            "UseZLibCompression": 0,
            "RequestDividendAdjustedStockData": 0,
            "Integer_1": 0,
        }),
    );
    let found = gather(&mut link, &request, timeout);
    link::log_off(&link.sender, "done");
    found
}

fn gather(link: &mut link::Link, request: &Message, timeout: Duration) -> Result<Vec<HistoryRecord>, Error> {
    link.sender.send(request)?;
    let mut records = Vec::new();
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Error::Timeout);
        }
        let _ = link.reader.set_read_timeout(Some(left));
        let message = match wire::read_message(&mut link.reader, &mut link.framer, &mut link.queued) {
            Ok(message) => message,
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Err(Error::Timeout),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => return Err(Error::Protocol(error.to_string())),
            // One request per connection: the server may close once it is done.
            Err(_) if !records.is_empty() => return Ok(records),
            Err(_) => return Err(Error::Closed),
        };
        match message.kind() {
            wire::HISTORICAL_PRICE_DATA_REJECT => {
                return Err(Error::Rejected(message.text("RejectText").unwrap_or_else(|| "the history request was refused".into())));
            }
            wire::HISTORICAL_PRICE_DATA_RESPONSE_HEADER => {
                if message.flag("UseZLibCompression") || message.flag("UseZLibNGCompression") {
                    return Err(Error::Protocol("the server compressed the records though it was asked not to".into()));
                }
                if message.flag("NoRecordsToReturn") {
                    return Ok(records);
                }
            }
            wire::HISTORICAL_PRICE_DATA_RECORD_RESPONSE => {
                let start = message.int_or_zero("StartDateTime");
                let start = if start > MICROSECONDS { start as f64 / 1e6 } else { start as f64 };
                let last = message.flag("IsFinalRecord");
                // The final record of a batch may be only the marker.
                if !(last && start == 0.0) {
                    records.push(HistoryRecord::Bar {
                        start,
                        open: message.number("OpenPrice").unwrap_or(0.0),
                        high: message.number("HighPrice").unwrap_or(0.0),
                        low: message.number("LowPrice").unwrap_or(0.0),
                        close: message.number("LastPrice").unwrap_or(0.0),
                        volume: message.number("Volume").unwrap_or(0.0),
                        trades: message.int("NumTrades").or(message.int("OpenInterest")).unwrap_or(0) as u32,
                        bid_volume: message.number("BidVolume").unwrap_or(0.0),
                        ask_volume: message.number("AskVolume").unwrap_or(0.0),
                    });
                }
                if last {
                    return Ok(records);
                }
            }
            wire::HISTORICAL_PRICE_DATA_TICK_RECORD_RESPONSE => {
                let at = message.number("DateTime").unwrap_or(0.0);
                let last = message.flag("IsFinalRecord");
                if !(last && at == 0.0) {
                    records.push(HistoryRecord::Tick {
                        at,
                        price: message.number("Price").unwrap_or(0.0),
                        volume: message.number("Volume").unwrap_or(0.0),
                        at_bid_or_ask: message.int_or_zero("AtBidOrAsk") as u8,
                    });
                }
                if last {
                    return Ok(records);
                }
            }
            wire::HISTORICAL_PRICE_DATA_RESPONSE_TRAILER => return Ok(records),
            wire::HEARTBEAT => {
                let _ = link.sender.send(&Message::new(wire::HEARTBEAT, json!({})));
            }
            _ => {}
        }
    }
}
