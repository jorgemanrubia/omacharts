//! Keeping the US listings current without a new release.
//!
//! The listings workflow rebuilds every US listing each weekday and publishes
//! it on the repository's `listings` branch, beside a small sidecar:
//!
//! ```text
//! listings.tsv    the file tools/build_listings.py writes
//! listings.json   {"built": "2026-11-21", "rows": 11394, "sha256": "…"}
//! ```
//!
//! Once a day, behind the window, the app asks for the sidecar — with the
//! `ETag` it was last given, so an unchanged one is a 304 and nothing more —
//! and downloads the file only when its checksum is not the one already
//! here. A download is used only if it matches that checksum and passes the
//! same checks the workflow held it to ([`validate`]); it is then written
//! next to the store, whole or not at all, and the inventory reads it from
//! there ([`crate::inventory`]). Anything that goes wrong leaves the file
//! that was already here, and search with it, exactly as it was.
//!
//! Which copy is used, the downloaded one or the one compiled in, is decided
//! by the date each was built ([`choose`]), so a release newer than the last
//! download is not shadowed by it.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Where the listings workflow publishes: the `listings` branch, raw.
pub const PUBLISHED: &str = "https://raw.githubusercontent.com/jorgemanrubia/omacharts/listings";

/// Fetch from here instead, for a test or a mirror.
pub const SOURCE_ENV: &str = "OMACHARTS_LISTINGS_URL";

/// The setting behind Preferences → Update symbol listings daily.
pub const SETTING: &str = "listings_update";

/// How long a check stands before the window asks again.
pub const CADENCE: Duration = Duration::from_secs(24 * 60 * 60);

/// How often a window that stays open looks at whether a check is due.
pub const LOOK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// The downloaded file, beside the store.
pub const FILE: &str = "listings.tsv";
/// What was last fetched: the sidecar as published, with its `ETag`.
const KEPT: &str = "listings.json";
/// Its modification time is the last check, whatever it found.
const CHECKED: &str = ".listings-checked";

/// The first line of every listings file.
const HEADER: &str = "# kind\tsymbol\tname\tsuffix\tcurrency\ttier\tsession_origin\tyahoo_override\texchange\tpopularity";

/// The same rules as tools/check_listings.py, which says why each is what it
/// is. Shrinking is the one held more loosely in both directions here:
/// growing is what a market does, and a file a tenth smaller than the one in
/// use is a feed that half answered.
const MIN_ROWS: usize = 9000;
const MIN_RANKED: f64 = 0.6;
const MAX_SHRINK: f64 = 0.10;

/// Larger than any listings file is going to be, and small enough that a
/// server answering with something else entirely is cut off.
const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_SIDECAR: u64 = 64 * 1024;

/// The folder the file is kept in: the store's.
pub fn dir() -> PathBuf {
    crate::store::data_dir()
}

/// The downloaded file in `dir`.
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Where to fetch from: [`SOURCE_ENV`], or [`PUBLISHED`]. In a test build
/// the discard port on the loopback, so that a test reaching a refresh it
/// did not mean to is refused at once rather than fetching from GitHub.
pub fn source() -> String {
    let published = if cfg!(test) { "http://127.0.0.1:9" } else { PUBLISHED };
    std::env::var(SOURCE_ENV)
        .ok()
        .filter(|base| !base.is_empty())
        .unwrap_or_else(|| published.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// A listings file at a glance: the day it was built and how many it lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub built: Option<String>,
    pub rows: usize,
}

/// The `# built YYYY-MM-DD` line in a file's header. A file written before
/// the line existed has none, and counts as older than any that does.
pub fn built(text: &str) -> Option<&str> {
    text.lines()
        .take(12)
        .take_while(|line| line.starts_with('#'))
        .filter_map(|line| line.strip_prefix("# built "))
        .map(str::trim)
        .find(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
}

/// The rows in a file, without judging it.
pub fn summary(text: &str) -> Summary {
    let rows = text.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).count();
    Summary { built: built(text).map(str::to_string), rows }
}

/// Whether `text` is a listings file fit to search: the header, a build date,
/// every row whole, a whole market's worth of them, mostly ranked — and,
/// given the count of the file it would replace, not much smaller than it.
pub fn validate(text: &str, replacing: Option<usize>) -> Result<Summary, String> {
    if text.lines().next() != Some(HEADER) {
        return Err("not a listings file: the first line is not its header".into());
    }
    let Some(date) = built(text) else {
        return Err("no build date in the header".into());
    };
    let (mut rows, mut ranked) = (0usize, [0usize; 2]);
    let mut counted = [0usize; 2];
    for (number, line) in text.lines().filter(|line| !line.is_empty() && !line.starts_with('#')).enumerate() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() != 10 {
            return Err(format!("row {} has {} columns", number + 1, fields.len()));
        }
        let kind = match fields[0] {
            "equity" => 0,
            "etf" => 1,
            other => return Err(format!("row {} is a {other:?}", number + 1)),
        };
        if fields[1].is_empty() || !fields[1].chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
            return Err(format!("row {} has ticker {:?}", number + 1, fields[1]));
        }
        rows += 1;
        counted[kind] += 1;
        if fields[9] != "-" {
            ranked[kind] += 1;
        }
    }
    if rows < MIN_ROWS {
        return Err(format!("only {rows} rows; a whole market is over {MIN_ROWS}"));
    }
    for (kind, name) in ["stock", "fund"].iter().enumerate() {
        if (ranked[kind] as f64) < counted[kind] as f64 * MIN_RANKED {
            return Err(format!("only {} of {} {name} rows ranked", ranked[kind], counted[kind]));
        }
    }
    if let Some(before) = replacing
        && (rows as f64) < before as f64 * (1.0 - MAX_SHRINK)
    {
        return Err(format!("{rows} rows against {before} in the file it would replace"));
    }
    Ok(Summary { built: Some(date.to_string()), rows })
}

