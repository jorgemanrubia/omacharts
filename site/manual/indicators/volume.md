---
title: "Volume"
description: "Volume, the volume profile and the cumulative delta, with every setting."
section: charting
parent: indicators
nav_order: 2
---

The volume indicators show how much traded: Volume bar by bar in a strip under the price, the volume profile at each price, over the price, and the cumulative delta, who was buying and who was selling, in a strip of its own. Volume and the volume profile are drawn as bars and have no thickness or line style.

## Volume

Its own strip. Each bar's volume, in the candles' up or down colour.

| Setting | Default | Allowed |
|---|---|---|
| **Pane height %** | 18% | 5 to 95% |
| **Colouring** | **Default** | **Vivid**, **Soft** or **Quiet**, so the volume can be painted in a colouring the price is not ([Colouring](/manual/charts/#colouring)) |
| **Opacity** | 85% | 20 to 100%. At 100% the bars hide what is behind them |
| **Up** colour | The colouring's up colour | A palette name, or a custom colour |
| **Down** colour | The colouring's down colour | A palette name, or a custom colour |

**Default** is the chart's own colouring, and follows it whatever it is changed to, and its **Swap up and down colours** with it. **Hollow up bars** does not reach the volume: a volume bar is a solid bar with no body to leave open. Choosing a colouring keeps the volume in it while the price changes: vivid candles over a soft volume, or the other way round. Monochrome is not offered, because it paints rising and falling bars alike; a strip already painted Monochrome keeps it, and it is listed last. A colouring of its own is painted its own way round, whether or not the chart's up and down are swapped. **Up** and **Down** then take that colouring's colours, and a colour set by hand wins over both. A selected strip is drawn solid whatever its **Opacity**.

The **Default** [chart template](/manual/chart-templates/) holds Volume, with [Session Times](/manual/indicators/session-times/).

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 1D --template none
  omacharts chart indicator add volume
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vol.png
crop: 0,812,1592,380 (then -quality 88 to site/assets/manual/indicators/volume.jpg)
-->
{% include figure.html src="/assets/manual/indicators/volume.jpg" alt="The volume strip under a SPY daily chart, bars in the candles' green and red" caption="The Volume strip under SPY daily." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart on the Default template, sidebar hidden)
  key ctrl+shift+s; click 700 72 (Indicators); click 600 282 (Volume); click 800 383 (the Colouring row)
capture: bin/headless-window shot /tmp/volume-colouring.png
crop: 700,84,1160,1276
-->
{% include figure.html src="/assets/manual/indicators/volume-colouring.png" alt="The Volume indicator's settings with its Colouring list open: Same as the chart, Vivid, Soft, Quiet, Monochrome" caption="A volume strip's own Colouring." %}

<!-- regenerate:
as above, with the Colouring row closed
crop: 762,640,1036,518 (the Colours group)
-->
{% include figure.html src="/assets/manual/indicators/volume-opacity.png" alt="The Volume indicator's Colours group: Colouring, then the Opacity slider at 85%, then Up and Down" caption="Opacity, under the strip's Colouring." %}

## Volume Profile

Drawn over the price. For each reset period, how much volume traded at each price, as a histogram against the left or the right edge of the period, as much of its width as **Width** says. Aligned right, the session still forming is drawn in the empty space past the last bar, against the price axis, when there is room for it there; scrolled back, or with no room open, it ends where its bars do like the rest. That space is widened to fit it, and narrowed again when no profile is aligned right. It is a share of the chart's width, not a number of bars, so zooming in and out moves the bars and leaves it, and the profiles in it, the same size. Scrolling forward stops at it, so coming back to the latest bar finds it as wide as before. Drag part of it shut and it stays as you left it, at the same size on screen as you zoom, until you scroll back to the latest bar or reset the view. Several profiles aligned to the same edge sit side by side, in the order the legend lists them, each as wide as its **Width** says, and squeezed alike when they would not fit; past the last bar they take at most 60% of the chart between them. The rows in the **value area**, the band around the busiest price that holds the given share of the period's volume, are drawn stronger, unless **Highlight value area** is off, and in a colour of their own if **Value area colour** gives them one. The **point of control**, the busiest row, is a line across the profile.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Only show in last period** | Off | On draws the newest period's profile alone: the same profile, with the same rows, as among the others, and past the last bar when aligned right |
| **Calculation method** | Total volume | Total volume, or Ask volume - bid volume |
| **Colouring** | **Default** | **Vivid**, **Soft** or **Quiet**. Only for Ask volume - bid volume |
| **Automatic**, **Rows** | Automatic | A number of rows per period, 4 to 800 |
| **Value area %** | 70% | 10 to 100%. Greyed for Ask volume - bid volume |
| **Highlight value area** | On | Off draws every row alike. Greyed for Ask volume - bid volume |
| **Value area colour** | The background colour | A palette name, or a custom colour, for the value area while it is highlighted. Greyed while it is not, and for Ask volume - bid volume |
| **Outline** | Off | On for an edge round filled rows. Always on, and greyed, in the Outline style. Its colour, style and thickness show while it is on |
| **Outline colour** | The background colour | A palette name, or a custom colour |
| **Outline style** | Solid | Solid, Dashed, Dotted |
| **Outline thickness** | 1 px | 0.5 to 6 px |
| **Background** colour | The next free colour | A palette name, or a custom colour |
| **Transparency** | 70% | 0 to 100%: how much of the chart shows through the rows and the outline. At 0% the value area is the background colour, solid, and at 100% the profile is gone |
| **Show point of control** | On | Off leaves the profile without its point of control line. Its colour, style and thickness show while it is on |
| **Point of control** colour | A contrasting colour from the theme | A palette name, or a custom colour |
| **Point of control style** | Solid | Solid, Dashed, Dotted |
| **Point of control thickness** | 1 px | 0.5 to 6 px |
| **Style** | Filled | Filled, or Outline: only the edge of the rows, so the candles under it stay clear |
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period |
| **Width** | 40% | 10 to 100%: how far the longest row reaches, of the period's width, or of the empty space past the last bar for the session drawn there |

**Ask volume - bid volume** draws each row as what was bought at the ask less what was sold at the bid at its prices: rows where buyers took more reach out in the up colour of its **Colouring**, rows where sellers took more in the down colour. **Default** is the colouring set in Preferences for new charts ([Defaults for new charts](/manual/chart-settings/#defaults-for-new-charts)), and changes when that does. Pick **Vivid**, **Soft** or **Quiet** to keep this profile in that colouring. Either way it is swapped when Preferences' **Swap up and down colours** is on. Monochrome is not offered, because it paints buying and selling alike. The point of control is the row furthest either way, and there is no value area. Which side a trade took is not in a bar, so this needs a data feed with volume at price: on any other it is one line saying so, and a period from before the feed's history begins is not drawn. The legend reads **VP Δ**.

**Automatic**, on a data feed with volume at price, gives each price step a row of its own. A period that would need more than 800 has its rows made 2, 5 or 10 price steps tall, and the periods beside it keep one a step. On any other feed it gives every period rows of the same height, a round multiple of the instrument's price step, about 600 to a typical period, and a period too quiet for 60 rows at that height has its rows split by 2, 5 or 10, down to the price step, so it is drawn smooth too. With **Automatic** off, **Rows** is the number of rows each period is divided into, on either feed.

### Composite profiles

Right-click a profile and choose **Merge with previous profile** to fold it into the one before it: one profile, with one point of control and one value area, over both periods. Merge again to take in another. With **Only show in last period** on there is no profile before it drawn to merge into, so it is not offered. **Split composite profile**, on a composite, puts its periods back as they were.

A composite belongs to the chart it was made on, to the symbol it is showing and to the period it was made under, even when the profile is a template's, and it is kept with the chart. Changing **Reset every**, or a resolution that changes the period in use, draws the new periods without it; going back brings it back. For a profile over any span you choose, use the [volume profile drawing](/manual/drawings/#volume-profiles).

<!-- regenerate:
theme: tokyo-night
window: 1600x1200 @2, scratchpad kit
state:
  key ctrl+shift+b
  omacharts chart set --symbol SPY --resolution 5m --session regular --template none
  omacharts chart indicator add volume_profile
  move 795 595
capture: bin/headless-window cli chart screenshot --output /tmp/vp.png; magick /tmp/vp.png -quality 88 site/assets/manual/indicators/volume-profile.jpg
crop: none
-->
{% include figure.html src="/assets/manual/indicators/volume-profile.jpg" alt="SPY 5m with a volume profile at the start of each session" caption="SPY 5m with a session volume profile." %}

## Cumulative Delta

Its own strip. The running total of what was bought at the ask less what was sold at the bid, the same measure as a volume profile's **Ask volume - bid volume**, added up bar by bar and started again from nought at each reset period. Above the zero line buyers have taken more since the period began; below it, sellers have. The legend reads **CVD · Session**.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)), or Never for one running total over the whole chart |
| **Style** | Candles | Candles, or Line: the running total at each bar's close |
| **Colouring** | **Default** | **Vivid**, **Soft** or **Quiet** |
| **Pane height %** | 16% | 5 to 95% |
| **Colour**, **Thickness**, **Line style** | The next free colour, 1 px, solid | Only for the Line style |

As candles, each bar opens where the total stood at the previous close and closes where it stands at its own. Its wicks are the highest and lowest the total reached inside the bar, minute by minute, so a 1-minute chart and a daily or longer one have no wicks. A bar that added to the total is drawn in the up colour of its **Colouring**, one that took from it in the down colour. **Default** is the colouring set in Preferences for new charts, as for a delta volume profile, and is swapped when Preferences' **Swap up and down colours** is on.

On a data feed with volume at price the total is measured from each trade's side. On any other feed, Yahoo for one, and for bars from before the feed's history begins, it is estimated from the bars instead: a bar that closed above its open counts all its volume as bought, one that closed below as sold, and one that closed where it opened goes by its close against the previous bar's close, or counts as nought if that is unchanged too. The estimate uses the chart's own bars, so it is coarser on a coarser chart. While any of the bars on the chart is estimated, the legend reads **CVD ≈** and the settings say so in one line.

## See also

- [Indicators](/manual/indicators/)
- [Indicators from the CLI](/manual/cli/#indicators)
