//! The session against the fake server, and the session Sierra itself sent
//! on this machine, replayed from its fixture.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::fake::{Fake, Fills};
use crate::wire::{self, Framer};
use crate::*;

fn quick() -> Timing {
    Timing {
        heartbeat: Duration::from_secs(1),
        connect: Duration::from_secs(2),
        request: Duration::from_secs(2),
        backoff: Duration::from_millis(50),
        backoff_cap: Duration::from_millis(200),
    }
}

type Heard = Arc<Mutex<Vec<Pulse>>>;

fn sign_in(fake: &Fake) -> (Trading, Heard) {
    let heard: Heard = Arc::default();
    let sink = heard.clone();
    let trading = Trading::sign_in_with(&fake.endpoint(), "omacharts test", quick(), move |pulse| sink.lock().unwrap().push(pulse))
        .expect("signs in to the fake");
    (trading, heard)
}

fn eventually(within: Duration, what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !check() {
        assert!(Instant::now() < deadline, "never happened: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn order(account: &str, symbol: &str, side: Side, qty: u32, kind: OrderKind, price1: Option<f64>) -> NewOrder {
    NewOrder {
        account: account.into(),
        symbol: symbol.into(),
        exchange: None,
        side,
        qty,
        kind,
        price1,
        price2: None,
        time_in_force: TimeInForce::Day,
        tag: Some("from the chart".into()),
    }
}

const ES: &str = "ESZ26_FUT_CME";

fn stocked() -> crate::fake::Builder {
    Fake::builder()
        .account("Sim1")
        .account("LIVE1")
        .balance("LIVE1", 25_000.0, "USD")
        .pnl("LIVE1", 125.0, -40.0)
        .position("Sim1", ES, 2, 7650.25)
        .open_pnl(62.5)
        .order("Sim1", ES, Side::Sell, 2, OrderKind::Limit, 7700.0)
}

#[test]
fn probing_a_port_nobody_listens_on_finds_nothing() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let endpoint = Endpoint { port, ..Endpoint::default() };
    let found = probe(&endpoint, Duration::from_secs(1));
    assert_eq!(found, Probe::Nothing(format!("127.0.0.1:{port}")));
    assert_eq!(found.to_string(), format!("Sierra Chart's DTC server isn't listening on 127.0.0.1:{port}"));
}

#[test]
fn probing_something_that_answers_garbage_finds_it_is_not_dtc() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 16];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
        }
    });
    let found = probe(&Endpoint { port, ..Endpoint::default() }, Duration::from_secs(1));
    assert!(matches!(found, Probe::NotDtc(_)), "{found:?}");
    assert_eq!(found.to_string(), "Something is listening there, but it isn't a DTC server");
}

#[test]
fn probing_a_server_that_refuses_the_logon_says_its_words() {
    let fake = Fake::builder().refuse_logon("Invalid username or password").start();
    let found = probe(&fake.endpoint(), Duration::from_secs(1));
    assert_eq!(found, Probe::Refused("Invalid username or password".into()));
}

#[test]
fn probing_a_server_ready_to_trade_reads_its_logon_and_logs_off() {
    let fake = Fake::builder().start();
    let Probe::Ready(server) = probe(&fake.endpoint(), Duration::from_secs(1)) else { panic!("not ready") };
    assert_eq!(server.name, "Fake DTC Server");
    assert!(server.trading && server.oco && server.brackets);
    assert_eq!(server.protocol, 8);
    assert_eq!(server.encoding, Encoding::Json);
    assert_eq!(server.service(), Some("fake"));
    eventually(Duration::from_secs(1), "the probe logs off", || !fake.received_of(wire::LOGOFF).is_empty());
    let asked = fake.received_of(wire::ENCODING_REQUEST);
    assert_eq!(asked[0].int("Encoding"), Some(2), "JSON is asked for");
    assert!(fake.received().iter().all(|message| !matches!(message.kind(), 300..=400 | 200..=210)), "a probe asks for nothing");
}

#[test]
fn probing_a_server_that_will_not_trade_says_to_allow_trading() {
    let fake = Fake::builder().trading_unsupported().start();
    let found = probe(&fake.endpoint(), Duration::from_secs(1));
    assert!(found.to_string().starts_with("Trading isn't allowed — turn on Allow Trading"), "{found}");
}

#[test]
fn a_logon_the_server_refuses_fails_signing_in_with_its_words() {
    let fake = Fake::builder().refuse_logon("Invalid username or password").start();
    let error = Trading::sign_in_with(&fake.endpoint(), "test", quick(), |_| {}).unwrap_err();
    assert_eq!(error, Error::Refused("Invalid username or password".into()));
}

