---
title: "Zig-zag"
description: "A line through as many corners as you place"
section: charting
parent: drawings
nav_order: 5
hotkey: "Alt + Z"
---

A line through as many corners as you place.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>Z</kbd> | Press at each corner. A second press on the last corner, a double-click or <kbd>Enter</kbd> ends it there; <kbd>Esc</kbd> drops it | `zigzag` |

## Properties

Two tabs: **General** for the zig-zag itself and **Text** for a label on it. <kbd>Alt</kbd> + <kbd>Enter</kbd> or a double-click on the zig-zag opens them, and <kbd>Alt</kbd> + <kbd>←</kbd> / <kbd>→</kbd> moves between the tabs. Defaults are those of a configuration as shipped.

### General

| Property | What it does | Default |
|---|---|---|
| **Colour** | The zig-zag's colour: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour that stays the same in every theme | The configuration's |
| **Thickness** | How thick the zig-zag is drawn, 0.5 to 12 px in halves. An arrowhead grows with it | 1.5 px |
| **Line style** | **Solid**, **Dashed** or **Dotted**. The dashes and dots lengthen as the zig-zag thickens. An arrowhead is always solid | Solid |
| **Arrow** | Which ends get an arrowhead: **None**, **At the end** (its last corner), **At the start**, or **Both ends**. The list draws each choice in the zig-zag's colour and thickness | At the end |
| **Arrowhead** | The head's shape: **Filled**, a solid triangle; **Open**, a chevron; **Barb**, a point with swept-back sides. Greyed while **Arrow** is None | Open |

### Text

| Property | What it does | Default |
|---|---|---|
| **Position** | Where the label sits, picked on a grid of nine. Left, centre and right are across the zig-zag's width; the top row puts it above the zig-zag, the bottom row below it, and the middle row about halfway up, raised clear of the middle. It stays on the part of the zig-zag that is on the chart, at any zoom | Centre |
| **Text** | The label's words. Empty means no label. Typing here drops any bold or italic; to set those, type on the chart instead ([Text and labels](/manual/drawings/#text-and-labels)) | Empty |
| **Colour** | The label's colour. It is lightened or darkened where it would not read against what is behind it | The configuration's |
| **Font** | **System font**, the desktop's own, named with its family, or any font installed on this computer | System font |
| **Font size** | The label's size, 6 to 96 px. <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> on the chart changes it a pixel at a time | 18 px |

**Position** and **Text** belong to this zig-zag alone. Every other property is part of its configuration, so changing one gives the zig-zag a **Custom** look ([Configurations](/manual/drawings/#configurations)).

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
