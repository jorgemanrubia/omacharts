---
title: "Volume profile"
description: "How much volume traded at each price over a span you choose"
section: charting
parent: drawings
nav_order: 9
hotkey: "Alt + V"
---

A profile of the bars in the span you drag across: how much volume traded at each price, the value area drawn stronger, and the point of control as a line across it.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>V</kbd> | Press where the span starts, release where it ends | `profile`, `vp` |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1h
  omacharts chart indicator add volume_profile
  omacharts chart drawing add profile --from 2026-10-14,700 --to 2026-10-21,700 --config 1
  omacharts chart drawing add profile --from 2026-10-26,700 --to 2026-10-30,700 --config 9
  move 1270 1000
capture: bin/headless-window cli chart screenshot --output /tmp/vp.png; magick /tmp/vp.png -quality 88 site/assets/manual/drawings/volume-profiles.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/volume-profiles.jpg" alt="SPY 1h with session volume profiles, a Violet profile filled over a week of them from the left and an Ink outline over the last days from the right" caption="Two volume profile drawings over the session study: configuration 1 over one week, configuration 9 over the next." %}

It covers any stretch, across sessions or inside one, over a [Volume Profile](/manual/indicators/volume/volume-profile/) study or on its own. Its height is the range of prices those bars reached. It is drawn under the bars, as the study's profiles are, so the candles stay on top.

Selected, it has a grip at each corner: the left two move its start and the right two its end, sideways only, and the profile is worked out again. It cannot be moved whole or nudged with the arrow keys; a press on it that drags pans the chart. It takes no text. On a chart of another resolution it is worked out from that chart's bars. A pasted copy lands a few bars to the right of the original.

## Properties

Its properties are the [Volume Profile](/manual/indicators/volume/volume-profile/) study's, with the same names, ranges and controls, in the same groups and order. It leaves out what only the study has:

| Left out | Because |
|---|---|
| **Reset every**, **Only show in last period** | The span is set by its ends |
| **Calculation method**, **Colouring** | A drawing is worked out from its bars' volume alone |
| **Width %** | A drawing is as wide as its span |

| Group | Property | Does |
|---|---|---|
| **Parameters** | **Automatic**, **Rows** | Automatic by default: rows a round multiple of the instrument's price step, about 600 to the span, and a price step each on a short one. With **Automatic** off, **Rows** is the number of rows, 4 to 800. While automatic, **Rows** shows how many are drawn |
| | **Value area %** | The share of the span's volume the value area holds, 10 to 100%, 70% by default |
| | **Highlight value area** | On by default: the value area is drawn stronger than the other rows, which are a little under half as strong. Off draws every row alike |
| | **Value area colour** | The value area's own colour while it is highlighted. By default the background's; **Reset** gives it back |
| **Style** | **Outline** | Whether filled rows have an edge round them. Off by default; always on, and greyed, for an outline. The outline's colour, thickness and style show while it is on |
| | **Outline colour** | The edge round the rows |
| | **Outline thickness** | 0.5 to 12 px. 1 px on a filled configuration, 1.5 px on an outline |
| | **Outline style** | Solid, Dashed or Dotted. Solid by default |
| | **Background** | The rows |
| | **Transparency %** | How much of the candles shows through the background, 70% by default. At 0% the value area is the background colour, solid; at 100% the profile is gone. The outline fades with it |
| | **Show point of control** | On by default. Off leaves the profile without its point of control. Its colour, thickness and style show while it is on |
| | **Point of control colour** | The line across the busiest price |
| | **Point of control thickness** | 0.5 to 12 px, 1.5 px by default |
| | **Point of control style** | Solid, Dashed or Dotted. Solid by default |
| | **Style** | **Filled**: the rows under the outline. **Outline**: the outline alone |
| | **Alignment** | **Left**: the rows grow from the start of the span. **Right**: from its end |

## Configurations

| Configuration | Colour | Style | Alignment |
|---|---|---|---|
| 1, 2, 3, 4 | Violet, Amber, Cyan, Blue | Filled | Left |
| 5, 6, 7, 8 | Violet, Amber, Cyan, Blue | Outline | Left |
| 9 | Ink | Outline | Right |

Every configuration's point of control is Ink, and its outline and background are the same colour. They leave out the candles' Up and Down, and Teal and Orange, which look close to green and red on some themes, because a profile shows where volume traded, not which way the price went ([Configurations](/manual/drawings/#configurations)).

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
