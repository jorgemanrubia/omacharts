//! What changed between two books, as events an agent reads one line at a
//! time: an order placed, filled, cancelled, rejected or moved; a position
//! opened, changed or closed; an account's P&L.
//!
//! Each event is a small JSON object with short keys, `ev` naming it, and
//! has a line of plain text that says the same in fewer words. An account
//! is named by its number alone.

use std::collections::HashSet;

use serde_json::{Value, json};

use super::reading::num;
use super::{Book, Order, OrderKind, Side, Stage};

/// An account key's number, `login/number` → `number`.
pub fn number(key: &str) -> &str {
    key.rsplit_once('/').map_or(key, |(_, number)| number)
}

fn side(side: Side) -> &'static str {
    match side {
        Side::Buy => "B",
        Side::Sell => "S",
    }
}

fn kind(kind: OrderKind) -> &'static str {
    match kind {
        OrderKind::Market => "mkt",
        OrderKind::Limit => "lmt",
        OrderKind::Stop => "stp",
    }
}

fn places(order: &Order) -> usize {
    super::reading::places(order.tick)
}

fn rounded(value: Option<f64>, places: usize) -> Value {
    match value.filter(|value| value.is_finite()) {
        Some(value) => {
            let scale = 10f64.powi(places as i32);
            json!((value * scale).round() / scale)
        }
        None => Value::Null,
    }
}

/// An order as an event called `ev`.
pub fn order(ev: &str, order: &Order) -> Value {
    let places = places(order);
    let mut value = json!({
        "ev": ev,
        "id": order.basket_id,
        "acct": number(&order.account),
        "sym": order.contract,
        "side": side(order.side),
        "qty": order.qty,
        "type": kind(order.kind),
    });
    let fields = value.as_object_mut().expect("an object");
    if order.price.is_some() {
        fields.insert("px".into(), rounded(order.price, places));
    }
    if order.filled > 0 {
        fields.insert("filled".into(), json!(order.filled));
        fields.insert("fill".into(), rounded(order.average_fill(), places));
    }
    if matches!(order.status.stage, Stage::Rejected | Stage::Cancelled)
        && let Some(said) = &order.status.said
    {
        fields.insert("why".into(), json!(said));
    }
    value
}

/// What `order` became, between being `was` and being as it is now, as
/// the event it makes, if it makes one.
fn change(was: Option<&Order>, now: &Order) -> Option<&'static str> {
    let stage = now.status.stage;
    match was {
        None => Some(match stage {
            Stage::Pending | Stage::Working => "placed",
            Stage::PartiallyFilled => "partial",
            Stage::Filled => "filled",
            Stage::Cancelled => "cancelled",
            Stage::Rejected => "rejected",
        }),
        Some(was) if was.status.stage != stage || was.filled != now.filled => match stage {
            Stage::Pending => None,
            // Taken by the exchange: nothing an agent acts on.
            Stage::Working if was.filled == now.filled => None,
            Stage::Working | Stage::PartiallyFilled => Some("partial"),
            Stage::Filled => Some("filled"),
            Stage::Cancelled => Some("cancelled"),
            Stage::Rejected => Some("rejected"),
        },
        Some(was) if was.price != now.price || was.qty != now.qty => Some("modified"),
        Some(_) => None,
    }
}

/// Every order and position event between `old` and `new`, orders first,
/// oldest first.
pub fn diff(old: &Book, new: &Book) -> Vec<Value> {
    let mut out = Vec::new();
    for now in new.orders.iter().rev() {
        let was = old.orders.iter().find(|was| was.basket_id == now.basket_id);
        if let Some(ev) = change(was, now) {
            out.push(order(ev, now));
        }
    }
    let mut seen = HashSet::new();
    for held in new.positions.iter().chain(old.positions.iter()) {
        if !seen.insert((held.account.clone(), held.contract.clone())) {
            continue;
        }
        let net_of = |book: &Book| {
            book.positions
                .iter()
                .find(|other| other.account == held.account && other.contract == held.contract)
                .map_or(0, |other| other.net)
        };
        let (was, net) = (net_of(old), net_of(new));
        if was == net {
            continue;
        }
        let avg = new
            .positions
            .iter()
            .find(|other| other.account == held.account && other.contract == held.contract && other.net != 0)
            .and_then(|other| other.avg);
        out.push(json!({
            "ev": "pos",
            "acct": number(&held.account),
            "sym": held.contract,
            "net": net,
            "was": was,
            "avg": rounded(avg, super::reading::places(held.tick)),
        }));
    }
    out
}

