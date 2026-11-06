//! What a contract is: its tick, its tick's worth, when it expires. For a
//! contract list and for pricing orders right, not for charts — Sierra's
//! DTC server refuses CME market data, and the bars come from its files.
//!
//! Sierra names a contract in full, "ESZ26_FUT_CME". Asked for "ESZ26" it
//! answers with an empty definition, and asked for a symbol it does not
//! know it answers nothing at all, so an unknown symbol is a
//! [`Error::Timeout`] here, or [`Error::Rejected`] when the definition that
//! came back is empty. Listing an underlying's contracts leaves out their
//! expiry dates, so [`definitions`] asks for each one's own definition too.
//! It asks for futures: Sierra lists nothing unless the request names a
//! security type.

use std::time::Duration;

use serde_json::json;

use crate::link::{self, Link};
use crate::wire::{self, Message};
use crate::{Endpoint, Error};

/// What kind of thing a symbol is: `SecurityTypeEnum`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityKind {
    Future,
    Stock,
    Forex,
    Index,
    FuturesStrategy,
    FutureOption,
    StockOption,
    IndexOption,
    Bond,
    MutualFund,
    Unknown,
}

impl SecurityKind {
    fn of(code: i64) -> SecurityKind {
        match code {
            1 => SecurityKind::Future,
            2 => SecurityKind::Stock,
            3 => SecurityKind::Forex,
            4 => SecurityKind::Index,
            5 => SecurityKind::FuturesStrategy,
            6 => SecurityKind::StockOption,
            7 => SecurityKind::FutureOption,
            8 => SecurityKind::IndexOption,
            9 => SecurityKind::Bond,
            10 => SecurityKind::MutualFund,
            _ => SecurityKind::Unknown,
        }
    }
}

/// A contract, from a SECURITY_DEFINITION_RESPONSE.
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    /// Sierra's name for it, which orders use: "ESZ26_FUT_CME".
    pub symbol: String,
    /// The exchange, when the server says; Sierra's does not, and puts it in
    /// the symbol.
    pub exchange: Option<String>,
    /// The exchange's own name for it: "ESZ26".
    pub exchange_symbol: Option<String>,
    /// The root it is a contract of: "ES".
    pub underlying: Option<String>,
    pub description: String,
    pub kind: SecurityKind,
    /// `MinPriceIncrement`, in real prices: 0.25 for ES.
    pub tick_size: f64,
    /// `PriceDisplayFormat`: decimal places 0 to 9, or a fractional format
    /// number from DTCProtocol.h; `None` when unset.
    pub price_display_format: Option<i32>,
    /// `CurrencyValuePerIncrement`: what one tick is worth, 12.5 for ES.
    pub currency_value_per_increment: f64,
    pub currency: Option<String>,
    /// `SecurityExpirationDate`, in unix seconds.
    pub expiration: Option<i64>,
    /// `RolloverDate`, in unix seconds.
    pub rollover: Option<i64>,
    /// `DisplayPriceMultiplier`: what a price on the wire is multiplied by to
    /// be a real price. Sierra's own futures service sends CME prices as
    /// integers of hundredths, 765125 for 7651.25, and says 0.01 here; 1 for
    /// a server that sends real prices.
    pub price_multiplier: f64,
}

impl Definition {
    pub(crate) fn from_message(message: &Message) -> Option<Definition> {
        let symbol = message.text("Symbol")?;
        let tick_size = message.nonzero("MinPriceIncrement").unwrap_or(0.0);
        let kind = SecurityKind::of(message.int_or_zero("SecurityType"));
        // Sierra's answer for a symbol spelled short is a name and nothing
        // else.
        if tick_size == 0.0 && kind == SecurityKind::Unknown {
            return None;
        }
        Some(Definition {
            symbol,
            exchange: message.text("Exchange"),
            exchange_symbol: message.text("ExchangeSymbol"),
            underlying: message.text("UnderlyingSymbol"),
            description: message.text_or_empty("Description"),
            kind,
            tick_size: round_float(tick_size),
            price_display_format: message.int("PriceDisplayFormat").map(|format| format as i32).filter(|format| *format >= 0),
            currency_value_per_increment: round_float(message.nonzero("CurrencyValuePerIncrement").unwrap_or(0.0)),
            currency: message.text("Currency"),
            expiration: message.int("SecurityExpirationDate").filter(|at| *at > 0),
            rollover: message.int("RolloverDate").filter(|at| *at > 0),
            price_multiplier: message.nonzero("DisplayPriceMultiplier").map(round_float).unwrap_or(1.0),
        })
    }

