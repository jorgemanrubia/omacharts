//! Intraday files: `.scid`, every trade Sierra Chart has recorded for a
//! symbol.
//!
//! The file is a 56-byte header and then 40-byte records, each one exactly
//! the [`Record`] struct below in little-endian memory order. So the file is
//! mapped and its records are a slice: opening an 80-million-record contract
//! costs the `mmap` call, and only the pages something reads ever come off
//! the disk. The header is 56 bytes, a multiple of the record's 8-byte
//! alignment, and a map starts on a page, so the cast is sound.
//!
//! In a tick file, which is what Sierra Chart records unless told
//! otherwise, a record is one trade: `close` is the trade price, `high` and
//! `low` the ask and bid at the time, `volume` its size, and `bid_volume` or
//! `ask_volume` whichever side the aggressor hit. `open` is 0, or one of the
//! two markers Sierra Chart uses to say a trade was one part of a larger
//! one split across price levels. Ticks downloaded from Sierra Chart's
//! historical servers, rather than recorded live, can carry the trade price
//! in `high` and `low` as well, so neither is a reliable quote. A file can
//! also hold bars Sierra Chart aggregated itself, where every field means
//! what its name says; [`Record::is_tick`] tells the two apart.
//!
//! Timestamps are UTC and never go backwards. Sierra Chart bumps the
//! microseconds of trades in the same instant to keep them apart, though
//! not always (a few hundred of the 63 million in one ES contract share a
//! timestamp), so the slice is sorted but not strictly, and [`Scid::from`]
//! can binary-search it.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use memmap2::{Mmap, MmapOptions};

/// The header every `.scid` starts with, in bytes.
pub const HEADER_SIZE: usize = 56;
/// One record, in bytes.
pub const RECORD_SIZE: usize = 40;

/// Microseconds from Sierra Chart's epoch, 1899-12-30 00:00 UTC, to the
/// unix epoch.
pub const SC_UNIX_OFFSET_MICROS: i64 = 25_569 * 86_400 * 1_000_000;

/// `open` of the first piece of a trade Sierra Chart split across prices:
/// the float Sierra Chart writes as `-1.99900095e+37f`.
pub const FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE: f32 = f32::from_bits(0xfd70_9ef9);
/// `open` of the last piece of a trade Sierra Chart split across prices:
/// `-1.99900197e+37f`.
pub const LAST_SUB_TRADE_OF_UNBUNDLED_TRADE: f32 = f32::from_bits(0xfd70_9f01);

/// A unix time in microseconds as Sierra Chart stores it: microseconds
/// since 1899-12-30.
pub const fn sc_from_unix_micros(unix_micros: i64) -> i64 {
    unix_micros + SC_UNIX_OFFSET_MICROS
}

/// A Sierra Chart timestamp as unix microseconds.
pub const fn unix_micros_from_sc(sc: i64) -> i64 {
    sc - SC_UNIX_OFFSET_MICROS
}

/// One record of a `.scid`, laid out exactly as the file holds it.
///
/// Prices are as stored, which means scaled: multiply by the symbol's
/// [`Scale::multiplier`](crate::Scale) for the real price.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Record {
    /// Microseconds since 1899-12-30 00:00 UTC.
    pub datetime: i64,
    /// A bar's open; in a tick file 0 or an unbundled-trade marker.
    pub open: f32,
    /// A bar's high; in a tick file the ask.
    pub high: f32,
    /// A bar's low; in a tick file the bid.
    pub low: f32,
    /// A bar's close; in a tick file the trade price.
    pub close: f32,
    /// How many trades the record stands for; 1 for a tick.
    pub trades: u32,
    /// Contracts traded.
    pub volume: u32,
    /// Contracts that traded at the bid: sellers hitting it.
    pub bid_volume: u32,
    /// Contracts that traded at the ask: buyers lifting it.
    pub ask_volume: u32,
}

const _: () = assert!(std::mem::size_of::<Record>() == RECORD_SIZE);
const _: () = assert!(std::mem::align_of::<Record>() == 8);
const _: () = assert!(HEADER_SIZE.is_multiple_of(std::mem::align_of::<Record>()));

impl Record {
    /// The time as unix microseconds.
    pub const fn unix_micros(&self) -> i64 {
        unix_micros_from_sc(self.datetime)
    }

    /// The time as whole unix seconds, rounded down.
    pub const fn unix_seconds(&self) -> i64 {
        self.unix_micros().div_euclid(1_000_000)
    }

    /// Whether this is one trade, whose price is `close` and whose `high`
    /// and `low` are the ask and bid, rather than a bar.
    pub fn is_tick(&self) -> bool {
        self.open == 0.0 || self.is_first_sub_trade() || self.is_last_sub_trade()
    }

    /// Whether this is the first piece of a trade split across prices.
    pub fn is_first_sub_trade(&self) -> bool {
        self.open == FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE
    }

    /// Whether this is the last piece of a trade split across prices.
    pub fn is_last_sub_trade(&self) -> bool {
        self.open == LAST_SUB_TRADE_OF_UNBUNDLED_TRADE
    }
}

