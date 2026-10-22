---
title: "Splitting and arranging charts"
description: "Dividing the window between several charts, and resizing or maximising one of them."
section: charting
nav_order: 18
---

The charts in a chartbook are arranged by splitting. A split divides one chart into two halves, each a chart of its own. Closing a chart gives its space to the chart beside it. A half can be split again.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Majors --symbol SPY --switch
  omacharts chart split horizontal
  omacharts chart split vertical --chart pos:0
  omacharts chart split vertical --chart pos:2
  omacharts chart set --chart pos:N --link pinned        # N = 0..3
  omacharts chart set --chart pos:1 --symbol QQQ
  omacharts chart set --chart pos:2 --symbol GLD
  omacharts chart set --chart pos:3 --symbol TLT
  omacharts chart focus pos:0
capture: bin/headless-window cli chartbook screenshot --output /tmp/grid.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/layouts/two-by-two.jpg" alt="Four daily charts in a two-by-two: SPY top left, QQQ bottom left, GLD top right, TLT bottom right" caption="SPY, QQQ, GLD and TLT in a two-by-two." wide="true" %}

## Splitting a chart

| Key | Does |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>H</kbd> | **Split horizontally**. A copy of the focused chart appears to its right |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>V</kbd> | **Split vertically**. A copy of the focused chart appears below it |

Both are in the [chart menu](/manual/charts/#the-chart-menu) too. The copy has the same settings and takes the focus. Splitting a maximised chart restores it first.

## Closing a chart

<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>X</kbd>, or **Close chart** in the chart menu, closes the focused chart. The chart it was split from or with takes its space, and the focus goes to the first chart of the chartbook. The last chart in a chartbook can't be closed; **Close chart** is in the menu only while there are two or more.

## Focusing a chart

Click a chart, or press <kbd>Alt</kbd> + an arrow to focus the nearest chart in that direction. Nearest is by position on screen. With no chart in that direction, nothing happens. See [The focused chart](/manual/window/#the-focused-chart).

## Resizing

Drag the divider between two charts, or hold <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + an arrow to grow the focused chart in that direction a little at a time. A divider stops short of either end. The key does nothing with a single chart, when no divider runs in that direction, or while a chart is maximised. In the sidebar, <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> switch watchlists instead ([Watchlists](/manual/watchlists/)).

Divider positions are saved with the chartbook, as are the heights of indicator strips dragged within a chart ([Indicators](/manual/indicators/)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Desk --symbol NVDA --switch
  omacharts chart split horizontal
  omacharts chart split vertical --chart pos:1
  omacharts chart set --chart pos:1 --resolution 1h
  omacharts chart set --chart pos:2 --resolution 15m
  omacharts chart focus pos:0
capture: bin/headless-window cli chartbook screenshot --output /tmp/bigleft.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/layouts/big-left.jpg" alt="Three NVDA charts: a daily chart filling the left half, an hourly chart top right and a 15-minute chart bottom right" caption="The daily on the left, the hourly and the 15-minute stacked on the right." wide="true" %}

## Maximising a chart

<kbd>Ctrl</kbd> + <kbd>M</kbd> gives the focused chart the whole window, and the same key puts it back. The maximise corner in a chart's top-right, and **Maximize** / **Restore** in the chart menu, do the same for that chart. Both appear once the chartbook has two charts.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the Desk state above, sidebar open)
  key alt+Right
  key ctrl+m
  move 700 300; move 702 302
capture: bin/headless-window shot /tmp/max.png
crop: 980,0,1580,170
-->
{% include figure.html src="/assets/manual/layouts/maximized-corner.png" alt="A maximised chart's top-right corner, with the maximise arrows pointing inward" caption="The corner of a maximised chart. The arrows point inward." %}

Restoring puts the arrangement back as it was. <kbd>Alt</kbd> + an arrow still moves the focus one chart at a time, and the window shows the chart it reaches. Which chart is maximised is saved with the chartbook.

## See also

- [Arranging charts from the CLI](/manual/cli/#arranging-charts)
