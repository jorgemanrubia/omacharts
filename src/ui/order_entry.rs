//! The New order dialog, Ctrl+Shift+E: an order for the focused chart's
//! account and contract with every choice in front of you — the side, the
//! kind, the quantity, the price where it has one, and the order template
//! it goes with.
//!
//! The dialog only asks. What it asks for goes to the window, which judges
//! and sends it as it does an order from the chart's own menu, so it is
//! refused for the same reasons, in the same words.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::trading::{OrderKind, Side};

pub const TITLE: &str = "New order";

/// What the dialog starts from: the focused chart's.
#[derive(Clone, Debug, PartialEq)]
pub struct Ticket {
    /// The account it goes on, as the header names it.
    pub account: String,
    /// What the order is for, as the button says it: "ES".
    pub contract: String,
    /// The step its price moves in, and how many places it is written to.
    pub tick: f64,
    pub decimals: usize,
    /// Where the price starts: the last trade.
    pub last: Option<f64>,
    /// The chart's quantity.
    pub quantity: u32,
    /// The kinds its trading service takes.
    pub kinds: Vec<OrderKind>,
    /// The order templates, as listed, by id, name and summary, and the
    /// chart's. None at all when the trading service takes no brackets.
    pub templates: Vec<(u32, String, String)>,
    pub template: Option<u32>,
}

/// What was asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Asked {
    pub side: Side,
    pub kind: OrderKind,
    pub quantity: u32,
    pub price: Option<f64>,
    pub template: Option<u32>,
}

/// The order in words, as its button says it: "Buy 2 ES at market",
/// "Sell 2 ES at 5,412.25", "Buy 2 ES stop 5,420.00", and its template
/// after a dot when it has one: "Buy 2 ES at market · Runner".
pub fn words(ticket: &Ticket, asked: &Asked) -> String {
    let order = order_words(ticket, asked);
    match asked.template.and_then(|id| ticket.templates.iter().find(|(each, ..)| *each == id)) {
        Some((_, name, _)) => format!("{order} · {name}"),
        None => order,
    }
}

fn order_words(ticket: &Ticket, asked: &Asked) -> String {
    let side = match asked.side {
        Side::Buy => "Buy",
        Side::Sell => "Sell",
    };
    let price = asked.price.map(|price| omacharts_engine::grouped_decimals(price, ticket.decimals)).unwrap_or_default();
    match asked.kind {
        OrderKind::Market => format!("{side} {} {} at market", asked.quantity, ticket.contract),
        OrderKind::Limit => format!("{side} {} {} at {price}", asked.quantity, ticket.contract),
        OrderKind::Stop => format!("{side} {} {} stop {price}", asked.quantity, ticket.contract),
    }
}

/// The parts of the dialog, kept for the tests to work as hands would.
pub struct Form {
    pub dialog: adw::Dialog,
    pub buy: gtk::ToggleButton,
    pub sell: gtk::ToggleButton,
    pub kind: adw::ComboRow,
    pub quantity: adw::SpinRow,
    pub price: adw::SpinRow,
    pub template: adw::ComboRow,
    pub place: gtk::Button,
}

/// Open it over `parent`; `place` is handed the order once its button is
/// pressed, and the dialog closes.
pub fn present(parent: &impl IsA<gtk::Widget>, ticket: Ticket, place: impl Fn(Asked) + 'static) -> Form {
    let form = build(ticket, place);
    form.dialog.present(Some(parent));
    form
}

