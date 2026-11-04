//! What trading puts in the window: the panel under the charts and its
//! toggle in the corner, the toasts orders report in, and later the
//! chart menu's orders and the marks on the charts.
//!
//! None of it exists without a broker login. The window of somebody who
//! only charts from Yahoo is the window it always was: no divider under the
//! charts, no toast overlay, no button and no action. The first login
//! added puts it all in, and removing the last takes it all out again,
//! both while the window is open.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::Window;
use crate::log_words;
use crate::trading::{self, Failure, OrderKind, Pending, Request, Side, Stage};
use crate::ui::pane::ChartPane;
use crate::ui::trading_panel::{Hands, TradingPanel};

/// Whether the panel is open, remembered for the next launch.
pub const SHOW_TRADING_PANEL: &str = "show_trading_panel";

/// The disabled action a row the menu cannot offer points at, so it is
/// drawn greyed.
const UNAVAILABLE: &str = "chart.order-unavailable";

/// The chart actions the trading keys reach, there only while trading is.
/// Without them the keys do nothing at all.
pub(super) const BUY_MARKET: &str = "buy-market";
pub(super) const SELL_MARKET: &str = "sell-market";
pub(super) const GO_FLAT: &str = "go-flat";
/// Entry N of the focused chart's size template, counting from 1.
pub(super) const TRADING_SIZE: &str = "trading-size";

/// The window's trading furniture, all of it built only once there is a
/// login.
#[derive(Default)]
pub(super) struct Trade {
    /// Trading is in the window now: there is a login.
    pub shown: Cell<bool>,
    pub toggle: RefCell<Option<gtk::ToggleButton>>,
    /// The divider between the charts and the panel, built the first time
    /// the panel is shown.
    pub paned: RefCell<Option<gtk::Paned>>,
    pub panel: RefCell<Option<Rc<TradingPanel>>>,
    pub toasts: RefCell<Option<adw::ToastOverlay>>,
    /// Where the window's parts sit, to put the panel and the toasts among
    /// them and take them out again.
    pub overlay: RefCell<Option<gtk::Overlay>>,
    pub root: RefCell<Option<gtk::Box>>,
    pub with_tools: RefCell<Option<gtk::Box>>,
    pub cluster: RefCell<Option<gtk::Box>>,
    /// Orders placed from this window, by basket, and where each stood when
    /// it was last reported, so each step is toasted once.
    pub placed: RefCell<HashMap<String, Stage>>,
    /// The desk's call to read Trading again, kept alive with the window.
    pub listener: RefCell<Option<Rc<dyn Fn()>>>,
    /// The chart's actions, which `chart.order` and `chart.cancel-order`
    /// join while there is a login.
    pub chart_actions: RefCell<Option<gio::SimpleActionGroup>>,
    /// The accounts with an order on its way: a place, a move or a cancel.
    pub placing: trading::Placing,
    /// Everything asked of a broker from here that its book has not
    /// answered yet: a new order, a move, a cancel, a flatten. Each is
    /// drawn as pending, faint, from the moment it is asked until the book
    /// says what became of it, or the broker refuses it. One that is never
    /// answered stays pending: nothing here assumes it was done.
    pub pending: RefCell<HashMap<Request, Pending>>,
    /// How many of each login's refreshes the pending requests have been
    /// reconciled against, so each refresh is once.
    pub reconciled: RefCell<HashMap<String, u32>>,
    /// The size templates, read when trading comes in and again whenever
    /// they are edited.
    pub sizes: RefCell<Vec<trading::sizes::SizeTemplate>>,
}

/// What a right click on a chart's Trading size offers.
pub(super) fn size_part_menu() -> gio::Menu {
    let model = gio::Menu::new();
    model.append(Some("Edit trading sizes…"), Some(&format!("win.{EDIT_SIZES}")));
    model
}

/// What cancelling `order` asks for: "Cancel Buy Limit 7,620.00".
fn cancel_words(order: &trading::Order) -> String {
    let decimals = crate::ui::trading_panel::decimals_of(order.tick);
    let price = order.price.map(|price| omacharts_engine::grouped_decimals(price, decimals)).unwrap_or_default();
    format!("Cancel {} {price}", trading::order_words(order.side, order.kind)).trim_end().to_string()
}

/// The window's action that opens the Trading sizes dialog.
const EDIT_SIZES: &str = "edit-trading-sizes";

/// The `chart.order` target: `side:kind:account:price`, the price empty at
/// the market.
pub(super) fn order_target_value(side: Side, kind: OrderKind, account: &str, price: Option<f64>) -> String {
    let price = price.map(|price| price.to_string()).unwrap_or_default();
    format!("{}:{}:{account}:{price}", side.key(), kind.key())
}

/// The other way: what a `chart.order` target asks for.
pub(super) fn parse_order_target(value: &str) -> Option<(Side, OrderKind, String, Option<f64>)> {
    let mut parts = value.splitn(4, ':');
    let side = Side::from_key(parts.next()?)?;
    let kind = OrderKind::from_key(parts.next()?)?;
    let account = parts.next()?.to_string();
    let price = match parts.next()? {
        "" => None,
        price => Some(price.parse::<f64>().ok()?),
    };
    match (kind, price) {
        (OrderKind::Market, _) | (_, Some(_)) => Some((side, kind, account, price)),
        _ => None,
    }
}

impl Window {
    /// Put trading into the window, or take it out, to match whether there
    /// is a login; then draw what is there from the view.
    pub(super) fn sync_trading(self: &Rc<Self>) {
        let trades = self.trading.borrow().trades();
        match (trades, self.trade.shown.get()) {
            (true, false) => self.install_trading(),
            (false, true) => self.remove_trading(),
            _ => {}
        }
        if !trades {
            return;
        }
        let panel = self.trade.panel.borrow().clone();
        if let Some(panel) = panel {
            panel.redraw(&self.trading.borrow());
        }
        self.say_session_trouble();
        self.report_orders();
        self.settle_pending();
        self.apply_marks();
    }

    /// Forget what was asked of a broker once its book has answered, and
    /// after a login's session was refreshed, what it never will.
    fn settle_pending(&self) {
        self.reconcile();
        let view = self.trading.borrow();
        self.trade.pending.borrow_mut().retain(|request, pending| !trading::answered(request, pending, &view.book));
    }

    /// Once a login's session has been refreshed — signed in, or back after
    /// a drop — its book is everything the broker has. An order asked for
    /// on one of its accounts that the book has never heard of did not get
    /// there, and is no longer drawn as on its way; a move, a cancel or a
    /// flatten is answered by the book as ever. Nothing is on its way to
    /// those accounts any more.
    fn reconcile(&self) {
        let mut never = Vec::new();
        {
            let view = self.trading.borrow();
            let mut reconciled = self.trade.reconciled.borrow_mut();
            let mut pending = self.trade.pending.borrow_mut();
            for login in &view.logins {
                let refreshes = self.desk.refreshes(&login.id);
                if reconciled.get(&login.id).copied().unwrap_or(0) == refreshes {
                    continue;
                }
                reconciled.insert(login.id.clone(), refreshes);
                let ours = |key: &str| key.split_once('/').is_some_and(|(of, _)| of == login.id);
                pending.retain(|request, asked| match (request, asked) {
                    (Request::Place(tag), Pending::Place { key, order }) if ours(key) => {
                        let heard = view.book.orders.iter().any(|known| known.tag.as_deref() == Some(tag.as_str()));
                        if !heard {
                            never.push((key.clone(), trading::never_placed_text(order)));
                        }
                        heard
                    }
                    _ => true,
                });
                for account in view.accounts.iter().filter(|account| account.login == login.id) {
                    self.trade.placing.done(&account.key());
                }
            }
        }
        for (key, text) in never {
            self.refuse_as(&self.account_label(&key), &text);
        }
    }

    /// Draw `request` as pending from now.
    fn ask(self: &Rc<Self>, request: Request, pending: Pending) {
        self.trade.pending.borrow_mut().insert(request, pending);
        self.apply_marks();
    }

    /// What the broker said to `request` when it was sent: a refusal takes
    /// what was drawn for it away and says why; no answer leaves it
    /// pending, since it may well have arrived, and says so.
    fn heard<T>(
        self: &Rc<Self>,
        request: &Request,
        outcome: Result<T, Failure>,
        not: &str,
        account: &str,
    ) -> Option<T> {
        match outcome {
            Ok(value) => {
                // The book may already say what became of it.
                self.desk.tell();
                Some(value)
            }
            Err(Failure::Refused(why)) => {
                self.trade.pending.borrow_mut().remove(request);
                self.apply_marks();
                self.refuse_as(account, &format!("{not}: {why}"));
                None
            }
            Err(Failure::Unanswered(why)) => {
                self.refuse_as(account, &format!("No answer yet: {why}"));
                None
            }
        }
    }

    fn install_trading(self: &Rc<Self>) {
        self.trade.shown.set(true);

        let toggle = gtk::ToggleButton::new();
        toggle.set_icon_name("view-list-symbolic");
        toggle.set_tooltip_text(Some("Trading panel"));
        toggle.add_css_class("flat");
        let this = self.clone();
        toggle.connect_toggled(move |toggle| this.show_trading_panel(toggle.is_active()));
        if let Some(cluster) = self.trade.cluster.borrow().as_ref() {
            cluster.prepend(&toggle);
        }
        *self.trade.toggle.borrow_mut() = Some(toggle.clone());
        // The corner is a button wider; the rail's + steps over for it.
        let (_, wide, _, _) = toggle.measure(gtk::Orientation::Horizontal, -1);
        if let Some(rail) = self.watchlist.borrow().as_ref() {
            rail.clear_corner(wide);
        }

        let action = gio::SimpleAction::new("trading-panel", None);
        let this = self.clone();
        action.connect_activate(move |_, _| {
            let toggle = this.trade.toggle.borrow().clone();
            if let Some(toggle) = toggle {
                toggle.set_active(!toggle.is_active());
            }
        });
        self.window.add_action(&action);

        let edit = gio::SimpleAction::new(EDIT_SIZES, None);
        let this = self.clone();
        edit.connect_activate(move |_, _| this.edit_trading_sizes(&this.window));
        self.window.add_action(&edit);

        *self.trade.sizes.borrow_mut() = trading::sizes::load(&self.store);

        // The chart menu's orders, and what the trading keys reach: buy and
        // sell the chart's size at the market, go flat, and pick the size.
        if let Some(actions) = self.trade.chart_actions.borrow().as_ref() {
            for (name, side) in [(BUY_MARKET, Side::Buy), (SELL_MARKET, Side::Sell)] {
                let market = gio::SimpleAction::new(name, None);
                let this = self.clone();
                market.connect_activate(move |_, _| this.market_from_keys(side));
                actions.add_action(&market);
            }
            let flat = gio::SimpleAction::new(GO_FLAT, None);
            let this = self.clone();
            flat.connect_activate(move |_, _| this.flat_from_keys());
            actions.add_action(&flat);
            let size = gio::SimpleAction::new(TRADING_SIZE, Some(glib::VariantTy::INT32));
            let this = self.clone();
            size.connect_activate(move |_, value| {
                if let Some(n) = value.and_then(|value| value.get::<i32>()).and_then(|n| usize::try_from(n).ok()) {
                    this.pick_size(this.focused.get(), n);
                }
            });
            actions.add_action(&size);
            let order = gio::SimpleAction::new("order", Some(glib::VariantTy::STRING));
            let this = self.clone();
            order.connect_activate(move |_, value| {
                if let Some(asked) = value.and_then(|value| value.str().and_then(parse_order_target)) {
                    this.order_from_chart(asked);
                }
            });
            actions.add_action(&order);
            let cancel = gio::SimpleAction::new("cancel-order", Some(glib::VariantTy::STRING));
            let this = self.clone();
            cancel.connect_activate(move |_, value| {
                if let Some(basket) = value.and_then(|value| value.str().map(str::to_string)) {
                    this.cancel_from_chart(&basket);
                }
            });
            actions.add_action(&cancel);
            // What a greyed row points at: GTK greys a row by its action.
            let unavailable = gio::SimpleAction::new("order-unavailable", None);
            unavailable.set_enabled(false);
            actions.add_action(&unavailable);
        }

        // Toasts lie over the whole window, rail included.
        let overlay = self.trade.overlay.borrow().clone();
        let root = self.trade.root.borrow().clone();
        if let (Some(overlay), Some(root)) = (overlay, root) {
            let toasts = adw::ToastOverlay::new();
            overlay.set_child(None::<&gtk::Widget>);
            toasts.set_child(Some(&root));
            overlay.set_child(Some(&toasts));
            *self.trade.toasts.borrow_mut() = Some(toasts);
        }

        if self.store.setting_bool(SHOW_TRADING_PANEL, false) {
            toggle.set_active(true);
        }
        self.sync_corner_clearance();
    }

