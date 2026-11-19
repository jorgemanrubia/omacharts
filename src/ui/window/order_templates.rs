//! Order templates in the window: the template each chart's orders go
//! with, its part in the chart's header and its menu, Ctrl+Shift and a
//! digit, the New order dialog, and the exits a working entry is given —
//! from a template, or dragged off it — which send it again as a bracket.
//!
//! A bracket is the trading service's: the entry with its exits, placed as
//! one, the exits worked once the entry fills. An entry already working
//! cannot be given exits where it stands, so giving it some, changing
//! them or taking them off cancels it and places it again, the same side,
//! kind, quantity and price, once the book has it cancelled. It loses its
//! place in the queue doing so. One that fills first is not sent again,
//! and the Log says so.

use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::Window;
use crate::cli::trading::Bracket;
use crate::log_words;
use crate::trading::brackets::{ExitLeg, Exits};
use crate::trading::order_templates::{self as templates, OrderTemplate, Worth};
use crate::trading::{self, OrderKind, Pending, Request, Side, Stage};
use crate::ui::pane::ChartPane;

/// The window's action that opens the Order templates dialog.
pub(super) const EDIT_ORDER_TEMPLATES: &str = "edit-order-templates";
/// The window's action that opens the New order dialog: Ctrl+Shift+E.
pub(super) const ORDER_ENTRY: &str = "order-entry";

/// What a working entry was bracketed with from here: the template, when
/// it went with one as it is, and its exits.
#[derive(Clone, Debug, PartialEq)]
pub struct Bracketed {
    pub name: Option<String>,
    pub exits: Exits,
}

/// An entry cancelled to be placed again with other exits, or none.
#[derive(Clone, Debug)]
pub struct Replacing {
    pub basket: String,
    pub target: trading::Target,
    pub order: trading::OrderRequest,
    pub bracketed: Option<Bracketed>,
    /// The cancel has been answered, so the account is free to place on.
    pub cancelled: bool,
}

impl Window {
    pub(super) fn install_order_templates(self: &Rc<Self>) {
        *self.trade.order_templates.borrow_mut() = templates::load(&self.store);
        let edit = gio::SimpleAction::new(EDIT_ORDER_TEMPLATES, None);
        let this = self.clone();
        edit.connect_activate(move |_, _| this.edit_order_templates(&this.window));
        self.window.add_action(&edit);
        let entry = gio::SimpleAction::new(ORDER_ENTRY, None);
        let this = self.clone();
        entry.connect_activate(move |_, _| this.open_order_entry());
        self.window.add_action(&entry);
        if let Some(actions) = self.trade.chart_actions.borrow().as_ref() {
            // "BASKET\tID", with 0 for none: the right click's Template.
            let attach = gio::SimpleAction::new("order-template", Some(glib::VariantTy::STRING));
            let this = self.clone();
            attach.connect_activate(move |_, value| {
                let Some(text) = value.and_then(|value| value.str().map(str::to_string)) else { return };
                let Some((basket, id)) = text.split_once('\t') else { return };
                let id = id.parse::<u32>().ok().filter(|id| *id > 0);
                this.template_on_order(&this.focused_pane(), basket, id);
            });
            actions.add_action(&attach);
        }
    }

    pub(super) fn remove_order_templates(self: &Rc<Self>) {
        self.window.remove_action(EDIT_ORDER_TEMPLATES);
        self.window.remove_action(ORDER_ENTRY);
        if let Some(actions) = self.trade.chart_actions.borrow().as_ref() {
            actions.remove_action("order-template");
        }
    }

    /// Read the templates again after they changed in the store: a chart
    /// on one that is gone is on none, and every header says its own.
    pub fn adopt_order_templates(self: &Rc<Self>) {
        *self.trade.order_templates.borrow_mut() = templates::load(&self.store);
        let panes = self.panes.borrow().clone();
        let mut let_go = false;
        for pane in &panes {
            if let Some(id) = pane.order_template.get()
                && templates::by_id(&self.trade.order_templates.borrow(), id).is_none()
            {
                pane.order_template.set(None);
                let_go = true;
            }
            self.write_size_of(pane);
        }
        if let_go {
            self.save_workspace();
        }
    }

    /// The Order templates dialog over `parent`, every chart showing what
    /// it saves as it is changed.
    pub fn edit_order_templates(self: &Rc<Self>, parent: &impl IsA<gtk::Widget>) {
        let this = Rc::downgrade(self);
        crate::ui::order_templates::present(
            parent,
            self.store.clone(),
            Rc::new(move || {
                if let Some(this) = this.upgrade() {
                    this.adopt_order_templates();
                }
            }),
        );
    }

    /// The template `pane`'s orders go with, while there is one.
    pub(super) fn chart_template(&self, pane: &ChartPane) -> Option<OrderTemplate> {
        let id = pane.order_template.get()?;
        templates::by_id(&self.trade.order_templates.borrow(), id).cloned()
    }

