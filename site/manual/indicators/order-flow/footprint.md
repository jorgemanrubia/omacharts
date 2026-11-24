---
title: "Footprint"
description: "Each bar's delta at every price, drawn beside a narrow bar once the bars are wide enough"
section: charting
parent: indicators/order-flow
nav_order: 3
---

Each bar's volume at the ask less its volume at the bid, at every price it traded, drawn inside the bar. The chart's own bar narrows to the bar's left edge, in the chart's style and colours: a narrow candle, filled or hollow as the chart's are, a narrow OHLC bar, or no bar at all on a **Line** chart, where the rows take its place. Beside it each price is a row, the rows stacked with no gap between them, each one's bar as long as its delta and its number written inside it at its left, in the buying colour when more was bought there and the selling colour when more was sold. The footprint's colours are its own: changing them leaves the bars alone, and changing the chart's colours leaves the rows alone. It comes on by itself as you zoom in: once a bar is wide enough for the narrow bar and a row that holds the widest number at the figures' size, about 65 px; with **Numbers** off, once a row has room for a short bar, about 20 px. Zooming back out, it holds on a few pixels past that, so a zoom resting at the edge does not flicker, and then the chart shows its ordinary bars.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the candles, from a bar width | **Footprint** | `footprint` | `footprint` |

A chart has one footprint. Picking **Footprint** again in the indicator picker opens the settings of the one already there, and pasting a footprint onto a chart that has one gives that footprint the pasted settings.

<!-- regenerate:
theme: midnight, the fallback when the sandboxed HOME has no Omarchy theme
window: 1400x900 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
state:
  key ctrl+comma; click 267 252 (Data); click 939 222 (Sign in, Fake Rich Feed)
  click 643 419; type 12345; click 606 473; type demo; click 767 542; click 1166 91; key Escape
  omacharts chartbook create "Order flow" --symbol ES --switch; omacharts chartbook sidebar off
  omacharts chart set --resolution 1m --feed fake --auto-scale on
  omacharts chart indicator add footprint
  scroll 1000 400 -2, nine times, to zoom in until the footprint shows
capture: bin/headless-window cli chart screenshot --output /tmp/footprint.png; magick /tmp/footprint.png -quality 88 site/assets/manual/indicators/footprint.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/footprint.jpg" alt="ES one-minute bars zoomed in, each a narrow candle beside a stack of touching rows of buying minus selling, one a price, each outlined in its colour with its number inside at its left" caption="A footprint on ES 1m, zoomed in, on the synthetic feed." %}

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Automatic rows** | On | Off sets **Row height (ticks)** by hand |
| **Row height (ticks)** | Greyed while automatic, showing the ticks a row holds | 1 to 10000 ticks a row |
| **Size against** | Visible bars | **Visible bars**: the biggest row on screen, so rows change length as the chart scrolls. **Session**: the session's. **RTH or ETH**: the session's regular or extended hours', each against its own ([Size against](#size-against)) |
| **Shade by delta** | On | Off fills every row alike, at its colour's opacity |
| **Numbers** | On | Off draws the rows alone, with no number in them |
| **Volume profile** | Off | On draws each bar's volume profile at its left ([Volume profile](#volume-profile)) |
| **Profile colour** | The theme's **Blue**, at 55% | A palette name, or a custom colour, with its own **Opacity**. Shown only with **Volume profile** on |
| **Profile numbers** | On | Off draws the profile's rows alone. Shown only with **Volume profile** on |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet**, **Monochrome** or **Custom** ([Colours](/manual/indicators/order-flow/#colours)) |
| **Opacity** | 85% | How solid buying and selling are both drawn, 0 to 100%. Shown for every colouring but **Custom** |
| **Buying** | The colour buying was drawn in before **Custom** was chosen | A palette name, or a custom colour, with its own **Opacity**. Shown only for **Custom** |
| **Selling** | The same for selling | The same |

## Rows

With **Automatic rows** on, a row holds 1, 2, 4, 5, 10, 20, 25, 50 or more ticks, the fewest that make it about a line of small type tall, 16 px, at the chart's price scale. The choice is made again only when the price scale changes, and kept while its rows stay between 13 and 40 px, so scrolling and small rescales leave the rows as they are. With it off, a row holds the **Row height (ticks)** set, or a multiple of it when that would make rows under 3 px. Every bar's rows cover the same prices. A row runs from the price halfway to the row below to the price halfway to the row above, so a bar's rows touch and the bar reads as one stacked block, and zoomed in a row grows with its prices. A row's number is written inside it at its left, even when its bar is shorter than the number. It is left out once the rows are under 11 px tall, and written short past a thousand: **1.2K**, **-35K**, **2.5M**. With **Numbers** off, none is written and the rows' lengths and shades carry the delta alone, which also makes each frame cheaper to draw.

