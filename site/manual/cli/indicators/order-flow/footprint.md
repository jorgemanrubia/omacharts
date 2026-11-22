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
| `--colouring C`, `--buy-color C`, `--sell-color C`, `--buy-transparency F`, `--sell-transparency F` | As for [every order flow indicator](/manual/cli/indicators/order-flow/#colours); the strongest row at 0.15 transparency unless set | Its rows |

`list --json` reads them back as `params.intensity`, `params.numbers` and `params.row_ticks` (`null` for `auto`).

```
omacharts chart indicator add footprint --intensity off
omacharts chart indicator set footprint --numbers off
omacharts chart indicator set footprint --row-ticks 4
```

## See also

- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
