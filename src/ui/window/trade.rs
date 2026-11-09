//! What trading puts in the window: the panel across the bottom, under
//! the charts and the rail alike, its toggle in the bottom-right corner,
//! the toasts orders report in, the chart menu's orders and the marks on
//! the charts.
//!
//! None of it exists without a trading service login. The window of somebody who
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

/// How big the toggle is, the stylesheet's 22-pixel glyph button, to
/// centre it in the corner it stands in before it is laid out.
const TOGGLE_SIZE: i32 = 22;

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
    /// The overlay over the charts, the drawing tools and the rail, where
    /// the toggle stands in the window's bottom-right corner.
    pub corners: RefCell<Option<gtk::Overlay>>,
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
    /// Everything asked of a trading service from here that its book has not
    /// answered yet: a new order, a move, a cancel, a flatten. Each is
    /// drawn as pending, faint, from the moment it is asked until the book
    /// says what became of it, or the trading service refuses it. One that is never
    /// answered stays pending: nothing here assumes it was done.
    pub pending: RefCell<HashMap<Request, Pending>>,
    /// The links asked for and not yet made: drawn as pending, joining
    /// their exits, until the trading service has answered.
    pub linking: RefCell<Vec<trading::Asked>>,
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
    /// Which side of the chart the marks' labels sit at, read when trading
    /// comes in and again whenever Preferences changes it.
    pub order_labels: Cell<crate::ui::chart::LabelSide>,
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
/// The panel's toggle, built in this one place: its look, its glyph and
/// its tooltip. Where it stands is `place_trading_toggle`'s business, and
/// what it does the window's.
fn trading_toggle() -> gtk::ToggleButton {
    let toggle = gtk::ToggleButton::new();
    toggle.set_child(Some(&panel_icon()));
    toggle.set_tooltip_text(Some(&crate::ui::shortcuts::tooltip("Trading", "win.trading-accounts")));
    toggle.add_css_class("flat");
    toggle.add_css_class("trading-toggle");
    toggle
}

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
        let book = self.marked_book(&self.trading.borrow().logins);
        self.trading.borrow_mut().book = book;
        let panel = self.trade.panel.borrow().clone();
        if let Some(panel) = panel {
            panel.redraw(&self.trading.borrow());
        }
        self.apply_marks();
    }

    /// Every live session's book, its positions marked to the trading service's
    /// market data, else, until that has heard a trade, to a chart on
    /// screen showing that very contract, from whichever feed: a dated
    /// contract's chart, or a root's on a feed that charts it as that month,
    /// whoever the account's trading service is. A chart with a trading symbol is
    /// taken for a chart of its trading contract, as each trading service signed in
    /// names it: what it trades is seldom charted anywhere, and moves with
    /// what it charts, MES with ES.
    pub(super) fn marked_book(&self, logins: &[trading::Login]) -> trading::Book {
        let charts: Vec<(omacharts_engine::Instrument, Option<f64>)> = self
            .panes
            .borrow()
            .iter()
            .flat_map(|pane| {
                let last = pane.view.last_price();
                if let Some(traded) = pane.trading_instead() {
                    return logins
                        .iter()
                        .filter_map(|login| self.desk.session(&login.id))
                        .filter_map(|session| session.contract_for(&traded).ok())
                        .map(|contract| {
                            let exchange = Some(contract.exchange).filter(|exchange| !exchange.trim().is_empty());
                            (omacharts_engine::Instrument { symbol: contract.symbol, exchange, ..traded.clone() }, last)
                        })
                        .collect();
                }
                let Some(instrument) = pane.instrument.borrow().clone() else { return Vec::new() };
                let charted = match self.route_chosen(pane, &instrument) {
                    Some(Ok(routed)) => routed.feed.provider.charted_contract(&instrument),
                    _ => None,
                };
                let instrument = match charted {
                    Some(contract) => omacharts_engine::Instrument { symbol: contract, ..instrument },
                    None if instrument.is_contract() => instrument,
                    None => return Vec::new(),
                };
                vec![(instrument, last)]
            })
            .collect();
        self.desk.marked(logins, |held| {
            trading::charted_price(held, charts.iter().map(|(instrument, last)| (instrument, *last)))
        })
    }

    /// `pane`'s chart moved: a position held in what it charts is marked
    /// again, to the chart itself while the trading service's market data has heard
    /// nothing, as often as [`Window::mark_to_market`] allows.
    pub(super) fn chart_moved(self: &Rc<Self>, pane: &ChartPane) {
        if !self.trade.shown.get() {
            return;
        }
        let Some(root) = pane.traded().map(|instrument| instrument.root().to_string()) else {
            return;
        };
        let holds = self.trading.borrow().book.positions.iter().any(|held| {
            held.net != 0 && omacharts_engine::specs::root_of(&held.contract).eq_ignore_ascii_case(&root)
        });
        if holds {
            self.mark_to_market();
        }
    }

    /// Forget what was asked of a trading service once its book has answered, and
    /// after a login's session was refreshed, what it never will.
    fn settle_pending(&self) {
        self.reconcile();
        let view = self.trading.borrow();
        self.trade.pending.borrow_mut().retain(|request, pending| !trading::answered(request, pending, &view.book));
    }

    /// Once a login's session has been refreshed — signed in, or back after
    /// a drop — its book is everything the trading service has. An order asked for
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

    /// What the trading service said to `request` when it was sent: a refusal takes
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

        // The panel's toggle: one for the window, as the panel is, so it
        // stands with the window's own furniture rather than over a chart.
        let toggle = trading_toggle();
        let this = self.clone();
        toggle.connect_toggled(move |toggle| this.show_trading_panel(toggle.is_active()));
        *self.trade.toggle.borrow_mut() = Some(toggle.clone());
        self.place_trading_toggle(&toggle);

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
        self.trade.order_labels.set(crate::ui::trading_settings::order_labels(&self.store));

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
        // The toggle out of the strip, and the strip back to what a
        // window with no login has.
        let toggle = self.trade.toggle.borrow_mut().take();
        if let Some(toggle) = toggle {
            self.unplace_trading_toggle(&toggle);
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
        self.trade.linking.borrow_mut().clear();
        for pane in self.panes.borrow().iter() {
            pane.view.set_marks(Vec::new(), super::marks::TradeRules::default());
            self.write_size_of(pane);
        }
    }

    /// Where the panel's toggle stands, the one place that says so. With
    /// one chartbook, in the window's bottom-right corner, at the time
    /// axis's height and centred under the prices, the mirror of the
    /// drawing tools' toggle in the bottom-left: laid over the charts and
    /// the rail together, so it stays where it is as the rail comes and
    /// goes. With the rail put away it is in the square where the
    /// bottom-right chart's prices meet its dates; with the rail out it is
    /// at the rail's foot, which leaves that row to it. With several, at the right end of the chartbook strip, the
    /// row under the charts once there is one, and the chart has its corner
    /// back. Called again whenever the strip is rebuilt; the toggle moves
    /// from one home to the other, the same button.
    pub(super) fn place_trading_toggle(self: &Rc<Self>, toggle: &gtk::ToggleButton) {
        self.unplace_trading_toggle(toggle);
        if self.books.borrow().len() > 1 {
            toggle.set_halign(gtk::Align::End);
            toggle.set_valign(gtk::Align::Center);
            toggle.set_margin_end(0);
            toggle.set_margin_bottom(0);
            let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            spacer.set_hexpand(true);
            spacer.add_css_class("trading-toggle-spacer");
            self.book_strip.append(&spacer);
            self.book_strip.append(toggle);
        } else {
            let ring = omacharts_engine::frame::RING_WIDTH as i32;
            let square = (crate::ui::chart::PRICE_AXIS_W as i32, crate::ui::chart::TIME_AXIS_H as i32);
            toggle.set_halign(gtk::Align::End);
            toggle.set_valign(gtk::Align::End);
            toggle.set_margin_end(ring + (square.0 - TOGGLE_SIZE) / 2);
            toggle.set_margin_bottom(ring + (square.1 - TOGGLE_SIZE) / 2);
            if let Some(corners) = self.trade.corners.borrow().as_ref() {
                corners.add_overlay(toggle);
            }
        }
        self.sync_corner_clearance();
    }

    /// Take the toggle out of whichever home `place_trading_toggle` put it
    /// in, and the spacer before it in the strip with it.
    fn unplace_trading_toggle(self: &Rc<Self>, toggle: &gtk::ToggleButton) {
        if let Some(corners) = self.trade.corners.borrow().as_ref()
            && toggle.parent().as_ref() == Some(corners.upcast_ref())
        {
            corners.remove_overlay(toggle);
        }
        if toggle.parent().as_ref() == Some(self.book_strip.upcast_ref()) {
            if let Some(spacer) = toggle.prev_sibling().filter(|w| w.has_css_class("trading-toggle-spacer")) {
                self.book_strip.remove(&spacer);
            }
            self.book_strip.remove(toggle);
        }
        self.sync_corner_clearance();
    }

    /// Is the toggle in the window's bottom-right corner, rather than in
    /// the strip or nowhere?
    pub(super) fn trading_toggle_over_charts(&self) -> bool {
        let corners = self.trade.corners.borrow().clone();
        let toggle = self.trade.toggle.borrow().clone();
        match (corners, toggle) {
            (Some(corners), Some(toggle)) => toggle.parent().as_ref() == Some(corners.upcast_ref()),
            _ => false,
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

    /// How much of the window's bottom the panel takes, with its divider,
    /// while it is open: what anything laid over the charts from the
    /// bottom has to clear.
    pub(super) fn trading_clearance(&self) -> i32 {
        if !self.trading_panel_shown() {
            return 0;
        }
        // Below what is above the divider, the charts and their strip.
        let Some(paned) = self.trade.paned.borrow().clone() else { return 0 };
        let above = paned.start_child().map_or(0, |books| books.height());
        (paned.height() - above).max(0)
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
        // are. Under the chartbook strip, at the very bottom: the tabs are
        // the charts', and go with them above the divider, while the panel
        // is the window's whichever book is in front.
        if let Some(root) = self.trade.root.borrow().as_ref() {
            let corners = self.trade.corners.borrow().clone().expect("the window's corners");
            root.remove(&corners);
            root.remove(&self.book_strip);
            let books = gtk::Box::new(gtk::Orientation::Vertical, 0);
            books.set_vexpand(true);
            books.append(&corners);
            books.append(&self.book_strip);
            paned.set_start_child(Some(&books));
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
        let corners = self.trade.corners.borrow().clone();
        if let (Some(root), Some(corners)) = (self.trade.root.borrow().as_ref(), corners) {
            if let Some(books) = paned.start_child().and_downcast::<gtk::Box>() {
                books.remove(&corners);
                books.remove(&self.book_strip);
            }
            paned.set_start_child(None::<&gtk::Widget>);
            paned.set_end_child(None::<&gtk::Widget>);
            root.remove(&paned);
            root.append(&corners);
            root.append(&self.book_strip);
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
        // Kept with the chart and not found yet: nothing is traded, and
        // above all not what the chart charts in its place.
        if let Some(waiting) = pane.trading_unresolved() {
            return Err(match self.index.complete() {
                true => format!("{waiting}, this chart's trading symbol, is not a symbol Omacharts knows"),
                false => format!("Finding {waiting}, this chart's trading symbol…"),
            });
        }
        let view = self.trading.borrow();
        let chosen = pane.account.borrow().clone();
        let instrument = pane.traded();
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
    ///
    /// A chart trading another contract than it charts is judged as
    /// [`Self::judged_by`] says.
    pub(super) fn market_price(&self, pane: &ChartPane, target: &trading::Target) -> Option<f64> {
        let chart = pane.view.last_price();
        if pane.trading_instead().is_some() {
            return match self.judged_by(pane, target) {
                super::marks::Market::Chart => chart,
                super::marks::Market::At(price) => Some(price),
                super::marks::Market::Unknown => None,
            };
        }
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

    /// What an order from `pane`, and a mark dragged on it, is judged by.
    /// A chart trading what it charts, its own last price. One trading
    /// another contract, that contract's last trade, as the trading service
    /// has it or as a chart of that very contract shows it; else, for the
    /// other size of what it charts in the same month, MES of the month an
    /// ES chart draws, the chart's own, which it trades at; else nothing,
    /// since another month is a spread away — see [`Self::unpriced`].
    pub(super) fn judged_by(&self, pane: &ChartPane, target: &trading::Target) -> super::marks::Market {
        use super::marks::Market;
        let Some(traded) = pane.trading_instead() else { return Market::Chart };
        let session = self.desk.session(&target.login.id);
        if let Some(price) = session.as_ref().and_then(|session| session.last_price(&traded)).or_else(|| self.charted_price_of(target)) {
            return Market::At(price);
        }
        let drawn = pane.instrument.borrow().clone().and_then(|charted| session?.contract_for(&charted).ok());
        match drawn.is_some_and(|drawn| omacharts_engine::specs::same_month_sizes(&drawn.symbol, &target.contract)) {
            true => Market::Chart,
            false => Market::Unknown,
        }
    }

    /// The last price of `target`'s contract on a chart in this window
    /// that charts that contract itself, trading nothing else.
    fn charted_price_of(&self, target: &trading::Target) -> Option<f64> {
        let session = self.desk.session(&target.login.id)?;
        self.panes.borrow().iter().filter(|other| other.trading_instead().is_none()).find_map(|other| {
            let charted = other.instrument.borrow().clone()?;
            let contract = session.contract_for(&charted).ok()?;
            (contract.symbol == target.contract).then(|| other.view.last_price()).flatten()
        })
    }

    /// Why a priced order from `pane` cannot be judged at all, when it
    /// cannot: it trades another contract than it charts, and nothing
    /// says what that contract last traded at.
    pub(super) fn unpriced(&self, pane: &ChartPane, target: &trading::Target) -> Option<String> {
        (pane.trading_instead().is_some() && self.market_price(pane, target).is_none())
            .then(|| format!("no price of {} to judge it by: chart {} beside this one", target.contract, target.contract))
    }

    /// The day's lowest and highest prices the exchange takes an order on
    /// `pane`'s instrument at, as far as the account's trading service has said.
    fn limits_of(&self, pane: &ChartPane, target: &trading::Target) -> (Option<f64>, Option<f64>) {
        let Some(instrument) = pane.traded() else { return (None, None) };
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
        let unpriced = self.unpriced(&pane, &target);
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
        // Its limits and stops are left out for want of a price to judge
        // them by: one greyed row says so.
        if let Some(why) = unpriced {
            let mut why = why;
            if let Some(first) = why.get(..1) {
                why = first.to_uppercase() + &why[1..];
            }
            section.append(Some(&why), Some(UNAVAILABLE));
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
        if let Some(why) = price.and_then(|_| self.unpriced(&pane, &target)) {
            return self.refuse_on(&target, &format!("Not placed · {why}"));
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

    /// [`Self::place`], and `then` with its basket once the trading service has
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
    pub(super) fn trade_from_chart(self: &Rc<Self>, id: u32, event: super::marks::TradeEvent) {
        use super::marks::TradeEvent;
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
    /// back if the trading service refuses.
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
    /// position, linked to the other exit when there is one and the trading service
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
        if let Some(why) = self.unpriced(pane, &target) {
            return self.refuse_on(&target, &format!("Not placed · {why}"));
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
        // Linked to the other exit working on the position, however the
        // trading service can: the plan knows how.
        let sibling = {
            let view = self.trading.borrow();
            trading::sibling_exit(view.book.working(&key, &target.contract), &target.contract, side, kind).cloned()
        };
        // What the working one is already paired with, which cancelling it
        // takes along: see the linking module's notes.
        let partners: Vec<trading::Order> = sibling.as_ref().map_or_else(Vec::new, |sibling| {
            let view = self.trading.borrow();
            self.desk
                .links()
                .into_iter()
                .filter(|link| link.account == key && link.holder == trading::Holder::TradingService)
                .filter(|link| link.baskets.contains(&sibling.basket_id))
                .flat_map(|link| link.baskets)
                .filter(|basket| *basket != sibling.basket_id)
                .filter_map(|basket| view.book.order(&basket).filter(|order| order.status.working()).cloned())
                .collect()
        });
        let plan = trading::Plan::exit(target.capabilities.link, order, sibling.as_ref()).paired_with(partners);
        self.run_plan(target, plan);
    }

    /// Carry out `plan` on `target`'s account: every order it places drawn
    /// faint at once, the one it cancels first faint too, the link it makes
    /// drawn pending; each solid, or gone with the trading service's reason, as the
    /// trading service answers.
    fn run_plan(self: &Rc<Self>, target: trading::Target, plan: trading::Plan) {
        let key = target.account.key();
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let name = self.account_label(&key);
        let decimals = crate::ui::trading_panel::decimals_of(target.tick);
        {
            let mut pending = self.trade.pending.borrow_mut();
            for order in plan.placing() {
                self.say(log_words::order_sent(&name, order, decimals));
                pending.insert(Request::Place(order.tag.clone()), Pending::Place { key: key.clone(), order: order.clone() });
            }
            if let Some(basket) = plan.cancelling() {
                pending.insert(Request::Cancel(basket.to_string()), Pending::Cancel);
            }
        }
        let asked = plan.asked(&key);
        if let Some(asked) = &asked {
            self.trade.linking.borrow_mut().push(asked.clone());
        }
        self.apply_marks();
        let login = target.login.id.clone();
        let cancelling = plan.cancelling().map(str::to_string);
        let tags: Vec<String> = plan.placing().iter().map(|order| order.tag.clone()).collect();
        self.send(&target, move |session, number| Ok(plan.run(session, number)), move |this, outcome| {
            this.trade.placing.done(&key);
            if let Some(asked) = &asked {
                this.trade.linking.borrow_mut().retain(|known| known != asked);
            }
            let ran = match outcome {
                Ok(ran) => ran,
                // Nothing was sent.
                Err(why) => {
                    let mut pending = this.trade.pending.borrow_mut();
                    for tag in &tags {
                        pending.remove(&Request::Place(tag.clone()));
                    }
                    if let Some(basket) = &cancelling {
                        pending.remove(&Request::Cancel(basket.clone()));
                    }
                    drop(pending);
                    this.apply_marks();
                    return this.refuse_as(&name, &format!("Not placed · {why}"));
                }
            };
            if let (Some(basket), Some(Err(Failure::Refused(_)))) = (&cancelling, &ran.cancelled) {
                this.trade.pending.borrow_mut().remove(&Request::Cancel(basket.clone()));
            }
            // A pair refused is one refusal, not two.
            let mut said = Vec::new();
            for (order, placed) in ran.placed {
                let words = placed.as_ref().err().map(|why| why.words().to_string());
                if words.as_ref().is_some_and(|words| said.contains(words)) {
                    this.trade.pending.borrow_mut().remove(&Request::Place(order.tag.clone()));
                    continue;
                }
                said.extend(words);
                if let Some(basket) = this.heard(&Request::Place(order.tag.clone()), placed, "Not placed", &name) {
                    this.trade.placed.borrow_mut().insert(basket, Stage::Pending);
                }
            }
            if let Some((order, placed)) = ran.restored {
                match placed {
                    Ok(basket) => {
                        this.trade.placed.borrow_mut().insert(basket, Stage::Pending);
                        this.say(log_words::note(&name, format!("Placed again, as it was · {}", trading::order_words(order.side, order.kind))));
                    }
                    Err(why) => this.refuse_as(&name, &format!("Not placed again · {why}")),
                }
            }
            if let Some((planned, sent)) = ran.resized {
                this.say(log_words::note(&name, format!("Placed for {sent} · {} filled meanwhile", planned - sent)));
            }
            if let Some((kind, had, sent)) = ran.again {
                this.say(log_words::note(&name, trading::again_text(kind, had, sent)));
            }
            for gone in &ran.went_with {
                let decimals = crate::ui::trading_panel::decimals_of(gone.tick);
                let price = gone.price.map(|price| omacharts_engine::grouped_decimals(price, decimals)).unwrap_or_default();
                this.say(log_words::note(&name, trading::went_with_text(gone.kind, &price)));
            }
            if let Some(why) = &ran.unsure {
                this.refuse_as(&name, &format!("Not placed again · {why}"));
            }
            match ran.link {
                Some(Ok(link)) => {
                    let holder = link.holder;
                    this.desk.linked(&login, &name, trading::Link { account: key.clone(), ..link });
                    this.confirm(&name, trading::linked_text(holder));
                }
                Some(Err(why)) => this.refuse_as(&name, &format!("Not linked · {why}")),
                None => {}
            }
            this.desk.tell();
            this.apply_marks();
        });
    }

    /// An order's line let go at `price`: move it there. Drawn faint at its
    /// new price until the book has it there; back where it was, and said,
    /// if the trading service refuses.
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
        if let Some(why) = self.unpriced(pane, &target) {
            return self.refuse_on(&target, &format!("Not moved · {why}"));
        }
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
                // One the trading service cancelled itself, or a link took with
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
        let instrument = pane.traded();
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
            let instrument = pane.traded();
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
    /// that could not place an order say why instead. The day's P&L too,
    /// once the account's trading service has said it: read from the book each time
    /// the book is, which marks it to every trade.
    pub(super) fn write_size_of(&self, pane: &ChartPane) {
        let has_account = self.trade.shown.get()
            && self.trading.borrow().account_of(pane.account.borrow().as_deref()).is_some();
        let text = match has_account {
            false => None,
            true => {
                let instrument = pane.traded();
                let templates = self.trade.sizes.borrow();
                instrument.as_ref().and_then(|instrument| {
                    let size = trading::sizes::size_for(&templates, instrument, pane.trading_size.borrow().as_ref())?;
                    Some(size.to_string())
                })
            }
        };
        pane.write_size(text.as_deref());
        pane.write_trade(has_account.then(|| self.order_target(pane).map(|_| ())));
        let day = {
            let view = self.trading.borrow();
            match self.trade.shown.get() {
                false => None,
                true => view.account_of(pane.account.borrow().as_deref()).and_then(|account| {
                    let day = view.book.money(&account.key())?.day_pnl?;
                    let sign = day.partial_cmp(&0.0)?;
                    Some((trading::pnl_text(day, account.currency.as_deref()), sign))
                }),
            }
        };
        pane.write_day_pnl(day.as_ref().map(|(text, sign)| (text.as_str(), *sign)));
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

    /// Every chart's labels to the side Preferences now says, at once.
    pub fn adopt_order_labels(self: &Rc<Self>) {
        let side = crate::ui::trading_settings::order_labels(&self.store);
        self.trade.order_labels.set(side);
        let panes = self.panes.borrow().clone();
        for pane in &panes {
            pane.view.set_mark_labels(side);
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
            let instrument = pane.traded();
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
        pane.view.set_mark_labels(self.trade.order_labels.get());
        let Ok(target) = self.order_target(pane) else {
            return pane.view.set_marks(Vec::new(), super::marks::TradeRules::default());
        };
        let key = target.account.key();
        let theme = self.theming.borrow().theme();
        // The theme's trading colours, with only the chart's up and down
        // switch turning profit and loss round; never its candles' colours.
        let look = omacharts_engine::theme_trading(&theme, pane.swapped.get());
        let places = crate::ui::trading_panel::decimals_of(Self::tick_of(pane, &target));
        let price = |price: f64| omacharts_engine::grouped_decimals(price, places);
        let view = self.trading.borrow();
        let pending = self.trade.pending.borrow();
        let currency = target.account.currency.clone();
        // An exit says what it would realise where it rests: closing the
        // position held here, at what a point of it is worth.
        let spec_point = pane.traded().and_then(|i| i.spec()).and_then(|spec| spec.point);
        let holding = view.book.position(&key, &target.contract).filter(|held| held.net != 0);
        let net = holding.map_or(0, |held| held.net);
        let exit_pnl = |side: Side, kind: OrderKind, qty: u32, at: f64| {
            let held = holding?;
            let avg = held.avg?;
            let point = held.point.or(spec_point)?;
            trading::is_exit(side, kind, held.net).then(|| trading::exit_pnl(avg, held.net, qty, point, at))
        };
        // Green and red are money's: a mark with money to say wears its
        // sign, and one without, its side, in the theme's own two.
        let money_colour = |pnl: f64| if pnl < 0.0 { look.loss.clone() } else { look.profit.clone() };
        let side_colour = |side: Side| match side {
            Side::Buy => look.buy.clone(),
            Side::Sell => look.sell.clone(),
        };
        let held_colour = |net: i32, pnl: Option<f64>| match (pnl, net > 0) {
            (Some(pnl), _) => money_colour(pnl),
            (None, true) => look.long.clone(),
            (None, false) => look.short.clone(),
        };
        // An exit's P&L as its body and its colour; an entry's side alone.
        let order_look = |side: Side, kind: OrderKind, qty: u32, at: f64| match exit_pnl(side, kind, qty, at) {
            Some(pnl) => (Some(trading::pnl_text(pnl, currency.as_deref())), money_colour(pnl)),
            None => (None, side_colour(side)),
        };
        // Linked with the other exit: solid once the trading service has made it,
        // pending until then.
        let links = self.desk.links();
        let asked = self.trade.linking.borrow();
        let link_of = |basket: &str, tag: Option<&str>| {
            let made = links.iter().find(|link| link.account == key && link.baskets.iter().any(|known| known == basket));
            match made {
                Some(link) => Some(super::marks::LinkMark { holder: link.holder, pending: false }),
                None => asked
                    .iter()
                    .find(|asked| asked.account == key && asked.has(basket, tag))
                    .map(|asked| super::marks::LinkMark { holder: asked.holder, pending: true }),
            }
        };
        let mut orders: Vec<super::marks::OrderMark> = view
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
                // What is left of it to fill is what it would still close.
                let left = order.qty.saturating_sub(order.filled);
                let (body, colour) = order_look(order.side, order.kind, left, at);
                Some(super::marks::OrderMark {
                    basket: order.basket_id.clone(),
                    side: order.side,
                    kind: order.kind,
                    qty: order.qty,
                    // Not yet working at the exchange, or asked to move or go.
                    dim: order.status == Stage::Pending || moving.is_some() || cancelling,
                    price: at,
                    tag: trading::order_tag(order, net),
                    body,
                    price_text: price(at),
                    colour,
                    style,
                    link: link_of(&order.basket_id, order.tag.as_deref()),
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
            let link = link_of("", Some(tag.as_str()));
            // A market order has no tag, only its body beside the last price.
            let (at, tag, (body, colour), style) = match (order.kind, order.price) {
                (OrderKind::Market, _) => match last {
                    Some(last) => {
                        let body = Some(trading::buying_text(order.side, order.qty));
                        (last, String::new(), (body, side_colour(order.side)), look.position)
                    }
                    None => continue,
                },
                (_, Some(_)) if known => continue,
                (kind, Some(at)) => (
                    at,
                    trading::mark_tag(kind, order.side, trading::is_exit(order.side, kind, net), &order.qty.to_string()),
                    order_look(order.side, kind, order.qty, at),
                    if kind == OrderKind::Stop { look.stop } else { look.limit },
                ),
                (_, None) => continue,
            };
            orders.push(super::marks::OrderMark {
                basket: String::new(),
                side: order.side,
                kind: order.kind,
                qty: order.qty,
                dim: true,
                price: at,
                link,
                tag,
                body,
                price_text: price(at),
                colour,
                style,
            });
        }
        // Positions in other months of the root, held past a roll or in
        // a month this chart is not of: drawn with their contract's name,
        // so that what is held is never out of sight.
        let root = pane.traded().map(|i| i.root().to_string()).unwrap_or_default();
        let held_elsewhere: Vec<super::marks::PositionMark> = view
            .book
            .positions
            .iter()
            .filter(|held| held.account == key && held.net != 0 && held.contract != target.contract)
            .filter(|held| omacharts_engine::specs::root_of(&held.contract) == root)
            .filter_map(|held| {
                let at = held.avg?;
                Some(super::marks::PositionMark {
                    contract: held.contract.clone(),
                    price: at,
                    tag: format!("{} {}", held.contract, trading::held_tag(held.net)),
                    body: held.open_pnl.map(|pnl| trading::pnl_text(pnl, currency.as_deref())),
                    price_text: price(at),
                    colour: held_colour(held.net, held.open_pnl),
                    style: look.position,
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
            // A tab for each exit the trading service takes and the position still
            // has room for, in the look of the order it would make: a
            // target in the profit colour, a stop in the loss colour.
            let exits = [(OrderKind::Limit, look.limit, &look.profit), (OrderKind::Stop, look.stop, &look.loss)]
                .into_iter()
                .filter(|(kind, _, _)| target.capabilities.kinds.contains(kind))
                .filter(|(kind, _, _)| trading::exit_offered(view.book.working(&key, &target.contract), held.net, *kind))
                .map(|(kind, style, colour)| super::marks::ExitTab { kind, colour: colour.clone(), style })
                .collect();
            Some(super::marks::PositionMark {
                contract: target.contract.clone(),
                price: at,
                tag: trading::held_tag(held.net),
                body: held.open_pnl.map(|pnl| trading::pnl_text(pnl, currency.as_deref())),
                price_text: price(at),
                colour: held_colour(held.net, held.open_pnl),
                style: look.position,
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
        drop(asked);
        let rules = super::marks::TradeRules::new(position.as_ref(), &orders, self.limits_of(pane, &target))
            .judged_by(self.judged_by(pane, &target));
        pane.view.set_marks(super::marks::price_marks(position.as_ref(), &held_elsewhere, &orders), rules);
    }

    /// Say each step of an order placed or cancelled here, once, when the
    /// trading service has said so: working, filled and cancelled in a toast too,
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
    use super::super::marks::MarksSeen as _;
    use omacharts_engine::FetchFailure;
    use crate::store::Store;
    use crate::trading::fake::Kept;
    use crate::trading::Session as _;

    /// A real window on a store in memory, charting from the synthetic
    /// feed: nothing reaches a network, a keyring or the session bus. With
    /// trading switched on, which is what these tests are about.
    pub(crate) fn window(store: &Rc<Store>) -> Rc<Window> {
        crate::features::set(store, crate::features::Feature::Trading, true);
        bare_window(store)
    }

    /// [`window`] with the feature flags as `store` has them.
    pub(crate) fn bare_window(store: &Rc<Store>) -> Rc<Window> {
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        let feed: Arc<dyn omacharts_engine::Provider> = Arc::new(omacharts_engine::providers::Synthetic::new(None));
        Window::build_with(&app, store.clone(), vec![feed])
    }

    pub(crate) fn typed() -> trading::Typed {
        trading::Typed {
            service: "rithmic".into(),
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

    /// Without a login there is no trace of trading: no toggle, no action,
    /// no divider under the charts and no toast overlay, and the chart's
    /// corner under its prices is its own. Adding one puts them in, and
    /// removing it takes them out, with the window open. The window's
    /// top-right corner and its chartbook strip are the same throughout:
    /// the toggle stands in the bottom-right corner of the charts, and the
    /// panel opens under them.
    #[test]
    fn trading_is_nowhere_without_a_login_and_comes_and_goes_with_one() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let bare = corner_buttons(&window);
            let corners = window.trade.corners.borrow().clone().expect("the window's corners");
            assert_eq!(window.split.parent(), Some(corners.clone().upcast()), "the corners over the charts and rail");
            let nothing = |window: &Window| {
                window.trade.toggle.borrow().is_none()
                    && window.window.lookup_action("trading-panel").is_none()
                    && window.trade.paned.borrow().is_none()
                    && window.trade.toasts.borrow().is_none()
                    && corners.parent() == window.trade.root.borrow().clone().map(|b| b.upcast())
                    && corners.next_sibling() == Some(window.book_strip.clone().upcast())
                    && window.watchlist.borrow().as_ref().is_some_and(|rail| rail.foot() == 0)
                    && window.book_strip.next_sibling().is_none()
                    && !window.book_strip.property::<bool>("visible")
                    && window.panes.borrow().iter().all(|pane| !pane.view.axis_corner_taken())
            };
            assert!(nothing(&window), "trading in a window with no login");

            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            let toggle = window.trade.toggle.borrow().clone().expect("a toggle");
            assert_eq!(corner_buttons(&window), bare, "the top-right corner is not where it goes");
            assert_eq!(toggle.parent(), Some(corners.clone().upcast()), "in the window's corner");
            assert_eq!((toggle.halign(), toggle.valign()), (gtk::Align::End, gtk::Align::End), "bottom-right");
            assert!(!window.book_strip.property::<bool>("visible"), "a strip with one chartbook");
            // With the rail out it stands at the rail's foot, which the rail
            // leaves to it; put away, over the chart, which leaves its
            // corner to it. The same place in the window either way.
            assert!(window.shows_sidebar());
            let rail = window.watchlist.borrow().clone().expect("the rail");
            let place = |toggle: &gtk::ToggleButton| (toggle.parent(), toggle.margin_end(), toggle.margin_bottom());
            let railed = place(&toggle);
            assert_eq!(rail.foot(), omacharts_engine::frame::RING_WIDTH as i32 + crate::ui::chart::TIME_AXIS_H as i32, "the rail's foot left to it");
            assert!(window.panes.borrow().iter().all(|pane| !pane.view.axis_corner_taken()), "not over the chart");
            window.toggle_watchlist();
            assert_eq!(place(&toggle), railed, "the rail put away moved the toggle");
            assert!(
                window.panes.borrow().iter().all(|pane| pane.view.axis_corner_taken()),
                "the chart under it left the corner to it"
            );
            window.toggle_watchlist();
            assert_eq!(place(&toggle), railed, "the rail brought back moved the toggle");
            assert_eq!(corners.parent(), window.trade.root.borrow().clone().map(|b| b.upcast()));
            assert!(window.window.lookup_action("trading-panel").is_some());
            assert!(window.trade.toasts.borrow().is_some());
            gio::prelude::ActionGroupExt::activate_action(&window.window, "trading-panel", None);
            assert!(window.trading_panel_shown());
            assert!(toggle.is_active(), "the toggle says the panel is open");
            let paned = window.trade.paned.borrow().clone().expect("a divider");
            let above = paned.start_child().expect("the charts' side of the divider");
            assert_eq!(corners.parent(), Some(above.clone()), "the charts and rail above it");
            assert_eq!(corners.next_sibling(), Some(window.book_strip.clone().upcast()), "the tabs under the charts");
            assert_eq!(window.book_strip.parent(), Some(above), "the tabs above the divider too");
            assert_eq!(paned.end_child(), window.trade.panel.borrow().as_ref().map(|panel| panel.root.clone().upcast()),
                "the panel under the divider"
            );
            assert_eq!(paned.parent(), window.trade.root.borrow().clone().map(|b| b.upcast()), "across the window");
            assert_eq!(paned.next_sibling(), None, "the panel at the very bottom");
            assert!(store.setting_bool(SHOW_TRADING_PANEL, false), "remembered");

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.tell();
            assert!(nothing(&window), "trading left behind by the last login");
            assert_eq!(corner_buttons(&window), bare);
            assert!(!window.trading_panel_shown());
        });
    }

    /// Every trading setting is in Preferences with no login, ready for
    /// the first: the Theme page's Trading colours and lines on a theme of
    /// one's own, the header's P&L, Buy Sell and Quantity in its Right, and
    /// the Trading page's own groups.
    #[test]
    fn preferences_show_every_trading_setting_without_a_login() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            assert!(!window.trading.borrow().trades());
            {
                let mut theming = window.theming.borrow_mut();
                let mine = theming.theme().duplicate("custom-mine", "Mine");
                store.save_theme(&mine);
                theming.reload_custom(&store);
                theming.select_theme("custom-mine", &store);
            }
            window.open_preferences_at(None);
            let dialog = window.window.visible_dialog().expect("the preferences");
            let widgets = |root: gtk::Widget| {
                let (mut found, mut stack) = (Vec::new(), vec![root]);
                while let Some(widget) = stack.pop() {
                    let mut child = widget.first_child();
                    while let Some(next) = child {
                        child = next.next_sibling();
                        stack.push(next);
                    }
                    found.push(widget);
                }
                found
            };
            let sidebar = widgets(dialog.clone().upcast())
                .into_iter()
                .find_map(|widget| widget.downcast::<gtk::ListBox>().ok().filter(|list| list.has_css_class("navigation-sidebar")))
                .expect("the sidebar");
            // Each section is put in the dialog when it is picked.
            let (mut groups, mut rows) = (Vec::new(), Vec::new());
            let mut index = 0;
            while let Some(section) = sidebar.row_at_index(index) {
                sidebar.select_row(Some(&section));
                for widget in widgets(dialog.clone().upcast()) {
                    if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
                        groups.push(group.title().to_string());
                    }
                    if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>() {
                        rows.push(row.title().to_string());
                    }
                }
                index += 1;
            }
            for group in ["Trading", "Right", "Order entry", "Chart"] {
                assert!(groups.iter().any(|title| title == group), "no {group} group in {groups:?}");
            }
            for row in ["Buy", "Sell", "Long", "Short", "Profit", "Loss", "P&L", "Buy Sell", "Quantity", "Order labels"] {
                assert!(rows.iter().any(|title| title == row), "no {row} row in {rows:?}");
            }
            dialog.force_close();
        });
    }

    /// The sidebar's sections, every group's title and every row's title in
    /// the Preferences `window` opens.
    fn preferences_words(window: &Rc<Window>) -> (Vec<String>, Vec<String>, Vec<String>) {
        window.open_preferences_at(None);
        let dialog = window.window.visible_dialog().expect("the preferences");
        let widgets = |root: gtk::Widget| {
            let (mut found, mut stack) = (Vec::new(), vec![root]);
            while let Some(widget) = stack.pop() {
                let mut child = widget.first_child();
                while let Some(next) = child {
                    child = next.next_sibling();
                    stack.push(next);
                }
                found.push(widget);
            }
            found
        };
        let sidebar = widgets(dialog.clone().upcast())
            .into_iter()
            .find_map(|widget| widget.downcast::<gtk::ListBox>().ok().filter(|list| list.has_css_class("navigation-sidebar")))
            .expect("the sidebar");
        let (mut sections, mut groups, mut rows) = (Vec::new(), Vec::new(), Vec::new());
        let mut index = 0;
        while let Some(section) = sidebar.row_at_index(index) {
            for widget in widgets(section.clone().upcast()) {
                if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                    sections.push(label.label().to_string());
                }
            }
            sidebar.select_row(Some(&section));
            for widget in widgets(dialog.clone().upcast()) {
                if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
                    groups.push(group.title().to_string());
                }
                if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>() {
                    rows.push(row.title().to_string());
                }
            }
            index += 1;
        }
        dialog.force_close();
        (sections, groups, rows)
    }

    /// Trading switched off, as it ships, is the app before trading even
    /// with a login saved: no toggle, no panel, no keys, no orders, no
    /// sign-in, and no Trading page, colours or header parts in
    /// Preferences. Switched on with the window open, it comes in place
    /// and the saved login signs in; switched off again, it goes, and the
    /// charts are as they were throughout.
    #[test]
    fn trading_switched_off_is_nowhere_and_comes_and_goes_in_place() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            let window = bare_window(&store);
            {
                let mut theming = window.theming.borrow_mut();
                let mine = theming.theme().duplicate("custom-mine", "Mine");
                store.save_theme(&mine);
                theming.reload_custom(&store);
                theming.select_theme("custom-mine", &store);
            }
            let off = |window: &Rc<Window>| {
                !window.trading.borrow().trades()
                    && window.trade.toggle.borrow().is_none()
                    && window.trade.panel.borrow().is_none()
                    && window.window.lookup_action("trading-panel").is_none()
                    && window.trade.toasts.borrow().is_none()
                    && window.desk.sessions_open() == 0
                    && !window.desk.opening(&id)
            };
            assert!(off(&window), "trading built with the flag off");
            assert!(window.order_section(10.0).is_none(), "orders in the chart menu");
            let (sections, groups, rows) = preferences_words(&window);
            assert_eq!(sections, ["General", "Charts", "Theme", "Data"]);
            for group in ["Trading", "Trading services", "Order entry"] {
                assert!(!groups.iter().any(|title| title == group), "a {group} group in {groups:?}");
            }
            for row in ["Profit", "Loss", "P&L", "Buy Sell", "Quantity", "Account", "Order labels"] {
                assert!(!rows.iter().any(|title| title == row), "a {row} row in {rows:?}");
            }

            let charts = |window: &Rc<Window>| {
                window.panes.borrow().iter().map(|pane| (pane.id, pane.instrument.borrow().clone())).collect::<Vec<_>>()
            };
            let before = charts(&window);
            let focused = window.focused_pane().id;
            crate::features::set(&store, crate::features::Feature::Trading, true);
            window.adopt_features();
            assert!(window.trading.borrow().trades());
            assert!(window.trade.toggle.borrow().is_some(), "no toggle switched on");
            assert!(window.window.lookup_action("trading-panel").is_some());
            assert!(window.desk.opening(&id) || window.desk.live(&id), "the saved login is not signing in");
            assert_eq!(charts(&window), before, "the charts were rebuilt");
            assert_eq!(window.focused_pane().id, focused);
            let (sections, _, _) = preferences_words(&window);
            assert_eq!(sections.last().map(String::as_str), Some("Trading"));

            crate::features::set(&store, crate::features::Feature::Trading, false);
            window.adopt_features();
            assert!(off(&window), "trading left behind switched off");
            assert_eq!(charts(&window), before, "the charts were rebuilt");
            assert_eq!(window.focused_pane().id, focused);
        });
    }

    /// The panel is the window's, not a chartbook's: open, it stays open
    /// across a switch to another book and back; shut, it stays shut. The
    /// one toggle goes where the strip says: in the bottom-right chart's
    /// corner with one book, at the strip's right end with several, the
    /// chart's corner its own again, and back when the strip goes. The
    /// strip runs under the charts, and the panel under the strip.
    #[test]
    fn the_panel_is_the_window_s_across_chartbooks() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let kept = Kept::default();
            trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            let toggle = window.trade.toggle.borrow().clone().expect("a toggle");
            let charts = window.trade.corners.borrow().clone().expect("the window's corners");
            assert_eq!(toggle.parent(), Some(charts.clone().upcast()), "one book: over the charts");
            gio::prelude::ActionGroupExt::activate_action(&window.window, "trading-panel", None);
            assert!(window.trading_panel_shown());

            window.duplicate_chartbook();
            assert_eq!(window.active.get(), 1);
            assert!(window.trading_panel_shown() && toggle.is_active(), "another book shut the panel");
            // Two books: the tabs show, under the charts and over the panel,
            // and the toggle stands at their right end.
            assert!(window.book_strip.property::<bool>("visible"), "a strip with two chartbooks");
            let paned = window.trade.paned.borrow().clone().expect("a divider");
            assert_eq!(window.book_strip.parent(), paned.start_child(), "the tabs above the divider");
            assert_eq!(window.book_strip.prev_sibling(), Some(charts.clone().upcast()), "under the charts");
            let in_strip = |window: &Window| {
                toggle.parent() == Some(window.book_strip.clone().upcast())
                    && window.book_strip.last_child() == Some(toggle.clone().upcast())
                    && window.panes.borrow().iter().all(|pane| !pane.view.axis_corner_taken())
            };
            assert!(in_strip(&window), "two books: the toggle at the strip's end, the corner the chart's");
            assert!(window.book_tab(1).is_some_and(|tab| tab.has_css_class("chartbook-tab")), "the tabs first");
            window.activate_book(0);
            assert!(window.trading_panel_shown() && toggle.is_active(), "switching back shut the panel");
            assert!(in_strip(&window), "a rebuilt strip lost the toggle");

            gio::prelude::ActionGroupExt::activate_action(&window.window, "trading-panel", None);
            assert!(!window.trading_panel_shown());
            window.activate_book(1);
            assert!(!window.trading_panel_shown() && !toggle.is_active(), "switching opened the panel");
            assert!(in_strip(&window));

            // Back to one book: the strip goes, and the toggle to the corner.
            window.remove_chartbook(1);
            assert!(!window.book_strip.property::<bool>("visible"));
            assert_eq!(toggle.parent(), Some(charts.upcast()), "one book again: over the charts");
            assert_eq!(window.book_strip.first_child(), None, "the strip left empty");
            let footed = window.watchlist.borrow().as_ref().is_some_and(|rail| rail.foot() > 0);
            let cornered = window.panes.borrow().iter().any(|pane| pane.view.axis_corner_taken());
            assert!(footed, "the rail's foot left to it");
            assert_eq!(cornered, !window.shows_sidebar(), "the chart's corner left to it while the rail is put away");
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
                ["Buy ", "Sell ", "Cancel ", "not signed in", "no accounts", "not tradable", "Finding "]
                    .iter()
                    .any(|word| label.contains(word))
            })
            .collect()
    }

    /// A login that signed in and was listed no accounts is not said to be
    /// signed out: the menu says it has no accounts, and so does an order
    /// asked for anyway.
    #[test]
    fn a_login_signed_in_with_no_accounts_says_so() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            let session = Arc::new(trading::fake::FakeSession::new(Vec::new(), Box::new(|| {})));
            assert!(window.desk.begin(&id));
            window.desk.opened(&id, Ok(session.clone()));
            window.desk.note(&store);
            window.desk.tell();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            let pane = window.focused_pane();
            window.show_in(&pane, es);
            session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
            assert_eq!(trading_rows(&window), vec![("Rithmic Test has no accounts".to_string(), false)]);
            assert_eq!(window.order_target(&pane).unwrap_err(), "Rithmic Test has no accounts");
        });
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
            assert_eq!(pane.view.marks(), (Some("▲ 1".to_string()), Vec::new()), "its size until the P&L has come");
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
            assert_eq!(pane.view.marks().1, vec!["L 1 · +$387.50".to_string()]);
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
                vec!["L 1 · +$387.50".to_string(), "STP 3 · -$612.50".to_string(), "▲ L 1".to_string()]
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
    /// links orders as `linking`; long one at 7,612.25.
    fn long_es(linking: trading::Linking) -> (Rc<Window>, Rc<ChartPane>, Arc<trading::fake::FakeSession>, String) {
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
        session.linking = linking;
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
            use super::super::marks::TradeEvent;
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
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

    /// After a drop the refreshed book is the trading service's word. An order sent
    /// before it that the book has never heard of did not get there: it is
    /// no longer drawn as on its way, and the account takes orders again.
    /// One the book has is left to be answered as ever.
    #[test]
    fn an_order_the_refreshed_book_never_heard_of_never_reached_the_service() {
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

            // One the trading service has, still pending at the exchange; one it never got.
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
            assert!(pending.contains_key(&Request::Place(reached.tag.clone())), "the trading service has it");
            assert!(!window.trade.placing.busy(&key), "the account still waiting on an answer");
            assert_eq!(trading::never_placed_text(&lost), "Not placed · Buy Limit 1 ESZ6 never reached the trading service");
        });
    }

    /// A saved login signs in by itself once the window's first frame is on
    /// screen, never before it, and its account's money and orders fill
    /// the panel when it is back.
    #[test]
    fn a_saved_login_signs_in_after_the_first_frame() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let saved = trading::Typed { service: "fake".into(), values: Vec::new(), ..typed() };
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
    /// back as it was when the trading service refuses, and not sent at all while
    /// another request to the account is on its way. The position's ×
    /// flattens it.
    #[test]
    fn the_marks_cross_cancels_an_order_and_flattens_the_position() {
        crate::ui::on_gtk(|| {
            use super::super::marks::TradeEvent;
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
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
            assert_eq!(orders(), vec![("L 1".to_string(), 7_640.0, false)]);

            // Refused: back as it was, and said.
            *session.refuse.lock().unwrap() = Some("too late".into());
            pane.view.drop_mark(TradeEvent::Cancel(basket.clone()));
            assert!(orders()[0].2, "not faint at once");
            settle(&window, || !orders().first().is_some_and(|order| order.2));
            assert_eq!(orders(), vec![("L 1".to_string(), 7_640.0, false)], "not back after a refusal");

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
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
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
    /// each new price, whatever the trading service last said, and a busy market is
    /// marked a few times a second with its last price never left out.
    #[test]
    fn open_p_and_l_moves_with_the_market() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            let pill = || pane.view.mark_states().0.map(|(words, _)| words).unwrap_or_default();
            let open = |window: &Window| window.trading.borrow().book.money(&key).and_then(|money| money.unrealized);
            assert_eq!(pill(), "▲ 1", "its size alone until the P&L has come");

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
            use super::super::marks::TradeEvent;
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
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

    /// L and STP pulled out of the position place a target and a stop for
    /// all of it, each tab going once its exit is working; the second is
    /// linked to the first, however the trading service can: after the fact, as one
    /// pair once the target is cancelled, or by the app. One on the wrong
    /// side of the market is not placed. Flattening cancels the exits left
    /// behind.
    #[test]
    fn exits_pulled_out_of_the_position_are_placed_and_linked() {
        crate::ui::on_gtk(|| {
            use super::super::marks::TradeEvent;
            use trading::{Holder, Linking};
            for linking in [Linking::AfterTheFact, Linking::AtSubmission, Linking::None] {
                let (window, pane, session, key) = long_es(linking);
                settle(&window, || pane.view.last_price().is_some());
                let last = pane.view.last_price().expect("a last price");
                let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
                assert_eq!(pane.view.exit_tabs(), vec![OrderKind::Limit, OrderKind::Stop]);
                let working = || session.book().orders.iter().filter(|o| o.status.working()).cloned().collect::<Vec<_>>();

                pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(20.0) });
                settle(&window, || false);
                assert!(working().is_empty(), "a long's stop placed above the market");

                pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(40.0) });
                // Answered too, so the account is free for the stop.
                settle(&window, || pane.view.exit_tabs() == vec![OrderKind::Stop] && !window.trade.placing.busy(&key));
                let target = session.book().orders[0].clone();
                assert_eq!((target.side, target.kind, target.qty, target.price), (Side::Sell, OrderKind::Limit, 1, Some(off(40.0))));
                assert_eq!(pane.view.exit_tabs(), vec![OrderKind::Stop], "the L tab stayed");

                pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(-40.0) });
                // The link is drawn pending at once, before the trading service answers.
                let pending = pane.view.mark_links().iter().flatten().any(|link| link.pending);
                assert!(pending, "{linking:?}: the link not drawn pending at once");
                settle(&window, || pane.view.exit_tabs().is_empty() && !window.trade.placing.busy(&key));
                let both = working();
                assert_eq!(both.len(), 2, "{linking:?}: {both:?}");
                let stop = both.iter().find(|o| o.kind == OrderKind::Stop).expect("the stop").clone();
                let now_target = both.iter().find(|o| o.kind == OrderKind::Limit).expect("the target").clone();
                assert_eq!((stop.side, stop.price), (Side::Sell, Some(off(-40.0))));
                assert_eq!(now_target.price, Some(off(40.0)), "{linking:?}: the target where it was");
                let links = window.desk.links();
                assert_eq!(links.len(), 1, "{linking:?}: {links:?}");
                let held = match linking {
                    Linking::AfterTheFact => {
                        assert_eq!(*session.links.lock().unwrap(), vec![vec![stop.basket_id.clone(), target.basket_id.clone()]]);
                        Holder::TradingService
                    }
                    Linking::AtSubmission => {
                        // The target cancelled first, then the two as one pair.
                        assert_eq!(session.pairs.load(std::sync::atomic::Ordering::Acquire), 1);
                        assert_ne!(now_target.basket_id, target.basket_id, "the target placed again with the stop");
                        Holder::TradingService
                    }
                    Linking::None => {
                        assert!(session.links.lock().unwrap().is_empty(), "linked by a trading service that cannot");
                        Holder::App
                    }
                };
                assert_eq!(links[0].holder, held, "{linking:?}");
                // Drawn solid once made, in its holder's look.
                let drawn: Vec<_> = pane.view.mark_links().into_iter().flatten().collect();
                assert_eq!(drawn, vec![super::super::marks::LinkMark { holder: held, pending: false }; 2], "{linking:?}");

                pane.view.drop_mark(TradeEvent::Flatten);
                settle(&window, || pane.view.mark_states().0.is_none() && working().is_empty());
                assert!(session.book().position("951", "ESZ6").is_none());
                assert!(working().is_empty(), "an exit outlived the position it was for");
                assert!(pane.view.mark_states().1.is_empty());
            }
        });
    }

    /// A target added to a working stop where the service links only at
    /// submission: at once, the stop is drawn faint while it is replaced
    /// and the pair faint until it is taken; then both solid, the stop
    /// where it was.
    #[test]
    fn a_stop_replaced_to_pair_it_with_a_target_is_drawn_pending_until_the_pair_is_taken() {
        crate::ui::on_gtk(|| {
            use super::super::marks::TradeEvent;
            let (window, pane, session, key) = long_es(trading::Linking::AtSubmission);
            settle(&window, || pane.view.last_price().is_some());
            let last = pane.view.last_price().expect("a last price");
            let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(-40.0) });
            settle(&window, || pane.view.exit_tabs() == vec![OrderKind::Limit] && !window.trade.placing.busy(&key));
            let orders = || {
                let mut orders: Vec<(f64, bool)> = pane.view.mark_states().1.into_iter().map(|(_, price, dim)| (price, dim)).collect();
                orders.sort_by(|a, b| a.0.total_cmp(&b.0));
                orders
            };
            assert_eq!(orders(), vec![(off(-40.0), false)]);

            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(40.0) });
            let now = orders();
            assert!(now.iter().all(|(_, dim)| *dim), "not all drawn pending at once: {now:?}");
            assert!(now.iter().any(|(price, _)| *price == off(-40.0)) && now.iter().any(|(price, _)| *price == off(40.0)), "{now:?}");

            settle(&window, || !window.trade.placing.busy(&key) && orders().iter().all(|(_, dim)| !dim));
            assert_eq!(orders(), vec![(off(-40.0), false), (off(40.0), false)], "one stop, where it was, and the target");
            assert_eq!(session.pairs.load(std::sync::atomic::Ordering::Acquire), 1);
            let heard = session.heard.lock().unwrap().clone();
            assert_eq!(heard[heard.len() - 2..], ["cancel", "pair"], "{heard:?}");
        });
    }

    /// Either exit may come first, on a long and on a short alike, on
    /// every trading service: the first one pulled out leaves the other's tab to be
    /// pulled out after it, and placing that one links the two.
    #[test]
    fn either_exit_may_come_first_on_a_long_or_a_short() {
        crate::ui::on_gtk(|| {
            use super::super::marks::TradeEvent;
            use trading::Linking;
            for linking in [Linking::AfterTheFact, Linking::AtSubmission, Linking::None] {
                let (window, pane, session, key) = long_es(linking);
                settle(&window, || pane.view.last_price().is_some());
                let last = pane.view.last_price().expect("a last price");
                let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
                let working = || session.book().orders.iter().filter(|o| o.status.working()).cloned().collect::<Vec<_>>();
                let hold = |net: i32| {
                    let now = session.book().position("951", "ESZ6").map_or(0, |held| held.net);
                    let side = if net > now { Side::Buy } else { Side::Sell };
                    let qty = (net - now).unsigned_abs();
                    session.place(&trading::OrderRequest { qty, ..resting(side, OrderKind::Market, None) }).unwrap();
                    window.desk.tell();
                    assert_eq!(session.book().position("951", "ESZ6").map(|held| held.net), Some(net));
                };
                for (net, first) in [(-1, OrderKind::Stop), (-1, OrderKind::Limit), (1, OrderKind::Stop), (1, OrderKind::Limit)] {
                    hold(net);
                    let round = format!("{linking:?}, net {net}, {} first", first.key());
                    settle(&window, || pane.view.exit_tabs() == vec![OrderKind::Limit, OrderKind::Stop]);
                    let exit = trading::exit_side(net);
                    // A short's stop above and target below; a long's the other way.
                    let at = |kind| match (kind, net > 0) {
                        (OrderKind::Stop, false) | (OrderKind::Limit, true) => off(40.0),
                        _ => off(-40.0),
                    };
                    let second = if first == OrderKind::Stop { OrderKind::Limit } else { OrderKind::Stop };
                    pane.view.drop_mark(TradeEvent::ExitAt { kind: first, price: at(first) });
                    settle(&window, || pane.view.exit_tabs() == vec![second] && !window.trade.placing.busy(&key));
                    let one = working();
                    assert_eq!(one.len(), 1, "{round}: {one:?}");
                    assert_eq!((one[0].side, one[0].kind, one[0].price), (exit, first, Some(at(first))), "{round}");
                    // Then the other, which still has its tab to be pulled from.
                    pane.view.drop_mark(TradeEvent::ExitAt { kind: second, price: at(second) });
                    settle(&window, || pane.view.exit_tabs().is_empty() && !window.trade.placing.busy(&key));
                    let both = working();
                    assert_eq!(both.len(), 2, "{round}: the second exit was not placed: {both:?}");
                    let other = both.iter().find(|o| o.kind == second).expect("the second exit");
                    assert_eq!((other.side, other.price), (exit, Some(at(second))), "{round}");
                    let mine = both.iter().find(|o| o.kind == first).expect("the first exit");
                    assert_eq!((mine.side, mine.price), (exit, Some(at(first))), "{round}: the first exit moved");
                    let links = window.desk.links();
                    let linked = links.iter().any(|link| both.iter().all(|order| link.baskets.contains(&order.basket_id)));
                    assert!(linked, "{round}: {links:?}");
                    if linking == Linking::AfterTheFact {
                        let service = session.links.lock().unwrap().clone();
                        assert!(service.contains(&vec![other.basket_id.clone(), one[0].basket_id.clone()]), "{round}: {service:?}");
                    }
                    // Flat for the next round.
                    pane.view.drop_mark(TradeEvent::Flatten);
                    settle(&window, || pane.view.mark_states().0.is_none() && working().is_empty());
                }
            }
        });
    }

    /// The app's own link, on a trading service that cannot link: once one exit
    /// fills, the app cancels the other.
    #[test]
    fn the_app_cancels_the_other_exit_once_one_fills() {
        crate::ui::on_gtk(|| {
            use super::super::marks::TradeEvent;
            let (window, pane, session, key) = long_es(trading::Linking::None);
            settle(&window, || pane.view.last_price().is_some());
            let last = pane.view.last_price().expect("a last price");
            let off = |ticks: f64| (last / 0.25).round() * 0.25 + ticks * 0.25;
            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Limit, price: off(40.0) });
            settle(&window, || pane.view.exit_tabs() == vec![OrderKind::Stop] && !window.trade.placing.busy(&key));
            pane.view.drop_mark(TradeEvent::ExitAt { kind: OrderKind::Stop, price: off(-40.0) });
            settle(&window, || !window.desk.links().is_empty());
            let link = window.desk.links()[0].clone();
            assert_eq!(link.holder, trading::Holder::App);
            session.fill(&link.baskets[0]);
            window.desk.take(trading::Wake::Moved);
            let gone = || session.book().order(&link.baskets[1]).is_some_and(|order| !order.status.working());
            settle(&window, gone);
            assert!(gone(), "the other exit outlived the position");
            assert!(session.book().position("951", "ESZ6").is_none());
            assert!(window.desk.links().is_empty());
        });
    }

    /// A position held before the chart's contract is known is drawn as
    /// soon as bars arrive, which is when the feed has found it: not on the
    /// next move of the book, nor the next time the menu opens.
    #[test]
    fn marks_appear_once_the_contract_is_known() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
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
            use super::super::marks::TradeEvent;
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
            assert!(pane.view.held_elsewhere().is_empty());
            // The root rolls to March: the trading service now trades ES as ESH7.
            session.contracts.lock().unwrap().insert("ES".into(), "ESH7".into());
            session.moved();
            settle(&window, || pane.view.mark_states().0.is_none());
            assert!(pane.view.mark_states().0.is_none(), "nothing is held in ESH7");
            let held = pane.view.held_elsewhere();
            assert_eq!(held.len(), 1, "{held:?}");
            assert!(held[0].0.starts_with("ESZ6 ▲ 1"), "named with its contract: {}", held[0].0);
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
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
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
            assert!(held.iter().any(|(words, _)| words.starts_with("ESH7 ▲ 1")), "{held:?}");
        });
    }

    /// What is asked shows at once, faint, and at full strength only once
    /// the exchange has it: an order until it is working, a market order
    /// beside the last price until it fills. A refusal takes it away; a
    /// request that is never answered stays pending. Part of an order
    /// filled says how much of how much.
    #[test]
    fn what_is_asked_is_pending_until_the_service_confirms_it() {
        crate::ui::on_gtk(|| {
            use super::super::marks::TradeEvent;
            use std::sync::atomic::Ordering;
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
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
            assert_eq!(orders(), vec![("L 1".to_string(), off(40.0), true)], "not drawn at once, faint");
            settle(&window, || session.book().orders.len() == 2);
            settle(&window, || false);
            assert_eq!(orders(), vec![("L 1".to_string(), off(40.0), true)], "received is not working");
            let target = session.book().orders[0].basket_id.clone();
            session.open(&target);
            window.desk.tell();
            assert_eq!(orders(), vec![("L 1".to_string(), off(40.0), false)], "working, yet faint");
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
            assert_eq!(orders()[1], ("STP 1".to_string(), off(-40.0), true), "taken as done without an answer");
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
            assert!(orders().iter().any(|(words, ..)| words == "▲ L 1/2"), "{:?}", orders());
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
    /// pending until the trading service fills it, as Ctrl+B does; Sell sells it.
    /// Without an account they are not there at all, which the test before
    /// this one shows.
    #[test]
    fn the_headers_buy_and_sell_trade_the_charts_quantity() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
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

    /// The header's P&L is the account's P&L for the day, the same
    /// figure the Accounts tab shows, in the profit colour up and the loss
    /// colour down, plain at nothing; nothing at all until the trading service has
    /// said, and nothing without an account.
    #[test]
    fn the_headers_day_pnl_is_the_accounts_day() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            assert_eq!(window(&store).focused_pane().day_pnl_text(), None, "a P&L with no account");

            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            let day = |window: &Rc<Window>| window.trading.borrow().book.money(&key).and_then(|money| money.day_pnl);
            settle(&window, || pane.view.last_price().is_some());
            assert_eq!(day(&window), None);
            assert_eq!(pane.day_pnl_text(), None, "a P&L before the trading service said one");

            for (said, class) in [(250.0, Some("pnl-gain")), (-340.5, Some("pnl-loss")), (0.0, None)] {
                session.book.lock().unwrap().accounts = vec![trading::Money {
                    key: "951".into(),
                    day_pnl: Some(said),
                    ..trading::Money::default()
                }];
                session.moved();
                window.desk.tell();
                settle(&window, || day(&window).is_some_and(|day| (day - said).abs() < 0.5));
                let shown = day(&window).expect("the day's P&L in the book");
                assert_eq!(
                    pane.day_pnl_text(),
                    Some((trading::pnl_text(shown, Some("USD")), class)),
                    "the header says the book's {said}"
                );
            }
        });
    }

    /// With an account, Ctrl+B buys the chart's size at the market and
    /// Ctrl+S sells it; Ctrl+2 makes the size the template's second, two
    /// contracts, which the header says, the next Ctrl+B buys and the chart
    /// keeps. Ctrl+F leaves nothing held and nothing working.
    #[test]
    fn the_trading_keys_buy_sell_size_and_go_flat() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
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
            // Pending until the trading service fills it: "Buying 2…" by the last price.
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

    /// A size picked for one contract is never another's: 5 picked while a
    /// chart of ES trades MES is not 5 ES once the trading symbol is
    /// cleared, nor 5 NQ once the chart moves to NQ; each starts at the
    /// first of its template, which the header says, and MES is 5 again
    /// once it is traded again. A size stored before sizes named their
    /// contract stays with the one the chart traded too.
    #[test]
    fn a_size_picked_for_one_contract_does_not_carry_to_another() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
            session.contracts.lock().unwrap().insert("MES".into(), "MESZ6".into());
            let find = |symbol: &str| window.index.find(symbol, None).expect(symbol);
            window.set_trading_symbol(pane.id, Some(find("MES")));
            window.pick_size(pane.id, 5);
            assert_eq!((pane.size_label_text(), window.size_of(&pane).get()), ("5".to_string(), 5));

            window.set_trading_symbol(pane.id, None);
            assert_eq!((pane.size_label_text(), window.size_of(&pane).get()), ("1".to_string(), 1), "5 MES became 5 ES");
            window.set_trading_symbol(pane.id, Some(find("MES")));
            assert_eq!(pane.size_label_text(), "5", "MES's own size forgotten");
            window.show_in(&pane, find("NQ"));
            assert_eq!((pane.size_label_text(), window.size_of(&pane).get()), ("1".to_string(), 1), "5 MES became 5 NQ");

            // Stored before sizes named their contract: NQ's, as it was traded.
            let three = trading::Quantity::new(3).unwrap();
            pane.trading_size.replace(Some(trading::sizes::Picked { template: "Futures".into(), size: three, contract: None }));
            assert_eq!(window.size_of(&pane), three);
            window.show_in(&pane, find("ES"));
            assert_eq!(window.size_of(&pane).get(), 1, "3 NQ became 3 ES");
            window.show_in(&pane, find("NQ"));
            assert_eq!(window.size_of(&pane), three);
        });
    }

    /// The size part's popover lists the template's sizes with the chart's
    /// marked; a right click on it offers to edit the templates, and the
    /// dialog's changes reach the header at once.
    #[test]
    fn the_size_part_offers_the_templates_sizes() {
        crate::ui::on_gtk(|| {
            let (window, pane, _session, _) = long_es(trading::Linking::AfterTheFact);
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

    /// The Theme page of a theme of one's own has its Trading group with
    /// or without a login, and keeps it while one is added and removed.
    #[test]
    fn the_theme_page_has_its_trading_group_with_or_without_a_login() {
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
            assert!(titles().iter().any(|title| title == "Trading"), "no Trading group with no login");

            let kept = Kept::default();
            let id = trading::add_login(&store, &kept, &typed()).expect("a login");
            window.desk.tell();
            assert!(titles().iter().any(|title| title == "Trading"), "no Trading group with a login");

            trading::remove_login(&store, &kept, &id, true).expect("removed");
            window.desk.tell();
            assert!(titles().iter().any(|title| title == "Trading"), "Trading gone with the last login");
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
        /// A futures feed draws a root as its front month, as Sierra Chart
        /// and Rithmic do.
        fn charted_contract(&self, instrument: &omacharts_engine::Instrument) -> Option<String> {
            if !self.futures_only || instrument.is_contract() {
                return instrument.is_contract().then(|| instrument.symbol.clone());
            }
            omacharts_engine::contracts::front(&instrument.symbol, omacharts_engine::contracts::today())
                .map(|front| front.symbol)
        }
    }

    /// A window charting from Yahoo then Rithmic, both usable whatever
    /// this machine has saved, or from Yahoo alone.
    fn window_on(store: &Rc<Store>, both: bool) -> Rc<Window> {
        crate::features::set(store, crate::features::Feature::Trading, true);
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

    /// A chart of ES on `feed`, a futures feed posing as the service named
    /// so, beside Yahoo, and a login of `typed`'s service signed in with
    /// account 951 holding one of ES's front month, bought 10 points under
    /// the chart's last price, its trading service's market data silent. The window,
    /// the chart, the session, the account's key and the contract.
    fn held_across(
        feed: &'static str,
        typed: &trading::Typed,
    ) -> (Rc<Window>, Rc<ChartPane>, Arc<trading::fake::FakeSession>, String, String) {
        let store = Rc::new(Store::memory().expect("a store"));
        crate::features::set(&store, crate::features::Feature::Trading, true);
        crate::features::set(&store, crate::features::Feature::Sierra, true);
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        let feeds = vec![Posing::feed("yahoo", "Yahoo Finance", false), Posing::feed(feed, "Posing", true)];
        let window = Window::build_with(&app, store.clone(), feeds);
        for listed in window.feeds() {
            listed.usable.set(true);
        }
        let pane = window.focused_pane();
        pane.feed.replace(Some(feed.into()));
        let es = window.index.find("ES", None).expect("ES in the curated half");
        window.show_in(&pane, es);
        assert!(settle(&window, || pane.view.last_price().is_some()), "the chart never drew");
        let last = pane.view.last_price().expect("a last price");
        let front = omacharts_engine::contracts::front("ES", omacharts_engine::contracts::today()).expect("a front month");
        let (id, session) = sign_in_as(&window, &store, typed, &["951"]);
        session.contracts.lock().unwrap().insert("ES".into(), front.symbol.clone());
        *session.last.lock().unwrap() = last - 10.0;
        *session.market.lock().unwrap() = None;
        let order = trading::OrderRequest { contract: front.symbol.clone(), ..resting(Side::Buy, OrderKind::Market, None) };
        session.place(&order).unwrap();
        window.desk.tell();
        (window, pane, session, format!("{id}/951"), front.symbol)
    }

    /// Sierra Chart set up as a feed and added as a trading service, with
    /// three accounts listed: a chart of ES is drawn from its files on the
    /// default account and on one of its own, and Sierra Chart chosen for
    /// it stays chosen.
    #[test]
    fn a_chart_on_a_sierra_account_is_drawn_from_sierras_files() {
        use omacharts_engine::providers::sierra::setup;
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            crate::features::set(&store, crate::features::Feature::Trading, true);
            crate::features::set(&store, crate::features::Feature::Sierra, true);
            crate::features::adopt(&store);
            let root = std::env::temp_dir().join(format!("omacharts-window-sierra-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Data")).unwrap();
            std::fs::write(root.join("Sierra4.config"), b"").unwrap();
            let days = "Date,  Open,  High,  Low,  Close,  Volume,  OpenInterest\r\n\
                2026/09/10, 6590.00, 6600.00, 6580.00, 6595.00, 1500000, 0\r\n";
            std::fs::write(root.join("Data").join("ESZ26_FUT_CME.dly"), days).unwrap();
            let sierra = omacharts_engine::providers::listed("sierra").unwrap();
            crate::feeds::set_up(&store, sierra, omacharts_engine::providers::Saved::default().with(setup::FOLDER, &root.display().to_string()).with(setup::PORT, "11099"));
            crate::feeds::add_feed(&store, "sierra");
            let typed = trading::Typed {
                service: "sierra".into(),
                name: "Sierra Chart".into(),
                user: String::new(),
                password: String::new(),
                values: vec![(setup::HOST.into(), setup::DEFAULT_HOST.into()), (setup::PORT.into(), "11099".into())],
            };
            let login = trading::add_login(&store, &trading::fake::Kept::default(), &typed).unwrap();
            let accounts = ["E7029", "Sim1", "Sim2"].map(|id| trading::fake::tests::account(&login, id)).to_vec();
            trading::cache_accounts(&store, &login, accounts);

            let _ = adw::init();
            let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            let feeds = crate::feeds::ordered(&store).into_iter().map(Arc::from).collect();
            let window = Window::build_with(&app, store.clone(), feeds);
            let ids: Vec<&str> = window.feeds().iter().map(|feed| feed.provider.id()).collect();
            assert_eq!(ids, ["sierra", "yahoo"]);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES");
            for account in [None, Some(format!("{login}/Sim1"))] {
                pane.account.replace(account.clone());
                let routed = window.route_pane(&pane, &es).map(|routed| routed.feed.provider.id());
                assert_eq!(routed, Some("sierra"), "ES on {account:?}");
            }
            window.show_in(&pane, es.clone());
            window.choose_feed(pane.id, Some("sierra".into()));
            window.show_in(&pane, es.clone());
            assert_eq!(pane.feed.borrow().as_deref(), Some("sierra"), "the feed chosen stays chosen");
            let _ = std::fs::remove_dir_all(&root);
        });
    }

    /// A Sierra Chart folder with daily bars of ES and nothing of MES, set
    /// up as the Sierra Chart feed of `store`. Removed when dropped.
    struct SierraFolder(std::path::PathBuf);

    impl SierraFolder {
        fn set_up(store: &Store, name: &str) -> SierraFolder {
            let folder = SierraFolder::written(name, true);
            folder.adopt(store);
            folder
        }

        /// The folder alone, with ES's files or with nothing in its data
        /// folder at all, not yet set up as anything.
        fn written(name: &str, with_es: bool) -> SierraFolder {
            let root = std::env::temp_dir().join(format!("omacharts-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("Data")).unwrap();
            std::fs::write(root.join("Sierra4.config"), b"").unwrap();
            if with_es {
                let days = "Date,  Open,  High,  Low,  Close,  Volume,  OpenInterest\r\n\
                    2026/09/10, 6590.00, 6600.00, 6580.00, 6595.00, 1500000, 0\r\n\
                    2026/09/11, 6595.00, 6610.00, 6590.00, 6605.00, 1400000, 0\r\n";
                std::fs::write(root.join("Data").join("ESZ26_FUT_CME.dly"), days).unwrap();
                std::fs::write(root.join("Data").join("ESZ26_FUT_CME.scid"), scid_of_es()).unwrap();
            }
            SierraFolder(root)
        }

        /// Set this folder up as the Sierra Chart feed of `store`, as its
        /// dialog does.
        fn adopt(&self, store: &Store) {
            use omacharts_engine::providers::sierra::setup;
            let root = &self.0;
            let sierra = omacharts_engine::providers::listed("sierra").unwrap();
            let saved = omacharts_engine::providers::Saved::default()
                .with(setup::FOLDER, &root.display().to_string())
                .with(setup::PORT, "11099");
            crate::feeds::set_up(store, sierra, saved);
        }
    }

    impl Drop for SierraFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A `.scid` file as Sierra Chart writes it: its header, then a few
    /// trades of ES on the second of the two days the `.dly` holds.
    fn scid_of_es() -> Vec<u8> {
        const HEADER: usize = 56;
        const RECORD: u32 = 40;
        let mut bytes = vec![0u8; HEADER];
        bytes[0..4].copy_from_slice(b"SCID");
        bytes[4..8].copy_from_slice(&(HEADER as u32).to_le_bytes());
        bytes[8..12].copy_from_slice(&RECORD.to_le_bytes());
        bytes[12..14].copy_from_slice(&1u16.to_le_bytes());
        // Sierra Chart counts microseconds from 1899-12-30.
        let from_1899 = 25_569i64 * 86_400;
        let start = 1_789_131_600i64; // 2026-09-11 13:00 UTC
        for (i, price) in [6600.0f32, 6601.0, 6602.0].into_iter().enumerate() {
            let at = (start + i as i64 * 60 + from_1899) * 1_000_000;
            bytes.extend_from_slice(&at.to_le_bytes());
            for field in [0.0, price + 0.25, price - 0.25, price] {
                bytes.extend_from_slice(&f32::to_le_bytes(field));
            }
            for count in [1u32, 2, 1, 1] {
                bytes.extend_from_slice(&count.to_le_bytes());
            }
        }
        bytes
    }

    /// What `window` streams now, as symbol and feed pairs.
    fn streamed(window: &Rc<Window>) -> Vec<(String, String)> {
        window
            .feeds()
            .iter()
            .filter_map(|feed| Some((feed.provider.id().to_string(), feed.live.clone()?)))
            .flat_map(|(id, live)| {
                live.report().into_iter().filter(|report| report.charts > 0).map(move |report| (report.symbol, id.clone()))
            })
            .collect()
    }

    /// Which feed `pane`'s chart is on now, by its id.
    fn feed_on(window: &Rc<Window>, pane: &ChartPane) -> Option<String> {
        let instrument = pane.instrument.borrow().clone()?;
        window.route_pane(pane, &instrument).map(|routed| routed.feed.provider.id().to_string())
    }

    /// A chart on ES from Sierra Chart moved to MES, which Sierra Chart has
    /// nothing of, falls back to Yahoo for as long as it is on MES; moved
    /// back to ES it is on Sierra Chart again, and nothing chose Yahoo for
    /// it on the way.
    #[test]
    fn a_chart_that_fell_back_to_yahoo_goes_back_to_sierra_with_a_symbol_sierra_has() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            crate::features::set(&store, crate::features::Feature::Sierra, true);
            crate::features::adopt(&store);
            let _folder = SierraFolder::set_up(&store, "fell-back");
            crate::feeds::add_feed(&store, "sierra");
            crate::feeds::save_order(&store, &["sierra".into(), "yahoo".into()]);
            crate::feeds::remember_logins(&store);
            let _ = adw::init();
            let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            let sierra: Arc<dyn omacharts_engine::Provider> =
                Arc::from(omacharts_engine::providers::ordered(&["sierra".to_string()]).remove(0));
            let window = Window::build_with(&app, store.clone(), vec![sierra, Posing::feed("yahoo", "Yahoo Finance", false)]);
            let pane = window.focused_pane();
            pane.timeframe.set(omacharts_engine::Timeframe::days(1));
            let es = window.index.find("ES", None).expect("ES");
            let mes = window.index.find("MES", None).expect("MES");

            window.show_in(&pane, es.clone());
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("sierra"));
            settle(&window, || pane.view.last_price().is_some());
            window.show_in(&pane, mes);
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"), "MES falls back");
            settle(&window, || pane.view.last_price().is_some());
            assert_eq!(pane.feed.borrow().as_deref(), None, "a fallback is never the chart's choice");
            window.show_in(&pane, es);
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("sierra"), "ES is Sierra Chart's again");
            settle(&window, || pane.view.last_price().is_some());
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("sierra"), "and stays there once asked");
        });
    }

    /// The window of [`a_chart_that_fell_back_to_yahoo_goes_back_to_sierra_with_a_symbol_sierra_has`]:
    /// Sierra Chart over Yahoo, with ES on file and no MES, on one daily chart.
    fn sierra_over_yahoo(name: &str) -> (Rc<Window>, Rc<ChartPane>, SierraFolder, Rc<Store>) {
        let store = Rc::new(Store::memory().expect("a store"));
        crate::features::set(&store, crate::features::Feature::Sierra, true);
        crate::features::adopt(&store);
        let folder = SierraFolder::set_up(&store, name);
        crate::feeds::add_feed(&store, "sierra");
        crate::feeds::save_order(&store, &["sierra".into(), "yahoo".into()]);
        crate::feeds::remember_logins(&store);
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        let sierra: Arc<dyn omacharts_engine::Provider> =
            Arc::from(omacharts_engine::providers::ordered(&["sierra".to_string()]).remove(0));
        let window = Window::build_with(&app, store.clone(), vec![sierra, Posing::feed("yahoo", "Yahoo Finance", false)]);
        let pane = window.focused_pane();
        pane.timeframe.set(omacharts_engine::Timeframe::minutes(5));
        (window, pane, folder, store)
    }

    /// A window opened on a machine with Sierra Chart switched on, set up
    /// and on top of the order charts from it: the flag is read before the
    /// feeds are, rather than the window opening on Yahoo alone and the
    /// chart of ES streaming nothing.
    #[test]
    fn a_window_opened_with_sierra_on_charts_from_it() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            crate::features::set(&store, crate::features::Feature::Sierra, true);
            let _folder = SierraFolder::set_up(&store, "opened-on");
            crate::feeds::add_feed(&store, "sierra");
            crate::feeds::save_order(&store, &["sierra".into(), "yahoo".into()]);
            // A fresh process: nothing has read the flags yet.
            crate::features::reset();
            let _ = adw::init();
            let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            let window = Window::build(&app, store.clone());
            let ids: Vec<&str> = window.feeds().iter().map(|feed| feed.provider.id()).collect();
            assert_eq!(ids, ["sierra", "yahoo"]);
        });
    }

    /// The author's chart after a restart: ES on account E7029 at Sierra
    /// Chart, trading MES, which the folder has no files of. It is charted
    /// from what it charts, ES, on Sierra Chart: the dot is drawn, and ES
    /// is what is streamed, from Sierra Chart.
    #[test]
    fn a_chart_of_es_trading_mes_is_charted_from_sierra_after_a_restart() {
        use omacharts_engine::providers::sierra::setup;
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            crate::features::set(&store, crate::features::Feature::Trading, true);
            crate::features::set(&store, crate::features::Feature::Sierra, true);
            let _folder = SierraFolder::set_up(&store, "trading-mes");
            let typed = trading::Typed {
                service: "sierra".into(),
                name: "Sierra Chart".into(),
                user: String::new(),
                password: String::new(),
                values: vec![(setup::HOST.into(), setup::DEFAULT_HOST.into()), (setup::PORT.into(), "11099".into())],
            };
            let login = trading::add_login(&store, &Kept::default(), &typed).unwrap();
            let accounts = ["E7029", "Sim1"].map(|id| trading::fake::tests::account(&login, id)).to_vec();
            trading::cache_accounts(&store, &login, accounts);
            assert_eq!(crate::feeds::stored_order(&store), ["sierra", "yahoo"]);
            // A fresh process: nothing has read the flags yet.
            crate::features::reset();
            let _ = adw::init();
            let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            let window = Window::build(&app, store.clone());
            let pane = window.focused_pane();
            pane.timeframe.set(omacharts_engine::Timeframe::minutes(2));
            pane.account.replace(Some(format!("{login}/E7029")));
            let es = window.index.find("ES", None).expect("ES");
            let mes = window.index.find("MES", None).expect("MES");
            window.show_in(&pane, es);
            window.set_trading_symbol(pane.id, Some(mes));
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("sierra"));
            assert!(pane.feed_dot_shown().is_some(), "the dot is drawn");
            assert!(
                settle(&window, || streamed(&window) == vec![("ES".to_string(), "sierra".to_string())]),
                "ES streams from Sierra Chart: {:?}",
                streamed(&window)
            );
            assert!(window.streaming().iter().any(|report| report.symbol == "ES"));
        });
    }

    /// A fallback is one chart's: with Sierra Chart over Yahoo stored and
    /// the window opened as a restart opens it, one chart falling back to
    /// Yahoo on MES leaves the window on Sierra Chart, and every chart of ES,
    /// in this chartbook and the next, charts from it.
    #[test]
    fn a_fallback_never_moves_the_window_off_sierra() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            crate::features::set(&store, crate::features::Feature::Sierra, true);
            let _folder = SierraFolder::set_up(&store, "window-wide");
            crate::feeds::add_feed(&store, "sierra");
            crate::feeds::save_order(&store, &["sierra".into(), "yahoo".into()]);
            crate::features::reset();
            let _ = adw::init();
            let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
            let window = Window::build(&app, store.clone());
            let in_use = |window: &Rc<Window>| window.provider().id();
            let es = window.index.find("ES", None).expect("ES");
            let mes = window.index.find("MES", None).expect("MES");
            assert_eq!(in_use(&window), "sierra");

            let first = window.focused_pane();
            first.timeframe.set(omacharts_engine::Timeframe::minutes(2));
            window.show_in(&first, mes);
            assert_eq!(feed_on(&window, &first).as_deref(), Some("yahoo"), "MES falls back");
            window.split_focused(true);
            let second = window.focused_pane();
            assert_ne!(first.id, second.id);
            second.timeframe.set(omacharts_engine::Timeframe::hours(1));
            window.show_in(&second, es.clone());
            assert_eq!(in_use(&window), "sierra", "the fallback is the chart's alone");
            assert_eq!(feed_on(&window, &second).as_deref(), Some("sierra"));

            window.duplicate_chartbook();
            let third = window.focused_pane();
            window.show_in(&third, es);
            assert_eq!(in_use(&window), "sierra");
            assert_eq!(feed_on(&window, &third).as_deref(), Some("sierra"));
            let ids: Vec<&str> = window.feeds().iter().map(|feed| feed.provider.id()).collect();
            assert_eq!(ids, ["sierra", "yahoo"]);
            assert!(
                settle(&window, || window.streaming().iter().any(|report| report.symbol == "ES" && report.charts > 0)),
                "ES streams from Sierra Chart: {:?}",
                window.streaming()
            );
        });
    }

    /// A chart streaming ES from Sierra Chart, moved to MES and on Yahoo
    /// for it, streams ES from Sierra Chart again once it is back: after the
    /// stream it left was torn down, and after a bounce quicker than that.
    #[test]
    fn a_chart_back_on_es_streams_it_from_sierra_again() {
        crate::ui::on_gtk(|| {
            let (window, pane, _folder, _store) = sierra_over_yahoo("streams-again");
            let es = window.index.find("ES", None).expect("ES");
            let mes = window.index.find("MES", None).expect("MES");
            let es_from_sierra = || streamed(&window) == vec![("ES".to_string(), "sierra".to_string())];

            window.show_in(&pane, es.clone());
            assert!(settle(&window, es_from_sierra), "ES streams from Sierra Chart: {:?}", streamed(&window));
            assert!(settle(&window, || pane.view.last_price().is_some()), "ES is drawn from Sierra Chart");

            // Long enough on MES for the stream it left to be reaped.
            window.show_in(&pane, mes.clone());
            let reaped = || window.lives().iter().all(|live| live.report().is_empty());
            assert!(settle(&window, || reaped() && pane.view.last_price().is_some()), "the stream of ES is torn down");
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"));
            window.show_in(&pane, es.clone());
            assert!(settle(&window, es_from_sierra), "ES streams from Sierra Chart again: {:?}", streamed(&window));
            assert!(settle(&window, || pane.view.last_price().is_some()), "and drawn");

            // On MES and back before anything is reaped.
            window.show_in(&pane, mes);
            window.show_in(&pane, es);
            assert!(settle(&window, es_from_sierra), "and after a quick bounce: {:?}", streamed(&window));
            assert!(settle(&window, || pane.view.last_price().is_some()));
        });
    }

    /// A window on Yahoo alone with Sierra Chart switched on, showing ES
    /// on a daily chart.
    fn on_yahoo_alone(store: &Rc<Store>) -> (Rc<Window>, Rc<ChartPane>) {
        crate::features::set(store, crate::features::Feature::Sierra, true);
        let _ = adw::init();
        let app = adw::Application::new(None::<&str>, gio::ApplicationFlags::NON_UNIQUE);
        let window = Window::build_with(&app, store.clone(), vec![Posing::feed("yahoo", "Yahoo Finance", false)]);
        let pane = window.focused_pane();
        pane.timeframe.set(omacharts_engine::Timeframe::days(1));
        let es = window.index.find("ES", None).expect("ES");
        window.show_in(&pane, es);
        assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"));
        (window, pane)
    }

    /// ES streamed from Sierra Chart and drawn, on `pane`.
    fn on_sierra_now(window: &Rc<Window>, pane: &ChartPane) {
        assert_eq!(feed_on(window, pane).as_deref(), Some("sierra"), "the chart is on Sierra Chart at once");
        assert_eq!(pane.served.borrow().as_deref().and_then(|key| key.split_once(':')).map(|(feed, _)| feed), Some("sierra"));
        assert!(
            settle(window, || streamed(window) == vec![("ES".to_string(), "sierra".to_string())]),
            "ES streams from Sierra Chart: {:?}",
            streamed(window)
        );
    }

    /// Sierra Chart added as a trading service, its folder already set up:
    /// the chart of ES on Yahoo is on Sierra Chart as soon as the panel says
    /// it changed, with nothing restarted.
    #[test]
    fn sierra_added_as_a_trading_service_takes_the_chart_at_once() {
        use omacharts_engine::providers::sierra::setup;
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            crate::features::set(&store, crate::features::Feature::Trading, true);
            let (window, pane) = on_yahoo_alone(&store);
            let _folder = SierraFolder::set_up(&store, "as-a-service");
            let typed = trading::Typed {
                service: "sierra".into(),
                name: "Sierra Chart".into(),
                user: String::new(),
                password: String::new(),
                values: vec![(setup::HOST.into(), setup::DEFAULT_HOST.into()), (setup::PORT.into(), "11099".into())],
            };
            trading::add_login(&store, &Kept::default(), &typed).expect("a login");
            window.adopt_feed();
            on_sierra_now(&window, &pane);
        });
    }

    /// Sierra Chart added on the Data page and set up there: the chart of
    /// ES is on it at once.
    #[test]
    fn sierra_added_on_the_data_page_takes_the_chart_at_once() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let (window, pane) = on_yahoo_alone(&store);
            crate::feeds::add_feed(&store, "sierra");
            window.adopt_feed();
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"), "not set up yet");
            let _folder = SierraFolder::set_up(&store, "on-the-data-page");
            window.adopt_feed();
            on_sierra_now(&window, &pane);
        });
    }

    /// Sierra Chart on the order and set up with a folder that has nothing
    /// of ES, then set up again with one that has: the feeds are the same and
    /// so is whether each can be used, and the chart still moves at once.
    #[test]
    fn a_sierra_folder_that_has_es_now_takes_the_chart_at_once() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let (window, pane) = on_yahoo_alone(&store);
            let empty = SierraFolder::written("has-nothing", false);
            empty.adopt(&store);
            crate::feeds::add_feed(&store, "sierra");
            crate::feeds::save_order(&store, &["sierra".into(), "yahoo".into()]);
            window.adopt_feed();
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"), "nothing of ES there");
            let _folder = SierraFolder::set_up(&store, "has-es");
            window.adopt_feed();
            on_sierra_now(&window, &pane);
        });
    }

    /// A feed picked from the chart's feed menu takes the chart at once,
    /// and the order's own again with it.
    #[test]
    fn a_feed_picked_from_the_menu_takes_the_chart_at_once() {
        crate::ui::on_gtk(|| {
            let (window, pane, _folder, _store) = sierra_over_yahoo("picked");
            let es = window.index.find("ES", None).expect("ES");
            window.show_in(&pane, es);
            on_sierra_now(&window, &pane);
            pane.menu_actions.activate_action("feed", Some(&"yahoo".to_variant()));
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"));
            assert!(settle(&window, || streamed(&window).is_empty()), "nothing streamed from Sierra Chart: {:?}", streamed(&window));
            pane.menu_actions.activate_action("feed", Some(&"sierra".to_variant()));
            on_sierra_now(&window, &pane);
        });
    }

    /// Yahoo taken off the order while Sierra Chart is set up: a chart of
    /// ES that was pointed at Yahoo is on Sierra Chart at once.
    #[test]
    fn yahoo_removed_puts_the_chart_on_sierra_at_once() {
        crate::ui::on_gtk(|| {
            let (window, pane, _folder, store) = sierra_over_yahoo("yahoo-removed");
            let es = window.index.find("ES", None).expect("ES");
            window.show_in(&pane, es);
            window.choose_feed(pane.id, Some("yahoo".into()));
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"));
            assert!(crate::feeds::remove_feed(&store, "yahoo"));
            window.adopt_feed();
            let ids: Vec<&str> = window.feeds().iter().map(|feed| feed.provider.id()).collect();
            assert_eq!(ids, ["sierra"]);
            on_sierra_now(&window, &pane);
        });
    }

    /// Sierra Chart chosen for a chart stays chosen for a future it has no
    /// files of, and is offered for it: MES is empty and says plainly why,
    /// rather than drawn from Yahoo, which nobody chose. Back on ES it is
    /// drawn from Sierra Chart; a stock, which it could never chart, goes
    /// back to the order.
    #[test]
    fn sierra_chosen_for_a_future_it_has_no_files_of_says_so() {
        crate::ui::on_gtk(|| {
            let (window, pane, _folder, _store) = sierra_over_yahoo("no-files-of");
            let es = window.index.find("ES", None).expect("ES");
            let mes = window.index.find("MES", None).expect("MES");
            window.show_in(&pane, es.clone());
            window.choose_feed(pane.id, Some("yahoo".into()));
            window.choose_feed(pane.id, Some("sierra".into()));
            assert_eq!(feeds(&window, &pane), ["Sierra Chart ✓", "Yahoo Finance"]);

            window.show_in(&pane, mes);
            assert_eq!(pane.feed.borrow().as_deref(), Some("sierra"), "the choice stays");
            assert_eq!(feed_on(&window, &pane), None, "nothing draws it");
            assert_eq!(pane.view.trouble_said().as_deref(), Some("MES isn't in Sierra Chart's data"));
            assert_eq!(dot(&pane).map(|dot| (dot.0, dot.1)), Some(("Sierra Chart · No MES in its data".into(), "down")));
            // Under how it stands, Yahoo first as the order's own for MES.
            assert_eq!(
                feeds(&window, &pane),
                ["Sierra Chart · No MES in its data", "Yahoo Finance", "Sierra Chart ✓"],
                "still offered, and still the one"
            );
            assert!(streamed(&window).is_empty());

            window.show_in(&pane, es);
            assert_eq!(pane.view.trouble_said(), None);
            on_sierra_now(&window, &pane);

            let spy = window.index.find("SPY", None).expect("SPY");
            window.show_in(&pane, spy);
            assert_eq!(pane.feed.borrow().as_deref(), None, "a stock is no future");
            assert_eq!(feed_on(&window, &pane).as_deref(), Some("yahoo"));
        });
    }

    /// Charting on one service and trading through another is one desk: a
    /// root charted on Sierra Chart's feed, ES as its front month, shows the
    /// position and orders an account at Rithmic holds in that month, and
    /// marks its open P&L to the chart's prices while the trading service's own say
    /// nothing; so does a root charted on Rithmic's feed for an account at
    /// Sierra Chart.
    #[test]
    fn a_chart_on_one_service_shows_and_marks_what_an_account_on_another_holds() {
        crate::ui::on_gtk(|| {
            let sierra = trading::Typed {
                service: "sierra".into(),
                name: "Sierra Chart".into(),
                user: String::new(),
                password: String::new(),
                values: Vec::new(),
            };
            for (feed, typed) in [("sierra", typed()), ("rithmic", sierra)] {
                let (window, pane, session, key, contract) = held_across(feed, &typed);
                window.mark_to_market();
                assert!(pane.view.mark_states().0.is_some(), "no position line on the {feed} chart");
                let held = window.trading.borrow().book.position(&key, &contract).and_then(|held| held.open_pnl);
                assert_eq!(held, Some(500.0), "not marked to the {feed} chart");
                let pill = pane.view.mark_states().0.map(|(words, _)| words).unwrap_or_default();
                assert!(pill.ends_with("+$500.00"), "{pill}");

                let last = pane.view.last_price().expect("a last price");
                let above = (last / 0.25).round() * 0.25 + 40.0 * 0.25;
                let limit = trading::OrderRequest {
                    contract: contract.clone(),
                    ..resting(Side::Sell, OrderKind::Limit, Some(above))
                };
                session.place(&limit).unwrap();
                window.desk.tell();
                let orders = pane.view.mark_states().1;
                assert_eq!(orders.iter().map(|(_, price, _)| *price).collect::<Vec<_>>(), vec![above], "{feed}");
            }
        });
    }

    /// [`settle`], with time rather than turns as its bound: up to `secs`
    /// seconds of the main loop, for a test that must not fail because
    /// the machine is busy.
    fn settle_for(window: &Rc<Window>, secs: u64, done: impl Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        while std::time::Instant::now() < deadline {
            window.desk.tell();
            if done() {
                return true;
            }
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        done()
    }

    /// A chart of ES set to trade MES charts ES and trades MES: its header
    /// says "ES/MES", Buy and the chart menu's orders go to MES's contract,
    /// the position drawn is MES's, marked to the chart's prices at MES's
    /// point value while the trading service's market says nothing. A limit
    /// on MES of the month ES is charted in is judged by the chart, which it
    /// trades at; on MES of another month only by that month's own last
    /// trade: with none known it is not offered, nor placed, nor allowed in
    /// hand, and the menu says why. Set back to none, it trades ES again.
    #[test]
    fn a_chart_with_a_trading_symbol_trades_it_and_charts_its_own() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            let log = super::super::log::tests::private(&window);
            session.contracts.lock().unwrap().insert("MES".into(), "MESZ6".into());
            assert!(settle_for(&window, 30, || pane.view.last_price().is_some()), "no bars on the chart");
            let mes = window.index.find("MES", None).expect("MES in the curated half");
            window.set_trading_symbol(pane.id, Some(mes));
            assert_eq!(pane.symbol_button.label().as_deref(), Some("ES/MES"));
            assert!(pane.label().starts_with("ES/MES"), "{}", pane.label());
            assert_eq!(window.order_target(&pane).map(|target| target.contract), Ok("MESZ6".into()));
            assert_eq!(pane.view.mark_states().0, None, "ES's position drawn on a chart trading MES");

            let last = pane.view.last_price().expect("a last price");
            let bought = last - 10.0;
            *session.last.lock().unwrap() = bought;
            *session.market.lock().unwrap() = None;
            pane.buy_button.emit_clicked();
            let held = |contract: &str| session.book().position("951", contract).map(|held| held.net);
            assert!(settle_for(&window, 30, || held("MESZ6") == Some(1)), "Buy did not buy MES");
            assert_eq!(held("ESZ6"), Some(1), "Buy bought ES");
            // Marked to the chart as it stands now, whatever it has moved
            // to meanwhile: nothing runs between reading it and marking.
            let now = pane.view.last_price().expect("a last price");
            // Not put off for having marked a moment ago, nor left to a
            // marking already put off.
            window.trade.marked_at.set(None);
            window.trade.marking.set(false);
            window.mark_to_market();
            let marked = window.trading.borrow().book.position(&key, "MESZ6").and_then(|held| held.open_pnl);
            let expected = (now - bought) * 5.0;
            assert!(marked.is_some_and(|marked| (marked - expected).abs() < 1e-6), "not marked to the chart at MES's point value: {marked:?}");
            let pill = trading::pnl_text(expected, Some("USD"));
            assert!(pane.view.mark_states().0.is_some_and(|(words, _)| words.ends_with(&pill)), "{:?} {pill}", pane.view.mark_states().0);

            // No price of MES anywhere, and MES of the month ES is charted
            // in: two sizes of one thing, at one price, so the chart's own
            // judges a limit, offered, placed and shown as allowed in hand.
            let below = (now / 0.25).round() * 0.25 - 40.0 * 0.25;
            let y = pane.view.y_of(below).expect("the price on screen");
            let menu = |y: f64| labels(window.chart_menu_model(y).upcast_ref());
            let limit = |contract: &str| {
                session.book().orders.iter().any(|order| order.kind == OrderKind::Limit && order.contract == contract)
            };
            let in_hand = || {
                let target = window.order_target(&pane).expect("a target");
                super::super::marks::TradeRules::default().judged_by(window.judged_by(&pane, &target)).allows(
                    Side::Buy,
                    OrderKind::Limit,
                    below,
                    pane.view.last_price(),
                )
            };
            // The Buy above heard back first: one order on its way at a time.
            let heard = || {
                window.catch_up_log();
                log.entries().into_iter().any(|e| e.line().contains("Filled 1 MESZ6"))
            };
            assert!(settle_for(&window, 30, heard), "the Buy was never heard filled");
            let rows = menu(y);
            assert!(rows.iter().any(|(label, on)| label.starts_with("Buy Limit") && *on), "{rows:?}");
            assert!(rows.iter().all(|(label, _)| !label.starts_with("No price")), "{rows:?}");
            assert!(in_hand(), "the order allowed, the drag refused");
            let actions = window.trade.chart_actions.borrow().clone().expect("the chart's orders");
            let value = order_target_value(Side::Buy, OrderKind::Limit, &key, Some(below));
            actions.activate_action("order", Some(&value.to_variant()));
            if !settle_for(&window, 30, || limit("MESZ6")) {
                window.catch_up_log();
                let said: Vec<String> = log.entries().into_iter().map(|e| e.line()).collect();
                panic!("MES's limit of ES's month was not sent: {said:#?}");
            }

            // MES of another month is a spread away: with no price of it,
            // the chart's is no stand-in. The menu says why there is no
            // limit, one asked for anyway is not placed, and said, and the
            // drag in hand is refused the same.
            session.contracts.lock().unwrap().insert("MES".into(), "MESH7".into());
            assert_eq!(window.order_target(&pane).map(|target| target.contract), Ok("MESH7".into()));
            let rows = menu(y);
            assert!(rows.iter().all(|(label, _)| !label.starts_with("Buy Limit")), "a limit judged by ES's prices: {rows:?}");
            assert!(rows.iter().any(|(label, on)| label.starts_with("No price of MESH7") && !on), "{rows:?}");
            assert!(!in_hand(), "the order refused, the drag allowed");
            actions.activate_action("order", Some(&value.to_variant()));
            settle_for(&window, 1, || false);
            assert!(!limit("MESH7"), "a limit placed with no price of MESH7 to judge it by");
            window.catch_up_log();
            let said: Vec<String> = log.entries().into_iter().map(|e| e.line()).collect();
            assert!(said.iter().any(|line| line.contains("Not placed · no price of MESH7")), "not said: {said:#?}");

            // Its own last trade, from the trading service: judged by it.
            *session.market.lock().unwrap() = Some(now);
            assert!(in_hand());
            actions.activate_action("order", Some(&value.to_variant()));
            assert!(settle_for(&window, 30, || limit("MESH7")), "the chart menu's limit was not sent");

            window.set_trading_symbol(pane.id, None);
            assert_eq!(pane.symbol_button.label().as_deref(), Some("ES"));
            assert_eq!(window.order_target(&pane).map(|target| target.contract), Ok("ESZ6".into()));
        });
    }

    /// What a chart trades instead goes with what it charted: moved from ES
    /// to NQ it trades NQ, its header says NQ alone and nothing of MES is
    /// kept, whether the chart was moved itself or followed another, while
    /// another month of ES keeps it. A trading symbol that does not track
    /// what is charted is never taken, and the picker lists only those that
    /// do.
    #[test]
    fn a_trading_symbol_goes_with_what_it_tracked() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let pane = window.focused_pane();
            let find = |symbol: &str| window.index.find(symbol, None).expect(symbol);
            window.show_in(&pane, find("ES"));
            window.set_trading_symbol(pane.id, Some(find("MES")));
            assert_eq!(pane.symbol_button.label().as_deref(), Some("ES/MES"));
            let march = find("ES").in_month(omacharts_engine::contracts::Month::new(2027, 3).unwrap()).unwrap();
            window.show_in(&pane, march);
            assert_eq!(window.trading_symbol_of(pane.id).map(|traded| traded.symbol), Some("MES".into()), "another month of ES");
            window.show_in(&pane, find("NQ"));
            assert_eq!(window.trading_symbol_of(pane.id), None, "NQ charted, MES traded");
            assert_eq!(pane.symbol_button.label().as_deref(), Some("NQ"));
            assert_eq!(pane.traded().map(|traded| traded.symbol), Some("NQ".into()));

            // Not taken where it does not track: NQ's chart trading MES.
            window.set_trading_symbol(pane.id, Some(find("MES")));
            assert_eq!(window.trading_symbol_of(pane.id), None, "MES set on a chart of NQ");
            window.set_trading_symbol(pane.id, Some(find("MNQ")));
            assert_eq!(window.trading_symbol_of(pane.id).map(|traded| traded.symbol), Some("MNQ".into()));

            // A chart following another, moved by the rail.
            window.split_focused(true);
            for each in window.panes.borrow().iter() {
                each.set_mode(omacharts_engine::LinkMode::Follow, None);
            }
            window.show_from_rail(find("ES"));
            assert!(window.panes.borrow().iter().all(|each| each.trading_symbol.borrow().is_none()), "MNQ kept on a chart of ES");
            let other = window.panes.borrow().iter().find(|each| each.id != window.focused.get()).cloned().expect("the other chart");
            window.set_trading_symbol(other.id, Some(find("MES")));
            window.show_from_rail(find("CL"));
            assert_eq!(window.trading_symbol_of(other.id), None, "MES kept on a chart that followed to CL");

            // The picker, on a chart of CL: CL, MCL and their months alone.
            window.focus(other.id);
            window.find_trading_symbol(other.id);
            let listed = window.search.listed();
            assert!(listed.iter().any(|symbol| symbol == "CL") && listed.iter().any(|symbol| symbol == "MCL"), "{listed:?}");
            assert!(listed.iter().all(|symbol| omacharts_engine::specs::track_together(omacharts_engine::specs::root_of(symbol), "CL")), "{listed:?}");
            let frame = window.search.frame();
            frame.entry.set_text("MES");
            assert!(settle(&window, || window.search.listed().is_empty()), "MES offered on a chart of CL: {:?}", window.search.listed());
            frame.popover.popdown();
        });
    }

    /// A trading symbol the window cannot name when it opens is kept,
    /// waiting, never traded in the charted symbol's place: orders are
    /// refused saying so, the header names it, and it is written back as
    /// it was. Once it can be named it is traded.
    #[test]
    fn a_trading_symbol_not_found_yet_is_waited_for_and_never_replaced_by_the_chart() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
            let store = window.store.clone();
            let six = window.index.find("6E", None).expect("6E in the curated half");
            session.contracts.lock().unwrap().insert("6E".into(), "6EZ6".into());
            window.show_in(&pane, six);
            // As a chart saved trading M6E, which no index here names.
            pane.trading_pending.replace(Some(("M6E".into(), None)));
            window.save_workspace();
            assert!(store.setting("workspace").is_some_and(|json| json.contains("\"trading_symbol\":\"M6E\"")));

            let again = self::window(&store);
            let pane = again.focused_pane();
            assert_eq!(pane.trading_unresolved().as_deref(), Some("M6E"));
            assert_eq!(pane.symbol_button.label().as_deref(), Some("6E/M6E"));
            assert_eq!(pane.traded(), None, "6E traded in M6E's place");
            let refused = again.order_target(&pane).expect_err("an order with its trading symbol unknown");
            assert!(refused.contains("M6E"), "{refused}");
            again.save_workspace();
            assert!(store.setting("workspace").is_some_and(|json| json.contains("\"trading_symbol\":\"M6E\"")), "lost on the next save");

            // Named at last: traded.
            pane.trading_pending.replace(Some(("ESZ6".into(), None)));
            again.resolve_pending();
            assert_eq!(pane.trading_unresolved(), None);
            assert_eq!(again.trading_symbol_of(pane.id), None, "ESZ6 does not track 6E");
            let es = again.index.find("ES", None).expect("ES");
            again.show_in(&pane, es);
            pane.trading_pending.replace(Some(("MES".into(), None)));
            again.resolve_pending();
            assert_eq!(again.trading_symbol_of(pane.id).map(|traded| traded.symbol), Some("MES".into()));
            assert_eq!(pane.symbol_button.label().as_deref(), Some("ES/MES"));
        });
    }

    /// Every widget under `root`, itself among them.
    fn widgets_under(root: gtk::Widget) -> Vec<gtk::Widget> {
        let (mut found, mut stack) = (Vec::new(), vec![root]);
        while let Some(widget) = stack.pop() {
            let mut child = widget.first_child();
            while let Some(next) = child {
                child = next.next_sibling();
                stack.push(next);
            }
            found.push(widget);
        }
        found
    }

    /// The titles of the chart settings' pages, and whether a Trading
    /// symbol row is among their rows, the dialog closed after.
    fn chart_settings_offer(window: &Rc<Window>) -> (Vec<String>, bool) {
        crate::ui::chart_settings::ChartSettings::present(window);
        let dialog = window.window.visible_dialog().expect("the chart's settings");
        let widgets = widgets_under(dialog.clone().upcast());
        let pages: Vec<String> = widgets
            .iter()
            .filter_map(|widget| widget.downcast_ref::<adw::PreferencesPage>().map(|page| page.title().to_string()))
            .collect();
        let row = widgets.iter().any(|widget| {
            widget
                .downcast_ref::<adw::ActionRow>()
                .is_some_and(|row| row.title() == crate::ui::chart_settings::TRADING_SYMBOL_LABEL)
        });
        dialog.force_close();
        (pages, row)
    }

    /// The trading symbol is picked over the chart settings it is asked
    /// from, which keep the keyboard: the search hangs from the dialog rather
    /// than the window behind it, and what is typed into it is searched.
    #[test]
    fn the_trading_symbol_is_searched_over_the_chart_settings() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let pane = window.focused_pane();
            window.show_in(&pane, window.index.find("ES", None).expect("ES"));
            crate::ui::chart_settings::ChartSettings::present(&window);
            let dialog = window.window.visible_dialog().expect("the chart's settings");
            window.find_trading_symbol(pane.id);
            let frame = window.search.frame();
            let parent = frame.popover.parent().expect("hung from something");
            assert!(parent.is_ancestor(&dialog) || parent == dialog.clone().upcast::<gtk::Widget>(), "hung behind the dialog");
            frame.entry.set_text("MES");
            assert!(settle(&window, || window.search.listed().first().map(String::as_str) == Some("MES")), "{:?}", window.search.listed());
            frame.popover.popdown();
            dialog.force_close();
        });
    }

    /// The trading symbol is picked from what a trading service takes
    /// orders on: futures roots tracking what is charted, opening on their
    /// months, and no stocks or indexes. With no trading service signed in
    /// a plain line says so, and typing still searches; once one is, the
    /// line goes.
    #[test]
    fn the_trading_symbol_is_picked_from_futures() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let pane = window.focused_pane();
            window.show_in(&pane, window.index.find("ES", None).expect("ES"));
            let all = |listed: &[String]| {
                listed.iter().all(|symbol| window.index.find(symbol, None).is_some_and(|found| found.kind == omacharts_engine::InstrumentKind::FutureRoot))
            };

            window.find_trading_symbol(pane.id);
            let frame = window.search.frame();
            assert_eq!(frame.note().as_deref(), Some("No trading service is signed in"));
            let listed = window.search.listed();
            assert!(["ES", "MES"].iter().all(|root| listed.iter().any(|symbol| symbol == root)), "{listed:?}");
            assert!(!listed.iter().any(|symbol| symbol == "MNQ"), "MNQ offered to a chart of ES: {listed:?}");
            assert!(all(&listed), "futures alone: {listed:?}");
            frame.entry.set_text("MES");
            assert!(settle(&window, || window.search.listed().first().map(String::as_str) == Some("MES")), "{:?}", window.search.listed());
            assert!(all(&window.search.listed()), "{:?}", window.search.listed());
            frame.entry.set_text("MESZ");
            assert!(
                settle(&window, || window.search.listed().first().is_some_and(|symbol| symbol.starts_with("MESZ"))),
                "a month typed: {:?}",
                window.search.listed()
            );
            frame.entry.set_text("AAPL");
            assert!(settle(&window, || window.search.listed().is_empty()), "no stocks: {:?}", window.search.listed());
            frame.popover.popdown();

            // The chart's own search is everything again, with nothing said.
            window.open_search_with("AAPL");
            assert!(settle(&window, || window.search.listed().first().map(String::as_str) == Some("AAPL")));
            assert_eq!(frame.note(), None);
            frame.popover.popdown();

            sign_in(&window, &store, &["951"]);
            window.find_trading_symbol(pane.id);
            assert_eq!(frame.note(), None, "signed in: nothing to say");
            frame.popover.popdown();
        });
    }

    /// The trading symbol is the chart's, kept with it across a restart.
    /// With trading off there is no Trading page, the header says ES alone
    /// and orders would be of ES: the one kept is ignored, not lost, and
    /// back when trading is.
    #[test]
    fn a_trading_symbol_is_kept_with_its_chart_and_ignored_with_trading_off() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window(&store);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            let (pages, row) = chart_settings_offer(&window);
            assert!(pages.contains(&"Trading".to_string()) && row, "{pages:?}");
            let mes = window.index.find("MES", None).expect("MES in the curated half");
            window.set_trading_symbol(pane.id, Some(mes));
            window.save_workspace();

            crate::features::set(&store, crate::features::Feature::Trading, false);
            let off = bare_window(&store);
            let pane = off.focused_pane();
            assert_eq!(pane.instrument.borrow().as_ref().map(|i| i.symbol.clone()).as_deref(), Some("ES"));
            assert_eq!(pane.trading_symbol.borrow().as_ref().map(|i| i.symbol.clone()).as_deref(), Some("MES"));
            assert_eq!(pane.symbol_button.label().as_deref(), Some("ES"));
            assert_eq!(pane.traded().map(|i| i.symbol), Some("ES".into()));
            let (pages, row) = chart_settings_offer(&off);
            assert!(!pages.contains(&"Trading".to_string()) && !row, "{pages:?}");
            off.save_workspace();

            let on = self::window(&store);
            assert_eq!(on.focused_pane().symbol_button.label().as_deref(), Some("ES/MES"), "not kept");
        });
    }

    /// Sign in a login with `numbers` as its accounts, through a fake
    /// session, and tell the window. Its id and the session.
    fn sign_in(window: &Rc<Window>, store: &Rc<Store>, numbers: &[&str]) -> (String, Arc<trading::fake::FakeSession>) {
        sign_in_as(window, store, &typed(), numbers)
    }

    /// [`sign_in`], as the login `typed`.
    fn sign_in_as(
        window: &Rc<Window>,
        store: &Rc<Store>,
        typed: &trading::Typed,
        numbers: &[&str],
    ) -> (String, Arc<trading::fake::FakeSession>) {
        let id = trading::add_login(store, &Kept::default(), typed).expect("a login");
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
        // A widget's room, such as a feed's artwork, by what it is for.
        if let Some(custom) = menu.item_attribute_value(at, "custom", None).and_then(|custom| custom.get::<String>()) {
            return format!("[{custom}]");
        }
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
                vec!["Yahoo Finance ✓", "Rithmic", "[credits-rithmic]"]
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
                    "Rithmic",
                    "[credits-rithmic]"
                ]
            );

            // A switch shows its choice at once, and goes orange until
            // the new feed's bars come.
            window.choose_feed(pane.id, Some("rithmic".into()));
            assert_eq!(dot(&pane), Some(("Rithmic · Connecting…".into(), "connecting", false, true)));
            assert_eq!(feeds(&window, &pane), vec!["Yahoo Finance", "Rithmic ✓", "[credits-rithmic]"]);
            // Choosing the default stores nothing.
            window.choose_feed(pane.id, None);
            assert!(pane.feed.borrow().is_none());

            // Passed over, it says so under the feed it fell back to.
            window.feeds()[0].usable.set(false);
            window.reroute();
            assert_eq!(
                dot(&pane).map(|dot| dot.0),
                Some("Rithmic · Connecting…\nYahoo Finance not signed in".into())
            );
            assert_eq!(feeds(&window, &pane), ["Yahoo Finance", "Rithmic ✓", "[credits-rithmic]"]);
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
            assert_eq!(feeds(&window, &pane), vec!["Rithmic ✓", "Yahoo Finance", "[credits-rithmic]"]);
            assert_eq!(dot(&pane), Some(("Rithmic · Connecting…".into(), "connecting", false, true)));

            // Picked from the menu, through its action, as a click does.
            pane.menu_actions.activate_action("feed", Some(&"yahoo".to_variant()));
            assert_eq!(feeds(&window, &pane), vec!["Rithmic", "Yahoo Finance ✓", "[credits-rithmic]"]);
            // The default chosen again is no choice: nothing is kept.
            window.choose_feed(pane.id, None);
            assert_eq!(dot(&pane).map(|dot| dot.0), Some("Rithmic · Connecting…".into()));

            // A chosen feed that cannot be used: red, no warning, and why.
            window.choose_feed(pane.id, Some("yahoo".into()));
            window.feeds()[0].usable.set(false);
            window.reroute();
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
    /// for its trouble. Choosing another account keeps the chart's feed,
    /// and a chart on the default follows the default.
    #[test]
    fn choosing_an_account_keeps_the_feed() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            let (id, session) = sign_in(&window, &store, &["951", "952"]);
            assert_eq!(pane.account_text().as_deref(), Some("951"));
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test", "951 ✓", "952"]);
            assert_eq!(feeds(&window, &pane), vec!["Rithmic ✓", "Yahoo Finance", "[credits-rithmic]"]);

            window.choose_feed(pane.id, Some("yahoo".into()));
            assert_eq!(pane.account_text().as_deref(), Some("951"), "the feed is the dot's, not the account's");
            assert_eq!(dot(&pane).map(|dot| dot.0), Some("Yahoo Finance · Connecting…".into()));
            let other = format!("{id}/952");
            pane.menu_actions.activate_action("account", Some(&other.to_variant()));
            assert_eq!(pane.account.borrow().as_deref(), Some(other.as_str()));
            assert_eq!(pane.feed.borrow().as_deref(), Some("yahoo"), "the feed stays with the chart");
            assert_eq!(pane.account_text().as_deref(), Some("952"));
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test", "951", "952 ✓"]);
            assert_eq!(dot(&pane).map(|dot| dot.0), Some("Yahoo Finance · Connecting…".into()));

            // Back on the default, and the default moves: the chart follows.
            window.choose_account(pane.id, None);
            assert_eq!(pane.account_text().as_deref(), Some("951"));
            trading::set_default_account(&store, &other);
            window.desk.tell();
            assert_eq!(pane.account_text().as_deref(), Some("952"));

            // The login in trouble: after the account, and under its row.
            session.sign_out();
            window.desk.tell();
            assert_eq!(pane.account_text().as_deref(), Some("952 signed out"));
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test · signed out", "951", "952 ✓"]);
        });
    }

    /// Two trading services with one account each: the Account part shows,
    /// its menu each service under its own heading, and picking the other
    /// service's account puts the chart on it. One account in all: none.
    #[test]
    fn two_services_with_an_account_each_offer_the_accounts_under_their_services() {
        crate::ui::on_gtk(|| {
            let store = Rc::new(Store::memory().expect("a store"));
            let window = window_on(&store, true);
            let pane = window.focused_pane();
            let es = window.index.find("ES", None).expect("ES in the curated half");
            window.show_in(&pane, es);
            let sierra = trading::Typed {
                service: "fake".into(),
                name: "Sierra Chart".into(),
                user: String::new(),
                password: String::new(),
                values: Vec::new(),
            };
            sign_in_as(&window, &store, &sierra, &["E7029"]);
            assert_eq!(pane.account_text(), None, "one account: nothing to choose");
            let (rithmic, _) = sign_in(&window, &store, &["ACC1", "ACC2"]);
            assert_eq!(pane.account_text().as_deref(), Some("E7029"));
            assert_eq!(accounts(&window, &pane), vec!["Sierra Chart", "E7029 ✓", "Rithmic Test", "ACC1", "ACC2"]);

            let acc2 = format!("{rithmic}/ACC2");
            pane.menu_actions.activate_action("account", Some(&acc2.to_variant()));
            assert_eq!(pane.account.borrow().as_deref(), Some(acc2.as_str()));
            assert_eq!(pane.account_text().as_deref(), Some("ACC2"), "at once");
            assert_eq!(accounts(&window, &pane), vec!["Sierra Chart", "E7029", "Rithmic Test", "ACC1", "ACC2 ✓"]);
            let view = window.trading.borrow();
            let chosen = view.account_of(pane.account.borrow().as_deref()).expect("an account");
            assert_eq!((chosen.login.as_str(), chosen.id.as_str()), (rithmic.as_str(), "ACC2"), "its service with it");
        });
    }

    /// Closing the window signs the trading sessions out with the feeds,
    /// so nothing is left signed in at the trading service.
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
            assert_eq!(accounts(&window, &pane), vec!["Rithmic Test", "951 ✓", "952"]);
            assert_eq!(pane.account_text().as_deref(), Some("951"));
            assert_eq!(dot(&pane), None);
        });
    }

    /// What is asked of a trading service from the chart, what became of it, what
    /// was refused, and a session ending by itself: each one entry, under
    /// the account's or the login's name.
    #[test]
    fn orders_asked_their_outcomes_and_refusals_are_logged() {
        crate::ui::on_gtk(|| {
            let (window, _pane, session, key) = long_es(trading::Linking::AfterTheFact);
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