pub fn build(ticket: Ticket, place: impl Fn(Asked) + 'static) -> Form {
    let ticket = Rc::new(ticket);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    page.add(&group);

    let side_row = adw::ActionRow::new();
    side_row.set_title("Side");
    let sides = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    sides.add_css_class("linked");
    sides.set_valign(gtk::Align::Center);
    let buy = gtk::ToggleButton::with_label("Buy");
    let sell = gtk::ToggleButton::with_label("Sell");
    sell.set_group(Some(&buy));
    buy.set_active(true);
    buy.add_css_class("trade-buy");
    sell.add_css_class("trade-sell");
    sides.append(&buy);
    sides.append(&sell);
    side_row.add_suffix(&sides);
    group.add(&side_row);

    let kinds: Vec<OrderKind> =
        [OrderKind::Market, OrderKind::Limit, OrderKind::Stop].into_iter().filter(|kind| ticket.kinds.contains(kind)).collect();
    let kind = adw::ComboRow::new();
    kind.set_title("Type");
    let labels: Vec<&str> = kinds
        .iter()
        .map(|kind| match kind {
            OrderKind::Market => "Market",
            OrderKind::Limit => "Limit",
            OrderKind::Stop => "Stop",
        })
        .collect();
    kind.set_model(Some(&gtk::StringList::new(&labels)));
    group.add(&kind);

    let quantity = adw::SpinRow::with_range(1.0, 100_000.0, 1.0);
    quantity.set_title("Quantity");
    quantity.set_digits(0);
    quantity.set_value(f64::from(ticket.quantity.max(1)));
    group.add(&quantity);

    let tick = if ticket.tick > 0.0 { ticket.tick } else { 0.01 };
    let price = adw::SpinRow::with_range(tick, 100_000_000.0, tick);
    price.set_title("Price");
    price.set_digits(ticket.decimals as u32);
    price.set_snap_to_ticks(true);
    price.set_value(ticket.last.unwrap_or(tick));
    price.set_visible(false);
    group.add(&price);

    let template = adw::ComboRow::new();
    template.set_title("Template");
    let mut names = vec!["No template".to_string()];
    names.extend(ticket.templates.iter().map(|(_, name, _)| name.clone()));
    template.set_model(Some(&gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>())));
    let at = ticket.template.and_then(|id| ticket.templates.iter().position(|(each, ..)| *each == id)).map_or(0, |at| at + 1);
    template.set_selected(at as u32);
    // No templates, or a trading service that takes no brackets: nothing
    // to choose.
    template.set_visible(!ticket.templates.is_empty());
    group.add(&template);

    let place_button = gtk::Button::new();
    place_button.add_css_class("pill");
    place_button.add_css_class("suggested-action");
    place_button.set_halign(gtk::Align::Center);
    place_button.set_margin_top(12);
    let action_group = adw::PreferencesGroup::new();
    action_group.add(&place_button);
    page.add(&action_group);

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(TITLE, &format!("{} · {}", ticket.account, ticket.contract))));
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&page));
    let dialog = adw::Dialog::new();
    dialog.set_title(TITLE);
    dialog.set_content_width(400);
    dialog.set_child(Some(&toolbar));
    dialog.set_default_widget(Some(&place_button));
    // On the quantity, not the button: Ctrl+Shift+E and Enter does not
    // send an order nobody has looked at.
    dialog.set_focus(Some(&quantity));

    let form = Form { dialog, buy, sell, kind, quantity, price, template, place: place_button };
    let kinds = Rc::new(kinds);
    let read = {
        let (buy, kind, quantity, price, template) =
            (form.buy.clone(), form.kind.clone(), form.quantity.clone(), form.price.clone(), form.template.clone());
        let (ticket, kinds) = (ticket.clone(), kinds.clone());
        move || {
            let kind = kinds.get(kind.selected() as usize).copied().unwrap_or(OrderKind::Market);
            Asked {
                side: if buy.is_active() { Side::Buy } else { Side::Sell },
                kind,
                quantity: quantity.value().round().max(1.0) as u32,
                price: (kind != OrderKind::Market).then(|| price.value()),
                template: match template.selected() {
                    0 => None,
                    at => ticket.templates.get(at as usize - 1).map(|(id, ..)| *id),
                },
            }
        }
    };
    let read = Rc::new(read);
    let said = {
        let (button, price, ticket, read) = (form.place.clone(), form.price.clone(), ticket.clone(), read.clone());
        let template = form.template.clone();
        Rc::new(move || {
            let asked = read();
            price.set_visible(asked.kind != OrderKind::Market);
            button.set_label(&words(&ticket, &asked));
            let summary = asked.template.and_then(|id| ticket.templates.iter().find(|(each, ..)| *each == id));
            template.set_subtitle(summary.map_or("Sent with no exits", |(.., summary)| summary.as_str()));
        })
    };
    said();
    for toggle in [&form.buy, &form.sell] {
        let said = said.clone();
        toggle.connect_toggled(move |_| said());
    }
    for combo in [&form.kind, &form.template] {
        let said = said.clone();
        combo.connect_selected_notify(move |_| said());
    }
    for spin in [&form.quantity, &form.price] {
        let said = said.clone();
        spin.connect_value_notify(move |_| said());
    }
    let placed = Rc::new(Cell::new(false));
    let closing = form.dialog.clone();
    form.place.connect_clicked(move |_| {
        if placed.replace(true) {
            return;
        }
        place(read());
        closing.close();
    });
    form
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> Ticket {
        Ticket {
            contract: "ES".into(),
            tick: 0.25,
            decimals: 2,
            last: Some(5412.25),
            quantity: 2,
            kinds: vec![OrderKind::Market, OrderKind::Limit, OrderKind::Stop],
            account: "Practice".into(),
            templates: vec![(4, "Scalp".into(), "+8t | −4t".into()), (7, "Swing".into(), "+2R | −1R".into())],
            template: Some(7),
        }
    }

    #[test]
    fn the_button_says_the_order() {
        let ticket = ticket();
        let asked = Asked { side: Side::Buy, kind: OrderKind::Market, quantity: 2, price: None, template: None };
        assert_eq!(words(&ticket, &asked), "Buy 2 ES at market");
        let asked = Asked { side: Side::Sell, kind: OrderKind::Limit, price: Some(5412.25), ..asked };
        assert_eq!(words(&ticket, &asked), "Sell 2 ES at 5,412.25");
        let asked = Asked { side: Side::Buy, kind: OrderKind::Stop, price: Some(5420.0), ..asked };
        assert_eq!(words(&ticket, &asked), "Buy 2 ES stop 5,420.00");
    }

    /// Started from the chart: its quantity and its template; a limit
    /// shows the price, and what is placed is what the form says.
    #[test]
    fn the_form_starts_from_the_chart_and_places_what_it_says() {
        crate::ui::on_gtk(|| {
            let placed = Rc::new(std::cell::RefCell::new(None));
            let kept = placed.clone();
            let form = build(ticket(), move |asked| *kept.borrow_mut() = Some(asked));
            assert_eq!(form.place.label().as_deref(), Some("Buy 2 ES at market · Swing"));
            assert_eq!(form.template.selected(), 2, "the chart's template");
            assert!(!form.price.is_visible());
            form.sell.set_active(true);
            form.kind.set_selected(1);
            assert!(form.price.is_visible());
            assert_eq!(form.place.label().as_deref(), Some("Sell 2 ES at 5,412.25 · Swing"));
            form.template.set_selected(0);
            assert_eq!(form.place.label().as_deref(), Some("Sell 2 ES at 5,412.25"));
            form.place.emit_clicked();
            assert_eq!(
                *placed.borrow(),
                Some(Asked { side: Side::Sell, kind: OrderKind::Limit, quantity: 2, price: Some(5412.25), template: None })
            );
        });
    }
}
