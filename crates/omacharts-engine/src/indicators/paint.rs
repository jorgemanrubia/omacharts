//! What about an indicator only paints it, and what its numbers depend on.
//!
//! A colour, a line's weight, a profile's outline or transparency, a strip's
//! height: none of them moves a value the indicator computes. Changing one
//! should cost a repaint, and nothing else — not a pass over the bars, and
//! never a profile worked out again from what traded at each price, which on
//! a long chart is long enough to see.
//!
//! So every field is classed here, one way or the other, and the matches
//! name every field rather than ending in `..`: a field added to an
//! indicator does not compile until somebody has said which it is. Classed
//! as numbers, it costs a recomputation when it changes; classed as paint,
//! [`Output::repaint`] has to carry it into what was computed, if what was
//! computed carries it at all.

use super::{Band, Indicator, Output, Params, TimeRange, MAX_PANE_SHARE, MIN_PANE_SHARE};

/// Put each field back to its type's default: what it is set to stops
/// mattering when two indicators are compared for their numbers.
macro_rules! blank {
    ($($field:ident),* $(,)?) => {{
        $(*$field = Default::default();)*
    }};
}

impl Indicator {
    /// This indicator with everything that only paints it put back to a
    /// default, leaving what its numbers depend on: two indicators with the
    /// same of this compute the same output over the same bars.
    pub fn unpainted(&self) -> Indicator {
        let mut bare = self.clone();
        let Indicator { id: _, kind: _, params, color, alpha, stroke, visible, above_price, in_legend } = &mut bare;
        // Hidden or shown, an indicator is computed: showing it again is
        // only drawing what was already there.
        blank!(color, alpha, stroke, visible, above_price, in_legend);
        params.unpaint();
        bare
    }

    /// Whether going from `self` to `now` changes only how the indicator is
    /// drawn, so that what was computed for `self` serves `now` once
    /// [`Output::repaint`] has carried the new paint into it.
    pub fn paints_only(&self, now: &Indicator) -> bool {
        self.unpainted() == now.unpainted()
    }
}

impl Params {
    fn unpaint(&mut self) {
        match self {
            Params::MovingAverage { period: _ } => {}
            Params::Vwap { reset: _, bands } | Params::Bollinger { period: _, bands } => {
                bands.iter_mut().for_each(Band::unpaint);
            }
            Params::VolumeProfile {
                reset: _,
                rows: _,
                value_area: _,
                poc_color,
                style,
                outline,
                outline_color,
                outline_style,
                outline_width,
                poc_width,
                poc_style,
                poc,
                alpha,
                opacity,
                highlight,
                value_area_color,
                value_area_alpha,
                outside_alpha,
                outline_alpha,
                poc_alpha,
                // Which periods are worked out, not only which are drawn.
                last_only: _,
                align,
                width,
                method: _,
                colouring,
                merged: _,
            } => blank!(
                poc_color,
                style,
                outline,
                outline_color,
                outline_style,
                outline_width,
                poc_width,
                poc_style,
                poc,
                alpha,
                opacity,
                highlight,
                value_area_color,
                value_area_alpha,
                outside_alpha,
                outline_alpha,
                poc_alpha,
                align,
                width,
                colouring,
            ),
            // Its edge and width are where it is drawn, and whether it pairs
            // with a volume profile is read from the settings as it is
            // drawn, not from what was computed: paint.
            Params::MarketProfile {
                reset: _,
                minutes: _,
                row_ticks: _,
                value_area: _,
                style,
                align,
                width,
                poc,
                highlight,
                poc_color,
                poc_width,
                poc_style,
                value_area_color,
                open_close,
                open_color,
                close_color,
                open_alpha,
                close_alpha,
                outline,
                outline_color,
                outline_style,
                outline_width,
                outline_alpha,
                alpha,
                value_area_alpha,
                outside_alpha,
                poc_alpha,
                last_only: _,
                merged: _,
                expand_last,
                expanded,
            } => blank!(
                expand_last,
                expanded,
                style,
                align,
                width,
                poc,
                highlight,
                poc_color,
                poc_width,
                poc_style,
                value_area_color,
                open_close,
                open_color,
                close_color,
                open_alpha,
                close_alpha,
                outline,
                outline_color,
                outline_style,
                outline_width,
                outline_alpha,
                alpha,
                value_area_alpha,
                outside_alpha,
                poc_alpha,
            ),
            Params::Volume { height, colouring, up_color, down_color, opacity, up_alpha, down_alpha } => {
                blank!(height, colouring, up_color, down_color, opacity, up_alpha, down_alpha)
            }
            Params::Cvd { reset: _, style, height, colouring } => blank!(style, height, colouring),
            // The levels are in what an RSI or a stochastic computes: its
            // guides and the band between them.
            Params::Rsi { period: _, height, overbought: _, oversold: _ } => blank!(height),
            Params::Atr { period: _, height } => blank!(height),
            Params::Macd { fast: _, slow: _, signal: _, height, signal_color, signal_style, signal_alpha } => {
                blank!(height, signal_color, signal_style, signal_alpha)
            }
            Params::Stochastic {
                period: _,
                k_smooth: _,
                d_period: _,
                height,
                overbought: _,
                oversold: _,
                d_color,
                d_style,
                d_alpha,
            } => blank!(height, d_color, d_style, d_alpha),
            Params::SessionTimes { ranges } => ranges.iter_mut().for_each(TimeRange::unpaint),
        }
    }
}

