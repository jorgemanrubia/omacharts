// The candle functions are the only production callers. Tests cover the rest
// of the session client, which the lib build otherwise reports as dead.
#![allow(dead_code)]

//! Chart candles from a thinkorswim session.
//!
//! One connection for the process. A chart request returns its first
//! snapshot. The live gateway is refused.

mod client;
mod config;
mod error;
mod patch;
mod protocol;
mod redact;
mod services;
mod session;
mod tsm;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use client::Client;
use config::TradingSystem;
use services::chart::{ChartBody, ChartParams};
use session::BrowserSession;

pub use error::{Error, Result};

/// One candle. `ts` is the unix second the bar opens at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// `TOS_ENV_FILE`: the session captured in a dotenv file.
pub fn session_file() -> Result<PathBuf> {
    std::env::var("TOS_ENV_FILE")
        .ok()
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            Error::Other(
                "no thinkorswim session; set TOS_ENV_FILE to a session file".into(),
            )
        })
}

/// Candles for one symbol, oldest first. `aggregation` and `range` are the
/// platform's own codes (`MIN5`, `DAY`, `DAY1`, `YEAR2`).
pub fn candles(symbol: &str, aggregation: &str, range: &str) -> Result<Vec<Candle>> {
    market()?.snapshot(symbol, aggregation, range)
}

fn market() -> Result<&'static Market> {
    static CELL: OnceLock<Market> = OnceLock::new();
    if let Some(ready) = CELL.get() {
        return Ok(ready);
    }
    let env = session_file()?;
    let built = Market::connect(&env)?;
    Ok(CELL.get_or_init(|| built))
}

struct Market {
    runtime: tokio::runtime::Runtime,
    client: Mutex<Client>,
    env: PathBuf,
}

impl Market {
    fn connect(env: &Path) -> Result<Self> {
        let session = BrowserSession::load_for(env, TradingSystem::PaperMoney)
            .filter(|s| s.trading_system == TradingSystem::PaperMoney)
            .ok_or_else(|| {
                Error::Other(format!(
                    "no thinkorswim session in {}",
                    env.display()
                ))
            })?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| Error::Other(e.to_string()))?;
        // `false`: a chart feed does not open the live gateway.
        let (client, _) = runtime.block_on(session.connect(env, false))?;
        Ok(Market {
            runtime,
            client: Mutex::new(client),
            env: env.to_path_buf(),
        })
    }

    fn snapshot(&self, symbol: &str, aggregation: &str, range: &str) -> Result<Vec<Candle>> {
        let client = self
            .client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        match self
            .runtime
            .block_on(fetch(&client, symbol, aggregation, range))
        {
            Err(error) if lost(&error) => {
                let (fresh, _) = self.runtime.block_on(reconnect(&self.env))?;
                *self.client.lock().unwrap_or_else(|e| e.into_inner()) = fresh.clone();
                self.runtime
                    .block_on(fetch(&fresh, symbol, aggregation, range))
            }
            other => other,
        }
    }
}

impl Drop for Market {
    fn drop(&mut self) {
        if let Ok(client) = self.client.lock() {
            client.disconnect();
        }
    }
}

fn reconnect(
    env: &Path,
) -> impl std::future::Future<Output = Result<(Client, BrowserSession)>> + '_ {
    let session = BrowserSession::load_for(env, TradingSystem::PaperMoney);
    async move {
        let session = session.ok_or_else(|| {
            Error::Other(format!("no thinkorswim session in {}", env.display()))
        })?;
        session.connect(env, false).await
    }
}

async fn fetch(
    client: &Client,
    symbol: &str,
    aggregation: &str,
    range: &str,
) -> Result<Vec<Candle>> {
    let mut sub = client.chart(&ChartParams::new(symbol, aggregation, range))?;
    let res = tokio::time::timeout(Duration::from_secs(30), sub.next())
        .await
        .map_err(|_| Error::Timeout("chart".into()))?
        .ok_or_else(|| Error::Timeout("chart".into()))?;
    let body: ChartBody = serde_json::from_value(res.into_result()?.body)?;
    Ok(candles_of(&body))
}

fn lost(error: &Error) -> bool {
    matches!(
        error,
        Error::Timeout(_) | Error::Closed | Error::ConnectionLost { .. }
    )
}

/// Parallel candle arrays from a chart snapshot, oldest first.
pub fn candles_of(body: &ChartBody) -> Vec<Candle> {
    let c = &body.candles;
    let mut out = Vec::with_capacity(c.len());
    for i in 0..c.len() {
        let (open, high, low, close) = (c.opens[i], c.highs[i], c.lows[i], c.closes[i]);
        if ![open, high, low, close].iter().all(|p| p.is_finite()) {
            continue;
        }
        let volume = c.volumes.get(i).copied().unwrap_or(0.0);
        out.push(Candle {
            ts: (c.timestamps[i] / 1000.0) as i64,
            open,
            high,
            low,
            close,
            volume: if volume.is_finite() { volume } else { 0.0 },
        });
    }
    out.sort_by_key(|bar| bar.ts);
    out.dedup_by_key(|bar| bar.ts);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::services::chart::ChartBody;

    #[test]
    fn a_snapshot_becomes_oldest_first_candles() {
        let body: ChartBody = serde_json::from_value(json!({
            "symbol": "/ES",
            "candles": {
                "opens": [2.0, 1.0],
                "highs": [2.5, 1.5],
                "lows": [1.5, 0.5],
                "closes": [2.2, 1.2],
                "volumes": [10.0, null],
                "timestamps": [2_000_000.0, 1_000_000.0]
            }
        }))
        .unwrap();
        let bars = candles_of(&body);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].ts, 1_000);
        assert_eq!(bars[0].volume, 0.0);
        assert_eq!(bars[1].close, 2.2);
    }

    /// Live chart. Point `TOS_ENV_FILE` at a session file.
    #[test]
    #[ignore]
    fn chart_snapshot() {
        let es = candles("/ES", "MIN5", "DAY1").expect("/ES");
        assert!(es.len() > 10, "/ES bars: {}", es.len());
        let aapl = candles("AAPL", "DAY", "MONTH6").expect("AAPL");
        assert!(aapl.len() > 20, "AAPL bars: {}", aapl.len());
        let hour = candles("/ES", "HOUR1", "DAY1").expect("HOUR1");
        assert!(hour.len() > 5, "HOUR1 bars: {}", hour.len());
        let spx = candles("SPX", "DAY", "MONTH6").expect("SPX");
        assert!(spx.len() > 20, "SPX bars: {}", spx.len());
        eprintln!(
            "/ES MIN5 {} bars, AAPL DAY {} bars, HOUR1 {} bars, SPX {} bars",
            es.len(),
            aapl.len(),
            hour.len(),
            spx.len()
        );
    }
}
