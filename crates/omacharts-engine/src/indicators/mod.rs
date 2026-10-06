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

pub mod oscillators;
pub mod periods;
pub mod profile;
pub mod vwap;

use serde::{Deserialize, Serialize};

use crate::bars::{Bar, Timeframe};
use crate::theme::{ColorChoice, Theme, SWATCH_SEQUENCE};

pub use periods::Reset;
pub use profile::{Profile, ProfileRow};
pub use vwap::{Bands, MAX_FILL_ALPHA, MIN_FILL_ALPHA};

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
    VolumeProfile,
    Volume,
    Rsi,
    Atr,
    Stochastic,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Volume,
        Kind::Sma,
        Kind::Ema,
        Kind::Vwap,
        Kind::VolumeProfile,
        Kind::Rsi,
        Kind::Atr,
        Kind::Stochastic,
    ];

    pub fn name(self) -> &'static str {
        match self {
            // Spelled out. The short form is beside it in its own column, so
            // repeating it in brackets says the same thing twice.
            Kind::Sma => "Simple Moving Average",
            Kind::Ema => "Exponential Moving Average",
            Kind::Vwap => "Volume Weighted Average Price",
            Kind::VolumeProfile => "Volume Profile",
            Kind::Volume => "Volume",
            Kind::Rsi => "Relative Strength Index",
            Kind::Atr => "Average True Range",
            Kind::Stochastic => "Stochastic",
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
            Kind::VolumeProfile => "VP",
            Kind::Volume => "Vol",
            Kind::Rsi => "RSI",
            Kind::Atr => "ATR",
            Kind::Stochastic => "Stoch",
        }
    }

    /// Searched alongside the name, so "moving average" and "ma" both find
    /// both averages, and "poc" finds the profile.
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Kind::Sma => &["sma", "ma", "moving average", "mean"],
            Kind::Ema => &["ema", "ma", "moving average", "exponential"],
            Kind::Vwap => &["vwap", "volume weighted", "average price", "bands"],
            Kind::VolumeProfile => &["volume profile", "vp", "poc", "value area", "tpo"],
            Kind::Volume => &["volume", "vol", "turnover"],
            Kind::Rsi => &["rsi", "relative strength", "oscillator", "momentum", "overbought"],
            Kind::Atr => &["atr", "average true range", "volatility", "range", "stop"],
            Kind::Stochastic => &["stochastic", "stoch", "kd", "%k", "oscillator", "momentum", "overbought"],
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Kind::Sma => "sma",
            Kind::Ema => "ema",
            Kind::Vwap => "vwap",
            Kind::VolumeProfile => "volume_profile",
            Kind::Volume => "volume",
            Kind::Rsi => "rsi",
            Kind::Atr => "atr",
            Kind::Stochastic => "stochastic",
        }
    }

    /// Does this draw in a strip of its own rather than over the price?
    ///
    /// A computed series already knows — that is [`Output::pane_height`] — but
    /// the order of the strips is edited where nothing has been computed: the
    /// settings list, and the command line.
    pub fn in_own_pane(self) -> bool {
        matches!(self, Kind::Volume | Kind::Rsi | Kind::Atr | Kind::Stochastic)
    }

    pub fn default_params(self) -> Params {
        match self {
            Kind::Sma => Params::MovingAverage { period: 50 },
            Kind::Ema => Params::MovingAverage { period: 21 },
            Kind::Vwap => Params::Vwap { reset: Reset::Session, bands: vwap::default_bands() },
            Kind::VolumeProfile => Params::VolumeProfile {
                reset: Reset::Session,
                rows: None,
                value_area: 0.70,
                poc_color: None,
            },
            Kind::Volume => Params::Volume { height: 0.18 },
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
            },
        }
    }
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
        bands: Vec<vwap::Band>,
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
    },
    Volume {
        /// How much of the chart's height the pane takes.
        height: f64,
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
    },
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
    #[serde(default)]
    pub above_price: bool,
}

