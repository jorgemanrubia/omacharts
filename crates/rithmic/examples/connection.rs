//! One login, both halves of its one connection, for a while: trading
//! signed in first, then market data on the same login, ES streamed and the
//! order plant kept up, one buy limit placed far below the market and
//! cancelled. Says whether either half was lost or signed out along the
//! way, which on one connection neither should be, and leaves the account
//! with nothing working.
//!
//! Reads the login from the file `OMACHARTS_RITHMIC_LOGIN` names (user ID on
//! the first line, password on the second), on Rithmic Test only, and does
//! nothing without one. The account number is never printed. Any other
//! session on the same login is signed out by it.
//!
//!     OMACHARTS_RITHMIC_LOGIN=~/.rithmic cargo run -p rithmic --example connection -- 600 100

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rithmic::{Kind, Live, Order, Pulse, Side, Status};

/// Who Rithmic Test is told is signing in, which takes any name.
const APP: rithmic::App = rithmic::App { name: "omacharts", version: "example" };

#[derive(Default)]
struct Heard {
    trades: AtomicU64,
    watching: AtomicU64,
    data_lost: Mutex<Vec<String>>,
    trading_lost: Mutex<Vec<String>>,
    back: AtomicU64,
    last: Mutex<Option<f64>>,
}

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let below: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(100.0);
    let login = match rithmic::Login::from_env(rithmic::TEST_SYSTEM, rithmic::TEST_URL) {
        Some(Ok(login)) => login,
        Some(Err(error)) => return eprintln!("{error}"),
        None => return eprintln!("skipped: {} is not set", rithmic::DEV_LOGIN),
    };
    let heard = Arc::new(Heard::default());
    let started = Instant::now();
    let at = move || format!("{:>4}s", started.elapsed().as_secs());

    let connection = match rithmic::connection(APP, "example", move || Ok(login.clone())) {
        Ok(connection) => connection,
        Err(error) => return eprintln!("{error}"),
    };
    let pulses = heard.clone();
    let trading = match connection.trading(move |pulse| match pulse {
        Pulse::Moved => {}
        Pulse::Lost(error) => pulses.trading_lost.lock().unwrap().push(error.to_string()),
        Pulse::Back => {
            pulses.back.fetch_add(1, Ordering::Relaxed);
        }
    }) {
        Ok(trading) => trading,
        Err(error) => return eprintln!("trading: {error}"),
    };
    println!("{} trading signed in, {} account(s)", at(), trading.accounts().len());
    let Some(account) = trading.accounts().first().map(|account| account.id.clone()) else {
        connection.close();
        return eprintln!("the login lists no account");
    };
    let hide = |text: String| text.replace(&account, "<account>");

    let client = connection.data();
    if let Err(error) = client.sign_in() {
        connection.close();
        return eprintln!("data: {error}");
    }
    let contract = match client.front_month("ES", "CME") {
        Ok(contract) => contract,
        Err(error) => {
            connection.close();
            return eprintln!("front month: {error}");
        }
    };
    println!("{} data signed in on the same connection, streaming {contract}", at());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let close = client
        .bars(&contract, "CME", rithmic::Period::Minutes(1), now - 86_400, now)
        .ok()
        .and_then(|bars| bars.last().map(|bar| bar.close));
    *heard.last.lock().unwrap() = close;

    let live = heard.clone();
    let watch = client.watch("ES", "CME", move |news| match news {
        Live::Trade(trade) => {
            live.trades.fetch_add(1, Ordering::Relaxed);
            *live.last.lock().unwrap() = Some(trade.price);
        }
        Live::Last(trade) => *live.last.lock().unwrap() = Some(trade.price),
        Live::Watching { .. } => {
            live.watching.fetch_add(1, Ordering::Relaxed);
        }
        Live::Lost(error) => live.data_lost.lock().unwrap().push(error.to_string()),
    });

    let until = started + Duration::from_secs(seconds);
    let mut placed = false;
    let mut next_report = Instant::now();
    while Instant::now() < until {
        if !placed && started.elapsed() >= Duration::from_secs(30) {
            placed = true;
            match *heard.last.lock().unwrap() {
                Some(price) => place_and_cancel(&trading, &account, &contract, price - below, &hide, &at),
                None => println!("{} no price heard; no order placed", at()),
            }
        }
        if Instant::now() >= next_report {
            next_report += Duration::from_secs(60);
            println!(
                "{} trades {}, watching {}, data lost {}, trading lost {}, back {}, trading trouble {:?}",
                at(),
                heard.trades.load(Ordering::Relaxed),
                heard.watching.load(Ordering::Relaxed),
                heard.data_lost.lock().unwrap().len(),
                heard.trading_lost.lock().unwrap().len(),
                heard.back.load(Ordering::Relaxed),
                trading.trouble().map(|error| error.to_string()),
            );
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    // Nothing left working, whatever happened above.
    if let Err(error) = trading.cancel_all(&account) {
        println!("cancel all: {}", hide(error.to_string()));
    }
    std::thread::sleep(Duration::from_secs(3));
    let book = trading.book();
    let working = book.orders.iter().filter(|order| order.status.working()).count();
    let held: i32 = book.positions.iter().map(|held| held.net).sum();
    println!("{} left: {held} held, {working} working", at());

    let data_lost = heard.data_lost.lock().unwrap().clone();
    let trading_lost = heard.trading_lost.lock().unwrap().clone();
    for lost in data_lost.iter().chain(&trading_lost) {
        println!("lost: {lost}");
    }
    let ok = data_lost.is_empty()
        && trading_lost.is_empty()
        && trading.trouble().is_none()
        && heard.watching.load(Ordering::Relaxed) == 1
        && working == 0
        && held == 0;
    drop(watch);
    connection.close();
    println!("{} {}", at(), if ok { "PASS: one connection, nothing lost, nothing working" } else { "FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

/// A buy limit `price` below the market, which works and does not fill,
/// then cancelled; each step waited for on the book.
fn place_and_cancel(
    trading: &rithmic::Trading,
    account: &str,
    contract: &str,
    price: f64,
    hide: &dyn Fn(String) -> String,
    at: &dyn Fn() -> String,
) {
    let price = (price * 4.0).round() / 4.0;
    let order = Order {
        account: account.to_string(),
        contract: contract.to_string(),
        exchange: "CME".into(),
        side: Side::Buy,
        qty: 1,
        kind: Kind::Limit,
        price: Some(price),
        tag: "omacharts:example:connection".into(),
    };
    let basket = match trading.place(&order) {
        Ok(basket) => basket,
        Err(error) => return println!("{} place: {}", at(), hide(error.to_string())),
    };
    let status = |trading: &rithmic::Trading| {
        trading.book().orders.iter().find(|order| order.basket_id == basket).map(|order| order.status.clone())
    };
    let wait = |done: &dyn Fn(&Status) -> bool| {
        let until = Instant::now() + Duration::from_secs(15);
        while Instant::now() < until {
            if status(trading).as_ref().is_some_and(done) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        status(trading)
    };
    let working = wait(&|status| *status != Status::Pending);
    println!("{} buy limit {contract} @ {price}: {working:?}", at());
    if let Err(error) = trading.cancel(account, &basket) {
        println!("{} cancel: {}", at(), hide(error.to_string()));
    }
    let cancelled = wait(&|status| !status.working());
    println!("{} cancelled: {cancelled:?}", at());
}
