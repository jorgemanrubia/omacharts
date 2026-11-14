---
title: "Volume Profile"
description: "How much volume traded at each price, period by period"
section: charting
parent: indicators/volume
nav_order: 2
---

For each reset period, how much volume traded at each price, as a histogram against the left or the right edge of the period. The rows in the **value area**, the band around the busiest price that holds the given share of the period's volume, are drawn stronger. The **point of control**, the busiest row, is a line across the profile.

| Drawn | Legend | Type to add | CLI |
|---|---|---|---|
| Over the price | **VP · Session** | `vp` | `volume_profile` |

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

## Settings

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Only show in last period** | Off | On draws the newest period's profile alone: the same profile, with the same rows, as among the others, and past the last bar when aligned right |
| **Calculation method** | Total volume | Total volume, or [Ask volume - bid volume](#ask-volume---bid-volume) |
| **Colouring** | **Default** | **Vivid**, **Soft** or **Quiet**. Only for Ask volume - bid volume |
| **Automatic**, **Rows** | Automatic | A number of [rows](#rows) per period, 4 to 800 |
| **Value area %** | 70% | 10 to 100%. Greyed for Ask volume - bid volume |
| **Highlight value area** | On | Off draws every row alike. Greyed for Ask volume - bid volume |
| **Value area colour** | The background colour | A palette name, or a custom colour, for the value area while it is highlighted. Greyed while it is not, and for Ask volume - bid volume |
| **Outline** | Off | On for an edge round filled rows. Always on, and greyed, in the Outline style. Its colour, thickness and style show while it is on |
| **Outline colour** | The background colour | A palette name, or a custom colour |
| **Outline thickness** | 1 px | 0.5 to 12 px |
| **Outline style** | Solid | Solid, Dashed, Dotted |
| **Background** colour | The next free colour | A palette name, or a custom colour |
| **Transparency %** | 70% | 0 to 100%: how much of the chart shows through the rows and the outline. At 0% the value area is the background colour, solid, and at 100% the profile is gone |
| **Show point of control** | On | Off leaves the profile without its point of control line. Its colour, thickness and style show while it is on |
| **Point of control colour** | A contrasting colour from the theme | A palette name, or a custom colour |
| **Point of control thickness** | 1 px | 0.5 to 12 px |
| **Point of control style** | Solid | Solid, Dashed, Dotted |
| **Style** | Filled | Filled, or Outline: only the edge of the rows, so the candles under it stay clear |
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period ([Aligned right](#aligned-right)) |
| **Width %** | 40% | 10 to 100%: how far the longest row reaches, of the period's width, or of the empty space past the last bar for the session drawn there |

Several profiles aligned to the same edge sit side by side, in the order the legend lists them, each as wide as its **Width %** says, and squeezed alike when they would not fit.

## Ask volume - bid volume

Each row is what was bought at the ask less what was sold at the bid at its prices: rows where buyers took more reach out in the up colour of its **Colouring**, rows where sellers took more in the down colour. The point of control is the row furthest either way, and there is no value area. The legend reads **VP Δ**.

**Default** is the colouring set in Preferences for new charts ([Defaults for new charts](/manual/chart-settings/#defaults-for-new-charts)), and changes when that does. Pick **Vivid**, **Soft** or **Quiet** to keep this profile in that colouring. Either way it is swapped when Preferences' **Swap up and down colours** is on. Monochrome is not offered, because it paints buying and selling alike.

Which side a trade took is not in a bar, so this needs a data feed with volume at price: on any other it is one line saying so, and a period from before the feed's history begins is not drawn.

## Rows

**Automatic**, on a data feed with volume at price, gives each price step a row of its own. A period that would need more than 800 has its rows made 2, 5 or 10 price steps tall, and the periods beside it keep one a step.

On any other feed it gives every period rows of the same height, a round multiple of the instrument's price step, about 600 to a typical period. A period too quiet for 60 rows at that height has its rows split by 2, 5 or 10, down to the price step, so it is drawn smooth too.

With **Automatic** off, **Rows** is the number of rows each period is divided into, on either feed. A [volume profile drawing](/manual/drawings/volume-profile/) has the same two rows and divides its span the same way, worked out from its bars alone.

## Aligned right

Aligned right, the session still forming is drawn in the empty space past the last bar, against the price axis, when there is room for it there. Scrolled back, or with no room open, it ends where its bars do like the rest.

That space is widened to fit it, and narrowed again when no profile is aligned right. It is a share of the chart's width, not a number of bars, so zooming in and out moves the bars and leaves it, and the profiles in it, the same size. Scrolling forward stops at it, so coming back to the latest bar finds it as wide as before. Drag part of it shut and it stays as you left it, at the same size on screen as you zoom, until you scroll back to the latest bar or reset the view. Past the last bar, the profiles take at most 60% of the chart between them.

## Composite profiles

Right-click a profile and choose **Merge with previous profile** to fold it into the one before it: one profile, with one point of control and one value area, over both periods. Merge again to take in another. With **Only show in last period** on there is no profile before it drawn to merge into, so it is not offered. **Split composite profile**, on a composite, puts its periods back as they were.

A composite belongs to the chart it was made on, to the symbol it is showing and to the period it was made under, even when the profile is a template's, and it is kept with the chart. Changing **Reset every**, or a resolution that changes the period in use, draws the new periods without it; going back brings it back. For a profile over any span you choose, use the [volume profile drawing](/manual/drawings/volume-profile/).

## See also

- [Volume](/manual/indicators/volume/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [Indicators from the CLI](/manual/cli/#indicators)
