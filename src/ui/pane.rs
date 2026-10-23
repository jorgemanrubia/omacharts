//! One chart in the layout.
//!
//! The window used to be a chart with a legend painted over it. With several
//! charts side by side, everything that was "the chart's" moves in here: which
//! symbol it shows, at what resolution, with which indicators, in which
//! session, drawn how. A pane is the unit you focus, split, and close.
//!
//! What stays with the window is what is genuinely one per window — the
//! watchlist, the theme, the symbol index — and the question of which pane is
//! focused.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gtk::prelude::*;
use omacharts_engine::{
    BarStyle, Indicator, Instrument, LinkMode, Session, Theme, Timeframe,
};

use crate::ui::chart::{ChartView, Hidden};
use crate::ui::shortcuts;

/// One chart filling the window, and the way back.
pub const MAXIMIZE: &str = "Maximize";
pub const RESTORE: &str = "Restore";

pub struct ChartPane {
    pub id: u32,
    pub view: Rc<ChartView>,
    /// What goes in the layout: the chart, its legend, and the focus ring.
    pub root: gtk::Box,
    /// The symbol, over the top left of the chart. A button: clicking the
    /// name of the thing you are looking at to change it is the shortest
    /// route there is.
    pub symbol_button: gtk::Button,
    /// This chart's resolutions, since the resolution is this chart's. One
    /// strip in the header could only ever describe one of them.
    ///
    /// Shown on the focused chart only: four strips on screen is the same row
    /// of buttons four times, and only one of them is the one you are about to
    /// press. The others say their resolution in a word instead.
    pub strip: gtk::Box,
    pub buttons: RefCell<Vec<(Timeframe, gtk::ToggleButton)>>,
    /// The resolution beside the symbol, which opens every resolution: the
    /// one way to change it that fits on any chart, however narrow.
    pub timeframe_menu: gtk::MenuButton,
    timeframe_label: gtk::Label,
    /// One row per indicator, under the readout. Shown on the focused chart
    /// only, like the strip: four charts each listing their moving averages
    /// over the drawing is the noise the strip already learned to avoid.
    pub indicator_legend: gtk::Box,
    pub gear: gtk::Button,
    /// The drawings, the indicators and the bars, one glyph each after the
    /// header, shown only while that one is hidden: struck through, so a
    /// bare chart says why it is bare rather than looking like one whose
    /// data never came. A click brings it back.
    pub hidden_glyphs: Vec<(Hideable, gtk::Button)>,
    pub link: gtk::MenuButton,
    /// Fills the window with this chart, and puts it back. In the top right
    /// corner, out from under the legend, and only there while the pointer is
    /// on the chart: with four charts open, four of these drawn all the time
    /// is four more things between you and the prices.
    pub expand: gtk::Button,
    expand_icon: gtk::DrawingArea,
    /// Which way the expand glyph points, and which word its tooltip uses.
    expand_state: Rc<Cell<bool>>,
    pub instrument: RefCell<Option<Instrument>>,
    /// The symbol and venue this chart was asked for and the index could not
    /// name yet.
    ///
    /// A chart restored from a saved arrangement can name a listing the
    /// curated half of the inventory does not carry, and the long tail only
    /// arrives once the window is up. Remembering what was asked for is what
    /// lets the chart be written back down under that symbol rather than under
    /// nothing, and filled in once the catalogue lands. It is cleared the
    /// moment an instrument is set, so a chart the user has since changed is
    /// no longer waiting for anything.
    pub pending: RefCell<Option<(String, Option<String>)>>,
    pub timeframe: Cell<Timeframe>,
    pub indicators: RefCell<Vec<Indicator>>,
    /// The chart template whose indicators these are, if any. While it has
    /// one, `indicators` is a copy of the template's, kept current by the
    /// window; see [`crate::templates`].
    pub template: Cell<Option<u32>>,
    /// What the header shows, in order. See [`crate::header`].
    pub header: RefCell<Vec<crate::header::Part>>,
    /// The line the header is written on, and the two parts of it only the
    /// window can fill in.
    pub header_row: gtk::Box,
    pub change_label: gtk::Label,
    pub template_label: gtk::Label,
    /// The style its bars are drawn in when they are drawn: hiding them
    /// leaves it alone, so showing them again brings back the same one.
    pub bar_style: Cell<BarStyle>,
    pub bars_hidden: Cell<bool>,
    /// How the theme's candle colours are painted on this chart.
    pub colouring: Cell<omacharts_engine::BarColouring>,
    pub session: Cell<Session>,
    pub show_grid: Cell<bool>,
    /// What the rail does to this chart.
    mode: Cell<LinkMode>,
    /// Whether this chart shows the drawings other charts of its symbol
    /// send, and whether it sends its own. Both on by default: a drawing
    /// made on a symbol is usually a drawing about the symbol.
    pub shows_drawings: Cell<bool>,
    pub sends_drawings: Cell<bool>,
    /// What this chart is called among the drawings it has made.
    ///
    /// Not the pane's id, which is handed out afresh every time the window
    /// rebuilds the arrangement — a drawing that named one would belong to
    /// a different chart after the next split. Minted once and written down
    /// with the chart.
    pub uid: RefCell<String>,
    /// The drawings that are this chart's alone, kept with it rather than
    /// in the store, since they belong to no symbol but to this pane.
    pub local_drawings: RefCell<Vec<omacharts_engine::Drawing>>,
    /// What the link icon is painted in, so a mode other than the default can
    /// be read off four charts at a glance rather than by opening four
    /// popovers. Held here because the drawing happens on every frame and the
    /// theme it comes from does not.
    link_colour: Rc<RefCell<Option<String>>>,
    /// What the link button draws. Shared with the icon.
    glyph: Rc<Cell<LinkMode>>,
}

