---
title: "Order flow"
description: "The flags of the cumulative delta, the volume delta, the footprint and large orders."
section: agents
parent: cli/indicators
nav_order: 3
---

The flags `chart indicator add` and `set` take for [order flow](/manual/indicators/order-flow/): `cvd`, `volume_delta`, `footprint` and `large_orders`, beside the ones [every indicator takes](/manual/cli/indicators/#flags). Each has a page with its own:

{% include children.html head="Indicator" what="What it covers" %}

## Colours

The volume delta, the footprint and large orders paint buying and selling, and take these. None of them has a line, so `--color`, `--transparency`, `--width` and `--line-style` are refused.

| Flag | Takes | For |
|---|---|---|
| `--colouring C` | `default`, the one chosen in Preferences, or `theme` (Vivid), `theme-soft`, `theme-quiet`, `theme-mono` (or `monochrome`: one colour for both, the Monochrome candles' ink) | The colouring buying and selling are taken from |
| `--buy-color C`, `--sell-color C` | A colour, or `none` for the colouring's | Buying's or selling's colour, set apart from the colouring |
| `--buy-transparency F`, `--sell-transparency F` | 0 to 1, or `default` for the indicator's own | How much of the chart shows through buying or selling |

```
$ omacharts chart indicator set volume_delta --colouring theme-soft --sell-color Rose --sell-transparency 0.2
```

`list --json` reads them back under `params.inks`, which is left out while every one is its default:

```json
"inks": { "colouring": "theme-soft", "sell": { "kind": "swatch", "name": "Rose" }, "sell_alpha": 0.8 }
```

A transparency comes back as its opacity, `sell_alpha`, 1 less the transparency given.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Indicators from the CLI](/manual/cli/indicators/)
