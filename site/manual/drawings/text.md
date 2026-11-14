---
title: "Text"
description: "Words at a point on the chart"
section: charting
parent: drawings
nav_order: 8
hotkey: "Alt + T"
---

Words at a point on the chart.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>T</kbd> | Click where the text goes, then type ([Text and labels](/manual/drawings/#text-and-labels)) | `text` |

## Properties

One page with no tabs, since a text drawing is its words. What it says is typed on the chart ([Text and labels](/manual/drawings/#text-and-labels)). <kbd>Alt</kbd> + <kbd>Enter</kbd> or **Properties…** in its menu opens them; a double-click puts the caret in the words instead. Defaults are those of a configuration as shipped.

| Property | What it does | Default |
|---|---|---|
| **Colour** | The words' colour: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour that stays the same in every theme. It is lightened or darkened where it would not read against the chart | The configuration's |
| **Font** | **System font**, the desktop's own, named with its family, or any font installed on this computer | System font |
| **Font size** | The words' size, 6 to 96 px. <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>−</kbd> on the chart changes it a pixel at a time | 18 px |

All three are part of the configuration, so changing one gives the text a **Custom** look ([Configurations](/manual/drawings/#configurations)). Bold and italic belong to the words and are set on the chart, with <kbd>Ctrl</kbd> + <kbd>B</kbd> and <kbd>Ctrl</kbd> + <kbd>I</kbd>.

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
