//! What follows from the book, the accounts and the feeds, with no store
//! and no broker: which account and feed a chart is on, where an order
//! goes, which side of the market it may go on, and what answers a request.

use std::sync::Arc;

use super::logins::login_in;
use super::*;

/// Which account a chart is about: the one chosen for it while it is still
/// listed, else the default. The header and the orders both ask this, so
/// they can never disagree.
pub fn account_of<'a>(accounts: &'a [Account], default: Option<&str>, chosen: Option<&str>) -> Option<&'a Account> {
    chosen
        .and_then(|key| accounts.iter().find(|account| account.key() == key))
        .or_else(|| default_account(accounts, default))
}

/// Is a chart's chosen account one that is no longer listed, say because
/// its login was removed? Then it is forgotten and the chart follows the
/// default.
pub fn stale(accounts: &[Account], chosen: Option<&str>) -> bool {
    chosen.is_some_and(|key| !accounts.iter().any(|account| account.key() == key))
}

/// Where an order from a chart goes: its account, inside a login that is
/// signed in, and the contract its instrument trades as now.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub account: Account,
    pub login: Login,
    pub contract: String,
    pub exchange: String,
    /// The step the contract's price moves in, when the broker knows it.
    pub tick: Option<f64>,
    pub capabilities: Capabilities,
}

impl Target {
    /// The contract its orders go to.
    pub fn contract(&self) -> Contract {
        Contract { symbol: self.contract.clone(), exchange: self.exchange.clone(), tick: self.tick }
    }
}

/// What going flat in a contract takes: the orders working in it to
/// cancel, by basket, and whether a position is held to flatten.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flat {
    pub cancel: Vec<String>,
    pub flatten: bool,
}

impl Flat {
    /// Already flat: nothing held and nothing working.
    pub fn is_empty(&self) -> bool {
        self.cancel.is_empty() && !self.flatten
    }
}

/// Going flat in `contract` on the account with the key `account`:
/// every order working in it, and the position if there is one. Orders in
/// other contracts, and other accounts', are left alone.
pub fn flat_plan(book: &Book, account: &str, contract: &str) -> Flat {
    Flat {
        cancel: book.working(account, contract).map(|order| order.basket_id.clone()).collect(),
        flatten: book.position(account, contract).is_some(),
    }
}

/// Where an order from a chart of `instrument`, on the account `chosen`,
/// would go, or why it cannot: the one rule the menu and the click both
/// ask. `session` is a login's session while it is signed in.
pub fn order_target(
    view: &View,
    session: impl Fn(&str) -> Option<Arc<dyn Session>>,
    chosen: Option<&str>,
    instrument: Option<&omacharts_engine::Instrument>,
) -> Result<Target, String> {
    // A login whose accounts were never listed has not signed in yet, or
    // signed in and was listed none: the broker gives it nothing to trade.
    let account = view.account_of(chosen).ok_or_else(|| match view.logins.first() {
        Some(login) if session(&login.id).is_some() => format!("{} has no accounts", login.name),
        Some(login) => format!("{} not signed in", login.name),
        None => "No trading account".to_string(),
    })?;
    let name = view.account_name(account);
    let login = view.login(&account.login).ok_or_else(|| format!("{name} has no login"))?;
    let instrument = instrument.ok_or_else(|| "Nothing on this chart to trade".to_string())?;
    let session = session(&login.id).ok_or_else(|| format!("{name} not signed in"))?;
    let contract = session.contract_for(instrument).map_err(|refusal| match refusal {
        Refusal::NotTradable => format!("{} is not tradable on {name}", instrument.symbol),
        Refusal::NotKnownYet => format!("Finding {}'s contract…", instrument.symbol),
    })?;
    Ok(Target {
        account: account.clone(),
        login: login.clone(),
        contract: contract.symbol,
        exchange: contract.exchange,
        tick: contract.tick,
        capabilities: session.capabilities(),
    })
}

/// The orders the chart menu offers, in its order: buy and sell at the
/// market, then a limit and a stop each way, and only the kinds the broker
/// takes.
pub fn menu_orders(capabilities: &Capabilities) -> Vec<(Side, OrderKind)> {
    [OrderKind::Market, OrderKind::Limit, OrderKind::Stop]
        .into_iter()
        .filter(|kind| capabilities.kinds.contains(kind))
        .flat_map(|kind| [(Side::Buy, kind), (Side::Sell, kind)])
        .collect()
}