#[test]
fn a_server_that_does_not_trade_is_refused_at_sign_in() {
    let fake = Fake::builder().account("Sim1").trading_unsupported().start();
    let error = Trading::sign_in_with(&fake.endpoint(), "test", quick(), |_| {}).unwrap_err();
    assert!(matches!(&error, Error::Refused(why) if why.contains("Allow Trading")), "{error:?}");
}

#[test]
fn signing_in_fills_the_book_with_accounts_balances_positions_and_working_orders() {
    let fake = stocked().start();
    let (trading, _) = sign_in(&fake);
    let book = trading.book();
    assert!(book.synced);
    assert_eq!(book.accounts.iter().map(|account| account.id.as_str()).collect::<Vec<_>>(), ["Sim1", "LIVE1"]);
    assert!(book.accounts[0].simulated && !book.accounts[1].simulated);
    assert_eq!(trading.balance("Sim1"), None, "a simulated account has no balance");
    let balance = trading.balance("LIVE1").unwrap();
    assert_eq!((balance.cash, balance.currency.as_deref(), balance.open_pnl, balance.day_pnl), (25_000.0, Some("USD"), Some(125.0), Some(-40.0)));
    assert_eq!(balance.daily_loss_limit, Some(-1500.0));
    assert_eq!(book.positions.len(), 1);
    let position = &book.positions[0];
    assert_eq!((position.symbol.as_str(), position.qty, position.avg_price, position.open_pnl), (ES, 2, Some(7650.25), Some(62.5)));
    assert_eq!(book.orders.len(), 1);
    let working = &book.orders[0];
    assert_eq!((working.server_id.as_deref(), working.side, working.qty, working.price1), (Some("1000"), Side::Sell, 2, Some(7700.0)));
    assert_eq!(working.status, Status::Working);
    for kind in [wire::ACCOUNT_BALANCE_REQUEST, wire::CURRENT_POSITIONS_REQUEST, wire::OPEN_ORDERS_REQUEST, wire::HISTORICAL_ORDER_FILLS_REQUEST] {
        let asked: Vec<String> = fake.received_of(kind).iter().filter_map(|message| message.text("TradeAccount")).collect();
        assert_eq!(asked, ["Sim1", "LIVE1"], "each account is asked of with {kind}");
    }
    trading.sign_out();
}

#[test]
fn a_heartbeat_goes_out_at_the_interval_asked_for() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, heard) = sign_in(&fake);
    let logon = &fake.received_of(wire::LOGON_REQUEST)[0];
    assert_eq!(logon.int("HeartbeatIntervalInSeconds"), Some(1));
    assert_eq!(logon.text("ClientName").as_deref(), Some("omacharts test"));
    std::thread::sleep(Duration::from_millis(2300));
    let beats = fake.received_of(wire::HEARTBEAT).len();
    assert!((2..=3).contains(&beats), "{beats} heartbeats in 2.3 seconds");
    // Heartbeats and nothing else, as a closed market is: never lost.
    assert!(!heard.lock().unwrap().iter().any(|pulse| matches!(pulse, Pulse::Lost(_))));
    assert_eq!(fake.logons(), 1);
    trading.sign_out();
}

#[test]
fn a_server_gone_silent_is_noticed_and_reconnected_once_it_answers() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, heard) = sign_in(&fake);
    fake.set_silent(true);
    eventually(Duration::from_secs(4), "the silence is noticed", || {
        heard.lock().unwrap().iter().any(|pulse| matches!(pulse, Pulse::Lost(Error::Unreachable(why)) if why.contains("stopped answering")))
    });
    assert!(trading.trouble().is_some());
    fake.set_silent(false);
    fake.drop_connections();
    eventually(Duration::from_secs(4), "it comes back", || heard.lock().unwrap().contains(&Pulse::Back));
    assert_eq!(trading.trouble(), None);
    trading.sign_out();
}

