//! What Rithmic asks a platform built on its R | Protocol API to show: the
//! Rithmic and OMNE artwork, and their copyright and trademark notices.
//!
//! Shown only where somebody connects to Rithmic: in its login dialog, on the
//! Data page while Rithmic is one of the feeds or there is a Rithmic login, and
//! in the menu a chart's feed dot opens when Rithmic is one of the feeds it
//! offers. `sign_in_dialogs` is how the rest of the app reaches it. Nothing
//! here is drawn on a chart, and with no Rithmic set up the app shows none of
//! it.
//!
//! The artwork is Rithmic's own, from its developer kit, as it was supplied:
//! `assets/rithmic/`. "Market Data by Rithmic" is the variant Rithmic offers a
//! platform that uses it for data, which is all omacharts does with trading
//! switched off; with trading on it is "Trading Platform by Rithmic". Each has
//! a light and a dark version, swapped as the theme changes.

use adw::prelude::*;
use gtk::{gdk, glib};

/// The notices, as Rithmic words them, in the order it lists them. The
/// R | API+ notice is left out: omacharts does not use R | API+.
pub const NOTICES: [&str; 4] = [
    "The R | Protocol API\u{2122} software is Copyright \u{a9} 2026 by Rithmic, LLC. All rights reserved.",
    "Trading Platform by Rithmic\u{2122} is a trademark of Rithmic, LLC. All rights reserved.",
    "The OMNE\u{2122} software is Copyright \u{a9} 2026 by Omnesys, LLC and Omnesys Technologies, Inc. All rights reserved.",
    "Powered by OMNE\u{2122} is a trademark of Omnesys, LLC and Omnesys Technologies, Inc. All rights reserved.",
];

/// One piece of artwork: what it is drawn from on a light theme and on a
/// dark one, and its proportions.
struct Artwork {
    light: &'static [u8],
    dark: &'static [u8],
    /// Width over height of each, from the files.
    aspect: (f64, f64),
}

const MARKET_DATA: Artwork = Artwork {
    light: include_bytes!("../../assets/rithmic/market-data-by-rithmic-black.png"),
    dark: include_bytes!("../../assets/rithmic/market-data-by-rithmic-white.png"),
    aspect: (2940.0 / 377.0, 2940.0 / 377.0),
};

const TRADING_PLATFORM: Artwork = Artwork {
    light: include_bytes!("../../assets/rithmic/trading-platform-by-rithmic-green.png"),
    dark: include_bytes!("../../assets/rithmic/trading-platform-by-rithmic-gray.png"),
    aspect: (863.0 / 115.0, 863.0 / 115.0),
};

const POWERED_BY_OMNE: Artwork = Artwork {
    light: include_bytes!("../../assets/rithmic/powered-by-omne.png"),
    dark: include_bytes!("../../assets/rithmic/powered-by-omne-white.png"),
    aspect: (961.0 / 129.0, 230.0 / 35.0),
};

/// Rithmic's logo for how omacharts uses it right now.
fn rithmic_artwork() -> &'static Artwork {
    match crate::features::trading() {
        true => &TRADING_PLATFORM,
        false => &MARKET_DATA,
    }
}

