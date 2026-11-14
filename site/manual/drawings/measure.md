---
title: "Measure"
description: "A ruler over a move: its change, its bars and its time"
section: charting
parent: drawings
nav_order: 1
hotkey: "Alt + M"
---

A ruler over a move between two points: its percentage change, its price change, its bars and its time.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>M</kbd>, or **Measure** just below the pointer at the top of the tools | Click the start, move the mouse, then click the end | [`chart measure`](/manual/cli/#measuring-a-move) |

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window (HOME sandboxed and the theme seeded, as for the tools above)
state:
  omacharts config set theme omarchy:tokyo-night   (before the window opens)
  omacharts chart set --symbol NVDA --resolution 1D --template none
  key ctrl+shift+d
  key alt+m
  click 488 581
  move 668 160
  click 668 160
  move 1275 795
capture: bin/headless-window shot /tmp/ruler.png; magick /tmp/ruler.png -crop 1950x1600+0+0 +repage -resize 1600x site/assets/manual/drawings/ruler.png
crop: 0,0,1950,1600
-->
{% include figure.html src="/assets/manual/drawings/ruler.png" alt="A rise on NVDA measured in green, labelled +23.65% +44.95 over 33 bars, 6w 6d" caption="A move measured from the August low, left fixed by the second click." %}

A rise is drawn in your bar colours' up colour and a fall in their down colour, with an arrow pointing each way the move went. The label shows the percentage change in bold, then the price change, and under them the bar distance and elapsed time: `+0.53%  +4.01` over `71 bars, 5h 55m`.

The measurement stays fixed after the second click. Click again to start another, or press <kbd>Esc</kbd> to clear it and return to the pointer. It is not saved or shared with other charts, and choosing another tool or changing the symbol or resolution clears it.

## What the label counts

| Part | Is |
|---|---|
| Price change | Written with as many decimals as the price axis gives that price |
| Percentage | Left out for a move that starts at zero |
| Bar distance | The spaces between candles. Left out with fewer than two loaded candles |
| Elapsed time | Weekends and market closures included. In days up to two weeks, then weeks up to two months, then months up to a year, then years: `5w 6d`, `7y 7mo` |

Points beyond the loaded candles use the chart's estimated times.

## See also

- [Drawings](/manual/drawings/)
- [Measuring from the CLI](/manual/cli/#measuring-a-move)
