//! What trading puts in the window: the panel across the bottom, under
//! the charts and the rail alike, its toggle in the bottom-left corner,
//! the toasts orders report in, the chart menu's orders and the marks on
//! the charts.
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

/// How tall the toggle's corner is before it has been laid out: the button
/// and `.window-corner.foot`'s margin under it.
const FOOT_HEIGHT: i32 = 22 + 2;

/// The panel's tabs that have a key each, as `win.trading-<tab>`.
const PANEL_TABS: [&str; 3] = ["accounts", "positions", "orders"];

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
    /// The charts and the rail with the toggle laid over them, in the
    /// split's place while there is a login: the toggle stands in the
    /// bottom-left corner of the pair whether or not the panel is open
    /// under them, and the panel's divider goes under all of it.
    pub docked: RefCell<Option<gtk::Overlay>>,
    /// The toggle's corner, the window's third: the mirror of the drawing
    /// tools' corner down the same edge.
    pub foot: RefCell<Option<gtk::Box>>,
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
    /// When the positions were last marked to a new price, and whether a
    /// marking is waiting its turn: a busy market is marked a few times a
    /// second, and its last price never left out.
    pub marked_at: Cell<Option<std::time::Instant>>,
    pub marking: Cell<bool>,
}

/// How often at most the positions are marked to a moving market.
const MARK_EVERY: std::time::Duration = std::time::Duration::from_millis(200);

/// The toggle's glyph: the sidebars' icon turned on its side. Adwaita's
/// `sidebar-show-right-symbolic` is a rounded frame with a divider and the
/// pane past it washed in; this is the same frame with the divider across
/// it and the pane under, which is where the panel is. Drawn rather than
/// looked up because no icon theme ships a bottom panel, and drawn to the
/// icon's own geometry — a 16-pixel frame in a two-pixel line, the pane at
/// a third of the ink — so beside the watchlist's toggle it is one family.
/// In the widget's own colour, which follows the theme, as the maximize
/// glyph does.
fn panel_icon() -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_draw_func(|area, cr, width, height| {
        let colour = area.color();
        let ink = |alpha: f64| {
            let (r, g, b, a) = (colour.red(), colour.green(), colour.blue(), colour.alpha());
            cr.set_source_rgba(r as f64, g as f64, b as f64, a as f64 * alpha);
        };
        ink(1.0);
        // The 16-pixel glyph, centred, and half a pixel lower where the
        // screen has half pixels: in the bottom-left corner the band it
        // centres in is an odd 25 tall, and at 1x the half would blur it.
        let scale = area.scale_factor() as f64;
        let half = (0.5 * scale).floor() / scale;
        let (x, y) = (((width - 16) / 2) as f64, ((height - 16) / 2) as f64 + half);
        let rounded = |x: f64, y: f64, w: f64, h: f64, r: f64| {
            use std::f64::consts::PI;
            cr.new_sub_path();
            cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
            cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
            cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
            cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
            cr.close_path();
        };
        // The frame: the outer shape less the inner one.
        cr.set_fill_rule(gtk::cairo::FillRule::EvenOdd);
        rounded(x, y, 16.0, 16.0, 3.0);
        rounded(x + 2.0, y + 2.0, 12.0, 12.0, 1.0);
        let _ = cr.fill();
        // The divider, and the pane under it.
        cr.set_fill_rule(gtk::cairo::FillRule::Winding);
        cr.rectangle(x + 2.0, y + 8.0, 12.0, 2.0);
        let _ = cr.fill();
        ink(0.35);
        cr.rectangle(x + 2.0, y + 10.0, 12.0, 4.0);
        let _ = cr.fill();
    });
    area
}

/// What a right click on a chart's Trading size offers.
pub(super) fn size_part_menu() -> gio::Menu {
    let model = gio::Menu::new();
    model.append(Some("Edit quantities…"), Some(&format!("win.{EDIT_SIZES}")));
    model
}

/// `order` in words: "Buy Limit 1 ESZ6 at 7,620.00".
fn order_in_words(order: &trading::Order) -> String {
    let decimals = crate::ui::trading_panel::decimals_of(order.tick);
    let price = order.price.map(|price| omacharts_engine::grouped_decimals(price, decimals));
    trading::order_text(order.side, order.kind, order.qty, &order.contract, price.as_deref())
}

