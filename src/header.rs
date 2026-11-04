//! What a chart's header shows: the line at its top-left, the row centred
//! across its top, the corner at its top-right, and in what order.
//!
//! One for the whole app, every chart in every chartbook showing the same
//! header, so that changing it changes all of them at once. Each part is in
//! the header once, in one section of the three. Until somebody chooses, each
//! is where [`Part::home`] puts it, which is how every chart looked before
//! there was a choice.

use crate::store::Store;

/// What the header shows on the left: the line it always had, so a header
/// stored before there were sections is read as this one.
pub const SETTING: &str = "chart_header";
/// What the header shows centred across the chart's top.
pub const CENTER_SETTING: &str = "chart_header_center";
/// What the header shows in the chart's top-right corner.
pub const RIGHT_SETTING: &str = "chart_header_right";

/// Where on a chart's top a part is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Left,
    Center,
    Right,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Left, Section::Center, Section::Right];

    /// The setting it is stored in.
    pub fn setting(self) -> &'static str {
        match self {
            Section::Left => SETTING,
            Section::Center => CENTER_SETTING,
            Section::Right => RIGHT_SETTING,
        }
    }

    /// How the preferences name it.
    pub fn title(self) -> &'static str {
        match self {
            Section::Left => "Left",
            Section::Center => "Center",
            Section::Right => "Right",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Part {
    Symbol,
    Resolution,
    /// The day's move, as a percentage, in the chart's own up and down colours.
    Change,
    /// The name of the chart template the chart is on; nothing when it is on
    /// none.
    Template,
    /// Which trading account the chart is on. Nothing at all while there
    /// is at most one account to be on.
    Account,
    /// A dot in the colour of the chart's data feed's connection, which
    /// names the feed and says how it stands when hovered, and offers the
    /// other feeds when there are any. Nothing at all with Yahoo alone:
    /// nothing to choose, and nothing to connect to.
    Feed,
    /// How many an order from the chart is for. Nothing at all without a
    /// trading account.
    Size,
    /// The preset resolutions as a row of buttons, on the focused chart only.
    Stripe,
    /// The gear that opens the chart's settings.
    Settings,
    /// The corner that grows the chart to fill the window, with a second
    /// chart on screen.
    Maximize,
    /// The chain that says what the watchlist does to the chart.
    Link,
}

impl Part {
    pub const ALL: [Part; 11] = [
        Part::Symbol,
        Part::Resolution,
        Part::Change,
        Part::Template,
        Part::Account,
        Part::Feed,
        Part::Size,
        Part::Stripe,
        Part::Settings,
        Part::Maximize,
        Part::Link,
    ];

    /// The section it is in until somebody places it, or `None` for a part
    /// that is only there once somebody adds it. The account, the feed and
    /// the size are at home on the left because they cost nothing: with one
    /// account, one feed, or no trading account, each draws nothing and
    /// takes no room. The three controls are at home in the corner they
    /// always stood in, in the order they stood.
    pub fn home(self) -> Option<Section> {
        match self {
            Part::Symbol => Some(Section::Left),
            Part::Resolution => Some(Section::Left),
            Part::Change => None,
            Part::Template => None,
            Part::Account => Some(Section::Left),
            Part::Feed => Some(Section::Left),
            Part::Size => Some(Section::Left),
            Part::Stripe => Some(Section::Center),
            Part::Settings => Some(Section::Right),
            Part::Maximize => Some(Section::Right),
            Part::Link => Some(Section::Right),
        }
    }

    /// How it is written down and typed.
    pub fn key(self) -> &'static str {
        match self {
            Part::Symbol => "symbol",
            Part::Resolution => "resolution",
            Part::Change => "change",
            Part::Template => "template",
            Part::Account => "account",
            Part::Feed => "feed",
            Part::Size => "size",
            Part::Stripe => "stripe",
            Part::Settings => "settings",
            Part::Maximize => "maximize",
            Part::Link => "link",
        }
    }

    pub fn from_key(key: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|part| part.key() == key)
    }

    /// How a list of them names it.
    pub fn label(self) -> &'static str {
        match self {
            Part::Symbol => "Symbol",
            Part::Resolution => "Resolution",
            Part::Change => "Daily change",
            Part::Template => "Chart template",
            Part::Account => "Account",
            Part::Feed => "Data feed",
            Part::Size => "Trading size",
            Part::Stripe => "Resolutions stripe",
            Part::Settings => "Chart settings",
            Part::Maximize => "Maximize",
            Part::Link => "Watchlist link",
        }
    }
}

/// The whole header: what each section shows, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header {
    pub left: Vec<Part>,
    pub center: Vec<Part>,
    pub right: Vec<Part>,
}

