//! Linking: a position's stop and target that never both outlive it.
//!
//! Every broker gets the same feature, asked for the same way — "link this
//! exit with the other" — and the foundation does it with whatever the
//! broker has, which [`Linking`] says:
//!
//! - **After the fact** (Rithmic): orders already working are linked where
//!   they stand. A new exit is placed, then linked with the other.
//! - **At submission only** (Sierra Chart over DTC): two orders are one-
//!   cancels-other only when they are submitted together, as one OCO pair.
//!   A bracket goes as one pair; a second exit added to one already working
//!   is a question of order, below.
//! - **Not at all**: the app holds the link itself, which only works while
//!   it runs.
//!
//! The window and the rules ask [`Plan::exit`] or [`Plan::bracket`] for a
//! plan, draw what it will place, cancel and link as pending, and hand
//! [`Plan::run`] to the session's thread. They never learn which of these
//! it was, beyond who holds the link ([`Holder`]), which they show.
//!
//! # A second exit, on a broker that links only at submission
//!
//! The position holds one exit, working alone, and the trader adds the
//! other. The pair can only be formed by submitting both anew, so the one
//! working has to be replaced, and two rules hold the order of the steps:
//!
//! 1. **Never unprotected**: a position that has a stop is never without
//!    one, not even for a round trip.
//! 2. **Never doubly protected in a way that could overfill**: never two
//!    working exits that one price touches at once. Two stops (or two
//!    targets) for the whole position, at the same price, both fill on the
//!    same tick and turn a long into a short of the same size.
//!
//! Neither reduce-only nor a position-sized quantity rescues an overlap:
//! DTC has no reduce-only (`OpenOrClose` "is not used for futures"), and
//! each of two overlapping stops has to be for the whole position to
//! protect it, so together they are for twice it. CANCEL_REPLACE_ORDER
//! changes a price, a quantity or a time in force, never a link, and a
//! pair's parent (`ParentTriggerClientOrderID`) is an entry that opens a
//! position, not a working exit. Sierra Chart's own "attached orders by
//! position" is its trade window's, not its DTC server's.
//!
//! So it depends on which exit is working:
//!
//! - **A target working, a stop added** — [`Plan::Reform`]: cancel the
//!   target, and once the broker says it is cancelled, submit the new stop
//!   and the target again as one pair. Between the two the position has no
//!   target, and it had no stop before: it is exposed to nothing it was not
//!   already exposed to, and at most misses a fill at the target for one
//!   round trip. Never two targets: the second is submitted only once the
//!   first is gone. The pair is sized to what is still held then — the
//!   target may have filled in part meanwhile — and a part of either
//!   filled later comes off the other (DTC's `PartialFillHandling`), so the
//!   two never close more than is held. If the pair is refused, the target
//!   goes back in alone, as it was, and the refusal is said. If the cancel
//!   is not answered, nothing is placed: the target may still be working.
//! - **A stop working, a target added** — no safe order exists. Cancelling
//!   the stop first leaves the position unprotected for a round trip, and
//!   is worse if the pair is then refused; submitting the pair first has
//!   two stops working at one price until the old one is cancelled. So the
//!   foundation does neither: it places the target alone and holds the
//!   link itself ([`Plan::PlaceThenWatch`]), as for a broker that cannot
//!   link at all, and says so. Placing the target first, or both at once,
//!   gets a link the broker holds.
//!
//! # The app's own link
//!
//! For a broker that cannot link, and for the case above, the app watches
//! the two: when one fills or is cancelled, it cancels the other
//! ([`due`], run by the desk on every change of the book). It belongs here
//! rather than nowhere because two exits left unlinked are worse — the one
//! left behind opens a position of its own when it is touched — and the
//! trader asked for the link. Its limits, which the window shows by
//! drawing such a link differently and saying "while Omacharts runs":
//!
//! - It holds only while the app runs and the session is signed in. Quit,
//!   and the two are unlinked at the broker.
//! - It acts a round trip after the broker says one filled, so a market
//!   that runs from one exit to the other within that time fills both.
//! - A part of one filled is not taken off the other: the other is
//!   cancelled only when the first is done.

use super::*;

