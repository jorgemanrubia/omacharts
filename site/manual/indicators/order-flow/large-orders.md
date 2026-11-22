---
title: "Large Orders"
description: "A bubble at each trade bigger than a size you choose"
section: charting
parent: indicators/order-flow
nav_order: 4
---

A bubble at the price and bar of every trade bigger than a size you choose, in the buying colour for a buyer who lifted the offer and the selling colour for a seller who hit the bid. A bubble's area is in proportion to the trade's size, against the other large orders it is compared with.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the candles | **Large > 50 · 20** | `large` | `large_orders` |

<!-- regenerate:
theme: midnight, the fallback when the sandboxed HOME has no Omarchy theme
window: 1400x900 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
state:
  key ctrl+comma; click 267 252 (Data); click 939 222 (Sign in, Fake Rich Feed)
  click 643 419; type 12345; click 606 473; type demo; click 767 542; click 1166 91; key Escape
  omacharts chartbook create "Order flow" --symbol ES --switch
  omacharts chart set --resolution 1m
  omacharts chart indicator add large_orders
  (wait for the bubbles: the trades are read after the bars)
  scroll 1000 450 -2 (zoom in to about 1h45m); scroll 700 889 0 (park the pointer on the tab bar, no crosshair)
capture: bin/headless-window cli chart screenshot --output /tmp/large-orders.png; magick /tmp/large-orders.png -quality 88 site/assets/manual/indicators/large-orders.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/large-orders.jpg" alt="ES one-minute candles with bubbles of different sizes at the prices of large trades, green for buying and red for selling" caption="Large Orders on ES 1m, on the synthetic feed." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **RTH threshold** | 50 | A trade in the regular session counts when it is bigger than this, in contracts or shares. 0 counts every trade |
| **ETH threshold** | 20 | The same outside the regular session |
| **Size against** | Session | **Visible bars**: the large orders on screen, so bubbles change size as the chart scrolls. **Session**: the session's. **RTH or ETH**: the session's regular or extended hours', each against its own |
| **Combine same-time prints** | On | Off makes every print its own order |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet**, **Monochrome** or **Custom** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Opacity** | 35%, a bubble's rim stronger | How solid buying and selling are both drawn, 0 to 100%. Shown for every colouring but **Custom** |
| **Buying** | The colour buying was drawn in before **Custom** was chosen | A palette name, or a custom colour, with its own **Opacity**. Shown only for **Custom** |
| **Selling** | The same for selling | The same |

## Combining prints

An order that takes more than the best price offers fills against several resting orders, and the exchange reports each fill as a print of its own, at the same moment and on the same side. With **Combine same-time prints** on, a run of such prints is one order, its size the sum of theirs and its price their average weighted by size. A print whose side is not known is always an order of its own.

## How far back

Large orders start where the feed's trades start. On Rithmic, earlier days' trades are fetched once from its history and kept. Rithmic publishes no limit on how far back that goes. On Sierra Chart they are read from its own data files. Nothing is shown for days before the trades begin, and the chart is drawn first: the bubbles of the last two days appear once their trades are read, and the earlier ones after.

A continuous chart of a futures root shows the large orders of the month that is front now. An earlier month traded at prices of its own, and its trades are not moved onto the continuous chart.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Footprint](/manual/indicators/order-flow/footprint/)
- [Large Orders from the CLI](/manual/cli/indicators/order-flow/large-orders/)
