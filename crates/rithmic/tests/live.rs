//! Against Rithmic Test, with a developer's own login, and skipped without
//! one: `OMACHARTS_RITHMIC_LOGIN` names the file. CI never has one, and no
//! login is ever read from the repo.
//!
//!     OMACHARTS_RITHMIC_LOGIN=~/.rithmic cargo test -p rithmic --test live -- --nocapture

use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rithmic::{Error, Live, Login, Period};

fn login() -> Option<impl Fn() -> Result<Login, Error> + Send + Sync + 'static> {
    std::env::var_os(rithmic::DEV_LOGIN).filter(|path| !path.is_empty())?;
    Some(|| Login::from_env(rithmic::TEST_SYSTEM, rithmic::TEST_URL).unwrap_or(Err(Error::Closed)))
}

/// Who Rithmic Test is told is signing in, which takes any name.
const APP: rithmic::App = rithmic::App { name: "omacharts", version: "test" };

/// One session, all the way through: the front month, its bars, its trades,
/// a connection cut from underneath, and the session back by itself.
#[test]
fn a_session_signs_in_serves_bars_and_survives_a_drop() {
    let Some(login) = login() else {
        eprintln!("skipped: {} is not set", rithmic::DEV_LOGIN);
        return;
    };
    let client = rithmic::session(APP, login).expect("a session");
    client.sign_in().expect("signed in");
    assert!(rithmic::running());

    let contract = client.front_month("ES", "CME").expect("a front month");
    assert!(contract.starts_with("ES"), "{contract}");
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let minutes = client.bars(&contract, "CME", Period::Minutes(1), now - 3 * 86_400, now).expect("minute bars");
    let days = client.bars(&contract, "CME", Period::Days(1), now - 3_650 * 86_400, now).expect("daily bars");
    eprintln!(
        "{contract}: {} minute bars from {:?}, {} daily bars from {:?}",
        minutes.len(),
        minutes.first().map(|bar| bar.start),
        days.len(),
        days.first().map(|bar| bar.start)
    );
    assert!(!minutes.is_empty());
    assert!(minutes.windows(2).all(|pair| pair[0].start < pair[1].start));

    let (heard, hearing) = mpsc::channel();
    let watch = client.watch("ES", "CME", move |live| {
        let _ = heard.send(live);
    });
    let first = hearing.recv_timeout(Duration::from_secs(30)).expect("watching");
    assert_eq!(first, Live::Watching { contract: contract.clone() });

    client.sever();
    let mut lost = false;
    let back = loop {
        match hearing.recv_timeout(Duration::from_secs(60)).expect("news of the drop") {
            Live::Lost(error) => {
                eprintln!("lost: {error}");
                lost = true;
            }
            Live::Watching { contract } if lost => break contract,
            _ => {}
        }
    };
    assert_eq!(back, contract);
    // And it answers requests again.
    client.front_month("ES", "CME").expect("a front month after the drop");

    drop(watch);
    rithmic::shutdown();
    assert!(!rithmic::running());
    assert_eq!(client.front_month("NQ", "CME"), Err(Error::Closed));
}
