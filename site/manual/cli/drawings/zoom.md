---
title: "Zoom"
description: "Zooming the focused chart to a box of time and prices, and back."
section: agents
parent: cli/drawings
nav_order: 2
---

`omacharts chart zoom` zooms the focused chart to a stretch of bars and,
given them, a band of prices, as the [Zoom](/manual/drawings/zoom/) tool does.
`omacharts chart zoom back` goes back to the view before the last zoom. A
window must be open; otherwise it exits 6.

| Flag | Does |
|---|---|
| `--from WHEN` | The first bar shown: a date, a date and time, or unix seconds, read in the chart's time zone |
| `--to WHEN` | The last bar shown; required with `--from` |
| `--low PRICE`, `--high PRICE` | The prices at the bottom and top of the price scale, which stops fitting the bars. Give both or neither; without them the price scale is left as it is |
| `--json` | Returns the chart position, `changed`, the first and last moments in view as `from` and `to`, the prices at the bottom and top of the scale as `low` and `high`, and `back`, the number of zooms left to go back through |

```
$ omacharts chart zoom --from 2026-09-01 --to 2026-09-19 --low 180 --high 195
zoomed pos:0 NVDA 1D in "Chartbook 1" to 2026-09-01 to 2026-09-19, prices 180 to 195
  [exit 0]
$ omacharts chart zoom back
back to the view before the last zoom on pos:0 NVDA 1D in "Chartbook 1": 2026-04-10 to 2026-11-27, prices 158.31 to 214.6
  [exit 0]
$ omacharts chart zoom back
no zoom to go back from on pos:0 NVDA 1D in "Chartbook 1"
  [exit 0]
```

`back` takes no flags. Zooms made with the mouse and with the command go back
in the same order, and are forgotten when the chart's symbol or resolution
changes.
