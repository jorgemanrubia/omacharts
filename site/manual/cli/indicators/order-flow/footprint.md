---
title: "Footprint"
description: "Adding a footprint and its flags."
section: agents
parent: cli/indicators/order-flow
nav_order: 3
---

Each bar's delta at every price, drawn inside the bar once the bars are wide enough ([Footprint](/manual/indicators/order-flow/footprint/)), as `footprint`.

| Flag | Takes | For |
|---|---|---|
| `--intensity B` | `on`, `off`; on when not given | Rows filled more solidly the bigger their delta, against the biggest on screen. Off fills every row alike |
| `--numbers B` | `on`, `off`; on when not given | Each row's delta written in it. Off draws the rows alone |
| `--row-ticks N` | `auto`, or ticks to a row, 1 to 10000; `auto` when not given | How many ticks a row holds. `auto` is the fewest of 1, 2, 4, 5, 10 and on that make a row about a line of small type tall at the chart's price scale |
| `--profile B` | `on`, `off`; off when not given | Each bar's volume profile at its left, the narrow bar and the delta rows moved right of it ([Volume profile](/manual/indicators/order-flow/footprint/#volume-profile)) |
| `--profile-color C` | A palette name, `#rrggbb`, or `default` for the theme's Blue | The profile's colour |
| `--profile-transparency F` | 0 to 1, or `default` (0.45 at its strongest row) | How much of the chart shows through the profile |
| `--profile-numbers B` | `on`, `off`; on when not given | Each profile row's total written in it |
| `--colouring C`, `--buy-color C`, `--sell-color C`, `--buy-transparency F`, `--sell-transparency F` | As for [every order flow indicator](/manual/cli/indicators/order-flow/#colours); the strongest row at 0.15 transparency unless set | Its rows |

`list --json` reads them back as `params.intensity`, `params.numbers` and `params.row_ticks` (`null` for `auto`), and the profile as `params.profile`: `on`, `numbers`, and `colour` and `alpha` (how solid, 1 less the transparency) once set, absent for their defaults. A footprint whose profile was never set has no `params.profile`.

```
omacharts chart indicator add footprint --intensity off
omacharts chart indicator set footprint --numbers off
omacharts chart indicator set footprint --row-ticks 4
omacharts chart indicator set footprint --profile on --profile-color violet --profile-numbers off
omacharts chart values --indicator footprint --last 5 --row-ticks 2 --json
```

## Reading it out

`chart values --indicator footprint` reads each bar's footprint back in numbers, row by row as the chart draws it: the same ladders, summed and folded into rows the same way. It reads the chart on screen, so it needs a window open. No footprint has to be on the chart. On a feed without volume at price it reads only `unmet`, as a footprint draws only its line there. `--footprint` is the same as `--indicator footprint`.

| Flag | Takes | For |
|---|---|---|
| `--row-ticks N` | `auto`, or ticks to a row, 1 to 10000 | How many ticks a row holds. `auto`, the default, is the footprint's own `--row-ticks`, else the step it draws, else one tick |
| `--last N` | 1 to 10000 | The last N bars, the forming one among them |
| `--from WHEN`, `--to WHEN` | `2026-09-01`, `2026-09-01T14:30` in the chart's clock, or unix seconds | The bars opening from `--from` up to, not including, `--to`; without `--to`, up to the newest |
| `--visible` | | The bars on screen |

With none of them it reads the newest bar. The rest of `chart values` is in [Charts](/manual/cli/charts/#chart-values).

`--json` writes one object:

```json
{"chart": "pos:0 ES 1m", "symbol": "ES", "resolution": "1m",
 "indicators": [{"id": 3, "kind": "footprint", "label": "Footprint", "visible": true, "tick_size": 0.25, "row_ticks": 4,
   "bars": [{"time": 1789990200, "open": 6000.25, "high": 6003.5, "low": 5999.75, "close": 6002.0,
             "volume": 1834, "delta": 212, "poc": 6001.0,
             "rows": [{"low": 5999.0, "high": 5999.75, "at_bid": 96, "at_ask": 40, "delta": -56, "total": 136, "trades": 51},
                      {"low": 6000.0, "high": 6000.75, "at_bid": 210, "at_ask": 260, "delta": 50, "total": 470, "trades": 160}]}]}]}
```

The footprint is an entry in `indicators`, its `id` `null` when none is drawn.

- `time` is when the bar opens, in unix seconds.
- `open`, `high`, `low`, `close` and `volume` are the bar's.
- `delta` is bought at the ask less sold at the bid over the bar: the volume delta's bar.
- `poc` is the low price of the bar's most traded row, the lowest of a tie; `null` with no rows.
- `rows` run lowest first, one for each row that traded. `low` and `high` are the first and last price it covers, `at_bid` what sold into the bid and `at_ask` what was bought from the ask, `delta` the second less the first, and `total` everything that traded there, a trade with no known side included. That is the row the footprint's volume profile draws. `trades` counts the trades with a side, where the feed counts them.

Without `--json`, it writes a line a bar (time, open, high, low, close, volume, delta and point of control), then its rows from the top, each written as its low price, `at_bid x at_ask`, delta, total and trades.

The large orders are not read out bar by bar: their bubbles are single trades, which `trading watch --json` already reports as they print.

## See also

- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
