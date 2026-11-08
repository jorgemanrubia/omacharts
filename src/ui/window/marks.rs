//! The account's orders and position, as marks on its chart.
//!
//! The chart draws price marks and says what the hand did to them, and knows
//! nothing of orders. This is where a working order, the position and a
//! position held in another month become marks, where a drag on one is held
//! to what the exchange takes and told what it would realise, and where what
//! the hand did becomes a [`TradeEvent`] for the session.

use omacharts_chart::{AxisChip, MarkDrag, MarkEvent, MarkLink, MarkPreview, MarkRules, MarkTab, PriceMark};
use omacharts_engine::grouped_decimals;
use omacharts_engine::indicators::LineStyle;

use crate::trading::{self, Holder, OrderKind, Side};

/// A working order on the chart: its line at its price, its price on the
/// axis and its label inside the plot, in its colour and its kind's
/// pattern. The colour is its money's, profit or loss, for an exit, and
/// its side's for an order that opens or adds, which has no money to
/// speak of.
#[derive(Clone, Debug, PartialEq)]
pub struct OrderMark {
    /// The broker's id for it, which a drag or its × hands back.
    pub basket: String,
    pub side: Side,
    pub kind: OrderKind,
    pub qty: u32,
    pub price: f64,
    /// "▲ L 1", "L 2": its kind and size, filled in its colour. Empty
    /// for a market order on its way, which has only a body.
    pub tag: String,
    /// After the tag, in the ink: an exit's P&L if it fills, "Buying 1…"
    /// for a market order.
    pub body: Option<String>,
    pub price_text: String,
    /// The whole mark's: its line, its label's outline, its tag's fill,
    /// its × and its chip. An exit's money's, an entry's side's.
    pub colour: String,
    pub style: LineStyle,
    /// Pending: not yet working at the exchange, or asked to move or go
    /// and not yet answered. Drawn faint, and not to be taken hold of
    /// until it is. A market order is always this, beside the last price,
    /// until it fills.
    pub dim: bool,
    /// Linked with the position's other exit: drawn joined to it.
    pub link: Option<LinkMark>,
}

/// How an exit is linked with the other, which the line joining them says:
/// solid where the broker holds it, dashed where the app does, which lasts
/// only while it runs, and faint until the broker has confirmed either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkMark {
    pub holder: Holder,
    pub pending: bool,
}

/// The position held, at its average price: a heavier line, in the profit
/// or loss colour once the broker's P&L has come and in the long or short
/// colour until then, its chip filled once there is money to say.
#[derive(Clone, Debug, PartialEq)]
pub struct PositionMark {
    /// The contract it is held in.
    pub contract: String,
    pub price: f64,
    /// "▲ 1", or "ESZ6 ▼ 2" for one held in another month: its side and
    /// size, filled in its colour.
    pub tag: String,
    /// Its open P&L, "+$125.00", once it has come.
    pub body: Option<String>,
    pub price_text: String,
    /// The whole mark's: its money's sign once the P&L has come, its
    /// side's until then.
    pub colour: String,
    pub style: LineStyle,
    /// Contracts held: positive long, negative short.
    pub net: i32,
    /// The exits it can still be given, a tab each beside its pill.
    pub exits: Vec<ExitTab>,
    /// Being flattened: drawn faint until the book says it is gone.
    pub dim: bool,
    /// What one point of its price is worth on one contract, and in what,
    /// so a dragged exit can say what it would realise.
    pub point: Option<f64>,
    pub currency: Option<String>,
    /// The colours money gained and lost are written in.
    pub profit: String,
    pub loss: String,
}

/// A tab beside the position's label, dragged out to place an exit: the
/// target (a limit) or the stop, outlined in the look of the order it
/// would make, the profit colour for the one and the loss colour for the
/// other.
#[derive(Clone, Debug, PartialEq)]
pub struct ExitTab {
    pub kind: OrderKind,
    pub colour: String,
    pub style: LineStyle,
}

/// What the hand did to the marks, for the window, which owns the session:
/// the chart previews and says, and never sends anything itself.
#[derive(Clone, PartialEq, Debug)]
pub enum TradeEvent {
    /// An exit of `kind` for the position, at `price`.
    ExitAt { kind: OrderKind, price: f64 },
    /// The working order `basket`, to `price`.
    Move { basket: String, price: f64 },
    /// The working order's ×.
    Cancel(String),
    /// The position's ×.
    Flatten,
    /// The × of a position held in another month of the chart's root:
    /// flatten that contract.
    FlattenHeld(String),
}