impl ChartPane {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u32,
        theme: Theme,
        scheme: omacharts_engine::theme::BarScheme,
        timeframe: Timeframe,
        indicators: Vec<Indicator>,
        bar_style: BarStyle,
        session: Session,
        show_grid: bool,
        mode: LinkMode,
    ) -> Rc<ChartPane> {
        let view = ChartView::new(theme, scheme);

        let symbol_button = gtk::Button::new();
        symbol_button.add_css_class("flat");
        symbol_button.add_css_class("legend-symbol");
        // The same weight the plain label had: it is still the title of the
        // chart, it just happens to be clickable.
        symbol_button.add_css_class("readout-symbol");
        symbol_button.set_valign(gtk::Align::Center);
        symbol_button.set_tooltip_text(Some(&shortcuts::tooltip("Find a symbol", "win.find")));

        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        strip.add_css_class("linked");
        strip.add_css_class("timeframe-strip");
        strip.set_valign(gtk::Align::Center);
        strip.set_visible(false);

        let timeframe_menu = gtk::MenuButton::new();
        timeframe_menu.add_css_class("flat");
        timeframe_menu.add_css_class("readout-symbol");
        timeframe_menu.set_valign(gtk::Align::Center);
        timeframe_menu.set_tooltip_text(Some("Resolution: click for the list"));
        // No chevron: a screenshot keeps this label because it is information,
        // and a chevron would put a control into every picture of a chart. The
        // flat button's hover is affordance enough, as it is for the symbol.
        // A label of our own rather than the button's, because a MenuButton
        // draws the arrow after a plain label whatever it is told, and only
        // listens when given a child.
        let timeframe_label = gtk::Label::new(None);
        timeframe_menu.set_child(Some(&timeframe_label));
        timeframe_menu.set_always_show_arrow(false);

        // Drawn rather than named. Adwaita's "insert-link" is a chain with a
        // downward arrow under it — it means *insert* a link, and the arrow
        // read as a dropdown nobody could open. There is no plain chain in the
        // theme, so here is one: two capsules and the bar that joins them,
        // painted in whatever colour the button currently has, which is what
        // makes it follow the theme and dim with the rest of the legend.
        let link_colour: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let glyph = Rc::new(Cell::new(mode));
        let link = gtk::MenuButton::new();
        link.set_child(Some(&mode_icon(link_colour.clone(), glyph.clone())));
        link.add_css_class("flat");
        link.add_css_class("legend-link");
        link.set_valign(gtk::Align::Center);
        // A MenuButton draws a chevron beside whatever it is given unless it
        // is told not to, and a chain with an arrow under it was the exact
        // thing the drawn icon exists to avoid.
        link.set_always_show_arrow(false);
        link.connect_active_notify(|link| match link.is_active() {
            true => link.add_css_class("open"),
            false => link.remove_css_class("open"),
        });
        set_link_look(&link, mode);
        // Hidden with the maximize corner until there is a second chart; the
        // window says when, through `set_expandable`.
        link.set_visible(mode == LinkMode::Pinned);

        let gear = gtk::Button::from_icon_name("emblem-system-symbolic");
        gear.add_css_class("flat");
        gear.add_css_class("legend-gear");
        gear.set_tooltip_text(Some(&shortcuts::tooltip("Chart settings", "chart.settings")));
        gear.set_valign(gtk::Align::Center);

        let hidden_glyphs = hidden_glyphs(&view);

        // The symbol and its resolution on the left, as the chart's title:
        // the resolution stays on every chart, focused or not, and in every
        // screenshot, because which bars these are is information. The
        // controls go to the opposite corner, where they stay out of it.
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        // The two parts a header may also show, written by the window when
        // the chart is showing them. Labels rather than buttons: there is
        // nothing to do to a day's move or a template's name from here.
        let change_label = gtk::Label::new(None);
        change_label.add_css_class("readout-symbol");
        change_label.set_valign(gtk::Align::Center);
        let template_label = gtk::Label::new(None);
        template_label.add_css_class("readout-symbol");
        template_label.add_css_class("dim-label");
        template_label.set_valign(gtk::Align::Center);
        // The padding the two buttons beside them have, so the four sit
        // evenly apart whichever order they are in.
        for label in [&change_label, &template_label] {
            label.set_margin_start(8);
            label.set_margin_end(8);
        }
        // As tall as the strip, so the symbol reads on the same line as the
        // resolutions. Alone it is shorter than the strip and sat above it.
        // A number rather than a size group, because the strip is hidden on
        // every chart but the focused one and the symbol would jump on focus.
        bar.set_height_request(SYMBOL_ROW_HEIGHT);

        let indicator_legend = gtk::Box::new(gtk::Orientation::Vertical, 0);
        indicator_legend.set_halign(gtk::Align::Start);
        indicator_legend.set_visible(false);

        let legend = gtk::Box::new(gtk::Orientation::Vertical, 0);
        legend.set_halign(gtk::Align::Start);
        legend.set_valign(gtk::Align::Start);
        legend.set_margin_start(LEGEND_MARGIN);
        legend.set_margin_top(6);
        legend.append(&bar);
        legend.append(&indicator_legend);

        // The strip sits across the top of its own chart rather than in the
        // legend: centred it reads as this chart's own toolbar, where in the
        // corner it was one more thing in a stack of names.
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        top.set_halign(gtk::Align::Center);
        top.set_valign(gtk::Align::Start);
        top.set_margin_top(6);
        top.append(&strip);

        // The opposite corner from the legend, and the only thing in it, so
        // that a chart which has grown to fill the window still shows the way
        // back without anything else being in the way.
        let expand_state = Rc::new(Cell::new(false));
        let expand_icon = expand_icon(expand_state.clone());
        let expand = gtk::Button::new();
        expand.set_child(Some(&expand_icon));
        expand.add_css_class("flat");
        expand.add_css_class("pane-expand");
        expand.set_valign(gtk::Align::Center);
        expand.set_tooltip_text(Some(&shortcuts::tooltip(MAXIMIZE, "chart.maximize")));
        // Nothing to maximize away from until there is a second chart.
        expand.set_visible(false);

        let corner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        corner.set_halign(gtk::Align::End);
        corner.set_valign(gtk::Align::Start);
        corner.set_margin_top(6);
        // The symbol's row's height, so the corner's buttons centre on the
        // line the symbol is written on rather than a few pixels above it.
        corner.set_height_request(SYMBOL_ROW_HEIGHT);
        corner.set_margin_end(corner_margin(0));
        corner.append(&gear);
        corner.append(&expand);
        // The link last, against the price axis: it is the one that stays up
        // at rest on a Follow or Pinned chart, and at the end of the row it
        // keeps its place when the gear and the maximize appear beside it.
        corner.append(&link);

        let overlay = gtk::Overlay::new();
        // The chart's own two layers, stacked, rather than the drawing area
        // alone: the crosshair is a widget over the bars now.
        overlay.set_child(Some(&view.root));
        overlay.add_overlay(&legend);
        overlay.add_overlay(&top);
        overlay.add_overlay(&corner);

        // The three are laid over the chart independently, so on a narrow
        // chart the centred strip lands on the symbol. It steps aside rather
        // than overlap, and the resolution beside the symbol offers the same
        // list. Decided
        // where the overlay places it, the one moment every width involved
        // is known.
        let (legend_box, legend_row, strip_row, corner_box) =
            (legend.clone(), bar.clone(), top.clone(), corner.clone());
        overlay.connect_get_child_position(move |overlay, child| {
            if child == strip_row.upcast_ref::<gtk::Widget>() {
                let natural = |w: &gtk::Widget| w.preferred_size().1.width();
                // Read here, not once: a chart on the window's left edge
                // steps its legend past the drawing toggle after it is built.
                let left = legend_box.margin_start() + natural(legend_row.upcast_ref());
                let right = natural(corner_box.upcast_ref()) + corner_box.margin_end();
                strip_row.set_child_visible(strip_fits(
                    overlay.width(),
                    left,
                    natural(strip_row.upcast_ref()),
                    right,
                ));
            }
            None
        });

        // A box rather than the overlay itself, so the focus ring is drawn on
        // something that is not also the drawing surface.
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("chart-pane");
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.append(&overlay);

        let pane = Rc::new(ChartPane {
            id,
            view,
            root,
            symbol_button,
            strip,
            buttons: RefCell::new(Vec::new()),
            timeframe_menu,
            timeframe_label,
            indicator_legend,
            gear,
            hidden_glyphs,
            link,
            expand,
            expand_icon,
            expand_state,
            instrument: RefCell::new(None),
            pending: RefCell::new(None),
            timeframe: Cell::new(timeframe),
            indicators: RefCell::new(indicators),
            template: Cell::new(None),
            header: RefCell::new(crate::header::standard()),
            header_row: bar,
            change_label,
            template_label,
            bar_style: Cell::new(bar_style),
            bars_hidden: Cell::new(false),
            colouring: Cell::new(Default::default()),
            session: Cell::new(session),
            show_grid: Cell::new(show_grid),
            mode: Cell::new(mode),
            shows_drawings: Cell::new(true),
            sends_drawings: Cell::new(true),
            uid: RefCell::new(gtk::glib::uuid_string_random().to_string()),
            local_drawings: RefCell::new(Vec::new()),
            link_colour,
            glyph,
        });
        pane.arrange_header();
        pane
    }

    /// Point this chart's strip, and the resolution beside its symbol, at the
    /// resolution it is showing.
    pub fn sync_strip(&self) {
        let current = self.timeframe.get();
        // Keep the keyboard with the resolution it is choosing. Clicking the
        // strip leaves the focus ring on the button clicked, and stepping with
        // the keys from there moved the pressed state along while the ring
        // stayed put — two answers to "which one is this", a button apart.
        // Only when the strip already has the keyboard: taking it from the
        // chart would send the arrow keys somewhere nobody pointed them.
        let chosen = {
            let buttons = self.buttons.borrow();
            let holds_keyboard = buttons.iter().any(|(_, button)| button.has_focus());
            for (listed, button) in buttons.iter() {
                button.set_active(*listed == current);
            }
            holds_keyboard
                .then(|| buttons.iter().find(|(listed, _)| *listed == current))
                .flatten()
                .map(|(_, button)| button.clone())
        };
        if let Some(button) = chosen {
            button.grab_focus();
        }
        self.write_timeframe();
    }

    /// Mark this one as the pane that keys and menus act on.
    ///
    /// A ring rather than anything louder: with four charts on screen the
    /// focused one has to be obvious at a glance and invisible the moment you
    /// stop looking for it.
    pub fn set_focused(&self, focused: bool) {
        if focused {
            self.root.add_css_class("focused");
        } else {
            self.root.remove_css_class("focused");
        }
        self.strip.set_visible(focused);
        self.indicator_legend.set_visible(focused);
    }

    pub fn mode(&self) -> LinkMode {
        self.mode.get()
    }

    /// Change what the rail does to this chart. The colour is resolved against
    /// the live theme by the window and handed in, because a pane has no theme
    /// of its own to ask once the user has changed it.
    pub fn set_mode(&self, mode: LinkMode, colour: Option<String>) {
        self.mode.set(mode);
        *self.link_colour.borrow_mut() = colour;
        self.glyph.set(mode);
        set_link_look(&self.link, mode);
        self.sync_link_shown();
        if let Some(icon) = self.link.child() {
            icon.queue_draw();
        }
    }

    /// Point at the pin, for a rail click this chart refused.
    ///
    /// A click that moves nothing reads as broken. Lighting up the one thing
    /// on screen that explains it says why, where the eye already is, and
    /// needs nothing added to the window to carry a message.
    ///
    /// What lights is the button's ground, a box where there was none, so the
    /// moment still shows to an eye that cannot tell the accent from the
    /// legend's grey.
    pub fn flag_pinned(&self) {
        self.link.add_css_class("refused");
        let link = self.link.clone();
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(700), move || {
            link.remove_css_class("refused");
        });
    }

    /// Push the maximize corner in from the right, to leave room for
    /// something else that wants the same corner.
    pub fn set_corner_clearance(&self, clearance: i32) {
        if let Some(corner) = self.expand.parent() {
            corner.set_margin_end(corner_margin(clearance));
        }
    }

    /// The same for the legend and the window's top-left corner, which
    /// holds the drawing tools' toggle and drops onto the top-left chart
    /// while the tools are put away. Every chart down the left edge takes
    /// the same step, so their names line up rather than only the top-left
    /// one moving.
    pub fn set_start_clearance(&self, clearance: i32) {
        if let Some(legend) = self.header_row.parent() {
            legend.set_margin_start(LEGEND_MARGIN + clearance);
        }
    }

    /// Offer the maximize corner at all, which is a question of whether
    /// there is anything else on screen to maximize away from. The link
    /// button goes with it: with one chart, Focus and Follow do the same
    /// thing, so there is nothing to choose between.
    pub fn set_expandable(&self, expandable: bool) {
        self.expand.set_visible(expandable);
        self.sync_link_shown();
    }

    fn sync_link_shown(&self) {
        // A lone Pinned chart keeps its pin, because a rail click it refuses
        // lights the pin, and a hidden one would leave the refusal unexplained.
        // `get_visible`, not `is_visible`: that one asks the whole ancestor
        // chain, which at launch is not on screen yet, so a chartbook opened
        // already split came up with no link button until the next split.
        self.link
            .set_visible(self.expand.get_visible() || self.mode.get() == LinkMode::Pinned);
    }

    /// Point the glyph in or out, and say which it is.
    pub fn set_maximized(&self, maximized: bool) {
        if self.expand_state.replace(maximized) != maximized {
            self.expand_icon.queue_draw();
        }
        let what = if maximized { RESTORE } else { MAXIMIZE };
        self.expand.set_tooltip_text(Some(&shortcuts::tooltip(what, "chart.maximize")));
    }

    pub fn label(&self) -> String {
        match self.instrument.borrow().as_ref() {
            Some(instrument) => format!(
                "{}  ·  {}",
                instrument.display_symbol(),
                self.timeframe.get().label()
            ),
            None => String::new(),
        }
    }

    /// Put the header's parts on its line in the chart's order, and take off
    /// the ones it does not show.
    pub fn arrange_header(&self) {
        let parts: [(crate::header::Part, gtk::Widget); 4] = [
            (crate::header::Part::Symbol, self.symbol_button.clone().upcast()),
            (crate::header::Part::Resolution, self.timeframe_menu.clone().upcast()),
            (crate::header::Part::Change, self.change_label.clone().upcast()),
            (crate::header::Part::Template, self.template_label.clone().upcast()),
        ];
        for (_, widget) in &parts {
            if widget.parent().is_some() {
                self.header_row.remove(widget);
            }
        }
        for part in self.header.borrow().iter() {
            if let Some((_, widget)) = parts.iter().find(|(p, _)| p == part) {
                self.header_row.append(widget);
            }
        }
        // Last, whatever the header shows: it says what is missing from this
        // chart, not what it is.
        for (_, button) in &self.hidden_glyphs {
            if button.parent().is_some() {
                self.header_row.remove(button);
            }
            self.header_row.append(button);
        }
    }

    /// Whether the header has `part` in it, so the window only works out what
    /// it is asked to show.
    pub fn shows(&self, part: crate::header::Part) -> bool {
        self.header.borrow().contains(&part)
    }

    /// The template's name, or nothing at all on a chart with none.
    pub fn write_template(&self, name: Option<&str>) {
        self.template_label.set_text(name.unwrap_or(""));
        self.template_label.set_visible(name.is_some());
    }

    /// The day's move, in `colour`; nothing at all until there is one.
    pub fn write_change(&self, change: Option<(f64, &str)>) {
        match change {
            Some((pct, colour)) => {
                let text = gtk::glib::markup_escape_text(&crate::header::change_text(pct));
                let colour = gtk::glib::markup_escape_text(colour);
                self.change_label.set_markup(&format!("<span foreground=\"{colour}\">{text}</span>"));
                self.change_label.set_visible(true);
            }
            None => {
                self.change_label.set_text("");
                self.change_label.set_visible(false);
            }
        }
    }

    pub fn write_readout(&self) {
        let symbol = self
            .instrument
            .borrow()
            .as_ref()
            .map(|i| i.display_symbol())
            .unwrap_or_default();
        self.symbol_button.set_label(&symbol);
        self.write_timeframe();
    }

    /// The resolution, beside the symbol.
    ///
    /// Written from the strip as well as from the readout because the
    /// resolution changes on its own, with the symbol doing nothing.
    fn write_timeframe(&self) {
        self.timeframe_label.set_text(&self.timeframe.get().label());
    }
}

