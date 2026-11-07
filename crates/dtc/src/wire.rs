//! The bytes on the socket: the binary encoding handshake every DTC
//! connection opens with, and the null-terminated JSON objects that follow.
//!
//! A message is kept as the JSON object it arrived as, read through a few
//! lenient accessors, rather than decoded into one struct per message type.
//! Sierra's server leaves fields out when they are zero (its security
//! definitions do), writes an unset double as `null` or as `-DBL_MAX`, and a
//! newer build adds fields an older header never heard of. An accessor that
//! reads a missing field as unset copes with all three; a derived struct
//! would need every one of those cases spelled out field by field.

use std::io::{self, Read};

use serde_json::{Map, Value};

/// The protocol version this client speaks: `CURRENT_VERSION` in Sierra's
/// DTCProtocol.h.
pub const PROTOCOL_VERSION: i32 = 8;

// Message types, as DTCProtocol.h numbers them.
pub const LOGON_REQUEST: u16 = 1;
pub const LOGON_RESPONSE: u16 = 2;
pub const HEARTBEAT: u16 = 3;
pub const LOGOFF: u16 = 5;
pub const ENCODING_REQUEST: u16 = 6;
pub const ENCODING_RESPONSE: u16 = 7;
pub const MARKET_DATA_REQUEST: u16 = 101;
pub const MARKET_DATA_REJECT: u16 = 103;
pub const MARKET_DATA_SNAPSHOT: u16 = 104;
pub const SUBMIT_NEW_OCO_ORDER: u16 = 201;
pub const SUBMIT_NEW_SINGLE_ORDER: u16 = 208;
pub const SUBMIT_FLATTEN_POSITION_ORDER: u16 = 209;
pub const CANCEL_ORDER: u16 = 203;
pub const CANCEL_REPLACE_ORDER: u16 = 204;
pub const OPEN_ORDERS_REQUEST: u16 = 300;
pub const ORDER_UPDATE: u16 = 301;
pub const OPEN_ORDERS_REJECT: u16 = 302;
pub const HISTORICAL_ORDER_FILLS_REQUEST: u16 = 303;
pub const HISTORICAL_ORDER_FILL_RESPONSE: u16 = 304;
pub const CURRENT_POSITIONS_REQUEST: u16 = 305;
pub const POSITION_UPDATE: u16 = 306;
pub const CURRENT_POSITIONS_REJECT: u16 = 307;
pub const HISTORICAL_ORDER_FILLS_REJECT: u16 = 308;
pub const TRADE_ACCOUNTS_REQUEST: u16 = 400;
pub const TRADE_ACCOUNT_RESPONSE: u16 = 401;
pub const SYMBOLS_FOR_EXCHANGE_REQUEST: u16 = 502;
pub const SYMBOLS_FOR_UNDERLYING_REQUEST: u16 = 504;
pub const SECURITY_DEFINITION_FOR_SYMBOL_REQUEST: u16 = 506;
pub const SECURITY_DEFINITION_RESPONSE: u16 = 507;
pub const SECURITY_DEFINITION_REJECT: u16 = 509;
pub const ACCOUNT_BALANCE_UPDATE: u16 = 600;
pub const ACCOUNT_BALANCE_REQUEST: u16 = 601;
pub const ACCOUNT_BALANCE_REJECT: u16 = 602;
pub const HISTORICAL_PRICE_DATA_REQUEST: u16 = 800;
pub const HISTORICAL_PRICE_DATA_RESPONSE_HEADER: u16 = 801;
pub const HISTORICAL_PRICE_DATA_REJECT: u16 = 802;
pub const HISTORICAL_PRICE_DATA_RECORD_RESPONSE: u16 = 803;
pub const HISTORICAL_PRICE_DATA_TICK_RECORD_RESPONSE: u16 = 804;
pub const HISTORICAL_PRICE_DATA_RESPONSE_TRAILER: u16 = 807;

// LogonStatusEnum.
pub const LOGON_SUCCESS: i64 = 1;