/// What each kind of mark is known by on the chart.
const POSITION: &str = "position";
const HELD: &str = "held:";
const ORDER: &str = "order:";

/// How heavy an order's line is: a stop heavier than a limit, so the two
/// are told apart when they wear the same pattern, and both apart from the
/// last price's hairline.
fn order_weight(kind: OrderKind) -> f64 {
    match kind {
        OrderKind::Stop => 2.0,
        _ => 1.5,
    }
}

/// The position's line: solid by default, as heavy as a stop's.
const POSITION_WEIGHT: f64 = 2.0;

/// An exit tab's id on the chart.
fn tab_id(kind: OrderKind) -> &'static str {
    match kind {
        OrderKind::Limit => "limit",
        OrderKind::Stop => "stop",
        OrderKind::Market => "market",
    }
}

fn tab_kind(id: &str) -> Option<OrderKind> {
    [OrderKind::Limit, OrderKind::Stop, OrderKind::Market].into_iter().find(|kind| tab_id(*kind) == id)
}

/// The orders, then the positions in other months, then the position: the
/// order the chart draws them in, the position over the line joining its
/// exits.
pub fn price_marks(position: Option<&PositionMark>, held: &[PositionMark], orders: &[OrderMark]) -> Vec<PriceMark> {
    let mut marks = Vec::with_capacity(orders.len() + held.len() + 1);
    for order in orders {
        // A market order on its way has no price of its own: its label
        // beside the last price, no line, no × and no chip.
        let priced = order.kind != OrderKind::Market;
        marks.push(PriceMark {
            id: format!("{ORDER}{}", order.basket),
            price: order.price,
            tag: order.tag.clone(),
            body: order.body.clone(),
            // An exit's chip is filled in its money's colour, an entry's
            // outlined in its side's.
            chip: priced.then(|| AxisChip { text: order.price_text.clone(), filled: order.body.is_some() }),
            colour: order.colour.clone(),
            style: order.style,
            // Heavier than the last price's hairline, so a dashed one can
            // never be taken for its dashes; a stop heavier than a limit.
            line: priced.then(|| order_weight(order.kind)),
            dim: order.dim,
            closable: priced,
            draggable: true,
            tabs: Vec::new(),
            link: order.link.map(|link| MarkLink { dashed: link.holder == Holder::App, pending: link.pending }),
            raised: false,
        });
    }
    let held_mark = |held: &PositionMark, id: String, chip: bool, raised: bool| PriceMark {
        id,
        price: held.price,
        tag: held.tag.clone(),
        body: held.body.clone(),
        chip: chip.then(|| AxisChip { text: held.price_text.clone(), filled: held.body.is_some() }),
        colour: held.colour.clone(),
        style: held.style,
        line: Some(POSITION_WEIGHT),
        dim: held.dim,
        closable: true,
        draggable: false,
        tabs: held.exits.iter().map(|tab| exit_tab(tab, held.net)).collect(),
        link: None,
        raised,
    };
    for held in held {
        marks.push(held_mark(held, format!("{HELD}{}", held.contract), true, false));
    }
    // The position has no chip: the last price's beside it says enough.
    if let Some(held) = position {
        marks.push(held_mark(held, POSITION.to_string(), false, true));
    }
    marks
}

/// A tab to drag an exit out of a position of `net`: its outline as much
/// heavier than a target's as its order's line is, its label in hand the
/// exit's tag.
fn exit_tab(tab: &ExitTab, net: i32) -> MarkTab {
    let side = trading::exit_side(net);
    MarkTab {
        id: tab_id(tab.kind).to_string(),
        text: trading::exit_tab(tab.kind).to_string(),
        tag: trading::mark_tag(tab.kind, side, true, &net.unsigned_abs().to_string()),
        colour: tab.colour.clone(),
        style: tab.style,
        outline: order_weight(tab.kind) / order_weight(OrderKind::Limit),
        weight: order_weight(tab.kind),
    }
}

/// What the hand did, as the session hears it.
pub fn trade_event(event: MarkEvent) -> Option<TradeEvent> {
    match event {
        MarkEvent::Close(id) if id == POSITION => Some(TradeEvent::Flatten),
        MarkEvent::Close(id) => match (id.strip_prefix(HELD), id.strip_prefix(ORDER)) {
            (Some(contract), _) => Some(TradeEvent::FlattenHeld(contract.to_string())),
            (_, Some(basket)) => Some(TradeEvent::Cancel(basket.to_string())),
            _ => None,
        },
        MarkEvent::Move { mark, price } => {
            Some(TradeEvent::Move { basket: mark.strip_prefix(ORDER)?.to_string(), price })
        }
        MarkEvent::Tab { mark, tab, price } if mark == POSITION => Some(TradeEvent::ExitAt { kind: tab_kind(&tab)?, price }),
        MarkEvent::Tab { .. } => None,
    }
}