/// The copy to search: the downloaded one, unless the one compiled in was
/// built more recently, which is a release newer than the last download.
pub fn choose<'a>(downloaded: Option<&'a str>, compiled: &'a str) -> &'a str {
    match downloaded {
        Some(text) if built(text) >= built(compiled) => text,
        _ => compiled,
    }
}

/// The sidecar, as the workflow writes it.
#[derive(Clone, Debug, Deserialize)]
struct Published {
    built: String,
    rows: usize,
    sha256: String,
}

/// The sidecar as last fetched, kept beside the file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Kept {
    built: String,
    rows: usize,
    sha256: String,
    #[serde(default)]
    etag: Option<String>,
}

/// What a refresh came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    /// The listings searched from now on.
    pub summary: Summary,
    /// A new file was written.
    pub updated: bool,
}

/// Ask for the published listings now, and keep them if they are newer than
/// what is here and fit to use. `compiled` is the copy built into the binary,
/// which a download has to be newer than to be worth fetching.
///
/// Blocks on the network: call it off the main loop.
pub fn refresh(base: &str, dir: &Path, compiled: &str) -> Result<Fetched, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(format!("omacharts/{}", crate::launch::VERSION))
        .http_status_as_error(false)
        .build()
        .into();

    let here = fs::read_to_string(path_in(dir)).ok();
    let in_use = summary(choose(here.as_deref(), compiled));
    // The ETag stands for the file it came with; with that file gone, it
    // would answer "unchanged" about something no longer here.
    let kept: Option<Kept> = here
        .as_ref()
        .and_then(|_| fs::read(dir.join(KEPT)).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());

    let mut asked = agent.get(&format!("{base}/listings.json"));
    if let Some(etag) = kept.as_ref().and_then(|kept| kept.etag.as_deref()) {
        asked = asked.header("If-None-Match", etag);
    }
    let response = asked.call().map_err(|error| format!("listings not reachable: {error}"))?;
    let unchanged = Fetched { summary: in_use.clone(), updated: false };
    match response.status().as_u16() {
        304 => {
            touch(dir);
            return Ok(unchanged);
        }
        200 => {}
        status => return Err(format!("listings.json answered {status}")),
    }
    let etag = response.headers().get("etag").and_then(|value| value.to_str().ok()).map(str::to_string);
    let sidecar = read_limited(response.into_body().into_reader(), MAX_SIDECAR)?;
    let published: Published =
        serde_json::from_slice(&sidecar).map_err(|error| format!("listings.json unreadable: {error}"))?;

    let have_it = kept.as_ref().is_some_and(|kept| kept.sha256.eq_ignore_ascii_case(&published.sha256));
    let not_newer = built(compiled).is_some_and(|compiled| compiled >= published.built.as_str())
        && here.as_deref().is_none_or(|here| built(here) <= built(compiled));
    if have_it || not_newer {
        if let Some(mut kept) = kept.filter(|_| have_it) {
            kept.etag = etag;
            write_whole(dir, KEPT, &serde_json::to_vec(&kept).unwrap_or_default());
        }
        touch(dir);
        return Ok(unchanged);
    }

    let response = agent
        .get(&format!("{base}/listings.tsv"))
        .call()
        .map_err(|error| format!("listings not reachable: {error}"))?;
    if response.status().as_u16() != 200 {
        return Err(format!("listings.tsv answered {}", response.status().as_u16()));
    }
    let bytes = read_limited(response.into_body().into_reader(), MAX_FILE)?;
    if !hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(&published.sha256) {
        // Most likely the two files caught mid-publish, each cached on its
        // own: the next check will find them agreeing.
        return Err("listings.tsv does not match the checksum published with it".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "listings.tsv is not text".to_string())?;
    let summary = validate(&text, Some(in_use.rows)).map_err(|why| format!("listings refused: {why}"))?;

    write_whole(dir, FILE, text.as_bytes()).ok_or_else(|| format!("could not write {}", path_in(dir).display()))?;
    let kept = Kept { built: published.built, rows: published.rows, sha256: published.sha256, etag };
    write_whole(dir, KEPT, &serde_json::to_vec(&kept).unwrap_or_default());
    touch(dir);
    Ok(Fetched { summary, updated: true })
}