    /// The Order template part: the bracket alone on none, the name beside
    /// it on one; nothing while the chart has no account to trade on.
    pub(super) fn write_order_template_of(&self, pane: &ChartPane, has_account: bool) {
        if !has_account {
            return pane.write_order_template(None, false);
        }
        let template = self.chart_template(pane);
        let summary = template.as_ref().map(OrderTemplate::summary);
        let app_held = template.is_some() && self.order_target(pane).is_ok_and(|target| target.capabilities.bracket_holder == trading::Holder::App);
        pane.write_order_template(Some(template.as_ref().map(|t| t.name.as_str()).zip(summary.as_deref())), app_held);
    }

    /// The Order template part's menu for chart `id`: No template, each
    /// template with its key, then the way to edit them.
    pub(super) fn order_template_menu(self: &Rc<Self>, id: u32) -> gio::Menu {
        let menu = gio::Menu::new();
        let Some(pane) = self.pane(id) else { return menu };
        if let Some(action) = pane.menu_actions.lookup_action("order-template").and_downcast::<gio::SimpleAction>() {
            action.set_state(&(pane.order_template.get().unwrap_or(0) as i32).to_variant());
        }
        let choices = gio::Menu::new();
        let none = gio::MenuItem::new(Some("No template"), None);
        none.set_action_and_target_value(Some("pane.order-template"), Some(&0i32.to_variant()));
        none.set_attribute_value("accel", Some(&"<Control><Shift>0".to_variant()));
        choices.append_item(&none);
        for (at, template) in self.trade.order_templates.borrow().iter().enumerate() {
            let item = gio::MenuItem::new(Some(&template.name), None);
            item.set_action_and_target_value(Some("pane.order-template"), Some(&(template.id as i32).to_variant()));
            if at < 9 {
                item.set_attribute_value("accel", Some(&format!("<Control><Shift>{}", at + 1).to_variant()));
            }
            choices.append_item(&item);
        }
        menu.append_section(None, &choices);
        let edit = gio::Menu::new();
        edit.append(Some("Edit order templates…"), Some(&format!("win.{EDIT_ORDER_TEMPLATES}")));
        menu.append_section(None, &edit);
        menu
    }

    /// The right click's Template for the working entry `basket`: No
    /// template, then each template, the one it goes with marked.
    pub(super) fn order_template_submenu(&self, basket: &str) -> gio::Menu {
        let menu = gio::Menu::new();
        let current = self.trade.bracketed.borrow().get(basket).and_then(|bracketed| bracketed.name.clone());
        let mark = |name: &str, on: bool| if on { format!("{name} ✓") } else { name.to_string() };
        let none = gio::MenuItem::new(Some(&mark("No template", current.is_none())), None);
        none.set_action_and_target_value(Some("chart.order-template"), Some(&format!("{basket}\t0").to_variant()));
        menu.append_item(&none);
        for template in self.trade.order_templates.borrow().iter() {
            let on = current.as_deref() == Some(template.name.as_str());
            let item = gio::MenuItem::new(Some(&mark(&template.name, on)), None);
            item.set_action_and_target_value(Some("chart.order-template"), Some(&format!("{basket}\t{}", template.id).to_variant()));
            menu.append_item(&item);
        }
        menu
    }

    /// Chart `id`'s orders to go with the template `template`, or none.
    /// Sticks with the chart.
    pub(super) fn pick_order_template(self: &Rc<Self>, id: u32, template: Option<u32>) {
        let Some(pane) = self.pane(id) else { return };
        let template = template.filter(|id| templates::by_id(&self.trade.order_templates.borrow(), *id).is_some());
        if pane.order_template.get() == template {
            return;
        }
        pane.order_template.set(template);
        self.write_size_of(&pane);
        self.save_workspace();
    }

    /// Ctrl+Shift and a digit, trading shown or not: template `n` in the
    /// order they are listed, 0 none. On the working order picked out on
    /// the focused chart, if one is; else the chart's own.
    pub(super) fn on_order_template_key(self: &Rc<Self>, n: usize) -> bool {
        let template = match n {
            0 => None,
            n => match self.trade.order_templates.borrow().get(n - 1) {
                Some(template) => Some(template.id),
                // No such template: the key is still the templates', and
                // does nothing.
                None => return true,
            },
        };
        let pane = self.focused_pane();
        let picked = self.trade.picked.borrow().clone().filter(|(id, _)| *id == pane.id);
        match picked {
            Some((_, basket)) => {
                self.trade.picked.replace(None);
                self.template_on_order(&pane, &basket, template);
            }
            None => self.pick_order_template(pane.id, template),
        }
        true
    }