#[test]
fn an_order_placed_is_acknowledged_then_filled_in_parts_and_the_book_follows() {
    let fake = Fake::builder().account("Sim1").fills(Fills::InParts(1)).market_price(7651.0).start();
    let (trading, heard) = sign_in(&fake);
    heard.lock().unwrap().clear();
    let id = trading.place(&order("Sim1", ES, Side::Buy, 3, OrderKind::Market, None)).unwrap();
    let sent = &fake.received_of(wire::SUBMIT_NEW_SINGLE_ORDER)[0];
    assert_eq!(sent.text("ClientOrderID").as_deref(), Some(id.as_str()));
    assert_eq!((sent.int("OrderType"), sent.int("BuySell"), sent.int("Quantity"), sent.int("TimeInForce")), (Some(1), Some(1), Some(3), Some(1)));
    assert_eq!(sent.text("FreeFormText").as_deref(), Some("from the chart"));
    assert!(id.len() < 32);
    eventually(Duration::from_secs(2), "it fills", || trading.book().orders.iter().any(|order| order.is(&id) && order.status == Status::Filled));
    let book = trading.book();
    let placed = book.orders.iter().find(|order| order.is(&id)).unwrap();
    assert_eq!((placed.filled, placed.avg_fill_price, placed.tag.as_deref()), (3, Some(7651.0), Some("from the chart")));
    assert!(placed.server_id.is_some());
    assert_eq!(book.fills.iter().filter(|fill| fill.server_id == placed.server_id).map(|fill| fill.qty).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(book.positions[0].qty, 3);
    assert!(heard.lock().unwrap().iter().filter(|pulse| **pulse == Pulse::Moved).count() >= 3);
    trading.sign_out();
}

#[test]
fn a_rejected_order_s_words_reach_both_the_caller_and_the_order() {
    let fake = Fake::builder().account("Sim1").reject_orders("Order quantity exceeds the maximum").start();
    let (trading, _) = sign_in(&fake);
    let error = trading.place(&order("Sim1", ES, Side::Buy, 50, OrderKind::Limit, Some(7600.0))).unwrap_err();
    assert_eq!(error, Error::Rejected("Order quantity exceeds the maximum".into()));
    let book = trading.book();
    assert_eq!(book.orders[0].status, Status::Rejected("Order quantity exceeds the maximum".into()));
    assert!(!book.orders[0].status.working());
    trading.sign_out();
}

#[test]
fn an_order_with_no_price_where_it_needs_one_is_not_sent() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, _) = sign_in(&fake);
    assert!(matches!(trading.place(&order("Sim1", ES, Side::Buy, 1, OrderKind::Limit, None)), Err(Error::Rejected(_))));
    assert!(fake.received_of(wire::SUBMIT_NEW_SINGLE_ORDER).is_empty());
    trading.sign_out();
}

#[test]
fn cancelling_names_the_order_by_both_its_ids_and_its_account() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, _) = sign_in(&fake);
    let id = trading.place(&order("Sim1", ES, Side::Buy, 1, OrderKind::Limit, Some(7600.0))).unwrap();
    trading.cancel(&id).unwrap();
    let sent = &fake.received_of(wire::CANCEL_ORDER)[0];
    let server_id = fake.order(&id).unwrap().server_id;
    assert_eq!(sent.text("ServerOrderID"), Some(server_id.clone()));
    assert_eq!(sent.text("ClientOrderID"), Some(id.clone()));
    assert_eq!(sent.text("TradeAccount").as_deref(), Some("Sim1"));
    assert_eq!(trading.book().orders.iter().find(|order| order.is(&id)).unwrap().status, Status::Cancelled);
    assert!(matches!(trading.cancel(&server_id), Err(Error::Rejected(_))), "a cancelled order is not cancelled twice");
    trading.sign_out();
}

#[test]
fn modifying_sends_a_cancel_replace_with_the_new_price_and_quantity() {
    let fake = Fake::builder().account("Sim1").order("Sim1", ES, Side::Sell, 2, OrderKind::Limit, 7700.0).start();
    let (trading, _) = sign_in(&fake);
    trading.modify("1000", Some(7710.5), None).unwrap();
    let sent = &fake.received_of(wire::CANCEL_REPLACE_ORDER)[0];
    assert_eq!(sent.text("ServerOrderID").as_deref(), Some("1000"));
    assert_eq!((sent.number("Price1"), sent.int("Price1IsSet"), sent.int("Price2IsSet"), sent.int("Quantity")), (Some(7710.5), Some(1), Some(0), Some(2)));
    trading.modify("1000", None, Some(5)).unwrap();
    let sent = &fake.received_of(wire::CANCEL_REPLACE_ORDER)[1];
    assert_eq!((sent.int("Price1IsSet"), sent.int("Quantity")), (Some(0), Some(5)));
    let moved = trading.book().orders.into_iter().find(|order| order.is("1000")).unwrap();
    assert_eq!((moved.price1, moved.qty), (Some(7710.5), 5));
    trading.sign_out();
}

#[test]
fn a_market_order_has_no_price_to_move() {
    let fake = Fake::builder().account("Sim1").order("Sim1", ES, Side::Sell, 2, OrderKind::Market, 0.0).start();
    let (trading, _) = sign_in(&fake);
    assert!(matches!(trading.modify("1000", Some(7710.5), None), Err(Error::Rejected(_))));
    assert!(fake.received_of(wire::CANCEL_REPLACE_ORDER).is_empty());
    trading.sign_out();
}