/// Records as bytes, for reading straight into them. Every bit pattern is
/// a valid `Record` (integers and floats, no padding), which is what makes
/// this and the map's cast sound.
pub(crate) fn as_bytes_mut(records: &mut [Record]) -> &mut [u8] {
    // SAFETY: `Record` is `repr(C)`, 40 bytes with no padding, and every
    // byte pattern is a valid value of each of its fields.
    unsafe { std::slice::from_raw_parts_mut(records.as_mut_ptr().cast::<u8>(), std::mem::size_of_val(records)) }
}

/// Checks a header, saying what is wrong with it if anything is.
pub(crate) fn check_header(h: &[u8]) -> io::Result<()> {
    let bad = |why: &str| Err(io::Error::new(io::ErrorKind::InvalidData, format!("not a Sierra Chart intraday file: {why}")));
    if h.len() < HEADER_SIZE {
        return bad("shorter than its header");
    }
    if &h[0..4] != b"SCID" {
        return bad("it does not start with SCID");
    }
    let header_size = u32::from_le_bytes(h[4..8].try_into().unwrap());
    let record_size = u32::from_le_bytes(h[8..12].try_into().unwrap());
    if header_size as usize != HEADER_SIZE {
        return bad(&format!("a {header_size}-byte header, not {HEADER_SIZE}"));
    }
    if record_size as usize != RECORD_SIZE {
        return bad(&format!("{record_size}-byte records, not {RECORD_SIZE}"));
    }
    Ok(())
}

/// A `.scid` file, mapped.
///
/// The map covers the file as long as it was when opened or last
/// [refreshed](Scid::refresh); records Sierra Chart appends after that are
/// not seen until the next refresh. A trailing record still being written
/// is left out.
///
/// A file must not be truncated while it is mapped: reading a page past
/// the new end would fault. Sierra Chart only ever appends, and replaces a
/// file by writing a new one, which leaves this map on the old one.
pub struct Scid {
    path: PathBuf,
    file: File,
    map: Option<Mmap>,
    count: usize,
}

impl std::fmt::Debug for Scid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scid").field("path", &self.path).field("records", &self.count).finish()
    }
}

impl Scid {
    /// Opens and maps a `.scid`, read only. A file that is only a header
    /// has no records; one whose header is not a Sierra Chart intraday
    /// header is an `InvalidData` error.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Scid> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let mut scid = Scid { path, file, map: None, count: 0 };
        let len = scid.file.metadata()?.len();
        if len < HEADER_SIZE as u64 {
            return check_header(&[]).map(|_| scid);
        }
        scid.map_to(len)?;
        check_header(scid.map.as_deref().unwrap_or(&[]))?;
        Ok(scid)
    }

    fn map_to(&mut self, len: u64) -> io::Result<()> {
        let len = usize::try_from(len).map_err(|_| io::Error::other("file too big to map"))?;
        // SAFETY: the map is read only. Sierra Chart appends to these files
        // and never truncates them in place, so the mapped range stays
        // backed by the file; see the type's documentation.
        let map = unsafe { MmapOptions::new().len(len).map(&self.file)? };
        self.count = len.saturating_sub(HEADER_SIZE) / RECORD_SIZE;
        self.map = Some(map);
        Ok(())
    }

    /// The file this was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many whole records the map holds.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether there are no records.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Every whole record, oldest first, straight from the map.
    pub fn records(&self) -> &[Record] {
        let Some(map) = &self.map else { return &[] };
        let body = &map[HEADER_SIZE..HEADER_SIZE + self.count * RECORD_SIZE];
        assert_eq!(body.as_ptr().align_offset(std::mem::align_of::<Record>()), 0);
        // SAFETY: the bytes are in bounds, aligned for `Record` (the map
        // is page-aligned and the header a multiple of 8), live as long as
        // `&self`, and any bit pattern is a valid `Record`.
        unsafe { std::slice::from_raw_parts(body.as_ptr().cast::<Record>(), self.count) }
    }

    /// The records at or after a unix time, found by binary search.
    pub fn from(&self, unix_seconds: i64) -> &[Record] {
        from(self.records(), unix_seconds)
    }

    /// The records in `[start, end)`, unix seconds, found by binary search.
    pub fn between(&self, start: i64, end: i64) -> &[Record] {
        between(self.records(), start, end)
    }

    /// Maps the file again if it has grown, and returns how many records
    /// there are now. A file that shrank is an error: it is not the file
    /// that was opened any more, so reopen it.
    pub fn refresh(&mut self) -> io::Result<usize> {
        let len = self.file.metadata()?.len();
        let mapped = self.map.as_ref().map_or(0, |m| m.len() as u64);
        if len < mapped {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the file shrank since it was opened"));
        }
        let whole = (len.saturating_sub(HEADER_SIZE as u64) / RECORD_SIZE as u64) as usize;
        if whole > self.count {
            self.map_to(len)?;
        }
        Ok(self.count)
    }
}

/// The records at or after a unix time, by binary search over sorted
/// records.
pub fn from(records: &[Record], unix_seconds: i64) -> &[Record] {
    let at = sc_from_unix_micros(unix_seconds.saturating_mul(1_000_000));
    &records[records.partition_point(|r| r.datetime < at)..]
}