/// What the legend can say a chart is hiding, in the order of their keys:
/// Ctrl+H, with Shift, with Alt as well.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hideable {
    Drawings,
    Indicators,
    Bars,
}

impl Hideable {
    const ALL: [Hideable; 3] = [Hideable::Drawings, Hideable::Indicators, Hideable::Bars];

    fn hidden(self, hidden: Hidden) -> bool {
        match self {
            Hideable::Drawings => hidden.drawings,
            Hideable::Indicators => hidden.indicators,
            Hideable::Bars => hidden.bars,
        }
    }

    fn tooltip(self) -> String {
        let (text, action) = match self {
            Hideable::Drawings => ("Drawings hidden: show them", "chart.show-drawings"),
            Hideable::Indicators => ("Indicators hidden: show them", "chart.show-indicators"),
            Hideable::Bars => ("Bars hidden: show them", "chart.bars"),
        };
        shortcuts::tooltip(text, action)
    }

    /// The glyph, before it is struck through:
    ///
    /// - drawings: a pencil. A line, which is what a drawing most often is,
    ///   is lost among the chart's own lines at this size;
    /// - indicators: a wave, the moving average every chart starts with;
    /// - bars: two candles, one filled and one hollow.
    ///
    /// On a 16-unit grid. Anything long lies across the slash, not along it.
    fn shape(self, cr: &gtk::cairo::Context) {
        match self {
            Hideable::Drawings => {
                // Eraser end top-left, point bottom-right.
                let at = |t: f64, o: f64| {
                    let d = std::f64::consts::FRAC_1_SQRT_2;
                    (3.0 + d * t + d * o, 3.0 + d * t - d * o)
                };
                let (half, cone, tip) = (1.6, 10.8, 14.6);
                for (t, o) in [(0.0, -half), (cone, -half), (tip, 0.0), (cone, half), (0.0, half)] {
                    let (x, y) = at(t, o);
                    cr.line_to(x, y);
                }
                cr.close_path();
                cr.set_line_width(0.6);
                let _ = cr.fill_preserve();
                let _ = cr.stroke();
            }
            Hideable::Indicators => {
                cr.set_line_width(1.4);
                cr.move_to(1.8, 10.5);
                cr.curve_to(4.5, 3.0, 6.8, 3.0, 8.0, 8.0);
                cr.curve_to(9.2, 13.0, 11.5, 13.0, 14.2, 5.5);
                let _ = cr.stroke();
            }
            Hideable::Bars => {
                // The hollow candle's wick starts below the slash, so the
                // gap leaves no sliver of it beyond.
                cr.set_line_width(1.2);
                cr.move_to(5.0, 2.0);
                cr.line_to(5.0, 14.0);
                cr.move_to(11.0, 5.0);
                cr.line_to(11.0, 12.5);
                let _ = cr.stroke();
                cr.rectangle(3.0, 4.5, 4.0, 6.5);
                let _ = cr.fill();
                // Hollow: the body is cleared first, so the wick does not
                // run through it.
                cr.set_operator(gtk::cairo::Operator::Clear);
                cr.rectangle(9.1, 6.0, 3.8, 4.5);
                let _ = cr.fill();
                cr.set_operator(gtk::cairo::Operator::Over);
                cr.rectangle(9.1, 6.0, 3.8, 4.5);
                let _ = cr.stroke();
            }
        }
    }
}

