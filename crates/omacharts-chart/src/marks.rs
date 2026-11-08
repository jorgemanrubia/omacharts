//! Price marks: what the embedding app lays over a chart at a price.
//!
//! A mark is a line across the plot at its price, a label at one end of it
//! and, if it has one, its price in a chip on the axis. The chart draws the
//! marks it is handed, lights their handles under the pointer, lets a hand
//! take a draggable one to another price, and says what was done through a
//! [`MarkEvent`]. It never decides what a mark means: whoever filled the
//! marks hears the event and does whatever it stands for.
//!
//! Where a drag may go, and what the label in hand says on its way, is the
//! app's to say too, through [`MarkRules`]. Without any, a mark goes
//! anywhere and its label says the price under the hand.
//!
//! Anything anchored in time as well as price — an annotation at a bar, an
//! arrow, a note — is a drawing ([`omacharts_engine::drawings::Drawing`]),
//! set with `ChartView::set_drawings`.

use std::any::Any;

use omacharts_engine::grouped_decimals;
use omacharts_engine::indicators::LineStyle;

/// How far from a draggable mark's line, in pixels, the pointer may be to
/// take hold of it.
pub const MARK_NEAR: f64 = 6.0;

/// One mark at a price.
#[derive(Clone, Debug, PartialEq)]
pub struct PriceMark {
    /// Whatever the app knows the mark by. Every event about it hands it
    /// back, so it should tell the marks apart.
    pub id: String,
    pub price: f64,
    /// The label's first segment, filled in the mark's colour: "L 1". No
    /// segment at all when empty.
    pub tag: String,
    /// After the tag, in the theme's ink, on the chart's ground.
    pub body: Option<String>,
    /// Its price on the axis, or no chip there at all.
    pub chip: Option<AxisChip>,
    /// The mark's line, its label's outline, its tag's fill and its ×.
    pub colour: String,
    pub style: LineStyle,
    /// How heavy its line is, or no line, the label alone.
    pub line: Option<f64>,
    /// Drawn faint and not to be taken hold of: on its way, or asked about
    /// and not yet answered.
    pub dim: bool,
    /// Its label ends in a ×, which [`MarkEvent::Close`] says was clicked.
    pub closable: bool,
    /// Its line, and its ×, can be taken to another price:
    /// [`MarkEvent::Move`].
    pub draggable: bool,
    /// Tabs beside its label, each dragged out to a price of its own:
    /// [`MarkEvent::Tab`].
    pub tabs: Vec<MarkTab>,
    /// Joined to the other linked marks by a line beside their labels.
    pub link: Option<MarkLink>,
    /// Over the line joining linked marks, where the others are under it;
    /// and its handles are found before theirs.
    pub raised: bool,
}

impl PriceMark {
    /// A plain mark: a line of weight 1.5 at `price` in `colour`, its tag
    /// alone in its label, nothing to take hold of.
    pub fn new(id: impl Into<String>, price: f64, tag: impl Into<String>, colour: impl Into<String>) -> PriceMark {
        PriceMark {
            id: id.into(),
            price,
            tag: tag.into(),
            body: None,
            chip: None,
            colour: colour.into(),
            style: LineStyle::Solid,
            line: Some(1.5),
            dim: false,
            closable: false,
            draggable: false,
            tabs: Vec::new(),
            link: None,
            raised: false,
        }
    }

    /// Its label in one line: "L 1 · +$125.00".
    pub fn words(&self) -> String {
        match &self.body {
            Some(body) if self.tag.is_empty() => body.clone(),
            Some(body) => format!("{} · {body}", self.tag),
            None => self.tag.clone(),
        }
    }
}

/// A mark's price on the axis.
#[derive(Clone, Debug, PartialEq)]
pub struct AxisChip {
    pub text: String,
    /// Filled in the mark's colour; outlined in it otherwise.
    pub filled: bool,
}