impl Band {
    fn unpaint(&mut self) {
        // A band that is off is computed anyway, so turning it on is paint.
        let Band { deviations: _, enabled, color, fill, fill_alpha, stroke, line_alpha } = self;
        blank!(enabled, color, fill, fill_alpha, stroke, line_alpha);
    }
}

impl TimeRange {
    fn unpaint(&mut self) {
        let TimeRange { name, hours: _, color, alpha } = self;
        blank!(name, color, alpha);
    }
}

impl Output {
    /// Carry `now`'s paint into what was computed for `was`, where it was
    /// copied in when it was computed: a band's colour and fill, a strip's
    /// height. Only for a change [`Indicator::paints_only`] allows.
    ///
    /// `false` when it cannot, and the indicator has to be computed again:
    /// a change that is not paint alone, or a session times range's shading,
    /// which its spans carry without saying which range each came from.
    /// Working those out again is cheap; it reads no prices.
    pub fn repaint(&mut self, was: &Indicator, now: &Indicator) -> bool {
        if !was.paints_only(now) {
            return false;
        }
        match self {
            Output::Line(_) | Output::Profiles(_) | Output::MarketProfiles(_) | Output::Unmet(_) => true,
            Output::Bands(computed) => {
                let Some(bands) = now.params.bands() else { return false };
                if bands.len() != computed.bands.len() {
                    return false;
                }
                for (series, band) in computed.bands.iter_mut().zip(bands) {
                    series.enabled = band.enabled;
                    series.color = band.color.clone();
                    series.fill = band.fill;
                    series.fill_alpha = band.alpha();
                    series.stroke = band.stroke;
                    series.line_alpha = band.edge_alpha();
                }
                true
            }
            Output::Volume { height, .. } => match now.params.clone().height_mut() {
                Some(share) => {
                    *height = share.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE);
                    true
                }
                None => false,
            },
            Output::Pane(pane) => match now.params.clone().height_mut() {
                Some(share) => {
                    pane.height = share.clamp(MIN_PANE_SHARE, MAX_PANE_SHARE);
                    true
                }
                None => false,
            },
            Output::Time(_) => was.params == now.params,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::bars::Timeframe;
    use crate::theme::ColorChoice;

    fn bars() -> Vec<Bar> {
        (0..400)
            .map(|i| {
                let t = i as f64;
                let close = 100.0 + (t / 7.0).sin() * 4.0 + t * 0.01;
                Bar {
                    ts: 1_772_000_000 + i * 60,
                    open: close - 0.3,
                    high: close + 0.8,
                    low: close - 0.9,
                    close,
                    volume: 1_000.0 + (t * 13.0) % 700.0,
                }
            })
            .collect()
    }

    fn context() -> Context<'static> {
        Context {
            session_origin: 0,
            timeframe: Timeframe::minutes(1),
            kind: None,
            instrument: None,
            zone: chrono_tz::UTC,
            ladders: None,
            awaiting_ladders: false,
            memos: None,
            schedule: None,
        }
    }

    fn red() -> Option<ColorChoice> {
        Some(ColorChoice::Fixed { hex: "#c0392b".into() })
    }

