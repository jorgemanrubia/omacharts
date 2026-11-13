//! The account's fills on its chart, for a chart that asks for them.
//!
//! A chart with Show fills on marks every fill of its account in the
//! contract it trades: those the trading service's fill history has, asked
//! for once for each account the first time a chart needs it, and those
//! the session's book has, today's and every one heard since, which arrive
//! as they happen. [`trading::executions`] makes one list of them and
//! pairs it into round trips; the chart draws what it is handed.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use omacharts_chart::{ExecutionMark, Executions, TripLine};

use super::Window;
use crate::trading::{self, executions, Side};
use crate::ui::pane::ChartPane;

/// How far back an account's fill history is asked for.
pub const HISTORY_DAYS: i64 = 30;

/// What the window keeps of the fills, while trading is in it.
#[derive(Default)]
pub(super) struct Fills {
    /// Each account's fill history, by key.
    histories: RefCell<HashMap<String, History>>,
    /// When a fill told with no time of its own was first seen here, by
    /// its order and its place among the order's fills: where it is
    /// marked, from then on.
    heard: RefCell<HashMap<(String, usize), i64>>,
}

impl Fills {
    /// Forget it all, as trading leaves the window.
    pub fn clear(&self) {
        self.histories.borrow_mut().clear();
        self.heard.borrow_mut().clear();
    }
}

/// Where an account's fill history stands.
enum History {
    /// Asked for, and not answered yet.
    Asking,
    Got(Vec<trading::Order>),
    /// The trading service would not give it, on its login's session as it
    /// stood after this many refreshes: asked again once it has refreshed
    /// again, after a drop or a new sign-in, and not before.
    Refused(u32),
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

impl Window {
    /// Whether the chart `id` marks its account's fills.
    pub fn shows_fills(&self, id: u32) -> bool {
        self.pane(id).is_some_and(|pane| pane.shows_fills.get())
    }

    /// Mark the chart `id`'s fills, or stop.
    pub fn set_shows_fills(self: &Rc<Self>, id: u32, shown: bool) {
        let Some(pane) = self.pane(id) else { return };
        if pane.shows_fills.replace(shown) == shown {
            return;
        }
        self.apply_fills_to(&pane);
        self.save_workspace();
    }

    /// Mark `pane`'s fills, when it asks for them and trades through an
    /// account; otherwise none.
    pub(super) fn apply_fills_to(self: &Rc<Self>, pane: &ChartPane) {
        let target = match pane.shows_fills.get() && self.trade.shown.get() {
            true => self.order_target(pane).ok(),
            false => None,
        };
        let Some(target) = target else {
            return pane.view.set_executions(Executions::default());
        };
        let key = target.account.key();
        self.ask_fill_history(&key, &target);
        let view = self.trading.borrow();
        let live: Vec<&trading::Order> = view.book.orders.iter().filter(|order| order.account == key).collect();
        // The book's word on an order is the last one: its copy in the
        // history is left out.
        let known: HashSet<&str> = live.iter().map(|order| order.basket_id.as_str()).collect();
        let histories = self.trade.fills.histories.borrow();
        let past = match histories.get(&key) {
            Some(History::Got(orders)) => &orders[..],
            _ => &[],
        };
        let past = past.iter().filter(|order| !known.contains(order.basket_id.as_str()));
        let mut heard = self.trade.fills.heard.borrow_mut();
        let at = now();
        let fills = executions::fills_of(live.iter().copied().chain(past), &target.contract, &mut |basket, index| {
            *heard.entry((basket.to_string(), index)).or_insert(at)
        });
        let held = view.book.position(&key, &target.contract).map_or(0, |held| held.net);
        let paired = executions::pair(&fills, executions::held_before(&fills, held));
        drop((heard, histories, view));

        // The candles' own colours for a buy and a sell, never turned round
        // by the chart's switch: a buy is the theme's rise. A trip's line is
        // its money's, in the trading colours, which the switch does turn.
        let theme = self.theming.borrow().theme();
        let bars = omacharts_engine::theme_bars(&theme);
        let look = omacharts_engine::theme_trading(&theme, pane.swapped.get());
        let mut trips: Vec<TripLine> = paired
            .trips
            .iter()
            .map(|trip| TripLine { from: trip.opened, to: trip.closed, won: trip.won() })
            .collect();
        trips.sort_by_key(|trip| trip.from.0);
        pane.view.set_executions(Executions {
            marks: paired
                .marks
                .iter()
                .map(|mark| ExecutionMark {
                    ts: mark.at,
                    price: mark.price,
                    buy: mark.side == Side::Buy,
                    opens: mark.opens,
                    closes: mark.closes,
                })
                .collect(),
            trips,
            buy: bars.up,
            sell: bars.down,
            won: look.profit,
            lost: look.loss,
            even: theme.ui.text_muted.clone(),
        });
    }

