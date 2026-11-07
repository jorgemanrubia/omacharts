//! What the Log says, and the one place anything is put in it from the app.
//!
//! A raw error is a feed's or a system's words: an HTTP status, an SQLite
//! code, a broker's refusal. Each is turned here into a sentence a trader
//! reads, with the raw words kept behind the entry's "Details". Nothing else
//! writes words into the log, so the voice is one and the rules hold in one
//! place:
//!
//! - who said it is a feed's or a login's **name**, never a user ID;
//! - no password, token or user ID is ever written: every login's user ID is
//!   concealed from the log as soon as it is known ([`conceal_logins`]);
//! - a routine success — a bar fetched, a chart opened — is not news and is
//!   not logged.

use omacharts_engine::log::{self, Entry};
use omacharts_engine::{FetchFailure, Timeframe};

use crate::trading::{self, Login, Order, OrderRequest, Stage};

/// Put `entry` in the log.
pub fn say(entry: Entry) {
    log::add(entry);
}

/// Keep every login's user ID out of the log from now on, whatever a feed or
/// a broker repeats back.
pub fn conceal_logins(logins: &[Login]) {
    for login in logins {
        log::global().conceal(&login.user);
    }
}

/// The first sentence of something a provider said, for a line with room
/// for one; the whole of it goes in the details.
fn gist(said: &str) -> &str {
    let said = said.trim();
    let line = said.lines().next().unwrap_or(said);
    match line.find(". ") {
        Some(end) => &line[..end],
        None => line.trim_end_matches('.'),
    }
}

/// Which failures need somebody to look: a reply nobody can read, or a disk
/// that will not open. The rest fix themselves or are on the chart already.
fn serious(failure: FetchFailure) -> bool {
    matches!(failure, FetchFailure::Garbled | FetchFailure::LocalCache)
}

/// A fetch for a chart failed: "AAPL 1D · The data feed is not answering.
/// Prices fill in when it does."
pub fn fetch_failed(feed: &str, symbol: &str, timeframe: Timeframe, failure: FetchFailure) -> Entry {
    let text = format!("{symbol} {} · {}", timeframe.label(), failure.message());
    match serious(failure) {
        true => Entry::error(feed, text),
        false => Entry::warning(feed, text),
    }
}

/// The watchlist's prices did not come.
pub fn closes_failed(feed: &str, failure: FetchFailure) -> Entry {
    let text = format!("Watchlist prices · {}", failure.message());
    match serious(failure) {
        true => Entry::error(feed, text),
        false => Entry::warning(feed, text),
    }
}

/// A live chart's stream stopped.
pub fn stream_lost(feed: &str, symbol: &str, timeframe: Timeframe, why: FetchFailure) -> Entry {
    Entry::warning(feed, format!("Lost {symbol} {} · {}", timeframe.label(), why.message()))
}

/// The same stream, flowing again.
pub fn stream_back(feed: &str, symbol: &str, timeframe: Timeframe) -> Entry {
    Entry::info(feed, format!("{symbol} {} back", timeframe.label()))
}

/// One step of signing in to a feed, as the feed says it.
pub fn feed_step(feed: &str, line: &str) -> Entry {
    Entry::info(feed, capitalised(line))
}

/// How signing in to a feed ended.
pub fn feed_signed_in(feed: &str, outcome: &Result<(), String>) -> Entry {
    match outcome {
        Ok(()) => Entry::info(feed, "Signed in"),
        Err(why) => Entry::error(feed, format!("Not signed in · {}", gist(why))).detail(why.clone()),
    }
}

/// The feeds took on what their logins may chart, and the charts moved.
pub fn serving_changed() -> Entry {
    Entry::info("Data feeds", "Data feeds changed what they serve · charts moved")
}

/// How a broker sign-in ended: "Signed in · 2 accounts", or the broker's
/// refusal. Signed in with none listed is a warning: nothing can be traded.
pub fn signed_in(login: &str, outcome: Result<usize, &str>) -> Entry {
    signed_in_heard(login, outcome, None)
}

/// [`signed_in`], with what the sign-in heard from the broker as the
/// entry's detail: for Rithmic, who signed in where and every account list
/// reply. A refusal one of those replies carried, though accounts were
/// listed, is a warning that says it.
pub fn signed_in_heard(login: &str, outcome: Result<usize, &str>, heard: Option<&trading::Heard>) -> Entry {
    let detail = heard.map(|heard| heard.detail.as_str()).unwrap_or_default();
    let warning = heard.and_then(|heard| heard.warning.as_deref());
    let counted = |accounts: usize| match accounts {
        1 => "Signed in · 1 account".to_string(),
        accounts => format!("Signed in · {accounts} accounts"),
    };
    match (outcome, warning) {
        (Err(why), _) => {
            let detail = [why, detail].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join("\n\n");
            Entry::error(login, format!("Not signed in · {}", gist(why))).detail(detail)
        }
        (Ok(0), None) => Entry::warning(login, "Signed in · the broker lists no accounts").detail(detail),
        (Ok(0), Some(refused)) => {
            Entry::warning(login, format!("Signed in · no accounts · the account list was refused: {}", gist(refused)))
                .detail(detail)
        }
        (Ok(accounts), Some(refused)) => Entry::warning(
            login,
            format!("{} · the account list was partly refused: {}", counted(accounts), gist(refused)),
        )
        .detail(detail),
        (Ok(accounts), None) => Entry::info(login, counted(accounts)).detail(detail),
    }
}

