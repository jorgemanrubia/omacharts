//! Indicators.
//!
//! Everything here is a pure function of bars plus parameters, which is what
//! makes it testable without a window and cheap to recompute on every repaint.
//! The renderer receives values and a style, and does no arithmetic of its own.
//!
//! Colour is not stored by default. An indicator carries a [`ColorChoice`],
//! and an unset one resolves through the active theme's palette in sequence —
//! so adding four indicators gives four colours that already work together,
//! in any theme, including one the user edited.

pub mod bands;
pub mod bollinger;
pub mod cvd;
pub mod flow;
pub mod market;
pub mod oscillators;
mod paint;
pub mod periods;
pub mod profile;
pub mod time;
pub mod vwap;

use serde::{Deserialize, Serialize};

use chrono_tz::Tz;

use crate::bars::{Bar, Timeframe};
use crate::ladders::{Days, Held, Ladder};
use crate::symbols::Instrument;
use crate::colouring::{ColoursMut, Colouring};
use crate::theme::{ColorChoice, Theme, SWATCH_SEQUENCE};

pub use bands::{Band, BandSeries, Bands, FILL_ALPHA, MAX_FILL_ALPHA, MIN_FILL_ALPHA};
pub use periods::Reset;
pub use market::MarketProfile;
pub use profile::{Profile, ProfileRow};
pub use time::{Hours, Preset, Span, Time, TimeRange};

/// How a line is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl LineStyle {
    pub const ALL: [LineStyle; 3] = [LineStyle::Solid, LineStyle::Dashed, LineStyle::Dotted];

    pub fn label(self) -> &'static str {
        match self {
            LineStyle::Solid => "Solid",
            LineStyle::Dashed => "Dashed",
            LineStyle::Dotted => "Dotted",
        }
    }

    /// The dash pattern, scaled to the line's width so a thick dashed line
    /// does not come out as a row of squares.
    pub fn dashes(self, width: f64) -> Vec<f64> {
        let unit = width.max(1.0);
        match self {
            LineStyle::Solid => Vec::new(),
            LineStyle::Dashed => vec![unit * 3.0, unit * 3.0],
            LineStyle::Dotted => vec![unit, unit * 2.0],
        }
    }

    /// The same pattern for a line drawn with round caps, as a drawing's
    /// strokes are. A round cap adds half the width to each end of a dash,
    /// so every dash is a width shorter and every gap a width longer than
    /// [`dashes`] says, to come out the same on screen; and a dot is a
    /// dash of no length, a cap on its own, which is a round dot exactly
    /// the line's width where a short butt-capped dash is a square.
    ///
    /// [`dashes`]: LineStyle::dashes
    pub fn round_dashes(self, width: f64) -> Vec<f64> {
        let unit = width.max(1.0);
        match self {
            LineStyle::Solid => Vec::new(),
            LineStyle::Dashed => vec![unit * 2.0, unit * 4.0],
            LineStyle::Dotted => vec![0.0, unit * 2.0],
        }
    }

    /// The pattern a line has unless somebody chose another.
    pub fn is_solid(&self) -> bool {
        *self == LineStyle::Solid
    }
}

/// A line's weight and pattern.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Stroke {
    /// Zero means the line is not drawn at all, which is how a shaded band
    /// gets a clean edge without borrowing the background colour.
    pub width: f64,
    pub style: LineStyle,
}

impl Default for Stroke {
    fn default() -> Stroke {
        Stroke { width: 1.5, style: LineStyle::Solid }
    }
}

impl Stroke {
    pub const fn new(width: f64, style: LineStyle) -> Stroke {
        Stroke { width, style }
    }

    /// Nothing to draw.
    pub fn is_hidden(&self) -> bool {
        self.width <= 0.0
    }
}

/// The indicators we know how to compute.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Sma,
    Ema,
    Vwap,
    Bollinger,
    VolumeProfile,
    MarketProfile,
    Volume,
    Cvd,
    VolumeDelta,
    Footprint,
    LargeOrders,
    Rsi,
    Macd,
    Atr,
    Stochastic,
    SessionTimes,
}

/// What an indicator is computed from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Needs {
    /// Bars, which every source has.
    Bars,
    /// What traded at each price and on which side: ladders, which only some
    /// sources have. See [`unmet`].
    VolumeAtPrice,
}

/// The line an indicator that needs volume at price shows on a chart whose
/// source has none.
pub const NEEDS_VOLUME_AT_PRICE: &str = "Needs a data feed with volume at price";

/// What an indicator that needs `needs` shows instead of itself on a chart
/// of `context`, if it cannot be computed there: one line, rather than an
/// invented estimate. `None` when it can.
pub fn unmet(needs: Needs, context: &Context) -> Option<Output> {
    match needs {
        Needs::Bars => None,
        Needs::VolumeAtPrice => context.ladders.is_none().then_some(Output::Unmet(NEEDS_VOLUME_AT_PRICE)),
    }
}

/// The families the picker and the manual sort indicators into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Averages,
    Volume,
    OrderFlow,
    Momentum,
    Time,
}

impl Category {
    pub const ALL: [Category; 5] =
        [Category::Averages, Category::Volume, Category::OrderFlow, Category::Momentum, Category::Time];

    pub fn label(self) -> &'static str {
        match self {
            Category::Averages => "Averages and bands",
            Category::Volume => "Volume",
            Category::OrderFlow => "Order flow",
            Category::Momentum => "Momentum and volatility",
            Category::Time => "Sessions and time",
        }
    }

    /// What typing the family's name into the picker should find.
    fn aliases(self) -> &'static [&'static str] {
        match self {
            Category::Averages => &["average", "averages", "bands", "trend", "overlay"],
            Category::Volume => &["volume"],
            Category::OrderFlow => &["order flow", "orderflow", "delta", "aggressor", "bid ask", "tape"],
            Category::Momentum => &["momentum", "volatility", "oscillator", "oscillators"],
            Category::Time => &["session", "sessions", "time", "hours", "rth", "eth"],
        }
    }

    /// Its members, in the order the picker lists them.
    pub fn kinds(self) -> Vec<Kind> {
        Kind::ALL.into_iter().filter(|kind| kind.category() == self).collect()
    }
}

impl Kind {
    pub const ALL: [Kind; 16] = [
        Kind::Volume,
        Kind::Sma,
        Kind::Ema,
        Kind::Vwap,
        // Beside the VWAP rather than beside the averages: the two draw the
        // same picture, and somebody who has used one is looking for the other.
        Kind::Bollinger,
        Kind::VolumeProfile,
        // Beside the volume profile: the same picture of a period, by time
        // rather than volume, and the two are drawn side by side.
        Kind::MarketProfile,
        // Order flow, from the whole session down to the single trade: the
        // running delta, each bar's delta, each price's inside the bar, and
        // the orders big enough to stand out on their own.
        Kind::Cvd,
        Kind::VolumeDelta,
        Kind::Footprint,
        Kind::LargeOrders,
        Kind::Rsi,
        Kind::Macd,
        Kind::Atr,
        Kind::Stochastic,
        Kind::SessionTimes,
    ];

    pub fn category(self) -> Category {
        match self {
            Kind::Sma | Kind::Ema | Kind::Vwap | Kind::Bollinger => Category::Averages,
            Kind::Volume | Kind::VolumeProfile | Kind::MarketProfile => Category::Volume,
            Kind::Cvd | Kind::VolumeDelta | Kind::Footprint | Kind::LargeOrders => Category::OrderFlow,
            Kind::Rsi | Kind::Macd | Kind::Atr | Kind::Stochastic => Category::Momentum,
            Kind::SessionTimes => Category::Time,
        }
    }

    /// What it cannot be computed without. Most need bars and nothing more;
    /// one that needs volume at price — the footprint, the large orders — is
    /// shown on a chart whose source has none as one line saying so, never
    /// as an estimate.
    pub fn needs(self) -> Needs {
        match self {
            Kind::Footprint | Kind::LargeOrders => Needs::VolumeAtPrice,
            _ => Needs::Bars,
        }
    }

    /// Whether it draws more exactly from volume at price when the chart's
    /// source has it: the volume profile, which otherwise spreads each
    /// bar's volume over its range, and the cumulative delta, which
    /// otherwise estimates each bar's side from its direction. A chart holds
    /// ladders only while one of its indicators says yes here or needs them
    /// outright.
    pub fn uses_volume_at_price(self) -> bool {
        matches!(self, Kind::VolumeProfile | Kind::Cvd | Kind::VolumeDelta) || self.needs() == Needs::VolumeAtPrice
    }

    /// Whether a new one is named in the chart's legend. Session shading is
    /// the chart's ground, and a line naming the ground is a line nobody
    /// needs to read.
    pub fn in_legend_by_default(self) -> bool {
        !matches!(self, Kind::SessionTimes)
    }

    /// Whether a colour from the palette is handed to it when nobody chose
    /// one. Session shading is the chart's ground, not a series: given the
    /// next sequence colour it would take the blue the first moving average
    /// was going to get, and shout across the whole chart. Its ranges each
    /// carry a colour of their own instead.
    pub fn takes_palette_colour(self) -> bool {
        !matches!(self, Kind::SessionTimes)
    }

    /// Whether a click on the chart can select it. Session shading is the
    /// ground every bar stands on: a click anywhere would land on it, so it
    /// is reached from the Indicators page instead.
    pub fn selectable(self) -> bool {
        !matches!(self, Kind::SessionTimes)
    }

    pub fn name(self) -> &'static str {
        match self {
            // Spelled out. The short form is beside it in its own column, so
            // repeating it in brackets says the same thing twice.
            Kind::Sma => "Simple Moving Average",
            Kind::Ema => "Exponential Moving Average",
            Kind::Vwap => "Volume Weighted Average Price",
            Kind::Bollinger => "Bollinger Bands",
            Kind::Macd => "Moving Average Convergence Divergence",
            Kind::VolumeProfile => "Volume Profile",
            Kind::MarketProfile => "Market Profile",
            Kind::Volume => "Volume",
            Kind::Cvd => "Cumulative Delta",
            Kind::VolumeDelta => "Volume Delta",
            Kind::Footprint => "Footprint",
            Kind::LargeOrders => "Large Orders",
            Kind::Rsi => "Relative Strength Index",
            Kind::Atr => "Average True Range",
            Kind::Stochastic => "Stochastic",
            Kind::SessionTimes => "Session Times",
        }
    }

    /// What the chart shows. Short, because it sits over the drawing, where
    /// the full name is a sentence in the way of the candles — the picker is
    /// where it is spelled out.
    pub fn short_name(self) -> &'static str {
        match self {
            Kind::Sma => "SMA",
            Kind::Ema => "EMA",
            Kind::Vwap => "VWAP",
            Kind::Bollinger => "BB",
            Kind::Macd => "MACD",
            Kind::VolumeProfile => "VP",
            Kind::MarketProfile => "TPO",
            Kind::Volume => "Vol",
            Kind::Cvd => "CVD",
            Kind::VolumeDelta => "VD",
            Kind::Footprint => "FP",
            Kind::LargeOrders => "Large",
            Kind::Rsi => "RSI",
            Kind::Atr => "ATR",
            Kind::Stochastic => "Stoch",
            Kind::SessionTimes => "Sessions",
        }
    }

    /// Searched alongside the name, so "moving average" and "ma" both find
    /// both averages, and "poc" finds the profile.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Kind::Sma => &["sma", "ma", "moving average", "mean"],
            Kind::Ema => &["ema", "ma", "moving average", "exponential"],
            Kind::Vwap => &["vwap", "volume weighted", "average price", "bands"],
            Kind::Bollinger => {
                &["bollinger", "bb", "bands", "bollinger bands", "deviation", "volatility"]
            }
            Kind::Macd => {
                &["macd", "convergence", "divergence", "histogram", "signal", "momentum"]
            }
            Kind::VolumeProfile => &["volume profile", "vp", "poc", "value area"],
            Kind::MarketProfile => &[
                "market profile", "tpo", "mp", "time price opportunity", "time at price", "letters", "poc",
                "value area",
            ],
            Kind::Volume => &["volume", "vol", "turnover"],
            Kind::Cvd => &[
                "cumulative delta", "cvd", "delta", "order flow", "cumulative volume delta", "volume delta",
                "aggressor", "bid ask",
            ],
            Kind::VolumeDelta => &[
                "volume delta", "vd", "delta", "bar delta", "order flow", "aggressor", "bid ask",
            ],
            Kind::Footprint => &[
                "footprint", "fp", "delta", "order flow", "bid ask", "numbers bars", "cluster", "volume at price",
            ],
            Kind::LargeOrders => &[
                "large orders", "large trades", "big trades", "bubbles", "block trades", "whales", "tape",
                "order flow", "large volume",
            ],
            Kind::Rsi => &["rsi", "relative strength", "oscillator", "momentum", "overbought"],
            Kind::Atr => &["atr", "average true range", "volatility", "range", "stop"],
            Kind::Stochastic => &["stochastic", "stoch", "kd", "%k", "oscillator", "momentum", "overbought"],
            Kind::SessionTimes => &[
                "session times", "sessions", "session", "eth", "rth", "extended hours",
                "regular hours", "overnight", "london", "asia", "new york", "background",
                "shading", "time range", "hours",
            ],
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Sma => "sma",
            Kind::Ema => "ema",
            Kind::Vwap => "vwap",
            Kind::Bollinger => "bollinger",
            Kind::Macd => "macd",
            Kind::VolumeProfile => "volume_profile",
            Kind::MarketProfile => "market_profile",
            Kind::Volume => "volume",
            Kind::Cvd => "cvd",
            Kind::VolumeDelta => "volume_delta",
            Kind::Footprint => "footprint",
            Kind::LargeOrders => "large_orders",
            Kind::Rsi => "rsi",
            Kind::Atr => "atr",
            Kind::Stochastic => "stochastic",
            Kind::SessionTimes => "session_times",
        }
    }

    /// Does this draw in a strip of its own rather than over the price?
    ///
    /// A computed series already knows — that is [`Output::pane_height`] — but
    /// the order of the strips is edited where nothing has been computed: the
    /// settings list, and the command line.
    pub fn in_own_pane(self) -> bool {
        matches!(self, Kind::Volume | Kind::Cvd | Kind::VolumeDelta | Kind::Rsi | Kind::Macd | Kind::Atr | Kind::Stochastic)
    }

    pub fn default_params(self) -> Params {
        match self {
            Kind::Sma => Params::MovingAverage { period: 50 },
            Kind::Ema => Params::MovingAverage { period: 21 },
            Kind::Vwap => Params::Vwap { reset: Reset::Session, bands: vwap::default_bands() },
            // Twenty bars at two standard deviations: what Bollinger
            // published, what `ta.bb` defaults to, and what the level
            // somebody is watching on another chart was drawn with.
            Kind::Bollinger => {
                Params::Bollinger { period: 20, bands: bollinger::default_bands() }
            }
            Kind::VolumeProfile => Params::VolumeProfile {
                reset: Reset::Session,
                rows: None,
                value_area: 0.70,
                poc_color: None,
                style: profile::Style::Filled,
                outline: false,
                outline_color: None,
                outline_style: LineStyle::Solid,
                outline_width: 1.0,
                poc_width: 1.0,
                poc_style: LineStyle::Solid,
                poc: true,
                alpha: profile::DESIGNED_ALPHA,
                opacity: 1.0,
                highlight: true,
                value_area_color: None,
                value_area_alpha: None,
                outside_alpha: None,
                outline_alpha: None,
                poc_alpha: None,
                last_only: false,
                align: profile::Align::Left,
                width: profile::DEFAULT_WIDTH,
                method: profile::Method::Total,
                colouring: None,
                buy_color: None,
                sell_color: None,
                merged: Vec::new(),
            },
            // Half-hour TPO periods and a 70% value area: what Steidlmayer
            // drew and what TradingView and Sierra Chart open with.
            Kind::MarketProfile => Params::MarketProfile {
                reset: Reset::Session,
                minutes: market::DEFAULT_MINUTES,
                row_ticks: None,
                value_area: market::DEFAULT_VALUE_AREA,
                style: market::Style::Letters,
                align: profile::Align::Left,
                width: profile::DEFAULT_WIDTH,
                poc: true,
                highlight: true,
                poc_color: None,
                poc_width: market::POC_WIDTH,
                poc_style: LineStyle::Solid,
                value_area_color: None,
                open_close: true,
                open_color: None,
                close_color: None,
                open_alpha: None,
                close_alpha: None,
                outline: false,
                outline_color: None,
                outline_style: LineStyle::Solid,
                outline_width: profile::hairline(),
                outline_alpha: None,
                alpha: None,
                value_area_alpha: None,
                outside_alpha: None,
                poc_alpha: None,
                last_only: false,
                merged: Vec::new(),
                expand_last: false,
                expanded: Vec::new(),
            },
            Kind::Volume => Params::Volume {
                height: 0.18,
                colouring: None,
                up_color: None,
                down_color: None,
                opacity: DEFAULT_VOLUME_OPACITY,
                up_alpha: None,
                down_alpha: None,
            },
            Kind::Cvd => Params::Cvd {
                reset: Some(Reset::Session),
                style: cvd::Style::Candles,
                height: 0.16,
                colouring: None,
                up_color: None,
                down_color: None,
            },
            Kind::VolumeDelta => {
                Params::VolumeDelta { height: flow::volume_delta_height(), inks: flow::Inks::default() }
            }
            Kind::Footprint => Params::Footprint {
                intensity: true,
                numbers: true,
                row_ticks: None,
                inks: flow::Inks::default(),
            },
            Kind::LargeOrders => Params::LargeOrders {
                regular: flow::REGULAR_THRESHOLD,
                extended: flow::EXTENDED_THRESHOLD,
                compare: flow::Compare::default(),
                combine: true,
                inks: flow::Inks::default(),
            },
            // Fourteen bars, overbought at seventy, oversold at thirty: the
            // numbers Wilder published and the ones every other chart shows,
            // so a level somebody is watching elsewhere is the same level here.
            Kind::Rsi => Params::Rsi {
                period: 14,
                height: 0.16,
                overbought: 70.0,
                oversold: 30.0,
            },
            Kind::Atr => Params::Atr { period: 14, height: 0.16 },
            // Twelve, twenty-six and nine, which is what Appel published and
            // what `ta.macd` defaults to.
            Kind::Macd => {
                Params::Macd {
                fast: 12,
                slow: 26,
                signal: 9,
                height: 0.16,
                signal_color: None,
                signal_style: None,
                signal_alpha: None,
            }
            }
            // TradingView's: fourteen bars, %K and %D each smoothed over three,
            // overbought at eighty and oversold at twenty.
            Kind::Stochastic => Params::Stochastic {
                period: 14,
                k_smooth: 3,
                d_period: 3,
                height: 0.16,
                overbought: 80.0,
                oversold: 20.0,
                d_color: None,
                d_style: None,
                d_alpha: None,
            },
            // The extended hours, set back: what most charts of a market with
            // a regular session want, and the one range that needs no clock.
            Kind::SessionTimes => Params::SessionTimes { ranges: vec![time::Preset::Eth.range()] },
        }
    }
}

