//! A chart subscription as candles: a snapshot, then the candles each patch
//! touched, until it is let go of or the connection fails under it.
//!
//! One thread per stream, blocked on the gateway's frames. It makes one
//! attempt: it joins the process's connection (building it if this is the
//! first thing to ask), sends the chart request, and turns what comes back
//! into [`Event`]s until the socket dies or the gateway refuses — at which
//! point it says so, once, and ends. Whether and when to try again is the
//! caller's: it knows how many charts are waiting and how long they have
//! been, and this thread knows only one socket.
//!
//! What a tick costs here is what the wire taught: a patch names the
//! candle indices it changed, so the changed candles are read straight off
//! the document's arrays and twenty sessions of candles are never walked
//! for a close that moved.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::Value;

use crate::client::Interrupter;
use crate::error::Error;
use crate::protocol::Response;
use crate::services::chart::{chart_request, ChartBody, ChartParams};
use crate::{candles_of, lost, market, Candle, Market};

/// What a stream delivers.
#[derive(Debug)]
pub enum Event {
    /// The whole series, oldest first: the first thing a stream delivers,
    /// and again whenever the gateway re-sends the document.
    Snapshot(Vec<Candle>),
    /// The candles a patch changed, oldest first — the forming bar, or the
    /// bar that just closed and the one that opened after it.
    Candles(Vec<Candle>),
    /// The stream has ended and this is why. Nothing follows.
    Lost(Error),
}

/// An open stream. Dropping it ends the thread without waiting for it.
pub struct Stream {
    stopped: Arc<AtomicBool>,
    interrupt: Arc<Mutex<Option<Interrupter>>>,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(interrupter) = self.interrupt.lock().unwrap_or_else(|e| e.into_inner()).take() {
            interrupter.interrupt();
        }
    }
}

/// Start streaming `params`, folding candles into `fold`-second buckets if
/// asked, and hand each event to `on` from the stream's own thread.
///
/// Returns at once. Everything that can fail — no session, a gateway that
/// is down, a symbol it does not know — arrives as [`Event::Lost`].
pub fn stream(params: ChartParams, fold: Option<i64>, on: Box<dyn FnMut(Event) + Send>) -> Stream {
    let stopped = Arc::new(AtomicBool::new(false));
    let interrupt: Arc<Mutex<Option<Interrupter>>> = Arc::new(Mutex::new(None));
    let handle = Stream { stopped: stopped.clone(), interrupt: interrupt.clone() };
    let name = format!("tos {} {}", params.symbol, params.time_aggregation);
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        let mut on = on;
        match market() {
            Ok(market) => serve(market, &params, fold, &mut on, &stopped, &interrupt),
            Err(error) => on(Event::Lost(error)),
        }
    });
    if let Err(error) = spawned {
        eprintln!("tos-market: could not start a stream thread: {error}");
    }
    handle
}

/// The thread's life: one subscription on the connection `market` holds,
/// replaced once if it has died, served until it ends.
fn serve(
    market: &Market,
    params: &ChartParams,
    fold: Option<i64>,
    on: &mut dyn FnMut(Event),
    stopped: &AtomicBool,
    interrupt: &Mutex<Option<Interrupter>>,
) {
    let request = chart_request(params);
    let (generation, client) = market.current();
    let mut subscription = match client.subscribe(request.clone()) {
        Ok(subscription) => subscription,
        // The connection this process holds has died since anybody last
        // used it. One replacement, shared with every other stream that
        // finds the same thing.
        Err(error) if lost(&error) => {
            match market.replace(generation).and_then(|fresh| fresh.subscribe(request)) {
                Ok(subscription) => subscription,
                Err(error) => return on(Event::Lost(error)),
            }
        }
        Err(error) => return on(Event::Lost(error)),
    };
    *interrupt.lock().unwrap_or_else(|e| e.into_inner()) = Some(subscription.interrupter());
    if stopped.load(Ordering::SeqCst) {
        return;
    }

    // The first frame is the whole document whatever the gateway calls it:
    // on a connection already streaming this id it is a patch, and the
    // document behind it is the one the previous subscriber left current.
    let mut first = true;
    loop {
        match subscription.next() {
            Ok(response) if response.is_error() => {
                return on(Event::Lost(response.into_result().unwrap_err()));
            }
            Ok(response) => {
                if first || response.is_whole_document() {
                    first = false;
                    on(Event::Snapshot(folded(whole(&response), fold)));
                } else {
                    let changed = changed(&response, fold);
                    if !changed.is_empty() {
                        on(Event::Candles(changed));
                    }
                }
            }
            Err(Error::Interrupted) => return,
            Err(error) => {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                // The gateway will not snapshot this id on this connection.
                // A fresh connection is the only fix, and it is made here so
                // that the caller's next attempt lands on it rather than on
                // the same answer.
                if matches!(error, Error::NoSnapshot(_)) {
                    let _ = market.replace(generation);
                }
                return on(Event::Lost(error));
            }
        }
    }
}

