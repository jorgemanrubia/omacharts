//! `omacharts trading` in the window: the same hands the chart, its menu,
//! its keys and the Trading panel use, given what was typed.
//!
//! Nothing here judges an order. Each verb goes through the very function
//! the chart's own gesture does, which checks it and sends it, or says in
//! the Log why it did not; the command listens while it runs and answers
//! with what was said. A refusal is the command failing, in the Log's
//! words. What the trading service answers later is in the Log, and in
//! `trading orders`, as it is for an order from the chart.

use std::rc::Rc;

use adw::prelude::*;

use omacharts_engine::log::Level;

use super::Window;
use crate::cli::trading::{Act, Said};
use crate::cli::Fault;
use crate::log_words;
use crate::trading::{self, OrderKind};
use crate::ui::pane::ChartPane;

impl Window {
    /// Trading as this window has it, and how each login stands.
    pub fn trading_now(&self) -> (trading::View, Vec<(String, trading::Standing)>) {
        let view = self.trading.borrow().clone();
        let standings = view
            .logins
            .iter()
            .map(|login| {
                let data = view.data_login.as_deref() == Some(login.id.as_str());
                (login.id.clone(), self.desk.standing(login, data))
            })
            .collect();
        (view, standings)
    }

    /// The trading services a login can be added for, as "Add a trading
    /// service…" offers them.
    pub fn trading_services(&self) -> Vec<(String, String)> {
        self.desk.services().iter().map(|service| (service.id().to_string(), service.label().to_string())).collect()
    }

    /// Open `service`'s sign-in dialog over the window, for a new login,
    /// which signs in once it is saved: "Add a trading service…".
    pub fn ask_login(self: &Rc<Self>, service: &str) -> Result<(), String> {
        let dialog = crate::ui::sign_in_dialogs::login_dialog(service)
            .ok_or_else(|| format!("{service} cannot be signed in to from this build"))?;
        let taken = trading::names_taken(&self.store, None);
        let this = Rc::downgrade(self);
        dialog(
            self.window.upcast_ref(),
            None,
            &taken,
            Box::new(move |typed| {
                let Some(this) = this.upgrade() else { return };
                let secrets = crate::ui::trading_settings::keyring_for(&typed.service);
                match trading::add_login(&this.store, secrets.as_ref(), &typed) {
                    Ok(id) => {
                        let _ = this.sign_in_login(&id);
                    }
                    Err(error) => this.say(log_words::trouble(&typed.name, "Login not saved", &error)),
                }
                this.desk.tell();
            }),
        );
        Ok(())
    }

    /// Sign the login `id` in, with its saved password; with none, its
    /// dialog again, prefilled, for one: true then.
    pub fn sign_in_login(self: &Rc<Self>, id: &str) -> Result<bool, String> {
        let logins = trading::logins(&self.store);
        let login = logins.iter().find(|login| login.id == id).ok_or_else(|| format!("no login {id}"))?;
        if self.desk.service_for(login).is_none() {
            return Err(format!("no trading service called {} in this build", login.service));
        }
        if self.desk.live(id) {
            return Err(format!("{} is signed in already", login.name));
        }
        if login.signed_out || login.user.is_empty() {
            let dialog = crate::ui::sign_in_dialogs::login_dialog(&login.service)
                .ok_or_else(|| format!("{} cannot be signed in to from this build", login.service))?;
            let taken = trading::names_taken(&self.store, Some(id));
            let this = Rc::downgrade(self);
            let id = id.to_string();
            dialog(
                self.window.upcast_ref(),
                Some(login),
                &taken,
                Box::new(move |typed| {
                    let Some(this) = this.upgrade() else { return };
                    let secrets = crate::ui::trading_settings::keyring_for(&typed.service);
                    if let Err(error) = trading::retype_login(&this.store, secrets.as_ref(), &id, &typed) {
                        this.say(log_words::trouble(&typed.name, "Login not saved", &error));
                    }
                    let _ = this.sign_in_login(&id);
                    this.desk.tell();
                }),
            );
            return Ok(true);
        }
        let data = trading::data_login_id(&self.store, &logins).as_deref() == Some(id);
        self.desk.sign_in(login, data);
        self.desk.tell();
        Ok(false)
    }

