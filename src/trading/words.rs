//! What the chart menu, the toasts, the pills and the account part say.

use super::domain::grouped;
use super::*;

/// Which side of the market a refused price was on, for the Log: "Not
/// placed · 7,612.50 is above the market now".
pub fn crossed_text(price: &str, kind: OrderKind, side: Side) -> String {
    let above = matches!((kind, side), (OrderKind::Limit, Side::Buy) | (OrderKind::Stop, Side::Sell));
    let which = if above { "above" } else { "below" };
    format!("Not placed · {price} is {which} the market now")
}

/// What an order dragged to a price says after its tag: "7,640.00 · +11
/// ticks", the ticks from where it started when the chart knows its tick.
pub fn preview_text(price: &str, ticks: Option<i64>) -> String {
    match ticks {
        Some(0) => format!("{price} · 0 ticks"),
        Some(ticks) if ticks.abs() == 1 => format!("{price} · {ticks:+} tick"),
        Some(ticks) => format!("{price} · {ticks:+} ticks"),
        None => price.to_string(),
    }
}

/// Why a dragged order cannot go where the hand is: "Stop above market ·
/// not allowed".
pub fn refused_text(kind: OrderKind, price: f64, last: f64) -> String {
    format!("{} {} · not allowed", kind.word(), market_side(price, last))
}

/// Why a dragged order cannot go past the day's limit: "Past daily limit
/// 7,117.25 · not allowed".
pub fn past_limit_preview(limit: &str) -> String {
    format!("Past daily limit {limit} · not allowed")
}

/// The same, as the reason a refusal gives: "past daily limit 7,117.25".
pub fn past_limit_text(limit: &str) -> String {
    format!("past daily limit {limit}")
}

/// The same, as the reason a refusal gives: "stop above market".
pub fn wrong_side_text(kind: OrderKind, price: f64, last: f64) -> String {
    format!("{} {}", kind.key(), market_side(price, last))
}

fn market_side(price: f64, last: f64) -> &'static str {
    match price.total_cmp(&last) {
        std::cmp::Ordering::Greater => "above market",
        std::cmp::Ordering::Less => "below market",
        std::cmp::Ordering::Equal => "at market",
    }
}

/// "Buy Limit", "Sell Stop".
pub fn order_words(side: Side, kind: OrderKind) -> String {
    format!("{} {}", side.word(), kind.word())
}

/// What a link made says: who holds it, and, for the app's own, that it
/// lasts only while the app runs.
pub fn linked_text(holder: Holder) -> &'static str {
    match holder {
        Holder::Broker => "Linked · whichever fills first cancels the other",
        Holder::App => "Linked by Omacharts while it runs · whichever fills first cancels the other",
    }
}

/// A row of the chart menu, for `size` of the contract: "Buy 2 ESZ6 at
/// market", "Sell Limit 2 ESZ6 at 7,620.00".
pub fn menu_text(side: Side, kind: OrderKind, size: Quantity, price: &str, contract: &str) -> String {
    match kind {
        OrderKind::Market => format!("{} {size} {contract} at market", side.word()),
        _ => format!("{} {size} {contract} at {price}", order_words(side, kind)),
    }
}

/// An order in words, with its price: "Buy Limit 1 ESZ6 at 7,620.00", and
/// "Buy 1 ESZ6 at market" for a market order.
pub fn order_text(side: Side, kind: OrderKind, qty: u32, contract: &str, price: Option<&str>) -> String {
    match (kind, price) {
        (OrderKind::Market, _) => format!("{} {qty} {contract} at market", side.word()),
        (_, Some(price)) => format!("{} {qty} {contract} at {price}", order_words(side, kind)),
        (_, None) => format!("{} {qty} {contract}", order_words(side, kind)),
    }
}

