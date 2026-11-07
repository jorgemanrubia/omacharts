//! A fake DTC server on 127.0.0.1, which speaks the binary encoding
//! handshake and then JSON as Sierra's does, and keeps a small scripted
//! world: accounts, balances, positions, working orders, fills, contract
//! definitions and bars. It answers what Sierra answers, the way this
//! machine's Sierra answered it — the empty snapshots with their `No…`
//! flags, nothing at all for a contract it does not know, a refusal for
//! fills asked of no account — and records everything it is sent, so a test
//! can say what reached the server.
//!
//! No test in this crate, nor in the app, places an order anywhere but
//! here: this is where order tests run.
//!
//! ```no_run
//! use dtc::fake::{Fake, Fills};
//! let fake = Fake::builder().account("Sim1").balance("Sim1", 50_000.0, "USD").fills(Fills::Immediately).start();
//! let endpoint = fake.endpoint();
//! ```

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::wire::{self, Encoding, Framer, Message};
use crate::{Endpoint, OrderKind, Side};

/// How orders placed with the fake fill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fills {
    /// They rest, working, until [`Fake::fill`].
    #[default]
    Never,
    /// Each fills at once, whole: a market order at the market price, any
    /// other at its own.
    Immediately,
    /// Each fills this many at once, then the rest.
    InParts(u32),
}

/// An order the fake holds.
#[derive(Clone, Debug, PartialEq)]
pub struct FakeOrder {
    pub server_id: String,
    pub client_id: String,
    pub account: String,
    pub symbol: String,
    pub side: Side,
    pub kind: OrderKind,
    pub qty: u32,
    pub filled: u32,
    pub price1: f64,
    pub price2: f64,
    /// `OrderStatusEnum`.
    pub status: i64,
    pub tag: String,
    pub avg_fill_price: Option<f64>,
    /// The other of its OCO pair, by server id: whichever ends first ends
    /// the other, and a part of one filled comes off the other.
    pub oco: Option<String>,
}