/// The data half of a login did not sign in, though trading did.
pub fn data_not_signed_in(login: &str, why: &str) -> Entry {
    Entry::error(login, format!("Data not signed in · {}", gist(why))).detail(why)
}

/// A session that ended by itself.
pub fn session_ended(login: &str, why: &str) -> Entry {
    Entry::error(login, format!("Signed out · {}", gist(why))).detail(why)
}

/// Signed out from Trading.
pub fn signed_out(login: &str) -> Entry {
    Entry::info(login, "Signed out")
}

/// An order sent: "Buy Limit 2 ESZ6 at 7,620.00", "Buy 2 ESZ6 at market".
pub fn order_sent(account: &str, order: &OrderRequest, decimals: usize) -> Entry {
    let price = order.price.map(|price| omacharts_engine::grouped_decimals(price, decimals));
    Entry::info(account, trading::order_text(order.side, order.kind, order.qty, &order.contract, price.as_deref()))
}

/// Something asked for that was not done: "Not flattened · nothing
/// held".
pub fn refused(account: &str, text: impl Into<String>) -> Entry {
    Entry::error(account, text)
}

/// Something asked for and done, said in a toast too.
pub fn confirmed(account: &str, text: impl Into<String>) -> Entry {
    Entry::info(account, text).confirming()
}

/// What became of an order placed or cancelled here, once the broker says
/// so: a fill or a cancel in a toast and here, a rejection as an error.
pub fn order_moved_on(order: &Order, before: Stage, decimals: usize, account: &str) -> Option<Entry> {
    let text = trading::outcome_text(order, before, decimals)?;
    let source = if account.is_empty() { "Trading" } else { account };
    Some(match (order.status.stage, order.status.bound, &order.status.said) {
        // The broker's own words, which the line has put plainly.
        (Stage::Rejected, Some(_), Some(said)) => Entry::error(source, text).detail(said.clone()),
        (Stage::Rejected, ..) => Entry::error(source, text),
        _ => Entry::info(source, text).confirming(),
    })
}

/// Volume at price for a series flowing again after a drop, the gap
/// asked for in `spans` stretches. Asking the first time is routine and not
/// said.
pub fn ladders_back(key: &str, spans: usize) -> Entry {
    let (feed, symbol) = feed_and_symbol(key);
    let stretches = if spans == 1 { "1 stretch".to_string() } else { format!("{spans} stretches") };
    Entry::info(feed, format!("{symbol} · Volume at price back after a drop · asked for {stretches}"))
}

/// Volume at price for a series did not come; it is asked again next time.
pub fn ladders_failed(key: &str, error: impl std::fmt::Display) -> Entry {
    let (feed, symbol) = feed_and_symbol(key);
    Entry::warning(feed, format!("{symbol} · Volume at price not loaded")).detail(error.to_string())
}

/// A cache key's feed, by name when it is one this build knows, and symbol.
fn feed_and_symbol(key: &str) -> (String, &str) {
    let (id, symbol) = key.split_once(':').unwrap_or(("", key));
    let feed = omacharts_engine::providers::listed(id).map_or(id.to_string(), |feed| feed.label.to_string());
    (feed, symbol)
}

/// Something of the app's own went wrong — a file, the database, the bar
/// widget: `what` in plain words, `error` as the system put it.
pub fn trouble(source: &str, what: &str, error: impl std::fmt::Display) -> Entry {
    Entry::error(source, what).detail(error.to_string())
}

/// Something worth knowing that is not trouble: anything asked of a broker
/// besides an order ("Flatten ESZ6"), the cache trimmed, ladders asked for.
pub fn note(source: &str, what: impl Into<String>) -> Entry {
    Entry::info(source, what)
}

