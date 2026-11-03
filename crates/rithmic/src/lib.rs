//! A Rithmic account: sign in to it, see what it holds, and watch it.
//!
//! Rithmic is reached over R | Protocol, protocol buffers on a WebSocket,
//! which `rithmic-rs` speaks. Its plants are separate logins: the ticker
//! plant for live prices, the order plant for accounts and orders, the PnL
//! plant for balances, the history plant for bars. A session here signs in to
//! each of them and reports what it learns as [`Event`]s, so the caller
//! needs neither tokio nor anything from `rithmic-rs` to use it.
//!
//! `rithmic-rs` is async and the app is not, so a session runs a runtime on a
//! thread of its own and hands its events to a callback. Dropping the
//! [`Session`] ends it: the thread logs out of every plant and stops.
//!
//! Only the ticker plant's login is required. Rithmic Test, for one, may have
//! no accounts and little history, and a sign-in that works should still say
//! so when the history plant has nothing for it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rithmic_rs::{
    rti::messages::RithmicMessage, ConnectStrategy, RithmicAccount, RithmicConfig, RithmicEnv,
    RithmicHistoryPlant, RithmicOrderPlant, RithmicPnlPlant, RithmicResponse, RithmicTickerPlant,
    TimeBarType,
};
use tokio::sync::oneshot;

/// The app name Rithmic knows this app by. Rithmic Test takes any name;
/// production takes only one that has passed conformance.
pub const APP_NAME: &str = "omacharts";

/// Rithmic's test system, which every developer kit login can use.
pub const TEST_SYSTEM: &str = "Rithmic Test";
pub const TEST_URL: &str = "wss://rituz00100.rithmic.com:443";

/// The contract a session watches, as the root of a CME future. The front
/// month is asked for rather than written down, so it never goes stale.
const WATCHED: (&str, &str) = ("ES", "CME");

/// How long any one request may take before the session says so and moves on.
const PATIENCE: Duration = Duration::from_secs(15);

/// Who to sign in as, and where.
#[derive(Clone)]
pub struct Credentials {
    pub user: String,
    pub password: String,
    /// The Rithmic system, such as "Rithmic Test" or a prop firm's own.
    pub system: String,
    /// The gateway's WebSocket URL.
    pub url: String,
}

impl std::fmt::Debug for Credentials {
    /// Everything but the password, which must never reach a log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("user", &self.user)
            .field("system", &self.system)
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// An account the login can trade.
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub currency: String,
}

/// What a session has learned, in the order it learns it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// What it is doing now, for somebody watching it work.
    Step(String),
    /// The ticker plant accepted the login.
    SignedIn { fcm: String, ib: String },
    /// The accounts the order plant lists, possibly none.
    Accounts(Vec<Account>),
    /// An account's balance and profit, as the PnL plant reports them.
    Balance { account: String, balance: String, open_pnl: String, closed_pnl: String },
    /// The contract the session watches, once the front month is known.
    Watching { symbol: String, exchange: String },
    /// A trade on the watched contract.
    Trade { price: f64, size: u64 },
    /// The best bid and offer on the watched contract.
    Quote { bid: f64, bid_size: u64, ask: f64, ask_size: u64 },
    /// How much of the last day's minute bars the history plant returned.
    History { bars: usize, last_close: Option<f64> },
    /// A plant that is not essential could not be used. The session goes on.
    Unavailable { what: String, why: String },
    /// The session could not start, or lost its connection. Nothing follows.
    Failed(String),
}

/// A running session. Dropping it signs out.
pub struct Session {
    stop: Option<oneshot::Sender<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Sign in and report on a thread of its own. `on_event` is called on that
/// thread.
pub fn connect(credentials: Credentials, on_event: impl Fn(Event) + Send + 'static) -> Session {
    let (stop, stopped) = oneshot::channel();
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => return on_event(Event::Failed(error.to_string())),
        };
        runtime.block_on(run(credentials, &on_event, stopped));
    });
    Session { stop: Some(stop) }
}

fn config(credentials: &Credentials) -> Result<RithmicConfig, String> {
    RithmicConfig::builder(RithmicEnv::Test)
        .url(&credentials.url)
        // Only read by a strategy that alternates, and required regardless.
        .beta_url(&credentials.url)
        .user(&credentials.user)
        .password(&credentials.password)
        .system_name(&credentials.system)
        .app_name(APP_NAME)
        .app_version(env!("CARGO_PKG_VERSION"))
        .connect_attempt_timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())
}

