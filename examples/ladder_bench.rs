//! What minute ladders cost: a year of ES-like data, encoded, stored, read
//! back and queried, and the live path a trade at a time.
//!
//! ```sh
//! cargo run --release --example ladder_bench
//! ```
//!
//! Not part of `examples/perf.rs`: these are sizes and timings for the
//! design of the ladders, read by a person, not a regression gate.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use omacharts::store::Store;
use omacharts_engine::ladders::{encode_day, encode_day_as, encode_ladder, to_price, Days, Ladder, Level, Minute, Series, Sides};
use omacharts_engine::ticks::{Tape, Trade};
use omacharts_engine::ladders::Side;
use omacharts_engine::{Instrument, InstrumentKind};

/// Counts allocations, so the live path can be shown not to make any.
struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

const TICK: f64 = 0.25;
const DAYS: usize = 250;
/// 17:00 to 16:00 Chicago.
const SESSION_MINUTES: i64 = 23 * 60;
/// 08:30 Chicago, minutes into the session.
const RTH_OPEN: i64 = 15 * 60 + 30;
const RTH_CLOSE: i64 = 22 * 60;

fn es() -> Instrument {
    Instrument {
        symbol: "ES".into(),
        name: "E-mini S&P 500".into(),
        kind: InstrumentKind::FutureRoot,
        suffix: None,
        currency: Some("USD".into()),
        tier: 0,
        session_origin: 79_200,
        overrides: Vec::new(),
        exchange: None,
        popularity: 0,
        local_name: None,
    }
}

/// One ES-like minute: about 30 prices, heavy in the cash session and thin
/// overnight, trades of one to a few contracts.
fn minute(rng: &mut Rng, ts: i64, center: &mut i32, rth: bool) -> Minute {
    let width = 15 + rng.below(31) as usize;
    *center += rng.below(9) as i32 - 4;
    let low_tick = *center - width as i32 / 2;
    let per_side = if rth { 60 } else { 12 };
    let mut levels = Vec::with_capacity(width);
    let mut sides = Sides::default();
    for _ in 0..width {
        let (bid, ask) = (rng.below(per_side), rng.below(per_side));
        let size = 1 + rng.below(3);
        let level = Level { at_bid: bid, at_ask: ask, unknown: 0, bid_trades: bid.div_ceil(size) as u32, ask_trades: ask.div_ceil(size) as u32 };
        sides.at_bid += level.at_bid;
        sides.at_ask += level.at_ask;
        sides.bid_trades += u64::from(level.bid_trades);
        sides.ask_trades += u64::from(level.ask_trades);
        levels.push(level);
    }
    sides.unknown = rng.below(3);
    sides.unknown_trades = sides.unknown.min(1);
    let high_tick = low_tick + width as i32 - 1;
    let tick = |rng: &mut Rng| low_tick + rng.below(width as u64) as i32;
    let (open, close) = (tick(rng), tick(rng));
    Minute {
        ts,
        open: to_price(open, TICK),
        high: to_price(high_tick, TICK),
        low: to_price(low_tick, TICK),
        close: to_price(close, TICK),
        volume: sides.volume(),
        ladder: Ladder { low_tick, levels, sides },
    }
}

fn year(days: &Days) -> Series {
    let mut rng = Rng(0x5eed);
    let mut series = Series::new(TICK);
    let mut center = 24_000;
    // Trading days named Monday to Friday from 2025-01-06.
    let mut day: i64 = 20_094;
    while series.days.len() < DAYS {
        let weekday = (day + 3).rem_euclid(7); // 0 is Monday
        if weekday < 5 {
            let start = days.start(day);
            for i in 0..SESSION_MINUTES {
                let m = minute(&mut rng, start + i * 60, &mut center, (RTH_OPEN..RTH_CLOSE).contains(&i));
                series.insert_minute(day, m);
            }
        }
        day += 1;
    }
    series
}

fn time<T>(runs: u32, mut f: impl FnMut() -> T) -> (Duration, T) {
    let mut best = Duration::MAX;
    let mut out = None;
    for _ in 0..runs {
        let t = Instant::now();
        let v = f();
        best = best.min(t.elapsed());
        out = Some(v);
    }
    (best, out.expect("at least one run"))
}

