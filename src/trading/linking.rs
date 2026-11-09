//! Linking: a position's stop and target that never both outlive it.
//!
//! Every trading service gets the same feature, asked for the same way — "link this
//! exit with the other" — and the foundation does it with whatever the
//! trading service has, which [`Linking`] says:
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
//! # A second exit, on a trading service that links only at submission
//!
//! The position holds one exit, working alone, and the trader adds the
//! other. The pair can only be formed by submitting both anew, so the one
//! working has to be replaced, and one rule holds the order of the steps:
//! **never doubly protected in a way that could overfill** — never two
//! working exits that one price touches at once. Two stops (or two
//! targets) for the whole position, at the same price, both fill on the
//! same tick and turn a long into a short of the same size.
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
//! So whichever exit is working, it is replaced the same way —
//! [`Plan::Reform`]: cancel it, and once the trading service says it is
//! cancelled, submit it again, at its price, with the new one as one pair,
//! the stop first. Never two of a kind: the second is submitted only once
//! the first is gone.
//!
//! **Both of the pair are for what is held then**, whatever the one
//! cancelled was for. An OCO pair is two ways out of one position: the
//! stop and the target each close all of it, and a part of either filled
//! comes off the other (DTC's `PartialFillHandling`), so the two never
//! close more than is held. A stop for 1 on a position since added to is
//! sent again for 2 beside a target for 2, rather than the target cut to
//! 1 and half the position left with no target; a stop for more than is
//! held comes down to it. Either is said, and so is a part of the one
//! cancelled that filled meanwhile. If it filled whole, nothing is placed
//! and that is said.
//!
//! **The one cancelled may be one of a pair already.** On Sierra Chart,
//! as on DTC generally, cancelling one of an OCO pair cancels the other:
//! a stop paired with a target for part of the position takes that target
//! with it. The new pair covers what is held, so nothing is left without a
//! stop or a target; the one that went with it is said, and if the trading
//! service left it working it is cancelled before the pair goes, since two
//! targets for the whole of a position would overfill.
//!
//! **Whenever the one cancelled is not confirmed working again, it goes
//! back in alone at once**, as it was: the pair refused, only the other of
//! it taken (then the app links the two, while it runs), or no answer at
//! all. With no answer the pair may yet arrive, so what the book holds of
//! it is cancelled first and the one cancelled placed back only once every
//! one of the pair is confirmed gone: cancelled, or, its cancel refused,
//! not among what the trading service then lists as working. Never two
//! stops, and never one put back on a position a leg of the pair closed:
//! if a cancel goes unanswered, a leg is still listed or one filled,
//! nothing more is sent, and the position is said to need looking at. If
//! the first cancel is not answered, nothing is placed — the one cancelled
//! may still be working — unless the book says it is cancelled, when it
//! goes back in.
//!
//! - **A target working, a stop added**: between the two steps the
//!   position has no target, and it had no stop before: it is exposed to
//!   nothing it was not already exposed to, and at most misses a fill at
//!   the target for one round trip.
//! - **A stop working, a target added**: between the two steps the
//!   position has **no stop**, for one round trip — the one risk of this
//!   plan, taken knowingly: a market that runs through the stop's price in
//!   that window is not stopped out there. Longer if the pair is refused,
//!   until the stop placed back alone is taken, which is why that is
//!   placed at once. Submitting the pair first instead would have two
//!   stops working at one price until the old one was cancelled, which is
//!   the overfill the rule forbids. Placing the target first, or both at
//!   once, has no such window.
//!
//! # The app's own link
//!
//! For a trading service that cannot link at all, the app watches
//! the two: when one fills or is cancelled, it cancels the other
//! ([`due`], run by the desk on every change of the book). It belongs here
//! rather than nowhere because two exits left unlinked are worse — the one
//! left behind opens a position of its own when it is touched — and the
//! trader asked for the link. Its limits, which the window shows by
//! drawing such a link differently and saying "while Omacharts runs":
//!
//! - It holds only while the app runs and the session is signed in. Quit,
//!   and the two are unlinked at the trading service.
//! - It acts a round trip after the trading service says one filled, so a market
//!   that runs from one exit to the other within that time fills both.
//! - A part of one filled is not taken off the other: the other is
//!   cancelled only when the first is done.