/// May a `side` `kind` order go in at `price` with the market at `last`?
/// A limit waits for a better price and a stop for a worse one, so each is
/// only on one side of the market; at the market itself neither is.
pub fn side_allowed(kind: OrderKind, side: Side, price: f64, last: f64) -> bool {
    match (kind, side) {
        (OrderKind::Market, _) => true,
        (OrderKind::Limit, Side::Buy) | (OrderKind::Stop, Side::Sell) => price < last,
        (OrderKind::Limit, Side::Sell) | (OrderKind::Stop, Side::Buy) => price > last,
    }
}

/// The day's limit `price` goes past, of the `(low, high)` the exchange
/// takes orders between, if it goes past one: an order there is refused.
/// A limit not known stops nothing.
pub fn past_limit(price: f64, (low, high): (Option<f64>, Option<f64>)) -> Option<f64> {
    low.filter(|low| price < *low).or(high.filter(|high| price > *high))
}

/// The side that closes a position of `net`: selling a long, buying back
/// a short.
pub fn exit_side(net: i32) -> Side {
    match net > 0 {
        true => Side::Sell,
        false => Side::Buy,
    }
}

/// May an exit of `kind` go in at `price`, closing a position of `net`,
/// with the market at `last`? A target is a limit and a stop a stop, on
/// the side that closes it, so each has only one side of the market.
pub fn exit_allowed(kind: OrderKind, net: i32, price: f64, last: f64) -> bool {
    net != 0 && kind != OrderKind::Market && side_allowed(kind, exit_side(net), price, last)
}

/// What a position's tab for an exit of `kind` says, as the exit's own
/// tag will: L for the target, a limit, STP for the stop.
pub fn exit_tab(kind: OrderKind) -> &'static str {
    match kind {
        OrderKind::Stop => "STP",
        _ => "L",
    }
}

/// Whether a position of `net` still has room for an exit of `kind`:
/// not while the working ones of that kind already close all of it.
pub fn exit_offered<'a>(orders: impl IntoIterator<Item = &'a Order>, net: i32, kind: OrderKind) -> bool {
    let side = exit_side(net);
    let covered: u32 = orders
        .into_iter()
        .filter(|order| order.status.working() && order.side == side && order.kind == kind)
        .map(|order| order.qty)
        .sum();
    net != 0 && covered < net.unsigned_abs()
}

/// What an exit of `qty` filled at `price` would realise, closing a
/// position of `net` held at `avg`, a point worth `point`: long gains as
/// the price is above its average, short as it is below. An exit larger
/// than the position closes only the position.
pub fn exit_pnl(avg: f64, net: i32, qty: u32, point: f64, price: f64) -> f64 {
    let closed = f64::from(qty.min(net.unsigned_abs()));
    (price - avg) * f64::from(net.signum()) * closed * point
}

/// Does an order of `side` and `kind` close some of a position of `net`?
/// A limit or stop on the side that closes it does; a market order on its
/// way, or one that adds to it, does not.
pub fn is_exit(side: Side, kind: OrderKind, net: i32) -> bool {
    net != 0 && kind != OrderKind::Market && side == exit_side(net)
}

/// How many ticks `to` is from `from`: positive above, negative below.
pub fn ticks_between(from: f64, to: f64, tick: f64) -> i64 {
    match tick > 0.0 {
        true => ((to - from) / tick).round() as i64,
        false => 0,
    }
}

/// The working exit a new one of `kind` on `side` pairs with: the stop for
/// a target, the target for a stop, in the same contract.
pub fn sibling_exit<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    contract: &str,
    side: Side,
    kind: OrderKind,
) -> Option<&'a Order> {
    let other = match kind {
        OrderKind::Limit => OrderKind::Stop,
        OrderKind::Stop => OrderKind::Limit,
        OrderKind::Market => return None,
    };
    orders
        .into_iter()
        .find(|order| order.status.working() && order.contract == contract && order.side == side && order.kind == other)
}

/// How far from an order's line, in pixels, the pointer may be for the
/// menu to offer cancelling it.
pub const NEAR: f64 = 6.0;

/// The working order whose line is nearest `y`, within [`NEAR`]: `to_y`
/// puts a price on the chart.
pub fn order_near<'a>(
    orders: impl IntoIterator<Item = &'a Order>,
    y: f64,
    to_y: impl Fn(f64) -> f64,
) -> Option<&'a Order> {
    orders
        .into_iter()
        .filter(|order| order.status.working())
        .filter_map(|order| Some((order, (to_y(order.price?) - y).abs())))
        .filter(|(_, off)| *off <= NEAR)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(order, _)| order)
}

