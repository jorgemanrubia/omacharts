//! Gateway-host gate.
//!
//! Charts come from any thinkorswim account, paper or live. What this crate
//! will not do is connect anywhere else: a session names its own gateway URL,
//! and only one of thinkorswim's own hosts — or a loopback address, where the
//! tests put a fake gateway — is opened. "Any account" is not "any URL", and
//! a session file is an ordinary text file that another program on the
//! machine can write to.
//!
//! The gate judges the host it parses out of the URL, with the same parser
//! the socket is opened with, so what is checked and what is connected to
//! cannot drift apart. It never judges a label the file or the server
//! supplies.

use crate::error::{Error, Result};

pub const TOS_WEB_ORIGIN: &str = "https://trade.thinkorswim.com";

/// thinkorswim's live gateway host.
const LIVE_HOST: &str = "thinkorswim-services.schwab.com";
/// The second live gateway host the web client has been seen to land on,
/// and that captured sessions carry. Refusing it would refuse the half of
/// live accounts that happen to be routed there.
const LIVE_HOST_B: &str = "thinkorswim-services-b.tos-prd.prd.gcp.schwabcloud.com";
/// thinkorswim's paperMoney gateway host.
const PAPER_HOST: &str = "papermoney-services.schwab.com";

/// Every host this crate will open a gateway socket to.
///
/// thinkorswim's three, and loopback on top of them (see [`is_loopback`])
/// for a fake gateway that never leaves the machine. [`fallback_gateway_urls`]
/// is built from the same constants, so the hosts that are allowed and the
/// hosts that are used cannot drift apart.
pub const KNOWN_GATEWAY_HOSTS: &[&str] = &[LIVE_HOST, LIVE_HOST_B, PAPER_HOST];

/// Which of thinkorswim's two gateways a session talks to.
///
/// Plumbing rather than a privilege: the two hosts are different servers with
/// different session ids, so the session file keeps a slot for each and the
/// browser capture reports which one it landed on. Nothing above this crate's
/// public API needs to know which one a session is.
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

    /// How to name this gateway to a person, as thinkorswim's own UI does.
    pub fn human(self) -> &'static str {
        match self {
            TradingSystem::LiveTrading => "live trading",
            TradingSystem::PaperMoney => "paperMoney",
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

    /// Which gateway a URL names. Only a `papermoney` host counts as paper;
    /// anything else — including a URL that cannot be parsed, or one that
    /// merely mentions papermoney in its path or query — reads as live.
    ///
    /// A classification, not a decision: it picks the slot the session file
    /// keeps the session in. Whether the URL may be connected to at all is
    /// [`assert_known_gateway`]'s answer, and it is the only one that gates
    /// anything.
    pub fn of_gateway_url(url: &str) -> Self {
        Self::implied_by_gateway_url(url).unwrap_or(TradingSystem::LiveTrading)
    }

    /// Like [`TradingSystem::of_gateway_url`], but `None` for a loopback
    /// host: a local fake gateway stands in for either system, so there the
    /// label decides. Every other host is classified by name.
    pub fn implied_by_gateway_url(url: &str) -> Option<Self> {
        let Some(host) = gateway_host(url) else {
            return Some(TradingSystem::LiveTrading);
        };
        if is_loopback(&host) {
            return None;
        }
        Some(if host.contains("papermoney") {
            TradingSystem::PaperMoney
        } else {
            TradingSystem::LiveTrading
        })
    }
}

/// The host a gateway URL names, lowercased, or `None` for anything this
/// refuses to read as one.
///
/// The same parser the socket is opened with, so what the gate judges and
/// what gets connected to cannot drift apart. A URL with no scheme is not a
/// gateway URL: `//thinkorswim-services.schwab.com/` reads as a host to a
/// parser that tolerates a missing scheme, and refusing it is how that stays
/// unable to name a thinkorswim gateway. Userinfo is not the host either —
/// `Authority::host` is what skips past `thinkorswim-services.schwab.com@`,
/// which is the shape an attempt to fool this would take.
fn gateway_host(url: &str) -> Option<String> {
    let uri: tungstenite::http::Uri = url.trim().parse().ok()?;
    uri.scheme_str()?;
    Some(uri.host()?.to_ascii_lowercase())
}

