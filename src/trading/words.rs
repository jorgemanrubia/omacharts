//! What the chart menu, the toasts, the pills and the account part say.

use super::domain::grouped;
use super::*;

/// Which side of the market a refused price was on, for the toast.
pub fn crossed_text(price: &str, kind: OrderKind, side: Side) -> String {
    let above = matches!((kind, side), (OrderKind::Limit, Side::Buy) | (OrderKind::Stop, Side::Sell));
    let which = if above { "above" } else { "below" };
    format!("{price} is now {which} the market · not placed")
}

/// What an order dragged to a price says beside the hand, and what a
/// live account is asked: "Sell Limit 1 · 7,640.00 · +11 ticks", the
/// ticks from where it started when the chart knows its tick.
pub fn preview_text(side: Side, kind: OrderKind, qty: u32, price: &str, ticks: Option<i64>) -> String {
    let words = format!("{} {qty} · {price}", order_words(side, kind));
    match ticks {
        Some(0) => format!("{words} · 0 ticks"),
        Some(ticks) if ticks.abs() == 1 => format!("{words} · {ticks:+} tick"),
        Some(ticks) => format!("{words} · {ticks:+} ticks"),
        None => words,
    }
}

/// Why a dragged order cannot go where the hand is: "Stop above market ·
/// not allowed".
pub fn refused_text(kind: OrderKind, price: f64, last: f64) -> String {
    let at = match price.total_cmp(&last) {
        std::cmp::Ordering::Greater => "above market",
        std::cmp::Ordering::Less => "below market",
        std::cmp::Ordering::Equal => "at market",
    };
    format!("{} {at} · not allowed", kind.word())
}

/// "Buy Limit", "Sell Stop".
pub fn order_words(side: Side, kind: OrderKind) -> String {
    format!("{} {}", side.word(), kind.word())
}

/// A row of the chart menu, for `size` of the contract: "Buy Market · 2
/// ESZ6 · 951", "Sell Limit 2 @ 7,620.00".
pub fn menu_text(side: Side, kind: OrderKind, size: Quantity, price: &str, contract: &str, account: &str) -> String {
    match kind {
        OrderKind::Market => format!("{} · {size} {contract} · {account}", order_words(side, kind)),
        _ => format!("{} {size} @ {price}", order_words(side, kind)),
    }
}

/// What a toast says when an order placed or cancelled here moves on:
/// working, filled (or part of it), cancelled or rejected, each only once
/// the broker has said so. Nothing for a step that is none of them.
pub fn outcome_text(order: &Order, before: Stage, decimals: usize, account: &str) -> Option<String> {
    if before == order.status.stage {
        return None;
    }
    let price = |price: Option<f64>| price.map(|price| omacharts_engine::grouped_decimals(price, decimals));
    match order.status.stage {
        Stage::Filled => Some(match price(order.fill_price) {
            Some(at) => format!("Filled {} {} at {at} · {account}", order.qty, order.contract),
            None => format!("Filled {} {} · {account}", order.qty, order.contract),
        }),
        Stage::PartiallyFilled if order.filled > 0 => {
            Some(format!("Filled {} of {} {} · {account}", order.filled, order.qty, order.contract))
        }
        Stage::Cancelled if order.kind != OrderKind::Market => Some(format!(
            "Cancelled {} {}",
            order_words(order.side, order.kind),
            price(order.price).unwrap_or_default()
        )),
        Stage::Working if order.kind != OrderKind::Market && before == Stage::Pending => {
            Some(format!("Working: {} {}", order_words(order.side, order.kind), price(order.price).unwrap_or_default()))
        }
        Stage::Rejected => Some(match &order.status.said {
            Some(why) => format!("Rejected: {why}"),
            None => "Rejected".to_string(),
        }),
        _ => None,
    }
}

/// What a toast says of an order the broker's refreshed book has never
/// heard of, sent before a drop: "Buy Limit 1 ESZ6 never reached the
/// broker".
pub fn never_placed_text(order: &OrderRequest) -> String {
    format!("{} {} {} never reached the broker", order_words(order.side, order.kind), order.qty, order.contract)
}