/// The event that says `event` to the chart, for tests that let go of a
/// mark as a hand would have.
#[cfg(test)]
pub fn mark_event(event: TradeEvent) -> MarkEvent {
    match event {
        TradeEvent::ExitAt { kind, price } => {
            MarkEvent::Tab { mark: POSITION.into(), tab: tab_id(kind).into(), price }
        }
        TradeEvent::Move { basket, price } => MarkEvent::Move { mark: format!("{ORDER}{basket}"), price },
        TradeEvent::Cancel(basket) => MarkEvent::Close(format!("{ORDER}{basket}")),
        TradeEvent::Flatten => MarkEvent::Close(POSITION.into()),
        TradeEvent::FlattenHeld(contract) => MarkEvent::Close(format!("{HELD}{contract}")),
    }
}

/// Where a mark in hand may go and what it says on the way: never past the
/// day's limits, an exit only to the side of the market that closes, an
/// order only where its kind rests; an exit saying what it would realise,
/// anything else its price and how far it has come.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TradeRules {
    position: Option<Held>,
    /// Each order's id on the chart, its side, kind and size.
    orders: Vec<(String, Side, OrderKind, u32)>,
    /// The day's lowest and highest prices the exchange takes an order at,
    /// as far as they are known.
    limits: (Option<f64>, Option<f64>),
}

/// What of the position a drag needs to say what an exit would realise.
#[derive(Clone, Debug, PartialEq)]
struct Held {
    price: f64,
    net: i32,
    point: Option<f64>,
    currency: Option<String>,
    profit: String,
    loss: String,
}

impl TradeRules {
    pub fn new(position: Option<&PositionMark>, orders: &[OrderMark], limits: (Option<f64>, Option<f64>)) -> TradeRules {
        TradeRules {
            position: position.map(|held| Held {
                price: held.price,
                net: held.net,
                point: held.point,
                currency: held.currency.clone(),
                profit: held.profit.clone(),
                loss: held.loss.clone(),
            }),
            orders: orders
                .iter()
                .map(|order| (format!("{ORDER}{}", order.basket), order.side, order.kind, order.qty))
                .collect(),
            limits,
        }
    }

    /// The kind of order a drag would make or move, and its side and size
    /// when it is one already working.
    fn dragged(&self, drag: &MarkDrag) -> Option<(OrderKind, Option<(Side, u32)>)> {
        match drag.tab {
            Some(tab) => Some((tab_kind(tab)?, None)),
            None => {
                let (_, side, kind, qty) = self.orders.iter().find(|(id, ..)| id == drag.mark)?;
                Some((*kind, Some((*side, *qty))))
            }
        }
    }

    /// What the exit in hand would realise at `drag.at`, and the position
    /// it closes: none for an order that is not an exit, or with no
    /// position or no worth of a point to count it in.
    fn pnl(&self, drag: &MarkDrag) -> Option<(f64, &Held)> {
        let held = self.position.as_ref()?;
        let point = held.point?;
        let qty = match self.dragged(drag)? {
            (_, None) => held.net.unsigned_abs(),
            (kind, Some((side, qty))) => {
                if !trading::is_exit(side, kind, held.net) {
                    return None;
                }
                qty
            }
        };
        Some((trading::exit_pnl(held.price, held.net, qty, point, drag.at), held))
    }
}

impl MarkRules for TradeRules {
    fn allowed(&self, drag: &MarkDrag) -> bool {
        let Some(last) = drag.last else { return false };
        if trading::past_limit(drag.at, self.limits).is_some() {
            return false;
        }
        match self.dragged(drag) {
            Some((kind, None)) => {
                let net = self.position.as_ref().map_or(0, |held| held.net);
                trading::exit_allowed(kind, net, drag.at, last)
            }
            Some((kind, Some((side, _)))) => trading::side_allowed(kind, side, drag.at, last),
            None => false,
        }
    }

