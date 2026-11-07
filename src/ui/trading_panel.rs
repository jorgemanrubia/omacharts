//! The trading panel, under the charts: the accounts, the positions held
//! and the orders of the day, with "Cancel all · Lucid" for the default
//! account.
//!
//! Only ever built once a broker login exists, and only the first time it
//! is shown: a window with no login has no panel, no divider for it and no
//! button to open it. Drawn again from the window's [`View`] whenever a
//! session pushes something, never on a timer.
//!
//! The Orders tab shows the open orders, or all of the day's: a pair of
//! toggles beside Cancel all, there only while that tab is in front,
//! which hide and show rows already drawn rather than drawing them again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use crate::store::Store;
use crate::trading::{self, Order, OrderKind, Side, Stage, Status, View};

/// Where the Orders tab was left: `open` or `all`.
pub const ORDERS_FILTER: &str = "orders_filter";

/// Which of the day's orders the Orders tab shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrdersFilter {
    /// Still to fill or be cancelled: pending at the broker, working,
    /// part filled. The same orders Cancel all would cancel.
    Open,
    /// Every order of the day, whatever became of it.
    All,
}

impl OrdersFilter {
    pub fn shows(self, status: &Status) -> bool {
        match self {
            OrdersFilter::Open => status.working(),
            OrdersFilter::All => true,
        }
    }

    fn key(self) -> &'static str {
        match self {
            OrdersFilter::Open => "open",
            OrdersFilter::All => "all",
        }
    }

    /// As last left, which is Open until somebody chooses All.
    fn load(store: &Store) -> OrdersFilter {
        match store.setting(ORDERS_FILTER).as_deref() {
            Some("all") => OrdersFilter::All,
            _ => OrdersFilter::Open,
        }
    }

    fn empty_words(self) -> &'static str {
        match self {
            OrdersFilter::Open => "No open orders",
            OrdersFilter::All => "No orders",
        }
    }
}

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
    /// The three tabs, by name: "accounts", "positions", "orders".
    stack: gtk::Stack,
    accounts: gtk::Box,
    positions: gtk::Box,
    orders: gtk::Box,
    cancel_all: gtk::Button,
    /// The default account's key, as last drawn.
    default: Rc<RefCell<Option<String>>>,
    hands: Rc<Hands>,
    store: Rc<Store>,
    filter: Cell<OrdersFilter>,
    /// The Open and All toggles, with the box they stand in.
    filter_toggles: (gtk::Box, gtk::ToggleButton, gtk::ToggleButton),
    /// The orders drawn, each row's cells with whether the order is open,
    /// and what is drawn for a filter that leaves none: a `redraw` builds
    /// them, a change of filter only shows and hides them.
    order_rows: RefCell<Vec<(bool, Vec<gtk::Widget>)>>,
    orders_drawn: RefCell<Option<(gtk::ScrolledWindow, gtk::Label)>>,
    /// The orders as last drawn, with their accounts' names, so a push
    /// that moved none of them draws nothing again. None before the first.
    drawn: RefCell<Option<(Vec<Order>, Vec<String>)>>,
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

/// A side or a position's size, in the theme's colour for it: the one the
/// chart's marks and the header's Buy and Sell wear, so the panel agrees
/// with them. Numeric for a size, which is read down a column.
fn side_cell(text: &str, class: &str) -> gtk::Label {
    let label = cell(text, text.starts_with(['+', '-']));
    label.add_css_class(class);
    label
}

/// A signed P&L, in the theme's profit or loss colour.
fn pnl_cell(value: Option<f64>, currency: Option<&str>) -> gtk::Label {
    let Some(value) = value else { return cell("—", true) };
    let label = cell(&trading::pnl_text(value, currency), true);
    match value {
        value if value > 0.0 => label.add_css_class("pnl-gain"),
        value if value < 0.0 => label.add_css_class("pnl-loss"),
        _ => {}
    }
    label
}

fn heading(text: &str, numeric: bool) -> gtk::Label {
    let label = cell(text, numeric);
    label.add_css_class("trading-heading");
    label
}

fn clear(into: &gtk::Box) {
    while let Some(child) = into.first_child() {
        into.remove(&child);
    }
}

/// What a tab says with nothing to list.
fn empty_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.add_css_class("dim-label");
    label.set_vexpand(true);
    label
}