/// One glyph for each thing the chart can hide, shown only while it is
/// hidden. A click brings it back.
fn hidden_glyphs(view: &ChartView) -> Vec<(Hideable, gtk::Button)> {
    let glyphs: Vec<(Hideable, gtk::Button)> = Hideable::ALL
        .iter()
        .map(|&what| {
            let button = gtk::Button::new();
            button.set_child(Some(&struck_icon(what)));
            button.add_css_class("flat");
            button.add_css_class("legend-hidden");
            button.set_valign(gtk::Align::Center);
            button.set_tooltip_text(Some(&what.tooltip()));
            (what, button)
        })
        .collect();
    let buttons = glyphs.clone();
    view.set_hidden_handler(move |hidden| {
        for (what, button) in &buttons {
            button.set_visible(what.hidden(hidden));
        }
    });
    glyphs
}

/// A glyph struck through.
///
/// The glyph keeps full ink and the slash runs bottom-left to top-right
/// through a gap it cuts first, the way an "eye-off" icon does, so the slash
/// reads as a slash rather than one more line of the shape. Kept thin: three
/// side by side should read as three shapes, not three crosses. Drawn on a
/// 16-unit grid, scaled to the area.
fn struck_icon(what: Hideable) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_draw_func(move |area, cr, width, height| {
        let colour = area.color();
        cr.set_source_rgba(
            colour.red() as f64,
            colour.green() as f64,
            colour.blue() as f64,
            colour.alpha() as f64,
        );
        cr.push_group();
        // One scale for both axes, centred: the button can hand the area
        // more height than width, and scaling each on its own stretches
        // the glyph upright.
        let (w, h) = (width as f64, height as f64);
        // A tenth over the 16 the grid is drawn on, beside the symbol's text.
        let size = w.min(h).min(17.6);
        cr.translate((w - size) / 2.0, (h - size) / 2.0);
        cr.scale(size / 16.0, size / 16.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        what.shape(cr);
        let (a, b) = ((2.0, 14.0), (14.0, 2.0));
        cr.set_operator(gtk::cairo::Operator::Clear);
        cr.set_line_width(3.0);
        cr.move_to(a.0, a.1);
        cr.line_to(b.0, b.1);
        let _ = cr.stroke();
        cr.set_operator(gtk::cairo::Operator::Over);
        cr.set_line_width(1.1);
        cr.move_to(a.0, a.1);
        cr.line_to(b.0, b.1);
        let _ = cr.stroke();
        let _ = cr.pop_group_to_source();
        let _ = cr.paint();
    });
    area
}

/// How far the legend sits in from the chart's left edge. The corner on the
/// right keeps the same gap past the window's controls, so the two corners
/// are a pair.
const LEGEND_MARGIN: i32 = 12;
/// The strip's height: its buttons plus the padding around them.
const SYMBOL_ROW_HEIGHT: i32 = 32;

/// The corner's margin with `clearance` to leave. With none, it still
/// clears the price axis: a Follow or Pinned chart's link is always on
/// screen, and over the axis it sat on its labels.
///
/// Past the window's controls, which are wider than the axis, it leaves a
/// few pixels more than the legend's gap. The two pairs stand in one row,
/// and at the plain gap the space between them was not much wider than the
/// space inside the window's pair, so the row read as four buttons rather
/// than two groups of two.
fn corner_margin(clearance: i32) -> i32 {
    match clearance {
        0 => crate::ui::chart::PRICE_AXIS_W as i32 + 6,
        _ => clearance + LEGEND_MARGIN + 4,
    }
}