/// A long's stop and target, as one pair.
fn exits(qty: u32) -> [NewOrder; 2] {
    let mut stop = order("Sim1", ES, Side::Sell, qty, OrderKind::Stop, Some(7600.0));
    stop.tag = Some("omacharts:1".into());
    let mut target = order("Sim1", ES, Side::Sell, qty, OrderKind::Limit, Some(7700.0));
    target.tag = Some("omacharts:2".into());
    [stop, target]
}

#[test]
fn a_pair_goes_out_as_one_oco_submission_and_each_keeps_its_own_tag() {
    let fake = Fake::builder().account("Sim1").position("Sim1", ES, 2, 7650.0).start();
    let (trading, _) = sign_in(&fake);
    let [stop, target] = trading.place_pair(&exits(2)).unwrap();
    assert!(fake.received_of(wire::SUBMIT_NEW_SINGLE_ORDER).is_empty(), "sent one by one");
    let sent = fake.received_of(wire::SUBMIT_NEW_OCO_ORDER);
    assert_eq!(sent.len(), 1);
    let sent = &sent[0];
    assert_eq!((sent.text("ClientOrderID_1"), sent.text("ClientOrderID_2")), (Some(stop.clone()), Some(target.clone())));
    assert_eq!((sent.int("OrderType_1"), sent.int("OrderType_2")), (Some(3), Some(2)));
    assert_eq!((sent.int("BuySell_1"), sent.int("BuySell_2")), (Some(2), Some(2)));
    assert_eq!((sent.number("Price1_1"), sent.number("Price1_2")), (Some(7600.0), Some(7700.0)));
    assert_eq!((sent.int("Quantity_1"), sent.int("Quantity_2")), (Some(2), Some(2)));
    assert_eq!(sent.int("PartialFillHandling"), Some(wire::PARTIAL_FILL_HANDLING_REDUCE_QUANTITY));
    assert_eq!(sent.text("ParentTriggerClientOrderID"), None, "no parent: the position is already held");
    assert_eq!(sent.text("TradeAccount").as_deref(), Some("Sim1"));
    let book = trading.book();
    let of = |id: &str| book.orders.iter().find(|order| order.is(id)).unwrap().clone();
    let (stop, target) = (of(&stop), of(&target));
    assert_eq!((stop.status.clone(), target.status.clone()), (Status::Working, Status::Working));
    assert_eq!((stop.tag.as_deref(), target.tag.as_deref()), (Some("omacharts:1"), Some("omacharts:2")));
    assert_eq!(stop.oco_linked, target.server_id, "each knows the other");
    assert_eq!(target.oco_linked, stop.server_id);
    trading.sign_out();
}

#[test]
fn a_part_of_one_of_a_pair_filled_comes_off_the_other_and_the_rest_cancels_it() {
    let fake = Fake::builder().account("Sim1").position("Sim1", ES, 3, 7650.0).start();
    let (trading, _) = sign_in(&fake);
    let [stop, target] = trading.place_pair(&exits(3)).unwrap();
    fake.fill(&target, 1, 7700.0);
    let qty = |id: &str| trading.book().orders.iter().find(|order| order.is(id)).map(|order| (order.qty, order.status.clone()));
    eventually(Duration::from_secs(2), "the stop is for two", || qty(&stop) == Some((2, Status::Working)));
    fake.fill(&target, 2, 7700.0);
    eventually(Duration::from_secs(2), "the stop is cancelled", || qty(&stop).is_some_and(|(_, status)| status == Status::Cancelled));
    assert!(trading.book().positions.iter().all(|held| held.qty == 0), "never more closed than was held");
    trading.sign_out();
}

#[test]
fn cancelling_one_of_a_pair_cancels_the_other() {
    let fake = Fake::builder().account("Sim1").position("Sim1", ES, 1, 7650.0).start();
    let (trading, _) = sign_in(&fake);
    let [stop, target] = trading.place_pair(&exits(1)).unwrap();
    trading.cancel(&target).unwrap();
    let status = |id: &str| trading.book().orders.iter().find(|order| order.is(id)).map(|order| order.status.clone());
    eventually(Duration::from_secs(2), "the stop goes too", || status(&stop) == Some(Status::Cancelled));
    trading.sign_out();
}

