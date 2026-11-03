//! A second rich feed that exists only to prove the boundary holds.
//!
//! It has what Rithmic has — bars, a stream, minute ladders and a sign-in
//! typed into a form — and none of it is named anywhere above this folder.
//! It is found by [`crate::providers::listed`] and never offered, and only
//! in a build with the `fakes` feature, which is a test build: the tests
//! that route a chart to it and draw its row and its login form on the Data
//! page do so through the same code every real feed goes through, and the
//! day one of them needs a line naming this feed, the boundary has a hole.
//!
//! Its bars and its stream are [`Synthetic`]'s. What is signed in is held in
//! memory, and so is its one secret.

use std::sync::{Mutex, RwLock};

use crate::bars::{Bar, Timeframe};
use crate::ladders::Minute;
use crate::provider::{
    Capability, LadderSink, Ladders, Provider, ProviderError, Stream, Subscription,
};
use crate::providers::{Access, Field, FieldKind, How, Listed, Saved, Secrets, SignIn, Setup, Synthetic, USER};
use crate::symbols::{Instrument, InstrumentKind};

/// How the fake is listed.
pub const LISTED: Listed = Listed {
    id: "fake",
    label: "Fake Rich Feed",
    serves: "Futures, invented, from a login typed into a form",
    covers: "Futures, invented",
    build: || Box::new(Fake::new()),
    setup: Some(&Signing),
    experimental: false,
};

/// What its form asks for: one of every kind of field.
pub const FIELDS: &[Field] = &[
    Field { key: USER, title: "Member number", kind: FieldKind::Text, default: "", required: true },
    Field { key: "token", title: "Token", kind: FieldKind::Secret, default: "", required: true },
    Field { key: "desk", title: "Desk", kind: FieldKind::Choice(&["North", "South"]), default: "South", required: false },
    Field { key: "server", title: "Server", kind: FieldKind::Url, default: "wss://fake.invalid", required: false },
];

const SETUP: Setup = Setup {
    explain: "Invented futures, for tests.",
    requires: &["A member number and a token"],
    scope: "Nothing real.",
};

pub struct Fake {
    bars: Synthetic,
}

impl Fake {
    pub fn new() -> Fake {
        Fake { bars: Synthetic::new(None) }
    }
}

impl Default for Fake {
    fn default() -> Fake {
        Fake::new()
    }
}

impl Provider for Fake {
    fn id(&self) -> &'static str {
        LISTED.id
    }

    fn label(&self) -> &'static str {
        LISTED.label
    }

    fn delay_minutes(&self, _: InstrumentKind) -> u32 {
        0
    }

    /// Futures roots only, so that a stock still goes to the next feed.
    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        (instrument.kind == InstrumentKind::FutureRoot).then(|| instrument.symbol.clone())
    }

    fn capabilities(&self) -> &'static [Capability] {
        self.bars.capabilities()
    }

    fn bars(&self, symbol: &str, timeframe: Timeframe, since: Option<i64>) -> Result<Vec<Bar>, ProviderError> {
        self.bars.bars(symbol, timeframe, since)
    }

    fn stream(&self) -> Option<&dyn Stream> {
        self.bars.stream()
    }

    fn ladders(&self) -> Option<&dyn Ladders> {
        Some(self)
    }
}

impl Ladders for Fake {
    fn serves(&self, _: &str) -> bool {
        true
    }

    fn history_days(&self) -> u32 {
        5
    }

    /// Nothing traded: an answer, and the whole truth about the span.
    fn minutes(&self, _: &str, _: f64, _: i64, _: i64) -> Result<Vec<Minute>, ProviderError> {
        Ok(Vec::new())
    }

    fn watch(&self, _: &str, _: f64, _: LadderSink) -> Result<Box<dyn Subscription>, ProviderError> {
        Ok(Box::new(Watching))
    }
}

struct Watching;

impl Subscription for Watching {}

static SAVED: RwLock<Option<Saved>> = RwLock::new(None);

/// Its secrets, in memory: the fake's keyring.
pub static KEPT: Memory = Memory(Mutex::new(Vec::new()));

/// Passwords by login, for nothing but this.
pub struct Memory(Mutex<Vec<(String, String)>>);

impl Memory {
    /// Is a secret kept for `login`?
    pub fn holds(&self, login: &str) -> bool {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(known, _)| known == login)
    }
}

impl Secrets for Memory {
    fn save(&self, login: &str, _: &str, _: &str, password: &str) -> Result<(), String> {
        let mut kept = self.0.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|(known, _)| known != login);
        kept.push((login.to_string(), password.to_string()));
        Ok(())
    }

    fn forget(&self, login: &str, _: &str, _: &str) -> Result<(), String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).retain(|(known, _)| known != login);
        Ok(())
    }
}

fn saved() -> Saved {
    SAVED.read().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

#[derive(Debug)]
pub struct Signing;

impl SignIn for Signing {
    fn about(&self) -> &'static Setup {
        &SETUP
    }

    fn how(&self) -> How {
        How::Form(FIELDS)
    }

    fn access(&self) -> Access {
        match saved().user {
            Some(_) => Access::Signed(saved().value("desk").unwrap_or("no desk").to_string()),
            None => Access::Missing,
        }
    }

    fn can_sign_in(&self) -> Result<String, String> {
        Ok("the token in memory".into())
    }

    fn sign_in(&self, log: Box<dyn Fn(&str) + Send + Sync>) -> Result<(), String> {
        let saved = saved();
        match (saved.user, saved.login) {
            (Some(_), Some(login)) if KEPT.holds(&login) => {
                log("found the token");
                Ok(())
            }
            _ => Err("no token is kept".into()),
        }
    }

    fn sign_out(&self) -> Result<(), String> {
        if let Some(login) = saved().login {
            KEPT.forget(&login, "", "")?;
        }
        Ok(())
    }

    fn connected(&self) -> bool {
        false
    }

    fn places(&self) -> Vec<(&'static str, String)> {
        vec![("Memory", "this process".into())]
    }

    fn remember(&self, saved: Saved) {
        *SAVED.write().unwrap_or_else(|e| e.into_inner()) = Some(saved);
    }

    fn promise(&self) -> String {
        "Your token is kept in memory and goes nowhere.".into()
    }

    fn keyring(&self) -> Option<&'static dyn Secrets> {
        Some(&KEPT)
    }
}
