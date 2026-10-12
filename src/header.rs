//! What a chart's header shows across its top, and where.
//!
//! Two rows, each with a left, a centre and a right, as a desktop bar has:
//! every part of the header, the symbol, the trading controls and the
//! chart's own buttons alike, goes in one of the six, in an order. One header
//! for the whole app, every chart in every chartbook showing the same unless
//! it wears a look of its own ([`crate::chart_look`]), so that changing it
//! changes all of them at once.

use crate::store::Store;

/// The header every chart shows, as [`spell`] writes it. A plain list with
/// no zones is how the left was written before there were zones.
pub const SETTING: &str = "chart_header";
/// What the header showed centred across the chart's top, and in its
/// top-right corner, when it had three sections stored apart. Read only to
/// bring a header written then forward, while [`SETTING`] has no zones.
pub const CENTER_SETTING: &str = "chart_header_center";
pub const RIGHT_SETTING: &str = "chart_header_right";

/// The switch the resolution strip had before it was a part of the header.
/// Read only to bring a header written then forward.
const OLD_STRIP_SETTING: &str = "resolution_strip";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Symbol,
    Resolution,
    /// The day's move, as a percentage, in the chart's own up and down colours.
    Change,
    /// The last price, as the rail has it.
    Last,
    /// The name of the chart template the chart is on; nothing when it is on
    /// none.
    Template,
    /// A window pulled off another, dimmed, on a chart whose look differs
    /// from every chart's; nothing on one that looks like every other.
    Look,
    /// The instrument's full name, faded.
    Name,
    /// The chart title, given in its menu, its settings or with
    /// `chart set --title`. Clicked, it is edited where it stands. On a chart
    /// with none it is blank until the pointer is on it.
    Title,
    /// Which trading account the chart is on. Nothing at all while there
    /// is at most one account to be on.
    Account,
    /// A dot in the colour of the chart's data feed's connection, which
    /// names the feed and says how it stands when hovered, and offers the
    /// other feeds when there are any. Nothing at all with Yahoo alone:
    /// nothing to choose, and nothing to connect to.
    Feed,
    /// The account's P&L for the day, closed and open: "P&L +$1,250.00".
    /// Nothing at all unless the chart has an account to trade on, or
    /// while its trading service has not said.
    DayPnl,
    /// Buy and Sell, at the market, for the chart's quantity: what Ctrl+B
    /// and Ctrl+S do. Nothing at all unless the chart has an account to
    /// trade on.
    Trade,
    /// How many an order from the chart is for, "Qty 2". Nothing at all
    /// unless the chart has an account to trade on.
    Size,
    /// The position the chart trades, its size signed and its open P&L in
    /// brackets: "+2 (+$125.00)", "-1 (-0.50R)". Nothing at all
    /// unless the chart's account holds a position in what it trades.
    Position,
    /// The preset resolutions as buttons. On the focused chart only.
    Strip,
    /// The gear that opens the chart's settings.
    Settings,
    /// The corner that fills the window with the chart. Only when there is
    /// another chart to maximize away from.
    Maximize,
    /// The chain that says how the chart follows the rail.
    Link,
}

impl Part {
    pub const ALL: [Part; 18] = [
        Part::Symbol,
        Part::Resolution,
        Part::Change,
        Part::Last,
        Part::Template,
        Part::Look,
        Part::Name,
        Part::Title,
        Part::Account,
        Part::Feed,
        Part::DayPnl,
        Part::Trade,
        Part::Size,
        Part::Position,
        Part::Strip,
        Part::Settings,
        Part::Maximize,
        Part::Link,
    ];

