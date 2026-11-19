//! Order templates on Sierra Chart: the app places a bracket's exits as
//! the entry fills, and keeps them within the position.
//!
//! DTC has brackets of its own — an entry sent as a parent, then one OCO
//! pair naming it — but only one pair to a parent, and nothing yet has
//! confirmed what Sierra Chart's DTC server does with one (see
//! `doc/prop-trading/sierra.md`). A template is any number of targets and
//! stops, and a market entry has no price for a pair's prices to keep
//! their distance from. So the app does it, the same way for every
//! template, Sierra Chart only:
//!
//! - **On a fill.** Each part of the entry heard filled gets its exits at
//!   once, at their distance from the entry's average fill: the template's
//!   legs that cover those contracts, a target and a stop paired into one
//!   OCO submission wherever both cover the same contracts
//!   ([`segments`]), so that once placed Sierra Chart itself cancels a
//!   stop whose target filled, and the other way round. A server that
//!   takes no OCO gets them one by one, and the keeping below cancels what
//!   is left over.
//! - **Within the position.** Whenever the book moves, and once at sign-in,
//!   the exits the app placed ([`EXIT`]) are added up for each account and
//!   contract: a group, the targets or the stops, for more than is held, or
//!   on the side that would open rather than close, is cut back, newest
//!   first, once it has stood so for [`SETTLE`] — longer than a fill's
//!   order update and its position update take to arrive apart.
//! - **Across a restart.** The entry carries its template's exits in its
//!   tag ([`entry_tag`]), so a working entry left by an earlier run still
//!   gets its exits when it fills, and only what fills from then on: what
//!   filled while the app was closed is said in the Log, not guessed at.
//!
//! All of it needs the app running and signed in: an entry that fills while
//! it is closed gets no exits until it runs again. The chart says so,
//! "· Omacharts" after the template's name
//! ([`crate::trading::Capabilities::bracket_holder`]).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use omacharts_engine::providers::sierra as feed;
use omacharts_engine::specs::{self, root_of};

use crate::log_words;
use crate::trading::brackets::{ExitLeg, Exits};
use crate::trading::{Failure, OrderRequest, Session, Side};

use super::SierraSession;

/// The tag an entry placed with a template's exits starts with.
pub const ENTRY: &str = "omx-b:";
/// The tag of an exit the app placed for one.
pub const EXIT: &str = "omx-x:";

/// How long the exits the app placed may stand over the position before
/// they are cut to it: longer than the order update of a fill and its
/// position update take to arrive apart.
#[cfg(not(test))]
pub const SETTLE: Duration = Duration::from_millis(1500);
#[cfg(test)]
pub const SETTLE: Duration = Duration::from_millis(250);

/// The most `FreeFormText` carries, as the DTC crate clips it.
const TAG_LENGTH: usize = 47;

/// Whether an order's tag says the app placed it as a bracket's exit.
pub fn is_exit(tag: Option<&str>) -> bool {
    tag.is_some_and(|tag| tag.starts_with(EXIT))
}

/// A short key that ties an entry's exits to it across restarts.
pub fn new_key() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    radix36(nanos as u64)
}

fn radix36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The entry's tag: `omx-b:<key>:<exits>`, the exits as `T8x1.16x1S6x2`,
/// ticks by quantity. None when that is longer than the server keeps: the
/// exits are still placed while the app runs, but a restart cannot know
/// them.
pub fn entry_tag(key: &str, exits: &Exits) -> Option<String> {
    let legs = |legs: &[ExitLeg]| {
        legs.iter()
            .map(|leg| format!("{}x{}", leg.ticks, leg.qty))
            .collect::<Vec<_>>()
            .join(".")
    };
    let mut code = String::new();
    if !exits.targets.is_empty() {
        code.push('T');
        code.push_str(&legs(&exits.targets));
    }
    if !exits.stops.is_empty() {
        code.push('S');
        code.push_str(&legs(&exits.stops));
    }
    let tag = format!("{ENTRY}{key}:{code}");
    (tag.len() <= TAG_LENGTH).then_some(tag)
}