/// Has the book answered `request`? An order is answered once the exchange
/// has it working (a market order once it has filled), or once it is done
/// whatever became of it; a move once the book has the order at the new
/// price; a cancel once the order is no longer working; a flatten once
/// nothing is held. Nothing that is not in the book is taken as answered:
/// a request that never hears back stays pending.
pub fn answered(request: &Request, pending: &Pending, book: &Book) -> bool {
    match (request, pending) {
        (Request::Place(tag), Pending::Place { key, order }) => {
            let found = book.orders.iter().find(|known| known.tag.as_deref() == Some(tag) && known.account == *key);
            match found {
                None => false,
                Some(known) if order.kind == OrderKind::Market => !known.status.working(),
                Some(known) => known.status != Stage::Pending,
            }
        }
        (Request::Move(basket), Pending::Move(price)) => book
            .order(basket)
            .is_some_and(|known| !known.status.working() || known.price == Some(*price)),
        (Request::Cancel(basket), _) => book.order(basket).is_some_and(|known| !known.status.working()),
        (Request::Flatten(key, contract), _) => book.position(key, contract).is_none(),
        _ => true,
    }
}

/// How an account is named: its login's name, "Lucid", with the account
/// number after it only when the name alone would not say which, because
/// the login holds several accounts or another login has the same name.
pub fn account_name(account: &Account, logins: &[Login], accounts: &[Account]) -> String {
    let Some(login) = login_in(logins, &account.login) else {
        return account.id.clone();
    };
    let several = accounts.iter().filter(|other| other.login == account.login).count() > 1;
    let shared = logins.iter().any(|other| other.id != login.id && other.name == login.name);
    match several || shared {
        true => format!("{} {}", login.name, account.id),
        false => login.name.clone(),
    }
}

/// The feed a chart on `account` is drawn from when nobody chose one: the
/// account's own login's feed while that login is the data login and the
/// feed serves `instrument`, else the first feed in the order that serves
/// it. Whether a feed can be used now does not move it: this is the one
/// the menu lists first, and a chart falls back from it only for
/// as long as it cannot be used.
pub fn default_feed<'a>(
    view: &View,
    account: Option<&Account>,
    feeds: impl IntoIterator<Item = &'a dyn omacharts_engine::Provider> + Clone,
    instrument: &omacharts_engine::Instrument,
) -> Option<&'static str> {
    own_feed(view, account, feeds.clone().into_iter().map(|provider| (provider, true)), instrument).or_else(|| {
        feeds.into_iter().find(|provider| provider.symbol_for(instrument).is_some()).map(|provider| provider.id())
    })
}

/// The account's own login's feed, when that login is the data login and
/// the feed serves `instrument` and can be used.
fn own_feed<'a>(
    view: &View,
    account: Option<&Account>,
    feeds: impl IntoIterator<Item = (&'a dyn omacharts_engine::Provider, bool)>,
    instrument: &omacharts_engine::Instrument,
) -> Option<&'static str> {
    let account = account?;
    if view.data_login.as_deref() != Some(account.login.as_str()) {
        return None;
    }
    let broker = &view.login(&account.login)?.broker;
    feeds
        .into_iter()
        .find(|(provider, _)| provider.id().eq_ignore_ascii_case(broker))
        .filter(|(provider, usable)| *usable && provider.symbol_for(instrument).is_some())
        .map(|(provider, _)| provider.id())
}

/// Which feed a chart on `account` is pointed at: the one `chosen` for it,
/// else the account's own login's feed while that login is the data login
/// and the feed can serve `instrument` now. `None` leaves it to the order,
/// [`omacharts_engine::providers::route`]. The header and the order ticket
/// both ask this, through the window's one route, so they never disagree.
pub fn feed_of<'a>(
    view: &View,
    account: Option<&Account>,
    chosen: Option<&'a str>,
    feeds: impl IntoIterator<Item = (&'a dyn omacharts_engine::Provider, bool)>,
    instrument: &omacharts_engine::Instrument,
) -> Option<&'a str> {
    chosen.or_else(|| own_feed(view, account, feeds, instrument))
}

