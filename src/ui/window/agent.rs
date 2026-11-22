//! What an agent trading from the command line asks of the window, behind
//! the `agent-trading` feature flag: a snapshot of a chart and its account
//! in a few lines, a wait for the next thing that matters, a stream of
//! every order, position and P&L change, and price alerts.
//!
//! Everything is answered from what the window already holds — the book
//! its sessions keep, the bars and indicators on screen — so a snapshot
//! costs no request to anybody, and an agent can ask for one every half
//! minute.
//!
//! Events are numbered as they happen and the last few hundred kept, so a
//! snapshot can hand out a token, `SEQ.BAR`, and the next one, given it,
//! says only what changed since: the bars that closed after it and the
//! events after it.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use serde_json::{Value, json};

use super::Window;
use crate::cli::trading::{Ask, Token};
use crate::cli::{Fault, Sink};
use crate::trading::{self, events, reading};
use crate::ui::pane::ChartPane;

/// How many events are kept for a token to be answered from.
const KEPT: usize = 512;

/// How many streams and waits may be open at once: a terminal closed
/// without a word is only noticed by writing to it, so the oldest goes.
const LISTENERS: usize = 16;

/// A fill comes as a burst — the order, then the position, then the cancel
/// of whatever was linked to it — and the trading service may tell each in
/// an update of its own. A wait woken by the first answers once the book
/// has said nothing new for this long…
const QUIET: Duration = Duration::from_millis(300);

/// …and the fill has reached the position and its bracket's other leg has
/// stopped working; or once this long has gone by regardless, saying so.
const SETTLE_AT_MOST: Duration = Duration::from_millis(1500);

/// What the window keeps for agents.
#[derive(Default)]
pub(super) struct Agent {
    /// The number of the last event.
    seq: Cell<u64>,
    log: RefCell<VecDeque<(u64, Value)>>,
    /// The book as events were last read off it; none until the first.
    seen: RefCell<Option<trading::Book>>,
    /// What was last said of each order and each position, by account and
    /// order or contract: a book that drops an order for a moment and has
    /// it again says nothing new.
    said: RefCell<std::collections::HashMap<String, Value>>,
    /// Each account's P&L as last said, and when.
    pnl_said: RefCell<Vec<(String, f64, f64)>>,
    pnl_at: Cell<Option<Instant>>,
    /// When the book last made an order or position event.
    book_moved_at: Cell<Option<Instant>>,
    listeners: RefCell<Vec<Listener>>,
    next_listener: Cell<u64>,
    alerts: RefCell<Vec<Alert>>,
    next_alert: Cell<u32>,
}

struct Listener {
    id: u64,
    sink: Rc<dyn Sink>,
    json: bool,
    kind: Kind,
}

enum Kind {
    /// `trading watch`: every event, P&L at most this often.
    Stream { pnl_every: Option<Duration> },
    /// `trading wait`: the first event that wakes it, or the time.
    Wait(Waiting),
}

struct Waiting {
    chart: u32,
    bars: usize,
    since: Option<Token>,
    /// The last event before it began to wait.
    from: u64,
    /// The order or position event that woke it, and when, while the rest
    /// of its burst comes in: see [`QUIET`].
    settling: Option<(Value, Instant)>,
    /// Each price it wakes at, with the side of it the price was last on.
    crosses: Vec<(f64, Option<bool>)>,
    /// Wake when a bar after this one begins.
    bar_close: Option<i64>,
}

/// A price alert on an instrument, said once when the price crosses it.
#[derive(Clone)]
struct Alert {
    id: u32,
    /// The instrument, by its display symbol.
    symbol: String,
    price: f64,
    places: usize,
    /// Whether the price was last above it; none until a price is heard.
    above: Option<bool>,
}

impl Window {
    /// Carry out what an agent asked.
    pub fn agent_command(self: &Rc<Self>, ask: Ask) -> Result<String, Fault> {
        match ask {
            Ask::Snapshot { chart, bars, since, json } => {
                let pane = self.agent_pane(chart)?;
                Ok(self.snapshot(&pane, bars, since, json))
            }
            Ask::Watch { pnl_every, json, sink } => {
                let sink = sink.ok_or_else(|| Fault::refused("nothing to stream to: run it from a terminal".into()))?;
                let pnl_every = (pnl_every > 0).then(|| Duration::from_secs(pnl_every));
                self.agent_seen();
                let watching = json!({"ev": "watching", "seq": self.agent.seq.get()});
                sink.say(&line(&watching, json));
                self.listen(sink, json, Kind::Stream { pnl_every });
                Ok(String::new())
            }
            Ask::Wait { chart, bars, since, timeout, crosses, bar_close, json, sink } => {
                let pane = self.agent_pane(chart)?;
                self.agent_seen();
                // Already happened since the snapshot it was given: at once.
                if let Some(token) = since {
                    let missed = self
                        .agent
                        .log
                        .borrow()
                        .iter()
                        .find(|(seq, event)| *seq > token.seq && events::wakes(event))
                        .map(|(_, event)| event.clone());
                    let closed = bar_close.then(|| self.closed_bar(&pane, token.bar)).flatten();
                    if let Some(event) = missed.or(closed) {
                        let settled = !from_book(&event) || (self.book_quiet() && self.fill_told(&event, token.seq));
                        let Some(sink) = sink.filter(|_| !settled) else {
                            return Ok(self.woken(&pane, &event, bars, since, json, true));
                        };
                        // The burst it began may still be coming in.
                        let waiting = Waiting {
                            chart,
                            bars,
                            since,
                            from: token.seq,
                            settling: Some((event, Instant::now())),
                            crosses: Vec::new(),
                            bar_close: None,
                        };
                        self.listen(sink, json, Kind::Wait(waiting));
                        self.settle_later(Duration::ZERO);
                        self.settle_later(SETTLE_AT_MOST);
                        return Ok(String::new());
                    }
                }
                let last = pane.view.last_price();
                let waiting = Waiting {
                    chart,
                    bars,
                    since,
                    from: self.agent.seq.get(),
                    settling: None,
                    crosses: crosses.into_iter().map(|price| (price, last.map(|last| last > price))).collect(),
                    bar_close: bar_close.then(|| pane.view.with_bars(|bars| bars.last().map(|bar| bar.ts))).flatten(),
                };
                let Some(sink) = sink else {
                    // Nothing to answer later on: say how it stands now.
                    return Ok(self.woken(&pane, &json!({"ev": "timeout"}), bars, since, json, true));
                };
                let id = self.listen(sink, json, Kind::Wait(waiting));
                let this = Rc::downgrade(self);
                glib::timeout_add_local_once(Duration::from_secs(timeout), move || {
                    if let Some(this) = this.upgrade() {
                        this.wake(id, &json!({"ev": "timeout"}));
                    }
                });
                Ok(String::new())
            }
            Ask::Alert { chart, price, remove, clear, json } => self.alert_command(chart, price, remove, clear, json),
        }
    }

