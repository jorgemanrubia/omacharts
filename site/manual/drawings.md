---
title: "Drawings"
description: "Marks you put on a chart by hand, their looks, and which charts show them."
section: charting
nav_order: 17
moved:
  measuring-a-move: /manual/drawings/measure/
  volume-profiles: /manual/drawings/volume-profile/
  anchored-vwap: /manual/drawings/anchored-vwap/
  fib-retracement: /manual/drawings/fib-retracement/
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

{% include children.html head="Tool" what="What it draws" hotkey="Key" %}

<kbd>Esc</kbd> is the pointer, no tool, which selects, moves and resizes drawings. Each tool's page says how it is placed.

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

A rectangle or circle with **Select by border only** on is picked up by its edge alone, whether or not a border is drawn. A click inside it selects the drawing under it, or reaches the chart, and a <kbd>Shift</kbd> + drag inside it selects what it touches without the rectangle or circle.

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

A figure (any drawing but text, a volume profile, an anchored VWAP and a Fib retracement) has two tabs, **General** and **Text**. <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between them.

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
| **Line style** | Line, level, arrow, zig-zag | **Solid**, **Dashed**, **Dotted**. The dashes and dots grow with the thickness. An arrowhead is always solid |
| **Arrow** | Line, level, arrow, zig-zag | **None**, **At the end**, **At the start**, **Both ends**, each drawn as it would look |
| **Arrowhead** | Line, level, arrow, zig-zag | **Filled**, **Open**, **Barb**. Greyed while there is no arrow |
| **Background** | Rectangle, circle | The fill's colour |
| **Transparency %** | Rectangle, circle | How much of the candles shows through the fill, 0 to 100%, on a slider with a box to type it in |
| **Border** | Rectangle, circle | Whether it has an edge |
| **Border colour**, **Border thickness**, **Border style** | Rectangle, circle | The edge, shown while there is one. **Border thickness** is 0.5 to 12 px, by halves; **Border style** is Solid, Dashed or Dotted |
| **Select by border only** | Rectangle, circle | Off by default. On, only the edge selects it, whether or not a border is drawn, and a click or a <kbd>Shift</kbd> + drag inside it reaches the chart and the drawings under it |

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
| **Position** | Where a figure's label sits inside it, at a corner, an edge or the centre. A line's label is placed against the part of the line on the chart, so it shows wherever the line does, at any resolution or zoom. Not on a text drawing, which sits at its point |
| **Text** | The words. Typing here removes bold and italic; set them on the chart |
| **Colour** | The text's colour |
| **Font** | **System font**, the desktop's own, or any font installed on this machine. System font is the default |
| **Font size** | 6 to 96 px; 18 by default |

## Configurations

A configuration is a saved look (colour, thickness, arrowheads, fill, font) for one kind of drawing. Each kind has nine, numbered 1 to 9. Every drawing follows one (configuration 1 unless you chose another) and changes when that configuration changes. Changing a property by hand takes the drawing off its configuration. Its look is then **Custom**, and changes to the configuration no longer reach it. Choosing a configuration puts it back on one. Changing a label does not take it off.

As shipped, the nine configurations are the nine [colours](#colours) in order, 1 Up, 2 Down, 3 Blue, 4 Amber, 5 Violet, 6 Teal, 7 Orange, 8 Cyan, 9 Ink, each on the kind's starting look. A volume profile's nine are its own ([Volume profile](/manual/drawings/volume-profile/)), and so are a Fib retracement's ([Fib retracement](/manual/drawings/fib-retracement/)).

| Kind | Starting look |
|---|---|
| Line, horizontal line | 1.5 px, no arrow |
| Arrow, zig-zag | 1.5 px, an open head at the end |
| Rectangle, circle | Filled at 13% of the colour, with a 1 px edge of it |
| Text | The colour, nothing else |
| Volume profile | See [Volume profile](/manual/drawings/volume-profile/) |
| Anchored VWAP | 1.5 px, no bands |
| Fib retracement | See [Fib retracement](/manual/drawings/fib-retracement/) |

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

Text is typed on the chart, into a [text drawing](/manual/drawings/text/) or a label. Any other drawing can carry a label, which sits inside it.

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

With showing off, the chart still shows every drawing made on it, including the ones it sent to other charts. A drawing from an early version that does not record which chart made it counts as another chart's.

New charts take their defaults from Preferences › Charts › Defaults › **Drawings**, the same two switches ([Chart settings](/manual/chart-settings/#defaults-for-new-charts)).

**Remove all drawings** in the chart menu removes every drawing the chart shows. <kbd>Ctrl</kbd> + <kbd>Z</kbd> brings them back.

## See also

- [Hotkeys](/manual/hotkeys/#drawing)
- [Drawing from the CLI](/manual/cli/#drawings)