/// A strength somebody set, kept to nought to one, or `otherwise` when
/// none was set or what was set is not a number: what every optional alpha
/// on an indicator is read through.
pub fn strength_or(set: Option<f64>, otherwise: f64) -> f64 {
    set.filter(|alpha| alpha.is_finite()).map_or(otherwise, |alpha| alpha.clamp(0.0, 1.0))
}

/// The least opacity a volume pane's bars take: none, as every other
/// colour's opacity can be set to. It was a fifth, from when the bars had a
/// row of their own for it; nothing stored below that, so nothing changes.
pub const MIN_VOLUME_OPACITY: f64 = 0.0;
/// How solid volume bars are drawn unless somebody chose otherwise.
pub const DEFAULT_VOLUME_OPACITY: f64 = 0.85;

fn default_volume_opacity() -> f64 {
    DEFAULT_VOLUME_OPACITY
}

fn cvd_session() -> Option<Reset> {
    Some(Reset::Session)
}

fn cvd_height() -> f64 {
    0.16
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Params {
    MovingAverage {
        period: usize,
    },
    Vwap {
        reset: Reset,
        /// Three bands, off until asked for.
        bands: Vec<bands::Band>,
    },
    Bollinger {
        /// Bars the average and the deviation are both taken over.
        period: usize,
        /// The envelope, in the same shape a VWAP's bands are so that one
        /// editor and one renderer serve both. Exactly one of them, because
        /// the multiple it sits at is what a Bollinger is.
        bands: Vec<bands::Band>,
    },
    VolumeProfile {
        reset: Reset,
        /// How many rows each period is divided into, or `None` to let the
        /// instrument decide: a row height snapped to a round multiple of its
        /// own quote increment. A stored number is honoured as it always was,
        /// so a profile somebody tuned by hand stays tuned.
        #[serde(default)]
        rows: Option<usize>,
        /// Fraction of volume the value area covers.
        value_area: f64,
        /// The line across the busiest price. Unset follows the profile's own
        /// colour, which is what it did before it could be set apart.
        #[serde(default)]
        poc_color: Option<ColorChoice>,
        /// Solid rows, or only their outline.
        #[serde(default)]
        style: profile::Style,
        /// Whether filled rows wear an outline too. The outline style is
        /// nothing but its outline, so this is for the filled one. Off until
        /// asked for.
        #[serde(default)]
        outline: bool,
        /// The outline's colour. Unset is the profile's own.
        #[serde(default)]
        outline_color: Option<ColorChoice>,
        /// The outline's pattern. Missing, solid, as it always was.
        #[serde(default, skip_serializing_if = "LineStyle::is_solid")]
        outline_style: LineStyle,
        /// The outline's thickness, in pixels.
        #[serde(default = "profile::hairline")]
        outline_width: f64,
        /// The point of control's thickness, in pixels.
        #[serde(default = "profile::hairline")]
        poc_width: f64,
        /// The point of control's pattern. Missing, solid.
        #[serde(default, skip_serializing_if = "LineStyle::is_solid")]
        poc_style: LineStyle,
        /// Whether the point of control is drawn. Missing, it is, as every
        /// profile drew it before it could be turned off.
        #[serde(default = "profile::shown")]
        poc: bool,
        /// How solid the value area's rows are: 0 is nothing, 1 is the
        /// colour itself. The Transparency row, the other way up; the rest
        /// of the rows follow it, and on the outline style, which has no
        /// rows, the outline does. Missing, the strength the rows were
        /// designed at, as every profile was drawn before this was stored.
        /// See [`profile::drawn_alpha`].
        #[serde(default = "profile::designed_alpha", skip_serializing_if = "profile::is_designed_alpha")]
        alpha: f64,
        /// A fade over `alpha`, from before the Transparency row set the
        /// strength itself: 1 draws `alpha` as it is. A profile stored with
        /// one is drawn as it was; the row folds it into `alpha` on the
        /// next change. Missing, 1.
        #[serde(default = "profile::opaque", skip_serializing_if = "profile::is_opaque")]
        opacity: f64,
        /// Whether the value area is set apart from the other rows. Off,
        /// every row is drawn alike. Missing, it is, as every profile was.
        #[serde(default = "profile::shown", skip_serializing_if = "profile::is_shown")]
        highlight: bool,
        /// The value area's own colour, while it is highlighted. Unset is
        /// the background's, so the value area is the same colour stronger,
        /// which is what it always was.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_area_color: Option<ColorChoice>,
        /// How solid the value area's rows, the rows outside it, the outline
        /// and the point of control are drawn, each set apart from `alpha`.
        /// Unset, each follows it as it always did. See
        /// [`profile::Strengths`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_area_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outside_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        poc_alpha: Option<f64>,
        /// Whether only the newest period's profile is drawn, the ones
        /// before it left out. Missing, every period is, as before it could
        /// be chosen.
        #[serde(default)]
        last_only: bool,
        /// The edge of the period the rows grow from.
        #[serde(default)]
        align: profile::Align,
        /// How much of its room the longest row reaches, as a share: of the
        /// period's width, or of the empty space past the last bar for the
        /// period drawn there. Missing, it is the 40% every profile was drawn
        /// at before it could be chosen.
        #[serde(default = "profile::default_width")]
        width: f64,
        /// What each row measures: all the volume at its price, or the ask
        /// volume less the bid volume there. Missing, the total it always was.
        #[serde(default)]
        method: profile::Method,
        /// The colouring a delta's bought and sold rows are painted in, as
        /// a chart's rising and falling bars are: unset, the colouring
        /// the chart's bars wear, and following it as it changes; set, one
        /// of [`profile::DELTA_COLOURINGS`] of its own, or Custom: see
        /// [`crate::colouring`]. A total is in the profile's colour and pays
        /// this no mind.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colouring: Option<Colouring>,
        /// A delta's bought and sold rows' own colours, while Custom.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        buy_color: Option<ColorChoice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sell_color: Option<ColorChoice>,
        /// Where each period starts that was merged into the one before it,
        /// as the start of its reset period. Several in a row make one
        /// composite of them all.
        ///
        /// Never written down with the indicator: the chart keeps them, and
        /// hands them in to be drawn. See [`profile::Merge`].
        #[serde(skip)]
        merged: Vec<i64>,
    },
    MarketProfile {
        reset: Reset,
        /// How long each TPO period is, in minutes. Missing, half an hour.
        #[serde(default = "market::default_minutes")]
        minutes: u32,
        /// How tall a row is, in ticks of the instrument's tick, or `None`
        /// for the automatic height: see [`market::TARGET_ROWS`].
        #[serde(default)]
        row_ticks: Option<u32>,
        /// Fraction of the TPOs the value area covers. Missing, 70%.
        #[serde(default = "market::default_value_area")]
        value_area: f64,
        /// Each TPO drawn as its period's letter, or as a block.
        #[serde(default)]
        style: market::Style,
        /// The edge of the period the rows grow from, as a volume
        /// profile's.
        #[serde(default)]
        align: profile::Align,
        /// How much of its room the longest row reaches, as a volume
        /// profile's.
        #[serde(default = "profile::default_width")]
        width: f64,
        /// Whether the point of control is drawn: a line across the profile
        /// through its row. Missing, it is.
        #[serde(default = "profile::shown")]
        poc: bool,
        /// Whether the value area's rows are set apart from the rest.
        /// Missing, they are.
        #[serde(default = "profile::shown")]
        highlight: bool,
        /// The point of control's colour. Unset is the theme's companion
        /// to the profile's own, as a volume profile's is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        poc_color: Option<ColorChoice>,
        /// The point of control's thickness, in pixels: a line across the
        /// profile through its row, as a volume profile's. Missing,
        /// [`market::POC_WIDTH`].
        #[serde(default = "market::poc_width", skip_serializing_if = "market::is_poc_width")]
        poc_width: f64,
        /// The point of control's pattern. Missing, solid.
        #[serde(default, skip_serializing_if = "LineStyle::is_solid")]
        poc_style: LineStyle,
        /// The value area's colour, while it is set apart. Unset is the
        /// profile's own, the rest drawn fainter.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_area_color: Option<ColorChoice>,
        /// Whether each profile marks its open and its close: an arrow
        /// beside the TPO at its first bar's opening price, and one beside
        /// the TPO at its last bar's closing price (the latest, while it
        /// forms), each in a colour of its own. Missing, they are. Read from `first_last` too,
        /// its name before it was released.
        #[serde(default = "profile::shown", alias = "first_last")]
        open_close: bool,
        /// The open's colour. Unset is the theme's, worked out from its up
        /// colour: see [`crate::theme::Theme::tpo_open`].
        #[serde(default, alias = "first_color", skip_serializing_if = "Option::is_none")]
        open_color: Option<ColorChoice>,
        /// The close's colour. Unset is the theme's, worked out from its
        /// down colour: see [`crate::theme::Theme::tpo_close`].
        #[serde(default, alias = "last_color", skip_serializing_if = "Option::is_none")]
        close_color: Option<ColorChoice>,
        /// How solid the open and close arrows are drawn, nought to one.
        /// Unset, solid: see [`market::end_strength`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        open_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        close_alpha: Option<f64>,
        /// Whether the TPOs wear an outline: the stepped edge round their
        /// cells, as a volume profile's round its rows. Missing, they do
        /// not.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        outline: bool,
        /// The outline's colour. Unset is the profile's own.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline_color: Option<ColorChoice>,
        /// The outline's pattern. Missing, solid.
        #[serde(default, skip_serializing_if = "LineStyle::is_solid")]
        outline_style: LineStyle,
        /// The outline's thickness, in pixels. Missing, a hairline.
        #[serde(default = "profile::hairline", skip_serializing_if = "profile::is_hairline")]
        outline_width: f64,
        /// How solid the outline is drawn. Unset, as a volume profile's
        /// edge is: see [`market::OUTLINE_ALPHA`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline_alpha: Option<f64>,
        /// How solid its rows are drawn, as letters: unset, as designed. A
        /// block is drawn as much fainter as its design is. See
        /// [`market::Strengths`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alpha: Option<f64>,
        /// The value area's rows, the rows outside it and the point of
        /// control's, each set apart from `alpha`; unset, each follows it as
        /// designed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_area_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outside_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        poc_alpha: Option<f64>,
        /// Whether only the newest period's profile is drawn, as a volume
        /// profile's can be. Missing, every period is.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        last_only: bool,
        /// Where each period starts that was merged into the one before it,
        /// as a volume profile's: kept by the chart, never with the
        /// indicator, and handed in to be drawn. See [`profile::Merge`].
        #[serde(skip)]
        merged: Vec<i64>,
        /// Whether the newest profile, the one forming, is always drawn
        /// expanded: its TPOs laid out over time, a column for each TPO
        /// period at that period's bars, rather than stacked against its
        /// edge. Missing, it is stacked unless expanded by hand.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        expand_last: bool,
        /// Where each profile starts that was expanded by hand: kept by the
        /// chart, as its merges are, and handed in to be drawn. See
        /// [`profile::Merge`].
        #[serde(skip)]
        expanded: Vec<i64>,
    },
    Volume {
        /// How much of the chart's height the pane takes.
        height: f64,
        /// How its bars are painted: unset, the way the chart paints its
        /// candles, so volume follows the chart; set, a colouring of its
        /// own, so a quiet volume can sit under loud candles, or the other
        /// way round; or Custom, its own two colours: see
        /// [`crate::colouring`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colouring: Option<Colouring>,
        /// The bars of a rising and a falling session. Unset is the candles'
        /// own colour, so volume reads as belonging to the bar above it.
        #[serde(default)]
        up_color: Option<ColorChoice>,
        #[serde(default)]
        down_color: Option<ColorChoice>,
        /// How solid its bars are drawn, from [`MIN_VOLUME_OPACITY`] to 1,
        /// where 1 hides nothing behind them. A chart saved before it was
        /// a choice opens at the default.
        #[serde(default = "default_volume_opacity")]
        opacity: f64,
        /// How solid a rising and a falling bar are drawn, each set apart
        /// from `opacity`. Unset, each follows it, as both did before they
        /// could be told apart. See [`Params::volume_alphas`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        up_alpha: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        down_alpha: Option<f64>,
    },
    Cvd {
        /// The period the running delta starts again from nought at, or
        /// `None` to run on from the first bar for ever. Missing, a session.
        #[serde(default = "cvd_session")]
        reset: Option<Reset>,
        /// Candles or a line. Missing, candles.
        #[serde(default)]
        style: cvd::Style,
        #[serde(default = "cvd_height")]
        height: f64,
        /// The colouring its rising and falling bars are painted in, as a
        /// delta profile's bought and sold rows are: unset, the colouring
        /// the chart's bars wear, and following it as it changes; set, one
        /// of [`profile::DELTA_COLOURINGS`] of its own, or Custom: see
        /// [`crate::colouring`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        colouring: Option<Colouring>,
        /// Its rising and falling bars' own colours, while Custom.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        up_color: Option<ColorChoice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        down_color: Option<ColorChoice>,
    },
    /// Each bar's own delta in a strip of its own, as a candle from nought.
    VolumeDelta {
        #[serde(default = "flow::volume_delta_height")]
        height: f64,
        #[serde(default)]
        inks: flow::Inks,
    },
    /// Each bar's delta at each price beside a narrow bar, once a bar is
    /// wide enough to show them. A `zoom` stored by an earlier version, the
    /// width it was drawn from, is read and let go: the width is worked out
    /// from what a column has to hold.
    Footprint {
        /// Whether a row is filled more strongly the bigger its delta, or
        /// every row alike.
        #[serde(default = "flow::yes")]
        intensity: bool,
        /// Whether each row's delta is written in it, or the rows are drawn
        /// alone.
        #[serde(default = "flow::yes")]
        numbers: bool,
        /// How many ticks a row holds, or `None` for a compact line's worth
        /// at the chart's price scale, as a market profile's row height.
        #[serde(default)]
        row_ticks: Option<u32>,
        #[serde(default)]
        inks: flow::Inks,
    },
    /// A bubble at each order bigger than the threshold of the hours it
    /// traded in.
    LargeOrders {
        #[serde(default = "flow::regular_threshold")]
        regular: u64,
        #[serde(default = "flow::extended_threshold")]
        extended: u64,
        #[serde(default)]
        compare: flow::Compare,
        /// Whether prints of one moment and one side are one order.
        #[serde(default = "flow::yes")]
        combine: bool,
        #[serde(default)]
        inks: flow::Inks,
    },
    Rsi {
        period: usize,
        height: f64,
        overbought: f64,
        oversold: f64,
    },
    Atr {
        period: usize,
        height: f64,
    },
    Macd {
        /// Bars the quick average covers.
        fast: usize,
        /// Bars the slow one covers.
        slow: usize,
        /// Bars the line itself is averaged over to give the signal.
        signal: usize,
        height: f64,
        /// The signal line. Unset is the theme's companion to the MACD line's
        /// colour, so the two are told apart without anybody having to choose,
        /// as a stochastic's %D is.
        #[serde(default)]
        signal_color: Option<ColorChoice>,
        /// The signal line's pattern. Unset is the MACD line's own, which is
        /// what it was drawn in before it could be set apart.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal_style: Option<LineStyle>,
        /// How solid the signal line is drawn. Unset is the MACD line's
        /// own: see [`Indicator::second_alpha`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signal_alpha: Option<f64>,
    },
    Stochastic {
        /// Bars whose range the close is placed in: %K's length.
        period: usize,
        /// Bars %K is averaged over.
        k_smooth: usize,
        /// Bars %K is averaged over again to give %D.
        d_period: usize,
        height: f64,
        overbought: f64,
        oversold: f64,
        /// The %D line. Unset is the theme's companion to %K's colour, so the
        /// two lines are told apart without anybody having to choose.
        #[serde(default)]
        d_color: Option<ColorChoice>,
        /// The %D line's pattern. Unset is %K's own, as it was drawn before
        /// it could be set apart.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        d_style: Option<LineStyle>,
        /// How solid the %D line is drawn. Unset is %K's own: see
        /// [`Indicator::second_alpha`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        d_alpha: Option<f64>,
    },
    SessionTimes {
        /// The stretches of each day it shades, each in its own colour,
        /// drawn in this order.
        ranges: Vec<TimeRange>,
    },
}

/// The roles of a study painted by rise and fall, and of one painted by
/// buying and selling: what each colour row of a Custom one is titled.
pub const VOLUME_ROLES: [&str; 2] = ["Up", "Down"];
pub const FLOW_ROLES: [&str; 2] = ["Buying", "Selling"];

impl Params {
    /// How solid a volume pane's rising and falling bars are drawn: each
    /// its own strength, or the pane's opacity. `None` for any other kind.
    pub fn volume_alphas(&self) -> Option<(f64, f64)> {
        let Params::Volume { opacity, up_alpha, down_alpha, .. } = self else { return None };
        let both = strength_or(Some(*opacity), DEFAULT_VOLUME_OPACITY);
        Some((strength_or(*up_alpha, both), strength_or(*down_alpha, both)))
    }

    /// The envelope this indicator carries, if it carries one.
    ///
    /// A VWAP and a Bollinger both do, and they are the same bands drawn by
    /// the same code — so the editor and the command line ask this rather than
    /// each knowing which kinds have bands, which is the sort of list that is
    /// wrong the next time one is added.
    pub fn bands(&self) -> Option<&[Band]> {
        match self {
            Params::Vwap { bands, .. } | Params::Bollinger { bands, .. } => Some(bands),
            _ => None,
        }
    }

    /// The same, to write to.
    pub fn bands_mut(&mut self) -> Option<&mut Vec<Band>> {
        match self {
            Params::Vwap { bands, .. } | Params::Bollinger { bands, .. } => Some(bands),
            _ => None,
        }
    }

    /// The share of the chart's height a strip of its own takes, for an
    /// indicator drawn in one. Asked here rather than by a list of kinds at
    /// each place that sets it — a drag on the strip's edge, the settings'
    /// slider — which is how a strip added after the list was written, the
    /// CVD's, came to be drawn at the height it was dragged to and stored
    /// at the one it had. Every kind is named, so a new one has to say.
    pub fn height_mut(&mut self) -> Option<&mut f64> {
        match self {
            Params::Volume { height, .. }
            | Params::Cvd { height, .. }
            | Params::VolumeDelta { height, .. }
            | Params::Rsi { height, .. }
            | Params::Atr { height, .. }
            | Params::Macd { height, .. }
            | Params::Stochastic { height, .. } => Some(height),
            Params::MovingAverage { .. }
            | Params::Vwap { .. }
            | Params::Bollinger { .. }
            | Params::VolumeProfile { .. }
            | Params::MarketProfile { .. }
            | Params::Footprint { .. }
            | Params::LargeOrders { .. }
            | Params::SessionTimes { .. } => None,
        }
    }

    /// The colours of a study painted in a colouring, to be read and
    /// changed the one way every study's are: see [`crate::colouring`].
    /// Every kind is named, so a new one has to say whether it has them.
    pub fn colours_mut(&mut self) -> Option<ColoursMut<'_>> {
        match self {
            Params::Volume { colouring, up_color, down_color, opacity, up_alpha, down_alpha, .. } => Some(ColoursMut {
                colouring,
                labels: &VOLUME_ROLES,
                colours: vec![up_color, down_color],
                alphas: vec![up_alpha, down_alpha],
                opacity: Some(opacity),
            }),
            Params::Cvd { colouring, up_color, down_color, .. } => Some(ColoursMut {
                colouring,
                labels: &VOLUME_ROLES,
                colours: vec![up_color, down_color],
                alphas: Vec::new(),
                opacity: None,
            }),
            // A delta's rows; how solid they are is the profile's own rows'.
            Params::VolumeProfile { colouring, buy_color, sell_color, .. } => Some(ColoursMut {
                colouring,
                labels: &FLOW_ROLES,
                colours: vec![buy_color, sell_color],
                alphas: Vec::new(),
                opacity: None,
            }),
            Params::VolumeDelta { inks, .. } | Params::Footprint { inks, .. } | Params::LargeOrders { inks, .. } => {
                Some(inks.colours_mut())
            }
            Params::MovingAverage { .. }
            | Params::Vwap { .. }
            | Params::Bollinger { .. }
            | Params::MarketProfile { .. }
            | Params::Rsi { .. }
            | Params::Atr { .. }
            | Params::Macd { .. }
            | Params::Stochastic { .. }
            | Params::SessionTimes { .. } => None,
        }
    }

    /// The colouring a study is painted in, and the colours it picked for
    /// its roles, for drawing: `None` for a study without one.
    pub fn colours(&self) -> Option<(Option<Colouring>, [Option<&ColorChoice>; 2])> {
        match self {
            Params::Volume { colouring, up_color, down_color, .. } | Params::Cvd { colouring, up_color, down_color, .. } => {
                Some((*colouring, [up_color.as_ref(), down_color.as_ref()]))
            }
            Params::VolumeProfile { colouring, buy_color, sell_color, .. } => {
                Some((*colouring, [buy_color.as_ref(), sell_color.as_ref()]))
            }
            params => params.inks().map(|inks| (inks.colouring, [inks.buy.as_ref(), inks.sell.as_ref()])),
        }
    }

    /// How an order flow indicator paints buying and selling.
    pub fn inks(&self) -> Option<&flow::Inks> {
        match self {
            Params::VolumeDelta { inks, .. } | Params::Footprint { inks, .. } | Params::LargeOrders { inks, .. } => {
                Some(inks)
            }
            _ => None,
        }
    }

    pub fn inks_mut(&mut self) -> Option<&mut flow::Inks> {
        match self {
            Params::VolumeDelta { inks, .. } | Params::Footprint { inks, .. } | Params::LargeOrders { inks, .. } => {
                Some(inks)
            }
            _ => None,
        }
    }

    /// A volume or market profile's reset and the edge it is set to grow
    /// from; nothing for any other kind.
    fn profile_edge(&self) -> Option<(Reset, profile::Align)> {
        match self {
            Params::VolumeProfile { reset, align, .. } | Params::MarketProfile { reset, align, .. } => {
                Some((*reset, *align))
            }
            Params::MovingAverage { .. }
            | Params::Vwap { .. }
            | Params::Bollinger { .. }
            | Params::Volume { .. }
            | Params::Cvd { .. }
            | Params::VolumeDelta { .. }
            | Params::Footprint { .. }
            | Params::LargeOrders { .. }
            | Params::Rsi { .. }
            | Params::Atr { .. }
            | Params::Macd { .. }
            | Params::Stochastic { .. }
            | Params::SessionTimes { .. } => None,
        }
    }
}