impl FakeOrder {
    fn working(&self) -> bool {
        matches!(self.status, wire::ORDER_STATUS_OPEN | wire::ORDER_STATUS_PARTIALLY_FILLED | wire::ORDER_STATUS_PENDING_OPEN)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct FakePosition {
    account: String,
    symbol: String,
    qty: i64,
    avg: f64,
    open_pnl: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
struct FakeBalance {
    cash: f64,
    currency: String,
    open_pnl: f64,
    day_pnl: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct FakeFill {
    account: String,
    symbol: String,
    side: Side,
    qty: u32,
    price: f64,
    at: i64,
    execution_id: String,
    server_id: String,
}

/// The fake's world, which the builder sets up and the connections change.
#[derive(Clone, Debug)]
struct World {
    accounts: Vec<String>,
    balances: HashMap<String, FakeBalance>,
    positions: Vec<FakePosition>,
    orders: Vec<FakeOrder>,
    fills: Vec<FakeFill>,
    definitions: Vec<Value>,
    /// Answer a definition for any symbol, with a tick of 0.25 and real
    /// prices, rather than nothing as Sierra does.
    define_anything: bool,
    refuse: Option<String>,
    trading: bool,
    /// Say OCO orders are supported, and take a pair.
    oco: bool,
    reject: Option<String>,
    filling: Fills,
    market_price: f64,
    silent: bool,
    flatten: bool,
    bars: HashMap<String, Vec<Value>>,
    history_reject: Option<String>,
    server_name: String,
    next_id: u64,
}

impl Default for World {
    fn default() -> World {
        World {
            accounts: Vec::new(),
            balances: HashMap::new(),
            positions: Vec::new(),
            orders: Vec::new(),
            fills: Vec::new(),
            definitions: Vec::new(),
            define_anything: true,
            refuse: None,
            trading: true,
            oco: true,
            reject: None,
            filling: Fills::Never,
            market_price: 100.0,
            silent: false,
            flatten: true,
            bars: HashMap::new(),
            history_reject: None,
            server_name: "Fake DTC Server".into(),
            next_id: 1000,
        }
    }
}

/// Sets up a [`Fake`]'s world, then [`Builder::start`]s it.
#[derive(Default)]
pub struct Builder {
    world: World,
}

impl Builder {
    /// A trade account, listed in the order added.
    pub fn account(mut self, id: &str) -> Builder {
        self.world.accounts.push(id.into());
        self
    }

    /// `account`'s cash; an account with none says it has no balance, as
    /// Sierra's simulated ones do.
    pub fn balance(mut self, account: &str, cash: f64, currency: &str) -> Builder {
        self.world.balances.insert(account.into(), FakeBalance { cash, currency: currency.into(), open_pnl: 0.0, day_pnl: 0.0 });
        self
    }

    /// `account`'s open and day P&L, beside its [`Builder::balance`].
    pub fn pnl(mut self, account: &str, open: f64, day: f64) -> Builder {
        if let Some(balance) = self.world.balances.get_mut(account) {
            balance.open_pnl = open;
            balance.day_pnl = day;
        }
        self
    }

    /// `qty` of `symbol` held on `account` (negative short), at `avg` as it
    /// travels.
    pub fn position(mut self, account: &str, symbol: &str, qty: i64, avg: f64) -> Builder {
        self.world.positions.push(FakePosition { account: account.into(), symbol: symbol.into(), qty, avg, open_pnl: None });
        self
    }

    /// The open P&L the last [`Builder::position`] reports.
    pub fn open_pnl(mut self, pnl: f64) -> Builder {
        if let Some(position) = self.world.positions.last_mut() {
            position.open_pnl = Some(pnl);
        }
        self
    }

    /// A working order, placed before anyone signed in. Its server id is
    /// "1000", "1001" and so on, in the order added.
    pub fn order(mut self, account: &str, symbol: &str, side: Side, qty: u32, kind: OrderKind, price: f64) -> Builder {
        let server_id = self.world.next_id.to_string();
        self.world.next_id += 1;
        self.world.orders.push(FakeOrder {
            server_id,
            client_id: String::new(),
            account: account.into(),
            symbol: symbol.into(),
            side,
            kind,
            qty,
            filled: 0,
            price1: price,
            price2: 0.0,
            status: wire::ORDER_STATUS_OPEN,
            tag: String::new(),
            avg_fill_price: None,
            oco: None,
        });
        self
    }

    /// A fill in the history, at unix second `at`.
    pub fn fill(mut self, account: &str, symbol: &str, side: Side, qty: u32, price: f64, at: i64) -> Builder {
        let n = self.world.fills.len();
        self.world.fills.push(FakeFill {
            account: account.into(),
            symbol: symbol.into(),
            side,
            qty,
            price,
            at,
            execution_id: format!("H{n}"),
            server_id: format!("H{n}"),
        });
        self
    }

    /// A contract: Sierra's name for it, its root, its tick, its tick's
    /// worth, its expiry and the multiplier its prices travel with.
    pub fn definition(mut self, symbol: &str, underlying: &str, tick: f64, worth: f64, expiration: i64, multiplier: f64) -> Builder {
        self.world.definitions.push(json!({
            "SecurityType": 1,
            "Symbol": symbol,
            "UnderlyingSymbol": underlying,
            "ExchangeSymbol": symbol.split('_').next().unwrap_or(symbol),
            "Description": format!("{underlying} future"),
            "MinPriceIncrement": tick,
            "CurrencyValuePerIncrement": worth,
            "PriceDisplayFormat": 2,
            "SecurityExpirationDate": expiration,
            "DisplayPriceMultiplier": multiplier,
            "Currency": "USD",
        }));
        self
    }

    /// Answer nothing for a contract with no [`Builder::definition`], as
    /// Sierra does, rather than a definition with real prices.
    pub fn unknown_contracts_unanswered(mut self) -> Builder {
        self.world.define_anything = false;
        self
    }

    /// Turn every logon down with `text`.
    pub fn refuse_logon(mut self, text: &str) -> Builder {
        self.world.refuse = Some(text.into());
        self
    }

    /// Say trading is not supported, as Sierra does with Allow Trading off.
    pub fn trading_unsupported(mut self) -> Builder {
        self.world.trading = false;
        self
    }

    /// Reject every order placed, with `text`.
    pub fn reject_orders(mut self, text: &str) -> Builder {
        self.world.reject = Some(text.into());
        self
    }

    /// Say OCO orders are not supported, and reject a pair submitted.
    pub fn oco_unsupported(mut self) -> Builder {
        self.world.oco = false;
        self
    }

    /// How orders placed fill.
    pub fn fills(mut self, fills: Fills) -> Builder {
        self.world.filling = fills;
        self
    }

    /// Where a market order fills, as it travels.
    pub fn market_price(mut self, price: f64) -> Builder {
        self.world.market_price = price;
        self
    }

    /// Bars for `symbol` from the historical data request: (start, open,
    /// high, low, close, volume).
    pub fn bars(mut self, symbol: &str, bars: &[(i64, f64, f64, f64, f64, f64)]) -> Builder {
        let records = bars
            .iter()
            .map(|&(start, open, high, low, close, volume)| {
                json!({ "StartDateTime": start * 1_000_000, "OpenPrice": open, "HighPrice": high, "LowPrice": low, "LastPrice": close, "Volume": volume, "NumTrades": 1 })
            })
            .collect();
        self.world.bars.insert(symbol.into(), records);
        self
    }

    /// Refuse every historical data request with `text`, as Sierra does CME.
    pub fn refuse_history(mut self, text: &str) -> Builder {
        self.world.history_reject = Some(text.into());
        self
    }

    /// Listen on a port of its own, and serve.
    pub fn start(self) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the fake binds a port");
        let port = listener.local_addr().map(|address| address.port()).unwrap_or(0);
        listener.set_nonblocking(true).expect("the fake listens without blocking");
        let shared = Arc::new(Shared {
            world: Mutex::new(self.world),
            received: Mutex::new(Vec::new()),
            writers: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            logons: AtomicUsize::new(0),
        });
        let accepting = shared.clone();
        let thread = std::thread::spawn(move || accept(&accepting, listener));
        Fake { shared, port, thread: Some(thread) }
    }
}

struct Shared {
    world: Mutex<World>,
    received: Mutex<Vec<Message>>,
    /// Every connection that has logged on, to push to.
    writers: Mutex<Vec<Writer>>,
    stop: AtomicBool,
    logons: AtomicUsize,
}

type Writer = Arc<Mutex<TcpStream>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A fake DTC server, serving until dropped.
pub struct Fake {
    shared: Arc<Shared>,
    port: u16,
    thread: Option<JoinHandle<()>>,
}

impl Fake {
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Where it listens, with no credentials.
    pub fn endpoint(&self) -> Endpoint {
        Endpoint { host: "127.0.0.1".into(), port: self.port, user: None, password: None }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Every message it was sent, in order, the encoding requests among them
    /// (as a `Type` 6 with their `Encoding`).
    pub fn received(&self) -> Vec<Message> {
        lock(&self.shared.received).clone()
    }

    /// Every message of `kind` it was sent.
    pub fn received_of(&self, kind: u16) -> Vec<Message> {
        self.received().into_iter().filter(|message| message.kind() == kind).collect()
    }

    /// How many logons it has answered, accepted or not.
    pub fn logons(&self) -> usize {
        self.shared.logons.load(Ordering::SeqCst)
    }

    /// Hang up on every connection.
    pub fn drop_connections(&self) {
        for writer in lock(&self.shared.writers).drain(..) {
            let _ = lock(&writer).shutdown(Shutdown::Both);
        }
    }

    /// Turn logons down with `text` from now on, or accept them again.
    pub fn set_refuse_logon(&self, text: Option<&str>) {
        lock(&self.shared.world).refuse = text.map(str::to_string);
    }

    /// Stop answering anything, heartbeats included, or start again.
    pub fn set_silent(&self, silent: bool) {
        lock(&self.shared.world).silent = silent;
    }

    /// Reject every order placed from now on with `text`, or none.
    pub fn set_reject_orders(&self, text: Option<&str>) {
        lock(&self.shared.world).reject = text.map(str::to_string);
    }

    /// How orders placed from now on fill.
    pub fn set_fills(&self, fills: Fills) {
        lock(&self.shared.world).filling = fills;
    }

    /// Hold `qty` of `symbol` on `account` at `avg` from now on, telling
    /// nobody: what a reconnect finds changed.
    pub fn set_position(&self, account: &str, symbol: &str, qty: i64, avg: f64) {
        let mut world = lock(&self.shared.world);
        world.positions.retain(|position| !(position.account == account && position.symbol == symbol));
        world.positions.push(FakePosition { account: account.into(), symbol: symbol.into(), qty, avg, open_pnl: None });
    }

    /// Forget order `id`, telling nobody: an order that finished while a
    /// session was away.
    pub fn forget_order(&self, id: &str) {
        lock(&self.shared.world).orders.retain(|order| order.server_id != id && order.client_id != id);
    }

    /// The order `id` names, by server or client id.
    pub fn order(&self, id: &str) -> Option<FakeOrder> {
        lock(&self.shared.world).orders.iter().find(|order| order.server_id == id || order.client_id == id).cloned()
    }

    /// Fill `qty` of order `id` at `price`, and tell every connection.
    pub fn fill(&self, id: &str, qty: u32, price: f64) {
        let pushes = {
            let mut world = lock(&self.shared.world);
            match world.orders.iter().position(|order| order.server_id == id || order.client_id == id) {
                Some(at) => fill(&mut world, at, qty, price),
                None => Vec::new(),
            }
        };
        self.shared.push(&pushes);
    }

    /// Change `account`'s cash, and tell every connection.
    pub fn push_balance(&self, account: &str, cash: f64) {
        let message = {
            let mut world = lock(&self.shared.world);
            let balance =
                world.balances.entry(account.into()).or_insert(FakeBalance { cash, currency: "USD".into(), open_pnl: 0.0, day_pnl: 0.0 });
            balance.cash = cash;
            balance_message(account, balance, 0, 1, 1, true)
        };
        self.shared.push(&[message]);
    }

    /// Stop serving and hang up.
    pub fn stop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.drop_connections();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    /// Send `messages` to every connection that has logged on.
    fn push(&self, messages: &[Message]) {
        if lock(&self.world).silent {
            return;
        }
        let writers = lock(&self.writers).clone();
        for writer in writers {
            for message in messages {
                let _ = lock(&writer).write_all(&message.frame());
            }
        }
    }
}

fn accept(shared: &Arc<Shared>, listener: TcpListener) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let serving = shared.clone();
                std::thread::spawn(move || {
                    let _ = serve(&serving, stream);
                });
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// One connection: the binary handshake, then JSON until either end hangs up.
fn serve(shared: &Arc<Shared>, mut stream: TcpStream) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_millis(20)))?;
    let mut request = [0u8; wire::ENCODING_MESSAGE_SIZE];
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut got = 0;
    while got < request.len() {
        match stream.read(&mut request[got..]) {
            Ok(0) => return Ok(()),
            Ok(read) => got += read,
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                if Instant::now() > deadline || shared.stop.load(Ordering::SeqCst) {
                    return Ok(());
                }
            }
            Err(error) => return Err(error),
        }
    }
    let asked = i32::from_le_bytes([request[8], request[9], request[10], request[11]]);
    lock(&shared.received).push(Message::new(wire::ENCODING_REQUEST, json!({ "Encoding": asked })));
    stream.write_all(&wire::encoding_response(Encoding::Json))?;

