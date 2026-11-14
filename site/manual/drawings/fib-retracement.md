---
title: "Fib retracement"
description: "A level at each Fibonacci ratio of a swing"
section: charting
parent: drawings
nav_order: 11
hotkey: "Alt + F"
---

A level at each Fibonacci ratio between two prices: 0%, 23.6%, 38.2%, 50%, 61.8%, 78.6% and 100%, across the time between the two points.

| Key | Placing it | CLI |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>F</kbd> | Press where the swing starts, drag, and release where it ends | `fib`, `fibonacci`, `retracement` |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts config set theme omarchy:tokyo-night   (before the window opens)
  omacharts chart set --symbol SPY --resolution 1D --template none
  key ctrl+shift+b   (until a chart screenshot is the window's full width: the sidebar hidden)
  omacharts chart drawing add fib --from 2026-06-22,717 --to 2026-08-13,780 --config 2
  omacharts chart drawing add fib --from 2026-10-06,782 --to 2026-11-04,733 --config 1
  move 1270 1000
capture: bin/headless-window cli chart screenshot --output /tmp/fib.png; magick /tmp/fib.png -quality 88 site/assets/manual/drawings/fib-retracement.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/fib-retracement.jpg" alt="SPY daily with two Fib retracements in a soft grey: one drawn up the summer rally, solid, and one drawn down the fall from the October high, its middle levels dashed and dotted" caption="Two retracements: configuration 2, every level solid, dragged up a rally, and configuration 1, as a new one is drawn, dragged down a later fall." %}

0% is where you released and 100% where you pressed, so 61.8% is how far a move has come back once it has given up 61.8% of the swing. Drag up from a low to a high for the pullback from that high, or down from a high to a low for the bounce from that low.

A dashed line joins the two points, and each level is labelled at its left end with its ratio, as a percentage. **Price labels** adds the price beside it. Every level is the drawing's one colour unless you give it one of its own.

Selected, it has a grip at each end. Drag a grip to move that end, or the levels or the dashed line to move the whole drawing. The arrow keys nudge it. It takes no text. A press between two levels goes to the chart under it.

## Properties

<kbd>Alt</kbd> + <kbd>Enter</kbd> or a double-click on a level opens them. Defaults are those of configuration 1 as shipped, with the other configurations' differences under [Configurations](#configurations).

| Property | What it does | Default |
|---|---|---|
| **Colour** | The colour of every level that has none of its own: one of the nine [presets](/manual/drawings/#colours), which change with the theme, or **Custom…** for a hex colour. Changing it recolours all of those levels at once | Ink |
| **Fib labels** | Writes each level's ratio at its left end, as 61.8% | On |
| **Price labels** | Writes each level's price at its left end, after the ratio when both are on | Off |

### Levels

One row a level, **0%** to **100%**, each with a switch that shows or hides that level and a picture of how it is drawn. Click a row to open it on three properties of its own.

| Property | What it does | Default |
|---|---|---|
| **Colour** | The level's own colour. **Reset** goes back to the drawing's, which the level then follows | The drawing's |
| **Thickness** | No line, 1, 1.5, 2.5 or 4 px, picked from drawn samples. No line keeps the level's label without its line | 1 px |
| **Line style** | **Solid**, **Dashed** or **Dotted** | Solid for 0% and 100%; dashed for 50%; dotted for the rest |

**Thickness** in the drawing's menu, and <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>+</kbd> / <kbd>−</kbd>, set every level at once.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the state of the Fib retracement figure above, its two drawings)
  key Escape
  click 1088 189   (the later retracement's 50% level)
  key alt+Return
  scroll 824 500 6
capture: bin/headless-window shot /tmp/fibprops.png; magick /tmp/fibprops.png -crop 926x1406+817+17 +repage site/assets/manual/drawings/fib-properties.png
crop: 817,17,926,1406
-->
{% include figure.html src="/assets/manual/drawings/fib-properties.png" alt="A Fib retracement's properties: Colour, Fib labels on and Price labels off, then the seven levels from 0% to 100%, each with its switch and a sample of its line" caption="A retracement's properties, on configuration 1." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the state of the previous figure)
  click 824 539    (the 61.8% row's arrow)
  scroll 824 500 6
capture: bin/headless-window shot /tmp/fiblevel.png; magick /tmp/fiblevel.png -crop 926x1406+817+17 +repage site/assets/manual/drawings/fib-level-properties.png
crop: 817,17,926,1406
-->
{% include figure.html src="/assets/manual/drawings/fib-level-properties.png" alt="The same properties with the 61.8% level open on its Colour, Thickness and Line style, dotted" caption="A level open: its own colour, thickness and line style." %}

<!-- regenerate:
theme: catppuccin-latte
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts config set theme omarchy:catppuccin-latte   (before the window opens)
  (the state of the Fib retracement figure above, its two drawings)
  key Escape
  key alt+f
  drag 307 182 349 339
  omacharts chart drawing set --id 2 --level 0.618 --level-color amber --level-width 2.5 --level-style dashed --price-labels on
  omacharts chart drawing set --id 1 --fib-labels off
  move 1270 1000
capture: bin/headless-window cli chart screenshot --output /tmp/fiblight.png; magick /tmp/fiblight.png -quality 88 site/assets/manual/drawings/fib-levels-light.jpg
crop: none
-->
{% include figure.html src="/assets/manual/drawings/fib-levels-light.jpg" alt="SPY daily on a light theme with three retracements in a mid grey: the summer rally's with no labels, a short June fall drawn with the mouse, and the October fall labelled with ratios and prices and its 61.8% level a thicker dashed amber" caption="On a light theme: no labels on the first, and on the last prices beside the ratios and its 61.8% level in Amber, 2.5 px, dashed." %}

## Configurations

Its nine configurations are one colour each, with no fills. None is Up or Down: a retracement is drawn over a move either way.

| Configuration | Is |
|---|---|
| 1 | Ink, 1 px, with 0% and 100% solid, 50% dashed and the rest dotted. Drawn soft, as a light grey on a dark theme and a mid grey on a light one |
| 2 | Ink, every level a solid 1 px line |
| 3 to 8 | Blue, Amber, Violet, Teal, Orange and Cyan, every level solid |
| 9 | Ink, without 23.6% and 78.6% |

Every property is part of the configuration, so **Save as…** keeps each level's colour, thickness and line style, and whether it is shown ([Configurations](/manual/drawings/#configurations)).

## See also

- [Drawings](/manual/drawings/)
- [Drawing from the CLI](/manual/cli/#drawings)