    /// Every way the settings, a drag or a command can change only how an
    /// indicator looks, on each kind it applies to.
    fn paint_changes(indicator: &Indicator) -> Vec<(&'static str, Indicator)> {
        let mut changes = Vec::new();
        let mut change = |name: &'static str, edit: &dyn Fn(&mut Indicator)| {
            let mut changed = indicator.clone();
            edit(&mut changed);
            if changed != *indicator {
                changes.push((name, changed));
            }
        };
        change("colour", &|i| i.color = red());
        change("line opacity", &|i| i.alpha = Some(0.4));
        change("thickness", &|i| i.stroke.width = 4.0);
        change("line style", &|i| i.stroke.style = LineStyle::Dotted);
        change("hidden", &|i| i.visible = false);
        change("out of the legend", &|i| i.in_legend = false);
        change("taller", &|i| {
            if let Some(height) = i.params.height_mut() {
                *height = 0.3;
            }
        });
        change("bands", &|i| {
            if let Some(bands) = i.params.bands_mut() {
                for band in bands {
                    band.enabled = !band.enabled;
                    band.color = red();
                    band.fill = !band.fill;
                    band.fill_alpha = Some(0.4);
                    band.stroke.width = 2.5;
                    band.line_alpha = Some(0.5);
                }
            }
        });
        change("profile look", &|i| {
            if let Params::VolumeProfile {
                poc_color,
                style,
                outline,
                outline_color,
                outline_style,
                outline_width,
                poc_width,
                poc_style,
                poc,
                alpha,
                opacity,
                highlight,
                value_area_color,
                align,
                width,
                colouring,
                ..
            } = &mut i.params
            {
                *poc_color = red();
                *style = profile::Style::Outline;
                *outline = true;
                *outline_color = red();
                *outline_style = LineStyle::Dashed;
                *outline_width = 3.0;
                *poc_width = 2.0;
                *poc_style = LineStyle::Dotted;
                *poc = false;
                *alpha = 0.8;
                *opacity = 0.5;
                *highlight = false;
                *value_area_color = red();
                *align = profile::Align::Right;
                *width = 0.7;
                *colouring = Some(crate::theme::BarColouring::ALL[1]);
            }
        });
        change("profile parts' transparency", &|i| {
            if let Params::VolumeProfile { value_area_alpha, outside_alpha, outline_alpha, poc_alpha, .. }
            | Params::MarketProfile { value_area_alpha, outside_alpha, outline_alpha, poc_alpha, .. } = &mut i.params
            {
                *value_area_alpha = Some(0.6);
                *outside_alpha = Some(0.2);
                *outline_alpha = Some(0.5);
                *poc_alpha = Some(0.9);
            }
        });
        change("market profile look", &|i| {
            if let Params::MarketProfile {
                style,
                align,
                width,
                poc,
                highlight,
                poc_color,
                poc_width,
                poc_style,
                value_area_color,
                open_close,
                open_color,
                close_color,
                open_alpha,
                close_alpha,
                outline,
                outline_color,
                outline_style,
                outline_width,
                alpha,
                ..
            } = &mut i.params
            {
                *style = market::Style::Blocks;
                *align = profile::Align::Right;
                *width = 0.7;
                *poc = false;
                *highlight = false;
                *poc_color = red();
                *poc_width = 3.0;
                *poc_style = LineStyle::Dashed;
                *value_area_color = red();
                *open_close = true;
                *open_color = red();
                *close_color = red();
                *open_alpha = Some(0.5);
                *close_alpha = Some(0.5);
                *outline = true;
                *outline_color = red();
                *outline_style = LineStyle::Dotted;
                *outline_width = 2.0;
                *alpha = Some(0.5);
            }
        });
        change("volume look", &|i| {
            if let Params::Volume { colouring, up_color, down_color, opacity, .. } = &mut i.params {
                *colouring = Some(crate::theme::BarColouring::ALL[1]);
                *up_color = red();
                *down_color = red();
                *opacity = 0.5;
            }
        });
        change("volume bars' opacity apart", &|i| {
            if let Params::Volume { up_alpha, down_alpha, .. } = &mut i.params {
                *up_alpha = Some(0.3);
                *down_alpha = Some(0.6);
            }
        });
        change("second line", &|i| match &mut i.params {
            Params::Macd { signal_color, signal_style, signal_alpha, .. } => {
                *signal_color = red();
                *signal_style = Some(LineStyle::Dashed);
                *signal_alpha = Some(0.5);
            }
            Params::Stochastic { d_color, d_style, d_alpha, .. } => {
                *d_color = red();
                *d_style = Some(LineStyle::Dashed);
                *d_alpha = Some(0.5);
            }
            _ => {}
        });
        change("cvd as a line", &|i| {
            if let Params::Cvd { style, colouring, .. } = &mut i.params {
                *style = cvd::Style::Line;
                *colouring = Some(crate::theme::BarColouring::ALL[1]);
            }
        });
        change("shading", &|i| {
            if let Params::SessionTimes { ranges } = &mut i.params {
                for range in ranges {
                    range.color = red();
                    range.alpha = 0.3;
                    range.name = "Renamed".into();
                }
            }
        });
        changes
    }

    /// A paint change repaints what was computed into exactly what computing
    /// it again would give, on every kind: nothing drawn after a colour
    /// change differs from a chart opened with that colour.
    #[test]
    fn a_change_of_paint_repaints_into_what_computing_it_again_gives() {
        let bars = bars();
        let context = context();
        for kind in Kind::ALL {
            let was = Indicator::new(1, kind);
            let computed = compute_in(&was, &bars, &context);
            let changes = paint_changes(&was);
            assert!(!changes.is_empty(), "{kind:?} has some paint to change");
            for (name, now) in changes {
                assert!(was.paints_only(&now), "{kind:?}: {name} is paint");
                let mut repainted = computed.clone();
                let fresh = compute_in(&now, &bars, &context);
                if repainted.repaint(&was, &now) {
                    assert_eq!(repainted, fresh, "{kind:?}: {name} repainted");
                } else {
                    // Only session times are worked out again, and only
                    // because their spans do not say which range is whose.
                    assert_eq!(kind, Kind::SessionTimes, "{kind:?}: {name} computed again");
                }
            }
        }
    }

    /// What a computation depends on is never taken for paint.
    #[test]
    fn a_change_to_what_is_computed_is_not_paint() {
        type Change = (&'static str, Indicator, fn(&mut Params));
        let numbers: Vec<Change> = vec![
            ("period", Indicator::new(1, Kind::Sma), |p| {
                if let Params::MovingAverage { period } = p {
                    *period += 1;
                }
            }),
            ("reset", Indicator::new(1, Kind::Vwap), |p| {
                if let Params::Vwap { reset, .. } = p {
                    *reset = Reset::Week;
                }
            }),
            ("deviations", Indicator::new(1, Kind::Bollinger), |p| {
                if let Some(bands) = p.bands_mut() {
                    bands[0].deviations = 3.0;
                }
            }),
            ("rows", Indicator::new(1, Kind::VolumeProfile), |p| {
                if let Params::VolumeProfile { rows, .. } = p {
                    *rows = Some(40);
                }
            }),
            ("value area", Indicator::new(1, Kind::VolumeProfile), |p| {
                if let Params::VolumeProfile { value_area, .. } = p {
                    *value_area = 0.5;
                }
            }),
            ("method", Indicator::new(1, Kind::VolumeProfile), |p| {
                if let Params::VolumeProfile { method, .. } = p {
                    *method = profile::Method::Delta;
                }
            }),
            ("profile reset", Indicator::new(1, Kind::VolumeProfile), |p| {
                if let Params::VolumeProfile { reset, .. } = p {
                    *reset = Reset::Week;
                }
            }),
            ("tpo period", Indicator::new(1, Kind::MarketProfile), |p| {
                if let Params::MarketProfile { minutes, .. } = p {
                    *minutes = 15;
                }
            }),
            ("row ticks", Indicator::new(1, Kind::MarketProfile), |p| {
                if let Params::MarketProfile { row_ticks, .. } = p {
                    *row_ticks = Some(7);
                }
            }),
            ("market profile value area", Indicator::new(1, Kind::MarketProfile), |p| {
                if let Params::MarketProfile { value_area, .. } = p {
                    *value_area = 0.5;
                }
            }),
            ("market profile reset", Indicator::new(1, Kind::MarketProfile), |p| {
                if let Params::MarketProfile { reset, .. } = p {
                    *reset = Reset::Week;
                }
            }),
            ("market profile last only", Indicator::new(1, Kind::MarketProfile), |p| {
                if let Params::MarketProfile { last_only, .. } = p {
                    *last_only = true;
                }
            }),
            ("last only", Indicator::new(1, Kind::VolumeProfile), |p| {
                if let Params::VolumeProfile { last_only, .. } = p {
                    *last_only = true;
                }
            }),
            ("overbought", Indicator::new(1, Kind::Rsi), |p| {
                if let Params::Rsi { overbought, .. } = p {
                    *overbought = 80.0;
                }
            }),
            ("signal", Indicator::new(1, Kind::Macd), |p| {
                if let Params::Macd { signal, .. } = p {
                    *signal += 1;
                }
            }),
            ("hours", Indicator::new(1, Kind::SessionTimes), |p| {
                if let Params::SessionTimes { ranges } = p {
                    ranges.clear();
                }
            }),
        ];
        for (name, was, edit) in numbers {
            let mut now = was.clone();
            edit(&mut now.params);
            assert_ne!(now, was, "{name} changed something");
            assert!(!was.paints_only(&now), "{name} is not paint");
            let mut output = compute_in(&was, &bars(), &context());
            assert!(!output.repaint(&was, &now), "{name} is not repainted");
        }
        let was = Indicator::new(1, Kind::Ema);
        assert!(!Indicator::new(1, Kind::Sma).paints_only(&was), "another kind");
        assert!(!Indicator::new(2, Kind::Ema).paints_only(&was), "another indicator");
    }
}
