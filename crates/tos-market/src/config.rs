//! Trading-system gate.
//!
//! A session names its own gateway URL. Only a papermoney host counts as
//! paper. This crate never sets `allow_live_trading`.

use crate::error::{Error, Result};

pub const TOS_WEB_ORIGIN: &str = "https://trade.thinkorswim.com";

/// Which gateway a session talks to. `LiveTrading` routes real orders and is
/// gated off unless explicitly allowed (see [`assert_trading_system_allowed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TradingSystem {
    LiveTrading,
    PaperMoney,
}

impl TradingSystem {
    pub fn as_str(self) -> &'static str {
        match self {
            TradingSystem::LiveTrading => "LiveTrading",
            TradingSystem::PaperMoney => "PaperMoney",
        }
    }

    /// Lenient parse: the SPA stores `PaperMoney` / `LiveTrading`, some keys
    /// use `traderx::papermoney`.
    pub fn parse(value: &str) -> Option<Self> {
        let v = value.to_ascii_lowercase();
        if v.contains("paper") {
            Some(TradingSystem::PaperMoney)
        } else if v.contains("live") {
            Some(TradingSystem::LiveTrading)
        } else {
            None
        }
    }

    /// Infers the trading system from a gateway URL. Only a `papermoney` host
    /// counts as paper; anything else — including a URL that cannot be parsed
    /// or that merely mentions papermoney in its path or query — is live.
    pub fn of_gateway_url(url: &str) -> Self {
        Self::implied_by_gateway_url(url).unwrap_or(TradingSystem::LiveTrading)
    }

    /// Like [`TradingSystem::of_gateway_url`], but `None` for a loopback host:
    /// a local fake gateway can stand in for either system, so there the label
    /// decides (and is still gated). Every other host is classified strictly.
    pub fn implied_by_gateway_url(url: &str) -> Option<Self> {
        let Ok(parsed) = url::Url::parse(url.trim()) else {
            return Some(TradingSystem::LiveTrading);
        };
        if matches!(
            parsed.host(),
            Some(url::Host::Ipv4(ip)) if ip.is_loopback()
        ) || matches!(
            parsed.host(),
            Some(url::Host::Ipv6(ip)) if ip.is_loopback()
        ) || parsed
            .host_str()
            .is_some_and(|h| h.eq_ignore_ascii_case("localhost"))
        {
            return None;
        }
        let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
        Some(if host.contains("papermoney") {
            TradingSystem::PaperMoney
        } else {
            TradingSystem::LiveTrading
        })
    }
}

/// Refuses a configuration whose trading-system label disagrees with the
/// gateway it would connect to, then applies the live gate to the system the
/// URL actually implies — a label alone never opens a live socket. A loopback
/// gateway implies neither system, so the label is gated as-is.
pub fn assert_gateway_matches(
    trading_system: TradingSystem,
    gateway_url: &str,
    allow_live_trading: bool,
) -> Result<()> {
    let effective = TradingSystem::implied_by_gateway_url(gateway_url).unwrap_or(trading_system);
    if effective != trading_system {
        return Err(Error::Config(format!(
            "trading system {trading_system} does not match gateway {gateway_url} ({effective})"
        )));
    }
    assert_trading_system_allowed(effective, allow_live_trading)
}

impl std::fmt::Display for TradingSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayUrls {
    pub livetrading_a: String,
    pub papermoney: String,
}

/// Last-known gateway hosts. The session file carries the URL that is used.
pub fn fallback_gateway_urls() -> GatewayUrls {
    GatewayUrls {
        livetrading_a: "wss://thinkorswim-services.schwab.com/Services/WsJson".into(),
        papermoney: "wss://papermoney-services.schwab.com/Services/WsJson".into(),
    }
}