use super::*;

/// How a trading service can make a stop and a target one-cancels-other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linking {
    /// Orders already working are linked where they stand, held by the
    /// trading service: Rithmic.
    AfterTheFact,
    /// Only two orders submitted together, as one OCO pair, held by the
    /// trading service: Sierra Chart over DTC.
    AtSubmission,
    /// Not at all: the app holds the link, while it runs.
    None,
}

/// Who holds a link, which decides whether it outlives the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// The trading service: it holds whether or not the app runs.
    TradingService,
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
    /// Placed, then the trading service links them with the working order `with`,
    /// when there is one.
    PlaceThenLink { place: Vec<OrderRequest>, with: Option<String> },
    /// Placed together, as one pair the trading service links.
    Pair([OrderRequest; 2]),
    /// The working exit `cancel` cancelled, then the pair placed: the stop
    /// first and the target second, one of them `cancel` again at its
    /// price, both sized to what is held then. `partners` are the orders
    /// `cancel` is already paired with, which its cancel takes with it.
    Reform { cancel: Box<Order>, pair: [OrderRequest; 2], partners: Vec<Order> },
    /// Placed, and the app holds the link with `with`, when there is one.
    PlaceThenWatch { place: Vec<OrderRequest>, with: Option<String> },
}

impl Plan {
    /// A new exit, `exit`, on a trading service that links as `linking`, with
    /// `sibling` the other exit working on the position, if any.
    pub fn exit(linking: Linking, exit: OrderRequest, sibling: Option<&Order>) -> Plan {
        let Some(sibling) = sibling else { return Plan::Alone(exit) };
        let with = Some(sibling.basket_id.clone());
        match linking {
            Linking::AfterTheFact => Plan::PlaceThenLink { place: vec![exit], with },
            // Cancel the working one, then the pair, the stop first.
            Linking::AtSubmission if reformable(sibling.kind, exit.kind) => {
                // Both for what is held, which is what the new exit is for:
                // see the module's notes.
                let qty = exit.qty;
                let again = OrderRequest {
                    account: exit.account.clone(),
                    contract: sibling.contract.clone(),
                    exchange: sibling.exchange.clone(),
                    side: sibling.side,
                    qty,
                    kind: sibling.kind,
                    price: sibling.price,
                    tag: format!("{}:again", exit.tag),
                };
                let pair = match exit.kind {
                    OrderKind::Stop => [exit, again],
                    _ => [again, exit],
                };
                Plan::Reform { cancel: Box::new(sibling.clone()), pair, partners: Vec::new() }
            }
            Linking::AtSubmission | Linking::None => Plan::PlaceThenWatch { place: vec![exit], with },
        }
    }

