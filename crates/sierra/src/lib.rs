//! Sierra Chart's own files, read the way Sierra Chart writes them.
//!
//! Sierra Chart keeps everything it records in plain files in its data
//! folder: one `.scid` per symbol with every trade it has seen, one `.dly`
//! with a bar per day, and an XML file per data service that says how each
//! symbol's prices are stored. Reading those files directly is the fastest
//! way there is to chart what Sierra Chart has, and it needs nothing from
//! Sierra Chart beyond the files being there: no DTC connection, no study,
//! no export step.
//!
//! **Nothing here writes.** An install is somebody's working trading
//! platform, usually running while it is read, so every file is opened read
//! only and nothing is ever created, renamed or locked inside it.
//!
//! **An intraday file is a slice.** A `.scid` is a 56-byte header followed
//! by 40-byte records, written in the machine's own little-endian layout,
//! so [`Scid`] maps the file and hands out `&[Record]` without copying or
//! parsing a byte. A contract of eighty million trades is open as soon as
//! the map is, and [`fold::bars`] turns it into bars at memory speed, on
//! every core when there is enough of it to be worth the threads.
//!
//! **Prices are stored scaled.** Sierra Chart writes ES at 7647.00 as
//! 764700.0, and leaves it to the symbol's settings to say so: a file price
//! times the symbol's real-time multiplier is the real price.
//! [`Install::scale`] finds that multiplier and the tick size the same way
//! Sierra Chart would, from the service files in `SymbolSettings`.
//!
//! **Live data is followed, not polled.** Sierra Chart appends to the file
//! of every symbol it is receiving, in batches. A [`Tail`] sleeps on inotify
//! until a batch lands and then reads only the bytes that batch added, so
//! following a contract costs nothing between trades and one `pread` per
//! batch.
//!
//! This crate depends on nothing in the app: the engine depends on it.

#[cfg(target_endian = "big")]
compile_error!("Sierra Chart's files are little-endian and are mapped as they are, so this crate is little-endian only");

pub mod dly;
pub mod fold;
pub mod install;
pub mod scid;
pub mod settings;
pub mod symbols;
pub mod tail;
mod time;

pub use dly::Daily;
pub use fold::Bar;
pub use install::{Install, Problem, candidates};
pub use scid::{Record, Scid, sc_from_unix_micros, unix_micros_from_sc};
pub use settings::{Scale, SymbolSettings};
pub use symbols::{Contract, Entry, FileSymbol, Intraday, Listing, parse};
pub use tail::{Appended, StopHandle, Tail};
pub use time::{civil_from_unix_seconds, unix_seconds_from_civil};

#[cfg(test)]
pub(crate) mod testing {
    //! Fixture files for the tests, written into a directory of their own
    //! that is removed when the test is over.

    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::scid::{HEADER_SIZE, RECORD_SIZE, Record};

    /// A temporary directory, gone when dropped.
    pub struct Scratch(pub PathBuf);

    impl Scratch {
        pub fn new() -> Scratch {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("sierra-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A `.scid` header as Sierra Chart writes it.
    pub fn header() -> Vec<u8> {
        let mut h = vec![0u8; HEADER_SIZE];
        h[0..4].copy_from_slice(b"SCID");
        h[4..8].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        h[8..12].copy_from_slice(&(RECORD_SIZE as u32).to_le_bytes());
        h[12..14].copy_from_slice(&1u16.to_le_bytes());
        h
    }

    /// Records as the bytes the file holds.
    pub fn bytes(records: &[Record]) -> Vec<u8> {
        let mut out = Vec::with_capacity(records.len() * RECORD_SIZE);
        for r in records {
            out.extend_from_slice(&r.datetime.to_le_bytes());
            for f in [r.open, r.high, r.low, r.close] {
                out.extend_from_slice(&f.to_le_bytes());
            }
            for u in [r.trades, r.volume, r.bid_volume, r.ask_volume] {
                out.extend_from_slice(&u.to_le_bytes());
            }
        }
        out
    }

    /// A whole `.scid` file: its header and these records.
    pub fn scid_file(path: &Path, records: &[Record]) {
        let mut b = header();
        b.extend(bytes(records));
        std::fs::write(path, b).unwrap();
    }

    /// One trade at a unix second, as a tick file records it.
    pub fn trade(unix_seconds: i64, price: f32, size: u32, at_ask: bool) -> Record {
        Record {
            datetime: crate::sc_from_unix_micros(unix_seconds * 1_000_000),
            open: 0.0,
            high: price + 25.0,
            low: price - 25.0,
            close: price,
            trades: 1,
            volume: size,
            bid_volume: if at_ask { 0 } else { size },
            ask_volume: if at_ask { size } else { 0 },
        }
    }
}