/// What the Log says when an order placed or cancelled here moves on:
/// working, filled (or part of it), cancelled or rejected, each only once
/// the broker has said so. Nothing for a step that is none of them. The
/// account is the entry's source, so it is not said again here.
pub fn outcome_text(order: &Order, before: Stage, decimals: usize) -> Option<String> {
    if before == order.status.stage {
        return None;
    }
    let price = |price: Option<f64>| price.map(|price| omacharts_engine::grouped_decimals(price, decimals));
    let whole = || order_text(order.side, order.kind, order.qty, &order.contract, price(order.price).as_deref());
    match order.status.stage {
        Stage::Filled => Some(match price(order.fill_price) {
            Some(at) => format!("Filled {} {} at {at}", order.qty, order.contract),
            None => format!("Filled {} {}", order.qty, order.contract),
        }),
        Stage::PartiallyFilled if order.filled > 0 => {
            Some(format!("Filled {} of {} {}", order.filled, order.qty, order.contract))
        }
        Stage::Cancelled if order.kind != OrderKind::Market => Some(format!("Cancelled {}", whole())),
        Stage::Working if order.kind != OrderKind::Market && before == Stage::Pending => {
            Some(format!("Working · {}", whole()))
        }
        Stage::Rejected => Some(match (order.status.bound, order.price, &order.status.said) {
            (Some(bound), Some(at), _) => {
                let side = if at < bound.price { "below" } else { "above" };
                let limit = price(Some(bound.price)).unwrap_or_default();
                format!("Rejected · {} · too far {side} the market ({} {limit})", whole(), bound.kind.words())
            }
            (_, _, Some(why)) => format!("Rejected · {} · {why}", whole()),
            _ => format!("Rejected · {}", whole()),
        }),
        _ => None,
    }
}

/// What the Log says of an order the broker's refreshed book has never
/// heard of, sent before a drop: "Not placed · Buy Limit 1 ESZ6 never
/// reached the broker".
pub fn never_placed_text(order: &OrderRequest) -> String {
    format!("Not placed · {} {} {} never reached the broker", order_words(order.side, order.kind), order.qty, order.contract)
}

/// Money as a P&L reads: "+$125.00", "-$3.94", "+12.50 EUR".
pub fn pnl_text(value: f64, currency: Option<&str>) -> String {
    let sign = if value < 0.0 { "-" } else { "+" };
    // To the cent first, then split: 15,349.999… is $15,350.00, not
    // $15,349.99, and never 15,349 and a hundred cents.
    let cents = (value.abs() * 100.0).round() as u64;
    let whole = grouped(cents / 100);
    let cents = cents % 100;
    match currency {
        None | Some("USD") => format!("{sign}${whole}.{cents:02}"),
        Some(currency) => format!("{sign}{whole}.{cents:02} {currency}"),
    }
}

/// Money as a balance reads: "$1,000,000.00".
pub fn money_text(value: f64, currency: Option<&str>) -> String {
    let text = pnl_text(value, currency);
    match value < 0.0 {
        true => text,
        false => text.trim_start_matches('+').to_string(),
    }
}

/// A position's size: "+1", "-2".
pub fn net_text(net: i32) -> String {
    format!("{net:+}")
}

/// The tag on a position's label: its side in the shape an entry's has
/// and its size, "▲ 5" long and "▼ 2" short.
pub fn held_tag(net: i32) -> String {
    let side = if net > 0 { Side::Buy } else { Side::Sell };
    format!("{} {}", side.glyph(), net.unsigned_abs())
}

/// What a market order on its way says beside the last price, until it
/// fills: "Buying 1…".
pub fn buying_text(side: Side, qty: u32) -> String {
    match side {
        Side::Buy => format!("Buying {qty}…"),
        Side::Sell => format!("Selling {qty}…"),
    }
}

