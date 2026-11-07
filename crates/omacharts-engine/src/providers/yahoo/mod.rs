//! Yahoo Finance, via the chart endpoint.
//!
//! `v8/finance/chart` is used for everything, quotes included — the older
//! `v7/finance/quote` now needs a cookie and crumb handshake, and this one
//! needs nothing. No key, no account, every instrument type we care about.
//!
//! Two things to respect. The data is delayed — ten minutes for futures,
//! fifteen for indexes — so the UI must say so. And the rate limiting is
//! aggressive: a single unlucky request can leave an address blocked for
//! every subsequent call. The cache above this layer is the real defence;
//! this layer's job is to ask for as little as possible and to report
//! throttling clearly rather than retrying into a wall.
//!
//! Nothing here waits. How far apart requests must be kept, how long to stay
//! away after a 429 and whether a failure is worth another try are rules
//! this module states through [`Provider`]; the queue that owns the requests
//! applies them, because it is the only thing that knows what else is
//! waiting. A sleep in here would overrule that queue from inside a call it
//! knows nothing about — which is exactly how a chart somebody had just
//! clicked came to wait two seconds for a warm-up nobody asked for.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::bars::{Bar, Timeframe, Unit};
use crate::provider::{Capability, Closes, Pacing, Provider, ProviderError, SharedCooldown};
use crate::exchanges::registry;
use crate::symbols::{Instrument, InstrumentKind};

const ENDPOINT: &str = "https://query1.finance.yahoo.com/v8/finance/chart";

/// Daily closes for many symbols in one request. Like the chart endpoint it
/// wants no cookie and no crumb.
const SPARK: &str = "https://query1.finance.yahoo.com/v8/finance/spark";

/// How many symbols one spark request may name. Twenty come back whole;
/// fifty are refused outright with a 400 (measured 2026-10-15).
const MAX_BATCH: usize = 20;

