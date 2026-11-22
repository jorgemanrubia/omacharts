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

## Alone or beside a market profile

A volume profile can be shown on its own or next to a [market profile](/manual/indicators/volume/market-profile/), which is a study of its own too. With a market profile of the same **Reset every** on the chart, each session's two are drawn inside it, each from its own **Alignment**: side by side when they are set alike, back to back when one is Left and the other Right. They merge and split together ([Beside a volume profile](/manual/indicators/volume/market-profile/#beside-a-volume-profile)). With different periods, each one keeps to itself.

From the [CLI](/manual/cli/indicators/volume/volume-profile/):

```
# A volume profile on its own
omacharts chart indicator add volume_profile --anchor session

# Beside a market profile, both per session: each session's two side by side
omacharts chart indicator add volume_profile --anchor session --profile-width 0.3
omacharts chart indicator add market_profile --anchor session --profile-width 0.4

# Merge a session into the one before it, and split it again; a pair does both
omacharts chart profile merge --at 2026-11-18
omacharts chart profile split --at 2026-11-18
```


## Settings

The settings are in four groups, in the order below. A switch's rows show while it is on. A [market profile](/manual/indicators/volume/market-profile/#settings) has the same groups, with the same names in the same places.

### Parameters

| Setting | Default | Allowed |
|---|---|---|
| **Reset every** | Session | Session, Week, Month, Quarter, Year ([Reset periods](/manual/indicators/#reset-periods)) |
| **Last period only** | Off | On draws the newest period's profile alone: the same profile, with the same rows, as among the others, and past the last bar when aligned right |
| **Calculation method** | Total volume | Total volume, or [Ask volume - bid volume](#ask-volume---bid-volume) |
| **Colouring** | **Default** | **Vivid**, **Soft**, **Quiet**, **Monochrome** or **Custom**. Only for Ask volume - bid volume |
| **Buying**, **Selling** | The colours drawn before **Custom** was chosen | A palette name, or a custom colour, for rows where more was bought, and where more was sold. Shown only for **Custom**. |
| **Automatic rows**, **Rows** | On | A number of [rows](#rows) per period, 4 to 800, with **Automatic rows** off |
| **Value area %** | 70% | 10 to 100%. Greyed for Ask volume - bid volume |

### Style

A colour and its opacity share one row: the colour's name, the swatch, then **Opacity**, a slider and a figure for how solid the colour is drawn, 100% solid and 0% invisible. **Default**, first in the swatch's palette, goes back to the colour it has unless one is picked. The command line's `--*-transparency` flags set the same thing the other way up, as how much of the chart shows through.

| Setting | Default | Allowed |
|---|---|---|
| **Display** | Filled | Filled, or Outline: only the edge of the rows, so the candles under it stay clear |
| **Alignment** | Left | Left, or Right: the rows grow back from the end of the period ([Aligned right](#aligned-right)). Beside a market profile with the same period each keeps its own ([Beside a volume profile](/manual/indicators/volume/market-profile/#beside-a-volume-profile)) |
| **Width %** | 40% | 10 to 100%: how far the longest row reaches, of the period's width, or of the empty space past the last bar for the session drawn there |
| **Colour** | The next free colour, at 30% opacity | A palette name, or a custom colour, greyed for Ask volume - bid volume, whose rows are in its **Colouring**. Its **Opacity** is how solid the rows are drawn, 0 to 100%: at 100% they are solid, and at 0% the profile is gone. The value area's two opacities follow it until they are set |
| **Outline** | Off | On for an edge round filled rows. Always on, and greyed, while **Display** is Outline |
| **Outline colour** | The profile's **Colour**, at 45% opacity round filled rows; in the Outline display its opacity follows the rows' | A palette name, or a custom colour. Its **Opacity** is how solid the edge is drawn, 0 to 100%; once set, it stays put when the rows' moves |
| **Outline thickness** | 1 px | 0.5 to 12 px |
| **Outline style** | Solid | Solid, Dashed, Dotted |

### Value area

| Setting | Default | Allowed |
|---|---|---|
| **Highlight** | On | Off draws every row alike. The group is greyed for Ask volume - bid volume, which has no value area |
| **Colour** | The profile's **Colour**, its opacity following the Style group's | A palette name, or a custom colour, for the value area. Its **Opacity** is how solid the value area's rows are drawn, 0 to 100% |
| **Outside rows opacity %** | A little under half as strong as the value area | How solid the rows above and below the value area are drawn, 0 to 100% |

### Point of control

| Setting | Default | Allowed |
|---|---|---|
| **Show** | On | Off leaves the profile without its point of control line |
| **Colour** | A contrasting colour from the theme, at 90% opacity | A palette name, or a custom colour. Its **Opacity** is how solid the line is drawn, 0 to 100% |
| **Thickness** | 1 px | 0.5 to 12 px |
| **Line style** | Solid | Solid, Dashed, Dotted |

Several profiles aligned to the same edge sit side by side, in the order the legend lists them, each as wide as its **Width %** says, and squeezed alike when they would not fit. A market profile among them sits beyond the volume profiles.

## Ask volume - bid volume

Each row is what was bought at the ask less what was sold at the bid at its prices: rows where buyers took more reach out in the up colour of its **Colouring**, rows where sellers took more in the down colour. The point of control is the row furthest either way, and there is no value area. The legend reads **VP Δ**.

**Default** paints it as the chart's bars. Pick **Vivid**, **Soft**, **Quiet** or **Monochrome** to keep this profile in that colouring, or **Custom** to pick its two colours yourself, starting from the ones it was drawn in: the same list as every other study's ([Colours](/manual/indicators/order-flow/#colours)). **Monochrome** paints buying and selling in one colour, the ink of the chart's Monochrome candles, so colour no longer tells one from the other, as on the candles.

Which side a trade took is not in a bar, so this needs a data feed with volume at price: on any other it is one line saying so, and a period from before the feed's history begins is not drawn.

## Rows

**Automatic rows**, on a data feed with volume at price, gives each price step a row of its own. A period that would need more than 800 has its rows made 2, 5 or 10 price steps tall, and the periods beside it keep one a step.

On any other feed it gives every period rows of the same height, a round multiple of the instrument's price step, about 600 to a typical period. A period too quiet for 60 rows at that height has its rows split by 2, 5 or 10, down to the price step, so it is drawn smooth too.

With **Automatic rows** off, **Rows** is the number of rows each period is divided into, on either feed. A [volume profile drawing](/manual/drawings/volume-profile/) has the same two rows and divides its span the same way, from the same volume at price, or from its bars on any other feed. It measures Ask volume - bid volume too.

## Aligned right

Aligned right, the session still forming is drawn in the empty space past the last bar, against the price axis, when there is room for it there. Scrolled back, or with no room open, it ends where its bars do like the rest.

That space is widened to fit it, and narrowed again when no profile is aligned right. It is a share of the chart's width, not a number of bars, so zooming in and out moves the bars and leaves it, and the profiles in it, the same size. Scrolling forward stops at it, so coming back to the latest bar finds it as wide as before. Drag part of it shut and it stays as you left it, at the same size on screen as you zoom, until you scroll back to the latest bar or reset the view. Past the last bar, the profiles take at most 60% of the chart between them.

## Composite profiles

Right-click a profile and choose **Merge with previous profile** to fold it into the one before it: one profile, with one point of control and one value area, over both periods. Merge again to take in another. With **Last period only** on there is no profile before it drawn to merge into, so it is not offered. **Split composite profile**, on a composite, puts its periods back as they were. Paired with a [market profile](/manual/indicators/volume/market-profile/#composite-profiles) of the same period, merging or splitting either does both.

A composite belongs to the chart it was made on, to the symbol it is showing and to the period it was made under, even when the profile is a template's, and it is kept with the chart. Changing **Reset every**, or a resolution that changes the period in use, draws the new periods without it; going back brings it back. For a profile over any span you choose, use the [volume profile drawing](/manual/drawings/volume-profile/).

## See also

- [Volume](/manual/indicators/volume/)
- [Adding an indicator](/manual/indicators/#adding-an-indicator)
- [The Volume Profile from the CLI](/manual/cli/indicators/volume/volume-profile/)
