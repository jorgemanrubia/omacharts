//! A tail fed bytes the way Sierra Chart wrote them.
//!
//! `fixtures/GCV26_FUT_CME.records` is the first 300 records Sierra Chart
//! appended to a gold contract while it downloaded history, copied from
//! the author's install: real records, unbundled-trade markers and all.
//! They are written to a fresh file in uneven pieces, several cut in the
//! middle of a record, and the tail must hand back exactly those records,
//! once each and in order.

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use sierra::{Appended, Record, Scid, Tail};

const RECORDS: &[u8] = include_bytes!("fixtures/GCV26_FUT_CME.records");

fn header() -> Vec<u8> {
    let mut h = vec![0u8; 56];
    h[0..4].copy_from_slice(b"SCID");
    h[4..8].copy_from_slice(&56u32.to_le_bytes());
    h[8..12].copy_from_slice(&40u32.to_le_bytes());
    h[12..14].copy_from_slice(&1u16.to_le_bytes());
    h
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sierra-replay-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_tail_hands_back_every_record_sierra_chart_appended_once_and_in_order() {
    let dir = scratch("tail");
    let path = dir.join("GCV26_FUT_CME.scid");
    std::fs::write(&path, header()).unwrap();

    let whole = {
        let mut b = header();
        b.extend_from_slice(RECORDS);
        let copy = dir.join("whole.scid");
        std::fs::write(&copy, b).unwrap();
        Scid::open(&copy).unwrap().records().to_vec()
    };
    assert_eq!(whole.len(), 300);
    assert!(whole.windows(2).all(|w| w[0].datetime <= w[1].datetime));
    assert!(whole.iter().all(Record::is_tick));

    let mut tail = Tail::open(&path, 0).unwrap();
    let writer = {
        let path = path.clone();
        std::thread::spawn(move || {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            let mut at = 0;
            for (i, piece) in [1, 39, 40, 41, 999, 7, 4000, 33, 1280].iter().cycle().enumerate() {
                if at >= RECORDS.len() {
                    break;
                }
                let end = (at + piece).min(RECORDS.len());
                f.write_all(&RECORDS[at..end]).unwrap();
                at = end;
                if i % 3 == 0 {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        })
    };
    let mut seen: Vec<Record> = Vec::new();
    while seen.len() < whole.len() {
        match tail.wait(Duration::from_secs(5)).unwrap() {
            Appended::Records(r) => seen.extend_from_slice(r),
            other => panic!("expected records, got {other:?} after {}", seen.len()),
        }
    }
    writer.join().unwrap();
    assert_eq!(seen, whole);
    assert_eq!(tail.wait(Duration::from_millis(20)).unwrap(), Appended::Idle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn real_records_fold_into_bars_at_their_real_prices() {
    let dir = scratch("fold");
    let path = dir.join("GCV26_FUT_CME.scid");
    let mut b = header();
    b.extend_from_slice(RECORDS);
    std::fs::write(&path, b).unwrap();
    let scid = Scid::open(&path).unwrap();
    let records = scid.records();
    let bars = sierra::fold::bars(records, 60, 0.1);
    assert!(!bars.is_empty());
    let volume: u64 = records.iter().map(|r| u64::from(r.volume)).sum();
    assert_eq!(bars.iter().map(|b| b.volume).sum::<u64>(), volume);
    // Gold is stored in tenths: 43459 is 4345.9.
    assert_eq!(bars[0].open, 4345.9);
    let _ = std::fs::remove_dir_all(&dir);
}
