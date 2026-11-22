---
title: "Volume Delta"
description: "Each bar's buying less selling, as a candle from nought"
section: charting
parent: indicators/order-flow
nav_order: 2
---

Each bar's volume traded at the ask less its volume traded at the bid, drawn as one candle per bar in a strip of its own. A candle opens at nought and closes at the bar's delta. Its wicks are the highest and lowest the delta reached inside the bar, minute by minute. It is the [cumulative delta](/manual/indicators/order-flow/cumulative-delta/) started again at every bar.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| In a strip of its own | **VD** | `vd` | `volume_delta` |

<!-- regenerate:
theme: midnight, the fallback when the sandboxed HOME has no Omarchy theme
window: 1400x900 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
state:
  key ctrl+comma; click 267 252 (Data); click 939 222 (Sign in, Fake Rich Feed)
  click 643 419; type 12345; click 606 473; type demo; click 767 542; click 1166 91; key Escape
  omacharts chartbook create "Order flow" --symbol ES --switch
  omacharts chart set --resolution 1m
  omacharts chart indicator add volume_delta
capture: bin/headless-window cli chart screenshot --output /tmp/volume-delta.png; magick /tmp/volume-delta.png -quality 88 site/assets/manual/indicators/volume-delta.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/volume-delta.jpg" alt="ES one-minute candles with a strip under them of one candle a bar, rising from nought for more buying and falling for more selling" caption="Volume Delta under ES 1m, on the synthetic feed." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Pane height %** | 16% | 5 to 95% |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet**, **Monochrome** or **Custom** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Opacity** | 100% | How solid buying and selling are both drawn, 0 to 100%. Shown for every colouring but **Custom** |
| **Buying** | The colour buying was drawn in before **Custom** was chosen | A palette name, or a custom colour, with its own **Opacity**. Shown only for **Custom** |
| **Selling** | The same for selling | The same |

A bar with more buying than selling is drawn in the buying colour, one with more selling in the selling colour.

## Estimated delta

On a feed without volume at price, and for bars from before the feed's history begins, a bar's delta is estimated from the bar as the cumulative delta's is ([Estimated delta](/manual/indicators/order-flow/cumulative-delta/#estimated-delta)): all its volume as bought when it closed up, as sold when it closed down. The legend then reads **VD ≈**, and an estimated candle has no wicks.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Volume Delta from the CLI](/manual/cli/indicators/order-flow/volume-delta/)
