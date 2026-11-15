//! `omacharts trading`: what the Trading panel, the Trading preferences and
//! a chart's orders do, from a terminal.
//!
//! Orders go through the window and nowhere else. Its sessions are the
//! ones signed in, it holds the book they keep, and it judges an order
//! exactly as it judges one from the chart's menu: the side of the market,
//! the day's price limits, the month the chart draws, one request on an
//! account at a time. A command is that menu with the prices typed, so it
//! is refused for the same reasons, in the same words, and nothing is sent
//! that the chart would not have sent. With no window open there is no
//! session to send through, and these verbs say so.
//!
//! What is kept in the store, the logins, their accounts as last listed
//! and the default account, is read and changed without a window too, and
//! a window that is open takes the change at once.
//!
//! An order is sent, not filled, when the command returns: the trading
//! service answers on its own time, and what it answered is in the Log and
//! in `trading orders`.

use serde_json::{json, Value};

use super::{Fault, Live, EXIT_NO_WINDOW};
use crate::store::Store;
use crate::trading::{self, Account, Login, OrderKind, Side, Standing, View};

/// Something done to an account through the window: the chart it is
/// done from by id, an order by its basket, an account by its key.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// Buy or sell what chart `chart` trades: at the market, or a limit
    /// or stop at `price`, for `quantity` or the chart's own.
    Order { chart: u32, side: Side, kind: OrderKind, price: Option<f64>, quantity: Option<u32> },
    /// A target, a stop, or both linked, for the position chart `chart` trades.
    Exit { chart: u32, target: Option<f64>, stop: Option<f64> },
    /// A working order moved to `price`: on `account` when it is given,
    /// for two accounts can each have an order of the same id.
    Modify { basket: String, account: Option<String>, price: f64 },
    /// A working order cancelled, on `account` when it is given.
    Cancel { basket: String, account: Option<String> },
    /// Every open order on the account cancelled.
    CancelAll { account: String },
    /// What chart `chart` trades flattened and its orders cancelled: Ctrl+F.
    FlattenChart { chart: u32 },
    /// `contract` flattened on `account`: the Positions tab's Flatten.
    Flatten { account: String, contract: String },
}

/// What the window said doing it: who said it, and what.
pub type Said = Vec<(String, String)>;

/// The window's answer to a verb that needs it, when there is none.
pub fn no_window() -> Fault {
    Fault::new(
        EXIT_NO_WINDOW,
        "no window is open, and trading happens in the window, where the sessions are signed in; start Omacharts".into(),
    )
}

/// The account `--account` names, by its key, when it was given.
fn account_key(store: &Store, live: &dyn Live, m: &clap::ArgMatches) -> Result<Option<String>, Fault> {
    let Some(text) = arg(m, "account") else { return Ok(None) };
    let view = view_of(store, Some(live));
    Ok(Some(find_account(&view, text)?.key()))
}