/// The market profile a volume or market profile is paired with on a chart
/// of `shown` indicators, in legend order: the first market profile among
/// them that resets on the same period, on `timeframe` when it is known.
/// Nothing when there is none, and for any other kind.
///
/// Paired, a session's profiles are drawn inside that session, each
/// growing from the edge it is set to: side by side when they grow the same
/// way, back to back when they grow apart. See [`pair`].
pub fn paired_with<'a>(
    me: &Params,
    shown: impl IntoIterator<Item = &'a Indicator>,
    timeframe: Option<Timeframe>,
) -> Option<&'a Indicator> {
    let effective = |reset: Reset| timeframe.map_or(reset, |timeframe| reset.effective_for(timeframe));
    let (reset, _) = me.profile_edge()?;
    shown.into_iter().find(|other| {
        matches!(other.params, Params::MarketProfile { .. })
            && other.params.profile_edge().is_some_and(|(theirs, _)| effective(theirs) == effective(reset))
    })
}

/// The profiles of `shown` drawn as one pair with `me`, itself among them,
/// in legend order: the market profile it is paired with and every volume
/// profile paired with that one. Empty when `me` is in no pair: a profile
/// alone on its period, two volume profiles without a market profile, or a
/// second market profile of a period, which is drawn on its own.
pub fn pair<'a>(me: &Indicator, shown: &[&'a Indicator], timeframe: Option<Timeframe>) -> Vec<&'a Indicator> {
    let leader = |of: &Params| paired_with(of, shown.iter().copied(), timeframe).map(|leader| leader.id);
    let Some(lead) = leader(&me.params) else { return Vec::new() };
    let together: Vec<&Indicator> = shown
        .iter()
        .copied()
        .filter(|other| {
            other.id == lead
                || (matches!(other.params, Params::VolumeProfile { .. }) && leader(&other.params) == Some(lead))
        })
        .collect();
    match together.len() > 1 && together.iter().any(|other| other.id == me.id) {
        true => together,
        false => Vec::new(),
    }
}

/// The volume and market profiles of `all` that merge and split together
/// with `me`, itself among them: every one resetting on the same period as
/// it, once a market profile is among those, since a session's market
/// profile and volume profile drawn side by side have to cover the same
/// span. Without a market profile, or for a profile whose period no other
/// shares, it alone. Nothing for any other kind.
///
/// Hidden ones too: a market profile shown again has to cover what its
/// volume profile was merged into while it was hidden.
pub fn merged_together(me: &Indicator, all: &[Indicator], timeframe: Option<Timeframe>) -> Vec<u32> {
    let effective = |reset: Reset| timeframe.map_or(reset, |timeframe| reset.effective_for(timeframe));
    let Some((reset, _)) = me.params.profile_edge() else { return Vec::new() };
    let together: Vec<&Indicator> = all
        .iter()
        .filter(|other| {
            other.id == me.id
                || other.params.profile_edge().is_some_and(|(theirs, _)| effective(theirs) == effective(reset))
        })
        .collect();
    match together.iter().any(|other| matches!(other.params, Params::MarketProfile { .. })) {
        true => together.iter().map(|other| other.id).collect(),
        false => vec![me.id],
    }
}

/// One indicator on a chart.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Indicator {
    /// Stable for the life of the chart, so the summary row and the drawing
    /// agree on which one you clicked.
    pub id: u32,
    pub kind: Kind,
    pub params: Params,
    /// Unset means "whatever the palette offers for my slot".
    pub color: Option<ColorChoice>,
    /// How solid `color` is drawn on a line or a strip's line, nought to
    /// one. Unset, solid, as every line was drawn before it could be set.
    /// A volume or market profile's rows have strengths of their own in
    /// their params, and volume bars and session shading draw no line, so
    /// those pay this no mind. See [`Indicator::line_alpha`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha: Option<f64>,
    #[serde(default)]
    pub stroke: Stroke,
    pub visible: bool,
    /// Whether this indicator's strip sits above the price plot rather than
    /// below it. Nothing for an indicator that draws over the price.
    ///
    /// Which side of the price a strip falls on is this flag; the order of the
    /// strips on one side is the order they are listed in. Together those say
    /// where every strip sits, and no combination of them is undrawable — which
    /// an index into the list saying "the price sits here" would not manage,
    /// because removing an indicator above the price would silently promote the
    /// one below it.
    ///
    /// Nobody chooses this today: every strip sits below the price, and a
    /// stored `true` is read back as `false` (see [`below_the_price`]). The
    /// drawing and [`move_pane`] still honour it, so a setting for it plugs
    /// in by dropping that override and offering [`cross_price`] again.
    #[serde(default, deserialize_with = "below_the_price")]
    pub above_price: bool,
    /// Whether the chart's legend names it. Drawn or not is `visible`; this
    /// is only whether it takes a line at the top of the chart.
    #[serde(default = "listed")]
    pub in_legend: bool,
}

/// Every strip is read back below the price, whatever was stored: a chart
/// left with a strip above it once had a way to move it back, and now has
/// none. The stored value is still read, so an older file stays readable.
fn below_the_price<'de, D: serde::Deserializer<'de>>(stored: D) -> Result<bool, D::Error> {
    let _ = <bool as Deserialize>::deserialize(stored)?;
    Ok(false)
}

/// An indicator stored before the legend could leave one out was in it.
fn listed() -> bool {
    true
}

/// The thicknesses a study's line is picked from: four, and none, which is
/// how a VWAP band is a shade with no outline.
pub const STROKE_WIDTHS: [f64; 5] = [0.0, 1.0, 1.5, 2.5, 4.0];

/// How thin and how thick a volume profile's outline and point of control
/// may be set, and the step between: a profile drawing's own, half a pixel
/// to twelve by halves, so the study and the drawing offer the same.
pub const MIN_OUTLINE_WIDTH: f64 = crate::drawings::MIN_WIDTH;
pub const MAX_OUTLINE_WIDTH: f64 = crate::drawings::MAX_WIDTH;
pub const OUTLINE_WIDTH_STEP: f64 = crate::drawings::WIDTH_STEP;

impl Indicator {
    /// One step thicker or thinner, as Alt+Shift with + and − asks: the
    /// thickness its settings call its line's, along the choices they
    /// offer, or a volume profile's outline by its own step. Never down to
    /// no line, which would take the selected line out from under the hand.
    /// `false` for what has no line to change, the volume bars and session
    /// shading.
    pub fn step_width(&mut self, thicker: bool) -> bool {
        match &mut self.params {
            Params::Volume { .. } | Params::MarketProfile { .. } | Params::SessionTimes { .. } => false,
            Params::VolumeProfile { outline_width, .. } => {
                let by = if thicker { OUTLINE_WIDTH_STEP } else { -OUTLINE_WIDTH_STEP };
                *outline_width = (*outline_width + by).clamp(MIN_OUTLINE_WIDTH, MAX_OUTLINE_WIDTH);
                true
            }
            _ => {
                let now = self.stroke.width;
                let mut drawn = STROKE_WIDTHS.iter().copied().filter(|width| *width > 0.0);
                let next = match thicker {
                    true => drawn.find(|width| *width > now + 1e-9).unwrap_or(now.max(STROKE_WIDTHS[4])),
                    false => drawn.rev().find(|width| *width < now - 1e-9).unwrap_or(STROKE_WIDTHS[1]),
                };
                self.stroke.width = next;
                true
            }
        }
    }

    /// How solid its line is drawn: its own strength, or solid.
    pub fn line_alpha(&self) -> f64 {
        strength_or(self.alpha, 1.0)
    }

    /// How solid a strip's second line is drawn, a MACD's signal or a
    /// stochastic's %D: its own strength, or the first line's.
    pub fn second_alpha(&self) -> f64 {
        let own = match &self.params {
            Params::Macd { signal_alpha, .. } => *signal_alpha,
            Params::Stochastic { d_alpha, .. } => *d_alpha,
            _ => None,
        };
        strength_or(own, self.line_alpha())
    }

    pub fn new(id: u32, kind: Kind) -> Indicator {
        Indicator {
            id,
            kind,
            params: kind.default_params(),
            color: None,
            alpha: None,
            stroke: Stroke::default(),
            visible: true,
            above_price: false,
            in_legend: kind.in_legend_by_default(),
        }
    }

    /// One of `kind` with `numbers` written into the lengths its label
    /// shows, in the label's order ("BB 20 2" is the period, then the
    /// deviations), and its defaults for the rest. What the picker adds for
    /// "sma 50".
    ///
    /// `None` when the label has fewer numbers than were given, or one does
    /// not fit: a length that is not a whole number of bars, one or more, a
    /// multiple past [`bollinger::MAX_DEVIATIONS`], a MACD whose fast average
    /// is not below its slow one.
    pub fn numbered(id: u32, kind: Kind, numbers: &[f64]) -> Option<Indicator> {
        fn bars(n: f64) -> Option<usize> {
            (n >= 1.0 && n.fract() == 0.0 && n <= u32::MAX as f64).then_some(n as usize)
        }
        fn lengths(slots: [&mut usize; 3], numbers: &[f64]) -> Option<()> {
            for (slot, n) in slots.into_iter().zip(numbers) {
                *slot = bars(*n)?;
            }
            Some(())
        }
        let mut indicator = Indicator::new(id, kind);
        match (&mut indicator.params, numbers) {
            (_, []) => {}
            (
                Params::MovingAverage { period } | Params::Rsi { period, .. } | Params::Atr { period, .. },
                [n],
            ) => *period = bars(*n)?,
            (Params::Bollinger { period, bands }, [n, rest @ ..]) if rest.len() <= 1 => {
                *period = bars(*n)?;
                if let [deviations] = rest {
                    if !(0.0..=bollinger::MAX_DEVIATIONS).contains(deviations) {
                        return None;
                    }
                    // Adding nought makes a negative nought the positive one,
                    // which is the one the label should write.
                    bands.first_mut()?.deviations = deviations + 0.0;
                }
            }
            (Params::Macd { fast, slow, signal, .. }, _) if numbers.len() <= 3 => {
                lengths([&mut *fast, &mut *slow, signal], numbers)?;
                if *fast >= *slow {
                    return None;
                }
            }
            (Params::Stochastic { period, k_smooth, d_period, .. }, _) if numbers.len() <= 3 => {
                lengths([period, k_smooth, d_period], numbers)?
            }
            _ => return None,
        }
        Some(indicator)
    }

    /// How the summary row labels it: "SMA 50", "VWAP · Session".
    ///
    /// The stored settings, which is what the settings page should show.
    pub fn label(&self) -> String {
        self.describe(None)
    }

    /// What it is actually doing on this chart.
    ///
    /// A reset period too fine for the timeframe is promoted before computing,
    /// so a session VWAP on a weekly chart is really a quarterly one. The
    /// legend has to say the promoted period or it is describing a chart that
    /// is not on screen.
    pub fn label_for(&self, timeframe: Timeframe) -> String {
        self.describe(Some(timeframe))
    }

    /// What the legend says of it on this chart: a cumulative delta any
    /// part of which was `estimated` from bar direction says so with a ≈
    /// after its name, "CVD ≈ · Session". See [`Output::is_estimated`].
    pub fn legend_for(&self, timeframe: Timeframe, estimated: bool) -> String {
        let label = self.label_for(timeframe);
        match (estimated, self.kind) {
            (true, Kind::Cvd) => label.replacen("CVD", "CVD ≈", 1),
            (true, Kind::VolumeDelta) => label.replacen("VD", "VD ≈", 1),
            _ => label,
        }
    }

    fn describe(&self, timeframe: Option<Timeframe>) -> String {
        let effective = |reset: Reset| match timeframe {
            Some(timeframe) => reset.effective_for(timeframe),
            None => reset,
        };
        match &self.params {
            Params::MovingAverage { period } => format!("{} {period}", self.kind.short_name()),
            Params::Vwap { reset, .. } => format!("VWAP · {}", effective(*reset).label()),
            // The multiple belongs in the name: two Bollingers at different
            // widths are a pair of envelopes nobody can tell apart otherwise.
            Params::Bollinger { period, bands } => {
                let deviations = bands.first().map(|band| band.deviations).unwrap_or_default();
                format!("BB {period} {deviations}")
            }
            Params::Macd { fast, slow, signal, .. } => format!("MACD {fast} {slow} {signal}"),
            // A delta is a different fact about the same prices, and two
            // profiles of one period are told apart by which they show.
            Params::VolumeProfile { reset, method, .. } => match method {
                profile::Method::Total => format!("VP · {}", effective(*reset).label()),
                profile::Method::Delta => format!("VP Δ · {}", effective(*reset).label()),
            },
            // The TPO period is part of the name: two profiles of one
            // session in different periods draw different letters.
            Params::MarketProfile { reset, minutes, .. } => {
                format!("TPO {minutes}m · {}", effective(*reset).label())
            }
            Params::Volume { .. } => "Volume".to_string(),
            Params::Cvd { reset, .. } => match reset {
                Some(reset) => format!("CVD · {}", effective(*reset).label()),
                None => "CVD".to_string(),
            },
            Params::VolumeDelta { .. } => "VD".to_string(),
            Params::Footprint { .. } => "FP".to_string(),
            // The thresholds are what tell two of these apart.
            Params::LargeOrders { regular, extended, .. } => match regular == extended {
                true => format!("Large > {regular}"),
                false => format!("Large > {regular} · {extended}"),
            },
            Params::Rsi { period, .. } | Params::Atr { period, .. } => {
                format!("{} {period}", self.kind.short_name())
            }
            Params::Stochastic { period, k_smooth, d_period, .. } => {
                format!("Stoch {period} {k_smooth} {d_period}")
            }
            // The ranges by name, which is what tells two of these apart.
            Params::SessionTimes { ranges } => match ranges.len() {
                0 => "Sessions".to_string(),
                _ => {
                    let names: Vec<&str> = ranges.iter().map(|r| r.name.trim()).collect();
                    format!("Sessions · {}", names.join(", "))
                }
            },
        }
    }

    /// The line colour, resolved against the theme.
    ///
    /// `slot` is this indicator's position among those on the chart, which is
    /// what makes a fresh set of indicators come out in sequence rather than
    /// all wearing the accent.
    pub fn color(&self, theme: &Theme, slot: usize) -> String {
        match &self.color {
            Some(choice) => choice.resolve(theme),
            None => theme.series(slot),
        }
    }
}

/// The colour each of these indicators draws in.
///
/// Returned for the whole set at once, because avoiding a collision is a
/// property of the set and not of any one member: a second moving average has
/// to know what the first one took. Walking them one at a time is how you end
/// up with two amber lines and no way to tell which is the fifty.
///
/// Three rules:
///
/// * **A colour somebody chose is never moved.** Those are taken first, so an
///   automatic one gives way to a pinned one rather than the other way round.
/// * **Automatic ones take the first sequence colour still free.** Past six
///   they repeat, because the palette has six and a chart with seven overlays
///   has worse problems than a repeated hue.
/// * **Oldest first, by id.** So adding an indicator cannot repaint the ones
///   already on the chart, and neither can reordering them.
pub fn palette_colors(indicators: &[Indicator], theme: &Theme) -> Vec<String> {
    let mut by_age: Vec<usize> = (0..indicators.len()).collect();
    by_age.sort_by_key(|&i| indicators[i].id);

    let mut taken: Vec<String> = indicators
        .iter()
        .filter_map(|indicator| indicator.color.as_ref())
        .map(|choice| choice.resolve(theme))
        .collect();

    let mut out = vec![String::new(); indicators.len()];
    for i in by_age {
        let indicator = &indicators[i];
        if let Some(choice) = &indicator.color {
            out[i] = choice.resolve(theme);
            continue;
        }
        if !indicator.kind.takes_palette_colour() {
            out[i] = theme.ui.text_muted.clone();
            continue;
        }
        let free = (0..SWATCH_SEQUENCE.len())
            .map(|n| theme.series(n))
            .find(|hex| !taken.contains(hex));
        let colour = free.unwrap_or_else(|| theme.series(taken.len()));
        taken.push(colour.clone());
        out[i] = colour;
    }
    out
}