/// How a broker can make a stop and a target one-cancels-other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linking {
    /// Orders already working are linked where they stand, held by the
    /// broker: Rithmic.
    AfterTheFact,
    /// Only two orders submitted together, as one OCO pair, held by the
    /// broker: Sierra Chart over DTC.
    AtSubmission,
    /// Not at all: the app holds the link, while it runs.
    None,
}

/// Who holds a link, which decides whether it outlives the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// The broker: it holds whether or not the app runs.
    Broker,
    /// The app: only while it runs and the session is signed in.
    App,
}

/// A link made between working orders of one account.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The account's key on the desk; its number inside a session.
    pub account: String,
    /// The orders linked, by basket.
    pub baskets: Vec<String>,
    pub holder: Holder,
}

/// A link asked for and not yet made, drawn as pending from the moment it
/// is asked: its new orders by tag, the working one it joins by basket.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    /// The account's key on the desk.
    pub account: String,
    pub tags: Vec<String>,
    pub baskets: Vec<String>,
    pub holder: Holder,
}

impl Asked {
    /// Is the order with `basket` and `tag` one of it?
    pub fn has(&self, basket: &str, tag: Option<&str>) -> bool {
        (!basket.is_empty() && self.baskets.iter().any(|known| known == basket))
            || tag.is_some_and(|tag| self.tags.iter().any(|known| known == tag))
    }
}

/// How exits go in, linked: what [`Plan::run`] does on the session's
/// thread.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Nothing to link with: placed alone.
    Alone(OrderRequest),
    /// Placed, then the broker links them with the working order `with`,
    /// when there is one.
    PlaceThenLink { place: Vec<OrderRequest>, with: Option<String> },
    /// Placed together, as one pair the broker links.
    Pair([OrderRequest; 2]),
    /// The working target `cancel` cancelled, then the new stop and the
    /// target again placed as one pair, sized to what is held then.
    Reform { cancel: Box<Order>, pair: [OrderRequest; 2] },
    /// Placed, and the app holds the link with `with`, when there is one.
    PlaceThenWatch { place: Vec<OrderRequest>, with: Option<String> },
}

impl Plan {
    /// A new exit, `exit`, on a broker that links as `linking`, with
    /// `sibling` the other exit working on the position, if any.
    pub fn exit(linking: Linking, exit: OrderRequest, sibling: Option<&Order>) -> Plan {
        let Some(sibling) = sibling else { return Plan::Alone(exit) };
        let with = Some(sibling.basket_id.clone());
        match linking {
            Linking::AfterTheFact => Plan::PlaceThenLink { place: vec![exit], with },
            // The working one a target: cancel it, then the pair.
            Linking::AtSubmission if sibling.kind == OrderKind::Limit && exit.kind == OrderKind::Stop => {
                let again = OrderRequest {
                    account: exit.account.clone(),
                    contract: sibling.contract.clone(),
                    exchange: sibling.exchange.clone(),
                    side: sibling.side,
                    qty: exit.qty,
                    kind: sibling.kind,
                    price: sibling.price,
                    tag: format!("{}:again", exit.tag),
                };
                Plan::Reform { cancel: Box::new(sibling.clone()), pair: [exit, again] }
            }
            // The working one a stop: no order of steps is safe.
            Linking::AtSubmission | Linking::None => Plan::PlaceThenWatch { place: vec![exit], with },
        }
    }

    /// A stop and a target for a position, placed together.
    pub fn bracket(linking: Linking, stop: OrderRequest, target: OrderRequest) -> Plan {
        match linking {
            Linking::AfterTheFact => Plan::PlaceThenLink { place: vec![stop, target], with: None },
            Linking::AtSubmission => Plan::Pair([stop, target]),
            Linking::None => Plan::PlaceThenWatch { place: vec![stop, target], with: None },
        }
    }

    /// Every order it places, to draw as pending from the moment it is
    /// asked.
    pub fn placing(&self) -> Vec<&OrderRequest> {
        match self {
            Plan::Alone(order) => vec![order],
            Plan::PlaceThenLink { place, .. } | Plan::PlaceThenWatch { place, .. } => place.iter().collect(),
            Plan::Pair(pair) | Plan::Reform { pair, .. } => pair.iter().collect(),
        }
    }