/// Run `trading VERB`.
pub fn run(store: &Store, verb: &str, m: &clap::ArgMatches, as_json: bool, live: Option<&dyn Live>) -> Result<String, Fault> {
    match verb {
        "logins" => logins(store, live, as_json),
        "login" => {
            let service = required(m, "SERVICE")?;
            let known = services(live);
            let Some((id, label)) = known.iter().find(|(id, label)| id.eq_ignore_ascii_case(service) || label.eq_ignore_ascii_case(service)) else {
                let names: Vec<&str> = known.iter().map(|(id, _)| id.as_str()).collect();
                return Err(Fault::not_found(format!("no trading service called {service:?}; there are {}", names.join(", "))));
            };
            let live = live.ok_or_else(no_window)?;
            live.add_login(id)?;
            said(as_json, json!({"service": id, "dialog": true}), format!("the {label} sign-in is open in the window; the password is typed there"))
        }
        "sign-in" => {
            let login = find_login(store, required(m, "LOGIN")?)?;
            let live = live.ok_or_else(no_window)?;
            let asked = live.sign_in_login(&login.id)?;
            let text = match asked {
                true => format!("the sign-in for {} is open in the window; its password is typed there", login.name),
                false => format!("signing in to {}; `omacharts trading logins` says when it is live", login.name),
            };
            said(as_json, json!({"login": login.name, "dialog": asked}), text)
        }
        "sign-out" => {
            let login = find_login(store, required(m, "LOGIN")?)?;
            if let Some(live) = live {
                live.end_session(&login.id);
            }
            let secrets = crate::ui::trading_settings::keyring_for(&login.service);
            trading::sign_out_login(store, secrets.as_ref(), &login.id).map_err(|why| Fault::new(super::EXIT_ERROR, why))?;
            crate::log_words::say(crate::log_words::signed_out(&login.name));
            adopt(live);
            said(as_json, json!({"login": login.name, "signed_out": true}), format!("signed out of {}, and its password forgotten", login.name))
        }
        "remove" => {
            let login = find_login(store, required(m, "LOGIN")?)?;
            let feed_too = flag(m, "with-feed");
            if let Some(live) = live {
                live.end_session(&login.id);
            }
            let removal = trading::removal(store, &login.id);
            let secrets = crate::ui::trading_settings::keyring_for(&login.service);
            trading::remove_login(store, secrets.as_ref(), &login.id, feed_too).map_err(|why| Fault::new(super::EXIT_ERROR, why))?;
            adopt(live);
            let feed = match (removal, feed_too) {
                (trading::Removal::HandOver { to }, _) => format!("; charts' data comes through {to} now"),
                (trading::Removal::Ask, true) => "; its data feed went with it".to_string(),
                (trading::Removal::Ask, false) => "; its data feed stays, not signed in".to_string(),
                (trading::Removal::KeepFeed, _) => "; its data feed stays, the last there is".to_string(),
                (trading::Removal::Login, _) => String::new(),
            };
            said(as_json, json!({"login": login.name, "removed": true, "feed_removed": feed_too}), format!("removed {}{feed}", login.name))
        }
        "data" => {
            let login = find_login(store, required(m, "LOGIN")?)?;
            if trading::data_feed_of(&login).is_none() {
                return Err(Fault::refused(format!("{} gives no market data to chart from", login.name)));
            }
            trading::make_data_login(store, &login.id);
            if let Some(live) = live {
                live.adopt_feed();
            }
            adopt(live);
            said(as_json, json!({"login": login.name, "data": true}), format!("charts' market data comes through {}", login.name))
        }
        "accounts" => accounts(store, live, as_json),
        "default" => {
            let view = view_of(store, live);
            let account = find_account(&view, required(m, "ACCOUNT")?)?.clone();
            trading::set_default_account(store, &account.key());
            adopt(live);
            let name = view.account_name(&account);
            said(as_json, json!({"default": account.key(), "name": name}), format!("{name} is the default account"))
        }
        "positions" => positions(live.ok_or_else(no_window)?, as_json),
        "orders" => orders(live.ok_or_else(no_window)?, flag(m, "all"), as_json),
        "buy" | "sell" => {
            let side = if verb == "buy" { Side::Buy } else { Side::Sell };
            let (kind, price) = match (price(m, "limit")?, price(m, "stop")?) {
                (Some(_), Some(_)) => return Err(Fault::usage("--limit and --stop are two orders; pass one".into())),
                (Some(price), None) => (OrderKind::Limit, Some(price)),
                (None, Some(price)) => (OrderKind::Stop, Some(price)),
                (None, None) => (OrderKind::Market, None),
            };
            let quantity = match arg(m, "quantity") {
                None => None,
                Some(text) => Some(quantity(text)?),
            };
            let live = live.ok_or_else(no_window)?;
            let chart = chart_of(store, m)?;
            act(live, Act::Order { chart, side, kind, price, quantity }, as_json)
        }
        "exit" => {
            let (target, stop) = (price(m, "target")?, price(m, "stop")?);
            if target.is_none() && stop.is_none() {
                return Err(Fault::usage("say where: --target, --stop, or both".into()));
            }
            let live = live.ok_or_else(no_window)?;
            let chart = chart_of(store, m)?;
            act(live, Act::Exit { chart, target, stop }, as_json)
        }
        "modify" => {
            let basket = required(m, "ORDER")?.to_string();
            let price = price(m, "price")?.ok_or_else(|| Fault::usage("say where it goes: --price".into()))?;
            let live = live.ok_or_else(no_window)?;
            let account = account_key(store, live, m)?;
            act(live, Act::Modify { basket, account, price }, as_json)
        }
        "cancel" => {
            let live = live.ok_or_else(no_window)?;
            match (arg(m, "ORDER"), flag(m, "all")) {
                (Some(_), true) => Err(Fault::usage("an order, or --all; not both".into())),
                (None, false) => Err(Fault::usage("say which order, or --all for every open one".into())),
                (Some(basket), false) => {
                    let account = account_key(store, live, m)?;
                    act(live, Act::Cancel { basket: basket.clone(), account }, as_json)
                }
                (None, true) => {
                    let view = view_of(store, Some(live));
                    let account = match arg(m, "account") {
                        Some(text) => find_account(&view, text)?.clone(),
                        None => view.default_account().cloned().ok_or_else(|| Fault::refused("there is no account to cancel on".into()))?,
                    };
                    act(live, Act::CancelAll { account: account.key() }, as_json)
                }
            }
        }
        "flatten" => {
            let live = live.ok_or_else(no_window)?;
            match arg(m, "CONTRACT") {
                None => {
                    if arg(m, "account").is_some() {
                        return Err(Fault::usage("--account goes with a contract; a chart flattens on its own account".into()));
                    }
                    let chart = chart_of(store, m)?;
                    act(live, Act::FlattenChart { chart }, as_json)
                }
                Some(contract) => {
                    if arg(m, "chart").is_some() {
                        return Err(Fault::usage("a contract, or --chart; not both".into()));
                    }
                    let view = view_of(store, Some(live));
                    let account = held_on(&view, contract, arg(m, "account").map(String::as_str))?;
                    act(live, Act::Flatten { account, contract: contract.to_uppercase() }, as_json)
                }
            }
        }
        "quantities" => quantities(store, m, as_json, live),
        _ => Err(Fault::usage(format!("no such command: trading {verb}"))),
    }
}

