//! Times what the engine does with Sierra Chart's files:
//!
//! ```text
//! cargo run --release -p sierra --example bench -- [--cold] <file.scid>... [<file.dly>...]
//! ```
//!
//! `--cold` first asks the kernel to drop each file's own pages from the
//! page cache (`posix_fadvise(DONTNEED)`, which needs no privileges and
//! changes nothing in the file), so the first pass reads from disk. Only
//! use it on files Sierra Chart is not writing to.

use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::{Duration, Instant};

use sierra::fold::{self, Bar};
use sierra::{Install, Record, Scale, Scid};

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let v = f();
    (v, t.elapsed())
}

fn row(what: &str, d: Duration, records: usize, note: &str) {
    let rate = if d.as_secs_f64() > 0.0 && records > 0 { format!("{:>8.0} M rec/s", records as f64 / d.as_secs_f64() / 1e6) } else { String::new() };
    println!("  {what:<34} {:>10.3} ms  {rate:>16}  {note}", d.as_secs_f64() * 1e3);
}

fn drop_cache(path: &Path) {
    if let Ok(f) = std::fs::File::open(path) {
        // SAFETY: an advisory call on a descriptor we own.
        unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) };
    }
}

fn scale_for(path: &Path) -> (Scale, String) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let root = path.parent().and_then(Path::parent);
    if let Some(install) = root.and_then(|r| Install::open(r).ok()) {
        let (found, d) = time(|| install.symbol_settings(stem));
        let (_, warm) = time(|| install.symbol_settings(stem));
        if let Some(found) = found {
            let file = found.file.file_name().unwrap_or_default().to_string_lossy().into_owned();
            return (
                found.scale,
                format!("{} via {} in {file} (lookup {:.1} ms first, {:.3} ms cached)", stem, found.symbol, d.as_secs_f64() * 1e3, warm.as_secs_f64() * 1e3),
            );
        }
    }
    (Scale { multiplier: 0.01, tick_size: 0.25 }, format!("{stem}: no settings found, assuming ES"))
}

/// Volume at price per minute, without a hash map: each minute's levels
/// go into a dense scratch array of ticks around the minute's first price,
/// and are written out as (tick, bid, ask) runs when the minute ends.
struct Profile {
    minutes: Vec<(i64, u32)>,
    levels: Vec<(i32, u64, u64)>,
}

fn profile(records: &[Record], multiplier: f64, tick: f64) -> Profile {
    const WIDTH: usize = 1 << 14;
    let per_tick = multiplier / tick;
    let mut out = Profile { minutes: Vec::new(), levels: Vec::new() };
    let mut scratch = vec![(0u64, 0u64); WIDTH];
    let (mut anchor, mut lo, mut hi) = (0i64, usize::MAX, 0usize);
    let mut minute = i64::MIN;
    let (mut start_dt, mut end_dt) = (i64::MAX, i64::MIN);
    let flush = |scratch: &mut [(u64, u64)], anchor: i64, lo: usize, hi: usize, out: &mut Profile| {
        if lo > hi {
            return;
        }
        for (i, slot) in scratch[lo..=hi].iter_mut().enumerate() {
            if slot.0 | slot.1 != 0 {
                out.levels.push(((anchor + (lo + i) as i64) as i32, slot.0, slot.1));
                *slot = (0, 0);
            }
        }
    };
    for r in records {
        if r.close.is_nan() || r.close <= 0.0 {
            continue;
        }
        if r.datetime < start_dt || r.datetime >= end_dt {
            let m = r.unix_seconds().div_euclid(60) * 60;
            if m != minute {
                flush(&mut scratch, anchor, lo, hi, &mut out);
                minute = m;
                out.minutes.push((m, out.levels.len() as u32));
                anchor = (f64::from(r.close) * per_tick).round() as i64 - (WIDTH / 2) as i64;
                (lo, hi) = (usize::MAX, 0);
            }
            start_dt = sierra::sc_from_unix_micros(m * 1_000_000);
            end_dt = start_dt + 60_000_000;
        }
        let t = (f64::from(r.close) * per_tick).round() as i64 - anchor;
        if !(0..WIDTH as i64).contains(&t) {
            continue;
        }
        let t = t as usize;
        lo = lo.min(t);
        hi = hi.max(t);
        scratch[t].0 += u64::from(r.bid_volume);
        scratch[t].1 += u64::from(r.ask_volume);
    }
    flush(&mut scratch, anchor, lo, hi, &mut out);
    out
}

