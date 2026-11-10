//! How the charts write P&L: as money, or in R, a multiple of what one unit
//! of risk is worth.
//!
//! Two settings, both read with a default so that a store that has never
//! heard of them reads as it always did: [`DISPLAY`] is `money` or `r`, and
//! [`R_AMOUNT`] what 1R is worth in the account's currency, 500 until
//! somebody says otherwise. Only what is drawn on a chart follows them — the
//! header's P&L, a position's open P&L on its line, an exit's P&L on its
//! mark and while it is dragged. The trading panel's tables are the
//! account's books, and always say money.

use crate::store::Store;

use super::domain::grouped;
use super::words::pnl_text;

/// `money` or `r`: how the charts write P&L.
pub const DISPLAY: &str = "pnl_display";

/// What 1R is worth, in the account's currency: a positive number.
pub const R_AMOUNT: &str = "pnl_r_amount";

/// What 1R is worth until somebody says otherwise.
pub const DEFAULT_R: f64 = 500.0;

/// The choice Preferences offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Display {
    #[default]
    Money,
    Risk,
}

impl Display {
    pub const ALL: [Display; 2] = [Display::Money, Display::Risk];

    pub fn key(self) -> &'static str {
        match self {
            Display::Money => "money",
            Display::Risk => "r",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Display::Money => "Money",
            Display::Risk => "Risk (R)",
        }
    }

    pub fn from_key(key: &str) -> Option<Display> {
        Display::ALL.into_iter().find(|display| display.key() == key)
    }
}

/// How a chart writes a P&L it is given in money.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ChartPnl {
    /// "+$125.00", as the account's books say it.
    #[default]
    Money,
    /// "+0.25R", the money divided by what 1R is worth.
    R(f64),
}

impl ChartPnl {
    /// `value`, money in `currency`, as this chart writes it.
    pub fn text(self, value: f64, currency: Option<&str>) -> String {
        match self {
            ChartPnl::Money => pnl_text(value, currency),
            ChartPnl::R(r) => r_text(value, r),
        }
    }
}

/// Money as a multiple of `r`: "+1.25R", "-0.50R", "+0.00R". Signed and
/// rounded as money is, to two places, so the two read alike.
pub fn r_text(value: f64, r: f64) -> String {
    let rs = value / r;
    let sign = if rs < 0.0 { "-" } else { "+" };
    let hundredths = (rs.abs() * 100.0).round() as u64;
    format!("{sign}{}.{:02}R", grouped(hundredths / 100), hundredths % 100)
}

/// How the charts write P&L, as stored: money unless R was chosen.
pub fn display(store: &Store) -> Display {
    store.setting(DISPLAY).and_then(|key| Display::from_key(key.trim())).unwrap_or_default()
}

/// What 1R is worth, as stored: [`DEFAULT_R`] for anything that is not a
/// positive number, which only `config set` could not have refused.
pub fn r_amount(store: &Store) -> f64 {
    store.setting(R_AMOUNT).and_then(|text| parse_amount(&text).ok()).unwrap_or(DEFAULT_R)
}

/// What the charts write P&L as, from both settings.
pub fn load(store: &Store) -> ChartPnl {
    match display(store) {
        Display::Money => ChartPnl::Money,
        Display::Risk => ChartPnl::R(r_amount(store)),
    }
}

pub fn save_display(store: &Store, display: Display) {
    store.set_setting(DISPLAY, display.key());
}

/// Keep `amount` as what 1R is worth, written the shortest way: `500`,
/// `262.5`.
pub fn save_amount(store: &Store, amount: f64) {
    store.set_setting(R_AMOUNT, &amount_text(amount));
}

pub fn amount_text(amount: f64) -> String {
    format!("{amount}")
}

/// Read `money` or `r` as `config set` is given one.
pub fn parse_display(value: &str) -> Result<Display, String> {
    Display::from_key(value.trim().to_lowercase().as_str())
        .ok_or_else(|| format!("{value:?} is not a way to show P&L; use money or r"))
}