    /// How it is written down and typed.
    pub fn key(self) -> &'static str {
        match self {
            Part::Symbol => "symbol",
            Part::Resolution => "resolution",
            Part::Change => "change",
            Part::Last => "last",
            Part::Template => "template",
            Part::Look => "look",
            Part::Name => "name",
            Part::Title => "title",
            Part::Account => "account",
            Part::Feed => "feed",
            Part::DayPnl => "day_pnl",
            Part::Trade => "trade",
            Part::Size => "size",
            Part::Position => "position",
            Part::Strip => "strip",
            Part::Settings => "settings",
            Part::Maximize => "maximize",
            Part::Link => "link",
        }
    }

    /// Any way it has been written: the strip was the `stripe` too.
    pub fn from_key(key: &str) -> Option<Part> {
        match key {
            "stripe" => Some(Part::Strip),
            key => Part::ALL.into_iter().find(|part| part.key() == key),
        }
    }

    /// How a list of them names it.
    pub fn label(self) -> &'static str {
        match self {
            Part::Symbol => "Symbol",
            Part::Resolution => "Resolution",
            Part::Change => "Daily change",
            Part::Last => "Last price",
            Part::Template => "Chart template",
            Part::Look => "Local visuals",
            Part::Name => "Name",
            Part::Title => "Chart title",
            Part::Account => "Account",
            Part::Feed => "Data feed",
            Part::DayPnl => "P&L",
            Part::Trade => "Buy Sell",
            Part::Size => "Quantity",
            Part::Position => "Current position",
            Part::Strip => "Resolution strip",
            Part::Settings => "Chart settings",
            Part::Maximize => "Maximize",
            Part::Link => "Link mode",
        }
    }

    /// Where it goes when it is put back without being placed: the words on
    /// the left, the title and the strip in the middle, trading's controls
    /// and the chart's buttons on the right.
    pub fn home(self) -> Zone {
        match self {
            Part::Title | Part::Strip => Zone::Center,
            Part::DayPnl | Part::Trade | Part::Size | Part::Position | Part::Settings | Part::Maximize | Part::Link => {
                Zone::Right
            }
            _ => Zone::Left,
        }
    }

    /// Is it one of trading's: shown, offered and typed only while
    /// trading is switched on ([`crate::features`]).
    pub fn trades(self) -> bool {
        matches!(self, Part::Account | Part::DayPnl | Part::Trade | Part::Size | Part::Position)
    }

    /// Can it be shown, offered and typed, as trading is switched now?
    pub fn shown(self) -> bool {
        crate::features::trading() || !self.trades()
    }

    /// Every part that can be, in [`Part::ALL`]'s order.
    pub fn every_shown() -> Vec<Part> {
        Part::ALL.into_iter().filter(|part| part.shown()).collect()
    }
}

/// One of the header's six places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Left,
    Center,
    Right,
    Left2,
    Center2,
    Right2,
}

impl Zone {
    pub const ALL: [Zone; 6] = [Zone::Left, Zone::Center, Zone::Right, Zone::Left2, Zone::Center2, Zone::Right2];

    pub fn key(self) -> &'static str {
        match self {
            Zone::Left => "left",
            Zone::Center => "center",
            Zone::Right => "right",
            Zone::Left2 => "left2",
            Zone::Center2 => "center2",
            Zone::Right2 => "right2",
        }
    }

    pub fn from_key(key: &str) -> Option<Zone> {
        Zone::ALL.into_iter().find(|zone| zone.key() == key)
    }

    /// 0 for the first row, 1 for the second.
    pub fn row(self) -> usize {
        self.index() / 3
    }

    /// The same place on the first row.
    pub fn first_row(self) -> Zone {
        match self {
            Zone::Left2 => Zone::Left,
            Zone::Center2 => Zone::Center,
            Zone::Right2 => Zone::Right,
            zone => zone,
        }
    }

    /// Where it is in [`Zone::ALL`], which lists the zones as they are
    /// declared.
    fn index(self) -> usize {
        self as usize
    }
}

/// Which parts the header shows, in which zone, in what order.
///
/// Every way of building one goes through [`Layout::settle`], so two things
/// always hold: no part is in it twice, and a place on the second row is
/// empty whenever the place above it shows nothing — a part under nothing
/// would only be a first-row part pushed down for no reason. "Shows" counts
/// only parts that can be shown: with trading off, its parts are kept where
/// they were but hold nothing up.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Layout {
    zones: [Vec<Part>; 6],
}

