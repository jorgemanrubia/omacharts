//! A theme on everything round the chart, not only in it.
//!
//! A program embedding the chart wants its own window to wear the chart's
//! theme: the same surfaces, text, borders and accent. libadwaita draws
//! that window from named colours, and these redefine them from a
//! [`Theme`]; the program adds them to the display's CSS, above the
//! platform's own, and sets the colour scheme from [`Theme::mode`].

use omacharts_engine::theme::{BarLook, Theme, TradingLook};

use crate::colors;

/// libadwaita's named colours, from `theme`, as CSS.
pub fn libadwaita_colours(theme: &Theme) -> String {
    libadwaita_named(theme).iter().map(|(name, value)| format!("@define-color {name} {value};\n")).collect()
}

/// libadwaita's named colours, from `theme`, by name: windows, views,
/// header bars, sidebars, cards, dialogs, popovers, borders and the accent.
pub fn libadwaita_named(theme: &Theme) -> Vec<(&'static str, String)> {
    let ui = &theme.ui;
    let accent_fg = colors::readable_on(&ui.accent);

    let mut colours = Vec::with_capacity(32);
    let mut define = |name: &'static str, value: &str| colours.push((name, value.to_string()));

    // Windows and views. `view_bg_color` is the chart's own ground, so lists
    // and the chart agree.
    define("window_bg_color", &ui.surface);
    define("window_fg_color", &ui.text);
    define("view_bg_color", &ui.background);
    define("view_fg_color", &ui.text);
    define("headerbar_bg_color", &ui.surface_variant);
    define("headerbar_fg_color", &ui.text);
    define("headerbar_border_color", &ui.border);
    define("headerbar_backdrop_color", &ui.surface);
    define("sidebar_bg_color", &ui.surface);
    define("sidebar_fg_color", &ui.text);
    define("sidebar_border_color", &ui.border);
    define("sidebar_backdrop_color", &ui.surface);
    define("secondary_sidebar_bg_color", &ui.surface);
    define("secondary_sidebar_fg_color", &ui.text);
    define("card_bg_color", &ui.surface_variant);
    define("card_fg_color", &ui.text);
    define("dialog_bg_color", &ui.surface);
    define("dialog_fg_color", &ui.text);
    // Menus, popovers, the picker and the question cards on the rail's own
    // ground: a lighter slab read as something pasted on from another app.
    // The hairline and the shadow set them apart, and a breath of the text
    // colour lifts them off it just enough to read as on top.
    define("popover_bg_color", &format!("mix({}, {}, 0.03)", ui.surface, ui.text));
    define("popover_fg_color", &ui.text);
    define("borders", &ui.border);

    // Accent. libadwaita 1.6 split the standalone accent from the filled one.
    define("accent_bg_color", &ui.accent);
    define("accent_fg_color", accent_fg);
    define("accent_color", &ui.accent);
    colours
}

/// The trading look's colours as named colours, and the classes a signed
/// P&L, a side and a position are written in with them: the one pair for
/// money, the other for a side, as the chart's marks wear them, on bars
/// that look like `bars`.
pub fn trading_css(theme: &Theme, trading: &TradingLook, bars: &BarLook) -> String {
    let look = omacharts_engine::theme_trading(theme, trading, bars);
    format!(
        "@define-color omacharts_profit {};\n@define-color omacharts_loss {};\n\
         @define-color omacharts_buy {};\n@define-color omacharts_sell {};\n\
         @define-color omacharts_long {};\n@define-color omacharts_short {};\n\
         .pnl-gain {{ color: @omacharts_profit; }}\n.pnl-loss {{ color: @omacharts_loss; }}\n\
         .trade-buy {{ color: @omacharts_buy; }}\n.trade-sell {{ color: @omacharts_sell; }}\n\
         .trade-long {{ color: @omacharts_long; }}\n.trade-short {{ color: @omacharts_short; }}\n",
        look.profit, look.loss, look.buy, look.sell, look.long, look.short
    )
}
