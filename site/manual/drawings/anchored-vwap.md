---
title: "Anchored VWAP"
description: "The volume-weighted average price from the bar you click"
section: charting
parent: drawings
nav_order: 10
hotkey: "Alt + W"
---

The volume-weighted average price from the bar you click to the last bar on the chart: each bar's price, weighted by its volume, averaged over every bar since the anchor.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>W</kbd> | Click the bar it starts from | `avwap`, `vwap` |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1h
  omacharts chart drawing add avwap --from 2026-10-20,0 --config 4 --bands 1,2
  omacharts chart drawing add avwap --from 2026-10-27T09:30,0 --config 3
  move 1270 1000
capture: bin/headless-window cli chart screenshot --output /tmp/avwap.png; magick /tmp/avwap.png -quality 88 site/assets/manual/drawings/anchored-vwap.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/anchored-vwap.jpg" alt="SPY 1h with an amber anchored VWAP from 20 October, its first band shaded and its second dashed, and a blue one from the 27 October open" caption="Two anchored VWAPs: configuration 4 from 20 October with its first two bands, the 1σ shaded and the 1.5σ dashed, and configuration 3 from the open of 27 October." %}

It is what a buyer since that bar has paid on average, so it is anchored where something happened (an earnings gap, a swing low, the first bar of a breakout) and read as support or resistance from there. Unlike the [VWAP](/manual/indicators/averages-and-bands/vwap/) study it never resets. On daily bars it follows weeks of trading from one bar.

It has no end. As new bars arrive the line runs on to them, worked out again from the anchor. A click past the last bar anchors it on the last bar, and one before the first on the first. On a bar with no volume, an index's, each bar counts once.

Selected, it glows in its own colour, as a [selected VWAP study](/manual/indicators/#selecting-an-indicator) does, and has one grip, at the anchor. Drag the grip to another bar and the line is worked out from there. It cannot be moved whole or nudged with the arrow keys; a press on the line that drags pans the chart. It takes no text. On a chart of another resolution it is worked out from that chart's bars, from the same moment. A pasted copy lands a few bars to the right of the original.

## Properties

Its properties are the [VWAP](/manual/indicators/averages-and-bands/vwap/) study's, in the same groups and order, with the same defaults: a line, and three bands at 1, 1.5 and 2 standard deviations, all off. When on, the first is shaded with no outline, the second dashed, the third a plain line. The deviation is weighted by volume, as the average is.

| Group | Row | Does | Default |
|---|---|---|---|
| **Parameters** | **Source** | The price each bar adds: **Typical** (high + low + close) / 3, the study's; **Median** (high + low) / 2; **Average** (open + high + low + close) / 4; or **Close** | Typical |
| **Line** | **Colour** | One of the [nine presets](/manual/drawings/#colours), or **Custom…** | The configuration's |
| | **Thickness** | No line, 1, 1.5, 2.5 or 4 px. No line leaves only the bands | 1.5 px |
| | **Line style** | Solid, Dashed, Dotted | Solid |
| **Band 1**, **Band 2**, **Band 3** | **Show** | Draws the band | Off |
| | **Standard deviations** | How far out it sits, 0.1 to 6, in tenths | 1, 1.5, 2 |
| | **Shaded** | Fills the area between its edges, under the candles | On for band 1 |
| | **Shading %** | How solid that fill is, 0 to 100% | 16% |
| | **Colour** | The band's colour. **Reset** goes back to the line's, which it follows | The line's |
| | **Thickness**, **Line style** | Its edges. No line leaves a shaded band with no outline | No line for band 1, 1 px dashed for band 2, 1 px solid for band 3 |

## Configurations

Every configuration is its colour at 1.5 px, solid, from the typical price, with no bands. Every property is part of the configuration: **Save as…** keeps the bands with the rest, and a drawing on that configuration draws them ([Configurations](/manual/drawings/#configurations)).

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
