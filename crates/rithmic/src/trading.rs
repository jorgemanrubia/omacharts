//! A login's trading session: the order plant, which lists the login's
//! accounts, and the PnL plant, which pushes their balances.
//!
//! Apart from [`Client`](crate::Client), which is market data: only one
//! login a process has supplies that, and every login can trade. They are
//! different plants, so the data session and a trading session on the same
//! login sit side by side. What must not happen is two of either for one
//! login, because Rithmic signs the first out when the same login signs in
//! to a plant again; keeping to one is the caller's, which holds them by
//! login.
//!
//! Signed in once and kept until [`Trading::sign_out`]. It does not sign in
//! again by itself after a drop, unlike the data session: nothing is
//! watching it yet that would be lost, and a trader signing in again is one
//! press. Orders, when they come, bring that with them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::rti::ResponseAccountList;
use rithmic_rs::{
    ConnectStrategy, RithmicAccount, RithmicOrderPlant, RithmicOrderPlantHandle, RithmicPnlPlant,
    RithmicPnlPlantHandle, RithmicResponse,
};
use tokio::sync::broadcast::error::RecvError;

use crate::client::{config, error_of, signed_in, within};
use crate::{App, Error, Login};

/// How long one request may take.
const REQUEST: Duration = Duration::from_secs(15);

/// One account inside a login, as the order plant lists it.
#[derive(Clone, PartialEq)]
pub struct Account {
    /// The account number, such as "951".
    pub id: String,
    /// The name the firm gave it, when it gave one.
    pub name: Option<String>,
    /// Its currency, such as "USD".
    pub currency: Option<String>,
    /// The futures commission merchant and introducing broker it is under,
    /// which an order names beside the account.
    pub fcm: String,
    pub ib: String,
}

impl std::fmt::Debug for Account {
    /// Without the number: it is somebody's account, and a log is no place
    /// for it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("currency", &self.currency).finish_non_exhaustive()
    }
}

type Changed = Arc<dyn Fn() + Send + Sync>;

struct Inner {
    handle: tokio::runtime::Handle,
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    order: RithmicOrderPlantHandle,
    pnl: RithmicPnlPlantHandle,
    plants: Mutex<Option<(RithmicOrderPlant, RithmicPnlPlant)>>,
    accounts: Vec<Account>,
    balances: Arc<Mutex<HashMap<String, f64>>>,
    /// Why the session ended by itself: a drop, or Rithmic signing it out.
    trouble: Arc<Mutex<Option<Error>>>,
    closed: AtomicBool,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.get_mut().unwrap_or_else(|e| e.into_inner()).take() {
            runtime.shutdown_background();
        }
    }
}

/// A login's trading session. Cheap to clone; every clone is the same
/// session.
#[derive(Clone)]
pub struct Trading {
    inner: Arc<Inner>,
}

impl Trading {
    /// Sign `login` in to the order and PnL plants, as `app`, list its
    /// accounts and ask for their balances. Blocks until Rithmic has answered, so it is
    /// for a thread of its own.
    ///
    /// `changed` is called on the session's thread whenever a balance
    /// arrives or the session ends by itself.
    pub fn sign_in(app: App, login: &Login, changed: impl Fn() + Send + Sync + 'static) -> Result<Trading, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("rithmic-trading")
            .enable_all()
            .build()
            .map_err(|error| Error::Connection(error.to_string()))?;
        let changed: Changed = Arc::new(changed);
        let signed = runtime.block_on(connect(app, login));
        let (order_plant, pnl_plant, order, pnl, accounts) = match signed {
            Ok(signed) => signed,
            Err(error) => {
                runtime.shutdown_background();
                return Err(error);
            }
        };
        let balances = Arc::new(Mutex::new(HashMap::new()));
        let trouble = Arc::new(Mutex::new(None));
        runtime.spawn(listen(
            pnl_plant.subscribe_all(),
            order_plant.subscribe_all(),
            balances.clone(),
            trouble.clone(),
            changed,
        ));
        // Balances come on the updates the listener now hears; a snapshot
        // request only acknowledges.
        runtime.block_on(async {
            for account in &accounts {
                let handle = pnl_plant.get_handle(&RithmicAccount::new(&account.fcm, &account.ib, &account.id));
                let _ = within(REQUEST, handle.subscribe_pnl_updates()).await;
                let _ = within(REQUEST, handle.get_pnl_position_snapshot()).await;
            }
        });
        Ok(Trading {
            inner: Arc::new(Inner {
                handle: runtime.handle().clone(),
                runtime: Mutex::new(Some(runtime)),
                order,
                pnl,
                plants: Mutex::new(Some((order_plant, pnl_plant))),
                accounts,
                balances,
                trouble,
                closed: AtomicBool::new(false),
            }),
        })
    }