    let writer: Writer = Arc::new(Mutex::new(stream.try_clone()?));
    let mut framer = Framer::default();
    let mut buffer = [0u8; 16 * 1024];
    let mut heartbeat: Option<Duration> = None;
    let mut beat = Instant::now();
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some(interval) = heartbeat
            && beat.elapsed() >= interval
            && !lock(&shared.world).silent
        {
            beat = Instant::now();
            send(&writer, &[Message::new(wire::HEARTBEAT, json!({ "NumDroppedMessages": 0, "CurrentDateTime": crate::now() }))])?;
        }
        let read = match stream.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(read) => read,
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => continue,
            Err(error) => return Err(error),
        };
        let messages = match framer.push(&buffer[..read]) {
            Ok(messages) => messages,
            Err(_) => return Ok(()),
        };
        for message in messages {
            lock(&shared.received).push(message.clone());
            if lock(&shared.world).silent {
                continue;
            }
            match message.kind() {
                wire::LOGON_REQUEST => {
                    shared.logons.fetch_add(1, Ordering::SeqCst);
                    let (refuse, trading, oco, name) = {
                        let world = lock(&shared.world);
                        (world.refuse.clone(), world.trading, world.oco, world.server_name.clone())
                    };
                    if let Some(text) = refuse {
                        send(&writer, &[Message::new(wire::LOGON_RESPONSE, json!({ "ProtocolVersion": 8, "Result": 2, "ResultText": text }))])?;
                        let _ = stream.shutdown(Shutdown::Both);
                        return Ok(());
                    }
                    let seconds = message.int("HeartbeatIntervalInSeconds").unwrap_or(10).max(1) as u64;
                    heartbeat = Some(Duration::from_secs(seconds));
                    beat = Instant::now();
                    send(
                        &writer,
                        &[Message::new(
                            wire::LOGON_RESPONSE,
                            json!({
                                "ProtocolVersion": 8, "Result": 1, "ResultText": "Connected to the fake. Service=fake",
                                "ServerName": name, "TradingIsSupported": u8::from(trading), "OCOOrdersSupported": u8::from(oco),
                                "BracketOrdersSupported": 1, "OrderCancelReplaceSupported": 1, "SecurityDefinitionsSupported": 1,
                                "HistoricalPriceDataSupported": 1, "MarketDataSupported": 1,
                            }),
                        )],
                    )?;
                    lock(&shared.writers).push(writer.clone());
                }
                wire::LOGOFF => {
                    let reason = message.text_or_empty("Reason");
                    send(&writer, &[Message::new(wire::LOGOFF, json!({ "DoNotReconnect": 1, "Reason": format!("Received a logoff from client: {reason}") }))])?;
                    let _ = stream.shutdown(Shutdown::Both);
                    return Ok(());
                }
                wire::HEARTBEAT => {}
                wire::HISTORICAL_PRICE_DATA_REQUEST => {
                    let answers = history(&lock(&shared.world), &message);
                    send(&writer, &answers)?;
                    let _ = stream.shutdown(Shutdown::Both);
                    return Ok(());
                }
                _ => {
                    let (replies, pushes) = answer(&mut lock(&shared.world), &message);
                    send(&writer, &replies)?;
                    shared.push(&pushes);
                }
            }
        }
    }
}

