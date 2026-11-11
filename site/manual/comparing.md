---
title: "Comparing markets"
description: "Other markets drawn on a chart as lines, each in percent from the same bar."
section: charting
nav_order: 14.5
---

Comparing draws other markets on a chart as lines, each rebased to 0% at the same bar, so you can read how far each one has come since then. While a chart compares anything, its price axis is in percent and every line, the chart's own bars included, starts from 0% at that bar.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Compare --symbol SPY --switch
  omacharts chart set --resolution 1D --session regular
  omacharts chart compare add GC NQ BTC-USD
  omacharts chart range 1y
  bin/headless-window move 1100 500   (pointer on the sidebar, so the legend reads the last bar)
capture: bin/headless-window cli chart screenshot --output site/assets/manual/comparing/compared.png
crop: none
-->
{% include figure.html src="/assets/manual/comparing/compared.png" alt="SPY daily with gold, Nasdaq futures and bitcoin drawn over it as coloured lines, the price axis in percent and each line's change at the right edge" caption="SPY compared with GC, NQ and BTC." %}

## Adding a market

Right-click the chart and choose **Compare with…**, then pick a symbol in the search. The line is drawn at once from what is already on this machine, and its row in the legend stays dimmed until the market's feed has answered. A market that fails to load keeps its row, with the reason in place of its change.

Each market is drawn in the next colour of the theme's palette that the chart's indicators have not taken. Its symbol and its change at the last bar on screen are at the right edge, and a market can be compared with any number of charts. A chart cannot be compared with its own symbol.

## Taking a market off

Hover a market's row in the legend and click its **×**, or remove it in [Chart settings](#in-chart-settings). With the last market gone, the chart goes back to prices and its own bars.

## Where the lines are measured from

By default every line is 0% at the first bar on screen. Scrolling and zooming move that bar, so the lines always show performance over the stretch you are looking at.

To hold the base still, right-click the bar to measure from and choose **Compare from here**; the menu offers it only over a bar. A dotted line marks that bar, and the legend ends with the day it is pinned to, such as **From 8 Jan 2026**. Its **×**, or **Compare from left edge** in the same menu, lets the base follow the screen again. A market that had not started trading by the base bar starts at 0% where its data begins.

## The legend and the crosshair

Under the chart's header, which names the chart's own symbol, the legend lists each market in the order you added it, with its change. Hover the chart and the legend reads the bar under the pointer, its rows staying where they are, with a dot on each line at that bar. With the pointer off the chart, it reads the last bar on screen. The crosshair's price label is in percent.

## Markets with different hours

Each market is read at the chart's own resolution and laid on the chart's own bars. A bar of the chart takes each market's last close at or before it, so a market that was shut, such as a stock over a weekend on a bitcoin chart, is drawn flat until it trades again. Daily and longer bars are matched by their day.

## Lines only

**Lines only** draws the chart's own symbol as a line like the others instead of its bars, as the **Line** style does ([Bars](/manual/charts/#style)), while keeping the chart's own style for when it compares nothing. It is in the chart menu and in Chart settings while the chart compares something.

<!-- regenerate:
as the figure above, then
  omacharts chart compare set --lines-only on
capture: bin/headless-window cli chart screenshot --output site/assets/manual/comparing/lines-only.png
-->
{% include figure.html src="/assets/manual/comparing/lines-only.png" alt="The same comparison with SPY drawn as a line instead of candles" caption="Lines only." %}

## In Chart settings

The **Compare** group on the Chart page of [Chart settings](/manual/chart-settings/) lists the markets.

| Row | Does |
|---|---|
| Each market | Its symbol and name. The swatch changes its colour; the bin takes it off |
| **Compare with…** | Closes the dialog and opens the search to add a market |
| **Lines only** | Draws the chart's own symbol as a line. Only there while the chart compares something |
| **Measured from** | **Left edge**, or the day the base is pinned to. A pinned base has a **Left edge** button that lets it follow the screen again |

## Saving comparisons

What a chart compares, its base and Lines only are saved with the chart and come back after a restart. A split copies them to the new chart. A [chartbook template](/manual/chartbook-templates/) saves them with each chart's position, and chartbooks on it take them. A [chart template](/manual/chart-templates/) holds indicators only, and does not.

## See also

- [Comparing markets with the CLI](/manual/cli/#comparing-markets)
- [The chart menu](/manual/charts/#the-chart-menu)