async fn within<T, E: std::fmt::Display>(
    future: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, String> {
    match tokio::time::timeout(PATIENCE, future).await {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => Err("no answer in time".to_string()),
    }
}

/// A refusal comes back as a response with its error set, not as an `Err`.
fn refused(response: &RithmicResponse) -> Option<String> {
    response.error.as_ref().map(ToString::to_string)
}

async fn run(credentials: Credentials, on_event: &impl Fn(Event), stopped: oneshot::Receiver<()>) {
    let config = match config(&credentials) {
        Ok(config) => config,
        Err(error) => return on_event(Event::Failed(error)),
    };

    on_event(Event::Step(format!("Connecting to {}", credentials.system)));
    let ticker = match within(RithmicTickerPlant::connect(&config, ConnectStrategy::Simple)).await {
        Ok(plant) => plant,
        Err(error) => return on_event(Event::Failed(format!("Could not connect: {error}"))),
    };
    let mut prices = ticker.get_handle();
    let login = match within(prices.login()).await {
        Ok(login) => login,
        Err(error) => return on_event(Event::Failed(format!("Sign-in refused: {error}"))),
    };
    if let Some(error) = refused(&login) {
        return on_event(Event::Failed(format!("Sign-in refused: {error}")));
    }
    let (fcm, ib) = match &login.message {
        RithmicMessage::ResponseLogin(login) => {
            (login.fcm_id.clone().unwrap_or_default(), login.ib_id.clone().unwrap_or_default())
        }
        _ => Default::default(),
    };
    on_event(Event::SignedIn { fcm: fcm.clone(), ib: ib.clone() });

    // Accounts, and then each one's balance. Neither is essential: a
    // developer login may have nothing to trade.
    on_event(Event::Step("Asking for accounts".to_string()));
    let orders = within(RithmicOrderPlant::connect(&config, ConnectStrategy::Simple)).await;
    let mut accounts = Vec::new();
    let order_handle = match orders {
        Ok(plant) => {
            let handle = plant.get_handle(&RithmicAccount::new(&fcm, &ib, ""));
            match within(handle.login()).await {
                Ok(_) => match within(handle.get_account_list()).await {
                    Ok(listed) => {
                        accounts = listed
                            .iter()
                            .filter_map(|response| match &response.message {
                                RithmicMessage::ResponseAccountList(account) => {
                                    Some(Account {
                                        id: account.account_id.clone()?,
                                        name: account.account_name.clone().unwrap_or_default(),
                                        currency: account
                                            .account_currency
                                            .clone()
                                            .unwrap_or_default(),
                                    })
                                }
                                _ => None,
                            })
                            .collect();
                        on_event(Event::Accounts(accounts.clone()));
                    }
                    Err(why) => on_event(Event::Unavailable { what: "Accounts".into(), why }),
                },
                Err(why) => on_event(Event::Unavailable { what: "Accounts".into(), why }),
            }
            Some((plant, handle))
        }
        Err(why) => {
            on_event(Event::Unavailable { what: "Accounts".into(), why });
            None
        }
    };

    // Every account's updates arrive on one channel, whichever handle they
    // came from.
    let (balance_sender, mut balance_updates) = tokio::sync::mpsc::unbounded_channel();
    let mut balances = Vec::new();
    let mut pnl_plant = None;
    if !accounts.is_empty() {
        match within(RithmicPnlPlant::connect(&config, ConnectStrategy::Simple)).await {
            Ok(plant) => {
                for account in &accounts {
                    let handle = plant.get_handle(&RithmicAccount::new(&fcm, &ib, &account.id));
                    if let Err(why) = within(handle.login()).await {
                        on_event(Event::Unavailable { what: "Balances".into(), why });
                        break;
                    }
                    if let Ok(snapshot) = within(handle.get_pnl_position_snapshot()).await {
                        report_balance(&snapshot.message, on_event);
                    }
                    let _ = within(handle.subscribe_pnl_updates()).await;
                    let mut updates = handle.subscription_receiver.resubscribe();
                    let sender = balance_sender.clone();
                    tokio::spawn(async move {
                        while let Ok(update) = updates.recv().await {
                            if sender.send(update.message).is_err() {
                                return;
                            }
                        }
                    });
                    balances.push(handle);
                }
                pnl_plant = Some(plant);
            }
            Err(why) => on_event(Event::Unavailable { what: "Balances".into(), why }),
        }
    }

    // The contract to watch.
    let front = within(prices.get_front_month_contract(WATCHED.0, WATCHED.1, false)).await;
    let symbol = match &front {
        Ok(response) => match &response.message {
            RithmicMessage::ResponseFrontMonthContract(front) => front
                .trading_symbol
                .clone()
                .or_else(|| front.symbol.clone())
                .filter(|symbol| !symbol.is_empty()),
            _ => None,
        },
        Err(_) => None,
    }
    .unwrap_or_else(|| guess_front_month(WATCHED.0));
    on_event(Event::Watching { symbol: symbol.clone(), exchange: WATCHED.1.to_string() });

    // A day of minute bars, to see what the history plant has.
    on_event(Event::Step("Asking for history".to_string()));
    match within(RithmicHistoryPlant::connect(&config, ConnectStrategy::Simple)).await {
        Ok(plant) => {
            let handle = plant.get_handle();
            match within(handle.login()).await {
                Ok(_) => {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_secs() as i32)
                        .unwrap_or_default();
                    let bars = within(handle.load_time_bars(
                        symbol.clone(),
                        WATCHED.1.to_string(),
                        TimeBarType::MinuteBar,
                        1,
                        now - 24 * 60 * 60,
                        now,
                    ))
                    .await;
                    match bars {
                        Ok(bars) => {
                            let closes: Vec<f64> = bars
                                .iter()
                                .filter_map(|response| match &response.message {
                                    RithmicMessage::ResponseTimeBarReplay(bar) => bar.close_price,
                                    _ => None,
                                })
                                .collect();
                            on_event(Event::History {
                                bars: closes.len(),
                                last_close: closes.last().copied(),
                            });
                        }
                        Err(why) => on_event(Event::Unavailable { what: "History".into(), why }),
                    }
                    let _ = within(handle.disconnect()).await;
                }
                Err(why) => on_event(Event::Unavailable { what: "History".into(), why }),
            }
        }
        Err(why) => on_event(Event::Unavailable { what: "History".into(), why }),
    }

    // And then live prices, until the session is dropped.
    match within(prices.subscribe(&symbol, WATCHED.1)).await {
        Ok(response) => {
            if let Some(why) = refused(&response) {
                on_event(Event::Unavailable { what: "Live prices".into(), why });
            }
        }
        Err(why) => on_event(Event::Unavailable { what: "Live prices".into(), why }),
    }
    on_event(Event::Step(String::new()));

    drop(balance_sender);
    let mut stopped = stopped;
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            update = prices.subscription_receiver.recv() => match update {
                Ok(update) => match &update.message {
                    RithmicMessage::LastTrade(trade) => {
                        if let Some(price) = trade.trade_price.filter(|price| *price > 0.0) {
                            on_event(Event::Trade {
                                price,
                                size: trade.trade_size.unwrap_or_default().max(0) as u64,
                            });
                        }
                    }
                    RithmicMessage::BestBidOffer(quote) => on_event(Event::Quote {
                        bid: quote.bid_price.unwrap_or_default(),
                        bid_size: quote.bid_size.unwrap_or_default().max(0) as u64,
                        ask: quote.ask_price.unwrap_or_default(),
                        ask_size: quote.ask_size.unwrap_or_default().max(0) as u64,
                    }),
                    RithmicMessage::ConnectionError
                    | RithmicMessage::HeartbeatTimeout
                    | RithmicMessage::ForcedLogout(_) => {
                        on_event(Event::Failed(match &update.error {
                            Some(error) => format!("Disconnected: {error}"),
                            None => "Disconnected".to_string(),
                        }));
                        break;
                    }
                    _ => {}
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    on_event(Event::Failed("Disconnected".to_string()));
                    break;
                }
            },
            Some(message) = balance_updates.recv() => report_balance(&message, on_event),
        }
    }

    let _ = within(prices.disconnect()).await;
    for handle in &balances {
        let _ = within(handle.disconnect()).await;
    }
    if let Some((_, handle)) = &order_handle {
        let _ = within(handle.disconnect()).await;
    }
    drop(pnl_plant);
}