fn send(writer: &Writer, messages: &[Message]) -> io::Result<()> {
    let mut stream = lock(writer);
    for message in messages {
        stream.write_all(&message.frame())?;
    }
    Ok(())
}

/// What the fake answers `message` with: replies to whoever asked, and
/// pushes to every connection.
fn answer(world: &mut World, message: &Message) -> (Vec<Message>, Vec<Message>) {
    let id = message.int_or_zero("RequestID");
    let account = message.text("TradeAccount");
    let mut replies = Vec::new();
    let mut pushes = Vec::new();
    match message.kind() {
        wire::TRADE_ACCOUNTS_REQUEST => {
            let total = world.accounts.len();
            if total == 0 {
                replies.push(Message::new(wire::TRADE_ACCOUNT_RESPONSE, json!({ "RequestID": id, "TotalNumberMessages": 0, "MessageNumber": 0, "TradeAccount": "" })));
            }
            for (n, account) in world.accounts.iter().enumerate() {
                replies.push(Message::new(
                    wire::TRADE_ACCOUNT_RESPONSE,
                    json!({ "RequestID": id, "TotalNumberMessages": total, "MessageNumber": n + 1, "TradeAccount": account, "TradingIsDisabled": 0 }),
                ));
            }
        }
        wire::ACCOUNT_BALANCE_REQUEST => {
            let mut held: Vec<(&String, &FakeBalance)> =
                world.balances.iter().filter(|(name, _)| account.as_ref().is_none_or(|account| account == *name)).collect();
            held.sort_by_key(|(name, _)| name.to_string());
            if held.is_empty() {
                replies.push(Message::new(
                    wire::ACCOUNT_BALANCE_UPDATE,
                    json!({ "RequestID": id, "TotalNumberMessages": 1, "MessageNumber": 1, "NoAccountBalances": 1, "TradeAccount": "" }),
                ));
            }
            let total = held.len();
            for (n, (name, balance)) in held.into_iter().enumerate() {
                replies.push(balance_message(name, balance, id, total, n + 1, false));
            }
        }
        wire::CURRENT_POSITIONS_REQUEST => {
            let held: Vec<&FakePosition> = world
                .positions
                .iter()
                .filter(|position| position.qty != 0 && account.as_ref().is_none_or(|account| *account == position.account))
                .collect();
            if held.is_empty() {
                replies.push(Message::new(
                    wire::POSITION_UPDATE,
                    json!({ "RequestID": id, "TotalNumberMessages": 1, "MessageNumber": 1, "NoPositions": 1, "Symbol": "", "TradeAccount": account.clone().unwrap_or_default() }),
                ));
            }
            let total = held.len();
            for (n, position) in held.into_iter().enumerate() {
                replies.push(position_message(position, id, total, n + 1));
            }
        }
        wire::OPEN_ORDERS_REQUEST => {
            let working: Vec<&FakeOrder> =
                world.orders.iter().filter(|order| order.working() && account.as_ref().is_none_or(|account| *account == order.account)).collect();
            if working.is_empty() {
                replies.push(Message::new(
                    wire::ORDER_UPDATE,
                    json!({ "RequestID": id, "TotalNumMessages": 1, "MessageNumber": 1, "OrderUpdateReason": 1, "NoOrders": 1, "Price1": null }),
                ));
            }
            let total = working.len();
            for (n, order) in working.into_iter().enumerate() {
                let mut update = order_message(order, 1, "");
                update.0.insert("RequestID".into(), json!(id));
                update.0.insert("TotalNumMessages".into(), json!(total));
                update.0.insert("MessageNumber".into(), json!(n + 1));
                replies.push(update);
            }
        }
        wire::HISTORICAL_ORDER_FILLS_REQUEST => {
            let Some(account) = account else {
                replies.push(Message::new(wire::HISTORICAL_ORDER_FILLS_REJECT, json!({ "RequestID": id, "RejectText": "TradeAccount is not set" })));
                return (replies, pushes);
            };
            let since = message.int_or_zero("StartDateTime");
            let fills: Vec<&FakeFill> = world.fills.iter().filter(|fill| fill.account == account && fill.at >= since).collect();
            if fills.is_empty() {
                replies.push(Message::new(
                    wire::HISTORICAL_ORDER_FILL_RESPONSE,
                    json!({ "RequestID": id, "TotalNumberMessages": 1, "MessageNumber": 1, "NoOrderFills": 1, "TradeAccount": account, "InfoText": "[final]" }),
                ));
            }
            let total = fills.len();
            for (n, fill) in fills.into_iter().enumerate() {
                replies.push(Message::new(
                    wire::HISTORICAL_ORDER_FILL_RESPONSE,
                    json!({
                        "RequestID": id, "TotalNumberMessages": total, "MessageNumber": n + 1, "Symbol": fill.symbol,
                        "Exchange": "", "ServerOrderID": fill.server_id, "BuySell": side_code(fill.side), "Price": fill.price,
                        "DateTime": fill.at, "Quantity": fill.qty, "UniqueExecutionID": fill.execution_id, "TradeAccount": fill.account,
                        "NoOrderFills": 0,
                    }),
                ));
            }
        }
        wire::SECURITY_DEFINITION_FOR_SYMBOL_REQUEST => {
            let symbol = message.text_or_empty("Symbol");
            let known = world.definitions.iter().find(|definition| definition["Symbol"] == symbol.as_str()).cloned();
            let definition = known.or_else(|| {
                world.define_anything.then(|| {
                    json!({ "SecurityType": 1, "Symbol": symbol, "MinPriceIncrement": 0.25, "CurrencyValuePerIncrement": 12.5, "DisplayPriceMultiplier": 1 })
                })
            });
            // Sierra answers nothing for a symbol it does not know.
            if let Some(mut definition) = definition {
                definition["RequestID"] = json!(id);
                definition["IsFinalMessage"] = json!(1);
                replies.push(Message::new(wire::SECURITY_DEFINITION_RESPONSE, definition));
            }
        }
        wire::SYMBOLS_FOR_UNDERLYING_REQUEST | wire::SYMBOLS_FOR_EXCHANGE_REQUEST => {
            // As Sierra: no security type is refused, an unset one ignored.
            match message.int("SecurityType") {
                None => {
                    replies.push(Message::new(wire::SECURITY_DEFINITION_REJECT, json!({ "RequestID": id, "RejectText": "Security type not set" })));
                    return (replies, pushes);
                }
                Some(0) => return (replies, pushes),
                Some(_) => {}
            }
            let underlying = message.text("UnderlyingSymbol");
            let listed: Vec<Value> = world
                .definitions
                .iter()
                .filter(|definition| underlying.as_ref().is_none_or(|root| definition["UnderlyingSymbol"] == root.as_str()))
                .cloned()
                .collect();
            if listed.is_empty() {
                replies.push(Message::new(wire::SECURITY_DEFINITION_REJECT, json!({ "RequestID": id, "RejectText": "No symbols found" })));
            }
            let total = listed.len();
            for (n, mut definition) in listed.into_iter().enumerate() {
                // A list leaves out the expiry, as Sierra's does.
                if let Some(fields) = definition.as_object_mut() {
                    fields.remove("SecurityExpirationDate");
                }
                definition["RequestID"] = json!(id);
                definition["IsFinalMessage"] = json!(u8::from(n + 1 == total));
                replies.push(Message::new(wire::SECURITY_DEFINITION_RESPONSE, definition));
            }
        }
        wire::MARKET_DATA_REQUEST => {
            if message.int("RequestAction") == Some(wire::SUBSCRIBE) {
                replies.push(Message::new(
                    wire::MARKET_DATA_REJECT,
                    json!({ "SymbolID": message.int_or_zero("SymbolID"), "RejectText": "Market data request not allowed" }),
                ));
            }
        }
        wire::SUBMIT_NEW_SINGLE_ORDER => pushes = place(world, message),
        wire::SUBMIT_NEW_OCO_ORDER => pushes = place_pair(world, message),
        wire::CANCEL_ORDER => pushes = cancel(world, message),
        wire::CANCEL_REPLACE_ORDER => pushes = replace(world, message),
        wire::SUBMIT_FLATTEN_POSITION_ORDER => pushes = flatten(world, message),
        _ => {}
    }
    (replies, pushes)
}