/// A table of `headings` with `rows` under them, scrolling.
fn grid_of(headings: &[(&str, bool)], rows: &[Vec<gtk::Widget>]) -> gtk::ScrolledWindow {
    let grid = gtk::Grid::new();
    grid.set_column_spacing(18);
    grid.set_row_spacing(4);
    grid.add_css_class("trading-table");
    for (column, (text, numeric)) in headings.iter().enumerate() {
        grid.attach(&heading(text, *numeric), column as i32, 0, 1, 1);
    }
    for (row, cells) in rows.iter().enumerate() {
        for (column, widget) in cells.iter().enumerate() {
            grid.attach(widget, column as i32, row as i32 + 1, 1, 1);
        }
    }
    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_child(Some(&grid));
    scrolled.set_vexpand(true);
    scrolled.set_propagate_natural_height(false);
    scrolled
}

/// A table of `headings` with `rows` under them, or `empty` with none.
fn table(into: &gtk::Box, headings: &[(&str, bool)], rows: Vec<Vec<gtk::Widget>>, empty: &str) {
    clear(into);
    match rows.is_empty() {
        true => into.append(&empty_label(empty)),
        false => into.append(&grid_of(headings, &rows)),
    }
}

const ORDER_HEADINGS: &[(&str, bool)] = &[
    ("Time", true),
    ("Account", false),
    ("Contract", false),
    ("Side", false),
    ("Qty", true),
    ("Type", false),
    ("Price", true),
    ("Status", false),
    ("Filled at", true),
    ("", false),
];

impl TradingPanel {
    pub fn new(hands: Hands, store: Rc<Store>) -> Rc<TradingPanel> {
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

        // Open or All, set like the tabs: the same flat toggles, one of
        // them down. Only with the Orders tab in front, since it is that
        // tab's.
        let filter = OrdersFilter::load(&store);
        let toggles = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let open = gtk::ToggleButton::with_label("Open");
        let all = gtk::ToggleButton::with_label("All");
        all.set_group(Some(&open));
        for toggle in [&open, &all] {
            toggle.add_css_class("flat");
            toggles.append(toggle);
        }
        match filter {
            OrdersFilter::Open => open.set_active(true),
            OrdersFilter::All => all.set_active(true),
        }
        let on_orders = |stack: &gtk::Stack| stack.visible_child_name().as_deref() == Some("orders");
        toggles.set_visible(on_orders(&stack));
        let shown = toggles.clone();
        stack.connect_visible_child_name_notify(move |stack| shown.set_visible(on_orders(stack)));

        let cancel_all = gtk::Button::with_label("Cancel all");
        cancel_all.add_css_class("flat");
        let trailing = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        trailing.set_halign(gtk::Align::End);
        trailing.set_hexpand(true);
        trailing.append(&toggles);
        trailing.append(&cancel_all);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        bar.add_css_class("trading-tabs");
        bar.append(&switcher);
        bar.append(&trailing);
        root.append(&bar);
        root.append(&stack);

        let hands = Rc::new(hands);
        let default: Rc<RefCell<Option<String>>> = Rc::default();
        let (pressed, on) = (hands.clone(), default.clone());
        cancel_all.connect_clicked(move |_| {
            if let Some(key) = on.borrow().clone() {
                (pressed.cancel_all)(&key);
            }
        });
        let panel = Rc::new(TradingPanel {
            root,
            stack,
            accounts,
            positions,
            orders,
            cancel_all,
            default,
            hands,
            store,
            filter: Cell::new(filter),
            filter_toggles: (toggles, open.clone(), all.clone()),
            order_rows: RefCell::default(),
            orders_drawn: RefCell::default(),
            drawn: RefCell::default(),
        });
        for (toggle, chosen) in [(open, OrdersFilter::Open), (all, OrdersFilter::All)] {
            let this = Rc::downgrade(&panel);
            toggle.connect_toggled(move |toggle| {
                if toggle.is_active()
                    && let Some(this) = this.upgrade()
                {
                    this.choose(chosen);
                }
            });
        }
        panel
    }

    /// The tab in front: "accounts", "positions" or "orders".
    pub fn tab(&self) -> Option<String> {
        self.stack.visible_child_name().map(|name| name.to_string())
    }

    /// Bring the tab called `name` to the front.
    pub fn show_tab(&self, name: &str) {
        self.stack.set_visible_child_name(name);
    }