/// Yahoo serves nothing without a browser-shaped agent.
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
                     (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// What Yahoo serves directly, and how far back each goes.
///
/// Anything else is folded from one of these by the caller, which is why a
/// three-minute chart works at all.
const CAPABILITIES: &[Capability] = &[
    // One-minute bars are kept for a month but served a week at a time, and
    // asking for the month returns an empty series rather than an error.
    Capability {
        timeframe: Timeframe::minutes(1),
        history_days: Some(30),
        max_request_days: Some(7),
    },
    Capability { timeframe: Timeframe::minutes(2), history_days: Some(60), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(5), history_days: Some(60), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(15), history_days: Some(60), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(30), history_days: Some(60), max_request_days: None },
    Capability { timeframe: Timeframe::minutes(90), history_days: Some(60), max_request_days: None },
    Capability { timeframe: Timeframe::hours(1), history_days: Some(730), max_request_days: None },
    Capability { timeframe: Timeframe::days(1), history_days: None, max_request_days: None },
];

/// How far inside its own limit to ask.
///
/// "Within the last 60 days" turns out to mean strictly within: asking for
/// exactly sixty days of fifteen-minute bars is refused outright — not an
/// empty series, a 422 — which is how DIA ended up blank at 15m while every
/// other resolution worked. The boundary is not even consistent between
/// intervals; five-minute bars accept the same window that fifteen-minute bars
/// reject.
///
/// Six hours of margin costs nothing measurable. The oldest bars in that
/// window are overnight ones that do not exist, so the same request comes back
/// with the same count either way.
const WINDOW_MARGIN: i64 = 6 * 3_600;

/// Smallest gap between two requests the user is waiting on.
///
/// Yahoo tolerates a steady trickle and punishes bursts, and nothing here
/// needs to be fast about fetching — the cache is what makes it feel fast.
const MIN_GAP: Duration = Duration::from_millis(350);

/// Smallest gap between two requests nobody asked for.
///
/// Speculative work gets a much longer leash. Filling a watchlist is a
/// one-time cost per symbol — the bars are kept forever — so there is no
/// reason to spend the request budget quickly, and every reason not to.
const MIN_GAP_SPECULATIVE: Duration = Duration::from_millis(2_000);

/// The two gaps as the queue reads them. See [`Provider::pacing`].
const PACING: Pacing = Pacing { min_gap: MIN_GAP, min_gap_speculative: MIN_GAP_SPECULATIVE };

/// How long to stop asking entirely after a 429, and the ceiling that repeated
/// throttling climbs to.
const COOLDOWN_START: Duration = Duration::from_secs(60);
const COOLDOWN_MAX: Duration = Duration::from_secs(900);

/// Transient failures worth one more try. Deliberately small: a chart that
/// paints from cache has nothing to gain from a third attempt.
const RETRIES: u32 = 2;

/// How long to leave a failed request before trying it again, doubling.
const BACKOFF: Duration = Duration::from_millis(400);

/// Whether Yahoo has told us to go away, and for how long it would next time.
///
/// State, not timing. This remembers what Yahoo said and reports until when
/// it stands; sitting it out is the queue's job, and the queue refuses
/// everything — speculative or not — until then. Letting the chart somebody
/// is looking at through would mean asking again inside the window Yahoo
/// just told us to stay out of, which is how a minute's cooldown becomes a
/// quarter of an hour's.
struct Cooldown {
    until: Option<Instant>,
    /// What the next 429 would cost. Doubles each time, relaxes on success.
    length: Duration,
}

impl Cooldown {
    fn new() -> Cooldown {
        Cooldown { until: None, length: COOLDOWN_START }
    }

    /// Back off harder each time Yahoo says no.
    fn throttled(&mut self, now: Instant) {
        self.until = Some(now + self.length);
        self.length = (self.length * 2).min(COOLDOWN_MAX);
    }

    /// And relax once it says yes.
    fn succeeded(&mut self) {
        self.length = COOLDOWN_START;
    }

    /// This cooldown in wall-clock terms, at `now` (`unix` seconds), or
    /// `None` when there is nothing to hand on: none running, none climbing.
    ///
    /// One that has run out keeps the time it ran out, not `now`: that is
    /// what a reader measures its quiet spell from, and restamping it on
    /// every request would keep a climbing backoff from ever being forgotten.
    fn shared(&self, now: Instant, unix: i64) -> Option<SharedCooldown> {
        let running = self.until.is_some_and(|until| until > now);
        if !running && self.length == COOLDOWN_START {
            return None;
        }
        let secs = |d: Duration| i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
        let until = match self.until {
            // Rounded up, so that another process never reads it as over a
            // moment early.
            Some(until) if running => {
                let left = until - now;
                unix.saturating_add(secs(left)).saturating_add(i64::from(left.subsec_nanos() > 0))
            }
            Some(until) => unix.saturating_sub(secs(now - until)),
            None => unix,
        };
        Some(SharedCooldown { until, next: self.length })
    }

    /// Carry on from what another process handed on, keeping whichever of
    /// the two is stricter.
    ///
    /// Never for longer than [`COOLDOWN_MAX`]: the row it came from can say
    /// anything, from `config set` or from a clock that has since been put
    /// back, and no cooldown of Yahoo's runs longer than that.
    fn resume(&mut self, shared: SharedCooldown, now: Instant, unix: i64) {
        let left = shared.until.saturating_sub(unix);
        let until = if left > 0 {
            let left = Duration::from_secs(u64::try_from(left).unwrap_or(u64::MAX));
            Some(now + left.min(COOLDOWN_MAX))
        } else {
            // Already run out: kept as when it did, for handing on again.
            now.checked_sub(Duration::from_secs(left.unsigned_abs()))
        };
        if let Some(until) = until {
            self.until = Some(self.until.map_or(until, |own| own.max(until)));
        }
        self.length = self.length.max(shared.next).min(COOLDOWN_MAX);
    }
}

/// How Yahoo is offered. First in [`crate::providers::LISTED`], which is
/// what makes it the default: no key, no account, nothing to set up, and
/// every instrument type the app knows about.
pub const LISTED: crate::providers::Listed = crate::providers::Listed {
    id: "yahoo",
    label: "Yahoo Finance",
    serves: "Every listing the symbol search covers",
    covers: "Stocks, ETFs, indexes, futures, FX, crypto",
    build: || Box::new(Yahoo::new()),
    setup: None,
    experimental: false,
    gate: None,
};

pub struct Yahoo {
    timeout: Option<Duration>,
    cooldown: Mutex<Cooldown>,
}

impl Default for Yahoo {
    fn default() -> Yahoo {
        Yahoo::new()
    }
}

impl Yahoo {
    pub fn new() -> Yahoo {
        Yahoo {
            timeout: Some(Duration::from_secs(20)),
            cooldown: Mutex::new(Cooldown::new()),
        }
    }

    /// Yahoo's own spelling of a resolution, for the ones it serves.
    fn interval(timeframe: Timeframe) -> Option<String> {
        if !CAPABILITIES.iter().any(|c| c.timeframe == timeframe) {
            // Folded from a served resolution by the caller.
            return None;
        }
        Some(match timeframe.unit {
            Unit::Minute => format!("{}m", timeframe.count),
            Unit::Hour => format!("{}h", timeframe.count),
            Unit::Day => format!("{}d", timeframe.count),
            Unit::Week => format!("{}wk", timeframe.count),
            Unit::Month => format!("{}mo", timeframe.count),
            Unit::Year => return None,
        })
    }

    /// How much history to ask for when we have none, in days.
    ///
    /// Never `range=max` for daily bars. Yahoo honours the interval only
    /// within a bounded window: ask for everything and it quietly coarsens,
    /// returning month-ends for the old part of the series and days only near
    /// the end. A chart built on that looks fine and is wrong.
    fn full_window_days(timeframe: Timeframe) -> i64 {
        let capability = CAPABILITIES.iter().find(|c| c.timeframe == timeframe);
        // Daily and coarser: two decades is more than any chart needs and well
        // inside the window Yahoo serves honestly.
        let history = capability.and_then(|c| c.history_days).map(i64::from).unwrap_or(20 * 365);
        let per_request = capability
            .and_then(|c| c.max_request_days)
            .map(i64::from)
            .unwrap_or(i64::MAX);
        history.min(per_request)
    }

    fn url(symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<String, ProviderError> {
        let interval = Self::interval(timeframe).ok_or_else(|| {
            ProviderError::Unsupported(format!("{} is folded, not fetched", timeframe.label()))
        })?;
        let encoded = encode(symbol);
        // Always an explicit window. For a tail fetch it is how the request
        // stays small; for a first fetch it is what stops Yahoo coarsening the
        // series behind our back.
        let now = chrono::Utc::now().timestamp();
        let to = now + timeframe.seconds();
        let from = since.unwrap_or_else(|| {
            now - Self::full_window_days(timeframe) * 86_400 + WINDOW_MARGIN
        });
        Ok(format!(
            "{ENDPOINT}/{encoded}?interval={interval}&period1={from}&period2={to}"
        ))
    }
}

/// One spark request for the last few sessions of each symbol.
///
/// Five days, because two closes is all a quote needs and a long weekend or a
/// holiday can put the session before three days back.
fn spark_url(symbols: &[&str]) -> String {
    let joined: Vec<String> = symbols.iter().map(|s| encode(s)).collect();
    format!("{SPARK}?symbols={}&range=5d&interval=1d", joined.join(","))
}

/// Percent-encode the handful of characters Yahoo symbols actually contain.
fn encode(symbol: &str) -> String {
    let mut out = String::with_capacity(symbol.len() + 4);
    for c in symbol.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '-' | '_' | '~' => out.push(c),
            '^' => out.push_str("%5E"),
            '=' => out.push_str("%3D"),
            other => {
                let mut buf = [0u8; 4];
                for b in other.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{b:02X}"));
                }
            }
        }
    }
    out
}

impl Yahoo {
    /// One request, keeping the cooldown's state: a 429 starts or lengthens
    /// it and any answer relaxes it, whichever endpoint it came from.
    fn ask(&self, url: &str) -> Result<serde_json::Value, ProviderError> {
        let answer = self.fetch(url);
        let mut cooldown = self.cooldown.lock().unwrap_or_else(|e| e.into_inner());
        match &answer {
            Ok(_) => cooldown.succeeded(),
            Err(ProviderError::RateLimited) => cooldown.throttled(Instant::now()),
            Err(_) => {}
        }
        answer
    }

