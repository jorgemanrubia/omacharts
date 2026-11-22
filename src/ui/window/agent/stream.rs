//! `chart stream`: one chart's readings, a JSON object a line, as the
//! market moves it. Read off what the chart has already worked out when its
//! bars moved — the same drawn indicators `chart values` reads — so a
//! stream draws nothing and works nothing out again; with nobody streaming
//! it costs a look at an empty list.
//!
//! A snapshot first; then an update whenever the newest bar has changed,
//! at most every `--every`, the last change said once that has gone by; and
//! a close, never held back, with each bar's final readings as the next
//! begins. Another symbol or resolution, or bars read in again, starts
//! over with a snapshot.

use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};

use super::{Kind, Listener};
use crate::cli::Fault;
use crate::trading::reading::{self, StreamAsk, ValuesAsk};
use crate::ui::Window;
use crate::ui::pane::ChartPane;

/// A stream on chart `chart`.
pub(super) struct Charting {
    pub(super) chart: u32,
    want: ValuesAsk,
    every: Duration,
    /// Updates and closes still to say, when it stops after a number.
    left: Option<u64>,
    /// The symbol and resolution the chart showed when last said.
    shows: (String, String),
    seen: Seen,
    said_at: Instant,
    /// An update is due once `every` has gone by since the last.
    due: bool,
    /// Stops watching for the reader to leave the bus.
    unwatch: Option<Box<dyn FnOnce()>>,
}

impl Drop for Charting {
    fn drop(&mut self) {
        if let Some(unwatch) = self.unwatch.take() {
            unwatch();
        }
    }
}

/// The newest bar as last said: how many bars there were, when it began,
/// and its high, low, close and volume, to the bit. The same again is
/// nothing new to say: the chart moved for some other reason.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
struct Seen {
    bars: usize,
    ts: i64,
    prices: [u64; 4],
}

fn seen(pane: &ChartPane) -> Seen {
    pane.view.with_bars(|bars| match bars.last() {
        Some(bar) => Seen {
            bars: bars.len(),
            ts: bar.ts,
            prices: [bar.high.to_bits(), bar.low.to_bits(), bar.close.to_bits(), bar.volume.to_bits()],
        },
        None => Seen::default(),
    })
}

fn shows(pane: &ChartPane) -> (String, String) {
    let symbol = pane.instrument.borrow().as_ref().map(|instrument| instrument.display_symbol()).unwrap_or_default();
    (symbol, pane.view.timeframe().label())
}

/// What a stream says next.
#[derive(Clone, Copy, Debug)]
enum Say {
    Snapshot,
    Update,
    /// The bar that began at this time has closed.
    Close(i64),
}

impl Window {
    /// Start `chart stream`: the snapshot now, the rest as the chart moves.
    pub fn stream_values(self: &Rc<Self>, ask: StreamAsk) -> Result<String, Fault> {
        let sink = ask.sink.ok_or_else(|| Fault::refused("nothing to stream to: run it from a terminal".into()))?;
        // Refused as `chart values` would refuse it: a chart or an
        // indicator it does not have.
        self.read_values(ask.chart, &ask.want)?;
        let pane = self.agent_pane(ask.chart)?;
        sink.say(&format!("{}\n", self.stream_line(&pane, &ask.want, Say::Snapshot)));
        let charting = Charting {
            chart: ask.chart,
            want: ask.want,
            every: ask.every,
            left: ask.count,
            shows: shows(&pane),
            seen: seen(&pane),
            said_at: Instant::now(),
            due: false,
            unwatch: None,
        };
        let id = self.listen(sink, true, Kind::Chart(Box::new(charting)));
        if let Some(name) = ask.caller
            && let Some(bus) = self.window.application().and_then(|app| app.dbus_connection())
        {
            let this = Rc::downgrade(self);
            let watch = gio::bus_watch_name_on_connection(
                &bus,
                &name,
                gio::BusNameWatcherFlags::NONE,
                |_, _, _| {},
                move |_, _| stopped(&this, id),
            );
            let unwatch: Box<dyn FnOnce()> = Box::new(move || gio::bus_unwatch_name(watch));
            match self.listener_kind(id).as_deref_mut() {
                Some(Kind::Chart(charting)) => charting.unwatch = Some(unwatch),
                _ => unwatch(),
            }
        }
        if let Some(lasting) = ask.lasting {
            let this = Rc::downgrade(self);
            glib::timeout_add_local_once(lasting, move || stopped(&this, id));
        }
        Ok(String::new())
    }

