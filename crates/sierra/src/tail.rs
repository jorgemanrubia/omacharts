//! Following one `.scid` while Sierra Chart appends to it.
//!
//! Sierra Chart writes a symbol's new trades in batches, every "Intraday
//! File Flush Time" (a setting of its own), with ordinary writes that
//! inotify sees even under Wine. A [`Tail`] sleeps in `poll(2)` on an
//! inotify descriptor until one lands, then reads exactly the whole records
//! the batch added with one `pread` into a buffer it keeps, so a
//! subscription costs nothing between batches and allocates nothing once
//! the buffer has grown to a batch's size. Nothing already read is read
//! again: a record half written is simply not read until the rest of it is
//! there.
//!
//! The data folder is watched too, so a file that is replaced (written
//! anew and renamed over the old one) or removed is noticed, and a file
//! that shrinks is reported rather than read past. In each case the caller
//! reloads from scratch; a tail never guesses which of its records still
//! stand.
//!
//! A [`StopHandle`] wakes a waiting tail from another thread, so dropping a
//! subscription never has to wait out a timeout.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::scid::{HEADER_SIZE, RECORD_SIZE, Record, as_bytes_mut, check_header};

/// The most records one [`Tail::wait`] hands back. A tail started far
/// behind the end of a big file catches up over several calls, each of
/// which returns at once, rather than reading gigabytes into one buffer.
pub const MAX_BATCH: usize = 1 << 18;

/// What a wait found.
#[derive(Debug, PartialEq)]
pub enum Appended<'a> {
    /// Whole records added since the last call, oldest first. Valid until
    /// the next call.
    Records(&'a [Record]),
    /// Nothing arrived before the timeout.
    Idle,
    /// The file is shorter than what was already read. Reload it.
    Truncated,
    /// The path now names a different file. Reload it.
    Replaced,
    /// Nothing is at the path any more.
    Removed,
    /// [`StopHandle::stop`] was called. Every later wait says so too.
    Stopped,
}

/// Wakes a [`Tail`] that is waiting, from any thread, for good.
#[derive(Clone, Debug)]
pub struct StopHandle {
    fd: Arc<OwnedFd>,
}

impl StopHandle {
    /// Stops the tail: the wait in progress, if any, returns
    /// [`Appended::Stopped`] at once, and so does every later one.
    pub fn stop(&self) {
        let one: u64 = 1;
        // SAFETY: writing eight bytes from a live u64 to an eventfd we own.
        unsafe { libc::write(self.fd.as_raw_fd(), (&one as *const u64).cast(), 8) };
    }
}

/// A `.scid` followed from a record onwards.
#[derive(Debug)]
pub struct Tail {
    path: PathBuf,
    name: Vec<u8>,
    file: File,
    identity: (u64, u64),
    next: u64,
    inotify: OwnedFd,
    stop: Arc<OwnedFd>,
    buffer: Vec<Record>,
    events: Vec<u8>,
    gone: bool,
}

const FILE_EVENTS: u32 = libc::IN_MODIFY | libc::IN_CLOSE_WRITE | libc::IN_ATTRIB | libc::IN_MOVE_SELF | libc::IN_DELETE_SELF;
const DIR_EVENTS: u32 = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_MOVED_FROM | libc::IN_DELETE;

fn cvt(r: libc::c_int) -> io::Result<libc::c_int> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(r) }
}

fn c_path(p: &Path) -> io::Result<CString> {
    CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a path with a NUL in it"))
}

impl Tail {
    /// Starts following a `.scid` from record `from_record` (0 for all of
    /// it, or [`Scid::len`](crate::Scid::len) for only what comes next).
    /// The header is checked, unless the file is still shorter than one.
    pub fn open(path: impl AsRef<Path>, from_record: usize) -> io::Result<Tail> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let meta = file.metadata()?;
        if meta.len() >= HEADER_SIZE as u64 {
            let mut h = [0u8; HEADER_SIZE];
            file.read_exact_at(&mut h, 0)?;
            check_header(&h)?;
        }
        let dir = match path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let name = path.file_name().map(|n| n.as_bytes().to_vec()).unwrap_or_default();