#[test]
fn a_pair_is_rejected_whole_in_the_server_s_words() {
    let fake = Fake::builder().account("Sim1").oco_unsupported().start();
    let (trading, _) = sign_in(&fake);
    assert!(!trading.server().oco, "the logon says no OCO");
    assert_eq!(trading.place_pair(&exits(1)), Err(Error::Rejected("OCO orders are not supported".into())));
    let book = trading.book();
    assert_eq!(book.orders.len(), 2);
    assert!(book.orders.iter().all(|order| !order.status.working()));
    trading.sign_out();

    let fake = Fake::builder().account("Sim1").reject_orders("Loss limit exceeded").start();
    let (trading, _) = sign_in(&fake);
    assert!(trading.server().oco);
    assert_eq!(trading.place_pair(&exits(1)), Err(Error::Rejected("Loss limit exceeded".into())));
    trading.sign_out();
}

#[test]
fn a_pair_on_two_contracts_is_not_sent() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, _) = sign_in(&fake);
    let mut pair = exits(1);
    pair[1].symbol = "NQZ26_FUT_CME".into();
    assert!(matches!(trading.place_pair(&pair), Err(Error::Rejected(_))));
    assert!(fake.received_of(wire::SUBMIT_NEW_OCO_ORDER).is_empty());
    trading.sign_out();
}

#[test]
fn cancel_all_cancels_each_order_working_on_the_account_and_no_other() {
    let fake = Fake::builder()
        .account("Sim1")
        .account("Sim2")
        .order("Sim1", ES, Side::Sell, 1, OrderKind::Limit, 7700.0)
        .order("Sim1", ES, Side::Buy, 1, OrderKind::Stop, 7600.0)
        .order("Sim2", ES, Side::Buy, 1, OrderKind::Limit, 7500.0)
        .start();
    let (trading, _) = sign_in(&fake);
    trading.cancel_all("Sim1").unwrap();
    let mut cancelled: Vec<String> = fake.received_of(wire::CANCEL_ORDER).iter().filter_map(|message| message.text("ServerOrderID")).collect();
    cancelled.sort();
    assert_eq!(cancelled, ["1000", "1001"]);
    let book = trading.book();
    assert!(book.orders.iter().filter(|order| order.account == "Sim1").all(|order| order.status == Status::Cancelled));
    assert!(book.orders.iter().any(|order| order.account == "Sim2" && order.status == Status::Working));
    trading.sign_out();
}

#[test]
fn flattening_cancels_what_works_on_the_contract_then_closes_what_is_held() {
    let fake = Fake::builder()
        .account("Sim1")
        .position("Sim1", ES, 2, 7650.0)
        .order("Sim1", ES, Side::Sell, 2, OrderKind::Limit, 7700.0)
        .market_price(7660.0)
        .start();
    let (trading, _) = sign_in(&fake);
    trading.flatten("Sim1", ES).unwrap();
    assert_eq!(fake.received_of(wire::CANCEL_ORDER)[0].text("ServerOrderID").as_deref(), Some("1000"));
    let flatten = &fake.received_of(wire::SUBMIT_FLATTEN_POSITION_ORDER)[0];
    assert_eq!((flatten.text("Symbol").as_deref(), flatten.text("TradeAccount").as_deref()), (Some(ES), Some("Sim1")));
    eventually(Duration::from_secs(2), "it is flat", || trading.book().positions.is_empty());
    trading.sign_out();
}

#[test]
fn flattening_nothing_held_sends_nothing() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, _) = sign_in(&fake);
    trading.flatten("Sim1", ES).unwrap();
    assert!(fake.received_of(wire::SUBMIT_FLATTEN_POSITION_ORDER).is_empty());
    trading.sign_out();
}

#[test]
fn history_asks_for_an_account_s_fills_since_a_time_and_keeps_to_the_range() {
    let day = 86_400;
    let now = crate::now();
    let fake = Fake::builder()
        .account("Sim1")
        .fill("Sim1", ES, Side::Buy, 1, 7600.0, now - 3 * day)
        .fill("Sim1", ES, Side::Sell, 1, 7610.0, now - 2 * day)
        .fill("Sim1", ES, Side::Buy, 2, 7620.0, now - 10)
        .start();
    let (trading, _) = sign_in(&fake);
    assert_eq!(trading.book().fills.len(), 1, "the book has today's");
    let fills = trading.history("Sim1", now - 4 * day, now - day).unwrap();
    assert_eq!(fills.iter().map(|fill| fill.price).collect::<Vec<_>>(), [7600.0, 7610.0]);
    let asked = fake.received_of(wire::HISTORICAL_ORDER_FILLS_REQUEST).pop().unwrap();
    assert_eq!((asked.text("TradeAccount").as_deref(), asked.int("StartDateTime")), (Some("Sim1"), Some(now - 4 * day)));
    assert!(asked.int("NumberOfDays").unwrap() >= 4);
    assert_eq!(trading.book().fills.len(), 1, "history is not added to the book");
    trading.sign_out();
}