/// Whether a host is this machine, which is allowed because a gateway on
/// loopback is one of our own tests and never leaves the machine.
///
/// An address only counts if it is written as one. A host that merely parses
/// as a number in some other base is read as a name instead, which leaves it
/// off the allowlist and refused — the safe side of the only mistake this can
/// make.
fn is_loopback(host: &str) -> bool {
    if host == "localhost" {
        return true;
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return inner
            .parse::<std::net::Ipv6Addr>()
            .is_ok_and(|ip| ip.is_loopback());
    }
    host.parse::<std::net::Ipv4Addr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// Whether a URL names a gateway this crate will open a socket to.
pub fn is_known_gateway(url: &str) -> bool {
    match gateway_host(url) {
        Some(host) => is_loopback(&host) || KNOWN_GATEWAY_HOSTS.contains(&host.as_str()),
        None => false,
    }
}

/// The host of a gateway URL as it may be repeated in an error or a log line.
///
/// The host and nothing else: a URL that reached this crate came out of a
/// session file or a browser capture, and the query of such a URL is exactly
/// where a one-time code would sit.
pub fn named_host(url: &str) -> String {
    gateway_host(url).unwrap_or_else(|| "(no host)".into())
}

/// Refuses a gateway that is not one of thinkorswim's.
///
/// The one gate between a saved session and a socket. It runs before anything
/// is opened, and it reads the host out of the URL rather than trusting what
/// the session calls itself.
pub fn assert_known_gateway(gateway_url: &str) -> Result<()> {
    if is_known_gateway(gateway_url) {
        return Ok(());
    }
    Err(Error::Config(format!(
        "{} is not a thinkorswim gateway host",
        named_host(gateway_url)
    )))
}

/// [`assert_known_gateway`], plus a refusal of a configuration whose
/// trading-system label disagrees with the gateway it would connect to.
///
/// The label picks the slot a session is written back to, so a label at odds
/// with the host would file a live session under paperMoney's keys and lose
/// it. A loopback gateway implies neither system, so there the label stands.
pub fn assert_gateway_allowed(trading_system: TradingSystem, gateway_url: &str) -> Result<()> {
    assert_known_gateway(gateway_url)?;
    if let Some(implied) = TradingSystem::implied_by_gateway_url(gateway_url) {
        if implied != trading_system {
            return Err(Error::Config(format!(
                "trading system {trading_system} does not match gateway host {} ({implied})",
                named_host(gateway_url)
            )));
        }
    }
    Ok(())
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

/// Last-known gateway URLs, one per trading system. The session file carries
/// the URL that is actually used; these are what a capture falls back to when
/// it read a token out of the SPA's storage without seeing the socket.
pub fn fallback_gateway_urls() -> GatewayUrls {
    GatewayUrls {
        livetrading_a: format!("wss://{LIVE_HOST}/Services/WsJson"),
        papermoney: format!("wss://{PAPER_HOST}/Services/WsJson"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The second live gateway the SPA has been seen to land on.
    const LIVE_B_URL: &str =
        "wss://thinkorswim-services-b.tos-prd.prd.gcp.schwabcloud.com/Services/WsJson";

    #[test]
    fn either_thinkorswim_gateway_is_allowed() {
        let urls = fallback_gateway_urls();
        // The change this crate exists to make: a live account charts.
        assert!(assert_known_gateway(&urls.livetrading_a).is_ok());
        assert!(assert_known_gateway(&urls.papermoney).is_ok());
        assert!(
            assert_gateway_allowed(TradingSystem::LiveTrading, &urls.livetrading_a).is_ok()
        );
        assert!(assert_gateway_allowed(TradingSystem::PaperMoney, &urls.papermoney).is_ok());
        // Case and surrounding blanks are not a way past the list.
        assert!(assert_known_gateway("  wss://ThinkorSwim-Services.schwab.com/x ").is_ok());
    }

    #[test]
    fn a_gateway_on_this_machine_is_allowed_for_the_fake_one_the_tests_run() {
        for url in [
            "ws://127.0.0.1:8765/Services/WsJson",
            "ws://localhost:8765/",
            "ws://[::1]:8765/",
            "ws://127.9.9.9:1/",
        ] {
            assert!(assert_known_gateway(url).is_ok(), "{url}");
            assert_eq!(TradingSystem::implied_by_gateway_url(url), None, "{url}");
            // On loopback the label stands, whichever it is.
            assert!(assert_gateway_allowed(TradingSystem::PaperMoney, url).is_ok(), "{url}");
            assert!(assert_gateway_allowed(TradingSystem::LiveTrading, url).is_ok(), "{url}");
        }
    }

    #[test]
    fn a_host_that_is_not_thinkorswims_is_refused() {
        for url in [
            // Nothing to do with thinkorswim.
            "wss://evil.example/Services/WsJson",
            // Userinfo is not the host.
            "wss://thinkorswim-services.schwab.com@evil.example/Services/WsJson",
            "wss://papermoney-services.schwab.com@evil.example/",
            // Nor is a path, a query or a fragment.
            "wss://evil.example/thinkorswim-services.schwab.com",
            "wss://evil.example/?h=papermoney-services.schwab.com",
            "wss://evil.example/#thinkorswim-services.schwab.com",
            // A host with no scheme is not a gateway URL at all.
            "//thinkorswim-services.schwab.com/Services/WsJson",
            // A subdomain of a known host is a different machine.
            "wss://thinkorswim-services.schwab.com.evil.example/",
            // Loopback written in a base nobody writes it in is a name.
            "ws://2130706433/",
            "ws://0177.0.0.1/",
            // Not a URL.
            "nope",
            "",
        ] {
            let error = assert_known_gateway(url).expect_err(url);
            assert!(
                matches!(error, Error::Config(ref m) if m.contains("not a thinkorswim gateway")),
                "{url}: {error}"
            );
            // And nothing a label says gets past it either.
            for system in [TradingSystem::PaperMoney, TradingSystem::LiveTrading] {
                assert!(assert_gateway_allowed(system, url).is_err(), "{url} as {system}");
            }
        }
    }

    /// The refusal names the host, never the URL: a URL that got this far
    /// came out of a session file or a browser capture, and its query is
    /// where a one-time code would be.
    #[test]
    fn a_refusal_quotes_the_host_and_not_the_rest_of_the_url() {
        let error = assert_known_gateway("wss://evil.example/Services/WsJson?code=SECRET")
            .expect_err("refused");
        let message = error.to_string();
        assert!(message.contains("evil.example"), "{message}");
        assert!(!message.contains("SECRET"), "{message}");
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
            TradingSystem::of_gateway_url("not a url papermoney"),
            TradingSystem::LiveTrading
        );
        assert_eq!(
            TradingSystem::of_gateway_url("  wss://PaperMoney-services.schwab.com/x "),
            TradingSystem::PaperMoney
        );
    }

    /// The label never decides anything the host disagrees with, which is
    /// what keeps a live session from being filed under paperMoney's keys.
    #[test]
    fn a_label_that_disagrees_with_the_host_is_a_config_error() {
        let urls = fallback_gateway_urls();
        assert!(matches!(
            assert_gateway_allowed(TradingSystem::PaperMoney, &urls.livetrading_a),
            Err(Error::Config(ref m)) if m.contains("PaperMoney") && m.contains("LiveTrading")
        ));
        assert!(matches!(
            assert_gateway_allowed(TradingSystem::LiveTrading, &urls.papermoney),
            Err(Error::Config(_))
        ));
    }

    /// The allowlist is built from the URLs the capture falls back to, so a
    /// fallback this crate would write down is a gateway it can open.
    #[test]
    fn the_fallback_gateways_are_on_the_allowlist() {
        let urls = fallback_gateway_urls();
        for url in [&urls.livetrading_a, &urls.papermoney] {
            assert!(is_known_gateway(url), "{url}");
        }
        assert!(is_known_gateway(LIVE_B_URL), "the second live gateway is thinkorswim's too");
        assert_eq!(KNOWN_GATEWAY_HOSTS.len(), 3);
    }
}