/// Read what 1R is worth: a positive number, nothing else. Blank, zero,
/// negative, and anything that is not a finite number are refused.
pub fn parse_amount(value: &str) -> Result<f64, String> {
    match value.trim().parse::<f64>() {
        Ok(amount) if amount.is_finite() && amount > 0.0 => Ok(amount),
        _ => Err(format!("{value:?} is not what 1R is worth; use a positive number, such as 500")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_is_written_in_r_signed_and_to_two_places() {
        assert_eq!(r_text(625.0, 500.0), "+1.25R");
        assert_eq!(r_text(-250.0, 500.0), "-0.50R");
        assert_eq!(r_text(0.0, 500.0), "+0.00R");
        assert_eq!(r_text(-1_000.0, 500.0), "-2.00R");
        assert_eq!(r_text(100.0, 300.0), "+0.33R");
        assert_eq!(r_text(-200.0, 300.0), "-0.67R");
        assert_eq!(r_text(2.5, 1.0), "+2.50R");
        assert_eq!(r_text(600_000.0, 500.0), "+1,200.00R");
    }

    #[test]
    fn a_chart_writes_money_or_r_as_chosen() {
        assert_eq!(ChartPnl::Money.text(125.0, Some("USD")), "+$125.00");
        assert_eq!(ChartPnl::Money.text(-3.94, Some("EUR")), "-3.94 EUR");
        assert_eq!(ChartPnl::R(500.0).text(125.0, Some("USD")), "+0.25R");
        assert_eq!(ChartPnl::R(250.0).text(-3.94, Some("EUR")), "-0.02R");
        assert_eq!(ChartPnl::R(500.0).text(0.0, None), "+0.00R");
    }

    #[test]
    fn what_1r_is_worth_is_a_positive_number_and_nothing_else() {
        assert_eq!(parse_amount("500"), Ok(500.0));
        assert_eq!(parse_amount(" 262.5 "), Ok(262.5));
        assert_eq!(parse_amount("0.01"), Ok(0.01));
        for refused in ["", "   ", "0", "-500", "abc", "500R", "NaN", "inf", "-0"] {
            assert!(parse_amount(refused).is_err(), "{refused:?} was taken");
        }
        assert_eq!(parse_display("money"), Ok(Display::Money));
        assert_eq!(parse_display("R"), Ok(Display::Risk));
        assert!(parse_display("").is_err());
        assert!(parse_display("risk").is_err());
    }

    /// Never set, it is money and 500; set to nonsense behind its back, the
    /// amount reads as 500 rather than dividing by it.
    #[test]
    fn the_settings_read_with_their_defaults() {
        let store = Store::memory().unwrap();
        assert_eq!(display(&store), Display::Money);
        assert_eq!(r_amount(&store), DEFAULT_R);
        assert_eq!(load(&store), ChartPnl::Money);
        save_display(&store, Display::Risk);
        assert_eq!(load(&store), ChartPnl::R(500.0));
        save_amount(&store, 250.0);
        assert_eq!(store.setting(R_AMOUNT).as_deref(), Some("250"));
        assert_eq!(load(&store), ChartPnl::R(250.0));
        for nonsense in ["", "0", "-5", "lots"] {
            store.set_setting(R_AMOUNT, nonsense);
            assert_eq!(load(&store), ChartPnl::R(DEFAULT_R), "{nonsense:?}");
        }
        store.set_setting(DISPLAY, "sideways");
        assert_eq!(load(&store), ChartPnl::Money);
    }

    /// Kept in the settings file, so a restart finds what was chosen.
    #[test]
    fn the_settings_are_kept_across_a_restart() {
        let dir = std::env::temp_dir().join(format!("omacharts-pnl-r-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("omacharts.db");
        {
            let store = Store::open_at(&path).unwrap();
            save_display(&store, Display::Risk);
            save_amount(&store, 750.0);
        }
        let store = Store::open_at(&path).unwrap();
        assert_eq!(load(&store), ChartPnl::R(750.0));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
