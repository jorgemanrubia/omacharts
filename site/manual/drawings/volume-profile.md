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

<kbd>Alt</kbd> + <kbd>Enter</kbd> or a double-click on the profile opens them. They are the [Volume Profile](/manual/indicators/volume/volume-profile/) study's, with the same names, ranges and controls, in the same two groups and order, less what only the study has:

| Left out | Because |
|---|---|
| **Reset every**, **Only show in last period** | The span is set by its ends |
| **Calculation method**, **Colouring** | A drawing is worked out from its bars' volume alone |
| **Width %** | A drawing is as wide as its span |

Defaults are those of configuration 1 as shipped, with the other configurations' differences under [Configurations](#configurations).

### Parameters

| Property | What it does | Default |
|---|---|---|
| **Automatic** | Picks how many rows the profile has: each row a round multiple of the instrument's price step, about 600 rows over the span, or one price step a row on a short one. While it is on, **Rows** is greyed and shows how many rows are drawn | On |
| **Rows** | With **Automatic** off, how many rows the span's price range is cut into, 4 to 800. Fewer rows give a coarser profile, more a finer one. Turning **Automatic** off starts from the count being drawn, so the profile only changes when you change the number | Automatic |
| **Value area %** | The share of the span's volume the value area holds, 10 to 100% | 70% |
| **Highlight value area** | Draws the value area's rows stronger than the others, which are a little under half as strong. Off draws every row alike | On |
| **Value area colour** | Gives the value area a colour of its own while it is highlighted. **Reset** goes back to the background's colour. Greyed while **Highlight value area** is off | The background's |

### Style

| Property | What it does | Default |
|---|---|---|
| **Outline** | Draws an edge round the filled rows. Always on, and greyed, while **Style** is Outline, since the edge is then all there is. The three outline rows below show while it is on | Off |
| **Outline colour** | The colour of the edge round the rows. Every configuration ships it the same colour as the background | The configuration's |
| **Outline thickness** | How thick the edge is, 0.5 to 12 px in halves | 1 px on a filled profile, 1.5 px on an outline |
| **Outline style** | **Solid**, **Dashed** or **Dotted** | Solid |
| **Background** | The rows' colour: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour | The configuration's |
| **Transparency %** | How much of the chart shows through the rows, 0 to 100%. At 0% the value area is solid; at 100% the profile is gone. The outline fades with the rows | 70% |
| **Show point of control** | Draws a line across the price with the most volume. Off leaves the profile without it. The three rows below show while it is on | On |
| **Point of control colour** | The colour of the line at the busiest price. Every configuration ships it in Ink, so it stands apart from the rows | Ink |
| **Point of control thickness** | How thick the line is, 0.5 to 12 px in halves | 1.5 px |
| **Point of control style** | **Solid**, **Dashed** or **Dotted** | Solid |
| **Style** | **Filled** draws the rows as solid bars. **Outline** draws only their edge, which leaves the candles under it clear | Filled |
| **Alignment** | **Left** grows the rows to the right from the start of the span. **Right** grows them to the left from its end | Left |

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