    /// The filter the Orders tab shows by.
    pub fn filter(&self) -> OrdersFilter {
        self.filter.get()
    }

    /// Show the Orders tab by `filter`, from now on and the next time.
    pub fn choose(&self, filter: OrdersFilter) {
        let (_, open, all) = &self.filter_toggles;
        let toggle = match filter {
            OrdersFilter::Open => open,
            OrdersFilter::All => all,
        };
        if !toggle.is_active() {
            // Comes back here through the toggle, and goes on from there.
            toggle.set_active(true);
            return;
        }
        if self.filter.replace(filter) == filter {
            return;
        }
        self.store.set_setting(ORDERS_FILTER, filter.key());
        self.apply_filter();
    }

    /// Show the rows the filter keeps and hide the rest, and with none
    /// kept, say so where the table was.
    fn apply_filter(&self) {
        let filter = self.filter.get();
        let mut shown = 0;
        for (open, cells) in self.order_rows.borrow().iter() {
            let visible = filter == OrdersFilter::All || *open;
            shown += visible as usize;
            for cell in cells {
                cell.set_visible(visible);
            }
        }
        if let Some((table, empty)) = self.orders_drawn.borrow().as_ref() {
            table.set_visible(shown > 0);
            empty.set_label(filter.empty_words());
            empty.set_visible(shown == 0);
        }
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
        let label = match view.default_account() {
            Some(account) => format!("Cancel all · {}", view.account_name(account)),
            None => "Cancel all".to_string(),
        };
        self.cancel_all.set_label(&label);
        // Read when it is pressed, so it cancels on the default in force.
        *self.default.borrow_mut() = default;
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
                let balance = match money.and_then(|m| m.balance).or(account.balance) {
                    Some(value) => trading::money_text(value, currency),
                    None => "—".to_string(),
                };
                let limits = money.and_then(|m| m.limits.as_ref());
                let amount = |value: Option<f64>| match value {
                    Some(value) => trading::money_text(value, currency),
                    None => "—".to_string(),
                };
                vec![
                    cell(&view.account_name(account), false).upcast(),
                    cell(currency.unwrap_or("—"), false).upcast(),
                    cell(&balance, true).upcast(),
                    pnl_cell(money.and_then(|m| m.day_pnl), currency).upcast(),
                    pnl_cell(money.and_then(|m| m.unrealized), currency).upcast(),
                    cell(&amount(limits.and_then(|l| l.loss_limit)), true).upcast(),
                    cell(&amount(limits.and_then(|l| l.liquidate_at)), true).upcast(),
                ]
            })
            .collect();
        table(
            &self.accounts,
            &[
                ("Account", false),
                ("Currency", false),
                ("Balance", true),
                ("Day P&L", true),
                ("Open P&L", true),
                ("Loss limit", true),
                ("Liquidates at", true),
            ],
            rows,
            "No accounts",
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
                vec![
                    cell(&view.name_of(&held.account), false).upcast(),
                    cell(&held.contract, false).upcast(),
                    side_cell(&trading::net_text(held.net), if held.net > 0 { "trade-long" } else { "trade-short" }).upcast(),
                    cell(&price_text(held.avg, held.tick), true).upcast(),
                    pnl_cell(held.open_pnl, Self::currency_of(view, &held.account)).upcast(),
                    flatten.upcast(),
                ]
            })
            .collect();
        table(
            &self.positions,
            &[("Account", false), ("Contract", false), ("Qty", true), ("Average", true), ("Open P&L", true), ("", false)],
            rows,
            "No positions",
        );
    }

    /// Every order of the day as a row, drawn only when one of them
    /// moved: a push that changed money or a position leaves them as they
    /// are. The filter then shows the rows it keeps.
    fn redraw_orders(&self, view: &View) {
        let orders = &view.book.orders;
        let names: Vec<String> = orders.iter().map(|order| view.name_of(&order.account)).collect();
        if self.drawn.borrow().as_ref().is_some_and(|(drawn, named)| drawn == orders && *named == names) {
            return;
        }
        let rows: Vec<(bool, Vec<gtk::Widget>)> = orders
            .iter()
            .zip(&names)
            .map(|(order, name)| {
                let status = match (order.status.stage, &order.status.said) {
                    (Stage::Rejected | Stage::Cancelled, Some(said)) => format!("{} · {said}", order.status.word()),
                    _ => order.status.word().to_string(),
                };
                let price = match order.kind {
                    OrderKind::Market => "Market".to_string(),
                    _ => price_text(order.price, order.tick),
                };
                let status_cell = cell(&status, false);
                status_cell.set_ellipsize(gtk::pango::EllipsizeMode::End);
                status_cell.set_max_width_chars(40);
                status_cell.set_tooltip_text(Some(&status));
                let labels = [
                    cell(&time_text(order.at), true),
                    cell(name, false),
                    cell(&order.contract, false),
                    side_cell(&format!("{} {}", order.side.glyph(), order.side.word()), match order.side {
                        Side::Buy => "trade-buy",
                        Side::Sell => "trade-sell",
                    }),
                    cell(&order.qty.to_string(), true),
                    cell(order.kind.word(), false),
                    cell(&price, true),
                    status_cell,
                    cell(&price_text(order.average_fill(), order.tick), true),
                ];
                // Pending is faint here as it is on the chart: asked, and
                // not yet the broker's.
                if order.status == Stage::Pending {
                    for label in &labels {
                        label.add_css_class("dim-label");
                    }
                }
                let mut cells: Vec<gtk::Widget> = labels.into_iter().map(Cast::upcast).collect();
                if order.status.working() {
                    let cancel = gtk::Button::with_label("Cancel");
                    cancel.add_css_class("flat");
                    let hands = self.hands.clone();
                    let (account, basket) = (order.account.clone(), order.basket_id.clone());
                    cancel.connect_clicked(move |_| (hands.cancel)(&account, &basket));
                    cells.push(cancel.upcast());
                }
                (order.status.working(), cells)
            })
            .collect();
        clear(&self.orders);
        let drawn = match rows.is_empty() {
            true => {
                self.orders.append(&empty_label("No orders"));
                None
            }
            false => {
                let cells: Vec<Vec<gtk::Widget>> = rows.iter().map(|(_, cells)| cells.clone()).collect();
                let table = grid_of(ORDER_HEADINGS, &cells);
                let empty = empty_label("");
                self.orders.append(&table);
                self.orders.append(&empty);
                Some((table, empty))
            }
        };
        *self.order_rows.borrow_mut() = rows;
        *self.orders_drawn.borrow_mut() = drawn;
        *self.drawn.borrow_mut() = Some((orders.clone(), names));
        self.apply_filter();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::fake::day_of_orders;

    #[test]
    fn a_price_is_written_to_its_tick() {
        assert_eq!(decimals_of(Some(0.25)), 2);
        assert_eq!(decimals_of(Some(0.001)), 3);
        assert_eq!(decimals_of(None), 2);
        assert_eq!(price_text(Some(7_612.25), Some(0.25)), "7,612.25");
    }

    /// Open is what Cancel all would cancel: pending at the broker
    /// counts, and nothing that is over does.
    #[test]
    fn open_orders_are_the_ones_still_to_fill_or_be_cancelled() {
        let open = [Stage::Pending, Stage::Working, Stage::PartiallyFilled];
        let over = [Stage::Filled, Stage::Cancelled, Stage::Rejected];
        for stage in open {
            assert!(OrdersFilter::Open.shows(&stage.into()), "{stage:?}");
        }
        for stage in over {
            assert!(!OrdersFilter::Open.shows(&stage.into()), "{stage:?}");
        }
        for stage in open.into_iter().chain(over) {
            assert!(OrdersFilter::All.shows(&stage.into()), "{stage:?}");
        }
    }

    fn panel(store: &Rc<Store>) -> Rc<TradingPanel> {
        let hands = Hands {
            cancel: Box::new(|_, _| {}),
            flatten: Box::new(|_, _, _| {}),
            cancel_all: Box::new(|_| {}),
        };
        TradingPanel::new(hands, store.clone())
    }

    fn view(orders: Vec<Order>) -> View {
        View { book: trading::Book { orders, ..day_of_orders() }, ..View::default() }
    }

    /// The status column of each row on show, top to bottom.
    fn shown(panel: &TradingPanel) -> Vec<String> {
        panel
            .order_rows
            .borrow()
            .iter()
            .filter(|(_, cells)| cells[0].is_visible())
            .map(|(_, cells)| cells[7].downcast_ref::<gtk::Label>().expect("a label").text().to_string())
            .collect()
    }

    fn stack(panel: &TradingPanel) -> gtk::Stack {
        panel.stack.clone()
    }

    fn toggles(panel: &TradingPanel) -> (bool, bool, bool) {
        let (toggles, open, all) = &panel.filter_toggles;
        (toggles.is_visible(), open.is_active(), all.is_active())
    }

    /// The tab opens on the open orders, with a pending one faint among
    /// them; All shows the day, is remembered, and the next panel opens
    /// on it. The toggles are there only with the Orders tab in front.
    #[test]
    fn the_orders_tab_shows_the_open_orders_or_all_and_remembers_which() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let panel = panel(&store);
            assert_eq!(panel.filter(), OrdersFilter::Open);
            assert_eq!(toggles(&panel), (false, true, false), "on the Accounts tab");
            stack(&panel).set_visible_child_name("orders");
            assert_eq!(toggles(&panel), (true, true, false));

            panel.redraw(&view(day_of_orders().orders));
            assert_eq!(shown(&panel), ["Pending", "Working", "Part filled"]);
            let (table, empty) = panel.orders_drawn.borrow().clone().expect("a table");
            assert!(table.is_visible() && !empty.is_visible());
            let rows = panel.order_rows.borrow();
            assert!(rows[0].1.iter().take(9).all(|cell| cell.has_css_class("dim-label")), "pending is faint");
            assert!(rows[1].1.iter().all(|cell| !cell.has_css_class("dim-label")), "working is not");
            assert_eq!(rows[0].1.len(), 10, "an open order can be cancelled");
            assert_eq!(rows[5].1.len(), 9, "a filled one cannot");
            drop(rows);

            panel.filter_toggles.2.set_active(true);
            assert_eq!(panel.filter(), OrdersFilter::All);
            assert_eq!(
                shown(&panel),
                ["Pending", "Working", "Part filled", "Cancelled · by the trader", "Rejected · too far from the market", "Filled"]
            );
            assert_eq!(store.setting(ORDERS_FILTER).as_deref(), Some("all"));
            assert_eq!(toggles(&panel), (true, false, true));

            let again = self::panel(&store);
            assert_eq!(again.filter(), OrdersFilter::All, "as it was left");
            assert_eq!(toggles(&again), (false, false, true));
            again.choose(OrdersFilter::Open);
            assert_eq!(toggles(&again), (false, true, false), "chosen from outside, the toggle follows");
            assert_eq!(store.setting(ORDERS_FILTER).as_deref(), Some("open"));

            stack(&panel).set_visible_child_name("positions");
            assert!(!toggles(&panel).0, "the toggles are the Orders tab's");
        });
    }

    /// With nothing the filter keeps, the table goes and the tab says so
    /// in two words; with no orders at all, as it always did. And a push
    /// that moved no order leaves the rows as they are.
    #[test]
    fn a_filter_that_keeps_nothing_says_so_and_an_idle_push_draws_nothing() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let panel = panel(&store);
            let over: Vec<Order> = day_of_orders().orders.into_iter().filter(|order| !order.status.working()).collect();
            panel.redraw(&view(over.clone()));
            let (table, empty) = panel.orders_drawn.borrow().clone().expect("a table");
            assert!(!table.is_visible() && empty.is_visible());
            assert_eq!(empty.text(), "No open orders");
            assert!(shown(&panel).is_empty());

            panel.redraw(&view(over.clone()));
            let (same, _) = panel.orders_drawn.borrow().clone().expect("a table");
            assert_eq!(same, table, "drawn again for nothing");

            panel.choose(OrdersFilter::All);
            assert!(table.is_visible() && !empty.is_visible());
            assert_eq!(shown(&panel).len(), 3);
            let (same, _) = panel.orders_drawn.borrow().clone().expect("a table");
            assert_eq!(same, table, "a change of filter draws nothing");

            let mut moved = over.clone();
            moved[0].status = Stage::Working.into();
            panel.redraw(&view(moved));
            let (other, _) = panel.orders_drawn.borrow().clone().expect("a table");
            assert_ne!(other, table, "an order that moved is drawn");

            panel.redraw(&view(Vec::new()));
            assert!(panel.orders_drawn.borrow().is_none());
            let label: gtk::Label = panel.orders.first_child().and_downcast().expect("a label");
            assert_eq!(label.text(), "No orders");
            assert!(panel.orders.first_child().and_then(|first| first.next_sibling()).is_none());
        });
    }
}