/// Every candle in the document.
fn whole(response: &Response) -> Vec<Candle> {
    // Deserialised in place rather than through a clone of the document:
    // this runs once per snapshot, but a snapshot is the big one.
    match ChartBody::deserialize(&*response.body) {
        Ok(body) => candles_of(&body),
        Err(error) => {
            eprintln!("tos-market: {}: a snapshot this could not read: {error}", response.id);
            Vec::new()
        }
    }
}

/// The candles a patch changed, read off the document's arrays by the
/// indices the patch named.
///
/// A path is `/candles/<field>/<index>`, or `/candles/<field>/-` for a
/// candle the patch appended — which, once every array has had its append,
/// is the last one. Anything under `/candles` that is not of that shape
/// changed the whole series and is reported as such by the caller through
/// [`Response::is_whole_document`]; here it is simply every index.
fn changed(response: &Response, fold: Option<i64>) -> Vec<Candle> {
    let Some(candles) = response.body.get("candles") else {
        return Vec::new();
    };
    let count = candles
        .get("timestamps")
        .and_then(Value::as_array)
        .map(|stamps| stamps.len())
        .unwrap_or(0);
    if count == 0 {
        return Vec::new();
    }
    let mut indices: Vec<usize> = Vec::with_capacity(2);
    for path in response.touched.iter() {
        let Some(rest) = path.strip_prefix("/candles/") else {
            continue;
        };
        let index = match rest.split_once('/') {
            Some((_, "-")) => count - 1,
            Some((_, index)) => match index.parse::<usize>() {
                Ok(index) if index < count => index,
                _ => continue,
            },
            // A whole array was replaced: every candle may have changed.
            None => {
                indices = (0..count).collect();
                break;
            }
        };
        if !indices.contains(&index) {
            indices.push(index);
        }
    }
    indices.sort_unstable();
    match fold {
        None => indices.iter().filter_map(|&i| candle_at(candles, i)).collect(),
        Some(bucket) => refolded(candles, &indices, bucket),
    }
}

/// The folded bar for each bucket a changed index falls in, rebuilt from
/// every native candle in that bucket.
fn refolded(candles: &Value, indices: &[usize], bucket: i64) -> Vec<Candle> {
    let mut out: Vec<Candle> = Vec::with_capacity(indices.len());
    for &index in indices {
        let Some(changed) = candle_at(candles, index) else { continue };
        let start = changed.ts.div_euclid(bucket) * bucket;
        if out.last().is_some_and(|last| last.ts == start) {
            continue;
        }
        // The bucket's candles are contiguous around the changed one.
        let mut members = Vec::new();
        let mut at = index;
        while at > 0 && candle_at(candles, at - 1).is_some_and(|c| c.ts >= start) {
            at -= 1;
        }
        while let Some(candle) = candle_at(candles, at) {
            if candle.ts >= start + bucket {
                break;
            }
            members.push(candle);
            at += 1;
        }
        out.extend(fold(members, bucket));
    }
    out
}