fn said(as_json: bool, value: Value, text: String) -> Result<String, Fault> {
    Ok(match as_json {
        true => format!("{value}\n"),
        false => format!("{text}\n"),
    })
}

fn arg<'a>(m: &'a clap::ArgMatches, id: &str) -> Option<&'a String> {
    m.try_get_one::<String>(id).ok().flatten()
}

fn flag(m: &clap::ArgMatches, id: &str) -> bool {
    m.try_get_one::<bool>(id).ok().flatten().copied().unwrap_or(false)
}

fn required<'a>(m: &'a clap::ArgMatches, id: &str) -> Result<&'a str, Fault> {
    arg(m, id).map(String::as_str).ok_or_else(|| Fault::usage(format!("{id} is missing")))
}

/// A price as typed: `7,612.25` as well as `7612.25`.
fn price(m: &clap::ArgMatches, id: &str) -> Result<Option<f64>, Fault> {
    let Some(text) = arg(m, id) else { return Ok(None) };
    text.replace(',', "")
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|price| price.is_finite() && *price > 0.0)
        .map(Some)
        .ok_or_else(|| Fault::usage(format!("--{id} is a price, not {text:?}")))
}

fn quantity(text: &str) -> Result<u32, Fault> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| Fault::usage(format!("--quantity is a whole number above 0, not {text:?}")))
}

/// The chart `--chart` names in the chartbook `--book` names, or the
/// focused one of the open chartbook, by id.
fn chart_of(store: &Store, m: &clap::ArgMatches) -> Result<u32, Fault> {
    let workspace = super::charts::Workspace::load(store);
    let at = workspace.resolve(arg(m, "book").map(String::as_str))?;
    if at != workspace.active() {
        return Err(Fault::refused(format!(
            "orders go from a chart on screen, and {} is not the open chartbook; switch to it first",
            workspace.name_of(at)
        )));
    }
    super::charts::resolve_pane(workspace.book(at)?, arg(m, "chart").map(String::as_str))
}

/// Trading as the window has it, or as the store has it with none.
fn view_of(store: &Store, live: Option<&dyn Live>) -> View {
    live.and_then(|live| live.trading_view()).map(|(view, _)| view).unwrap_or_else(|| View::load(store))
}

/// The trading services a login can be added for, by id and label.
fn services(live: Option<&dyn Live>) -> Vec<(String, String)> {
    let listed = live.map(|live| live.trading_services()).filter(|listed| !listed.is_empty());
    listed.unwrap_or_else(|| trading::services::registry().iter().map(|service| (service.id().to_string(), service.label().to_string())).collect())
}