    /// Ask for `key`'s fill history, unless it has been, is on its way, or
    /// was refused on the session as it stands. Every chart is marked again
    /// once it comes.
    fn ask_fill_history(self: &Rc<Self>, key: &str, target: &trading::Target) {
        let login = target.login.id.clone();
        if !target.capabilities.history || !self.desk.live(&login) {
            return;
        }
        let refreshes = self.desk.refreshes(&login);
        match self.trade.fills.histories.borrow().get(key) {
            Some(History::Asking | History::Got(_)) => return,
            Some(History::Refused(on)) if *on == refreshes => return,
            _ => {}
        }
        let Some(session) = self.desk.session(&login) else { return };
        self.trade.fills.histories.borrow_mut().insert(key.to_string(), History::Asking);
        let number = target.account.id.clone();
        let to = now();
        let from = to - HISTORY_DAYS * 86_400;
        let (sender, receiver) = async_channel::bounded(1);
        std::thread::spawn(move || {
            // A minute past now, for a clock a little behind the trading
            // service's.
            let _ = sender.send_blocking(session.history(&number, from, to + 60));
        });
        let this = Rc::downgrade(self);
        let key = key.to_string();
        gtk::glib::spawn_future_local(async move {
            let Ok(outcome) = receiver.recv().await else { return };
            let Some(this) = this.upgrade() else { return };
            // Trading left the window meanwhile: nothing to keep it for.
            if !this.trade.shown.get() {
                return;
            }
            let history = match outcome {
                Ok(orders) => History::Got(orders),
                Err(_) => History::Refused(refreshes),
            };
            this.trade.fills.histories.borrow_mut().insert(key, history);
            let panes = this.panes.borrow().clone();
            for pane in &panes {
                this.apply_fills_to(pane);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gtk::glib;

    use super::*;
    use crate::store::Store;
    use crate::trading::fake::{FakeSession, Kept};
    use crate::trading::{Fill, OrderKind, Session as _, Stage};

    /// A filled order of one fill, as a fill history has it: synthetic,
    /// on the tests' account.
    fn filled(basket: &str, side: Side, at: i64, price: f64) -> trading::Order {
        trading::Order {
            account: "951".into(),
            basket_id: basket.into(),
            contract: "ESZ6".into(),
            exchange: "CME".into(),
            side,
            qty: 1,
            filled: 1,
            kind: OrderKind::Market,
            price: None,
            status: Stage::Filled.into(),
            fill_price: Some(price),
            fills: vec![Fill { basket: basket.into(), at, qty: 1, price }],
            at,
            tag: None,
            tick: Some(0.25),
        }
    }

    /// A window with ES on its chart, signed in to a fake session on
    /// account 951 that trades ES as ESZ6 and whose fill history has a
    /// long bought and sold yesterday.
    fn signed_in() -> (Rc<Store>, Rc<Window>, Rc<ChartPane>, Arc<FakeSession>, String) {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = super::super::trade::tests::window(&store);
        let es = window.index.find("ES", None).expect("ES in the curated half");
        let pane = window.focused_pane();
        window.show_in(&pane, es);
        let id = trading::add_login(&store, &Kept::default(), &super::super::trade::tests::typed()).expect("a login");
        let account =
            trading::Account { login: id.clone(), id: "951".into(), name: None, currency: Some("USD".into()), balance: None };
        let session = FakeSession::new(vec![account], Box::new(|| {}));
        session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
        let day = now() - 86_400;
        *session.past.lock().unwrap() = vec![filled("P1", Side::Buy, day, 100.0), filled("P2", Side::Sell, day + 600, 102.0)];
        let session = Arc::new(session);
        assert!(window.desk.begin(&id));
        window.desk.opened(&id, Ok(session.clone()));
        window.desk.note(&store);
        window.desk.tell();
        (store, window, pane, session, id)
    }

    /// `login` signed in again on `window`, through a fake session with
    /// accounts 951 and 952, ES as ESZ6, and a long bought and sold
    /// yesterday on 952 alone: what a chart on the default, 951, has none
    /// of.
    fn open_two(window: &Rc<Window>, store: &Store, login: &str) -> Arc<FakeSession> {
        let account = |number: &str| trading::Account {
            login: login.to_string(),
            id: number.into(),
            name: None,
            currency: Some("USD".into()),
            balance: None,
        };
        let session = FakeSession::new(vec![account("951"), account("952")], Box::new(|| {}));
        session.contracts.lock().unwrap().insert("ES".into(), "ESZ6".into());
        let day = now() - 86_400;
        let on_952 = |order: trading::Order| trading::Order { account: "952".into(), ..order };
        *session.past.lock().unwrap() =
            vec![on_952(filled("P1", Side::Buy, day, 100.0)), on_952(filled("P2", Side::Sell, day + 600, 102.0))];
        let session = Arc::new(session);
        assert!(window.desk.begin(login));
        window.desk.opened(login, Ok(session.clone()));
        window.desk.note(store);
        window.desk.tell();
        session
    }

    /// A window with ES on its chart, signed in with [`open_two`], and the
    /// login's id.
    fn two_accounts() -> (Rc<Store>, Rc<Window>, Rc<ChartPane>, String) {
        let store = Rc::new(Store::memory().expect("a store"));
        let window = super::super::trade::tests::window(&store);
        let es = window.index.find("ES", None).expect("ES in the curated half");
        let pane = window.focused_pane();
        window.show_in(&pane, es);
        let login = trading::add_login(&store, &Kept::default(), &super::super::trade::tests::typed()).expect("a login");
        open_two(&window, &store, &login);
        (store, window, pane, login)
    }

    /// Run the main loop until `done`, two seconds at most.
    fn settle(window: &Rc<Window>, done: impl Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        for _ in 0..400 {
            if done() {
                return true;
            }
            window.desk.tell();
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    }

    /// Off, the default: nothing marked and nothing asked of the trading
    /// service. On: the history's round trip, asked once, then a fill
    /// heard live as it comes; and the choice is kept with the chart.
    #[test]
    fn a_chart_marks_its_fills_only_once_asked_and_live_ones_as_they_come() {
        crate::ui::on_gtk(|| {
            let (store, window, pane, session, _) = signed_in();
            assert!(!window.shows_fills(pane.id));
            assert!(pane.view.executions().is_empty());
            assert_eq!(session.histories.load(std::sync::atomic::Ordering::Acquire), 0, "asked with fills off");

            window.set_shows_fills(pane.id, true);
            assert!(settle(&window, || pane.view.executions().marks.len() == 2), "the history never came");
            let marked = pane.view.executions();
            let ends: Vec<(f64, bool, bool, bool)> =
                marked.marks.iter().map(|mark| (mark.price, mark.buy, mark.opens, mark.closes)).collect();
            assert_eq!(ends, [(100.0, true, true, false), (102.0, false, false, true)]);
            assert_eq!(marked.trips.len(), 1);
            assert_eq!(marked.trips[0].won, Some(true));
            assert_eq!(marked.buy, omacharts_engine::theme_bars(&window.theming.borrow().theme()).up);

            // A market sell now: marked at once, opening a short.
            session
                .place(&trading::OrderRequest {
                    account: "951".into(),
                    contract: "ESZ6".into(),
                    exchange: "CME".into(),
                    side: Side::Sell,
                    qty: 1,
                    kind: OrderKind::Market,
                    price: None,
                    tag: trading::new_tag(),
                })
                .unwrap();
            window.desk.tell();
            assert!(settle(&window, || pane.view.executions().marks.len() == 3), "the live fill was not marked");
            let last = pane.view.executions().marks[2];
            assert!(!last.buy && last.opens && !last.closes, "{last:?}");
            // Where it is marked stays put as the window reads the book again.
            window.desk.tell();
            settle(&window, || false);
            assert_eq!(pane.view.executions().marks[2], last);
            assert_eq!(session.histories.load(std::sync::atomic::Ordering::Acquire), 1, "the history asked again");

            let saved = store.setting(super::super::SETTING_WORKSPACE).expect("a workspace");
            assert!(saved.contains("\"shows_fills\":true"), "{saved}");
            window.set_shows_fills(pane.id, false);
            assert!(pane.view.executions().is_empty());
            let saved = store.setting(super::super::SETTING_WORKSPACE).expect("a workspace");
            assert!(!saved.contains("shows_fills"), "off is written as nothing at all");
        });
    }

    /// A history the trading service refused is asked for again only once
    /// the session has refreshed, never on every redraw; meanwhile the
    /// fills the book has are marked all the same.
    #[test]
    fn a_refused_history_is_asked_again_after_a_refresh() {
        crate::ui::on_gtk(|| {
            let (_store, window, pane, session, login) = signed_in();
            *session.refuse.lock().unwrap() = Some("not now".into());
            window.set_shows_fills(pane.id, true);
            let asked = || session.histories.load(std::sync::atomic::Ordering::Acquire);
            assert!(settle(&window, || asked() == 1));
            settle(&window, || false);
            window.apply_marks();
            settle(&window, || false);
            assert_eq!(asked(), 1, "asked again with nothing changed");
            assert!(pane.view.executions().is_empty());

            window.desk.refresh(&login);
            assert!(settle(&window, || pane.view.executions().marks.len() == 2), "not asked again after the refresh");
            assert_eq!(asked(), 2);
        });
    }

    /// The author's chart: on account 952 with its fills shown, split, the
    /// new half maximized at another resolution. The half is on 952 too and
    /// marks the same trip, whatever its resolution and after a restart;
    /// on main it was on the default, 951, and marked nothing.
    #[test]
    fn a_split_chart_trades_on_the_account_it_was_split_from() {
        crate::ui::on_gtk(|| {
            let (store, window, pane, login) = two_accounts();
            let chosen = format!("{login}/952");
            window.set_timeframe(omacharts_engine::Timeframe::minutes(5));
            window.choose_account(pane.id, Some(chosen.clone()));
            window.set_shows_fills(pane.id, true);
            assert!(settle(&window, || pane.view.executions().marks.len() == 2), "the original marks no fills");

            window.split_focused(true);
            let added = window.focused_pane();
            assert_ne!(added.id, pane.id);
            assert_eq!(added.account.borrow().as_deref(), Some(chosen.as_str()), "the split chart is on another account");
            assert!(window.shows_fills(added.id), "the split chart does not show fills");
            assert!(settle(&window, || added.view.executions().marks.len() == 2), "the split chart marks no fills");

            // Maximized, at another resolution, and the switch turned off
            // and on there: still the same trip.
            window.toggle_maximized();
            window.set_timeframe(omacharts_engine::Timeframe::minutes(15));
            window.set_shows_fills(added.id, false);
            assert!(added.view.executions().is_empty());
            window.set_shows_fills(added.id, true);
            assert!(settle(&window, || added.view.executions().marks.len() == 2), "no fills at 15m");
            let marks: Vec<(f64, bool)> = added.view.executions().marks.iter().map(|mark| (mark.price, mark.buy)).collect();
            assert_eq!(marks, [(100.0, true), (102.0, false)]);
            assert_eq!(pane.timeframe.get(), omacharts_engine::Timeframe::minutes(5), "the original moved too");

            // After a restart.
            window.save_workspace();
            let again = super::super::trade::tests::window(&store);
            open_two(&again, &store, &login);
            let back = again.focused_pane();
            assert_eq!(back.timeframe.get(), omacharts_engine::Timeframe::minutes(15), "not the split chart");
            assert_eq!(back.account.borrow().as_deref(), Some(chosen.as_str()));
            assert!(again.shows_fills(back.id));
            assert!(settle(&again, || back.view.executions().marks.len() == 2), "no fills after a restart");
        });
    }

    /// A chart on the default account splits into one on the default, not
    /// one pinned to whichever account the default is now: change the
    /// default and both halves move to it together.
    #[test]
    fn a_chart_on_the_default_account_splits_into_one_on_the_default() {
        crate::ui::on_gtk(|| {
            let (store, window, pane, login) = two_accounts();
            window.split_focused(true);
            let added = window.focused_pane();
            assert_eq!(*added.account.borrow(), None);
            let on = |window: &Rc<Window>, pane: &ChartPane| {
                window.trading.borrow().account_of(pane.account.borrow().as_deref()).map(trading::Account::key)
            };
            assert_eq!(on(&window, &added), Some(format!("{login}/951")));
            store.set_setting(trading::DEFAULT_ACCOUNT, &format!("{login}/952"));
            window.adopt_trading();
            assert_eq!(on(&window, &pane), Some(format!("{login}/952")));
            assert_eq!(on(&window, &added), Some(format!("{login}/952")));
        });
    }
}