    /// Chart `id` on screen.
    fn agent_pane(&self, id: u32) -> Result<Rc<ChartPane>, Fault> {
        self.panes.borrow().iter().find(|pane| pane.id == id).cloned().ok_or_else(|| Fault::not_found(format!("no chart {id} on screen")))
    }

    fn listen(&self, sink: Rc<dyn Sink>, json: bool, kind: Kind) -> u64 {
        let id = self.agent.next_listener.get() + 1;
        self.agent.next_listener.set(id);
        let mut listeners = self.agent.listeners.borrow_mut();
        if listeners.len() >= LISTENERS {
            let oldest = listeners.remove(0);
            oldest.sink.end(0);
        }
        listeners.push(Listener { id, sink, json, kind });
        id
    }

    /// Answer the wait `id` with `event` and the snapshot, and let it go.
    /// One already woken by an order or a position answers with that, once
    /// it settles, rather than with this.
    fn wake(self: &Rc<Self>, id: u64, event: &Value) {
        let settling = self
            .agent
            .listeners
            .borrow()
            .iter()
            .any(|listener| listener.id == id && matches!(listener.kind, Kind::Wait(Waiting { settling: Some(_), .. })));
        if !settling {
            self.answer(id, event, true);
        }
    }

    /// Answer the wait `id`, saying so when the book had not settled.
    fn answer(self: &Rc<Self>, id: u64, event: &Value, settled: bool) {
        let found = {
            let mut listeners = self.agent.listeners.borrow_mut();
            listeners.iter().position(|listener| listener.id == id).map(|at| listeners.remove(at))
        };
        let Some(listener) = found else { return };
        let Kind::Wait(waiting) = &listener.kind else {
            return;
        };
        let text = match self.agent_pane(waiting.chart) {
            Ok(pane) => self.woken(&pane, event, waiting.bars, waiting.since, listener.json, settled),
            Err(fault) => format!("{}\n{}\n", line(event, listener.json).trim_end(), fault.message),
        };
        listener.sink.say(&text);
        listener.sink.end(0);
    }

    /// What woke a wait, then the snapshot.
    fn woken(&self, pane: &ChartPane, event: &Value, bars: usize, since: Option<Token>, json: bool, settled: bool) -> String {
        match json {
            true => {
                let snapshot: Value = serde_json::from_str(&self.snapshot(pane, bars, since, true)).unwrap_or(Value::Null);
                let mut answer = json!({"woke": event, "snapshot": snapshot});
                if !settled {
                    answer["settled"] = json!(false);
                }
                format!("{answer}\n")
            }
            false => {
                let unsettled = match settled {
                    true => String::new(),
                    false => format!("unsettled: the book was still moving after {}s\n", SETTLE_AT_MOST.as_secs_f64()),
                };
                format!("woke {}\n{unsettled}{}", events::text(event), self.snapshot(pane, bars, since, false))
            }
        }
    }

    /// Look at the waits that are settling once `after` has gone by.
    fn settle_later(self: &Rc<Self>, after: Duration) {
        let this = Rc::downgrade(self);
        glib::timeout_add_local_once(after, move || {
            if let Some(this) = this.upgrade() {
                this.agent_settle();
            }
        });
    }

    /// Answer each wait woken by an order or a position whose burst is
    /// over: the book quiet for [`QUIET`] and showing what the fill did;
    /// or [`SETTLE_AT_MOST`] gone by, saying it had not settled.
    fn agent_settle(self: &Rc<Self>) {
        let quiet = self.book_quiet();
        let mut done = Vec::new();
        for listener in self.agent.listeners.borrow().iter() {
            let Kind::Wait(Waiting { settling: Some((event, began)), from, .. }) = &listener.kind else {
                continue;
            };
            if began.elapsed() >= SETTLE_AT_MOST {
                done.push((listener.id, event.clone(), false));
            } else if quiet && self.fill_told(event, *from) {
                done.push((listener.id, event.clone(), true));
            }
        }
        for (id, event, settled) in done {
            self.answer(id, &event, settled);
        }
    }

    /// Whether the book has said nothing new for [`QUIET`].
    fn book_quiet(&self) -> bool {
        self.agent.book_moved_at.get().is_none_or(|at| at.elapsed() >= QUIET)
    }

