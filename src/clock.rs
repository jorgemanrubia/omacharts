//! Which clock the charts read, and how they write what it says.
//!
//! Two settings for the whole app, as the header is: every chart's time axis,
//! its crosshair, and every field a time is typed into follow them, so a time
//! read off the axis is the time typed into an editor. See
//! [`omacharts_engine::clock`] for what the values mean.

use omacharts_engine::clock::{ChartZone, TimeFormat};

use crate::store::Store;

/// The time zone charts are drawn in: an IANA name, or `exchange` for each
/// chart in its own exchange's clock.
pub const SETTING_ZONE: &str = "chart_time_zone";
/// `24h` or `12h`.
pub const SETTING_FORMAT: &str = "time_format";

pub fn zone(store: &Store) -> ChartZone {
    store.setting(SETTING_ZONE).and_then(|key| ChartZone::from_key(&key)).unwrap_or_default()
}

/// How times are written: as chosen, or as the desktop's own clock writes
/// them until somebody chooses.
pub fn format(store: &Store) -> TimeFormat {
    store
        .setting(SETTING_FORMAT)
        .and_then(|key| TimeFormat::from_key(&key))
        .or_else(desktop_format)
        .unwrap_or_default()
}

/// The desktop's clock format, where it keeps one: GNOME's
/// `clock-format`, which most of what runs on a Linux desktop reads too. Asked
/// only when the schema is installed, because asking GSettings for one that is
/// not aborts the process.
fn desktop_format() -> Option<TimeFormat> {
    use gtk::gio;
    use gtk::prelude::SettingsExt;
    const SCHEMA: &str = "org.gnome.desktop.interface";
    let source = gio::SettingsSchemaSource::default()?;
    let schema = source.lookup(SCHEMA, true)?;
    if !schema.has_key("clock-format") {
        return None;
    }
    TimeFormat::from_key(&gio::Settings::new(SCHEMA).string("clock-format"))
}

pub fn save_zone(store: &Store, zone: ChartZone) {
    store.set_setting(SETTING_ZONE, &zone.key());
}

pub fn save_format(store: &Store, format: TimeFormat) {
    store.set_setting(SETTING_FORMAT, format.key());
}

/// Read a zone as `config set` is given one, for the message a typo gets.
pub fn parse_zone(value: &str) -> Result<ChartZone, String> {
    ChartZone::from_key(value).ok_or_else(|| {
        format!("{value:?} is not a time zone; use an IANA name such as Europe/London, or `exchange`")
    })
}

pub fn parse_format(value: &str) -> Result<TimeFormat, String> {
    TimeFormat::from_key(value).ok_or_else(|| format!("{value:?} is not a time format; use 24h or 12h"))
}