/// Tell an open window that Trading changed in the store.
fn adopt(live: Option<&dyn Live>) {
    if let Some(live) = live {
        live.adopt_trading();
    }
}

/// A login by its name, or its id.
fn find_login(store: &Store, text: &str) -> Result<Login, Fault> {
    let logins = trading::logins(store);
    logins
        .iter()
        .find(|login| login.name.eq_ignore_ascii_case(text.trim()) || login.id == text.trim())
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = logins.iter().map(|login| login.name.as_str()).collect();
            Fault::not_found(match names.is_empty() {
                true => format!("no login called {text:?}; there are none yet: `omacharts trading login SERVICE` adds one"),
                false => format!("no login called {text:?}; there are {}", names.join(", ")),
            })
        })
}

/// An account by its key (as `trading accounts --json` gives it), its
/// login's name and its number, `Lucid/1234`, its number, or its name here.
pub fn find_account<'a>(view: &'a View, text: &str) -> Result<&'a Account, Fault> {
    let text = text.trim();
    if let Some(account) = view.accounts.iter().find(|account| account.key() == text) {
        return Ok(account);
    }
    let login_name = |account: &Account| {
        view.logins.iter().find(|login| login.id == account.login).map(|login| login.name.as_str())
    };
    let fits: Vec<&Account> = view
        .accounts
        .iter()
        .filter(|account| {
            account.id.eq_ignore_ascii_case(text)
                || login_name(account).is_some_and(|login| format!("{login}/{}", account.id).eq_ignore_ascii_case(text))
                || view.account_name(account).eq_ignore_ascii_case(text)
                || account.name.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(text))
        })
        .collect();
    match fits.as_slice() {
        [one] => Ok(one),
        [] => {
            let names: Vec<String> = view.accounts.iter().map(|account| view.account_name(account)).collect();
            Err(Fault::not_found(match names.is_empty() {
                true => format!("no account {text:?}; there are none: sign in to a trading service first"),
                false => format!("no account {text:?}; there are {}", names.join(", ")),
            }))
        }
        _ => {
            let keys: Vec<String> = fits.iter().map(|account| account.key()).collect();
            Err(Fault::ambiguous(format!("{text:?} is more than one account; say which: {}", keys.join(", "))))
        }
    }
}

/// The account holding `contract`, by key: the one named, or the only one
/// that holds it.
fn held_on(view: &View, contract: &str, account: Option<&str>) -> Result<String, Fault> {
    let holding: Vec<String> = view
        .book
        .positions
        .iter()
        .filter(|held| held.net != 0 && held.contract.eq_ignore_ascii_case(contract))
        .map(|held| held.account.clone())
        .collect();
    let chosen = match account {
        Some(text) => Some(find_account(view, text)?.key()),
        None => None,
    };
    match (chosen, holding.as_slice()) {
        (Some(key), _) if holding.contains(&key) => Ok(key),
        (Some(key), _) => Err(Fault::refused(format!("Not flattened · {} holds no {contract}", view.name_of(&key)))),
        (None, [one]) => Ok(one.clone()),
        (None, []) => Err(Fault::refused(format!("Not flattened · nothing held in {contract}"))),
        (None, _) => {
            let names: Vec<String> = holding.iter().map(|key| view.name_of(key)).collect();
            Err(Fault::ambiguous(format!("{contract} is held on {}; say which with --account", names.join(" and "))))
        }
    }
}

/// Carry out `act` in the window, and say what it said.
fn act(live: &dyn Live, act: Act, as_json: bool) -> Result<String, Fault> {
    let said = live.trade(act)?;
    let lines: Vec<String> = said.iter().map(|(source, text)| format!("{source}: {text}")).collect();
    let rows: Vec<Value> = said.iter().map(|(source, text)| json!({"account": source, "text": text})).collect();
    self::said(as_json, json!({"said": rows}), lines.join("\n"))
}

