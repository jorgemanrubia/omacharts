//! Look at the Sierra Chart DTC server on this machine without touching
//! anything: probe it, list a root's contracts, sign in and read the book,
//! sign out. It sends no order, cancel, modify or flatten message.
//!
//! ```sh
//! cargo run -p dtc --example look -- ES
//! ```
//!
//! An account that is not one of Sierra's simulated ones is printed without
//! its number.

use std::time::Duration;

fn shown(account: &str) -> String {
    if account.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case("sim")) { account.to_string() } else { "(live account)".into() }
}

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| "ES".into());
    let endpoint = dtc::Endpoint::default();
    let found = dtc::probe(&endpoint, Duration::from_secs(3));
    println!("probe: {found}");
    if let dtc::Probe::Ready(server) = &found {
        println!("  {server:?}");
    }

    match dtc::definitions(&endpoint, &root, Duration::from_secs(10)) {
        Ok(definitions) => {
            println!("{} contracts of {root}:", definitions.len());
            for definition in definitions {
                println!(
                    "  {} tick {} worth {} expires {:?} month {:?} multiplier {}",
                    definition.symbol,
                    definition.tick_size,
                    definition.currency_value_per_increment,
                    definition.expiration,
                    definition.contract_month(),
                    definition.price_multiplier
                );
            }
        }
        Err(error) => println!("definitions: {error}"),
    }

    match dtc::Trading::sign_in(&endpoint, "omacharts look", |_| {}) {
        Ok(trading) => {
            let book = trading.book();
            println!("signed in; synced {}", book.synced);
            for account in &book.accounts {
                println!("  account {} simulated {} disabled {}", shown(&account.id), account.simulated, account.trading_disabled);
            }
            for balance in &book.balances {
                println!(
                    "  balance {}: cash {} {:?} open {:?} day {:?} limit {:?}",
                    shown(&balance.account),
                    balance.cash,
                    balance.currency,
                    balance.open_pnl,
                    balance.day_pnl,
                    balance.daily_loss_limit
                );
            }
            println!("  {} positions, {} orders, {} fills today", book.positions.len(), book.orders.len(), book.fills.len());
            for fill in book.fills.iter().take(3) {
                println!("  fill {} {:?} {} at {}", fill.symbol, fill.side, fill.qty, fill.price);
            }
            trading.sign_out();
        }
        Err(error) => println!("sign in: {error}"),
    }
}