// OrderStatusEnum.
pub const ORDER_STATUS_UNSPECIFIED: i64 = 0;
pub const ORDER_STATUS_ORDER_SENT: i64 = 1;
pub const ORDER_STATUS_PENDING_OPEN: i64 = 2;
pub const ORDER_STATUS_PENDING_CHILD: i64 = 3;
pub const ORDER_STATUS_OPEN: i64 = 4;
pub const ORDER_STATUS_PENDING_CANCEL_REPLACE: i64 = 5;
pub const ORDER_STATUS_PENDING_CANCEL: i64 = 6;
pub const ORDER_STATUS_FILLED: i64 = 7;
pub const ORDER_STATUS_CANCELED: i64 = 8;
pub const ORDER_STATUS_REJECTED: i64 = 9;
pub const ORDER_STATUS_PARTIALLY_FILLED: i64 = 10;

// OrderUpdateReasonEnum.
pub const NEW_ORDER_ACCEPTED: i64 = 2;
pub const ORDER_FILLED: i64 = 4;
pub const ORDER_FILLED_PARTIALLY: i64 = 5;
pub const ORDER_CANCELED: i64 = 6;
pub const ORDER_CANCEL_REPLACE_COMPLETE: i64 = 7;
pub const NEW_ORDER_REJECTED: i64 = 8;
pub const ORDER_CANCEL_REJECTED: i64 = 9;
pub const ORDER_CANCEL_REPLACE_REJECTED: i64 = 10;

// OrderTypeEnum.
pub const ORDER_TYPE_MARKET: i64 = 1;
pub const ORDER_TYPE_LIMIT: i64 = 2;
pub const ORDER_TYPE_STOP: i64 = 3;
pub const ORDER_TYPE_STOP_LIMIT: i64 = 4;

// TimeInForceEnum.
/// `PartialFillHandlingEnum`: a part of one of an OCO pair filled takes
/// as much off the other.
pub const PARTIAL_FILL_HANDLING_REDUCE_QUANTITY: i64 = 1;

pub const TIF_DAY: i64 = 1;
pub const TIF_GOOD_TILL_CANCELED: i64 = 2;

// BuySellEnum.
pub const BUY: i64 = 1;
pub const SELL: i64 = 2;

// RequestActionEnum.
pub const SUBSCRIBE: i64 = 1;
pub const UNSUBSCRIBE: i64 = 2;

/// How the two ends write their messages after the handshake:
/// `EncodingEnum` in DTCProtocol.h.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Binary,
    BinaryVariableLength,
    Json,
    JsonCompact,
    ProtocolBuffers,
    /// A number the header does not name.
    Other(i32),
}

impl Encoding {
    pub fn code(self) -> i32 {
        match self {
            Encoding::Binary => 0,
            Encoding::BinaryVariableLength => 1,
            Encoding::Json => 2,
            Encoding::JsonCompact => 3,
            Encoding::ProtocolBuffers => 4,
            Encoding::Other(code) => code,
        }
    }

    pub fn of(code: i32) -> Encoding {
        match code {
            0 => Encoding::Binary,
            1 => Encoding::BinaryVariableLength,
            2 => Encoding::Json,
            3 => Encoding::JsonCompact,
            4 => Encoding::ProtocolBuffers,
            other => Encoding::Other(other),
        }
    }
}

/// The size of `s_EncodingRequest` and `s_EncodingResponse`: a u16 size, a
/// u16 type, an i32 protocol version, an i32 encoding and `char[4]` "DTC".
pub const ENCODING_MESSAGE_SIZE: usize = 16;