    /// The working order it cancels first, by basket.
    pub fn cancelling(&self) -> Option<&str> {
        match self {
            Plan::Reform { cancel, .. } => Some(&cancel.basket_id),
            _ => None,
        }
    }

    /// The working order the new ones link with, by basket.
    pub fn with(&self) -> Option<&str> {
        match self {
            Plan::PlaceThenLink { with, .. } | Plan::PlaceThenWatch { with, .. } => with.as_deref(),
            _ => None,
        }
    }

    /// Who will hold the link it makes, when it makes one.
    pub fn holder(&self) -> Option<Holder> {
        match self {
            Plan::Alone(_) => None,
            Plan::PlaceThenLink { place, with } if place.len() + usize::from(with.is_some()) >= 2 => Some(Holder::Broker),
            Plan::PlaceThenWatch { place, with } if place.len() + usize::from(with.is_some()) >= 2 => Some(Holder::App),
            Plan::Pair(_) | Plan::Reform { .. } => Some(Holder::Broker),
            _ => None,
        }
    }

    /// The link it will make, as it is drawn while pending, on the account
    /// with the key `account`; `None` when it makes none.
    pub fn asked(&self, account: &str) -> Option<Asked> {
        let holder = self.holder()?;
        Some(Asked {
            account: account.to_string(),
            tags: self.placing().iter().map(|order| order.tag.clone()).collect(),
            baskets: self.with().map(str::to_string).into_iter().collect(),
            holder,
        })
    }

    /// Do it, on the session's thread, for the account numbered `account`:
    /// every call blocks until the broker has answered.
    pub fn run(&self, session: &dyn Session, account: &str) -> Ran {
        match self {
            Plan::Alone(order) => Ran { placed: vec![(order.clone(), session.place(order))], ..Ran::default() },
            Plan::PlaceThenLink { place, with } => place_then(session, account, place, with.as_deref(), Holder::Broker),
            Plan::PlaceThenWatch { place, with } => place_then(session, account, place, with.as_deref(), Holder::App),
            Plan::Pair(pair) => pair_placed(session, account, pair.clone()),
            Plan::Reform { cancel, pair } => reform(session, account, cancel, pair),
        }
    }
}

/// What came of a [`Plan`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ran {
    /// Each order it was to place, as it went: its basket, or why not.
    pub placed: Vec<(OrderRequest, Result<String, Failure>)>,
    /// The cancel it made first, when it made one.
    pub cancelled: Option<Result<(), Failure>>,
    /// The link made, or why there is none; `None` when there was nothing
    /// to link, or nothing was placed to link.
    pub link: Option<Result<Link, Failure>>,
    /// A target cancelled to form a pair the broker then refused, placed
    /// back alone as it was.
    pub restored: Option<(OrderRequest, Result<String, Failure>)>,
}

impl Ran {
    /// The baskets placed, in order.
    fn baskets(&self) -> Vec<String> {
        self.placed.iter().filter_map(|(_, placed)| placed.as_ref().ok().cloned()).collect()
    }
}

/// Is the order `basket` still working, as the session's book says?
fn working(session: &dyn Session, basket: &str) -> bool {
    session.book().order(basket).is_some_and(|order| order.status.working())
}

/// Each of `place` placed in turn, then linked with `with` by `holder`:
/// the broker's link, or the app's.
fn place_then(session: &dyn Session, account: &str, place: &[OrderRequest], with: Option<&str>, holder: Holder) -> Ran {
    let mut ran = Ran::default();
    for order in place {
        let placed = session.place(order);
        let refused = placed.is_err();
        ran.placed.push((order.clone(), placed));
        // A bracket's first refused: the second is not sent alone.
        if refused {
            return ran;
        }
    }
    let mut baskets = ran.baskets();
    // The other exit may have filled or gone while this was placed: then
    // there is nothing left to link with.
    if let Some(with) = with.filter(|with| working(session, with)) {
        baskets.push(with.to_string());
    }
    if baskets.len() < 2 {
        return ran;
    }
    let link = Link { account: account.to_string(), baskets, holder };
    ran.link = Some(match holder {
        Holder::Broker => session.link(account, &link.baskets).map(|()| link),
        Holder::App => Ok(link),
    });
    ran
}

