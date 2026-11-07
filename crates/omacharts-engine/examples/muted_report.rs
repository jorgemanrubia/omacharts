//! The design round for muted candles, as numbers: every theme on this
//! machine under every approach, as a TSV on stdout.
//!
//!     cargo run -p omacharts-engine --example muted_report
//!
//! Each row is one theme under one approach: the muted pair, how far apart
//! the two are (OKLab, and under a deutan and a protan eye), each one's
//! contrast against the chart, and how far each sits from the coloured
//! pair it replaces. Not part of the app; it exists for the proposal.

use omacharts_engine::omarchy;
use omacharts_engine::theme::{
    builtin_themes, contrast_ratio, delta_e, oklab, rgb, theme_bars, MutedApproach, Theme,
    MUTED_APART,
};

fn main() {
    let home = std::env::var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let mut themes: Vec<Theme> = builtin_themes();
    themes.extend(omarchy::installed(&home));
    println!(
        "theme\tmode\tapproach\tbg\ttext\tup0\tdown0\tup\tdown\tde\tde_deutan\tde_protan\tcr_up\tcr_down\tmute_up\tmute_down\tde0\tde0_deutan\tmono"
    );
    for theme in &themes {
        let coloured = theme_bars(theme);
        for approach in MutedApproach::ALL {
            let (up, down) = approach.pair(theme);
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{:.2}\t{:.2}\t{:.3}\t{:.3}\t{:.3}\t{:.3}\t{}",
                theme.id,
                theme.mode.label(),
                approach.key(),
                theme.ui.background,
                theme.ui.text,
                coloured.up,
                coloured.down,
                up,
                down,
                delta_e(&up, &down),
                cvd_delta(&up, &down, DEUTAN),
                cvd_delta(&up, &down, PROTAN),
                contrast_ratio(&up, &theme.ui.background),
                contrast_ratio(&down, &theme.ui.background),
                delta_e(&up, &coloured.up),
                delta_e(&down, &coloured.down),
                delta_e(&coloured.up, &coloured.down),
                cvd_delta(&coloured.up, &coloured.down, DEUTAN),
                coloured.mono,
            );
        }
    }
    let _ = MUTED_APART;
}

// Machado, Oliveira & Fernandes (2009) at severity 1.0, on linear RGB.
const DEUTAN: [[f64; 3]; 3] = [
    [0.367322, 0.860646, -0.227968],
    [0.280085, 0.672501, 0.047413],
    [-0.011820, 0.042940, 0.968881],
];
const PROTAN: [[f64; 3]; 3] = [
    [0.152286, 1.052583, -0.204868],
    [0.114503, 0.786281, 0.099216],
    [-0.003882, -0.048116, 1.051998],
];

fn simulate(hex: &str, m: [[f64; 3]; 3]) -> String {
    let (r, g, b) = rgb(hex).unwrap();
    let lin = |v: u8| {
        let c = v as f64 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let out = |row: [f64; 3]| {
        let c = (row[0] * r + row[1] * g + row[2] * b).clamp(0.0, 1.0);
        let s = if c <= 0.0031308 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", out(m[0]), out(m[1]), out(m[2]))
}

fn cvd_delta(a: &str, b: &str, m: [[f64; 3]; 3]) -> f64 {
    let (a, b) = (simulate(a, m), simulate(b, m));
    let (Some(a), Some(b)) = (oklab(&a), oklab(&b)) else {
        return 0.0;
    };
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}