/// The key and the exits an entry's tag carries.
pub fn read_entry_tag(tag: &str) -> Option<(String, Exits)> {
    let rest = tag.strip_prefix(ENTRY)?;
    let (key, code) = rest.split_once(':')?;
    let legs = |text: &str| -> Option<Vec<ExitLeg>> {
        text.split('.')
            .map(|leg| {
                let (ticks, qty) = leg.split_once('x')?;
                let leg = ExitLeg {
                    ticks: ticks.parse().ok()?,
                    qty: qty.parse().ok()?,
                };
                (leg.ticks > 0 && leg.qty > 0).then_some(leg)
            })
            .collect()
    };
    let (targets, stops) = match code.split_once('S') {
        Some((targets, stops)) => (targets, Some(stops)),
        None => (code, None),
    };
    let targets = match targets.strip_prefix('T') {
        Some(text) => legs(text)?,
        None if targets.is_empty() => Vec::new(),
        None => return None,
    };
    let stops = match stops {
        Some(text) => legs(text)?,
        None => Vec::new(),
    };
    let exits = Exits { targets, stops };
    (!key.is_empty() && !exits.is_empty()).then(|| (key.to_string(), exits))
}

/// The key in an entry's tag.
fn key_of(tag: &str) -> Option<String> {
    let rest = tag.strip_prefix(ENTRY)?;
    let key = rest.split(':').next()?;
    (!key.is_empty()).then(|| key.to_string())
}

/// The exit's tag: `omx-x:<key>`, the key of the entry it closes.
pub fn exit_tag(key: &str) -> String {
    format!("{EXIT}{key}")
}

/// What of `legs` closes the entry's contracts `from` up to `to`, counted
/// as the legs are listed: the first leg the first contracts. Taken part by
/// part as the entry fills, the parts add up to the legs.
pub fn slice(legs: &[ExitLeg], from: u32, to: u32) -> Vec<ExitLeg> {
    let mut start = 0;
    let mut out = Vec::new();
    for leg in legs {
        let end = start + leg.qty;
        let qty = end.min(to).saturating_sub(start.max(from));
        if qty > 0 {
            out.push(ExitLeg {
                qty,
                ticks: leg.ticks,
            });
        }
        start = end;
    }
    out
}

/// One placement of exits: a target and a stop for the same contracts, as
/// one OCO pair, or one of them alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub qty: u32,
    pub target: Option<u32>,
    pub stop: Option<u32>,
}

/// The exits paired: walking the targets and the stops side by side, the
/// contracts both close are cut wherever either changes leg, so that each
/// piece has one target and one stop, the two never closing more than the
/// piece. A template with one group only, or a group shorter than the
/// other, has pieces with one exit.
pub fn segments(exits: &Exits) -> Vec<Segment> {
    let mut out = Vec::new();
    let (mut targets, mut stops) = (exits.targets.iter().copied(), exits.stops.iter().copied());
    let (mut target, mut stop) = (targets.next(), stops.next());
    loop {
        match (&mut target, &mut stop) {
            (Some(t), Some(s)) => {
                let qty = t.qty.min(s.qty);
                out.push(Segment {
                    qty,
                    target: Some(t.ticks),
                    stop: Some(s.ticks),
                });
                t.qty -= qty;
                s.qty -= qty;
                if t.qty == 0 {
                    target = targets.next();
                }
                if s.qty == 0 {
                    stop = stops.next();
                }
            }
            (Some(t), None) => {
                out.push(Segment {
                    qty: t.qty,
                    target: Some(t.ticks),
                    stop: None,
                });
                target = targets.next();
            }
            (None, Some(s)) => {
                out.push(Segment {
                    qty: s.qty,
                    target: None,
                    stop: Some(s.ticks),
                });
                stop = stops.next();
            }
            (None, None) => return out,
        }
    }
}

/// An entry whose exits the app places as it fills.
#[derive(Clone, Debug)]
struct Held {
    /// The ids the server and this run know the entry by.
    client: Option<String>,
    server: Option<String>,
    key: String,
    /// Its template's exits, for the whole of the entry.
    exits: Exits,
    /// How much of it has its exits already.
    covered: u32,
}

