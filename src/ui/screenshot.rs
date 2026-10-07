//! Pictures of charts.
//!
//! A screenshot of a chart is the chart and what names it — the candles, the
//! scales, the symbol, the resolution, the indicator legend — and none of the
//! things that are only there to be clicked. The gear, the chain, the
//! resolution buttons and the maximize corner mean nothing in an image: a
//! picture of a button nobody can press is a picture of nothing.
//!
//! The hard constraint is that taking one must not change anything on screen,
//! not for a frame. That rules out hiding the controls, snapshotting and
//! putting them back: those are three separate things that happen across
//! frames, GTK lays out and paints in between, and an early return anywhere in
//! the middle leaves the user's chart with no gear button.
//!
//! So nothing is touched. [`gtk::prelude::WidgetExt::snapshot_child`] asks a
//! widget to draw one of its children into a snapshot of our own, which is a
//! read: it allocates nothing, invalidates nothing and queues no redraw. What
//! is left out is left out by walking the tree ourselves and simply not asking
//! for it. The image is then the pixels that are on screen minus the ones we
//! never requested — which is also why it needs no second drawing of the
//! legend in a different font from the real one.
//!
//! Two things are deliberately dropped that are not controls, for the same
//! reason as each other: neither is part of what the chart shows.
//!
//! The focus ring is the window's answer to "where is the keyboard", and it
//! goes with the rest of the furniture.
//!
//! The crosshair goes too, with the pointer's own labels and the corner boxes
//! a strip wears while the pointer is in it. They say where the mouse is, and
//! a saved picture has no mouse — a dashed cross frozen wherever the hand
//! happened to be is a fact about the moment the key was pressed rather than
//! about the market. It also makes a chart's picture the same picture whether
//! or not somebody was pointing at it when they asked, and it takes the strip
//! controls out of an image they only ever reached by accident. Reading a
//! price off the crosshair is what the chart is for; reading it off a
//! screenshot of the crosshair is not. The axes and the last-price chip stay,
//! and those are what carry the scale.
//!
//! All of it comes out by class, because the crosshair is now a widget of its
//! own over the chart — which is why that layer exists at all, and what makes
//! leaving it out a matter of not asking for one child.
//!
//! Where it goes is the clipboard, always. That is what somebody pressing the
//! key wants nine times out of ten — the picture lands in the chat, the note,
//! the issue — and it is the one destination no setting governs. Keeping a PNG
//! as well is the part that is optional, because a folder quietly filling with
//! images you pasted once is a cost somebody should be able to decline.

use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gdk, gio, graphene, gsk};
use omacharts_engine::Theme;

use crate::store::Store;
use crate::ui::chart::POINTER_LAYER;
use crate::ui::colors;
use crate::ui::pane::ChartPane;

/// Where screenshots are written. Unset means the default, which is derived
/// rather than stored: writing it down on first run would freeze today's
/// answer for everyone who never chose.
pub const SETTING_FOLDER: &str = "screenshot_folder";

/// Whether a PNG is kept as well as copied. Unset means on.
pub const SETTING_SAVE_FILE: &str = "screenshot_save_file";

/// The folder made inside the user's pictures directory. Capitalised the way
/// the application is.
pub const FOLDER: &str = "Omacharts";

/// The widgets a screenshot leaves out, by the CSS class they already carry.
///
/// By class rather than by identity because that is the one description that
/// is already true: each of these is styled as what it is, so a new control
/// added to the legend inherits being left out rather than having to be
/// remembered here.
///
/// - `timeframe-strip` — the segmented resolution buttons. The resolution
///   itself still appears: it is written beside the symbol as plain text, and
///   that label is information and stays.
/// - `legend-gear`, `legend-link` — chart settings, and the link group.
/// - `legend-button` — a row's hide, settings and remove buttons.
/// - `pane-expand` — the maximize corner.
/// - `pane-clock` — the time now, which is not the time of anything in the
///   picture and would date it to the second for no reason.
/// - [`POINTER_LAYER`] — the crosshair, its axis labels, and the boxes
///   a strip wears under the pointer. Not a control, and the only thing here
///   that is not: it is where the mouse is, and a picture has no mouse.
const LEFT_OUT: &[&str] = &[
    "timeframe-strip",
    "legend-gear",
    "legend-link",
    "legend-button",
    "pane-expand",
    "pane-clock",
    POINTER_LAYER,
];

