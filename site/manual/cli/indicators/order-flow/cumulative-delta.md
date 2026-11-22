---
title: "Cumulative Delta"
description: "Adding a cumulative delta and its flags."
section: agents
parent: cli/indicators/order-flow
nav_order: 1
---

Buying less selling added up over a period ([Cumulative Delta](/manual/indicators/order-flow/cumulative-delta/)), as `cvd`.

| Flag | Takes | For |
|---|---|---|
| `--anchor P` | `session`, `week`, `month`, `quarter`, `year`, or `never` for one running total | Where it starts again from nought |
| `--cvd-style S` | `candles`, `line` | How it is drawn |
| `--colouring C` | `default`, the one chosen in Preferences, or `theme`, `theme-soft`, `theme-quiet`, `theme-mono` (or `monochrome`), or `custom` | Its rises and falls. `custom` keeps each colour its own, starting from the colours drawn a moment before; any other lets the custom colours go |
| `--up-color C`, `--down-color C` | A colour | Its rises' or falls' colour. Given on a colouring other than `custom`, it makes the colouring `custom`, the other colour kept as drawn |
| `--height F` | 0.05 to 0.95; 0.16 when not given | Its strip |

As a line it also takes `--color`, `--transparency`, `--width` and `--line-style`.

```
omacharts chart indicator add cvd --anchor week --cvd-style line
```

## See also

- [Order flow from the CLI](/manual/cli/indicators/order-flow/)