    /// One request, no pacing and no retries.
    fn fetch(&self, url: &str) -> Result<serde_json::Value, ProviderError> {
        let mut request = ureq::get(url).header("User-Agent", AGENT);
        if let Some(timeout) = self.timeout {
            request = request.config().timeout_global(Some(timeout)).build();
        }

        let mut response = match request.call() {
            Ok(response) => response,
            Err(ureq::Error::StatusCode(code)) => return Err(refusal(code)),
            Err(error) => return Err(could_not_reach(error)),
        };

        response
            .body_mut()
            .read_json()
            .map_err(|e| ProviderError::Malformed(e.to_string()))
    }
}

/// What an HTTP status other than success means for the next request.
///
/// A 400 or a 422 is an answer about the request itself: a 422 is how Yahoo
/// says a window is outside what it serves. Asking again half a second later
/// gets the same one at the price of another request, so it is asked once.
/// Both used to read as a garbled reply and be tried three times.
///
/// A 403 is a refusal of the client rather than the request, and Yahoo
/// sends them alongside its 429s: it turns some user agents and TLS
/// fingerprints away regardless of how often they ask. So it starts the
/// same cooldown as a 429 rather than being tried again at once.
///
/// A 5xx, a 408 and a 425 are transient: the edge, a request that took too
/// long, a connection that was early. Those keep their retries. Anything
/// else Yahoo has no documented reason to send, and reads as it always has,
/// as a reply nobody expected.
fn refusal(code: u16) -> ProviderError {
    match code {
        429 | 403 => ProviderError::RateLimited,
        404 => ProviderError::NotFound,
        400 | 422 => ProviderError::Unsupported(format!("HTTP {code}")),
        408 | 425 | 500..=599 => ProviderError::Network(format!("HTTP {code}")),
        _ => ProviderError::Malformed(format!("HTTP {code}")),
    }
}

/// Which side of the wire a transport failure was on.
///
/// Worth separating because the chart says one of two different things, and
/// only one of them is about Yahoo. A name that cannot be resolved and a route
/// that does not exist mean this machine is not connected to anything — the
/// case a fresh install on a laptop with the wifi off hits first — and telling
/// that person the provider is having trouble sends them looking in the wrong
/// place. Anything else is Yahoo's end, and worth a retry; being offline is
/// not, which is why [`ProviderError::Offline`] is absent from the retry arm.
///
/// A connection that could not be made is Yahoo's end, not ours: the name
/// resolved, so the machine is on a network, and what refused or dropped the
/// handshake sits on the other side of it. Filing it as offline told a
/// person with working wifi that they had none.
fn could_not_reach(error: ureq::Error) -> ProviderError {
    let offline = match &error {
        ureq::Error::HostNotFound => true,
        ureq::Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::NetworkUnreachable
                | std::io::ErrorKind::HostUnreachable
                | std::io::ErrorKind::NetworkDown
                | std::io::ErrorKind::AddrNotAvailable
        ),
        _ => false,
    };
    match offline {
        true => ProviderError::Offline(error.to_string()),
        false => ProviderError::Network(error.to_string()),
    }
}

impl Provider for Yahoo {
    fn id(&self) -> &'static str {
        "yahoo"
    }

    fn label(&self) -> &'static str {
        "Yahoo Finance"
    }

    fn delay_minutes(&self, kind: InstrumentKind) -> u32 {
        match kind {
            InstrumentKind::FutureRoot => 10,
            InstrumentKind::Index => 15,
            // Equities and ETFs are near real time, FX and crypto are
            // continuous. None of it is guaranteed, so claim nothing.
            _ => 0,
        }
    }

    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        if let Some(explicit) = instrument.override_for(self.id()) {
            return Some(explicit.to_string());
        }
        Some(match instrument.kind {
            InstrumentKind::Index => format!("^{}", instrument.symbol),
            // A root is Yahoo's own continuous series; a dated contract is
            // spelled with two digits of year and the exchange it is on,
            // ESH27.CME, and is not served for a root on no known exchange.
            InstrumentKind::FutureRoot => match instrument.contract() {
                None => format!("{}=F", instrument.symbol),
                Some(month) => {
                    let venue = match registry().contract_exchange(instrument.root())? {
                        "CME" => "CME",
                        "CBOT" => "CBT",
                        "NYMEX" => "NYM",
                        "COMEX" => "CMX",
                        "ICEUS" => "NYB",
                        _ => return None,
                    };
                    format!("{}{}{:02}.{venue}", instrument.root(), month.code(), month.year.rem_euclid(100))
                }
            },
            InstrumentKind::Fx => format!("{}=X", instrument.symbol),
            InstrumentKind::Crypto => format!("{}-USD", instrument.symbol),
            InstrumentKind::Equity | InstrumentKind::Etf => {
                // Yahoo spells class shares with a hyphen: BRK.B is BRK-B.
                let base = instrument.symbol.replace('.', "-");
                match &instrument.suffix {
                    Some(suffix) => format!("{base}.{suffix}"),
                    None => base,
                }
            }
        })
    }

    fn capabilities(&self) -> &'static [Capability] {
        CAPABILITIES
    }

    /// One request for bars, made now.
    ///
    /// Blocks only for the network: the pacing before a request and the
    /// backoff after a failed one are rules stated below and applied by the
    /// caller's queue, never slept out here. What this does keep is the
    /// cooldown's state, because the 429 that starts one and the success
    /// that relaxes it both arrive here.
    fn bars(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
    ) -> Result<Vec<Bar>, ProviderError> {
        let url = Self::url(symbol, timeframe, since)?;
        let body = self.ask(&url)?;
        let session = session_start(&body);
        parse_chart(&body).map(|bars| collapse_days(bars, timeframe, session))
    }

    fn pacing(&self) -> Pacing {
        PACING
    }

    fn max_batch(&self) -> usize {
        MAX_BATCH
    }

    /// Through spark, which answers for up to [`MAX_BATCH`] symbols at once.
    fn closes(&self, symbols: &[&str]) -> Result<Vec<Closes>, ProviderError> {
        if symbols.is_empty() {
            return Ok(Vec::new());
        }
        if symbols.len() > MAX_BATCH {
            return Err(ProviderError::Unsupported(format!(
                "{} symbols in one request, at most {MAX_BATCH}",
                symbols.len()
            )));
        }
        parse_spark(&self.ask(&spark_url(symbols))?)
    }

    // No `stream`, so `delivery` is polled. There is nothing to subscribe
    // to: Yahoo's quote path is the very same chart endpoint the bars come
    // from — the older `v7/finance/quote` wants a cookie and crumb handshake
    // and is not used at all — so there is nothing cheaper to poll than the
    // series itself, and no fresher number hiding behind a different URL.
    // Keeping a chart current therefore means refetching its tail, as rarely
    // as it can be got away with; the cadence is the caller's, see
    // [`crate::refresh`].

    fn cooldown_until(&self) -> Option<Instant> {
        self.cooldown.lock().unwrap_or_else(|e| e.into_inner()).until
    }

    fn shared_cooldown(&self) -> Option<SharedCooldown> {
        let unix = chrono::Utc::now().timestamp();
        self.cooldown.lock().unwrap_or_else(|e| e.into_inner()).shared(Instant::now(), unix)
    }

    fn resume_cooldown(&self, cooldown: SharedCooldown) {
        let unix = chrono::Utc::now().timestamp();
        let mut own = self.cooldown.lock().unwrap_or_else(|e| e.into_inner());
        own.resume(cooldown, Instant::now(), unix);
    }

    /// A dropped connection or a 503 on the way through Yahoo's edge is
    /// common enough to be worth one more try, a little later each time. A
    /// 429 is not: trying again is what makes the cooldown longer. And an
    /// unknown symbol or a resolution Yahoo does not serve will not have
    /// changed in half a second.
    fn retry_after(&self, error: &ProviderError, attempt: u32) -> Option<Duration> {
        let transient = matches!(error, ProviderError::Network(_) | ProviderError::Malformed(_));
        (transient && attempt < RETRIES).then(|| BACKOFF * (1 << attempt))
    }
}

