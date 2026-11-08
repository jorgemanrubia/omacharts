---
title: "Drawings"
description: "Marks you put on a chart by hand, their looks, and which charts show them."
section: charting
nav_order: 17
---

A drawing is a mark you put on a chart by hand. It stays on the same bars as the chart pans and zooms, and shows on every chart of the symbol, at any resolution.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit (bin/headless-window, sandboxed HOME)
state:
  key ctrl+shift+b
  omacharts chart set --symbol NVDA --resolution 1D --template none
  omacharts chart drawing add line --from 2026-06-26,190 --to 2026-09-22,209 --config 3 --text "rising lows" --text-at bottom
  omacharts chart drawing add hline --from 2026-05-14,236 --to 2026-10-20,236 --config 4 --text "236 resistance" --text-at top-left
  omacharts chart drawing add zigzag --point 2026-04-06,165 --point 2026-05-14,236 --point 2026-06-26,190 --point 2026-08-14,228 --point 2026-08-28,207 --config 5
  omacharts chart drawing add arrow --from 2026-07-17,182 --to 2026-07-30,195 --config 9
  omacharts chart drawing add rect --from 2026-09-01,207 --to 2026-09-24,232 --config 6 --text range --text-at top
  omacharts chart drawing add ellipse --from 2026-09-29,233 --to 2026-10-12,247 --config 2
  omacharts chart drawing add text --from 2026-07-08,180 --text "**the** turn" --config 9
  move 1275 795
capture: bin/headless-window cli chart screenshot --output /tmp/all.png; magick /tmp/all.png -resize 1600x -quality 88 site/assets/manual/drawings/every-kind.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/every-kind.jpg" alt="NVDA daily with a trend line, a level, a zig-zag, an arrow, a box, a circle and a text drawing, some of them labelled" caption="A line, a level, a zig-zag, an arrow, a box, a circle and a text drawing, some with labels." %}

## The tools

<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>D</kbd>, the toggle in the window's bottom-left corner, or **Sidebars** → **Drawing tools** in a chart's right-click menu shows and hides the tools down the left edge. The toggle stays where it is as they come and go: left of the bottom-left chart's first date while the tools are hidden, and at their foot while they are out. A tool's key picks it whether the tools are showing or not.