impl Layout {
    /// What a chart shows when nobody has chosen: on the left the data
    /// feed's dot, the symbol, its resolution and the day's move; the
    /// chart's title across the middle; on the right the account, the day's
    /// P&L, Buy, Sell, the quantity and the position held, then the chart's
    /// buttons. Trading's parts and the dot draw nothing until there is
    /// something for them to say.
    pub fn standard() -> Layout {
        let mut layout = Layout::default();
        layout.zones[Zone::Left.index()] = vec![Part::Feed, Part::Symbol, Part::Resolution, Part::Change];
        layout.zones[Zone::Center.index()] = vec![Part::Title];
        layout.zones[Zone::Right.index()] =
            vec![Part::Account, Part::DayPnl, Part::Trade, Part::Size, Part::Position, Part::Settings, Part::Maximize, Part::Link];
        layout
    }

    pub fn zone(&self, zone: Zone) -> &[Part] {
        &self.zones[zone.index()]
    }

    /// What `zone` shows of the parts that can be.
    pub fn shown(&self, zone: Zone) -> Vec<Part> {
        self.zone(zone).iter().copied().filter(|part| part.shown()).collect()
    }

    /// The header as it can be seen: without trading's parts while it is
    /// off. What a header is printed as, so that what is printed can be
    /// typed back.
    pub fn seen(&self) -> Layout {
        let mut seen = Layout { zones: std::array::from_fn(|at| self.shown(Zone::ALL[at])) };
        seen.settle();
        seen
    }

    pub fn contains(&self, part: Part) -> bool {
        self.zone_of(part).is_some()
    }

    pub fn zone_of(&self, part: Part) -> Option<Zone> {
        Zone::ALL.into_iter().find(|zone| self.zone(*zone).contains(&part))
    }

    /// Whether the second row shows anything.
    pub fn second_row(&self) -> bool {
        Zone::ALL[3..].iter().any(|zone| self.shows_other_than(*zone, None))
    }

    /// Whether `zone` can take `part`: any place on the first row, one on
    /// the second only while the place above it shows something besides
    /// `part`. Taken from the only place above it, `part` would leave that
    /// place empty, and [`Layout::settle`] would move it straight back up.
    pub fn takes(&self, zone: Zone, part: Part) -> bool {
        zone.row() == 0 || self.shows_other_than(zone.first_row(), Some(part))
    }

    fn shows_other_than(&self, zone: Zone, part: Option<Part>) -> bool {
        self.zone(zone).iter().any(|p| Some(*p) != part && p.shown())
    }

    /// Take `part` out, wherever it is.
    pub fn remove(&mut self, part: Part) {
        self.take(part);
        self.settle();
    }

    /// Put `part` in `zone` at `at`, from wherever it was. Past the end puts
    /// it last.
    pub fn place(&mut self, part: Part, zone: Zone, at: usize) {
        self.take(part);
        let list = &mut self.zones[zone.index()];
        list.insert(at.min(list.len()), part);
        self.settle();
    }

    /// Put `part` at the end of its home zone, if it is not shown already.
    pub fn add(&mut self, part: Part) {
        if !self.contains(part) {
            let at = self.zone(part.home()).len();
            self.place(part, part.home(), at);
        }
    }

    /// Put back, where `old` had them, the parts it holds that cannot be
    /// shown with trading off and this one lacks: a header typed or arranged
    /// while trading is off is typed over what anybody can see, and keeps
    /// trading's parts for when it is on.
    pub fn keep_hidden(&mut self, old: &Layout) {
        for zone in Zone::ALL {
            for (at, part) in old.zone(zone).iter().copied().enumerate() {
                if !part.shown() && !self.contains(part) {
                    let list = &mut self.zones[zone.index()];
                    list.insert(at.min(list.len()), part);
                }
            }
        }
        self.settle();
    }

    /// Take `part` out of whichever zone holds it, leaving the settling to
    /// the caller.
    fn take(&mut self, part: Part) {
        for zone in &mut self.zones {
            zone.retain(|p| *p != part);
        }
    }

    fn settle(&mut self) {
        let mut seen = Vec::new();
        for zone in &mut self.zones {
            zone.retain(|part| match seen.contains(part) {
                true => false,
                false => {
                    seen.push(*part);
                    true
                }
            });
        }
        for zone in Zone::ALL[3..].iter().copied() {
            if !self.shows_other_than(zone.first_row(), None) {
                let parts = std::mem::take(&mut self.zones[zone.index()]);
                self.zones[zone.first_row().index()].extend(parts);
            }
        }
    }