/// Pull bars out of a chart response.
///
/// Yahoo pads its arrays with nulls wherever it has no print, so a bar is only
/// real if every one of its four prices is present. Inventing a value here
/// would put a candle on the chart that never traded.
pub fn parse_chart(body: &serde_json::Value) -> Result<Vec<Bar>, ProviderError> {
    let chart = &body["chart"];
    if let Some(code) = chart["error"]["code"].as_str() {
        return Err(match code {
            "Not Found" => ProviderError::NotFound,
            other => ProviderError::Malformed(other.to_string()),
        });
    }

    let result = chart["result"]
        .get(0)
        .ok_or_else(|| ProviderError::Malformed("no result".into()))?;

    let stamps = result["timestamp"]
        .as_array()
        .ok_or_else(|| ProviderError::Malformed("no timestamps".into()))?;

    let quote = result["indicators"]["quote"]
        .get(0)
        .ok_or_else(|| ProviderError::Malformed("no quote block".into()))?;

    let column = |name: &str| quote[name].as_array().cloned().unwrap_or_default();
    let (open, high, low, close, volume) = (
        column("open"),
        column("high"),
        column("low"),
        column("close"),
        column("volume"),
    );

    let mut bars = Vec::with_capacity(stamps.len());
    for (i, stamp) in stamps.iter().enumerate() {
        let Some(ts) = stamp.as_i64() else { continue };
        let at = |series: &Vec<serde_json::Value>| series.get(i).and_then(|v| v.as_f64());
        let (Some(o), Some(h), Some(l), Some(c)) = (at(&open), at(&high), at(&low), at(&close))
        else {
            continue;
        };
        bars.push(Bar { ts, open: o, high: h, low: l, close: c, volume: at(&volume).unwrap_or(0.0) });
    }

    // Yahoo is ordered oldest first, but nothing promises it.
    bars.sort_by_key(|b| b.ts);
    bars.dedup_by_key(|b| b.ts);
    Ok(bars)
}

/// Pull the latest two closes per symbol out of a spark response.
///
/// Spark answers with an object keyed by symbol, each holding its stamps and
/// closes and nothing else worth reading. `fulldayPrice` is not the close:
/// before the open it is the pre-market print, which is not what a list of
/// closes means today. Nulls are skipped, as on a chart, and a second stamp
/// on one day keeps the later close, the way [`collapse_days`] does. A symbol
/// left with fewer than two closes has no move to show and is left out, as is
/// one Yahoo did not know, which it drops from the answer without a word.
///
/// The session in progress comes stamped with the time of its last print,
/// as on a chart, and is put on the series' own clock the same way, a whole
/// number of days after the session before: EURUSD's Thursday is Wednesday
/// 23:00, not Thursday 10:57. Left as the print time it reads as a session
/// of its own a day after the bars' one.
///
/// A 200 can still carry an error, `{"finance":{"error":…}}`, which has no
/// symbol in it to skip: that is an answer that went wrong, not one that
/// knew none of the symbols, and says so rather than coming back empty.
pub fn parse_spark(body: &serde_json::Value) -> Result<Vec<Closes>, ProviderError> {
    let symbols =
        body.as_object().ok_or_else(|| ProviderError::Malformed("no spark object".into()))?;
    if let Some(error) = symbols.values().find_map(|entry| entry["error"]["code"].as_str()) {
        return Err(match error {
            "Not Found" => ProviderError::NotFound,
            other => ProviderError::Malformed(other.to_string()),
        });
    }
    let answered = |entry: &serde_json::Value| {
        entry["timestamp"].is_array() || entry["symbol"].is_string()
    };
    if !symbols.is_empty() && !symbols.values().any(answered) {
        return Err(ProviderError::Malformed("no symbol in the spark answer".into()));
    }
    let mut out = Vec::with_capacity(symbols.len());
    for (symbol, entry) in symbols {
        let (Some(stamps), Some(closes)) =
            (entry["timestamp"].as_array(), entry["close"].as_array())
        else {
            continue;
        };
        let mut prints: Vec<(i64, f64)> = stamps
            .iter()
            .zip(closes)
            .filter_map(|(stamp, close)| Some((stamp.as_i64()?, close.as_f64()?)))
            .collect();
        // Oldest first, as the chart endpoint is, but nothing promises it.
        prints.sort_by_key(|&(at, _)| at);
        if let [.., (before, _), (latest, _)] = &mut prints[..] {
            *latest -= (*latest - *before).rem_euclid(86_400);
        }
        let mut days: Vec<(i64, f64)> = Vec::with_capacity(prints.len());
        for (at, close) in prints {
            match days.last_mut() {
                Some(last) if last.0.div_euclid(86_400) == at.div_euclid(86_400) => {
                    *last = (last.0, close)
                }
                _ => days.push((at, close)),
            }
        }
        let [.., (previous_at, previous), (at, last)] = days[..] else { continue };
        out.push(Closes { symbol: symbol.clone(), last, at, previous, previous_at });
    }
    Ok(out)
}