/// The tag on an order's label: its kind and size in a code, L for a
/// limit and STP for a stop. "L 1" and "STP 1" for the orders that close
/// some of the position, whose side is the position's other one and goes
/// without saying; "▲ L 1" and "▼ STP 1" for the ones that open or add to
/// one, the side in the shape, as the Orders tab tells it.
pub fn mark_tag(kind: OrderKind, side: Side, exit: bool, size: &str) -> String {
    match (kind, exit) {
        (OrderKind::Limit, true) => format!("L {size}"),
        (OrderKind::Stop, true) => format!("STP {size}"),
        (OrderKind::Limit, false) => format!("{} L {size}", side.glyph()),
        (OrderKind::Stop, false) => format!("{} STP {size}", side.glyph()),
        (OrderKind::Market, _) => format!("{} M {size}", side.glyph()),
    }
}

/// A working order's tag, closing some of a position of `net` or not:
/// "L 1", and once some of it has filled, how much of how much: "L 1/2".
pub fn order_tag(order: &Order, net: i32) -> String {
    let size = match order.status.stage {
        Stage::PartiallyFilled => format!("{}/{}", order.filled, order.qty),
        _ => order.qty.to_string(),
    };
    mark_tag(order.kind, order.side, is_exit(order.side, order.kind, net), &size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::tests::*;

    /// The words beside a dragged order and the reason one is refused.
    #[test]
    fn what_a_dragged_order_says() {
        assert_eq!(ticks_between(7_612.25, 7_615.0, 0.25), 11);
        assert_eq!(ticks_between(7_612.25, 7_612.0, 0.25), -1);
        assert_eq!(preview_text("7,640.00", Some(11)), "7,640.00 · +11 ticks");
        assert_eq!(preview_text("7,600.00", Some(-1)), "7,600.00 · -1 tick");
        assert_eq!(preview_text("7,600.00", Some(0)), "7,600.00 · 0 ticks");
        assert_eq!(preview_text("7,600.00", None), "7,600.00");
        assert_eq!(refused_text(OrderKind::Stop, 7_640.0, 7_612.25), "Stop above market · not allowed");
        assert_eq!(refused_text(OrderKind::Limit, 7_600.0, 7_612.25), "Limit below market · not allowed");
        assert_eq!(refused_text(OrderKind::Limit, 7_612.25, 7_612.25), "Limit at market · not allowed");
        assert_eq!(wrong_side_text(OrderKind::Stop, 7_640.0, 7_612.25), "stop above market");
    }

    #[test]
    fn what_the_menu_and_toasts_say() {
        let one = Quantity::ONE;
        let two = Quantity::new(2).unwrap();
        assert_eq!(menu_text(Side::Buy, OrderKind::Market, one, "", "ESZ6"), "Buy 1 ESZ6 at market");
        assert_eq!(menu_text(Side::Sell, OrderKind::Market, two, "", "ESZ6"), "Sell 2 ESZ6 at market");
        assert_eq!(menu_text(Side::Sell, OrderKind::Limit, two, "7,620.00", "ESZ6"), "Sell Limit 2 ESZ6 at 7,620.00");
        assert_eq!(menu_text(Side::Buy, OrderKind::Stop, one, "7,630.25", "ESZ6"), "Buy Stop 1 ESZ6 at 7,630.25");
        assert_eq!(order_text(Side::Buy, OrderKind::Limit, 1, "ESZ6", Some("7,620.00")), "Buy Limit 1 ESZ6 at 7,620.00");
        assert_eq!(order_text(Side::Buy, OrderKind::Market, 2, "ESZ6", None), "Buy 2 ESZ6 at market");

        let mut bought = order("a", Side::Buy, OrderKind::Market, None, Stage::Filled);
        bought.fill_price = Some(7_612.25);
        assert_eq!(outcome_text(&bought, Stage::Pending, 2).as_deref(), Some("Filled 1 ESZ6 at 7,612.25"));
        assert_eq!(outcome_text(&bought, Stage::Filled, 2), None, "said once");
        let working = order("b", Side::Sell, OrderKind::Limit, Some(7_620.0), Stage::Working);
        assert_eq!(outcome_text(&working, Stage::Pending, 2).as_deref(), Some("Working · Sell Limit 1 ESZ6 at 7,620.00"));
        let refused = order("c", Side::Buy, OrderKind::Limit, Some(1.0), Status::said(Stage::Rejected, "price out of band"));
        assert_eq!(outcome_text(&refused, Stage::Pending, 2).as_deref(), Some("Rejected · Buy Limit 1 ESZ6 at 1.00 · price out of band"));
        let mut banded =
            order("f", Side::Sell, OrderKind::Limit, Some(7_480.0), Status::said(Stage::Rejected, "outside bands"));
        banded.status.bound = Some(Bound { price: 7_521.25, kind: BoundKind::Band });
        assert_eq!(
            outcome_text(&banded, Stage::Pending, 2).as_deref(),
            Some("Rejected · Sell Limit 1 ESZ6 at 7,480.00 · too far below the market (exchange band 7,521.25)")
        );
        assert_eq!(
            outcome_text(&order("d", Side::Buy, OrderKind::Limit, Some(7_620.0), Stage::Cancelled), Stage::Working, 2)
                .as_deref(),
            Some("Cancelled Buy Limit 1 ESZ6 at 7,620.00")
        );
        let mut part = order("e", Side::Sell, OrderKind::Limit, Some(7_620.0), Stage::PartiallyFilled);
        (part.qty, part.filled) = (2, 1);
        assert_eq!(outcome_text(&part, Stage::Working, 2).as_deref(), Some("Filled 1 of 2 ESZ6"));

        assert_eq!(net_text(1), "+1");
        assert_eq!(net_text(-2), "-2");
        assert_eq!(held_tag(5), "▲ 5");
        assert_eq!(held_tag(-2), "▼ 2");
        assert_eq!(money_text(1_000_000.0, Some("USD")), "$1,000,000.00");
        assert_eq!(pnl_text(125.0, Some("USD")), "+$125.00");
        assert_eq!(pnl_text(-3.94, None), "-$3.94");
        assert_eq!(pnl_text(12.5, Some("EUR")), "+12.50 EUR");
    }

    /// Money is rounded to the cent before it is split into dollars and
    /// cents, so a stop 102⅓ points under 3 ES at $50 says -$15,350.00,
    /// not -$15,349.99.
    #[test]
    fn money_rounds_to_the_cent() {
        assert_eq!(pnl_text((7_629.0 - (7_731.0 + 1.0 / 3.0)) * 3.0 * 50.0, Some("USD")), "-$15,350.00");
        assert_eq!(pnl_text(0.995, Some("USD")), "+$1.00");
        assert_eq!(pnl_text(0.994, Some("USD")), "+$0.99");
    }

    /// An order's tag: its kind and size in a code, the kind's telling
    /// whether it closes some of the position or not, and the side as a
    /// shape only on one that opens or adds, an exit's going without saying.
    #[test]
    fn what_an_orders_tag_says() {
        assert_eq!(mark_tag(OrderKind::Limit, Side::Sell, true, "1"), "L 1");
        assert_eq!(mark_tag(OrderKind::Stop, Side::Sell, true, "3"), "STP 3");
        assert_eq!(mark_tag(OrderKind::Limit, Side::Buy, false, "2"), "▲ L 2");
        assert_eq!(mark_tag(OrderKind::Stop, Side::Sell, false, "1"), "▼ STP 1");
        // A sell limit closes a long and opens or adds to a short.
        let sell = order("a", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Working);
        assert_eq!(order_tag(&sell, 1), "L 1");
        assert_eq!(order_tag(&sell, 0), "▼ L 1");
        assert_eq!(order_tag(&sell, -1), "▼ L 1");
        let mut part = order("b", Side::Buy, OrderKind::Stop, Some(7_640.0), Stage::PartiallyFilled);
        (part.qty, part.filled) = (2, 1);
        assert_eq!(order_tag(&part, -2), "STP 1/2");
        assert_eq!(order_tag(&part, 2), "▲ STP 1/2");
    }
}