fn logins(store: &Store, live: Option<&dyn Live>, as_json: bool) -> Result<String, Fault> {
    let (view, standings) = match live.and_then(|live| live.trading_view()) {
        Some((view, standings)) => (view, Some(standings)),
        None => (View::load(store), None),
    };
    let standing_of = |login: &Login| -> Standing {
        match &standings {
            Some(standings) => standings.iter().find(|(id, _)| *id == login.id).map(|(_, standing)| standing.clone()).unwrap_or(Standing::Ready),
            // Nothing is signed in with no window open.
            None if login.signed_out || login.user.is_empty() => Standing::SignedOut,
            None => Standing::Ready,
        }
    };
    let services = services(live);
    let label_of = |id: &str| services.iter().find(|(service, _)| service == id).map_or(id.to_string(), |(_, label)| label.clone());
    let rows: Vec<Value> = view
        .logins
        .iter()
        .map(|login| {
            let accounts: Vec<String> = view.accounts.iter().filter(|account| account.login == login.id).map(|account| view.account_name(account)).collect();
            json!({
                "name": login.name,
                "service": login.service,
                "standing": standing_of(login).words(),
                "live": standing_of(login) == Standing::Live,
                "data": view.data_login.as_deref() == Some(login.id.as_str()),
                "accounts": accounts,
            })
        })
        .collect();
    if as_json {
        let offered: Vec<Value> = services.iter().map(|(id, label)| json!({"id": id, "label": label})).collect();
        return Ok(format!("{}\n", json!({"logins": rows, "services": offered})));
    }
    if view.logins.is_empty() {
        let ids: Vec<&str> = services.iter().map(|(id, _)| id.as_str()).collect();
        return Ok(format!("no trading services yet; `omacharts trading login SERVICE` adds one, of {}\n", ids.join(", ")));
    }
    let lines: Vec<String> = view
        .logins
        .iter()
        .zip(&rows)
        .map(|(login, row)| {
            let data = if row["data"] == true { " · data" } else { "" };
            format!("{:<20} {:<22} {}{data}", login.name, label_of(&login.service), row["standing"].as_str().unwrap_or(""))
        })
        .collect();
    Ok(format!("{}\n", lines.join("\n")))
}

/// An amount as the panel writes it, or a dash.
fn amount(value: Option<f64>, currency: Option<&str>) -> String {
    value.map_or_else(|| "—".to_string(), |value| trading::money_text(value, currency))
}

fn signed(value: Option<f64>, currency: Option<&str>) -> String {
    value.map_or_else(|| "—".to_string(), |value| trading::pnl_text(value, currency))
}

fn accounts(store: &Store, live: Option<&dyn Live>, as_json: bool) -> Result<String, Fault> {
    let view = view_of(store, live);
    let default = view.default_account().map(Account::key);
    let rows: Vec<(Value, String)> = view
        .accounts
        .iter()
        .map(|account| {
            let key = account.key();
            let money = view.book.money(&key);
            let limits = money.and_then(|money| money.limits.as_ref());
            let currency = account.currency.as_deref();
            let balance = money.and_then(|money| money.balance).or(account.balance);
            let is_default = default.as_deref() == Some(key.as_str());
            let name = view.account_name(account);
            let value = json!({
                "account": key,
                "name": name,
                "login": view.login(&account.login).map(|login| login.name.clone()),
                "currency": account.currency,
                "balance": balance,
                "day_pnl": money.and_then(|money| money.day_pnl),
                "open_pnl": money.and_then(|money| money.unrealized),
                "loss_limit": limits.and_then(|limits| limits.loss_limit),
                "liquidates_at": limits.and_then(|limits| limits.liquidate_at),
                "max_order_qty": limits.and_then(|limits| limits.max_order_qty),
                "default": is_default,
            });
            let text = format!(
                "{}{:<18} {:<4} {:>12} day {:>11} open {:>11} loss limit {:>10} liquidates at {}",
                if is_default { "* " } else { "  " },
                name,
                currency.unwrap_or(""),
                amount(balance, currency),
                signed(money.and_then(|money| money.day_pnl), currency),
                signed(money.and_then(|money| money.unrealized), currency),
                amount(limits.and_then(|limits| limits.loss_limit), currency),
                amount(limits.and_then(|limits| limits.liquidate_at), currency),
            );
            (value, text)
        })
        .collect();
    match as_json {
        true => Ok(format!("{}\n", json!({"accounts": rows.into_iter().map(|(value, _)| value).collect::<Vec<_>>()}))),
        false if rows.is_empty() => Ok("no accounts; sign in to a trading service first\n".into()),
        false => Ok(format!("{}\n", rows.into_iter().map(|(_, text)| text.trim_end().to_string()).collect::<Vec<_>>().join("\n"))),
    }
}