/// Fold bars that land on the same day into one, for daily and coarser.
///
/// Yahoo appends a partial bar for the session in progress, stamped with the
/// current time rather than the session's open. On a daily series that is a
/// second bar for today, and anything comparing the last two closes — a
/// change column, say — reads a change of exactly zero.
///
/// FX is the case a calendar day misses: its days are stamped at London
/// midnight, 23:00 UTC the evening before in summer, so Friday's session is
/// Thursday 23:00 and its partial is Friday 09:36. See [`on_the_clock`].
fn collapse_days(mut bars: Vec<Bar>, timeframe: Timeframe, session: Option<i64>) -> Vec<Bar> {
    if !matches!(timeframe.unit, Unit::Day | Unit::Week) || bars.len() < 2 {
        return bars;
    }
    if timeframe.unit == Unit::Day {
        on_the_clock(&mut bars, session);
    }
    let day_of = |ts: i64| ts.div_euclid(86_400);
    let mut out: Vec<Bar> = Vec::with_capacity(bars.len());
    for bar in bars {
        match out.last_mut() {
            // The later bar is the more complete one, but the day's extremes
            // belong to the day, not to whichever slice arrived last.
            Some(last) if day_of(last.ts) == day_of(bar.ts) => {
                last.high = last.high.max(bar.high);
                last.low = last.low.min(bar.low);
                last.close = bar.close;
                last.volume = last.volume.max(bar.volume);
            }
            _ => out.push(bar),
        }
    }
    out
}

/// Put the partial bar for the session in progress on the series' own clock.
///
/// Yahoo stamps it with the time of the last print, so every refresh brings
/// the same day under a new stamp: the store keeps each as a bar of its own,
/// and once the day closes, Yahoo's own bar for it lands beside them all.
/// EURUSD collected seven bars for one Friday that way.
///
/// It takes the stamp of the session Yahoo says is current, so the next
/// refresh replaces it and the closed bar does too. Working that session out
/// from the clock instead goes wrong at the edges: FX prints until 21:30 UTC
/// on a Friday and is stamped on Saturdays, and a stamp Yahoo never uses
/// again is a bar that stays in the cache for good. A last bar already on the
/// clock is left alone, and so is one Yahoo gives no session for.
fn on_the_clock(bars: &mut [Bar], session: Option<i64>) {
    let [.., before, last] = bars else { return };
    if (last.ts - before.ts).rem_euclid(86_400) == 0 {
        return;
    }
    if let Some(start) = session.filter(|&start| start >= before.ts) {
        last.ts = start;
    }
}