fn report_balance(message: &RithmicMessage, on_event: &impl Fn(Event)) {
    if let RithmicMessage::AccountPnLPositionUpdate(pnl) = message {
        on_event(Event::Balance {
            account: pnl.account_id.clone().unwrap_or_default(),
            balance: pnl.account_balance.clone().unwrap_or_default(),
            open_pnl: pnl.open_position_pnl.clone().unwrap_or_default(),
            closed_pnl: pnl.closed_position_pnl.clone().unwrap_or_default(),
        });
    }
}

/// The quarterly contract trading now, for when the server will not say:
/// March, June, September and December, rolled a week before they expire.
fn guess_front_month(root: &str) -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() / 86_400)
        .unwrap_or_default() as i64;
    let (year, month, day) = civil(days);
    let quarter_month = ((month - 1) / 3 + 1) * 3;
    let (year, quarter_month) = if month == quarter_month && day >= 12 {
        if quarter_month == 12 { (year + 1, 3) } else { (year, quarter_month + 3) }
    } else {
        (year, quarter_month)
    };
    let code = match quarter_month {
        3 => 'H',
        6 => 'M',
        9 => 'U',
        _ => 'Z',
    };
    format!("{root}{code}{}", year % 10)
}

/// A day count since 1970 as year, month and day.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_is_never_in_what_credentials_print() {
        let credentials = Credentials {
            user: "trader".into(),
            password: "SECRET".into(),
            system: TEST_SYSTEM.into(),
            url: TEST_URL.into(),
        };
        let printed = format!("{credentials:?}");
        assert!(!printed.contains("SECRET"), "{printed}");
        assert!(printed.contains("trader"), "{printed}");
    }

    #[test]
    fn days_are_read_as_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(20_760), (2026, 11, 3));
    }
}