fn capitalised(line: &str) -> String {
    let mut chars = line.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::OrderKind;
    use omacharts_engine::log::Level;

    #[test]
    fn a_fetch_failure_says_its_sentence_and_only_a_bad_reply_is_an_error() {
        let entry = fetch_failed("Yahoo Finance", "AAPL", Timeframe::days(1), FetchFailure::Unreachable);
        assert_eq!(entry.level, Level::Warning);
        assert_eq!(entry.source, "Yahoo Finance");
        assert_eq!(entry.text, "AAPL 1D · The data feed is not answering. Prices fill in when it does.");
        assert_eq!(fetch_failed("Yahoo Finance", "AAPL", Timeframe::days(1), FetchFailure::Garbled).level, Level::Error);
    }

    #[test]
    fn a_refusal_keeps_its_first_sentence_and_the_rest_behind_details() {
        let entry = signed_in("Rithmic Test", Err("permission denied. Check the system name.\ncode 13"));
        assert_eq!(entry.level, Level::Error);
        assert_eq!(entry.text, "Not signed in · permission denied");
        assert_eq!(entry.detail.as_deref(), Some("permission denied. Check the system name.\ncode 13"));
        assert_eq!(signed_in("Rithmic Test", Ok(2)).text, "Signed in · 2 accounts");
        assert_eq!(signed_in("Rithmic Test", Ok(1)).text, "Signed in · 1 account");
        let none = signed_in("Rithmic Test", Ok(0));
        assert_eq!((none.level, none.text.as_str()), (Level::Warning, "Signed in · the broker lists no accounts"));
        assert_eq!(feed_step("Rithmic Test", "connecting to the gateway").text, "Connecting to the gateway");
    }

    /// A sign-in's entry carries what the broker said as its detail, and
    /// warns of a refusal on the account list though accounts were listed.
    #[test]
    fn a_sign_in_says_what_the_broker_heard_in_its_detail() {
        let heard = trading::Heard { warning: None, detail: "User: ••• (the login's own)\nAccount list replies: 1".into() };
        let none = signed_in_heard("Rithmic Test", Ok(0), Some(&heard));
        assert_eq!((none.level, none.text.as_str()), (Level::Warning, "Signed in · the broker lists no accounts"));
        assert_eq!(none.detail.as_deref(), Some(heard.detail.as_str()));
        let one = signed_in_heard("Rithmic Test", Ok(1), Some(&heard));
        assert_eq!((one.level, one.detail.as_deref()), (Level::Info, Some(heard.detail.as_str())));

        let refused = trading::Heard { warning: Some("Rithmic refused the request: permission denied".into()), ..heard.clone() };
        let partly = signed_in_heard("Rithmic Test", Ok(1), Some(&refused));
        assert_eq!(partly.level, Level::Warning);
        assert_eq!(
            partly.text,
            "Signed in · 1 account · the account list was partly refused: Rithmic refused the request: permission denied"
        );
        let failed = signed_in_heard("Rithmic Test", Err("permission denied"), Some(&heard));
        assert_eq!(failed.text, "Not signed in · permission denied");
        assert_eq!(failed.detail.as_deref(), Some(format!("permission denied\n\n{}", heard.detail).as_str()));
    }

    #[test]
    fn an_order_sent_reads_as_it_was_asked() {
        let mut order = OrderRequest {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: trading::Side::Buy,
            qty: 2,
            kind: OrderKind::Market,
            price: None,
            tag: "omacharts:1".into(),
        };
        assert_eq!(order_sent("Lucid", &order, 2).text, "Buy 2 ESZ6 at market");
        (order.kind, order.price, order.side) = (OrderKind::Limit, Some(7_620.0), trading::Side::Sell);
        assert_eq!(order_sent("Lucid", &order, 2).text, "Sell Limit 2 ESZ6 at 7,620.00");
    }

    #[test]
    fn a_fill_is_confirmed_and_a_rejection_is_an_error() {
        use crate::trading::fake::tests::order;
        use crate::trading::{Side, Status};
        let mut bought = order("a", Side::Buy, OrderKind::Market, None, Stage::Filled);
        bought.fill_price = Some(7_612.25);
        let filled = order_moved_on(&bought, Stage::Pending, 2, "951").expect("said");
        assert!(filled.confirms);
        assert_eq!(filled.level, Level::Info);
        let refused = order("c", Side::Buy, OrderKind::Limit, Some(1.0), Status::said(Stage::Rejected, "price out of band"));
        let rejected = order_moved_on(&refused, Stage::Pending, 2, "951").expect("said");
        assert_eq!((rejected.level, rejected.confirms), (Level::Error, false));
        assert_eq!(rejected.line(), "951 · Rejected · Buy Limit 1 ESZ6 at 1.00 · price out of band", "the account said once");

        let raw = "Order price is outside bands 'Ask of 748000.00 violates Low Band 752125.00'";
        let mut banded = order("f", Side::Sell, OrderKind::Limit, Some(7_480.0), Status::said(Stage::Rejected, raw));
        banded.status.bound = Some(trading::Bound { price: 7_521.25, kind: trading::BoundKind::Band });
        let rejected = order_moved_on(&banded, Stage::Pending, 2, "951").expect("said");
        assert_eq!(rejected.text, "Rejected · Sell Limit 1 ESZ6 at 7,480.00 · too far below the market (exchange band 7,521.25)");
        assert_eq!(rejected.detail.as_deref(), Some(raw), "the broker's words kept behind Details");
    }

    #[test]
    fn trouble_of_the_apps_own_is_a_plain_sentence_with_the_error_behind_it() {
        let entry = trouble("Backups", "Settings not backed up", "disk I/O error (code 10)");
        assert_eq!(entry.text, "Settings not backed up");
        assert_eq!(entry.detail.as_deref(), Some("disk I/O error (code 10)"));
    }
}