impl Indicator {
    pub fn new(id: u32, kind: Kind) -> Indicator {
        Indicator {
            id,
            kind,
            params: kind.default_params(),
            color: None,
            stroke: Stroke::default(),
            visible: true,
            above_price: false,
        }
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

    fn describe(&self, timeframe: Option<Timeframe>) -> String {
        let effective = |reset: Reset| match timeframe {
            Some(timeframe) => reset.effective_for(timeframe),
            None => reset,
        };
        match &self.params {
            Params::MovingAverage { period } => format!("{} {period}", self.kind.short_name()),
            Params::Vwap { reset, .. } => format!("VWAP · {}", effective(*reset).label()),
            Params::VolumeProfile { reset, .. } => format!("VP · {}", effective(*reset).label()),
            Params::Volume { .. } => "Volume".to_string(),
            Params::Rsi { period, .. } | Params::Atr { period, .. } => {
                format!("{} {period}", self.kind.short_name())
            }
            Params::Stochastic { period, k_smooth, d_period, .. } => {
                format!("Stoch {period} {k_smooth} {d_period}")
            }
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
    /// Volume per bar, with the share of the chart its pane takes.
    Volume { values: Vec<f64>, height: f64 },
    /// A series drawn in a strip of its own, on its own scale.
    Pane(Pane),
}

/// An indicator that cannot share the price scale, and so gets a strip of its
/// own above or below the chart.
#[derive(Clone, PartialEq, Debug)]
pub struct Pane {
    pub values: Vec<Option<f64>>,
    /// A second line over the first, on the same scale: a stochastic's %D.
    pub signal: Option<Vec<Option<f64>>>,
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

/// Compute an indicator over `bars`.
///
/// `session_origin` is the instrument's session open, which is what makes a
/// session-reset VWAP on futures start in the evening rather than at midnight.
/// `timeframe` lets a reset period that is too fine for the chart be promoted
/// rather than drawn as nonsense.
pub fn compute(
    indicator: &Indicator,
    bars: &[Bar],
    session_origin: i64,
    timeframe: Timeframe,
    kind: Option<crate::symbols::InstrumentKind>,
) -> Output {
    match (&indicator.kind, &indicator.params) {
        (Kind::Sma, Params::MovingAverage { period }) => Output::Line(sma(bars, *period)),
        (Kind::Ema, Params::MovingAverage { period }) => Output::Line(ema(bars, *period)),
        (Kind::Vwap, Params::Vwap { reset, bands }) => Output::Bands(vwap::compute(
            bars,
            reset.effective_for(timeframe),
            session_origin,
            bands,
        )),
        (Kind::Rsi, Params::Rsi { period, height, overbought, oversold }) => Output::Pane(Pane {
            values: oscillators::rsi(bars, *period),
            signal: None,
            height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
            bounds: Some((0.0, 100.0)),
            guides: vec![*oversold, 50.0, *overbought],
            band: Some((*oversold, *overbought)),
        }),
        (Kind::Atr, Params::Atr { period, height }) => Output::Pane(Pane {
            values: oscillators::atr(bars, *period),
            signal: None,
            height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
            bounds: None,
            guides: Vec::new(),
            band: None,
        }),
        (
            Kind::Stochastic,
            Params::Stochastic { period, k_smooth, d_period, height, overbought, oversold, .. },
        ) => {
            let (k, d) = oscillators::stochastic(bars, *period, *k_smooth, *d_period);
            Output::Pane(Pane {
                values: k,
                signal: Some(d),
                height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
                bounds: Some((0.0, 100.0)),
                guides: vec![*oversold, 50.0, *overbought],
                band: Some((*oversold, *overbought)),
            })
        }
        (Kind::Volume, Params::Volume { height }) => Output::Volume {
            values: bars.iter().map(|bar| bar.volume).collect(),
            height: height.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE),
        },
        (Kind::VolumeProfile, Params::VolumeProfile { reset, rows, value_area, .. }) => {
            Output::Profiles(profile::compute(
                bars,
                reset.effective_for(timeframe),
                session_origin,
                *rows,
                *value_area,
                kind,
            ))
        }
        // Params and kind are set together; a mismatch means a stored
        // indicator from a future version. Draw nothing rather than guess.
        _ => Output::Line(vec![None; bars.len()]),
    }
}

/// Simple moving average of closes.
pub fn sma(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 || bars.len() < period {
        return out;
    }
    let mut sum = 0.0;
    for (i, bar) in bars.iter().enumerate() {
        sum += bar.close;
        if i >= period {
            sum -= bars[i - period].close;
        }
        if i + 1 >= period {
            out[i] = Some(sum / period as f64);
        }
    }
    out
}

/// Exponential moving average of closes, seeded with the simple average of
/// the first `period` bars so it does not start from an arbitrary value.
pub fn ema(bars: &[Bar], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; bars.len()];
    if period == 0 || bars.len() < period {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let seed: f64 = bars[..period].iter().map(|b| b.close).sum::<f64>() / period as f64;
    let mut value = seed;
    out[period - 1] = Some(value);
    for (i, bar) in bars.iter().enumerate().skip(period) {
        value = bar.close * alpha + value * (1.0 - alpha);
        out[i] = Some(value);
    }
    out
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
            } else {
                return None;
            };
            Some((score, kind))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(_, kind)| kind).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        volume.params = Params::Volume { height: 5.0 };
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
        assert_eq!(vwap.label_for(Timeframe::days(1)), "VWAP · Month");
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

    #[test]
    fn computing_every_indicator_is_cheap_enough_to_do_on_every_repaint() {
        // Two years of hourly bars, which is the most this app ever holds.
        let series: Vec<Bar> = (0..12_000)
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
            .collect();

        let indicators: Vec<Indicator> =
            Kind::ALL.into_iter().enumerate().map(|(i, k)| Indicator::new(i as u32, k)).collect();

        let start = std::time::Instant::now();
        let rounds = 20;
        for _ in 0..rounds {
            for indicator in &indicators {
                let _ = compute(indicator, &series, 0, Timeframe::hours(1), None);
            }
        }
        let per_repaint = start.elapsed() / rounds;

        // Caching these would mean storing and invalidating derived values to
        // save a few milliseconds a repaint. The budget here is deliberately
        // loose; it exists to catch an indicator that becomes quadratic.
        assert!(
            per_repaint < std::time::Duration::from_millis(50),
            "all four indicators over 12k bars took {per_repaint:?}"
        );
        eprintln!("all indicators over 12k bars: {per_repaint:?} per repaint");
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

    #[test]
    fn a_strip_above_the_price_survives_being_written_down() {
        let mut set = stacked();
        move_pane(&mut set, 1, Move::Up);
        let json = serde_json::to_string(&set).unwrap();
        let back: Vec<Indicator> = serde_json::from_str(&json).unwrap();
        assert_eq!(stack(&back), stack(&set));
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
}