fn price_text(price: Option<f64>, tick: Option<f64>) -> String {
    price.map_or_else(|| "—".to_string(), |price| omacharts_engine::grouped_decimals(price, crate::ui::trading_panel::decimals_of(tick)))
}

fn positions(live: &dyn Live, as_json: bool) -> Result<String, Fault> {
    let (view, _) = live.trading_view().ok_or_else(no_window)?;
    let held: Vec<&trading::Position> = view.book.positions.iter().filter(|held| held.net != 0).collect();
    let currency = |key: &str| view.accounts.iter().find(|account| account.key() == key).and_then(|account| account.currency.clone());
    if as_json {
        let rows: Vec<Value> = held
            .iter()
            .map(|held| {
                json!({
                    "account": held.account,
                    "name": view.name_of(&held.account),
                    "contract": held.contract,
                    "exchange": held.exchange,
                    "qty": held.net,
                    "average": held.avg,
                    "open_pnl": held.open_pnl,
                })
            })
            .collect();
        return Ok(format!("{}\n", json!({"positions": rows})));
    }
    if held.is_empty() {
        return Ok("no positions\n".into());
    }
    let lines: Vec<String> = held
        .iter()
        .map(|held| {
            format!(
                "{:<18} {:<8} {:>5} at {:>12}  {}",
                view.name_of(&held.account),
                held.contract,
                trading::net_text(held.net),
                price_text(held.avg, held.tick),
                signed(held.open_pnl, currency(&held.account).as_deref()),
            )
        })
        .collect();
    Ok(format!("{}\n", lines.join("\n")))
}

fn orders(live: &dyn Live, all: bool, as_json: bool) -> Result<String, Fault> {
    let (view, _) = live.trading_view().ok_or_else(no_window)?;
    let mut shown: Vec<&trading::Order> = view.book.orders.iter().filter(|order| all || order.status.working()).collect();
    shown.sort_by_key(|order| std::cmp::Reverse(order.at));
    if as_json {
        let rows: Vec<Value> = shown
            .iter()
            .map(|order| {
                json!({
                    "id": order.basket_id,
                    "at": order.at,
                    "account": order.account,
                    "name": view.name_of(&order.account),
                    "contract": order.contract,
                    "side": order.side.key(),
                    "qty": order.qty,
                    "filled": order.filled,
                    "type": order.kind.key(),
                    "price": order.price,
                    "status": order.status.word(),
                    "working": order.status.working(),
                    "filled_at": order.fill_price.or_else(|| order.average_fill()),
                })
            })
            .collect();
        return Ok(format!("{}\n", json!({"orders": rows})));
    }
    if shown.is_empty() {
        return Ok(if all { "no orders today\n" } else { "no open orders\n" }.into());
    }
    let lines: Vec<String> = shown
        .iter()
        .map(|order| {
            let time = chrono::DateTime::from_timestamp(order.at, 0)
                .map(|utc| chrono::DateTime::<chrono::Local>::from(utc).format("%H:%M:%S").to_string())
                .unwrap_or_default();
            let filled = order.fill_price.or_else(|| order.average_fill());
            format!(
                "{:<12} {time} {:<14} {:<8} {:<4} {:>4} {:<6} {:>12}  {}{}",
                order.basket_id,
                view.name_of(&order.account),
                order.contract,
                order.side.word(),
                order.qty,
                order.kind.word(),
                price_text(order.price, order.tick),
                order.status.word(),
                filled.map(|price| format!(" at {}", price_text(Some(price), order.tick))).unwrap_or_default(),
            )
        })
        .collect();
    Ok(format!("{}\n", lines.join("\n")))
}