        // SAFETY: plain syscalls; each descriptor is owned as soon as it exists.
        let inotify = unsafe { OwnedFd::from_raw_fd(cvt(libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC))?) };
        let stop = unsafe { OwnedFd::from_raw_fd(cvt(libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC))?) };
        let cp = c_path(&path)?;
        let cd = c_path(&dir)?;
        unsafe {
            cvt(libc::inotify_add_watch(inotify.as_raw_fd(), cp.as_ptr(), FILE_EVENTS))?;
            cvt(libc::inotify_add_watch(inotify.as_raw_fd(), cd.as_ptr(), DIR_EVENTS))?;
        }
        Ok(Tail {
            path,
            name,
            file,
            identity: (meta.dev(), meta.ino()),
            next: HEADER_SIZE as u64 + from_record as u64 * RECORD_SIZE as u64,
            inotify,
            stop: Arc::new(stop),
            buffer: Vec::new(),
            events: vec![0u8; 16 * 1024],
            gone: false,
        })
    }

    /// The file being followed.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many records have been handed out, counting the ones skipped by
    /// starting later: the index of the next record to come.
    pub fn position(&self) -> usize {
        ((self.next - HEADER_SIZE as u64) / RECORD_SIZE as u64) as usize
    }

    /// A handle that stops this tail from another thread.
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle { fd: self.stop.clone() }
    }

    /// Waits up to `timeout` for whole records to be appended, and returns
    /// them. Records already there return at once.
    pub fn wait(&mut self, timeout: Duration) -> io::Result<Appended<'_>> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.stopped() {
                return Ok(Appended::Stopped);
            }
            match self.check()? {
                Check::Ready(n) => return self.read(n),
                Check::Nothing => {}
                Check::Truncated => return Ok(Appended::Truncated),
                Check::Replaced => return Ok(Appended::Replaced),
                Check::Removed => return Ok(Appended::Removed),
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Appended::Idle);
            }
            let mut fds = [
                libc::pollfd { fd: self.inotify.as_raw_fd(), events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.stop.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            let ms = left.as_millis().clamp(1, i32::MAX as u128) as i32;
            // SAFETY: two valid pollfds, owned by this frame.
            let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) };
            if r < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if fds[0].revents != 0 {
                self.drain();
            }
        }
    }

    fn stopped(&self) -> bool {
        let mut fds = [libc::pollfd { fd: self.stop.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        // SAFETY: one valid pollfd; a zero timeout never blocks.
        unsafe { libc::poll(fds.as_mut_ptr(), 1, 0) > 0 }
    }

    /// Reads every pending inotify event, noting whether the file's own
    /// name came or went in the directory.
    fn drain(&mut self) {
        loop {
            // SAFETY: reading into a buffer we own, of the length we pass.
            let n = unsafe { libc::read(self.inotify.as_raw_fd(), self.events.as_mut_ptr().cast(), self.events.len()) };
            if n <= 0 {
                return;
            }
            let mut at = 0usize;
            let header = std::mem::size_of::<libc::inotify_event>();
            while at + header <= n as usize {
                let ev = &self.events[at..];
                let mask = u32::from_ne_bytes(ev[4..8].try_into().unwrap());
                let len = u32::from_ne_bytes(ev[12..16].try_into().unwrap()) as usize;
                let name = &ev[header..header + len];
                let name = &name[..name.iter().position(|&b| b == 0).unwrap_or(len)];
                if mask & (libc::IN_DELETE_SELF | libc::IN_MOVE_SELF) != 0
                    || (mask & (libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_CREATE | libc::IN_MOVED_TO) != 0 && name == self.name)
                {
                    self.gone = true;
                }
                at += header + len;
            }
        }
    }

    fn check(&mut self) -> io::Result<Check> {
        if self.gone {
            match std::fs::metadata(&self.path) {
                Ok(m) if (m.dev(), m.ino()) == self.identity => self.gone = false,
                Ok(_) => return Ok(Check::Replaced),
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Check::Removed),
                Err(e) => return Err(e),
            }
        }
        let len = self.file.metadata()?.len();
        if len < self.next {
            return Ok(Check::Truncated);
        }
        let whole = ((len - self.next) / RECORD_SIZE as u64) as usize;
        Ok(if whole == 0 { Check::Nothing } else { Check::Ready(whole.min(MAX_BATCH)) })
    }

    fn read(&mut self, n: usize) -> io::Result<Appended<'_>> {
        if self.next == HEADER_SIZE as u64 {
            let mut h = [0u8; HEADER_SIZE];
            self.file.read_exact_at(&mut h, 0)?;
            check_header(&h)?;
        }
        self.buffer.resize(n, Record::default());
        self.file.read_exact_at(as_bytes_mut(&mut self.buffer), self.next)?;
        self.next += (n * RECORD_SIZE) as u64;
        Ok(Appended::Records(&self.buffer))
    }
}