fn bench_scid(path: &Path, cold: bool, recent_first: bool) -> std::io::Result<()> {
    let (scale, note) = scale_for(path);
    println!("\n{}  ({:.2} GB)", path.display(), std::fs::metadata(path)?.len() as f64 / 1e9);
    println!("  {note}: multiplier {}, tick {}", scale.multiplier, scale.tick_size);
    for pass in if cold { ["cold", "warm"].as_slice() } else { ["warm"].as_slice() } {
        if *pass == "cold" {
            drop_cache(path);
        }
        println!("  -- {pass}");
        let (scid, d) = time(|| Scid::open(path));
        let scid = scid?;
        let n = scid.len();
        row("open + mmap", d, 0, &format!("{n} records"));
        if recent_first {
            // What a chart opening on a contract does: the last month only.
            let since = scid.records().last().map_or(0, |r| r.unix_seconds() - 30 * 86_400);
            let (b, d) = time(|| fold::bars(scid.from(since), 60, scale.multiplier));
            row("first thing: 1m bars, last 30 days", d, scid.from(since).len(), &format!("{} bars", b.len()));
        }
        let records = scid.records();
        let (sum, d) = time(|| {
            std::thread::scope(|s| {
                let size = records.len().div_ceil(32).max(1);
                let parts: Vec<_> = records.chunks(size).map(|c| s.spawn(move || c.iter().map(|r| u64::from(r.volume)).sum::<u64>())).collect();
                parts.into_iter().map(|h| h.join().unwrap()).sum::<u64>()
            })
        });
        row("full pass, parallel (sum volume)", d, n, &format!("volume {sum}"));
        let (bars, d) = time(|| fold::bars(records, 60, scale.multiplier));
        row("1m bars, parallel", d, n, &format!("{} bars", bars.len()));
        let (serial, d) = time(|| fold::bars_serial(records, 60, scale.multiplier));
        row("1m bars, serial", d, n, &format!("{} bars, identical: {}", serial.len(), serial == bars));
        let (sum, d) = time(|| records.iter().map(|r| u64::from(r.volume)).sum::<u64>());
        row("full pass, serial (sum volume)", d, n, &format!("volume {sum}"));
        if let Some(last) = records.last() {
            let since = last.unix_seconds() - 30 * 86_400;
            let (recent, d) = time(|| scid.from(since));
            row("from(30 days ago) binary search", d, 0, &format!("{} records", recent.len()));
            let (b, d) = time(|| fold::bars(recent, 60, scale.multiplier));
            row("1m bars, last 30 days", d, recent.len(), &format!("{} bars", b.len()));
            let (p, d) = time(|| profile(recent, scale.multiplier, scale.tick_size));
            row("1m volume at price, last 30 days", d, recent.len(), &format!("{} minutes, {} levels", p.minutes.len(), p.levels.len()));
            let session = |s: i64| (s - 22 * 3600).div_euclid(86_400) * 86_400 + 22 * 3600;
            let (b, d) = time(|| fold::bars_with(records, scale.multiplier, session));
            row("bars_with(22:00 UTC days), all", d, n, &format!("{} bars", b.len()));
        }
        if *pass == "warm" {
            report_oddities(records, &bars);
        }
    }
    Ok(())
}

/// What in a file is not what a tick file should be.
fn report_oddities(records: &[Record], bars: &[Bar]) {
    let mut backwards = 0usize;
    let mut equal = 0usize;
    let mut not_ticks = 0usize;
    let mut first_sub = 0usize;
    let mut last_sub = 0usize;
    let mut bad_close = 0usize;
    let mut multi_trade = 0usize;
    let mut vol_mismatch = 0usize;
    let mut prev = i64::MIN;
    for r in records {
        if r.datetime < prev {
            backwards += 1;
        } else if r.datetime == prev {
            equal += 1;
        }
        prev = r.datetime;
        if !r.is_tick() {
            not_ticks += 1;
        }
        first_sub += usize::from(r.is_first_sub_trade());
        last_sub += usize::from(r.is_last_sub_trade());
        bad_close += usize::from(!(r.close > 0.0 && r.close.is_finite()));
        multi_trade += usize::from(r.trades != 1);
        vol_mismatch += usize::from(u64::from(r.bid_volume) + u64::from(r.ask_volume) != u64::from(r.volume));
    }
    println!(
        "  oddities: {backwards} backwards, {equal} equal timestamps, {not_ticks} non-tick records, {first_sub}/{last_sub} first/last sub-trades, {bad_close} bad closes, {multi_trade} with trades != 1, {vol_mismatch} with bid+ask != volume"
    );
    if let (Some(first), Some(last)) = (bars.first(), bars.last()) {
        let (y0, m0, d0) = sierra::civil_from_unix_seconds(first.start);
        let (y1, m1, d1) = sierra::civil_from_unix_seconds(last.start);
        println!("  spans {y0}-{m0:02}-{d0:02} to {y1}-{m1:02}-{d1:02}; last 1m close {}", last.close);
    }
}

fn bench_dly(path: &Path) -> std::io::Result<()> {
    println!("\n{}  ({} KB)", path.display(), std::fs::metadata(path)?.len() / 1024);
    let (days, d) = time(|| sierra::dly::read(path));
    let days = days?;
    row("dly::read", d, 0, &format!("{} days", days.len()));
    let (days2, d) = time(|| sierra::dly::read(path));
    row("dly::read again", d, 0, &format!("{} days", days2?.len()));
    let bytes = std::fs::read(path)?;
    let best = (0..50).map(|_| time(|| sierra::dly::parse(&bytes)).1).min().unwrap();
    row("dly::parse, in memory, best of 50", best, 0, &format!("{:.0} MB/s", bytes.len() as f64 / best.as_secs_f64() / 1e6));
    if let Some(last) = days.last() {
        let (recent, d) = time(|| sierra::dly::read_since(path, last.year - 1, last.month, last.day));
        row("dly::read_since(a year back)", d, 0, &format!("{} days", recent?.len()));
        println!("  last: {last:?}");
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut cold = false;
    let mut recent_first = false;
    let mut paths = Vec::new();
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--cold" => cold = true,
            "--recent-first" => recent_first = true,
            _ => paths.push(a),
        }
    }
    for p in &paths {
        let path = Path::new(p);
        match path.extension().and_then(|e| e.to_str()) {
            Some("dly") => bench_dly(path)?,
            _ => bench_scid(path, cold, recent_first)?,
        }
    }
    let (found, d) = time(sierra::candidates);
    row("\ncandidates()", d, 0, &format!("{found:?}"));
    if let Some(install) = found.first().and_then(|r| Install::open(r).ok()) {
        let (listing, d) = time(|| install.listing());
        row("listing()", d, 0, &format!("{} symbols, {} roots", listing.len(), listing.roots().count()));
    }
    Ok(())
}