    /// The same plan, knowing that the working exit it cancels is one of a
    /// pair with `partners`, which its cancel takes with it.
    pub fn paired_with(self, partners: Vec<Order>) -> Plan {
        match self {
            Plan::Reform { cancel, pair, .. } => Plan::Reform { cancel, pair, partners },
            plan => plan,
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
            Plan::PlaceThenLink { place, with } if place.len() + usize::from(with.is_some()) >= 2 => Some(Holder::TradingService),
            Plan::PlaceThenWatch { place, with } if place.len() + usize::from(with.is_some()) >= 2 => Some(Holder::App),
            Plan::Pair(_) | Plan::Reform { .. } => Some(Holder::TradingService),
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
    /// every call blocks until the trading service has answered.
    pub fn run(&self, session: &dyn Session, account: &str) -> Ran {
        match self {
            Plan::Alone(order) => Ran { placed: vec![(order.clone(), session.place(order))], ..Ran::default() },
            Plan::PlaceThenLink { place, with } => place_then(session, account, place, with.as_deref(), Holder::TradingService),
            Plan::PlaceThenWatch { place, with } => place_then(session, account, place, with.as_deref(), Holder::App),
            Plan::Pair(pair) => pair_placed(session, account, pair.clone()),
            Plan::Reform { cancel, pair, partners } => reform(session, account, cancel, pair, partners),
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
    /// An exit cancelled to form a pair that did not come to be working,
    /// placed back alone as it was.
    pub restored: Option<(OrderRequest, Result<String, Failure>)>,
    /// A reformed pair sized down from what was planned, as `(planned,
    /// sent)`: the exit cancelled filled in part meanwhile.
    pub resized: Option<(u32, u32)>,
    /// The exit cancelled, sent again for another quantity than it had
    /// left, as `(kind, had, sent)`: for what is held.
    pub again: Option<(OrderKind, u32, u32)>,
    /// Orders the exit cancelled was paired with, which went with it.
    pub went_with: Vec<Order>,
    /// Why the position may be left without the exit cancelled, when
    /// nothing more could be done about it: the trading service stopped
    /// answering.
    pub unsure: Option<String>,
}

/// Can a working exit of `working` and a new one of `added` be made one
/// pair, the working one cancelled first? A stop and a target, either way.
fn reformable(working: OrderKind, added: OrderKind) -> bool {
    matches!((working, added), (OrderKind::Limit, OrderKind::Stop) | (OrderKind::Stop, OrderKind::Limit))
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
/// the trading service's link, or the app's.
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
        Holder::TradingService => session.link(account, &link.baskets).map(|()| link),
        Holder::App => Ok(link),
    });
    ran
}

/// `pair` placed as one, the trading service holding the link.
fn pair_placed(session: &dyn Session, account: &str, pair: [OrderRequest; 2]) -> Ran {
    let mut ran = Ran::default();
    match session.place_pair(&pair) {
        Ok(baskets) => {
            ran.link = Some(Ok(Link { account: account.to_string(), baskets: baskets.to_vec(), holder: Holder::TradingService }));
            ran.placed = pair.into_iter().zip(baskets).map(|(order, basket)| (order, Ok(basket))).collect();
        }
        // Refused, though one of the two may be working all the same, if
        // the trading service took only one: the book says which.
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

/// Is the order `basket` working as the trading service has confirmed,
/// not merely sent and waiting to be heard of?
fn confirmed(book: &Book, basket: &str) -> bool {
    book.order(basket).is_some_and(|order| matches!(order.status.stage, Stage::Working | Stage::PartiallyFilled))
}

/// The order the book knows by `tag`, at whatever stage.
fn tagged<'a>(book: &'a Book, tag: &str) -> Option<&'a Order> {
    book.orders.iter().find(|order| order.tag.as_deref() == Some(tag))
}

/// Cancel the working exit, then place the pair, it among them: see the
/// module's notes for why in this order.
fn reform(session: &dyn Session, account: &str, cancel: &Order, pair: &[OrderRequest; 2], partners: &[Order]) -> Ran {
    let mut ran = Ran::default();
    let what = if cancel.kind == OrderKind::Stop { "stop" } else { "target" };
    // The one placed again, which is the one cancelled.
    let again = usize::from(pair[1].kind == cancel.kind);
    let cancelled = session.cancel(account, &cancel.basket_id);
    let failed = cancelled.clone().err();
    ran.cancelled = Some(cancelled);
    if let Some(why) = failed {
        // Not cancelled, or no answer: it may still be working, and a
        // second one would overfill. Nothing is placed. Refused because it
        // filled meanwhile, that is what is said.
        let book = session.book();
        let now = book.order(&cancel.basket_id);
        let filled = now.is_some_and(|now| !now.status.working() && now.filled > cancel.filled);
        // Unanswered, and yet the book has it cancelled: the position has
        // lost it, so it goes back in.
        let gone = matches!(why, Failure::Unanswered(_)) && now.is_some_and(|now| now.status.stage == Stage::Cancelled);
        let why = match why {
            Failure::Refused(_) if filled => Failure::Refused(format!("nothing left to close: the {what} filled meanwhile")),
            Failure::Refused(said) => Failure::Refused(format!("the {what} could not be replaced: {said}")),
            unanswered => unanswered,
        };
        ran.placed = pair.iter().map(|order| (order.clone(), Err(why.clone()))).collect();
        if gone {
            let left = cancel.qty.saturating_sub(now.map_or(cancel.filled, |now| now.filled));
            if left > 0 {
                let back = OrderRequest { qty: left, ..pair[again].clone() };
                put_back(session, &mut ran, back);
            }
        }
        return ran;
    }
    let planned = pair[again].qty;
    let left = left_to_close(session, account, cancel, planned);
    if left == 0 {
        let why = Failure::Refused(format!("nothing left to close: the {what} filled meanwhile"));
        ran.placed = pair.iter().map(|order| (order.clone(), Err(why.clone()))).collect();
        return ran;
    }
    let sized = pair.clone().map(|order| OrderRequest { qty: order.qty.min(left), ..order });
    let restore = OrderRequest { qty: cancel.qty.saturating_sub(cancel.filled).min(left), ..sized[again].clone() };

    // What it was paired with went with it, or is cancelled now: a target
    // left working beside the new pair's would close the position twice.
    for partner in partners {
        if !session.book().order(&partner.basket_id).is_some_and(|order| order.status.working()) {
            ran.went_with.push(partner.clone());
            continue;
        }
        match session.cancel(account, &partner.basket_id) {
            Ok(()) => ran.went_with.push(partner.clone()),
            // Done meanwhile: not working, which is all that matters.
            Err(Failure::Refused(_)) if !working(session, &partner.basket_id) => ran.went_with.push(partner.clone()),
            Err(why) => {
                let why = Failure::Refused(format!("the {what}'s pair could not be cancelled: {}", why.words()));
                ran.placed = pair.iter().map(|order| (order.clone(), Err(why.clone()))).collect();
                put_back(session, &mut ran, restore);
                return ran;
            }
        }
    }

    ran = Ran { cancelled: ran.cancelled, went_with: ran.went_with, ..pair_placed(session, account, sized.clone()) };
    ran.resized = (left < planned).then_some((planned, left));
    let had = cancel.qty.saturating_sub(session.book().order(&cancel.basket_id).map_or(cancel.filled, |order| order.filled));
    if had != sized[again].qty && ran.placed.get(again).is_some_and(|(_, placed)| placed.is_ok()) {
        ran.again = Some((cancel.kind, had, sized[again].qty));
    }

    let unanswered = ran.placed.iter().any(|(_, placed)| matches!(placed, Err(Failure::Unanswered(_))));
    if unanswered {
        // The pair may be on its way: whatever the book holds of it is
        // cancelled first, so that the one put back is never a second.
        let book = session.book();
        let legs: Vec<String> = sized.iter().filter_map(|order| tagged(&book, &order.tag)).map(|order| order.basket_id.clone()).collect();
        let on_its_way: Vec<String> =
            legs.iter().filter(|basket| book.order(basket).is_some_and(|order| order.status.working())).cloned().collect();
        drop(book);
        if on_its_way.iter().all(|basket| confirmed(&session.book(), basket)) && on_its_way.len() == 2 {
            // Both taken after all: the pair is working and linked.
            ran.placed = sized.iter().cloned().zip(on_its_way.iter().cloned().map(Ok)).collect();
            ran.link = Some(Ok(Link { account: account.to_string(), baskets: on_its_way, holder: Holder::TradingService }));
            return ran;
        }
        let mut refused = false;
        for basket in &on_its_way {
            match session.cancel(account, basket) {
                Ok(()) => {}
                Err(Failure::Unanswered(_)) => {
                    ran.unsure = Some(format!("no answer from the trading service: the {what} may not be working, check the account"));
                    return ran;
                }
                Err(Failure::Refused(_)) => refused = true,
            }
        }
        // A cancel refused is no word that the order is gone: the trading
        // service may hold it and not cancel it, or not know it by the id
        // it was sent under. What it lists as working is asked for, and the
        // one cancelled goes back only once every one of the pair is gone
        // from it, none of them filled.
        if refused && session.refresh().is_err() {
            ran.unsure = Some(format!("the new pair could not be confirmed cancelled: the {what} was not put back, check the account"));
            return ran;
        }
        let book = session.book();
        if let Some(leg) = legs.iter().filter_map(|basket| book.order(basket)).find(|order| order.filled > 0) {
            let leg = if leg.kind == OrderKind::Stop { "stop" } else { "target" };
            ran.unsure = Some(format!("the new pair was taken after all and its {leg} filled: the {what} was not put back, check the position"));
            return ran;
        }
        if legs.iter().any(|basket| book.order(basket).is_some_and(|order| order.status.working())) {
            ran.unsure = Some(format!("the new pair is still working and would not cancel: the {what} was not put back, check the account"));
            return ran;
        }
        drop(book);
        put_back(session, &mut ran, restore);
        return ran;
    }

    // Confirmed working again as one of the pair: nothing more to do.
    let back = ran.placed.get(again).and_then(|(_, placed)| placed.as_ref().ok()).is_some();
    if back {
        return ran;
    }
    // Refused, whole or but for the other of it: the one cancelled back in
    // alone, as it was, at once; linked by the app with the other, if that
    // was taken.
    let other = ran.placed.get(1 - again).and_then(|(_, placed)| placed.as_ref().ok()).cloned();
    put_back(session, &mut ran, restore);
    if let (Some(other), Some((_, Ok(basket)))) = (other, ran.restored.as_ref()) {
        ran.link = Some(Ok(Link { account: account.to_string(), baskets: vec![basket.clone(), other], holder: Holder::App }));
    }
    ran
}

/// `order`, the exit cancelled, placed back alone under a new tag.
fn put_back(session: &dyn Session, ran: &mut Ran, order: OrderRequest) {
    let again = OrderRequest { tag: new_tag(), ..order };
    let placed = session.place(&again);
    ran.restored = Some((again, placed));
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

/// Is a link the trading service holds still standing in `book`? Only while every
/// order of it is working; once one is done the trading service has dealt with the
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

    /// Each trading service links the way it can, and the plan says who holds it
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
        assert_eq!((plan.holder(), plan.with(), plan.cancelling()), (Some(Holder::TradingService), Some("S"), None));

        let plan = Plan::exit(Linking::AtSubmission, stop(1), Some(&target_working));
        let Plan::Reform { pair, .. } = &plan else { panic!("{plan:?}") };
        assert_eq!((pair[0].kind, pair[1].kind, pair[1].price), (OrderKind::Stop, OrderKind::Limit, Some(7_600.0)));
        assert_ne!(pair[0].tag, pair[1].tag, "each of the pair is known by its own tag");
        assert_eq!((plan.holder(), plan.cancelling(), plan.placing().len()), (Some(Holder::TradingService), Some("S"), 2));

        // A stop working and a target added: the stop cancelled, then the
        // stop again, as it was, and the target as one pair, the stop first.
        let plan = Plan::exit(Linking::AtSubmission, target(1), Some(&stop_working));
        let Plan::Reform { pair, .. } = &plan else { panic!("{plan:?}") };
        assert_eq!((pair[0].kind, pair[0].price, pair[0].qty), (OrderKind::Stop, Some(7_600.0), 1));
        assert_eq!((pair[1].kind, pair[1].price), (OrderKind::Limit, Some(7_640.0)));
        assert_eq!((plan.holder(), plan.cancelling()), (Some(Holder::TradingService), Some("S")));
        let plan = Plan::exit(Linking::None, stop(1), Some(&target_working));
        assert_eq!(plan.holder(), Some(Holder::App));

        assert!(matches!(Plan::bracket(Linking::AtSubmission, stop(1), target(1)), Plan::Pair(_)));
        assert_eq!(Plan::bracket(Linking::AfterTheFact, stop(1), target(1)).holder(), Some(Holder::TradingService));
        assert_eq!(Plan::bracket(Linking::None, stop(1), target(1)).holder(), Some(Holder::App));
    }

    /// Both exits at once: one pair where the trading service links only so, placed
    /// and linked after where it links working orders, placed and watched
    /// where it cannot link.
    #[test]
    fn a_bracket_is_linked_however_the_service_can() {
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
                    assert_eq!(link.holder, Holder::TradingService);
                    assert_eq!(*session.links.lock().unwrap(), vec![baskets.clone()]);
                    assert_eq!(session.pairs.load(Ordering::Acquire), 0);
                }
                Linking::AtSubmission => {
                    assert_eq!(link.holder, Holder::TradingService);
                    assert_eq!(session.pairs.load(Ordering::Acquire), 1, "one submission");
                }
                Linking::None => {
                    assert_eq!(link.holder, Holder::App);
                    assert!(session.links.lock().unwrap().is_empty(), "nothing asked of a trading service that cannot");
                }
            }
            // The target fills: the stop goes with it, by the trading service's hand
            // or the app's.
            session.fill(&baskets[1]);
            let book = session.book();
            match link.holder {
                Holder::TradingService => assert!(!book.order(&baskets[0]).unwrap().status.working(), "{linking:?}"),
                Holder::App => assert_eq!(due(&link, &book), Due::Cancel(vec![baskets[0].clone()])),
            }
        }
    }

    /// On a trading service that links only at submission, a stop added to a working
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
        // The order the trading service heard them in: the cancel, then the pair.
        let heard = session.heard.lock().unwrap().clone();
        assert_eq!(heard[heard.len() - 2..], ["cancel", "pair"], "{heard:?}");
        let now = working(&session);
        assert_eq!(now.len(), 2, "{now:?}");
        assert_eq!(now.iter().filter(|order| order.kind == OrderKind::Limit).count(), 1, "one target, never two");
        let link = ran.link.unwrap().unwrap();
        assert_eq!(link.holder, Holder::TradingService);
        assert!(!session.book().order(&working_target.basket_id).unwrap().status.working());
        // Linked at the trading service: the stop filling takes the target with it.
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

    /// A target added to a working stop, where the trading service links
    /// only at submission: the stop cancelled, then the stop again and the
    /// target as one pair, never two stops working; the pair refused, the
    /// stop back alone at once; the stop filled meanwhile, nothing placed.
    #[test]
    fn a_target_added_to_a_working_stop_reforms_the_pair_after_the_stop_is_cancelled() {
        let session = session(Linking::AtSubmission);
        long(&session, 2);
        let working_stop = first(&session, &stop(2));
        let ran = Plan::exit(Linking::AtSubmission, target(2), Some(&working_stop)).run(&session, "951");
        assert_eq!(ran.cancelled, Some(Ok(())));
        let heard = session.heard.lock().unwrap().clone();
        assert_eq!(heard[heard.len() - 2..], ["cancel", "pair"], "{heard:?}");
        let now = working(&session);
        assert_eq!(now.iter().filter(|order| order.kind == OrderKind::Stop).count(), 1, "one stop, never two");
        let again = now.iter().find(|order| order.kind == OrderKind::Stop).unwrap();
        assert_eq!((again.price, again.qty), (Some(7_590.0), 2), "the stop as it was");
        assert_eq!(ran.link.unwrap().unwrap().holder, Holder::TradingService);

        let session = self::session(Linking::AtSubmission);
        long(&session, 2);
        let working_stop = first(&session, &stop(2));
        let plan = Plan::exit(Linking::AtSubmission, target(2), Some(&working_stop));
        *session.refuse_pairs.lock().unwrap() = Some("Rejected at RMS".into());
        let ran = plan.run(&session, "951");
        let (again, placed) = ran.restored.expect("the stop placed again");
        assert!(placed.is_ok());
        assert_eq!((again.kind, again.price, again.qty), (OrderKind::Stop, Some(7_590.0), 2));
        let now = working(&session);
        assert_eq!((now.len(), now[0].kind), (1, OrderKind::Stop));

        let session = self::session(Linking::AtSubmission);
        long(&session, 3);
        let working_stop = first(&session, &stop(3));
        let plan = Plan::exit(Linking::AtSubmission, target(3), Some(&working_stop));
        session.fill_part(&working_stop.basket_id, 1);
        let ran = plan.run(&session, "951");
        assert!(ran.placed.iter().all(|(order, placed)| placed.is_ok() && order.qty == 2), "{ran:?}");
        assert_eq!(ran.resized, Some((3, 2)));
    }

    /// Either exit may go first, on every trading service: the second is placed and
    /// linked, by whoever can hold the link.
    #[test]
    fn either_exit_may_go_first_however_the_service_links() {
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
                let expected = match linking {
                    Linking::AfterTheFact | Linking::AtSubmission => Holder::TradingService,
                    Linking::None => Holder::App,
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

        // The trading service refuses the link: said, and the exit stays placed.
        let session = self::session(Linking::AfterTheFact);
        long(&session, 1);
        let working_stop = first(&session, &stop(1));
        *session.refuse_links.lock().unwrap() = Some("no".into());
        let ran = Plan::exit(Linking::AfterTheFact, target(1), Some(&working_stop)).run(&session, "951");
        assert!(ran.placed[0].1.is_ok());
        assert_eq!(ran.link, Some(Err(Failure::Refused("no".into()))));
    }
}