    /// Whether all a fill did has reached the book, as far as can be told:
    /// the position has moved since event `from`, and, the position flat,
    /// nothing placed along with the filled order still works — the other
    /// leg of its bracket. Anything but a fill has nothing to wait for.
    fn fill_told(&self, event: &Value, from: u64) -> bool {
        if !matches!(event["ev"].as_str(), Some("filled" | "partial")) {
            return true;
        }
        let (Some(account), Some(contract), Some(basket)) = (event["acct"].as_str(), event["sym"].as_str(), event["id"].as_str()) else {
            return true;
        };
        let moved = self
            .agent
            .log
            .borrow()
            .iter()
            .any(|(seq, told)| *seq > from && told["ev"] == "pos" && told["acct"] == account && told["sym"] == contract);
        if !moved {
            return false;
        }
        let view = self.trading.borrow();
        let on = |order: &trading::Order| events::number(&order.account) == account && order.contract == contract;
        let flat = !view
            .book
            .positions
            .iter()
            .any(|held| events::number(&held.account) == account && held.contract == contract && held.net != 0);
        let tag = view.book.orders.iter().find(|order| order.basket_id == basket && on(order)).and_then(|order| order.tag.as_deref());
        match (flat, tag) {
            (true, Some(tag)) => !view
                .book
                .orders
                .iter()
                .any(|order| order.basket_id != basket && on(order) && order.status.working() && order.tag.as_deref() == Some(tag)),
            _ => true,
        }
    }

    /// Number `event`, keep it, and tell whoever listens.
    fn agent_event(self: &Rc<Self>, mut event: Value) {
        let seq = self.agent.seq.get() + 1;
        self.agent.seq.set(seq);
        if let Some(fields) = event.as_object_mut() {
            fields.insert("seq".into(), json!(seq));
        }
        {
            let mut log = self.agent.log.borrow_mut();
            if log.len() >= KEPT {
                log.pop_front();
            }
            log.push_back((seq, event.clone()));
        }
        let pnl = event["ev"] == "pnl";
        let book = from_book(&event);
        if book {
            self.agent.book_moved_at.set(Some(Instant::now()));
        }
        let mut woken = Vec::new();
        let (mut settling, mut began) = (false, false);
        for listener in self.agent.listeners.borrow_mut().iter_mut() {
            match &mut listener.kind {
                Kind::Stream { pnl_every } => {
                    if !pnl || pnl_every.is_some() {
                        listener.sink.say(&line(&event, listener.json));
                    }
                }
                // Already woken, by what it will answer with.
                Kind::Wait(Waiting { settling: Some(_), .. }) => settling = true,
                // By an order or a position: answered once what that set
                // off has come in too, so the snapshot shows it done.
                Kind::Wait(waiting) if book && events::wakes(&event) => {
                    waiting.settling = Some((event.clone(), Instant::now()));
                    (settling, began) = (true, true);
                }
                Kind::Wait(_) if events::wakes(&event) => woken.push(listener.id),
                Kind::Wait(_) => {}
            }
        }
        for id in woken {
            self.wake(id, &event);
        }
        if settling && book {
            self.settle_later(QUIET + Duration::from_millis(10));
        }
        if began {
            self.settle_later(SETTLE_AT_MOST);
        }
    }

    /// Note the book as it is, so the first events are about what changes
    /// from now on rather than everything there is.
    fn agent_seen(&self) {
        if self.agent.seen.borrow().is_none() {
            *self.agent.seen.borrow_mut() = Some(self.trading.borrow().book.clone());
        }
    }

    /// The book moved: say what changed in it. Nothing is kept until
    /// somebody has asked, so with no agent it costs nothing.
    pub(super) fn agent_heard_book(self: &Rc<Self>) {
        if !crate::features::agent_trading() {
            return;
        }
        let now = self.trading.borrow().book.clone();
        let before = self.agent.seen.borrow_mut().replace(now.clone());
        let Some(before) = before else { return };
        for event in events::diff(&before, &now) {
            let (key, what) = match event["ev"].as_str() {
                Some("pos") => (format!("{}/{}", event["acct"], event["sym"]), event["net"].clone()),
                _ => (format!("{}/{}", event["acct"], event["id"]), json!([event["ev"], event["filled"], event["px"]])),
            };
            if self.agent.said.borrow().get(&key) == Some(&what) {
                continue;
            }
            self.agent.said.borrow_mut().insert(key, what);
            self.agent_event(event);
        }
        self.agent_heard_pnl();
    }

    /// The positions were marked to a new price: say each account's P&L
    /// as often as a stream asks.
    pub(super) fn agent_heard_pnl(self: &Rc<Self>) {
        if !crate::features::agent_trading() {
            return;
        }
        let every = self
            .agent
            .listeners
            .borrow()
            .iter()
            .filter_map(|listener| match listener.kind {
                Kind::Stream { pnl_every } => pnl_every,
                Kind::Wait(_) => None,
            })
            .min();
        let Some(every) = every else { return };
        if self.agent.pnl_at.get().is_some_and(|at| at.elapsed() < every) {
            return;
        }
        let moved = events::pnl(&self.trading.borrow().book, &self.agent.pnl_said.borrow());
        if moved.is_empty() {
            return;
        }
        self.agent.pnl_at.set(Some(Instant::now()));
        {
            let mut said = self.agent.pnl_said.borrow_mut();
            for (key, day, open) in &moved {
                said.retain(|(other, _, _)| other != key);
                said.push((key.clone(), *day, *open));
            }
        }
        for (key, day, open) in moved {
            self.agent_event(events::pnl_event(&key, day, open));
        }
    }

