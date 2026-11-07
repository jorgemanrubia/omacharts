---
title: "Volume"
description: "Volume and the volume profile, with every setting."
section: charting
parent: indicators
nav_order: 2
---

The volume indicators show how much traded: Volume bar by bar in a strip under the price, and the volume profile at each price, over the price. Both are drawn as bars and have no thickness or line style.

## Volume

Its own strip. Each bar's volume, in the candles' up or down colour.

| Setting | Default | Allowed |
|---|---|---|
| **Pane height %** | 18% | 5 to 95% |
| **Colouring** | **Same as the chart** | **Vivid**, **Soft**, **Quiet** or **Monochrome**, so the volume can be painted in a colouring the price is not ([Colouring](/manual/charts/#colouring)) |
| **Opacity** | 85% | 20 to 100%. At 100% the bars hide what is behind them |
| **Up** colour | The colouring's up colour | A palette name, or a custom colour |
| **Down** colour | The colouring's down colour | A palette name, or a custom colour |

**Same as the chart** follows the chart's own colouring, whatever it is changed to, and its **Swap up and down colours** with it. **Hollow up bars** does not reach the volume: a volume bar is a solid bar with no body to leave open. Choosing a colouring keeps the volume in it while the price changes: vivid candles over a soft volume, or the other way round. A colouring of its own is painted its own way round, whether or not the chart's up and down are swapped. **Up** and **Down** then take that colouring's colours, and a colour set by hand wins over both. A selected strip is drawn solid whatever its **Opacity**.

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

Drawn over the price. For each reset period, how much volume traded at each price, as a histogram against the left or the right edge of the period, at most 40% of its width. The rows in the **value area**, the band around the busiest price that holds the given share of the period's volume, are drawn stronger. The **point of control**, the busiest row, is a line across the profile.

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Automatic**, **Rows** | Automatic | A number of rows per period, 4 to 800 |
| **Value area %** | 70% | 10 to 100% |
| **Outline** | Off | On for an edge round filled rows. Always on in the Outline style. Off, its colour and thickness are greyed |
| **Outline colour** | The background colour | A palette name, or a custom colour |
| **Outline thickness** | 1 px | 0.5 to 6 px |
| **Background** colour | The next free colour | A palette name, or a custom colour |
| **Point of control** colour | A contrasting colour from the theme | A palette name, or a custom colour |
| **Point of control thickness** | 1 px | 0.5 to 6 px |
| **Style** | Filled | Filled, or Outline: only the edge of the rows, so the candles under it stay clear |
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period |

**Automatic** gives every period rows of the same height, a round multiple of the instrument's price step, about 400 to a typical period. A period too quiet for 40 rows at that height has its rows split by 2, 5 or 10, down to the price step, so it is drawn smooth too. With **Automatic** off, **Rows** is the number of rows each period is divided into.

### Composite profiles

Right-click a profile and choose **Merge with previous profile** to fold it into the one before it: one profile, with one point of control and one value area, over both periods. Merge again to take in another. **Split composite profile**, on a composite, puts its periods back as they were.

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

## See also

- [Indicators](/manual/indicators/)
- [Indicators from the CLI](/manual/cli/#indicators)