#[test]
fn a_dropped_connection_is_lost_then_back_with_the_book_asked_for_again() {
    let fake = stocked().start();
    let (trading, heard) = sign_in(&fake);
    // What changes while the session is away, telling nobody.
    fake.forget_order("1000");
    fake.set_position("Sim1", ES, -1, 7690.0);
    fake.drop_connections();
    eventually(Duration::from_secs(3), "it comes back", || heard.lock().unwrap().contains(&Pulse::Back));
    let pulses = heard.lock().unwrap().clone();
    let lost = pulses.iter().position(|pulse| matches!(pulse, Pulse::Lost(_))).expect("lost first");
    let back = pulses.iter().position(|pulse| *pulse == Pulse::Back).unwrap();
    assert!(lost < back);
    assert_eq!(fake.logons(), 2);
    let book = trading.book();
    assert!(book.synced);
    assert!(book.orders.iter().all(|order| !order.status.working()), "the order that went is not shown working");
    assert_eq!(book.positions.iter().map(|position| position.qty).collect::<Vec<_>>(), [-1]);
    assert!(fake.received_of(wire::SUBMIT_NEW_SINGLE_ORDER).is_empty(), "nothing is resent");
    trading.sign_out();
}

#[test]
fn placing_while_the_connection_is_down_fails_at_once() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, heard) = sign_in(&fake);
    fake.set_refuse_logon(Some("held off"));
    fake.drop_connections();
    eventually(Duration::from_secs(2), "it is lost", || heard.lock().unwrap().iter().any(|pulse| matches!(pulse, Pulse::Lost(_))));
    let started = Instant::now();
    assert!(trading.place(&order("Sim1", ES, Side::Buy, 1, OrderKind::Market, None)).is_err());
    assert!(started.elapsed() < Duration::from_millis(500));
    trading.sign_out();
}

#[test]
fn a_logon_refused_on_reconnecting_is_not_retried() {
    let fake = Fake::builder().account("Sim1").start();
    let (trading, heard) = sign_in(&fake);
    fake.set_refuse_logon(Some("Invalid username or password"));
    fake.drop_connections();
    eventually(Duration::from_secs(2), "the refusal is heard", || {
        heard.lock().unwrap().iter().any(|pulse| matches!(pulse, Pulse::Lost(Error::Refused(_))))
    });
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(fake.logons(), 2, "one sign-in, one refused retry, and no more");
    assert_eq!(trading.trouble(), Some(Error::Refused("Invalid username or password".into())));
    trading.sign_out();
}

#[test]
fn signing_out_logs_off_and_returns_promptly() {
    let fake = stocked().start();
    let (trading, _) = sign_in(&fake);
    let started = Instant::now();
    trading.sign_out();
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
    assert_eq!(fake.received_of(wire::LOGOFF).len(), 1);
    assert_eq!(trading.place(&order("Sim1", ES, Side::Buy, 1, OrderKind::Market, None)), Err(Error::Closed));
}

#[test]
fn signing_out_while_reconnecting_returns_promptly() {
    let fake = Fake::builder().account("Sim1").start();
    let mut timing = quick();
    timing.backoff = Duration::from_secs(20);
    let trading = Trading::sign_in_with(&fake.endpoint(), "test", timing, |_| {}).unwrap();
    fake.drop_connections();
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    trading.sign_out();
    assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
}

#[test]
fn prices_travel_in_the_contract_s_units_and_are_real_in_the_book() {
    // Sierra's futures service: CME prices in hundredths, multiplier 0.01.
    let fake = Fake::builder()
        .account("Sim1")
        .definition(ES, "ES", 0.25, 12.5, 1797206400, 0.009999999776482582)
        .position("Sim1", ES, 1, 765025.0)
        .order("Sim1", ES, Side::Sell, 1, OrderKind::Limit, 770000.0)
        .start();
    let (trading, _) = sign_in(&fake);
    let book = trading.book();
    assert_eq!(book.positions[0].avg_price, Some(7650.25));
    assert_eq!(book.orders[0].price1, Some(7700.0));
    let id = trading.place(&order("Sim1", ES, Side::Buy, 1, OrderKind::Limit, Some(7651.25))).unwrap();
    assert_eq!(fake.received_of(wire::SUBMIT_NEW_SINGLE_ORDER)[0].number("Price1"), Some(765125.0));
    assert_eq!(trading.book().orders.iter().find(|order| order.is(&id)).unwrap().price1, Some(7651.25));
    trading.modify(&id, Some(7652.0), None).unwrap();
    assert_eq!(fake.received_of(wire::CANCEL_REPLACE_ORDER)[0].number("Price1"), Some(765200.0));
    trading.sign_out();
}

