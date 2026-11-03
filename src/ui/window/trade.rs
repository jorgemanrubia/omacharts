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
use crate::trading::{self, Status};
use crate::ui::trading_panel::{Hands, TradingPanel};

/// Whether the panel is open, remembered for the next launch.
pub(super) const SHOW_TRADING_PANEL: &str = "show_trading_panel";

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

        let action = gio::SimpleAction::new("trading-panel", None);
        let this = self.clone();
        action.connect_activate(move |_, _| {
            let toggle = this.trade.toggle.borrow().clone();
            if let Some(toggle) = toggle {
                toggle.set_active(!toggle.is_active());
            }
        });
        self.window.add_action(&action);

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
        self.window.remove_action("trading-panel");
        let overlay = self.trade.overlay.borrow().clone();
        let root = self.trade.root.borrow().clone();
        if let (Some(overlay), Some(root), Some(toasts)) = (overlay, root, self.trade.toasts.borrow_mut().take()) {
            toasts.set_child(None::<&gtk::Widget>);
            overlay.set_child(Some(&root));
        }
        self.trade.placed.borrow_mut().clear();
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

    /// The name an account goes by here: "951", or "Lucid 1234" with
    /// several logins.
    pub(super) fn account_label(&self, key: &str) -> String {
        let view = self.trading.borrow();
        view.accounts
            .iter()
            .find(|account| account.key() == key)
            .map(|account| trading::account_name(account, &view.logins))
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

    /// Toast each step of an order placed here, once: filled, working,
    /// rejected.
    fn report_orders(&self) {
        let mut said = Vec::new();
        {
            let view = self.trading.borrow();
            let mut placed = self.trade.placed.borrow_mut();
            for (basket, before) in placed.iter_mut() {
                let Some(order) = view.book.order(basket) else { continue };
                let decimals = crate::ui::trading_panel::decimals_of(&order.contract);
                let name = view
                    .accounts
                    .iter()
                    .find(|account| account.key() == order.account)
                    .map(|account| trading::account_name(account, &view.logins))
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
}