/// When the session in progress began, which is the stamp Yahoo gives its
/// bar once it has one.
fn session_start(body: &serde_json::Value) -> Option<i64> {
    body["chart"]["result"][0]["meta"]["currentTradingPeriod"]["regular"]["start"].as_i64()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EURUSD on Monday 2026-10-19 at 20:15 UTC, as Yahoo sent it: the
    /// session is stamped Sunday 23:00, and its partial comes stamped with
    /// the time of its last print.
    #[test]
    fn an_fx_day_in_progress_takes_its_session_stamp() {
        let body = serde_json::json!({ "chart": { "result": [{
            "meta": { "currentTradingPeriod": { "regular": { "start": 1_792_364_400 } } },
            "timestamp": [1_792_105_200, 1_792_364_400, 1_792_440_948],
            "indicators": { "quote": [{
                "open": [1.1212, 1.1229, 1.1242],
                "high": [1.1270, 1.1236, 1.1242],
                "low": [1.1209, 1.1188, 1.1183],
                "close": [1.1213, 1.1225, 1.1215],
                "volume": [0, 0, 0],
            }] },
        }] } });
        let session = session_start(&body);
        let out = collapse_days(parse_chart(&body).unwrap(), Timeframe::days(1), session);
        let stamps: Vec<i64> = out.iter().map(|b| b.ts).collect();
        assert_eq!(stamps, [1_792_105_200, 1_792_364_400], "Monday is one bar, on Sunday 23:00");
        assert_eq!(out[1].close, 1.1215, "the latest print is its close");
        assert_eq!(out[1].low, 1.1183, "and the day keeps its extreme");
    }

    /// Late on a Friday, or on a Saturday, the session Yahoo calls current is
    /// one it has a stamp for, so nothing lands on a day FX does not trade.
    #[test]
    fn a_partial_never_invents_a_stamp() {
        let bar = |ts| Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 0.0 };
        let (thu_23, day) = (1_792_105_200, 86_400);
        let fetch = |partial, session| {
            collapse_days(vec![bar(thu_23 - day), bar(thu_23), bar(partial)], Timeframe::days(1), session)
                .iter()
                .map(|b| b.ts)
                .collect::<Vec<_>>()
        };
        // Friday 21:29 and Saturday 14:08 both fold into Friday.
        assert_eq!(fetch(thu_23 + 22 * 3_600 + 29 * 60, Some(thu_23)), [thu_23 - day, thu_23]);
        assert_eq!(fetch(thu_23 + day + 15 * 3_600, Some(thu_23)), [thu_23 - day, thu_23]);
        // Sunday evening's first prints belong to Monday.
        let sun_23 = thu_23 + 3 * day;
        assert_eq!(fetch(sun_23 - 1_800, Some(sun_23)), [thu_23 - day, thu_23, sun_23]);
        // Without a session, or with one older than the bars, it stays put.
        assert_eq!(fetch(thu_23 + 3_600, None), [thu_23 - day, thu_23, thu_23 + 3_600]);
        assert_eq!(fetch(thu_23 + 3_600, Some(thu_23 - day)), [thu_23 - day, thu_23, thu_23 + 3_600]);
        // A bar on the clock stays where it is.
        assert_eq!(fetch(thu_23 + day, Some(thu_23)), [thu_23 - day, thu_23, thu_23 + day]);
    }

    /// Yahoo has no stream, so a chart drawn from it has to be asked again or
    /// it quietly stops being true. The window polls because this says to,
    /// not because a setting does.
    #[test]
    fn yahoo_has_to_be_polled_to_keep_a_chart_current() {
        assert_eq!(Yahoo::new().delivery(), crate::provider::Delivery::Polled);
    }

    /// A laptop with the wifi off must not be told the data provider is
    /// having trouble: the provider is fine and the user would go looking in
    /// the wrong place.
    #[test]
    fn a_name_that_cannot_be_resolved_is_reported_as_being_offline() {
        assert!(matches!(
            could_not_reach(ureq::Error::HostNotFound),
            ProviderError::Offline(_)
        ));
        let unreachable = std::io::Error::from(std::io::ErrorKind::NetworkUnreachable);
        assert!(matches!(
            could_not_reach(ureq::Error::Io(unreachable)),
            ProviderError::Offline(_)
        ));
    }

    /// The name resolved, so there is a network; whatever would not take the
    /// handshake is on Yahoo's side of it.
    #[test]
    fn a_connection_that_could_not_be_made_is_reported_against_the_provider() {
        assert!(matches!(
            could_not_reach(ureq::Error::ConnectionFailed),
            ProviderError::Network(_)
        ));
        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert!(matches!(
            could_not_reach(ureq::Error::Io(refused)),
            ProviderError::Network(_)
        ));
    }

    #[test]
    fn a_connection_that_broke_midway_is_reported_against_the_provider() {
        let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
        assert!(matches!(
            could_not_reach(ureq::Error::Io(reset)),
            ProviderError::Network(_)
        ));
        assert!(matches!(
            could_not_reach(ureq::Error::TooManyRedirects),
            ProviderError::Network(_)
        ));
    }

    fn instrument(kind: InstrumentKind, symbol: &str) -> Instrument {
        Instrument {
            symbol: symbol.to_string(),
            name: String::new(),
            kind,
            suffix: None,
            currency: None,
            tier: 0,
            session_origin: 0,
            overrides: Vec::new(),
            exchange: None,
            popularity: 0,
            local_name: None,
        }
    }

    #[test]
    fn templates_produce_yahoo_symbology() {
        let y = Yahoo::new();
        let cases = [
            (InstrumentKind::Index, "GSPC", "^GSPC"),
            (InstrumentKind::FutureRoot, "GC", "GC=F"),
            (InstrumentKind::Fx, "EURUSD", "EURUSD=X"),
            (InstrumentKind::Crypto, "BTC", "BTC-USD"),
            (InstrumentKind::Equity, "AAPL", "AAPL"),
            (InstrumentKind::Equity, "BRK.B", "BRK-B"),
        ];
        for (kind, symbol, expected) in cases {
            assert_eq!(y.symbol_for(&instrument(kind, symbol)).unwrap(), expected);
        }
    }

    /// A dated contract is on Yahoo under the exchange's suffix, which is
    /// not the exchange's own code; a root on no exchange the table knows
    /// has no contracts there.
    #[test]
    fn a_dated_contract_is_spelled_with_its_year_and_its_exchange() {
        let y = Yahoo::new();
        let today = crate::contracts::today();
        for (typed, expected) in [("ESH7", "ESH27.CME"), ("ZNH7", "ZNH27.CBT"), ("CLF7", "CLF27.NYM"), ("GCG7", "GCG27.CMX"), ("KCH7", "KCH27.NYB"), ("MESZ6", "MESZ26.CME")] {
            let (root, month) = crate::contracts::parse(typed, today).unwrap();
            let contract = instrument(InstrumentKind::FutureRoot, &root).in_month(month).unwrap();
            assert_eq!(y.symbol_for(&contract).as_deref(), Some(expected), "{typed}");
        }
        let (root, month) = crate::contracts::parse("ZZQH7", today).unwrap();
        assert_eq!(y.symbol_for(&instrument(InstrumentKind::FutureRoot, &root).in_month(month).unwrap()), None);
        assert_eq!(y.symbol_for(&instrument(InstrumentKind::FutureRoot, "ZZQ")).as_deref(), Some("ZZQ=F"));
    }

    #[test]
    fn a_suffix_is_appended_for_foreign_listings() {
        let y = Yahoo::new();
        let mut inst = instrument(InstrumentKind::Equity, "SAN");
        inst.suffix = Some("MC".into());
        assert_eq!(y.symbol_for(&inst).unwrap(), "SAN.MC");
    }

    #[test]
    fn an_override_beats_the_template() {
        let y = Yahoo::new();
        let mut inst = instrument(InstrumentKind::Index, "DXY");
        inst.overrides = vec![("yahoo".into(), "DX-Y.NYB".into())];
        assert_eq!(y.symbol_for(&inst).unwrap(), "DX-Y.NYB");
    }

    #[test]
    fn urls_encode_and_always_use_an_explicit_window() {
        let full = Yahoo::url("^GSPC", Timeframe::days(1), None).unwrap();
        assert!(full.contains("%5EGSPC"), "{full}");
        assert!(full.contains("interval=1d"), "{full}");
        // Never range=max: it makes Yahoo coarsen a daily series into
        // month-ends without saying so.
        assert!(!full.contains("range="), "{full}");
        assert!(full.contains("period1=") && full.contains("period2="), "{full}");

        let tail = Yahoo::url("GC=F", Timeframe::hours(1), Some(1_700_000_000)).unwrap();
        assert!(tail.contains("GC%3DF"), "{tail}");
        assert!(tail.contains("period1=1700000000"), "{tail}");
    }

    #[test]
    fn intraday_asks_for_less_history_than_daily() {
        assert!(
            Yahoo::full_window_days(Timeframe::minutes(5))
                < Yahoo::full_window_days(Timeframe::days(1))
        );
        assert_eq!(Yahoo::full_window_days(Timeframe::minutes(5)), 60);
    }

    /// Yahoo reads its own limit as strictly inside, and not even the same way
    /// for every interval: sixty days of fifteen-minute bars is a 422 while
    /// sixty days of five-minute bars is fine. Asking for the exact window is
    /// how DIA went blank at 15m with every other resolution working.
    #[test]
    fn a_full_window_stops_short_of_the_limit_it_is_allowed() {
        for timeframe in
            [Timeframe::minutes(5), Timeframe::minutes(15), Timeframe::minutes(30)]
        {
            let url = Yahoo::url("DIA", timeframe, None).unwrap();
            let from: i64 = url
                .split("period1=")
                .nth(1)
                .and_then(|rest| rest.split('&').next())
                .and_then(|v| v.parse().ok())
                .expect("period1");
            let asked = chrono::Utc::now().timestamp() - from;
            let limit = Yahoo::full_window_days(timeframe) * 86_400;
            assert!(asked < limit, "{} asked for the whole window", timeframe.label());
            // And not so far short that a day of history is thrown away.
            assert!(asked > limit - 86_400, "{} gave up too much", timeframe.label());
        }
    }

    #[test]
    fn a_request_never_exceeds_the_per_request_cap() {
        // One-minute bars exist for a month but are served a week at a time.
        // Asking for the month hands back an empty series, which is how a
        // three-minute chart ended up saying "no data for this symbol".
        assert_eq!(Yahoo::full_window_days(Timeframe::minutes(1)), 7);

        let url = Yahoo::url("AAPL", Timeframe::minutes(1), None).unwrap();
        let from: i64 = url
            .split("period1=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .and_then(|v| v.parse().ok())
            .expect("period1");
        let days = (chrono::Utc::now().timestamp() - from) / 86_400;
        assert!((6..=8).contains(&days), "asked for {days} days of one-minute bars");
    }

    #[test]
    fn every_capability_can_be_asked_for_in_one_request() {
        for capability in CAPABILITIES {
            let days = Yahoo::full_window_days(capability.timeframe);
            if let Some(cap) = capability.max_request_days {
                assert!(days <= cap as i64, "{:?}", capability.timeframe);
            }
        }
    }

    #[test]
    fn a_partial_bar_for_today_does_not_become_a_second_day() {
        let bar = |ts, close, volume| Bar {
            ts,
            open: close,
            high: close + 1.0,
            low: close - 1.0,
            close,
            volume,
        };
        // Yesterday, today's session bar, and today's partial update.
        let day = 86_400;
        let bars = vec![
            bar(10 * day, 100.0, 500.0),
            bar(11 * day, 110.0, 400.0),
            bar(11 * day + 50_000, 112.0, 450.0),
        ];
        let out = collapse_days(bars, Timeframe::days(1), None);
        assert_eq!(out.len(), 2, "today must be one bar");
        assert_eq!(out[1].close, 112.0, "the later close wins");
        assert_eq!(out[1].high, 113.0, "the day keeps its extreme");
        assert_eq!(out[1].volume, 450.0);

        // And the change between the last two closes is no longer zero.
        assert_ne!(out[1].close, out[0].close);
    }

    #[test]
    fn intraday_bars_are_left_alone() {
        let bar = |ts| Bar { ts, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0 };
        let bars = vec![bar(0), bar(300), bar(600)];
        assert_eq!(collapse_days(bars.clone(), Timeframe::minutes(5), None).len(), 3);
        // But a daily series with three stamps on one day is one bar.
        assert_eq!(collapse_days(bars, Timeframe::days(1), None).len(), 1);
    }

    #[test]
    fn derived_timeframes_are_never_fetched() {
        assert!(Yahoo::url("AAPL", Timeframe::hours(4), None).is_err());
        assert!(Yahoo::url("AAPL", Timeframe::weeks(1), None).is_err());
        let y = Yahoo::new();
        assert!(!y.serves(Timeframe::hours(4)));
        assert!(y.serves(Timeframe::hours(1)));
    }

    #[test]
    fn nulls_are_skipped_rather_than_invented() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"chart":{"error":null,"result":[{
                 "timestamp":[100,200,300],
                 "indicators":{"quote":[{
                   "open":[1.0,null,3.0],
                   "high":[2.0,null,4.0],
                   "low":[0.5,null,2.5],
                   "close":[1.5,null,3.5],
                   "volume":[10,null,30]}]}}]}}"#,
        )
        .unwrap();
        let bars = parse_chart(&body).unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].ts, 100);
        assert_eq!(bars[1].ts, 300);
        assert_eq!(bars[1].close, 3.5);
    }

    #[test]
    fn a_missing_volume_is_zero_not_a_dropped_bar() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"chart":{"result":[{"timestamp":[100],"indicators":{"quote":[{
                 "open":[1.0],"high":[2.0],"low":[0.5],"close":[1.5]}]}}]}}"#,
        )
        .unwrap();
        let bars = parse_chart(&body).unwrap();
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].volume, 0.0);
    }

    #[test]
    fn an_error_payload_becomes_not_found() {
        let body: serde_json::Value =
            serde_json::from_str(r#"{"chart":{"error":{"code":"Not Found"},"result":null}}"#)
                .unwrap();
        assert!(matches!(parse_chart(&body), Err(ProviderError::NotFound)));
    }

    /// The gaps are rules the queue reads, not waits this module performs.
    /// What matters here is only that the work nobody asked for is held
    /// further apart than the chart somebody is looking at.
    #[test]
    fn speculative_work_is_paced_far_further_apart() {
        let pacing = Yahoo::new().pacing();
        assert_eq!(pacing.gap(false), MIN_GAP);
        assert_eq!(pacing.gap(true), MIN_GAP_SPECULATIVE);
        assert!(pacing.min_gap_speculative > pacing.min_gap * 4, "{pacing:?}");
    }

    #[test]
    fn a_429_sets_a_cooldown_the_queue_can_read() {
        let y = Yahoo::new();
        assert_eq!(y.cooldown_until(), None, "nothing has gone wrong yet");

        let t0 = Instant::now();
        y.cooldown.lock().unwrap().throttled(t0);
        let until = y.cooldown_until().expect("a cooldown is set");
        assert!(until > t0, "inside the cooldown");
        assert_eq!(until, t0 + COOLDOWN_START, "and it ends");
    }

    #[test]
    fn repeated_throttling_backs_off_and_success_relaxes_it() {
        let mut cooldown = Cooldown::new();
        let t0 = Instant::now();

        cooldown.throttled(t0);
        assert_eq!(cooldown.length, COOLDOWN_START * 2);
        cooldown.throttled(t0);
        assert_eq!(cooldown.length, COOLDOWN_START * 4);

        cooldown.succeeded();
        assert_eq!(cooldown.length, COOLDOWN_START);
    }

    #[test]
    fn the_backoff_has_a_ceiling() {
        let mut cooldown = Cooldown::new();
        let t0 = Instant::now();
        for _ in 0..20 {
            cooldown.throttled(t0);
        }
        assert_eq!(cooldown.length, COOLDOWN_MAX);
    }

    /// The widget's next run is a new process. What this one was told has to
    /// reach it, or it asks straight into the same refusal.
    #[test]
    fn a_cooldown_handed_on_is_kept_by_the_next_process() {
        let (t0, unix) = (Instant::now(), 1_000_000);
        let mut first = Cooldown::new();
        first.throttled(t0);
        let shared = first.shared(t0, unix).expect("a running cooldown is handed on");
        assert_eq!(shared.until, unix + 60);
        assert_eq!(shared.next, COOLDOWN_START * 2);

        // Twenty seconds later, somewhere else.
        let (t1, unix1) = (Instant::now(), unix + 20);
        let mut second = Cooldown::new();
        second.resume(shared, t1, unix1);
        assert_eq!(second.until, Some(t1 + Duration::from_secs(40)), "the rest of it");

        // And its next refusal carries on climbing rather than starting over.
        second.throttled(t1);
        assert_eq!(second.length, COOLDOWN_START * 4);
    }

    /// A row written by hand, or by a clock that has since gone back, can put
    /// the end of a cooldown anywhere. It is still never longer than the
    /// longest Yahoo hands out, and never a panic.
    #[test]
    fn a_cooldown_handed_on_from_far_ahead_lasts_at_most_the_ceiling() {
        let (now, unix) = (Instant::now(), 1_000_000);
        for until in [unix + 86_400 * 365 * 100, i64::MAX] {
            let mut cooldown = Cooldown::new();
            cooldown.resume(SharedCooldown { until, next: Duration::MAX }, now, unix);
            assert_eq!(cooldown.until, Some(now + COOLDOWN_MAX), "{until}");
            assert_eq!(cooldown.length, COOLDOWN_MAX, "{until}");
            // And hands on what it actually holds, not what it was told.
            let shared = cooldown.shared(now, unix).unwrap();
            assert_eq!(shared.until, unix + 900, "{until}");
        }
        let mut cooldown = Cooldown::new();
        cooldown.resume(SharedCooldown { until: i64::MIN, next: Duration::ZERO }, now, unix);
        assert_eq!(cooldown.length, COOLDOWN_START, "nor from far behind");
    }

    /// Two more tries, each twice as far out as the last, and then a final
    /// answer. The queue does the waiting; this only has to say how long.
    #[test]
    fn a_transient_failure_is_worth_two_more_tries_with_a_doubling_backoff() {
        let y = Yahoo::new();
        let dropped = ProviderError::Network("connection reset".into());
        assert_eq!(y.retry_after(&dropped, 0), Some(Duration::from_millis(400)));
        assert_eq!(y.retry_after(&dropped, 1), Some(Duration::from_millis(800)));
        assert_eq!(y.retry_after(&dropped, 2), None, "the third failure is final");

        let garbled = ProviderError::Malformed("not json".into());
        assert_eq!(y.retry_after(&garbled, 0), Some(Duration::from_millis(400)));
    }

    /// Asking again is the one thing that makes a 429 worse, and an answer
    /// Yahoo gave on purpose will not change in half a second.
    #[test]
    fn a_refusal_is_never_retried() {
        let y = Yahoo::new();
        for error in [
            ProviderError::RateLimited,
            ProviderError::NotFound,
            ProviderError::Unsupported("4h".into()),
            ProviderError::Offline("no route".into()),
        ] {
            assert_eq!(y.retry_after(&error, 0), None, "{error}");
        }
    }

    /// Each status gets the reading the next request needs: a 400 or 422 is
    /// an answer and asked once, a 403 sits out the cooldown like a 429, only
    /// the transient ones are tried again, and the rest read as before.
    #[test]
    fn each_status_is_read_for_what_it_means_next() {
        for code in [400, 422] {
            assert!(matches!(refusal(code), ProviderError::Unsupported(_)), "{code}");
        }
        for code in [403, 429] {
            assert!(matches!(refusal(code), ProviderError::RateLimited), "{code}");
        }
        assert!(matches!(refusal(404), ProviderError::NotFound));
        for code in [408, 425, 500, 503] {
            assert!(matches!(refusal(code), ProviderError::Network(_)), "{code}");
        }
        for code in [401, 410, 451] {
            assert!(matches!(refusal(code), ProviderError::Malformed(_)), "{code}");
        }
    }

    /// A real spark answer from 2026-10-15 before the US open, cut to two of
    /// its symbols. AAPL's `fulldayPrice`, 345.935, is the pre-market print,
    /// not the close. EURUSD's days are stamped at London midnight, 23:00 UTC
    /// the evening before, and Thursday's session in progress at its last
    /// print, 10:57: it is Wednesday 23:00's session, the stamp the bars give
    /// it, not a day of its own after them.
    #[test]
    fn a_spark_answer_gives_each_symbol_its_last_two_closes_on_its_own_clock() {
        let body: serde_json::Value = serde_json::from_str(include_str!("spark.json")).unwrap();
        let all = parse_spark(&body).unwrap();
        let of = |symbol: &str| all.iter().find(|c| c.symbol == symbol).unwrap();
        let aapl = of("AAPL");
        assert_eq!((aapl.previous, aapl.last, aapl.at), (341.27, 342.96, 1_791_984_600));
        let eurusd = of("EURUSD=X");
        assert_eq!((eurusd.previous, eurusd.last), (1.1204, 1.1217));
        assert_eq!(eurusd.at, 1_792_018_800, "2026-10-14 23:00 UTC");
    }

    /// An error in a 200 is not an answer that knew none of the symbols.
    #[test]
    fn a_spark_error_is_an_error_not_an_empty_answer() {
        let body = r#"{"finance":{"result":null,"error":{"code":"Bad Request","description":"x"}}}"#;
        let answer = parse_spark(&serde_json::from_str(body).unwrap());
        assert!(matches!(answer, Err(ProviderError::Malformed(code)) if code == "Bad Request"));
    }

    #[test]
    fn delays_are_disclosed_per_kind() {
        let y = Yahoo::new();
        assert_eq!(y.delay_minutes(InstrumentKind::FutureRoot), 10);
        assert_eq!(y.delay_minutes(InstrumentKind::Index), 15);
        assert_eq!(y.delay_minutes(InstrumentKind::Equity), 0);
    }
}