fn side_code(side: Side) -> i64 {
    match side {
        Side::Buy => wire::BUY,
        Side::Sell => wire::SELL,
    }
}

fn kind_code(kind: OrderKind) -> i64 {
    match kind {
        OrderKind::Market => wire::ORDER_TYPE_MARKET,
        OrderKind::Limit => wire::ORDER_TYPE_LIMIT,
        OrderKind::Stop => wire::ORDER_TYPE_STOP,
        OrderKind::StopLimit => wire::ORDER_TYPE_STOP_LIMIT,
    }
}

fn balance_message(account: &str, balance: &FakeBalance, id: i64, total: usize, number: usize, unsolicited: bool) -> Message {
    Message::new(
        wire::ACCOUNT_BALANCE_UPDATE,
        json!({
            "RequestID": id, "TotalNumberMessages": total, "MessageNumber": number, "TradeAccount": account,
            "CashBalance": balance.cash, "BalanceAvailableForNewPositions": balance.cash, "AccountCurrency": balance.currency,
            "OpenPositionsProfitLoss": balance.open_pnl, "DailyProfitLoss": balance.day_pnl, "NoAccountBalances": 0,
            "Unsolicited": u8::from(unsolicited), "DailyNetLossLimit": -1500,
        }),
    )
}

fn position_message(position: &FakePosition, id: i64, total: usize, number: usize) -> Message {
    Message::new(
        wire::POSITION_UPDATE,
        json!({
            "RequestID": id, "TotalNumberMessages": total, "MessageNumber": number, "Symbol": position.symbol, "Exchange": "",
            "Quantity": position.qty, "AveragePrice": position.avg, "TradeAccount": position.account, "NoPositions": 0,
            "Unsolicited": u8::from(id == 0), "OpenProfitLoss": position.open_pnl.unwrap_or(0.0),
        }),
    )
}