    fn preview(&self, drag: &MarkDrag, allowed: bool) -> MarkPreview {
        if !allowed {
            let text = match trading::past_limit(drag.at, self.limits) {
                Some(limit) => trading::past_limit_preview(&grouped_decimals(limit, drag.places)),
                None => {
                    let kind = self.dragged(drag).map_or(OrderKind::Limit, |(kind, _)| kind);
                    trading::refused_text(kind, drag.at, drag.last.unwrap_or(drag.at))
                }
            };
            return MarkPreview { text, colour: None };
        }
        if let Some((pnl, held)) = self.pnl(drag) {
            let colour = if pnl < 0.0 { held.loss.clone() } else { held.profit.clone() };
            return MarkPreview { text: trading::pnl_text(pnl, held.currency.as_deref()), colour: Some(colour) };
        }
        let ticks = drag.tick.map(|tick| trading::ticks_between(drag.from, drag.at, tick));
        MarkPreview { text: trading::preview_text(&grouped_decimals(drag.at, drag.places), ticks), colour: None }
    }
}

/// The marks on a chart as the window's tests read them back.
#[cfg(test)]
pub trait MarksSeen {
    /// The position's words, and each order's.
    fn marks(&self) -> (Option<String>, Vec<String>);
    /// How each order drawn is linked, if it is.
    fn mark_links(&self) -> Vec<Option<LinkMark>>;
    /// The position's words and whether it is faint, then each order's
    /// words, price and whether it is faint.
    #[allow(clippy::type_complexity)]
    fn mark_states(&self) -> (Option<(String, bool)>, Vec<(String, f64, bool)>);
    /// The positions held in other months of the root: each one's words
    /// and whether it is faint.
    fn held_elsewhere(&self) -> Vec<(String, bool)>;
    /// The exit tabs the position wears.
    fn exit_tabs(&self) -> Vec<OrderKind>;
    /// Let go of a mark as a hand would have.
    fn drop_mark(&self, event: TradeEvent);
}

#[cfg(test)]
impl MarksSeen for omacharts_chart::ChartView {
    fn marks(&self) -> (Option<String>, Vec<String>) {
        let (position, orders) = self.mark_states();
        (position.map(|(words, _)| words), orders.into_iter().map(|(words, ..)| words).collect())
    }

    fn mark_links(&self) -> Vec<Option<LinkMark>> {
        let link = |link: MarkLink| LinkMark {
            holder: if link.dashed { Holder::App } else { Holder::Broker },
            pending: link.pending,
        };
        self.price_marks().iter().filter(|mark| mark.id.starts_with(ORDER)).map(|mark| mark.link.map(link)).collect()
    }

    fn mark_states(&self) -> (Option<(String, bool)>, Vec<(String, f64, bool)>) {
        let marks = self.price_marks();
        let position = marks.iter().find(|mark| mark.id == POSITION).map(|mark| (mark.words(), mark.dim));
        let orders = marks
            .iter()
            .filter(|mark| mark.id.starts_with(ORDER))
            .map(|mark| (mark.words(), mark.price, mark.dim))
            .collect();
        (position, orders)
    }

    fn held_elsewhere(&self) -> Vec<(String, bool)> {
        let marks = self.price_marks();
        marks.iter().filter(|mark| mark.id.starts_with(HELD)).map(|mark| (mark.words(), mark.dim)).collect()
    }

    fn exit_tabs(&self) -> Vec<OrderKind> {
        let marks = self.price_marks();
        let position = marks.iter().find(|mark| mark.id == POSITION);
        position.map(|mark| mark.tabs.iter().filter_map(|tab| tab_kind(&tab.id)).collect()).unwrap_or_default()
    }