/// The last price a chart on screen shows for `held`'s contract, to mark
/// it to while the broker's market data has heard nothing: only a chart of
/// that very contract, MNQZ6 for MNQZ6, on the exchange it is held on.
/// Never a continuous series, MNQ, whose price may be another month's or
/// back-adjusted. `charts` is every chart's instrument and last price.
pub fn charted_price<'a>(
    held: &Position,
    charts: impl IntoIterator<Item = (&'a omacharts_engine::Instrument, Option<f64>)>,
) -> Option<f64> {
    let exchange_of = |instrument: &'a omacharts_engine::Instrument| {
        instrument
            .exchange
            .clone()
            .or_else(|| omacharts_engine::exchanges::registry().contract_exchange(instrument.root()).map(str::to_string))
    };
    charts
        .into_iter()
        .filter(|(instrument, _)| instrument.is_contract() && instrument.symbol.eq_ignore_ascii_case(&held.contract))
        .filter(|(instrument, _)| {
            held.exchange.trim().is_empty()
                || exchange_of(instrument).is_none_or(|exchange| exchange.eq_ignore_ascii_case(held.exchange.trim()))
        })
        .find_map(|(_, last)| last)
}

/// What the Account part says: the account's name, with its trouble
/// after it when its login is in trouble, "Lucid 1234 signed out".
pub fn account_text(name: &str, trouble: Option<&str>) -> String {
    match trouble {
        Some(trouble) => format!("{name} {trouble}"),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::tests::*;
    use crate::trading::fake::*;

    /// An exit's P&L: ES at $50 a point, NQ at $20, long and short, its
    /// target and its stop, and an exit closing part of the position.
    #[test]
    fn an_exit_realises_its_distance_from_the_average() {
        // Long 2 ES at 7,600: a target 5 points up is +$500, a stop 2.5
        // points down -$250.
        assert_eq!(exit_pnl(7_600.0, 2, 2, 50.0, 7_605.0), 500.0);
        assert_eq!(exit_pnl(7_600.0, 2, 2, 50.0, 7_597.5), -250.0);
        // Short 3 NQ at 25,000: a target 10 points down is +$600, a stop
        // 4 points up -$240.
        assert_eq!(exit_pnl(25_000.0, -3, 3, 20.0, 24_990.0), 600.0);
        assert_eq!(exit_pnl(25_000.0, -3, 3, 20.0, 25_004.0), -240.0);
        // A partial exit: 1 of a long 3 ES, 4 points up.
        assert_eq!(exit_pnl(7_600.0, 3, 1, 50.0, 7_604.0), 200.0);
        // An exit bigger than the position closes only the position.
        assert_eq!(exit_pnl(7_600.0, 1, 5, 50.0, 7_604.0), 200.0);
        // At the average, nothing.
        assert_eq!(exit_pnl(7_600.0, 1, 1, 50.0, 7_600.0), 0.0);
    }

    /// Only a limit or a stop on the closing side is an exit.
    #[test]
    fn an_exit_closes_the_position() {
        assert!(is_exit(Side::Sell, OrderKind::Limit, 2));
        assert!(is_exit(Side::Sell, OrderKind::Stop, 2));
        assert!(is_exit(Side::Buy, OrderKind::Limit, -1));
        assert!(!is_exit(Side::Buy, OrderKind::Limit, 2), "adds to a long");
        assert!(!is_exit(Side::Sell, OrderKind::Market, 2), "a market order on its way");
        assert!(!is_exit(Side::Sell, OrderKind::Limit, 0), "nothing held");
    }

    /// The default is the one made default while it is listed, else the
    /// first; a chart's own account wins while it is listed, and one that is
    /// not is stale and falls back to the default.
    #[test]
    fn a_charts_account_is_its_own_or_the_default() {
        let accounts = vec![account("a", "1234"), account("a", "1235"), account("b", "951")];
        let key = |account: Option<&Account>| account.map(Account::key);
        assert_eq!(key(default_account(&accounts, None)).as_deref(), Some("a/1234"));
        assert_eq!(key(default_account(&accounts, Some("b/951"))).as_deref(), Some("b/951"));
        assert_eq!(key(default_account(&accounts, Some("gone/1"))).as_deref(), Some("a/1234"), "stale is ignored");
        assert_eq!(key(account_of(&accounts, Some("b/951"), None)).as_deref(), Some("b/951"));
        assert_eq!(key(account_of(&accounts, Some("b/951"), Some("a/1235"))).as_deref(), Some("a/1235"));
        assert_eq!(key(account_of(&accounts, Some("b/951"), Some("gone/1"))).as_deref(), Some("b/951"));
        assert!(stale(&accounts, Some("gone/1")));
        assert!(!stale(&accounts, Some("a/1235")));
        assert!(!stale(&accounts, None));
        assert_eq!(account_of(&[], None, Some("a/1")), None);
    }

    /// The Account part names the account, and its trouble after it.
    #[test]
    fn the_account_part_names_the_account_and_its_trouble() {
        assert_eq!(account_text("Lucid 1234", None), "Lucid 1234");
        assert_eq!(account_text("Lucid 1234", Some("signed out")), "Lucid 1234 signed out");
    }

    /// An account goes by its login's name, with its number only when the
    /// name alone would not say which: the login holds several, or another
    /// login has the same name.
    #[test]
    fn an_account_is_named_by_its_login() {
        let rithmic = test_login("a", "Rithmic", TEST_SYSTEM);
        let lucid = test_login("b", "Lucid", "LucidTrading");
        let a951 = account("a", "951");
        let b1234 = account("b", "1234");
        let b1235 = account("b", "1235");

        assert_eq!(account_name(&a951, std::slice::from_ref(&rithmic), std::slice::from_ref(&a951)), "Rithmic");
        let logins = [rithmic.clone(), lucid.clone()];
        let accounts = [a951.clone(), b1234.clone(), b1235.clone()];
        assert_eq!(account_name(&a951, &logins, &accounts), "Rithmic");
        assert_eq!(account_name(&b1234, &logins, &accounts), "Lucid 1234");
        assert_eq!(account_name(&b1235, &logins, &accounts), "Lucid 1235");
        // Two logins called the same.
        let twin = Login { id: "c".into(), ..rithmic.clone() };
        let c7 = account("c", "7");
        assert_eq!(account_name(&a951, &[rithmic, twin], &[a951.clone(), c7.clone()]), "Rithmic 951");
        // A login that is gone: the number is all there is.
        assert_eq!(account_name(&c7, &[lucid], std::slice::from_ref(&c7)), "7");
    }

    /// A chart's feed: the one chosen for it; else its account's own,
    /// while that account's login is the data login and the feed serves the
    /// instrument and can be used; else the order decides. The default the
    /// popover marks is the account's own whether or not it can be used
    /// now, and the order's first that serves it otherwise.
    #[test]
    fn a_charts_feed_is_its_own_the_accounts_or_the_order() {
        use omacharts_engine::providers::built;
        use omacharts_engine::InstrumentKind;
        let yahoo = built("yahoo").expect("Yahoo");
        let rithmic = built("rithmic").expect("Rithmic");
        let es = instrument("ES", InstrumentKind::FutureRoot);
        let spy = instrument("SPY", InstrumentKind::Etf);
        assert!(rithmic.symbol_for(&spy).is_none() && rithmic.symbol_for(&es).is_some());
        let data = account("a", "951");
        let other = account("b", "1234");
        let view = View {
            logins: vec![test_login("a", "Rithmic", TEST_SYSTEM), test_login("b", "Lucid", "LucidTrading")],
            accounts: vec![data.clone(), other.clone()],
            data_login: Some("a".into()),
            ..View::default()
        };
        // Yahoo on top of the order, so the account's own is seen to win.
        let order = |rithmic_usable: bool| [(yahoo.as_ref(), true), (rithmic.as_ref(), rithmic_usable)];
        let plain = [yahoo.as_ref(), rithmic.as_ref()];

        assert_eq!(feed_of(&view, Some(&data), Some("yahoo"), order(true), &es), Some("yahoo"), "chosen wins");
        assert_eq!(feed_of(&view, Some(&data), None, order(true), &es), Some("rithmic"), "the account's own");
        assert_eq!(feed_of(&view, Some(&data), None, order(false), &es), None, "falls back to the order");
        assert_eq!(feed_of(&view, Some(&data), None, order(true), &spy), None, "its own does not serve it");
        assert_eq!(feed_of(&view, Some(&other), None, order(true), &es), None, "not the data login");
        assert_eq!(feed_of(&view, None, None, order(true), &es), None, "no account");

        assert_eq!(default_feed(&view, Some(&data), plain, &es), Some("rithmic"));
        assert_eq!(default_feed(&view, Some(&data), plain, &spy), Some("yahoo"), "the order's pick");
        assert_eq!(default_feed(&view, Some(&other), plain, &es), Some("yahoo"));
        assert_eq!(default_feed(&view, None, [rithmic.as_ref(), yahoo.as_ref()], &es), Some("rithmic"));
    }

    /// A long closes by selling and a short by buying back; its target is
    /// a limit on the far side of the market and its stop a stop on the
    /// near side, and a tab goes once the exits of its kind close it all.
    #[test]
    fn an_exit_closes_the_position_on_its_own_side_of_the_market() {
        assert_eq!(exit_side(2), Side::Sell);
        assert_eq!(exit_side(-1), Side::Buy);
        let last = 7_612.25;
        assert!(exit_allowed(OrderKind::Limit, 1, 7_640.0, last), "a long's target above");
        assert!(!exit_allowed(OrderKind::Limit, 1, 7_600.0, last));
        assert!(exit_allowed(OrderKind::Stop, 1, 7_600.0, last), "a long's stop below");
        assert!(!exit_allowed(OrderKind::Stop, 1, 7_640.0, last));
        assert!(exit_allowed(OrderKind::Limit, -1, 7_600.0, last), "a short's target below");
        assert!(exit_allowed(OrderKind::Stop, -1, 7_640.0, last), "a short's stop above");
        assert!(!exit_allowed(OrderKind::Stop, 1, last, last), "at the market itself");
        assert!(!exit_allowed(OrderKind::Limit, 0, 7_640.0, last), "nothing held");
        assert_eq!((exit_tab(OrderKind::Limit), exit_tab(OrderKind::Stop)), ("L", "STP"));

        let target = order("T", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Working);
        assert!(!exit_offered([&target], 1, OrderKind::Limit), "a one-lot already has its target");
        assert!(exit_offered([&target], 1, OrderKind::Stop));
        assert!(exit_offered([&target], 2, OrderKind::Limit), "one of two still open");
        let gone = order("G", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Cancelled);
        assert!(exit_offered([&gone], 1, OrderKind::Limit), "a cancelled one closes nothing");
    }

    /// A new target pairs with the stop already working on the same side
    /// of the same contract, and a new stop with the target.
    #[test]
    fn a_new_exit_pairs_with_the_other_kind_on_its_side() {
        let stop = order("S", Side::Sell, OrderKind::Stop, Some(7_600.0), Stage::Working);
        let buy_stop = order("BS", Side::Buy, OrderKind::Stop, Some(7_650.0), Stage::Working);
        let target = order("T", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Working);
        let orders = [buy_stop, stop];
        let found = sibling_exit(&orders, "ESZ6", Side::Sell, OrderKind::Limit).map(|o| o.basket_id.as_str());
        assert_eq!(found, Some("S"));
        assert!(sibling_exit(&orders, "NQZ6", Side::Sell, OrderKind::Limit).is_none(), "another contract");
        assert!(sibling_exit(&orders, "ESZ6", Side::Sell, OrderKind::Stop).is_none(), "a stop pairs with a target");
        let found = sibling_exit([&target], "ESZ6", Side::Sell, OrderKind::Stop).map(|o| o.basket_id.as_str());
        assert_eq!(found, Some("T"));
    }

    /// What becomes of each request, read off the book: an order only once
    /// the exchange has it working, a market order only on its fill, a
    /// move once the book has the new price, a cancel once it is not
    /// working, a flatten once nothing is held; and nothing that is not in
    /// the book at all.
    #[test]
    fn a_request_is_answered_only_by_what_the_book_says() {
        let mut book = Book::default();
        let sent = OrderRequest {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Sell,
            qty: 1,
            kind: OrderKind::Limit,
            price: Some(7_640.0),
            tag: "omacharts:1".into(),
        };
        let place = Request::Place("omacharts:1".into());
        let pending = Pending::Place { key: "t/951".into(), order: sent.clone() };
        assert!(!answered(&place, &pending, &book), "never heard of: still pending");
        let mut known = order("B1", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Pending);
        known.tag = Some("omacharts:1".into());
        book.orders.push(known);
        assert!(!answered(&place, &pending, &book), "received is not working");
        book.orders[0].status = Stage::Working.into();
        assert!(answered(&place, &pending, &book));
        book.orders[0].status = Status::said(Stage::Rejected, "no");
        assert!(answered(&place, &pending, &book), "a rejection is an answer");

        let market = Pending::Place { key: "t/951".into(), order: OrderRequest { kind: OrderKind::Market, price: None, ..sent } };
        book.orders[0].kind = OrderKind::Market;
        book.orders[0].status = Stage::Working.into();
        assert!(!answered(&place, &market, &book), "a market order is answered by its fill");
        book.orders[0].status = Stage::Filled.into();
        assert!(answered(&place, &market, &book));

        book.orders[0] = order("B1", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Working);
        let moved = Request::Move("B1".into());
        assert!(!answered(&moved, &Pending::Move(7_645.0), &book));
        book.orders[0].price = Some(7_645.0);
        assert!(answered(&moved, &Pending::Move(7_645.0), &book));
        assert!(!answered(&Request::Move("B9".into()), &Pending::Move(1.0), &book), "not in the book");

        let cancel = Request::Cancel("B1".into());
        assert!(!answered(&cancel, &Pending::Cancel, &book));
        book.orders[0].status = Stage::Cancelled.into();
        assert!(answered(&cancel, &Pending::Cancel, &book));

        let flatten = Request::Flatten("t/951".into(), "ESZ6".into());
        book.positions.push(Position {
            account: "t/951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            net: 1,
            avg: Some(7_612.25),
            open_pnl: None,
            tick: Some(0.25),
            point: Some(50.0),
        });
        assert!(!answered(&flatten, &Pending::Flatten, &book));
        book.positions.clear();
        assert!(answered(&flatten, &Pending::Flatten, &book));

        assert_eq!(buying_text(Side::Buy, 1), "Buying 1…");
    }

    /// A limit waits for a better price and a stop for a worse one; at the
    /// market neither is allowed, and a market order always is.
    #[test]
    fn a_limit_and_a_stop_are_each_on_one_side_of_the_market() {
        let last = 7_612.25;
        assert!(side_allowed(OrderKind::Limit, Side::Buy, 7_600.0, last));
        assert!(!side_allowed(OrderKind::Limit, Side::Buy, 7_620.0, last));
        assert!(side_allowed(OrderKind::Limit, Side::Sell, 7_620.0, last));
        assert!(!side_allowed(OrderKind::Limit, Side::Sell, 7_600.0, last));
        assert!(side_allowed(OrderKind::Stop, Side::Buy, 7_620.0, last));
        assert!(!side_allowed(OrderKind::Stop, Side::Buy, 7_600.0, last));
        assert!(side_allowed(OrderKind::Stop, Side::Sell, 7_600.0, last));
        assert!(!side_allowed(OrderKind::Stop, Side::Sell, 7_620.0, last));
        for kind in [OrderKind::Limit, OrderKind::Stop] {
            for side in [Side::Buy, Side::Sell] {
                assert!(!side_allowed(kind, side, last, last), "{kind:?} {side:?} at the market");
            }
        }
        assert!(side_allowed(OrderKind::Market, Side::Sell, 0.0, last));

        // Only what the broker takes is offered.
        let all = Capabilities {
            kinds: vec![OrderKind::Stop, OrderKind::Market, OrderKind::Limit],
            flatten: true,
            cancel_all: true,
            modify: true,
            link: Linking::AfterTheFact,
            history: true,
            snapshot: true,
        };
        assert_eq!(menu_orders(&all).len(), 6);
        assert_eq!(menu_orders(&all)[..2], [(Side::Buy, OrderKind::Market), (Side::Sell, OrderKind::Market)]);
        let no_stops = Capabilities { kinds: vec![OrderKind::Market, OrderKind::Limit], ..all };
        assert!(menu_orders(&no_stops).iter().all(|(_, kind)| *kind != OrderKind::Stop));
        assert_eq!(menu_orders(&no_stops).len(), 4);
        assert_eq!(crossed_text("7,612.50", OrderKind::Limit, Side::Buy), "Not placed · 7,612.50 is above the market now");
        assert_eq!(crossed_text("7,600.00", OrderKind::Limit, Side::Sell), "Not placed · 7,600.00 is below the market now");
    }

    /// The menu offers to cancel the working order whose line the pointer
    /// is on, within six pixels, the nearest of two, and never a done one.
    #[test]
    fn the_order_near_the_pointer_is_the_nearest_working_one() {
        let orders = vec![
            order("a", Side::Sell, OrderKind::Limit, Some(7_620.0), Stage::Working),
            order("b", Side::Sell, OrderKind::Stop, Some(7_622.0), Stage::Working),
            order("c", Side::Buy, OrderKind::Limit, Some(7_600.0), Stage::Cancelled),
            order("d", Side::Buy, OrderKind::Market, None, Stage::Pending),
        ];
        // A point a pixel.
        let to_y = |price: f64| 8_000.0 - price;
        let near = |y: f64| order_near(&orders, y, to_y).map(|order| order.basket_id.as_str());
        assert_eq!(near(380.0), Some("a"));
        assert_eq!(near(379.5), Some("a"));
        assert_eq!(near(378.4), Some("b"), "nearer the stop");
        assert_eq!(near(386.0), Some("a"), "six pixels is near");
        assert_eq!(near(386.5), None);
        assert_eq!(near(400.0), None, "a cancelled order is not offered");
    }

    /// Every way a chart cannot take an order, in the words the menu shows.
    #[test]
    fn an_order_target_says_why_it_cannot_be_had() {
        use omacharts_engine::InstrumentKind;
        let view = View {
            logins: vec![test_login("t", "Rithmic Test", TEST_SYSTEM)],
            accounts: vec![account("t", "951")],
            ..View::default()
        };
        let es = instrument("ES", InstrumentKind::FutureRoot);
        let aapl = instrument("AAPL", InstrumentKind::Equity);
        let fake = FakeSession::new(vec![account("t", "951")], Box::new(|| {}));
        fake.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
        let session: Arc<dyn Session> = Arc::new(fake);
        let target = |live: bool, what: &omacharts_engine::Instrument| {
            order_target(&view, |_| live.then(|| session.clone()), None, Some(what))
        };
        let found = target(true, &es).unwrap();
        assert_eq!((found.contract.as_str(), found.account.id.as_str()), ("ESZ6", "951"));
        assert_eq!(target(true, &aapl).unwrap_err(), "AAPL is not tradable on Rithmic Test");
        assert_eq!(target(false, &es).unwrap_err(), "Rithmic Test not signed in");
        let nq = instrument("NQ", InstrumentKind::FutureRoot);
        assert_eq!(target(true, &nq).unwrap_err(), "Finding NQ's contract…");
        let unlisted = View { accounts: Vec::new(), ..view.clone() };
        assert_eq!(order_target(&unlisted, |_| None, None, Some(&es)).unwrap_err(), "Rithmic Test not signed in");
        assert_eq!(
            order_target(&unlisted, |_| Some(session.clone()), None, Some(&es)).unwrap_err(),
            "Rithmic Test has no accounts",
            "signed in, and the broker listed none"
        );
        assert_eq!(order_target(&View::default(), |_| None, None, Some(&es)).unwrap_err(), "No trading account");
    }

    /// Going flat in a contract cancels every order working in it, entries
    /// and exits alike, and flattens what is held; nothing in another
    /// contract or account, and nothing at all when already flat.
    #[test]
    fn going_flat_takes_every_working_order_in_the_contract_and_the_position() {
        let mut book = Book::default();
        assert!(flat_plan(&book, "t/951", "ESZ6").is_empty());
        book.orders.push(order("a", Side::Buy, OrderKind::Limit, Some(7_600.0), Stage::Working));
        book.orders.push(order("b", Side::Sell, OrderKind::Stop, Some(7_590.0), Stage::Pending));
        book.orders.push(order("c", Side::Sell, OrderKind::Limit, Some(7_640.0), Stage::Filled));
        book.orders.push(Order { contract: "NQZ6".into(), ..order("d", Side::Buy, OrderKind::Limit, Some(1.0), Stage::Working) });
        book.orders.push(Order { account: "t/952".into(), ..order("e", Side::Buy, OrderKind::Limit, Some(1.0), Stage::Working) });
        assert_eq!(flat_plan(&book, "t/951", "ESZ6"), Flat { cancel: vec!["a".into(), "b".into()], flatten: false });
        book.positions.push(Position {
            account: "t/951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            net: -2,
            avg: Some(7_612.25),
            open_pnl: None,
            tick: Some(0.25),
            point: Some(50.0),
        });
        assert!(flat_plan(&book, "t/951", "ESZ6").flatten);
        assert!(!flat_plan(&book, "t/951", "NQZ6").flatten);
    }

    /// A position is marked to a chart only while it shows that very
    /// contract on its exchange: MNQZ6 by an MNQZ6 chart, from any feed,
    /// never by a continuous MNQ, whose price may be another month's or
    /// back-adjusted, nor by another month or another market.
    #[test]
    fn a_chart_marks_a_position_only_when_it_shows_that_very_contract() {
        use omacharts_engine::InstrumentKind::FutureRoot;
        let held = Position {
            account: "t/951".into(),
            contract: "MNQZ6".into(),
            exchange: "CME".into(),
            net: 1,
            avg: Some(25_000.0),
            open_pnl: Some(0.0),
            tick: Some(0.25),
            point: Some(2.0),
        };
        let continuous = instrument("MNQ", FutureRoot);
        let march = instrument("MNQH7", FutureRoot);
        let december = instrument("MNQZ6", FutureRoot);
        let elsewhere = omacharts_engine::Instrument { exchange: Some("CBOT".into()), ..december.clone() };
        assert_eq!(charted_price(&held, [(&continuous, Some(24_990.0)), (&march, Some(25_210.0))]), None);
        assert_eq!(charted_price(&held, [(&elsewhere, Some(25_005.0))]), None, "another exchange's");
        assert_eq!(
            charted_price(&held, [(&continuous, Some(24_990.0)), (&december, None), (&december, Some(25_012.5))]),
            Some(25_012.5),
            "the chart of the contract with a price"
        );
        assert_eq!(charted_price(&held, std::iter::empty()), None);
    }
}
