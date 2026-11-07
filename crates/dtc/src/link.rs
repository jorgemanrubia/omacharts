//! One connection, opened as DTC opens one: TCP, the binary encoding
//! request asking for JSON, then the logon. Everything that talks to the
//! server — the probe, the trading session, the short-lived reference and
//! history connections — opens through here, so the handshake is written
//! once.

use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::wire::{self, Encoding, Framer, Message};
use crate::{Endpoint, Error, Server};

/// Why a connection did not open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Unopened {
    /// Nothing accepted the connection.
    Nothing(String),
    /// Something answered, and it was not a DTC server.
    NotDtc(String),
    /// The server turned the logon down, in its words.
    Refused(String),
    /// It accepted, then went quiet or hung up before the logon was answered.
    Lost(String),
}

impl From<Unopened> for Error {
    fn from(unopened: Unopened) -> Error {
        match unopened {
            Unopened::Nothing(why) | Unopened::Lost(why) => Error::Unreachable(why),
            Unopened::NotDtc(why) => Error::Protocol(why),
            Unopened::Refused(why) => Error::Refused(why),
        }
    }
}

/// The write half of a connection, shared by everyone who sends.
#[derive(Clone)]
pub(crate) struct Sender(Arc<Mutex<TcpStream>>);

impl Sender {
    pub fn send(&self, message: &Message) -> Result<(), Error> {
        let mut stream = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        stream.write_all(&message.frame()).map_err(|_| Error::Closed)
    }

    /// Close both halves, which also wakes a reader blocked on the socket.
    pub fn shut(&self) {
        let stream = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
}

/// A connection that has logged on.
pub(crate) struct Link {
    pub reader: TcpStream,
    pub sender: Sender,
    pub framer: Framer,
    /// What arrived behind the logon response, in the same read.
    pub queued: Vec<Message>,
    pub server: Server,
}

/// The longest a client name may be: `char ClientName[32]` with its null.
const CLIENT_NAME: usize = 31;

/// Open `endpoint` and log on as `client_name`, asking for a heartbeat every
/// `heartbeat`. `timeout` bounds the connect and each wait for an answer.
pub(crate) fn open(endpoint: &Endpoint, client_name: &str, heartbeat: Duration, timeout: Duration) -> Result<Link, Unopened> {
    let place = endpoint.place();
    let addresses: Vec<_> = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .map_err(|error| Unopened::Nothing(format!("{place}: {error}")))?
        .collect();
    let mut last = format!("{place}: no address");
    let mut connected = None;
    for address in addresses {
        match TcpStream::connect_timeout(&address, timeout) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last = format!("{place}: {error}"),
        }
    }
    let mut stream = connected.ok_or(Unopened::Nothing(last))?;
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(timeout)).map_err(|error| Unopened::Lost(error.to_string()))?;
    let lost = |error: io::Error| match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => Unopened::Lost(format!("{place} accepted the connection and said nothing")),
        _ => Unopened::Lost(format!("{place}: {error}")),
    };

    stream.write_all(&wire::encoding_request(Encoding::Json)).map_err(lost)?;
    let reply = wire::read_encoding_response(&mut stream).map_err(|why| {
        if why.starts_with("no encoding response") { Unopened::Lost(format!("{place}: {why}")) } else { Unopened::NotDtc(why) }
    })?;
    if reply.encoding != Encoding::Json {
        return Err(Unopened::NotDtc(format!("the server offered {:?} encoding, not JSON", reply.encoding)));
    }

    let name: String = client_name.chars().take(CLIENT_NAME).collect();
    let mut logon = json!({
        "ProtocolVersion": wire::PROTOCOL_VERSION,
        "HeartbeatIntervalInSeconds": heartbeat.as_secs().max(1),
        "ClientName": name,
    });
    if let Some(user) = &endpoint.user {
        logon["Username"] = json!(user);
    }
    if let Some(password) = &endpoint.password {
        logon["Password"] = json!(password);
    }
    stream.write_all(&Message::new(wire::LOGON_REQUEST, logon).frame()).map_err(lost)?;

    let mut framer = Framer::default();
    let mut queued = Vec::new();
    let deadline = Instant::now() + timeout;
    let response = loop {
        let message = wire::read_message(&mut stream, &mut framer, &mut queued).map_err(|error| match error.kind() {
            io::ErrorKind::InvalidData => Unopened::NotDtc(error.to_string()),
            _ => lost(error),
        })?;
        match message.kind() {
            wire::LOGON_RESPONSE => break message,
            wire::LOGOFF => {
                return Err(Unopened::Refused(message.text("Reason").unwrap_or_else(|| "the server logged the connection off".into())));
            }
            _ if Instant::now() > deadline => return Err(Unopened::Lost(format!("{place} never answered the logon"))),
            _ => continue,
        }
    };
    if response.int("Result") != Some(wire::LOGON_SUCCESS) {
        return Err(Unopened::Refused(response.text("ResultText").unwrap_or_else(|| "the logon was refused".into())));
    }
    let server = Server::from_logon(&response, reply);
    let writer = stream.try_clone().map_err(|error| Unopened::Lost(error.to_string()))?;
    Ok(Link { reader: stream, sender: Sender(Arc::new(Mutex::new(writer))), framer, queued, server })
}

/// Say goodbye as DTC does, and close.
pub(crate) fn log_off(sender: &Sender, reason: &str) {
    let _ = sender.send(&Message::new(wire::LOGOFF, json!({ "Reason": reason, "DoNotReconnect": 1 })));
    sender.shut();
}

/// A short-lived connection's request: send `request`, gather every message
/// that answers `id` until the last of the batch, or the reject that ends it.
/// For the reference data and history helpers, which open, ask once and close.
pub(crate) fn ask_once(link: &mut Link, request: &Message, id: i64, timeout: Duration, rejects: &[u16]) -> Result<Vec<Message>, Error> {
    link.sender.send(request)?;
    let deadline = Instant::now() + timeout;
    let mut answers = Vec::new();
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
            Err(_) => return Err(Error::Closed),
        };
        if message.kind() == wire::HEARTBEAT {
            let _ = link.sender.send(&Message::new(wire::HEARTBEAT, json!({})));
            continue;
        }
        if message.int("RequestID") != Some(id) {
            continue;
        }
        if rejects.contains(&message.kind()) {
            return Err(Error::Rejected(message.text("RejectText").unwrap_or_else(|| "the server refused the request".into())));
        }
        let last = message.final_in_batch();
        answers.push(message);
        if last {
            return Ok(answers);
        }
    }
}