/// What computing an indicator produces.
#[derive(Clone, PartialEq, Debug)]
pub enum Output {
    /// One value per bar. `None` where there is not enough history yet —
    /// never a fabricated number.
    Line(Vec<Option<f64>>),
    Bands(Bands),
    /// One profile per reset period.
    Profiles(Vec<Profile>),
    /// One market profile per reset period.
    MarketProfiles(Vec<std::sync::Arc<MarketProfile>>),
    /// Volume per bar, with the share of the chart its pane takes.
    Volume { values: Vec<f64>, height: f64 },
    /// A series drawn in a strip of its own, on its own scale.
    Pane(Pane),
    /// Stretches of time shaded behind the bars.
    Time(Time),
    /// A footprint in place of the candles, from the bar width its settings
    /// say, read from the chart's ladders as it is drawn.
    Footprint,
    /// A bubble at each large order, from the chart's prints.
    LargeOrders,
    /// Nothing to draw, and the one line that says why: what it needs, this
    /// chart's source does not have. See [`unmet`].
    Unmet(&'static str),
}

/// An indicator that cannot share the price scale, and so gets a strip of its
/// own above or below the chart.
#[derive(Clone, PartialEq, Debug)]
pub struct Pane {
    pub values: Vec<Option<f64>>,
    /// A second line over the first, on the same scale: a stochastic's %D, a
    /// MACD's signal.
    pub signal: Option<Vec<Option<f64>>>,
    /// Bars from zero rather than a line, on the same scale again: a MACD's
    /// histogram. Drawn under both lines, as the volume strip's bars are.
    pub histogram: Option<Vec<Option<f64>>>,
    /// Share of the chart's height this strip takes.
    pub height: f64,
    /// The scale it always uses, or `None` to fit whatever is on screen.
    ///
    /// An RSI is always 0 to 100 — half its meaning is where the line sits
    /// between them — while an ATR is a price distance with no natural
    /// ceiling, and fitting it is the only way to see its shape.
    pub bounds: Option<(f64, f64)>,
    /// Horizontal reference lines.
    pub guides: Vec<f64>,
    /// The pair of guides worth shading between, if any.
    pub band: Option<(f64, f64)>,
    /// A bar of a cumulative delta for each bar, drawn as candles or a
    /// line in place of `values`, which hold their closes.
    pub delta: Option<Vec<cvd::DeltaBar>>,
}

/// The least of the chart's height a pane may be dragged down to. Below this
/// it is a line rather than a strip, and there is nothing to read in it.
pub const MIN_PANE_SHARE: f64 = 0.05;
/// The most it may be given. Not a judgement about what looks right — the
/// chart stays usable well past the point it looks odd, and somebody stacking
/// a tall volume pane under a sliver of price is entitled to.
pub const MAX_PANE_SHARE: f64 = 0.95;

/// One row of a chart's vertical stack, top to bottom.
///
/// The price plot is a row like any other rather than a fixed first one,
/// because a strip can be moved above it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Row {
    Price,
    /// The strip belonging to the indicator with this id.
    Strip(u32),
}

/// The rows a chart stacks, top to bottom.
///
/// Only an indicator that is drawn and that needs a strip of its own is a row.
/// An overlay is drawn on the price plot and a hidden indicator is drawn
/// nowhere, so neither is something a strip can be moved past.
pub fn stack(indicators: &[Indicator]) -> Vec<Row> {
    let strips = |side: bool| {
        indicators
            .iter()
            .filter(move |i| i.visible && i.kind.in_own_pane() && i.above_price == side)
            .map(|i| Row::Strip(i.id))
    };
    strips(true).chain(std::iter::once(Row::Price)).chain(strips(false)).collect()
}

/// Which way a strip travels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Move {
    Up,
    Down,
}

/// Move one strip one row along the stack, past the price plot when the price
/// plot is what is next to it.
///
/// `false`, and nothing touched, when the strip is already at that end of the
/// stack — or is not a strip at all.
///
/// The strips are permuted within the slots they already occupy in the list,
/// so reordering them never moves an overlay, and so cannot change which line
/// is drawn over which on the price plot.
pub fn move_pane(indicators: &mut [Indicator], id: u32, direction: Move) -> bool {
    let rows = stack(indicators);
    let Some(at) = rows.iter().position(|row| *row == Row::Strip(id)) else { return false };
    let neighbour = match direction {
        Move::Up => at.checked_sub(1),
        Move::Down => (at + 1 < rows.len()).then_some(at + 1),
    };
    let Some(neighbour) = neighbour else { return false };

    match rows[neighbour] {
        Row::Price => cross_price(indicators, id),
        // Two strips on the same side of the price simply trade places.
        Row::Strip(other) => {
            let (mut above, mut below) = (sided(indicators, true), sided(indicators, false));
            let group = if above.contains(&id) { &mut above } else { &mut below };
            let (Some(from), Some(to)) = (
                group.iter().position(|x| *x == id),
                group.iter().position(|x| *x == other),
            ) else {
                return false;
            };
            group.swap(from, to);
            reslot(indicators, &above, &below);
            true
        }
    }
}

/// Move one strip one row along the stack on its own side of the price.
///
/// What the app offers: a strip never crosses the price, so at the price's
/// edge, as at either end of the stack, this is `false` and nothing moves.
/// [`move_pane`] is the same move without that limit, for when where a strip
/// sits becomes a setting.
pub fn move_strip(indicators: &mut [Indicator], id: u32, direction: Move) -> bool {
    let rows = stack(indicators);
    let Some(at) = rows.iter().position(|row| *row == Row::Strip(id)) else { return false };
    let neighbour = match direction {
        Move::Up => at.checked_sub(1),
        Move::Down => (at + 1 < rows.len()).then_some(at + 1),
    };
    match neighbour.map(|n| rows[n]) {
        Some(Row::Strip(_)) => move_pane(indicators, id, direction),
        _ => false,
    }
}

/// Put a strip on the other side of the price plot, against the price itself.
///
/// It lands at the edge it arrived from rather than at the end of its new side,
/// so crossing the price is one step and not a jump over whatever is already
/// over there. `false` when there is no such strip.
pub fn cross_price(indicators: &mut [Indicator], id: u32) -> bool {
    let Some(was_above) =
        indicators.iter().find(|i| i.id == id && i.kind.in_own_pane()).map(|i| i.above_price)
    else {
        return false;
    };
    let (mut above, mut below) = (sided(indicators, true), sided(indicators, false));
    if was_above {
        above.retain(|other| *other != id);
        below.insert(0, id);
    } else {
        below.retain(|other| *other != id);
        above.push(id);
    }
    reslot(indicators, &above, &below);
    true
}

/// The ids of the strips on one side of the price, in the order they are
/// listed.
fn sided(indicators: &[Indicator], above: bool) -> Vec<u32> {
    indicators
        .iter()
        .filter(|i| i.kind.in_own_pane() && i.above_price == above)
        .map(|i| i.id)
        .collect()
}

/// Put the strips back into the slots the strips already hold, in this order,
/// each flagged by the side of the price it ended up on.
fn reslot(indicators: &mut [Indicator], above: &[u32], below: &[u32]) {
    let slots: Vec<usize> =
        (0..indicators.len()).filter(|at| indicators[*at].kind.in_own_pane()).collect();
    let ordered: Vec<Indicator> = above
        .iter()
        .chain(below)
        .filter_map(|id| indicators.iter().find(|i| i.id == *id).cloned())
        .collect();
    for (slot, mut indicator) in slots.into_iter().zip(ordered) {
        indicator.above_price = above.contains(&indicator.id);
        indicators[slot] = indicator;
    }
}

impl Output {
    /// The periods a volume or market profile draws, in order, as profiles:
    /// what merging and splitting them, and reading them, go by. Nothing
    /// for any other output.
    pub fn periods(&self) -> Option<Vec<&Profile>> {
        match self {
            Output::Profiles(profiles) => Some(profiles.iter().collect()),
            Output::MarketProfiles(profiles) => Some(profiles.iter().map(|market| &market.profile).collect()),
            _ => None,
        }
    }

    /// Whether any part of it was estimated rather than read: a cumulative
    /// delta's bars taken from their direction.
    pub fn is_estimated(&self) -> bool {
        match self {
            Output::Pane(Pane { delta: Some(bars), .. }) => bars.iter().any(|bar| bar.estimated),
            _ => false,
        }
    }

    /// The share of the chart's height this indicator wants for its own strip,
    /// if it needs one at all.
    pub fn pane_height(&self) -> Option<f64> {
        match self {
            Output::Volume { height, .. } => Some(*height),
            Output::Pane(pane) => Some(pane.height),
            _ => None,
        }
    }

    /// Resize the strip, for dragging its edge. The computed series is left
    /// alone — only the box it is drawn in changes, so there is nothing to
    /// recompute while the hand is moving.
    pub fn set_pane_height(&mut self, share: f64) {
        let share = share.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE);
        match self {
            Output::Volume { height, .. } => *height = share,
            Output::Pane(pane) => pane.height = share,
            _ => {}
        }
    }
}

/// What an indicator is computed against, besides its bars.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    /// The instrument's session open, which is what makes a session-reset
    /// VWAP on futures start in the evening rather than at midnight.
    pub session_origin: i64,
    /// Lets a reset period that is too fine for the chart be promoted rather
    /// than drawn as nonsense.
    pub timeframe: Timeframe,
    /// What sort of thing is charted, which a volume profile sizes its rows by.
    pub kind: Option<crate::symbols::InstrumentKind>,
    /// What is charted, which is where its exchange and its day come from.
    pub instrument: Option<&'a Instrument>,
    /// The clock the chart reads, which a range of hours typed in it keeps.
    pub zone: Tz,
    /// What traded at each price, when the chart's source has it and one of
    /// its indicators wants it. `None` on every chart of a source without
    /// ladders, which is then computed exactly as it always was.
    pub ladders: Option<&'a Held>,
    /// The chart's source has ladders of it and they are being read, so
    /// `ladders` is `None` only for now. A volume profile draws nothing
    /// until they come, rather than an estimate from the bars that would be
    /// replaced, rows and shape, a moment later.
    pub awaiting_ladders: bool,
    /// What a cumulative delta and a market profile worked out on this
    /// chart last time, so a tick works out only the newest bars, or the
    /// forming period, again. `None` works out every bar every time.
    pub memos: Option<&'a Memos>,
    /// Trading hours the embedding program keeps itself, in place of the
    /// instrument's exchange's from the registry, for the sessions Session
    /// Times shades. `None` uses the registry. Nothing else reads it: a
    /// session VWAP, a cumulative delta and a volume profile reset on
    /// [`Context::session_origin`], and trading days are the registry's.
    pub schedule: Option<crate::exchanges::Schedule<'a>>,
}