/// A tab beside a mark's label, dragged out to place something at a price.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkTab {
    /// What the tab is, handed back in [`MarkEvent::Tab`].
    pub id: String,
    /// The word on it.
    pub text: String,
    /// The tag of the label that follows the hand while it is dragged out.
    pub tag: String,
    /// Its outline, and the line that follows the hand.
    pub colour: String,
    pub style: LineStyle,
    /// How heavy the tab's outline is.
    pub outline: f64,
    /// How heavy the line that follows the hand is.
    pub weight: f64,
}

/// How a mark is linked with the others: they are joined by one line,
/// dashed if any of them asks for it, faint while any is pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkLink {
    pub dashed: bool,
    pub pending: bool,
}

/// What the hand did to the marks. The chart only says; nothing is done
/// until whoever hears it does it.
#[derive(Clone, PartialEq, Debug)]
pub enum MarkEvent {
    /// A mark's × was clicked.
    Close(String),
    /// A draggable mark was let go at `price`.
    Move { mark: String, price: f64 },
    /// A mark's tab was dragged out and let go at `price`.
    Tab { mark: String, tab: String, price: f64 },
}

/// A mark, or one of its tabs, in hand: what [`MarkRules`] is asked about.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkDrag<'a> {
    /// The mark taken hold of, or whose tab was.
    pub mark: &'a str,
    /// The tab, when one is being dragged out.
    pub tab: Option<&'a str>,
    /// The mark's own price, where the drag started from.
    pub from: f64,
    /// Where the hand has it, on the series' tick.
    pub at: f64,
    /// The last bar's close, if there are bars.
    pub last: Option<f64>,
    /// The series' tick, when it has one.
    pub tick: Option<f64>,
    /// The decimals `at` is written to.
    pub places: usize,
}

/// What the label in hand says, and in what colour.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkPreview {
    pub text: String,
    /// The colour of the mark in hand, where it is not its own. Ignored
    /// where it may not go, which is drawn muted.
    pub colour: Option<String>,
}

/// Where a mark in hand may go and what its label says on the way.
///
/// Handed to the chart with the marks, and asked again whenever the marks
/// change, so it should be a value that compares equal while what it says
/// would be the same: a chart handed equal marks and equal rules does not
/// redraw.
pub trait MarkRules: Any {
    /// May it be let go at `drag.at`? A drag let go where it may not go
    /// says nothing.
    fn allowed(&self, drag: &MarkDrag) -> bool {
        let _ = drag;
        true
    }

    /// What the label in hand says at `drag.at`, given whether it may go
    /// there.
    fn preview(&self, drag: &MarkDrag, allowed: bool) -> MarkPreview {
        let _ = allowed;
        MarkPreview { text: grouped_decimals(drag.at, drag.places), colour: None }
    }
}

/// The rules of a chart that was given none: anywhere, saying the price.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnyPrice;

impl MarkRules for AnyPrice {}

/// Rules as the chart keeps them: any type, compared with the next ones it
/// is handed.
pub(crate) trait KeptRules {
    fn rules(&self) -> &dyn MarkRules;
    fn same(&self, other: &dyn Any) -> bool;
    fn as_any(&self) -> &dyn Any;
}

impl<R: MarkRules + PartialEq> KeptRules for R {
    fn rules(&self) -> &dyn MarkRules {
        self
    }

    fn same(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<R>().is_some_and(|other| other == self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Which end of the chart the marks' labels sit at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelSide {
    /// At the plot's left edge, the tag at the edge and the × towards the
    /// bars: they never move with the bars.
    #[default]
    Left,
    /// By the price axis, in the room after the last bar.
    Right,
}

impl LabelSide {
    pub const ALL: [LabelSide; 2] = [LabelSide::Left, LabelSide::Right];

    pub fn key(self) -> &'static str {
        match self {
            LabelSide::Left => "left",
            LabelSide::Right => "right",
        }
    }

    pub fn from_key(key: &str) -> Option<LabelSide> {
        LabelSide::ALL.into_iter().find(|side| side.key() == key)
    }

    pub fn label(self) -> &'static str {
        match self {
            LabelSide::Left => "Left",
            LabelSide::Right => "Right",
        }
    }
}
