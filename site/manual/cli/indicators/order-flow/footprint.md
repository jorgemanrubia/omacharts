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
| `--show-from PX` | 24 to 400 pixels; 64 when not given | The bar width it is drawn from. Narrower bars are drawn as candles |
| `--intensity B` | `on`, `off`; on when not given | Rows filled more solidly the bigger their delta, against the biggest on screen. Off fills every row alike |
| `--colouring C`, `--buy-color C`, `--sell-color C`, `--buy-transparency F`, `--sell-transparency F` | As for [every order flow indicator](/manual/cli/indicators/order-flow/#colours); the strongest row at 0.15 transparency unless set | Its rows |

`list --json` reads them back as `params.zoom` and `params.intensity`.

```
omacharts chart indicator add footprint --show-from 90 --intensity off
```

## See also

- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
