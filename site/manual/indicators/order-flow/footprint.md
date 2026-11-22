---
title: "Footprint"
description: "Each bar's delta at every price, drawn beside a narrow bar once the bars are wide enough"
section: charting
parent: indicators/order-flow
nav_order: 3
---

Each bar's volume at the ask less its volume at the bid, at every price it traded, drawn inside the bar. The chart's own bar narrows to the bar's left edge, in the chart's style and colours: a narrow candle, filled or hollow as the chart's are, a narrow OHLC bar, or no bar at all on a **Line** chart, where the rows take its place. Beside it each price is a row, a line of small type high, its bar as long as its delta and its number written inside it at its left, in the buying colour when more was bought there and the selling colour when more was sold. The footprint's colours are its own: changing them leaves the bars alone, and changing the chart's colours leaves the rows alone. It comes on by itself as you zoom in: once a bar is wide enough for the narrow bar and a row that holds the widest number at the figures' size, about 65 px; with **Numbers** off, once a row has room for a short bar, about 20 px. Zooming back out, it holds on a few pixels past that, so a zoom resting at the edge does not flicker, and then the chart shows its ordinary bars.

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
  scroll 1000 400 -2, nine times, to zoom in until the footprint shows
capture: bin/headless-window cli chart screenshot --output /tmp/footprint.png; magick /tmp/footprint.png -quality 88 site/assets/manual/indicators/footprint.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/footprint.jpg" alt="ES one-minute bars zoomed in, each a narrow candle beside a stack of thin rows of buying minus selling, one a price, each boxed in its colour with its number inside at its left" caption="A footprint on ES 1m, zoomed in, on the synthetic feed." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Automatic rows** | On | Off sets **Row height (ticks)** by hand |
| **Row height (ticks)** | Greyed while automatic, showing the ticks a row holds | 1 to 10000 ticks a row |
| **Shade by delta** | On | Off fills every row alike, at its colour's opacity |
| **Numbers** | On | Off draws the rows alone, with no number in them |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet**, **Monochrome** or **Custom** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Opacity** | 85% | How solid buying and selling are both drawn, 0 to 100%. Shown for every colouring but **Custom** |
| **Buying** | The colour buying was drawn in before **Custom** was chosen | A palette name, or a custom colour, with its own **Opacity**. Shown only for **Custom** |
| **Selling** | The same for selling | The same |

## Rows

With **Automatic rows** on, a row holds 1, 2, 4, 5, 10, 20, 25, 50 or more ticks, the fewest that make it about a line of small type tall, 16 px, at the chart's price scale. The choice is made again only when the price scale changes, and kept while its rows stay between 13 and 40 px, so scrolling and small rescales leave the rows as they are. With it off, a row holds the **Row height (ticks)** set, or a multiple of it when that would make rows under 3 px. Every bar's rows cover the same prices. However far the chart is zoomed in, a row is never drawn taller than a line of small type: it is centred on its prices with a gap of a pixel or two to the next. A row's number is written inside it at its left, even when its bar is shorter than the number. It is left out once the rows are under 11 px tall, and written short past a thousand: **1.2K**, **-35K**, **2.5M**. With **Numbers** off, none is written and the rows' lengths and shades carry the delta alone, which also makes each frame cheaper to draw.

## Shade by delta

With **Shade by delta** on, a row is filled more solidly the bigger its delta, in five steps from faint up to its colour's opacity. The strongest is measured against the biggest delta of any row on screen, so a row's length and its strength say the same thing, and scrolling to a busier stretch makes the rest fainter. Each row keeps a thin outline in its own colour, so a faint row is still seen. In **Monochrome** every row is the one colour, so a strength says only how big a delta is, and the minus sign in a row's number says which way it went.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Volume profile](/manual/indicators/volume/volume-profile/), the same measure over a session
- [Footprint from the CLI](/manual/cli/indicators/order-flow/footprint/)