/// Is a check due? None has been made, or the last was [`CADENCE`] ago, or
/// the clock went backwards past it.
pub fn due(dir: &Path, now: SystemTime) -> bool {
    match fs::metadata(dir.join(CHECKED)).and_then(|meta| meta.modified()) {
        Ok(checked) => now.duration_since(checked).map_or(true, |since| since >= CADENCE),
        Err(_) => true,
    }
}

fn touch(dir: &Path) {
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(dir.join(CHECKED), b"");
}

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("listings download broke off: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("listings download larger than {limit} bytes"));
    }
    Ok(bytes)
}

/// Write `name` in `dir` whole or not at all: into a file of its own first,
/// flushed to disk, then renamed over the old one, which a reader sees as one
/// step. A disk that fills halfway leaves the old file and a stray the next
/// write replaces.
fn write_whole(dir: &Path, name: &str, bytes: &[u8]) -> Option<()> {
    fs::create_dir_all(dir).ok()?;
    let incoming = dir.join(format!(".{name}.incoming-{}", std::process::id()));
    let written = (|| {
        let mut file = fs::File::create(&incoming)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&incoming, dir.join(name))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&incoming);
    }
    written.ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A listings file of `rows` rows built on `date`, every one ranked.
    pub(crate) fn file(date: &str, rows: usize) -> String {
        let mut text = format!("{HEADER}\n# built {date}\n");
        for n in 0..rows {
            let kind = if n % 2 == 0 { "equity" } else { "etf" };
            text.push_str(&format!("{kind}\tT{n}\tTicker {n}\t-\tUSD\t2\t0\t-\tNASDAQ\t{}\n", 1 + n % 9));
        }
        text
    }

    fn sidecar(text: &str) -> String {
        let summary = summary(text);
        format!(
            "{{\"built\":\"{}\",\"rows\":{},\"sha256\":\"{}\"}}",
            summary.built.unwrap(),
            summary.rows,
            hex(&Sha256::digest(text.as_bytes()))
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("omacharts-listings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// One request as the server saw it.
    #[derive(Clone, Debug)]
    struct Asked {
        path: String,
        if_none_match: Option<String>,
    }

    /// A server on a local port answering from a table of paths, with an
    /// `ETag` on the sidecar, and a 304 when it is asked for with it. Each
    /// request is written down. Nothing here reaches past the loopback.
    struct Server {
        base: String,
        asked: Arc<Mutex<Vec<Asked>>>,
    }

    impl Server {
        fn start(files: Vec<(&'static str, Vec<u8>)>) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
            let log = asked.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() {
                        continue;
                    }
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let mut if_none_match = None;
                    loop {
                        let mut header = String::new();
                        if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                            break;
                        }
                        if let Some((name, value)) = header.split_once(':')
                            && name.eq_ignore_ascii_case("if-none-match")
                        {
                            if_none_match = Some(value.trim().to_string());
                        }
                    }
                    log.lock().unwrap().push(Asked { path: path.clone(), if_none_match: if_none_match.clone() });
                    let found = files.iter().find(|(name, _)| path == format!("/{name}"));
                    let etag = found.map(|(_, body)| format!("\"{}\"", &hex(&Sha256::digest(body))[..16]));
                    let head = match (found, &etag) {
                        (Some(_), Some(etag)) if if_none_match.as_deref() == Some(etag.as_str()) => {
                            format!("HTTP/1.1 304 Not Modified\r\nETag: {etag}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                                .into_bytes()
                        }
                        (Some((_, body)), Some(etag)) => {
                            let mut out = format!(
                                "HTTP/1.1 200 OK\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .into_bytes();
                            out.extend_from_slice(body);
                            out
                        }
                        _ => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
                    };
                    let _ = stream.write_all(&head);
                }
            });
            Server { base, asked }
        }

        fn asked(&self) -> Vec<Asked> {
            self.asked.lock().unwrap().clone()
        }
    }

    #[test]
    fn a_whole_file_passes() {
        let summary = validate(&file("2026-11-21", 9500), Some(9400)).unwrap();
        assert_eq!(summary, Summary { built: Some("2026-11-21".into()), rows: 9500 });
    }

    /// The compiled-in copy is the file the workflow publishes, and the
    /// rules a download is held to must not refuse it.
    #[test]
    fn the_compiled_copy_passes() {
        let compiled = crate::inventory::compiled();
        let summary = validate(compiled, None).unwrap();
        assert!(summary.rows > 10_000);
    }

    #[test]
    fn a_truncated_or_garbled_file_is_refused() {
        let good = file("2026-11-21", 9500);
        let cut = &good[..=good[..good.len() / 2].rfind('\n').unwrap()];
        assert!(validate(cut, None).unwrap_err().contains("only"), "half a download");
        // Cut mid-row: the last line is missing its columns.
        let mid_row = &good[..good.len() - 7];
        assert!(validate(mid_row, None).unwrap_err().contains("columns"));
        assert!(validate("<html>rate limited</html>", None).unwrap_err().contains("header"));
        assert!(validate("", None).is_err());
        let undated = good.replace("# built 2026-11-21\n", "");
        assert!(validate(&undated, None).unwrap_err().contains("date"));
        let alien = good.replacen("equity\tT0", "bond\tT0", 1);
        assert!(validate(&alien, None).unwrap_err().contains("bond"));
    }

    /// A ranking feed that was down publishes a day of worse ordering; the
    /// day before's is better.
    #[test]
    fn a_file_mostly_unranked_is_refused() {
        let unranked: String = file("2026-11-21", 9500)
            .lines()
            .map(|line| match line.starts_with("equity") {
                true => format!("{}\t-", line.rsplit_once('\t').unwrap().0),
                false => line.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(validate(&unranked, None).unwrap_err().contains("stock rows ranked"));
    }

    #[test]
    fn a_file_much_smaller_than_the_one_it_replaces_is_refused() {
        assert!(validate(&file("2026-11-21", 9500), Some(11_000)).unwrap_err().contains("against"));
        assert!(validate(&file("2026-11-21", 9500), Some(9800)).is_ok(), "a few delistings are a day");
    }

    #[test]
    fn the_later_build_is_the_one_searched() {
        let (old, new) = (file("2026-11-01", 1), file("2026-11-21", 1));
        assert_eq!(choose(Some(&new), &old), new, "a download newer than the release");
        assert_eq!(choose(Some(&old), &new), new, "a release newer than the download");
        assert_eq!(choose(None, &old), old);
        let undated = old.replace("# built 2026-11-01\n", "");
        assert_eq!(choose(Some(&old), &undated), old, "an undated copy is the oldest");
    }

    #[test]
    fn a_check_is_due_once_a_day() {
        let dir = scratch("due");
        let now = SystemTime::now();
        assert!(due(&dir, now), "never checked");
        touch(&dir);
        assert!(!due(&dir, now));
        assert!(due(&dir, now + CADENCE));
        let _ = fs::remove_dir_all(&dir);
    }

    /// The whole round: a first fetch downloads and writes, the next is a
    /// 304 on the sidecar alone, and the file is never asked for twice.
    #[test]
    fn a_refresh_downloads_once_and_then_asks_only_whether_it_changed() {
        let compiled = file("2026-11-01", 9500);
        let fresh = file("2026-11-21", 9600);
        let server = Server::start(vec![
            ("listings.json", sidecar(&fresh).into_bytes()),
            ("listings.tsv", fresh.clone().into_bytes()),
        ]);
        let dir = scratch("round");

        let first = refresh(&server.base, &dir, &compiled).unwrap();
        assert!(first.updated);
        assert_eq!(first.summary, Summary { built: Some("2026-11-21".into()), rows: 9600 });
        assert_eq!(fs::read_to_string(path_in(&dir)).unwrap(), fresh);
        assert!(!due(&dir, SystemTime::now()));

        let second = refresh(&server.base, &dir, &compiled).unwrap();
        assert!(!second.updated);
        assert_eq!(second.summary.rows, 9600);

        let asked = server.asked();
        let paths: Vec<&str> = asked.iter().map(|asked| asked.path.as_str()).collect();
        assert_eq!(paths, ["/listings.json", "/listings.tsv", "/listings.json"]);
        assert_eq!(asked[0].if_none_match, None, "nothing to be conditional on the first time");
        assert!(asked[2].if_none_match.is_some(), "the second asks with the ETag it was given");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The file lost, its ETag means nothing: asked again unconditionally,
    /// and downloaded again.
    #[test]
    fn a_missing_file_is_fetched_again_whatever_the_etag() {
        let compiled = file("2026-11-01", 9500);
        let fresh = file("2026-11-21", 9600);
        let server = Server::start(vec![
            ("listings.json", sidecar(&fresh).into_bytes()),
            ("listings.tsv", fresh.clone().into_bytes()),
        ]);
        let dir = scratch("lost");
        refresh(&server.base, &dir, &compiled).unwrap();
        fs::remove_file(path_in(&dir)).unwrap();
        assert!(refresh(&server.base, &dir, &compiled).unwrap().updated);
        assert_eq!(server.asked()[2].if_none_match, None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A release built after the last publish has nothing to learn from it.
    #[test]
    fn nothing_is_downloaded_that_is_older_than_the_release() {
        let published = file("2026-11-01", 9500);
        let compiled = file("2026-11-21", 9600);
        let server = Server::start(vec![
            ("listings.json", sidecar(&published).into_bytes()),
            ("listings.tsv", published.into_bytes()),
        ]);
        let dir = scratch("older");
        let fetched = refresh(&server.base, &dir, &compiled).unwrap();
        assert!(!fetched.updated);
        assert_eq!(fetched.summary.built.as_deref(), Some("2026-11-21"));
        assert!(!path_in(&dir).exists());
        assert_eq!(server.asked().len(), 1, "the sidecar alone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Whatever arrives, a bad download never replaces a good file.
    #[test]
    fn a_bad_download_leaves_the_file_that_was_here() {
        let compiled = file("2026-11-01", 9500);
        let good = file("2026-11-10", 9550);
        let dir = scratch("bad");
        fs::write(path_in(&dir), &good).unwrap();

        let fresh = file("2026-11-21", 9600);
        // Caught mid-publish: the sidecar is for one file, the file another.
        let server = Server::start(vec![
            ("listings.json", sidecar(&fresh).into_bytes()),
            ("listings.tsv", file("2026-11-20", 9600).into_bytes()),
        ]);
        assert!(refresh(&server.base, &dir, &compiled).unwrap_err().contains("checksum"));
        assert_eq!(fs::read_to_string(path_in(&dir)).unwrap(), good);

        // Matching its checksum, and truncated when it was built.
        let short = file("2026-11-21", 2000);
        let server = Server::start(vec![
            ("listings.json", sidecar(&short).into_bytes()),
            ("listings.tsv", short.into_bytes()),
        ]);
        assert!(refresh(&server.base, &dir, &compiled).unwrap_err().contains("refused"));
        assert_eq!(fs::read_to_string(path_in(&dir)).unwrap(), good);

        // Nobody there at all.
        let server = Server::start(Vec::new());
        assert!(refresh(&server.base, &dir, &compiled).unwrap_err().contains("404"));
        assert_eq!(fs::read_to_string(path_in(&dir)).unwrap(), good);

        let strays: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains("incoming"))
            .collect();
        assert!(strays.is_empty(), "nothing half-written left behind");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_replaces_the_file_whole() {
        let dir = scratch("whole");
        write_whole(&dir, FILE, b"one").unwrap();
        write_whole(&dir, FILE, b"two").unwrap();
        assert_eq!(fs::read(path_in(&dir)).unwrap(), b"two");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "no stray beside it");
        let _ = fs::remove_dir_all(&dir);
    }
}