/// A rendered screenshot, not yet anywhere.
///
/// A texture and a name, because those are the two things its destinations
/// want: the clipboard takes the texture, and a file needs something to be
/// called. The display comes along because the clipboard belongs to it rather
/// than to any window, which is also why putting an image there cannot disturb
/// what is on screen.
pub struct Shot {
    texture: gdk::Texture,
    display: gdk::Display,
    /// What to call the file, without a folder and without `.png`.
    stem: String,
}

impl Shot {
    /// Put it on the clipboard.
    ///
    /// The texture itself, not an encoding of it: GDK registers serializers
    /// from `GdkTexture` to `image/png` and the other image types, so a
    /// receiver that wants PNG bytes is offered them and one that can take the
    /// texture directly avoids the round trip.
    pub fn copy(&self) {
        self.display.clipboard().set_texture(&self.texture);
    }

    /// Write it next to whatever else is in `folder`, under a name nothing
    /// else there has.
    pub fn save_into(&self, folder: &Path) -> Result<PathBuf, String> {
        make_folder(folder)?;
        let path = free_path(folder, &self.stem);
        self.save_as(&path)?;
        Ok(path)
    }

    /// Write it exactly here, folder and all.
    pub fn save_as(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            make_folder(parent)?;
        }
        self.texture
            .save_to_png(path)
            .map_err(|error| format!("{} could not be written: {error}", path.display()))
    }
}

// -- what the GUI calls ----------------------------------------------------

/// Ctrl+O: a picture of one chart.
///
/// Everything on screen is left exactly as it is, including the chart this is
/// a picture of.
pub fn take_chart(
    window: &adw::ApplicationWindow,
    store: &Store,
    pane: &ChartPane,
    theme: &Theme,
) {
    deliver(window, store, chart(pane, theme), "Chart");
}

/// Ctrl+Shift+O: a picture of every chart in the arrangement, as laid out.
pub fn take_chartbook(
    window: &adw::ApplicationWindow,
    store: &Store,
    area: &impl IsA<gtk::Widget>,
    book: &str,
    theme: &Theme,
) {
    deliver(window, store, chartbook(area, book, theme), "Chartbook");
}

/// Copy it, keep it if that is wanted, and say in one line what happened.
fn deliver(
    window: &adw::ApplicationWindow,
    store: &Store,
    shot: Result<Shot, String>,
    what: &str,
) {
    let shot = match shot {
        Ok(shot) => shot,
        Err(error) => return tell(window, "Screenshot not taken", &error, true),
    };
    // First, and whatever happens next. A screenshot that reached the
    // clipboard is a screenshot, whether or not a file was also kept.
    shot.copy();

    let copied = format!("{what} screenshot copied");
    if !save_file(store) {
        return tell(window, &copied, "Ready to paste", false);
    }
    match shot.save_into(&folder(store)) {
        Ok(path) => tell(window, &copied, &format!("Also saved to {}", readable(&path)), false),
        // The picture is not lost — it is on the clipboard — so this says what
        // was wanted and then what went wrong, in that order.
        Err(error) => tell(window, &copied, &format!("Not saved: {error}"), true),
    }
}

/// One line, outside the window.
///
/// A desktop notification rather than anything drawn in the window: it is what
/// every other screenshot tool on this desktop does, it cannot appear in a
/// picture of the window, and it needs nothing added to the window to carry
/// it. A banner inside the app would mean an `AdwToastOverlay` around the
/// content, which is a reasonable thing to want for its own sake but is not
/// something this should invent on its way past.
fn tell(window: &adw::ApplicationWindow, headline: &str, body: &str, wrong: bool) {
    let Some(app) = window.application() else { return };
    let notification = gio::Notification::new(headline);
    notification.set_body(Some(body));
    if wrong {
        notification.set_priority(gio::NotificationPriority::High);
    }
    // One id, so a run of screenshots replaces its own notification rather
    // than stacking six of them down the corner of the screen.
    app.send_notification(Some("omacharts-screenshot"), &notification);
}

// -- what a command calls --------------------------------------------------