/// Each account whose day or open P&L moved by a cent or more since
/// `said`, which is what was last said of each, as `(key, day, open)`.
pub fn pnl(book: &Book, said: &[(String, f64, f64)]) -> Vec<(String, f64, f64)> {
    book.accounts
        .iter()
        .filter_map(|money| {
            let day = money.day_pnl.unwrap_or(0.0);
            let open = money.unrealized.unwrap_or(0.0);
            let before = said.iter().find(|(key, _, _)| *key == money.key);
            let moved = before.is_none_or(|(_, was_day, was_open)| (was_day - day).abs() >= 0.005 || (was_open - open).abs() >= 0.005);
            (moved && (money.day_pnl.is_some() || money.unrealized.is_some())).then(|| (money.key.clone(), day, open))
        })
        .collect()
}

/// A P&L event.
pub fn pnl_event(key: &str, day: f64, open: f64) -> Value {
    json!({"ev": "pnl", "acct": number(key), "day": rounded(Some(day), 2), "open": rounded(Some(open), 2)})
}

/// Money, signed, to the cent and no further: `+125`, `-37.5`.
pub fn signed(amount: f64) -> String {
    let text = num(amount, 2);
    match amount > 0.0 {
        true => format!("+{text}"),
        false => text,
    }
}

/// Whether an event is one a `trading wait` wakes for: something done to
/// an order or a position, an alert, a level crossed, a bar closed. A P&L moving with the market is not.
pub fn wakes(event: &Value) -> bool {
    !matches!(event["ev"].as_str(), Some("pnl" | "watching" | "placed"))
}

/// An event as one line of plain text.
pub fn text(event: &Value) -> String {
    let s = |key: &str| match &event[key] {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.as_f64().map_or_else(|| number.to_string(), |n| num(n, 6)),
        Value::Null => "-".into(),
        other => other.to_string(),
    };
    let ev = s("ev");
    match ev.as_str() {
        "pos" => {
            format!("pos {} {} was {} avg {} acct {}", s("sym"), signed_int(&event["net"]), signed_int(&event["was"]), s("avg"), s("acct"))
        }
        "pnl" => format!(
            "pnl acct {} day {} open {}",
            s("acct"),
            signed(event["day"].as_f64().unwrap_or(0.0)),
            signed(event["open"].as_f64().unwrap_or(0.0))
        ),
        "alert" => format!("alert {} {} {} at {} last {}", s("id"), s("sym"), s("dir"), s("px"), s("last")),
        "cross" => format!("cross {} {} last {}", s("dir"), s("px"), s("last")),
        "bar" => format!("bar closed {} o{} h{} l{} c{} v{}", s("t"), s("o"), s("h"), s("l"), s("c"), s("v")),
        "timeout" => "timeout".into(),
        "closed" => "closed: the window went away".into(),
        "watching" => "watching".into(),
        _ if event.get("id").is_some() => {
            let mut line = format!("{ev} {} {} {} {} {}", s("id"), s("side"), s("qty"), s("sym"), s("type"));
            if event.get("px").is_some() {
                line.push_str(&format!(" @{}", s("px")));
            }
            if event.get("fill").is_some() {
                line.push_str(&format!(" filled {} at {}", s("filled"), s("fill")));
            }
            if event.get("why").is_some() {
                line.push_str(&format!(" ({})", s("why")));
            }
            line
        }
        _ => event.to_string(),
    }
}