/// Money as a P&L reads: "+$125.00", "-$3.94", "+12.50 EUR".
pub fn pnl_text(value: f64, currency: Option<&str>) -> String {
    let sign = if value < 0.0 { "-" } else { "+" };
    let whole = grouped(value.abs().trunc() as u64);
    let cents = ((value.abs().fract() * 100.0).round() as u64).min(99);
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

/// What a position's pill says: its size and Rithmic's open P&L, or its
/// price until the P&L has arrived.
pub fn position_text(held: &Position, decimals: usize, currency: Option<&str>) -> String {
    match (held.open_pnl, held.avg) {
        (Some(pnl), _) => format!("{} · {}", net_text(held.net), pnl_text(pnl, currency)),
        (None, Some(avg)) => format!("{} @ {}", net_text(held.net), omacharts_engine::grouped_decimals(avg, decimals)),
        (None, None) => net_text(held.net),
    }
}

/// What a market order on its way says beside the last price, until it
/// fills: "Buying 1…".
pub fn buying_text(side: Side, qty: u32) -> String {
    match side {
        Side::Buy => format!("Buying {qty}…"),
        Side::Sell => format!("Selling {qty}…"),
    }
}

/// What a working order's pill says: "▼ Sell Limit 1", and once some of it
/// has filled, how much of how much: "▼ Sell Limit 1/2".
pub fn order_pill_text(order: &Order) -> String {
    let size = match order.status.stage {
        Stage::PartiallyFilled => format!("{}/{}", order.filled, order.qty),
        _ => order.qty.to_string(),
    };
    format!("{} {} {size}", order.side.glyph(), order_words(order.side, order.kind))
}

/// What the account part says it opens.
pub const PART_TOOLTIP: &str = "Account: click to choose this chart's account and data feed";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::tests::*;

    /// The words beside a dragged order and the reason one is refused.
    #[test]
    fn what_a_dragged_order_says() {
        assert_eq!(ticks_between(7_612.25, 7_615.0, 0.25), 11);
        assert_eq!(ticks_between(7_612.25, 7_612.0, 0.25), -1);
        assert_eq!(
            preview_text(Side::Sell, OrderKind::Limit, 1, "7,640.00", Some(11)),
            "Sell Limit 1 · 7,640.00 · +11 ticks"
        );
        assert_eq!(preview_text(Side::Buy, OrderKind::Stop, 2, "7,600.00", Some(-1)), "Buy Stop 2 · 7,600.00 · -1 tick");
        assert_eq!(preview_text(Side::Buy, OrderKind::Stop, 1, "7,600.00", Some(0)), "Buy Stop 1 · 7,600.00 · 0 ticks");
        assert_eq!(preview_text(Side::Buy, OrderKind::Limit, 1, "7,600.00", None), "Buy Limit 1 · 7,600.00");
        assert_eq!(refused_text(OrderKind::Stop, 7_640.0, 7_612.25), "Stop above market · not allowed");
        assert_eq!(refused_text(OrderKind::Limit, 7_600.0, 7_612.25), "Limit below market · not allowed");
        assert_eq!(refused_text(OrderKind::Limit, 7_612.25, 7_612.25), "Limit at market · not allowed");
    }

    #[test]
    fn what_the_menu_and_toasts_say() {
        let one = Quantity::ONE;
        let two = Quantity::new(2).unwrap();
        assert_eq!(menu_text(Side::Buy, OrderKind::Market, one, "", "ESZ6", "951"), "Buy Market · 1 ESZ6 · 951");
        assert_eq!(menu_text(Side::Buy, OrderKind::Market, two, "", "ESZ6", "951"), "Buy Market · 2 ESZ6 · 951");
        assert_eq!(menu_text(Side::Sell, OrderKind::Limit, two, "7,620.00", "ESZ6", "951"), "Sell Limit 2 @ 7,620.00");
        assert_eq!(menu_text(Side::Buy, OrderKind::Stop, one, "7,630.25", "ESZ6", "951"), "Buy Stop 1 @ 7,630.25");

        let mut bought = order("a", Side::Buy, OrderKind::Market, None, Stage::Filled);
        bought.fill_price = Some(7_612.25);
        assert_eq!(outcome_text(&bought, Stage::Pending, 2, "951").as_deref(), Some("Filled 1 ESZ6 at 7,612.25 · 951"));
        assert_eq!(outcome_text(&bought, Stage::Filled, 2, "951"), None, "said once");
        let working = order("b", Side::Sell, OrderKind::Limit, Some(7_620.0), Stage::Working);
        assert_eq!(outcome_text(&working, Stage::Pending, 2, "951").as_deref(), Some("Working: Sell Limit 7,620.00"));
        let refused = order("c", Side::Buy, OrderKind::Limit, Some(1.0), Status::said(Stage::Rejected, "price out of band"));
        assert_eq!(outcome_text(&refused, Stage::Pending, 2, "951").as_deref(), Some("Rejected: price out of band"));
        assert_eq!(
            outcome_text(&order("d", Side::Buy, OrderKind::Limit, Some(7_620.0), Stage::Cancelled), Stage::Working, 2, "951")
                .as_deref(),
            Some("Cancelled Buy Limit 7,620.00")
        );
        let mut part = order("e", Side::Sell, OrderKind::Limit, Some(7_620.0), Stage::PartiallyFilled);
        (part.qty, part.filled) = (2, 1);
        assert_eq!(outcome_text(&part, Stage::Working, 2, "951").as_deref(), Some("Filled 1 of 2 ESZ6 · 951"));

        let mut held = Position {
            account: "t/951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            net: 1,
            avg: Some(7_612.25),
            open_pnl: None,
            tick: Some(0.25),
        };
        assert_eq!(position_text(&held, 2, Some("USD")), "+1 @ 7,612.25");
        held.open_pnl = Some(125.0);
        assert_eq!(position_text(&held, 2, Some("USD")), "+1 · +$125.00");
        held.net = -2;
        held.open_pnl = Some(-3.94);
        assert_eq!(position_text(&held, 2, None), "-2 · -$3.94");
        assert_eq!(money_text(1_000_000.0, Some("USD")), "$1,000,000.00");
        assert_eq!(pnl_text(12.5, Some("EUR")), "+12.50 EUR");
    }
}