/// One candle off the parallel arrays, if every array reaches `index` and
/// the prices are numbers.
fn candle_at(candles: &Value, index: usize) -> Option<Candle> {
    let number = |field: &str| candles.get(field)?.get(index)?.as_f64();
    let ts = (number("timestamps")? / 1000.0) as i64;
    let (open, high, low, close) = (number("opens")?, number("highs")?, number("lows")?, number("closes")?);
    if ![open, high, low, close].iter().all(|p| p.is_finite()) {
        return None;
    }
    // Indexes send `null` volumes; a candle with no volume is still a candle.
    let volume = number("volumes").filter(|v| v.is_finite()).unwrap_or(0.0);
    Some(Candle { ts, open, high, low, close, volume })
}

/// Fold candles into `bucket`-second bars, for the resolutions the gateway
/// has no code of its own for. `None` and a bucket of zero leave them alone.
pub fn fold(mut candles: Vec<Candle>, bucket: i64) -> Vec<Candle> {
    if bucket <= 0 || candles.is_empty() {
        return candles;
    }
    candles.sort_by_key(|candle| candle.ts);
    let mut out: Vec<Candle> = Vec::with_capacity(candles.len());
    for candle in candles {
        let start = candle.ts.div_euclid(bucket) * bucket;
        match out.last_mut() {
            Some(last) if last.ts == start => {
                last.high = last.high.max(candle.high);
                last.low = last.low.min(candle.low);
                last.close = candle.close;
                last.volume += candle.volume;
            }
            _ => out.push(Candle { ts: start, ..candle }),
        }
    }
    out
}

