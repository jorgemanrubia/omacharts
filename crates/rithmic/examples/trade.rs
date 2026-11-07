//! Trade one contract on Rithmic Test and print what comes back: a buy
//! limit at the first price given, moved a point lower, then cancelled; a
//! market buy; a sell limit at the second price and a sell stop at the
//! first, linked; the limit cancelled, which takes the linked stop with it;
//! then flatten, and the account left with nothing held and nothing
//! working.
//!
//! Reads the login from the file `OMACHARTS_RITHMIC_LOGIN` names (user ID on
//! the first line, password on the second), on Rithmic Test only, and does
//! nothing without one. The account number is never printed.
//!
//!     OMACHARTS_RITHMIC_LOGIN=~/.rithmic cargo run -p rithmic --example trade -- ESZ6 7400 7800

use std::sync::mpsc;
use std::time::{Duration, Instant};

use rithmic::{Book, Kind, Order, Side, Status};

/// Who Rithmic Test is told is signing in, which takes any name.
const APP: rithmic::App = rithmic::App { name: "omacharts", version: "example" };

fn main() {
    let contract = std::env::args().nth(1).unwrap_or_else(|| "ESZ6".into());
    let login = match rithmic::Login::from_env(rithmic::TEST_SYSTEM, rithmic::TEST_URL) {
        Some(Ok(login)) => login,
        Some(Err(error)) => return eprintln!("{error}"),
        None => return eprintln!("skipped: {} is not set", rithmic::DEV_LOGIN),
    };
    let (moved, heard) = mpsc::channel();
    let connection = match rithmic::connection(APP, "example", move || Ok(login.clone())) {
        Ok(connection) => connection,
        Err(error) => return eprintln!("{error}"),
    };
    let trading = match connection.trading(move |_| {
        let _ = moved.send(());
    }) {
        Ok(trading) => trading,
        Err(error) => return eprintln!("{error}"),
    };
    if let Err(error) = trading.refresh() {
        eprintln!("refresh: {error}");
    }
    let Some(account) = trading.accounts().first().map(|account| account.id.clone()) else {
        trading.sign_out();
        return eprintln!("the login lists no account");
    };
    let hide = |text: String| text.replace(&account, "<account>");
    let settle = |seconds: u64| {
        let until = Instant::now() + Duration::from_secs(seconds);
        while let Some(left) = until.checked_duration_since(Instant::now()) {
            if heard.recv_timeout(left).is_err() {
                break;
            }
        }
    };
    let show = |when: &str, book: &Book| {
        println!("== {when}");
        for money in &book.accounts {
            println!("  account balance {:?} realized {:?} open {:?}", money.balance, money.realized_day, money.unrealized);
        }
        for held in &book.positions {
            println!("  position {} {:+} @ {:?} open P&L {:?}", held.contract, held.net, held.avg, held.open_pnl);
        }
        for order in book.orders.iter().take(4) {
            println!("  {}", hide(format!(
                "order {} {:?} {:?} {} {} @ {:?} {:?} fill {:?}",
                order.basket_id, order.side, order.kind, order.qty, order.contract, order.price, order.status, order.fill_price
            )));
        }
    };
    settle(3);
    show("signed in", &trading.book());

    let order = |side, kind, price| Order {
        account: account.clone(),
        contract: contract.clone(),
        exchange: "CME".into(),
        side,
        qty: 1,
        kind,
        price,
        tag: format!("omacharts:example:{kind:?}"),
    };
    let report = |what: &str, outcome: Result<String, rithmic::Error>| match outcome {
        Ok(basket) => {
            println!("{what}: taken as {basket}");
            Some(basket)
        }
        Err(error) => {
            println!("{what}: {}", hide(error.to_string()));
            None
        }
    };

    // Below the market and inside the exchange's bands, which refuse a
    // price too far from it: it works, and does not fill.
    let below = std::env::args().nth(2).and_then(|price| price.parse::<f64>().ok());
    if let Some(price) = below
        && let Some(basket) = report("buy limit", trading.place(&order(Side::Buy, Kind::Limit, Some(price))))
    {
        settle(4);
        show("limit working", &trading.book());
        match trading.modify(&account, &basket, price - 1.0) {
            Ok(()) => println!("modify: taken"),
            Err(error) => println!("modify: {}", hide(error.to_string())),
        }
        settle(4);
        show("limit moved a point lower", &trading.book());
        if let Err(error) = trading.cancel(&account, &basket) {
            println!("cancel: {}", hide(error.to_string()));
        }
        settle(4);
        show("limit cancelled", &trading.book());
    }

    report("market buy", trading.place(&order(Side::Buy, Kind::Market, None)));
    settle(8);
    show("bought", &trading.book());

    // A target above and a stop below, linked: cancelling the target
    // cancels the stop on Rithmic's side.
    let above = std::env::args().nth(3).and_then(|price| price.parse::<f64>().ok());
    if let (Some(below), Some(above)) = (below, above) {
        let target = report("sell limit", trading.place(&order(Side::Sell, Kind::Limit, Some(above))));
        let stop = report("sell stop", trading.place(&order(Side::Sell, Kind::Stop, Some(below))));
        settle(4);
        if let (Some(target), Some(stop)) = (target, stop) {
            match trading.link(&account, &[target.clone(), stop.clone()]) {
                Ok(()) => println!("link: taken"),
                Err(error) => println!("link: {}", hide(error.to_string())),
            }
            settle(3);
            show("target and stop working, linked", &trading.book());
            if let Err(error) = trading.cancel(&account, &target) {
                println!("cancel target: {}", hide(error.to_string()));
            }
            settle(6);
            let book = trading.book();
            show("target cancelled", &book);
            let status = book.orders.iter().find(|order| order.basket_id == stop).map(|order| order.status.clone());
            println!("the linked stop: {status:?}");
        }
    }

    if let Err(error) = trading.flatten(&account, &contract, "CME") {
        println!("flatten: {}", hide(error.to_string()));
    }
    settle(8);
    let book = trading.book();
    show("flattened", &book);
    let working = book.orders.iter().filter(|order| order.status.working()).count();
    let held: i32 = book.positions.iter().filter(|held| held.contract == contract).map(|held| held.net).sum();
    println!("left: {held} held, {working} working");
    if book.orders.iter().any(|order| matches!(order.status, Status::Rejected(_))) {
        println!("something was rejected; see above");
    }
    trading.sign_out();
}
