---
title: "Cumulative Delta"
description: "The running total of buying at the ask less selling at the bid"
section: charting
parent: indicators/volume
nav_order: 3
---

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

- [Volume](/manual/indicators/volume/)
- [Indicators from the CLI](/manual/cli/#indicators)