/// `pair` placed as one, the broker holding the link.
fn pair_placed(session: &dyn Session, account: &str, pair: [OrderRequest; 2]) -> Ran {
    let mut ran = Ran::default();
    match session.place_pair(&pair) {
        Ok(baskets) => {
            ran.link = Some(Ok(Link { account: account.to_string(), baskets: baskets.to_vec(), holder: Holder::Broker }));
            ran.placed = pair.into_iter().zip(baskets).map(|(order, basket)| (order, Ok(basket))).collect();
        }
        // Refused, though one of the two may be working all the same, if
        // the broker took only one: the book says which.
        Err(Failure::Refused(why)) => {
            let book = session.book();
            ran.placed = pair
                .into_iter()
                .map(|order| {
                    let taken = book
                        .orders
                        .iter()
                        .find(|known| known.tag.as_deref() == Some(order.tag.as_str()) && known.status.working());
                    let placed = taken.map(|known| known.basket_id.clone()).ok_or_else(|| Failure::Refused(why.clone()));
                    (order, placed)
                })
                .collect();
            if ran.placed.iter().any(|(_, placed)| placed.is_ok()) {
                ran.link = Some(Err(Failure::Refused(format!("only one of the pair was taken: {why}"))));
            }
        }
        // It may well have arrived: nothing is assumed either way.
        Err(failure) => ran.placed = pair.into_iter().map(|order| (order, Err(failure.clone()))).collect(),
    }
    ran
}

/// What is still to close of a position of `planned` once the target
/// `cancelled` is cancelled, as the session's book says: less what the
/// target filled since it was planned, and never more than is held.
fn left_to_close(session: &dyn Session, account: &str, cancelled: &Order, planned: u32) -> u32 {
    let book = session.book();
    let filled = book.order(&cancelled.basket_id).map_or(cancelled.filled, |order| order.filled);
    let left = planned.saturating_sub(filled.saturating_sub(cancelled.filled));
    match book.position(account, &cancelled.contract) {
        // Held on the side the target closes: no more than that.
        Some(held) if exit_side(held.net) == cancelled.side => left.min(held.net.unsigned_abs()),
        // Held the other way: these exits would open a position.
        Some(_) => 0,
        // Not told yet: what the fills say.
        None => left,
    }
}

/// Cancel the working target, then place the new stop and the target again
/// as one pair: see the module's notes for why in this order.
fn reform(session: &dyn Session, account: &str, cancel: &Order, pair: &[OrderRequest; 2]) -> Ran {
    let mut ran = Ran::default();
    let cancelled = session.cancel(account, &cancel.basket_id);
    let failed = cancelled.clone().err();
    ran.cancelled = Some(cancelled);
    if let Some(why) = failed {
        // Not cancelled, or no answer: the target may still be working, and
        // a second one would overfill. Nothing is placed.
        let why = match why {
            Failure::Refused(said) => Failure::Refused(format!("the target could not be replaced: {said}")),
            unanswered => unanswered,
        };
        ran.placed = pair.iter().map(|order| (order.clone(), Err(why.clone()))).collect();
        return ran;
    }
    let left = left_to_close(session, account, cancel, pair[0].qty);
    if left == 0 {
        let why = Failure::Refused("nothing left to close: the target filled meanwhile".into());
        ran.placed = pair.iter().map(|order| (order.clone(), Err(why.clone()))).collect();
        return ran;
    }
    let sized = pair.clone().map(|order| OrderRequest { qty: left, ..order });
    let target = sized[1].clone();
    ran = Ran { cancelled: ran.cancelled, ..pair_placed(session, account, sized) };
    // Refused whole: the target back in alone, as it was before.
    if ran.placed.iter().all(|(_, placed)| matches!(placed, Err(Failure::Refused(_)))) {
        let again = OrderRequest { tag: new_tag(), ..target };
        let placed = session.place(&again);
        ran.restored = Some((again, placed));
    }
    ran
}

/// What the app does about a link it holds, as the book now stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Due {
    /// Every order of it still working, or not heard of yet: watch on.
    Keep,
    /// One of it filled or went: cancel these, the rest still working,
    /// and forget it.
    Cancel(Vec<String>),
    /// None of it working any more: forget it.
    Drop,
}