    /// Chart `pane` moved: alerts and waits on its price, and on its bars.
    pub(super) fn agent_heard_price(self: &Rc<Self>, pane: &ChartPane) {
        if !crate::features::agent_trading() {
            return;
        }
        if self.agent.alerts.borrow().is_empty()
            && self.agent.listeners.borrow().iter().all(|listener| matches!(listener.kind, Kind::Stream { .. }))
        {
            return;
        }
        let Some(last) = pane.view.last_price() else {
            return;
        };
        let symbol = pane.instrument.borrow().as_ref().map(|instrument| instrument.display_symbol()).unwrap_or_default();
        let mut crossed = Vec::new();
        self.agent.alerts.borrow_mut().retain_mut(|alert| {
            if alert.symbol != symbol {
                return true;
            }
            let above = last > alert.price;
            let hit = last == alert.price || alert.above.is_some_and(|was| was != above);
            alert.above = Some(above);
            if hit {
                crossed.push(json!({
                    "ev": "alert",
                    "id": alert.id,
                    "sym": alert.symbol,
                    "dir": if above { "up" } else { "down" },
                    "px": alert.price,
                    "last": reading::num(last, alert.places).parse::<f64>().unwrap_or(last),
                }));
            }
            !hit
        });
        for event in crossed {
            self.agent_event(event);
        }
        // The waits on this chart.
        let mut woken = Vec::new();
        let latest = pane.view.with_bars(|bars| bars.last().map(|bar| bar.ts));
        for listener in self.agent.listeners.borrow_mut().iter_mut() {
            let Kind::Wait(waiting) = &mut listener.kind else {
                continue;
            };
            if waiting.chart != pane.id {
                continue;
            }
            for (price, side) in waiting.crosses.iter_mut() {
                let above = last > *price;
                let hit = last == *price || side.is_some_and(|was| was != above);
                *side = Some(above);
                if hit {
                    woken.push((listener.id, json!({"ev": "cross", "dir": if above { "up" } else { "down" }, "px": *price, "last": last})));
                    break;
                }
            }
            if let (Some(started), Some(latest)) = (waiting.bar_close, latest)
                && latest > started
                && let Some(event) = self.closed_bar(pane, started)
            {
                woken.push((listener.id, event));
            }
        }
        for (id, event) in woken {
            self.wake(id, &event);
        }
    }

    /// The bar that began at or after `from` and has closed, as a `bar`
    /// event: none while the bar `from` is the newest.
    fn closed_bar(&self, pane: &ChartPane, from: i64) -> Option<Value> {
        let places = reading::places(pane.view.tick());
        pane.view.with_bars(|bars| {
            let newest = bars.last()?;
            if newest.ts <= from {
                return None;
            }
            let closed = bars.iter().rev().nth(1)?;
            let p = |price: f64| reading::num(price, places).parse::<f64>().unwrap_or(price);
            Some(json!({"ev": "bar", "t": closed.ts, "o": p(closed.open), "h": p(closed.high), "l": p(closed.low), "c": p(closed.close), "v": closed.volume.round()}))
        })
    }

    /// The window is closing: every stream and wait is told, and let go.
    pub(super) fn agent_closing(&self) {
        let listeners: Vec<Listener> = self.agent.listeners.borrow_mut().drain(..).collect();
        for listener in listeners {
            listener.sink.say(&line(&json!({"ev": "closed"}), listener.json));
            listener.sink.end(0);
        }
    }

    fn alert_command(
        self: &Rc<Self>,
        chart: Option<u32>,
        price: Option<f64>,
        remove: Option<u32>,
        clear: bool,
        as_json: bool,
    ) -> Result<String, Fault> {
        if clear {
            self.agent.alerts.borrow_mut().clear();
        }
        if let Some(id) = remove {
            let mut alerts = self.agent.alerts.borrow_mut();
            let before = alerts.len();
            alerts.retain(|alert| alert.id != id);
            if alerts.len() == before {
                return Err(Fault::not_found(format!("no alert {id}")));
            }
        }
        if let (Some(chart), Some(price)) = (chart, price) {
            let pane = self.agent_pane(chart)?;
            let symbol = pane
                .instrument
                .borrow()
                .as_ref()
                .map(|instrument| instrument.display_symbol())
                .ok_or_else(|| Fault::refused("that chart charts nothing".into()))?;
            let id = self.agent.next_alert.get() + 1;
            self.agent.next_alert.set(id);
            let above = pane.view.last_price().map(|last| last > price);
            let places = reading::places(pane.view.tick());
            self.agent.alerts.borrow_mut().push(Alert { id, symbol, price, places, above });
        }
        let alerts = self.agent.alerts.borrow();
        if as_json {
            let rows: Vec<Value> = alerts.iter().map(|alert| json!({"id": alert.id, "sym": alert.symbol, "px": alert.price})).collect();
            return Ok(format!("{}\n", json!({"alerts": rows})));
        }
        if alerts.is_empty() {
            return Ok("no alerts\n".into());
        }
        Ok(alerts.iter().map(|alert| format!("{} {} {}\n", alert.id, alert.symbol, reading::num(alert.price, alert.places))).collect())
    }

    /// Chart `chart` on screen in numbers.
    pub fn read_chart(&self, chart: u32, last: usize) -> Result<reading::ChartRead, Fault> {
        let pane = self.agent_pane(chart)?;
        Ok(self.chart_read(&pane, last))
    }