fn order_message(order: &FakeOrder, reason: i64, said: &str) -> Message {
    Message::new(
        wire::ORDER_UPDATE,
        json!({
            "RequestID": 0, "TotalNumMessages": 1, "MessageNumber": 1, "Symbol": order.symbol, "Exchange": "",
            "ServerOrderID": order.server_id, "ClientOrderID": order.client_id, "OrderStatus": order.status,
            "OrderUpdateReason": reason, "OrderType": kind_code(order.kind), "BuySell": side_code(order.side),
            "Price1": if order.kind == OrderKind::Market { Value::Null } else { json!(order.price1) },
            "Price2": if order.kind == OrderKind::StopLimit { json!(order.price2) } else { Value::Null },
            "TimeInForce": 1, "OrderQuantity": order.qty, "FilledQuantity": order.filled,
            "RemainingQuantity": order.qty - order.filled.min(order.qty), "AverageFillPrice": order.avg_fill_price,
            "TradeAccount": order.account, "InfoText": said, "FreeFormText": order.tag, "NoOrders": 0,
            "OCOLinkedOrderServerOrderID": order.oco.clone().unwrap_or_default(),
        }),
    )
}

/// The order `message` submits, its fields named with `leg` after them:
/// "" for a single order, "_1" and "_2" for the two of an OCO pair.
fn submitted(world: &mut World, message: &Message, leg: &str) -> FakeOrder {
    let server_id = world.next_id.to_string();
    world.next_id += 1;
    let field = |name: &str| format!("{name}{leg}");
    FakeOrder {
        server_id,
        client_id: message.text_or_empty(&field("ClientOrderID")),
        account: message.text_or_empty("TradeAccount"),
        symbol: message.text_or_empty("Symbol"),
        side: if message.int(&field("BuySell")) == Some(wire::SELL) { Side::Sell } else { Side::Buy },
        kind: match message.int_or_zero(&field("OrderType")) {
            wire::ORDER_TYPE_LIMIT => OrderKind::Limit,
            wire::ORDER_TYPE_STOP => OrderKind::Stop,
            wire::ORDER_TYPE_STOP_LIMIT => OrderKind::StopLimit,
            _ => OrderKind::Market,
        },
        qty: message.number(&field("Quantity")).unwrap_or(0.0) as u32,
        filled: 0,
        price1: message.number(&field("Price1")).unwrap_or(0.0),
        price2: message.number(&field("Price2")).unwrap_or(0.0),
        status: wire::ORDER_STATUS_PENDING_OPEN,
        tag: message.text_or_empty("FreeFormText"),
        avg_fill_price: None,
        oco: None,
    }
}