impl<'a> Context<'a> {
    /// Everything there is to know about a chart of `instrument`.
    pub fn of(instrument: &'a Instrument, timeframe: Timeframe, zone: Tz) -> Context<'a> {
        Context {
            session_origin: instrument.session_origin,
            timeframe,
            kind: Some(instrument.kind),
            instrument: Some(instrument),
            zone,
            ladders: None,
            awaiting_ladders: false,
            memos: None,
            schedule: None,
        }
    }

    /// The same chart, the sessions Session Times shades the embedding
    /// program's own rather than the registry's: see [`Context::schedule`].
    pub fn with_schedule(self, schedule: Option<crate::exchanges::Schedule<'a>>) -> Context<'a> {
        Context { schedule, ..self }
    }

    /// The same chart, with its ladders.
    pub fn with_ladders(self, ladders: Option<&'a Held>) -> Context<'a> {
        Context { ladders, ..self }
    }

    /// The same chart, its ladders on their way: see
    /// [`Context::awaiting_ladders`].
    pub fn awaiting_ladders(self, awaiting: bool) -> Context<'a> {
        Context { awaiting_ladders: awaiting, ..self }
    }

    /// The same chart, keeping what it worked out from one computation to
    /// the next.
    pub fn with_memos(self, memos: Option<&'a Memos>) -> Context<'a> {
        Context { memos, ..self }
    }
}

/// What a chart's indicators worked out last time, kept for the next
/// computation on it, by the indicator's id: see [`Context::memos`].
#[derive(Debug, Default)]
pub struct Memos {
    pub cvd: cvd::Memos,
    pub market: market::Memos,
    pub profile: profile::Memos,
}

impl Memos {
    pub fn new() -> Memos {
        Memos::default()
    }

    /// Forget every memo but those of `ids`: an indicator taken off.
    pub fn keep(&self, ids: &[u32]) {
        self.cvd.keep(ids);
        self.market.keep(ids);
        self.profile.keep(ids);
    }
}

/// The ladder of a run of bars on a chart of `context`, from its held
/// ladders: by minutes for intraday bars, by whole trading days for daily
/// and longer ones, whose stamps are their trading dates.
fn ladder_of(held: &Held, days: &Days, timeframe: Timeframe, bars: &[Bar]) -> Option<Ladder> {
    let (first, last) = (bars.first()?, bars.last()?);
    if timeframe.is_intraday() {
        held.between(first.ts, last.ts + timeframe.seconds())
    } else {
        let end = timeframe.shift(last.ts, 1);
        held.days_between(first.ts.div_euclid(86_400), end.div_euclid(86_400), days)
    }
}

/// Why a volume profile on a chart of `context` draws nothing, before any
/// bar is read.
enum Withheld {
    /// It cannot be drawn on this chart at all.
    Unmet(&'static str),
    /// Its ladders are on their way.
    Awaiting,
}

/// Whether a volume profile measuring by `method` is drawn nothing on a
/// chart of `context`, the study and a drawing alike.
fn profile_withheld(method: profile::Method, context: &Context) -> Option<Withheld> {
    // Which side each trade took is in no bar: a delta is drawn from
    // volume at price or not at all.
    if method == profile::Method::Delta && context.ladders.is_none() {
        return Some(Withheld::Unmet(NEEDS_VOLUME_AT_PRICE));
    }
    // Exact in a moment: nothing until then, never the estimate.
    (context.ladders.is_none() && context.awaiting_ladders).then_some(Withheld::Awaiting)
}

/// What a volume profile on a chart of `context` reads what traded at each
/// price from: the ladder of any run of its bars, and whether one it does
/// not hold yet is on its way. `None` on a chart without ladders, which is
/// then drawn from its bars. One source for the study and a drawing alike.
fn with_at_price<T>(context: &Context, read: impl FnOnce(Option<&profile::AtPrice>) -> T) -> T {
    let timeframe = context.timeframe;
    // Days are cut by the exchange's calendar, which needs the instrument; a
    // chart without one has no ladders either.
    let days = context.ladders.and(context.instrument).map(Days::new);
    let of = |run: &[Bar]| match (context.ladders, &days) {
        (Some(held), Some(days)) => ladder_of(held, days, timeframe, run),
        _ => None,
    };
    // A day's piece still being read; whole days need none.
    let coming = |run: &[Bar]| match (context.ladders, run.first(), run.last()) {
        (Some(held), Some(first), Some(last)) if timeframe.is_intraday() => {
            held.coming(first.ts, last.ts + timeframe.seconds())
        }
        _ => false,
    };
    let at_price = context
        .ladders
        .filter(|_| days.is_some())
        .map(|held| profile::AtPrice { tick_size: held.tick_size, of: &of, coming: &coming });
    read(at_price.as_ref())
}

/// The volume profile of every bar in `bars`, which start at `offset` on the
/// chart, worked out as the study works out each of its periods: what a
/// volume profile drawing shows of the span it covers, on a chart of
/// `context`.
///
/// `Err` with the reason when it cannot be drawn on this chart at all — a
/// delta where nothing says which side each trade took — which is what the
/// study says in that case; `Ok(None)` when there is nothing to draw now,
/// its ladders being on their way among other things.
pub fn span_profile(
    bars: &[Bar],
    offset: usize,
    rows: Option<usize>,
    value_area: f64,
    method: profile::Method,
    context: &Context,
) -> Result<Option<Profile>, &'static str> {
    match profile_withheld(method, context) {
        Some(Withheld::Unmet(why)) => return Err(why),
        Some(Withheld::Awaiting) => return Ok(None),
        None => {}
    }
    Ok(with_at_price(context, |at_price| {
        profile::of_span_with(bars, offset, rows, value_area, context.kind, at_price, method)
    }))
}

/// Compute an indicator over `bars`, knowing nothing of the instrument beyond
/// its kind.
///
/// `session_origin` is the instrument's session open, which is what makes a
/// session-reset VWAP on futures start in the evening rather than at midnight.
/// `timeframe` lets a reset period that is too fine for the chart be promoted
/// rather than drawn as nonsense. The indicators of time need the instrument
/// itself, for its exchange; see [`compute_in`].
pub fn compute(
    indicator: &Indicator,
    bars: &[Bar],
    session_origin: i64,
    timeframe: Timeframe,
    kind: Option<crate::symbols::InstrumentKind>,
) -> Output {
    let context = Context {
        session_origin,
        timeframe,
        kind,
        instrument: None,
        zone: crate::clock::ChartZone::default().for_instrument(None),
        ladders: None,
        awaiting_ladders: false,
        memos: None,
        schedule: None,
    };
    compute_in(indicator, bars, &context)
}

/// Compute an indicator over `bars`, on a chart of what `context` says.
pub fn compute_in(indicator: &Indicator, bars: &[Bar], context: &Context) -> Output {
    let Context { session_origin, timeframe, kind, .. } = *context;
    if let Some(unmet) = unmet(indicator.kind.needs(), context) {
        return unmet;
    }
    let schedule = || context.schedule.or_else(|| time::schedule_for(context.instrument));
    match (&indicator.kind, &indicator.params) {
        (Kind::Sma, Params::MovingAverage { period }) => Output::Line(sma(bars, *period)),
        (Kind::Ema, Params::MovingAverage { period }) => Output::Line(ema(bars, *period)),
        (Kind::Vwap, Params::Vwap { reset, bands }) => Output::Bands(vwap::compute(
            bars,
            reset.effective_for(timeframe),
            session_origin,
            bands,
        )),
        (Kind::Bollinger, Params::Bollinger { period, bands }) => {
            Output::Bands(bollinger::compute(bars, *period, bands))
        }
        (Kind::Macd, Params::Macd { fast, slow, signal, height, .. }) => {
            let (line, signal, histogram) = oscillators::macd(bars, *fast, *slow, *signal);
            Output::Pane(Pane {
                values: line,
                signal: Some(signal),
                histogram: Some(histogram),
                height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
                // Fitted, not fixed: a MACD is a price distance with no
                // natural ceiling, and it goes either side of nothing.
                bounds: None,
                // The only level a MACD has, and the one that means
                // something: the quick average crossing the slow one.
                guides: vec![0.0],
                band: None,
                delta: None,
            })
        }
        (Kind::Rsi, Params::Rsi { period, height, overbought, oversold }) => Output::Pane(Pane {
            values: oscillators::rsi(bars, *period),
            signal: None,
            histogram: None,
            height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
            bounds: Some((0.0, 100.0)),
            guides: vec![*oversold, 50.0, *overbought],
            band: Some((*oversold, *overbought)),
            delta: None,
        }),
        (Kind::Atr, Params::Atr { period, height }) => Output::Pane(Pane {
            values: oscillators::atr(bars, *period),
            signal: None,
            histogram: None,
            height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
            bounds: None,
            guides: Vec::new(),
            band: None,
            delta: None,
        }),
        (
            Kind::Stochastic,
            Params::Stochastic { period, k_smooth, d_period, height, overbought, oversold, .. },
        ) => {
            let (k, d) = oscillators::stochastic(bars, *period, *k_smooth, *d_period);
            Output::Pane(Pane {
                values: k,
                signal: Some(d),
                histogram: None,
                height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
                bounds: Some((0.0, 100.0)),
                guides: vec![*oversold, 50.0, *overbought],
                band: Some((*oversold, *overbought)),
                delta: None,
            })
        }
        (Kind::Cvd, Params::Cvd { reset, height, .. }) => {
            let reset = reset.map(|reset| reset.effective_for(timeframe));
            let days = context.ladders.and(context.instrument).map(Days::new);
            let source = match (context.ladders, &days) {
                (Some(held), Some(days)) => Some(cvd::Source { held, days }),
                _ => None,
            };
            let work = |memo: Option<&mut cvd::Memo>| {
                cvd::compute(bars, reset, session_origin, timeframe, source.as_ref(), memo)
            };
            let delta = match context.memos {
                Some(memos) => memos.cvd.with(indicator.id, |memo| work(Some(memo))),
                None => work(None),
            };
            Output::Pane(Pane {
                values: delta.iter().map(|bar| Some(bar.close)).collect(),
                signal: None,
                histogram: None,
                height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
                // Fitted: a running sum of contracts has no natural ceiling,
                // and which side of nought it is on is the point.
                bounds: None,
                guides: vec![0.0],
                band: None,
                delta: Some(delta),
            })
        }
        (Kind::VolumeDelta, Params::VolumeDelta { height, .. }) => {
            let days = context.ladders.and(context.instrument).map(Days::new);
            let source = match (context.ladders, &days) {
                (Some(held), Some(days)) => Some(cvd::Source { held, days }),
                _ => None,
            };
            let work = |memo: Option<&mut cvd::Memo>| cvd::per_bar(bars, session_origin, timeframe, source.as_ref(), memo);
            let delta = match context.memos {
                Some(memos) => memos.cvd.with(indicator.id, |memo| work(Some(memo))),
                None => work(None),
            };
            Output::Pane(Pane {
                values: delta.iter().map(|bar| Some(bar.close)).collect(),
                signal: None,
                histogram: None,
                height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
                bounds: None,
                guides: vec![0.0],
                band: None,
                delta: Some(delta),
            })
        }
        // Drawn from the chart's ladders and its prints as they stand when
        // it is drawn, which is no series to compute here.
        (Kind::Footprint, Params::Footprint { .. }) => Output::Footprint,
        (Kind::LargeOrders, Params::LargeOrders { .. }) => Output::LargeOrders,
        (Kind::Volume, Params::Volume { height, .. }) => Output::Volume {
            values: bars.iter().map(|bar| bar.volume).collect(),
            height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
        },
        (Kind::VolumeProfile, Params::VolumeProfile { reset, rows, value_area, merged, method, last_only, .. }) => {
            match profile_withheld(*method, context) {
                Some(Withheld::Unmet(why)) => return Output::Unmet(why),
                Some(Withheld::Awaiting) => return Output::Profiles(Vec::new()),
                None => {}
            }
            Output::Profiles(with_at_price(context, |at_price| match (at_price, context.memos, method) {
                // From the bars alone: what the tick before worked out is
                // kept for every period it did not move.
                (None, Some(memos), profile::Method::Total) => memos.profile.with(indicator.id, |memo| {
                    profile::compute_kept(
                        bars,
                        reset.effective_for(timeframe),
                        session_origin,
                        *rows,
                        *value_area,
                        kind,
                        merged,
                        *last_only,
                        memo,
                    )
                }),
                _ => profile::compute_with(
                    bars,
                    reset.effective_for(timeframe),
                    session_origin,
                    *rows,
                    *value_area,
                    kind,
                    merged,
                    at_price,
                    *method,
                    *last_only,
                )
            }))
        }
        (
            Kind::MarketProfile,
            Params::MarketProfile {
                reset,
                minutes,
                row_ticks,
                value_area,
                merged,
                last_only,
                ..
            },
        ) => {
            let known = context
                .instrument
                .and_then(Instrument::tick_size)
                .or(context.ladders.map(|held| held.tick_size));
            let work = |memo: &mut market::Memo| {
                market::compute_kept(
                    bars,
                    reset.effective_for(timeframe),
                    session_origin,
                    *minutes,
                    *row_ticks,
                    *value_area,
                    market::tick_size(bars, known, kind),
                    merged,
                    *last_only,
                    memo,
                )
            };
            Output::MarketProfiles(match context.memos {
                Some(memos) => memos.market.with(indicator.id, work),
                None => work(&mut market::Memo::default()),
            })
        }
        (Kind::SessionTimes, Params::SessionTimes { ranges }) => {
            Output::Time(time::session_times(bars, timeframe, ranges, context.zone, schedule()))
        }
        // Params and kind are set together; a mismatch means a stored
        // indicator from a future version. Draw nothing rather than guess.
        _ => Output::Line(vec![None; bars.len()]),
    }
}

/// Simple moving average of closes.
pub fn sma(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    if period == 0 || bars.len() < period {
        return vec![None; bars.len()];
    }
    omacharts_studies::series::sma(bars, period)
}

/// Exponential moving average of closes, seeded with the simple average of
/// the first `period` bars so it does not start from an arbitrary value.
pub fn ema(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    if period == 0 || bars.len() < period {
        return vec![None; bars.len()];
    }
    omacharts_studies::series::ema(bars, period)
}

/// A search over the indicators, for the picker.
///
/// Same shape as the symbol search: everything is in memory, so a keystroke
/// re-runs it.
pub fn search(query: &str) -> Vec<Kind> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Kind::ALL.to_vec();
    }
    let mut scored: Vec<(i32, Kind)> = Kind::ALL
        .into_iter()
        .filter_map(|kind| {
            let name = kind.name().to_lowercase();
            let short = kind.short_name().to_lowercase();
            let score = if short == q {
                100
            } else if kind.aliases().iter().any(|a| *a == q) {
                90
            } else if short.starts_with(&q) || name.starts_with(&q) {
                70
            } else if kind.aliases().iter().any(|a| a.starts_with(&q)) {
                60
            } else if name.contains(&q) || kind.aliases().iter().any(|a| a.contains(&q)) {
                40
            } else if kind.category().label().to_lowercase().contains(&q)
                || kind.category().aliases().iter().any(|a| a.starts_with(&q))
            {
                // Its family's name: "volume" finds both volume indicators,
                // "time" the session one.
                30
            } else {
                return None;
            };
            Some((score, kind))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, kind)| kind).collect()
}

/// A picker query split into the name searched for and the numbers typed
/// after it: "sma 50", "sma50", "bb 20,2" and "macd(12 26 9)" all read.
///
/// The numbers are everything from the first digit on, where nothing past it
/// is a letter; a tail that does not read as numbers ("sma 5..0") is left in
/// the name, which then finds nothing rather than something else.
pub fn split_query(query: &str) -> (&str, Vec<f64>) {
    let query = query.trim();
    let tail = |c: char| c.is_ascii_digit() || matches!(c, '.' | ',' | '(' | ')') || c.is_whitespace();
    let Some(start) = query.find(|c: char| c.is_ascii_digit()) else { return (query, Vec::new()) };
    if !query[start..].chars().all(tail) {
        return (query, Vec::new());
    }
    let name = query[..start].trim_end_matches(|c: char| c == '(' || c.is_whitespace());
    let numbers: Option<Vec<f64>> = query[start..]
        .split(|c: char| matches!(c, ',' | '(' | ')') || c.is_whitespace())
        .filter(|token| !token.is_empty())
        .map(|token| token.parse().ok())
        .collect();
    match numbers {
        Some(numbers) => (name, numbers),
        None => (query, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    /// Every indicator drawn in a strip of its own has a height a drag or
    /// the slider can write, and nothing drawn over the price has one.
    #[test]
    fn every_strip_has_a_height_to_write() {
        for kind in Kind::ALL {
            let mut params = kind.default_params();
            assert_eq!(params.height_mut().is_some(), kind.in_own_pane(), "{kind:?}");
        }
    }

    use super::*;

    /// A cumulative delta saved before it moved into Order flow reads back
    /// as the same indicator and writes back the same bytes: only its family
    /// changed, never its name in a workspace.
    #[test]
    fn a_cumulative_delta_saved_under_volume_loads_unchanged() {
        let saved = serde_json::json!({
            "id": 4,
            "kind": "cvd",
            "params": { "kind": "cvd", "reset": "week", "style": "line", "height": 0.25, "colouring": "theme-quiet" },
            "color": { "kind": "swatch", "name": "Teal" },
            "stroke": { "width": 1.5, "style": "solid" },
            "visible": true,
            "above_price": false,
            "in_legend": true
        });
        let read: Indicator = serde_json::from_value(saved.clone()).unwrap();
        assert_eq!(read.kind, Kind::Cvd);
        assert_eq!(read.kind.category(), Category::OrderFlow);
        let Params::Cvd { reset, style, height, colouring, .. } = read.params else { panic!("{:?}", read.params) };
        assert_eq!((reset, style, height), (Some(Reset::Week), cvd::Style::Line, 0.25));
        assert_eq!(colouring, Some(crate::colouring::Colouring::Scheme(crate::theme::BarColouring::Quiet)));
        assert_eq!(serde_json::to_value(&read).unwrap(), saved, "written back as it was read");
        assert_eq!(Kind::Cvd.key(), "cvd");
    }

    /// Every indicator and band stored before its colours carried an
    /// opacity reads back with none set, drawn as it always was; one nobody
    /// changed writes none, and one set comes back as set.
    #[test]
    fn colours_stored_before_they_had_an_opacity_read_as_they_were_drawn() {
        for kind in Kind::ALL {
            let fresh = Indicator::new(1, kind);
            let written = serde_json::to_value(&fresh).unwrap();
            assert!(written.get("alpha").is_none(), "{kind:?} wrote its line's alpha: {written}");
            for key in ["signal_alpha", "d_alpha", "up_alpha", "down_alpha", "line_alpha"] {
                assert!(!written.to_string().contains(&format!("\"{key}\"")), "{kind:?} wrote {key}: {written}");
            }
            let read: Indicator = serde_json::from_value(written).unwrap();
            assert_eq!(read, fresh, "{kind:?}");
            assert_eq!(read.line_alpha(), 1.0, "{kind:?}: a line solid as before");
            assert_eq!(read.second_alpha(), 1.0, "{kind:?}");
        }
        let volume = Indicator::new(1, Kind::Volume);
        let opacity = match volume.params {
            Params::Volume { opacity, .. } => opacity,
            _ => unreachable!(),
        };
        assert_eq!(volume.params.volume_alphas(), Some((opacity, opacity)), "both bars at the one opacity");

        let mut set = Indicator::new(1, Kind::Macd);
        set.alpha = Some(0.4);
        if let Params::Macd { signal_alpha, .. } = &mut set.params {
            *signal_alpha = Some(0.6);
        }
        let read: Indicator = serde_json::from_str(&serde_json::to_string(&set).unwrap()).unwrap();
        assert_eq!((read.line_alpha(), read.second_alpha()), (0.4, 0.6));

        let mut bands = Indicator::new(1, Kind::Bollinger);
        if let Some(bands) = bands.params.bands_mut() {
            bands[0].line_alpha = Some(0.3);
        }
        let read: Indicator = serde_json::from_str(&serde_json::to_string(&bands).unwrap()).unwrap();
        assert_eq!(read.params.bands().map(|b| b[0].edge_alpha()), Some(0.3));

        let mut volume = Indicator::new(1, Kind::Volume);
        if let Params::Volume { up_alpha, down_alpha, .. } = &mut volume.params {
            (*up_alpha, *down_alpha) = (Some(0.2), Some(0.9));
        }
        let read: Indicator = serde_json::from_str(&serde_json::to_string(&volume).unwrap()).unwrap();
        assert_eq!(read.params.volume_alphas(), Some((0.2, 0.9)));
    }

    /// A footprint stored before its figures could be turned off or its rows
    /// set reads with them on and its rows automatic, and one with the bar
    /// width it was drawn from reads with it dropped; one turned off reads
    /// back off.
    #[test]
    fn a_stored_footprint_keeps_its_numbers() {
        let mut stored = serde_json::to_value(Indicator::new(1, Kind::Footprint)).unwrap();
        let params = stored["params"].as_object_mut().unwrap();
        params.remove("numbers");
        params.remove("row_ticks");
        let read: Indicator = serde_json::from_value(stored).unwrap();
        assert!(matches!(read.params, Params::Footprint { numbers: true, row_ticks: None, .. }), "{read:?}");

        // The bar width it was once drawn from is read and let go: it is
        // worked out now, and never written again.
        let mut zoomed = serde_json::to_value(Indicator::new(1, Kind::Footprint)).unwrap();
        zoomed["params"]["zoom"] = serde_json::json!(120.0);
        let read: Indicator = serde_json::from_value(zoomed).expect("a footprint with a zoom");
        assert!(matches!(read.params, Params::Footprint { numbers: true, .. }), "{read:?}");
        assert!(serde_json::to_value(&read).unwrap()["params"].get("zoom").is_none(), "the zoom written back");

        let mut off = Indicator::new(1, Kind::Footprint);
        if let Params::Footprint { numbers, .. } = &mut off.params {
            *numbers = false;
        }
        let read: Indicator = serde_json::from_value(serde_json::to_value(&off).unwrap()).unwrap();
        assert!(matches!(read.params, Params::Footprint { numbers: false, .. }), "{read:?}");
    }

    /// A volume profile stored before its strength, highlight and value
    /// area colour could be set reads as it was drawn, its old fade kept;
    /// a new one writes none of the three, and one set writes them back.
    #[test]
    fn a_stored_volume_profile_reads_as_it_was_drawn() {
        let fresh = Indicator::new(1, Kind::VolumeProfile);
        let mut stored = serde_json::to_value(&fresh).unwrap();
        let params = stored["params"].as_object_mut().unwrap();
        for key in ["alpha", "opacity", "highlight", "value_area_color"] {
            assert!(!params.contains_key(key), "{key} written by default: {params:?}");
        }
        params.insert("opacity".into(), serde_json::json!(0.4));
        let read: Indicator = serde_json::from_value(stored).unwrap();
        let Params::VolumeProfile { alpha, opacity, highlight, value_area_color, .. } = &read.params else {
            panic!("{read:?}")
        };
        assert!((profile::drawn_alpha(*alpha, *opacity) - 0.30 * 0.4).abs() < 1e-12);
        assert!(*highlight && value_area_color.is_none());

        let mut chosen = fresh.clone();
        if let Params::VolumeProfile { alpha, highlight, value_area_color, .. } = &mut chosen.params {
            *alpha = 1.0;
            *highlight = false;
            *value_area_color = Some(ColorChoice::swatch("Neutral"));
        }
        let back: Indicator = serde_json::from_str(&serde_json::to_string(&chosen).unwrap()).unwrap();
        assert_eq!(back, chosen);
    }

    /// Alt+Shift with + and − walks a study's line along the thicknesses
    /// its settings offer and stops at the ends, never at no line; a volume
    /// profile's outline goes by halves from half a pixel to six.
    #[test]
    fn a_studys_line_steps_along_the_thicknesses_its_settings_offer() {
        let mut sma = Indicator::new(1, Kind::Sma);
        let mut seen = vec![sma.stroke.width];
        for _ in 0..4 {
            assert!(sma.step_width(true));
            seen.push(sma.stroke.width);
        }
        assert_eq!(seen, [1.5, 2.5, 4.0, 4.0, 4.0]);
        for _ in 0..5 {
            sma.step_width(false);
        }
        assert_eq!(sma.stroke.width, 1.0, "never down to no line");
        sma.stroke.width = 2.0;
        sma.step_width(true);
        assert_eq!(sma.stroke.width, 2.5, "from between two, to the next");
        sma.stroke.width = 0.0;
        sma.step_width(true);
        assert_eq!(sma.stroke.width, 1.0);

        let mut profile = Indicator::new(2, Kind::VolumeProfile);
        for _ in 0..40 {
            profile.step_width(true);
        }
        let Params::VolumeProfile { outline_width, .. } = profile.params else { unreachable!() };
        assert_eq!(outline_width, MAX_OUTLINE_WIDTH);
        for _ in 0..40 {
            profile.step_width(false);
        }
        let Params::VolumeProfile { outline_width, .. } = profile.params else { unreachable!() };
        assert_eq!(outline_width, MIN_OUTLINE_WIDTH);
        assert_eq!(profile.stroke, Stroke::default(), "the profile's line is its outline");

        assert!(!Indicator::new(3, Kind::Volume).step_width(true), "bars have no line");
        assert!(!Indicator::new(4, Kind::SessionTimes).step_width(true));
    }

    fn bars(closes: &[f64]) -> Vec<Bar> {
        closes
            .iter()
            .enumerate()
            .map(|(i, &c)| Bar {
                ts: i as i64 * 86_400,
                open: c,
                high: c + 1.0,
                low: c - 1.0,
                close: c,
                volume: 100.0,
            })
            .collect()
    }

    #[test]
    fn sma_has_no_value_until_it_has_enough_bars() {
        let out = sma(&bars(&[1.0, 2.0, 3.0, 4.0]), 3);
        assert_eq!(out, vec![None, None, Some(2.0), Some(3.0)]);
    }

    #[test]
    fn sma_of_a_flat_series_is_the_level() {
        let out = sma(&bars(&[5.0; 10]), 4);
        assert!(out[3..].iter().all(|v| *v == Some(5.0)));
    }

    #[test]
    fn sma_handles_degenerate_inputs() {
        assert!(sma(&bars(&[1.0, 2.0]), 5).iter().all(Option::is_none));
        assert!(sma(&bars(&[1.0]), 0).iter().all(Option::is_none));
        assert!(sma(&[], 3).is_empty());
    }

    #[test]
    fn ema_is_seeded_with_the_simple_average() {
        let out = ema(&bars(&[1.0, 2.0, 3.0, 4.0]), 3);
        assert_eq!(out[0], None);
        assert_eq!(out[1], None);
        assert_eq!(out[2], Some(2.0), "seeded with the mean of the first three");

        // Then it follows: 4 * 0.5 + 2 * 0.5 = 3.
        assert_eq!(out[3], Some(3.0));
    }

    #[test]
    fn ema_tracks_a_flat_series_exactly() {
        let out = ema(&bars(&[7.0; 20]), 5);
        for value in out[4..].iter() {
            assert!((value.unwrap() - 7.0).abs() < 1e-9);
        }
    }

    #[test]
    fn ema_reacts_faster_than_sma() {
        // A step up: the exponential average should be above the simple one.
        let mut closes = vec![10.0; 30];
        closes.extend(vec![20.0; 5]);
        let series = bars(&closes);
        let fast = ema(&series, 10).last().unwrap().unwrap();
        let slow = sma(&series, 10).last().unwrap().unwrap();
        assert!(fast > slow, "ema {fast} should lead sma {slow}");
    }

    #[test]
    fn volume_is_an_indicator_like_any_other() {
        let volume = Indicator::new(1, Kind::Volume);
        assert_eq!(volume.label(), "Volume");
        assert_eq!(volume.kind.short_name(), "Vol");
        assert!(volume.visible);

        let series = bars(&[1.0, 2.0, 3.0]);
        match compute(&volume, &series, 0, Timeframe::days(1), None) {
            Output::Volume { values, height } => {
                assert_eq!(values, vec![100.0, 100.0, 100.0]);
                assert!((0.05..=0.6).contains(&height));
            }
            other => panic!("expected a volume pane, got {other:?}"),
        }
    }

    #[test]
    fn an_absurd_pane_height_is_brought_back_into_range() {
        let mut volume = Indicator::new(1, Kind::Volume);
        volume.params = Params::Volume { height: 5.0, colouring: None, up_color: None, down_color: None, opacity: DEFAULT_VOLUME_OPACITY, up_alpha: None, down_alpha: None };
        match compute(&volume, &bars(&[1.0, 2.0]), 0, Timeframe::days(1), None) {
            Output::Volume { height, .. } => assert_eq!(height, MAX_PANE_SHARE),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_picker_offers_volume_first() {
        // It is the one almost every chart wants, so it leads the list.
        assert_eq!(Kind::ALL[0], Kind::Volume);
        assert_eq!(search("vol")[0], Kind::Volume);
    }

    #[test]
    fn labels_say_what_the_indicator_is() {
        assert_eq!(Indicator::new(1, Kind::Sma).label(), "SMA 50");
        assert_eq!(Indicator::new(2, Kind::Ema).label(), "EMA 21");
        assert_eq!(Indicator::new(3, Kind::Vwap).label(), "VWAP · Session");
        assert_eq!(Indicator::new(4, Kind::VolumeProfile).label(), "VP · Session");
    }

    #[test]
    fn the_legend_names_the_period_actually_in_use() {
        let vwap = Indicator::new(1, Kind::Vwap);
        // Stored as a session reset, and that is what settings should show.
        assert_eq!(vwap.label(), "VWAP · Session");
        // But a session is one bar on a weekly chart, so it is promoted — and
        // saying "Session" there would describe a chart nobody is looking at.
        assert_eq!(vwap.label_for(Timeframe::weeks(1)), "VWAP · Quarter");
        assert_eq!(vwap.label_for(Timeframe::days(1)), "VWAP · Week");
        // Intraday, nothing is promoted and the two agree.
        assert_eq!(vwap.label_for(Timeframe::minutes(5)), "VWAP · Session");
    }

    #[test]
    fn a_moving_average_reads_the_same_everywhere() {
        let sma = Indicator::new(1, Kind::Sma);
        assert_eq!(sma.label(), "SMA 50");
        assert_eq!(sma.label_for(Timeframe::weeks(1)), "SMA 50");
    }

    #[test]
    fn a_fresh_set_of_indicators_comes_out_in_palette_sequence() {
        let theme = &crate::theme::builtin_themes()[0];
        let colors: Vec<String> = (0..4)
            .map(|slot| Indicator::new(slot as u32, Kind::Sma).color(theme, slot))
            .collect();
        let mut unique = colors.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 4, "four indicators, four colours: {colors:?}");
        assert_eq!(colors[0], theme.series(0));
    }

    #[test]
    fn an_explicit_colour_overrides_the_palette_slot() {
        let theme = &crate::theme::builtin_themes()[0];
        let mut indicator = Indicator::new(1, Kind::Sma);
        indicator.color = Some(ColorChoice::swatch("Rose"));
        assert_eq!(indicator.color(theme, 0), theme.swatch("Rose").unwrap().hex);
    }

    /// Five of the same indicator, which is the case the sequence exists for:
    /// a second moving average that comes out the colour of the first is two
    /// lines you cannot tell apart.
    #[test]
    fn repeating_an_indicator_takes_the_next_colour_along() {
        let theme = &crate::theme::builtin_themes()[0];
        let set: Vec<Indicator> = (1..=5).map(|id| Indicator::new(id, Kind::Sma)).collect();
        let colors = palette_colors(&set, theme);
        let mut unique = colors.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 5, "five averages, five colours: {colors:?}");
        assert_eq!(colors[0], theme.series(0), "the first still starts the sequence");
    }

    #[test]
    fn a_colour_somebody_chose_is_never_handed_to_anybody_else() {
        let theme = &crate::theme::builtin_themes()[0];
        // The second indicator is pinned to the colour the first would have
        // taken automatically. The first has to move, not the pinned one.
        let mut set: Vec<Indicator> = (1..=3).map(|id| Indicator::new(id, Kind::Ema)).collect();
        let first = theme.series(0);
        set[1].color = Some(ColorChoice::Fixed { hex: first.clone() });

        let colors = palette_colors(&set, theme);
        assert_eq!(colors[1], first, "the pinned one keeps what it was given");
        assert_ne!(colors[0], first, "the automatic one gives way");
        assert_ne!(colors[2], first);
        assert_ne!(colors[0], colors[2]);
    }

    #[test]
    fn adding_or_reordering_does_not_repaint_what_is_already_there() {
        let theme = &crate::theme::builtin_themes()[0];
        let mut set: Vec<Indicator> = (1..=3).map(|id| Indicator::new(id, Kind::Sma)).collect();
        let before = palette_colors(&set, theme);

        set.push(Indicator::new(4, Kind::Vwap));
        let after_adding = palette_colors(&set, theme);
        assert_eq!(&after_adding[..3], &before[..], "the three already drawn keep their colours");

        // Same indicators, listed the other way round: colours follow the
        // indicator, not the row it happens to sit in.
        set.reverse();
        let reordered = palette_colors(&set, theme);
        for (indicator, colour) in set.iter().zip(&reordered) {
            let was = &after_adding[(indicator.id - 1) as usize];
            assert_eq!(colour, was, "indicator {} changed colour on reorder", indicator.id);
        }
    }

    #[test]
    fn past_the_palette_colours_repeat_rather_than_running_out() {
        let theme = &crate::theme::builtin_themes()[0];
        let set: Vec<Indicator> = (1..=9).map(|id| Indicator::new(id, Kind::Sma)).collect();
        let colors = palette_colors(&set, theme);
        assert_eq!(colors.len(), 9);
        assert!(colors.iter().all(|c| !c.is_empty()), "every one gets a colour");
    }

    /// The time this thread has spent on a CPU: what a piece of work cost,
    /// leaving out the time it waited while something else had the core.
    fn on_cpu() -> std::time::Duration {
        let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: `now` is a valid timespec for the call to write into.
        let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
        assert_eq!(ok, 0, "the thread's CPU clock");
        std::time::Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
    }

    /// Caching indicators would mean storing and invalidating derived values
    /// to save a few milliseconds a repaint. This exists to catch one that
    /// stops being cheap, above all one that becomes quadratic.
    ///
    /// What it measures is the work, not the wait. A shared CI runner is a
    /// slice of a busy machine, and the rest of the suite runs beside this
    /// on its threads, so the wall clock there says as much about the
    /// neighbours as about the indicators: it failed an 80 ms budget at 81,
    /// 85 and 86 with no change to them. So each repaint is timed on this
    /// thread's CPU clock, which stops while the thread waits for a core,
    /// and the best of several is taken, which leaves out the rounds a busy
    /// core's caches slowed. Neither can make the work look cheaper than it
    /// is. The budget stays at 80 ms: this desktop's debug build spends 26
    /// of CPU on it.
    ///
    /// And the shape is held as well as the size, because a fast machine
    /// can hide a quadratic indicator under any budget. Four times the bars
    /// costing four times as much is linear, and reads 4.0 to 4.2; wholly
    /// quadratic would be sixteen. The line is at six, which leaves that
    /// reading room to wobble and still catches a quadratic part by the
    /// time it is half the cost. It holds on any machine, however fast,
    /// because it is the same machine on both sides.
    #[test]
    fn computing_every_indicator_is_cheap_enough_to_do_on_every_repaint() {
        let series = |bars: usize| -> Vec<Bar> {
            (0..bars)
                .map(|i| {
                    let price = 100.0 + (i as f64 / 50.0).sin() * 5.0;
                    Bar {
                        ts: i as i64 * 3_600,
                        open: price,
                        high: price + 0.5,
                        low: price - 0.5,
                        close: price + 0.1,
                        volume: 1_000.0,
                    }
                })
                .collect()
        };
        // Two years of hourly bars, which is the most this app ever holds,
        // and a quarter of that.
        let most = series(12_000);
        let quarter = series(3_000);

        let indicators: Vec<Indicator> =
            Kind::ALL.into_iter().enumerate().map(|(i, k)| Indicator::new(i as u32, k)).collect();
        let repaint = |series: &[Bar]| {
            let start = on_cpu();
            for indicator in &indicators {
                let _ = compute(indicator, series, 0, Timeframe::hours(1), None);
            }
            on_cpu() - start
        };

        let (mut best, mut best_quarter) = (std::time::Duration::MAX, std::time::Duration::MAX);
        for _ in 0..10 {
            best_quarter = best_quarter.min(repaint(&quarter));
            best = best.min(repaint(&most));
        }

        assert!(best < std::time::Duration::from_millis(80), "every indicator over 12k bars took {best:?}");
        let growth = best.as_secs_f64() / best_quarter.as_secs_f64();
        assert!(growth < 6.0, "four times the bars cost {growth:.1} times as much: {best_quarter:?} and then {best:?}");
        eprintln!("every indicator over 12k bars: {best:?} per repaint, {growth:.1}x a quarter of them");
    }

    /// Volume, RSI and ATR in that order, with a moving average among them to
    /// prove an overlay is not a row.
    fn stacked() -> Vec<Indicator> {
        vec![
            Indicator::new(1, Kind::Volume),
            Indicator::new(2, Kind::Sma),
            Indicator::new(3, Kind::Rsi),
            Indicator::new(4, Kind::Atr),
        ]
    }

    #[test]
    fn strips_stack_under_the_price_in_the_order_they_are_listed() {
        assert_eq!(
            stack(&stacked()),
            vec![Row::Price, Row::Strip(1), Row::Strip(3), Row::Strip(4)]
        );
    }

    #[test]
    fn an_overlay_is_not_a_row_the_arrows_can_move() {
        let mut set = stacked();
        assert!(!move_pane(&mut set, 2, Move::Up), "a moving average has no strip to move");
        assert_eq!(stack(&set), stack(&stacked()), "and nothing else moved either");
    }

    #[test]
    fn moving_a_strip_up_steps_past_the_strip_above_it() {
        let mut set = stacked();
        assert!(move_pane(&mut set, 4, Move::Up));
        assert_eq!(
            stack(&set),
            vec![Row::Price, Row::Strip(1), Row::Strip(4), Row::Strip(3)]
        );
    }

    #[test]
    fn the_topmost_strip_under_the_price_steps_above_the_price_next() {
        let mut set = stacked();
        assert!(move_pane(&mut set, 1, Move::Up));
        assert_eq!(
            stack(&set),
            vec![Row::Strip(1), Row::Price, Row::Strip(3), Row::Strip(4)]
        );
        assert!(set[0].above_price);
    }

    #[test]
    fn a_strip_above_the_price_comes_back_down_the_way_it_went_up() {
        let mut set = stacked();
        move_pane(&mut set, 1, Move::Up);
        assert!(move_pane(&mut set, 1, Move::Down));
        assert_eq!(stack(&set), stack(&stacked()), "back where it started");
        assert!(!set.iter().any(|i| i.above_price));
    }

    /// Two strips above the price keep their own order, and a third arriving
    /// from below lands between the price and the one that was already lowest.
    #[test]
    fn a_strip_crossing_the_price_lands_at_the_edge_it_arrived_from() {
        let mut set = stacked();
        move_pane(&mut set, 1, Move::Up);
        move_pane(&mut set, 3, Move::Up);
        move_pane(&mut set, 3, Move::Up);
        assert_eq!(
            stack(&set),
            vec![Row::Strip(3), Row::Strip(1), Row::Price, Row::Strip(4)],
            "the second one to cross stopped at the price, then traded places"
        );

        move_pane(&mut set, 3, Move::Down);
        assert_eq!(
            stack(&set),
            vec![Row::Strip(1), Row::Strip(3), Row::Price, Row::Strip(4)],
            "and back down to the price's edge without leaving its own side"
        );

        move_pane(&mut set, 4, Move::Up);
        assert_eq!(
            stack(&set),
            vec![Row::Strip(1), Row::Strip(3), Row::Strip(4), Row::Price],
            "it joined the group at the price, not over the top of it"
        );
    }

    #[test]
    fn the_strip_at_either_end_of_the_stack_has_nowhere_further_to_go() {
        let mut set = stacked();
        assert!(!move_pane(&mut set, 4, Move::Down), "already the bottom row");
        move_pane(&mut set, 1, Move::Up);
        assert!(!move_pane(&mut set, 1, Move::Up), "already the top row");
    }

    /// The one case a chart with a single strip has: it cannot swap with
    /// another strip, but it can still cross the price.
    #[test]
    fn the_only_strip_on_a_chart_can_still_cross_the_price() {
        let mut set = vec![Indicator::new(1, Kind::Volume)];
        assert!(!move_pane(&mut set, 1, Move::Down), "nothing below the bottom row");
        assert!(move_pane(&mut set, 1, Move::Up));
        assert_eq!(stack(&set), vec![Row::Strip(1), Row::Price]);
        assert!(!move_pane(&mut set, 1, Move::Up), "and nothing above the top one");
    }

    /// A strip somebody has switched off is drawn nowhere, so clicking the
    /// arrow on the strip above it has to step past it rather than trade
    /// places with something invisible and look like it did nothing.
    #[test]
    fn a_hidden_strip_is_not_a_row_either() {
        let mut set = stacked();
        set[2].visible = false;
        assert_eq!(stack(&set), vec![Row::Price, Row::Strip(1), Row::Strip(4)]);
        assert!(move_pane(&mut set, 4, Move::Up));
        assert_eq!(stack(&set), vec![Row::Price, Row::Strip(4), Row::Strip(1)]);

        set[2].visible = true;
        assert!(
            stack(&set).contains(&Row::Strip(3)),
            "and switching it back on puts it back in the stack"
        );
    }

    /// Reordering strips must not touch the overlays: their order decides
    /// which line is drawn over which on the price plot.
    #[test]
    fn reordering_strips_leaves_the_overlays_where_they_were() {
        let mut set = vec![
            Indicator::new(1, Kind::Volume),
            Indicator::new(2, Kind::Sma),
            Indicator::new(3, Kind::Rsi),
            Indicator::new(4, Kind::Ema),
            Indicator::new(5, Kind::Atr),
        ];
        let overlays = |set: &[Indicator]| -> Vec<(usize, u32)> {
            set.iter()
                .enumerate()
                .filter(|(_, i)| !i.kind.in_own_pane())
                .map(|(at, i)| (at, i.id))
                .collect()
        };
        let before = overlays(&set);
        move_pane(&mut set, 5, Move::Up);
        move_pane(&mut set, 5, Move::Up);
        move_pane(&mut set, 5, Move::Up);
        assert_eq!(overlays(&set), before);
    }

    /// Nothing rewrites the stored workspace, so every indicator anybody has
    /// was written before a strip could sit above the price. Reading one has
    /// to leave the chart looking the way they left it.
    #[test]
    fn an_indicator_written_before_strips_could_sit_above_the_price_comes_back_below_it() {
        let json = r#"{"id": 1, "kind": "rsi", "visible": true,
            "params": {"kind": "rsi", "period": 14, "height": 0.16,
                       "overbought": 70.0, "oversold": 30.0}}"#;
        let indicator: Indicator = serde_json::from_str(json).expect("an older indicator");
        assert!(!indicator.above_price);
        let back: Indicator =
            serde_json::from_str(&serde_json::to_string(&indicator).unwrap()).unwrap();
        assert!(!back.above_price, "and it round trips as it is");
    }

    /// A volume profile stored before its width could be chosen is drawn at
    /// the share of its period every profile was drawn at then.
    #[test]
    fn a_profile_written_before_its_width_could_be_chosen_keeps_its_width() {
        let json = r#"{"id": 1, "kind": "volume_profile", "visible": true,
            "params": {"kind": "volume_profile", "reset": "session", "value_area": 0.7,
                       "align": "right"}}"#;
        let indicator: Indicator = serde_json::from_str(json).expect("an older profile");
        let Params::VolumeProfile { width, .. } = indicator.params else { unreachable!() };
        assert_eq!(width, profile::DEFAULT_WIDTH);
        assert_eq!(width, 0.4);
    }

    /// A profile, a MACD and a stochastic stored before their lines could be
    /// patterned or the profile faded are drawn as they were — solid, the
    /// second line as the first, the profile as designed — and write none of
    /// it back until it is chosen.
    #[test]
    fn line_patterns_and_a_profile_s_opacity_default_to_what_there_was() {
        let profile: Indicator = serde_json::from_str(
            r#"{"id": 1, "kind": "volume_profile", "visible": true,
                "params": {"kind": "volume_profile", "reset": "session", "value_area": 0.7}}"#,
        )
        .expect("an older profile");
        let Params::VolumeProfile { outline_style, poc_style, opacity, .. } = &profile.params else { unreachable!() };
        assert_eq!((*outline_style, *poc_style, *opacity), (LineStyle::Solid, LineStyle::Solid, 1.0));
        let written = serde_json::to_value(&profile).unwrap();
        for key in ["outline_style", "poc_style", "opacity"] {
            assert!(written["params"].get(key).is_none(), "{key} written back: {written}");
        }

        let macd: Indicator = serde_json::from_str(
            r#"{"id": 2, "kind": "macd", "visible": true,
                "params": {"kind": "macd", "fast": 12, "slow": 26, "signal": 9, "height": 0.16}}"#,
        )
        .expect("an older MACD");
        assert!(matches!(macd.params, Params::Macd { signal_style: None, .. }));
        assert!(serde_json::to_value(&macd).unwrap()["params"].get("signal_style").is_none());