    /// The header as it was written before it had zones: `left` on the first
    /// row's left, the name on a row of its own under it, where it used to
    /// sit; the middle and the right as they were stored apart, or as they
    /// ship. `strip` is whether the strip's old switch was on.
    fn from_sections(left: &[Part], center: Option<Vec<Part>>, right: Option<Vec<Part>>, strip: bool) -> Layout {
        let mut layout = Layout::standard();
        if let Some(center) = center {
            layout.zones[Zone::Center.index()] = center;
        }
        if let Some(right) = right {
            layout.zones[Zone::Right.index()] = right;
        }
        layout.zones[Zone::Left.index()].clear();
        // The left is what was stored first, and wins a part named twice.
        for part in left {
            let zone = if *part == Part::Name { Zone::Left2 } else { Zone::Left };
            layout.take(*part);
            layout.zones[zone.index()].push(*part);
        }
        if !strip {
            layout.take(Part::Strip);
        }
        layout.settle();
        layout
    }
}

/// The header as typed: `left=symbol,resolution center=strip right=settings`,
/// any of the six zones, each at most once, the rest empty; `none` for a
/// header with nothing in it.
///
/// A plain list with no zones, `symbol,resolution,change`, is the way the
/// left was written before there were zones, and still means what it did:
/// those on the left, the name under them, the rest as it ships.
///
/// Refuses a name it does not know and a part or zone named twice, rather
/// than dropping either: a header that came out different from the one typed
/// would look as though the command worked. With trading switched off, its
/// parts are not parts of a header at all.
pub fn parse(text: &str) -> Result<Layout, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("none") {
        return Ok(Layout::default());
    }
    if !text.contains('=') {
        return Ok(Layout::from_sections(&parse_list(text)?, None, None, true));
    }
    let mut named: Vec<Part> = Vec::new();
    let mut layout = Layout::default();
    let mut zones_named: Vec<Zone> = Vec::new();
    for field in text.split_whitespace() {
        let (zone, list) = field.split_once('=').ok_or_else(|| format!("{field:?} names no zone; write it as left={field}"))?;
        let zone = zone.trim().to_lowercase();
        let zone = Zone::from_key(&zone).ok_or_else(|| {
            format!("{zone:?} is not a place in the header; try {}", Zone::ALL.map(Zone::key).join(", "))
        })?;
        if zones_named.contains(&zone) {
            return Err(format!("{} is named twice", zone.key()));
        }
        zones_named.push(zone);
        for part in parse_list(list)? {
            if named.contains(&part) {
                return Err(format!("{} is named twice", part.key()));
            }
            named.push(part);
            layout.zones[zone.index()].push(part);
        }
    }
    layout.settle();
    Ok(layout)
}

/// `symbol,resolution,change`, as typed: one zone's parts, or none.
///
/// Refuses a name it does not know, or that is one of trading's while it is
/// switched off, and a part named twice.
pub fn parse_list(text: &str) -> Result<Vec<Part>, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    for key in text.split(',').map(|key| key.trim().to_lowercase()).filter(|key| !key.is_empty()) {
        let Some(part) = Part::from_key(&key).filter(|part| part.shown()) else {
            return Err(format!(
                "{key:?} is not part of a header; try {}",
                Part::every_shown().into_iter().map(Part::key).collect::<Vec<_>>().join(", ")
            ));
        };
        if parts.contains(&part) {
            return Err(format!("{key} is named twice"));
        }
        parts.push(part);
    }
    Ok(parts)
}

/// The way [`parse`] reads it: the zones with something in them, in order.
pub fn spell(layout: &Layout) -> String {
    let fields: Vec<String> = Zone::ALL
        .into_iter()
        .filter(|zone| !layout.zone(*zone).is_empty())
        .map(|zone| {
            let keys: Vec<&str> = layout.zone(zone).iter().map(|part| part.key()).collect();
            format!("{}={}", zone.key(), keys.join(","))
        })
        .collect();
    match fields.is_empty() {
        true => "none".to_string(),
        false => fields.join(" "),
    }
}

/// The parts named in `list`, in order, skipping any this build does not
/// know and any named twice — a header written by a newer build keeps what
/// this one can show rather than losing the whole arrangement.
fn lenient(list: &str) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for part in list.split(',').filter_map(|key| Part::from_key(&key.trim().to_lowercase())) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