fn signed_int(value: &Value) -> String {
    match value.as_i64() {
        Some(n) if n > 0 => format!("+{n}"),
        Some(n) => n.to_string(),
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{Money, Position, Status};

    fn an_order(basket: &str, stage: Stage, filled: u32) -> Order {
        Order {
            account: "lucid/LT50-1043".into(),
            basket_id: basket.into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Buy,
            qty: 1,
            filled,
            kind: OrderKind::Limit,
            price: Some(7610.0),
            status: Status::from(stage),
            fill_price: (filled > 0).then_some(7610.0),
            fills: Vec::new(),
            at: 0,
            tag: None,
            tick: Some(0.25),
        }
    }

    fn held(net: i32) -> Position {
        Position {
            account: "lucid/LT50-1043".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            net,
            avg: Some(7610.0),
            open_pnl: None,
            tick: Some(0.25),
            point: Some(50.0),
        }
    }

    #[test]
    fn an_order_placed_then_filled_and_the_position_it_opened() {
        let empty = Book::default();
        let placed = Book { orders: vec![an_order("B1", Stage::Working, 0)], ..Book::default() };
        let events = diff(&empty, &placed);
        assert_eq!(
            events,
            vec![
                json!({"ev": "placed", "id": "B1", "acct": "LT50-1043", "sym": "ESZ6", "side": "B", "qty": 1, "type": "lmt", "px": 7610.0})
            ]
        );
        assert_eq!(text(&events[0]), "placed B1 B 1 ESZ6 lmt @7610");

        let filled = Book { orders: vec![an_order("B1", Stage::Filled, 1)], positions: vec![held(1)], ..Book::default() };
        let events = diff(&placed, &filled);
        assert_eq!(events.len(), 2);
        assert_eq!(text(&events[0]), "filled B1 B 1 ESZ6 lmt @7610 filled 1 at 7610");
        assert_eq!(events[1], json!({"ev": "pos", "acct": "LT50-1043", "sym": "ESZ6", "net": 1, "was": 0, "avg": 7610.0}));
        assert_eq!(text(&events[1]), "pos ESZ6 +1 was 0 avg 7610 acct LT50-1043");

        let flat = Book { orders: filled.orders.clone(), ..Book::default() };
        let events = diff(&filled, &flat);
        assert_eq!(events, vec![json!({"ev": "pos", "acct": "LT50-1043", "sym": "ESZ6", "net": 0, "was": 1, "avg": null})]);
        assert!(diff(&flat, &flat).is_empty(), "nothing moved, nothing said");
    }

    #[test]
    fn a_moved_order_and_a_rejected_one() {
        let was = Book { orders: vec![an_order("B1", Stage::Working, 0)], ..Book::default() };
        let mut moved = was.clone();
        moved.orders[0].price = Some(7611.0);
        assert_eq!(diff(&was, &moved)[0]["ev"], "modified");
        let mut rejected = was.clone();
        rejected.orders[0].status = Status::said(Stage::Rejected, "past the limit");
        let events = diff(&was, &rejected);
        assert_eq!(events[0]["why"], "past the limit");
        assert!(wakes(&events[0]));
        assert!(!wakes(&json!({"ev": "pnl"})));
    }

    #[test]
    fn p_and_l_is_said_when_it_moves_a_cent() {
        let book = Book {
            accounts: vec![Money { key: "a/1".into(), day_pnl: Some(125.0), unrealized: Some(50.0), ..Money::default() }],
            ..Book::default()
        };
        assert_eq!(pnl(&book, &[]), vec![("a/1".to_string(), 125.0, 50.0)]);
        assert!(pnl(&book, &[("a/1".into(), 125.001, 50.0)]).is_empty());
        assert_eq!(text(&pnl_event("a/1", 125.0, -37.5)), "pnl acct 1 day +125 open -37.5");
    }
}
