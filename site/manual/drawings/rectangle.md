---
title: "Rectangle"
description: "A box between two corners"
section: charting
parent: drawings
nav_order: 6
hotkey: "Alt + R"
---

A box between two corners.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>R</kbd> | Press at one corner, release at the other | `rect` |

## Properties

Two tabs: **General** for the rectangle itself and **Text** for a label in it. <kbd>Alt</kbd> + <kbd>Enter</kbd> or a double-click on the rectangle opens them, and <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between the tabs. Defaults are those of a configuration as shipped.

### General

| Property | What it does | Default |
|---|---|---|
| **Background** | The fill's colour: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour that stays the same in every theme | The configuration's |
| **Transparency %** | How much of the chart shows through the fill, 0 to 100%, on a slider with a box to type it in. At 0% the fill is solid and hides the candles under it; at 100% there is no fill, only the border | 87% |
| **Border** | Draws an edge round the rectangle. Off leaves the fill on its own. The three border rows below show only while it is on | On |
| **Border colour** | The edge's colour. The edge is drawn at about a third of the colour's strength, so it reads as the rim of the fill rather than a separate line | The configuration's |
| **Border thickness** | How thick the edge is, 0.5 to 12 px in halves, drawn just inside the box so the box keeps its size | 1 px |
| **Border style** | **Solid**, **Dashed** or **Dotted** | Solid |
| **Select by border only** | Makes only the edge pick the rectangle up, whether or not a border is drawn. A click or a <kbd>Shift</kbd> + drag inside it then goes to the chart and the drawings under it. Turn it on for a large rectangle over bars you still want to click and draw on | Off |

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit
state:
  (the state of the first figure on the Drawings page)
  key ctrl+shift+d
  key Escape
  click 960 330
  key alt+Return
  move 1275 795
capture: bin/headless-window shot /tmp/props.png
crop: 840,220,880,1160
-->
{% include figure.html src="/assets/manual/drawings/properties-general.png" alt="The General tab of a box's properties, with a picture of the box tagged 6 across the top" caption="A box's **General** tab." %}

### Text

| Property | What it does | Default |
|---|---|---|
| **Position** | Where the label sits inside the rectangle, picked on a grid of nine: a corner, the middle of an edge, or the centre. It keeps a small margin from the edge | Centre |
| **Text** | The label's words. Empty means no label. Typing here drops any bold or italic; to set those, type on the chart instead ([Text and labels](/manual/drawings/#text-and-labels)) | Empty |
| **Colour** | The label's colour. It is lightened or darkened where it would not read against the fill | The configuration's |
| **Font** | **System font**, the desktop's own, named with its family, or any font installed on this computer | System font |
| **Font size** | The label's size, 6 to 96 px. <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> on the chart changes it a pixel at a time | 18 px |

**Position** and **Text** belong to this rectangle alone. Every other property is part of its configuration, so changing one gives the rectangle a **Custom** look ([Configurations](/manual/drawings/#configurations)).

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

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
