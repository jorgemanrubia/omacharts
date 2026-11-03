//! The trading panel, under the charts: the accounts, the positions held
//! and the orders of the day, with "Cancel all" for the default account.
//!
//! Only ever built once a broker login exists, and only the first time it
//! is shown: a window with no login has no panel, no divider for it and no
//! button to open it. Drawn again from the window's [`View`] whenever a
//! session pushes something, never on a timer.

use std::rc::Rc;

use adw::prelude::*;

use crate::trading::{self, OrderKind, Status, View};

/// A call about an account and an order: the account's key, the basket.
pub type OnOrder = Box<dyn Fn(&str, &str)>;
/// A call about an account's position: its key, the contract, the exchange.
pub type OnPosition = Box<dyn Fn(&str, &str, &str)>;

/// What the panel asks of the window: the orders and positions belong to
/// sessions only the window can reach.
pub struct Hands {
    /// Cancel the order `basket` on the account with this key.
    pub cancel: OnOrder,
    /// Flatten the account's position in a contract, on an exchange.
    pub flatten: OnPosition,
    /// Cancel every working order on the account with this key.
    pub cancel_all: Box<dyn Fn(&str)>,
}

pub struct TradingPanel {
    pub root: gtk::Box,
    stack: gtk::Stack,
    accounts: gtk::Box,
    positions: gtk::Box,
    orders: gtk::Box,
    cancel_all: gtk::Button,
    /// The default account's key, as last drawn.
    default: Rc<std::cell::RefCell<Option<String>>>,
    hands: Rc<Hands>,
}

/// How many places a price is written to: its tick's, when the broker said
/// it, else two.
pub fn decimals_of(tick: Option<f64>) -> usize {
    tick.map(|tick| omacharts_engine::on_tick(0.0, tick).1).unwrap_or(2)
}

fn price_text(price: Option<f64>, tick: Option<f64>) -> String {
    match price {
        Some(price) => omacharts_engine::grouped_decimals(price, decimals_of(tick)),
        None => "—".into(),
    }
}

fn time_text(at: i64) -> String {
    gtk::glib::DateTime::from_unix_local(at)
        .and_then(|time| time.format("%H:%M:%S"))
        .map(|text| text.to_string())
        .unwrap_or_default()
}

fn cell(text: &str, numeric: bool) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(if numeric { 1.0 } else { 0.0 });
    if numeric {
        label.add_css_class("numeric");
    }
    label
}

fn heading(text: &str, numeric: bool) -> gtk::Label {
    let label = cell(text, numeric);
    label.add_css_class("trading-heading");
    label
}

/// A table of `headings` with `rows` under them, or `empty` with none.
fn table(into: &gtk::Box, headings: &[(&str, bool)], rows: Vec<Vec<gtk::Widget>>, empty: &str) {
    while let Some(child) = into.first_child() {
        into.remove(&child);
    }
    if rows.is_empty() {
        let label = gtk::Label::new(Some(empty));
        label.add_css_class("dim-label");
        label.set_vexpand(true);
        into.append(&label);
        return;
    }
    let grid = gtk::Grid::new();
    grid.set_column_spacing(18);
    grid.set_row_spacing(4);
    grid.add_css_class("trading-table");
    for (column, (text, numeric)) in headings.iter().enumerate() {
        grid.attach(&heading(text, *numeric), column as i32, 0, 1, 1);
    }
    for (row, cells) in rows.into_iter().enumerate() {
        for (column, widget) in cells.into_iter().enumerate() {
            grid.attach(&widget, column as i32, row as i32 + 1, 1, 1);
        }
    }
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_child(Some(&grid));
    scrolled.set_vexpand(true);
    scrolled.set_propagate_natural_height(false);
    into.append(&scrolled);
}

impl TradingPanel {
    pub fn new(hands: Hands) -> Rc<TradingPanel> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("trading-panel");
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        let page = |name: &str, title: &str| {
            let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
            page.add_css_class("trading-page");
            stack.add_titled(&page, Some(name), title);
            page
        };
        let accounts = page("accounts", "Accounts");
        let positions = page("positions", "Positions");
        let orders = page("orders", "Orders");

        let switcher = gtk::StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        let cancel_all = gtk::Button::with_label("Cancel all");
        cancel_all.add_css_class("flat");
        cancel_all.set_halign(gtk::Align::End);
        cancel_all.set_hexpand(true);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.add_css_class("trading-tabs");
        bar.append(&switcher);
        bar.append(&cancel_all);
        root.append(&bar);
        root.append(&stack);