    /// Chart `chart`'s footprint in numbers: see [`omacharts_chart::ChartView::read_footprint`].
    pub fn read_footprint(
        &self,
        chart: u32,
        pick: omacharts_chart::BarPick,
        row_ticks: Option<u32>,
    ) -> Result<Option<omacharts_chart::FootprintRead>, Fault> {
        let pane = self.agent_pane(chart)?;
        Ok(pane.view.read_footprint(pick, row_ticks))
    }

    fn chart_read(&self, pane: &ChartPane, last: usize) -> reading::ChartRead {
        let instrument = pane.instrument.borrow().clone();
        let timeframe = pane.view.timeframe();
        let intraday = timeframe.seconds() < 86_400;
        let places = reading::places(pane.view.tick());
        let zone = instrument.as_ref().map_or(chrono_tz::UTC, |instrument| self.clock_for(instrument).zone);
        let bars = pane.view.with_bars(|bars| bars[bars.len().saturating_sub(last)..].to_vec());
        let (values, levels) = pane.view.with_indicators(|drawn| {
            let values = reading::values(drawn, places, |one| one.indicator.label_for(timeframe));
            let levels = pane.view.with_bars(|bars| reading::levels(bars, drawn, intraday));
            (values, levels)
        });
        reading::ChartRead {
            label: self.chart_named(pane),
            symbol: instrument.as_ref().map(|instrument| instrument.display_symbol()).unwrap_or_default(),
            timeframe: timeframe.label(),
            intraday,
            places,
            zone,
            bars,
            values,
            levels,
        }
    }

    /// The last price of a chart on screen of `instrument`.
    pub fn chart_price(&self, instrument: &omacharts_engine::Instrument) -> Option<(f64, i64, Option<f64>)> {
        let panes = self.panes.borrow().clone();
        panes.iter().find_map(|pane| {
            let same = pane
                .instrument
                .borrow()
                .as_ref()
                .is_some_and(|shown| shown.symbol.eq_ignore_ascii_case(&instrument.symbol) && shown.suffix == instrument.suffix);
            if !same {
                return None;
            }
            let (last, at) = pane.view.with_bars(|bars| bars.last().map(|bar| (bar.close, bar.ts)))?;
            Some((last, at, pane.view.tick()))
        })
    }

    /// Chart `pane` and its account, terse: see [`crate::cli::trading::Ask::Snapshot`].
    fn snapshot(&self, pane: &ChartPane, bars: usize, since: Option<Token>, as_json: bool) -> String {
        let read = self.chart_read(pane, bars.max(1));
        let places = read.places;
        let n = |price: f64| reading::num(price, places);
        let newest = read.bars.last().map_or(0, |bar| bar.ts);
        let token = Token { seq: self.agent.seq.get(), bar: newest };
        // Only what is newer than the token: the bar forming then has
        // closed since, so it is shown again.
        let shown: Vec<omacharts_engine::Bar> = match since {
            Some(since) => read.bars.iter().filter(|bar| bar.ts >= since.bar).copied().collect(),
            None => {
                if bars == 0 {
                    Vec::new()
                } else {
                    read.bars.clone()
                }
            }
        };
        let happened: Vec<Value> = match since {
            Some(since) => self.agent.log.borrow().iter().filter(|(seq, _)| *seq > since.seq).map(|(_, event)| event.clone()).collect(),
            None => Vec::new(),
        };
        let target = self.order_target(pane).ok();
        let view = self.trading.borrow();
        let key = target.as_ref().map(|target| target.account.key());
        let contract = target.as_ref().map(|target| target.contract.clone());
        let last = target.as_ref().and_then(|target| self.market_price(pane, target)).or_else(|| pane.view.last_price());
        let money = key.as_deref().and_then(|key| view.book.money(key)).cloned();
        let money = money.as_ref();
        let held = match (&key, &contract) {
            (Some(key), Some(contract)) => view.book.position(key, contract).cloned(),
            _ => None,
        };
        let working: Vec<trading::Order> = match (&key, &contract) {
            (Some(key), Some(contract)) => view.book.working(key, contract).cloned().collect(),
            _ => Vec::new(),
        };
        drop(view);
        let day = money.and_then(|money| money.day_pnl);
        if as_json {
            let p = |price: Option<f64>| price.map(|price| reading::num(price, places).parse::<f64>().unwrap_or(price));
            return format!(
                "{}\n",
                json!({
                    "tok": token.to_string(),
                    "sym": read.symbol,
                    "contract": contract,
                    "tf": read.timeframe,
                    "last": p(last),
                    "bid": null,
                    "ask": null,
                    "acct": key.as_ref().map(|key| json!({
                        "id": events::number(key),
                        "bal": money.and_then(|money| money.balance),
                        "day": day,
                        "open": money.and_then(|money| money.unrealized),
                    })),
                    "pos": held.as_ref().map(|held| json!({"net": held.net, "avg": p(held.avg), "open": held.open_pnl})),
                    "ord": working.iter().map(|order| json!([order.basket_id, side_letter(order.side), order.qty, kind_word(order.kind), p(order.price)])).collect::<Vec<_>>(),
                    "lv": read.levels.json(places),
                    "bars": shown.iter().map(|bar| reading::bar_json(bar, places)).collect::<Vec<_>>(),
                    "ev": happened,
                })
            );
        }
        let mut out = String::new();
        let price_or_dash = |price: Option<f64>| price.map_or("-".to_string(), n);
        out.push_str(&format!(
            "tok {token} {} {} last {} bid - ask -\n",
            contract.as_deref().unwrap_or(&read.symbol),
            read.timeframe,
            price_or_dash(last)
        ));
        match (&key, money) {
            (Some(key), money) => {
                let mut line = format!("acct {}", events::number(key));
                if let Some(balance) = money.and_then(|money| money.balance) {
                    line.push_str(&format!(" bal {}", reading::num(balance, 2)));
                }
                line.push_str(&format!(
                    " day {} open {}",
                    events::signed(day.unwrap_or(0.0)),
                    events::signed(money.and_then(|money| money.unrealized).unwrap_or(0.0))
                ));
                out.push_str(&line);
                out.push('\n');
            }
            (None, _) => out.push_str("acct - (the chart trades no account)\n"),
        }
        match &held {
            Some(held) => out.push_str(&format!(
                "pos {}{} @{} open {}\n",
                if held.net > 0 { "+" } else { "" },
                held.net,
                price_or_dash(held.avg),
                events::signed(held.open_pnl.unwrap_or(0.0))
            )),
            None => out.push_str("pos flat\n"),
        }
        match working.is_empty() {
            true => out.push_str("ord none\n"),
            false => {
                let orders: Vec<String> = working
                    .iter()
                    .map(|order| {
                        format!(
                            "{} {} {} {} {}",
                            order.basket_id,
                            side_letter(order.side),
                            order.qty,
                            kind_word(order.kind),
                            price_or_dash(order.price)
                        )
                    })
                    .collect();
                out.push_str(&format!("ord {}\n", orders.join(" | ")));
            }
        }
        let levels = read.levels.text(places);
        if !levels.is_empty() {
            out.push_str(&format!("lv {levels}\n"));
        }
        if !shown.is_empty() {
            out.push_str("bars t o h l c v\n");
            out.push_str(&reading::bars_text(&shown, places, read.intraday, read.zone));
        }
        if since.is_some() {
            match happened.is_empty() {
                true => out.push_str("ev none\n"),
                false => {
                    for event in &happened {
                        out.push_str(&format!("ev {}\n", events::text(event)));
                    }
                }
            }
        }
        out
    }
}