/// What `link`, held by the app, needs now that `book` stands so. An order
/// the book has not heard of — on its way, or not refreshed after a drop —
/// is taken as still working. A part filled is not acted on: the other is
/// cancelled once the first is done.
pub fn due(link: &Link, book: &Book) -> Due {
    let orders: Vec<Option<&Order>> = link.baskets.iter().map(|basket| book.order(basket)).collect();
    let working = |order: &Option<&Order>| order.is_none_or(|order| order.status.working());
    if orders.iter().all(working) {
        return Due::Keep;
    }
    let rest: Vec<String> = link
        .baskets
        .iter()
        .zip(&orders)
        .filter(|(_, order)| order.is_some_and(|order| order.status.working()))
        .map(|(basket, _)| basket.clone())
        .collect();
    match rest.is_empty() {
        true => Due::Drop,
        false => Due::Cancel(rest),
    }
}

/// Is a link the broker holds still standing in `book`? Only while every
/// order of it is working; once one is done the broker has dealt with the
/// rest.
pub fn standing(link: &Link, book: &Book) -> bool {
    link.baskets.iter().all(|basket| book.order(basket).is_none_or(|order| order.status.working()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::FakeSession;
    use std::sync::atomic::Ordering;

    fn session(linking: Linking) -> FakeSession {
        let mut session = FakeSession::new(Vec::new(), Box::new(|| {}));
        session.linking = linking;
        session
    }

    fn request(side: Side, kind: OrderKind, qty: u32, price: Option<f64>) -> OrderRequest {
        OrderRequest {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side,
            qty,
            kind,
            price,
            tag: new_tag(),
        }
    }

    /// Long `qty` ES at 7,600 on account 951.
    fn long(session: &FakeSession, qty: u32) {
        session.place(&request(Side::Buy, OrderKind::Market, qty, None)).unwrap();
    }

    fn stop(qty: u32) -> OrderRequest {
        request(Side::Sell, OrderKind::Stop, qty, Some(7_590.0))
    }

    fn target(qty: u32) -> OrderRequest {
        request(Side::Sell, OrderKind::Limit, qty, Some(7_640.0))
    }

    /// Place `order` alone, as the first exit, and the order it is.
    fn first(session: &FakeSession, order: &OrderRequest) -> Order {
        let basket = session.place(order).unwrap();
        session.book().order(&basket).unwrap().clone()
    }

    fn working(session: &FakeSession) -> Vec<Order> {
        session.book().orders.into_iter().filter(|order| order.status.working()).collect()
    }

    /// Each broker links the way it can, and the plan says who holds it
    /// before anything is sent.
    #[test]
    fn every_way_of_linking_has_its_plan() {
        let sibling = |kind| Order {
            account: "951".into(),
            basket_id: "S".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty: 1,
            filled: 0,
            kind,
            price: Some(7_600.0),
            status: Stage::Working.into(),
            fill_price: None,
            fills: Vec::new(),
            at: 0,
            tag: None,
            tick: Some(0.25),
        };
        let (stop_working, target_working) = (sibling(OrderKind::Stop), sibling(OrderKind::Limit));
        for linking in [Linking::AfterTheFact, Linking::AtSubmission, Linking::None] {
            assert!(matches!(Plan::exit(linking, stop(1), None), Plan::Alone(_)), "nothing to link with");
        }
        let plan = Plan::exit(Linking::AfterTheFact, target(1), Some(&stop_working));
        assert!(matches!(plan, Plan::PlaceThenLink { .. }));
        assert_eq!((plan.holder(), plan.with(), plan.cancelling()), (Some(Holder::Broker), Some("S"), None));

        let plan = Plan::exit(Linking::AtSubmission, stop(1), Some(&target_working));
        let Plan::Reform { pair, .. } = &plan else { panic!("{plan:?}") };
        assert_eq!((pair[0].kind, pair[1].kind, pair[1].price), (OrderKind::Stop, OrderKind::Limit, Some(7_600.0)));
        assert_ne!(pair[0].tag, pair[1].tag, "each of the pair is known by its own tag");
        assert_eq!((plan.holder(), plan.cancelling(), plan.placing().len()), (Some(Holder::Broker), Some("S"), 2));

        // A stop working and a target added: no safe order of steps, so the
        // app holds the link.
        let plan = Plan::exit(Linking::AtSubmission, target(1), Some(&stop_working));
        assert!(matches!(plan, Plan::PlaceThenWatch { .. }), "{plan:?}");
        assert_eq!((plan.holder(), plan.cancelling()), (Some(Holder::App), None));
        let plan = Plan::exit(Linking::None, stop(1), Some(&target_working));
        assert_eq!(plan.holder(), Some(Holder::App));

        assert!(matches!(Plan::bracket(Linking::AtSubmission, stop(1), target(1)), Plan::Pair(_)));
        assert_eq!(Plan::bracket(Linking::AfterTheFact, stop(1), target(1)).holder(), Some(Holder::Broker));
        assert_eq!(Plan::bracket(Linking::None, stop(1), target(1)).holder(), Some(Holder::App));
    }

    /// Both exits at once: one pair where the broker links only so, placed
    /// and linked after where it links working orders, placed and watched
    /// where it cannot link.
    #[test]
    fn a_bracket_is_linked_however_the_broker_can() {
        for linking in [Linking::AfterTheFact, Linking::AtSubmission, Linking::None] {
            let session = session(linking);
            long(&session, 2);
            let ran = Plan::bracket(linking, stop(2), target(2)).run(&session, "951");
            let baskets: Vec<String> = ran.placed.iter().map(|(_, placed)| placed.clone().unwrap()).collect();
            assert_eq!(baskets.len(), 2, "{linking:?}");
            let link = ran.link.clone().expect("linked").expect("linked");
            assert_eq!(link.baskets.len(), 2, "{linking:?}");
            match linking {
                Linking::AfterTheFact => {
                    assert_eq!(link.holder, Holder::Broker);
                    assert_eq!(*session.links.lock().unwrap(), vec![baskets.clone()]);
                    assert_eq!(session.pairs.load(Ordering::Acquire), 0);
                }
                Linking::AtSubmission => {
                    assert_eq!(link.holder, Holder::Broker);
                    assert_eq!(session.pairs.load(Ordering::Acquire), 1, "one submission");
                }
                Linking::None => {
                    assert_eq!(link.holder, Holder::App);
                    assert!(session.links.lock().unwrap().is_empty(), "nothing asked of a broker that cannot");
                }
            }
            // The target fills: the stop goes with it, by the broker's hand
            // or the app's.
            session.fill(&baskets[1]);
            let book = session.book();
            match link.holder {
                Holder::Broker => assert!(!book.order(&baskets[0]).unwrap().status.working(), "{linking:?}"),
                Holder::App => assert_eq!(due(&link, &book), Due::Cancel(vec![baskets[0].clone()])),
            }
        }
    }

    /// On a broker that links only at submission, a stop added to a working
    /// target cancels the target first and then places both as one pair:
    /// never two targets working, never fewer stops than before.
    #[test]
    fn a_stop_added_to_a_working_target_reforms_the_pair_after_the_target_is_cancelled() {
        let session = session(Linking::AtSubmission);
        long(&session, 1);
        let working_target = first(&session, &target(1));
        let plan = Plan::exit(Linking::AtSubmission, stop(1), Some(&working_target));
        let ran = plan.run(&session, "951");
        assert_eq!(ran.cancelled, Some(Ok(())));
        // The order the broker heard them in: the cancel, then the pair.
        let heard = session.heard.lock().unwrap().clone();
        assert_eq!(heard[heard.len() - 2..], ["cancel", "pair"], "{heard:?}");
        let now = working(&session);
        assert_eq!(now.len(), 2, "{now:?}");
        assert_eq!(now.iter().filter(|order| order.kind == OrderKind::Limit).count(), 1, "one target, never two");
        let link = ran.link.unwrap().unwrap();
        assert_eq!(link.holder, Holder::Broker);
        assert!(!session.book().order(&working_target.basket_id).unwrap().status.working());
        // Linked at the broker: the stop filling takes the target with it.
        let stop = now.iter().find(|order| order.kind == OrderKind::Stop).unwrap();
        session.fill(&stop.basket_id);
        assert!(working(&session).is_empty());
        assert!(session.book().position("951", "ESZ6").is_none(), "closed once, not twice");
    }

    /// The target filled in part before it was cancelled: the pair is for
    /// what is still held.
    #[test]
    fn a_reformed_pair_is_sized_to_what_is_still_held() {
        let session = session(Linking::AtSubmission);
        long(&session, 3);
        let working_target = first(&session, &target(3));
        let plan = Plan::exit(Linking::AtSubmission, stop(3), Some(&working_target));
        session.fill_part(&working_target.basket_id, 1);
        let ran = plan.run(&session, "951");
        assert!(ran.placed.iter().all(|(order, placed)| placed.is_ok() && order.qty == 2), "{ran:?}");
        assert!(working(&session).iter().all(|order| order.qty == 2));

        // Filled whole meanwhile: nothing left to close, nothing placed.
        let session = self::session(Linking::AtSubmission);
        long(&session, 1);
        let working_target = first(&session, &target(1));
        let plan = Plan::exit(Linking::AtSubmission, stop(1), Some(&working_target));
        session.fill(&working_target.basket_id);
        let ran = plan.run(&session, "951");
        assert!(ran.placed.iter().all(|(_, placed)| placed.is_err()));
        assert!(working(&session).is_empty(), "an exit placed for a position already closed");
    }

    /// The pair refused: the target goes back in alone, as it was. The
    /// cancel refused or unanswered: nothing is placed, since the target
    /// may still be working.
    #[test]
    fn a_reform_that_fails_leaves_the_position_as_it_was() {
        let session = session(Linking::AtSubmission);
        long(&session, 1);
        let working_target = first(&session, &target(1));
        let plan = Plan::exit(Linking::AtSubmission, stop(1), Some(&working_target));
        *session.refuse_pairs.lock().unwrap() = Some("Rejected at RMS".into());
        let ran = plan.run(&session, "951");
        assert!(ran.placed.iter().all(|(_, placed)| *placed == Err(Failure::Refused("Rejected at RMS".into()))));
        assert_eq!(ran.link, None);
        let (again, placed) = ran.restored.expect("the target placed again");
        assert!(placed.is_ok());
        assert_eq!((again.kind, again.price, again.qty), (OrderKind::Limit, Some(7_640.0), 1));
        let now = working(&session);
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].kind, OrderKind::Limit);

        let session = self::session(Linking::AtSubmission);
        long(&session, 1);
        let working_target = first(&session, &target(1));
        let plan = Plan::exit(Linking::AtSubmission, stop(1), Some(&working_target));
        session.silent.store(true, Ordering::Release);
        let ran = plan.run(&session, "951");
        assert!(matches!(ran.cancelled, Some(Err(Failure::Unanswered(_)))));
        assert!(ran.placed.iter().all(|(_, placed)| matches!(placed, Err(Failure::Unanswered(_)))));
        assert_eq!(session.heard.lock().unwrap().iter().filter(|heard| **heard == "pair").count(), 0, "a pair sent blind");
        assert!(ran.restored.is_none());
    }

    /// A target added to a working stop, where the broker links only at
    /// submission: placed alone, the stop never touched, and the app holds
    /// the link.
    #[test]
    fn a_target_added_to_a_working_stop_is_never_paired_by_touching_the_stop() {
        let session = session(Linking::AtSubmission);
        long(&session, 1);
        let working_stop = first(&session, &stop(1));
        let ran = Plan::exit(Linking::AtSubmission, target(1), Some(&working_stop)).run(&session, "951");
        assert_eq!(ran.cancelled, None);
        assert_eq!(*session.heard.lock().unwrap().last().unwrap(), "place");
        assert!(session.book().order(&working_stop.basket_id).unwrap().status.working(), "the stop left alone");
        let link = ran.link.unwrap().unwrap();
        assert_eq!(link.holder, Holder::App);
        assert_eq!(link.baskets[1], working_stop.basket_id);
        // The target fills: the app cancels the stop.
        session.fill(&link.baskets[0]);
        assert_eq!(due(&link, &session.book()), Due::Cancel(vec![working_stop.basket_id.clone()]));
    }

    /// Either exit may go first, on every broker: the second is placed and
    /// linked, by whoever can hold the link.
    #[test]
    fn either_exit_may_go_first_however_the_broker_links() {
        for linking in [Linking::AfterTheFact, Linking::AtSubmission, Linking::None] {
            for first_kind in [OrderKind::Stop, OrderKind::Limit] {
                let session = session(linking);
                long(&session, 1);
                let (one, two) = match first_kind {
                    OrderKind::Stop => (stop(1), target(1)),
                    _ => (target(1), stop(1)),
                };
                let working_one = first(&session, &one);
                let sibling = sibling_exit(&session.book().orders, "ESZ6", Side::Sell, two.kind).cloned();
                assert_eq!(sibling.as_ref().map(|o| o.basket_id.clone()), Some(working_one.basket_id.clone()));
                let ran = Plan::exit(linking, two, sibling.as_ref()).run(&session, "951");
                let round = format!("{linking:?}, {} first", first_kind.key());
                let link = ran.link.expect(&round).expect(&round);
                let now = working(&session);
                assert_eq!(now.len(), 2, "{round}: {now:?}");
                assert!(link.baskets.iter().all(|basket| now.iter().any(|order| order.basket_id == *basket)), "{round}");
                let expected = match (linking, first_kind) {
                    (Linking::AfterTheFact, _) | (Linking::AtSubmission, OrderKind::Limit) => Holder::Broker,
                    _ => Holder::App,
                };
                assert_eq!(link.holder, expected, "{round}");
            }
        }
    }

    /// The app's link: kept while both work or one is not heard of yet,
    /// the other cancelled once one is done, forgotten once neither works.
    #[test]
    fn the_app_cancels_the_other_once_one_is_done() {
        let session = session(Linking::None);
        long(&session, 2);
        let (s, t) = (first(&session, &stop(2)), first(&session, &target(2)));
        let link = Link { account: "951".into(), baskets: vec![s.basket_id.clone(), t.basket_id.clone(), "B99".into()], holder: Holder::App };
        assert_eq!(due(&link, &session.book()), Due::Keep);
        session.fill_part(&t.basket_id, 1);
        assert_eq!(due(&link, &session.book()), Due::Keep, "a part filled is not acted on");
        session.cancel("951", &t.basket_id).unwrap();
        assert_eq!(due(&link, &session.book()), Due::Cancel(vec![s.basket_id.clone()]));
        session.cancel("951", &s.basket_id).unwrap();
        assert_eq!(due(&link, &session.book()), Due::Drop);
        assert!(!standing(&link, &session.book()));
    }

    /// After the fact, as Rithmic: placed, then linked with the working
    /// one, as before — and not linked with one that went meanwhile.
    #[test]
    fn after_the_fact_places_then_links_as_before() {
        let session = session(Linking::AfterTheFact);
        long(&session, 1);
        let working_stop = first(&session, &stop(1));
        let ran = Plan::exit(Linking::AfterTheFact, target(1), Some(&working_stop)).run(&session, "951");
        let placed = ran.placed[0].1.clone().unwrap();
        assert_eq!(*session.links.lock().unwrap(), vec![vec![placed, working_stop.basket_id.clone()]]);
        let heard = session.heard.lock().unwrap().clone();
        assert_eq!(heard[heard.len() - 2..], ["place", "link"]);

        let session = self::session(Linking::AfterTheFact);
        long(&session, 1);
        let working_stop = first(&session, &stop(1));
        let plan = Plan::exit(Linking::AfterTheFact, target(1), Some(&working_stop));
        session.cancel("951", &working_stop.basket_id).unwrap();
        let ran = plan.run(&session, "951");
        assert_eq!(ran.link, None, "nothing left to link with");
        assert!(session.links.lock().unwrap().is_empty());

        // The broker refuses the link: said, and the exit stays placed.
        let session = self::session(Linking::AfterTheFact);
        long(&session, 1);
        let working_stop = first(&session, &stop(1));
        *session.refuse_links.lock().unwrap() = Some("no".into());
        let ran = Plan::exit(Linking::AfterTheFact, target(1), Some(&working_stop)).run(&session, "951");
        assert!(ran.placed[0].1.is_ok());
        assert_eq!(ran.link, Some(Err(Failure::Refused("no".into()))));
    }
}