    /// A click on a working order: picked out, or let go if it was.
    pub(super) fn pick_order(self: &Rc<Self>, pane: &ChartPane, basket: &str) {
        let was = self.trade.picked.borrow().clone();
        let now = match was {
            Some((id, picked)) if id == pane.id && picked == basket => None,
            _ => Some((pane.id, basket.to_string())),
        };
        self.trade.picked.replace(now);
        self.apply_marks();
    }

    /// What a tick of `target`'s contract is worth, and R, to turn money
    /// into ticks.
    fn worth(&self, pane: &ChartPane, target: &trading::Target) -> Worth {
        let spec = pane.traded().and_then(|instrument| instrument.spec()).and_then(|spec| spec.point);
        let point = omacharts_engine::specs::point(omacharts_engine::specs::root_of(&target.contract)).or(spec).or_else(|| {
            let view = self.trading.borrow();
            view.book.position(&target.account.key(), &target.contract).and_then(|held| held.point)
        });
        Worth { tick: Self::tick_of(pane, target).unwrap_or(0.0), point, r: trading::chart_pnl::r_amount(&self.store) }
    }

    /// The exits an order from `pane` goes with, as `bracket` says: none,
    /// or a template's, or a target and a stop typed. A template does not
    /// bracket an order that closes some of the position held — it would
    /// be exits for an exit — and the Log says so.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bracket_of(
        &self,
        pane: &ChartPane,
        target: &trading::Target,
        side: Side,
        kind: OrderKind,
        price: Option<f64>,
        size: trading::Quantity,
        bracket: Bracket,
    ) -> Result<Option<Bracketed>, String> {
        let id = match bracket {
            Bracket::Chart => pane.order_template.get(),
            Bracket::Template(id) => id,
            Bracket::Legs(legs) => {
                let reference = price.or_else(|| self.market_price(pane, target));
                let tick = Self::tick_of(pane, target);
                let exits = match (reference, tick) {
                    (Some(reference), Some(tick)) => {
                        trading::brackets::exits(side, reference, tick, size.get(), legs.target, legs.stop)?
                    }
                    (None, _) => return Err("there is no price to count its exits from yet".into()),
                    (_, None) => return Err("the contract's tick is not known, so its exits cannot be counted".into()),
                };
                return Ok(Some(Bracketed { name: None, exits }));
            }
        };
        let Some(template) = id.and_then(|id| templates::by_id(&self.trade.order_templates.borrow(), id).cloned()) else {
            return Ok(None);
        };
        let net = self.trading.borrow().book.position(&target.account.key(), &target.contract).map_or(0, |held| held.net);
        if net != 0 && side == trading::exit_side(net) {
            self.note_on(target, &format!("{} not used · the order closes some of the position", template.name));
            return Ok(None);
        }
        if !target.capabilities.brackets {
            return Err(format!("{} holds no brackets, which an order template needs", target.login.name));
        }
        let _ = kind;
        let exits = templates::bracket(&template, size.get(), self.worth(pane, target))?;
        if exits.is_empty() {
            // Nothing to bracket with, or every exit sized to nothing on
            // this quantity: the order goes plain.
            self.note_on(target, &format!("{} not used · it has no exits for {}", template.name, size.get()));
            return Ok(None);
        }
        Ok(Some(Bracketed { name: Some(template.name), exits }))
    }

    /// The right click's Template, or a template's key on an order picked
    /// out: the working entry `basket` sent again with the template's
    /// exits, or with none.
    pub(super) fn template_on_order(self: &Rc<Self>, pane: &ChartPane, basket: &str, template: Option<u32>) {
        let template = template.and_then(|id| templates::by_id(&self.trade.order_templates.borrow(), id).cloned());
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not changed · {why}")),
        };
        let Some(order) = self.entry(&target, basket) else { return };
        let bracketed = match template {
            None => None,
            Some(template) => match templates::bracket(&template, order.qty, self.worth(pane, &target)) {
                Ok(exits) => Some(Bracketed { name: Some(template.name), exits }),
                Err(why) => return self.refuse_on(&target, &format!("Not changed · {why}")),
            },
        };
        self.send_again(pane, target, order, bracketed);
    }

    /// A target or a stop dragged off the working entry `basket` to
    /// `price`: for all of it, as far from its price as it was let go.
    pub(super) fn bracket_at(self: &Rc<Self>, pane: &ChartPane, basket: &str, kind: OrderKind, price: f64) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not changed · {why}")),
        };
        let Some(order) = self.entry(&target, basket) else { return };
        let (Some(entry), Some(tick)) = (order.price, Self::tick_of(pane, &target)) else {
            return self.refuse_on(&target, "Not changed · the contract's tick is not known");
        };
        let ticks = trading::ticks_between(entry, price, tick).unsigned_abs().max(1) as u32;
        let mut exits = self.trade.bracketed.borrow().get(basket).map(|bracketed| bracketed.exits.clone()).unwrap_or_default();
        let leg = ExitLeg { qty: order.qty, ticks };
        match kind {
            OrderKind::Stop => exits.stops = vec![leg],
            _ => exits.targets = vec![leg],
        }
        self.send_again(pane, target, order, Some(Bracketed { name: None, exits }));
    }

    /// An exit of the entry `basket`'s bracket dragged to `price`, or,
    /// with none, taken off.
    pub(super) fn move_bracket_exit(self: &Rc<Self>, pane: &ChartPane, basket: &str, stop: bool, at: usize, price: Option<f64>) {
        let target = match self.order_target(pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("Not changed · {why}")),
        };
        let Some(order) = self.entry(&target, basket) else { return };
        let Some(mut exits) = self.trade.bracketed.borrow().get(basket).map(|bracketed| bracketed.exits.clone()) else {
            return;
        };
        let group = if stop { &mut exits.stops } else { &mut exits.targets };
        if at >= group.len() {
            return;
        }
        match price {
            Some(price) => {
                let (Some(entry), Some(tick)) = (order.price, Self::tick_of(pane, &target)) else { return };
                group[at].ticks = trading::ticks_between(entry, price, tick).unsigned_abs().max(1) as u32;
            }
            None => {
                // What it closed goes to the one before it, or after.
                let gone = group.remove(at);
                if let Some(other) = group.get_mut(at.saturating_sub(1)) {
                    other.qty += gone.qty;
                }
            }
        }
        let bracketed = (!exits.is_empty()).then_some(Bracketed { name: None, exits });
        self.send_again(pane, target, order, bracketed);
    }

    /// The working entry `basket` on `target`'s account, while it can be
    /// sent again: a limit or a stop, nothing of it filled, not an exit;
    /// said in the Log when it cannot.
    fn entry(&self, target: &trading::Target, basket: &str) -> Option<trading::Order> {
        let key = target.account.key();
        let view = self.trading.borrow();
        let order = view.book.order(basket).filter(|order| order.account == key && order.status.working()).cloned();
        let net = view.book.position(&key, &target.contract).map_or(0, |held| held.net);
        drop(view);
        let why = match &order {
            None => Some("already done"),
            Some(order) if order.kind == OrderKind::Market => Some("a market order has no price to bracket from"),
            Some(order) if order.filled > 0 => Some("some of it has filled"),
            Some(order) if trading::is_exit(order.side, order.kind, net) => Some("it is an exit"),
            Some(_) if !target.capabilities.brackets => Some("the trading service holds no brackets"),
            Some(_) if self.trade.replacing.borrow().iter().any(|replacing| replacing.basket == basket) => {
                Some("it is being sent again already")
            }
            Some(_) => None,
        };
        match why {
            Some(why) => {
                self.refuse_on(target, &format!("Not changed · {why}"));
                None
            }
            None => order,
        }
    }

    /// `order` cancelled, and placed again with `bracketed`, or plain,
    /// once the book has it cancelled. Faint meanwhile.
    fn send_again(self: &Rc<Self>, pane: &ChartPane, target: trading::Target, order: trading::Order, bracketed: Option<Bracketed>) {
        let key = target.account.key();
        let request = match order
            .price
            .ok_or_else(|| "it has no price".to_string())
            .and_then(|price| {
                let qty = trading::Quantity::new(order.qty).ok_or_else(|| "it is for nothing".to_string())?;
                trading::OrderRequest::request(&target.account.id, &target.contract(), order.side, order.kind, qty, Some(price))
            }) {
            Ok(request) => request,
            Err(why) => return self.refuse_on(&target, &format!("Not changed · {why}")),
        };
        if let Err(why) = self.trade.placing.begin(&key) {
            return self.refuse_on(&target, &why);
        }
        let name = self.account_label(&key);
        let with = match &bracketed {
            Some(Bracketed { name: Some(template), .. }) => template.clone(),
            Some(Bracketed { exits, .. }) => exits.words(),
            None => "no exits".to_string(),
        };
        self.say(log_words::note(&name, format!("Send {} again with {with} · cancel, then place", super::trade::order_in_words(&order))));
        let basket = order.basket_id.clone();
        self.trade.replacing.borrow_mut().push(Replacing {
            basket: basket.clone(),
            target: target.clone(),
            order: request,
            bracketed,
            cancelled: false,
        });
        let cancel = Request::Cancel(basket.clone());
        self.ask(cancel.clone(), Pending::Cancel);
        self.apply_marks_to(pane);
        self.send(&target, move |session, number| session.cancel(number, &basket), move |this, outcome| {
            this.trade.placing.done(&key);
            let gone = cancel.clone();
            match this.heard(&cancel, outcome, "Not changed", &name) {
                Some(()) => {
                    if let Some(replacing) = this.trade.replacing.borrow_mut().iter_mut().find(|r| Request::Cancel(r.basket.clone()) == gone) {
                        replacing.cancelled = true;
                    }
                    this.settle_replacing();
                }
                None => this.trade.replacing.borrow_mut().retain(|r| Request::Cancel(r.basket.clone()) != gone),
            }
        });
    }

    /// Place again each entry the book now has cancelled; let go of one
    /// that filled first, saying so.
    pub(super) fn settle_replacing(self: &Rc<Self>) {
        let mut ready = Vec::new();
        let mut filled = Vec::new();
        {
            let view = self.trading.borrow();
            self.trade.replacing.borrow_mut().retain(|replacing| {
                if !replacing.cancelled {
                    return true;
                }
                match view.book.order(&replacing.basket).map(|order| order.status.stage) {
                    Some(Stage::Cancelled) | None => {
                        ready.push(replacing.clone());
                        false
                    }
                    Some(Stage::Filled | Stage::PartiallyFilled) => {
                        filled.push(replacing.clone());
                        false
                    }
                    Some(_) => true,
                }
            });
        }
        for replacing in filled {
            self.trade.bracketed.borrow_mut().remove(&replacing.basket);
            self.refuse_on(&replacing.target, "Not sent again · it filled first, with the exits it had");
        }
        for replacing in ready {
            self.trade.bracketed.borrow_mut().remove(&replacing.basket);
            match replacing.bracketed {
                Some(bracketed) => self.place_bracket(replacing.target, replacing.order, bracketed),
                None => self.place(replacing.target, replacing.order),
            }
        }
    }

    /// The exits each working entry on `key`'s account in `target`'s
    /// contract was bracketed with from here, where they will rest once it
    /// fills: faint while the entry is being sent again.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bracket_marks(
        &self,
        view: &trading::View,
        bracketed: &HashMap<String, Bracketed>,
        key: &str,
        target: &trading::Target,
        tick: Option<f64>,
        look: &omacharts_engine::TradingColours,
        price: &dyn Fn(f64) -> String,
        pending: &HashMap<Request, Pending>,
    ) -> Vec<super::marks::BracketMark> {
        let Some(tick) = tick.filter(|tick| *tick > 0.0) else { return Vec::new() };
        let mut marks = Vec::new();
        for order in view.book.working(key, &target.contract) {
            let (Some(entry), Some(bracketed)) = (order.price, bracketed.get(&order.basket_id)) else { continue };
            let dim = pending.contains_key(&Request::Cancel(order.basket_id.clone()));
            let exit_side = match order.side {
                Side::Buy => Side::Sell,
                Side::Sell => Side::Buy,
            };
            let (targets, stops) = bracketed.exits.prices(order.side, entry, tick);
            for (stop, legs) in [(false, targets), (true, stops)] {
                for (at, (qty, price_at)) in legs.into_iter().enumerate() {
                    let kind = if stop { OrderKind::Stop } else { OrderKind::Limit };
                    marks.push(super::marks::BracketMark {
                        basket: order.basket_id.clone(),
                        stop,
                        at,
                        price: price_at,
                        entry,
                        entry_side: order.side,
                        tag: trading::mark_tag(kind, exit_side, true, &qty.to_string()),
                        body: None,
                        price_text: price(price_at),
                        colour: if stop { look.loss.clone() } else { look.profit.clone() },
                        style: if stop { look.stop } else { look.limit },
                        dim,
                    });
                }
            }
        }
        marks
    }

    /// Ctrl+Shift+E: the New order dialog, for the focused chart.
    pub(super) fn open_order_entry(self: &Rc<Self>) {
        let pane = self.focused_pane();
        let target = match self.order_target(&pane) {
            Ok(target) => target,
            Err(why) => return self.refuse(&format!("No order · {why}")),
        };
        let tick = Self::tick_of(&pane, &target).unwrap_or(0.01);
        let ticket = crate::ui::order_entry::Ticket {
            account: self.account_label(&target.account.key()),
            contract: target.contract.clone(),
            tick,
            decimals: crate::ui::trading_panel::decimals_of(Some(tick)),
            last: self.market_price(&pane, &target).map(|last| omacharts_engine::on_tick(last, tick).0),
            quantity: self.size_of(&pane).get(),
            kinds: target.capabilities.kinds.clone(),
            templates: match target.capabilities.brackets {
                true => self.trade.order_templates.borrow().iter().map(|t| (t.id, t.name.clone(), t.summary())).collect(),
                false => Vec::new(),
            },
            template: pane.order_template.get(),
        };
        let this = Rc::downgrade(self);
        let id = pane.id;
        let account = target.account.key();
        crate::ui::order_entry::present(&self.window, ticket, move |asked| {
            let Some(this) = this.upgrade() else { return };
            let Some(pane) = this.pane(id) else { return };
            this.order_with_exits(
                &pane,
                asked.side,
                asked.kind,
                Some(&account),
                asked.price,
                trading::Quantity::new(asked.quantity),
                Bracket::Template(asked.template),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::marks::{MarksSeen as _, TradeEvent};
    use super::super::trade::tests::{long_es, long_es_on, settle};
    use super::*;
    use crate::trading::order_templates::{Distance, Exit, Size};
    use crate::trading::Session as _;

    /// [`settle`], for up to ten times as long: a busy machine answers late.
    fn wait(window: &Rc<Window>, done: impl Fn() -> bool) -> bool {
        (0..10).any(|_| settle(window, &done))
    }

    /// Two templates: Scalp, 8 ticks up and 4 down; Runner, two targets
    /// at 1R and 2R and a stop risking $250 on the order.
    fn templates_in(window: &Rc<Window>) -> (u32, u32) {
        let scalp = OrderTemplate {
            id: 1,
            name: "Scalp".into(),
            targets: vec![Exit { distance: Distance::Ticks(8), size: Size::ALL }],
            stops: vec![Exit { distance: Distance::Ticks(4), size: Size::ALL }],
        };
        let runner = OrderTemplate {
            id: 2,
            name: "Runner".into(),
            targets: vec![
                Exit { distance: Distance::R(1.0), size: Size::Share(1, 2) },
                Exit { distance: Distance::R(2.0), size: Size::Share(1, 2) },
            ],
            stops: vec![Exit { distance: Distance::R(1.0), size: Size::ALL }],
        };
        templates::save(&window.store, &[scalp, runner]);
        window.store.set_setting(crate::trading::chart_pnl::R_AMOUNT, "250");
        window.adopt_order_templates();
        (1, 2)
    }

    fn limit(session: &crate::trading::fake::FakeSession, price: f64, qty: u32) -> String {
        let order = trading::OrderRequest {
            account: "951".into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side: Side::Buy,
            qty,
            kind: OrderKind::Limit,
            price: Some(price),
            tag: trading::new_tag(),
        };
        session.place(&order).unwrap()
    }

    /// The bracket the session holds for the entry at `price`, as legs.
    fn bracket_at(session: &crate::trading::fake::FakeSession, price: f64) -> Option<Exits> {
        let brackets = session.brackets.lock().unwrap();
        brackets.values().find(|(entry, _)| entry.price == Some(price)).map(|(_, exits)| exits.clone())
    }

    /// No template until one is picked; Ctrl+Shift and a digit picks one
    /// in the order they are listed, 0 none again; the header says it, and
    /// the chart keeps it.
    #[test]
    fn the_keys_pick_a_chart_s_template_and_the_header_says_it() {
        crate::ui::on_gtk(|| {
            let (window, pane, _session, _) = long_es(trading::Linking::AfterTheFact);
            let (scalp, runner) = templates_in(&window);
            assert_eq!(pane.order_template.get(), None, "none to begin with");
            assert_eq!(pane.order_template_text(), Some(String::new()), "the bracket alone");
            assert!(window.on_order_template_key(2));
            assert_eq!(pane.order_template.get(), Some(runner));
            assert_eq!(pane.order_template_text(), Some("Runner".into()));
            assert!(window.on_order_template_key(1));
            assert_eq!(pane.order_template.get(), Some(scalp));
            assert!(window.on_order_template_key(7), "the key is the templates' whatever is shown");
            assert_eq!(pane.order_template.get(), Some(scalp), "no seventh template: nothing changes");
            assert!(window.on_order_template_key(0));
            assert_eq!(pane.order_template.get(), None, "0 is none");
            // Trading hidden, the keys are still the templates'.
            window.trade.shown.set(false);
            assert!(window.on_order_template_key(1));
            assert_eq!(pane.order_template.get(), Some(scalp), "picked with trading hidden");
            window.trade.shown.set(true);
            window.pick_order_template(pane.id, Some(runner));
            let menu = window.order_template_menu(pane.id);
            let first = menu.item_link(0, "section").unwrap();
            let label = |at: i32| first.item_attribute_value(at, "label", None).and_then(|v| v.get::<String>());
            assert_eq!((label(0), label(1), label(2)), (Some("No template".into()), Some("Scalp".into()), Some("Runner".into())));
            // A template removed takes the charts on it back to none.
            templates::save(&window.store, &window.trade.order_templates.borrow()[..1]);
            window.adopt_order_templates();
            assert_eq!(pane.order_template.get(), None);
        });
    }

    /// A market order from a chart on a template goes as one bracket,
    /// its exits counted from the template; one that closes some of the
    /// position goes plain.
    #[test]
    fn a_chart_s_template_brackets_its_market_orders() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            let (_, runner) = templates_in(&window);
            window.pick_order_template(pane.id, Some(runner));
            window.pick_size(pane.id, 2);
            let size = window.size_of(&pane).get();
            assert_eq!(size, 2);
            window.market_on(&pane, Side::Buy);
            let heard = || session.heard.lock().unwrap().last().copied();
            assert!(wait(&window, || heard() == Some("bracket")), "no bracket sent: {:?}", heard());
            // Filled at once at the fake's last, 7,612.25, and its exits in.
            // ES at $12.50 a tick a contract, two contracts: $25 a tick.
            // 1R is $250: 10 ticks, 2R 20, and the stop at 1R 10 ticks.
            let mut exits: Vec<(OrderKind, u32, f64)> = session
                .book()
                .orders
                .iter()
                .filter(|order| order.status.working())
                .map(|order| (order.kind, order.qty, order.price.unwrap_or_default()))
                .collect();
            exits.sort_by(|a, b| a.2.total_cmp(&b.2));
            assert_eq!(
                exits,
                [(OrderKind::Stop, 2, 7_609.75), (OrderKind::Limit, 1, 7_614.75), (OrderKind::Limit, 1, 7_617.25)]
            );
            // The first answered, so the account takes the next.
            let free = || {
                let free = window.trade.placing.begin(&key).is_ok();
                if free {
                    window.trade.placing.done(&key);
                }
                free
            };
            assert!(wait(&window, free), "the bracket never answered");
            let orders = session.book().orders.len();
            window.market_on(&pane, Side::Sell);
            assert!(wait(&window, || session.book().orders.len() > orders), "the sell never sent");
            assert_eq!(heard(), Some("place"), "a sell closing the long went as a bracket");
        });
    }

    /// Half off on the fake service, R at $500, buying onto the long: a
    /// 3-lot is stopped whole 13 ticks down ($487.50, rounded toward the
    /// fill) and has 1 contract taken off 20 ticks up ($750); a 1-lot has
    /// no half to take off, so goes with its stop alone, 40 ticks down.
    #[test]
    fn half_off_brackets_what_each_order_adds() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, key) = long_es(trading::Linking::AfterTheFact);
            let half = OrderTemplate {
                id: 3,
                name: "Half off".into(),
                targets: vec![Exit { distance: Distance::R(1.5), size: Size::Share(1, 2) }],
                stops: vec![Exit { distance: Distance::R(1.0), size: Size::ALL }],
            };
            templates::save(&window.store, &[half]);
            window.store.set_setting(crate::trading::chart_pnl::R_AMOUNT, "500");
            window.adopt_order_templates();
            window.pick_order_template(pane.id, Some(3));
            let heard = || session.heard.lock().unwrap().last().copied();
            let free = || {
                let free = window.trade.placing.begin(&key).is_ok();
                if free {
                    window.trade.placing.done(&key);
                }
                free
            };
            let exits = || {
                let mut exits: Vec<(OrderKind, u32, f64)> = session
                    .book()
                    .orders
                    .iter()
                    .filter(|order| order.status.working())
                    .map(|order| (order.kind, order.qty, order.price.unwrap_or_default()))
                    .collect();
                exits.sort_by(|a, b| a.2.total_cmp(&b.2));
                exits
            };
            // Filled at the fake's last, 7,612.25.
            window.pick_size(pane.id, 3);
            window.market_on(&pane, Side::Buy);
            assert!(wait(&window, || heard() == Some("bracket")), "no bracket sent: {:?}", heard());
            assert_eq!(exits(), [(OrderKind::Stop, 3, 7_609.00), (OrderKind::Limit, 1, 7_617.25)]);
            assert!(wait(&window, free), "the bracket never answered");

            window.pick_size(pane.id, 1);
            window.market_on(&pane, Side::Buy);
            assert!(wait(&window, || exits().len() == 3), "no second bracket: {:?}", exits());
            assert_eq!(heard(), Some("bracket"));
            assert_eq!(
                exits(),
                [(OrderKind::Stop, 1, 7_602.25), (OrderKind::Stop, 3, 7_609.00), (OrderKind::Limit, 1, 7_617.25)],
                "the 1-lot with its stop alone"
            );
        });
    }

    /// A template given to a working limit, from its right click or a key
    /// on it picked out, sends it again as a bracket, the template's name
    /// beside it on the chart; No template sends it again plain.
    #[test]
    fn a_template_is_attached_to_a_working_limit() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
            let (scalp, _) = templates_in(&window);
            wait(&window, || pane.view.last_price().is_some());
            let at = (pane.view.last_price().expect("a last price") / 0.25).round() * 0.25 - 2.0;
            let basket = limit(&session, at, 1);
            window.desk.tell();
            let section = window.order_section(pane.view.y_of(at).expect("on screen")).unwrap();
            let submenu = (0..section.n_items()).find_map(|at| section.item_link(at, "submenu"));
            assert!(submenu.is_some(), "no Order template on the order's right click");

            window.template_on_order(&pane, &basket, Some(scalp));
            assert!(wait(&window, || bracket_at(&session, at).is_some()), "not sent again as a bracket");
            assert_eq!(session.book().order(&basket).map(|order| order.status.stage), Some(Stage::Cancelled));
            let exits = bracket_at(&session, at).unwrap();
            assert_eq!((exits.targets[0].ticks, exits.stops[0].ticks), (8, 4));
            wait(&window, || window.trade.bracketed.borrow().len() == 1);
            let words = pane.view.marks().1;
            assert!(words.iter().any(|w| w.contains("Scalp")), "the name beside it: {words:?}");
            // Its exits drawn where they will rest: 8 ticks up, 4 down.
            let prices: Vec<f64> =
                pane.view.price_marks().iter().filter(|mark| mark.id.starts_with("exit:")).map(|mark| mark.price).collect();
            assert!(prices.contains(&(at + 2.0)) && prices.contains(&(at - 1.0)), "{prices:?}");

            // Picked out by a click, and a key: Scalp off, by 0.
            let new = window.trade.bracketed.borrow().keys().next().cloned().unwrap();
            pane.view.drop_mark(TradeEvent::Pick(new.clone()));
            assert_eq!(window.trade.picked.borrow().clone(), Some((pane.id, new.clone())));
            session.brackets.lock().unwrap().clear();
            let before = session.book().orders.len();
            assert!(window.on_order_template_key(0));
            assert_eq!(pane.order_template.get(), None, "the chart's own was not what changed");
            assert!(wait(&window, || session.book().orders.len() > before), "not sent again");
            assert!(session.brackets.lock().unwrap().is_empty(), "sent again with exits");
            assert_eq!(session.book().order(&new).map(|order| order.status.stage), Some(Stage::Cancelled));
        });
    }

    /// Where the app, not the trading service, places a template's exits
    /// as the entry fills (Sierra Chart), the header's template and the
    /// entry's name say "Omacharts"; nowhere else.
    #[test]
    fn a_template_whose_exits_the_app_places_says_so() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es_on(|session| session.bracket_holder = trading::Holder::App);
            let (scalp, _) = templates_in(&window);
            assert_eq!(pane.order_template_text(), Some(String::new()), "no template, no mark");
            window.pick_order_template(pane.id, Some(scalp));
            assert_eq!(pane.order_template_text(), Some(format!("Scalp · {}", trading::APP_HELD)));
            assert!(pane.order_template_menu.tooltip_text().is_some_and(|tip| tip.contains(trading::APP_HELD_WORDS)));
            wait(&window, || pane.view.last_price().is_some());
            let at = (pane.view.last_price().expect("a last price") / 0.25).round() * 0.25 - 2.0;
            let basket = limit(&session, at, 1);
            window.desk.tell();
            window.template_on_order(&pane, &basket, Some(scalp));
            assert!(wait(&window, || window.trade.bracketed.borrow().len() == 1), "never bracketed");
            let words = pane.view.marks().1;
            assert!(words.iter().any(|w| w.contains(&format!("Scalp · {}", trading::APP_HELD))), "{words:?}");

            let (window, pane, _, _) = long_es(trading::Linking::AfterTheFact);
            let (scalp, _) = templates_in(&window);
            window.pick_order_template(pane.id, Some(scalp));
            assert_eq!(pane.order_template_text(), Some("Scalp".into()), "the trading service holds them");
        });
    }

    /// A stop dragged off a working limit, as off a position: the limit
    /// sent again as a bracket with that stop, as far from it as it was let
    /// go; then a target the same way, the stop kept.
    #[test]
    fn a_stop_is_dragged_off_a_working_limit() {
        crate::ui::on_gtk(|| {
            let (window, pane, session, _) = long_es(trading::Linking::AfterTheFact);
            let basket = limit(&session, 7_600.0, 2);
            window.desk.tell();
            pane.view.drop_mark(TradeEvent::BracketAt { basket, kind: OrderKind::Stop, price: 7_597.0 });
            assert!(wait(&window, || bracket_at(&session, 7_600.0).is_some()), "not sent again as a bracket");
            let exits = bracket_at(&session, 7_600.0).unwrap();
            assert_eq!(exits.stops, [ExitLeg { qty: 2, ticks: 12 }]);
            assert!(exits.targets.is_empty());
            wait(&window, || window.trade.bracketed.borrow().len() == 1);

            let new = window.trade.bracketed.borrow().keys().next().cloned().unwrap();
            session.brackets.lock().unwrap().clear();
            pane.view.drop_mark(TradeEvent::BracketAt { basket: new, kind: OrderKind::Limit, price: 7_605.0 });
            assert!(wait(&window, || bracket_at(&session, 7_600.0).is_some()));
            let exits = bracket_at(&session, 7_600.0).unwrap();
            assert_eq!((exits.targets.clone(), exits.stops.clone()), (vec![ExitLeg { qty: 2, ticks: 20 }], vec![ExitLeg { qty: 2, ticks: 12 }]));
        });
    }
}