/// A command's screenshot of one chart: a file, and the clipboard only if the
/// command said so.
///
/// Where the keyboard means "give me this image", a command means "write this
/// file", and the difference is not a default to be flipped. A loop taking
/// fifty screenshots would stamp fifty times on whatever somebody had copied,
/// and an agent working in the background has no business owning the
/// clipboard at all — so the setting that governs the window is not read here,
/// and copying happens only when `clipboard` says to.
pub fn write_chart(
    store: &Store,
    pane: &ChartPane,
    theme: &Theme,
    into: Option<&Path>,
    clipboard: bool,
) -> Result<PathBuf, String> {
    write(chart(pane, theme)?, store, into, clipboard)
}

/// The same for the whole arrangement.
pub fn write_chartbook(
    store: &Store,
    area: &impl IsA<gtk::Widget>,
    book: &str,
    theme: &Theme,
    into: Option<&Path>,
    clipboard: bool,
) -> Result<PathBuf, String> {
    write(chartbook(area, book, theme)?, store, into, clipboard)
}

fn write(
    shot: Shot,
    store: &Store,
    into: Option<&Path>,
    clipboard: bool,
) -> Result<PathBuf, String> {
    if clipboard {
        shot.copy();
    }
    match into {
        // A directory given where a file was expected is a thing a script does
        // on purpose, so it means "name it yourself, in here".
        Some(path) if path.is_dir() => shot.save_into(path),
        Some(path) => shot.save_as(path).map(|()| path.to_path_buf()),
        None => shot.save_into(&folder(store)),
    }
}

// -- rendering -------------------------------------------------------------

/// One chart, named by what it shows.
pub fn chart(pane: &ChartPane, theme: &Theme) -> Result<Shot, String> {
    let symbol = pane
        .instrument
        .borrow()
        .as_ref()
        .map(|instrument| instrument.display_symbol())
        .unwrap_or_default();
    let stem = chart_stem(&symbol, &pane.timeframe.get().label(), &now());
    let root: &gtk::Widget = pane.root.upcast_ref();
    Ok(Shot { texture: render(root, theme)?, display: root.display(), stem })
}

/// Every chart in the arrangement, as laid out, dividers and all.
pub fn chartbook(
    area: &impl IsA<gtk::Widget>,
    book: &str,
    theme: &Theme,
) -> Result<Shot, String> {
    let stem = book_stem(book, &now());
    let root = area.as_ref();
    Ok(Shot { texture: render(root, theme)?, display: root.display(), stem })
}

/// Draw `root` and everything under it except the controls.
///
/// The colour goes down first because the containers whose own background is
/// skipped are skipped for a reason: `.chart-pane` is descended into rather
/// than drawn, since the controls are inside it, and descending leaves the
/// hairline of its border unpainted. The chart's own background is what shows
/// through there on screen, so it is what shows through here.
fn render(root: &gtk::Widget, theme: &Theme) -> Result<gdk::Texture, String> {
    let (width, height) = (root.width() as f32, root.height() as f32);
    if width < 1.0 || height < 1.0 {
        return Err("the chart has no size on screen yet".to_string());
    }
    // A renderer of our own, rather than the one drawing the window.
    //
    // The window's renderer is busy drawing the window, and the one rule this
    // module is built around is that a screenshot touches nothing that is on
    // screen. The cairo renderer is GSK's universal fallback — there is no
    // node type it cannot draw — so what comes out is the same picture
    // whatever display backend the app happens to be running on.
    let surface = root.native().and_then(|native| native.surface());
    let renderer = gsk::CairoRenderer::new();
    renderer
        .realize(surface.as_ref())
        .map_err(|error| format!("a screenshot could not be set up: {error}"))?;

    // The screen's own scale, so a chart on a HiDPI display is saved at the
    // pixels it is actually drawn with. Everything below is in the logical
    // units the widgets are laid out in, and this is the only place that knows
    // the difference.
    let scale = root.scale_factor().max(1) as f32;
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(scale, scale);
    snapshot.append_color(
        &colors::parse(&theme.ui.background),
        &graphene::Rect::new(0.0, 0.0, width, height),
    );
    draw_children(root, &snapshot);
    let node = snapshot.to_node().ok_or("there was nothing to draw")?;
    let viewport = graphene::Rect::new(0.0, 0.0, width * scale, height * scale);
    let texture = renderer.render_texture(&node, Some(&viewport));
    renderer.unrealize();
    Ok(texture)
}