    /// The contract month a future's exchange symbol names, as (year, month):
    /// "ESZ26" is (2026, 12). From the symbol, because a list of an
    /// underlying's contracts does not carry their expiry.
    pub fn contract_month(&self) -> Option<(i32, u32)> {
        let name = self.exchange_symbol.as_deref().unwrap_or_else(|| self.symbol.split('_').next().unwrap_or(""));
        contract_month(name)
    }
}

/// The month and year at the end of a future's name: "ESZ26" → (2026, 12),
/// "6EH7" → (2027, 3) by the nearest decade to come.
pub fn contract_month(name: &str) -> Option<(i32, u32)> {
    let digits = name.chars().rev().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 2 || name.len() <= digits {
        return None;
    }
    let (head, year) = name.split_at(name.len() - digits);
    let month = match head.chars().last()? {
        'F' => 1,
        'G' => 2,
        'H' => 3,
        'J' => 4,
        'K' => 5,
        'M' => 6,
        'N' => 7,
        'Q' => 8,
        'U' => 9,
        'V' => 10,
        'X' => 11,
        'Z' => 12,
        _ => return None,
    };
    let year: i32 = year.parse().ok()?;
    let year = if digits == 2 {
        2000 + year
    } else {
        // One digit: the first year at or after this one that ends in it.
        let now = 1970 + (crate::now() / 31_556_952) as i32;
        let decade = now - now % 10;
        if decade + year >= now - 1 { decade + year } else { decade + 10 + year }
    };
    Some((year, month))
}

/// A float that travelled as a 32-bit float, 0.009999999776482582, back to
/// the decimal it was written as.
fn round_float(value: f64) -> f64 {
    let rounded = (value * 1e7).round() / 1e7;
    if (rounded - value).abs() < 1e-6 { rounded } else { value }
}

/// How long one definition may take when [`definitions`] fills in expiries.
const EACH: Duration = Duration::from_secs(3);

/// The most contracts [`definitions`] asks one by one for their expiry; past
/// that a list is given as it came.
const FILL_IN: usize = 40;

/// Every contract of `underlying` the server knows ("ES"), each with its
/// expiry, over a connection of its own that trades nothing.
pub fn definitions(endpoint: &Endpoint, underlying: &str, timeout: Duration) -> Result<Vec<Definition>, Error> {
    let mut link = link::open(endpoint, crate::REFERENCE_NAME, Duration::from_secs(60), timeout)?;
    let found = list(&mut link, underlying, timeout);
    let found = found.map(|mut found| {
        if found.len() <= FILL_IN {
            for definition in found.iter_mut().filter(|definition| definition.expiration.is_none()) {
                if let Ok(full) = one(&mut link, &definition.symbol, EACH.min(timeout)) {
                    *definition = Definition { underlying: definition.underlying.clone().or(full.underlying.clone()), ..full };
                }
            }
        }
        found
    });
    link::log_off(&link.sender, "done");
    found
}

/// One contract's definition by Sierra's name for it, over a connection of
/// its own.
pub fn definition(endpoint: &Endpoint, symbol: &str, timeout: Duration) -> Result<Definition, Error> {
    let mut link = link::open(endpoint, crate::REFERENCE_NAME, Duration::from_secs(60), timeout)?;
    let found = one(&mut link, symbol, timeout);
    link::log_off(&link.sender, "done");
    found
}

const REJECTS: &[u16] = &[wire::SECURITY_DEFINITION_REJECT];

/// `SECURITY_TYPE_FUTURES`. Sierra lists an underlying's contracts only when
/// the request names a security type: with none it refuses, and with
/// `SECURITY_TYPE_UNSET` it answers nothing at all.
const FUTURES: i64 = 1;