    /// Read Trading again after a command changed it in the store, and
    /// what hangs on it: the quantities and where the labels sit.
    pub fn adopt_trading_settings(self: &Rc<Self>) {
        self.adopt_trading();
        self.adopt_sizes();
        self.adopt_order_labels();
        self.desk.tell();
    }

    /// Carry out `act`, and answer with what was said doing it: a refusal
    /// as the command's failure.
    pub fn trade_command(self: &Rc<Self>, act: Act) -> Result<Said, Fault> {
        match act {
            Act::Order { chart, side, kind, price, quantity } => {
                let pane = self.chart_to_trade(chart)?;
                let size = match quantity {
                    None => None,
                    Some(n) => {
                        let traded = pane.traded().ok_or_else(|| Fault::refused("this chart charts nothing to trade".into()))?;
                        Some(crate::cli::trading::picked(&self.store, &traded, n)?.size)
                    }
                };
                self.heard_doing(|| self.order_on(&pane, side, kind, None, price, size))
            }
            Act::Exit { chart, target, stop } => {
                let pane = self.chart_to_trade(chart)?;
                self.heard_doing(|| match (target, stop) {
                    (Some(target), Some(stop)) => self.bracket_mark(&pane, stop, target),
                    (Some(target), None) => self.exit_mark(&pane, OrderKind::Limit, target),
                    (None, Some(stop)) => self.exit_mark(&pane, OrderKind::Stop, stop),
                    (None, None) => {}
                })
            }
            Act::Modify { basket, account, price } => {
                let order = self.working_order(&basket, account.as_deref())?;
                // Moved from a chart of it, which judges the new price as a
                // drag would: the side of the market, the limits, the tick.
                let pane = self.chart_trading(&order.account, &order.contract).ok_or_else(|| {
                    Fault::refused(format!(
                        "Not moved · no chart trades {} on {}: open one to move it from",
                        order.contract,
                        self.account_label(&order.account)
                    ))
                })?;
                self.heard_doing(|| self.move_mark(&pane, &basket, price))
            }
            Act::Cancel { basket, account } => {
                let order = self.working_order(&basket, account.as_deref())?;
                self.heard_doing(|| {
                    let name = self.account_label(&order.account);
                    self.say(log_words::note(&name, super::trade::cancel_words(&order)));
                    self.on_session(&order.account, move |session, number| session.cancel(number, &basket), |_, _, ()| {});
                })
            }
            Act::CancelAll { account } => {
                let can = self.trading.borrow().book.capabilities_of(&account).is_none_or(|can| can.cancel_all);
                if !can {
                    return Err(Fault::refused(format!("{} cannot cancel all at once", self.account_label(&account))));
                }
                self.heard_doing(|| {
                    self.say(log_words::note(&self.account_label(&account), "Cancel all open orders"));
                    self.on_session(&account, |session, number| session.cancel_all(number), |_, _, ()| {});
                })
            }
            Act::FlattenChart { chart } => {
                let pane = self.chart_to_trade(chart)?;
                self.heard_doing(|| self.flat_on(&pane))
            }
            Act::Flatten { account, contract } => {
                let held = self.trading.borrow().book.position(&account, &contract).filter(|held| held.net != 0).cloned();
                let held = held.ok_or_else(|| Fault::refused(format!("Not flattened · nothing held in {contract}")))?;
                self.heard_doing(|| {
                    let name = self.account_label(&account);
                    self.say(log_words::note(&name, format!("Flatten {}", held.contract)));
                    let (contract, exchange) = (held.contract.clone(), held.exchange.clone());
                    self.on_session(
                        &account,
                        move |session, number| session.flatten(number, &contract, &exchange),
                        |this, name, uncancelled| {
                            if uncancelled > 0 {
                                this.say(log_words::flattened_uncancelled(name, uncancelled));
                            }
                        },
                    );
                })
            }
        }
    }

