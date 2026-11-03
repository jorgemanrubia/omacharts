//! Run a session for a while and print what it reports.
//!
//! Reads the user from the first line of `~/.rithmic` and the password from
//! the second, and signs in to Rithmic Test:
//!
//!     cargo run -p rithmic --example session -- 30

use std::time::Duration;

fn main() {
    let seconds = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let path = std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join(".rithmic");
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines();
    let credentials = rithmic::Credentials {
        user: lines.next().unwrap_or_default().to_string(),
        password: lines.next().unwrap_or_default().to_string(),
        system: rithmic::TEST_SYSTEM.to_string(),
        url: rithmic::TEST_URL.to_string(),
    };
    let session = rithmic::connect(credentials, |event| println!("{event:?}"));
    std::thread::sleep(Duration::from_secs(seconds));
    drop(session);
    std::thread::sleep(Duration::from_secs(2));
}