impl Held {
    /// By the server's id once known: a client id is only this run's, and
    /// a later run's count starts again.
    fn is(&self, order: &dtc::Order) -> bool {
        match &self.server {
            Some(server) => order.server_id.as_ref() == Some(server),
            None => {
                self.client.is_some()
                    && order.client_id == self.client
                    && order.tag.as_deref().and_then(key_of).as_ref() == Some(&self.key)
            }
        }
    }
}

/// The app's part of Sierra Chart's brackets: the entries it is to place
/// exits for, and how long each account's exits have stood over its
/// position.
#[derive(Default)]
pub struct Keeper {
    held: Mutex<Vec<Held>>,
    /// The tag the app asked each entry placed with, by its key, which the
    /// book says in place of the one it went out
    /// with so that the chart knows it for its own, for as long as the
    /// session lasts.
    asked: Mutex<HashMap<String, String>>,
    over: Mutex<HashMap<(String, String), Instant>>,
    /// The working entries of an earlier run were looked for.
    resumed: Mutex<bool>,
    /// One keeping at a time.
    busy: Mutex<()>,
}

impl Keeper {
    /// Whether there is anything to look at as time passes, rather than
    /// only as the book moves.
    pub fn waiting(&self) -> bool {
        !lock(&self.held).is_empty() || !lock(&self.over).is_empty() || !*lock(&self.resumed)
    }

    /// Asked before it is placed, so that the book never shows it under
    /// the tag it went out with.
    fn ask(&self, key: &str, asked: String) {
        lock(&self.asked).insert(key.to_string(), asked);
    }

    fn hold(&self, client: String, key: String, exits: Exits) {
        let mut held = lock(&self.held);
        // Picked up already by the first look at the book, as one an
        // earlier run left.
        if held.iter().any(|entry| entry.key == key) {
            return;
        }
        held.push(Held {
            client: Some(client),
            server: None,
            key,
            exits,
            covered: 0,
        });
    }

