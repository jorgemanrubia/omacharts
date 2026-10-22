//! Everything omacharts knows that is not a widget.
//!
//! This crate links neither GTK nor SQLite. The app owns storage and drawing;
//! the engine owns themes, bars, the symbol index and the data providers. That
//! split keeps the parts that deserve tests testable without a display, and
//! keeps the provider boundary honest.

pub mod bars;
pub mod clock;
pub mod drawings;
pub mod exchanges;
pub mod frame;
pub mod indicators;
pub mod ink;
pub mod link;
pub mod omarchy;
pub mod palette;
pub mod provider;
pub mod refresh;
pub mod session;
pub mod providers;
pub mod stream;
pub mod symbols;
pub mod theme;

pub use bars::{
    bucket_of, drawable, drawable_from, grouped_decimals, price_decimals, repair_continuous_opens, resample, Bar,
    BarStyle, Timeframe,
};
pub use indicators::{palette_colors, Indicator, Kind as IndicatorKind, Output, Params, Reset};
pub use link::LinkMode;
pub use provider::{
    Capability, Closes, Delivery, FetchFailure, Pacing, Provider, ProviderError, SharedCooldown,
    Sink, Stream, Subscription, Update,
};
pub use session::Session;
pub use symbols::{Instrument, InstrumentKind, SearchHit, SearchIndex};
pub use drawings::{
    Align, Anchor, Arrow, ArrowHead, Configurations, Drawing, Grip, Kind as DrawingKind, Paint, Place,
    Preset, Projected, Scope, Span, Style, Text as DrawingText, TextStyle,
};
pub use frame::Frame;
pub use ink::{Ink, Tag};
pub use theme::{
    editable_bars, theme_bars, BarColouring, BarScheme, BarSlot, ColorChoice, Direction, Mode, Source, Swatch, Theme,
    UiColors, UiSlot,
};
