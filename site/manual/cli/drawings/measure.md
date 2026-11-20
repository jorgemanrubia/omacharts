---
title: "Measure"
description: "A ruler on the focused chart, and the measurements it returns."
section: agents
parent: cli/drawings
nav_order: 1
---

`omacharts chart measure` displays a temporary ruler on the focused chart and
returns its measurements. A window must be open; otherwise it exits 6.

| Flag | Does |
|---|---|
| `--from WHEN,PRICE` | Starting point, with the date or time read in the chart's time zone. Unix seconds are accepted too |
| `--to WHEN,PRICE` | End point; required with `--from` |
| `--clear` | Clears the ruler. Use it alone |
| `--json` | Returns the chart position and the measurement: `from`, `to`, `price_change`, `percent_change`, `bars` and `elapsed_seconds` |

```
$ omacharts chart measure --from 2026-09-01,650 --to 2026-10-15,700
+7.69% +50.00; 31 bars, 6w 2d on pos:0 SPY 1D in "Chartbook 1"
  [exit 0]
```

The reply is the percentage change, the price change at the price axis's
precision, the bar distance and the elapsed time. Bar distance counts column
intervals; elapsed time includes market closures. With fewer than two loaded
candles the reply has no bar distance and `bars` is `null`. A zero starting price has
`null` for `percent_change`. The measurement is not saved or shared. The mouse
tool is in [Measure](/manual/drawings/measure/).