thread_local! {
    /// Each file decoded once, the first time it is shown.
    static TEXTURES: std::cell::RefCell<Vec<(usize, gdk::Texture)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The texture `png` decodes to, decoded the first time it is asked for.
fn texture(png: &'static [u8]) -> Option<gdk::Texture> {
    let key = png.as_ptr() as usize;
    TEXTURES.with(|textures| {
        if let Some((_, texture)) = textures.borrow().iter().find(|(at, _)| *at == key) {
            return Some(texture.clone());
        }
        match gdk::Texture::from_bytes(&glib::Bytes::from_static(png)) {
            Ok(texture) => {
                textures.borrow_mut().push((key, texture.clone()));
                Some(texture)
            }
            Err(error) => {
                eprintln!("omacharts: Rithmic artwork: {error}");
                None
            }
        }
    })
}

/// `artwork` drawn `height` pixels tall, in the version the theme wants, and
/// again in the other version whenever the theme changes.
///
/// In a clamp as wide as that, because a picture asks for the whole size of
/// what it shows, and the files are made to be shrunk: a menu gives a child
/// all it asks for.
fn picture(artwork: &'static Artwork, height: i32) -> adw::Clamp {
    let clamp = adw::Clamp::new();
    clamp.set_halign(gtk::Align::Center);
    clamp.set_valign(gtk::Align::Center);
    let picture = gtk::Picture::new();
    clamp.set_child(Some(&picture));
    picture.set_can_shrink(true);
    picture.set_content_fit(gtk::ContentFit::Contain);
    picture.set_halign(gtk::Align::Center);
    picture.set_valign(gtk::Align::Center);
    let styles = adw::StyleManager::default();
    let paint = move |clamp: &adw::Clamp, picture: &gtk::Picture, dark: bool| {
        let (png, aspect) = match dark {
            true => (artwork.dark, artwork.aspect.1),
            false => (artwork.light, artwork.aspect.0),
        };
        let width = (height as f64 * aspect).round() as i32;
        clamp.set_maximum_size(width);
        clamp.set_tightening_threshold(width);
        picture.set_size_request(width, height);
        match texture(png) {
            Some(texture) => picture.set_paintable(Some(&texture)),
            None => picture.set_paintable(None::<&gdk::Texture>),
        }
    };
    paint(&clamp, &picture, styles.is_dark());
    let (weak_clamp, weak) = (clamp.downgrade(), picture.downgrade());
    let handler = styles.connect_dark_notify(move |styles| {
        if let (Some(clamp), Some(picture)) = (weak_clamp.upgrade(), weak.upgrade()) {
            paint(&clamp, &picture, styles.is_dark());
        }
    });
    // The style manager outlives every dialog, so its handler goes with the
    // picture.
    let handler = std::cell::Cell::new(Some(handler));
    picture.connect_destroy(move |_| {
        if let Some(handler) = handler.take() {
            adw::StyleManager::default().disconnect(handler);
        }
    });
    clamp
}

/// Both logos, Rithmic's and OMNE's, `height` pixels tall, side by side or
/// one over the other.
pub fn logos(height: i32, orientation: gtk::Orientation) -> gtk::Box {
    let spacing = match orientation {
        gtk::Orientation::Horizontal => height * 2,
        _ => height / 2,
    };
    let logos = gtk::Box::new(orientation, spacing);
    logos.add_css_class("rithmic-attribution");
    logos.set_halign(gtk::Align::Center);
    logos.append(&picture(rithmic_artwork(), height));
    logos.append(&picture(&POWERED_BY_OMNE, height));
    logos
}

/// The logos and the notices, for the Data page while Rithmic is set up.
pub fn group() -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let artwork = logos(22, gtk::Orientation::Horizontal);
    artwork.set_halign(gtk::Align::Start);
    artwork.set_margin_bottom(10);
    content.append(&artwork);
    for notice in NOTICES {
        let label = gtk::Label::new(Some(notice));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.add_css_class("caption");
        label.add_css_class("dim-label");
        content.append(&label);
    }
    group.add(&content);
    group
}

/// The logos, small and one over the other, for the menu a chart's feed dot
/// opens.
pub fn menu_logos() -> gtk::Box {
    let logos = logos(14, gtk::Orientation::Vertical);
    logos.set_margin_top(6);
    logos.set_margin_bottom(6);
    logos.set_margin_start(8);
    logos.set_margin_end(8);
    logos
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The proportions written down are the files' own, so no logo is
    /// drawn squashed.
    #[test]
    fn the_artwork_is_drawn_in_its_own_proportions() {
        for artwork in [&MARKET_DATA, &TRADING_PLATFORM, &POWERED_BY_OMNE] {
            for (png, aspect) in [(artwork.light, artwork.aspect.0), (artwork.dark, artwork.aspect.1)] {
                assert_eq!(&png[1..4], b"PNG");
                let width = u32::from_be_bytes(png[16..20].try_into().unwrap()) as f64;
                let height = u32::from_be_bytes(png[20..24].try_into().unwrap()) as f64;
                assert!((width / height - aspect).abs() / aspect < 0.01, "{width}x{height}");
            }
        }
    }

    /// Rithmic's words, R | API+ left out.
    #[test]
    fn the_notices_are_rithmics() {
        assert!(NOTICES.iter().all(|notice| notice.ends_with("All rights reserved.")));
        assert!(!NOTICES.iter().any(|notice| notice.contains("API+")));
    }
}