/// Whether a strip `strip` wide, centred in `width`, clears what sits at
/// either edge — `left` and `right` of it — with a little air on each side.
fn strip_fits(width: i32, left: i32, strip: i32, right: i32) -> bool {
    const GAP: i32 = 8;
    let start = (width - strip) / 2;
    start >= left + GAP && start + strip <= width - right - GAP
}

/// Two arrows on a diagonal, pointing away from each other — and, once the
/// chart has the window to itself, back towards each other.
///
/// The same glyph every tiling window manager and every video player uses for
/// this, which is the point: it has to be readable at eighteen pixels without
/// a label, and the shape people already know is the one that is. Drawn rather
/// than named for the same reason the chain is: Adwaita's nearest icons are
/// "view-fullscreen", which is a frame with corner brackets and reads as a
/// crop tool at this size, and "zoom-fit-best", which reads as a magnifier.
fn expand_icon(maximized: Rc<Cell<bool>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_draw_func(move |area, cr, width, height| {
        let colour = area.color();
        cr.set_source_rgba(
            colour.red() as f64,
            colour.green() as f64,
            colour.blue() as f64,
            colour.alpha() as f64,
        );
        let (w, h) = (width as f64, height as f64);
        cr.set_line_width(1.3);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);

        let (cx, cy) = (w / 2.0, h / 2.0);

        // An arrowhead on a diagonal: the two legs are the pointing direction
        // turned a quarter either way, which on a diagonal lands them square
        // on the axes and keeps them crisp at this size.
        let head_at = |tx: f64, ty: f64, dx: f64, dy: f64, head: f64| {
            cr.move_to(tx - dx * head, ty);
            cr.line_to(tx, ty);
            cr.line_to(tx, ty - dy * head);
        };

        if maximized.get() {
            // Coming back in: two arrows from the corners towards the middle,
            // stopping short of each other so the pair reads as two arrows
            // and not as one line with a knot in it. They start nearer the
            // corners than the other glyph's do, because an arrow needs a
            // shaft behind its head to be an arrow at eighteen pixels.
            let head = w * 0.22;
            let (near, far) = (w * 0.1, w - w * 0.1);
            let gap = w * 0.11;
            cr.move_to(far, near);
            cr.line_to(cx + gap, cy - gap);
            cr.move_to(near, far);
            cr.line_to(cx - gap, cy + gap);
            let _ = cr.stroke();
            head_at(cx + gap, cy - gap, -1.0, 1.0, head);
            head_at(cx - gap, cy + gap, 1.0, -1.0, head);
        } else {
            // Going out: one shaft corner to corner, pointed at both ends.
            let head = w * 0.26;
            let (near, far) = (w * 0.2, w - w * 0.2);
            cr.move_to(near, far);
            cr.line_to(far, near);
            let _ = cr.stroke();
            head_at(far, near, 1.0, -1.0, head);
            head_at(near, far, -1.0, 1.0, head);
        }
        let _ = cr.stroke();
    });
    area
}

/// A mode's icon for a row of the mode menu, in the colour the chart's button
/// takes in that mode: the legend's own, dimmed, for the default, Focus, and
/// the accent for the other two. So the menu shows what the button will look like.
///
/// Coloured from CSS rather than handed a hex, because the popover outlives
/// any one theme and CSS is what follows a change of it.
pub(crate) fn menu_mode_icon(mode: LinkMode) -> gtk::DrawingArea {
    let area = mode_icon(
        Rc::new(RefCell::new(None)),
        Rc::new(Cell::new(mode)),
    );
    area.add_css_class(match mode {
        LinkMode::Focus => "dim-label",
        LinkMode::Follow | LinkMode::Pinned => "link-mode-set",
    });
    area
}

/// The chart's link button, which has to say which of three modes it is in
/// without being opened, so each mode has a shape of its own: one link for
/// Focus, the chain for Follow, a pin for Pinned. One link and two say how
/// much the chart is tied to the watchlist, only while focused or always;
/// "never moves" is not a fainter "always moves", so it is not a chain at all.
fn mode_icon(tint: Rc<RefCell<Option<String>>>, glyph: Rc<Cell<LinkMode>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(18);
    area.set_content_height(18);
    area.set_draw_func(move |area, cr, width, height| {
        // The mode's colour when it has one, and the legend's own otherwise:
        // the default is furniture and should dim with everything else
        // around it rather than insisting on a hue of its own.
        match tint.borrow().as_deref() {
            Some(hex) => crate::ui::colors::set_source(cr, hex),
            None => {
                let colour = area.color();
                cr.set_source_rgba(
                    colour.red() as f64,
                    colour.green() as f64,
                    colour.blue() as f64,
                    colour.alpha() as f64,
                );
            }
        }
        let (w, h) = (width as f64, height as f64);
        cr.set_line_width(1.3);
        cr.save().ok();
        cr.translate(w / 2.0, h / 2.0);
        match glyph.get() {
            LinkMode::Focus => draw_link(cr, w, h),
            LinkMode::Follow => draw_chain(cr, w, h),
            LinkMode::Pinned => draw_pin(cr, w),
        }
        cr.restore().ok();
    });
    area
}

/// The size of one link of the chain, for a box `w` by `h`.
fn link_size(w: f64, h: f64) -> (f64, f64) {
    (w * 0.56, h * 0.36)
}

/// One rounded link on the diagonal, around the origin: the chain with its
/// other half missing, the same size and angle so the two read as one family.
fn draw_link(cr: &gtk::cairo::Context, w: f64, h: f64) {
    let (link_w, link_h) = link_size(w, h);
    cr.rotate(-std::f64::consts::FRAC_PI_4);
    rounded(cr, -link_w / 2.0, -link_h / 2.0, link_w, link_h, link_h / 2.0);
    let _ = cr.stroke();
}

/// Two rounded links, overlapping, on the diagonal, around the origin.
///
/// Flat and side by side they read as a Venn diagram — two ovals that happen
/// to overlap. On the diagonal the same two shapes read as a chain, which is
/// why every chain icon is drawn that way, and it survives being shrunk to
/// sixteen pixels where the flat version does not.
fn draw_chain(cr: &gtk::cairo::Context, w: f64, h: f64) {
    let (link_w, link_h) = link_size(w, h);
    let overlap = link_h * 0.34;
    cr.rotate(-std::f64::consts::FRAC_PI_4);
    let total = link_w * 2.0 - overlap;
    for x in [-total / 2.0, -total / 2.0 + link_w - overlap] {
        rounded(cr, x, -link_h / 2.0, link_w, link_h, link_h / 2.0);
        let _ = cr.stroke();
    }
}

/// A thumbtack, leaning the way the chain does, around the origin: the head,
/// the waist and the flared collar as one solid shape, over a bare point.
fn draw_pin(cr: &gtk::cairo::Context, w: f64) {
    cr.rotate(std::f64::consts::FRAC_PI_4);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    // Drawn on an 18-pixel box and scaled to this one.
    let k = w / 18.0;
    let outline = [
        (-2.6, -7.6),
        (2.6, -7.6),
        (2.6, -5.8),
        (1.4, -5.8),
        (1.4, -2.4),
        (4.0, -1.0),
        (-4.0, -1.0),
        (-1.4, -2.4),
        (-1.4, -5.8),
        (-2.6, -5.8),
    ];
    for (i, &(x, y)) in outline.iter().enumerate() {
        match i {
            0 => cr.move_to(x * k, y * k),
            _ => cr.line_to(x * k, y * k),
        }
    }
    cr.close_path();
    let _ = cr.fill_preserve();
    let _ = cr.stroke();
    cr.move_to(0.0, -k);
    cr.line_to(0.0, 7.6 * k);
    let _ = cr.stroke();
}