    /// Chart `id` on screen, to trade from.
    fn chart_to_trade(&self, id: u32) -> Result<Rc<ChartPane>, Fault> {
        self.pane(id).ok_or_else(|| Fault::not_found(format!("no chart {id} on screen")))
    }

    /// The order `basket`, while it is working.
    fn working_order(&self, basket: &str, account: Option<&str>) -> Result<trading::Order, Fault> {
        let view = self.trading.borrow();
        let found: Vec<&trading::Order> =
            view.book.orders.iter().filter(|order| order.basket_id == basket && account.is_none_or(|key| order.account == key)).collect();
        let order = match found.as_slice() {
            [] => return Err(Fault::not_found(format!("no order {basket} today; `omacharts trading orders` lists them"))),
            [order] => (*order).clone(),
            // Two accounts can each have an order of the same id: which
            // one is meant has to be said, never guessed.
            several => {
                let names: Vec<String> = several.iter().map(|order| view.name_of(&order.account)).collect();
                return Err(Fault::ambiguous(format!("{basket} is an order on {}; say which with --account", names.join(" and "))));
            }
        };
        match order.status.working() {
            true => Ok(order),
            false => Err(Fault::refused(format!("{basket} is {} already: nothing to change", order.status.word()))),
        }
    }

    /// A chart on screen trading `contract` on the account `account`: the
    /// focused one when it does.
    fn chart_trading(&self, account: &str, contract: &str) -> Option<Rc<ChartPane>> {
        let focused = self.focused_pane();
        let panes = self.panes.borrow().clone();
        std::iter::once(focused).chain(panes).find(|pane| {
            self.order_target(pane).is_ok_and(|target| target.account.key() == account && target.contract.eq_ignore_ascii_case(contract))
        })
    }

    /// Run `act`, listening to what it says: an error is the answer as a
    /// refusal; otherwise what was said, who said it first.
    fn heard_doing(&self, act: impl FnOnce()) -> Result<Said, Fault> {
        self.messages.heard.replace(Some(Vec::new()));
        act();
        let heard = self.messages.heard.replace(None).unwrap_or_default();
        let refused: Vec<String> =
            heard.iter().filter(|entry| entry.level == Level::Error).map(|entry| format!("{}: {}", entry.source, entry.text)).collect();
        if !refused.is_empty() {
            return Err(Fault::refused(refused.join("\n")));
        }
        if heard.is_empty() {
            return Err(Fault::refused("nothing was sent".into()));
        }
        Ok(heard.into_iter().map(|entry| (entry.source, entry.text)).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use serde_json::Value;

    use super::super::trade::tests::{long_es, settle};
    use super::*;
    use crate::trading::Session as _;

    /// Run a command against the window, as a terminal would: its exit
    /// code, and what it printed or why it failed.
    fn command(window: &Rc<Window>, line: &str) -> (u8, String) {
        let args: Vec<String> = std::iter::once("omacharts").chain(line.split_whitespace()).map(String::from).collect();
        let live: &dyn crate::cli::Live = window;
        let outcome = crate::cli::run(&args, &window.store, Some(live), &crate::cli::Here);
        (outcome.code, if outcome.code == 0 { outcome.out } else { outcome.err })
    }

    /// [`settle`] until `done`, and until nothing asked of the account is
    /// still on its way: the next command would be refused for it, as the
    /// next click would.
    fn settled(window: &Rc<Window>, key: &str, done: impl Fn() -> bool) -> bool {
        settle(window, || done() && !window.trade.placing.busy(key))
    }

    fn json(window: &Rc<Window>, line: &str) -> Value {
        let (code, out) = command(window, line);
        assert_eq!(code, 0, "{line}: {out}");
        serde_json::from_str(&out).unwrap_or_else(|_| panic!("{line}: not JSON: {out}"))
    }

    /// The chart's last price, on its tick, and that price `ticks` away.
    fn away(last: f64, ticks: i32) -> String {
        format!("{:.2}", (last / 0.25).round() * 0.25 + f64::from(ticks) * 0.25)
    }

    /// From a terminal: a limit placed, listed, moved and cancelled; one
    /// on the wrong side of the market refused in the chart's words; a
    /// target and a stop placed together for the position; the position
    /// flattened. All on the tests' session, through the chart's account.
    #[test]
    fn orders_are_placed_moved_cancelled_and_flattened_from_a_terminal() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()), "no price on the chart");
            let last = pane.view.last_price().unwrap();

