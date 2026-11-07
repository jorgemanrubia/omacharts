//! Setting Sierra Chart up as a feed: which folder it is in, and the port
//! its DTC server listens on.
//!
//! There is nothing to sign in to. Sierra Chart signs in to its own data
//! service and writes what it hears to its `Data` folder; this feed reads
//! the folder. So the setup is the folder, kept by the app like any feed's
//! setup (`feeds::SETUPS`), and handed back here with [`SignIn::remember`].
//! Reading how it stands is a `stat` of the folder, never a connection.
//!
//! The port is for what the files cannot say: which contracts a root lists
//! and when each expires, asked of Sierra Chart's DTC server when it is
//! listening. It is optional, and the charts never wait for it.

use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

use crate::provider::ProviderError;
use crate::providers::{Access, Field, How, Saved, SignIn, Verbs};

/// What a chart says while no folder is set up.
pub const NOT_SET_UP: &str = "Sierra Chart is not set up. Open Preferences → Data.";

/// The key the folder is kept under.
pub const FOLDER: &str = "folder";
/// The key the DTC server's port is kept under.
pub const PORT: &str = "port";
/// The key a broker login keeps the DTC server's host under; its port is
/// under [`PORT`], as the feed's is.
pub const HOST: &str = "host";
/// Where Sierra Chart's DTC server is, unless a login says otherwise: on
/// this machine, which is all "Local Computer Only" allows.
pub const DEFAULT_HOST: &str = "127.0.0.1";
/// Sierra Chart's own default for its DTC server's "Listening Port".
pub const DEFAULT_PORT: u16 = 11099;

const FIELDS: &[Field] = &[Field { key: FOLDER, label: "Folder" }, Field { key: PORT, label: "DTC port" }];

static SAVED: RwLock<Option<Saved>> = RwLock::new(None);

fn saved() -> Saved {
    SAVED.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

/// `~/…` as the home folder it names.
pub fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// The home folder as `~`, for a row with room for a few words.
pub fn shown(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The folder set up, if one is.
pub fn folder() -> Option<PathBuf> {
    saved().value(FOLDER).map(expand)
}

/// The DTC server's port set up, or Sierra Chart's default.
pub fn port() -> u16 {
    saved().value(PORT).and_then(|port| port.trim().parse().ok()).unwrap_or(DEFAULT_PORT)
}

/// What is wrong with `folder` as Sierra Chart's, or how much it holds.
pub fn check(folder: &Path) -> Result<String, String> {
    let install = sierra::Install::open(folder).map_err(|problem| problem.to_string())?;
    let listing = install.listing();
    Ok(match listing.entries().iter().filter(|entry| entry.has_intraday_records()).count() {
        0 => "no contracts with trades yet".to_string(),
        1 => "1 contract with trades".to_string(),
        n => format!("{n} contracts with trades"),
    })
}

/// Setting Sierra Chart up, as the boundary asks it of every feed.
#[derive(Debug)]
pub struct Setup;

impl SignIn for Setup {
    fn how(&self) -> How {
        How::Dialog
    }

    fn access(&self) -> Access {
        match folder() {
            None => Access::Missing,
            Some(folder) => match sierra::Install::open(&folder) {
                Ok(_) => Access::Signed(shown(&folder)),
                Err(problem) => Access::Expired(problem.to_string()),
            },
        }
    }

    fn can_sign_in(&self) -> Result<String, String> {
        let folder = folder().ok_or("Choose Sierra Chart's folder")?;
        check(&folder).map(|_| shown(&folder))
    }

    /// Nothing to sign in to: the folder is read, and the DTC server
    /// asked whether it is there, which is said and never fails it.
    fn sign_in(&self, log: Box<dyn Fn(&str) + Send + Sync>) -> Result<(), String> {
        super::forget_opened();
        let folder = folder().ok_or("Choose Sierra Chart's folder")?;
        log(&check(&folder)?);
        log(&listening(port()).unwrap_or_else(|why| why));
        Ok(())
    }

    fn sign_out(&self) -> Result<(), String> {
        super::forget_opened();
        Ok(())
    }

    fn connected(&self) -> bool {
        false
    }

    fn places(&self) -> Vec<(&'static str, String)> {
        let Some(folder) = folder() else { return Vec::new() };
        let mut places = vec![("Folder", folder.display().to_string())];
        if let Ok(install) = sierra::Install::open(&folder) {
            places.push(("Data", install.data.display().to_string()));
        }
        places
    }

    fn remember(&self, saved: Saved) {
        *SAVED.write().unwrap_or_else(|e| e.into_inner()) = Some(saved);
        super::forget_opened();
    }

    fn refusal(&self) -> Option<String> {
        match self.access() {
            Access::Expired(problem) => Some(problem),
            _ => None,
        }
    }

    fn fields(&self) -> &'static [Field] {
        FIELDS
    }

    fn verbs(&self) -> Verbs {
        Verbs { start: "Set up…", change: Some("Change…"), end: None, missing: "not set up", ready: "Set up" }
    }

    /// The first Sierra Chart folder on this machine, with the port as it
    /// was or Sierra Chart's default.
    fn found(&self) -> Option<Saved> {
        let first = candidates().into_iter().next()?;
        Some(saved().with(FOLDER, &shown(&first)))
    }
}

/// Is Sierra Chart's DTC server listening on `port`, on this machine?
pub fn probe(port: u16, timeout: Duration) -> dtc::Probe {
    dtc::probe(&dtc::Endpoint { port, ..dtc::Endpoint::default() }, timeout)
}

/// [`probe`], in the words a dialog shows: `Ok` with the server's line
/// when it answers as a DTC server, `Err` with what is wrong otherwise.
/// Connects, signs on and off: for a thread of its own.
pub fn listening(port: u16) -> Result<String, String> {
    match probe(port, Duration::from_millis(800)) {
        // Said of the server alone: whether it trades is the broker's.
        dtc::Probe::Ready(_) => Ok(format!("DTC server listening on port {port}")),
        other => Err(other.to_string()),
    }
}

/// The folders Sierra Chart is likely to be in on this machine, best
/// first: only ones that are there. A few `stat`s.
pub fn candidates() -> Vec<PathBuf> {
    sierra::install::candidates()
        .into_iter()
        .filter(|folder| sierra::Install::open(folder).is_ok())
        .collect()
}

/// The contracts of `root` Sierra Chart's DTC server lists, each in the
/// exchange's spelling with the day it stops trading.
pub fn contracts(root: &str) -> Result<Vec<(String, chrono::NaiveDate)>, ProviderError> {
    let endpoint = dtc::Endpoint { port: port(), ..dtc::Endpoint::default() };
    let definitions = dtc::definitions(&endpoint, root, Duration::from_secs(3))
        .map_err(|error| ProviderError::Unsupported(format!("Sierra Chart's DTC server: {error}")))?;
    let listed: Vec<(String, chrono::NaiveDate)> = definitions
        .iter()
        .filter_map(|definition| {
            let file = sierra::symbols::parse(&definition.symbol)?;
            let contract = file.contract?;
            (file.root == root).then_some(())?;
            let expires = chrono::DateTime::from_timestamp(definition.expiration?, 0)?.date_naive();
            let month = crate::contracts::Month::of(contract.month, contract.year)?;
            Some((crate::contracts::symbol(&file.root, month), expires))
        })
        .collect();
    match listed.is_empty() {
        true => Err(ProviderError::Unsupported(format!("Sierra Chart lists no contracts of {root}"))),
        false => Ok(listed),
    }
}