    /// The kind of listener `id`, borrowed for a moment.
    fn listener_kind(&self, id: u64) -> Option<std::cell::RefMut<'_, Kind>> {
        let listeners = self.agent.listeners.borrow_mut();
        let at = listeners.iter().position(|listener| listener.id == id)?;
        Some(std::cell::RefMut::map(listeners, |listeners| &mut listeners[at].kind))
    }

    /// Let stream `id` go, its reader getting its prompt back.
    fn stream_stop(&self, id: u64) {
        let found: Option<Listener> = {
            let mut listeners = self.agent.listeners.borrow_mut();
            listeners.iter().position(|listener| listener.id == id).map(|at| listeners.remove(at))
        };
        if let Some(listener) = found {
            listener.sink.end(0);
        }
    }

    /// Chart `pane` moved: what each stream on it has to say.
    pub(super) fn stream_heard(self: &Rc<Self>, pane: &ChartPane) {
        let mut says = Vec::new();
        {
            let mut listeners = self.agent.listeners.borrow_mut();
            let mut now: Option<(Seen, (String, String), Instant)> = None;
            for listener in listeners.iter_mut() {
                let Kind::Chart(charting) = &mut listener.kind else { continue };
                if charting.chart != pane.id {
                    continue;
                }
                let (seen, shows, at) = now.get_or_insert_with(|| (seen(pane), shows(pane), Instant::now())).clone();
                if shows != charting.shows || seen.ts < charting.seen.ts || seen.bars < charting.seen.bars {
                    (charting.shows, charting.seen, charting.said_at, charting.due) = (shows, seen, at, false);
                    says.push((listener.id, Say::Snapshot));
                    continue;
                }
                if seen == charting.seen {
                    continue;
                }
                let closed = (seen.ts > charting.seen.ts).then_some(charting.seen.ts);
                charting.seen = seen;
                if let Some(ts) = closed {
                    says.push((listener.id, Say::Close(ts)));
                }
                let gone = at.duration_since(charting.said_at);
                if closed.is_some() || gone >= charting.every {
                    (charting.said_at, charting.due) = (at, false);
                    says.push((listener.id, Say::Update));
                } else if !charting.due {
                    charting.due = true;
                    let this = Rc::downgrade(self);
                    let id = listener.id;
                    glib::timeout_add_local_once(charting.every - gone, move || {
                        if let Some(this) = this.upgrade() {
                            this.stream_due(id);
                        }
                    });
                }
            }
        }
        for (id, say) in says {
            self.stream_say(id, pane, say);
        }
    }

    /// Stream `id`'s held-back update, once its time has come.
    fn stream_due(self: &Rc<Self>, id: u64) {
        let chart = match self.listener_kind(id).as_deref_mut() {
            Some(Kind::Chart(charting)) if charting.due => {
                (charting.due, charting.said_at) = (false, Instant::now());
                charting.chart
            }
            _ => return,
        };
        match self.agent_pane(chart) {
            Ok(pane) => self.stream_say(id, &pane, Say::Update),
            Err(_) => self.stream_gone(id),
        }
    }

    /// The chart stream `id` reads is no longer on screen.
    fn stream_gone(&self, id: u64) {
        if let Some(sink) = self.agent.listeners.borrow().iter().find(|listener| listener.id == id).map(|listener| listener.sink.clone()) {
            sink.say(&format!("{}\n", json!({"ev": "gone"})));
        }
        self.stream_stop(id);
    }

    fn stream_say(&self, id: u64, pane: &ChartPane, say: Say) {
        let found = self.agent.listeners.borrow().iter().find_map(|listener| match &listener.kind {
            Kind::Chart(charting) if listener.id == id => Some((listener.sink.clone(), charting.want.clone())),
            _ => None,
        });
        let Some((sink, want)) = found else { return };
        sink.say(&format!("{}\n", self.stream_line(pane, &want, say)));
        if matches!(say, Say::Snapshot) {
            return;
        }
        let over = match self.listener_kind(id).as_deref_mut() {
            Some(Kind::Chart(charting)) => match &mut charting.left {
                Some(left) => {
                    *left = left.saturating_sub(1);
                    *left == 0
                }
                None => false,
            },
            _ => false,
        };
        if over {
            self.stream_stop(id);
        }
    }

    /// One line of a stream, as JSON.
    fn stream_line(&self, pane: &ChartPane, want: &ValuesAsk, say: Say) -> Value {
        let places = reading::places(pane.view.tick());
        let at = pane.view.with_bars(|bars| match say {
            Say::Close(ts) => bars.partition_point(|bar| bar.ts < ts),
            Say::Snapshot | Say::Update => bars.len().saturating_sub(1),
        });
        let bar = pane.view.with_bars(|bars| bars.get(at).map(|bar| reading::bar_json(bar, places)));
        let indicators = self.readings_at(pane, want, at, places);
        match say {
            Say::Snapshot => {
                let (symbol, resolution) = shows(pane);
                json!({"ev": "snapshot", "chart": self.chart_named(pane), "symbol": symbol, "resolution": resolution, "bar": bar, "indicators": indicators})
            }
            Say::Update => json!({"ev": "update", "bar": bar, "indicators": indicators}),
            Say::Close(_) => json!({"ev": "close", "bar": bar, "indicators": indicators}),
        }
    }

    /// What each indicator `want` names reads at bar `at`, as
    /// `chart values --last 1` writes a bar of it.
    fn readings_at(&self, pane: &ChartPane, want: &ValuesAsk, at: usize, places: usize) -> Vec<Value> {
        use omacharts_chart::BarPick;
        use omacharts_engine::indicators::{Kind, NEEDS_VOLUME_AT_PRICE, Output};
        let view = &pane.view;
        let timeframe = view.timeframe();
        let Some(ts) = view.with_bars(|bars| bars.get(at).map(|bar| bar.ts)) else { return Vec::new() };
        let footprint = |mut fields: Value| {
            match view.read_footprint(BarPick::Between(ts, ts + 1), want.row_ticks) {
                Some(read) => {
                    if let (Some(fields), Value::Object(rows)) = (fields.as_object_mut(), reading::footprint_json(&read, places)) {
                        fields.extend(rows);
                    }
                }
                None => fields["unmet"] = json!(NEEDS_VOLUME_AT_PRICE),
            }
            fields
        };
        let mut out = Vec::new();
        let mut footprinted = false;
        view.with_indicators(|drawn| {
            for one in drawn {
                let (id, kind) = (one.indicator.id, one.indicator.kind);
                if !want.wants(id, kind) || matches!(one.output, Output::Time(_)) {
                    continue;
                }
                let mut fields = json!({"id": id, "kind": kind.key(), "label": one.indicator.label_for(timeframe), "visible": one.indicator.visible});
                let read = match &one.output {
                    Output::Unmet(why) => Some(json!({"unmet": why})),
                    Output::Footprint => {
                        footprinted = true;
                        out.push(footprint(fields));
                        continue;
                    }
                    Output::LargeOrders => Some(json!({
                        "orders": view.read_large_orders(id, at..at + 1).map(|(orders, tick)| reading::orders_json(&orders, tick, places)),
                    })),
                    Output::Profiles(_) | Output::MarketProfiles(_) => {
                        Some(json!({"profiles": reading::profiles_json(&one.output, at..at + 1, places)}))
                    }
                    output => reading::reading_at(kind, output, at, places),
                };
                if let (Some(fields), Some(Value::Object(read))) = (fields.as_object_mut(), read) {
                    fields.extend(read);
                }
                out.push(fields);
            }
        });
        if want.kinds.contains(&Kind::Footprint) && !footprinted {
            out.push(footprint(json!({"id": null, "kind": Kind::Footprint.key(), "label": "Footprint", "visible": false})));
        }
        out
    }
}

/// Stream `id` is over: its time is up, or its reader has gone.
fn stopped(this: &Weak<Window>, id: u64) {
    if let Some(this) = this.upgrade() {
        this.stream_stop(id);
    }
}