impl Header {
    /// What a chart shows when nobody has chosen.
    pub fn standard() -> Header {
        Header {
            left: standard(Section::Left),
            center: standard(Section::Center),
            right: standard(Section::Right),
        }
    }

    pub fn section(&self, section: Section) -> &[Part] {
        match section {
            Section::Left => &self.left,
            Section::Center => &self.center,
            Section::Right => &self.right,
        }
    }

    fn section_mut(&mut self, section: Section) -> &mut Vec<Part> {
        match section {
            Section::Left => &mut self.left,
            Section::Center => &mut self.center,
            Section::Right => &mut self.right,
        }
    }

    pub fn contains(&self, part: Part) -> bool {
        Section::ALL.into_iter().any(|section| self.section(section).contains(&part))
    }

    /// Put `parts` in `section`, taking each out of the others: a part is
    /// in the header once, wherever it was put last.
    pub fn set(&mut self, section: Section, parts: Vec<Part>) {
        for other in Section::ALL.into_iter().filter(|other| *other != section) {
            self.section_mut(other).retain(|part| !parts.contains(part));
        }
        *self.section_mut(section) = parts;
    }
}

/// What `section` shows when nobody has chosen: the parts at home in it, in
/// the order [`Part::ALL`] lists them.
pub fn standard(section: Section) -> Vec<Part> {
    Part::ALL.into_iter().filter(|part| part.home() == Some(section)).collect()
}

/// The parts named in `keys`, in order, skipping any this build does not
/// know — a header written by a newer build keeps what this one can show
/// rather than losing the whole arrangement it was stored in.
pub fn from_keys<S: AsRef<str>>(keys: &[S]) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for part in keys.iter().filter_map(|key| Part::from_key(key.as_ref())) {
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts
}

pub fn keys(parts: &[Part]) -> Vec<String> {
    parts.iter().map(|part| part.key().to_string()).collect()
}

/// `symbol,resolution,change`, as typed; `none` for an empty section.
///
/// Refuses a name it does not know and a part named twice, rather than
/// dropping either: a header that came out different from the one typed would
/// look as though the command worked.
pub fn parse(text: &str) -> Result<Vec<Part>, String> {
    let text = text.trim();
    if text.is_empty() || text.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    for key in text.split(',').map(|key| key.trim().to_lowercase()) {
        let Some(part) = Part::from_key(&key) else {
            return Err(format!(
                "{key:?} is not part of a header; try {}",
                Part::ALL.map(Part::key).join(", ")
            ));
        };
        if parts.contains(&part) {
            return Err(format!("{key} is named twice"));
        }
        parts.push(part);
    }
    Ok(parts)
}

/// The way [`parse`] reads it.
pub fn spell(parts: &[Part]) -> String {
    match parts.is_empty() {
        true => "none".to_string(),
        false => keys(parts).join(","),
    }
}

/// The header every chart shows.
///
/// Read leniently, where [`parse`] is strict: a part only a newer build knows
/// is left out rather than costing the whole line, since the settings may
/// have been written by one. A section never stored is its standard one,
/// which is what keeps a header stored before there were sections as it was:
/// its line is the left, the stripe stays in the center, and the controls
/// stay in the corner. A part stored in two sections is in the first of
/// them, the left being the section that was always there.
pub fn load(store: &Store) -> Header {
    let mut header = Header { left: Vec::new(), center: Vec::new(), right: Vec::new() };
    let mut placed: Vec<Part> = Vec::new();
    for section in Section::ALL {
        let mut parts = load_section(store, section);
        parts.retain(|part| !placed.contains(part));
        placed.extend(parts.iter().copied());
        *header.section_mut(section) = parts;
    }
    header
}

fn load_section(store: &Store, section: Section) -> Vec<Part> {
    match store.setting(section.setting()) {
        None => standard(section),
        Some(text) if text.trim().eq_ignore_ascii_case("none") => Vec::new(),
        Some(text) => from_keys(&text.split(',').map(str::trim).collect::<Vec<_>>()),
    }
}

pub fn save(store: &Store, header: &Header) {
    store.set_setting(SETTING, &spell(&header.left));
    store.set_setting(CENTER_SETTING, &spell(&header.center));
    store.set_setting(RIGHT_SETTING, &spell(&header.right));
}

