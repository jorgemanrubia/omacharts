//! Sign in, find the ES front month, load a day of minute bars and print the
//! trades that arrive for a while.
//!
//! Reads the login from the file `OMACHARTS_RITHMIC_LOGIN` names (user ID on
//! the first line, password on the second) and signs in to Rithmic Test:
//!
//!     OMACHARTS_RITHMIC_LOGIN=~/.rithmic cargo run -p rithmic --example session -- 30

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Who Rithmic Test is told is signing in, which takes any name. The app's
/// own is set in the engine's `providers/rithmic/session.rs`.
const APP: rithmic::App = rithmic::App { name: "omacharts", version: "example" };

fn main() {
    let seconds = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let login = || {
        rithmic::Login::from_env(rithmic::TEST_SYSTEM, rithmic::TEST_URL)
            .unwrap_or_else(|| Err(rithmic::Error::NoLogin(format!("set {}", rithmic::DEV_LOGIN))))
    };
    let client = match rithmic::session(APP, login) {
        Ok(client) => client,
        Err(error) => return eprintln!("{error}"),
    };
    if let Err(error) = client.sign_in() {
        return eprintln!("{error}");
    }
    let contract = match client.front_month("ES", "CME") {
        Ok(contract) => contract,
        Err(error) => return eprintln!("{error}"),
    };
    println!("front month: {contract}");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    match client.bars(&contract, "CME", rithmic::Period::Minutes(1), now - 86_400, now) {
        Ok(bars) => println!("{} minute bars, the last {:?}", bars.len(), bars.last()),
        Err(error) => println!("bars: {error}"),
    }
    match client.bars(&contract, "CME", rithmic::Period::Days(1), now - 400 * 86_400, now) {
        Ok(bars) => println!("{} daily bars, the last {:?}", bars.len(), bars.last()),
        Err(error) => println!("daily bars: {error}"),
    }
    let watch = client.watch("ES", "CME", |live| println!("{live:?}"));
    std::thread::sleep(Duration::from_secs(seconds));
    drop(watch);
    rithmic::shutdown();
}
