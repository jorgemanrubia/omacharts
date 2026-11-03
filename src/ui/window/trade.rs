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
use crate::trading::{self, OrderKind, Side, Status};
use crate::ui::pane::ChartPane;
use crate::ui::trading_panel::{Hands, TradingPanel};

/// Whether the panel is open, remembered for the next launch.
pub const SHOW_TRADING_PANEL: &str = "show_trading_panel";

/// The disabled action a row the menu cannot offer points at, so it is
/// drawn greyed.
const UNAVAILABLE: &str = "chart.order-unavailable";

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
    pub placed: RefCell<HashMap<String, Status>>,
    /// The desk's call to read Trading again, kept alive with the window.
    pub listener: RefCell<Option<Rc<dyn Fn()>>>,
    /// The chart's actions, which `chart.order` and `chart.cancel-order`
    /// join while there is a login.
    pub chart_actions: RefCell<Option<gio::SimpleActionGroup>>,
    /// The accounts with an order on its way.
    pub placing: trading::Placing,
}

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
        self.report_orders();
        self.apply_marks();
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

        // The chart menu's orders. No key reaches them: an order is placed
        // with a click on a menu that says what it is about to do.
        if let Some(actions) = self.trade.chart_actions.borrow().as_ref() {
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
        if let Some(actions) = self.trade.chart_actions.borrow().as_ref() {
            actions.remove_action("order");
            actions.remove_action("cancel-order");
            actions.remove_action("order-unavailable");
        }
        let overlay = self.trade.overlay.borrow().clone();
        let root = self.trade.root.borrow().clone();
        if let (Some(overlay), Some(root), Some(toasts)) = (overlay, root, self.trade.toasts.borrow_mut().take()) {
            toasts.set_child(None::<&gtk::Widget>);
            overlay.set_child(Some(&root));
        }
        self.trade.placed.borrow_mut().clear();
        for pane in self.panes.borrow().iter() {
            pane.view.set_marks(None, Vec::new());
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

    /// Say `text` in a toast over the window.
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
        job: impl FnOnce(&dyn trading::Session, &str) -> Result<(), String> + Send + 'static,
    ) {
        let Some((login, number)) = account.split_once('/') else { return };
        let Some(session) = self.desk.session(login).filter(|_| self.desk.live(login)) else {
            self.toast(&format!("{} not signed in", self.account_label(account)));
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
                this.toast(&why);
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
        for (side, kind) in trading::menu_orders(&target.capabilities) {
            let price = (kind != OrderKind::Market).then_some(at).flatten();
            let text = match (kind, price) {
                (OrderKind::Market, _) => trading::menu_text(side, kind, "", &target.contract, &name),
                (_, Some((price, places))) => trading::menu_text(
                    side,
                    kind,
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

    /// `chart.order`: check again what the menu offered, ask on a live
    /// account, and place it.
    fn order_from_chart(self: &Rc<Self>, (side, kind, account, price): (Side, OrderKind, String, Option<f64>)) {
        let pane = self.focused_pane();
        // Resolved again: the chart, its account or the login may have
        // changed since the menu opened.
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => return self.toast(&format!("{why} · not placed")),
        };
        if target.account.key() != account {
            return self.toast("The chart's account changed · not placed");
        }
        let places = Self::tick_of(&pane, &target).map_or(2, |tick| omacharts_engine::on_tick(0.0, tick).1);
        let price_text = price.map(|price| omacharts_engine::grouped_decimals(price, places)).unwrap_or_default();
        if let (Some(at), Some(last)) = (price, pane.view.last_price())
            && !trading::side_allowed(kind, side, at, last)
        {
            return self.toast(&trading::crossed_text(&price_text, kind, side));
        }
        let order = trading::Order {
            account: target.account.id.clone(),
            contract: target.contract.clone(),
            exchange: target.exchange.clone(),
            side,
            qty: 1,
            kind,
            price,
            tag: trading::new_tag(),
        };
        if !target.live() {
            return self.place(target, order);
        }
        // Money: asked every time, naming the account and the system.
        let dialog = adw::AlertDialog::new(Some(&trading::confirm_text(side, kind, &price_text, &target)), None);
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("place", &format!("{} {}", side.word(), target.contract));
        dialog.set_response_appearance("place", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let this = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "place" {
                this.place(target.clone(), order.clone());
            }
        });
        dialog.present(Some(&self.window));
    }

    /// Send `order` off the main loop, one at a time per account. A failure
    /// is said and never retried.
    fn place(self: &Rc<Self>, target: trading::Target, order: trading::Order) {
        let key = target.account.key();
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.toast(&why);
        }
        let Some(session) = self.desk.session(&target.login.id).filter(|_| self.desk.live(&target.login.id)) else {
            self.trade.placing.done(&key);
            return self.toast(&format!("{} not signed in · not placed", self.account_label(&key)));
        };
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = sender.send_blocking(session.place(&order));
        });
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let outcome = receiver.recv().await.unwrap_or_else(|_| Err("the session went away".into()));
            let Some(this) = this.upgrade() else { return };
            this.trade.placing.done(&key);
            match outcome {
                Ok(basket) => {
                    this.trade.placed.borrow_mut().insert(basket, Status::Pending);
                    // What it has done already, the book may say now.
                    this.desk.tell();
                }
                Err(why) => this.toast(&format!("Not placed: {why}")),
            }
        });
    }

    /// `chart.cancel-order`: cancel the working order `basket`, no
    /// question asked.
    fn cancel_from_chart(self: &Rc<Self>, basket: &str) {
        let account = self.trading.borrow().book.order(basket).map(|order| order.account.clone());
        let Some(account) = account else { return };
        let basket = basket.to_string();
        self.on_session(&account, move |session, number| session.cancel(number, &basket));
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
        let orders = view
            .book
            .working(&key, &target.contract)
            .filter_map(|order| {
                let at = order.price?;
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
                    price: at,
                    words: format!(
                        "{} {} {}",
                        order.side.glyph(),
                        trading::order_words(order.side, order.kind),
                        order.qty
                    ),
                    price_text: price(at),
                    colour,
                    style,
                })
            })
            .collect();
        let currency = target.account.currency.clone();
        let position = view.book.position(&key, &target.contract).and_then(|held| {
            let at = held.avg?;
            let (side, colour) = match held.net > 0 {
                true => (Side::Buy, look.long.clone()),
                false => (Side::Sell, look.short.clone()),
            };
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
            })
        });
        pane.view.set_marks(position, orders);
    }

    /// Toast each step of an order placed here, once: filled, working,
    /// rejected.
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
                if let Some(text) = trading::outcome_text(order, before, decimals, &name) {
                    said.push(text);
                }
                *before = order.status.clone();
            }
            placed.retain(|_, status| status.working());
        }
        for text in said {
            self.toast(&text);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::store::Store;
    use crate::trading::tests::Kept;
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
            name: "Rithmic Test".into(),
            user: "someone".into(),
            password: String::new(),
            system: String::new(),
            gateway: "wss://example.invalid:443".into(),
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
        if !crate::ui::gtk_ready() {
            return;
        }
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
        if !crate::ui::gtk_ready() {
            return;
        }
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
            fcm: String::new(),
            ib: String::new(),
            balance: None,
        };
        let session = Arc::new(trading::tests::FakeSession::new(vec![account.clone()], Box::new(|| {})));
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
        let limit = trading::Order {
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
        if !crate::ui::gtk_ready() {
            return;
        }
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
    fn sign_in(window: &Rc<Window>, store: &Rc<Store>, numbers: &[&str]) -> (String, Arc<trading::tests::FakeSession>) {
        let id = trading::add_login(store, &Kept::default(), &typed()).expect("a login");
        let accounts = numbers
            .iter()
            .map(|number| trading::Account {
                login: id.clone(),
                id: number.to_string(),
                name: None,
                currency: Some("USD".into()),
                fcm: String::new(),
                ib: String::new(),
                balance: None,
            })
            .collect();
        let session = Arc::new(trading::tests::FakeSession::new(accounts, Box::new(|| {})));
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
        if !crate::ui::gtk_ready() {
            return;
        }
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
    }

    /// One account and two feeds: the account a heading, and under it its
    /// feeds with its own login's the default, ahead of the order. The
    /// header names the account, and the feed only once it is not the
    /// account's own; trouble goes after the account.
    #[test]
    fn the_picker_with_one_account_lists_its_feeds_under_it() {
        if !crate::ui::gtk_ready() {
            return;
        }
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
                "──".to_string(),
                "Trading accounts…".to_string(),
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
        assert_eq!(picker(&window, &pane)[0], "Rithmic Test ✓ — signed out (heading)");

        // A stock: the account's feed does not serve it, and the default
        // moves to the order's pick.
        let spy = window.index.find("SPY", None).expect("SPY");
        window.show_in(&pane, spy);
        assert_eq!(
            picker(&window, &pane),
            vec!["Rithmic Test ✓ — signed out (heading)", "    Yahoo Finance (default) ✓", "──", "Trading accounts…"]
        );
        assert_eq!(part(&pane).as_deref(), Some("Rithmic Test signed out"));
    }

    /// Two accounts and two feeds: the accounts, the feeds under the
    /// chart's own only. Choosing another account forgets the chart's
    /// feed, and a chart on the default follows the default.
    #[test]
    fn choosing_an_account_forgets_the_feed() {
        if !crate::ui::gtk_ready() {
            return;
        }
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
                "Rithmic Test 952",
                "──",
                "Trading accounts…"
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
                "    Yahoo Finance",
                "──",
                "Trading accounts…"
            ]
        );

        // Back on the default, and the default moves: the chart follows.
        window.choose_account(pane.id, None);
        assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 951"));
        trading::set_default_account(&store, &other);
        window.desk.tell();
        assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 952"));
    }

    /// Two accounts and one feed: the accounts alone, and nothing about
    /// the feed anywhere.
    #[test]
    fn the_picker_with_one_feed_lists_the_accounts_alone() {
        if !crate::ui::gtk_ready() {
            return;
        }
        let store = Rc::new(Store::memory().expect("a store"));
        let window = window_on(&store, false);
        let pane = window.focused_pane();
        let es = window.index.find("ES", None).expect("ES in the curated half");
        window.show_in(&pane, es);
        assert_eq!(part(&pane), None, "one feed and no account: nothing");
        sign_in(&window, &store, &["951", "952"]);
        assert_eq!(picker(&window, &pane), vec!["Rithmic Test 951 ✓", "Rithmic Test 952", "──", "Trading accounts…"]);
        assert_eq!(part(&pane).as_deref(), Some("Rithmic Test 951"));
    }
}