/// `trading quantities`: the templates a chart's Qty offers its
/// quantities from, as the Quantities dialog edits them.
fn quantities(store: &Store, m: &clap::ArgMatches, as_json: bool, live: Option<&dyn Live>) -> Result<String, Fault> {
    use trading::sizes::{self, Applies, Category, SizeTemplate};
    let mut templates = sizes::load(store);
    let applies_text = |applies: &Applies| {
        let kinds: Vec<String> = applies.categories.iter().map(|category| category.label().to_lowercase()).collect();
        match applies.roots.is_empty() {
            true => format!("for {}", kinds.join(" and ")),
            false => format!("for {} · only {}", kinds.join(" and "), applies.roots.join(", ")),
        }
    };
    let listed = |templates: &[SizeTemplate]| -> Value {
        json!({"templates": templates.iter().map(|template| json!({
            "name": template.name,
            "quantities": template.sizes.iter().map(|size| size.get()).collect::<Vec<_>>(),
            "applies_to": template.applies_to.categories.iter().map(|category| category.label().to_lowercase()).collect::<Vec<_>>(),
            "symbols": template.applies_to.roots,
        })).collect::<Vec<_>>()})
    };
    match arg(m, "ACTION").map(String::as_str).unwrap_or("list") {
        "list" => {
            if as_json {
                return Ok(format!("{}\n", listed(&templates)));
            }
            let lines: Vec<String> = templates
                .iter()
                .map(|template| format!("{}: {} · {}", template.name, sizes::spell_sizes(&template.sizes), applies_text(&template.applies_to)))
                .collect();
            Ok(format!("{}\n", lines.join("\n")))
        }
        "set" => {
            let name = required(m, "NAME")?.trim().to_string();
            if name.is_empty() {
                return Err(Fault::usage("a template needs a name".into()));
            }
            let given = arg(m, "QUANTITIES").ok_or_else(|| Fault::usage("say its quantities, such as 1,2,5,10".into()))?;
            let quantities = sizes::parse_sizes(given).map_err(Fault::usage)?;
            let at = templates.iter().position(|template| template.name.eq_ignore_ascii_case(&name));
            let mut applies = at.map(|at| templates[at].applies_to.clone()).unwrap_or(Applies { categories: vec![Category::Futures], roots: Vec::new() });
            match arg(m, "applies-to").map(String::as_str) {
                Some("stocks") => applies.categories = vec![Category::Stocks],
                Some("futures") => applies.categories = vec![Category::Futures],
                Some(other) => return Err(Fault::usage(format!("--applies-to is stocks or futures, not {other:?}"))),
                None => {}
            }
            if let Some(symbols) = arg(m, "symbols") {
                applies.roots = sizes::parse_roots(symbols);
            }
            let template = SizeTemplate { name: name.clone(), sizes: quantities, applies_to: applies };
            let text = format!("{name}: {} · {}", sizes::spell_sizes(&template.sizes), applies_text(&template.applies_to));
            match at {
                Some(at) => templates[at] = template,
                None => templates.push(template),
            }
            sizes::save(store, &templates);
            adopt(live);
            said(as_json, listed(&templates), text)
        }
        "remove" => {
            let name = required(m, "NAME")?;
            let at = templates
                .iter()
                .position(|template| template.name.eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| Fault::not_found(format!("no quantities called {name:?}")))?;
            let gone = templates.remove(at);
            sizes::save(store, &templates);
            adopt(live);
            said(as_json, listed(&templates), format!("removed {}", gone.name))
        }
        other => Err(Fault::usage(format!("{other:?} is list, set or remove"))),
    }
}

/// The quantity `n` as chart `instrument`'s Qty offers it, as what is
/// stored with the chart: the template it comes from and the root.
pub fn picked(store: &Store, instrument: &omacharts_engine::Instrument, n: u32) -> Result<trading::sizes::Picked, Fault> {
    let templates = trading::sizes::load(store);
    let template = trading::sizes::template_for(&templates, instrument).ok_or_else(|| {
        Fault::refused(format!("no quantities apply to {}; `omacharts trading quantities set` makes some", instrument.display_symbol()))
    })?;
    let at = template.sizes.iter().position(|size| size.get() == n).ok_or_else(|| {
        Fault::refused(format!(
            "{} offers {} for {}, not {n}",
            template.name,
            trading::sizes::spell_sizes(&template.sizes),
            instrument.display_symbol()
        ))
    })?;
    trading::sizes::pick(&templates, instrument, at + 1).ok_or_else(|| Fault::refused(format!("{n} is not offered")))
}