/// Whether `event` came from the book: an order's or a position's.
fn from_book(event: &Value) -> bool {
    event["ev"] == "pos" || (event.get("id").is_some() && event.get("acct").is_some())
}

/// An event or an answer, one line: JSON or text.
fn line(event: &Value, json: bool) -> String {
    match json {
        true => format!("{event}\n"),
        false => format!("{}\n", events::text(event)),
    }
}

fn side_letter(side: trading::Side) -> &'static str {
    match side {
        trading::Side::Buy => "B",
        trading::Side::Sell => "S",
    }
}

fn kind_word(kind: trading::OrderKind) -> &'static str {
    match kind {
        trading::OrderKind::Market => "mkt",
        trading::OrderKind::Limit => "lmt",
        trading::OrderKind::Stop => "stp",
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use serde_json::Value;

    use super::super::trade::tests::{long_es, settle};
    use super::*;
    use crate::trading::{OrderKind, Session as _};

    /// A terminal, kept: what it was told, and whether it was let go.
    #[derive(Default)]
    struct Terminal {
        said: RefCell<String>,
        ended: Cell<bool>,
    }

    impl Sink for Terminal {
        fn say(&self, text: &str) {
            self.said.borrow_mut().push_str(text);
        }
        fn end(&self, _code: u8) {
            self.ended.set(true);
        }
    }

    /// A caller whose terminal is [`Terminal`].
    struct Typed(Rc<Terminal>);

    impl crate::cli::Caller for Typed {
        fn read(&self, path: &str) -> Result<String, Fault> {
            crate::cli::Here.read(path)
        }
        fn sink(&self) -> Option<Rc<dyn Sink>> {
            Some(self.0.clone())
        }
    }

    fn command_from(window: &Rc<Window>, line: &str, caller: &dyn crate::cli::Caller) -> (u8, String) {
        let args: Vec<String> = std::iter::once("omacharts").chain(line.split_whitespace()).map(String::from).collect();
        let live: &dyn crate::cli::Live = window;
        let outcome = crate::cli::run(&args, &window.store, Some(live), caller);
        (outcome.code, if outcome.code == 0 { outcome.out } else { outcome.err })
    }

    fn command(window: &Rc<Window>, line: &str) -> (u8, String) {
        command_from(window, line, &crate::cli::Here)
    }

    fn working(session: &trading::fake::FakeSession) -> Vec<trading::Order> {
        session.book.lock().unwrap().orders.iter().filter(|order| order.status.working()).cloned().collect()
    }

    fn agents_on() {
        crate::features::force(crate::features::Feature::AgentTrading, true);
    }

    /// `--target` and `--stop-loss` send the entry and its exits as one
    /// bracket: the trading service puts the target and the stop in,
    /// linked, at their distance from the fill. Off, they are refused.
    #[test]
    fn an_entry_goes_with_its_exits_as_one_bracket() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            let (code, why) = command(&window, "trading sell --target -6 --stop-loss +4");
            assert_eq!(code, crate::cli::EXIT_USAGE, "off, a bracket is not an order: {why}");

            agents_on();
            let (code, why) = command(&window, "trading sell --target +6");
            assert_eq!(code, crate::cli::EXIT_REFUSED, "a short's target is below: {why}");
            assert!(why.contains("- points"), "{why}");

            let (code, out) = command(&window, "trading sell --quantity 1 --target -6 --stop-loss +4");
            assert_eq!(code, 0, "{out}");
            assert!(settle(&window, || working(&session).len() == 2 && !window.trade.placing.busy(&key)), "no exits");
            assert!(session.heard.lock().unwrap().contains(&"bracket"));
            let fill = session
                .book
                .lock()
                .unwrap()
                .orders
                .iter()
                .find(|order| order.kind == OrderKind::Market && order.side == trading::Side::Sell)
                .and_then(|order| order.fill_price)
                .expect("the entry filled");
            let exits = working(&session);
            let target = exits.iter().find(|order| order.kind == OrderKind::Limit).expect("a target");
            let stop = exits.iter().find(|order| order.kind == OrderKind::Stop).expect("a stop");
            assert_eq!((target.side, target.price), (trading::Side::Buy, Some(fill - 6.0)));
            assert_eq!((stop.side, stop.price), (trading::Side::Buy, Some(fill + 4.0)));
            assert!(
                session.links.lock().unwrap().iter().any(|group| group.contains(&target.basket_id) && group.contains(&stop.basket_id)),
                "not linked"
            );
        });
    }

    /// `trading watch` tells a terminal each order and position change as
    /// it happens, and lets it go when the window closes; `trading wait`
    /// answers with what woke it and the snapshot; a snapshot's token,
    /// given back, says only what came after it.
    #[test]
    fn a_watch_streams_a_wait_wakes_and_a_snapshot_says_what_changed() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            agents_on();
            window.agent_heard_book();

            let watching = Rc::new(Terminal::default());
            let (code, out) = command_from(&window, "trading watch --json", &Typed(watching.clone()));
            assert_eq!((code, out.as_str()), (0, ""));
            let waiting = Rc::new(Terminal::default());
            let (code, out) = command_from(&window, "trading wait --timeout 60", &Typed(waiting.clone()));
            assert_eq!((code, out.as_str()), (0, ""), "it answers later");

            let (code, out) = command(&window, "trading sell");
            assert_eq!(code, 0, "{out}");
            assert!(settle(&window, || waiting.ended.get() && !window.trade.placing.busy(&key)), "the wait never woke");
            assert!(settle(&window, || watching.said.borrow().contains(r#""ev":"pos""#)), "{}", watching.said.borrow());
            let lines: Vec<Value> = watching.said.borrow().lines().map(|line| serde_json::from_str(line).expect("JSON")).collect();
            assert_eq!(lines[0]["ev"], "watching");
            assert!(lines.iter().any(|line| line["ev"] == "filled" && line["side"] == "S"), "{lines:?}");
            assert!(lines.iter().any(|line| line["ev"] == "pos" && line["net"] == 0 && line["was"] == 1), "{lines:?}");
            let woke = waiting.said.borrow().clone();
            assert!(woke.starts_with("woke "), "{woke}");
            assert!(woke.contains("\ntok "), "{woke}");

            let (code, first) = command(&window, "trading snapshot");
            assert_eq!(code, 0, "{first}");
            assert!(first.starts_with("tok "), "{first}");
            assert!(first.contains("\npos flat\n"), "{first}");
            assert!(first.contains("\nbars "), "{first}");
            let token = first.split_whitespace().nth(1).expect("a token").to_string();
            let (_, again) = command(&window, &format!("trading snapshot --since {token}"));
            assert!(again.contains("ev none"), "{again}");
            let seq: u64 = token.split('.').next().unwrap().parse().unwrap();
            session
                .place(&trading::OrderRequest {
                    account: "951".into(),
                    contract: "ESZ6".into(),
                    exchange: "CME".into(),
                    side: trading::Side::Buy,
                    qty: 1,
                    kind: OrderKind::Limit,
                    price: Some(7000.0),
                    tag: trading::new_tag(),
                })
                .unwrap();
            assert!(settle(&window, || window.agent.seq.get() > seq), "no event");
            let (_, after) = command(&window, &format!("trading snapshot --since {token} --json"));
            let after: Value = serde_json::from_str(&after).expect("JSON");
            assert_eq!(after["ev"][0]["ev"], "placed", "{after}");
            assert!(after["ord"].to_string().contains("7000"), "{after}");
            // An order put in is no news to whoever put it in: a wait from
            // that token sleeps on, and wakes when it is cancelled.
            let later = Rc::new(Terminal::default());
            let (code, out) = command_from(&window, &format!("trading wait --since {token} --timeout 60"), &Typed(later.clone()));
            assert_eq!((code, out.as_str()), (0, ""), "woken by its own order");
            let basket = working(&session).iter().find(|order| order.price == Some(7000.0)).expect("the order").basket_id.clone();
            session.cancel("951", &basket).unwrap();
            assert!(settle(&window, || later.ended.get()), "the wait never woke");
            assert!(later.said.borrow().starts_with("woke cancelled"), "{}", later.said.borrow());
            // One given a token from before the cancel answers at once.
            let (code, out) =
                command_from(&window, &format!("trading wait --since {token} --timeout 60"), &Typed(Rc::new(Terminal::default())));
            assert_eq!(code, 0, "{out}");
            assert!(out.starts_with("woke cancelled"), "{out}");

            window.agent_closing();
            assert!(watching.ended.get(), "still held after the window went");
            assert!(watching.said.borrow().trim_end().ends_with(r#"{"ev":"closed"}"#), "{}", watching.said.borrow());
        });
    }

    /// A bracket's stop fills and the trading service tells it in three
    /// updates: the fill, then the position, then the target cancelled.
    /// The wait woken by the fill answers with the book as it ends up —
    /// flat, and nothing working — not as it stood at the first of them.
    #[test]
    fn a_wait_woken_by_a_fill_answers_once_the_bracket_has_settled() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            agents_on();
            let (code, out) = command(&window, "trading sell");
            assert_eq!(code, 0, "{out}");
            assert!(settle(&window, || window.trading.borrow().book.positions.is_empty() && !window.trade.placing.busy(&key)));
            let (code, out) = command(&window, "trading sell --quantity 1 --target -6 --stop-loss +4");
            assert_eq!(code, 0, "{out}");
            assert!(settle(&window, || window.trading.borrow().book.orders.iter().filter(|order| order.status.working()).count() == 2 && !window.trade.placing.busy(&key)), "no exits");
            let exits = working(&session);
            let stop = exits.iter().find(|order| order.kind == OrderKind::Stop).expect("a stop").basket_id.clone();
            let target = exits.iter().find(|order| order.kind == OrderKind::Limit).expect("a target").basket_id.clone();
            pump(&window, Duration::from_millis(400));

            let waiting = Rc::new(Terminal::default());
            let (code, out) = command_from(&window, "trading wait --timeout 60", &Typed(waiting.clone()));
            assert_eq!((code, out.as_str()), (0, ""));

            // The stop fills: the order first, on its own.
            {
                let mut book = session.book.lock().unwrap();
                let order = book.orders.iter_mut().find(|order| order.basket_id == stop).expect("the stop");
                order.status = trading::Stage::Filled.into();
                order.filled = 1;
                order.fill_price = order.price;
            }
            session.moved();
            pump(&window, Duration::from_millis(100));
            assert!(!waiting.ended.get(), "answered before the position: {}", waiting.said.borrow());
            // Then the position, flat.
            session.book.lock().unwrap().positions.clear();
            session.moved();
            // And, longer after than the book stays quiet, the target cancelled.
            pump(&window, Duration::from_millis(500));
            assert!(!waiting.ended.get(), "answered with the target working: {}", waiting.said.borrow());
            {
                let mut book = session.book.lock().unwrap();
                let order = book.orders.iter_mut().find(|order| order.basket_id == target).expect("the target");
                order.status = trading::Stage::Cancelled.into();
            }
            session.moved();
            let until = Instant::now() + Duration::from_secs(3);
            while !waiting.ended.get() && Instant::now() < until {
                pump(&window, Duration::from_millis(20));
            }
            let woke = waiting.said.borrow().clone();
            assert!(woke.starts_with(&format!("woke filled {stop} B 1 ESZ6 stp")), "{woke}");
            assert!(woke.contains("\npos flat\n"), "{woke}");
            assert!(woke.contains("\nord none\n"), "{woke}");
            assert!(!woke.contains("unsettled"), "{woke}");
        });
    }

    /// Run the window for `long`, hearing the trading service throughout.
    fn pump(window: &Rc<Window>, long: Duration) {
        let context = glib::MainContext::default();
        let until = Instant::now() + long;
        while Instant::now() < until {
            window.desk.tell();
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A wait with nothing to wake it answers at its timeout.
    #[test]
    fn a_wait_times_out() {
        crate::ui::on_gtk(|| {
            let (window, pane, _session, _) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            agents_on();
            let waiting = Rc::new(Terminal::default());
            let (code, _) = command_from(&window, "trading wait --timeout 1 --cross 1", &Typed(waiting.clone()));
            assert_eq!(code, 0);
            let context = glib::MainContext::default();
            let until = Instant::now() + Duration::from_secs(3);
            while !waiting.ended.get() && Instant::now() < until {
                context.iteration(false);
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(waiting.said.borrow().starts_with("woke timeout"), "{}", waiting.said.borrow());
        });
    }

    /// The chart in numbers: bars, indicator values, and a quote.
    #[test]
    fn a_chart_is_read_in_numbers() {
        crate::ui::on_gtk(|| {
            let (window, pane, _session, _) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.bar_count() > 5));
            let (code, why) = command(&window, "chart bars --last 3 --json");
            assert_eq!(code, crate::cli::EXIT_USAGE, "off, there is no such command: {why}");
            agents_on();
            let (code, out) = command(&window, "chart bars --last 3 --json");
            assert_eq!(code, 0, "{out}");
            let bars: Value = serde_json::from_str(&out).expect("JSON");
            assert_eq!(bars["bars"].as_array().map(Vec::len), Some(3), "{bars}");
            let close = pane.view.last_price().map(|last| (last * 100.0).round() / 100.0);
            assert_eq!(bars["bars"][2][4].as_f64(), close, "{bars}");
            let (code, out) = command(&window, "chart values --json");
            assert_eq!(code, 0, "{out}");
            let values: Value = serde_json::from_str(&out).expect("JSON");
            assert!(values["indicators"].is_array(), "{values}");
            assert!(values["levels"].is_object(), "{values}");
            let (code, out) = command(&window, "symbol quote ES --json");
            assert_eq!(code, 0, "{out}");
            let quote: Value = serde_json::from_str(&out).expect("JSON");
            assert_eq!(quote["source"], "chart", "{quote}");
            assert_eq!(quote["last"].as_f64(), close, "{quote}");
            assert!(quote["bid"].is_null());
        });
    }

    /// Alerts are set, listed and taken off.
    #[test]
    fn alerts_are_set_listed_and_removed() {
        crate::ui::on_gtk(|| {
            let (window, pane, _session, _) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            agents_on();
            let (code, out) = command(&window, "trading alert 9000");
            assert_eq!(code, 0, "{out}");
            let (code, out) = command(&window, "trading alert --json");
            assert_eq!(code, 0, "{out}");
            assert!(out.contains("9000"), "{out}");
            let (code, out) = command(&window, "trading alert --remove 1");
            assert_eq!(code, 0, "{out}");
            assert!(!out.contains("9000"), "{out}");
            let (code, _) = command(&window, "trading alert --remove 1");
            assert_eq!(code, crate::cli::EXIT_NOT_FOUND);
        });
    }
}