/// The ENCODING_REQUEST, in binary as every DTC connection begins whatever
/// it asks to speak after: little-endian, laid out as `s_EncodingRequest`.
pub fn encoding_request(encoding: Encoding) -> [u8; ENCODING_MESSAGE_SIZE] {
    let mut bytes = [0u8; ENCODING_MESSAGE_SIZE];
    bytes[0..2].copy_from_slice(&(ENCODING_MESSAGE_SIZE as u16).to_le_bytes());
    bytes[2..4].copy_from_slice(&ENCODING_REQUEST.to_le_bytes());
    bytes[4..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    bytes[8..12].copy_from_slice(&encoding.code().to_le_bytes());
    bytes[12..15].copy_from_slice(b"DTC");
    bytes
}

/// What an ENCODING_RESPONSE said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingReply {
    pub protocol: i32,
    pub encoding: Encoding,
}

/// Read the binary ENCODING_RESPONSE that answers an encoding request, or
/// say what came instead. Anything that is not one, a stray HTTP banner or
/// a JSON object, is not a DTC server.
pub fn read_encoding_response(stream: &mut impl Read) -> Result<EncodingReply, String> {
    let mut head = [0u8; 4];
    stream.read_exact(&mut head).map_err(|error| format!("no encoding response ({error})"))?;
    let size = u16::from_le_bytes([head[0], head[1]]) as usize;
    let kind = u16::from_le_bytes([head[2], head[3]]);
    if kind != ENCODING_RESPONSE || !(12..=256).contains(&size) {
        return Err(format!("the first bytes were not an encoding response: {}", printable(&head)));
    }
    let mut rest = vec![0u8; size - 4];
    stream.read_exact(&mut rest).map_err(|error| format!("the encoding response was cut short ({error})"))?;
    let protocol = i32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]);
    let encoding = i32::from_le_bytes([rest[4], rest[5], rest[6], rest[7]]);
    Ok(EncodingReply { protocol, encoding: Encoding::of(encoding) })
}

/// The binary ENCODING_RESPONSE a server sends, for the fake.
pub fn encoding_response(encoding: Encoding) -> [u8; ENCODING_MESSAGE_SIZE] {
    let mut bytes = encoding_request(encoding);
    bytes[2..4].copy_from_slice(&ENCODING_RESPONSE.to_le_bytes());
    bytes
}

/// Bytes something sent, as text a person can read in an error.
fn printable(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| if byte.is_ascii_graphic() { char::from(byte).to_string() } else { format!("\\x{byte:02x}") }).collect()
}

/// One message, as the JSON object it travels as.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Message(pub Map<String, Value>);

/// Anything at or beyond this is DTC's `DBL_MAX`, which means unset.
const UNSET: f64 = 1.0e300;