/// Ask `parent` for each child that belongs in the picture.
///
/// A child with nothing left out anywhere under it is handed straight to GTK,
/// which draws the whole subtree with its own transform. One that holds
/// something left out is descended into instead, and then placing it is ours
/// to do.
///
/// Placed by moving to where its origin sits in the parent, rather than by
/// applying the transform `compute_transform` hands back. That transform is a
/// 4×4 matrix, and a matrix is opaque: GSK files one as a transform of unknown
/// category, which its cairo path refuses to draw and fills with magenta
/// instead. Every widget in a chart is laid out by position alone, so a move
/// says the same thing and says it in the form everything can draw.
fn draw_children(parent: &gtk::Widget, snapshot: &gtk::Snapshot) {
    let mut next = parent.first_child();
    while let Some(child) = next {
        next = child.next_sibling();
        if !child.is_visible() || left_out(&child) {
            continue;
        }
        if holds_left_out(&child) {
            snapshot.save();
            if let Some(at) = child.compute_point(parent, &graphene::Point::new(0.0, 0.0)) {
                snapshot.translate(&at);
            }
            draw_children(&child, snapshot);
            snapshot.restore();
        } else {
            parent.snapshot_child(&child, snapshot);
        }
    }
}

fn left_out(widget: &gtk::Widget) -> bool {
    LEFT_OUT.iter().any(|class| widget.has_css_class(class))
}

fn holds_left_out(widget: &gtk::Widget) -> bool {
    let mut child = widget.first_child();
    while let Some(found) = child {
        if left_out(&found) || holds_left_out(&found) {
            return true;
        }
        child = found.next_sibling();
    }
    false
}

// -- where it goes ---------------------------------------------------------

/// The folder screenshots are written to.
pub fn folder(store: &Store) -> PathBuf {
    match store.setting(SETTING_FOLDER).filter(|set| !set.trim().is_empty()) {
        Some(set) => expand_home(set.trim(), &crate::store::home()),
        None => default_folder(),
    }
}

/// `<Pictures>/Omacharts`.
pub fn default_folder() -> PathBuf {
    pictures().join(FOLDER)
}

pub fn save_file(store: &Store) -> bool {
    store.setting_bool(SETTING_SAVE_FILE, true)
}

