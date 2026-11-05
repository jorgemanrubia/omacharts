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
    let client = rithmic::connection(APP, "live", login).expect("a connection").data();
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

/// A short bracketed on Rithmic Test, the stop first and then the target,
/// as a trader does it: both are taken, both work, and they link. Printed
/// as Rithmic reports each, since what Rithmic says of a stop is what the
/// app's tabs go by.
#[test]
fn a_short_takes_a_stop_and_then_a_target() {
    let Some(login) = login() else {
        eprintln!("skipped: {} is not set", rithmic::DEV_LOGIN);
        return;
    };
    let connection = rithmic::connection(APP, "live-short", login).expect("a connection");
    let data = connection.data();
    data.sign_in().expect("signed in");
    let (heard, hearing) = mpsc::channel();
    let _watch = data.watch("ES", "CME", move |live| {
        let _ = heard.send(live);
    });
    let mut contract = None;
    let mut last = None;
    let until = std::time::Instant::now() + Duration::from_secs(60);
    while (contract.is_none() || last.is_none()) && std::time::Instant::now() < until {
        match hearing.recv_timeout(Duration::from_secs(30)) {
            Ok(Live::Watching { contract: found }) => contract = Some(found),
            Ok(Live::Trade(trade)) | Ok(Live::Last(trade)) => last = Some(trade.price),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let contract = contract.expect("the front month");
    // A quiet market says nothing: the last minute bar's close then.
    let last = last.unwrap_or_else(|| {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let bars = data.bars(&contract, "CME", Period::Minutes(1), now - 3 * 86_400, now).expect("minute bars");
        bars.last().expect("a bar").close
    });
    eprintln!("{contract} last {last}");

    let trading = connection.trading(|_| {}).expect("the trading half");
    trading.refresh().expect("refreshed");
    let account = trading.accounts().into_iter().next().expect("an account").id;
    let order = |side, kind, qty, price| rithmic::Order {
        account: account.clone(),
        contract: contract.clone(),
        exchange: "CME".into(),
        side,
        qty,
        kind,
        price,
        tag: format!("live-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()),
    };
    let wait = |what: &str, done: &dyn Fn(&rithmic::Book) -> bool| {
        let until = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < until {
            if done(&trading.book()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("{what}: not within 30s: {:#?}", trading.book());
    };
    let show = |step: &str| {
        let book = trading.book();
        eprintln!("--- {step}: positions {:?}", book.positions.iter().map(|p| (&p.contract, p.net)).collect::<Vec<_>>());
        for o in book.orders.iter().filter(|o| o.contract == contract) {
            eprintln!("    {:?} {:?} {} @ {:?} {:?} tag {:?} basket {}", o.side, o.kind, o.qty, o.price, o.status, o.tag, o.basket_id);
        }
    };
    // The account is the author's: never cancel or close what they hold.
    // Anything open or working means it is in use, and the test stands down.
    std::thread::sleep(Duration::from_secs(3));
    let book = trading.book();
    if book.positions.iter().any(|p| p.net != 0) || book.orders.iter().any(|o| o.status == rithmic::Status::Working) {
        eprintln!("skipped: the account holds a position or a working order, and they are not the test's to touch");
        rithmic::shutdown();
        return;
    }

    let sold = trading.place(&order(rithmic::Side::Sell, rithmic::Kind::Market, 1, None)).expect("sold");
    // Rithmic Test's risk desk refuses with a cancel that says why; the
    // reason is the order's, and there is nothing to bracket then.
    wait("the sale answered", &|book| {
        book.positions.iter().any(|p| p.contract == contract && p.net == -1)
            || book.orders.iter().any(|o| o.basket_id == sold && matches!(o.status, rithmic::Status::Rejected(_)))
    });
    if let Some(why) = trading.book().orders.iter().find(|o| o.basket_id == sold).and_then(|o| match &o.status {
        rithmic::Status::Rejected(why) => Some(why.clone()),
        _ => None,
    }) {
        eprintln!("skipped: Rithmic Test refused the sale: {why}");
        rithmic::shutdown();
        return;
    }
    show("short");

    let stop_at = (last / 0.25).round() * 0.25 + 5.0;
    let stop = trading.place(&order(rithmic::Side::Buy, rithmic::Kind::Stop, 1, Some(stop_at)));
    eprintln!("stop at {stop_at}: {stop:?}");
    let stop = stop.expect("the stop taken");
    wait("the stop working", &|book| book.orders.iter().any(|o| o.basket_id == stop && o.status == rithmic::Status::Working));
    show("after the stop");

    let target_at = (last / 0.25).round() * 0.25 - 5.0;
    let target = trading.place(&order(rithmic::Side::Buy, rithmic::Kind::Limit, 1, Some(target_at)));
    eprintln!("target at {target_at}: {target:?}");
    let target = target.expect("the target taken");
    wait("the target working", &|book| book.orders.iter().any(|o| o.basket_id == target && o.status == rithmic::Status::Working));
    show("after the target");
    let linked = trading.link(&account, &[target.clone(), stop.clone()]);
    eprintln!("linked: {linked:?}");
    std::thread::sleep(Duration::from_secs(2));
    show("after the link");

    // Only the test's own orders, and only the position it opened.
    for basket in [&stop, &target] {
        let _ = trading.cancel(&account, basket);
    }
    trading.flatten(&account, &contract, "CME").expect("flattened");
    wait("flat again", &|book| !book.positions.iter().any(|p| p.contract == contract && p.net != 0));
    show("flat again");
    rithmic::shutdown();
}