impl Message {
    /// A message of `kind` with `fields`, which must be a JSON object.
    pub fn new(kind: u16, fields: Value) -> Message {
        let mut map = match fields {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        map.insert("Type".into(), Value::from(kind));
        Message(map)
    }

    /// Its `Type`, or 0 when it has none.
    pub fn kind(&self) -> u16 {
        self.int("Type").and_then(|kind| u16::try_from(kind).ok()).unwrap_or(0)
    }

    /// An integer field. A number written as a string, or as a float with no
    /// fraction, counts.
    pub fn int(&self, field: &str) -> Option<i64> {
        match self.0.get(field)? {
            Value::Number(number) => number.as_i64().or_else(|| number.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
            Value::String(text) => text.trim().parse().ok(),
            Value::Bool(flag) => Some(i64::from(*flag)),
            _ => None,
        }
    }

    /// An integer field, 0 when missing.
    pub fn int_or_zero(&self, field: &str) -> i64 {
        self.int(field).unwrap_or(0)
    }

    /// A flag: a `uint8_t` of 0 or 1.
    pub fn flag(&self, field: &str) -> bool {
        self.int(field).unwrap_or(0) != 0
    }

    /// A double that is set: not missing, not `null`, not `±DBL_MAX`.
    pub fn number(&self, field: &str) -> Option<f64> {
        let value = match self.0.get(field)? {
            Value::Number(number) => number.as_f64()?,
            Value::String(text) => text.trim().parse().ok()?,
            _ => return None,
        };
        (value.is_finite() && value.abs() < UNSET).then_some(value)
    }

    /// A double that is set and is not zero, which is how most of Sierra's
    /// optional numbers say they are not there.
    pub fn nonzero(&self, field: &str) -> Option<f64> {
        self.number(field).filter(|value| *value != 0.0)
    }

    /// A text field, trimmed, `None` when missing or empty.
    pub fn text(&self, field: &str) -> Option<String> {
        match self.0.get(field)? {
            Value::String(text) => Some(text.trim()).filter(|text| !text.is_empty()).map(str::to_string),
            _ => None,
        }
    }

    /// A text field, empty when missing.
    pub fn text_or_empty(&self, field: &str) -> String {
        self.text(field).unwrap_or_default()
    }

    /// The message as it travels: its JSON, `Type` first, and the null that
    /// ends it. First because Sierra's parser reads `Type` and then the
    /// fields after it: a field written before `Type` is silently dropped,
    /// and a request whose every field went that way is answered with
    /// nothing at all.
    pub fn frame(&self) -> Vec<u8> {
        let mut bytes = format!("{{\"Type\":{}", self.kind()).into_bytes();
        for (field, value) in self.0.iter().filter(|(field, _)| field.as_str() != "Type") {
            bytes.push(b',');
            bytes.extend(serde_json::to_vec(field).unwrap_or_default());
            bytes.push(b':');
            bytes.extend(serde_json::to_vec(value).unwrap_or_default());
        }
        bytes.extend(b"}\0");
        bytes
    }

    /// The last of a batch: `MessageNumber` has reached `TotalNumberMessages`
    /// (spelled `TotalNumMessages` in ORDER_UPDATE), or the batch says there
    /// was nothing (`NoOrders` and its siblings), or it has neither count.
    pub fn final_in_batch(&self) -> bool {
        for none in ["NoOrders", "NoPositions", "NoAccountBalances", "NoOrderFills"] {
            if self.flag(none) {
                return true;
            }
        }
        if self.0.contains_key("IsFinalMessage") {
            return self.flag("IsFinalMessage");
        }
        let total = self.int("TotalNumberMessages").or_else(|| self.int("TotalNumMessages"));
        match (total, self.int("MessageNumber")) {
            (Some(total), Some(number)) => number >= total,
            _ => true,
        }
    }
}

/// Splits the byte stream into messages at each null, however the reads
/// fall: a message across several reads, or several in one.
#[derive(Default)]
pub struct Framer {
    pending: Vec<u8>,
}

/// The longest a message may grow without its null before the stream is
/// taken to be something other than DTC JSON.
const LONGEST: usize = 1 << 20;

impl Framer {
    /// Take in what a read returned, and hand out every message it finished.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Message>, String> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(end) = self.pending.iter().position(|&byte| byte == 0) {
            let frame: Vec<u8> = self.pending.drain(..=end).collect();
            let body = &frame[..frame.len() - 1];
            if body.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Value>(body) {
                Ok(Value::Object(map)) => out.push(Message(map)),
                _ => return Err(format!("not a DTC JSON message: {}", printable(&body[..body.len().min(60)]))),
            }
        }
        if self.pending.len() > LONGEST {
            return Err("a message went on for a megabyte without ending".into());
        }
        Ok(out)
    }
}