/// A header as [`spell`] writes it, read leniently where [`parse`] is
/// strict: a part or a zone only a newer build knows is left out rather than
/// costing the whole header, since a newer build may have written it. A
/// plain list is the left from before zones, the rest as it ships; `strip`
/// whether the strip's old switch was on.
pub fn read(text: &str, strip: bool) -> Layout {
    let text = text.trim();
    if text.eq_ignore_ascii_case("none") {
        return Layout::default();
    }
    if !text.contains('=') {
        return Layout::from_sections(&lenient(text), None, None, strip);
    }
    let mut layout = Layout::default();
    for (zone, list) in text.split_whitespace().filter_map(|field| field.split_once('=')) {
        if let Some(zone) = Zone::from_key(&zone.trim().to_lowercase()) {
            layout.zones[zone.index()].extend(lenient(list));
        }
    }
    layout.settle();
    layout
}

/// The standard headers of builds that kept three sections apart, as
/// Preferences wrote them out once anybody touched it or restored it: the
/// one with the day's P&L ending the left, the one with the strip in the
/// center before charts had titles, the one with the account ending the
/// left, the one before the day's move, and the one before the position
/// held. Read as [`Layout::standard`], so a header that was only ever the
/// standard one follows the standard. One that differs from them in anything
/// is somebody's, and kept.
const OLD_STANDARDS: [[(&str, &str); 3]; 5] = [
    [
        (SETTING, "feed,symbol,resolution,account,day_pnl"),
        (CENTER_SETTING, "stripe"),
        (RIGHT_SETTING, "trade,size,settings,maximize,link"),
    ],
    [
        (SETTING, "feed,symbol,resolution,account"),
        (CENTER_SETTING, "stripe"),
        (RIGHT_SETTING, "day_pnl,trade,size,settings,maximize,link"),
    ],
    [
        (SETTING, "feed,symbol,resolution,account"),
        (CENTER_SETTING, "title"),
        (RIGHT_SETTING, "day_pnl,trade,size,settings,maximize,link"),
    ],
    [
        (SETTING, "feed,symbol,resolution"),
        (CENTER_SETTING, "title"),
        (RIGHT_SETTING, "account,day_pnl,trade,size,settings,maximize,link"),
    ],
    [
        (SETTING, "feed,symbol,resolution,change"),
        (CENTER_SETTING, "title"),
        (RIGHT_SETTING, "account,day_pnl,trade,size,settings,maximize,link"),
    ],
];

/// The standard header as the one setting spelled it before the position
/// held was in it: read as [`Layout::standard`], so a header only ever
/// restored to the standard gains the position after the quantity.
const BEFORE_POSITION: &str =
    "left=feed,symbol,resolution,change center=title right=account,day_pnl,trade,size,settings,maximize,link";

/// The header every chart shows.
pub fn load(store: &Store) -> Layout {
    let strip = store.setting_bool(OLD_STRIP_SETTING, true);
    let Some(text) = store.setting(SETTING) else {
        return Layout::standard();
    };
    // `none` was the left alone while the other two were stored apart; with
    // neither left, it is a header with nothing in it.
    let apart = store.has_setting(CENTER_SETTING) || store.has_setting(RIGHT_SETTING);
    if text.trim() == BEFORE_POSITION {
        return Layout::standard();
    }
    if text.contains('=') || (!apart && text.trim().eq_ignore_ascii_case("none")) {
        return read(&text, strip);
    }
    // Written as sections kept apart: a section never stored is as it ships.
    let old_standard = OLD_STANDARDS.iter().any(|old| {
        old.iter().all(|(key, value)| store.setting(key).is_none_or(|text| text == *value))
    });
    if old_standard {
        return Layout::standard();
    }
    // A section stored as `none` names no part, so it reads as empty.
    let section = |key: &str| store.setting(key).map(|text| lenient(&text));
    Layout::from_sections(&lenient(&text), section(CENTER_SETTING), section(RIGHT_SETTING), strip)
}

/// Every chart's header, in the one setting: the sections a header was
/// once stored in apart are forgotten, having been brought forward.
pub fn save(store: &Store, layout: &Layout) {
    store.set_setting(SETTING, &spell(layout));
    store.remove_setting(CENTER_SETTING);
    store.remove_setting(RIGHT_SETTING);
}