/// An OCO pair, SUBMIT_NEW_OCO_ORDER: both taken, each knowing the other,
/// or both rejected, each with its own update, as DTC asks. They rest
/// until [`Fake::fill`], however orders are set to fill.
fn place_pair(world: &mut World, message: &Message) -> Vec<Message> {
    let mut pair = [submitted(world, message, "_1"), submitted(world, message, "_2")];
    let mut pushes: Vec<Message> = pair.iter().map(|order| order_message(order, 3, "")).collect();
    let refused = match (&world.reject, world.oco) {
        (Some(text), _) => Some(text.clone()),
        (None, false) => Some("OCO orders are not supported".to_string()),
        (None, true) if !world.accounts.contains(&pair[0].account) => Some("Trade account not found".to_string()),
        (None, true) => None,
    };
    let (status, reason, said) = match &refused {
        Some(text) => (wire::ORDER_STATUS_REJECTED, wire::NEW_ORDER_REJECTED, text.as_str()),
        None => (wire::ORDER_STATUS_OPEN, wire::NEW_ORDER_ACCEPTED, ""),
    };
    let ids = [pair[0].server_id.clone(), pair[1].server_id.clone()];
    for (at, order) in pair.iter_mut().enumerate() {
        order.status = status;
        if refused.is_none() {
            order.oco = Some(ids[1 - at].clone());
        }
        pushes.push(order_message(order, reason, said));
    }
    world.orders.extend(pair);
    pushes
}

fn place(world: &mut World, message: &Message) -> Vec<Message> {
    let mut order = submitted(world, message, "");
    let mut pushes = vec![order_message(&order, 3, "")];
    if let Some(text) = world.reject.clone() {
        order.status = wire::ORDER_STATUS_REJECTED;
        pushes.push(order_message(&order, wire::NEW_ORDER_REJECTED, &text));
        world.orders.push(order);
        return pushes;
    }
    if !world.accounts.contains(&order.account) {
        order.status = wire::ORDER_STATUS_REJECTED;
        pushes.push(order_message(&order, wire::NEW_ORDER_REJECTED, "Trade account not found"));
        world.orders.push(order);
        return pushes;
    }
    order.status = wire::ORDER_STATUS_OPEN;
    pushes.push(order_message(&order, wire::NEW_ORDER_ACCEPTED, ""));
    let price = if order.kind == OrderKind::Market { world.market_price } else { order.price1 };
    let qty = order.qty;
    world.orders.push(order);
    let at = world.orders.len() - 1;
    match world.filling {
        Fills::Never => {}
        Fills::Immediately => pushes.extend(fill(world, at, qty, price)),
        Fills::InParts(part) => {
            let part = part.min(qty);
            pushes.extend(fill(world, at, part, price));
            if qty > part {
                pushes.extend(fill(world, at, qty - part, price));
            }
        }
    }
    pushes
}

/// Fill `qty` of the order at `at`, moving the position: its order update
/// and its position update.
fn fill(world: &mut World, at: usize, qty: u32, price: f64) -> Vec<Message> {
    let order = &mut world.orders[at];
    let qty = qty.min(order.qty - order.filled);
    if qty == 0 {
        return Vec::new();
    }
    let before = f64::from(order.filled) * order.avg_fill_price.unwrap_or(0.0);
    order.filled += qty;
    order.avg_fill_price = Some((before + f64::from(qty) * price) / f64::from(order.filled));
    let whole = order.filled == order.qty;
    order.status = if whole { wire::ORDER_STATUS_FILLED } else { wire::ORDER_STATUS_PARTIALLY_FILLED };
    let execution = format!("X{}-{}", order.server_id, order.filled);
    let mut update = order_message(order, if whole { wire::ORDER_FILLED } else { wire::ORDER_FILLED_PARTIALLY }, "");
    update.0.insert("LastFillPrice".into(), json!(price));
    update.0.insert("LastFillQuantity".into(), json!(qty));
    update.0.insert("LastFillExecutionID".into(), json!(execution));
    update.0.insert("LastFillDateTime".into(), json!(crate::now() * 1000));
    let (account, symbol, side, server_id) = (order.account.clone(), order.symbol.clone(), order.side, order.server_id.clone());
    let sibling = order.oco.clone();
    world.fills.push(FakeFill { account: account.clone(), symbol: symbol.clone(), side, qty, price, at: crate::now(), execution_id: execution, server_id });

    let signed = match side {
        Side::Buy => i64::from(qty),
        Side::Sell => -i64::from(qty),
    };
    let at = match world.positions.iter().position(|position| position.account == account && position.symbol == symbol) {
        Some(at) => at,
        None => {
            world.positions.push(FakePosition { account, symbol, qty: 0, avg: 0.0, open_pnl: None });
            world.positions.len() - 1
        }
    };
    let position = &mut world.positions[at];
    let held = position.qty + signed;
    position.avg = if position.qty == 0 || position.qty.signum() != held.signum() {
        price
    } else if position.qty.signum() == signed.signum() {
        (position.avg * position.qty.abs() as f64 + price * signed.abs() as f64) / held.abs() as f64
    } else {
        position.avg
    };
    position.qty = held;
    if held == 0 {
        position.avg = 0.0;
    }
    let mut pushes = vec![update, position_message(position, 0, 1, 1)];
    // The other of its pair: cancelled once this one is whole, and as much
    // taken off it as this one filled until then.
    if let Some(other) = sibling.and_then(|id| world.orders.iter_mut().find(|order| order.server_id == id && order.working())) {
        match whole || other.qty.saturating_sub(qty) <= other.filled {
            true => {
                other.status = wire::ORDER_STATUS_CANCELED;
                pushes.push(order_message(other, wire::ORDER_CANCELED, "OCO"));
            }
            false => {
                other.qty -= qty;
                pushes.push(order_message(other, wire::ORDER_CANCEL_REPLACE_COMPLETE, "OCO quantity reduced"));
            }
        }
    }
    pushes
}