/// The records in `[start, end)`, unix seconds, by binary search over
/// sorted records.
pub fn between(records: &[Record], start: i64, end: i64) -> &[Record] {
    let lo = sc_from_unix_micros(start.saturating_mul(1_000_000));
    let hi = sc_from_unix_micros(end.saturating_mul(1_000_000));
    let a = records.partition_point(|r| r.datetime < lo);
    let b = records.partition_point(|r| r.datetime < hi).max(a);
    &records[a..b]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use std::io::Write;

    #[test]
    fn a_header_only_file_has_no_records() {
        let dir = Scratch::new();
        let path = dir.join("ESZ26_FUT_CME.scid");
        scid_file(&path, &[]);
        let scid = Scid::open(&path).unwrap();
        assert_eq!(scid.len(), 0);
        assert!(scid.records().is_empty());
        assert!(scid.from(0).is_empty());
    }

    #[test]
    fn a_file_that_is_not_an_intraday_file_is_refused() {
        let dir = Scratch::new();
        let path = dir.join("bad.scid");
        let mut h = header();
        h[0..4].copy_from_slice(b"SCIX");
        std::fs::write(&path, &h).unwrap();
        assert_eq!(Scid::open(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);

        let mut h = header();
        h[8..12].copy_from_slice(&32u32.to_le_bytes());
        std::fs::write(&path, &h).unwrap();
        assert_eq!(Scid::open(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);

        std::fs::write(&path, b"SCID").unwrap();
        assert_eq!(Scid::open(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        std::fs::write(&path, b"").unwrap();
        assert_eq!(Scid::open(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn records_read_back_as_they_were_written() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        let written = [trade(1_789_084_800, 764_700.0, 3, true), trade(1_789_084_801, 764_725.0, 1, false)];
        scid_file(&path, &written);
        let scid = Scid::open(&path).unwrap();
        assert_eq!(scid.records(), &written);
        assert_eq!(scid.records()[0].unix_seconds(), 1_789_084_800);
        assert!(scid.records()[0].is_tick());
    }

    #[test]
    fn a_trailing_partial_record_is_left_out_until_it_is_whole() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        let records = [trade(100, 1.0, 1, true), trade(101, 2.0, 1, true)];
        let all = bytes(&records);
        let mut b = header();
        b.extend(&all[..RECORD_SIZE + 17]);
        std::fs::write(&path, &b).unwrap();
        let mut scid = Scid::open(&path).unwrap();
        assert_eq!(scid.records(), &records[..1]);

        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&all[RECORD_SIZE + 17..]).unwrap();
        assert_eq!(scid.refresh().unwrap(), 2);
        assert_eq!(scid.records(), &records);
    }

    #[test]
    fn a_header_only_file_finds_its_records_once_they_arrive() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        scid_file(&path, &[]);
        let mut scid = Scid::open(&path).unwrap();
        let r = trade(100, 1.0, 1, true);
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(&bytes(&[r])).unwrap();
        assert_eq!(scid.refresh().unwrap(), 1);
        assert_eq!(scid.records(), &[r]);
    }

    #[test]
    fn from_finds_the_first_record_at_or_after_a_time() {
        let records: Vec<Record> = (0..1000).map(|i| trade(1_000 + i / 3, 1.0, 1, true)).collect();
        assert_eq!(from(&records, 0).len(), 1000);
        assert_eq!(from(&records, 1_000).len(), 1000);
        assert_eq!(from(&records, 1_001).len(), 997);
        assert_eq!(from(&records, 1_333).len(), 1);
        assert!(from(&records, 1_334).is_empty());
        assert_eq!(between(&records, 1_001, 1_003).len(), 6);
        assert!(between(&records, 1_003, 1_001).is_empty());
    }

    #[test]
    fn the_unbundled_trade_markers_are_ticks() {
        let mut r = trade(0, 5.0, 1, true);
        r.open = FIRST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        assert!(r.is_tick() && r.is_first_sub_trade() && !r.is_last_sub_trade());
        assert_eq!(r.open, "-1.99900095e+37".parse::<f32>().unwrap());
        assert_eq!(LAST_SUB_TRADE_OF_UNBUNDLED_TRADE, "-1.99900197e+37".parse::<f32>().unwrap());
        r.open = LAST_SUB_TRADE_OF_UNBUNDLED_TRADE;
        assert!(r.is_tick() && r.is_last_sub_trade());
        r.open = 4.0;
        assert!(!r.is_tick());
    }

    #[test]
    fn sierra_time_and_unix_time_convert_both_ways() {
        // 1970-01-01 is day 25569 of Sierra Chart's calendar.
        assert_eq!(sc_from_unix_micros(0), 25_569 * 86_400 * 1_000_000);
        assert_eq!(unix_micros_from_sc(sc_from_unix_micros(1_234_567)), 1_234_567);
        let r = Record { datetime: sc_from_unix_micros(-1), ..Record::default() };
        assert_eq!(r.unix_seconds(), -1);
    }
}
