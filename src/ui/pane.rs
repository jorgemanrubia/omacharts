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
    BarStyle, FetchFailure, Indicator, Instrument, LinkMode, Session, Theme, Timeframe,
};

use crate::ui::chart::{ChartView, Hidden};
use crate::ui::shortcuts;

/// A chart's feed of volume at price, and the contract in it.
#[derive(Clone)]
pub struct LadderSource {
    pub at_price: Rc<crate::at_price::AtPrice>,
    /// The cache key, `provider:symbol`.
    pub key: String,
    pub symbol: String,
}

/// One chart filling the window, and the way back.
pub const MAXIMIZE: &str = "Maximize";
pub const RESTORE: &str = "Restore";

/// Where a chart's data feed stands, as the Data feed part's dot says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedPhase {
    /// Signing in, or waiting for the first bars: orange.
    Connecting,
    /// Bars came, and nothing has gone wrong since: green.
    Connected,
    /// Nothing newer is coming, and this is why: red, with a warning
    /// beside it for a failure, and without one for a feed nobody has
    /// signed in to, which has not failed at anything.
    Lost(FetchFailure),
}

/// What the window tells the Data feed part about the feed a chart is on,
/// known before anything is asked of it. The rest, [`FeedPhase`], is what
/// came of asking.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FeedPart {
    /// The feed's name, "Rithmic"; `None` when no feed charts the symbol.
    pub label: Option<String>,
    /// Can another feed be chosen for the chart? Then the dot opens the
    /// list; otherwise it only says how the feed stands.
    pub clickable: bool,
    /// Why the chosen feed cannot be used, "not signed in", when it was
    /// chosen and cannot: nothing was asked of it.
    pub refused: Option<String>,
    /// The feeds the order passed over on the way to this one, each with
    /// why: "thinkorswim not signed in".
    pub passed: Vec<String>,
}

/// The dot's colour as a CSS class, whether the warning stands beside it,
/// and the words of the tooltip after the feed's name.
pub fn feed_status(part: &FeedPart, phase: FeedPhase) -> (&'static str, bool, String) {
    if part.label.is_none() {
        return ("down", true, "No data feed charts this symbol".into());
    }
    if let Some(why) = &part.refused {
        return ("down", false, capitalised(why));
    }
    match phase {
        FeedPhase::Connecting => ("connecting", false, "Connecting…".into()),
        FeedPhase::Connected => ("connected", false, "Connected".into()),
        FeedPhase::Lost(FetchFailure::NeedsSignIn) => ("down", false, "Not signed in".into()),
        FeedPhase::Lost(FetchFailure::Unserved) => ("down", true, "Does not chart this symbol".into()),
        FeedPhase::Lost(why) => ("down", true, format!("Can't connect: {}", decapitalised(why.message()))),
    }
}

/// What the dot's tooltip says: the feed, how it stands, and on lines of
/// their own what the order passed over to get here.
pub fn feed_tooltip(part: &FeedPart, phase: FeedPhase) -> String {
    let (_, _, words) = feed_status(part, phase);
    let mut tooltip = match &part.label {
        Some(label) => format!("{label} · {words}"),
        None => words,
    };
    for line in &part.passed {
        tooltip.push('\n');
        tooltip.push_str(line);
    }
    tooltip
}

fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn decapitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Where a title typed into the header is handed.
type TitleHandler = Rc<dyn Fn(String)>;

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
    /// Dims the chart while a picker is open over it, so the chart a pick
    /// will change is the one that looks different from the rest.
    pub shade: gtk::Box,
    expand_icon: gtk::DrawingArea,
    /// Which way the expand glyph points, and which word its tooltip uses.
    expand_state: Rc<Cell<bool>>,
    pub instrument: RefCell<Option<Instrument>>,
    /// Where this chart's volume at price comes from, when its feed has
    /// ladders of what it shows; `None` for every other chart, which then
    /// holds none and asks for none.
    pub ladders: RefCell<Option<LadderSource>>,
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
    pub header: RefCell<crate::header::Header>,
    /// The line the header is written on, and the two parts of it only the
    /// window can fill in.
    pub header_row: gtk::Box,
    /// The header's center: the row across the chart's top, where the strip
    /// is until somebody moves it.
    pub header_center: gtk::Box,
    pub change_label: gtk::Label,
    /// What the day's move is measured from: the close before the session,
    /// and that session's day (see [`crate::cli::session_day`]). Kept when
    /// the move is read from the cache, so a trade can move it without
    /// reading the cache again.
    pub change_basis: Cell<Option<(f64, i64)>>,
    /// The move written, in hundredths of a percent: a trade that leaves it
    /// where it is writes nothing.
    pub change_shown: Cell<Option<i64>>,
    pub template_label: gtk::Label,
    /// The Title part: the chart title, a flat button like the symbol
    /// that turns into an entry where it stands when clicked, Enter or
    /// clicking away keeping what was typed and Escape what was there.
    pub title_part: gtk::Stack,
    pub title_button: gtk::Button,
    title_label: gtk::Label,
    pub title_entry: gtk::Entry,
    /// The chart title, empty for none.
    pub title: RefCell<String>,
    /// While the entry is up: Enter and the focus leaving it both finish
    /// it, and only the first of them may.
    title_editing: Cell<bool>,
    /// Where a title typed in the header goes: the window, which keeps it.
    title_handler: RefCell<Option<TitleHandler>>,
    /// The header's right: the corner the controls stand in until somebody
    /// moves them.
    corner: gtk::Box,
    /// The Account part: which trading account the chart is on, "Lucid
    /// 1234", with its trouble after it. Clicked, it offers the accounts.
    /// Hidden with at most one account, which leaves nothing to choose,
    /// and a hidden child of the row takes no room at all.
    pub account_menu: gtk::MenuButton,
    account_label: gtk::Label,
    /// The Data feed part: a dot in the colour of the feed's connection,
    /// naming the feed and how it stands when hovered. Clicked, it offers
    /// the feeds that could serve the chart instead, while there is more
    /// than one. Hidden with Yahoo alone: nothing to choose, nothing to
    /// connect to, and no room taken.
    pub feed_menu: gtk::MenuButton,
    feed_dot: gtk::Box,
    feed_warning: gtk::Image,
    feed_part: RefCell<Option<FeedPart>>,
    feed_phase: Cell<FeedPhase>,
    /// What builds the menu the dot opens, handed in by the window and
    /// attached only while there is something to choose.
    feed_menu_model: RefCell<Option<Rc<dyn Fn() -> gtk::gio::Menu>>>,
    /// The header menus' choices, as `pane.account`, `pane.feed` and
    /// `pane.quantity`: what the window adds, the menus name and GTK draws
    /// the mark of.
    pub menu_actions: gtk::gio::SimpleActionGroup,
    /// The feed somebody chose for this chart, by id; `None` follows the
    /// order, which is every chart until somebody chooses.
    pub feed: RefCell<Option<String>>,
    /// The trading account somebody chose for this chart, as `<login
    /// id>/<account number>`; `None` is the default account.
    pub account: RefCell<Option<String>>,
    /// Buy and Sell at the market, for the chart's quantity. Hidden, and
    /// taking no room, unless the chart has an account to trade on.
    pub trade_part: gtk::Box,
    pub buy_button: gtk::Button,
    pub sell_button: gtk::Button,
    /// The Quantity part: how many an order from this chart is for,
    /// "Qty 2". Clicked, it offers the template's quantities. Hidden, and
    /// taking no room, unless the chart has an account to trade on.
    pub size_menu: gtk::MenuButton,
    size_label: gtk::Label,
    /// "P&L" and the account's P&L for the day after it.
    pub day_part: gtk::Box,
    day_amount: gtk::Label,
    /// The size somebody picked for this chart, and from which template;
    /// `None` is the first size of whichever template applies.
    pub trading_size: RefCell<Option<crate::trading::sizes::Picked>>,
    /// The style its bars are drawn in when they are drawn: hiding them
    /// leaves it alone, so showing them again brings back the same one.
    pub bar_style: Cell<BarStyle>,
    pub bars_hidden: Cell<bool>,
    /// How the theme's candle colours are painted on this chart.
    pub colouring: Cell<omacharts_engine::BarColouring>,
    /// The two switches worn over that colouring: its up and down
    /// exchanged, red for a rise, and a rising body drawn as an outline.
    pub swapped: Cell<bool>,
    pub hollow_up: Cell<bool>,
    /// How many pixels wide its OHLC bars are drawn.
    pub ohlc_thickness: Cell<u8>,
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
    /// The volume profile periods merged on this chart, kept with it: a
    /// composite is a study of this chart's, not of the indicator's.
    pub profile_merges: RefCell<Vec<omacharts_engine::indicators::profile::Merge>>,
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
        // The title reads as the header's other words do, the symbol's
        // weight and its hover, and is a button only so the pointer can say
        // it is one. Untitled, its words are there and not drawn, so the
        // slot has a size to point at: under the pointer they read "Add
        // title", quietly, and a click titles the chart where it stands.
        let title_label = gtk::Label::new(Some(TITLE_PLACEHOLDER));
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.set_max_width_chars(TITLE_MAX_CHARS);
        let title_button = gtk::Button::new();
        title_button.set_child(Some(&title_label));
        for class in ["flat", "legend-symbol", "readout-symbol", "chart-title", TITLE_UNTITLED] {
            title_button.add_css_class(class);
        }
        title_button.set_valign(gtk::Align::Center);
        title_button.set_cursor_from_name(Some("pointer"));
        let title_entry = gtk::Entry::new();
        title_entry.add_css_class("readout-symbol");
        title_entry.add_css_class("chart-title-entry");
        title_entry.set_valign(gtk::Align::Center);
        // Its own width, centred, even while an untitled slot spans the top.
        title_entry.set_halign(gtk::Align::Center);
        gtk::prelude::EntryExt::set_alignment(&title_entry, 0.5);
        title_entry.set_max_length(TITLE_MAX_LENGTH);
        title_entry.set_width_chars(title_width(""));
        title_entry.connect_changed(|entry| entry.set_width_chars(title_width(&entry.text())));
        let title_part = gtk::Stack::new();
        title_part.set_hhomogeneous(false);
        title_part.set_vhomogeneous(false);
        title_part.set_valign(gtk::Align::Center);
        title_part.set_height_request(SYMBOL_ROW_HEIGHT);
        // Given more than it asks for only when it is the whole centre of an
        // untitled chart, which then answers the pointer across the top.
        title_part.set_hexpand(true);
        title_part.add_named(&title_button, Some("shown"));
        title_part.add_named(&title_entry, Some("editing"));
        title_part.set_visible_child_name("shown");
        // A button like the resolution beside it, with no chevron for the
        // same reason, and dim like the template: it is a note on the chart
        // more than its title.
        let account_label = gtk::Label::new(None);
        let account_menu = gtk::MenuButton::new();
        account_menu.add_css_class("flat");
        account_menu.add_css_class("readout-symbol");
        account_menu.add_css_class("dim-label");
        account_menu.set_valign(gtk::Align::Center);
        account_menu.set_child(Some(&account_label));
        account_menu.set_always_show_arrow(false);
        account_menu.set_visible(false);
        // The feed is a dot, not a word: which feed and how it stands are
        // the tooltip's, and the colour is what a glance at four charts
        // reads. The warning beside it is there only when something went
        // wrong, so a dot that is merely red says "not connected" and a
        // dot with a warning says "tried, and could not".
        let feed_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        feed_dot.add_css_class("feed-dot");
        feed_dot.set_valign(gtk::Align::Center);
        let feed_warning = gtk::Image::from_icon_name("dialog-warning-symbolic");
        feed_warning.add_css_class("feed-warning");
        feed_warning.set_pixel_size(12);
        feed_warning.set_valign(gtk::Align::Center);
        feed_warning.set_visible(false);
        let feed_light = gtk::Box::new(gtk::Orientation::Horizontal, 3);
        feed_light.append(&feed_dot);
        feed_light.append(&feed_warning);
        let feed_menu = gtk::MenuButton::new();
        feed_menu.add_css_class("flat");
        feed_menu.add_css_class("feed-part");
        feed_menu.set_valign(gtk::Align::Center);
        feed_menu.set_child(Some(&feed_light));
        feed_menu.set_always_show_arrow(false);
        feed_menu.set_visible(false);
        // Two words in the trading colours, as calm as the rest of the
        // line until the pointer is on one.
        let buy_button = gtk::Button::with_label("Buy");
        let sell_button = gtk::Button::with_label("Sell");
        for (button, class) in [(&buy_button, "trade-buy"), (&sell_button, "trade-sell")] {
            button.add_css_class("flat");
            button.add_css_class("trade-button");
            button.add_css_class(class);
            button.set_valign(gtk::Align::Center);
        }
        let trade_part = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        trade_part.append(&buy_button);
        trade_part.append(&sell_button);
        trade_part.set_visible(false);
        // The same kind of button, not dim: it is what the next order
        // spends. "Qty" in the line's quiet grey, the number in full.
        let size_label = gtk::Label::new(None);
        let size_word = gtk::Label::new(Some("Qty"));
        size_word.add_css_class("dim-label");
        let size_line = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        size_line.append(&size_word);
        size_line.append(&size_label);
        let size_menu = gtk::MenuButton::new();
        size_menu.add_css_class("flat");
        size_menu.add_css_class("readout-symbol");
        size_menu.set_valign(gtk::Align::Center);
        size_menu.set_child(Some(&size_line));
        size_menu.set_always_show_arrow(false);
        size_menu.set_visible(false);
        // Read like the quantity: the word quiet, the money in the profit
        // or loss colour, or plain at nothing either way.
        let day_word = gtk::Label::new(Some("P&L"));
        day_word.add_css_class("dim-label");
        let day_amount = gtk::Label::new(None);
        let day_part = gtk::Box::new(gtk::Orientation::Horizontal, 5);
        day_part.add_css_class("readout-symbol");
        day_part.set_valign(gtk::Align::Center);
        day_part.append(&day_word);
        day_part.append(&day_amount);
        day_part.set_visible(false);
        // A part's tooltip is not drawn over its own open menu.
        for part in [&account_menu, &feed_menu, &size_menu] {
            part.connect_active_notify(|part| {
                let open = part.is_active();
                part.set_has_tooltip(!open && part.tooltip_text().is_some());
            });
        }
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
        // Spaced as the line on the left is, for when the center holds more
        // than the strip.
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        // Placed by the overlay below, which centres it itself.
        top.set_halign(gtk::Align::Fill);
        top.set_valign(gtk::Align::Start);
        top.set_margin_top(6);

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
        // Over the bars, under the legend: the chart dims and its name
        // stays readable, which is half of saying which chart it is.
        let shade = gtk::Box::new(gtk::Orientation::Vertical, 0);
        shade.add_css_class("pick-shade");
        shade.set_can_target(false);
        shade.set_visible(false);
        overlay.add_overlay(&shade);
        overlay.add_overlay(&legend);
        overlay.add_overlay(&top);
        overlay.add_overlay(&corner);

        // The three are laid over the chart independently, so on a narrow
        // chart the centred row lands on the symbol or under the corner. It
        // slides along to clear them, and steps aside altogether when there
        // is no room between them; for the strip, the resolution beside the
        // symbol offers the same list. Decided
        // where the overlay places it, the one moment every width involved
        // is known.
        //
        // On the left, the strip steps aside from the corner the same way:
        // the line grows towards it, and on a narrow chart would run under
        // the gear. The legend is placed first, so the center is measured
        // against the line as it will be drawn.
        let (legend_box, legend_row, strip_row, corner_box, strip_part) =
            (legend.clone(), bar.clone(), top.clone(), corner.clone(), strip.clone());
        let (lone_title, untitled) = (title_part.clone(), title_button.clone());
        overlay.connect_get_child_position(move |overlay, child| {
            let natural = |w: &gtk::Widget| w.preferred_size().1.width();
            if child == legend_box.upcast_ref::<gtk::Widget>()
                && strip_part.parent().is_some_and(|parent| parent == legend_row)
            {
                let strip_width = natural(strip_part.upcast_ref());
                // The line without the strip, however it was last decided:
                // a strip not laid out is not in the line's own measure.
                let laid_out = strip_part.is_visible() && strip_part.is_child_visible();
                let rest = natural(legend_row.upcast_ref())
                    - if laid_out { strip_width + legend_row.spacing() } else { 0 };
                let right = natural(corner_box.upcast_ref()) + corner_box.margin_end();
                let fits = legend_box.margin_start() + rest + legend_row.spacing() + strip_width + STRIP_GAP
                    <= overlay.width() - right;
                if strip_part.is_child_visible() != fits {
                    strip_part.set_child_visible(fits);
                }
            }
            if child == strip_row.upcast_ref::<gtk::Widget>() {
                // Read here, not once: a chart on the window's left edge
                // steps its legend past the drawing toggle after it is built.
                let left = legend_box.margin_start() + natural(legend_row.upcast_ref());
                let right = natural(corner_box.upcast_ref()) + corner_box.margin_end();
                let (_, size) = strip_row.preferred_size();
                // An untitled chart's title, alone in the centre, is the
                // whole of the top between the two sides: pointed at
                // anywhere there it offers Add title, and clicked, it edits.
                let spans = untitled.has_css_class(TITLE_UNTITLED)
                    && strip_row.first_child().is_some_and(|child| child == lone_title)
                    && lone_title.next_sibling().is_none();
                let place = center_place(overlay.width(), left, size.width(), right, spans);
                if strip_row.is_child_visible() != place.is_some() {
                    strip_row.set_child_visible(place.is_some());
                }
                return place.map(|(x, width)| gtk::gdk::Rectangle::new(x, 0, width, size.height()));
            }
            None
        });

        // A box rather than the overlay itself, so the focus ring is drawn on
        // something that is not also the drawing surface.
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let menu_actions = gtk::gio::SimpleActionGroup::new();
        root.insert_action_group("pane", Some(&menu_actions));
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
            shade,
            expand_icon,
            expand_state,
            instrument: RefCell::new(None),
            ladders: RefCell::new(None),
            pending: RefCell::new(None),
            timeframe: Cell::new(timeframe),
            indicators: RefCell::new(indicators),
            template: Cell::new(None),
            header: RefCell::new(crate::header::Header::standard()),
            header_row: bar,
            header_center: top,
            change_label,
            change_basis: Cell::new(None),
            change_shown: Cell::new(None),
            template_label,
            title_part,
            title_button,
            title_label,
            title_entry,
            title: RefCell::new(String::new()),
            title_editing: Cell::new(false),
            title_handler: RefCell::new(None),
            corner,
            account_menu,
            account_label,
            feed_menu,
            feed_dot,
            feed_warning,
            feed_part: RefCell::new(None),
            feed_phase: Cell::new(FeedPhase::Connecting),
            feed_menu_model: RefCell::new(None),
            menu_actions,
            feed: RefCell::new(None),
            account: RefCell::new(None),
            trade_part,
            buy_button,
            sell_button,
            size_menu,
            size_label,
            day_part,
            day_amount,
            trading_size: RefCell::new(None),
            bar_style: Cell::new(bar_style),
            bars_hidden: Cell::new(false),
            colouring: Cell::new(Default::default()),
            swapped: Cell::new(false),
            hollow_up: Cell::new(false),
            ohlc_thickness: Cell::new(crate::chart_defaults::DEFAULT_OHLC_THICKNESS),
            session: Cell::new(session),
            show_grid: Cell::new(show_grid),
            mode: Cell::new(mode),
            shows_drawings: Cell::new(true),
            sends_drawings: Cell::new(true),
            uid: RefCell::new(gtk::glib::uuid_string_random().to_string()),
            local_drawings: RefCell::new(Vec::new()),
            profile_merges: RefCell::new(Vec::new()),
            link_colour,
            glyph,
        });
        pane.arrange_header();
        pane.wire_title_entry();
        pane
    }

    /// Enter keeps what was typed, and so does clicking away; Escape puts
    /// back what was there.
    fn wire_title_entry(self: &Rc<Self>) {
        let this = Rc::downgrade(self);
        self.title_entry.connect_activate(move |_| {
            if let Some(pane) = this.upgrade() {
                pane.finish_title_edit(true);
            }
        });
        let focus = gtk::EventControllerFocus::new();
        let this = Rc::downgrade(self);
        focus.connect_leave(move |_| {
            if let Some(pane) = this.upgrade() {
                pane.finish_title_edit(true);
            }
        });
        self.title_entry.add_controller(focus);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let this = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            if key != gtk::gdk::Key::Escape {
                return gtk::glib::Propagation::Proceed;
            }
            if let Some(pane) = this.upgrade() {
                pane.finish_title_edit(false);
            }
            gtk::glib::Propagation::Stop
        });
        self.title_entry.add_controller(keys);
    }

    /// Where a title typed into the header is handed.
    pub fn set_title_handler(&self, handler: impl Fn(String) + 'static) {
        *self.title_handler.borrow_mut() = Some(Rc::new(handler));
    }

    /// Show `title` as the chart title, or nothing for none.
    pub fn write_title(&self, title: &str) {
        let title = title.trim();
        if *self.title.borrow() != title {
            *self.title.borrow_mut() = title.to_string();
        }
        let titled = !title.is_empty();
        let shown = if titled { title } else { TITLE_PLACEHOLDER };
        if self.title_label.text() != shown {
            self.title_label.set_text(shown);
        }
        match titled {
            true => self.title_button.remove_css_class(TITLE_UNTITLED),
            false => self.title_button.add_css_class(TITLE_UNTITLED),
        }
        // A long title is cut short in the header and read in full here.
        self.title_button.set_tooltip_text(titled.then_some(title));
    }

    /// Is the title being typed into the header?
    pub fn editing_title(&self) -> bool {
        self.title_editing.get()
    }

    /// Swap the title for an entry holding it, where it stands.
    pub fn begin_title_edit(&self) {
        if self.title_editing.replace(true) {
            return;
        }
        self.title_entry.set_text(&self.title.borrow());
        self.title_part.set_visible_child_name("editing");
        self.title_entry.grab_focus();
        self.title_entry.select_region(0, -1);
    }

    /// Put the title back where the entry was, keeping what was typed when
    /// `keep` and it changed. Blank is an answer: the chart has no title.
    pub fn finish_title_edit(&self, keep: bool) {
        if !self.title_editing.replace(false) {
            return;
        }
        let typed = self.title_entry.text().trim().to_string();
        self.title_part.set_visible_child_name("shown");
        // The keys go back to the chart they were typed over.
        self.view.area.grab_focus();
        if !keep || typed == *self.title.borrow() {
            return;
        }
        let handler = self.title_handler.borrow().clone();
        match handler {
            Some(handler) => handler(typed),
            None => self.write_title(&typed),
        }
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
    /// Draw its OHLC bars this many pixels wide.
    pub fn set_ohlc_thickness(&self, thickness: u8) {
        let thickness = crate::chart_defaults::clamp_thickness(thickness);
        self.ohlc_thickness.set(thickness);
        self.view.set_ohlc_thickness(thickness);
    }

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
        self.corner.set_margin_end(corner_margin(clearance));
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

    /// Put the header's parts in its three sections in the chart's order,
    /// and take off the ones it does not show.
    pub fn arrange_header(&self) {
        use crate::header::Part;
        let parts: [(Part, gtk::Widget); 14] = [
            (Part::Symbol, self.symbol_button.clone().upcast()),
            (Part::Resolution, self.timeframe_menu.clone().upcast()),
            (Part::Change, self.change_label.clone().upcast()),
            (Part::Template, self.template_label.clone().upcast()),
            (Part::Title, self.title_part.clone().upcast()),
            (Part::Account, self.account_menu.clone().upcast()),
            (Part::Feed, self.feed_menu.clone().upcast()),
            (Part::DayPnl, self.day_part.clone().upcast()),
            (Part::Trade, self.trade_part.clone().upcast()),
            (Part::Size, self.size_menu.clone().upcast()),
            (Part::Stripe, self.strip.clone().upcast()),
            (Part::Settings, self.gear.clone().upcast()),
            (Part::Maximize, self.expand.clone().upcast()),
            (Part::Link, self.link.clone().upcast()),
        ];
        for (_, widget) in &parts {
            if let Some(parent) = widget.parent().and_downcast::<gtk::Box>() {
                parent.remove(widget);
            }
        }
        // Whichever section it lands in decides afresh whether it fits.
        self.strip.set_child_visible(true);
        let header = self.header.borrow();
        for (row, section) in [
            (&self.header_row, &header.left),
            (&self.header_center, &header.center),
            (&self.corner, &header.right),
        ] {
            for part in section {
                if let Some((_, widget)) = parts.iter().find(|(p, _)| p == part) {
                    row.append(widget);
                }
            }
        }
        // Leading the line, the dot steps out of its button's padding so it
        // stands over the legend's dots, the button's hover reaching left
        // past them instead.
        match header.left.first() == Some(&Part::Feed) {
            true => self.feed_menu.add_css_class("lead"),
            false => self.feed_menu.remove_css_class("lead"),
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

    /// Whether the title can be typed where it stands: the header has it,
    /// and has room for the row it is in.
    pub fn title_in_view(&self) -> bool {
        self.shows(crate::header::Part::Title)
            && self.title_part.parent().is_some_and(|row| row.is_child_visible())
    }

    /// Whether the header has `part` in it, so the window only works out what
    /// it is asked to show.
    pub fn shows(&self, part: crate::header::Part) -> bool {
        self.header.borrow().contains(part)
    }

    /// The template's name, or nothing at all on a chart with none.
    pub fn write_template(&self, name: Option<&str>) {
        self.template_label.set_text(name.unwrap_or(""));
        self.template_label.set_visible(name.is_some());
    }

    /// Which account the chart is on, or nothing at all: hidden, the part
    /// takes no width and no spacing, as though it were not in the header.
    pub fn write_account(&self, account: Option<&str>) {
        if self.account_label.text() != account.unwrap_or("") {
            self.account_label.set_text(account.unwrap_or(""));
        }
        if self.account_menu.is_visible() != account.is_some() {
            self.account_menu.set_visible(account.is_some());
        }
    }

    /// Which feed the chart is on, or nothing at all: hidden, the dot
    /// takes no width and no spacing.
    pub fn write_feed(&self, part: Option<FeedPart>) {
        if *self.feed_part.borrow() == part {
            return;
        }
        *self.feed_part.borrow_mut() = part;
        self.paint_feed_dot();
    }

    /// How the chart's feed stands now, from what came of asking it.
    pub fn set_feed_phase(&self, phase: FeedPhase) {
        if self.feed_phase.replace(phase) != phase {
            self.paint_feed_dot();
        }
    }

    pub fn feed_phase(&self) -> FeedPhase {
        self.feed_phase.get()
    }

    /// Is the dot red? Only then does its list say how the feed stands:
    /// green and orange already say it.
    pub fn feed_in_trouble(&self) -> bool {
        let part = self.feed_part.borrow();
        part.as_ref().is_some_and(|part| feed_status(part, self.feed_phase.get()).0 == "down")
    }

    /// How the feed stands, in the dot's tooltip's words.
    pub fn feed_status_words(&self) -> Option<String> {
        let part = self.feed_part.borrow();
        part.as_ref().map(|part| feed_tooltip(part, self.feed_phase.get()))
    }

    /// What builds the menu the dot opens, afresh each time it opens.
    /// Attached only while there is more than one feed to choose, so with
    /// one the dot is nothing but its colour and its tooltip.
    pub fn set_feed_menu(&self, model: Rc<dyn Fn() -> gtk::gio::Menu>) {
        *self.feed_menu_model.borrow_mut() = Some(model);
        self.feed_menu.set_widget_name("");
        self.paint_feed_dot();
    }

    fn paint_feed_dot(&self) {
        let part = self.feed_part.borrow();
        let Some(part) = part.as_ref() else {
            self.feed_menu.set_visible(false);
            return;
        };
        let phase = self.feed_phase.get();
        let (colour, warning, _) = feed_status(part, phase);
        for known in ["connecting", "connected", "down"] {
            if known != colour {
                self.feed_dot.remove_css_class(known);
            }
        }
        self.feed_dot.add_css_class(colour);
        self.feed_warning.set_visible(warning);
        self.feed_menu.set_tooltip_text(Some(&feed_tooltip(part, phase)));
        // Not over its own open menu.
        if self.feed_menu.is_active() {
            self.feed_menu.set_has_tooltip(false);
        }
        let model = self.feed_menu_model.borrow();
        let offered = part.clickable.then_some(()).and(model.as_ref());
        // Named while it opens a menu, so it is only attached and taken off
        // when that changes, not on every repaint.
        let attached = self.feed_menu.widget_name() == "offers";
        if attached != offered.is_some() {
            match offered {
                Some(model) => {
                    let model = model.clone();
                    self.feed_menu.set_create_popup_func(move |button| {
                        crate::ui::menus::set_model(button, &model());
                        if let Some(menu) = button.popover().and_downcast::<gtk::PopoverMenu>() {
                            crate::ui::sign_in_dialogs::fill_credits(&menu);
                        }
                    });
                    self.feed_menu.set_widget_name("offers");
                }
                None => {
                    self.feed_menu.unset_create_popup_func();
                    self.feed_menu.set_menu_model(None::<&gtk::gio::MenuModel>);
                    self.feed_menu.set_widget_name("");
                }
            }
        }
        match offered.is_some() {
            true => self.feed_menu.remove_css_class("inert"),
            false => self.feed_menu.add_css_class("inert"),
        }
        self.feed_menu.set_visible(true);
    }

    /// The dot as it is drawn, for the tests: its colour's class, whether
    /// the warning is beside it, and whether it opens anything.
    #[cfg(test)]
    pub fn feed_dot_shown(&self) -> Option<(&'static str, bool, bool)> {
        self.feed_menu.get_visible().then(|| {
            let colour = ["connecting", "connected", "down"]
                .into_iter()
                .find(|class| self.feed_dot.has_css_class(class))
                .unwrap_or("none");
            (colour, self.feed_warning.get_visible(), self.feed_menu.widget_name() == "offers")
        })
    }

    /// What the Account part says, for the tests.
    #[cfg(test)]
    pub fn account_text(&self) -> Option<String> {
        self.account_menu.get_visible().then(|| self.account_label.text().to_string())
    }

    /// How many the next order is for, or nothing at all: hidden, the part
    /// takes no width and no spacing.
    pub fn write_size(&self, size: Option<&str>) {
        if self.size_label.text() != size.unwrap_or("") {
            self.size_label.set_text(size.unwrap_or(""));
        }
        if self.size_menu.is_visible() != size.is_some() {
            self.size_menu.set_visible(size.is_some());
        }
    }

    /// The account's P&L for the day, already written as money, and whether
    /// it is up or down; nothing at all, and no room taken, without an
    /// account or while its broker has not said.
    pub fn write_day_pnl(&self, day: Option<(&str, std::cmp::Ordering)>) {
        if self.day_part.get_visible() != day.is_some() {
            self.day_part.set_visible(day.is_some());
        }
        let Some((text, sign)) = day else { return };
        if self.day_amount.text() != text {
            self.day_amount.set_text(text);
        }
        let on = match sign {
            std::cmp::Ordering::Greater => Some("pnl-gain"),
            std::cmp::Ordering::Less => Some("pnl-loss"),
            std::cmp::Ordering::Equal => None,
        };
        for class in ["pnl-gain", "pnl-loss"] {
            let wanted = on == Some(class);
            if self.day_amount.has_css_class(class) != wanted {
                match wanted {
                    true => self.day_amount.add_css_class(class),
                    false => self.day_amount.remove_css_class(class),
                }
            }
        }
    }

    /// What the P&L part says and in which colour, for the tests.
    #[cfg(test)]
    pub fn day_pnl_text(&self) -> Option<(String, Option<&'static str>)> {
        self.day_part.get_visible().then(|| {
            let class = ["pnl-gain", "pnl-loss"].into_iter().find(|class| self.day_amount.has_css_class(class));
            (self.day_amount.text().to_string(), class)
        })
    }

    /// Buy and Sell, or nothing at all: `None` without an account to
    /// trade on, `Err` with why an order from this chart would not go, which
    /// the buttons say, insensitive, when hovered.
    pub fn write_trade(&self, state: Option<Result<(), String>>) {
        if self.trade_part.is_visible() != state.is_some() {
            self.trade_part.set_visible(state.is_some());
        }
        let Some(state) = state else { return };
        let able = state.is_ok();
        for (button, words, action) in [
            (&self.buy_button, "Buy at market", "chart.buy-market"),
            (&self.sell_button, "Sell at market", "chart.sell-market"),
        ] {
            if button.is_sensitive() != able {
                button.set_sensitive(able);
            }
            let tip = able.then(|| crate::ui::shortcuts::tooltip(words, action));
            if button.tooltip_text().map(|tip| tip.to_string()) != tip {
                button.set_tooltip_text(tip.as_deref());
            }
        }
        // Why not, on the pair: a button that cannot be pressed is passed
        // over for its tooltip.
        let why = state.err();
        if self.trade_part.tooltip_text().map(|tip| tip.to_string()) != why {
            self.trade_part.set_tooltip_text(why.as_deref());
        }
    }

    /// What Buy and Sell say when hovered, for the tests: the pair's reason,
    /// or the buttons' own words.
    #[cfg(test)]
    pub fn trade_tooltip(&self) -> Option<String> {
        self.trade_part
            .tooltip_text()
            .or_else(|| self.buy_button.tooltip_text())
            .map(|tip| tip.to_string())
    }

    /// What the Quantity part says.
    #[cfg(test)]
    pub fn size_label_text(&self) -> String {
        self.size_label.text().to_string()
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
/// are a pair. The drawing tools' toggle in the bottom-left corner stands
/// its glyph at the same inset, so the header and the toggle start at one x.
pub(crate) const LEGEND_MARGIN: i32 = 12;
/// The side of a legend row's dot's box: the 8 px dot and room round it for
/// the ring a selected indicator's dot wears.
pub const LEGEND_DOT: i32 = 12;
/// From a legend row's dot's box to its name. The header's line keeps the
/// same column when the data feed's dot leads it (the stylesheet's
/// `.feed-part.lead`), so the symbol's first letter is the first letter of
/// every name under it, and its dot stands over theirs.
const LEGEND_GAP: i32 = 5;

/// A row of the legend under the header: an indicator's dot and its name,
/// the buttons that act on it appended after.
pub fn legend_row(dot: &impl IsA<gtk::Widget>, name: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, LEGEND_GAP);
    row.add_css_class("legend-row");
    row.append(dot);
    row.append(name);
    row
}

/// The strip's height: its buttons plus the padding around them.
const SYMBOL_ROW_HEIGHT: i32 = 32;

/// What an untitled chart's Title part says under the pointer, and holds the
/// room of at rest without drawing it.
pub const TITLE_PLACEHOLDER: &str = "Add title";
/// The class an untitled chart's Title part wears; a screenshot leaves it
/// out, since it is a control and not a title.
pub const TITLE_UNTITLED: &str = "chart-title-untitled";
/// How long a title may be, and how much of it the header shows before it
/// is cut short.
pub const TITLE_MAX_LENGTH: i32 = 80;
const TITLE_MAX_CHARS: i32 = 40;

/// How wide the entry a title is typed into is: what it holds and a
/// character to grow into, so it reads as the words rather than a field.
fn title_width(text: &str) -> i32 {
    (text.chars().count() as i32 + 1).clamp(8, TITLE_MAX_CHARS)
}

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

/// Where a row `center` wide starts across `width`: centred, or slid
/// towards whichever side has room so it clears what sits at either edge —
/// `left` and `right` of it — with a little air on each side. `None` when
/// even that leaves no room, and the row is not shown.
fn center_start(width: i32, left: i32, center: i32, right: i32) -> Option<i32> {
    let (lowest, highest) = (left + STRIP_GAP, width - right - STRIP_GAP - center);
    (lowest <= highest).then(|| ((width - center) / 2).clamp(lowest, highest))
}

/// Where the centre row goes and how wide: its own width, placed by
/// [`center_start`], or, when it `spans`, all the room between the two
/// sides, so long as that holds its own width.
fn center_place(width: i32, left: i32, center: i32, right: i32, spans: bool) -> Option<(i32, i32)> {
    let start = center_start(width, left, center, right)?;
    match spans {
        true => Some((left + STRIP_GAP, width - right - STRIP_GAP - (left + STRIP_GAP))),
        false => Some((start, center)),
    }
}

/// The air the strip keeps from what is beside it.
const STRIP_GAP: i32 = 8;

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

    /// With one account the Account part says nothing, and with Yahoo alone
    /// the Data feed part draws nothing: a header with either laid out is a
    /// header without it, the same width, and the part after it starting
    /// at the same place. No gap where it would be, no room taken.
    #[test]
    fn a_blank_account_or_feed_takes_no_room_in_the_header() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            let pane = test_pane();
            pane.symbol_button.set_label("ES");
            pane.write_template(Some("Footprint"));
            let laid_out = |parts: Vec<Part>| {
                *pane.header.borrow_mut() = Header { left: parts, center: Vec::new(), right: Vec::new() };
                pane.arrange_header();
                let width = pane.header_row.preferred_size().1.width();
                pane.header_row.allocate(width, SYMBOL_ROW_HEIGHT, -1, None);
                let template = pane.template_label.compute_bounds(&pane.header_row).expect("placed").x();
                (width, template)
            };
            let without = laid_out(vec![Part::Symbol, Part::Resolution, Part::Template]);
            pane.write_account(None);
            pane.write_feed(None);
            let blank = laid_out(vec![Part::Symbol, Part::Account, Part::Feed, Part::Resolution, Part::Template]);
            assert_eq!(blank, without);
            assert!(!pane.account_menu.is_visible());
            assert!(!pane.feed_menu.is_visible());

            // And written, each does take its room: the same measure sees it.
            pane.write_account(Some("Rithmic Test"));
            let shown = laid_out(vec![Part::Symbol, Part::Account, Part::Resolution, Part::Template]);
            assert!(shown.0 > without.0 && shown.1 > without.1, "{shown:?} against {without:?}");
            pane.write_account(None);
            pane.write_feed(Some(FeedPart { label: Some("Rithmic".into()), ..FeedPart::default() }));
            let dotted = laid_out(vec![Part::Symbol, Part::Feed, Part::Resolution, Part::Template]);
            assert!(dotted.0 > without.0 && dotted.1 > without.1, "{dotted:?} against {without:?}");
            // A dot, not a word: far narrower than a name.
            assert!(dotted.0 - without.0 < shown.0 - without.0, "{dotted:?} against {shown:?}");
        });
    }

    /// The dot is the feed's connection: orange while connecting, green
    /// once connected, red when not, and red with a warning when it tried
    /// and could not. Its tooltip names the feed and says the same in
    /// words; and it opens the list only while there is a feed to choose.
    #[test]
    fn the_feed_dot_is_the_colour_of_the_connection() {
        crate::ui::on_gtk(|| {
            let pane = test_pane();
            pane.set_feed_menu(Rc::new(gtk::gio::Menu::new));
            let on = |label: &str, clickable: bool| FeedPart { label: Some(label.into()), clickable, ..FeedPart::default() };
            pane.write_feed(Some(on("Rithmic", true)));
            pane.set_feed_phase(FeedPhase::Connecting);
            assert_eq!(pane.feed_dot_shown(), Some(("connecting", false, true)));
            assert_eq!(pane.feed_menu.tooltip_text().as_deref(), Some("Rithmic · Connecting…"));

            pane.set_feed_phase(FeedPhase::Connected);
            assert_eq!(pane.feed_dot_shown(), Some(("connected", false, true)));
            assert_eq!(pane.feed_menu.tooltip_text().as_deref(), Some("Rithmic · Connected"));

            pane.set_feed_phase(FeedPhase::Lost(FetchFailure::Unreachable));
            assert_eq!(pane.feed_dot_shown(), Some(("down", true, true)));
            assert_eq!(
                pane.feed_menu.tooltip_text().as_deref(),
                Some("Rithmic · Can't connect: the data feed is not answering. Prices fill in when it does.")
            );

            // Not signed in: red, and no warning, since nothing was tried.
            pane.set_feed_phase(FeedPhase::Lost(FetchFailure::NeedsSignIn));
            assert_eq!(pane.feed_dot_shown(), Some(("down", false, true)));
            assert_eq!(pane.feed_menu.tooltip_text().as_deref(), Some("Rithmic · Not signed in"));

            // The one feed: the same dot and tooltip, opening nothing.
            pane.set_feed_phase(FeedPhase::Connected);
            let mut passed = on("Yahoo Finance", false);
            passed.passed = vec!["Rithmic not signed in".into()];
            pane.write_feed(Some(passed));
            assert_eq!(pane.feed_dot_shown(), Some(("connected", false, false)));
            assert!(pane.feed_menu.has_css_class("inert"));
            assert_eq!(
                pane.feed_menu.tooltip_text().as_deref(),
                Some("Yahoo Finance · Connected\nRithmic not signed in")
            );

            // A chosen feed that cannot be used says why, whatever came
            // before; and no feed at all is a warning.
            let mut refused = on("Rithmic", true);
            refused.refused = Some("session expired".into());
            pane.write_feed(Some(refused));
            assert_eq!(pane.feed_dot_shown(), Some(("down", false, true)));
            assert_eq!(pane.feed_menu.tooltip_text().as_deref(), Some("Rithmic · Session expired"));
            pane.write_feed(Some(FeedPart::default()));
            assert_eq!(pane.feed_dot_shown(), Some(("down", true, false)));
            assert_eq!(pane.feed_menu.tooltip_text().as_deref(), Some("No data feed charts this symbol"));

            pane.write_feed(None);
            assert_eq!(pane.feed_dot_shown(), None);
        });
    }

    /// The corner's controls are header parts: in the corner in their
    /// order out of the box, in the line when put there, and gone when
    /// taken out.
    #[test]
    fn the_corner_s_controls_go_where_the_header_puts_them() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            let pane = test_pane();
            let children = |row: &gtk::Box| {
                let mut found = Vec::new();
                let mut next = row.first_child();
                while let Some(child) = next {
                    next = child.next_sibling();
                    found.push(child);
                }
                found
            };
            let corner = |pane: &ChartPane| children(&pane.corner);
            assert_eq!(
                corner(&pane),
                vec![
                    pane.day_part.clone().upcast::<gtk::Widget>(),
                    pane.trade_part.clone().upcast(),
                    pane.size_menu.clone().upcast(),
                    pane.gear.clone().upcast(),
                    pane.expand.clone().upcast(),
                    pane.link.clone().upcast()
                ]
            );
            let header = |left: Vec<Part>, right: Vec<Part>| {
                *pane.header.borrow_mut() = Header { left, center: Vec::new(), right };
                pane.arrange_header();
            };
            header(vec![Part::Settings, Part::Symbol], vec![Part::Link, Part::Maximize]);
            assert_eq!(corner(&pane), vec![pane.link.clone().upcast::<gtk::Widget>(), pane.expand.clone().upcast()]);
            assert_eq!(children(&pane.header_row)[0], pane.gear.clone().upcast::<gtk::Widget>());
            header(vec![Part::Symbol], Vec::new());
            assert!(corner(&pane).is_empty());
            assert!(pane.gear.parent().is_none());
            header(Header::standard().left, Header::standard().right);
            // The P&L, Buy and Sell and the quantity, then the three controls.
            assert_eq!(corner(&pane).len(), 6);
        });
    }

    /// Without a trading account the size says nothing and takes no room:
    /// the same width as a header without it. Written, it does.
    #[test]
    fn a_blank_trading_size_takes_no_room_in_the_header() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            let pane = test_pane();
            pane.symbol_button.set_label("ES");
            pane.write_template(Some("Footprint"));
            let laid_out = |parts: Vec<Part>| {
                *pane.header.borrow_mut() = Header { left: parts, center: Vec::new(), right: Vec::new() };
                pane.arrange_header();
                let width = pane.header_row.preferred_size().1.width();
                pane.header_row.allocate(width, SYMBOL_ROW_HEIGHT, -1, None);
                let template = pane.template_label.compute_bounds(&pane.header_row).expect("placed").x();
                (width, template)
            };
            let without = laid_out(vec![Part::Symbol, Part::Template]);
            pane.write_size(None);
            assert_eq!(laid_out(vec![Part::Symbol, Part::Size, Part::Template]), without);
            assert!(!pane.size_menu.is_visible());
            pane.write_size(Some("2"));
            let shown = laid_out(vec![Part::Symbol, Part::Size, Part::Template]);
            assert!(shown.0 > without.0 && shown.1 > without.1, "{shown:?} against {without:?}");
        });
    }

    /// The dot with one feed is the dot with two, colour and strength: an
    /// insensitive button is drawn faded, and the stylesheet undoes that
    /// for the dot. Read off the pixels, in a window, with the app's own
    /// stylesheet.
    #[test]
    fn the_dot_with_nothing_to_open_is_not_faded() {
        use gtk::gdk;
        crate::ui::on_gtk(|| {
            let Some(display) = gdk::Display::default() else { return };
            // The fading is libadwaita's stylesheet, which the app runs under.
            if adw::init().is_err() {
                return;
            }
            let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
            let scheme = omacharts_engine::theme::theme_bars(&theme);
            let provider = gtk::CssProvider::new();
            provider.load_from_data(&crate::theming::stylesheet(&theme, &scheme));
            gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);

            let pane = test_pane();
            pane.set_feed_menu(Rc::new(gtk::gio::Menu::new));
            pane.set_feed_phase(FeedPhase::Connected);
            let window = gtk::Window::new();
            window.set_default_size(400, 200);
            window.set_child(Some(&pane.root));
            window.present();
            let brightest = |clickable: bool| -> [u8; 3] {
                pane.write_feed(Some(FeedPart { label: Some("Rithmic".into()), clickable, ..FeedPart::default() }));
                let context = gtk::glib::MainContext::default();
                for _ in 0..50 {
                    while context.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                // The button, whose fading is what is in question, and the
                // middle of the dot inside it.
                let paintable = gtk::WidgetPaintable::new(Some(&pane.feed_menu));
                let (width, height) = (pane.feed_menu.width().max(1), pane.feed_menu.height().max(1));
                let dot = pane.feed_dot.compute_bounds(&pane.feed_menu).expect("the dot is in its button");
                let (x, y) = ((dot.x() + dot.width() / 2.0) as i32, (dot.y() + dot.height() / 2.0) as i32);
                let snapshot = gtk::Snapshot::new();
                paintable.snapshot(&snapshot, width as f64, height as f64);
                let node = snapshot.to_node().expect("the dot draws");
                let renderer = gtk::gsk::CairoRenderer::new();
                renderer.realize(None::<&gdk::Surface>).expect("a renderer");
                let texture = renderer.render_texture(
                    &node,
                    Some(&gtk::graphene::Rect::new(0.0, 0.0, width as f32, height as f32)),
                );
                renderer.unrealize();
                let mut bytes = vec![0u8; (width * height * 4) as usize];
                texture.download(&mut bytes, (width * 4) as usize);
                // Cairo's order: blue, green, red, alpha.
                let at = ((y * width + x) * 4) as usize;
                [bytes[at + 2], bytes[at + 1], bytes[at]]
            };
            let offered = brightest(true);
            let alone = brightest(false);
            assert!(pane.feed_menu.has_css_class("inert"));
            assert_ne!(offered, [0, 0, 0], "the dot drew nothing");
            assert_eq!(alone, offered);
            window.close();
        });
    }

    /// The symbol's first letter is the first letter of the legend's names
    /// under it, and the feed's dot is centred over their dots, at any size
    /// of type. Measured as laid out, in a window, with the app's own
    /// stylesheet.
    #[test]
    fn the_symbol_and_the_legend_s_names_start_in_one_column() {
        use gtk::gdk;
        crate::ui::on_gtk(|| {
            let Some(display) = gdk::Display::default() else { return };
            if adw::init().is_err() {
                return;
            }
            let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
            let scheme = omacharts_engine::theme::theme_bars(&theme);
            let provider = gtk::CssProvider::new();
            provider.load_from_data(&crate::theming::stylesheet(&theme, &scheme));
            gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);

            let pane = test_pane();
            pane.symbol_button.set_label("MNQ");
            pane.set_feed_menu(Rc::new(gtk::gio::Menu::new));
            pane.set_feed_phase(FeedPhase::Connected);
            pane.write_feed(Some(FeedPart { label: Some("Rithmic".into()), clickable: true, ..FeedPart::default() }));
            let dot = gtk::DrawingArea::new();
            dot.set_size_request(LEGEND_DOT, LEGEND_DOT);
            let name = gtk::Label::new(Some("VWAP · Year"));
            name.add_css_class("legend-indicator");
            pane.indicator_legend.append(&legend_row(&dot, &name));
            pane.indicator_legend.set_visible(true);
            let window = gtk::Window::new();
            window.set_default_size(500, 200);
            window.set_child(Some(&pane.root));
            window.present();
            let settings = gtk::Settings::default().expect("settings");
            let font = settings.gtk_font_name();
            for size in ["Sans 8", "Sans 11", "Sans 16"] {
                settings.set_gtk_font_name(Some(size));
                let context = gtk::glib::MainContext::default();
                for _ in 0..30 {
                    while context.iteration(false) {}
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                let symbol = pane.symbol_button.child().expect("the symbol's label");
                let bounds = |widget: &gtk::Widget| widget.compute_bounds(&pane.root).expect("laid out");
                let (symbol, name) = (bounds(&symbol), bounds(name.upcast_ref()));
                assert_eq!(symbol.x(), name.x(), "the symbol and the names, in {size}");
                let (feed, legend) = (bounds(pane.feed_dot.upcast_ref()), bounds(dot.upcast_ref()));
                assert_eq!(
                    feed.x() + feed.width() / 2.0,
                    legend.x() + legend.width() / 2.0,
                    "the feed's dot and the legend's, in {size}"
                );
            }
            settings.set_gtk_font_name(font.as_deref());
            window.close();
        });
    }

    fn test_pane() -> Rc<ChartPane> {
        let theme = omacharts_engine::theme::builtin_themes().into_iter().next().expect("a built-in theme");
        let scheme = omacharts_engine::theme::theme_bars(&theme);
        ChartPane::new(
            1,
            theme,
            scheme,
            Timeframe::days(1),
            Vec::new(),
            BarStyle::default(),
            Session::default(),
            true,
            LinkMode::default(),
        )
    }

    /// The stripe is a part like the others: across the top when put
    /// there, in the line on the left when put there, and nowhere, taking
    /// no room at all, when taken out, which is where it is by default.
    #[test]
    fn the_stripe_goes_where_the_header_puts_it() {
        use crate::header::{Header, Part};
        crate::ui::on_gtk(|| {
            let pane = test_pane();
            pane.symbol_button.set_label("ES");
            for label in ["5m", "1h", "1D"] {
                pane.strip.append(&gtk::ToggleButton::with_label(label));
            }
            pane.set_focused(true);
            let width = |row: &gtk::Box| row.preferred_size().1.width();
            let parent = || pane.strip.parent();
            let header = |left: Vec<Part>, center: Vec<Part>| {
                *pane.header.borrow_mut() = Header { left, center, right: Header::standard().right };
                pane.arrange_header();
            };

            assert!(parent().is_none(), "the stripe is in the standard header");
            assert!(pane.title_part.parent().is_some_and(|p| p == pane.header_center));
            header(Header::standard().left, vec![Part::Stripe]);
            assert!(parent().is_some_and(|p| p == pane.header_center));
            let line = width(&pane.header_row);
            assert!(width(&pane.header_center) > 0);
            header(vec![Part::Symbol, Part::Resolution, Part::Account, Part::Stripe], Vec::new());
            assert!(parent().is_some_and(|p| p == pane.header_row));
            assert_eq!(pane.header_row.last_child(), pane.hidden_glyphs.last().map(|(_, b)| b.clone().upcast()));
            assert!(width(&pane.header_row) > line);
            assert_eq!(width(&pane.header_center), 0);

            header(vec![Part::Symbol, Part::Resolution, Part::Account], Vec::new());
            assert!(parent().is_none());
            assert_eq!(width(&pane.header_row), line);
            assert_eq!(width(&pane.header_center), 0);

            header(Header::standard().left, vec![Part::Stripe]);
            assert!(parent().is_some_and(|p| p == pane.header_center));
            assert_eq!(width(&pane.header_row), line);

            header(Header::standard().left, Header::standard().center);
            assert!(parent().is_none());
            assert!(pane.title_part.parent().is_some_and(|p| p == pane.header_center));
        });
    }

    #[test]
    fn the_center_row_steps_aside_from_the_corners_and_hides_only_without_room() {
        // A wide chart: centred, with room on either side.
        assert_eq!(center_start(1200, 160, 300, 40), Some(450));
        // A half-width chart: centred, a title would run under the corner,
        // so it slides left until it clears it.
        assert_eq!(center_start(496, 106, 95, 218), Some(175));
        // Or right, off the symbol, when that is the wider of the two.
        assert_eq!(center_start(600, 250, 100, 40), Some(258));
        // Narrow: no room between the two at all.
        assert_eq!(center_start(500, 160, 300, 40), None);
    }

    #[test]
    fn an_untitled_center_spans_the_room_between_the_two_sides() {
        // A title, or the strip, is as wide as it is, where it fits.
        assert_eq!(center_place(1200, 160, 90, 40, false), Some((555, 90)));
        // An untitled chart's lone title takes all the top between the two
        // sides, with the same air either side.
        assert_eq!(center_place(1200, 160, 90, 40, true), Some((168, 984)));
        assert_eq!(center_place(496, 106, 95, 218, true), Some((114, 156)));
        // And no more than a title would have where there is no room.
        assert_eq!(center_place(300, 160, 90, 100, true), None);
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
