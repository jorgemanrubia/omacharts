//! Sign in to trading on Rithmic Test and say what the account list
//! answered — how many accounts, and each reply's codes — placing nothing.
//! Account numbers are shown only as their length.
//!
//!     OMACHARTS_RITHMIC_LOGIN=~/.rithmic cargo run -p rithmic --example accounts

const APP: rithmic::App = rithmic::App { name: "omacharts", version: "example" };

fn main() {
    let login = match rithmic::Login::from_env(rithmic::TEST_SYSTEM, rithmic::TEST_URL) {
        Some(Ok(login)) => login,
        Some(Err(error)) => return eprintln!("{error}"),
        None => return eprintln!("skipped: {} is not set", rithmic::DEV_LOGIN),
    };
    let connection = match rithmic::connection(APP, "example", move || Ok(login.clone())) {
        Ok(connection) => connection,
        Err(error) => return eprintln!("connection: {error}"),
    };
    let trading = connection.trading(|_| {});
    std::thread::sleep(std::time::Duration::from_secs(3));
    match &trading {
        Ok(trading) => println!("accounts: {}", trading.accounts().len()),
        Err(error) => println!("trading: {error}"),
    }
    if let Some(listing) = rithmic::listing("example") {
        println!("system: {}  url: {}  replies: {}  failed: {:?}", listing.system, listing.url, listing.replies.len(), listing.failed);
        for reply in &listing.replies {
            println!(
                "  rp_code {:?} rq_handler {:?} account {:?} fcm {:?} ib {:?} error {:?}",
                reply.rp_code,
                reply.rq_handler_rp_code,
                reply.account_id.as_ref().map(|a| format!("<{} chars>", a.len())),
                reply.fcm_id,
                reply.ib_id,
                reply.error
            );
        }
    } else {
        println!("no listing noted");
    }
    rithmic::shutdown();
}