fn list(link: &mut Link, underlying: &str, timeout: Duration) -> Result<Vec<Definition>, Error> {
    let request = Message::new(
        wire::SYMBOLS_FOR_UNDERLYING_REQUEST,
        json!({ "RequestID": 1, "UnderlyingSymbol": underlying, "Exchange": "", "SecurityType": FUTURES }),
    );
    match link::ask_once(link, &request, 1, timeout, REJECTS) {
        Ok(answers) => Ok(answers.iter().filter_map(Definition::from_message).collect()),
        // Nothing by that name is an empty list, not a failure.
        Err(Error::Rejected(_)) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn one(link: &mut Link, symbol: &str, timeout: Duration) -> Result<Definition, Error> {
    let id = 2 + i64::from(symbol.len() as u16);
    let request = Message::new(wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST, json!({ "RequestID": id, "Symbol": symbol, "Exchange": "" }));
    let answers = link::ask_once(link, &request, id, timeout, REJECTS)?;
    answers
        .iter()
        .find_map(Definition::from_message)
        .ok_or_else(|| Error::Rejected(format!("Sierra Chart has no contract named {symbol}")))
}

/// A market data snapshot: the optional extra, which Sierra refuses for CME,
/// CBOT, NYMEX and COMEX ("Market data request not allowed").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quote {
    pub bid: Option<f64>,
    pub ask: Option<f64>,
    pub last: Option<f64>,
    pub settlement: Option<f64>,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub volume: Option<f64>,
    /// When it last traded, in unix seconds.
    pub last_at: Option<f64>,
}

/// Ask for `symbol`'s market data snapshot, then unsubscribe. Prices as they
/// travel: multiply by the contract's [`Definition::price_multiplier`].
pub fn quote(endpoint: &Endpoint, symbol: &str, timeout: Duration) -> Result<Quote, Error> {
    let mut link = link::open(endpoint, crate::REFERENCE_NAME, Duration::from_secs(60), timeout)?;
    let found = snapshot(&mut link, symbol, timeout);
    let _ = link.sender.send(&Message::new(
        wire::MARKET_DATA_REQUEST,
        json!({ "RequestAction": wire::UNSUBSCRIBE, "SymbolID": 1, "Symbol": symbol, "Exchange": "" }),
    ));
    link::log_off(&link.sender, "done");
    found
}

fn snapshot(link: &mut Link, symbol: &str, timeout: Duration) -> Result<Quote, Error> {
    link.sender.send(&Message::new(
        wire::MARKET_DATA_REQUEST,
        json!({ "RequestAction": wire::SUBSCRIBE, "SymbolID": 1, "Symbol": symbol, "Exchange": "" }),
    ))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(Error::Timeout);
        }
        let _ = link.reader.set_read_timeout(Some(left));
        let message = wire::read_message(&mut link.reader, &mut link.framer, &mut link.queued).map_err(|error| match error.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => Error::Timeout,
            _ => Error::Closed,
        })?;
        match message.kind() {
            wire::MARKET_DATA_REJECT => {
                return Err(Error::Rejected(message.text("RejectText").unwrap_or_else(|| "market data refused".into())));
            }
            wire::MARKET_DATA_SNAPSHOT => {
                return Ok(Quote {
                    bid: message.nonzero("BidPrice"),
                    ask: message.nonzero("AskPrice"),
                    last: message.nonzero("LastTradePrice"),
                    settlement: message.nonzero("SessionSettlementPrice"),
                    open: message.nonzero("SessionOpenPrice"),
                    high: message.nonzero("SessionHighPrice"),
                    low: message.nonzero("SessionLowPrice"),
                    volume: message.nonzero("SessionVolume"),
                    last_at: message.nonzero("LastTradeDateTime"),
                });
            }
            _ => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(value: serde_json::Value) -> Message {
        Message(value.as_object().unwrap().clone())
    }

    #[test]
    fn sierra_s_definition_of_es_reads_with_its_tick_its_worth_and_its_expiry() {
        // As Sierra's server sent it, zero fields left out.
        let definition = Definition::from_message(&message(json!({
            "Type":507,"RequestID":140,"SecurityType":1,"MinPriceIncrement":0.25,"PriceDisplayFormat":2,
            "CurrencyValuePerIncrement":12.5,"IsFinalMessage":1,"SecurityExpirationDate":1797206400,
            "DisplayPriceMultiplier":0.009999999776482582,"RolloverDate":1797206400,"SecurityIdentifier":10252,
            "Symbol":"ESZ26_FUT_CME","Description":"E-MINI S&P 500 FUTURES ES Dec 2026","UnderlyingSymbol":"ES",
            "ExchangeSymbol":"ESZ26","Currency":"USD"
        })))
        .unwrap();
        assert_eq!(definition.symbol, "ESZ26_FUT_CME");
        assert_eq!(definition.kind, SecurityKind::Future);
        assert_eq!(definition.tick_size, 0.25);
        assert_eq!(definition.currency_value_per_increment, 12.5);
        assert_eq!(definition.expiration, Some(1797206400));
        assert_eq!(definition.price_multiplier, 0.01);
        assert_eq!(definition.price_display_format, Some(2));
        assert_eq!(definition.contract_month(), Some((2026, 12)));
    }

    #[test]
    fn the_empty_definition_a_short_symbol_gets_is_no_definition() {
        let empty = message(json!({"Type":507,"RequestID":51,"IsFinalMessage":1,"DisplayPriceMultiplier":1,"SecurityIdentifier":10,"Symbol":"ESZ26","Currency":""}));
        assert_eq!(Definition::from_message(&empty), None);
    }

    #[test]
    fn a_contract_month_is_read_from_the_end_of_its_name() {
        assert_eq!(contract_month("ESH27"), Some((2027, 3)));
        assert_eq!(contract_month("MESZ26"), Some((2026, 12)));
        assert_eq!(contract_month("ES"), None);
        assert_eq!(contract_month("AAPL"), None);
    }
}
