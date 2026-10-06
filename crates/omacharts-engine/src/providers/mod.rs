//! The data providers.
//!
//! Yahoo is the default. `OMACHARTS_PROVIDER=tos`, or a stored `provider`
//! setting of `tos`, uses the Thinkorswim data feed. The choice is read
//! when the process starts.

mod tos;
pub mod yahoo;

use std::time::{Duration, Instant};

use crate::bars::{Bar, Timeframe};
use crate::provider::{Capability, Delivery, Pacing, Provider, ProviderError};
use crate::symbols::{Instrument, InstrumentKind};

pub use tos::Tos;
pub use yahoo::Yahoo;

/// The provider this process is using.
pub enum Feed {
    Yahoo(Yahoo),
    Tos(Tos),
}

/// `OMACHARTS_PROVIDER` wins over the stored setting. Anything other than
/// `tos` / `thinkorswim` is Yahoo.
pub fn selected(stored: Option<&str>) -> Feed {
    let from_env = std::env::var("OMACHARTS_PROVIDER").ok();
    let name = from_env
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| stored.unwrap_or("yahoo").trim());
    match name.to_ascii_lowercase().as_str() {
        "tos" | "thinkorswim" => Feed::Tos(Tos::new()),
        _ => Feed::Yahoo(Yahoo::new()),
    }
}

impl Provider for Feed {
    fn id(&self) -> &'static str {
        match self {
            Feed::Yahoo(provider) => provider.id(),
            Feed::Tos(provider) => provider.id(),
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Feed::Yahoo(provider) => provider.label(),
            Feed::Tos(provider) => provider.label(),
        }
    }

    fn delay_minutes(&self, kind: InstrumentKind) -> u32 {
        match self {
            Feed::Yahoo(provider) => provider.delay_minutes(kind),
            Feed::Tos(provider) => provider.delay_minutes(kind),
        }
    }

    fn symbol_for(&self, instrument: &Instrument) -> Option<String> {
        match self {
            Feed::Yahoo(provider) => provider.symbol_for(instrument),
            Feed::Tos(provider) => provider.symbol_for(instrument),
        }
    }

    fn capabilities(&self) -> &'static [Capability] {
        match self {
            Feed::Yahoo(provider) => provider.capabilities(),
            Feed::Tos(provider) => provider.capabilities(),
        }
    }

    fn bars(
        &self,
        symbol: &str,
        timeframe: Timeframe,
        since: Option<i64>,
    ) -> Result<Vec<Bar>, ProviderError> {
        match self {
            Feed::Yahoo(provider) => provider.bars(symbol, timeframe, since),
            Feed::Tos(provider) => provider.bars(symbol, timeframe, since),
        }
    }

    fn delivery(&self) -> Delivery {
        match self {
            Feed::Yahoo(provider) => provider.delivery(),
            Feed::Tos(provider) => provider.delivery(),
        }
    }

    fn pacing(&self) -> Pacing {
        match self {
            Feed::Yahoo(provider) => provider.pacing(),
            Feed::Tos(provider) => provider.pacing(),
        }
    }

    fn cooldown_until(&self) -> Option<Instant> {
        match self {
            Feed::Yahoo(provider) => provider.cooldown_until(),
            Feed::Tos(provider) => provider.cooldown_until(),
        }
    }

    fn retry_after(&self, error: &ProviderError, attempt: u32) -> Option<Duration> {
        match self {
            Feed::Yahoo(provider) => provider.retry_after(error, attempt),
            Feed::Tos(provider) => provider.retry_after(error, attempt),
        }
    }
}