/// The user's pictures directory.
///
/// Read rather than assumed: it is localised and relocatable, so a machine set
/// up in Spanish keeps its screenshots in `Imágenes` and one with a mounted
/// media disk keeps them wherever that is. `~/Pictures` is the last resort,
/// not the first guess.
fn pictures() -> PathBuf {
    let home = crate::store::home();
    if let Some(set) = std::env::var_os("XDG_PICTURES_DIR").filter(|set| !set.is_empty()) {
        return expand_home(&set.to_string_lossy(), &home);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|base| !base.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    std::fs::read_to_string(config.join("user-dirs.dirs"))
        .ok()
        .and_then(|text| pictures_in(&text, &home))
        .unwrap_or_else(|| home.join("Pictures"))
}

/// Pull `XDG_PICTURES_DIR` out of a `user-dirs.dirs`.
///
/// The file is a shell fragment the desktop writes, so the value is quoted and
/// written against `$HOME` — and a commented-out line is not a setting, which
/// is what the leading `#` in a default file means.
fn pictures_in(text: &str, home: &Path) -> Option<PathBuf> {
    for line in text.lines().map(str::trim) {
        let Some(value) = line.strip_prefix("XDG_PICTURES_DIR=") else { continue };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match value.strip_prefix("$HOME") {
            Some(rest) if !rest.trim_start_matches('/').is_empty() => {
                return Some(home.join(rest.trim_start_matches('/')));
            }
            Some(_) => return Some(home.to_path_buf()),
            None if value.is_empty() => continue,
            None => return Some(expand_home(value, home)),
        }
    }
    None
}

/// `~/Pictures` written literally is still the home directory.
///
/// Nothing in the app writes a `~`, but `config set` takes whatever it is
/// handed, and a path typed by a person who expected a shell to be involved
/// should not quietly make a directory called `~`.
fn expand_home(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}

/// Make the folder, the first time a screenshot needs it.
///
/// On demand rather than at startup: a chart application that puts a directory
/// in your Pictures the moment it is installed is presumptuous.
fn make_folder(folder: &Path) -> Result<(), String> {
    if folder.is_dir() {
        return Ok(());
    }
    if folder.exists() {
        return Err(format!("{} is a file, not a folder", readable(folder)));
    }
    std::fs::create_dir_all(folder)
        .map_err(|error| format!("{} could not be made: {error}", readable(folder)))
}

/// A path in `folder` that nothing is using.
///
/// Two screenshots inside one second is a thing a held key does, and the
/// second one overwriting the first silently would be the worst of the
/// answers available.
fn free_path(folder: &Path, stem: &str) -> PathBuf {
    let first = folder.join(format!("{stem}.png"));
    if !first.exists() {
        return first;
    }
    for n in 2.. {
        let next = folder.join(format!("{stem}-{n}.png"));
        if !next.exists() {
            return next;
        }
    }
    first
}

/// A path as a person would write it, so a message about one is readable.
fn readable(path: &Path) -> String {
    let home = crate::store::home();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

// -- what it is called -----------------------------------------------------

/// `NVDA-1h-20261004-143012`.
///
/// The symbol first, so a folder of these groups by what is in them, and the
/// timestamp after it so each group is in order. Seconds because a screenshot
/// is a thing people take several of in a row.
fn chart_stem(symbol: &str, resolution: &str, at: &str) -> String {
    let parts = [tidy(symbol), tidy(resolution)];
    let named: Vec<&str> = parts.iter().map(String::as_str).filter(|p| !p.is_empty()).collect();
    if named.is_empty() {
        return format!("chart-{at}");
    }
    format!("{}-{at}", named.join("-"))
}

/// `Macro-20261004-143012`, and `chartbook-…` for a book nobody has named.
fn book_stem(book: &str, at: &str) -> String {
    match tidy(book) {
        name if name.is_empty() => format!("chartbook-{at}"),
        name => format!("{name}-{at}"),
    }
}

/// Make a filename out of a symbol or a chartbook's name.
///
/// Symbols are not words: `^GSPC`, `ES=F` and `BRK.B` are all real, and a
/// chartbook is called whatever somebody typed. Anything that is not plainly
/// safe in a filename becomes a dash, runs of them collapse, and the ends are
/// trimmed — so `^GSPC` is `GSPC` rather than a file starting with a dash,
/// which half the command line reads as a flag.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches(['-', '.']).to_string()
}

fn now() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The clipboard is not a setting and the file is, so a fresh install
    /// keeps one — and the switch that turns it off has to be the only thing
    /// that can.
    #[test]
    fn a_png_is_kept_unless_somebody_says_otherwise() {
        let store = Store::memory().expect("a store");
        assert!(save_file(&store), "a fresh install keeps the file");
        store.set_setting_bool(SETTING_SAVE_FILE, false);
        assert!(!save_file(&store), "and stops when it is told to");
        store.set_setting_bool(SETTING_SAVE_FILE, true);
        assert!(save_file(&store));
    }

    #[test]
    fn a_chart_is_named_by_what_it_shows_and_when() {
        assert_eq!(chart_stem("NVDA", "1h", "20261004-143012"), "NVDA-1h-20261004-143012");
    }

    #[test]
    fn a_chart_with_no_symbol_yet_is_still_named() {
        assert_eq!(chart_stem("", "1D", "20261004-143012"), "1D-20261004-143012");
        assert_eq!(chart_stem("", "", "20261004-143012"), "chart-20261004-143012");
    }

    #[test]
    fn a_chartbook_is_named_after_the_book() {
        assert_eq!(book_stem("Macro", "20261004-143012"), "Macro-20261004-143012");
        assert_eq!(book_stem("  ", "20261004-143012"), "chartbook-20261004-143012");
    }

    /// Every one of these is a symbol the app will chart.
    #[test]
    fn a_symbol_that_is_not_a_word_still_makes_a_filename() {
        assert_eq!(tidy("^GSPC"), "GSPC");
        assert_eq!(tidy("ES=F"), "ES-F");
        assert_eq!(tidy("BRK.B"), "BRK.B");
        assert_eq!(tidy("EUR/USD"), "EUR-USD");
    }

    /// A name that came from a person can hold anything, including the two
    /// characters that would put the file somewhere else entirely.
    #[test]
    fn a_name_cannot_escape_the_folder_it_is_saved_in() {
        for name in ["../../etc/passwd", "/etc/passwd", "..", "."] {
            let stem = book_stem(name, "20261004-143012");
            assert!(!stem.contains('/'), "{stem}");
            assert!(!stem.starts_with('.'), "{stem}");
            assert!(!stem.starts_with('-'), "{stem}");
        }
    }

    #[test]
    fn the_pictures_directory_is_read_from_the_desktops_own_answer() {
        let home = Path::new("/home/someone");
        let text = "# This file is written by xdg-user-dirs-update\n\
                    XDG_DESKTOP_DIR=\"$HOME/Desktop\"\n\
                    XDG_PICTURES_DIR=\"$HOME/Imágenes\"\n";
        assert_eq!(pictures_in(text, home), Some(PathBuf::from("/home/someone/Imágenes")));
    }

    #[test]
    fn a_pictures_directory_somewhere_else_entirely_is_taken_as_written() {
        let home = Path::new("/home/someone");
        let text = "XDG_PICTURES_DIR=\"/mnt/media/pictures\"\n";
        assert_eq!(pictures_in(text, home), Some(PathBuf::from("/mnt/media/pictures")));
    }

    #[test]
    fn a_file_with_no_pictures_line_falls_through() {
        let home = Path::new("/home/someone");
        assert_eq!(pictures_in("XDG_MUSIC_DIR=\"$HOME/Music\"\n", home), None);
        assert_eq!(pictures_in("", home), None);
    }

    #[test]
    fn a_tilde_in_a_configured_folder_is_the_home_directory() {
        let home = Path::new("/home/someone");
        assert_eq!(expand_home("~/shots", home), PathBuf::from("/home/someone/shots"));
        assert_eq!(expand_home("~", home), PathBuf::from("/home/someone"));
        assert_eq!(expand_home("/tmp/shots", home), PathBuf::from("/tmp/shots"));
        // Not a home directory reference, and not ours to rewrite.
        assert_eq!(expand_home("~someone/shots", home), PathBuf::from("~someone/shots"));
    }

    #[test]
    fn two_screenshots_in_the_same_second_do_not_overwrite_each_other() {
        let folder = std::env::temp_dir().join(format!("omacharts-shot-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a temporary folder");

        let named = |path: PathBuf| {
            path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default()
        };

        let first = free_path(&folder, "NVDA-1h-20261004-143012");
        assert_eq!(named(first.clone()), "NVDA-1h-20261004-143012.png");
        std::fs::write(&first, b"").expect("a file");

        let second = free_path(&folder, "NVDA-1h-20261004-143012");
        assert_eq!(named(second.clone()), "NVDA-1h-20261004-143012-2.png");
        std::fs::write(&second, b"").expect("a file");

        let third = free_path(&folder, "NVDA-1h-20261004-143012");
        assert_eq!(named(third), "NVDA-1h-20261004-143012-3.png");
        std::fs::remove_dir_all(&folder).ok();
    }

    #[test]
    fn a_folder_that_is_really_a_file_says_so_rather_than_failing_silently() {
        let folder = std::env::temp_dir().join(format!("omacharts-file-{}", std::process::id()));
        std::fs::write(&folder, b"").expect("a file");
        let error = make_folder(&folder).expect_err("a file is not a folder");
        assert!(error.contains("is a file"), "{error}");
        std::fs::remove_file(&folder).ok();
    }

    #[test]
    fn the_folder_is_made_when_a_screenshot_needs_it_and_not_before() {
        let folder = std::env::temp_dir()
            .join(format!("omacharts-make-{}", std::process::id()))
            .join("Omacharts");
        std::fs::remove_dir_all(&folder).ok();
        assert!(!folder.exists());
        make_folder(&folder).expect("the folder is made on demand");
        assert!(folder.is_dir());
        std::fs::remove_dir_all(folder.parent().expect("a parent")).ok();
    }

    /// Every control left out is styled as a control in the stylesheet, which
    /// is how the capture knows to find it — so the two lists have to agree.
    ///
    /// The pointer layer is the one entry that is not a control, and it has no
    /// style of its own: it is a drawing area over the chart, and its class is
    /// there to be matched rather than to paint anything. It takes its name
    /// from the chart's own constant, so there is no second spelling of it to
    /// drift.
    #[test]
    fn every_control_left_out_of_a_screenshot_is_one_the_stylesheet_knows_about() {
        let themes = omacharts_engine::theme::builtin_themes();
        let schemes = omacharts_engine::theme::builtin_bar_schemes();
        let css = crate::theming::stylesheet(&themes[0], &schemes[0]);
        for class in LEFT_OUT.iter().filter(|class| **class != POINTER_LAYER) {
            assert!(css.contains(&format!(".{class}")), "{class} is not styled anywhere");
        }
    }
}