fn rounded(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(x + r, y + h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
    cr.arc(x + r, y + r, r, std::f64::consts::PI, 1.5 * std::f64::consts::PI);
    cr.close_path();
}

/// The legend stays out of the way, so a Focus chart's button is nearly
/// invisible: it is the default, and four faint links say nothing is going on.
/// Follow and Pinned are not: a chart that moves on every click, or on none,
/// is the state worth being able to see across four charts without looking.
fn set_link_look(link: &gtk::MenuButton, mode: LinkMode) {
    link.set_tooltip_text(Some(&format!("{} — {}", mode.label(), mode.blurb())));
    match mode == LinkMode::default() {
        true => link.add_css_class("quiet"),
        false => link.remove_css_class("quiet"),
    }
}

/// How the panes are arranged.
///
/// A binary tree, the way a tiling window manager keeps one: splitting divides
/// the focused pane in two, and closing a pane hands its space back to its
/// sibling by collapsing the split above them. Nothing else moves.
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    Leaf(u32),
    Split {
        horizontal: bool,
        /// Where the divider sits, as a share of the split's length. Stored
        /// with the shape because it is part of the arrangement: a layout that
        /// comes back with every divider centred is not the layout you left.
        #[serde(default = "half")]
        ratio: f64,
        first: Box<Node>,
        second: Box<Node>,
    },
}

fn half() -> f64 {
    0.5
}

/// How close to an edge a divider may be pushed.
///
/// The same bounds a dragged handle is held to, because a key that can park a
/// divider somewhere the mouse cannot would make the two ways of moving it
/// disagree about where the ends are. Past this a pane has no chart left in
/// it, only its own chrome.
const DIVIDER_MIN: f64 = 0.05;
const DIVIDER_MAX: f64 = 0.95;

/// How much of a split one press of a resize key moves its divider.
///
/// A fiftieth, which is some thirty pixels of a full-width window: large
/// enough to see a single press land, small enough to stop on the ratio you
/// meant. Held down at the usual repeat rate it crosses the whole range in
/// well over a second, so the divider travels rather than jumping to the end.
const RESIZE_STEP: f64 = 0.02;

/// What a resize key asks for: this chart wider, narrower, taller or shorter.
///
/// Named by what happens to the chart rather than by the arrow that was
/// pressed, because the two only agree half the time. Right makes the focused
/// chart wider whichever side of its divider it sits on, so the divider goes
/// right for a chart in the first half and *left* for one in the second — a
/// `Right` variant would be naming the key in one case and the divider in the
/// other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resize {
    Left,
    Right,
    Up,
    Down,
}

impl Resize {
    /// Whether it is a left-right divider that has to move.
    fn horizontal(self) -> bool {
        matches!(self, Resize::Left | Resize::Right)
    }

    /// How far the divider goes, and which way.
    fn delta(self) -> f64 {
        match self {
            Resize::Right | Resize::Down => RESIZE_STEP,
            Resize::Left | Resize::Up => -RESIZE_STEP,
        }
    }
}

impl Node {
    pub fn leaf(id: u32) -> Node {
        Node::Leaf(id)
    }

    /// Replace the leaf `id` with a split holding it and `added`.
    pub fn split(&self, id: u32, added: u32, horizontal: bool) -> Node {
        match self {
            Node::Leaf(leaf) if *leaf == id => Node::Split {
                horizontal,
                ratio: 0.5,
                first: Box::new(Node::Leaf(id)),
                second: Box::new(Node::Leaf(added)),
            },
            Node::Leaf(leaf) => Node::Leaf(*leaf),
            Node::Split { horizontal: h, ratio, first, second } => Node::Split {
                horizontal: *h,
                ratio: *ratio,
                first: Box::new(first.split(id, added, horizontal)),
                second: Box::new(second.split(id, added, horizontal)),
            },
        }
    }

