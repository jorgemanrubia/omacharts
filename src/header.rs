//! What a chart's header shows: the line at its top-left, the row centred
//! across its top, and in what order.
//!
//! One for the whole app, every chart in every chartbook showing the same
//! header, so that changing it changes all of them at once. Each part is in
//! the header once, in one section or the other. Until somebody chooses, each
//! is where [`Part::home`] puts it, which is how every chart looked before
//! there was a choice.

use crate::store::Store;

/// What the header shows on the left: the line it always had, so a header
/// stored before there were sections is read as this one.
pub const SETTING: &str = "chart_header";
/// What the header shows centred across the chart's top.
pub const CENTER_SETTING: &str = "chart_header_center";

/// Where on a chart's top a part is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Left,
    Center,
}

impl Section {
    pub const ALL: [Section; 2] = [Section::Left, Section::Center];

    /// The setting it is stored in.
    pub fn setting(self) -> &'static str {
        match self {
            Section::Left => SETTING,
            Section::Center => CENTER_SETTING,
        }
    }

    /// How the preferences name it.
    pub fn title(self) -> &'static str {
        match self {
            Section::Left => "Left",
            Section::Center => "Center",
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
    /// Which data source the chart is from, and any it was passed over.
    /// Nothing at all while there is only one source to be from.
    Account,
    /// How many an order from the chart is for. Nothing at all without a
    /// trading account.
    Size,
    /// The preset resolutions as a row of buttons, on the focused chart only.
    Stripe,
}

impl Part {
    pub const ALL: [Part; 7] =
        [Part::Symbol, Part::Resolution, Part::Change, Part::Template, Part::Account, Part::Size, Part::Stripe];

    /// The section it is in until somebody places it, or `None` for a part
    /// that is only there once somebody adds it. The account and the size
    /// are at home on the left because they cost nothing: with one source,
    /// or with no trading account, each draws nothing and takes no room.
    pub fn home(self) -> Option<Section> {
        match self {
            Part::Symbol => Some(Section::Left),
            Part::Resolution => Some(Section::Left),
            Part::Change => None,
            Part::Template => None,
            Part::Account => Some(Section::Left),
            Part::Size => Some(Section::Left),
            Part::Stripe => Some(Section::Center),
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
            Part::Size => "size",
            Part::Stripe => "stripe",
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
            Part::Size => "Trading size",
            Part::Stripe => "Resolutions stripe",
        }
    }
}

/// The whole header: what each section shows, in order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Header {
    pub left: Vec<Part>,
    pub center: Vec<Part>,
}

impl Header {
    /// What a chart shows when nobody has chosen.
    pub fn standard() -> Header {
        Header { left: standard(Section::Left), center: standard(Section::Center) }
    }

    pub fn section(&self, section: Section) -> &[Part] {
        match section {
            Section::Left => &self.left,
            Section::Center => &self.center,
        }
    }

    pub fn contains(&self, part: Part) -> bool {
        self.left.contains(&part) || self.center.contains(&part)
    }

    /// Put `parts` in `section`, taking each out of the other: a part is in
    /// the header once, wherever it was put last.
    pub fn set(&mut self, section: Section, parts: Vec<Part>) {
        let (into, other) = match section {
            Section::Left => (&mut self.left, &mut self.center),
            Section::Center => (&mut self.center, &mut self.left),
        };
        other.retain(|part| !parts.contains(part));
        *into = parts;
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
/// its line is the left, and the stripe stays in the center. A part stored in
/// both is on the left, the section that was always there.
pub fn load(store: &Store) -> Header {
    let left = load_section(store, Section::Left);
    let mut center = load_section(store, Section::Center);
    center.retain(|part| !left.contains(part));
    Header { left, center }
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
        let header = Header { left: vec![Part::Change, Part::Symbol], center: vec![Part::Stripe] };
        save(&store, &header);
        assert_eq!(load(&store), header);
        let empty = Header { left: Vec::new(), center: Vec::new() };
        save(&store, &empty);
        assert_eq!(load(&store), empty);
    }

    /// Out of the box the left is the line every chart always had, and the
    /// center the stripe it always had across its top.
    #[test]
    fn the_standard_header_is_the_line_on_the_left_and_the_stripe_in_the_center() {
        let header = Header::standard();
        assert_eq!(header.left, vec![Part::Symbol, Part::Resolution, Part::Account, Part::Size]);
        assert_eq!(header.center, vec![Part::Stripe]);
    }

    /// A header stored before there were sections keeps its line on the
    /// left, and has the stripe in the center, where it already was.
    #[test]
    fn a_header_stored_before_sections_is_unchanged() {
        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "change,symbol,account");
        assert_eq!(
            load(&store),
            Header { left: vec![Part::Change, Part::Symbol, Part::Account], center: vec![Part::Stripe] }
        );
        store.set_setting(SETTING, "none");
        assert_eq!(load(&store), Header { left: Vec::new(), center: vec![Part::Stripe] });
    }

    /// Placing a part in one section takes it out of the other; and a part
    /// stored in both, which only a hand-written setting can do, is shown
    /// once, on the left.
    #[test]
    fn a_part_is_in_the_header_once() {
        let mut header = Header::standard();
        header.set(Section::Left, vec![Part::Symbol, Part::Stripe]);
        assert_eq!(header, Header { left: vec![Part::Symbol, Part::Stripe], center: Vec::new() });
        header.set(Section::Center, vec![Part::Symbol]);
        assert_eq!(header, Header { left: vec![Part::Stripe], center: vec![Part::Symbol] });

        let store = Store::memory().unwrap();
        store.set_setting(SETTING, "symbol,stripe");
        store.set_setting(CENTER_SETTING, "stripe,change");
        assert_eq!(load(&store), Header { left: vec![Part::Symbol, Part::Stripe], center: vec![Part::Change] });
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