enum Check {
    Ready(usize),
    Nothing,
    Truncated,
    Replaced,
    Removed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    use std::io::Write;

    fn append(path: &Path, b: &[u8]) {
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(b).unwrap();
    }

    fn records(a: Appended<'_>) -> Vec<Record> {
        match a {
            Appended::Records(r) => r.to_vec(),
            other => panic!("expected records, got {other:?}"),
        }
    }

    #[test]
    fn a_tail_sees_records_appended_after_it_started() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        let old = [trade(1, 1.0, 1, true)];
        scid_file(&path, &old);
        let mut tail = Tail::open(&path, 1).unwrap();
        assert_eq!(tail.wait(Duration::from_millis(20)).unwrap(), Appended::Idle);

        let new = [trade(2, 2.0, 1, true), trade(3, 3.0, 2, false)];
        let writer = {
            let path = path.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                append(&path, &bytes(&new));
            })
        };
        let started = Instant::now();
        assert_eq!(records(tail.wait(Duration::from_secs(5)).unwrap()), new);
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(tail.position(), 3);
        writer.join().unwrap();
    }

    #[test]
    fn a_tail_from_the_start_hands_over_what_is_there_at_once() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        let old = [trade(1, 1.0, 1, true), trade(2, 1.0, 1, true)];
        scid_file(&path, &old);
        let mut tail = Tail::open(&path, 0).unwrap();
        assert_eq!(records(tail.wait(Duration::ZERO).unwrap()), old);
    }

    #[test]
    fn a_record_written_in_two_pieces_arrives_once_it_is_whole() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        scid_file(&path, &[]);
        let mut tail = Tail::open(&path, 0).unwrap();
        let rs = [trade(1, 1.0, 1, true), trade(2, 2.0, 1, true)];
        let b = bytes(&rs);
        append(&path, &b[..RECORD_SIZE + 13]);
        assert_eq!(records(tail.wait(Duration::from_secs(1)).unwrap()), &rs[..1]);
        assert_eq!(tail.wait(Duration::from_millis(30)).unwrap(), Appended::Idle);
        append(&path, &b[RECORD_SIZE + 13..]);
        assert_eq!(records(tail.wait(Duration::from_secs(1)).unwrap()), &rs[1..]);
    }

    #[test]
    fn a_file_that_shrinks_is_reported_truncated() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        scid_file(&path, &[trade(1, 1.0, 1, true), trade(2, 1.0, 1, true)]);
        let mut tail = Tail::open(&path, 2).unwrap();
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(HEADER_SIZE as u64).unwrap();
        assert_eq!(tail.wait(Duration::from_secs(1)).unwrap(), Appended::Truncated);
    }

    #[test]
    fn a_file_renamed_over_is_reported_replaced_and_a_removed_one_removed() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        scid_file(&path, &[trade(1, 1.0, 1, true)]);
        let mut tail = Tail::open(&path, 1).unwrap();
        let fresh = dir.join("ES.scid.new");
        scid_file(&fresh, &[trade(1, 1.0, 1, true), trade(2, 1.0, 1, true)]);
        std::fs::rename(&fresh, &path).unwrap();
        assert_eq!(tail.wait(Duration::from_secs(1)).unwrap(), Appended::Replaced);

        let mut tail = Tail::open(&path, 2).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(tail.wait(Duration::from_secs(1)).unwrap(), Appended::Removed);
    }

    #[test]
    fn a_stop_handle_wakes_a_waiting_tail() {
        let dir = Scratch::new();
        let path = dir.join("ES.scid");
        scid_file(&path, &[]);
        let mut tail = Tail::open(&path, 0).unwrap();
        let stop = tail.stop_handle();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            stop.stop();
        });
        let started = Instant::now();
        assert_eq!(tail.wait(Duration::from_secs(30)).unwrap(), Appended::Stopped);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(tail.wait(Duration::from_secs(30)).unwrap(), Appended::Stopped);
        t.join().unwrap();
    }
}
