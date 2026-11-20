---
title: "Comparing markets"
description: "Drawing other markets on a chart as lines in percent."
section: agents
parent: cli
nav_order: 5
---

`omacharts chart compare ACTION [SYMBOL…]` draws other markets on a chart as
lines in percent. What comparing is, and how it looks, is in
[Comparing markets](/manual/comparing/). `--book BOOK` and `--chart CHART` pick
the chart as under [Which chart a command acts on](/manual/cli/#which-chart-a-command-acts-on).

| Action | Does |
|---|---|
| `list` | The markets the chart compares, where they are measured from, and whether it is lines only. |
| `add SYMBOL…` | Compares one or more markets. A market already compared is said and left alone. The chart's own symbol is refused, exit `5`. `BTC-USD` is the coin listed as `BTC`. |
| `remove SYMBOL…` | Stops comparing them. A market the chart does not compare is refused, exit `3`. |
| `set` | Changes only the flags below. |
| `clear` | Stops comparing anything. Lines only and the base go with the markets. |

| Flag | Does |
|---|---|
| `--from WHEN` | The bar every line is 0% at: a date `2026-01-02`, a moment `2026-01-02T14:30` in the chart's time zone, or unix seconds. On daily and longer bars a date is that day's bar. `left` measures from the first bar on screen again |
| `--lines-only on`, `off` | Draws the chart's own symbol as a line instead of its bars |
| `--color COLOUR` | The colour of the markets named on the line: a palette name, `#rrggbb`, or `auto` for the theme's |
| `--json` | Returns the chart, `compare` (each market's `symbol` and `color`), `from` in unix seconds or `null`, `lines_only` and `message` |

Every symbol is checked before anything is written, so a command naming one
unknown symbol (exit `3`) compares none of them. `--from`, `--lines-only` and
`--color` on a chart that compares nothing are refused, exit `5`.

```
$ omacharts chart compare add GC NQ BTC-USD
compared GC, NQ, BTC on pos:0 SPY 1D in "Macro"
  [exit 0]

$ omacharts chart compare set --from 2026-01-02
from 2026-01-02 on pos:0 SPY 1D in "Macro"
  [exit 0]

$ omacharts chart compare set GC --color Rose
GC coloured Rose on pos:0 SPY 1D in "Macro"
  [exit 0]

$ omacharts chart compare list
pos:0 SPY 1D compared with GC, NQ, BTC, from 2026-01-02
  [exit 0]

$ omacharts chart compare add SPY
omacharts: pos:0 SPY 1D is SPY; compare it with another market
  [exit 5]

$ omacharts chart compare set --lines-only on --from left
from the left edge, lines only on pos:0 SPY 1D in "Macro"
  [exit 0]

$ omacharts chart compare clear
stopped comparing anything on pos:0 SPY 1D in "Macro"
  [exit 0]
```

`chart list --json` gives each chart's compared markets as `compare`.