## Volume profile

With **Volume profile** on, each bar also shows everything that traded at each of its rows, sides or not: at_bid plus at_ask, and any volume whose side the feed did not say. The profile takes the left of the bar's column, each row ending against the narrow bar and as long as its total, and the narrow bar and the delta rows move right to make room, the two sides sharing the column half and half. Its rows are the delta rows' own: the same ticks to a row, the same height and grid, the same outline, and with **Shade by delta** on the same five strengths. A row's length and strength are measured against the most traded row of the same bars a delta row's are measured against ([Size against](#size-against)): on screen, unless set otherwise. Its total is written inside it, against the narrow bar, in the type the deltas are written in and written short the same way, dark on a row solid enough to need it. Its colour says nothing of up or down: one swatch, the theme's **Blue** unless another is picked, following the theme as it changes, at 55% at its strongest so it stays quieter than the deltas.

A footprint with its profile needs room for both sides, so it comes on at about twice the bar width: about 130 px with both sides' numbers, less with either turned off. A chart with a footprint on can be zoomed in to four bars across, rather than twelve, to give it that room. With **Volume profile** off, nothing of it is drawn or measured and the footprint is exactly as it was.

<!-- regenerate:
as the footprint picture above, then:
  omacharts chart indicator set footprint --profile on
capture: bin/headless-window cli chart screenshot --output /tmp/footprint-profile.png; magick /tmp/footprint-profile.png -quality 88 site/assets/manual/indicators/footprint-profile.jpg
then:
  omacharts chart indicator set footprint --numbers off --profile-numbers off
capture: bin/headless-window cli chart screenshot --output /tmp/footprint-profile-numbers-off.png; magick /tmp/footprint-profile-numbers-off.png -quality 88 site/assets/manual/indicators/footprint-profile-numbers-off.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/footprint-profile.jpg" alt="ES one-minute footprint bars, each with blue rows of total volume at its left, the narrow candle in the middle and the delta rows at its right, every row with its number" caption="The footprint with its volume profile, on ES 1m, on the synthetic feed." %}

{% include figure.html src="/assets/manual/indicators/footprint-profile-numbers-off.jpg" alt="The same footprint bars with their volume profiles, the rows drawn without numbers" caption="The same, with Numbers and Profile numbers off." %}

## Shade by delta

With **Shade by delta** on, a row is filled more solidly the bigger its delta, in five steps from faint up to its colour's opacity. The strongest is measured against the biggest delta the row's length is measured against ([Size against](#size-against)), so a row's length and its strength say the same thing. Against the rows on screen, the default, scrolling to a busier stretch makes the rest fainter. Each row keeps a thin outline in its own colour, so a faint row is still seen. Where two rows meet, the line between them is drawn once. In **Monochrome** every row is the one colour, so a strength says only how big a delta is, and the minus sign in a row's number says which way it went.

## Size against

**Size against** is what a row's length and strength are measured against, with the same choices as a large order's bubble: the biggest row on screen counts as the longest row and the strongest shade.

- **Visible bars**, the default: the biggest delta of any row on screen. Rows change length as the chart scrolls to a busier or a quieter stretch.
- **Session**: the biggest of the bar's session, on screen or not. A row keeps its length as the chart scrolls, and a quiet session's rows stay short beside a busy one's.
- **RTH or ETH**: the biggest of the bar's session's regular hours, or of its extended hours, whichever the bar is in. Overnight rows are measured against the overnight's, so they are not all drowned by the open. On an instrument with no regular hours it is the same as **Session**.

The volume profile's rows are measured the same way, against the most traded row of the same bars. A bar of a day or longer is a session of its own. A session's biggest is read once, when the footprint is first drawn over it, and kept; only the newest bars are added again as they trade.

## See also

- [Order flow](/manual/indicators/order-flow/)
- [Volume profile](/manual/indicators/volume/volume-profile/), the same measure over a session
- [Footprint from the CLI](/manual/cli/indicators/order-flow/footprint/)