| Tool | Key | Placing it |
|---|---|---|
| Pointer | <kbd>Esc</kbd> | No tool. Selects, moves and resizes drawings |
| Measure | <kbd>Alt</kbd> + <kbd>M</kbd> | Click the start, move the mouse, then click the end |
| Line | <kbd>Alt</kbd> + <kbd>L</kbd> | Press where it starts, then where it ends |
| Horizontal line | <kbd>Alt</kbd> + <kbd>H</kbd> | A level. It stays flat however you drag it and spans the time between its two ends |
| Arrow | <kbd>Alt</kbd> + <kbd>A</kbd> | A line with a head at the end |
| Zig-zag | <kbd>Alt</kbd> + <kbd>Z</kbd> | Press at each corner. A second press on the last corner, a double-click or <kbd>Enter</kbd> ends it there; <kbd>Esc</kbd> drops it |
| Rectangle | <kbd>Alt</kbd> + <kbd>R</kbd> | Press at one corner, release at the other |
| Circle | <kbd>Alt</kbd> + <kbd>C</kbd> | Placed like a rectangle; the ellipse fills it |
| Text | <kbd>Alt</kbd> + <kbd>T</kbd> | Click where the text goes, then type |
| Volume profile | <kbd>Alt</kbd> + <kbd>V</kbd> | Press where the span starts, release where it ends ([Volume profiles](#volume-profiles)) |
| Anchored VWAP | <kbd>Alt</kbd> + <kbd>W</kbd> | Click the bar it starts from ([Anchored VWAP](#anchored-vwap)) |
| Fib retracement | <kbd>Alt</kbd> + <kbd>F</kbd> | Press where the swing starts, release where it ends ([Fib retracement](#fib-retracement)) |

After one drawing the tool is deselected. <kbd>Ctrl</kbd> + click a tool to keep it selected until <kbd>Esc</kbd> or another tool. The selected tool draws on whichever chart you press. A configuration is a saved look for a tool's drawings, numbered 1 to 9 ([Configurations](#configurations)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window (HOME sandboxed, with the Omarchy theme's colors.toml and icons.theme copied into its .local/share/omarchy/themes/tokyo-night and gtk-icon-theme-name=Yaru-magenta in its .config/gtk-4.0/settings.ini)
state:
  omacharts config set theme omarchy:tokyo-night   (before the window opens)
  omacharts chart set --symbol NVDA --resolution 1D --template none
  key ctrl+shift+d
  key alt+r
  key alt+6
  move 1275 795
capture: bin/headless-window shot /tmp/bar.png; magick /tmp/bar.png -crop 760x1100+0+0 +repage site/assets/manual/drawings/tools-bar.png
crop: 0,0,760,1100
-->
{% include figure.html src="/assets/manual/drawings/tools-bar.png" alt="The drawing tools down the left edge, with the rectangle lit and a small 6 on it" caption="The tools, with the rectangle selected and configuration 6 chosen for it." %}

| While placing | Does |
|---|---|
| <kbd>Ctrl</kbd> held | Snaps the point to the nearest open, high, low or close of the candle under it, and the crosshair with it |
| Right-click | Drops the half-made drawing and keeps the tool |
| <kbd>Alt</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | The configuration the drawing will follow, shown as a number on the tool. With no tool selected, it applies to whatever is drawn next |
| <kbd>Esc</kbd> | Drops the drawing and deselects the tool |

A finished drawing is left selected.

## Measuring a move

The ruler measures a move between two points. Press <kbd>Alt</kbd> +
<kbd>M</kbd>, or choose **Measure** just below the pointer at the top of the
tools. Click the starting point, move the mouse, then click the end point. A
rise is drawn in your bar colours' up colour and a fall in their down colour,
with an arrow pointing each way the move went. The label shows the percentage
change in bold, then the price change, and under them the bar distance and
elapsed time: `+0.53%  +4.01` over `71 bars, 5h 55m`. The measurement stays
fixed after the second click. Click again to start another, or press
<kbd>Esc</kbd> to clear it and return to the pointer.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window (HOME sandboxed and the theme seeded, as for the tools above)
state:
  omacharts config set theme omarchy:tokyo-night   (before the window opens)
  omacharts chart set --symbol NVDA --resolution 1D --template none
  key ctrl+shift+d
  key alt+m
  click 515 520
  move 668 160
  click 668 160
  move 1275 795
capture: bin/headless-window shot /tmp/ruler.png; magick /tmp/ruler.png -crop 1950x1600+0+0 +repage -resize 1600x site/assets/manual/drawings/ruler.png
crop: 0,0,1950,1600
-->
{% include figure.html src="/assets/manual/drawings/ruler.png" alt="A rise on NVDA measured in green, labelled +22.92% +43.62 over 28 bars, 41d" caption="A move measured from the August low, left fixed by the second click." %}

Bar distance counts the spaces between candles. Elapsed time includes weekends
and market closures. The price change has as many decimals as the price axis
gives that price. With fewer than two loaded candles the label shows the time
without a bar distance, and a move that starts at zero shows no percentage.
Points beyond the loaded candles use the chart's estimated times. The
measurement is not saved or shared with other charts. Choosing another tool or
changing the symbol or resolution clears it.

## Selecting and moving

| Key or gesture | Does |
|---|---|
| Click a drawing | Selects it alone |
| <kbd>Shift</kbd> + click, <kbd>Ctrl</kbd> + click | Adds it to the selection, or takes it out |
| <kbd>Shift</kbd> + drag on empty chart | Adds every drawing the box touches to the selection |
| <kbd>Ctrl</kbd> + <kbd>A</kbd> | Selects every drawing on the chart |
| Click empty chart, <kbd>Esc</kbd> | Clears the selection |
| Drag an end or a corner | Moves that point. With <kbd>Ctrl</kbd> held it snaps to the candle, as when placing. While you drag, the axes show the point's price and time in the drawing's colour |
| Drag the body | Moves the whole drawing, and every other selected drawing with it |
| <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> | Nudges the selection a pixel; ten with <kbd>Shift</kbd> |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>↑</kbd> / <kbd>↓</kbd> | Brings it to the front or sends it to the back among the drawings. Every drawing sits over the candles |
| <kbd>Ctrl</kbd> + <kbd>X</kbd>, <kbd>Ctrl</kbd> + <kbd>C</kbd>, <kbd>Ctrl</kbd> + <kbd>V</kbd> | Cuts or copies the selection, and pastes it on the focused chart at the same times and prices. A paste that would land exactly on its source is moved off it a little |
| <kbd>Delete</kbd> / <kbd>Backspace</kbd> | Removes the selection |
| <kbd>Ctrl</kbd> + <kbd>Z</kbd>; <kbd>Ctrl</kbd> + <kbd>Y</kbd> or <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> | Undo and redo, on this chart's drawings |
| <kbd>Alt</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Puts the selection on that configuration |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>+</kbd> / <kbd>−</kbd> | Thickens or thins a line, an anchored VWAP, a retracement's levels, a box's or circle's edge, or a volume profile's outline, by half a pixel, from 0.5 to 12 px |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>A</kbd> | Steps a line's arrowhead: none, filled, open, barb |

Whatever you do to a selection is done to every drawing in it. Configurations and the properties need the selected drawings to be of one kind.

## The drawing's menu

Right-click a drawing for its menu. It acts on the selected drawings. A right-click on empty chart opens the chart's own menu, which has **Paste** and **Remove all drawings** ([Charts](/manual/charts/)).

Cut, copy and paste take an indicator as well as drawings. **Paste** puts down whichever was copied last ([Selecting an indicator](/manual/indicators/#selecting-an-indicator)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window start --size 2560x1600 (HOME sandboxed)
state:
  (the state of the first figure)
  key ctrl+shift+d
  key Escape
  click 900 300          # the range box, nearer its left edge so the menu has room
  run bin/virtual-pointer --extent 1280x800 --button right click 900 300
capture: bin/headless-window shot /tmp/dmenu.png
crop: 1666,320,642,888   (the box above, the menu with 20px layout margin right and below)
-->
{% include figure.html src="/assets/manual/drawings/drawing-menu.png" alt="A selected teal box with grips at its corners and edges, and its right-click menu" caption="A selected box and its menu." %}

| Item | Does |
|---|---|
| **Edit text** / **Add text** (<kbd>Enter</kbd>) | Puts the caret in the drawing, to type on the chart |
| **Cut** (<kbd>Ctrl</kbd> + <kbd>X</kbd>) | Copies the selection and removes it |
| **Copy** (<kbd>Ctrl</kbd> + <kbd>C</kbd>) | Copies the selection |
| **Paste** (<kbd>Ctrl</kbd> + <kbd>V</kbd>) | Only while something is copied: drawings, or an indicator |
| **Remove** (<kbd>Delete</kbd>) | Removes the selection |
| **Configuration** | Configurations 1 to 4, each drawn as this kind of drawing. 5 to 9 are <kbd>Alt</kbd> + <kbd>5</kbd> … <kbd>9</kbd>, or the picker in the properties |
| **Thickness** | 1, 1.5, 2.5 or 4 px. Not on text |
| **Arrowhead** | None, filled, open or barb. On lines, levels, arrows and zig-zags |
| **Bring to front**, **Send to back** | As <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>↑</kbd> / <kbd>↓</kbd> |
| **Properties…** (<kbd>Alt</kbd> + <kbd>Enter</kbd>) | Opens the properties |

## Properties

<kbd>Alt</kbd> + <kbd>Enter</kbd>, **Properties…**, or a double-click on any drawing but text, opens the properties of the selected drawings. With kinds mixed nothing opens. Each change applies as you make it, with the chart visible behind the dialog. **Done**, <kbd>Ctrl</kbd> + <kbd>Enter</kbd> or <kbd>Esc</kbd> closes it.

Across the top is a picture of the drawing, tagged with its configuration, or **Custom** when it has a look of its own. Click the picture to see the nine configurations drawn as this drawing, with its own look beside them while it has one. <kbd>Alt</kbd> + <kbd>1</kbd> … <kbd>9</kbd> works inside the dialog too. Under a **Custom** look, **Save as…** writes the look over one of the nine configurations (the one it started from is marked `*`) and puts the drawing back on that number. Every drawing on that number changes with it.

A figure (any drawing but text, a volume profile, an anchored VWAP and a Fib retracement) has two tabs, **General** and **Text**. <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between them. A text drawing has no tabs, only **Colour**, **Font** and **Font size**.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit
state:
  (the state of the first figure)
  key ctrl+shift+d
  key Escape
  click 960 330
  key alt+Return
  move 1275 795
capture: bin/headless-window shot /tmp/props.png
crop: 840,220,880,1160
-->
{% include figure.html src="/assets/manual/drawings/properties-general.png" alt="The General tab of a box's properties, with a picture of the box tagged 6 across the top" caption="A box's **General** tab." %}

| Row | Kinds | Does |
|---|---|---|
| **Colour** | Line, level, arrow, zig-zag | One of the [nine presets](#colours), or **Custom…** for a hex colour |
| **Thickness** | Line, level, arrow, zig-zag | 0.5 to 12 px, by halves |
| **Arrow** | Line, level, arrow, zig-zag | **None**, **At the end**, **At the start**, **Both ends**, each drawn as it would look |
| **Arrowhead** | Line, level, arrow, zig-zag | **Filled**, **Open**, **Barb**. Greyed while there is no arrow |
| **Background** | Rectangle, circle | The fill's colour |
| **Transparency** | Rectangle, circle | How much of the candles shows through the fill, 0 to 100% |
| **Border** | Rectangle, circle | Whether it has an edge |
| **Border thickness**, **Border colour** | Rectangle, circle | The edge, while there is one |

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit
state:
  (the state of the previous figure)
  click 706 133        # Text
  move 1275 795
capture: bin/headless-window shot /tmp/propt.png
crop: 840,220,880,1160
-->
{% include figure.html src="/assets/manual/drawings/properties-text.png" alt="The Text tab of a box's properties, with the Position grid and the label range" caption="The **Text** tab of the same box." %}

| Row | Does |
|---|---|
| **Position** | Where a figure's label sits inside it, at a corner, an edge or the centre. Not on a text drawing, which sits at its point |
| **Text** | The words. Typing here removes bold and italic; set them on the chart |
| **Colour** | The text's colour |
| **Font** | **System font**, the desktop's own, or any font installed on this machine. System font is the default |
| **Font size** | 6 to 96 px; 18 by default |

## Configurations

A configuration is a saved look (colour, thickness, arrowheads, fill, font) for one kind of drawing. Each kind has nine, numbered 1 to 9. Every drawing follows one (configuration 1 unless you chose another) and changes when that configuration changes. Changing a property by hand takes the drawing off its configuration. Its look is then **Custom**, and changes to the configuration no longer reach it. Choosing a configuration puts it back on one. Changing a label does not take it off.

As shipped, the nine configurations are the nine [colours](#colours) in order, 1 Up, 2 Down, 3 Blue, 4 Amber, 5 Violet, 6 Teal, 7 Orange, 8 Cyan, 9 Ink, each on the kind's starting look. A volume profile's nine are its own ([Volume profiles](#volume-profiles)), and so are a Fib retracement's ([Fib retracement](#fib-retracement)).

| Kind | Starting look |
|---|---|
| Line, horizontal line | 1.5 px, no arrow |
| Arrow, zig-zag | 1.5 px, an open head at the end |
| Rectangle, circle | Filled at 16% of the colour, with a 1 px edge of it |
| Text | The colour, nothing else |
| Volume profile | See [Volume profiles](#volume-profiles) |
| Anchored VWAP | 1.5 px, no bands |
| Fib retracement | See [Fib retracement](#fib-retracement) |

Text in every configuration is 18 px in the system font, in the configuration's colour.

To edit a tool's configurations, right-click it and choose **Line configurations…** (or the matching item for the tool). Each row is a picture and a description; a row changed from how it shipped is marked **Edited**. Click a row for the same rows the properties have, and every drawing on it changes as you go. **Restore defaults** puts all nine back as shipped. **Done** or <kbd>Ctrl</kbd> + <kbd>Enter</kbd> closes.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit
state:
  (the state of the first figure)
  key ctrl+shift+d
  run bin/virtual-pointer --button right click 24 116
  click 90 139         # Line configurations…
  move 1275 795
capture: bin/headless-window shot /tmp/cfg.png
crop: 800,180,960,1240
-->
{% include figure.html src="/assets/manual/drawings/line-configurations.png" alt="The Line configurations dialog, listing configurations 1 to 9 with a picture of each line" caption="The **Line configurations** dialog, as shipped." %}

## Volume profiles

The volume profile tool draws a profile of the bars in the span you drag across: how much volume traded at each price, the value area drawn stronger, and the point of control as a line across it. It covers any stretch, across sessions or inside one, over a [Volume Profile](/manual/indicators/volume/#volume-profile) study or on its own. Its height is the range of prices those bars reached.

It is drawn under the bars, as the study's profiles are, so the candles stay on top. Selected, it has a grip at each corner: the left two move its start and the right two its end, sideways only, and the profile is worked out again. It cannot be moved whole or nudged with the arrow keys; a press on it that drags pans the chart. It takes no text. On a chart of another resolution it is worked out from that chart's bars. A pasted copy lands a few bars to the right of the original.

| Property | Does |
|---|---|
| **Outline** | Whether filled rows have an edge round them. Off by default; always on for an outline. Off, the outline's colour and thickness are greyed |
| **Outline colour** | The edge round the rows |
| **Outline thickness** | The outline. 1 px on a filled configuration, 1.5 px on an outline |
| **Background** | The rows |
| **Transparency** | Fades the outline and the background together. 0% is the look as configured |
| **Point of control** | The line across the busiest price |
| **Point of control thickness** | The point of control, 1.5 px |
| **Style** | **Filled**: the rows under the outline. **Outline**: the outline alone |
| **Alignment** | **Left**: the rows grow from the start of the span. **Right**: from its end |
| **Value area %** | The share of the span's volume the value area holds, 10 to 100% |

| Configuration | Colour | Style | Alignment |
|---|---|---|---|
| 1, 2, 3, 4 | Violet, Amber, Cyan, Blue | Filled | Left |
| 5, 6, 7, 8 | Violet, Amber, Cyan, Blue | Outline | Left |
| 9 | Ink | Outline | Right |

A volume profile's configurations leave out the candles' Up and Down, and Teal and Orange, which look close to green and red on some themes, because a profile shows where volume traded, not which way the price went. Every configuration's point of control is Ink, and its outline and background are the same colour.

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

## Anchored VWAP

The anchored VWAP tool draws the volume-weighted average price from the bar you click to the last bar on the chart: each bar's price, weighted by its volume, averaged over every bar since the anchor. It is what a buyer since that bar has paid on average, so it is anchored where something happened (an earnings gap, a swing low, the first bar of a breakout) and read as support or resistance from there. Unlike the [VWAP](/manual/indicators/averages-and-bands/#volume-weighted-average-price) study it never resets. On daily bars it follows weeks of trading from one bar.

It has no end. As new bars arrive the line runs on to them, worked out again from the anchor. A click past the last bar anchors it on the last bar, and one before the first on the first. On a bar with no volume, an index's, each bar counts once.

Selected, it glows in its own colour, as a [selected VWAP study](/manual/indicators/#selecting-an-indicator) does, and has one grip, at the anchor. Drag the grip to another bar and the line is worked out from there. It cannot be moved whole or nudged with the arrow keys; a press on the line that drags pans the chart. It takes no text. On a chart of another resolution it is worked out from that chart's bars, from the same moment. A pasted copy lands a few bars to the right of the original.

Its properties are the [VWAP](/manual/indicators/averages-and-bands/#volume-weighted-average-price) study's, in the same groups and order, with the same defaults: a line, and three bands at 1, 1.5 and 2 standard deviations, all off. When on, the first is shaded with no outline, the second dashed, the third a plain line. The deviation is weighted by volume, as the average is.

| Group | Row | Does | Default |
|---|---|---|---|
| **Parameters** | **Source** | The price each bar adds: **Typical** (high + low + close) / 3, the study's; **Median** (high + low) / 2; **Average** (open + high + low + close) / 4; or **Close** | Typical |
| **Line** | **Colour** | One of the [nine presets](#colours), or **Custom…** | The configuration's |
| | **Thickness** | No line, 1, 1.5, 2.5 or 4 px. No line leaves only the bands | 1.5 px |
| | **Line style** | Solid, Dashed, Dotted | Solid |
| **Band 1**, **Band 2**, **Band 3** | **Show** | Draws the band | Off |
| | **Standard deviations** | How far out it sits, 0.1 to 6, in tenths | 1, 1.5, 2 |
| | **Shaded** | Fills the area between its edges, under the candles | On for band 1 |
| | **Shading %** | How solid that fill is, 0 to 100% | 16% |
| | **Colour** | The band's colour. **Reset** goes back to the line's, which it follows | The line's |
| | **Thickness**, **Line style** | Its edges. No line leaves a shaded band with no outline | No line for band 1, 1 px dashed for band 2, 1 px solid for band 3 |

Every property is part of the configuration: **Save as…** keeps the bands with the rest, and a drawing on that configuration draws them. Every configuration is its colour at 1.5 px, solid, from the typical price, with no bands.

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

## Fib retracement

The Fib retracement tool draws a level at each Fibonacci ratio between two prices: press where a swing starts, drag, and release where it ends. The levels run across the time between the two points, at 0%, 23.6%, 38.2%, 50%, 61.8%, 78.6% and 100%. 0% is where you released and 100% where you pressed, so 61.8% is how far a move has come back once it has given up 61.8% of the swing. Drag up from a low to a high for the pullback from that high, or down from a high to a low for the bounce from that low.

A dashed line joins the two points, and each level is labelled at its left end with its ratio, as a percentage. **Price labels** adds the price beside it. Every level is the drawing's one colour unless you give it one of its own.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts config set theme omarchy:tokyo-night   (before the window opens)
  omacharts chart set --symbol SPY --resolution 1D --template none
  key ctrl+shift+b   (until a chart screenshot is the window's full width: the sidebar hidden)
  omacharts chart drawing add fib --from 2026-06-22,717 --to 2026-08-13,780 --config 2
  omacharts chart drawing add fib --from 2026-10-06,782 --to 2026-11-04,733 --config 1
  move 1270 1000
capture: bin/headless-window cli chart screenshot --output /tmp/fib.png; magick /tmp/fib.png -quality 88 site/assets/manual/drawings/fib-retracement.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/fib-retracement.jpg" alt="SPY daily with two Fib retracements in a soft grey: one drawn up the summer rally, solid, and one drawn down the fall from the October high, its middle levels dashed and dotted" caption="Two retracements: configuration 2, every level solid, dragged up a rally, and configuration 1, as a new one is drawn, dragged down a later fall." %}

Selected, it has a grip at each end. Drag a grip to move that end, or the levels or the dashed line to move the whole drawing. The arrow keys nudge it. It takes no text. A press between two levels goes to the chart under it.

| Group | Row | Does | Default |
|---|---|---|---|
| | **Colour** | The colour of every level that has none of its own. One of the [nine presets](#colours), or **Custom…** | The configuration's |
| | **Fib labels** | Labels each level with its ratio, as 61.8% | On |
| | **Price labels** | Labels each level with its price, after the ratio when both are on | Off |
| **Levels** | **0%** … **100%** | One row a level, with a switch that shows or hides it and a picture of how it is drawn. Click the row to open it | Every level shown |
| | **Colour** | The level's colour. **Reset** goes back to the drawing's, which it follows | The drawing's |
| | **Thickness** | 1, 1.5, 2.5 or 4 px | 1 px |
| | **Line style** | Solid, Dashed, Dotted | Solid |

**Thickness** in the drawing's menu, and <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>+</kbd> / <kbd>−</kbd>, set every level at once.

<!-- regenerate:
theme: catppuccin-latte
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts config set theme omarchy:catppuccin-latte   (before the window opens)
  (the state of the Fib retracement figure above, its two drawings)
  key Escape
  key alt+f
  drag 307 182 349 339
  omacharts chart drawing set --id 2 --level 0.618 --level-color amber --level-width 2.5 --level-style dashed --price-labels on
  omacharts chart drawing set --id 1 --fib-labels off
  move 1270 1000
capture: bin/headless-window cli chart screenshot --output /tmp/fiblight.png; magick /tmp/fiblight.png -quality 88 site/assets/manual/drawings/fib-levels-light.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/fib-levels-light.jpg" alt="SPY daily on a light theme with three retracements in a mid grey: the summer rally's with no labels, a short June fall drawn with the mouse, and the October fall labelled with ratios and prices and its 61.8% level a thicker dashed amber" caption="On a light theme: no labels on the first, and on the last prices beside the ratios and its 61.8% level in Amber, 2.5 px, dashed." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the state of the Fib retracement figure above, its two drawings)
  key Escape
  click 1088 189   (the later retracement's 50% level)
  key alt+Return
  scroll 824 500 6
capture: bin/headless-window shot /tmp/fibprops.png; magick /tmp/fibprops.png -crop 926x1406+817+17 +repage site/assets/manual/drawings/fib-properties.png
crop: 817,17,926,1406
-->
{% include figure.html src="/assets/manual/drawings/fib-properties.png" alt="A Fib retracement's properties: Colour, Fib labels on and Price labels off, then the seven levels from 0% to 100%, each with its switch and a sample of its line" caption="A retracement's properties, on configuration 1." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the state of the previous figure)
  click 824 539    (the 61.8% row's arrow)
  scroll 824 500 6
capture: bin/headless-window shot /tmp/fiblevel.png; magick /tmp/fiblevel.png -crop 926x1406+817+17 +repage site/assets/manual/drawings/fib-level-properties.png
crop: 817,17,926,1406
-->
{% include figure.html src="/assets/manual/drawings/fib-level-properties.png" alt="The same properties with the 61.8% level open on its Colour, Thickness and Line style, dotted" caption="A level open: its own colour, thickness and line style." %}

Its nine configurations are one colour each, with no fills. None is Up or Down: a retracement is drawn over a move either way.

| Configuration | Is |
|---|---|
| 1 | Ink, 1 px, with 0% and 100% solid, 50% dashed and the rest dotted. Drawn soft, as a light grey on a dark theme and a mid grey on a light one |
| 2 | Ink, every level a solid 1 px line |
| 3 to 8 | Blue, Amber, Violet, Teal, Orange and Cyan, every level solid |
| 9 | Ink, without 23.6% and 78.6% |

Every property is part of the configuration, so **Save as…** keeps each level's colour, thickness and line style, and whether it is shown.

## Colours

A drawing's colour is one of nine presets, or a hex colour.

| Preset | Is |
|---|---|
| **Up**, **Down** | The theme's candle colours |
| **Blue**, **Amber**, **Violet**, **Teal**, **Orange**, **Cyan** | The theme's colours of those names |
| **Ink** | A grey matching the chart's text |

A preset changes with the theme ([Themes and colours](/manual/themes/)). A hex colour does not. **Custom…** in any colour row picks one.

A label's colour is lightened or darkened when it would not read against the fill or the chart background.

## Text and labels

A text drawing is text at a point. Any other drawing can carry a label, which sits inside it.

| Key or gesture | Does |
|---|---|
| <kbd>Enter</kbd>, <kbd>F2</kbd>, **Edit text**, or double-click a text drawing | Puts the caret in the selected drawing, on the chart, in its own font and colour. Not on a volume profile, an anchored VWAP or a Fib retracement, which take no text |
| <kbd>Enter</kbd> while typing, or a click elsewhere | Commits |
| <kbd>Shift</kbd> + <kbd>Enter</kbd> | A new line |
| <kbd>Ctrl</kbd> + <kbd>B</kbd>, <kbd>Ctrl</kbd> + <kbd>I</kbd> | Bold or italic, over the selection or the word the caret is in |
| <kbd>Esc</kbd> while typing | Leaves the text as it was. A new text drawing with nothing typed goes away |
| <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> | Grows or shrinks the selected drawing's text by a pixel. On a figure, its label, not the figure |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> | Puts a figure's label against that edge. Only when it has a label; otherwise the arrows nudge |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>.</kbd> | Puts the label back in the centre |

A corner position is set in the properties' **Position** grid. While you are typing in a drawing, <kbd>Ctrl</kbd> + <kbd>B</kbd> and <kbd>Ctrl</kbd> + <kbd>I</kbd> are bold and italic and nothing else: <kbd>Ctrl</kbd> + <kbd>I</kbd> does not open the indicators.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit
state:
  (the state of the first figure)
  key ctrl+shift+d
  key Escape
  click 1050 85
  key Return
  type "new high"
  key ctrl+shift+b
capture: bin/headless-window shot /tmp/typing.png
crop: 1780,40,560,400
-->
{% include figure.html src="/assets/manual/drawings/typing-a-label.png" alt="A red circle selected, with new high being typed inside it, the word high in bold and the caret after it" caption="Typing a label on a circle. The word under the caret is bold." %}

## Hiding the drawings

<kbd>Ctrl</kbd> + <kbd>H</kbd> hides every drawing on the focused chart, its own and those other charts send it, and pressing it again brings them back. **Drawings** in the chart menu does the same. Nothing is removed: while they are hidden, none can be selected or deleted, and **Remove all drawings** and <kbd>Ctrl</kbd> + <kbd>Z</kbd> leave them alone. Picking up a drawing tool shows them again. Hidden drawings are kept with the chart.

While they are hidden, a struck-through pencil after the chart's resolution says so. Click it to show them again.

## Sharing drawings between charts

Each chart has two switches, in the **Drawings** group of [Chart settings](/manual/chart-settings/) (<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>S</kbd>). Both are on by default.

| Switch | Off means |
|---|---|
| **Show drawings from other charts** | The chart shows only its own drawings |
| **Send drawings to other charts** | What is drawn on this chart stays on this chart |

Both act on existing drawings as well as new ones. Turning sending off takes this chart's drawings off the other charts, and turning it back on sends them again.

New charts take their defaults from Preferences › Charts › Defaults › **Drawings**, the same two switches ([Chart settings](/manual/chart-settings/#defaults-for-new-charts)).

**Remove all drawings** in the chart menu removes every drawing the chart shows. <kbd>Ctrl</kbd> + <kbd>Z</kbd> brings them back.

## See also

- [Drawing from the CLI](/manual/cli/#drawings)
- [Measuring with the CLI](/manual/cli/#measuring-a-move)