    fn remove_trading(self: &Rc<Self>) {
        self.trade.shown.set(false);
        self.unmount_panel();
        if let Some(toggle) = self.trade.toggle.borrow_mut().take()
            && let Some(cluster) = self.trade.cluster.borrow().as_ref()
        {
            cluster.remove(&toggle);
        }
        if let Some(rail) = self.watchlist.borrow().as_ref() {
            rail.clear_corner(0);
        }
        self.window.remove_action("trading-panel");
        self.window.remove_action(EDIT_SIZES);
        if let Some(actions) = self.trade.chart_actions.borrow().as_ref() {
            actions.remove_action("order");
            actions.remove_action("cancel-order");
            actions.remove_action("order-unavailable");
            for name in [BUY_MARKET, SELL_MARKET, GO_FLAT, TRADING_SIZE] {
                actions.remove_action(name);
            }
        }
        let overlay = self.trade.overlay.borrow().clone();
        let root = self.trade.root.borrow().clone();
        if let (Some(overlay), Some(root), Some(toasts)) = (overlay, root, self.trade.toasts.borrow_mut().take()) {
            toasts.set_child(None::<&gtk::Widget>);
            overlay.set_child(Some(&root));
        }
        self.trade.placed.borrow_mut().clear();
        self.trade.pending.borrow_mut().clear();
        for pane in self.panes.borrow().iter() {
            pane.view.set_marks(None, Vec::new());
            self.write_size_of(pane);
        }
        self.sync_corner_clearance();
    }

    /// Open or close the panel, building it the first time.
    pub(super) fn show_trading_panel(self: &Rc<Self>, shown: bool) {
        if !self.trade.shown.get() {
            return;
        }
        self.store.set_setting_bool(SHOW_TRADING_PANEL, shown);
        match shown {
            true => {
                let panel = self.mount_panel();
                panel.redraw(&self.trading.borrow());
                panel.root.set_visible(true);
            }
            false => {
                if let Some(panel) = self.trade.panel.borrow().as_ref() {
                    panel.root.set_visible(false);
                }
            }
        }
        if let Some(toggle) = self.trade.toggle.borrow().as_ref()
            && toggle.is_active() != shown
        {
            toggle.set_active(shown);
        }
    }

    /// Is the panel on screen?
    pub fn trading_panel_shown(&self) -> bool {
        self.trade.panel.borrow().as_ref().is_some_and(|panel| panel.root.property::<bool>("visible"))
    }

    /// The panel, put under the charts behind a divider of its own.
    fn mount_panel(self: &Rc<Self>) -> Rc<TradingPanel> {
        if let Some(panel) = self.trade.panel.borrow().as_ref() {
            return panel.clone();
        }
        let hands = Hands {
            cancel: Box::new({
                let this = Rc::downgrade(self);
                move |account: &str, basket: &str| {
                    let Some(this) = this.upgrade() else { return };
                    let basket = basket.to_string();
                    this.on_session(account, move |session, number| session.cancel(number, &basket));
                }
            }),
            flatten: Box::new({
                let this = Rc::downgrade(self);
                move |account: &str, contract: &str, exchange: &str| {
                    let Some(this) = this.upgrade() else { return };
                    let (contract, exchange) = (contract.to_string(), exchange.to_string());
                    this.on_session(account, move |session, number| session.flatten(number, &contract, &exchange));
                }
            }),
            cancel_all: Box::new({
                let this = Rc::downgrade(self);
                move |account: &str| {
                    let Some(this) = this.upgrade() else { return };
                    this.on_session(account, |session, number| session.cancel_all(number));
                }
            }),
        };
        let panel = TradingPanel::new(hands);
        panel.root.set_size_request(-1, 150);
        let paned = gtk::Paned::new(gtk::Orientation::Vertical);
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(false);
        paned.set_shrink_end_child(false);
        paned.set_vexpand(true);
        paned.set_hexpand(true);
        if let Some(with_tools) = self.trade.with_tools.borrow().as_ref() {
            with_tools.remove(&self.chart_host);
            paned.set_start_child(Some(&self.chart_host));
            paned.set_end_child(Some(&panel.root));
            with_tools.append(&paned);
        }
        *self.trade.paned.borrow_mut() = Some(paned);
        *self.trade.panel.borrow_mut() = Some(panel.clone());
        panel
    }

    /// Take the panel and its divider out, the charts back where they were.
    fn unmount_panel(&self) {
        self.trade.panel.borrow_mut().take();
        let Some(paned) = self.trade.paned.borrow_mut().take() else { return };
        if let Some(with_tools) = self.trade.with_tools.borrow().as_ref() {
            paned.set_start_child(None::<&gtk::Widget>);
            paned.set_end_child(None::<&gtk::Widget>);
            with_tools.remove(&paned);
            with_tools.append(&self.chart_host);
        }
    }

    /// Say that what was asked was not done, before anything reached an
    /// account: in the Log, and so in the corner notice.
    pub(super) fn refuse(&self, text: &str) {
        self.say(log_words::refused("Trading", text));
    }

    /// The same, for what was asked of `target`'s account.
    pub(super) fn refuse_on(&self, target: &trading::Target, text: &str) {
        self.refuse_as(&self.account_label(&target.account.key()), text);
    }

    /// The same, of the account called `account`.
    pub(super) fn refuse_as(&self, account: &str, text: &str) {
        self.say(log_words::refused(account, text));
    }

    /// Say that what was asked of the account called `account` was done: in
    /// the Log, and in a toast.
    pub(super) fn confirm(&self, account: &str, text: &str) {
        self.say(log_words::confirmed(account, text));
    }

    /// Say `text` in a toast over the window. Only for something asked for
    /// and done, which [`crate::ui::notice::surface`] decides.
    pub(super) fn toast(&self, text: &str) {
        if let Some(toasts) = self.trade.toasts.borrow().as_ref() {
            let toast = adw::Toast::new(text);
            toast.set_timeout(4);
            toasts.add_toast(toast);
        }
    }

    /// The name an account goes by here: "Lucid", or "Lucid 1234" when
    /// the login holds several.
    pub(super) fn account_label(&self, key: &str) -> String {
        let view = self.trading.borrow();
        view.accounts
            .iter()
            .find(|account| account.key() == key)
            .map(|account| view.account_name(account))
            .unwrap_or_else(|| key.rsplit('/').next().unwrap_or(key).to_string())
    }