            let held = json(&window, "trading positions --json");
            assert_eq!(held["positions"][0]["contract"], "ESZ6", "{held}");
            assert_eq!(held["positions"][0]["qty"], 1, "{held}");

            // Crossed: a buy limit over the market would fill at once.
            let (code, why) = command(&window, &format!("trading buy --limit {}", away(last, 40)));
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");
            assert!(session.book().orders.iter().all(|order| order.kind != OrderKind::Limit), "a crossed limit was sent");

            let (code, out) = command(&window, &format!("trading buy --limit {} --quantity 2", away(last, -40)));
            assert_eq!(code, 0, "{out}");
            assert!(out.contains("Buy Limit 2 ESZ6 at"), "{out}");
            let working = || session.book().orders.iter().filter(|order| order.status.working()).cloned().collect::<Vec<_>>();
            assert!(settled(&window, &key, || working().len() == 1), "not placed");
            let listed = json(&window, "trading orders --json");
            let id = listed["orders"][0]["id"].as_str().expect("an id").to_string();
            assert_eq!(listed["orders"][0]["qty"], 2, "{listed}");

            // A quantity the chart does not offer is refused.
            let (code, why) = command(&window, "trading sell --quantity 12");
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");
            assert!(why.contains("1, 2, 3"), "{why}");

