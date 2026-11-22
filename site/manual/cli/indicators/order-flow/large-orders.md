---
title: "Large Orders"
description: "Adding large orders and their flags."
section: agents
parent: cli/indicators/order-flow
nav_order: 4
---

A bubble at each trade bigger than a size you choose ([Large Orders](/manual/indicators/order-flow/large-orders/)), as `large_orders`.

| Flag | Takes | For |
|---|---|---|
| `--rth-threshold N` | A whole number of contracts, 0 or more; 50 when not given | The size a trade in the regular session has to be bigger than |
| `--eth-threshold N` | The same; 20 when not given | The same outside the regular session |
| `--size-against W` | `visible`, `session`, `hours`; `session` when not given | What a bubble's size is measured against: the large orders on screen, the session's, or its regular or extended hours' |
| `--combine B` | `on`, `off`; on when not given | Prints of one moment and one side counted as one order |
| `--colouring C`, `--buy-color C`, `--sell-color C`, `--buy-transparency F`, `--sell-transparency F` | As for [every order flow indicator](/manual/cli/indicators/order-flow/#colours); 0.45 transparency unless set | Its bubbles |

`list --json` reads them back as `params.regular`, `params.extended`, `params.compare` and `params.combine`.

```
$ omacharts chart indicator add large_orders --rth-threshold 100 --eth-threshold 40 --size-against visible
```

## See also

- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