pub fn assert_trading_system_allowed(
    trading_system: TradingSystem,
    allow_live_trading: bool,
) -> Result<()> {
    if trading_system == TradingSystem::LiveTrading && !allow_live_trading {
        return Err(Error::LiveTradingDisabled);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The second live gateway the SPA can land on; captured sessions carry it.
    const LIVE_B_URL: &str =
        "wss://thinkorswim-services-b.tos-prd.prd.gcp.schwabcloud.com/Services/WsJson";

    #[test]
    fn gates_live_trading_unless_explicitly_allowed() {
        assert!(matches!(
            assert_trading_system_allowed(TradingSystem::LiveTrading, false),
            Err(Error::LiveTradingDisabled)
        ));
        assert!(assert_trading_system_allowed(TradingSystem::LiveTrading, true).is_ok());
        assert!(assert_trading_system_allowed(TradingSystem::PaperMoney, false).is_ok());
    }

    #[test]
    fn parses_trading_system_leniently() {
        assert_eq!(
            TradingSystem::parse("traderx::papermoney"),
            Some(TradingSystem::PaperMoney)
        );
        assert_eq!(
            TradingSystem::parse("LiveTrading"),
            Some(TradingSystem::LiveTrading)
        );
        assert_eq!(TradingSystem::parse("nope"), None);
        assert_eq!(
            TradingSystem::of_gateway_url(&fallback_gateway_urls().papermoney),
            TradingSystem::PaperMoney
        );
    }

    #[test]
    fn only_a_papermoney_host_counts_as_paper() {
        let urls = fallback_gateway_urls();
        assert_eq!(
            TradingSystem::of_gateway_url(&urls.livetrading_a),
            TradingSystem::LiveTrading
        );
        assert_eq!(
            TradingSystem::of_gateway_url(LIVE_B_URL),
            TradingSystem::LiveTrading
        );
        assert_eq!(
            TradingSystem::of_gateway_url(
                "wss://thinkorswim-services.schwab.com/papermoney/WsJson?x=papermoney"
            ),
            TradingSystem::LiveTrading
        );
        assert_eq!(
            TradingSystem::of_gateway_url("ws://127.0.0.1:9/papermoney"),
            TradingSystem::LiveTrading
        );
        assert_eq!(
            TradingSystem::of_gateway_url("not a url papermoney"),
            TradingSystem::LiveTrading
        );
        assert_eq!(
            TradingSystem::of_gateway_url("  wss://PaperMoney-services.schwab.com/x "),
            TradingSystem::PaperMoney
        );
    }

    #[test]
    fn a_loopback_gateway_implies_no_system() {
        for url in [
            "ws://127.0.0.1:8765/Services/WsJson",
            "ws://localhost:8765/",
            "ws://[::1]:8765/",
            "ws://127.9.9.9:1/",
        ] {
            assert_eq!(TradingSystem::implied_by_gateway_url(url), None, "{url}");
            // The strict form still fails closed.
            assert_eq!(
                TradingSystem::of_gateway_url(url),
                TradingSystem::LiveTrading,
                "{url}"
            );
        }
        assert_eq!(
            TradingSystem::implied_by_gateway_url("ws://127.0.0.1.example.com/"),
            Some(TradingSystem::LiveTrading)
        );
        assert_eq!(
            TradingSystem::implied_by_gateway_url("wss://papermoney-services.schwab.com/"),
            Some(TradingSystem::PaperMoney)
        );
        // Unparseable is not "unknown": it fails closed like any other host.
        assert_eq!(
            TradingSystem::implied_by_gateway_url("nope"),
            Some(TradingSystem::LiveTrading)
        );
        assert!(matches!(
            assert_gateway_matches(TradingSystem::PaperMoney, "nope", false),
            Err(Error::Config(_))
        ));
        // On loopback the label stands and is gated as itself.
        assert!(
            assert_gateway_matches(TradingSystem::PaperMoney, "ws://127.0.0.1:1", false).is_ok()
        );
        assert!(matches!(
            assert_gateway_matches(TradingSystem::LiveTrading, "ws://127.0.0.1:1", false),
            Err(Error::LiveTradingDisabled)
        ));
        assert!(
            assert_gateway_matches(TradingSystem::LiveTrading, "ws://127.0.0.1:1", true).is_ok()
        );
    }

    #[test]
    fn gateway_gate_uses_the_url_not_the_label() {
        let urls = fallback_gateway_urls();
        // Paper label pointing at a live host: refused even though the label
        // alone would pass the gate.
        assert!(matches!(
            assert_gateway_matches(TradingSystem::PaperMoney, &urls.livetrading_a, false),
            Err(Error::Config(ref m)) if m.contains("PaperMoney") && m.contains("LiveTrading")
        ));
        assert!(matches!(
            assert_gateway_matches(TradingSystem::PaperMoney, &urls.livetrading_a, true),
            Err(Error::Config(_))
        ));
        // Live label on the paper host is a config error too, never a live socket.
        assert!(matches!(
            assert_gateway_matches(TradingSystem::LiveTrading, &urls.papermoney, true),
            Err(Error::Config(_))
        ));
        // Consistent pairs behave as before.
        assert!(assert_gateway_matches(TradingSystem::PaperMoney, &urls.papermoney, false).is_ok());
        assert!(matches!(
            assert_gateway_matches(TradingSystem::LiveTrading, &urls.livetrading_a, false),
            Err(Error::LiveTradingDisabled)
        ));
        assert!(assert_gateway_matches(TradingSystem::LiveTrading, LIVE_B_URL, true).is_ok());
        // An unknown host is live and therefore gated.
        assert!(matches!(
            assert_gateway_matches(TradingSystem::LiveTrading, "ws://127.0.0.1:1", false),
            Err(Error::LiveTradingDisabled)
        ));
    }
}