/// What cancelling `order` asks for: "Cancel Buy Limit 1 ESZ6 at
/// 7,620.00".
fn cancel_words(order: &trading::Order) -> String {
    format!("Cancel {}", order_in_words(order))
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

    /// A contract held traded at a new price: read the book again, which
    /// marks the positions to it, and redraw the panel and the charts'
    /// positions. Nothing else about Trading is read, as nothing else moved.
    pub(super) fn mark_to_market(self: &Rc<Self>) {
        if !self.trade.shown.get() || self.trade.marking.get() {
            return;
        }
        let since = self.trade.marked_at.get().map(|at| at.elapsed());
        if let Some(wait) = since.and_then(|since| MARK_EVERY.checked_sub(since)) {
            self.trade.marking.set(true);
            let this = Rc::downgrade(self);
            glib::timeout_add_local_once(wait, move || {
                if let Some(this) = this.upgrade() {
                    this.trade.marking.set(false);
                    this.mark_to_market();
                }
            });
            return;
        }
        self.trade.marked_at.set(Some(std::time::Instant::now()));
        let book = self.desk.book(&self.trading.borrow().logins);
        self.trading.borrow_mut().book = book;
        let panel = self.trade.panel.borrow().clone();
        if let Some(panel) = panel {
            panel.redraw(&self.trading.borrow());
        }
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
                self.refuse_as(account, &format!("{not} · {why}"));
                None
            }
            Err(Failure::Unanswered(why)) => {
                self.refuse_as(account, &format!("No answer · {why}"));
                None
            }
        }
    }

    fn install_trading(self: &Rc<Self>) {
        self.trade.shown.set(true);

        // The panel's toggle, in the bottom-left corner: the drawing
        // tools' corner's mirror down the left edge, on the row the
        // bottom-left chart writes its dates on — at the foot of the
        // tools' bar while the bar is out, and over the chart's first
        // date, which the chart leaves out, while it is put away. Laid
        // over the split of charts and rail rather than the window, so
        // that it keeps its place above the panel when the panel opens
        // under them, and so the chartbook strip has nothing to clear.
        let toggle = gtk::ToggleButton::new();
        toggle.set_child(Some(&panel_icon()));
        toggle.set_tooltip_text(Some(&crate::ui::shortcuts::tooltip("Trading", "win.trading-accounts")));
        toggle.add_css_class("flat");
        let this = self.clone();
        toggle.connect_toggled(move |toggle| this.show_trading_panel(toggle.is_active()));
        *self.trade.toggle.borrow_mut() = Some(toggle.clone());
        let foot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        foot.add_css_class("window-corner");
        foot.add_css_class("foot");
        foot.set_halign(gtk::Align::Start);
        foot.set_valign(gtk::Align::End);
        foot.append(&toggle);
        if let Some(root) = self.trade.root.borrow().as_ref() {
            let docked = gtk::Overlay::new();
            root.remove(&self.split);
            docked.set_child(Some(&self.split));
            docked.add_overlay(&foot);
            root.prepend(&docked);
            *self.trade.docked.borrow_mut() = Some(docked);
        }
        *self.trade.foot.borrow_mut() = Some(foot);
        // The bottom-left chart keeps its first date out from under it.
        self.sync_corner_clearance();

        let action = gio::SimpleAction::new("trading-panel", None);
        let this = self.clone();
        action.connect_activate(move |_, _| {
            let toggle = this.trade.toggle.borrow().clone();
            if let Some(toggle) = toggle {
                toggle.set_active(!toggle.is_active());
            }
        });
        self.window.add_action(&action);

        // Ctrl+Shift+A, P and O: the panel on that tab, or shut if it is
        // already showing it.
        for tab in PANEL_TABS {
            let action = gio::SimpleAction::new(&format!("trading-{tab}"), None);
            let this = self.clone();
            action.connect_activate(move |_, _| this.toggle_panel_tab(tab));
            self.window.add_action(&action);
        }

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
                    this.cancel_mark(&this.focused_pane(), &basket);
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
    }

    fn remove_trading(self: &Rc<Self>) {
        self.trade.shown.set(false);
        self.unmount_panel();
        // The split back in its own place, with nothing over it: the tree
        // a window with no login has.
        self.trade.toggle.borrow_mut().take();
        let foot = self.trade.foot.borrow_mut().take();
        if let Some(docked) = self.trade.docked.borrow_mut().take()
            && let Some(root) = self.trade.root.borrow().as_ref()
        {
            if let Some(foot) = foot {
                docked.remove_overlay(&foot);
            }
            docked.set_child(None::<&gtk::Widget>);
            root.remove(&docked);
            root.prepend(&self.split);
        }
        self.window.remove_action("trading-panel");
        for tab in PANEL_TABS {
            self.window.remove_action(&format!("trading-{tab}"));
        }
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
            pane.view.set_marks(None, Vec::new(), Vec::new(), (None, None));
            self.write_size_of(pane);
        }
    }

    /// The split of charts and rail as it stands in the window: under the
    /// toggle while there is a login, bare when there is none.
    fn docked(&self) -> gtk::Widget {
        match self.trade.docked.borrow().as_ref() {
            Some(docked) => docked.clone().upcast(),
            None => self.split.clone().upcast(),
        }
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

    /// The panel on `tab`, or the panel shut when it is open on `tab`
    /// already: what each tab's key does.
    pub(super) fn toggle_panel_tab(self: &Rc<Self>, tab: &str) {
        let open = self.trade.toggle.borrow().as_ref().is_some_and(|toggle| toggle.is_active());
        let front = self.trade.panel.borrow().as_ref().and_then(|panel| panel.tab());
        if open && front.as_deref() == Some(tab) {
            self.show_trading_panel(false);
            return;
        }
        self.show_trading_panel(true);
        if let Some(panel) = self.trade.panel.borrow().as_ref() {
            panel.show_tab(tab);
        }
    }

    /// How much of the window's bottom trading takes: the panel and its
    /// divider while the panel is open, and the toggle's corner, which
    /// anything laid over the charts from the bottom-left has to clear.
    pub(super) fn trading_clearance(&self) -> i32 {
        let foot = self.trade.foot.borrow().as_ref().map_or(0, |foot| foot.height().max(FOOT_HEIGHT));
        if !self.trading_panel_shown() {
            return foot;
        }
        let paned = self.trade.paned.borrow().as_ref().map_or(0, |paned| paned.height());
        let docked = self.trade.docked.borrow().as_ref().map_or(0, |docked| docked.height());
        (paned - docked).max(0) + foot
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
        let panel = TradingPanel::new(hands, self.store.clone());
        panel.root.set_size_request(-1, 150);
        let paned = gtk::Paned::new(gtk::Orientation::Vertical);
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(false);
        paned.set_shrink_end_child(false);
        paned.set_vexpand(true);
        paned.set_hexpand(true);
        // Across the whole window, rail included: the panel is the
        // account's, and the rail is as much the account's as the charts
        // are. Above the chartbook strip, which is the window's status line.
        if let Some(root) = self.trade.root.borrow().as_ref() {
            let docked = self.docked();
            root.remove(&docked);
            paned.set_start_child(Some(&docked));
            paned.set_end_child(Some(&panel.root));
            root.prepend(&paned);
        }
        *self.trade.paned.borrow_mut() = Some(paned);
        *self.trade.panel.borrow_mut() = Some(panel.clone());
        panel
    }

    /// Take the panel and its divider out, the charts back where they were.
    fn unmount_panel(&self) {
        self.trade.panel.borrow_mut().take();
        let Some(paned) = self.trade.paned.borrow_mut().take() else { return };
        if let Some(root) = self.trade.root.borrow().as_ref() {
            paned.set_start_child(None::<&gtk::Widget>);
            paned.set_end_child(None::<&gtk::Widget>);
            root.remove(&paned);
            root.prepend(&self.docked());
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

    /// Say that what was asked of `target`'s account needed nothing doing,
    /// "Already flat in ESZ6": in the Log, and nowhere else.
    pub(super) fn note_on(&self, target: &trading::Target, text: &str) {
        self.say(log_words::note(&self.account_label(&target.account.key()), text));
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

    /// The name an account goes by here: see [`trading::View::name_of`].
    pub(super) fn account_label(&self, key: &str) -> String {
        self.trading.borrow().name_of(key)
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
        let name = self.account_label(account);
        let Some(session) = self.desk.session(login).filter(|_| self.desk.live(login)) else {
            self.refuse_as(&name, &format!("{name} not signed in"));
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
                this.refuse_as(&name, why.words());
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

    /// The price an order from `pane` on `target` is judged against: the
    /// chart's own when the account's feed charts it, else the last trade
    /// the account's session has heard, which on a chart fed from elsewhere
    /// can be a long way off; the chart's while the session has none.
    pub(super) fn market_price(&self, pane: &ChartPane, target: &trading::Target) -> Option<f64> {
        let chart = pane.view.last_price();
        let Some(instrument) = pane.instrument.borrow().clone() else { return chart };
        let own = trading::data_feed_of(&target.login).map(|feed| feed.id);
        let charted = match self.route_chosen(pane, &instrument) {
            Some(Ok(routed)) => Some(routed.feed.provider.id()),
            _ => None,
        };
        if own.is_some() && own == charted.as_deref() {
            return chart;
        }
        self.desk.session(&target.login.id).and_then(|session| session.last_price(&instrument)).or(chart)
    }

    /// The day's lowest and highest prices the exchange takes an order on
    /// `pane`'s instrument at, as far as the account's broker has said.
    fn limits_of(&self, pane: &ChartPane, target: &trading::Target) -> (Option<f64>, Option<f64>) {
        let Some(instrument) = pane.instrument.borrow().clone() else { return (None, None) };
        self.desk.session(&target.login.id).map_or((None, None), |session| session.price_limits(&instrument))
    }

    /// Why an order at `price` on `pane` would be refused for going past
    /// the day's limit, put plainly, if it would.
    fn past_limit(&self, pane: &ChartPane, target: &trading::Target, price: f64) -> Option<String> {
        let limit = trading::past_limit(price, self.limits_of(pane, target))?;
        let places = Self::tick_of(pane, target).map_or(2, |tick| omacharts_engine::on_tick(limit, tick).1);
        Some(trading::past_limit_text(&omacharts_engine::grouped_decimals(limit, places)))
    }

    /// The step an order's price moves in: the contract's, from its
    /// specification, else the step `pane`'s prices are on.
    fn tick_of(pane: &ChartPane, target: &trading::Target) -> Option<f64> {
        target.tick.or_else(|| pane.view.tick())
    }

    /// The chart menu's orders, at height `y` of the focused chart, in one
    /// section: buy and sell at the market, then the limit and stop each
    /// way that the price there allows, and cancelling the order under the
    /// pointer. One that could not go is left out; a chart that cannot
    /// trade says why in one greyed row. No name over them: they
    /// go to the account the header has chosen. With no login, nothing.
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
        // The contract may have become known since the book last moved.
        self.apply_marks_to(&pane);
        let tick = Self::tick_of(&pane, &target);
        let at = pane.view.price_under(y).map(|price| match tick {
            Some(tick) => omacharts_engine::on_tick(price, tick),
            None => (price, 2),
        });
        let last = self.market_price(&pane, &target);
        let limits = self.limits_of(&pane, &target);
        let size = self.size_of(&pane);
        for (side, kind) in trading::menu_orders(&target.capabilities) {
            let price = (kind != OrderKind::Market).then_some(at).flatten();
            // One that could not go is left out, not greyed: the wrong side
            // of the market, past the day's limit, or with no price to judge.
            let allowed = match (kind, price, last) {
                (OrderKind::Market, _, _) => true,
                (_, Some((price, _)), Some(last)) => {
                    trading::side_allowed(kind, side, price, last) && trading::past_limit(price, limits).is_none()
                }
                _ => false,
            };
            if !allowed {
                continue;
            }
            let text = match price {
                None => trading::menu_text(side, kind, size, "", &target.contract),
                Some((price, places)) => trading::menu_text(
                    side,
                    kind,
                    size,
                    &omacharts_engine::grouped_decimals(price, places),
                    &target.contract,
                ),
            };
            let item = gio::MenuItem::new(Some(&text), None);
            let value = order_target_value(side, kind, &key, price.map(|(price, _)| price));
            item.set_action_and_target_value(Some("chart.order"), Some(&value.to_variant()));
            section.append_item(&item);
        }

        // Cancelling the order whose line the pointer is on.
        let view = self.trading.borrow();
        let near = trading::order_near(view.book.working(&key, &target.contract), y, |price| {
            pane.view.y_of(price).unwrap_or(f64::INFINITY)
        });
        if let Some(order) = near {
            let item = gio::MenuItem::new(Some(&cancel_words(order)), None);
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
            Err(why) => return self.refuse(&format!("Not placed · {why}")),
        };
        if target.account.key() != account {
            return self.refuse_on(&target, "Not placed · the chart's account changed");
        }
        let places = Self::tick_of(&pane, &target).map_or(2, |tick| omacharts_engine::on_tick(0.0, tick).1);
        let price_text = price.map(|price| omacharts_engine::grouped_decimals(price, places)).unwrap_or_default();
        if let (Some(at), Some(last)) = (price, self.market_price(&pane, &target))
            && !trading::side_allowed(kind, side, at, last)
        {
            return self.refuse_on(&target, &trading::crossed_text(&price_text, kind, side));
        }
        if let Some(why) = price.and_then(|price| self.past_limit(&pane, &target, price)) {
            return self.refuse_on(&target, &format!("Not placed · {why}"));
        }
        let size = self.size_of(&pane);
        match trading::OrderRequest::request(&target.account.id, &target.contract(), side, kind, size, price) {
            Ok(order) => self.place(target, order),
            Err(why) => self.refuse_on(&target, &format!("Not placed · {why}")),
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
        let name = self.account_label(&key);
        let decimals = crate::ui::trading_panel::decimals_of(target.tick);
        self.say(log_words::order_sent(&name, &order, decimals));
        // Drawn at once, faint, until the exchange has it working.
        let request = Request::Place(order.tag.clone());
        self.ask(request.clone(), Pending::Place { key: key.clone(), order: order.clone() });
        self.send(&target, move |session, _| session.place(&order), move |this, outcome| {
            this.trade.placing.done(&key);
            if let Some(basket) = this.heard(&request, outcome, "Not placed", &name) {
                this.trade.placed.borrow_mut().insert(basket.clone(), Stage::Pending);
                this.desk.tell();
                then(this, basket);
            }
        });
    }

    /// What the hand did to the marks on the chart `id`: the chart only
    /// says, and this is what sends.
    pub(super) fn trade_from_chart(self: &Rc<Self>, id: u32, event: crate::ui::chart::TradeEvent) {
        use crate::ui::chart::TradeEvent;
        let Some(pane) = self.pane(id) else { return };
        self.focus(id);
        match event {
            TradeEvent::Cancel(basket) => self.cancel_mark(&pane, &basket),
            TradeEvent::Flatten => self.flatten_mark(&pane, None),
            TradeEvent::FlattenHeld(contract) => self.flatten_mark(&pane, Some(contract)),
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
            Err(why) => return self.refuse(&format!("Not cancelled · {why}")),
        };
        let key = target.account.key();
        let order = self.trading.borrow().book.order(basket).filter(|order| order.account == key).cloned();
        let Some(order) = order.filter(|order| order.status.working()) else {
            return self.note_on(&target, "Not cancelled · already done");
        };
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let name = self.account_label(&key);
        self.say(log_words::note(&name, cancel_words(&order)));
        let basket = basket.to_string();
        let request = Request::Cancel(basket.clone());
        self.ask(request.clone(), Pending::Cancel);
        // Said once the book has it cancelled.
        self.trade.placed.borrow_mut().insert(basket.clone(), order.status.stage);
        self.send(&target, move |session, number| session.cancel(number, &basket), move |this, outcome| {
            this.trade.placing.done(&key);
            this.heard(&request, outcome, "Not cancelled", &name);
        });
    }

    /// A tab pulled out to `price`: an exit of `kind` for the whole of the
    /// position, linked to the other exit when there is one and the broker
    /// links orders.
    fn exit_mark(self: &Rc<Self>, pane: &ChartPane, kind: OrderKind, price: f64) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not placed · {why}")),
        };
        let key = target.account.key();
        let held = self.trading.borrow().book.position(&key, &target.contract).cloned();
        let Some(held) = held else { return self.refuse_on(&target, "Not placed · nothing held") };
        if !target.capabilities.kinds.contains(&kind) {
            return self.refuse_on(&target, &format!("Not placed · {} takes no {} orders", target.login.name, kind.key()));
        }
        if let Some(last) = self.market_price(pane, &target)
            && !trading::exit_allowed(kind, held.net, price, last)
        {
            return self.refuse_on(&target, &format!("Not placed · {}", trading::wrong_side_text(kind, price, last)));
        }
        if let Some(why) = self.past_limit(pane, &target, price) {
            return self.refuse_on(&target, &format!("Not placed · {why}"));
        }
        let side = trading::exit_side(held.net);
        // The whole of what is held, whatever the chart's size: an exit
        // closes the position.
        let Some(size) = trading::Quantity::new(held.net.unsigned_abs()) else { return };
        let order = match trading::OrderRequest::request(&target.account.id, &target.contract(), side, kind, size, Some(price)) {
            Ok(order) => order,
            Err(why) => return self.refuse_on(&target, &format!("Not placed · {why}")),
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
            return self.refuse_on(target, &format!("Not linked · {} cannot link orders", target.login.name));
        }
        let baskets = vec![basket, sibling];
        let name = self.account_label(&key);
        self.send(target, move |session, number| session.link(number, &baskets), move |this, outcome| match outcome {
            Ok(()) => this.confirm(&name, "Linked · whichever fills first cancels the other"),
            Err(why) => this.refuse_as(&name, &format!("Not linked · {why}")),
        });
    }

    /// An order's line let go at `price`: move it there. Drawn faint at its
    /// new price until the book has it there; back where it was, and said,
    /// if the broker refuses.
    fn move_mark(self: &Rc<Self>, pane: &ChartPane, basket: &str, price: f64) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not moved · {why}")),
        };
        if !target.capabilities.modify {
            return self.refuse_on(&target, &format!("Not moved · {} cannot move orders", target.login.name));
        }
        let key = target.account.key();
        let order = self.trading.borrow().book.order(basket).filter(|order| order.account == key).cloned();
        let Some(order) = order.filter(|order| order.status.working()) else {
            return self.refuse_on(&target, "Not moved · filled meanwhile");
        };
        // The market may have crossed it while the hand was letting go.
        if let Some(last) = self.market_price(pane, &target)
            && !trading::side_allowed(order.kind, order.side, price, last)
        {
            return self.refuse_on(&target, &format!("Not moved · {}", trading::wrong_side_text(order.kind, price, last)));
        }
        if let Some(why) = self.past_limit(pane, &target, price) {
            return self.refuse_on(&target, &format!("Not moved · {why}"));
        }
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let name = self.account_label(&key);
        let decimals = crate::ui::trading_panel::decimals_of(order.tick);
        self.say(log_words::note(
            &name,
            format!(
                "Move {} {} {} to {}",
                trading::order_words(order.side, order.kind),
                order.qty,
                order.contract,
                omacharts_engine::grouped_decimals(price, decimals)
            ),
        ));
        let basket = basket.to_string();
        let request = Request::Move(basket.clone());
        self.ask(request.clone(), Pending::Move(price));
        self.send(&target, move |session, number| session.modify(number, &basket, price), move |this, outcome| {
            this.trade.placing.done(&key);
            this.heard(&request, outcome, "Not moved", &name);
        });
    }

    /// The position's ×: flatten it, and cancel the exits working on it
    /// once it is flat. Faint at once, gone when the book says it is flat.
    /// `held` names a position in another month of the chart's root; the
    /// chart's own contract otherwise.
    fn flatten_mark(self: &Rc<Self>, pane: &ChartPane, held: Option<String>) {
        let target = match self.order_target(pane) {
            Ok(target) => match held {
                Some(contract) => trading::Target { contract, ..target },
                None => target,
            },
            Err(why) => return self.refuse(&format!("Not flattened · {why}")),
        };
        let key = target.account.key();
        let held = self.trading.borrow().book.position(&key, &target.contract).cloned();
        let Some(held) = held else { return self.refuse_on(&target, "Not flattened · nothing held") };
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
        let name = self.account_label(&key);
        self.say(log_words::note(&name, format!("Flatten {}", target.contract)));
        let request = Request::Flatten(key.clone(), target.contract.clone());
        self.ask(request.clone(), Pending::Flatten);
        let (contract, exchange) = (target.contract.clone(), target.exchange.clone());
        let again = target.clone();
        self.send(
            &target,
            move |session, number| session.flatten(number, &contract, &exchange),
            move |this, outcome| {
                if this.heard(&request, outcome, "Not flattened", &name).is_none() {
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
        self.market_on(&pane, side);
    }

    /// Buy or sell `pane`'s quantity at the market: the keys on the
    /// focused chart, and a chart's own Buy and Sell.
    pub(super) fn market_on(self: &Rc<Self>, pane: &ChartPane, side: Side) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not placed · {why}")),
        };
        if !target.capabilities.kinds.contains(&OrderKind::Market) {
            return self.refuse_on(&target, &format!("Not placed · {} takes no market orders", target.login.name));
        }
        let size = self.size_of(pane);
        match trading::OrderRequest::request(&target.account.id, &target.contract(), side, OrderKind::Market, size, None) {
            Ok(order) => self.place(target, order),
            Err(why) => self.refuse_on(&target, &format!("Not placed · {why}")),
        }
    }

    /// Ctrl+F: nothing left in the focused chart's contract. Every order
    /// working in it is cancelled and the position flattened, each faint
    /// until the book says so.
    pub(super) fn flat_from_keys(self: &Rc<Self>) {
        let pane = self.focused_pane();
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not flattened · {why}")),
        };
        let key = target.account.key();
        let flat = trading::flat_plan(&self.trading.borrow().book, &key, &target.contract);
        if flat.is_empty() {
            return self.note_on(&target, &format!("Already flat in {}", target.contract));
        }
        let name = self.account_label(&key);
        self.say(log_words::note(&name, format!("Flatten {} and cancel its orders", target.contract)));
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
                this.heard(&flatten, outcome, "Not flattened", &name);
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

    /// The Quantity part, "Qty 2", and Buy and Sell beside it, while the
    /// chart has an account to trade on (and, for the quantity, a template
    /// applies); nothing at all, and no room taken, otherwise. Buy and Sell
    /// that could not place an order say why instead.
    pub(super) fn write_size_of(&self, pane: &ChartPane) {
        let has_account = self.trade.shown.get()
            && self.trading.borrow().account_of(pane.account.borrow().as_deref()).is_some();
        let text = match has_account {
            false => None,
            true => {
                let instrument = pane.instrument.borrow();
                let templates = self.trade.sizes.borrow();
                instrument.as_ref().and_then(|instrument| {
                    let size = trading::sizes::size_for(&templates, instrument, pane.trading_size.borrow().as_ref())?;
                    Some(size.to_string())
                })
            }
        };
        pane.write_size(text.as_deref());
        pane.write_trade(has_account.then(|| self.order_target(pane).map(|_| ())));
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

    /// The Quantity part's menu, for the chart `id` as it is now: the
    /// quantities of its template, each a number, a mark on the chart's,
    /// then the way to edit the templates, which a right click on the part
    /// offers too.
    pub(super) fn size_menu(self: &Rc<Self>, id: u32) -> gio::Menu {
        let menu = gio::Menu::new();
        let Some(pane) = self.pane(id) else { return menu };
        let sizes = {
            let instrument = pane.instrument.borrow();
            let templates = self.trade.sizes.borrow();
            instrument
                .as_ref()
                .and_then(|instrument| trading::sizes::template_for(&templates, instrument))
                .map(|template| template.sizes.clone())
                .unwrap_or_default()
        };
        let current = self.size_of(&pane);
        let at = sizes.iter().position(|size| *size == current).map_or(0, |at| at as i32 + 1);
        if let Some(action) = pane.menu_actions.lookup_action("quantity").and_downcast::<gio::SimpleAction>() {
            action.set_state(&at.to_variant());
        }
        let choices = gio::Menu::new();
        // The number alone, as the part says it: "Qty" says what it counts.
        for (at, size) in sizes.iter().enumerate() {
            let item = gio::MenuItem::new(Some(&size.to_string()), None);
            item.set_action_and_target_value(Some("pane.quantity"), Some(&(at as i32 + 1).to_variant()));
            choices.append_item(&item);
        }
        menu.append_section(None, &choices);
        menu.append_section(None, &size_part_menu());
        menu
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
            return pane.view.set_marks(None, Vec::new(), Vec::new(), (None, None));
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
        let currency = target.account.currency.clone();
        // An exit says what it would realise where it rests: closing the
        // position held here, at what a point of it is worth.
        let spec_point = pane.instrument.borrow().as_ref().and_then(|i| i.spec()).and_then(|spec| spec.point);
        let holding = view.book.position(&key, &target.contract).filter(|held| held.net != 0);
        let exit_words = |side: Side, kind: OrderKind, qty: u32, at: f64, words: String| {
            let money = holding.and_then(|held| {
                let avg = held.avg?;
                let point = held.point.or(spec_point)?;
                trading::is_exit(side, kind, held.net).then(|| trading::exit_pnl(avg, held.net, qty, point, at))
            });
            match money {
                Some(pnl) => {
                    let mut words = words;
                    words.push_str(" · ");
                    let from = words.len();
                    words.push_str(&trading::pnl_text(pnl, currency.as_deref()));
                    let colour = if pnl < 0.0 { look.loss.clone() } else { look.profit.clone() };
                    (words, Some(crate::ui::chart::MarkPnl { from, colour }))
                }
                None => (words, None),
            }
        };
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
                // What is left of it to fill is what it would still close.
                let left = order.qty.saturating_sub(order.filled);
                let (words, pnl) = exit_words(order.side, order.kind, left, at, trading::order_pill_text(order));
                Some(crate::ui::chart::OrderMark {
                    basket: order.basket_id.clone(),
                    side: order.side,
                    kind: order.kind,
                    qty: order.qty,
                    // Not yet working at the exchange, or asked to move or go.
                    dim: order.status == Stage::Pending || moving.is_some() || cancelling,
                    price: at,
                    words,
                    price_text: price(at),
                    colour,
                    style,
                    pnl,
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
            let (words, pnl) = exit_words(order.side, order.kind, order.qty, at, words);
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
                pnl,
            });
        }
        // Positions in other months of the root, held past a roll or in
        // a month this chart is not of: drawn with their contract's name,
        // so that what is held is never out of sight.
        let root = pane.instrument.borrow().as_ref().map(|i| i.root().to_string()).unwrap_or_default();
        let held_elsewhere: Vec<crate::ui::chart::PositionMark> = view
            .book
            .positions
            .iter()
            .filter(|held| held.account == key && held.net != 0 && held.contract != target.contract)
            .filter(|held| omacharts_engine::specs::root_of(&held.contract) == root)
            .filter_map(|held| {
                let at = held.avg?;
                let (side, colour) = match held.net > 0 {
                    true => (Side::Buy, look.long.clone()),
                    false => (Side::Sell, look.short.clone()),
                };
                Some(crate::ui::chart::PositionMark {
                    contract: held.contract.clone(),
                    price: at,
                    words: format!(
                        "{} {} {}",
                        side.glyph(),
                        held.contract,
                        trading::position_text(held, places, currency.as_deref())
                    ),
                    price_text: price(at),
                    colour,
                    style: look.position,
                    pill: held.open_pnl.map(|pnl| match pnl < 0.0 {
                        true => look.loss.clone(),
                        false => look.profit.clone(),
                    }),
                    net: held.net,
                    exits: Vec::new(),
                    dim: pending.contains_key(&Request::Flatten(key.clone(), held.contract.clone())),
                    point: held.point.or(spec_point),
                    currency: currency.clone(),
                    profit: look.profit.clone(),
                    loss: look.loss.clone(),
                })
            })
            .collect();
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
                contract: target.contract.clone(),
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
                point: held.point.or(spec_point),
                currency: currency.clone(),
                profit: look.profit.clone(),
                loss: look.loss.clone(),
            })
        });
        drop(pending);
        pane.view.set_marks(position, held_elsewhere, orders, self.limits_of(pane, &target));
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
                if let Some(entry) = log_words::order_moved_on(order, *before, decimals, &view.name_of(&order.account)) {
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
    use omacharts_engine::FetchFailure;
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

    /// How many buttons the window's top-right corner holds. The trading
    /// toggle is not one of them: it has a corner of its own.
    fn corner_buttons(window: &Window) -> usize {
        let cluster = window.corner.borrow().as_ref().and_then(|corner| corner.child()).expect("a corner");
        let mut count = 0;
        let mut child = cluster.first_child();
        while let Some(widget) = child {
            count += 1;
            child = widget.next_sibling();
        }
        count
    }

    /// Without a login there is no trace of trading: no toggle over the
    /// charts, no action, no divider under the charts and no toast
    /// overlay. Adding one puts them in, and removing it takes them out,
    /// with the window open. The window's top-right corner is the same
    /// throughout: the toggle stands at the foot of the price axis, over
    /// the charts, and the panel opens under it.
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
                    && window.split.parent() == window.trade.root.borrow().clone().map(|b| b.upcast())
            };
            assert!(nothing(&window), "trading in a window with no login");

            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            let toggle = window.trade.toggle.borrow().clone().expect("a toggle");
            assert_eq!(corner_buttons(&window), bare, "the top-right corner is not where it goes");
            let foot = window.trade.foot.borrow().clone().expect("its corner");
            let docked = window.trade.docked.borrow().clone().expect("the charts and rail under it");
            assert_eq!(toggle.parent(), Some(foot.clone().upcast()));
            assert_eq!(foot.parent(), Some(docked.clone().upcast()));
            assert_eq!(foot.halign(), gtk::Align::Start, "the bottom-left corner");
            assert_eq!(foot.valign(), gtk::Align::End);
            assert_eq!(window.split.parent(), Some(docked.clone().upcast()));
            assert_eq!(docked.parent(), window.trade.root.borrow().clone().map(|b| b.upcast()));
            assert!(window.window.lookup_action("trading-panel").is_some());
            assert!(window.trade.toasts.borrow().is_some());
            gio::prelude::ActionGroupExt::activate_action(&window.window, "trading-panel", None);
            assert!(window.trading_panel_shown());
            assert!(toggle.is_active(), "the toggle says the panel is open");
            let paned = window.trade.paned.borrow().clone().expect("a divider");
            assert_eq!(docked.parent(), Some(paned.clone().upcast()), "the toggle stays over the charts and rail");
            assert_eq!(paned.parent(), window.trade.root.borrow().clone().map(|b| b.upcast()), "across the window");
            assert_eq!(paned.next_sibling(), Some(window.book_strip.clone().upcast()), "above the strip");
            assert!(store.setting_bool(SHOW_TRADING_PANEL, false), "remembered");

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.tell();
            assert!(nothing(&window), "trading left behind by the last login");
            assert_eq!(corner_buttons(&window), bare);
            assert!(!window.trading_panel_shown());
        });
    }

    /// Ctrl+Shift+A, P and O: each opens the panel on its tab, moves an
    /// open panel to it, and shuts the panel when its tab is the one in
    /// front. With no login they are not there to press.
    #[test]
    fn each_tabs_key_shows_its_tab_or_shuts_the_panel() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            for tab in PANEL_TABS {
                assert!(window.window.lookup_action(&format!("trading-{tab}")).is_none(), "{tab} with no login");
            }
            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            let press = |tab: &str| {
                gio::prelude::ActionGroupExt::activate_action(&window.window, &format!("trading-{tab}"), None)
            };
            let front = || window.trade.panel.borrow().as_ref().and_then(|panel| panel.tab());
            let toggle = window.trade.toggle.borrow().clone().expect("a toggle");

            press("orders");
            assert!(window.trading_panel_shown() && toggle.is_active());
            assert_eq!(front().as_deref(), Some("orders"));
            press("positions");
            assert!(window.trading_panel_shown(), "another tab's key shut the panel");
            assert_eq!(front().as_deref(), Some("positions"));
            press("positions");
            assert!(!window.trading_panel_shown() && !toggle.is_active(), "its own tab's key left it open");
            press("accounts");
            assert!(window.trading_panel_shown());
            assert_eq!(front().as_deref(), Some("accounts"));
            press("accounts");
            assert!(!window.trading_panel_shown());

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.tell();
            for tab in PANEL_TABS {
                assert!(window.window.lookup_action(&format!("trading-{tab}")).is_none(), "{tab} left behind");
            }
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
                ["Buy ", "Sell ", "Cancel ", "not signed in", "not tradable", "Finding "]
                    .iter()
                    .any(|word| label.contains(word))
            })
            .collect()
    }

    /// The gear is a header part, and a header without it still has a way
    /// into the chart's settings: the chart's menu, only then.
    #[test]
    fn the_chart_menu_opens_the_settings_only_when_the_gear_is_gone() {
        use crate::header::{Part, Section};
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let settings = |window: &Rc<Window>| {
                labels(window.chart_menu_model(100.0).upcast_ref()).iter().any(|(label, _)| label == "Chart settings")
            };
            assert!(!settings(&window), "offered twice: the gear is in the header");
            let mut header = crate::header::load(&store);
            header.set(Section::Right, vec![Part::Maximize, Part::Link]);
            crate::header::save(&store, &header);
            window.apply_header();
            assert!(settings(&window));
        });
    }

    /// The chart menu has no orders without a login, says why it cannot
    /// trade with one that is not signed in, offers the orders once it
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
            assert_eq!(trading_rows(&window), vec![("Finding ES's contract…".to_string(), false)]);
            session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
            let rows = trading_rows(&window);
            assert_eq!(rows.first(), Some(&("Buy 1 ESZ6 at market".to_string(), true)));
            assert_eq!(rows.get(1), Some(&("Sell 1 ESZ6 at market".to_string(), true)));
            // No price under the pointer: only the market pair, nothing greyed.
            assert_eq!(rows.len(), 2, "{rows:?}");
            let section = window.order_section(100.0).expect("the orders");
            assert_eq!(labels(section.upcast_ref()).len(), 2, "the orders, one section");
            let model = window.chart_menu_model(100.0);
            for at in 0..model.n_items() {
                let heading = model.item_attribute_value(at, "label", None);
                assert!(heading.is_none(), "a heading over a section: {heading:?}");
            }

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
            assert_eq!(pane.view.marks(), (Some("▲ +1 at 7,612.25".to_string()), Vec::new()));
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
            // An exit says what it would realise where it rests: 7.75
            // points on one ES.
            assert_eq!(pane.view.marks().1, vec!["▼ Sell Limit 1 · +$387.50".to_string()]);
            // A stop under the average loses; one for 3 closes only the 1
            // held; a buy adds to the long and says nothing of money.
            let stop = trading::OrderRequest {
                kind: OrderKind::Stop,
                qty: 3,
                price: Some(7_600.0),
                tag: trading::new_tag(),
                ..limit.clone()
            };
            let more = trading::OrderRequest {
                side: Side::Buy,
                price: Some(7_590.0),
                tag: trading::new_tag(),
                ..limit.clone()
            };
            let stop = session.place(&stop).unwrap();
            let more = session.place(&more).unwrap();
            window.desk.tell();
            let mut words = pane.view.marks().1;
            words.sort();
            assert_eq!(
                words,
                vec![
                    "▲ Buy Limit 1".to_string(),
                    "▼ Sell Limit 1 · +$387.50".to_string(),
                    "▼ Sell Stop 3 · -$612.50".to_string(),
                ]
            );
            for basket in [&basket, &stop, &more] {
                session.cancel("951", basket).unwrap();
            }
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

    /// Past the day's price limit the exchange refuses an order, however
    /// near the market it looks on the chart: a target pulled out there,
    /// or a line moved there, is not sent, and says the limit; inside the
    /// limits it goes as ever. The menu greys an order past them.
    #[test]
    fn nothing_is_sent_past_the_day_s_price_limit() {
        crate::ui::on_gtk(|| {
            use crate::ui::chart::TradeEvent;
            let (window, pane, session, _) = long_es(true);
            let log = super::super::log::tests::private(&window);
            settle(&window, || pane.view.last_price().is_some());
            let last = pane.view.last_price().expect("a last price");
            let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
            let sent = || session.book().orders.len();
            *session.limits.lock().unwrap() = (Some(off(-30.0)), Some(off(30.0)));
            window.desk.tell();
            let before = sent();

            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(40.0) });
            settle(&window, || false);
            assert_eq!(sent(), before, "a target past the high limit was sent");
            window.catch_up_log();
            let limit = omacharts_engine::grouped_decimals(off(30.0), 2);
            let said: Vec<String> = log.entries().into_iter().map(|e| e.line()).collect();
            assert!(
                said.iter().any(|line| line.ends_with(&format!("Not placed · past daily limit {limit}"))),
                "not said: {said:#?}"
            );

            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(20.0) });
            settle(&window, || sent() == before + 1);
            assert_eq!(sent(), before + 1, "a target inside the limits not sent");

            let y = pane.view.y_of(off(-40.0)).expect("the price on screen");
            let rows = labels(window.chart_menu_model(y).upcast_ref());
            let buy_limit = rows.iter().find(|(label, _)| label.starts_with("Buy Limit")).map(|(_, on)| *on);
            assert_eq!(buy_limit, None, "a buy limit past the low limit offered: {rows:?}");
        });
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
            assert_eq!(trading::never_placed_text(&lost), "Not placed · Buy Limit 1 ESZ6 never reached the broker");
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

    /// Run the main loop until `done`, or for a second at most, and say
    /// which. A session moves on its own thread, so `done` can come true
    /// just after the window last read the book: it reads it once more
    /// then, and what the test does next sees what `done` saw.
    fn settle(window: &Rc<Window>, done: impl Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        for _ in 0..200 {
            if done() {
                window.desk.tell();
                return true;
            }
            window.desk.tell();
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
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
            // What each order is, where and how faint, without the money an
            // exit also says.
            let orders = || {
                let mut orders = pane.view.mark_states().1;
                for (words, ..) in &mut orders {
                    words.truncate(words.find(" · ").unwrap_or(words.len()));
                }
                orders
            };
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

    /// On a chart fed from elsewhere, which side of the market a limit or a
    /// stop is on is the account's own market's word, not the chart's; the
    /// chart's until the session has heard a trade.
    #[test]
    fn the_menu_judges_sides_by_the_account_s_own_market() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(true);
            settle(&window, || pane.view.last_price().is_some());
            let last = pane.view.last_price().expect("a last price");
            let above = (last / 0.25).round() * 0.25 + 10.0;
            let y = pane.view.y_of(above).expect("the price on screen");
            let rows_at = |y: f64| {
                labels(window.chart_menu_model(y).upcast_ref())
                    .into_iter()
                    .filter(|(label, _)| label.starts_with("Buy ") || label.starts_with("Sell "))
                    .collect::<Vec<_>>()
            };
            let words = |rows: Vec<(String, bool)>| {
                rows.into_iter()
                    .map(|(label, on)| {
                        assert!(on, "{label} greyed rather than left out");
                        label.split(" 1 ").next().unwrap_or_default().to_string()
                    })
                    .collect::<Vec<_>>()
            };
            let offered = |words: &str| rows_at(y).into_iter().find(|(label, _)| label.starts_with(words)).map(|(_, on)| on);
            assert_eq!(words(rows_at(y)), ["Buy", "Sell", "Sell Limit", "Buy Stop"], "above the chart's market");
            let below = pane.view.y_of(above - 20.0).expect("the price on screen");
            assert_eq!(words(rows_at(below)), ["Buy", "Sell", "Buy Limit", "Sell Stop"], "below the chart's market");

            *session.market.lock().unwrap() = Some(above + 50.0);
            assert_eq!(offered("Sell Limit"), None, "below the account's market, though above the chart's");
            assert_eq!(offered("Buy Limit"), Some(true));
            assert_eq!(offered("Sell Stop"), Some(true));
        });
    }

    /// A position is marked to the account's own market as it trades: the
    /// chart's pill, the Positions tab and the account's open P&L move with
    /// each new price, whatever the broker last said, and a busy market is
    /// marked a few times a second with its last price never left out.
    #[test]
    fn open_p_and_l_moves_with_the_market() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(true);
            let pill = || pane.view.mark_states().0.map(|(words, _)| words).unwrap_or_default();
            let open = |window: &Window| window.trading.borrow().book.money(&key).and_then(|money| money.unrealized);
            assert!(pill().contains("at 7,612.25"), "{}", pill());

            *session.market.lock().unwrap() = Some(7_622.25);
            window.mark_to_market();
            assert!(pill().ends_with("+$500.00"), "{}", pill());
            assert_eq!(window.trading.borrow().book.position(&key, "ESZ6").and_then(|held| held.open_pnl), Some(500.0));
            assert_eq!(open(&window), Some(500.0));

            // Again at once: held back, then marked at the price it got to.
            *session.market.lock().unwrap() = Some(7_602.25);
            window.mark_to_market();
            *session.market.lock().unwrap() = Some(7_592.25);
            window.mark_to_market();
            assert!(pill().ends_with("+$500.00"), "marked again within {MARK_EVERY:?}");
            settle(&window, || pill().ends_with("-$1,000.00"));
            assert_eq!(open(&window), Some(-1_000.0));
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

    /// A position held in a contract the root has rolled off stays on the
    /// root's chart, named with its contract and priced from it, with a ×
    /// that flattens that contract; the chart's own position line is for
    /// the contract trading now, which holds nothing.
    #[test]
    fn a_position_held_past_the_roll_stays_on_the_chart_named_with_its_contract() {
        crate::ui::on_gtk(|| {
            use crate::ui::chart::TradeEvent;
            let (window, pane, session, _) = long_es(true);
            assert!(pane.view.held_elsewhere().is_empty());
            // The root rolls to March: the broker now trades ES as ESH7.
            session.contracts.lock().unwrap().insert("ES".into(), "ESH7".into());
            session.moved();
            settle(&window, || pane.view.mark_states().0.is_none());
            assert!(pane.view.mark_states().0.is_none(), "nothing is held in ESH7");
            let held = pane.view.held_elsewhere();
            assert_eq!(held.len(), 1, "{held:?}");
            assert!(held[0].0.starts_with("▲ ESZ6 +1 at "), "named with its contract: {}", held[0].0);
            assert!(!held[0].1);
            // Its × flattens ESZ6, not the chart's contract.
            pane.view.drop_mark(TradeEvent::FlattenHeld("ESZ6".into()));
            assert_eq!(pane.view.held_elsewhere()[0].1, true, "faint until the book says it is flat");
            settle(&window, || session.book().position("951", "ESZ6").is_none());
            assert!(session.book().position("951", "ESZ6").is_none());
            settle(&window, || pane.view.held_elsewhere().is_empty());
            assert!(pane.view.held_elsewhere().is_empty());
        });
    }

    /// An order from a chart of a dated contract goes to that contract,
    /// whatever the root trades as now; and the position it opens is the
    /// chart's own, while the root's chart shows it as held in another
    /// month.
    #[test]
    fn trading_a_specific_contract_goes_to_that_contract() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(true);
            let march = window.index.find("ESH7", None).expect("ESH7 resolves from ES");
            window.show_in(&pane, march);
            let target = window.order_target(&pane).expect("a target");
            assert_eq!((target.contract.as_str(), target.exchange.as_str()), ("ESH7", "CME"));
            window.market_from_keys(Side::Buy);
            settle(&window, || session.book().position("951", "ESH7").is_some());
            assert!(session.book().position("951", "ESH7").is_some(), "the order went to ESH7");
            settle(&window, || pane.view.mark_states().0.is_some());
            assert!(pane.view.mark_states().0.is_some(), "the chart's own position");
            assert_eq!(pane.view.held_elsewhere().len(), 1, "December, held elsewhere");
            // Back on the root, which trades as December: March is the one
            // held elsewhere.
            let es = window.index.find("ES", None).unwrap();
            window.show_in(&pane, es);
            settle(&window, || pane.view.held_elsewhere().iter().any(|(words, _)| words.contains("ESH7")));
            assert!(pane.view.mark_states().0.is_some());
            let held = pane.view.held_elsewhere();
            assert!(held.iter().any(|(words, _)| words.starts_with("▲ ESH7 +1")), "{held:?}");
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
            // What each order is, where and how faint, without the money an
            // exit also says.
            let orders = || {
                let mut orders = pane.view.mark_states().1;
                for (words, ..) in &mut orders {
                    words.truncate(words.find(" · ").unwrap_or(words.len()));
                }
                orders
            };
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
            assert!(!window.focused_pane().trade_part.property::<bool>("visible"), "Buy and Sell with no account");
        });
    }

    /// The header's Buy places the chart's quantity at the market, drawn
    /// pending until the broker fills it, as Ctrl+B does; Sell sells it.
    /// Without an account they are not there at all, which the test before
    /// this one shows.
    #[test]
    fn the_headers_buy_and_sell_trade_the_charts_quantity() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(true);
            let held = || session.book().position("951", "ESZ6").map(|held| held.net);
            let free = |window: &Rc<Window>| !window.trade.placing.busy(&key);
            settle(&window, || pane.view.last_price().is_some());
            assert!(pane.trade_part.property::<bool>("visible"), "no Buy and Sell with an account");
            assert!(pane.buy_button.is_sensitive() && pane.sell_button.is_sensitive());
            assert!(pane.trade_tooltip().is_some_and(|tip| tip.starts_with("Buy at market")), "{:?}", pane.trade_tooltip());
            assert_eq!(held(), Some(1));

            session.holds.store(true, std::sync::atomic::Ordering::Release);
            pane.buy_button.emit_clicked();
            settle(&window, || session.book().orders.iter().any(|order| order.side == Side::Buy && order.kind == OrderKind::Market));
            let buying = pane.view.mark_states().1;
            assert!(buying.iter().any(|(words, ..)| words == "Buying 1…"), "{buying:?}");
            let basket = session
                .book()
                .orders
                .iter()
                .find(|order| order.side == Side::Buy && order.kind == OrderKind::Market)
                .unwrap()
                .basket_id
                .clone();
            session.fill(&basket);
            settle(&window, || held() == Some(2) && free(&window));
            assert_eq!(held(), Some(2), "Buy bought the chart's quantity");
            session.holds.store(false, std::sync::atomic::Ordering::Release);

            pane.sell_button.emit_clicked();
            settle(&window, || held() == Some(1) && free(&window));
            assert_eq!(held(), Some(1), "Sell sold it");
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
            assert_eq!(pane.size_label_text(), "1");

            assert!(window.on_trading_key(gtk::gdk::Key::b, 0));
            settle(&window, || held() == Some(2) && free(&window));
            assert_eq!(held(), Some(2), "Ctrl+B bought one");

            // Ctrl+2, by the key's place on the number row.
            assert!(window.on_trading_key(gtk::gdk::Key::_2, 11));
            assert_eq!(pane.size_label_text(), "2");
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
            let shown = rows(&pane, &window.size_menu(pane.id));
            assert_eq!(shown.len(), 10, "one to nine contracts, then the way to edit them: {shown:?}");
            assert_eq!(shown[0], "1 ✓");
            assert_eq!(shown[9], "Edit quantities…");
            // Picked from the menu, through its action, as a click does.
            pane.menu_actions.activate_action("quantity", Some(&3i32.to_variant()));
            assert_eq!(pane.size_label_text(), "3");
            assert_eq!(rows(&pane, &window.size_menu(pane.id))[2], "3 ✓");
            let menu = size_part_menu();
            assert_eq!(labels(menu.upcast_ref()), vec![("Edit quantities…".to_string(), true)]);
            assert!(window.window.lookup_action(EDIT_SIZES).is_some());

            let mut templates = trading::sizes::load(&window.store);
            templates[1].sizes = trading::sizes::parse_sizes("3, 6").unwrap();
            trading::sizes::save(&window.store, &templates);
            window.adopt_sizes();
            assert_eq!(pane.size_label_text(), "3", "the header kept the old template");
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

    /// What a part's menu shows, a line a row: "✓" on the one in force,
    /// which is the one its action's state names, and a section's title
    /// as a plain line, such as the feed list's status.
    fn rows(pane: &ChartPane, menu: &gio::Menu) -> Vec<String> {
        let mut rows = Vec::new();
        for at in 0..menu.n_items() {
            let label = menu.item_attribute_value(at, "label", None).and_then(|label| label.get::<String>());
            match menu.item_link(at, "section") {
                Some(section) => {
                    rows.extend(label);
                    for row in 0..section.n_items() {
                        rows.push(row_words(pane, &section, row));
                    }
                }
                None => rows.push(row_words(pane, menu.upcast_ref(), at)),
            }
        }
        rows
    }

    fn row_words(pane: &ChartPane, menu: &gio::MenuModel, at: i32) -> String {
        let mut text = menu
            .item_attribute_value(at, "label", None)
            .and_then(|label| label.get::<String>())
            .unwrap_or_default();
        let action = menu.item_attribute_value(at, "action", None).and_then(|action| action.get::<String>());
        let target = menu.item_attribute_value(at, "target", None);
        let state = action
            .as_deref()
            .and_then(|action| action.strip_prefix("pane."))
            .and_then(|name| pane.menu_actions.action_state(name));
        if target.is_some() && target == state {
            text.push_str(" ✓");
        }
        text
    }

    /// What the Account part's menu shows for `pane`.
    fn accounts(window: &Rc<Window>, pane: &ChartPane) -> Vec<String> {
        rows(pane, &window.account_menu(pane.id))
    }

    /// What the Data feed part's menu shows for `pane`: the status when
    /// something is wrong, then the feeds.
    fn feeds(window: &Rc<Window>, pane: &ChartPane) -> Vec<String> {
        rows(pane, &window.feed_menu(pane.id))
    }

    /// The Data feed part as drawn: its tooltip, the dot's colour, whether
    /// the warning is beside it, and whether it opens the list.
    fn dot(pane: &ChartPane) -> Option<(String, &'static str, bool, bool)> {
        let (colour, warning, clickable) = pane.feed_dot_shown()?;
        let tooltip = pane.feed_menu.tooltip_text().map(|text| text.to_string()).unwrap_or_default();
        Some((tooltip, colour, warning, clickable))
    }

    /// No account and two feeds: no Account part, and the dot opens a flat
    /// list of the feeds that serve the chart, the order's pick the
    /// default, naming the feed and what it passed over on hover. Orange
    /// until bars come, green when they do, red with a warning when they
    /// do not.
    #[test]
    fn the_feed_dot_with_no_account_offers_the_feeds() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es.clone());
            assert_eq!(pane.account_text(), None);
            assert_eq!(dot(&pane), Some(("Yahoo Finance · Connecting…".into(), "connecting", false, true)));
            assert_eq!(
                feeds(&window, &pane),
                vec!["Yahoo Finance ✓", "Rithmic"]
            );

            // Bars came, then did not.
            let key = window.route_pane(&pane, &es).expect("routed").key();
            let native = pane.timeframe.get().native();
            window.present(&key, native, Vec::new(), None);
            assert_eq!(dot(&pane), Some(("Yahoo Finance · Connected".into(), "connected", false, true)));
            window.present(&key, native, Vec::new(), Some(FetchFailure::Unreachable));
            assert_eq!(
                dot(&pane),
                Some((
                    "Yahoo Finance · Can't connect: the data feed is not answering. Prices fill in when it does.".into(),
                    "down",
                    true,
                    true
                ))
            );
            // Red, and only then, the list says why above the feeds.
            assert_eq!(
                feeds(&window, &pane),
                vec![
                    "Yahoo Finance · Can't connect: the data feed is not answering. Prices fill in when it does.",
                    "Yahoo Finance ✓",
                    "Rithmic"
                ]
            );

            // A switch shows its choice at once, and goes orange until
            // the new feed's bars come.
            window.choose_feed(pane.id, Some("rithmic".into()));
            assert_eq!(dot(&pane), Some(("Rithmic · Connecting…".into(), "connecting", false, true)));
            assert_eq!(feeds(&window, &pane), vec!["Yahoo Finance", "Rithmic ✓"]);
            // Choosing the default stores nothing.
            window.choose_feed(pane.id, None);
            assert!(pane.feed.borrow().is_none());

            // Passed over, it says so under the feed it fell back to.
            let before = window.routes_on_screen();
            window.feeds()[0].usable.set(false);
            window.reroute(&before);
            assert_eq!(
                dot(&pane).map(|dot| dot.0),
                Some("Rithmic · Connecting…\nYahoo Finance not signed in".into())
            );
            assert_eq!(feeds(&window, &pane), ["Yahoo Finance", "Rithmic ✓"]);
            window.feeds()[0].usable.set(true);

            // A stock: Rithmic does not serve it, so there is nothing to
            // choose, and the dot opens nothing while still saying how
            // the feed stands.
            let spy = window.index.find("SPY", None).expect("SPY");
            window.show_in(&pane, spy);
            assert_eq!(dot(&pane), Some(("Yahoo Finance · Connecting…".into(), "connecting", false, false)));
            assert_eq!(feeds(&window, &pane), vec!["Yahoo Finance ✓"]);
        });
    }

    /// One account and two feeds: no Account part, since there is nothing
    /// to choose, and the dot's list has the account's own login's feed as
    /// the default, ahead of the order. A chosen feed that cannot be used
    /// is a red dot saying why.
    #[test]
    fn the_feed_dot_with_one_account_has_its_feed_as_the_default() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es.clone());
            let (_, session) = sign_in(&window, &store, &["951"]);

            assert_eq!(pane.account_text(), None, "one account: nothing to choose");
            assert_eq!(feeds(&window, &pane), vec!["Rithmic ✓", "Yahoo Finance"]);
            assert_eq!(dot(&pane), Some(("Rithmic · Connecting…".into(), "connecting", false, true)));

            // Picked from the menu, through its action, as a click does.
            pane.menu_actions.activate_action("feed", Some(&"yahoo".to_variant()));
            assert_eq!(feeds(&window, &pane), vec!["Rithmic", "Yahoo Finance ✓"]);
            // The default chosen again is no choice: nothing is kept.
            window.choose_feed(pane.id, None);
            assert_eq!(dot(&pane).map(|dot| dot.0), Some("Rithmic · Connecting…".into()));

            // A chosen feed that cannot be used: red, no warning, and why.
            window.choose_feed(pane.id, Some("yahoo".into()));
            let before = window.routes_on_screen();
            window.feeds()[0].usable.set(false);
            window.reroute(&before);
            assert_eq!(dot(&pane), Some(("Yahoo Finance · Not signed in".into(), "down", false, true)));
            window.feeds()[0].usable.set(true);
            window.choose_feed(pane.id, None);

            // The login in trouble: still nothing to choose in the header.
            session.sign_out();
            window.desk.tell();
            assert_eq!(pane.account_text(), None);

            // A stock: the account's feed does not serve it, and the
            // default moves to the order's pick.
            let spy = window.index.find("SPY", None).expect("SPY");
            window.show_in(&pane, spy);
            assert_eq!(feeds(&window, &pane), vec!["Yahoo Finance ✓"]);
            assert!(!dot(&pane).expect("a dot").3);
        });
    }

    /// Two accounts and two feeds: the Account part names the chart's,
    /// and opens the accounts with a mark on it and a line under one only
    /// for its trouble. Choosing another account forgets the chart's feed,
    /// and a chart on the default follows the default.
    #[test]
    fn choosing_an_account_forgets_the_feed() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            let (id, session) = sign_in(&window, &store, &["951", "952"]);
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 951"));
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test 951 ✓", "Rithmic Test 952"]);
            assert_eq!(feeds(&window, &pane), vec!["Rithmic ✓", "Yahoo Finance"]);

            window.choose_feed(pane.id, Some("yahoo".into()));
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 951"), "the feed is the dot's, not the account's");
            assert_eq!(dot(&pane).map(|dot| dot.0), Some("Yahoo Finance · Connecting…".into()));
            let other = format!("{id}/952");
            pane.menu_actions.activate_action("account", Some(&other.to_variant()));
            assert_eq!(pane.account.borrow().as_deref(), Some(other.as_str()));
            assert!(pane.feed.borrow().is_none(), "the feed went with the account");
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 952"));
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test 951", "Rithmic Test 952 ✓"]);
            assert_eq!(dot(&pane).map(|dot| dot.0), Some("Rithmic · Connecting…".into()));

            // Back on the default, and the default moves: the chart follows.
            window.choose_account(pane.id, None);
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 951"));
            trading::set_default_account(&store, &other);
            window.desk.tell();
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 952"));

            // The login in trouble: after the account, and under its row.
            session.sign_out();
            window.desk.tell();
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 952 signed out"));
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test 951 · signed out", "Rithmic Test 952 · signed out ✓"]);
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

    /// Two accounts and Yahoo alone: the accounts to choose from, and no
    /// dot at all, since there is nothing to choose and nothing to connect
    /// to. With one account and Yahoo alone, nothing at all.
    #[test]
    fn with_yahoo_alone_there_is_no_dot() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, false);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            assert_eq!(pane.account_text(), None, "one feed and no account: nothing");
            assert_eq!(dot(&pane), None);
            sign_in(&window, &store, &["951", "952"]);
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test 951 ✓", "Rithmic Test 952"]);
            assert_eq!(pane.account_text().as_deref(), Some("Rithmic Test 951"));
            assert_eq!(dot(&pane), None);
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
            assert!(settle(&window, || held() == Some(2) && free(&window)), "bought");
            window.catch_up_log();
            assert!(window.on_trading_key(gtk::gdk::Key::f, 0));
            assert!(settle(&window, || held().is_none() && free(&window)), "flat");
            assert!(window.on_trading_key(gtk::gdk::Key::f, 0));
            window.catch_up_log();

            let said: Vec<(omacharts_engine::log::Level, String, bool)> =
                log.entries().into_iter().map(|e| (e.level, e.line(), e.confirms)).collect();
            let account = window.account_label(&key);
            let one = |level: omacharts_engine::log::Level, line: String, confirms: bool| {
                let count = said.iter().filter(|said| **said == (level, line.clone(), confirms)).count();
                assert_eq!(count, 1, "{line:?} said {count} times in {said:#?}");
            };
            one(omacharts_engine::log::Level::Info, format!("{account} · Buy 1 ESZ6 at market"), false);
            one(omacharts_engine::log::Level::Info, format!("{account} · Filled 1 ESZ6 at 7,612.25"), true);
            one(omacharts_engine::log::Level::Info, format!("{account} · Flatten ESZ6 and cancel its orders"), false);
            one(omacharts_engine::log::Level::Info, format!("{account} · Already flat in ESZ6"), false);
            assert!(window.notice_shown().is_none(), "nothing to do is not trouble");

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
