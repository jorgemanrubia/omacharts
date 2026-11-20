---
title: "Arranging charts"
description: "Splitting, closing, focusing and listing the charts of a chartbook."
section: agents
parent: cli
nav_order: 9
---

`chart split`, `chart close`, `chart focus` and `chart list` arrange the
charts of a chartbook ([Splitting and arranging charts](/manual/layouts/)).
`--book` and `--chart` pick the chartbook and chart ([Which chart a command
acts on](/manual/cli/#which-chart-a-command-acts-on)). A chartbook that is not open can
be changed too; the change is there when you open it.

Divider positions, and the heights of indicator strips dragged within a
chart, are saved with the chartbook. Neither has a command.

## `chart split`

```
$ omacharts chart split horizontal
split chart 6 horizontally; the new chart is 7
  [exit 0]
```

| Argument or flag | Takes |
|---|---|
| `DIRECTION` | `horizontal` (side by side) or `vertical` (stacked) |
| `--chart CHART` | The chart to split; the focused one when not given |
| `--book BOOK` | The chartbook; the open one when not given |
| `--json` | The result as JSON |

## `chart close`

```
$ omacharts chart close --chart pos:1
closed chart 13
  [exit 0]

$ omacharts chart close
omacharts: this is the only chart in the chartbook; there has to be one
  [exit 5]
```

Takes `--chart`, `--book` and `--json`.

## `chart focus`

Makes a chart the one the keyboard, the menus and later commands act on.

```
$ omacharts chart focus pos:0
focused pos:0 NVDA 10m
  [exit 0]

$ omacharts chart focus pos:7
omacharts: this chartbook has 3 charts, so there is no pos:7
  [exit 3]
```

| Argument or flag | Takes |
|---|---|
| `CHART` | `pos:N`, or a raw id from `chart list` |
| `--book BOOK` | The chartbook; the open one when not given |
| `--json` | The result as JSON |

## `chart list`

Lists the charts in a chartbook: raw id, symbol, resolution, bar style
(`none` while the bars are hidden), link mode and chart title, with `*` on the focused one. `--chart` takes these raw ids. The
list is by id, not layout order; `status show` gives positions.

```
$ omacharts chart list
Desk — watchlist "Default"
* 159  NVDA         1D     candles  follow  "Semis desk"
  160  AMD          1h     candles  follow
  161  AMD          15m    candles  follow
  [exit 0]
```

`--book` lists another chartbook. `--json` gives each chart in full:
its chart title, `title` (`null` when it has none), colouring, session, gridlines, both scales, whether it shows and sends
drawings, indicators and template.