            let moved_to = away(last, -60);
            let (code, out) = command(&window, &format!("trading modify {id} --price {moved_to}"));
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || working().first().and_then(|order| order.price) == moved_to.parse().ok()), "not moved");

            let (code, out) = command(&window, &format!("trading cancel {id}"));
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || working().is_empty()), "not cancelled");
            let (code, why) = command(&window, &format!("trading cancel {id}"));
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");
            assert!(json(&window, "trading orders --all --json")["orders"].as_array().unwrap().len() >= 2);

            let (code, out) = command(&window, &format!("trading exit --target {} --stop {}", away(last, 40), away(last, -40)));
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || working().len() == 2), "no exits");
            let mut kinds: Vec<OrderKind> = working().iter().map(|order| order.kind).collect();
            kinds.sort_by_key(|kind| kind.key());
            assert_eq!(kinds, vec![OrderKind::Limit, OrderKind::Stop]);
            assert!(working().iter().all(|order| order.side == trading::Side::Sell && order.qty == 1));
            // A second pair while those work: refused.
            let (code, why) = command(&window, &format!("trading exit --target {} --stop {}", away(last, 44), away(last, -44)));
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");

            let (code, out) = command(&window, "trading flatten");
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || session.book().position("951", "ESZ6").is_none()), "not flattened");
            assert!(settled(&window, &key, || working().is_empty()), "exits left working");
            // Nothing to do is said, and is no failure: flat is what was asked.
            let (code, out) = command(&window, "trading flatten");
            assert_eq!(code, 0, "{out}");
            assert!(out.contains("Already flat in ESZ6"), "{out}");
        });
    }

    /// The accounts with their money and the default, the logins with how
    /// they stand, cancel all, and the panel's flatten of a contract named.
    #[test]
    fn accounts_logins_cancel_all_and_a_contract_flattened() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            let last = pane.view.last_price().unwrap();

            let accounts = json(&window, "trading accounts --json");
            assert_eq!(accounts["accounts"][0]["account"], key.as_str(), "{accounts}");
            assert_eq!(accounts["accounts"][0]["default"], true, "{accounts}");
            let logins = json(&window, "trading logins --json");
            assert_eq!(logins["logins"][0]["live"], true, "{logins}");
            assert_eq!(logins["logins"][0]["accounts"][0], "Rithmic Test", "{logins}");

            let working = || session.book().orders.iter().filter(|order| order.status.working()).count();
            for (ticks, placed) in [(-40, 1), (-48, 2)] {
                let (code, out) = command(&window, &format!("trading buy --limit {}", away(last, ticks)));
                assert_eq!(code, 0, "{out}");
                assert!(settled(&window, &key, || working() == placed), "not placed");
            }
            assert_eq!(working(), 2);
            let (code, out) = command(&window, "trading cancel --all");
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || working() == 0), "not all cancelled");

            let (code, why) = command(&window, "trading flatten NQZ6");
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");
            let (code, out) = command(&window, "trading flatten ESZ6");
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || session.book().position("951", "ESZ6").is_none()), "not flattened");
        });
    }

    /// A chart's account, quantity, trading symbol and fills, set from a
    /// terminal, are the window's at once and stored with the chart.
    #[test]
    fn a_chart_s_trading_is_set_from_a_terminal() {
        crate::ui::on_gtk(|| {
            let (window, _, _, key) = long_es(trading::Linking::AfterTheFact);
            let (code, out) = command(&window, "chart set --quantity 3 --trading-symbol MES --show-fills on");
            assert_eq!(code, 0, "{out}");
            let pane = window.focused_pane();
            assert_eq!(pane.trading_symbol.borrow().as_ref().map(|traded| traded.symbol.clone()).as_deref(), Some("MES"));
            assert_eq!(window.size_of(&pane).get(), 3);
            assert!(pane.shows_fills.get());
            let shown = json(&window, "chart list --json")["charts"][0].clone();
            assert_eq!(shown["trading_symbol"], "MES", "{shown}");
            assert_eq!(shown["quantity"], 3, "{shown}");
            assert_eq!(shown["shows_fills"], true, "{shown}");

            let (code, why) = command(&window, "chart set --trading-symbol NQ");
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");
            let (code, why) = command(&window, "chart set --quantity 12");
            assert_eq!(code, crate::cli::EXIT_REFUSED, "{why}");

            let (code, out) = command(&window, "chart set --account 951");
            assert_eq!(code, 0, "{out}");
            assert_eq!(window.focused_pane().account.borrow().as_deref(), Some(key.as_str()));
            let (code, out) = command(&window, "chart set --account default --trading-symbol none");
            assert_eq!(code, 0, "{out}");
            assert!(window.focused_pane().account.borrow().is_none());
            assert!(window.focused_pane().trading_symbol.borrow().is_none());
            drop(pane);
        });
    }

    /// Two accounts can each have an order of the same id. Named alone,
    /// it is refused as ambiguous rather than guessed; with --account,
    /// the one on that account is cancelled.
    #[test]
    fn an_order_id_two_accounts_share_needs_its_account() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            assert!(settle(&window, || pane.view.last_price().is_some()));
            let last = pane.view.last_price().unwrap();
            let (code, out) = command(&window, &format!("trading buy --limit {}", away(last, -40)));
            assert_eq!(code, 0, "{out}");
            let working = || session.book().orders.iter().filter(|order| order.status.working()).count();
            assert!(settled(&window, &key, || working() == 1), "not placed");
            let id = json(&window, "trading orders --json")["orders"][0]["id"].as_str().unwrap().to_string();
            // The same id on another account, as a second login's could be.
            {
                let mut view = window.trading.borrow_mut();
                let mut twin = view.book.orders.iter().find(|order| order.basket_id == id).cloned().unwrap();
                twin.account = format!("{key}-twin");
                view.book.orders.push(twin);
            }
            let (code, why) = command(&window, &format!("trading cancel {id}"));
            assert_eq!(code, crate::cli::EXIT_AMBIGUOUS, "{why}");
            assert!(why.contains("--account"), "{why}");
            assert_eq!(working(), 1, "an ambiguous id cancelled something");
            let (code, out) = command(&window, &format!("trading cancel {id} --account 951"));
            assert_eq!(code, 0, "{out}");
            assert!(settled(&window, &key, || working() == 0), "not cancelled");
        });
    }
}

