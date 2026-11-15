---
title: "Arrow"
description: "A line with a head at the end"
section: charting
parent: drawings
nav_order: 4
hotkey: "Alt + A"
---

A line with a head at the end.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>A</kbd> | Placed as a [line](/manual/drawings/line/); the head is at the end you place last | `arrow` |

## Properties

Two tabs: **General** for the arrow itself and **Text** for a label on it. <kbd>Alt</kbd> + <kbd>Enter</kbd> or a double-click on the arrow opens them, and <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between the tabs. Defaults are those of a configuration as shipped. With **Arrow** set to None it is drawn as a plain line.

### General

| Property | What it does | Default |
|---|---|---|
| **Colour** | The arrow's colour: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour that stays the same in every theme | The configuration's |
| **Thickness** | How thick the arrow is drawn, 0.5 to 12 px in halves. An arrowhead grows with it | 1.5 px |
| **Line style** | **Solid**, **Dashed** or **Dotted**. The dashes and dots lengthen as the arrow thickens. An arrowhead is always solid | Solid |
| **Arrow** | Which ends get an arrowhead: **None**, **At the end** (the end you placed last), **At the start**, or **Both ends**. The list draws each choice in the arrow's colour and thickness | At the end |
| **Arrowhead** | The head's shape: **Filled**, a solid triangle; **Open**, a chevron; **Barb**, a point with swept-back sides. Greyed while **Arrow** is None | Open |

### Text

| Property | What it does | Default |
|---|---|---|
| **Position** | Where the label sits, picked on a grid of nine. Left, centre and right put it at the arrow's left end, its middle or its right end; the top row puts it above the arrow, the bottom row below, and the middle row just above it, so the words never cover the arrow. It stays on the part of the arrow that is on the chart, at any zoom | Centre |
| **Text** | The label's words. Empty means no label. Typing here drops any bold or italic; to set those, type on the chart instead ([Text and labels](/manual/drawings/#text-and-labels)) | Empty |
| **Colour** | The label's colour. It is lightened or darkened where it would not read against what is behind it | The configuration's |
| **Font** | **System font**, the desktop's own, named with its family, or any font installed on this computer | System font |
| **Font size** | The label's size, 6 to 96 px. <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> on the chart changes it a pixel at a time | 18 px |

**Position** and **Text** belong to this arrow alone. Every other property is part of its configuration, so changing one gives the arrow a **Custom** look ([Configurations](/manual/drawings/#configurations)).

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