    /// Do `job` on the session of the account with the key `account`, off
    /// the main loop, and toast what went wrong. What it changes arrives
    /// on the book.
    pub(super) fn on_session(
        self: &Rc<Self>,
        account: &str,
        job: impl FnOnce(&dyn trading::Session, &str) -> Result<(), Failure> + Send + 'static,
    ) {
        let Some((login, number)) = account.split_once('/') else { return };
        let source = self.account_label(account);
        let Some(session) = self.desk.session(login).filter(|_| self.desk.live(login)) else {
            self.refuse_as(&source, &format!("{source} not signed in"));
            return;
        };
        let number = number.to_string();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(job(session.as_ref(), &number));
        });
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let Ok(outcome) = receiver.recv().await else { return };
            if let (Some(this), Err(why)) = (this.upgrade(), outcome) {
                this.refuse_as(&source, why.words());
            }
        });
    }

    /// Where an order from `pane` would go, or why it cannot: the one
    /// resolver the menu and the click both ask.
    pub(super) fn order_target(&self, pane: &ChartPane) -> Result<trading::Target, String> {
        let view = self.trading.borrow();
        let chosen = pane.account.borrow().clone();
        let instrument = pane.instrument.borrow().clone();
        trading::order_target(
            &view,
            |login| self.desk.session(login).filter(|_| self.desk.live(login)),
            chosen.as_deref(),
            instrument.as_ref(),
        )
    }

    /// The step `pane`'s prices move in: what its bars show, else what the
    /// broker says of the contract.
    fn tick_of(pane: &ChartPane, target: &trading::Target) -> Option<f64> {
        pane.view.tick().or(target.tick)
    }

    /// The chart menu's orders, at height `y` of the focused chart: buy and
    /// sell at the market, then a limit and a stop each way at the price
    /// there. One that would be on the wrong side of the market is greyed;
    /// a chart that cannot trade says why in one greyed row. With no login,
    /// nothing at all.
    pub(super) fn order_section(self: &Rc<Self>, y: f64) -> Option<gio::Menu> {
        if !self.trading.borrow().trades() {
            return None;
        }
        let section = gio::Menu::new();
        let pane = self.focused_pane();
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => {
                section.append(Some(&why), Some(UNAVAILABLE));
                return Some(section);
            }
        };
        let key = target.account.key();
        let name = self.account_label(&key);
        // The contract may have become known since the book last moved.
        self.apply_marks_to(&pane);
        let tick = Self::tick_of(&pane, &target);
        let at = pane.view.price_under(y).map(|price| match tick {
            Some(tick) => omacharts_engine::on_tick(price, tick),
            None => (price, 2),
        });
        let last = pane.view.last_price();
        let size = self.size_of(&pane);
        for (side, kind) in trading::menu_orders(&target.capabilities) {
            let price = (kind != OrderKind::Market).then_some(at).flatten();
            let text = match (kind, price) {
                (OrderKind::Market, _) => trading::menu_text(side, kind, size, "", &target.contract, &name),
                (_, Some((price, places))) => trading::menu_text(
                    side,
                    kind,
                    size,
                    &omacharts_engine::grouped_decimals(price, places),
                    &target.contract,
                    &name,
                ),
                (_, None) => trading::order_words(side, kind),
            };
            let item = gio::MenuItem::new(Some(&text), None);
            let allowed = match (kind, price, last) {
                (OrderKind::Market, _, _) => true,
                (_, Some((price, _)), Some(last)) => trading::side_allowed(kind, side, price, last),
                _ => false,
            };
            if allowed {
                let value = order_target_value(side, kind, &key, price.map(|(price, _)| price));
                item.set_action_and_target_value(Some("chart.order"), Some(&value.to_variant()));
            } else {
                item.set_action_and_target_value(Some(UNAVAILABLE), None);
            }
            section.append_item(&item);
        }

        // Cancelling the order whose line the pointer is on.
        let view = self.trading.borrow();
        let near = trading::order_near(view.book.working(&key, &target.contract), y, |price| {
            pane.view.y_of(price).unwrap_or(f64::INFINITY)
        });
        if let Some(order) = near {
            let places = tick.map_or(2, |tick| omacharts_engine::on_tick(0.0, tick).1);
            let price = order.price.map(|price| omacharts_engine::grouped_decimals(price, places)).unwrap_or_default();
            let text = format!("Cancel {} {price}", trading::order_words(order.side, order.kind));
            let item = gio::MenuItem::new(Some(&text), None);
            item.set_action_and_target_value(Some("chart.cancel-order"), Some(&order.basket_id.to_variant()));
            section.append_item(&item);
        }
        Some(section)
    }

    /// `chart.order`: check again what the menu offered, and place it.
    /// Nothing asks first, on any account: confirmations are off for now.
    fn order_from_chart(self: &Rc<Self>, (side, kind, account, price): (Side, OrderKind, String, Option<f64>)) {
        let pane = self.focused_pane();
        // Resolved again: the chart, its account or the login may have
        // changed since the menu opened.
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not placed")),
        };
        if target.account.key() != account {
            return self.refuse_on(&target, "The chart's account changed · not placed");
        }
        let places = Self::tick_of(&pane, &target).map_or(2, |tick| omacharts_engine::on_tick(0.0, tick).1);
        let price_text = price.map(|price| omacharts_engine::grouped_decimals(price, places)).unwrap_or_default();
        if let (Some(at), Some(last)) = (price, pane.view.last_price())
            && !trading::side_allowed(kind, side, at, last)
        {
            return self.refuse_on(&target, &trading::crossed_text(&price_text, kind, side));
        }
        let size = self.size_of(&pane);
        match trading::OrderRequest::request(&target.account.id, &target.contract(), side, kind, size, price) {
            Ok(order) => self.place(target, order),
            Err(why) => self.refuse_on(&target, &format!("{why} · not placed")),
        }
    }

    /// Send `order` off the main loop, one at a time per account. A failure
    /// is said and never retried.
    fn place(self: &Rc<Self>, target: trading::Target, order: trading::OrderRequest) {
        self.place_then(target, order, |_, _| {});
    }

    /// [`Self::place`], and `then` with its basket once the broker has
    /// taken it.
    fn place_then(
        self: &Rc<Self>,
        target: trading::Target,
        order: trading::OrderRequest,
        then: impl FnOnce(&Rc<Self>, String) + 'static,
    ) {
        let key = target.account.key();
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let source = self.account_label(&key);
        let decimals = crate::ui::trading_panel::decimals_of(target.tick);
        self.say(log_words::order_sent(&source, &order, decimals));
        // Drawn at once, faint, until the exchange has it working.
        let request = Request::Place(order.tag.clone());
        self.ask(request.clone(), Pending::Place { key: key.clone(), order: order.clone() });
        self.send(&target, move |session, _| session.place(&order), move |this, outcome| {
            this.trade.placing.done(&key);
            if let Some(basket) = this.heard(&request, outcome, "Not placed", &source) {
                this.trade.placed.borrow_mut().insert(basket.clone(), Stage::Pending);
                this.desk.tell();
                then(this, basket);
            }
        });
    }

    /// `chart.cancel-order`: cancel the working order `basket`, no
    /// question asked.
    fn cancel_from_chart(self: &Rc<Self>, basket: &str) {
        let order = self.trading.borrow().book.order(basket).cloned();
        let Some(order) = order else { return };
        let account = order.account.clone();
        self.say(log_words::note(&self.account_label(&account), cancel_words(&order)));
        let basket = basket.to_string();
        self.on_session(&account, move |session, number| session.cancel(number, &basket));
    }

    /// What the hand did to the marks on the chart `id`: the chart only
    /// says, and this is what sends.
    pub(super) fn trade_from_chart(self: &Rc<Self>, id: u32, event: crate::ui::chart::TradeEvent) {
        use crate::ui::chart::TradeEvent;
        let Some(pane) = self.pane(id) else { return };
        self.focus(id);
        match event {
            TradeEvent::Cancel(basket) => self.cancel_mark(&pane, &basket),
            TradeEvent::Flatten => self.flatten_mark(&pane),
            TradeEvent::Move { basket, price } => self.move_mark(&pane, &basket, price),
            TradeEvent::ExitAt { kind, price } => self.exit_mark(&pane, kind, price),
        }
    }

    /// Run `job` on the session of `target`, off the main loop, and hand
    /// what it says to `then` back on it.
    fn send<T: Send + 'static>(
        self: &Rc<Self>,
        target: &trading::Target,
        job: impl FnOnce(&dyn trading::Session, &str) -> Result<T, Failure> + Send + 'static,
        then: impl FnOnce(&Rc<Self>, Result<T, Failure>) + 'static,
    ) {
        let login = target.login.id.clone();
        let Some(session) = self.desk.session(&login).filter(|_| self.desk.live(&login)) else {
            let why = format!("{} not signed in", self.account_label(&target.account.key()));
            return then(self, Err(Failure::Refused(why)));
        };
        let number = target.account.id.clone();
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(job(session.as_ref(), &number));
        });
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let outcome =
                receiver.recv().await.unwrap_or_else(|_| Err(Failure::Unanswered("the session went away".into())));
            if let Some(this) = this.upgrade() {
                then(&this, outcome);
            }
        });
    }

    /// An order's ×: cancel it. Faint at once, gone when the book says so,
    /// back if the broker refuses.
    fn cancel_mark(self: &Rc<Self>, pane: &ChartPane, basket: &str) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not cancelled")),
        };
        let key = target.account.key();
        let order = self.trading.borrow().book.order(basket).filter(|order| order.account == key).cloned();
        let Some(order) = order.filter(|order| order.status.working()) else {
            return self.refuse_on(&target, "Already done · not cancelled");
        };
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let source = self.account_label(&key);
        self.say(log_words::note(&source, cancel_words(&order)));
        let basket = basket.to_string();
        let request = Request::Cancel(basket.clone());
        self.ask(request.clone(), Pending::Cancel);
        // Said once the book has it cancelled.
        self.trade.placed.borrow_mut().insert(basket.clone(), order.status.stage);
        self.send(&target, move |session, number| session.cancel(number, &basket), move |this, outcome| {
            this.trade.placing.done(&key);
            this.heard(&request, outcome, "Not cancelled", &source);
        });
    }

    /// A tab pulled out to `price`: an exit of `kind` for the whole of the
    /// position, linked to the other exit when there is one and the broker
    /// links orders.
    fn exit_mark(self: &Rc<Self>, pane: &ChartPane, kind: OrderKind, price: f64) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not placed")),
        };
        let key = target.account.key();
        let held = self.trading.borrow().book.position(&key, &target.contract).cloned();
        let Some(held) = held else { return self.refuse_on(&target, "Nothing held · not placed") };
        if !target.capabilities.kinds.contains(&kind) {
            return self.refuse_on(&target, &format!("{} takes no {} orders · not placed", target.login.name, kind.word()));
        }
        if let Some(last) = pane.view.last_price()
            && !trading::exit_allowed(kind, held.net, price, last)
        {
            return self.refuse_on(&target, &format!("{} · not placed", trading::refused_text(kind, price, last)));
        }
        let side = trading::exit_side(held.net);
        // The whole of what is held, whatever the chart's size: an exit
        // closes the position.
        let Some(size) = trading::Quantity::new(held.net.unsigned_abs()) else { return };
        let order = match trading::OrderRequest::request(&target.account.id, &target.contract(), side, kind, size, Some(price)) {
            Ok(order) => order,
            Err(why) => return self.refuse_on(&target, &format!("{why} · not placed")),
        };
        let linking = target.clone();
        self.place_then(target, order, move |this, basket| this.link_exit(&linking, side, kind, basket));
    }

    /// The exit just placed as `basket`, linked to the other one working
    /// on the position, so that whichever goes first takes the other with
    /// it; said when the broker cannot.
    fn link_exit(self: &Rc<Self>, target: &trading::Target, side: Side, kind: OrderKind, basket: String) {
        let key = target.account.key();
        let sibling = {
            let view = self.trading.borrow();
            let working = view.book.working(&key, &target.contract).filter(|order| order.basket_id != basket);
            trading::sibling_exit(working, &target.contract, side, kind).map(|order| order.basket_id.clone())
        };
        let Some(sibling) = sibling else { return };
        if !target.capabilities.link {
            return self.refuse_on(target, "Stop and target not linked on this broker");
        }
        let baskets = vec![basket, sibling];
        let source = self.account_label(&key);
        self.send(target, move |session, number| session.link(number, &baskets), move |this, outcome| match outcome {
            Ok(()) => this.confirm(&source, "Linked: stop cancels target"),
            Err(why) => this.refuse_as(&source, &format!("Not linked: {why}")),
        });
    }

    /// An order's line let go at `price`: move it there. Drawn faint at its
    /// new price until the book has it there; back where it was, and said,
    /// if the broker refuses.
    fn move_mark(self: &Rc<Self>, pane: &ChartPane, basket: &str, price: f64) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not moved")),
        };
        if !target.capabilities.modify {
            return self.refuse_on(&target, &format!("{} cannot move an order · not moved", target.login.name));
        }
        let key = target.account.key();
        let order = self.trading.borrow().book.order(basket).filter(|order| order.account == key).cloned();
        let Some(order) = order.filter(|order| order.status.working()) else {
            return self.refuse_on(&target, "Filled while moving");
        };
        // The market may have crossed it while the hand was letting go.
        if let Some(last) = pane.view.last_price()
            && !trading::side_allowed(order.kind, order.side, price, last)
        {
            return self.refuse_on(&target, &format!("{} · not moved", trading::refused_text(order.kind, price, last)));
        }
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let source = self.account_label(&key);
        let decimals = crate::ui::trading_panel::decimals_of(order.tick);
        self.say(log_words::note(
            &source,
            format!(
                "Move {} to {}",
                trading::order_words(order.side, order.kind),
                omacharts_engine::grouped_decimals(price, decimals)
            ),
        ));
        let basket = basket.to_string();
        let request = Request::Move(basket.clone());
        self.ask(request.clone(), Pending::Move(price));
        self.send(&target, move |session, number| session.modify(number, &basket, price), move |this, outcome| {
            this.trade.placing.done(&key);
            this.heard(&request, outcome, "Not moved", &source);
        });
    }

    /// The position's ×: flatten it, and cancel the exits working on it
    /// once it is flat. Faint at once, gone when the book says it is flat.
    fn flatten_mark(self: &Rc<Self>, pane: &ChartPane) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not flattened")),
        };
        let key = target.account.key();
        let held = self.trading.borrow().book.position(&key, &target.contract).cloned();
        let Some(held) = held else { return self.refuse_on(&target, "Nothing held · not flattened") };
        // The exits working on it, to cancel once it is flat: a stop left
        // behind would open a position of its own.
        let exits: Vec<String> = {
            let view = self.trading.borrow();
            let side = trading::exit_side(held.net);
            view.book
                .working(&key, &target.contract)
                .filter(|order| order.side == side && order.kind != OrderKind::Market)
                .map(|order| order.basket_id.clone())
                .collect()
        };
        let source = self.account_label(&key);
        self.say(log_words::note(&source, format!("Flatten {}", target.contract)));
        let request = Request::Flatten(key.clone(), target.contract.clone());
        self.ask(request.clone(), Pending::Flatten);
        let (contract, exchange) = (target.contract.clone(), target.exchange.clone());
        let again = target.clone();
        self.send(
            &target,
            move |session, number| session.flatten(number, &contract, &exchange),
            move |this, outcome| {
                if this.heard(&request, outcome, "Not flattened", &source).is_none() {
                    return;
                }
                // One the broker cancelled itself, or a link took with
                // its sibling, is not working any more: nothing to say.
                if exits.is_empty() {
                    return;
                }
                this.send(
                    &again,
                    move |session, number| {
                        for basket in &exits {
                            let _ = session.cancel(number, basket);
                        }
                        Ok(())
                    },
                    |_, _: Result<(), Failure>| {},
                );
            },
        );
    }

    /// Ctrl+B and Ctrl+S: buy or sell the focused chart's size at the
    /// market, at once. "Buying 2…" beside the last price until it fills.
    pub(super) fn market_from_keys(self: &Rc<Self>, side: Side) {
        let pane = self.focused_pane();
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not placed")),
        };
        if !target.capabilities.kinds.contains(&OrderKind::Market) {
            return self.refuse_on(&target, &format!("{} takes no market orders · not placed", target.login.name));
        }
        let size = self.size_of(&pane);
        match trading::OrderRequest::request(&target.account.id, &target.contract(), side, OrderKind::Market, size, None) {
            Ok(order) => self.place(target, order),
            Err(why) => self.refuse_on(&target, &format!("{why} · not placed")),
        }
    }

    /// Ctrl+F: nothing left in the focused chart's contract. Every order
    /// working in it is cancelled and the position flattened, each faint
    /// until the book says so.
    pub(super) fn flat_from_keys(self: &Rc<Self>) {
        let pane = self.focused_pane();
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("{why} · not flattened")),
        };
        let key = target.account.key();
        let flat = trading::flat_plan(&self.trading.borrow().book, &key, &target.contract);
        if flat.is_empty() {
            return self.refuse_on(&target, &format!("Already flat in {}", target.contract));
        }
        let source = self.account_label(&key);
        self.say(log_words::note(&source, format!("Flatten {} and cancel its orders", target.contract)));
        let flatten = Request::Flatten(key.clone(), target.contract.clone());
        {
            let mut pending = self.trade.pending.borrow_mut();
            for basket in &flat.cancel {
                pending.insert(Request::Cancel(basket.clone()), Pending::Cancel);
            }
            if flat.flatten {
                pending.insert(flatten.clone(), Pending::Flatten);
            }
        }
        self.apply_marks();
        let (contract, exchange) = (target.contract.clone(), target.exchange.clone());
        let cancels = flat.cancel.clone();
        // The orders first, so that none of them fills into the position
        // being closed; then the position.
        self.send(
            &target,
            move |session, number| {
                let mut failed = None;
                for basket in &flat.cancel {
                    if let Err(why) = session.cancel(number, basket) {
                        failed.get_or_insert(why);
                    }
                }
                if flat.flatten
                    && let Err(why) = session.flatten(number, &contract, &exchange)
                {
                    failed.get_or_insert(why);
                }
                failed.map_or(Ok(()), Err)
            },
            move |this, outcome| {
                if let Err(Failure::Refused(_)) = &outcome {
                    // Refused: drawn as it stands again. What the book has
                    // already answered is settled either way.
                    let mut pending = this.trade.pending.borrow_mut();
                    for basket in &cancels {
                        pending.remove(&Request::Cancel(basket.clone()));
                    }
                }
                this.heard(&flatten, outcome, "Not flattened", &source);
            },
        );
    }

    /// A trading key, Ctrl held and nothing being typed: true when it
    /// reached one of trading's actions, which are there only with a
    /// trading account. See `trading_key` for which key is which.
    pub(super) fn on_trading_key(self: &Rc<Self>, key: gtk::gdk::Key, keycode: u32) -> bool {
        let Some((name, target)) = super::trading_key(key, keycode) else { return false };
        let actions = self.trade.chart_actions.borrow().clone();
        match actions.filter(|actions| actions.has_action(name)) {
            Some(actions) => {
                actions.activate_action(name, target.as_ref());
                true
            }
            None => false,
        }
    }

    /// The size an order from `pane` is for: the one picked for it, else
    /// the first of its template; one when no template applies.
    pub(super) fn size_of(&self, pane: &ChartPane) -> trading::Quantity {
        let instrument = pane.instrument.borrow();
        let templates = self.trade.sizes.borrow();
        instrument
            .as_ref()
            .and_then(|instrument| trading::sizes::size_for(&templates, instrument, pane.trading_size.borrow().as_ref()))
            .unwrap_or(trading::Quantity::ONE)
    }

    /// Ctrl and a digit, or a row of the size part: the chart's size to
    /// entry `n` of its template. Sticks with the chart.
    pub(super) fn pick_size(self: &Rc<Self>, id: u32, n: usize) {
        let Some(pane) = self.pane(id) else { return };
        let picked = {
            let instrument = pane.instrument.borrow();
            let templates = self.trade.sizes.borrow();
            instrument.as_ref().and_then(|instrument| trading::sizes::pick(&templates, instrument, n))
        };
        let Some(picked) = picked else { return };
        if pane.trading_size.borrow().as_ref() == Some(&picked) {
            return;
        }
        pane.trading_size.replace(Some(picked));
        self.write_size_of(&pane);
        self.save_workspace();
    }

    /// The Trading size part: the chart's size in words while it has an
    /// account and a template applies; nothing at all, and no room taken,
    /// otherwise.
    pub(super) fn write_size_of(&self, pane: &ChartPane) {
        let text = match self.trade.shown.get() {
            false => None,
            true => {
                let view = self.trading.borrow();
                let account = view.account_of(pane.account.borrow().as_deref());
                let instrument = pane.instrument.borrow();
                let templates = self.trade.sizes.borrow();
                account.and(instrument.as_ref()).and_then(|instrument| {
                    let size = trading::sizes::size_for(&templates, instrument, pane.trading_size.borrow().as_ref())?;
                    Some(trading::sizes::size_text(size, instrument.kind))
                })
            }
        };
        pane.write_size(text.as_deref());
    }

    /// Read the size templates again, after the dialog changed them, and
    /// say every chart's size from them.
    pub fn adopt_sizes(self: &Rc<Self>) {
        *self.trade.sizes.borrow_mut() = trading::sizes::load(&self.store);
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            self.write_size_of(pane);
        }
    }

    /// The Trading sizes dialog over `parent`, every chart saying its size
    /// from what it saves as it is changed.
    pub fn edit_trading_sizes(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>) {
        let this = Rc::downgrade(self);
        crate::ui::size_templates::present(
            parent,
            self.store.clone(),
            Rc::new(move || {
                if let Some(this) = this.upgrade() {
                    this.adopt_sizes();
                }
            }),
        );
    }

    /// The size part's popover: the template's sizes, the chart's marked,
    /// each with the key that picks it, picked with a click.
    pub(super) fn fill_size_menu(self: &Rc<Self>, popover: &gtk::Popover, id: u32) {
        let Some(pane) = self.pane(id) else { return };
        let menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let (sizes, kind) = {
            let instrument = pane.instrument.borrow();
            let templates = self.trade.sizes.borrow();
            let sizes = instrument
                .as_ref()
                .and_then(|instrument| trading::sizes::template_for(&templates, instrument))
                .map(|template| template.sizes.clone())
                .unwrap_or_default();
            (sizes, instrument.as_ref().map(|instrument| instrument.kind))
        };
        let current = self.size_of(&pane);
        for (at, size) in sizes.iter().enumerate() {
            let button = gtk::Button::new();
            button.add_css_class("flat");
            button.add_css_class("link-row");
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            let label = match kind {
                Some(kind) => trading::sizes::size_text(*size, kind),
                None => size.to_string(),
            };
            let title = gtk::Label::new(Some(&label));
            title.set_xalign(0.0);
            title.set_hexpand(true);
            line.append(&title);
            if at < 9 {
                let key = crate::ui::shortcuts::accel_label(&format!("<Ctrl>{}", at + 1)).unwrap_or_default();
                let key = gtk::Label::new(Some(&key));
                key.add_css_class("dim-label");
                line.append(&key);
            }
            // Kept in place when not drawn, so the column does not move.
            let mark = gtk::Image::from_icon_name("object-select-symbolic");
            mark.set_opacity(if *size == current { 1.0 } else { 0.0 });
            line.append(&mark);
            button.set_child(Some(&line));
            let this = self.clone();
            let closing = popover.downgrade();
            button.connect_clicked(move |_| {
                if let Some(popover) = closing.upgrade() {
                    popover.popdown();
                }
                this.focus(id);
                this.pick_size(id, at + 1);
            });
            menu.append(&button);
        }
        popover.set_child(Some(&menu));
    }

    /// Mark every chart with its account's position and working orders in
    /// what it shows, or with nothing.
    pub(super) fn apply_marks(self: &Rc<Self>) {
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            self.apply_marks_to(pane);
        }
    }

    /// Mark one chart: only one whose instrument trades as the contract,
    /// on the account the chart is about.
    pub(super) fn apply_marks_to(self: &Rc<Self>, pane: &ChartPane) {
        if !self.trade.shown.get() {
            return;
        }
        self.write_size_of(pane);
        let Ok(target) = self.order_target(pane) else {
            return pane.view.set_marks(None, Vec::new());
        };
        let key = target.account.key();
        let theme = self.theming.borrow().theme();
        // The theme's trading colours, with only the chart's Red up turning
        // profit and loss round; never its candles' colours.
        let look = omacharts_engine::theme_trading(&theme, pane.colouring.get());
        let places = crate::ui::trading_panel::decimals_of(Self::tick_of(pane, &target));
        let price = |price: f64| omacharts_engine::grouped_decimals(price, places);
        let view = self.trading.borrow();
        let pending = self.trade.pending.borrow();
        let mut orders: Vec<crate::ui::chart::OrderMark> = view
            .book
            .working(&key, &target.contract)
            .filter_map(|order| {
                // On its way to a new price: drawn there, faint.
                let moving = match pending.get(&Request::Move(order.basket_id.clone())) {
                    Some(Pending::Move(price)) => Some(*price),
                    _ => None,
                };
                let cancelling = pending.contains_key(&Request::Cancel(order.basket_id.clone()));
                let at = moving.or(order.price)?;
                let style = match order.kind {
                    OrderKind::Limit => look.limit,
                    OrderKind::Stop => look.stop,
                    OrderKind::Market => return None,
                };
                let colour = match order.side {
                    Side::Buy => look.buy.clone(),
                    Side::Sell => look.sell.clone(),
                };
                Some(crate::ui::chart::OrderMark {
                    basket: order.basket_id.clone(),
                    side: order.side,
                    kind: order.kind,
                    qty: order.qty,
                    // Not yet working at the exchange, or asked to move or go.
                    dim: order.status == Stage::Pending || moving.is_some() || cancelling,
                    price: at,
                    words: trading::order_pill_text(order),
                    price_text: price(at),
                    colour,
                    style,
                })
            })
            .collect();
        // Orders asked for and not yet in the book: a limit or a stop at its
        // price, a market order beside the last price until it fills.
        let last = pane.view.last_price();
        for (request, asked) in pending.iter() {
            let (Request::Place(tag), Pending::Place { key: on, order }) = (request, asked) else { continue };
            if *on != key || order.contract != target.contract {
                continue;
            }
            let known = view.book.orders.iter().any(|known| known.tag.as_deref() == Some(tag.as_str()));
            let (at, words, style) = match (order.kind, order.price) {
                (OrderKind::Market, _) => match last {
                    Some(last) => (last, trading::buying_text(order.side, order.qty), look.position),
                    None => continue,
                },
                (_, Some(_)) if known => continue,
                (kind, Some(at)) => (
                    at,
                    format!("{} {} {}", order.side.glyph(), trading::order_words(order.side, kind), order.qty),
                    if kind == OrderKind::Stop { look.stop } else { look.limit },
                ),
                (_, None) => continue,
            };
            orders.push(crate::ui::chart::OrderMark {
                basket: String::new(),
                side: order.side,
                kind: order.kind,
                qty: order.qty,
                dim: true,
                price: at,
                words,
                price_text: price(at),
                colour: match order.side {
                    Side::Buy => look.buy.clone(),
                    Side::Sell => look.sell.clone(),
                },
                style,
            });
        }
        let currency = target.account.currency.clone();
        let position = view.book.position(&key, &target.contract).and_then(|held| {
            let at = held.avg?;
            let (side, colour) = match held.net > 0 {
                true => (Side::Buy, look.long.clone()),
                false => (Side::Sell, look.short.clone()),
            };
            // A tab for each exit the broker takes and the position still
            // has room for, in the look of the order it would make.
            let exit_colour = match trading::exit_side(held.net) {
                Side::Buy => look.buy.clone(),
                Side::Sell => look.sell.clone(),
            };
            let exits = [(OrderKind::Limit, look.limit), (OrderKind::Stop, look.stop)]
                .into_iter()
                .filter(|(kind, _)| target.capabilities.kinds.contains(kind))
                .filter(|(kind, _)| trading::exit_offered(view.book.working(&key, &target.contract), held.net, *kind))
                .map(|(kind, style)| crate::ui::chart::ExitTab { kind, colour: exit_colour.clone(), style })
                .collect();
            Some(crate::ui::chart::PositionMark {
                price: at,
                words: format!("{} {}", side.glyph(), trading::position_text(held, places, currency.as_deref())),
                price_text: price(at),
                colour,
                style: look.position,
                pill: held.open_pnl.map(|pnl| match pnl < 0.0 {
                    true => look.loss.clone(),
                    false => look.profit.clone(),
                }),
                net: held.net,
                exits,
                dim: pending.contains_key(&Request::Flatten(key.clone(), target.contract.clone())),
            })
        });
        drop(pending);
        pane.view.set_marks(position, orders);
    }

    /// Say each step of an order placed or cancelled here, once, when the
    /// broker has said so: working, filled and cancelled in a toast too,
    /// rejected in the corner notice.
    fn report_orders(&self) {
        let mut said = Vec::new();
        {
            let view = self.trading.borrow();
            let mut placed = self.trade.placed.borrow_mut();
            for (basket, before) in placed.iter_mut() {
                let Some(order) = view.book.order(basket) else { continue };
                let decimals = crate::ui::trading_panel::decimals_of(order.tick);
                let name = view
                    .accounts
                    .iter()
                    .find(|account| account.key() == order.account)
                    .map(|account| view.account_name(account))
                    .unwrap_or_default();
                if let Some(entry) = log_words::order_moved_on(order, *before, decimals, &name) {
                    said.push(entry);
                }
                *before = order.status.stage;
            }
            placed.retain(|_, stage| stage.working());
        }
        for entry in said {
            self.say(entry);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;
    use crate::trading::fake::Kept;
    use crate::trading::Session as _;

    /// A real window on a store in memory, charting from the synthetic
    /// feed: nothing reaches a network, a keyring or the session bus.
    pub(crate) fn window(store: &Rc<Store>) -> Rc<Window> {
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        let feed: Arc<dyn omacharts_engine::Provider> = Arc::new(omacharts_engine::providers::Synthetic::new(None));
        Window::build_with(&app, store.clone(), vec![feed])
    }

    pub(crate) fn typed() -> trading::Typed {
        trading::Typed {
            broker: "rithmic".into(),
            name: "Rithmic Test".into(),
            user: "someone".into(),
            password: String::new(),
            values: vec![("system".into(), "Rithmic Test".into()), ("gateway".into(), "wss://example.invalid:443".into())],
        }
    }

    fn corner_buttons(window: &Window) -> usize {
        let cluster = window.trade.cluster.borrow().clone().expect("a corner");
        let mut count = 0;
        let mut child = cluster.first_child();
        while let Some(widget) = child {
            count += 1;
            child = widget.next_sibling();
        }
        count
    }

    /// Without a login there is no trace of trading: no toggle in the
    /// corner, no action, no divider under the charts and no toast
    /// overlay. Adding one puts them in, and removing it takes them out,
    /// with the window open.
    #[test]
    fn trading_is_nowhere_without_a_login_and_comes_and_goes_with_one() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let bare = corner_buttons(&window);
            let nothing = |window: &Window| {
                window.trade.toggle.borrow().is_none()
                    && window.window.lookup_action("trading-panel").is_none()
                    && window.trade.paned.borrow().is_none()
                    && window.trade.toasts.borrow().is_none()
                    && window.chart_host.parent() == window.trade.with_tools.borrow().clone().map(|b| b.upcast())
            };
            assert!(nothing(&window), "trading in a window with no login");

            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            assert!(window.trade.toggle.borrow().is_some());
            assert_eq!(corner_buttons(&window), bare + 1);
            assert!(window.window.lookup_action("trading-panel").is_some());
            assert!(window.trade.toasts.borrow().is_some());
            gio::prelude::ActionGroupExt::activate_action(&window.window, "trading-panel", None);
            assert!(window.trading_panel_shown());
            assert!(window.trade.paned.borrow().is_some());
            assert!(store.setting_bool(SHOW_TRADING_PANEL, false), "remembered");

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.tell();
            assert!(nothing(&window), "trading left behind by the last login");
            assert_eq!(corner_buttons(&window), bare);
            assert!(!window.trading_panel_shown());
        });
    }

    /// Every label in a menu, its sections and submenus included.
    fn labels(model: &gio::MenuModel) -> Vec<(String, bool)> {
        let mut found = Vec::new();
        for at in 0..model.n_items() {
            let label = model.item_attribute_value(at, "label", None).and_then(|v| v.str().map(str::to_string));
            // Offered: pointing at an action that can run.
            let action = model
                .item_attribute_value(at, "action", None)
                .and_then(|v| v.str().map(str::to_string))
                .is_some_and(|action| format!("chart.{}", action.trim_start_matches("chart.")) != UNAVAILABLE);
            if let Some(label) = label {
                found.push((label, action));
            }
            for link in ["section", "submenu"] {
                if let Some(inner) = model.item_link(at, link) {
                    found.extend(labels(&inner));
                }
            }
        }
        found
    }

    fn trading_rows(window: &Rc<Window>) -> Vec<(String, bool)> {
        let model = window.chart_menu_model(100.0);
        labels(model.upcast_ref())
            .into_iter()
            .filter(|(label, _)| {
                ["Buy ", "Sell ", "Cancel ", "not signed in", "not tradable", "not known yet"]
                    .iter()
                    .any(|word| label.contains(word))
            })
            .collect()
    }

    /// The chart menu has no orders without a login, says why it cannot
    /// trade with one that is not signed in, offers the six orders once it
    /// is, places a market order through the session, and loses all of it
    /// with the login.
    #[test]
    fn the_chart_menu_offers_orders_only_with_a_login() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            assert!(trading_rows(&window).is_empty(), "orders in a menu with no login");
            assert!(window.trade.chart_actions.borrow().as_ref().unwrap().lookup_action("order").is_none());

            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            assert_eq!(trading_rows(&window), vec![("Rithmic Test not signed in".to_string(), false)]);

            let account = trading::Account {
                login: id.clone(),
                id: "951".into(),
                name: None,
                currency: Some("USD".into()),
                    balance: None,
            };
            let session = Arc::new(trading::fake::FakeSession::new(vec![account.clone()], Box::new(|| {})));
            assert!(window.desk.begin(&id));
            window.desk.opened(&id, Ok(session.clone()));
            window.desk.note(&store);
            window.desk.tell();
            // The chart is on SPY: not a future.
            assert_eq!(trading_rows(&window), vec![("SPY is not tradable on Rithmic Test".to_string(), false)]);

            let es = window.index.find("ES", None).expect("ES in the curated half");
            let pane = window.focused_pane();
            window.show_in(&pane, es);
            assert_eq!(trading_rows(&window), vec![("ES's contract not known yet".to_string(), false)]);
            session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
            let rows = trading_rows(&window);
            assert_eq!(rows.first(), Some(&("Buy Market · 1 ESZ6 · Rithmic Test".to_string(), true)));
            assert_eq!(rows.get(1), Some(&("Sell Market · 1 ESZ6 · Rithmic Test".to_string(), true)));
            assert_eq!(rows.len(), 6);

            let value = order_target_value(Side::Buy, OrderKind::Market, &account.key(), None);
            let actions = window.trade.chart_actions.borrow().clone().unwrap();
            actions.activate_action("order", Some(&value.to_variant()));
            let context = glib::MainContext::default();
            for _ in 0..200 {
                if pane.view.marks().0.is_some() {
                    break;
                }
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert_eq!(session.book().position("951", "ESZ6").map(|held| held.net), Some(1), "bought through the session");
            // The chart shows the position, and a working order once there is one.
            assert_eq!(pane.view.marks(), (Some("▲ +1 @ 7,612.25".to_string()), Vec::new()));
            let limit = trading::OrderRequest {
                account: "951".into(),
                contract: "ESZ6".into(),
                exchange: "CME".into(),
                side: Side::Sell,
                qty: 1,
                kind: OrderKind::Limit,
                price: Some(7_620.0),
                tag: trading::new_tag(),
            };
            let basket = session.place(&limit).unwrap();
            window.desk.tell();
            assert_eq!(pane.view.marks().1, vec!["▼ Sell Limit 1".to_string()]);
            session.cancel("951", &basket).unwrap();
            window.desk.tell();
            assert!(pane.view.marks().1.is_empty(), "a cancelled order left on the chart");
            // Answered, so the account is free for the next.
            assert!(window.trade.placing.begin(&account.key()).is_ok(), "the first was answered");
            window.trade.placing.done(&account.key());
            // One on its way holds a second back, which is never sent.
            window.trade.placing.begin(&account.key()).unwrap();
            let before = session.book().orders.len();
            actions.activate_action("order", Some(&value.to_variant()));
            context.iteration(false);
            assert_eq!(session.book().orders.len(), before, "a second order sent while the first was on its way");
            window.trade.placing.done(&account.key());

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.end(&id);
            window.desk.tell();
            assert!(trading_rows(&window).is_empty(), "orders left behind by the last login");
            assert!(window.trade.chart_actions.borrow().as_ref().unwrap().lookup_action("order").is_none());
            assert_eq!(pane.view.marks(), (None, Vec::new()), "marks left behind by the last login");
        });
    }

    /// A window on the synthetic feed with ES on its chart, signed in to
    /// the tests' session on account 951, which trades ES as ESZ6 and
    /// links orders or not; long one at 7,612.25.
    fn long_es(links: bool) -> (Rc<Window>, Rc<ChartPane>, Arc<trading::fake::FakeSession>, String) {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window(&store);
        let es = window.index.find("ES", None).expect("ES in the curated half");
        let pane = window.focused_pane();
        window.show_in(&pane, es);
        let id = trading::add_login(&store, &Kept::default(), &typed()).expect("a login");
        let account = trading::Account {
            login: id.clone(),
            id: "951".into(),
            name: None,
            currency: Some("USD".into()),
            balance: None,
        };
        let mut session = trading::fake::FakeSession::new(vec![account.clone()], Box::new(|| {}));
        session.links_orders = links;
        session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
        let session = Arc::new(session);
        assert!(window.desk.begin(&id));
        window.desk.opened(&id, Ok(session.clone()));
        window.desk.note(&store);
        session.place(&resting(Side::Buy, OrderKind::Market, None)).unwrap();
        window.desk.tell();
        assert!(pane.view.mark_states().0.is_some(), "no position on the chart");
        (window, pane, session, account.key())
    }

    fn resting(side: Side, kind: OrderKind, price: Option<f64>) -> trading::OrderRequest {
        trading::OrderRequest {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side,
            qty: 1,
            kind,
            price,
            tag: trading::new_tag(),
        }
    }

    /// After a drop the refreshed book is the broker's word. An order sent
    /// before it that the book has never heard of did not get there: it is
    /// no longer drawn as on its way, and the account takes orders again.
    /// One the book has is left to be answered as ever.
    #[test]
    fn an_order_the_refreshed_book_never_heard_of_never_reached_the_broker() {
        crate::ui::on_gtk(|| {
            use std::sync::atomic::Ordering;
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let id = trading::add_login(&store, &Kept::default(), &typed()).expect("a login");
            let account = trading::Account {
                login: id.clone(),
                id: "951".into(),
                name: None,
                currency: Some("USD".into()),
                balance: None,
            };
            let session = Arc::new(trading::fake::FakeSession::told(vec![account], window.desk.waker(&id)));
            assert!(window.desk.begin(&id));
            window.desk.opened(&id, Ok(session.clone()));
            window.desk.note(&store);
            window.desk.tell();
            let key = format!("{id}/951");

            // One the broker has, still pending at the exchange; one it never got.
            session.holds.store(true, Ordering::Release);
            let reached = resting(Side::Sell, OrderKind::Limit, Some(7_700.0));
            session.place(&reached).unwrap();
            let lost = resting(Side::Buy, OrderKind::Limit, Some(7_500.0));
            for order in [&reached, &lost] {
                window
                    .trade
                    .pending
                    .borrow_mut()
                    .insert(Request::Place(order.tag.clone()), Pending::Place { key: key.clone(), order: order.clone() });
            }
            window.trade.placing.begin(&key).unwrap();
            window.desk.tell();
            assert_eq!(window.trade.pending.borrow().len(), 2, "both on their way before the drop");

            session.drop_and_return(|_| {});
            settle(&window, || window.trade.pending.borrow().len() == 1);
            let pending = window.trade.pending.borrow();
            assert!(!pending.contains_key(&Request::Place(lost.tag.clone())), "never reached, still drawn");
            assert!(pending.contains_key(&Request::Place(reached.tag.clone())), "the broker has it");
            assert!(!window.trade.placing.busy(&key), "the account still waiting on an answer");
            assert_eq!(trading::never_placed_text(&lost), "Buy Limit 1 ESZ6 never reached the broker");
        });
    }

    /// A saved login signs in by itself once the window's first frame is on
    /// screen, never before it, and its account's money and orders fill
    /// the panel when it is back.
    #[test]
    fn a_saved_login_signs_in_after_the_first_frame() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let saved = trading::Typed { broker: "fake".into(), values: Vec::new(), ..typed() };
            let id = trading::add_login(&store, &Kept::default(), &saved).expect("a login");
            let window = window(&store);
            assert!(!window.desk.opening(&id), "signing in before the window is even shown");

            window.window.present();
            let clock = window.window.frame_clock().expect("a frame clock once shown");
            let first: Rc<Cell<Option<bool>>> = Rc::default();
            let (seen, desk, login) = (first.clone(), window.desk.clone(), id.clone());
            clock.connect_after_paint(move |_| {
                if seen.get().is_none() {
                    seen.set(Some(desk.opening(&login) || desk.live(&login)));
                }
            });
            assert!(!window.desk.opening(&id), "signing in as the window maps");
            settle(&window, || window.desk.live(&id));
            assert_eq!(first.get(), Some(false), "signed in before the first frame was painted");
            assert!(window.desk.live(&id));
            assert_eq!(window.desk.refreshes(&id), 1, "refreshed as it signed in");
            assert!(window.trading.borrow().book.refreshed);
            let key = format!("{id}/FAKE-1");
            assert_eq!(window.trading.borrow().book.money(&key).and_then(|money| money.balance), Some(50_000.0));
            window.desk.sign_out_all(trading::SIGN_OUT_WAIT);
            window.window.close();
        });
    }

    /// Run the main loop until `done`, or for a second at most.
    fn settle(window: &Rc<Window>, done: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        for _ in 0..200 {
            window.desk.tell();
            if done() {
                return;
            }
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// An order's × cancels it: faint at once, gone when the book says so;
    /// back as it was when the broker refuses, and not sent at all while
    /// another request to the account is on its way. The position's ×
    /// flattens it.
    #[test]
    fn the_marks_cross_cancels_an_order_and_flattens_the_position() {
        crate::ui::on_gtk(|| {
            use crate::ui::chart::TradeEvent;
            let (window, pane, session, key) = long_es(true);
            let basket = session.place(&resting(Side::Sell, OrderKind::Limit, Some(7_640.0))).unwrap();
            window.desk.tell();
            let orders = || pane.view.mark_states().1;
            assert_eq!(orders(), vec![("▼ Sell Limit 1".to_string(), 7_640.0, false)]);

            // Refused: back as it was, and said.
            *session.refuse.lock().unwrap() = Some("too late".into());
            pane.view.drop_mark(TradeEvent::Cancel(basket.clone()));
            assert!(orders()[0].2, "not faint at once");
            settle(&window, || !orders().first().is_some_and(|order| order.2));
            assert_eq!(orders(), vec![("▼ Sell Limit 1".to_string(), 7_640.0, false)], "not back after a refusal");

            // Another request on its way: nothing sent.
            window.trade.placing.begin(&key).unwrap();
            pane.view.drop_mark(TradeEvent::Cancel(basket.clone()));
            settle(&window, || false);
            assert!(session.book().order(&basket).unwrap().status.working(), "sent while another was on its way");
            window.trade.placing.done(&key);

            pane.view.drop_mark(TradeEvent::Cancel(basket.clone()));
            settle(&window, || orders().is_empty());
            assert_eq!(session.book().order(&basket).map(|order| order.status.stage), Some(Stage::Cancelled));
            assert!(orders().is_empty(), "a cancelled order left on the chart");
            assert!(window.trade.pending.borrow().is_empty(), "still waiting on a cancel the book has answered");

            pane.view.drop_mark(TradeEvent::Flatten);
            assert_eq!(pane.view.mark_states().0.map(|(_, dim)| dim), Some(true), "not faint at once");
            settle(&window, || pane.view.mark_states().0.is_none());
            assert!(session.book().position("951", "ESZ6").is_none(), "not flattened");
            assert!(pane.view.mark_states().0.is_none());
        });
    }

    /// An order's line let go at a new price moves it there: faint at the
    /// new price at once, as it was once the book has it. A refusal puts it
    /// back where it was; a price the market has crossed, an order filled
    /// while it was moving, or another request on its way sends nothing.
    #[test]
    fn an_order_dragged_to_a_new_price_is_moved_there() {
        crate::ui::on_gtk(|| {
            use crate::ui::chart::TradeEvent;
            let (window, pane, session, key) = long_es(true);
            settle(&window, || pane.view.last_price().is_some());
            let last = pane.view.last_price().expect("a last price");
            let above = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
            let basket = session.place(&resting(Side::Sell, OrderKind::Limit, Some(above(40.0)))).unwrap();
            window.desk.tell();
            let order = || pane.view.mark_states().1.first().map(|(_, price, dim)| (*price, *dim));
            let moved = |price| TradeEvent::Move { basket: basket.clone(), price };

            pane.view.drop_mark(moved(above(44.0)));
            assert_eq!(order(), Some((above(44.0), true)), "not drawn faint at its new price");
            settle(&window, || order() == Some((above(44.0), false)));
            assert_eq!(session.book().order(&basket).and_then(|o| o.price), Some(above(44.0)));
            assert_eq!(order(), Some((above(44.0), false)));
            assert!(window.trade.pending.borrow().is_empty());

            *session.refuse.lock().unwrap() = Some("price out of band".into());
            pane.view.drop_mark(moved(above(48.0)));
            settle(&window, || order() == Some((above(44.0), false)));
            assert_eq!(order(), Some((above(44.0), false)), "not back where it was after a refusal");

            // Below the market a sell limit would fill: refused here.
            pane.view.drop_mark(moved(above(-8.0)));
            settle(&window, || false);
            assert_eq!(session.book().order(&basket).and_then(|o| o.price), Some(above(44.0)), "moved across the market");

            window.trade.placing.begin(&key).unwrap();
            pane.view.drop_mark(moved(above(52.0)));
            settle(&window, || false);
            assert_eq!(session.book().order(&basket).and_then(|o| o.price), Some(above(44.0)), "moved while busy");
            window.trade.placing.done(&key);

            session.fill(&basket);
            pane.view.drop_mark(moved(above(52.0)));
            settle(&window, || false);
            assert_eq!(session.book().order(&basket).map(|o| o.status.stage), Some(Stage::Filled));
            assert!(window.trade.pending.borrow().is_empty(), "a filled order waiting to move");
        });
    }

    /// TP and SL pulled out of the position place a target and a stop for
    /// all of it, each tab going once its exit is working; the second is
    /// linked to the first. One on the wrong side of the market is not
    /// placed. Without links, nothing is linked, and flattening cancels
    /// the exits left behind.
    #[test]
    fn exits_pulled_out_of_the_position_are_placed_and_linked() {
        crate::ui::on_gtk(|| {
            use crate::ui::chart::TradeEvent;
            for links in [true, false] {
                let (window, pane, session, key) = long_es(links);
                settle(&window, || pane.view.last_price().is_some());
                let last = pane.view.last_price().expect("a last price");
                let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
                assert_eq!(pane.view.exit_tabs(), vec![OrderKind::Limit, OrderKind::Stop]);
                let working = || session.book().orders.iter().filter(|o| o.status.working()).count();

                pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(20.0) });
                settle(&window, || false);
                assert_eq!(working(), 0, "a long's stop placed above the market");

                pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(40.0) });
                // Answered too, so the account is free for the stop.
                settle(&window, || pane.view.exit_tabs() == vec![OrderKind::Stop] && !window.trade.placing.busy(&key));
                let target = session.book().orders[0].clone();
                assert_eq!((target.side, target.kind, target.qty, target.price), (Side::Sell, OrderKind::Limit, 1, Some(off(40.0))));
                assert_eq!(pane.view.exit_tabs(), vec![OrderKind::Stop], "the TP tab stayed");

                pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(-40.0) });
                settle(&window, || pane.view.exit_tabs().is_empty() && (!links || !session.links.lock().unwrap().is_empty()));
                let stop = session.book().orders[0].clone();
                assert_eq!((stop.side, stop.kind, stop.price), (Side::Sell, OrderKind::Stop, Some(off(-40.0))));
                let linked = session.links.lock().unwrap().clone();
                match links {
                    true => assert_eq!(linked, vec![vec![stop.basket_id.clone(), target.basket_id.clone()]]),
                    false => assert!(linked.is_empty(), "linked on a broker that does not link"),
                }

                pane.view.drop_mark(TradeEvent::Flatten);
                settle(&window, || pane.view.mark_states().0.is_none() && working() == 0);
                assert!(session.book().position("951", "ESZ6").is_none());
                assert_eq!(working(), 0, "an exit outlived the position it was for");
                assert!(pane.view.mark_states().1.is_empty());
            }
        });
    }

    /// A position held before the chart's contract is known is drawn as
    /// soon as bars arrive, which is when the feed has found it: not on the
    /// next move of the book, nor the next time the menu opens.
    #[test]
    fn marks_appear_once_the_contract_is_known() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(true);
            session.contracts.lock().unwrap().clear();
            window.desk.tell();
            assert!(pane.view.mark_states().0.is_none(), "marked with no contract");
            // The chart asks its feed for bars, and the feed finds the contract
            // while they are on their way. The book does not move.
            let es = pane.instrument.borrow().clone().expect("ES");
            window.show_in(&pane, es);
            assert!(pane.view.mark_states().0.is_none());
            session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
            let context = glib::MainContext::default();
            for _ in 0..200 {
                if pane.view.mark_states().0.is_some() {
                    break;
                }
                context.iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(pane.view.mark_states().0.is_some(), "the position waited for the book to move");
        });
    }

    /// What is asked shows at once, faint, and at full strength only once
    /// the exchange has it: an order until it is working, a market order
    /// beside the last price until it fills. A refusal takes it away; a
    /// request that is never answered stays pending. Part of an order
    /// filled says how much of how much.
    #[test]
    fn what_is_asked_is_pending_until_the_broker_confirms_it() {
        crate::ui::on_gtk(|| {
            use crate::ui::chart::TradeEvent;
            use std::sync::atomic::Ordering;
            let (window, pane, session, key) = long_es(true);
            settle(&window, || pane.view.last_price().is_some());
            let last = pane.view.last_price().expect("a last price");
            let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
            let orders = || pane.view.mark_states().1;
            session.holds.store(true, Ordering::Release);

            // Pending, then working.
            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(40.0) });
            assert_eq!(orders(), vec![("▼ Sell Limit 1".to_string(), off(40.0), true)], "not drawn at once, faint");
            settle(&window, || session.book().orders.len() == 2);
            settle(&window, || false);
            assert_eq!(orders(), vec![("▼ Sell Limit 1".to_string(), off(40.0), true)], "received is not working");
            let target = session.book().orders[0].basket_id.clone();
            session.open(&target);
            window.desk.tell();
            assert_eq!(orders(), vec![("▼ Sell Limit 1".to_string(), off(40.0), false)], "working, yet faint");
            assert!(window.trade.pending.borrow().is_empty());

            // Refused: gone, and said.
            *session.refuse.lock().unwrap() = Some("price out of band".into());
            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(-40.0) });
            assert_eq!(orders().len(), 2, "the stop not drawn at once");
            settle(&window, || orders().len() == 1);
            assert_eq!(orders().len(), 1, "a refused stop left on the chart");
            assert!(window.trade.pending.borrow().is_empty());

            // Never answered: still pending, however long.
            session.silent.store(true, Ordering::Release);
            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(-40.0) });
            settle(&window, || false);
            assert_eq!(orders().len(), 2);
            assert_eq!(orders()[1], ("▼ Sell Stop 1".to_string(), off(-40.0), true), "taken as done without an answer");
            assert_eq!(window.trade.pending.borrow().len(), 1);
            session.silent.store(false, Ordering::Release);
            window.trade.pending.borrow_mut().clear();

            // A market order: beside the last price until its fill.
            let value = order_target_value(Side::Buy, OrderKind::Market, &key, None);
            let actions = window.trade.chart_actions.borrow().clone().unwrap();
            actions.activate_action("order", Some(&value.to_variant()));
            assert!(orders().iter().any(|(words, _, dim)| words == "Buying 1…" && *dim), "no pending market order");
            settle(&window, || session.book().orders.iter().any(|o| o.kind == OrderKind::Market && o.status == Stage::Pending));
            settle(&window, || false);
            assert!(orders().iter().any(|(words, ..)| words == "Buying 1…"), "acknowledged is not filled");
            assert_eq!(session.book().position("951", "ESZ6").map(|held| held.net), Some(1));
            let bought = session.book().orders.iter().find(|o| o.kind == OrderKind::Market && o.status == Stage::Pending).unwrap().basket_id.clone();
            session.fill(&bought);
            window.desk.tell();
            assert!(!orders().iter().any(|(words, ..)| words == "Buying 1…"), "filled, still pending");
            assert_eq!(session.book().position("951", "ESZ6").map(|held| held.net), Some(2));

            // Part of an order filled.
            session.holds.store(false, Ordering::Release);
            let two = session.place(&trading::OrderRequest { qty: 2, ..resting(Side::Buy, OrderKind::Limit, Some(off(-60.0))) }).unwrap();
            session.fill_part(&two, 1);
            window.desk.tell();
            assert!(orders().iter().any(|(words, ..)| words == "▲ Buy Limit 1/2"), "{:?}", orders());
            assert_eq!(session.book().position("951", "ESZ6").map(|held| held.net), Some(3), "only what filled is held");
        });
    }

    /// Without a trading account the trading keys reach nothing: there is
    /// no action for Ctrl+B to buy with, and it does not show the
    /// watchlist either, which is Ctrl+Shift+B now.
    #[test]
    fn the_trading_keys_do_nothing_without_an_account() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let actions = window.trade.chart_actions.borrow().clone().unwrap();
            for name in [BUY_MARKET, SELL_MARKET, GO_FLAT, TRADING_SIZE] {
                assert!(!actions.has_action(name), "{name} without an account");
            }
            assert!(window.window.lookup_action("edit-trading-sizes").is_none());
            let shown = window.shows_sidebar();
            assert!(!window.on_trading_key(gtk::gdk::Key::b, 0), "Ctrl+B taken with no account");
            assert_eq!(window.shows_sidebar(), shown, "Ctrl+B toggled the watchlist");
            assert!(!window.on_trading_key(gtk::gdk::Key::_2, 11), "Ctrl+2 taken with no account");
            assert!(!window.focused_pane().size_menu.property::<bool>("visible"), "a size part with no account");
        });
    }

    /// With an account, Ctrl+B buys the chart's size at the market and
    /// Ctrl+S sells it; Ctrl+2 makes the size the template's second, two
    /// contracts, which the header says, the next Ctrl+B buys and the chart
    /// keeps. Ctrl+F leaves nothing held and nothing working.
    #[test]
    fn the_trading_keys_buy_sell_size_and_go_flat() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(true);
            let held = || session.book().position("951", "ESZ6").map(|held| held.net);
            let free = |window: &Rc<Window>| !window.trade.placing.busy(&key);
            // A market order on its way is drawn beside the last price.
            settle(&window, || pane.view.last_price().is_some());
            assert_eq!(held(), Some(1));
            assert!(pane.size_menu.property::<bool>("visible"), "no size part with an account");
            assert_eq!(pane.size_label_text(), "1 contract");

            assert!(window.on_trading_key(gtk::gdk::Key::b, 0));
            settle(&window, || held() == Some(2) && free(&window));
            assert_eq!(held(), Some(2), "Ctrl+B bought one");

            // Ctrl+2, by the key's place on the number row.
            assert!(window.on_trading_key(gtk::gdk::Key::_2, 11));
            assert_eq!(pane.size_label_text(), "2 contracts");
            assert_eq!(window.size_of(&pane).get(), 2);
            // Pending until the broker fills it: "Buying 2…" by the last price.
            session.holds.store(true, std::sync::atomic::Ordering::Release);
            assert!(window.on_trading_key(gtk::gdk::Key::b, 0));
            settle(&window, || session.book().orders.iter().filter(|order| order.qty == 2).count() == 1);
            let buying = pane.view.mark_states().1;
            assert!(buying.iter().any(|(words, ..)| words == "Buying 2…"), "{buying:?}");
            let basket = session.book().orders.iter().find(|order| order.qty == 2).unwrap().basket_id.clone();
            session.fill(&basket);
            settle(&window, || held() == Some(4) && free(&window));
            assert_eq!(held(), Some(4), "Ctrl+2 then Ctrl+B bought two");
            session.holds.store(false, std::sync::atomic::Ordering::Release);

            assert!(window.on_trading_key(gtk::gdk::Key::s, 0));
            settle(&window, || held() == Some(2) && free(&window));
            assert_eq!(held(), Some(2), "Ctrl+S sold two");

            // The size is the chart's, written down with it.
            let stored = window.store.setting(super::super::SETTING_WORKSPACE).unwrap_or_default();
            assert!(stored.contains("\"trading_size\""), "{stored}");

            // An order working in it too, then Ctrl+F: nothing left either way.
            let resting_basket = session.place(&resting(Side::Sell, OrderKind::Limit, Some(7_640.0))).unwrap();
            window.desk.tell();
            assert!(window.on_trading_key(gtk::gdk::Key::f, 0));
            settle(&window, || held().is_none() && !session.book().order(&resting_basket).unwrap().status.working());
            assert_eq!(held(), None, "Ctrl+F left a position");
            assert!(!session.book().order(&resting_basket).unwrap().status.working(), "Ctrl+F left an order");
        });
    }

    /// The size part's popover lists the template's sizes with the chart's
    /// marked; a right click on it offers to edit the templates, and the
    /// dialog's changes reach the header at once.
    #[test]
    fn the_size_part_offers_the_templates_sizes() {
        crate::ui::on_gtk(|| {
            let (window, pane, _session, _) = long_es(true);
            let popover = gtk::Popover::new();
            window.fill_size_menu(&popover, pane.id);
            let rows = popover.child().and_downcast::<gtk::Box>().expect("a list");
            let mut count = 0;
            let mut child = rows.first_child();
            while let Some(row) = child {
                count += 1;
                child = row.next_sibling();
            }
            assert_eq!(count, 9, "one to nine contracts");
            let menu = size_part_menu();
            assert_eq!(labels(menu.upcast_ref()), vec![("Edit trading sizes…".to_string(), true)]);
            assert!(window.window.lookup_action(EDIT_SIZES).is_some());

            let mut templates = trading::sizes::load(&window.store);
            templates[1].sizes = trading::sizes::parse_sizes("3, 6").unwrap();
            trading::sizes::save(&window.store, &templates);
            window.adopt_sizes();
            assert_eq!(pane.size_label_text(), "3 contracts", "the header kept the old template");
        });
    }

    /// The titles of every preferences group under `widget`.
    fn group_titles(widget: &gtk::Widget) -> Vec<String> {
        let mut found = Vec::new();
        if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
            found.push(group.title().to_string());
        }
        let mut child = widget.first_child();
        while let Some(inner) = child {
            found.extend(group_titles(&inner));
            child = inner.next_sibling();
        }
        found
    }

    /// The Theme page of a theme of one's own has a Trading group only
    /// while there is a login, and the open dialog follows a login added
    /// and removed.
    #[test]
    fn the_theme_page_speaks_of_trading_only_with_a_login() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let mut mine = omacharts_engine::theme::builtin_themes().remove(0);
            mine.id = "custom-test".into();
            mine.name = "Mine".into();
            mine.source = omacharts_engine::Source::Custom;
            store.save_theme(&mine);
            store.set_setting(crate::theming::SETTING_THEME, "custom-test");
            let window = window(&store);
            crate::ui::preferences::open_next_at_theme();
            window.open_preferences_at(None);
            let dialog = window.window.visible_dialog().expect("the preferences");
            let titles = || group_titles(dialog.upcast_ref());
            assert!(titles().iter().any(|title| title == "Bars"), "an editable theme: {:?}", titles());
            assert!(!titles().iter().any(|title| title == "Trading"), "Trading with no login");

            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            assert!(titles().iter().any(|title| title == "Trading"), "no Trading group with a login");

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.tell();
            assert!(!titles().iter().any(|title| title == "Trading"), "Trading left behind by the last login");
        });
    }

    #[test]
    fn an_order_target_value_reads_back() {
        let value = order_target_value(Side::Sell, OrderKind::Limit, "1a2b/951", Some(7_620.25));
        assert_eq!(value, "sell:limit:1a2b/951:7620.25");
        assert_eq!(parse_order_target(&value), Some((Side::Sell, OrderKind::Limit, "1a2b/951".into(), Some(7_620.25))));
        assert_eq!(parse_order_target("buy:market:x/1:"), Some((Side::Buy, OrderKind::Market, "x/1".into(), None)));
        assert_eq!(parse_order_target("buy:limit:x/1:"), None, "a limit with no price");
        assert_eq!(parse_order_target("hold:market:x/1:"), None);
    }

    /// A feed in memory that goes by a real feed's name, so the window
    /// treats it as that feed: Rithmic serving futures only, Yahoo serving
    /// everything. Its bars are the synthetic feed's; nothing is fetched.
    struct Posing {
        id: &'static str,
        label: &'static str,
        futures_only: bool,
        bars: omacharts_engine::providers::Synthetic,
    }

    impl Posing {
        fn feed(id: &'static str, label: &'static str, futures_only: bool) -> Arc<dyn omacharts_engine::Provider> {
            Arc::new(Posing { id, label, futures_only, bars: omacharts_engine::providers::Synthetic::new(None) })
        }
    }

    impl omacharts_engine::Provider for Posing {
        fn id(&self) -> &'static str {
            self.id
        }
        fn label(&self) -> &'static str {
            self.label
        }
        fn delay_minutes(&self, _: omacharts_engine::InstrumentKind) -> u32 {
            0
        }
        fn symbol_for(&self, instrument: &omacharts_engine::Instrument) -> Option<String> {
            let future = instrument.kind == omacharts_engine::InstrumentKind::FutureRoot;
            (future || !self.futures_only).then(|| instrument.symbol.clone())
        }
        fn capabilities(&self) -> &'static [omacharts_engine::Capability] {
            self.bars.capabilities()
        }
        fn bars(
            &self,
            symbol: &str,
            timeframe: omacharts_engine::Timeframe,
            since: Option<i64>,
        ) -> Result<Vec<omacharts_engine::Bar>, omacharts_engine::ProviderError> {
            self.bars.bars(symbol, timeframe, since)
        }
    }

    /// A window charting from Yahoo then Rithmic, both usable whatever
    /// this machine has saved, or from Yahoo alone.
    fn window_on(store: &Rc<Store>, both: bool) -> Rc<Window> {
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        let mut feeds = vec![Posing::feed("yahoo", "Yahoo Finance", false)];
        if both {
            feeds.push(Posing::feed("rithmic", "Rithmic", true));
        }
        let window = Window::build_with(&app, store.clone(), feeds);
        for feed in window.feeds() {
            feed.usable.set(true);
        }
        window
    }

    /// Sign in a login with `numbers` as its accounts, through a fake
    /// session, and tell the window. Its id and the session.
    fn sign_in(window: &Rc<Window>, store: &Rc<Store>, numbers: &[&str]) -> (String, Arc<trading::fake::FakeSession>) {
        let id = trading::add_login(store, &Kept::default(), &typed()).expect("a login");
        let accounts = numbers
            .iter()
            .map(|number| trading::Account {
                login: id.clone(),
                id: number.to_string(),
                name: None,
                currency: Some("USD".into()),
                    balance: None,
            })
            .collect();
        let session = Arc::new(trading::fake::FakeSession::new(accounts, Box::new(|| {})));
        assert!(window.desk.begin(&id));
        window.desk.opened(&id, Ok(session.clone()));
        window.desk.note(store);
        window.desk.tell();
        (id, session)
    }

    /// What the account's popover shows for `pane`, a line a row: indented
    /// under its account, "✓" on the one in force, a line under it after
    /// "—", "(heading)" on one that cannot be clicked, and "──" for the rule.
    fn picker(window: &Rc<Window>, pane: &ChartPane) -> Vec<String> {
        let popover = gtk::Popover::new();
        window.fill_source_menu(&popover, pane.id);
        let menu = popover.child().expect("a list");
        let mut rows = Vec::new();
        let mut next = menu.first_child();
        while let Some(child) = next {
            next = child.next_sibling();
            if child.is::<gtk::Separator>() {
                rows.push("──".to_string());
                continue;
            }
            let button = child.downcast::<gtk::Button>().expect("a row");
            let line = button.child().expect("a line");
            let mut text = if line.margin_start() == 24 { "    ".to_string() } else { String::new() };
            let mut part = line.first_child();
            while let Some(widget) = part {
                part = widget.next_sibling();
                if let Some(mark) = widget.downcast_ref::<gtk::Image>() {
                    if mark.opacity() == 1.0 {
                        text.push_str(" ✓");
                    }
                    continue;
                }
                let mut word = widget.first_child();
                let mut first = true;
                while let Some(label) = word {
                    word = label.next_sibling();
                    let label = label.downcast::<gtk::Label>().expect("words");
                    if !first {
                        text.push_str(" — ");
                    }
                    text.push_str(&label.label());
                    first = false;
                }
            }
            if !button.is_sensitive() {
                text.push_str(" (heading)");
            }
            rows.push(text);
        }
        rows
    }

    fn part(pane: &ChartPane) -> Option<String> {
        pane.source_menu.get_visible().then(|| {
            let label = pane.source_menu.child().and_downcast::<gtk::Label>().expect("the part's words");
            label.label().to_string()
        })
    }

    /// No account and two feeds: a flat list of the feeds that serve the
    /// chart, the order's pick the default, and the header naming the feed
    /// with what it passed over.
    #[test]
    fn the_picker_with_no_account_lists_the_feeds() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es.clone());
            assert_eq!(picker(&window, &pane), vec!["Yahoo Finance (default) ✓", "Rithmic"]);
            assert_eq!(part(&pane).as_deref(), Some("Yahoo Finance"));

            window.choose_source(pane.id, Some("rithmic".into()));
            assert_eq!(picker(&window, &pane), vec!["Yahoo Finance (default)", "Rithmic ✓"]);
            assert_eq!(part(&pane).as_deref(), Some("Rithmic"));
            // Choosing the default stores nothing.
            window.choose_source(pane.id, None);
            assert!(pane.source.borrow().is_none());

            // Passed over, it says so after the feed it fell back to.
            let before = window.routes_on_screen();
            window.feeds()[0].usable.set(false);
            window.reroute(&before);
            assert_eq!(part(&pane).as_deref(), Some("Rithmic · Yahoo Finance not signed in"));
            assert_eq!(picker(&window, &pane), vec!["Yahoo Finance (default)", "Rithmic ✓"]);
            window.feeds()[0].usable.set(true);

            // A stock: Rithmic does not serve it, so it is not offered.
            let spy = window.index.find("SPY", None).expect("SPY");
            window.show_in(&pane, spy);
            assert_eq!(picker(&window, &pane), vec!["Yahoo Finance (default) ✓"]);
        });
    }

    /// One account and two feeds: the account a heading, and under it its
    /// feeds with its own login's the default, ahead of the order. The
    /// header names the account, and the feed only once it is not the
    /// account's own; trouble goes after the account.
    #[test]
    fn the_picker_with_one_account_lists_its_feeds_under_it() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es.clone());
            let (_, session) = sign_in(&window, &store, &["951"]);

            let rows = |marks: [&str; 2]| {
                vec![
                    "Rithmic Test ✓ (heading)".to_string(),
                    format!("    Rithmic (default){}", marks[0]),
                    format!("    Yahoo Finance{}", marks[1]),
                ]
            };
            assert_eq!(picker(&window, &pane), rows([" ✓", ""]));
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test"), "on its own feed, the account alone");
            assert_eq!(pane.source_menu.tooltip_text().as_deref(), Some(trading::PART_TOOLTIP));

            window.choose_source(pane.id, Some("yahoo".into()));
            assert_eq!(picker(&window, &pane), rows(["", " ✓"]));
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test · Yahoo Finance"));
            // The default chosen again is no choice: nothing is kept.
            window.choose_source(pane.id, None);
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test"));

            // A chosen feed that cannot be used is said, after the account.
            window.choose_source(pane.id, Some("yahoo".into()));
            let before = window.routes_on_screen();
            window.feeds()[0].usable.set(false);
            window.reroute(&before);
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test · Yahoo Finance not signed in"));
            window.feeds()[0].usable.set(true);
            window.choose_source(pane.id, None);

            // The login in trouble.
            session.sign_out();
            window.desk.tell();
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test signed out"));
            assert_eq!(picker(&window, &pane)[0], "Rithmic Test — signed out ✓ (heading)");

            // A stock: the account's feed does not serve it, and the default
            // moves to the order's pick.
            let spy = window.index.find("SPY", None).expect("SPY");
            window.show_in(&pane, spy);
            assert_eq!(
                picker(&window, &pane),
                vec!["Rithmic Test — signed out ✓ (heading)", "    Yahoo Finance (default) ✓"]
            );
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test signed out"));
        });
    }

    /// Two accounts and two feeds: the accounts, the feeds under the
    /// chart's own only. Choosing another account forgets the chart's
    /// feed, and a chart on the default follows the default.
    #[test]
    fn choosing_an_account_forgets_the_feed() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            let (id, _session) = sign_in(&window, &store, &["951", "952"]);
            assert_eq!(
                picker(&window, &pane),
                vec![
                    "Rithmic Test 951 ✓",
                    "    Rithmic (default) ✓",
                    "    Yahoo Finance",
                    "Rithmic Test 952"
                ]
            );
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 951"));

            window.choose_source(pane.id, Some("yahoo".into()));
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 951 · Yahoo Finance"));
            let other = format!("{id}/952");
            window.choose_account(pane.id, Some(other.clone()));
            assert_eq!(pane.account.borrow().as_deref(), Some(other.as_str()));
            assert!(pane.source.borrow().is_none(), "the feed went with the account");
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 952"));
            assert_eq!(
                picker(&window, &pane),
                vec![
                    "Rithmic Test 951",
                    "Rithmic Test 952 ✓",
                    "    Rithmic (default) ✓",
                    "    Yahoo Finance"
                ]
            );

            // Back on the default, and the default moves: the chart follows.
            window.choose_account(pane.id, None);
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 951"));
            trading::set_default_account(&store, &other);
            window.desk.tell();
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 952"));
        });
    }

    /// Closing the window signs the trading sessions out with the feeds,
    /// so nothing is left signed in at the broker.
    #[test]
    fn closing_the_window_signs_trading_out() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, false);
            let (id, session) = sign_in(&window, &store, &["951"]);
            assert!(window.desk.live(&id));
            // Closing asks only a window that is on screen.
            window.window.present();
            window.window.close();
            assert!(session.trouble().is_some(), "still signed in after the window closed");
            assert!(!window.desk.live(&id));
        });
    }

    /// Two accounts and one feed: the accounts alone, and nothing about
    /// the feed anywhere.
    #[test]
    fn the_picker_with_one_feed_lists_the_accounts_alone() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, false);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            assert_eq!(part(&pane), None, "one feed and no account: nothing");
            sign_in(&window, &store, &["951", "952"]);
            assert_eq!(picker(&window, &pane), vec!["Rithmic Test 951 ✓", "Rithmic Test 952"]);
            assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 951"));
        });
    }

    /// What is asked of a broker from the chart, what became of it, what
    /// was refused, and a session ending by itself: each one entry, under
    /// the account's or the login's name.
    #[test]
    fn orders_asked_their_outcomes_and_refusals_are_logged() {
        crate::ui::on_gtk(|| {
            let (window, _pane, session, key) = long_es(true);
            let log = super::super::log::tests::private(&window);
            let held = || session.book().position("951", "ESZ6").map(|held| held.net);
            let free = |window: &Rc<Window>| !window.trade.placing.busy(&key);

            assert!(window.on_trading_key(gtk::gdk::Key::b, 0));
            settle(&window, || held() == Some(2) && free(&window));
            window.catch_up_log();
            assert!(window.on_trading_key(gtk::gdk::Key::f, 0));
            settle(&window, || held().is_none() && free(&window));
            assert!(window.on_trading_key(gtk::gdk::Key::f, 0));
            window.catch_up_log();

            let said: Vec<(omacharts_engine::log::Level, String, bool)> =
                log.entries().into_iter().map(|e| (e.level, e.line(), e.confirms)).collect();
            let account = window.account_label(&key);
            let one = |level: omacharts_engine::log::Level, line: String, confirms: bool| {
                let count = said.iter().filter(|said| **said == (level, line.clone(), confirms)).count();
                assert_eq!(count, 1, "{line:?} said {count} times in {said:#?}");
            };
            one(omacharts_engine::log::Level::Info, format!("{account} · Buy Market 1 ESZ6"), false);
            one(omacharts_engine::log::Level::Info, format!("{account} · Filled 1 ESZ6 at 7,612.25 · {account}"), true);
            one(omacharts_engine::log::Level::Info, format!("{account} · Flatten ESZ6 and cancel its orders"), false);
            one(omacharts_engine::log::Level::Error, format!("{account} · Already flat in ESZ6"), false);
            assert!(window.notice_shown().is_some(), "the refusal is not in the notice");

            // A session that ends by itself is said once, by the login's name.
            session.sign_out();
            window.desk.tell();
            window.desk.tell();
            let ended: Vec<String> =
                log.entries().into_iter().filter(|e| e.text.starts_with("Signed out")).map(|e| e.line()).collect();
            assert_eq!(ended, ["Rithmic Test · Signed out · signed out"]);
        });
    }
}