    /// The login's accounts, as the order plant listed them at sign-in.
    pub fn accounts(&self) -> Vec<Account> {
        self.inner.accounts.clone()
    }

    /// The balance of the account `id`, once the PnL plant has said.
    pub fn balance(&self, id: &str) -> Option<f64> {
        self.inner.balances.lock().unwrap_or_else(|e| e.into_inner()).get(id).copied()
    }

    /// Why the session is no longer signed in, if it ended by itself.
    pub fn trouble(&self) -> Option<Error> {
        if self.inner.closed.load(Ordering::Acquire) {
            return Some(Error::Closed);
        }
        self.inner.trouble.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Sign out of both plants, waiting a few seconds at most.
    pub fn sign_out(&self) {
        if self.inner.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        let plants = self.inner.plants.lock().unwrap_or_else(|e| e.into_inner()).take();
        let order = &self.inner.order;
        let pnl = &self.inner.pnl;
        self.inner.handle.block_on(async {
            let _ = within(Duration::from_secs(2), order.disconnect()).await;
            let _ = within(Duration::from_secs(2), pnl.disconnect()).await;
            order.abort();
            pnl.abort();
            if let Some((order_plant, pnl_plant)) = plants {
                let _ = tokio::time::timeout(Duration::from_secs(3), order_plant.await_shutdown()).await;
                let _ = tokio::time::timeout(Duration::from_secs(3), pnl_plant.await_shutdown()).await;
            }
        });
    }
}

type Signed = (RithmicOrderPlant, RithmicPnlPlant, RithmicOrderPlantHandle, RithmicPnlPlantHandle, Vec<Account>);

async fn connect(app: App, login: &Login) -> Result<Signed, Error> {
    let credentials = login.credentials().await?;
    let config = config(app, &credentials)?;
    drop(credentials);

    // No account is known before the list, and the login and the list are
    // the plant's, not an account's.
    let nobody = RithmicAccount::new("", "", "");
    let order_plant = within(REQUEST, RithmicOrderPlant::connect(&config, ConnectStrategy::Simple)).await?;
    let order = order_plant.get_handle(&nobody);
    if let Err(error) = signed_in(within(REQUEST, order.login()).await) {
        order.abort();
        return Err(error);
    }
    let listed = match within(REQUEST, order.get_account_list()).await {
        Ok(listed) => listed,
        Err(error) => {
            let _ = within(REQUEST, order.disconnect()).await;
            return Err(error);
        }
    };
    let accounts = accounts_of(&listed)?;
    let pnl_plant = match within(REQUEST, RithmicPnlPlant::connect(&config, ConnectStrategy::Simple)).await {
        Ok(plant) => plant,
        Err(error) => {
            let _ = within(REQUEST, order.disconnect()).await;
            return Err(error);
        }
    };
    let pnl = pnl_plant.get_handle(&nobody);
    if let Err(error) = signed_in(within(REQUEST, pnl.login()).await) {
        pnl.abort();
        let _ = within(REQUEST, order.disconnect()).await;
        return Err(error);
    }
    Ok((order_plant, pnl_plant, order, pnl, accounts))
}

/// The accounts in an account list reply, in the order listed.
fn accounts_of(responses: &[RithmicResponse]) -> Result<Vec<Account>, Error> {
    let accounts = listed(responses.iter().filter_map(|response| match &response.message {
        RithmicMessage::ResponseAccountList(found) => Some(found),
        _ => None,
    }));
    if accounts.is_empty()
        && let Some(error) = responses.last().and_then(|end| end.error.as_ref())
    {
        return Err(error_of(error.clone()));
    }
    Ok(accounts)
}

fn listed<'a>(found: impl Iterator<Item = &'a ResponseAccountList>) -> Vec<Account> {
    let mut accounts: Vec<Account> = Vec::new();
    for found in found {
        let Some(id) = found.account_id.clone().filter(|id| !id.is_empty()) else { continue };
        if accounts.iter().any(|known| known.id == id) {
            continue;
        }
        accounts.push(Account {
            id,
            name: found.account_name.clone().filter(|name| !name.is_empty()),
            currency: found.account_currency.clone().filter(|currency| !currency.is_empty()),
            fcm: found.fcm_id.clone().unwrap_or_default(),
            ib: found.ib_id.clone().unwrap_or_default(),
        });
    }
    accounts
}

