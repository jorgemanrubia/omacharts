---
title: "Footprint"
description: "Each bar's delta at every price, drawn beside a narrow candle once the bars are wide enough"
section: charting
parent: indicators/order-flow
nav_order: 3
---

Each bar's volume at the ask less its volume at the bid, at every price it traded, drawn inside the bar. The candle narrows to the bar's left edge and is drawn hollow. Beside it each price is a row, its bar as long as its delta and its number inside it, or just past its end when it is short, in the buying colour when more was bought there and the selling colour when more was sold. Zoomed out, the chart shows its ordinary candles.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the candles, from a bar width | **FP** | `fp` | `footprint` |

<!-- regenerate:
theme: midnight, the fallback when the sandboxed HOME has no Omarchy theme
window: 1400x900 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
state:
  key ctrl+comma; click 267 252 (Data); click 939 222 (Sign in, Fake Rich Feed)
  click 643 419; type 12345; click 606 473; type demo; click 767 542; click 1166 91; key Escape
  omacharts chartbook create "Order flow" --symbol ES --switch
  omacharts chart set --resolution 1m
  omacharts chart indicator add footprint
  scroll 1000 400 -2, nine times, to zoom in past 64 px a bar
capture: bin/headless-window cli chart screenshot --output /tmp/footprint.png; magick /tmp/footprint.png -quality 88 site/assets/manual/indicators/footprint.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/footprint.jpg" alt="ES one-minute bars zoomed in, each a narrow hollow candle beside rows of buying minus selling at each price, with the number written by each row" caption="A footprint on ES 1m, zoomed in, on the synthetic feed." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Min bar width px** | 64 px | 24 to 400 px. Narrower bars are drawn as candles |
| **Shade by delta** | On | Off fills every row alike, at the side's **Opacity** |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet** or **Monochrome** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Buying** | The colouring's up colour, at 85% opacity | A palette name, or a custom colour, with its **Opacity** |
| **Selling** | The colouring's down colour, at 85% opacity | The same |

## Rows

A row is one tick while the ticks are at least 3 px tall. When they are thinner, a row holds 2, 4, 5, 10, 20, 25, 50 or more ticks together, so the rows stay readable, and every bar's rows cover the same prices. A row's number is written inside it when the row is long enough to hold it and just past its end when not. It is left out once the rows are under 11 px tall, and written short past a thousand: **1.2K**, **-35K**, **2.5M**.

## Shade by delta

With **Shade by delta** on, a row is filled more solidly the bigger its delta, in five steps from faint up to the side's **Opacity**. The strongest is measured against the biggest delta of any row on screen, so a row's length and its strength say the same thing, and scrolling to a busier stretch makes the rest fainter. Each row keeps a thin outline in its own colour, so a faint row is still seen. In **Monochrome**, buying is never fainter than half and selling never more solid than half, so the strongest sell does not read as a buy.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Volume profile](/manual/indicators/volume/volume-profile/), the same measure over a session
- [Footprint from the CLI](/manual/cli/indicators/order-flow/footprint/)