fn folded(candles: Vec<Candle>, bucket: Option<i64>) -> Vec<Candle> {
    match bucket {
        Some(bucket) => fold(candles, bucket),
        None => candles,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use tungstenite::Message;

    use super::*;
    use crate::client::fake::{gateway, log_in, Session, EXPIRED, ONE_BAR};

    fn env_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tos-market-stream-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("tos.env")
    }

    fn market_at(tag: &str, url: &str) -> Market {
        let path = env_path(tag);
        std::fs::write(&path, format!("TOS_PAPER_ACCESS_TOKEN=tok\nTOS_PAPER_GATEWAY_URL={url}\n"))
            .unwrap();
        Market::connect(&path).expect("a connection to the fake gateway")
    }

    /// Serve `params` on `market` from a thread of its own, collecting
    /// events on a channel, and hand back the handle that stops it.
    fn streaming(
        market: &'static Market,
        params: ChartParams,
        fold: Option<i64>,
    ) -> (Stream, mpsc::Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let interrupt: Arc<Mutex<Option<Interrupter>>> = Arc::new(Mutex::new(None));
        let handle = Stream { stopped: stopped.clone(), interrupt: interrupt.clone() };
        std::thread::spawn(move || {
            let mut on = move |event| {
                let _ = tx.send(event);
            };
            serve(market, &params, fold, &mut on, &stopped, &interrupt);
        });
        (handle, rx)
    }

    fn next(rx: &mpsc::Receiver<Event>) -> Event {
        rx.recv_timeout(Duration::from_secs(10)).expect("an event in time")
    }

    const TWO_BARS: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":1,"type":"snapshot"},"body":{"symbol":"/ES","candles":{"opens":[1.0,2.0],"highs":[1.5,2.5],"lows":[0.5,1.5],"closes":[1.2,2.2],"volumes":[7.0,8.0],"timestamps":[300000.0,600000.0]}}}]}"#;

    /// What the wire sends for a close that moved.
    const TICK: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":1,"type":"patch"},"body":{"patches":[{"op":"replace","path":"/candles/volumes/1","value":9.0},{"op":"replace","path":"/candles/closes/1","value":2.4}]}}]}"#;

    /// And for a bar closing and the next one opening: the closed bar's
    /// final values and the new bar appended to every array, in one frame.
    const NEW_BAR: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":1,"type":"patch"},"body":{"patches":[{"op":"add","path":"/candles/volumes/-","value":1.0},{"op":"replace","path":"/candles/volumes/1","value":10.0},{"op":"add","path":"/candles/closes/-","value":3.0},{"op":"replace","path":"/candles/closes/1","value":2.3},{"op":"add","path":"/candles/lows/-","value":3.0},{"op":"add","path":"/candles/highs/-","value":3.0},{"op":"add","path":"/candles/opens/-","value":3.0},{"op":"add","path":"/candles/timestamps/-","value":900000.0}]}}]}"#;

    /// A changed range on a live id arrives like this.
    const WHOLE: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN5","ver":1,"type":"patch"},"body":{"patches":[{"op":"replace","path":"","value":{"symbol":"/ES","candles":{"opens":[5.0],"highs":[5.0],"lows":[5.0],"closes":[5.0],"volumes":[1.0],"timestamps":[1200000.0]}}}]}}]}"#;

    fn session(frames: &'static [&'static str]) -> Session {
        Box::new(move |ws| {
            log_in(ws);
            ws.read().unwrap();
            for frame in frames {
                ws.send(Message::text(*frame)).unwrap();
            }
        })
    }

    /// The whole life of a stream against the wire shapes recorded from the
    /// gateway: a snapshot, a tick, a bar boundary, a whole-document patch.
    #[test]
    fn a_stream_delivers_the_snapshot_and_then_what_each_patch_touched() {
        let (url, server) = gateway(vec![session(&[TWO_BARS, TICK, NEW_BAR, WHOLE])]);
        let market: &'static Market = Box::leak(Box::new(market_at("life", &url)));
        let (handle, rx) = streaming(market, ChartParams::new("/ES", "MIN5", "DAY1"), None);

        let Event::Snapshot(bars) = next(&rx) else { panic!("a snapshot first") };
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[1].ts, 600);

        // The tick names one candle, and that candle alone comes through,
        // with the values the patch left it with.
        let Event::Candles(changed) = next(&rx) else { panic!("the changed candle") };
        assert_eq!(changed.len(), 1);
        assert_eq!((changed[0].ts, changed[0].close, changed[0].volume), (600, 2.4, 9.0));

        // The boundary: the closed bar and the new one, oldest first.
        let Event::Candles(changed) = next(&rx) else { panic!("two candles") };
        assert_eq!(changed.iter().map(|c| c.ts).collect::<Vec<_>>(), vec![600, 900]);
        assert_eq!(changed[0].close, 2.3);
        assert_eq!(changed[1].open, 3.0);

        // The gateway re-sent the document: it is a snapshot to us.
        let Event::Snapshot(bars) = next(&rx) else { panic!("a whole document is a snapshot") };
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].ts, 1_200);

        drop(handle);
        market.current().1.disconnect();
        server.join().unwrap();
    }

    /// Ninety-minute bars are the gateway's half-hour bars folded three at
    /// a time, and a tick on one half hour has to come out as the whole
    /// ninety-minute bar it belongs to, rebuilt — not as a half-hour bar.
    #[test]
    fn a_folded_stream_rebuilds_the_bucket_a_tick_lands_in() {
        // Three half-hour bars from 00:00, 00:30, 01:00 (ms timestamps).
        const THREE: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN30","ver":1,"type":"snapshot"},"body":{"symbol":"/ES","candles":{"opens":[1.0,2.0,3.0],"highs":[1.0,2.0,3.0],"lows":[1.0,2.0,3.0],"closes":[1.0,2.0,3.0],"volumes":[1.0,1.0,1.0],"timestamps":[0.0,1800000.0,3600000.0]}}}]}"#;
        const TICK: &str = r#"{"payload":[{"header":{"service":"chart","id":"chart-/ES-MIN30","ver":1,"type":"patch"},"body":{"patches":[{"op":"replace","path":"/candles/highs/2","value":9.0},{"op":"replace","path":"/candles/closes/2","value":8.0}]}}]}"#;
        let (url, server) = gateway(vec![session(&[THREE, TICK])]);
        let market: &'static Market = Box::leak(Box::new(market_at("fold", &url)));
        let (handle, rx) =
            streaming(market, ChartParams::new("/ES", "MIN30", "DAY1"), Some(90 * 60));

        let Event::Snapshot(bars) = next(&rx) else { panic!("a snapshot first") };
        assert_eq!(bars.len(), 1, "three half hours are one ninety-minute bar");
        assert_eq!((bars[0].open, bars[0].close, bars[0].volume), (1.0, 3.0, 3.0));

        let Event::Candles(changed) = next(&rx) else { panic!("the refolded bar") };
        assert_eq!(changed.len(), 1);
        assert_eq!((changed[0].ts, changed[0].high, changed[0].close, changed[0].volume), (0, 9.0, 8.0, 3.0));

        drop(handle);
        market.current().1.disconnect();
        server.join().unwrap();
    }

    /// A session that dies under the stream ends it with the gateway's own
    /// reason — the one somebody can act on — and nothing after it.
    #[test]
    fn a_stream_that_loses_its_session_says_so_and_ends() {
        let (url, server) = gateway(vec![session(&[ONE_BAR, EXPIRED])]);
        let market: &'static Market = Box::leak(Box::new(market_at("expired", &url)));
        let (handle, rx) = streaming(market, ChartParams::new("/ES", "MIN5", "DAY1"), None);
        assert!(matches!(next(&rx), Event::Snapshot(_)));
        let Event::Lost(error) = next(&rx) else { panic!("a loss") };
        assert!(matches!(error, Error::Login(_)), "{error}");
        assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "nothing after a loss");
        drop(handle);
        server.join().unwrap();
    }

    /// Letting go of a stream ends its thread at once, not on the next
    /// frame — which with the market shut could be hours away.
    #[test]
    fn dropping_the_handle_ends_the_thread_without_waiting_for_a_frame() {
        let (url, server) = gateway(vec![session(&[ONE_BAR])]);
        let market: &'static Market = Box::leak(Box::new(market_at("drop", &url)));
        let (handle, rx) = streaming(market, ChartParams::new("/ES", "MIN5", "DAY1"), None);
        assert!(matches!(next(&rx), Event::Snapshot(_)));
        drop(handle);
        // The thread's end is the channel closing, and no loss is reported.
        match rx.recv_timeout(Duration::from_secs(5)) {
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
            other => panic!("expected the thread to end quietly, got {other:?}"),
        }
        market.current().1.disconnect();
        server.join().unwrap();
    }

    /// The connection this process holds has died since anybody last used
    /// it. A stream started afterwards replaces it rather than failing on it.
    #[test]
    fn a_stream_started_on_a_dead_connection_replaces_it() {
        let (url, server) = gateway(vec![
            Box::new(|ws| {
                log_in(ws);
            }),
            session(&[ONE_BAR]),
        ]);
        let market: &'static Market = Box::leak(Box::new(market_at("replace", &url)));
        // Kill the first connection and wait for its actor to go.
        let (generation, first) = market.current();
        first.disconnect();
        std::thread::sleep(Duration::from_millis(300));
        let (handle, rx) = streaming(market, ChartParams::new("/ES", "MIN5", "DAY1"), None);
        assert!(matches!(next(&rx), Event::Snapshot(_)));
        assert_ne!(market.current().0, generation, "a fresh connection was made");
        drop(handle);
        market.current().1.disconnect();
        server.join().unwrap();
    }

    #[test]
    fn candles_are_read_off_the_arrays_by_index() {
        let doc: Value = serde_json::from_str(
            r#"{"candles":{"opens":[1.0,2.0],"highs":[1.5,2.5],"lows":[0.5,1.5],"closes":[1.2,2.2],"volumes":[7.0,null],"timestamps":[300000.0,600000.0]}}"#,
        )
        .unwrap();
        let candles = &doc["candles"];
        assert_eq!(candle_at(candles, 1).map(|c| (c.ts, c.volume)), Some((600, 0.0)));
        assert!(candle_at(candles, 2).is_none());
    }
}