/// Hear balances, and the session ending, until it does.
async fn listen(
    mut pnl: tokio::sync::broadcast::Receiver<RithmicResponse>,
    mut order: tokio::sync::broadcast::Receiver<RithmicResponse>,
    balances: Arc<Mutex<HashMap<String, f64>>>,
    trouble: Arc<Mutex<Option<Error>>>,
    changed: Changed,
) {
    let ended = |error: Error| {
        *trouble.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
        changed();
    };
    loop {
        let update = tokio::select! {
            update = pnl.recv() => update,
            update = order.recv() => update,
        };
        let update = match update {
            Ok(update) => update,
            Err(RecvError::Lagged(_)) => continue,
            Err(RecvError::Closed) => return ended(Error::Connection("the session closed".into())),
        };
        match &update.message {
            RithmicMessage::ForcedLogout(_) => {
                return ended(Error::Refused(
                    "Rithmic signed this session out, as it does when the same login signs in somewhere else"
                        .into(),
                ));
            }
            RithmicMessage::ConnectionError => {
                return ended(Error::Connection(match &update.error {
                    Some(error) => error.to_string(),
                    None => "the connection dropped".into(),
                }));
            }
            RithmicMessage::AccountPnLPositionUpdate(position) => {
                let (Some(account), Some(balance)) = (
                    position.account_id.as_deref(),
                    position.account_balance.as_deref().and_then(|text| text.trim().parse::<f64>().ok()),
                ) else {
                    continue;
                };
                let moved = {
                    let mut held = balances.lock().unwrap_or_else(|e| e.into_inner());
                    held.insert(account.to_string(), balance) != Some(balance)
                };
                if moved {
                    changed();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(id: &str, currency: &str) -> ResponseAccountList {
        let mut found = ResponseAccountList::default();
        found.fcm_id = Some("Rithmic-FCM".into());
        found.ib_id = Some("Prospects".into());
        found.account_id = Some(id.into());
        found.account_currency = Some(currency.into());
        found
    }

    #[test]
    fn an_account_list_is_read_in_order_once_each() {
        let replies = [found("1234", "USD"), found("1235", "USD"), found("1234", "USD")];
        let accounts = listed(replies.iter());
        let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["1234", "1235"]);
        assert_eq!(accounts[0].fcm, "Rithmic-FCM");
        assert_eq!(accounts[0].currency.as_deref(), Some("USD"));
        assert!(!format!("{:?}", accounts[0]).contains("1234"), "an account number in a log");
    }
}