        let stochastic: Indicator = serde_json::from_str(
            r#"{"id": 3, "kind": "stochastic", "visible": true,
                "params": {"kind": "stochastic", "period": 14, "k_smooth": 3, "d_period": 3,
                           "height": 0.16, "overbought": 80, "oversold": 20}}"#,
        )
        .expect("an older stochastic");
        assert!(matches!(stochastic.params, Params::Stochastic { d_style: None, .. }));
        assert!(serde_json::to_value(&stochastic).unwrap()["params"].get("d_style").is_none());

        let mut faded = profile.clone();
        if let Params::VolumeProfile { opacity, outline_style, .. } = &mut faded.params {
            *opacity = 0.4;
            *outline_style = LineStyle::Dotted;
        }
        let back: Indicator = serde_json::from_value(serde_json::to_value(&faded).unwrap()).unwrap();
        assert!(matches!(back.params, Params::VolumeProfile { opacity: 0.4, outline_style: LineStyle::Dotted, .. }));
    }

    /// A profile stored before it could measure anything else measures what
    /// it always did, and is named as it always was.
    #[test]
    fn a_profile_written_before_its_method_could_be_chosen_is_the_total() {
        let json = r#"{"id": 1, "kind": "volume_profile", "visible": true,
            "params": {"kind": "volume_profile", "reset": "session", "value_area": 0.7}}"#;
        let indicator: Indicator = serde_json::from_str(json).expect("an older profile");
        let Params::VolumeProfile { method, .. } = indicator.params else { unreachable!() };
        assert_eq!(method, profile::Method::Total);
        assert_eq!(indicator.describe(None), "VP · Session");
    }

    /// A profile written before its point of control could be turned off
    /// draws it, and one turned off stays off.
    #[test]
    fn a_profile_draws_its_point_of_control_until_it_is_turned_off() {
        let json = r#"{"id": 1, "kind": "volume_profile", "visible": true,
            "params": {"kind": "volume_profile", "reset": "session", "value_area": 0.7}}"#;
        let older: Indicator = serde_json::from_str(json).expect("an older profile");
        assert!(matches!(older.params, Params::VolumeProfile { poc: true, .. }));
        assert!(matches!(Indicator::new(1, Kind::VolumeProfile).params, Params::VolumeProfile { poc: true, .. }));

        let mut off = older.clone();
        if let Params::VolumeProfile { poc, .. } = &mut off.params {
            *poc = false;
        }
        let back: Indicator = serde_json::from_str(&serde_json::to_string(&off).unwrap()).unwrap();
        assert_eq!(back, off);
    }

    /// A profile written before it could be shown in its last period only
    /// draws every period, and one shown in the last only stays so.
    #[test]
    fn a_profile_draws_every_period_until_it_is_shown_in_the_last_only() {
        let json = r#"{"id": 1, "kind": "volume_profile", "visible": true,
            "params": {"kind": "volume_profile", "reset": "session", "value_area": 0.7}}"#;
        let older: Indicator = serde_json::from_str(json).expect("an older profile");
        assert!(matches!(older.params, Params::VolumeProfile { last_only: false, .. }));
        assert!(matches!(Indicator::new(1, Kind::VolumeProfile).params, Params::VolumeProfile { last_only: false, .. }));

        let mut last = older.clone();
        if let Params::VolumeProfile { last_only, .. } = &mut last.params {
            *last_only = true;
        }
        let back: Indicator = serde_json::from_str(&serde_json::to_string(&last).unwrap()).unwrap();
        assert_eq!(back, last);
    }

    /// A delta's colouring is written down only once one is picked, by the
    /// key a chart's colouring is, and a profile written before it could be
    /// picked follows Preferences.
    #[test]
    fn a_delta_profile_keeps_the_colouring_picked_for_it() {
        let json = r#"{"id": 1, "kind": "volume_profile", "visible": true,
            "params": {"kind": "volume_profile", "reset": "session", "value_area": 0.7, "method": "delta"}}"#;
        let older: Indicator = serde_json::from_str(json).expect("an older profile");
        let Params::VolumeProfile { colouring, .. } = &older.params else { unreachable!() };
        assert_eq!(*colouring, None);
        assert!(!serde_json::to_string(&older).unwrap().contains("colouring"), "nothing picked, nothing written");

        let mut picked = older.clone();
        if let Params::VolumeProfile { colouring, .. } = &mut picked.params {
            *colouring = Some(crate::colouring::Colouring::Scheme(crate::theme::BarColouring::Quiet));
        }
        let json = serde_json::to_string(&picked).unwrap();
        assert!(json.contains(r#""colouring":"theme-quiet""#), "{json}");
        let back: Indicator = serde_json::from_str(&json).unwrap();
        assert_eq!(back, picked);
    }

    #[test]
    fn a_delta_profile_says_so_in_the_legend() {
        let mut indicator = Indicator::new(1, Kind::VolumeProfile);
        if let Params::VolumeProfile { method, .. } = &mut indicator.params {
            *method = profile::Method::Delta;
        }
        assert_eq!(indicator.describe(None), "VP Δ · Session");
        let back: Indicator = serde_json::from_str(&serde_json::to_string(&indicator).unwrap()).unwrap();
        assert_eq!(back, indicator);
    }

    /// A strip stored above the price, from when it could be put there, comes
    /// back below it: nothing in the app could move it back down.
    #[test]
    fn a_strip_stored_above_the_price_comes_back_below_it() {
        let mut set = stacked();
        move_pane(&mut set, 1, Move::Up);
        assert_eq!(stack(&set)[0], Row::Strip(1), "above the price before it is stored");
        let json = serde_json::to_string(&set).unwrap();
        let back: Vec<Indicator> = serde_json::from_str(&json).unwrap();
        assert!(!back.iter().any(|i| i.above_price));
        assert_eq!(stack(&back), vec![Row::Price, Row::Strip(1), Row::Strip(3), Row::Strip(4)]);
    }

    #[test]
    fn a_strip_moved_from_the_app_stays_on_its_side_of_the_price() {
        let mut set = stacked();
        assert!(!move_strip(&mut set, 1, Move::Up), "the price is above it");
        assert_eq!(stack(&set), stack(&stacked()));
        assert!(move_strip(&mut set, 3, Move::Up), "past the strip above it");
        assert_eq!(stack(&set), vec![Row::Price, Row::Strip(3), Row::Strip(1), Row::Strip(4)]);
        assert!(!move_strip(&mut set, 4, Move::Down), "already the bottom row");
        assert!(!move_strip(&mut set, 2, Move::Up), "an overlay has no strip");
    }

    #[test]
    fn the_picker_finds_things_by_name_abbreviation_and_concept() {
        assert_eq!(search("sma")[0], Kind::Sma);
        assert_eq!(search("vwap")[0], Kind::Vwap);
        assert_eq!(search("poc")[0], Kind::VolumeProfile);
        assert_eq!(search("volume profile")[0], Kind::VolumeProfile);
        assert_eq!(search("exponential")[0], Kind::Ema);

        // "ma" is an alias of both averages and should surface them together.
        let mas = search("ma");
        assert!(mas.contains(&Kind::Sma) && mas.contains(&Kind::Ema));

        assert_eq!(search("").len(), Kind::ALL.len());
        assert!(search("zzzz").is_empty());
    }

    /// Numbers typed after the name are the lengths, however they are
    /// separated; a tail that is not numbers stays in the name.
    #[test]
    fn a_picker_query_splits_into_a_name_and_its_numbers() {
        assert_eq!(split_query("sma 50"), ("sma", vec![50.0]));
        assert_eq!(split_query(" sma50 "), ("sma", vec![50.0]));
        assert_eq!(split_query("sma(50)"), ("sma", vec![50.0]));
        assert_eq!(split_query("bb 20,2.5"), ("bb", vec![20.0, 2.5]));
        assert_eq!(split_query("macd 12 26 9"), ("macd", vec![12.0, 26.0, 9.0]));
        assert_eq!(split_query("volume profile"), ("volume profile", vec![]));
        assert_eq!(split_query("sma 5..0"), ("sma 5..0", vec![]));
        assert_eq!(split_query("50 sma"), ("50 sma", vec![]));
    }

    /// The numbers fill what the label shows, in its order, and a number the
    /// label has no place for, or one that does not fit, adds nothing.
    #[test]
    fn typed_numbers_fill_the_lengths_the_label_shows() {
        let label = |kind, numbers: &[f64]| Indicator::numbered(1, kind, numbers).map(|i| i.label());
        assert_eq!(label(Kind::Sma, &[50.0]).as_deref(), Some("SMA 50"));
        assert_eq!(label(Kind::Ema, &[]).as_deref(), Some("EMA 21"));
        assert_eq!(label(Kind::Bollinger, &[10.0, 2.5]).as_deref(), Some("BB 10 2.5"));
        assert_eq!(label(Kind::Bollinger, &[10.0, -0.0]).as_deref(), Some("BB 10 0"));
        assert_eq!(label(Kind::Macd, &[5.0, 35.0, 5.0]).as_deref(), Some("MACD 5 35 5"));
        assert_eq!(label(Kind::Macd, &[8.0]).as_deref(), Some("MACD 8 26 9"));
        assert_eq!(label(Kind::Stochastic, &[21.0, 5.0]).as_deref(), Some("Stoch 21 5 3"));

        assert_eq!(label(Kind::Sma, &[50.0, 20.0]), None, "one more than the label shows");
        assert_eq!(label(Kind::Sma, &[0.0]), None, "no bars");
        assert_eq!(label(Kind::Sma, &[2.5]), None, "part of a bar");
        assert_eq!(label(Kind::Macd, &[30.0]), None, "fast not below the slow 26");
        assert_eq!(label(Kind::Bollinger, &[20.0, 51.0]), None, "past the widest multiple");
        assert_eq!(label(Kind::Vwap, &[1.0]), None, "no length to take it");
    }

    #[test]
    fn the_two_band_indicators_are_computed_into_the_same_shape() {
        let series = bars(&(0..60).map(|i| 100.0 + (i as f64 / 4.0).sin() * 3.0).collect::<Vec<_>>());
        let bollinger = Indicator::new(1, Kind::Bollinger);
        let vwap = Indicator::new(2, Kind::Vwap);

        for indicator in [&bollinger, &vwap] {
            match compute(indicator, &series, 0, Timeframe::days(1), None) {
                Output::Bands(bands) => {
                    assert_eq!(bands.center.len(), series.len());
                    assert!(bands.bands.iter().all(|band| band.upper.len() == series.len()));
                }
                other => panic!("{:?} is not an envelope: {other:?}", indicator.kind),
            }
        }
        // Which is the point of them being the same shape: neither needs a
        // strip, and the renderer has one path for both.
        assert!(!Kind::Bollinger.in_own_pane() && !Kind::Vwap.in_own_pane());
    }

    #[test]
    fn a_macd_is_a_strip_with_two_lines_and_a_histogram_in_it() {
        let series = bars(&(0..80).map(|i| 100.0 + i as f64 * 0.3).collect::<Vec<_>>());
        let macd = Indicator::new(1, Kind::Macd);
        match compute(&macd, &series, 0, Timeframe::days(1), None) {
            Output::Pane(pane) => {
                assert_eq!(pane.values.len(), series.len());
                assert_eq!(pane.signal.as_ref().map(Vec::len), Some(series.len()));
                assert_eq!(pane.histogram.as_ref().map(Vec::len), Some(series.len()));
                // Fitted rather than fixed, and with nothing as its one level.
                assert_eq!(pane.bounds, None);
                assert_eq!(pane.guides, vec![0.0]);
                assert_eq!(pane.band, None, "a MACD has no region to shade");
            }
            other => panic!("expected a strip, got {other:?}"),
        }
        assert!(Kind::Macd.in_own_pane());
    }

    /// Every other strip hands back no histogram, so nothing that was here
    /// before starts drawing bars it never drew.
    #[test]
    fn only_the_macd_draws_a_histogram() {
        let series = bars(&(0..80).map(|i| 100.0 + (i as f64 / 5.0).sin()).collect::<Vec<_>>());
        for kind in Kind::ALL {
            let Output::Pane(pane) =
                compute(&Indicator::new(1, kind), &series, 0, Timeframe::days(1), None)
            else {
                continue;
            };
            assert_eq!(
                pane.histogram.is_some(),
                kind == Kind::Macd,
                "{kind:?} answered with {:?}",
                pane.histogram.is_some()
            );
        }
    }

    #[test]
    fn the_new_indicators_arrive_with_the_settings_everybody_elses_chart_uses() {
        let Params::Bollinger { period, bands } = Kind::Bollinger.default_params() else {
            panic!("not a Bollinger")
        };
        assert_eq!(period, 20);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].deviations, 2.0);

        let Params::Macd { fast, slow, signal, .. } = Kind::Macd.default_params() else {
            panic!("not a MACD")
        };
        assert_eq!((fast, slow, signal), (12, 26, 9));
    }

    #[test]
    fn the_new_indicators_say_what_they_are() {
        assert_eq!(Indicator::new(1, Kind::Bollinger).label(), "BB 20 2");
        assert_eq!(Indicator::new(2, Kind::Macd).label(), "MACD 12 26 9");

        // Neither is promoted by the timeframe, as a VWAP's anchor is: a
        // window of bars means the same thing on any chart.
        assert_eq!(Indicator::new(1, Kind::Bollinger).label_for(Timeframe::weeks(1)), "BB 20 2");
        assert_eq!(Indicator::new(2, Kind::Macd).label_for(Timeframe::weeks(1)), "MACD 12 26 9");

        // A multiple somebody moved shows in the name, or two envelopes at
        // different widths are two legend rows nobody can tell apart.
        let mut wider = Indicator::new(1, Kind::Bollinger);
        if let Some(bands) = wider.params.bands_mut() {
            bands[0].deviations = 2.5;
        }
        assert_eq!(wider.label(), "BB 20 2.5");
    }

    #[test]
    fn the_picker_finds_both_of_them() {
        assert_eq!(search("bollinger")[0], Kind::Bollinger);
        assert_eq!(search("bb")[0], Kind::Bollinger);
        assert_eq!(search("macd")[0], Kind::Macd);
        assert!(search("divergence").contains(&Kind::Macd));
        // "bands" is a VWAP's word too, and both should come back for it.
        let bands = search("bands");
        assert!(bands.contains(&Kind::Vwap) && bands.contains(&Kind::Bollinger));
    }

    /// Only the two of them carry an envelope, and the accessor is what the
    /// editor and the command line ask rather than each keeping a list.
    #[test]
    fn the_indicators_that_carry_bands_are_the_two_that_draw_them() {
        for kind in Kind::ALL {
            let carries = kind.default_params().bands().is_some();
            assert_eq!(
                carries,
                matches!(kind, Kind::Vwap | Kind::Bollinger),
                "{kind:?} answered {carries}"
            );
        }
    }

    /// A MACD's strip stacks and moves like any other, which is the whole
    /// reason it is one.
    #[test]
    fn a_macd_takes_its_place_in_the_stack_like_the_strips_already_there() {
        let mut set = vec![
            Indicator::new(1, Kind::Volume),
            Indicator::new(2, Kind::Bollinger),
            Indicator::new(3, Kind::Rsi),
            Indicator::new(4, Kind::Macd),
        ];
        assert_eq!(
            stack(&set),
            vec![Row::Price, Row::Strip(1), Row::Strip(3), Row::Strip(4)],
            "the Bollinger is an overlay, so it is not a row"
        );
        assert!(move_pane(&mut set, 4, Move::Up));
        assert_eq!(stack(&set), vec![Row::Price, Row::Strip(1), Row::Strip(4), Row::Strip(3)]);
    }

    /// Both are written into the stored arrangement, so both have to survive
    /// the trip through it — a band somebody restyled included.
    #[test]
    fn the_new_indicators_survive_being_written_down() {
        let mut set =
            vec![Indicator::new(1, Kind::Bollinger), Indicator::new(2, Kind::Macd)];
        if let Some(bands) = set[0].params.bands_mut() {
            bands[0].deviations = 1.5;
            bands[0].fill = false;
        }
        if let Params::Macd { signal_color, .. } = &mut set[1].params {
            *signal_color = Some(ColorChoice::swatch("Amber"));
        }
        let json = serde_json::to_string(&set).expect("writable");
        let back: Vec<Indicator> = serde_json::from_str(&json).expect("readable");
        assert_eq!(back, set);
        assert!(json.contains(r#""kind":"bollinger""#), "{json}");
        assert!(json.contains(r#""kind":"macd""#), "{json}");
    }

    /// A MACD written before it had a signal colour of its own — which is
    /// every one anybody could have, but the default is what keeps it true
    /// when the next field arrives.
    #[test]
    fn a_macd_written_without_a_signal_colour_reads_back_following_the_theme() {
        let json = r#"{"id": 1, "kind": "macd", "visible": true,
            "params": {"kind": "macd", "fast": 12, "slow": 26, "signal": 9, "height": 0.16}}"#;
        let indicator: Indicator = serde_json::from_str(json).expect("an older MACD");
        let Params::Macd { signal_color, .. } = &indicator.params else { panic!("not a MACD") };
        assert_eq!(*signal_color, None);
    }

    mod at_price {
        use super::*;
        use crate::ladders::{to_price, Minute, Series, Side};
        use crate::symbols::InstrumentKind;

        const TICK: f64 = 0.25;

        fn es() -> Instrument {
            Instrument {
                symbol: "ES".into(),
                name: "E-mini S&P 500".into(),
                kind: InstrumentKind::FutureRoot,
                suffix: None,
                currency: Some("USD".into()),
                tier: 0,
                session_origin: 79_200,
                overrides: Vec::new(),
                exchange: None,
                popularity: 0,
                local_name: None,
                formula: None,
            }
        }

        /// Three CME sessions of one-minute ladders from 2026-10-11 22:00
        /// UTC, the halt left out, and the bars they make.
        fn sessions(instrument: &Instrument) -> (Series, Vec<Bar>) {
            sessions_of(instrument, true)
        }

        /// [`sessions`], every print with its aggressor or, `sided` false,
        /// none with one: a feed that never says.
        fn sessions_of(instrument: &Instrument, sided: bool) -> (Series, Vec<Bar>) {
            let days = Days::new(instrument);
            let mut state = 0x9e37_79b9_7f4a_7c15u64;
            let mut next = move |n: u64| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state % n.max(1)
            };
            let mut series = Series::new(TICK);
            // 2026-10-11 22:00 UTC, Sunday 17:00 Chicago: the week opens.
            let start: i64 = 1_791_842_400;
            let mut center = 26_000i32;
            for i in 0..3 * 1_440 {
                let ts = start + i * 60;
                if ts.rem_euclid(86_400) / 3_600 == 21 {
                    continue; // the daily halt, 16:00 to 17:00 Chicago
                }
                center += next(9) as i32 - 4;
                let mut minute = Minute::opening(ts, to_price(center, TICK));
                for _ in 0..1 + next(30) {
                    let tick = center + next(13) as i32 - 6;
                    let side = match next(12) {
                        _ if !sided => Side::Unknown,
                        0 => Side::Unknown,
                        1..=5 => Side::Sell,
                        _ => Side::Buy,
                    };
                    minute.add_trade(tick, to_price(tick, TICK), 1 + next(25), side, 1);
                }
                series.insert(&days, minute);
            }
            let bars = series
                .minutes()
                .map(|m| Bar { ts: m.ts, open: m.open, high: m.high, low: m.low, close: m.close, volume: m.volume as f64 })
                .collect();
            (series, bars)
        }

        fn profile_of(bars: &[Bar], instrument: &Instrument, rows: Option<usize>, ladders: Option<&Held>) -> Vec<Profile> {
            let mut indicator = Indicator::new(1, Kind::VolumeProfile);
            if let Params::VolumeProfile { rows: r, reset, .. } = &mut indicator.params {
                *r = rows;
                *reset = Reset::Session;
            }
            let context = Context::of(instrument, Timeframe::minutes(1), chrono_tz::UTC).with_ladders(ladders);
            match compute_in(&indicator, bars, &context) {
                Output::Profiles(profiles) => profiles,
                other => panic!("not profiles: {other:?}"),
            }
        }

        /// What a delta profile of `bars` draws, whatever that is.
        fn delta_of(bars: &[Bar], instrument: &Instrument, rows: Option<usize>, ladders: Option<&Held>) -> Output {
            let mut indicator = Indicator::new(1, Kind::VolumeProfile);
            if let Params::VolumeProfile { rows: r, reset, method, .. } = &mut indicator.params {
                *r = rows;
                *reset = Reset::Session;
                *method = profile::Method::Delta;
            }
            let context = Context::of(instrument, Timeframe::minutes(1), chrono_tz::UTC).with_ladders(ladders);
            compute_in(&indicator, bars, &context)
        }

        /// A cumulative delta is the aggressors' on a chart with volume at
        /// price, and says so as plain "CVD"; without it, it is estimated
        /// from bar direction rather than refused, and the legend marks it
        /// "CVD ≈". A delta profile on that same chart is still refused.
        #[test]
        fn a_cumulative_delta_is_estimated_where_a_delta_profile_is_refused() {
            let instrument = es();
            let (series, bars) = sessions(&instrument);
            let held = Held::of(&series);
            let indicator = Indicator::new(1, Kind::Cvd);
            let minute = Timeframe::minutes(1);
            let bare = Context::of(&instrument, minute, chrono_tz::UTC);
            let rich = bare.with_ladders(Some(&held));

            let estimated = compute_in(&indicator, &bars, &bare);
            let real = compute_in(&indicator, &bars, &rich);
            assert!(estimated.is_estimated());
            assert!(!real.is_estimated());
            assert_eq!(indicator.legend_for(minute, estimated.is_estimated()), "CVD ≈ · Session");
            assert_eq!(indicator.legend_for(minute, real.is_estimated()), "CVD · Session");

            // The aggressors' sum is not the bars' direction: the two differ.
            let closes = |output: &Output| match output {
                Output::Pane(Pane { delta: Some(delta), guides, .. }) => {
                    assert_eq!(guides, &vec![0.0], "a zero line");
                    delta.iter().map(|bar| bar.close).collect::<Vec<f64>>()
                }
                other => panic!("not a delta pane: {other:?}"),
            };
            assert_eq!(closes(&estimated).len(), bars.len());
            assert_ne!(closes(&estimated), closes(&real));

            assert!(matches!(delta_of(&bars, &instrument, None, None), Output::Unmet(_)), "no estimate for a profile");
        }

        /// Each row of a delta profile is what was bought at the ask less
        /// what was sold at the bid at its prices, worked out here minute by
        /// minute and price by price; its point of control is the greatest
        /// either way, and it has no value area beyond it.
        #[test]
        fn a_delta_profile_is_the_ask_volume_less_the_bid_volume_at_each_price() {
            let instrument = es();
            let (series, bars) = sessions(&instrument);
            let held = Held::of(&series);
            for rows in [None, Some(24)] {
                let Output::Profiles(profiles) = delta_of(&bars, &instrument, rows, Some(&held)) else {
                    panic!("a delta profile on a chart with ladders")
                };
                assert_eq!(profiles.len(), 3, "{rows:?}");
                for profile in &profiles {
                    let (from, to) = (bars[profile.first_bar].ts, bars[profile.last_bar].ts + 60);
                    let mut expected = vec![0.0; profile.rows.len()];
                    for minute in series.minutes().filter(|m| (from..to).contains(&m.ts)) {
                        for (tick, level) in minute.ladder.iter() {
                            let price = to_price(tick, TICK);
                            let row = profile
                                .rows
                                .iter()
                                .position(|r| price >= r.low - 1e-6 && price < r.high - 1e-6)
                                .expect("every price has a row");
                            expected[row] += level.at_ask as f64 - level.at_bid as f64;
                        }
                    }
                    let got: Vec<f64> = profile.rows.iter().map(|r| r.volume).collect();
                    assert_eq!(got, expected, "{rows:?} at {from}");
                    assert!(got.iter().any(|d| *d > 0.0) && got.iter().any(|d| *d < 0.0), "both sides");
                    let most = got.iter().fold(0.0f64, |most, d| most.max(d.abs()));
                    assert_eq!(profile.max_volume, most);
                    assert_eq!(profile.rows[profile.poc].volume.abs(), most);
                    assert_eq!(profile.value_area, (profile.poc, profile.poc));
                    assert_eq!(profile.total_volume, got.iter().sum::<f64>(), "the net delta");
                }
            }
        }

        /// A delta is never estimated: with no ladders it is one line
        /// saying what it needs, a period from before they begin is not
        /// drawn, and a feed that never says the aggressor draws nothing.
        #[test]
        fn a_delta_profile_is_drawn_only_from_volume_at_price() {
            let instrument = es();
            let (series, bars) = sessions(&instrument);
            assert_eq!(delta_of(&bars, &instrument, None, None), Output::Unmet(NEEDS_VOLUME_AT_PRICE));
            assert!(!profile_of(&bars, &instrument, None, None).is_empty(), "the total is estimated as ever");

            let mut held = Held::of(&series);
            held.floor = bars[2_000].ts;
            let Output::Profiles(late) = delta_of(&bars, &instrument, Some(24), Some(&held)) else {
                panic!("profiles")
            };
            let all = profile_of(&bars, &instrument, Some(24), Some(&held));
            assert_eq!(all.len(), 3);
            assert_eq!(late.iter().map(|p| p.start_ts).collect::<Vec<_>>(), vec![all[2].start_ts]);

            let (series, bars) = sessions_of(&instrument, false);
            let held = Held::of(&series);
            assert_eq!(delta_of(&bars, &instrument, None, Some(&held)), Output::Profiles(Vec::new()));
        }

        /// The profile drawn from ladders is the sum, row by row, of what
        /// every minute in the period traded at each price in that row,
        /// worked out here the slow way, minute by minute and price by price.
        #[test]
        fn a_profile_from_ladders_is_every_minute_summed_by_price() {
            for sided in [true, false] {
                every_minute_summed_by_price(sided);
            }
        }

        /// A feed that never says the aggressor still draws its profile
        /// from what traded at each price, and its delta is nothing.
        #[test]
        fn trades_with_no_aggressor_still_make_a_profile_and_no_delta() {
            let instrument = es();
            let (series, bars) = sessions_of(&instrument, false);
            let held = Held::of(&series);
            let profiles = profile_of(&bars, &instrument, None, Some(&held));
            assert_eq!(profiles.len(), 3);
            let traded: f64 = bars.iter().map(|b| b.volume).sum();
            assert_eq!(profiles.iter().map(|p| p.total_volume).sum::<f64>(), traded);
            let (from, to) = (bars[0].ts, bars[bars.len() - 1].ts + 60);
            let ladder = held.between(from, to).expect("held whole");
            assert_eq!((ladder.total() as f64, ladder.delta()), (traded, 0));
            assert!(ladder.levels.iter().all(|l| l.at_bid == 0 && l.at_ask == 0));
        }

        fn every_minute_summed_by_price(sided: bool) {
            let instrument = es();
            let (series, bars) = sessions_of(&instrument, sided);
            let held = Held::of(&series);
            for rows in [None, Some(24)] {
                let profiles = profile_of(&bars, &instrument, rows, Some(&held));
                assert_eq!(profiles.len(), 3, "{rows:?}");
                for profile in &profiles {
                    let (from, to) = (bars[profile.first_bar].ts, bars[profile.last_bar].ts + 60);
                    let mut expected = vec![0.0; profile.rows.len()];
                    let mut total = 0u64;
                    for minute in series.minutes().filter(|m| (from..to).contains(&m.ts)) {
                        for (tick, level) in minute.ladder.iter() {
                            let price = to_price(tick, TICK);
                            let row = profile
                                .rows
                                .iter()
                                .position(|r| price >= r.low - 1e-6 && price < r.high - 1e-6)
                                .expect("every price has a row");
                            expected[row] += level.volume() as f64;
                            total += level.volume();
                        }
                    }
                    let got: Vec<f64> = profile.rows.iter().map(|r| r.volume).collect();
                    assert_eq!(got, expected, "{rows:?} at {from}");
                    assert_eq!(profile.total_volume, total as f64);
                    for row in &profile.rows {
                        let ticks = (row.high - row.low) / TICK;
                        assert!((ticks - ticks.round()).abs() < 1e-9 && ticks >= 1.0, "rows are whole ticks: {ticks}");
                    }
                }
            }
        }

        /// A period from before the ladders begin keeps the estimate, and
        /// one after it does not; a chart with no ladders is the estimate
        /// throughout, by the very code it always ran.
        #[test]
        fn a_period_before_the_ladders_begin_keeps_the_estimate() {
            let instrument = es();
            let (series, bars) = sessions(&instrument);
            let mut held = Held::of(&series);
            held.floor = bars[2_000].ts;
            let exact = profile_of(&bars, &instrument, Some(24), Some(&held));
            let estimate = profile_of(&bars, &instrument, Some(24), None);
            let plain = profile::compute(
                &bars,
                Reset::Session,
                79_200,
                Some(24),
                0.7,
                Some(InstrumentKind::FutureRoot),
                &[],
            );
            assert_eq!(estimate, plain, "no ladders is the old code path");
            // Spread over ranges, an estimate's rows are fractions; counted
            // from ladders they are whole contracts.
            let whole = |p: &Profile| p.rows.iter().all(|r| r.volume.fract() == 0.0);
            assert!(!whole(&exact[0]) && !whole(&exact[1]), "before the floor");
            assert!(whole(&exact[2]), "after it");
        }

        /// Ladders on their way are waited for, never estimated meanwhile:
        /// a chart whose ladders are still loading draws no profile, and a
        /// period whose piece of a day is still being read draws none
        /// either, then the exact one when it is in. Where the ladder will
        /// not come — a piece the store lacks, a source with no ladders — the
        /// estimate is drawn as it always was.
        #[test]
        fn a_profile_waits_for_ladders_on_their_way_rather_than_estimating() {
            let instrument = es();
            let (series, bars) = sessions(&instrument);
            // Ten hours into the first session: its period cuts that day.
            let bars = &bars[600..];
            let days = series
                .days
                .iter()
                .map(|d| crate::ladders::HeldDay::total(d.day, d.first_ts().unwrap(), d.last_ts().unwrap(), d.total.clone()))
                .collect::<Vec<_>>();
            let floor = days.first().map_or(i64::MAX, |d| d.first_ts);
            let mut held = Held { tick_size: TICK, days, floor, floor_day: i64::MIN, ..Held::default() };
            let whole = |p: &Profile| p.rows.iter().all(|r| r.volume.fract() == 0.0);

            // Loading: nothing at all, where the estimate used to be drawn.
            let mut indicator = Indicator::new(1, Kind::VolumeProfile);
            if let Params::VolumeProfile { reset, .. } = &mut indicator.params {
                *reset = Reset::Session;
            }
            let bare = Context::of(&instrument, Timeframe::minutes(1), chrono_tz::UTC);
            let profiles = |context: &Context| match compute_in(&indicator, bars, context) {
                Output::Profiles(profiles) => profiles,
                other => panic!("not profiles: {other:?}"),
            };
            assert!(profiles(&bare.awaiting_ladders(true)).is_empty(), "nothing while they load");
            let estimated = profiles(&bare);
            assert_eq!(estimated.len(), 3, "a source without ladders draws the estimate");
            assert!(!whole(&estimated[0]));

            // Loaded by day totals: the cut session waits for its piece.
            let waiting = profile_of(bars, &instrument, None, Some(&held));
            assert_eq!(waiting.len(), 2, "the cut session is not drawn yet");
            assert!(waiting.iter().all(whole), "the others are exact");
            let wanted = held.take_wanted();
            assert_eq!(wanted.len(), 1);

            // The store turns out not to have it: estimated, as before.
            held.missing.insert(wanted[0]);
            let kept = profile_of(bars, &instrument, None, Some(&held));
            assert_eq!(kept.len(), 3);
            assert!(!whole(&kept[0]), "estimated");

            // In: exact, on the first frame it is drawn.
            let day = series.days.iter().find(|d| d.day == wanted[0].day).unwrap();
            assert!(held.put_piece(wanted[0], day.piece(wanted[0].from, wanted[0].to)));
            assert!(held.missing.is_empty(), "no longer missing");
            let exact = profile_of(bars, &instrument, None, Some(&held));
            assert_eq!(exact.len(), 3);
            assert!(exact.iter().all(whole), "every session exact");
        }

        /// The mechanism an indicator that needs volume at price stands on:
        /// on a chart with ladders it is computed, on one without it is one
        /// line, and nothing that needs only bars is ever stopped.
        #[test]
        fn an_indicator_that_needs_volume_at_price_says_so_on_a_chart_without() {
            let instrument = es();
            let (series, _) = sessions(&instrument);
            let held = Held::of(&series);
            let bare = Context::of(&instrument, Timeframe::minutes(1), chrono_tz::UTC);
            let rich = bare.with_ladders(Some(&held));

            // What a footprint declares.
            let footprint = Kind::Footprint.needs();
            assert_eq!(footprint, Needs::VolumeAtPrice);
            assert_eq!(unmet(footprint, &bare), Some(Output::Unmet("Needs a data feed with volume at price")));
            assert_eq!(unmet(footprint, &rich), None);

            // Only the two that read single trades or prices need them;
            // everything else, the deltas included, falls back to bars.
            for kind in Kind::ALL {
                let wants = matches!(kind, Kind::Footprint | Kind::LargeOrders);
                assert_eq!(kind.needs() == Needs::VolumeAtPrice, wants, "{kind:?}");
                if !wants {
                    assert_eq!(unmet(kind.needs(), &bare), None);
                }
            }
            let users: Vec<Kind> = Kind::ALL.into_iter().filter(|k| k.uses_volume_at_price()).collect();
            assert_eq!(users, vec![Kind::VolumeProfile, Kind::Cvd, Kind::VolumeDelta, Kind::Footprint, Kind::LargeOrders]);
        }

        /// A volume profile drawn over a span that is one of the study's
        /// sessions is that session's profile, row for row, whichever it
        /// measures: the two are worked out by the same code from the same
        /// ladders.
        #[test]
        fn a_drawn_profile_over_a_session_is_the_studys_profile_of_it() {
            let instrument = es();
            let (series, bars) = sessions(&instrument);
            let held = Held::of(&series);
            let context = Context::of(&instrument, Timeframe::minutes(1), chrono_tz::UTC).with_ladders(Some(&held));
            for rows in [None, Some(24)] {
                let Output::Profiles(deltas) = delta_of(&bars, &instrument, rows, Some(&held)) else {
                    panic!("a delta profile on a chart with ladders")
                };
                let totals = profile_of(&bars, &instrument, rows, Some(&held));
                for (method, study) in [(profile::Method::Delta, &deltas), (profile::Method::Total, &totals)] {
                    assert_eq!(study.len(), 3);
                    for session in study {
                        let span = &bars[session.first_bar..=session.last_bar];
                        let drawn = span_profile(span, session.first_bar, rows, 0.70, method, &context);
                        assert_eq!(drawn, Ok(Some(session.clone())), "{method:?} {rows:?}");
                    }
                }
            }
        }

        /// Without volume at price, a drawn delta is refused for the reason
        /// the study gives, and a drawn total is estimated from the bars, as
        /// the study's is, unless the ladders are on their way: then it waits
        /// for them, drawing nothing rather than an estimate.
        #[test]
        fn a_drawn_delta_without_ladders_is_refused_as_the_studys_is() {
            let instrument = es();
            let (_, bars) = sessions(&instrument);
            let bare = Context::of(&instrument, Timeframe::minutes(1), chrono_tz::UTC);
            let span = &bars[..600];
            let Output::Unmet(why) = delta_of(&bars, &instrument, None, None) else { panic!("the study is refused") };
            assert_eq!(span_profile(span, 0, None, 0.70, profile::Method::Delta, &bare), Err(why));
            assert!(matches!(
                span_profile(span, 0, None, 0.70, profile::Method::Total, &bare),
                Ok(Some(_))
            ));
            let waiting = bare.awaiting_ladders(true);
            assert_eq!(span_profile(span, 0, None, 0.70, profile::Method::Total, &waiting), Ok(None));
        }
    }

    /// Every study painted in a colouring offers Custom, from one model: a
    /// colour saved on one before Custom was a choice, over a preset or
    /// not, loads as Custom with that colour, and is drawn in it as before.
    #[test]
    fn every_study_with_a_colouring_reads_a_saved_colour_as_custom() {
        use crate::colouring::{self, Colouring};
        let hex = "#123456";
        let cases: [(Kind, serde_json::Value); 6] = [
            (Kind::Volume, serde_json::json!({"colouring": "theme-soft", "up_color": {"kind": "fixed", "hex": hex}})),
            (Kind::Cvd, serde_json::json!({"up_color": {"kind": "fixed", "hex": hex}})),
            (Kind::VolumeProfile, serde_json::json!({"buy_color": {"kind": "fixed", "hex": hex}})),
            (Kind::VolumeDelta, serde_json::json!({"inks": {"colouring": "theme", "buy": {"kind": "fixed", "hex": hex}}})),
            (Kind::Footprint, serde_json::json!({"inks": {"buy": {"kind": "fixed", "hex": hex}}})),
            (Kind::LargeOrders, serde_json::json!({"inks": {"buy": {"kind": "fixed", "hex": hex}}})),
        ];
        let theme = crate::theme::builtin_themes().remove(0);
        for (kind, saved) in cases {
            let mut stored = serde_json::to_value(Indicator::new(1, kind)).unwrap();
            for (key, value) in saved.as_object().unwrap() {
                match (stored["params"].get_mut(key), value) {
                    (Some(serde_json::Value::Object(held)), serde_json::Value::Object(given)) => {
                        held.extend(given.clone());
                    }
                    _ => stored["params"][key] = value.clone(),
                }
            }
            let mut read: Indicator = serde_json::from_value(stored).unwrap();
            let (chosen, own) = read.params.colours().unwrap_or_else(|| panic!("{kind:?} has a colouring"));
            assert!(colouring::is_custom(chosen, &own), "{kind:?}");
            assert_eq!(colouring::drawn(true, own[0], "#ffffff", &theme), hex, "{kind:?} draws its colour");
            let mut colours = read.params.colours_mut().unwrap();
            assert_eq!(colours.shown(), Some(Colouring::Custom), "{kind:?} shows Custom");
            // Choosing a preset again lets the colour go, and is written so.
            colours.pick(Some(Colouring::Scheme(crate::theme::BarColouring::Quiet)), &[]);
            let written = serde_json::to_value(&read).unwrap();
            assert!(!written.to_string().contains(hex), "{kind:?}: {written}");
            let back: Indicator = serde_json::from_value(written).unwrap();
            let (chosen, own) = back.params.colours().unwrap();
            assert!(!colouring::is_custom(chosen, &own), "{kind:?}");
            assert_eq!(chosen, Some(Colouring::Scheme(crate::theme::BarColouring::Quiet)), "{kind:?}");
        }
    }

    /// Custom is written as `custom`, and read back as it was.
    #[test]
    fn custom_is_written_by_name_and_read_back() {
        use crate::colouring::Colouring;
        let mut volume = Indicator::new(1, Kind::Volume);
        volume.params.colours_mut().unwrap().pick(Some(Colouring::Custom), &["#010203".into(), "#040506".into()]);
        let written = serde_json::to_value(&volume).unwrap();
        assert_eq!(written["params"]["colouring"], "custom");
        assert_eq!(written["params"]["up_color"]["hex"], "#010203");
        let back: Indicator = serde_json::from_value(written).unwrap();
        assert_eq!(back.params.colours().unwrap().0, Some(Colouring::Custom));
    }
}
