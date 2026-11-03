//! Trade one contract on Rithmic Test and print what comes back: a buy
//! limit at the price given, cancelled; a market buy; then flatten, and the
//! account left with nothing held and nothing working.
//!
//! Reads the login from the file `OMACHARTS_RITHMIC_LOGIN` names (user ID on
//! the first line, password on the second), on Rithmic Test only, and does
//! nothing without one. The account number is never printed.
//!
//!     OMACHARTS_RITHMIC_LOGIN=~/.rithmic cargo run -p rithmic --example trade -- ESZ6 7400

use std::sync::mpsc;
use std::time::{Duration, Instant};

use rithmic::{Book, Kind, Order, Side, Status, Trading};

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
    let trading = match Trading::sign_in(APP, &login, move || {
        let _ = moved.send(());
    }) {
        Ok(trading) => trading,
        Err(error) => return eprintln!("{error}"),
    };
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
        if let Err(error) = trading.cancel(&account, &basket) {
            println!("cancel: {}", hide(error.to_string()));
        }
        settle(4);
        show("limit cancelled", &trading.book());
    }

    report("market buy", trading.place(&order(Side::Buy, Kind::Market, None)));
    settle(8);
    show("bought", &trading.book());

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
