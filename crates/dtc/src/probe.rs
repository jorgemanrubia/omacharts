//! Is Sierra Chart's DTC server there, and will it trade? For a settings
//! row: one connection, the handshake and the logon, then logoff. It asks
//! for nothing and sends no order, so it is safe to run whenever the page
//! is shown.

use std::time::Duration;

use crate::link::{self, Unopened};
use crate::wire::{EncodingReply, Message};
use crate::{Encoding, Endpoint};

/// What the logon response says the server is and will do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    /// `ServerName`, such as "SC DTC Server. Build=57532".
    pub name: String,
    /// `ResultText`, which on Sierra names the service it is connected to:
    /// "Connected to SC DTC Protocol server. Service=…".
    pub text: String,
    /// `TradingIsSupported`: Allow Trading is on.
    pub trading: bool,
    /// `OCOOrdersSupported`.
    pub oco: bool,
    /// `BracketOrdersSupported`.
    pub brackets: bool,
    /// `OrderCancelReplaceSupported`: an order can be moved in place.
    pub cancel_replace: bool,
    /// `SecurityDefinitionsSupported`.
    pub definitions: bool,
    /// `HistoricalPriceDataSupported`, which Sierra says only on its
    /// historical data port.
    pub historical: bool,
    /// `MarketDataSupported`.
    pub market_data: bool,
    /// The protocol version the encoding response agreed.
    pub protocol: i32,
    /// The encoding the server agreed to speak.
    pub encoding: Encoding,
}

impl Server {
    pub(crate) fn from_logon(response: &Message, reply: EncodingReply) -> Server {
        Server {
            name: response.text_or_empty("ServerName"),
            text: response.text_or_empty("ResultText"),
            trading: response.flag("TradingIsSupported"),
            oco: response.flag("OCOOrdersSupported"),
            brackets: response.flag("BracketOrdersSupported"),
            cancel_replace: response.int("OrderCancelReplaceSupported").is_none_or(|flag| flag != 0),
            definitions: response.flag("SecurityDefinitionsSupported"),
            historical: response.flag("HistoricalPriceDataSupported"),
            market_data: response.int("MarketDataSupported").is_none_or(|flag| flag != 0),
            protocol: response.int("ProtocolVersion").map(|version| version as i32).unwrap_or(reply.protocol),
            encoding: reply.encoding,
        }
    }

    /// The data or trading service Sierra says it is connected to, from
    /// `Service=` in its logon text.
    pub fn service(&self) -> Option<&str> {
        let start = self.text.find("Service=")? + "Service=".len();
        let rest = &self.text[start..];
        let end = rest.find(['|', ' ', ',']).unwrap_or(rest.len());
        Some(&rest[..end]).filter(|service| !service.is_empty())
    }
}

/// What a [`probe`] found at an endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// Nothing is listening: the server is off, or on another port. Carries
    /// the place it tried, `host:port`.
    Nothing(String),
    /// Something answered that is not a DTC server, and what was wrong.
    NotDtc(String),
    /// The server turned the logon down, in its words: bad credentials, most
    /// often.
    Refused(String),
    /// Logged on.
    Ready(Server),
}

impl std::fmt::Display for Probe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Probe::Nothing(place) => write!(f, "Sierra Chart's DTC server isn't listening on {place}"),
            Probe::NotDtc(_) => write!(f, "Something is listening there, but it isn't a DTC server"),
            Probe::Refused(why) => write!(f, "Sierra Chart refused the logon: {why}"),
            Probe::Ready(server) if !server.trading => {
                write!(f, "Trading isn't allowed — turn on Allow Trading in Sierra Chart Server Settings, [DTC Protocol Server]")
            }
            Probe::Ready(server) => match server.service() {
                Some(service) => write!(f, "Ready to trade through Sierra Chart ({service})"),
                None => write!(f, "Ready to trade through Sierra Chart"),
            },
        }
    }
}

/// Connect to `endpoint`, negotiate, log on, read the answer, log off. Each
/// step waits at most `timeout`.
pub fn probe(endpoint: &Endpoint, timeout: Duration) -> Probe {
    match link::open(endpoint, crate::PROBE_NAME, Duration::from_secs(60), timeout) {
        Ok(link) => {
            link::log_off(&link.sender, "probe");
            Probe::Ready(link.server)
        }
        Err(Unopened::Nothing(_)) => Probe::Nothing(endpoint.place()),
        Err(Unopened::NotDtc(why)) => Probe::NotDtc(why),
        Err(Unopened::Refused(why)) => Probe::Refused(why),
        // It took the connection and then said nothing a DTC server would
        // have said: not a DTC server, for whoever reads the row.
        Err(Unopened::Lost(why)) => Probe::NotDtc(why),
    }
}