/// Read until one message is whole, for the handshake's few messages before
/// a reader thread takes over. `framer` keeps whatever followed it.
pub fn read_message(stream: &mut impl Read, framer: &mut Framer, queue: &mut Vec<Message>) -> io::Result<Message> {
    loop {
        if !queue.is_empty() {
            return Ok(queue.remove(0));
        }
        let mut buffer = [0u8; 8192];
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the server closed the connection"));
        }
        let messages = framer.push(&buffer[..read]).map_err(|why| io::Error::new(io::ErrorKind::InvalidData, why))?;
        queue.extend(messages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_encoding_request_is_laid_out_as_the_header_lays_it_out() {
        assert_eq!(
            encoding_request(Encoding::Json),
            [16, 0, 6, 0, 8, 0, 0, 0, 2, 0, 0, 0, b'D', b'T', b'C', 0]
        );
    }

    #[test]
    fn the_encoding_response_sierra_sent_reads_as_json_on_version_eight() {
        // What Sierra's server answered on this machine.
        let bytes = [0x10, 0, 0x07, 0, 0x08, 0, 0, 0, 0x02, 0, 0, 0, 0x44, 0x54, 0x43, 0];
        let reply = read_encoding_response(&mut &bytes[..]).unwrap();
        assert_eq!(reply, EncodingReply { protocol: 8, encoding: Encoding::Json });
        assert_eq!(encoding_response(Encoding::Json), bytes);
    }

    #[test]
    fn something_that_is_not_an_encoding_response_is_said_to_be_so() {
        let error = read_encoding_response(&mut &b"HTTP/1.1 400 Bad Request\r\n"[..]).unwrap_err();
        assert!(error.contains("HTTP"), "{error}");
    }

    #[test]
    fn a_message_is_its_json_and_a_null() {
        let message = Message::new(HEARTBEAT, json!({}));
        assert_eq!(message.frame(), b"{\"Type\":3}\0");
    }

    #[test]
    fn type_is_written_before_every_other_field_because_sierra_drops_what_comes_before_it() {
        let message = Message::new(SYMBOLS_FOR_UNDERLYING_REQUEST, json!({ "Exchange": "", "RequestID": 1, "UnderlyingSymbol": "ES" }));
        let framed = String::from_utf8(message.frame()).unwrap();
        assert!(framed.starts_with("{\"Type\":504,"), "{framed}");
        assert!(framed.ends_with("}\0"));
        let back = Framer::default().push(framed.as_bytes()).unwrap();
        assert_eq!(back, [message]);
    }

    #[test]
    fn a_message_split_across_reads_comes_out_once_it_is_whole() {
        let mut framer = Framer::default();
        assert!(framer.push(b"{\"Type\":3,\"NumDr").unwrap().is_empty());
        let messages = framer.push(b"oppedMessages\":0}\0").unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].kind(), HEARTBEAT);
    }

    #[test]
    fn several_messages_in_one_read_come_out_in_order() {
        let mut framer = Framer::default();
        let messages = framer.push(b"{\"Type\":3}\0{\"Type\":401,\"TradeAccount\":\"Sim1\"}\0{\"Type\":").unwrap();
        assert_eq!(messages.iter().map(Message::kind).collect::<Vec<_>>(), [3, 401]);
        assert_eq!(framer.push(b"5}\0").unwrap()[0].kind(), LOGOFF);
    }

    #[test]
    fn garbage_is_not_taken_for_a_message() {
        assert!(Framer::default().push(b"hello\0").is_err());
    }

    #[test]
    fn an_unset_double_reads_as_none_however_it_is_written() {
        let message = Message(
            json!({"Price1": null, "Price2": -1.7976931348623157e308, "AverageFillPrice": 1.7976931348623157e308, "LastFillPrice": 12.5})
                .as_object()
                .unwrap()
                .clone(),
        );
        assert_eq!(message.number("Price1"), None);
        assert_eq!(message.number("Price2"), None);
        assert_eq!(message.number("AverageFillPrice"), None);
        assert_eq!(message.number("Missing"), None);
        assert_eq!(message.number("LastFillPrice"), Some(12.5));
    }

    #[test]
    fn a_batch_ends_at_its_last_message_or_when_it_says_it_is_empty() {
        let of = |value: Value| Message(value.as_object().unwrap().clone());
        assert!(!of(json!({"TotalNumberMessages": 3, "MessageNumber": 1})).final_in_batch());
        assert!(of(json!({"TotalNumberMessages": 3, "MessageNumber": 3})).final_in_batch());
        assert!(of(json!({"TotalNumMessages": 1, "MessageNumber": 1, "NoOrders": 1})).final_in_batch());
        assert!(!of(json!({"IsFinalMessage": 0})).final_in_batch());
        assert!(of(json!({"IsFinalMessage": 1})).final_in_batch());
    }
}