        let hands = Rc::new(hands);
        let default: Rc<std::cell::RefCell<Option<String>>> = Rc::default();
        let (pressed, on) = (hands.clone(), default.clone());
        cancel_all.connect_clicked(move |_| {
            if let Some(key) = on.borrow().clone() {
                (pressed.cancel_all)(&key);
            }
        });
        Rc::new(TradingPanel { root, stack, accounts, positions, orders, cancel_all, default, hands })
    }

    /// The tab showing, by name: "accounts", "positions", "orders".
    pub fn tab(&self) -> Option<String> {
        self.stack.visible_child_name().map(|name| name.to_string())
    }

    pub fn show_tab(&self, name: &str) {
        self.stack.set_visible_child_name(name);
    }

    /// Draw everything again from `view`.
    pub fn redraw(&self, view: &View) {
        self.redraw_accounts(view);
        self.redraw_positions(view);
        self.redraw_orders(view);

        let default = view.default_account().map(trading::Account::key);
        let working = default
            .as_deref()
            .is_some_and(|key| view.book.orders.iter().any(|order| order.account == key && order.status.working()));
        self.cancel_all.set_sensitive(working);
        let can = default.as_deref().and_then(|key| view.book.capabilities_of(key)).is_none_or(|can| can.cancel_all);
        self.cancel_all.set_visible(can);
        let tip = match view.default_account() {
            Some(account) => format!("Cancel every working order on {}", trading::account_name(account, &view.logins)),
            None => "Cancel every working order".to_string(),
        };
        self.cancel_all.set_tooltip_text(Some(&tip));
        // Read when it is pressed, so it cancels on the default in force.
        *self.default.borrow_mut() = default;
    }

    fn name_of(view: &View, key: &str) -> String {
        view.accounts
            .iter()
            .find(|account| account.key() == key)
            .map(|account| trading::account_name(account, &view.logins))
            .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(key).to_string())
    }

    fn currency_of<'a>(view: &'a View, key: &str) -> Option<&'a str> {
        view.accounts.iter().find(|account| account.key() == key).and_then(|account| account.currency.as_deref())
    }

    fn redraw_accounts(&self, view: &View) {
        let rows = view
            .accounts
            .iter()
            .map(|account| {
                let money = view.book.money(&account.key());
                let currency = account.currency.as_deref();
                let amount = |value: Option<f64>, signed: bool| match value {
                    Some(value) if signed => trading::pnl_text(value, currency),
                    Some(value) => trading::money_text(value, currency),
                    None => "—".to_string(),
                };
                vec![
                    cell(&trading::account_name(account, &view.logins), false).upcast(),
                    cell(currency.unwrap_or("—"), false).upcast(),
                    cell(&amount(money.and_then(|m| m.balance).or(account.balance), false), true).upcast(),
                    cell(&amount(money.and_then(|m| m.realized_day), true), true).upcast(),
                    cell(&amount(money.and_then(|m| m.unrealized), true), true).upcast(),
                ]
            })
            .collect();
        table(
            &self.accounts,
            &[("Account", false), ("Currency", false), ("Balance", true), ("Day P&L", true), ("Open P&L", true)],
            rows,
            "No accounts listed yet",
        );
    }

    fn redraw_positions(&self, view: &View) {
        let rows = view
            .book
            .positions
            .iter()
            .filter(|held| held.net != 0)
            .map(|held| {
                let flatten = gtk::Button::with_label("Flatten");
                flatten.add_css_class("flat");
                let hands = self.hands.clone();
                let (account, contract, exchange) = (held.account.clone(), held.contract.clone(), held.exchange.clone());
                flatten.connect_clicked(move |_| (hands.flatten)(&account, &contract, &exchange));
                flatten.set_visible(view.book.capabilities_of(&held.account).is_some_and(|can| can.flatten));
                let pnl = held
                    .open_pnl
                    .map(|pnl| trading::pnl_text(pnl, Self::currency_of(view, &held.account)))
                    .unwrap_or_else(|| "—".into());
                vec![
                    cell(&Self::name_of(view, &held.account), false).upcast(),
                    cell(&held.contract, false).upcast(),
                    cell(&trading::net_text(held.net), true).upcast(),
                    cell(&price_text(held.avg, held.tick), true).upcast(),
                    cell(&pnl, true).upcast(),
                    flatten.upcast(),
                ]
            })
            .collect();
        table(
            &self.positions,
            &[("Account", false), ("Contract", false), ("Qty", true), ("Avg", true), ("Open P&L", true), ("", false)],
            rows,
            "No positions",
        );
    }

    fn redraw_orders(&self, view: &View) {
        let rows = view
            .book
            .orders
            .iter()
            .map(|order| {
                let status = match &order.status {
                    Status::Rejected(why) => format!("Rejected: {why}"),
                    status => status.word().to_string(),
                };
                let price = match order.kind {
                    OrderKind::Market => "Market".to_string(),
                    _ => price_text(order.price, order.tick),
                };
                let status_cell = cell(&status, false);
                status_cell.set_ellipsize(gtk::pango::EllipsizeMode::End);
                status_cell.set_max_width_chars(40);
                if let Status::Rejected(why) = &order.status {
                    status_cell.set_tooltip_text(Some(why));
                }
                let mut cells: Vec<gtk::Widget> = vec![
                    cell(&time_text(order.at), true).upcast(),
                    cell(&Self::name_of(view, &order.account), false).upcast(),
                    cell(&order.contract, false).upcast(),
                    cell(order.side.word(), false).upcast(),
                    cell(&order.qty.to_string(), true).upcast(),
                    cell(order.kind.word(), false).upcast(),
                    cell(&price, true).upcast(),
                    status_cell.upcast(),
                    cell(&price_text(order.fill_price, order.tick), true).upcast(),
                ];
                if order.status.working() {
                    let cancel = gtk::Button::with_label("Cancel");
                    cancel.add_css_class("flat");
                    let hands = self.hands.clone();
                    let (account, basket) = (order.account.clone(), order.basket_id.clone());
                    cancel.connect_clicked(move |_| (hands.cancel)(&account, &basket));
                    cells.push(cancel.upcast());
                }
                cells
            })
            .collect();
        table(
            &self.orders,
            &[
                ("Time", true),
                ("Account", false),
                ("Contract", false),
                ("Side", false),
                ("Qty", true),
                ("Type", false),
                ("Price", true),
                ("Status", false),
                ("Fill", true),
                ("", false),
            ],
            rows,
            "No orders",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_price_is_written_to_its_tick() {
        assert_eq!(decimals_of(Some(0.25)), 2);
        assert_eq!(decimals_of(Some(0.001)), 3);
        assert_eq!(decimals_of(None), 2);
        assert_eq!(price_text(Some(7_612.25), Some(0.25)), "7,612.25");
    }
}