    fn drop_mark(&self, event: TradeEvent) {
        self.tell_mark(mark_event(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long 1 ES at 5,000 at $50 a point, and a working sell limit for it.
    fn long() -> (PositionMark, Vec<OrderMark>) {
        let position = PositionMark {
            contract: "ESZ6".into(),
            price: 5000.0,
            tag: "▲ 1".into(),
            body: Some("+$125.00".into()),
            price_text: "5,000.00".into(),
            colour: "#00ff00".into(),
            style: LineStyle::Solid,
            net: 1,
            exits: vec![
                ExitTab { kind: OrderKind::Limit, colour: "#00aa00".into(), style: LineStyle::Dashed },
                ExitTab { kind: OrderKind::Stop, colour: "#aa0000".into(), style: LineStyle::Dashed },
            ],
            dim: false,
            point: Some(50.0),
            currency: Some("USD".into()),
            profit: "#00aa00".into(),
            loss: "#aa0000".into(),
        };
        let order = OrderMark {
            basket: "B1".into(),
            side: Side::Sell,
            kind: OrderKind::Limit,
            qty: 1,
            price: 5010.0,
            tag: "L 1".into(),
            body: None,
            price_text: "5,010.00".into(),
            colour: "#ff0000".into(),
            style: LineStyle::Dashed,
            dim: false,
            link: None,
        };
        (position, vec![order])
    }

    /// A drag of the position's `tab`, or of the order `B1` with none, to
    /// `at`, the market at 5,005.
    fn drag(tab: Option<&str>, at: f64) -> MarkDrag<'_> {
        let mark = if tab.is_some() { POSITION } else { "order:B1" };
        MarkDrag { mark, tab, from: 5000.0, at, last: Some(5005.0), tick: Some(0.25), places: 2 }
    }

    fn said(rules: &TradeRules, tab: Option<&str>, at: f64) -> (String, Option<String>) {
        let drag = drag(tab, at);
        let preview = rules.preview(&drag, rules.allowed(&drag));
        (preview.text, preview.colour)
    }

    /// An exit in hand says what it would realise, a part of an order
    /// larger than the position counting only the position; an order that
    /// adds to the position what it is and how far it has come; and where
    /// it may not go, why.
    #[test]
    fn a_dragged_exit_says_what_it_would_realise() {
        let (mut position, mut orders) = long();
        let rules = TradeRules::new(Some(&position), &orders, (None, None));
        let profit = Some("#00aa00".to_string());
        let loss = Some("#aa0000".to_string());
        // Long 1 ES: the target 10 points up, the stop 2 down.
        assert_eq!(said(&rules, Some("limit"), 5010.0), ("+$500.00".into(), profit.clone()));
        assert_eq!(said(&rules, Some("stop"), 4998.0), ("-$100.00".into(), loss.clone()));
        // The working sell limit is an exit too; one for 2 closes only the 1.
        orders[0].qty = 2;
        let rules = TradeRules::new(Some(&position), &orders, (None, None));
        assert_eq!(said(&rules, None, 5006.0), ("+$300.00".into(), profit.clone()));
        // Refused: why, not money.
        assert_eq!(said(&rules, Some("stop"), 5010.0), ("Stop above market · not allowed".into(), None));

        // Short 2 NQ at $20 a point: the target 10 down, the stop 3 up, and
        // a buy stop for 1 of the 2.
        position.net = -2;
        position.point = Some(20.0);
        orders[0].side = Side::Buy;
        orders[0].kind = OrderKind::Stop;
        orders[0].qty = 1;
        let rules = TradeRules::new(Some(&position), &orders, (None, None));
        assert_eq!(said(&rules, Some("limit"), 4990.0), ("+$400.00".into(), profit));
        assert_eq!(said(&rules, Some("stop"), 5006.0), ("-$240.00".into(), loss.clone()));
        assert_eq!(said(&rules, None, 5006.0), ("-$120.00".into(), loss));

        // A sell adds to a short: no money, where it is and how far it has
        // come.
        orders[0].side = Side::Sell;
        orders[0].kind = OrderKind::Limit;
        let rules = TradeRules::new(Some(&position), &orders, (None, None));
        assert_eq!(said(&rules, None, 5006.0), ("5,006.00 · +24 ticks".into(), None));
        // With nothing held there is no exit to count.
        let rules = TradeRules::new(None, &orders, (None, None));
        assert_eq!(said(&rules, None, 5006.0), ("5,006.00 · +24 ticks".into(), None));
        // In another currency, written after the money.
        position.currency = Some("EUR".into());
        let rules = TradeRules::new(Some(&position), &orders, (None, None));
        assert_eq!(said(&rules, Some("limit"), 4990.0).0, "+400.00 EUR");
    }

    /// What an order in hand may do: an exit goes only on its own side of
    /// the market, a moved order keeps its own side and kind, and nothing
    /// goes past the day's limits, whose preview says so. With no market
    /// there is no side of it to go to.
    #[test]
    fn an_order_in_hand_goes_only_where_it_may() {
        let (position, orders) = long();
        let rules = TradeRules::new(Some(&position), &orders, (None, None));
        let allowed = |rules: &TradeRules, tab, at| rules.allowed(&drag(tab, at));
        assert!(allowed(&rules, Some("stop"), 5000.0), "a long's stop below the market");
        assert!(!allowed(&rules, Some("stop"), 5010.0));
        assert!(allowed(&rules, Some("limit"), 5010.0));
        assert!(!allowed(&rules, Some("limit"), 5000.0));
        assert!(allowed(&rules, None, 5010.0), "a sell limit stays above");
        assert!(!allowed(&rules, None, 5000.0));
        let stray = MarkDrag { mark: "order:B9", ..drag(None, 5010.0) };
        assert!(!rules.allowed(&stray), "no such order");
        let blind = MarkDrag { last: None, ..drag(Some("limit"), 5010.0) };
        assert!(!rules.allowed(&blind), "no market");

        let rules = TradeRules::new(Some(&position), &orders, (Some(4995.0), Some(5015.0)));
        assert!(allowed(&rules, Some("limit"), 5010.0), "a target inside the limits");
        assert!(!allowed(&rules, Some("limit"), 5020.0), "past the high limit");
        assert!(allowed(&rules, Some("stop"), 5000.0));
        assert!(!allowed(&rules, Some("stop"), 4990.0), "past the low limit");
        assert_eq!(said(&rules, Some("limit"), 5020.0), ("Past daily limit 5,015.00 · not allowed".into(), None));
    }

    /// Only the position's line is solid: a limit, a stop, a target and a
    /// stop loss are dashed, and a stop heavier than a limit, its tab's
    /// outline as much heavier.
    #[test]
    fn only_the_positions_line_is_solid() {
        for theme in omacharts_engine::theme::builtin_themes() {
            let look = omacharts_engine::theme_trading(&theme, Default::default());
            assert_eq!(look.limit, LineStyle::Dashed, "{}: a limit, a target among them", theme.name);
            assert_eq!(look.stop, LineStyle::Dashed, "{}: a stop, a stop loss among them", theme.name);
            assert_eq!(look.position, LineStyle::Solid, "{}", theme.name);
        }
        assert!(order_weight(OrderKind::Stop) > order_weight(OrderKind::Limit));
        assert!(order_weight(OrderKind::Limit) > 1.0, "heavier than the last price's hairline");
        let (position, _) = long();
        let marks = price_marks(Some(&position), &[], &[]);
        let outlines: Vec<_> = marks[0].tabs.iter().map(|tab| (tab.id.as_str(), tab.outline)).collect();
        assert_eq!(outlines, vec![("limit", 1.0), ("stop", 2.0 / 1.5)]);
    }

    /// The chart is handed the orders, then the positions in other months,
    /// then the position over the line joining the exits; a market order
    /// on its way with no line, ×, or chip; the position with no chip.
    #[test]
    fn the_marks_are_handed_over_in_the_order_they_are_drawn() {
        let (position, mut orders) = long();
        let mut market = orders[0].clone();
        market.basket = "B2".into();
        market.kind = OrderKind::Market;
        orders.push(market);
        let mut held = position.clone();
        held.contract = "ESH7".into();
        let marks = price_marks(Some(&position), &[held], &orders);
        let ids: Vec<_> = marks.iter().map(|mark| mark.id.as_str()).collect();
        assert_eq!(ids, ["order:B1", "order:B2", "held:ESH7", "position"]);
        let shape = |mark: &PriceMark| (mark.line.is_some(), mark.closable, mark.chip.is_some(), mark.raised);
        assert_eq!(shape(&marks[0]), (true, true, true, false));
        assert_eq!(shape(&marks[1]), (false, false, false, false));
        assert_eq!(shape(&marks[2]), (true, true, true, false));
        assert_eq!(shape(&marks[3]), (true, true, false, true));
        assert!(marks[0].draggable && !marks[3].draggable);
    }

    /// Every event the session hears is said back by the chart as itself,
    /// and a tab or a move on a mark that is no order says nothing.
    #[test]
    fn what_the_hand_did_is_heard_as_the_session_hears_it() {
        let events = [
            TradeEvent::ExitAt { kind: OrderKind::Stop, price: 4990.0 },
            TradeEvent::Move { basket: "B1".into(), price: 5010.0 },
            TradeEvent::Cancel("B1".into()),
            TradeEvent::Flatten,
            TradeEvent::FlattenHeld("ESH7".into()),
        ];
        for event in events {
            assert_eq!(trade_event(mark_event(event.clone())), Some(event));
        }
        assert_eq!(trade_event(MarkEvent::Move { mark: POSITION.into(), price: 1.0 }), None);
        assert_eq!(trade_event(MarkEvent::Tab { mark: "held:ESH7".into(), tab: "stop".into(), price: 1.0 }), None);
        assert_eq!(trade_event(MarkEvent::Close("elsewhere".into())), None);
    }
}