    /// The tag the app asked `order` placed with, for an entry placed in
    /// this session that went out with its exits in its tag instead.
    pub fn asked_tag(&self, order: &dtc::Order) -> Option<String> {
        lock(&self.asked)
            .get(&key_of(order.tag.as_deref()?)?)
            .cloned()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn closing(side: Side) -> dtc::Side {
    match side {
        Side::Buy => dtc::Side::Sell,
        Side::Sell => dtc::Side::Buy,
    }
}

impl SierraSession {
    /// Place `order` as a bracket's entry, the app to place `exits` as it
    /// fills: see the module.
    pub(super) fn place_held_bracket(
        &self,
        order: &OrderRequest,
        exits: &Exits,
    ) -> Result<String, Failure> {
        if specs::tick(root_of(&order.contract)).is_none() {
            return Err(Failure::Refused(format!(
                "the tick of {} is not known, so its exits could not be placed",
                order.contract
            )));
        }
        let key = new_key();
        let mut entry = order.clone();
        entry.tag = match entry_tag(&key, exits) {
            Some(tag) => tag,
            None => {
                log_words::say(log_words::note(
                    feed::LISTED.label,
                    format!(
                        "{} · The template's exits are too many to keep with the order on Sierra Chart: they are placed only while Omacharts runs",
                        order.contract
                    ),
                ));
                format!("{ENTRY}{key}")
            }
        };
        self.keeper.ask(&key, order.tag.clone());
        let id = self.place(&entry)?;
        self.keeper.hold(id.clone(), key, exits.clone());
        self.keep_brackets();
        Ok(id)
    }

    /// Place the exits of what has filled since the last look, and keep
    /// the app's exits within each position: see the module.
    pub(super) fn keep_brackets(&self) {
        let _one = lock(&self.keeper.busy);
        let book = self.trading.book();
        if !book.synced {
            return;
        }
        self.resume(&book);
        let mut held = lock(&self.keeper.held).clone();
        let mut done = Vec::new();
        for (at, entry) in held.iter_mut().enumerate() {
            let Some(order) = book.orders.iter().find(|order| entry.is(order)) else {
                done.push(at);
                continue;
            };
            if entry.server.is_none() {
                entry.server = order.server_id.clone();
            }
            if order.filled > entry.covered {
                // Its average fill, or once it has done, its own price.
                let fill = match (order.avg_fill_price, order.status.working()) {
                    (Some(fill), _) => fill,
                    (None, false) => match order.price1 {
                        Some(price) => price,
                        None => {
                            let contract = feed::exchange_contract(&order.symbol);
                            log_words::say(log_words::refused(
                                &order.account,
                                format!(
                                    "{contract} · Not protected: Sierra Chart never said the entry's fill price, so no exits were placed"
                                ),
                            ));
                            done.push(at);
                            continue;
                        }
                    },
                    (None, true) => continue,
                };
                let part = Exits {
                    targets: slice(&entry.exits.targets, entry.covered, order.filled),
                    stops: slice(&entry.exits.stops, entry.covered, order.filled),
                };
                entry.covered = order.filled;
                self.place_exits(order, &entry.key, &part, fill);
            }
            if !order.status.working() {
                done.push(at);
            }
        }
        {
            let mut kept = lock(&self.keeper.held);
            // What was held meanwhile, by a placement on another thread,
            // stays; what was looked at takes its new state.
            for entry in kept.iter_mut() {
                if let Some(seen) = held.iter().find(|seen| seen.key == entry.key) {
                    *entry = seen.clone();
                }
            }
            let gone: Vec<String> = done.iter().map(|at| held[*at].key.clone()).collect();
            kept.retain(|entry| !gone.contains(&entry.key));
        }
        self.cut_to_positions(&book);
    }

    /// Once, at the first book: a working entry an earlier run placed with
    /// its exits in its tag is held again, from what has filled of it now.
    fn resume(&self, book: &dtc::Book) {
        let mut resumed = lock(&self.keeper.resumed);
        if *resumed {
            return;
        }
        *resumed = true;
        let mut held = lock(&self.keeper.held);
        for order in book.orders.iter().filter(|order| order.status.working()) {
            let Some((key, exits)) = order.tag.as_deref().and_then(read_entry_tag) else {
                continue;
            };
            if held.iter().any(|entry| entry.key == key || entry.is(order)) {
                continue;
            }
            let contract = feed::exchange_contract(&order.symbol);
            let words = if order.filled > 0 {
                format!(
                    "{contract} · An order template's entry is working again: its exits go in as more of it fills. {} filled while Omacharts was closed: their exits are as they were left",
                    order.filled
                )
            } else {
                format!(
                    "{contract} · An order template's entry is working again: its exits go in as it fills"
                )
            };
            log_words::say(log_words::note(feed::LISTED.label, words));
            held.push(Held {
                client: order.client_id.clone(),
                server: order.server_id.clone(),
                key,
                exits,
                covered: order.filled,
            });
        }
    }

    /// The exits `part` of the entry `order`, from its fill at `fill`:
    /// each target and stop that close the same contracts as one OCO pair
    /// when the server takes them, else each alone.
    fn place_exits(&self, order: &dtc::Order, key: &str, part: &Exits, fill: f64) {
        let contract = feed::exchange_contract(&order.symbol);
        let Some(tick) = specs::tick(root_of(&contract)) else {
            log_words::say(log_words::refused(
                &order.account,
                format!("{contract} · No exits placed: the contract's tick is not known"),
            ));
            return;
        };
        let entry = match order.side {
            dtc::Side::Buy => Side::Buy,
            dtc::Side::Sell => Side::Sell,
        };
        let toward = if entry == Side::Buy { 1.0 } else { -1.0 };
        let price = |ticks: u32, sign: f64| {
            crate::trading::on_contract_tick(fill + sign * f64::from(ticks) * tick, Some(tick))
        };
        let exit = |kind: dtc::OrderKind, qty: u32, at: f64| dtc::NewOrder {
            account: order.account.clone(),
            symbol: order.symbol.clone(),
            exchange: order.exchange.clone(),
            side: closing(entry),
            qty,
            kind,
            price1: Some(at),
            price2: None,
            time_in_force: dtc::TimeInForce::Day,
            tag: Some(exit_tag(key)),
        };
        let pairs = self.trading.server().oco;
        let mut placed = Vec::new();
        for segment in segments(part) {
            let target = segment
                .target
                .map(|ticks| exit(dtc::OrderKind::Limit, segment.qty, price(ticks, toward)));
            let stop = segment
                .stop
                .map(|ticks| exit(dtc::OrderKind::Stop, segment.qty, price(ticks, -toward)));
            let sent = match (target, stop) {
                (Some(target), Some(stop)) if pairs => self
                    .trading
                    .place_pair(&[target.clone(), stop.clone()])
                    .map(|_| vec![target, stop]),
                (target, stop) => {
                    let mut sent = Vec::new();
                    let mut outcome = Ok(());
                    for one in [target, stop].into_iter().flatten() {
                        match self.trading.place(&one) {
                            Ok(_) => sent.push(one),
                            Err(error) => outcome = Err(error),
                        }
                    }
                    outcome.map(|()| sent)
                }
            };
            match sent {
                Ok(sent) => placed.extend(sent),
                Err(error) => log_words::say(log_words::refused(
                    &order.account,
                    format!(
                        "{contract} · Not protected: the exits for {} filled were refused: {}",
                        segment.qty,
                        super::said(error)
                    ),
                )),
            }
        }
        if !placed.is_empty() {
            let words: Vec<String> = placed
                .iter()
                .map(|exit| {
                    let kind = if exit.kind == dtc::OrderKind::Limit {
                        "target"
                    } else {
                        "stop"
                    };
                    format!("{kind} {} ×{}", exit.price1.unwrap_or_default(), exit.qty)
                })
                .collect();
            log_words::say(log_words::confirmed(
                &order.account,
                format!(
                    "{contract} · Exits placed by Omacharts for {} filled: {}",
                    segments(part)
                        .iter()
                        .map(|segment| segment.qty)
                        .sum::<u32>(),
                    words.join(", ")
                ),
            ));
        }
    }

    /// The exits the app placed, added up for each account and contract
    /// against what is held there, and cut back once they have stood over
    /// it for [`SETTLE`]: a group for more than is held, newest first, and
    /// all of those on the side that would open rather than close.
    fn cut_to_positions(&self, book: &dtc::Book) {
        let mut places: HashMap<(String, String), Vec<&dtc::Order>> = HashMap::new();
        for order in book
            .orders
            .iter()
            .filter(|order| order.status.working() && is_exit(order.tag.as_deref()))
        {
            places
                .entry((order.account.clone(), order.symbol.clone()))
                .or_default()
                .push(order);
        }
        let mut over = lock(&self.keeper.over);
        over.retain(|place, _| places.contains_key(place));
        for (place, exits) in places {
            let net = book
                .positions
                .iter()
                .find(|held| held.account == place.0 && held.symbol == place.1)
                .map_or(0, |held| held.qty);
            let mut cuts: Vec<(&dtc::Order, u32)> = Vec::new();
            for kind in [dtc::OrderKind::Limit, dtc::OrderKind::Stop] {
                let targets = kind == dtc::OrderKind::Limit;
                let mut group: Vec<&dtc::Order> = exits
                    .iter()
                    .copied()
                    .filter(|exit| (exit.kind == dtc::OrderKind::Limit) == targets)
                    .collect();
                group.sort_by_key(|exit| std::cmp::Reverse(exit.at));
                let closes = |exit: &dtc::Order| match exit.side {
                    dtc::Side::Sell => net > 0,
                    dtc::Side::Buy => net < 0,
                };
                let left = |exit: &dtc::Order| exit.qty.saturating_sub(exit.filled);
                let mut excess = group
                    .iter()
                    .filter(|exit| closes(exit))
                    .map(|exit| left(exit))
                    .sum::<u32>()
                    .saturating_sub(net.unsigned_abs());
                for exit in group {
                    if !closes(exit) {
                        cuts.push((exit, left(exit)));
                    } else if excess > 0 {
                        let cut = excess.min(left(exit));
                        cuts.push((exit, cut));
                        excess -= cut;
                    }
                }
            }
            if cuts.is_empty() {
                over.remove(&place);
                continue;
            }
            let since = *over.entry(place.clone()).or_insert_with(Instant::now);
            if since.elapsed() < SETTLE {
                continue;
            }
            over.remove(&place);
            let contract = feed::exchange_contract(&place.1);
            for (exit, cut) in cuts {
                let id = exit
                    .client_id
                    .clone()
                    .or_else(|| exit.server_id.clone())
                    .unwrap_or_default();
                let left = exit.qty.saturating_sub(exit.filled);
                let outcome = if cut >= left {
                    self.trading.cancel(&id)
                } else {
                    self.trading.modify(&id, None, Some(exit.qty - cut))
                };
                let what = if exit.kind == dtc::OrderKind::Limit {
                    "target"
                } else {
                    "stop"
                };
                let words = match (&outcome, cut >= left) {
                    (Ok(()), true) => format!(
                        "{contract} · Cancelled an exit Omacharts placed, a {what} for {left}: it was more than the position, {net}"
                    ),
                    (Ok(()), false) => format!(
                        "{contract} · Cut an exit Omacharts placed, a {what}, by {cut} to the position, {net}"
                    ),
                    (Err(error), _) => format!(
                        "{contract} · An exit Omacharts placed, a {what}, is more than the position, {net}, and could not be cut: {error}"
                    ),
                };
                log_words::say(match outcome {
                    Ok(()) => log_words::confirmed(&place.0, words),
                    Err(_) => log_words::refused(&place.0, words),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legs(legs: &[(u32, u32)]) -> Vec<ExitLeg> {
        legs.iter()
            .map(|&(qty, ticks)| ExitLeg { qty, ticks })
            .collect()
    }

    /// The exits travel in the entry's tag, within what the server keeps,
    /// and come back as they went.
    #[test]
    fn an_entry_s_tag_carries_its_exits() {
        let exits = Exits {
            targets: legs(&[(1, 8), (1, 16)]),
            stops: legs(&[(2, 6)]),
        };
        let tag = entry_tag("k1", &exits).expect("short enough");
        assert_eq!(tag, "omx-b:k1:T8x1.16x1S6x2");
        assert_eq!(read_entry_tag(&tag), Some(("k1".into(), exits)));
        let stops = Exits {
            targets: Vec::new(),
            stops: legs(&[(3, 12)]),
        };
        assert_eq!(
            read_entry_tag(&entry_tag("k", &stops).unwrap()),
            Some(("k".into(), stops))
        );
        let many = Exits {
            targets: legs(&[(1, 100); 9]),
            stops: legs(&[(9, 100)]),
        };
        assert_eq!(entry_tag(&new_key(), &many), None, "too long to keep");
        assert_eq!(read_entry_tag("omacharts:123"), None);
        assert_eq!(read_entry_tag("omx-b:k:Tx1"), None);
    }

    /// Part by part as the entry fills, the parts add up to the legs.
    #[test]
    fn a_part_filled_takes_the_legs_that_cover_it() {
        let targets = legs(&[(1, 8), (2, 16)]);
        assert_eq!(slice(&targets, 0, 1), legs(&[(1, 8)]));
        assert_eq!(slice(&targets, 1, 2), legs(&[(1, 16)]));
        assert_eq!(slice(&targets, 0, 2), legs(&[(1, 8), (1, 16)]));
        assert_eq!(slice(&targets, 2, 3), legs(&[(1, 16)]));
        assert_eq!(slice(&targets, 3, 4), Vec::new());
    }

    /// Each piece has one target and one stop for the same contracts.
    #[test]
    fn targets_and_stops_are_paired_contract_by_contract() {
        let seg = |qty, target, stop| Segment { qty, target, stop };
        let runner = Exits {
            targets: legs(&[(1, 8), (1, 16)]),
            stops: legs(&[(2, 6)]),
        };
        assert_eq!(
            segments(&runner),
            [seg(1, Some(8), Some(6)), seg(1, Some(16), Some(6))]
        );
        let uneven = Exits {
            targets: legs(&[(2, 8), (1, 16)]),
            stops: legs(&[(1, 4), (2, 6)]),
        };
        assert_eq!(
            segments(&uneven),
            [
                seg(1, Some(8), Some(4)),
                seg(1, Some(8), Some(6)),
                seg(1, Some(16), Some(6))
            ]
        );
        let targets = Exits {
            targets: legs(&[(2, 8)]),
            stops: Vec::new(),
        };
        assert_eq!(segments(&targets), [seg(2, Some(8), None)]);
    }
}
