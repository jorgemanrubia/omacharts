//! Follows a `.scid` as Sierra Chart appends to it and prints what each
//! wake brought:
//!
//! ```text
//! cargo run --release -p sierra --example follow -- <path.scid> [seconds]
//! ```

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sierra::{Appended, Scid, Tail, civil_from_unix_seconds};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("a path to a .scid");
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(60);
    let start_at = Scid::open(&path)?.len();
    let mut tail = Tail::open(&path, start_at)?;
    println!("following {path} from record {start_at} for {seconds}s");
    let began = Instant::now();
    let end = began + Duration::from_secs(seconds);
    let mut last_wake: Option<Instant> = None;
    let (mut wakes, mut total) = (0usize, 0usize);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tail.wait(left)? {
            Appended::Records(records) => {
                let now = Instant::now();
                let gap = last_wake.map(|w| format!("{:>6} ms", (now - w).as_millis())).unwrap_or_else(|| "     -   ".into());
                last_wake = Some(now);
                wakes += 1;
                total += records.len();
                let newest = records.last().unwrap();
                let micros = newest.unix_micros();
                let secs = micros.div_euclid(1_000_000);
                let (y, m, d) = civil_from_unix_seconds(secs);
                let tod = secs.rem_euclid(86_400);
                let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_micros() as i64;
                println!(
                    "{:>8.3}s  gap {gap}  {:>6} records  newest {y}-{m:02}-{d:02} {:02}:{:02}:{:02}.{:06} UTC ({:.1}s behind)  close {}",
                    (now - began).as_secs_f64(),
                    records.len(),
                    tod / 3600,
                    tod / 60 % 60,
                    tod % 60,
                    micros.rem_euclid(1_000_000),
                    (wall - micros) as f64 / 1e6,
                    newest.close
                );
            }
            Appended::Idle => break,
            other => {
                println!("{other:?}");
                break;
            }
        }
    }
    println!("{wakes} wakes, {total} records in {:.1}s", began.elapsed().as_secs_f64());
    Ok(())
}
