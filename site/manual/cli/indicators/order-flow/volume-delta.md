---
title: "Volume Delta"
description: "Adding a volume delta and its flags."
section: agents
parent: cli/indicators/order-flow
nav_order: 2
---

Each bar's buying less selling, as a candle from nought ([Volume Delta](/manual/indicators/order-flow/volume-delta/)), as `volume_delta`.

| Flag | Takes | For |
|---|---|---|
| `--height F` | 0.05 to 0.95; 0.16 when not given | Its strip |
| `--colouring C`, `--buy-color C`, `--sell-color C`, `--buy-transparency F`, `--sell-transparency F` | As for [every order flow indicator](/manual/cli/indicators/order-flow/#colours); solid unless set | Its candles |

```
omacharts chart indicator add volume_delta --height 0.2 --colouring theme-quiet
```

## See also

- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
