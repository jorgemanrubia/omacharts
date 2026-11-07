//! The design round for colorless candles, as numbers: every theme on this
//! machine under every approach, as a TSV on stdout.
//!
//!     cargo run -p omacharts-engine --example muted_report
//!
//! Each row is one theme under one approach: the two greys and whether
//! each is drawn hollow, the contrast between them (WCAG, and as OKLab
//! lightness), each one's contrast against the chart, how much colour each
//! keeps, and the coloured pair they replace with its contrasts. Not part
//! of the app; it exists for the proposal.

use omacharts_engine::omarchy;
use omacharts_engine::theme::{
    builtin_themes, contrast_ratio, theme_bars, ColorlessApproach, Oklch, Theme,
};

fn main() {
    let home = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let mut themes: Vec<Theme> = builtin_themes();
    themes.extend(omarchy::installed(&home));
    println!(
        "theme\tmode\tapproach\tbg\ttext\tup0\tdown0\tcr0_up\tcr0_down\tup\tdown\tup_hollow\tdown_hollow\tcr_pair\tdl_pair\tcr_up\tcr_down\tchroma_up\tchroma_down"
    );
    for theme in &themes {
        let coloured = theme_bars(theme);
        let ground = &theme.ui.background;
        for approach in ColorlessApproach::ALL {
            let pair = approach.pair(theme);
            let l = |hex: &str| Oklch::of(hex).map_or(0.0, |c| c.l);
            let c = |hex: &str| Oklch::of(hex).map_or(0.0, |c| c.c);
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.2}\t{}\t{}\t{}\t{}\t{:.2}\t{:.3}\t{:.2}\t{:.2}\t{:.3}\t{:.3}",
                theme.id,
                theme.mode.label(),
                approach.key(),
                ground,
                theme.ui.text,
                coloured.up,
                coloured.down,
                contrast_ratio(&coloured.up, ground),
                contrast_ratio(&coloured.down, ground),
                pair.up,
                pair.down,
                pair.up_hollow,
                pair.down_hollow,
                contrast_ratio(&pair.up, &pair.down),
                (l(&pair.up) - l(&pair.down)).abs(),
                contrast_ratio(&pair.up, ground),
                contrast_ratio(&pair.down, ground),
                c(&pair.up),
                c(&pair.down),
            );
        }
    }
}