/// A day's move as the header writes it: `+1.25%`.
pub fn change_text(change_pct: f64) -> String {
    format!("{change_pct:+.2}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_is_typed_and_written_the_same_way() {
        let parts = parse("Symbol, change,template").unwrap();
        assert_eq!(parts, vec![Part::Symbol, Part::Change, Part::Template]);
        assert_eq!(spell(&parts), "symbol,change,template");
        assert_eq!(parse("stripe").unwrap(), vec![Part::Stripe]);
        assert_eq!(parse("none").unwrap(), Vec::new());
        assert_eq!(spell(&[]), "none");
        assert!(parse("symbol,price").is_err());
        assert!(parse("symbol,symbol").is_err());
    }

    #[test]
    fn what_is_saved_is_what_every_chart_shows() {
        let store = Store::memory().unwrap();
        assert_eq!(load(&store), Header::standard());
        let header =
            Header { left: vec![Part::Change, Part::Symbol], center: vec![Part::Stripe], right: vec![Part::Settings] };
        save(&store, &header);
        assert_eq!(load(&store), header);
        let empty = Header { left: Vec::new(), center: Vec::new(), right: Vec::new() };
        save(&store, &empty);
        assert_eq!(load(&store), empty);
    }

    /// Out of the box the left is the line every chart always had, the
    /// center the stripe it always had across its top, and the right the
    /// three controls that always stood in its corner, in their order.
    #[test]
    fn the_standard_header_is_the_line_the_stripe_and_the_corner() {
        let header = Header::standard();
        assert_eq!(header.left, vec![Part::Symbol, Part::Resolution, Part::Account, Part::Feed, Part::Size]);
        assert_eq!(header.center, vec![Part::Stripe]);
        assert_eq!(header.right, vec![Part::Settings, Part::Maximize, Part::Link]);
    }

    /// A header stored before there were sections keeps its line on the
    /// left, and has the stripe in the center and the controls in the
    /// corner, where they already were.
    #[test]
    fn a_header_stored_before_sections_is_unchanged() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "change,symbol,account");
        assert_eq!(
            load(&store),
            Header {
                left: vec![Part::Change, Part::Symbol, Part::Account],
                center: vec![Part::Stripe],
                right: vec![Part::Settings, Part::Maximize, Part::Link],
            }
        );
        store.set_setting(SETTING, "none");
        assert_eq!(load(&store).left, Vec::new());
        assert_eq!(load(&store).center, vec![Part::Stripe]);
    }

    /// Placing a part in one section takes it out of the others; and a
    /// part stored in two, which only a hand-written setting can do, is
    /// shown once, in the first of them.
    #[test]
    fn a_part_is_in_the_header_once() {
        let corner = vec![Part::Settings, Part::Maximize, Part::Link];
        let mut header = Header::standard();
        header.set(Section::Left, vec![Part::Symbol, Part::Stripe]);
        assert_eq!(header, Header { left: vec![Part::Symbol, Part::Stripe], center: Vec::new(), right: corner.clone() });
        header.set(Section::Center, vec![Part::Symbol]);
        assert_eq!(header, Header { left: vec![Part::Stripe], center: vec![Part::Symbol], right: corner.clone() });
        header.set(Section::Right, vec![Part::Stripe, Part::Settings]);
        assert_eq!(
            header,
            Header { left: Vec::new(), center: vec![Part::Symbol], right: vec![Part::Stripe, Part::Settings] }
        );

        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "symbol,stripe");
        store.set_setting(CENTER_SETTING, "stripe,change,settings");
        store.set_setting(RIGHT_SETTING, "settings,change,link");
        assert_eq!(
            load(&store),
            Header { left: vec![Part::Symbol, Part::Stripe], center: vec![Part::Change, Part::Settings], right: vec![Part::Link] }
        );
    }

    /// The corner's controls are parts like the others: typed by name,
    /// placed in any section, and taken out.
    #[test]
    fn the_corner_s_controls_are_parts() {
        assert_eq!(parse("settings,maximize,link").unwrap(), vec![Part::Settings, Part::Maximize, Part::Link]);
        assert_eq!(parse("feed,account").unwrap(), vec![Part::Feed, Part::Account]);
        let store = Store::memory().unwrap();
        store.set_setting(RIGHT_SETTING, "none");
        store.set_setting(SETTING, "settings,symbol");
        let header = load(&store);
        assert_eq!(header.right, Vec::new());
        assert_eq!(header.left, vec![Part::Settings, Part::Symbol]);
        assert!(!header.contains(Part::Maximize));
    }

    /// A newer build's part is dropped, not the header: the rest still shows.
    #[test]
    fn a_part_this_build_does_not_know_is_left_out() {
        assert_eq!(from_keys(&["symbol", "volume", "symbol", "change"]), vec![Part::Symbol, Part::Change]);
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "volume,change,symbol");
        assert_eq!(load(&store).left, vec![Part::Change, Part::Symbol]);
    }

    #[test]
    fn a_change_reads_with_its_sign() {
        assert_eq!(change_text(-1.0), "-1.00%");
        assert_eq!(change_text(0.256), "+0.26%");
    }
}