    /// Take `id` out, collapsing the split that held it into its sibling.
    /// `None` when `id` was the only leaf, which the caller refuses to do.
    pub fn remove(&self, id: u32) -> Option<Node> {
        match self {
            Node::Leaf(leaf) => (*leaf != id).then_some(Node::Leaf(*leaf)),
            Node::Split { horizontal, ratio, first, second } => {
                match (first.remove(id), second.remove(id)) {
                    (Some(a), Some(b)) => Some(Node::Split {
                        horizontal: *horizontal,
                        ratio: *ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    /// Move the divider of the split at `path`, where each step says which
    /// half to descend into.
    pub fn with_ratio(&self, path: &[bool], ratio: f64) -> Node {
        match self {
            Node::Leaf(id) => Node::Leaf(*id),
            Node::Split { horizontal, ratio: current, first, second } => {
                let (ratio, first, second) = match path.split_first() {
                    None => (ratio.clamp(DIVIDER_MIN, DIVIDER_MAX), first.clone(), second.clone()),
                    Some((true, rest)) => (
                        *current,
                        Box::new(first.with_ratio(rest, ratio)),
                        second.clone(),
                    ),
                    Some((false, rest)) => (
                        *current,
                        first.clone(),
                        Box::new(second.with_ratio(rest, ratio)),
                    ),
                };
                Node::Split { horizontal: *horizontal, ratio, first, second }
            }
        }
    }

    /// Which halves to descend into to reach the pane `id`, which is also the
    /// path of every divider standing over it.
    fn path_to(&self, id: u32) -> Option<Vec<bool>> {
        match self {
            Node::Leaf(leaf) => (*leaf == id).then(Vec::new),
            Node::Split { first, second, .. } => {
                let (half, mut rest) = match (first.path_to(id), second.path_to(id)) {
                    (Some(rest), _) => (true, rest),
                    (None, Some(rest)) => (false, rest),
                    (None, None) => return None,
                };
                rest.insert(0, half);
                Some(rest)
            }
        }
    }

    /// The divider that resizes the pane `id`: the path to it, and where it
    /// has to go. Feed both back to `with_ratio`, or to the handle on screen.
    ///
    /// The one that moves is the nearest divider *of the matching
    /// orientation*, not the nearest divider. In a two-by-two every pane has
    /// both a row divider and a column divider standing over it and the closer
    /// of the two is the wrong one for two of the four keys — which looks like
    /// the right pane resizing along the wrong axis.
    ///
    /// `None` when there is nothing to move: one chart on its own, or a chart
    /// in a column asked to be wider. That is a key that does nothing, not an
    /// error and not a wrap onto some other divider, because no other divider
    /// is the answer to the question that was asked.
    pub fn resize(&self, id: u32, how: Resize) -> Option<(Vec<bool>, f64)> {
        let path = self.path_to(id)?;
        let mut node = self;
        let mut found: Option<(Vec<bool>, f64, bool)> = None;
        for (depth, half) in path.iter().enumerate() {
            let Node::Split { horizontal, ratio, first, second } = node else { break };
            if *horizontal == how.horizontal() {
                found = Some((path[..depth].to_vec(), *ratio, *half));
            }
            node = if *half { first } else { second };
        }
        let (divider, ratio, _) = found?;
        // The arrow moves the boundary, not the chart. A chart in the second
        // half therefore shrinks when the key points away from it, which is
        // what every tiling window manager does and what the key looks like it
        // should do — growing whichever chart has the focus means the same
        // arrow moves the divider opposite ways depending on which side you
        // happen to be sitting on, and in the bottom-right pane of a four-way
        // split all four keys then feel backwards.
        Some((divider, (ratio + how.delta()).clamp(DIVIDER_MIN, DIVIDER_MAX)))
    }

    /// Point every leaf at its new id, in one pass.
    ///
    /// One pass is the whole point. Renaming one id at a time looks equivalent
    /// and is not: once the ids being handed out overlap the ids already in the
    /// tree, each rename can catch a leaf an earlier rename just wrote, and the
    /// renames chase each other down the tree. 1→2 then 2→3 then 3→4 leaves
    /// three different panes all called 4, and an arrangement whose every
    /// quadrant names one chart is an arrangement that cannot be drawn.
    pub fn relabel(&self, ids: &HashMap<u32, u32>) -> Node {
        match self {
            Node::Leaf(id) => Node::Leaf(ids.get(id).copied().unwrap_or(*id)),
            Node::Split { horizontal, ratio, first, second } => Node::Split {
                horizontal: *horizontal,
                ratio: *ratio,
                first: Box::new(first.relabel(ids)),
                second: Box::new(second.relabel(ids)),
            },
        }
    }

    /// The same arrangement with any leaf that appears twice taken out, the
    /// first of each kept where it is.
    ///
    /// `None` when nothing is left, which cannot happen for a tree that has a
    /// leaf at all. A pane can only be in one place, so a repeated leaf is not
    /// a layout with an odd shape — it is a layout that was written down
    /// wrongly, and mounting it would put one chart's widget into two parents
    /// and silently lose whichever came second.
    pub fn deduped(&self) -> Option<Node> {
        self.pruned(&mut HashSet::new())
    }

    fn pruned(&self, seen: &mut HashSet<u32>) -> Option<Node> {
        match self {
            Node::Leaf(id) => seen.insert(*id).then_some(Node::Leaf(*id)),
            Node::Split { horizontal, ratio, first, second } => {
                match (first.pruned(seen), second.pruned(seen)) {
                    (Some(a), Some(b)) => Some(Node::Split {
                        horizontal: *horizontal,
                        ratio: *ratio,
                        first: Box::new(a),
                        second: Box::new(b),
                    }),
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
        }
    }

    /// Every pane, left to right and top to bottom — which is the order the
    /// keyboard walks them in.
    pub fn leaves(&self) -> Vec<u32> {
        match self {
            Node::Leaf(id) => vec![*id],
            Node::Split { first, second, .. } => {
                let mut out = first.leaves();
                out.extend(second.leaves());
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_shows_only_where_it_clears_both_corners() {
        // A wide chart: room on either side.
        assert!(strip_fits(1200, 160, 300, 40));
        // Narrow: centred, it would start on top of the symbol.
        assert!(!strip_fits(500, 160, 300, 40));
        // Or end under the maximize corner, when that is the wider of the two.
        assert!(!strip_fits(600, 40, 300, 160));
    }

    #[test]
    fn splitting_replaces_the_leaf_with_a_pair() {
        let layout = Node::leaf(1).split(1, 2, true);
        assert_eq!(layout.leaves(), vec![1, 2]);
        assert!(matches!(layout, Node::Split { horizontal: true, .. }));
    }

    #[test]
    fn splitting_only_touches_the_pane_asked_for() {
        let layout = Node::leaf(1).split(1, 2, true).split(2, 3, false);
        assert_eq!(layout.leaves(), vec![1, 2, 3]);
    }

    #[test]
    fn removing_gives_the_space_to_the_sibling() {
        // Two panes side by side: closing one leaves the other alone at the
        // top of the tree, holding everything.
        let layout = Node::leaf(1).split(1, 2, true);
        assert_eq!(layout.remove(2), Some(Node::Leaf(1)));
        assert_eq!(layout.remove(1), Some(Node::Leaf(2)));
    }

    #[test]
    fn removing_collapses_only_the_split_that_held_it() {
        let layout = Node::leaf(1).split(1, 2, true).split(2, 3, false);
        let left = layout.remove(3).unwrap();
        assert_eq!(left.leaves(), vec![1, 2]);
        assert!(matches!(left, Node::Split { horizontal: true, .. }));
    }

    #[test]
    fn the_last_pane_cannot_be_removed() {
        assert_eq!(Node::leaf(1).remove(1), None);
    }

    /// Four panes from two splits of a split, the way tmux does it: every
    /// leaf is splittable, including ones that came from a split.
    #[test]
    fn splitting_a_split_pane_gives_four() {
        let layout = Node::leaf(1)
            .split(1, 2, true)
            .split(1, 3, false)
            .split(2, 4, false);
        assert_eq!(layout.leaves().len(), 4);
        // And the shape is a pair of columns, each divided in two.
        let Node::Split { horizontal, first, second, .. } = &layout else { panic!("{layout:?}") };
        assert!(*horizontal);
        assert!(matches!(**first, Node::Split { horizontal: false, .. }));
        assert!(matches!(**second, Node::Split { horizontal: false, .. }));
    }

    /// Closing one of four leaves three, and only the split that held it
    /// collapses — the other column keeps its own division.
    #[test]
    fn closing_one_of_four_leaves_the_rest_alone() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        let left = layout.remove(3).unwrap();
        assert_eq!(left.leaves(), vec![1, 2, 4]);
        let Node::Split { first, second, .. } = &left else { panic!("{left:?}") };
        assert_eq!(**first, Node::Leaf(1));
        assert!(matches!(**second, Node::Split { .. }));
    }

    /// The arrangement this bug destroyed: four panes in two columns, restored
    /// into ids that overlap the ones it was saved with.
    ///
    /// Renaming one id at a time turned all four leaves into the same pane —
    /// 1→2 caught the leaf already called 2, which 2→3 then caught again — and
    /// a window cannot draw one chart in four places, so three quarters of it
    /// came back empty.
    #[test]
    fn relabelling_into_ids_that_overlap_the_old_ones_keeps_the_panes_apart() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        let ids = HashMap::from([(1, 2), (2, 3), (3, 4), (4, 5)]);

        let moved = layout.relabel(&ids);
        assert_eq!(moved.leaves(), vec![2, 4, 3, 5], "every pane keeps its own id");

        // And the shape is the one that was saved: two columns, each divided.
        let Node::Split { horizontal, first, second, .. } = &moved else { panic!("{moved:?}") };
        assert!(*horizontal);
        assert!(matches!(**first, Node::Split { horizontal: false, .. }));
        assert!(matches!(**second, Node::Split { horizontal: false, .. }));
    }

    #[test]
    fn relabelling_leaves_an_id_nobody_renamed_where_it_was() {
        let layout = Node::leaf(1).split(1, 2, true);
        let moved = layout.relabel(&HashMap::from([(2, 9)]));
        assert_eq!(moved.leaves(), vec![1, 9]);
    }

    /// A pane can only be in one place, so a tree naming one twice was written
    /// down wrongly. Mounting it would put a single chart's widget into two
    /// parents, and GTK would refuse the second and leave a hole.
    #[test]
    fn a_pane_named_twice_is_kept_only_where_it_first_appears() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        let collapsed = layout.relabel(&HashMap::from([(1, 7), (2, 7), (3, 7), (4, 7)]));
        assert_eq!(collapsed.leaves(), vec![7, 7, 7, 7], "the shape this used to produce");
        assert_eq!(collapsed.deduped(), Some(Node::Leaf(7)));
    }

    #[test]
    fn deduping_a_tree_that_names_each_pane_once_changes_nothing() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        assert_eq!(layout.deduped(), Some(layout.clone()));
    }

    /// A dragged divider is part of the arrangement, and has to survive both
    /// a restart and anything else happening to the tree.
    #[test]
    fn a_dragged_divider_is_remembered() {
        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 0.7);
        let Node::Split { ratio, .. } = &layout else { panic!("{layout:?}") };
        assert!((ratio - 0.7).abs() < 1e-9, "{ratio}");

        // Splitting one half leaves the outer divider where it was.
        let deeper = layout.split(2, 3, false);
        let Node::Split { ratio, .. } = &deeper else { panic!("{deeper:?}") };
        assert!((ratio - 0.7).abs() < 1e-9, "outer divider moved: {ratio}");

        // And so does closing a pane in the other half.
        let closed = deeper.remove(3).unwrap();
        let Node::Split { ratio, .. } = &closed else { panic!("{closed:?}") };
        assert!((ratio - 0.7).abs() < 1e-9, "outer divider moved on close: {ratio}");
    }

    #[test]
    fn an_inner_divider_moves_without_disturbing_the_outer_one() {
        let layout = Node::leaf(1)
            .split(1, 2, true)
            .with_ratio(&[], 0.3)
            .split(2, 3, false)
            .with_ratio(&[false], 0.8);
        let Node::Split { ratio, second, .. } = &layout else { panic!() };
        assert!((ratio - 0.3).abs() < 1e-9, "outer: {ratio}");
        let Node::Split { ratio, .. } = &**second else { panic!() };
        assert!((ratio - 0.8).abs() < 1e-9, "inner: {ratio}");
    }

    #[test]
    fn a_divider_cannot_be_pushed_off_the_end() {
        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 9.0);
        let Node::Split { ratio, .. } = &layout else { panic!() };
        assert!(*ratio <= 0.95 && *ratio >= 0.05, "{ratio}");
    }

    #[test]
    fn leaves_come_back_in_layout_order() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);
        assert_eq!(layout.leaves(), vec![1, 3, 2, 4]);
    }

    /// The key moves the boundary, so it moves the same way whichever side of
    /// it asked. Which chart grows is then a consequence of where you were
    /// sitting, and never a surprise about which way the key points.
    #[test]
    fn a_divider_goes_the_way_the_key_points_from_either_side_of_it() {
        let layout = Node::leaf(1).split(1, 2, true);

        let (divider, ratio) = layout.resize(1, Resize::Right).unwrap();
        assert_eq!(divider, Vec::<bool>::new());
        assert!((ratio - 0.52).abs() < 1e-9, "the left pane gains: {ratio}");

        let (divider, ratio) = layout.resize(2, Resize::Right).unwrap();
        assert_eq!(divider, Vec::<bool>::new());
        assert!((ratio - 0.52).abs() < 1e-9, "same divider, same way: {ratio}");

        let (_, ratio) = layout.resize(1, Resize::Left).unwrap();
        assert!((ratio - 0.48).abs() < 1e-9, "{ratio}");
        let (_, ratio) = layout.resize(2, Resize::Left).unwrap();
        assert!((ratio - 0.48).abs() < 1e-9, "{ratio}");
    }

    /// Two columns, each divided: every pane has a column divider and a row
    /// divider over it, and the nearer of the two is the row. Pressing
    /// sideways has to reach past it to the column divider at the root.
    #[test]
    fn a_two_by_two_resizes_on_the_divider_of_the_orientation_asked_for() {
        let layout = Node::leaf(1).split(1, 2, true).split(1, 3, false).split(2, 4, false);

        // Top left: the root divider across, its own column's divider down.
        assert_eq!(layout.resize(1, Resize::Right), Some((vec![], 0.52)));
        assert_eq!(layout.resize(1, Resize::Down), Some((vec![true], 0.52)));

        // Bottom right reaches the same two orientations on different
        // dividers — the root across, its own column's down.
        assert_eq!(layout.resize(4, Resize::Right), Some((vec![], 0.52)));
        assert_eq!(layout.resize(4, Resize::Down), Some((vec![false], 0.52)));

        // And every one of them answers the key, not the pane's side of it.
        assert_eq!(layout.resize(3, Resize::Right), Some((vec![], 0.52)));
        assert_eq!(layout.resize(3, Resize::Up), Some((vec![true], 0.48)));
    }

    /// Nothing to move is a key that does nothing. There is no divider that
    /// way, and no other divider that would be the right answer instead.
    #[test]
    fn a_chart_with_no_divider_that_way_does_not_resize() {
        assert_eq!(Node::leaf(1).resize(1, Resize::Right), None);
        assert_eq!(Node::leaf(1).resize(1, Resize::Down), None);

        let column = Node::leaf(1).split(1, 2, false);
        assert_eq!(column.resize(1, Resize::Right), None, "a column has no width to give");
        assert!(column.resize(1, Resize::Up).is_some());

        let row = Node::leaf(1).split(1, 2, true);
        assert_eq!(row.resize(2, Resize::Down), None, "a row has no height to give");

        // And a pane that is not in this arrangement at all.
        assert_eq!(row.resize(9, Resize::Right), None);
    }

    /// Held down, a resize key arrives at the end of its travel and stops
    /// there — at the same place a dragged handle stops, not past it.
    #[test]
    fn a_nudged_divider_stops_where_a_dragged_one_does() {
        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 0.94);
        let (divider, ratio) = layout.resize(1, Resize::Right).unwrap();
        assert!((ratio - DIVIDER_MAX).abs() < 1e-9, "{ratio}");

        // Twice more and it is still there rather than off the end.
        let layout = layout.with_ratio(&divider, ratio);
        let (_, ratio) = layout.resize(1, Resize::Right).unwrap();
        assert!((ratio - DIVIDER_MAX).abs() < 1e-9, "{ratio}");

        let layout = Node::leaf(1).split(1, 2, true).with_ratio(&[], 0.06);
        let (_, ratio) = layout.resize(1, Resize::Left).unwrap();
        assert!((ratio - DIVIDER_MIN).abs() < 1e-9, "{ratio}");
    }

    /// What the key press actually does: the divider named is the one that
    /// moves, and every other divider is left where it was.
    #[test]
    fn resizing_moves_one_divider_and_leaves_the_others_alone() {
        let layout = Node::leaf(1)
            .split(1, 2, true)
            .split(1, 3, false)
            .split(2, 4, false)
            .with_ratio(&[true], 0.3)
            .with_ratio(&[false], 0.7);

        let (divider, ratio) = layout.resize(3, Resize::Down).unwrap();
        let resized = layout.with_ratio(&divider, ratio);

        let Node::Split { ratio: across, first, second, .. } = &resized else { panic!() };
        assert!((across - 0.5).abs() < 1e-9, "the columns did not move: {across}");
        let Node::Split { ratio: left, .. } = &**first else { panic!() };
        assert!((left - 0.32).abs() < 1e-9, "the left column's divider went down: {left}");
        let Node::Split { ratio: right, .. } = &**second else { panic!() };
        assert!((right - 0.7).abs() < 1e-9, "the right column did not move: {right}");
    }
}