/// A day's move as the header writes it: `+1.25%`.
pub fn change_text(change_pct: f64) -> String {
    format!("{change_pct:+.2}%")
}

/// The colour a day's move is written in: `scheme`'s rise or fall, the
/// chart's look turned only as a red-up chart turns it, so Monochrome or
/// Quiet candles still have a coloured rise here. A move of exactly
/// nothing has none, and is written in the header's own text.
pub fn change_colour(scheme: &omacharts_engine::BarScheme, change: f64) -> Option<String> {
    use omacharts_engine::Direction;
    match Direction::of_change(change) {
        Direction::Flat => None,
        direction => Some(scheme.outline(direction).to_string()),
    }
}

/// `words` as markup, in `colour` when there is one: how the header writes
/// a move.
pub fn change_markup(words: &str, colour: Option<&str>) -> String {
    let words = gtk::glib::markup_escape_text(words);
    match colour {
        Some(colour) => format!("<span foreground=\"{}\">{words}</span>", gtk::glib::markup_escape_text(colour)),
        None => words.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{force, Feature};

    /// `text` as typed with trading on, so every part is one.
    fn layout(text: &str) -> Layout {
        force(Feature::Trading, true);
        parse(text).unwrap()
    }

    #[test]
    fn a_zone_is_where_it_stands_in_all() {
        for (at, zone) in Zone::ALL.into_iter().enumerate() {
            assert_eq!(zone.index(), at, "{zone:?}");
        }
    }

    #[test]
    fn a_header_is_typed_and_written_the_same_way() {
        let typed = "left=Symbol,change center=strip right=settings,link left2=NAME";
        let header = layout(typed);
        assert_eq!(header.zone(Zone::Left), [Part::Symbol, Part::Change]);
        assert_eq!(header.zone(Zone::Left2), [Part::Name]);
        assert_eq!(spell(&header), "left=symbol,change center=strip right=settings,link left2=name");
        assert_eq!(layout(&spell(&header)), header);
        assert_eq!(layout("none"), Layout::default());
        assert_eq!(spell(&Layout::default()), "none");
        assert_eq!(
            spell(&Layout::standard()),
            "left=feed,symbol,resolution,change center=title right=account,day_pnl,trade,size,position,settings,maximize,link"
        );
        assert_eq!(layout("left=stripe").zone(Zone::Left), [Part::Strip], "the strip's old name");
    }

    #[test]
    fn what_is_not_a_header_is_refused() {
        force(Feature::Trading, true);
        assert!(parse("left=symbol,price").is_err());
        assert!(parse("left=symbol right=symbol").is_err());
        assert!(parse("left=symbol left=change").is_err());
        assert!(parse("top=symbol").is_err());
        assert!(parse("left=symbol change").is_err());
        assert!(parse("symbol,symbol").is_err());
    }

    /// A list with no zones is the left, as it was before there were zones:
    /// the name under it, the rest as it ships.
    #[test]
    fn a_plain_list_is_the_left() {
        let header = layout("symbol,change,name");
        assert_eq!(header.zone(Zone::Left), [Part::Symbol, Part::Change]);
        assert_eq!(header.zone(Zone::Left2), [Part::Name]);
        assert_eq!(header.zone(Zone::Center), Layout::standard().zone(Zone::Center));
        assert_eq!(header.zone(Zone::Right), Layout::standard().zone(Zone::Right));
    }

    /// A place on the second row holds something only under a place that
    /// shows something: emptied above, it moves up, and under an empty
    /// place it takes nothing.
    #[test]
    fn a_second_row_place_under_an_empty_one_moves_up() {
        let header = layout("left2=name right2=change");
        assert_eq!(header.zone(Zone::Left), [Part::Name]);
        assert_eq!(header.zone(Zone::Right), [Part::Change]);
        assert!(!header.second_row());

        let mut header = layout("left=symbol right=settings,link left2=name right2=change");
        assert!(header.takes(Zone::Right2, Part::Symbol));
        assert!(!header.takes(Zone::Center2, Part::Title), "nothing in the centre above");
        assert!(!header.takes(Zone::Left2, Part::Symbol), "the left above would be empty");
        assert!(header.takes(Zone::Center, Part::Symbol));
        header.remove(Part::Symbol);
        assert_eq!(header.zone(Zone::Left), [Part::Name]);
        assert!(header.zone(Zone::Left2).is_empty());
        assert_eq!(header.zone(Zone::Right2), [Part::Change], "the right above still shows something");
        header.remove(Part::Settings);
        header.remove(Part::Link);
        assert_eq!(header.zone(Zone::Right), [Part::Change]);
        assert!(!header.second_row());
        header.place(Part::Title, Zone::Center2, 0);
        assert_eq!(header.zone(Zone::Center), [Part::Title], "placed under nothing, it lands above");
    }

    /// With trading off its parts hold nothing up: a place that holds only
    /// them is empty to the place under it, though they stay in it.
    #[test]
    fn trading_s_hidden_parts_hold_no_row_up() {
        use crate::features::reset;
        let typed = "left=symbol right=account,trade right2=name";
        force(Feature::Trading, true);
        let header = layout(typed);
        assert_eq!(header.zone(Zone::Right2), [Part::Name]);
        assert!(header.takes(Zone::Right2, Part::Symbol));
        force(Feature::Trading, false);
        assert!(!header.takes(Zone::Right2, Part::Symbol));
        let header = read(typed, true);
        assert_eq!(header.zone(Zone::Right), [Part::Account, Part::Trade, Part::Name]);
        assert!(!header.second_row());
        let header = read("left=symbol left2=trade", true);
        assert!(!header.second_row(), "only a hidden part under it");
        reset();
    }

    #[test]
    fn a_part_moves_rather_than_doubling() {
        let mut header = Layout::standard();
        header.place(Part::Title, Zone::Right, 0);
        assert!(header.zone(Zone::Center).is_empty());
        assert_eq!(header.zone(Zone::Right)[0], Part::Title);
        header.place(Part::Name, Zone::Left2, 9);
        assert_eq!(header.zone(Zone::Left2), [Part::Name]);
        header.add(Part::Name);
        assert_eq!(header.zone_of(Part::Name), Some(Zone::Left2));
        header.add(Part::Strip);
        assert_eq!(header.zone(Zone::Center), [Part::Strip]);
        assert!(header.takes(Zone::Left2, Part::Symbol));
        assert!(!layout("left=symbol").takes(Zone::Left2, Part::Symbol));
    }

    #[test]
    fn what_is_saved_is_what_every_chart_shows() {
        let store = Store::memory().unwrap();
        assert_eq!(load(&store), Layout::standard());
        let header = layout("left=change,symbol right=link");
        save(&store, &header);
        assert_eq!(load(&store), header);
        save(&store, &Layout::default());
        assert_eq!(load(&store), Layout::default());
    }

    /// The standard header as the one setting spelled it before the
    /// position held was in it is the standard one now, the position after
    /// the quantity; one of anybody's own is kept as it was saved, without it.
    #[test]
    fn the_standard_header_from_before_the_position_gains_it() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, BEFORE_POSITION);
        assert_eq!(load(&store), Layout::standard());
        assert!(load(&store).zone(Zone::Right).contains(&Part::Position));
        store.set_setting(SETTING, "left=symbol right=account,trade,size,link");
        assert!(!load(&store).contains(Part::Position), "a header of its own");
        assert_eq!(Part::Position.label(), "Current position");
        assert_eq!(Part::from_key("position"), Some(Part::Position));
    }

    /// A header stored before zones keeps what it showed: its line on the
    /// left, the sections stored apart in the middle and on the right, and
    /// the strip's old switch.
    #[test]
    fn an_old_header_comes_forward() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "symbol,change,name");
        let header = load(&store);
        assert_eq!(header.zone(Zone::Left), [Part::Symbol, Part::Change]);
        assert_eq!(header.zone(Zone::Left2), [Part::Name]);
        assert_eq!(header.zone(Zone::Right), Layout::standard().zone(Zone::Right));

        store.set_setting(CENTER_SETTING, "stripe,change,settings");
        store.set_setting(RIGHT_SETTING, "settings,change,link");
        let header = load(&store);
        assert_eq!(header.zone(Zone::Center), [Part::Strip, Part::Settings], "the left wins a part named twice");
        assert_eq!(header.zone(Zone::Right), [Part::Link]);

        store.set_setting_bool(OLD_STRIP_SETTING, false);
        assert!(!load(&store).contains(Part::Strip));
        store.set_setting(SETTING, "none");
        assert!(load(&store).zone(Zone::Left).is_empty());
        assert_eq!(load(&store).zone(Zone::Right), [Part::Link], "the left alone was none");

        // Saved once, the header is the one setting, and the sections are
        // forgotten.
        save(&store, &load(&store));
        assert!(!store.has_setting(CENTER_SETTING) && !store.has_setting(RIGHT_SETTING));
        save(&store, &Layout::default());
        assert_eq!(load(&store), Layout::default());
    }

    /// The standard header a build with sections kept apart wrote out is
    /// the standard one now; one that differs from it in anything is kept.
    #[test]
    fn an_old_standard_header_is_the_standard() {
        let store = Store::memory().unwrap();
        for old in OLD_STANDARDS {
            for (key, value) in old {
                store.set_setting(key, value);
            }
            assert_eq!(load(&store), Layout::standard());
        }
        store.set_setting(CENTER_SETTING, "stripe");
        store.set_setting(SETTING, "symbol,resolution");
        let kept = load(&store);
        assert_eq!(kept.zone(Zone::Left), [Part::Symbol, Part::Resolution]);
        assert_eq!(kept.zone(Zone::Center), [Part::Strip]);
    }

    /// A newer build's part or zone is dropped, not the header: the rest
    /// still shows.
    #[test]
    fn what_this_build_does_not_know_is_left_out() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "left=volume,change,symbol,change top=link right=link");
        let header = load(&store);
        assert_eq!(header.zone(Zone::Left), [Part::Change, Part::Symbol]);
        assert_eq!(header.zone(Zone::Right), [Part::Link]);
        store.set_setting(SETTING, "volume,change");
        assert_eq!(load(&store).zone(Zone::Left), [Part::Change]);
    }

    /// With trading off its parts are no header's: not typed, not
    /// suggested, and those a header holds are kept where they were by a
    /// change to the parts that are shown.
    #[test]
    fn trading_s_parts_are_kept_out_of_sight_while_it_is_off() {
        force(Feature::Trading, true);
        assert!(parse_list("symbol,trade").is_ok());
        assert_eq!(Part::every_shown(), Part::ALL.to_vec());
        force(Feature::Trading, false);
        assert!(parse_list("symbol,trade").is_err());
        let refused = parse_list("banana").unwrap_err();
        assert!(!refused.contains("day_pnl") && refused.contains("symbol"), "{refused}");
        assert!(parse("left=account").is_err());
        assert!(Part::every_shown().iter().all(|part| !part.trades()));

        let header = Layout::standard();
        assert_eq!(header.shown(Zone::Left), [Part::Feed, Part::Symbol, Part::Resolution, Part::Change]);
        let mut typed = parse("left=symbol,feed right=link").unwrap();
        typed.keep_hidden(&header);
        assert_eq!(typed.zone(Zone::Left), [Part::Symbol, Part::Feed]);
        assert_eq!(typed.zone(Zone::Right), [Part::Account, Part::DayPnl, Part::Trade, Part::Size, Part::Position, Part::Link]);
        assert!(typed.zone(Zone::Center).is_empty());
        assert_eq!(spell(&typed.seen()), "left=symbol,feed right=link");
    }

    /// A rise in the scheme's up colour and a fall in its down; nothing at
    /// all is the text.
    #[test]
    fn a_change_is_coloured_from_the_scheme() {
        let theme = omacharts_engine::theme::builtin_themes().remove(0);
        let scheme = omacharts_engine::theme_bars(&theme);
        assert_eq!(change_colour(&scheme, 0.01), Some(scheme.up.clone()), "a cent up");
        assert_eq!(change_colour(&scheme, -0.01), Some(scheme.down.clone()), "a cent down");
        assert_eq!(change_colour(&scheme, 0.0), None);
    }

    #[test]
    fn a_change_reads_with_its_sign() {
        assert_eq!(change_text(-1.0), "-1.00%");
        assert_eq!(change_text(0.256), "+0.26%");
    }
}