#[test]
fn a_priced_order_for_a_contract_sierra_does_not_define_is_not_sent() {
    let fake = Fake::builder().account("Sim1").unknown_contracts_unanswered().start();
    let mut timing = quick();
    timing.request = Duration::from_millis(500);
    let trading = Trading::sign_in_with(&fake.endpoint(), "test", timing, |_| {}).unwrap();
    let error = trading.place(&order("Sim1", "NOSUCH_FUT_CME", Side::Buy, 1, OrderKind::Limit, Some(1.0))).unwrap_err();
    assert!(matches!(&error, Error::Rejected(why) if why.contains("NOSUCH_FUT_CME")), "{error:?}");
    assert!(fake.received_of(wire::SUBMIT_NEW_SINGLE_ORDER).is_empty());
    trading.sign_out();
}

#[test]
fn a_balance_pushed_lands_in_the_book_and_pulses() {
    let fake = stocked().start();
    let (trading, heard) = sign_in(&fake);
    heard.lock().unwrap().clear();
    fake.push_balance("LIVE1", 24_500.0);
    eventually(Duration::from_secs(2), "the balance moves", || trading.balance("LIVE1").is_some_and(|balance| balance.cash == 24_500.0));
    assert!(heard.lock().unwrap().contains(&Pulse::Moved));
    trading.sign_out();
}

#[test]
fn refreshing_asks_again_and_drops_what_is_no_longer_working() {
    let fake = stocked().start();
    let (trading, _) = sign_in(&fake);
    fake.forget_order("1000");
    trading.refresh().unwrap();
    assert!(trading.book().orders.is_empty());
    assert_eq!(fake.received_of(wire::OPEN_ORDERS_REQUEST).len(), 4);
    trading.sign_out();
}

#[test]
fn definitions_lists_an_underlying_s_contracts_with_each_one_s_expiry() {
    let fake = Fake::builder()
        .definition("ESZ26_FUT_CME", "ES", 0.25, 12.5, 1797206400, 0.01)
        .definition("ESH27_FUT_CME", "ES", 0.25, 12.5, 1805068800, 0.01)
        .definition("NQZ26_FUT_CME", "NQ", 0.25, 5.0, 1797206400, 0.01)
        .start();
    let found = definitions(&fake.endpoint(), "ES", Duration::from_secs(2)).unwrap();
    assert_eq!(found.iter().map(|definition| definition.symbol.as_str()).collect::<Vec<_>>(), ["ESZ26_FUT_CME", "ESH27_FUT_CME"]);
    assert_eq!(found.iter().map(|definition| definition.expiration).collect::<Vec<_>>(), [Some(1797206400), Some(1805068800)]);
    assert_eq!((found[0].tick_size, found[0].currency_value_per_increment, found[0].kind), (0.25, 12.5, SecurityKind::Future));
    assert!(definitions(&fake.endpoint(), "ZZ", Duration::from_secs(2)).unwrap().is_empty());
    let one = definition(&fake.endpoint(), "NQZ26_FUT_CME", Duration::from_secs(2)).unwrap();
    assert_eq!(one.currency_value_per_increment, 5.0);
}

#[test]
fn a_session_asks_a_definition_over_its_own_connection() {
    let fake = Fake::builder().account("Sim1").definition(ES, "ES", 0.25, 12.5, 1797206400, 0.01).start();
    let (trading, _) = sign_in(&fake);
    assert_eq!(trading.security_definition(ES).unwrap().expiration, Some(1797206400));
    trading.sign_out();
}

#[test]
fn market_data_for_cme_is_refused_as_sierra_refuses_it() {
    let fake = Fake::builder().start();
    assert_eq!(quote(&fake.endpoint(), ES, Duration::from_secs(2)), Err(Error::Rejected("Market data request not allowed".into())));
}

#[test]
fn history_reads_bars_and_carries_a_refusal() {
    let fake = Fake::builder().bars(ES, &[(1_794_000_000, 1.0, 2.0, 0.5, 1.5, 10.0), (1_794_000_060, 1.5, 2.5, 1.0, 2.0, 20.0)]).start();
    let records = history(&fake.endpoint(), ES, Interval::Seconds(60), 1_793_990_000, 0, Duration::from_secs(2)).unwrap();
    assert_eq!(records.len(), 2);
    assert!(matches!(records[1], HistoryRecord::Bar { start, close, volume, .. } if start == 1_794_000_060.0 && close == 2.0 && volume == 20.0));
    let refused = Fake::builder().refuse_history("Request is not authorized.").start();
    assert_eq!(
        history(&refused.endpoint(), ES, Interval::Tick, 0, 0, Duration::from_secs(2)),
        Err(Error::Rejected("Request is not authorized.".into()))
    );
}