fn main() {
    let days = Days::new(&es());
    let (built, series) = time(1, || year(&days));
    let minutes: usize = series.days.iter().map(|d| d.minutes.len()).sum();
    let cells: usize = series.minutes().map(|m| m.ladder.levels.len()).sum();
    let volume: u64 = series.days.iter().map(|d| d.total.total()).sum();
    println!("A year of ES-like minute ladders: {} days, {minutes} minutes, {cells} price cells", series.days.len());
    println!("  {:.0} contracts a day, {:.1} prices a minute, built in {built:.1?}", volume as f64 / DAYS as f64, cells as f64 / minutes as f64);

    let with: usize = series.days.iter().map(|d| encode_day(d, TICK).len()).sum();
    let without: usize = series.days.iter().map(|d| encode_day_as(d, TICK, false).len()).sum();
    let totals: usize = series.days.iter().map(|d| encode_ladder(&d.total).len()).sum();
    println!("\nEncoded, per day");
    println!("  minutes, with per-price trade counts  {:>8.1} KB", with as f64 / DAYS as f64 / 1024.0);
    println!("  minutes, without                      {:>8.1} KB", without as f64 / DAYS as f64 / 1024.0);
    println!("  day total                             {:>8.1} KB", totals as f64 / DAYS as f64 / 1024.0);
    println!("  a year, with counts                   {:>8.1} MB", (with + totals) as f64 / 1048576.0);

    let in_memory: usize = series
        .minutes()
        .map(|m| std::mem::size_of::<Minute>() + m.ladder.levels.capacity() * std::mem::size_of::<Level>())
        .sum();
    println!("  in memory, decoded                    {:>8.1} MB", in_memory as f64 / 1048576.0);

    let dir = std::env::temp_dir().join(format!("omacharts-ladder-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join("omacharts.db");
    let (saved, ()) = time(1, || {
        let store = Store::open_at(&path).expect("a store");
        store.save_ladder_days("ES", TICK, &series.days).expect("saved");
    });
    let file = std::fs::metadata(dir.join("market.db")).map(|m| m.len()).unwrap_or(0)
        + std::fs::metadata(dir.join("market.db-wal")).map(|m| m.len()).unwrap_or(0);

    let store = Store::open_at(&path).expect("a store");
    let (first, last) = (series.days[0].day, series.days[DAYS - 1].day);
    let (loaded, back) = time(5, || store.load_ladder_series("ES", first, last));
    assert_eq!(back, series, "the year reads back as written");
    println!("\nStore");
    println!("  save a year, one transaction          {saved:>10.1?}");
    println!("  market.db on disk                     {:>8.1} MB", file as f64 / 1048576.0);
    println!("  load a year (best of 5)               {loaded:>10.1?}");

    let mut rng = Rng(77);
    let fresh = Minute { ts: series.days[DAYS - 1].last_ts().unwrap_or(0) + 60, ..series.days[0].minutes[0].clone() };
    let (append, ()) = time(20, || store.append_ladder_minute("ES", TICK, last, &fresh, None).expect("appended"));
    println!("  append one live minute                {append:>10.1?}");

    let (from, to) = (i64::MIN, i64::MAX);
    let (whole, ladder) = time(50, || series.volume_at_price(from, to));
    let (sides, _) = time(50, || series.side_totals(from, to));
    let windows: Vec<(i64, i64)> = (0..200)
        .map(|_| {
            let d = rng.below(DAYS as u64 - 3) as usize;
            let a = series.days[d].minutes[rng.below(SESSION_MINUTES as u64) as usize].ts;
            (a, a + 3 * 86_400)
        })
        .collect();
    let t = Instant::now();
    let mut levels = 0;
    for &(a, b) in &windows {
        levels += series.volume_at_price(a, b).levels.len();
    }
    let window = t.elapsed() / windows.len() as u32;
    let week = (series.days[100].first_ts().unwrap_or(0), series.days[105].first_ts().unwrap_or(0));
    let (footprints, bars) = time(10, || series.bars_by(week.0, week.1, |ts| ts.div_euclid(300) * 300));
    println!("\nQueries");
    println!("  volume at price, the whole year       {whole:>10.1?}  ({} prices)", ladder.levels.len());
    println!("  side totals, the whole year           {sides:>10.1?}");
    println!("  volume at price, a random 3 days      {window:>10.1?}  (mean of 200, {} prices on average)", levels / windows.len());
    println!("  5m footprints over a week             {footprints:>10.1?}  ({} bars)", bars.len());

    // The live path: a print into the buffer and the forming minute.
    const CAP: usize = 1_000_000;
    let mut tape = Tape::new(TICK, CAP);
    let mut ts = 1_700_000_000 * 1_000_000_000i64;
    let mut tick = 24_000;
    let mut next = |rng: &mut Rng| {
        ts += 5_000_000 + rng.below(20_000_000) as i64; // ~4,000 a minute
        tick += rng.below(3) as i32 - 1;
        let side = if rng.below(2) == 0 { Side::Buy } else { Side::Sell };
        Trade { ts_nanos: ts, price_tick: tick, size: 1 + rng.below(4), side }
    };
    let mut sealed = 0;
    for _ in 0..2 * CAP {
        sealed += usize::from(tape.push(next(&mut rng)).is_some());
    }
    let n = 5_000_000;
    let trades: Vec<Trade> = (0..n).map(|_| next(&mut rng)).collect();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let t = Instant::now();
    let mut minutes_sealed = 0u64;
    for trade in &trades {
        if let Some(m) = tape.push(*trade) {
            minutes_sealed += 1;
            std::hint::black_box(m);
        }
    }
    let per = t.elapsed() / n as u32;
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    std::hint::black_box(sealed);
    println!("\nTicks");
    println!("  a trade                               {:>8} bytes", std::mem::size_of::<Trade>());
    println!("  per million trades held               {:>8.1} MB", (1_000_000 * std::mem::size_of::<Trade>()) as f64 / 1048576.0);
    println!("  push one trade, buffer full           {per:>10.1?}  (into the buffer and the forming minute)");
    // A new minute's ladder, and its growth past the first 32 prices: per
    // minute, never per trade.
    println!(
        "  allocations                           {allocations} over {n} trades: {:.1} per minute sealed ({minutes_sealed})",
        allocations as f64 / minutes_sealed.max(1) as f64
    );

    let _ = std::fs::remove_dir_all(&dir);
}