fn named<'a>(world: &'a mut World, message: &Message) -> Option<&'a mut FakeOrder> {
    let server = message.text("ServerOrderID");
    let client = message.text("ClientOrderID");
    world.orders.iter_mut().find(|order| {
        server.as_ref().is_some_and(|id| *id == order.server_id) || client.as_ref().is_some_and(|id| !id.is_empty() && *id == order.client_id)
    })
}

fn cancel(world: &mut World, message: &Message) -> Vec<Message> {
    match named(world, message) {
        Some(order) if order.working() => {
            order.status = wire::ORDER_STATUS_CANCELED;
            let mut pushes = vec![order_message(order, wire::ORDER_CANCELED, "")];
            // The other of a pair with no parent goes with it.
            if let Some(id) = order.oco.clone()
                && let Some(other) = world.orders.iter_mut().find(|order| order.server_id == id && order.working())
            {
                other.status = wire::ORDER_STATUS_CANCELED;
                pushes.push(order_message(other, wire::ORDER_CANCELED, "OCO"));
            }
            pushes
        }
        Some(order) => vec![order_message(order, wire::ORDER_CANCEL_REJECTED, "Order is not working")],
        None => vec![Message::new(
            wire::ORDER_UPDATE,
            json!({
                "ServerOrderID": message.text_or_empty("ServerOrderID"), "ClientOrderID": message.text_or_empty("ClientOrderID"),
                "OrderUpdateReason": wire::ORDER_CANCEL_REJECTED, "InfoText": "Order not found",
            }),
        )],
    }
}

fn replace(world: &mut World, message: &Message) -> Vec<Message> {
    match named(world, message) {
        Some(order) if order.working() => {
            if message.flag("Price1IsSet") {
                let moved = message.number("Price1").unwrap_or(order.price1) - order.price1;
                order.price1 += moved;
                // Sierra keeps a stop-limit's limit the same distance away.
                if order.kind == OrderKind::StopLimit {
                    order.price2 += moved;
                }
            }
            if let Some(qty) = message.number("Quantity").filter(|qty| *qty > 0.0) {
                order.qty = qty as u32;
            }
            vec![order_message(order, wire::ORDER_CANCEL_REPLACE_COMPLETE, "")]
        }
        Some(order) => vec![order_message(order, wire::ORDER_CANCEL_REPLACE_REJECTED, "Order is not working")],
        None => Vec::new(),
    }
}

fn flatten(world: &mut World, message: &Message) -> Vec<Message> {
    let account = message.text_or_empty("TradeAccount");
    let symbol = message.text_or_empty("Symbol");
    let held = world.positions.iter().find(|position| position.account == account && position.symbol == symbol).map(|position| position.qty).unwrap_or(0);
    let server_id = world.next_id.to_string();
    world.next_id += 1;
    let mut order = FakeOrder {
        server_id,
        client_id: message.text_or_empty("ClientOrderID"),
        account,
        symbol,
        side: if held > 0 { Side::Sell } else { Side::Buy },
        kind: OrderKind::Market,
        qty: held.unsigned_abs() as u32,
        filled: 0,
        price1: 0.0,
        price2: 0.0,
        status: wire::ORDER_STATUS_OPEN,
        tag: message.text_or_empty("FreeFormText"),
        avg_fill_price: None,
        oco: None,
    };
    if !world.flatten || held == 0 {
        order.status = wire::ORDER_STATUS_REJECTED;
        let pushes = vec![order_message(&order, wire::NEW_ORDER_REJECTED, "No position to flatten")];
        world.orders.push(order);
        return pushes;
    }
    let mut pushes = vec![order_message(&order, wire::NEW_ORDER_ACCEPTED, "")];
    let qty = order.qty;
    let price = world.market_price;
    world.orders.push(order);
    let at = world.orders.len() - 1;
    pushes.extend(fill(world, at, qty, price));
    pushes
}

/// The answer to a historical price data request: a header, the records, the
/// last marked final; or a reject.
fn history(world: &World, message: &Message) -> Vec<Message> {
    let id = message.int_or_zero("RequestID");
    if let Some(text) = &world.history_reject {
        return vec![Message::new(wire::HISTORICAL_PRICE_DATA_REJECT, json!({ "RequestID": id, "RejectText": text, "RejectReasonCode": 4 }))];
    }
    let symbol = message.text_or_empty("Symbol");
    let records = world.bars.get(&symbol).cloned().unwrap_or_default();
    let mut out = vec![Message::new(
        wire::HISTORICAL_PRICE_DATA_RESPONSE_HEADER,
        json!({ "RequestID": id, "RecordInterval": message.int_or_zero("RecordInterval"), "UseZLibCompression": 0, "NoRecordsToReturn": u8::from(records.is_empty()) }),
    )];
    let total = records.len();
    for (n, mut record) in records.into_iter().enumerate() {
        record["RequestID"] = json!(id);
        record["IsFinalRecord"] = json!(u8::from(n + 1 == total));
        out.push(Message::new(wire::HISTORICAL_PRICE_DATA_RECORD_RESPONSE, record));
    }
    out
}