#[test]
fn no_error_carries_the_password() {
    let fake = Fake::builder().refuse_logon("Invalid username or password").start();
    let endpoint = Endpoint { user: Some("trader".into()), password: Some("SECRET-PASSWORD".into()), ..fake.endpoint() };
    let refused = Trading::sign_in_with(&endpoint, "test", quick(), |_| {}).unwrap_err();
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let unreachable = Trading::sign_in_with(&Endpoint { port, ..endpoint.clone() }, "test", quick(), |_| {}).unwrap_err();
    for printed in [format!("{refused} {refused:?}"), format!("{unreachable} {unreachable:?}"), format!("{endpoint:?}")] {
        assert!(!printed.contains("SECRET-PASSWORD"), "{printed}");
    }
    let logon = &fake.received_of(wire::LOGON_REQUEST)[0];
    assert_eq!(logon.text("Password").as_deref(), Some("SECRET-PASSWORD"), "it does reach the server");
}

/// The session Sierra sent on this machine, the live account's number and
/// its holder's name replaced: every message it answered with.
const SIERRA_OPEN: &str = include_str!("../tests/fixtures/sierra-session-open.jsonl");

fn sierra_messages(fixture: &str) -> Vec<wire::Message> {
    let mut framer = Framer::default();
    let mut messages = Vec::new();
    for line in fixture.lines().filter_map(|line| line.strip_prefix("<< ")) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(0);
        messages.extend(framer.push(&bytes).expect("Sierra's own messages frame"));
    }
    messages
}

#[test]
fn the_session_sierra_sent_folds_into_a_book_of_its_accounts_balance_and_fills() {
    let messages = sierra_messages(SIERRA_OPEN);
    assert!(messages.len() > 30);
    let mut state = State::default();
    for message in &messages {
        state.fold_for_test(message);
    }
    let book = state.book();
    assert_eq!(book.accounts.iter().map(|account| account.id.as_str()).collect::<Vec<_>>(), ["LIVE1", "Sim1", "Sim2"]);
    assert_eq!(book.balances.len(), 1, "the simulated accounts have none");
    assert_eq!((book.balances[0].cash, book.balances[0].daily_loss_limit), (1852.88, Some(-1500.0)));
    assert!(book.positions.is_empty() && book.orders.is_empty());
    // The fills arrived in hundredths, before any definition said so; the
    // definition of MES would make them real.
    assert!(book.fills.iter().all(|fill| fill.symbol == "MESZ26_FUT_CME" && fill.price > 700_000.0));
    let logon = messages.iter().find(|message| message.kind() == wire::LOGON_RESPONSE).unwrap();
    let server = Server::from_logon(logon, wire::EncodingReply { protocol: 8, encoding: Encoding::Json });
    assert!(server.trading && server.oco && server.brackets && !server.historical);
    assert_eq!(server.service(), Some("sc_futures_direct.dtc.trading"));
    let definitions: Vec<Definition> = messages.iter().filter_map(Definition::from_message).collect();
    assert!(definitions.iter().any(|definition| definition.symbol == ES && definition.expiration == Some(1797206400)));
    let reject = messages.iter().find(|message| message.kind() == wire::MARKET_DATA_REJECT).unwrap();
    assert_eq!(reject.text("RejectText").as_deref(), Some("Market data request not allowed"));
}

/// The same session, recorded after CME closed for the weekend.
const SIERRA_CLOSED: &str = include_str!("../tests/fixtures/sierra-session-closed.jsonl");

#[test]
fn a_closed_market_is_heartbeats_and_the_same_answers_with_no_status_to_handle() {
    let open = sierra_messages(SIERRA_OPEN);
    let closed = sierra_messages(SIERRA_CLOSED);
    // No feed or trading status messages arrive on either side of the close.
    for status in [100, 116, 138] {
        assert!(!closed.iter().chain(&open).any(|message| message.kind() == status));
    }
    assert!(closed.iter().any(|message| message.kind() == wire::HEARTBEAT));
    let kinds = |messages: &[wire::Message]| {
        let mut kinds: Vec<u16> = messages.iter().map(wire::Message::kind).filter(|kind| *kind != wire::HEARTBEAT).collect();
        kinds.dedup();
        kinds
    };
    assert_eq!(kinds(&open), kinds(&closed));
    let mut state = State::default();
    for message in &closed {
        state.fold_for_test(message);
    }
    assert_eq!(state.book().accounts.len(), 3);
}
