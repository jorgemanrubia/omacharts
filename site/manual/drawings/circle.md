---
title: "Circle"
description: "An ellipse inside a box between two corners"
section: charting
parent: drawings
nav_order: 7
hotkey: "Alt + C"
---

An ellipse that fills the box between two corners.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>C</kbd> | Press at one corner, release at the other, as for a [rectangle](/manual/drawings/rectangle/) | `ellipse`, `circle` |

## Properties

Two tabs: **General** for the circle itself and **Text** for a label in it. <kbd>Alt</kbd> + <kbd>Enter</kbd> or a double-click on the circle opens them, and <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between the tabs. Defaults are those of a configuration as shipped.

### General

| Property | What it does | Default |
|---|---|---|
| **Background** | The fill's colour: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour that stays the same in every theme | The configuration's |
| **Transparency %** | How much of the chart shows through the fill, 0 to 100%, on a slider with a box to type it in. At 0% the fill is solid and hides the candles under it; at 100% there is no fill, only the border | 87% |
| **Border** | Draws an edge round the circle. Off leaves the fill on its own. The three border rows below show only while it is on | On |
| **Border colour** | The edge's colour. The edge is drawn at about a third of the colour's strength, so it reads as the rim of the fill rather than a separate line | The configuration's |
| **Border thickness** | How thick the edge is, 0.5 to 12 px in halves | 1 px |
| **Border style** | **Solid**, **Dashed** or **Dotted** | Solid |
| **Select by border only** | Makes only the edge pick the circle up, whether or not a border is drawn. A click or a <kbd>Shift</kbd> + drag inside it then goes to the chart and the drawings under it. Turn it on for a large circle over bars you still want to click and draw on | Off |

### Text

| Property | What it does | Default |
|---|---|---|
| **Position** | Where the label sits inside the circle, picked on a grid of nine: a corner, the middle of an edge, or the centre. Corners and edges are inside the curve, so a top-left label stays within the circle rather than in the corner of the box around it | Centre |
| **Text** | The label's words. Empty means no label. Typing here drops any bold or italic; to set those, type on the chart instead ([Text and labels](/manual/drawings/#text-and-labels)) | Empty |
| **Colour** | The label's colour. It is lightened or darkened where it would not read against the fill | The configuration's |
| **Font** | **System font**, the desktop's own, named with its family, or any font installed on this computer | System font |
| **Font size** | The label's size, 6 to 96 px. <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> on the chart changes it a pixel at a time | 18 px |

**Position** and **Text** belong to this circle alone. Every other property is part of its configuration, so changing one gives the circle a **Custom** look ([Configurations](/manual/drawings/#configurations)).

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
