---
title: "Chartbook templates"
description: "A saved arrangement of charts that several chartbooks share, so a change to one reaches them all."
section: charting
nav_order: 20
---

A chartbook template is a saved arrangement of charts under a name. It holds
the splits, where the dividers sit, and each chart's resolution and
[chart template](/manual/chart-templates/), or its own indicators when it has
none. Put several chartbooks on the same template and they all have that
arrangement. Change the arrangement in any one of them and the others follow.
Symbols and link modes are not part of a template; each chartbook keeps its
own.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture in Chartbooks, then)
  omacharts chartbook template save Desk --book Semis
  omacharts chartbook template apply Desk --book Commodities
  omacharts chartbook switch Commodities     # wait for the data
capture:
  bin/headless-window cli chartbook screenshot --output commodities.png
  omacharts chartbook switch Semis
  bin/headless-window cli chartbook screenshot --output semis.png
  magick ( semis.png -resize 990x ) ( -size 20x10 xc:#16161e ) ( commodities.png -resize 990x ) +append -quality 88
crop: none
-->
{% include figure.html src="/assets/manual/chartbook-templates/two-books-one-template.jpg" alt="Two chartbooks side by side with the same arrangement: NVDA daily, hourly and 15-minute on the left, GLD daily, hourly and 15-minute on the right" caption="*Semis* and *Commodities*, both on the *Desk* template, each with its own symbols." wide="true" %}

## Creating a template

1. Arrange a chartbook the way you want: split it, set each chart's resolution and indicators.
2. Right-click its tab in the chartbook strip and choose **Template** › **Save as New Template…**.
3. Type a name and click **Save**. The chartbook is now on the template.
4. Put another chartbook on it from its tab's **Template** menu ([Putting a chartbook on a template](#putting-a-chartbook-on-a-template)).

## The Template menu

Right-click a chartbook tab and open **Template**
([Chartbooks](/manual/chartbooks/#the-strip)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture in Chartbooks, then)
  omacharts chartbook template save Desk --book Semis
  omacharts chart split vertical --book Macro
  omacharts chartbook template save Pair --book Macro
  bin/virtual-pointer --button right click 104 708   # the Semis tab
  click 154 685                                       # Template
  move 420 560
capture: bin/headless-window shot submenu.png
crop: 0,960,760,480
-->
{% include figure.html src="/assets/manual/chartbook-templates/template-submenu.png" alt="The tab menu's Template submenu, with Desk chosen" caption="The **Template** submenu of a chartbook on *Desk*." %}

| Item | Does |
|---|---|
| **None** | Takes the chartbook off its template. It keeps what it has. |
| A template | Puts the chartbook on it, asking first when that would lose an arrangement no template holds. The one it is on is marked. |
| **Save as New Template…** | Asks for a name, saves the chartbook's arrangement as a new template, and puts the chartbook on it. |
| **Manage Templates…** | Every template, to rename or delete. |

The **New chartbook** dialog has a **Template** row once a template exists,
so a chartbook can start on one ([Chartbooks](/manual/chartbooks/#a-new-chartbook)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture in Chartbooks, then)
  bin/virtual-pointer --button right click 104 708   # the Semis tab
  click 154 685                                       # Template
  click 230 641                                       # Save as New Template…
  type Desk
  move 1279 719
capture: bin/headless-window shot save.png
crop: 880,470,800,500
-->
{% include figure.html src="/assets/manual/chartbook-templates/save-new.png" alt="The Save as New Chartbook Template dialog with Desk typed in the Name box, and Cancel and Save buttons" caption="Saving a chartbook as a new template." %}

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (as for the Template submenu above, then)
  omacharts chartbook template apply Desk --book Commodities
  click 230 685                                       # Manage Templates…
  move 1279 719
capture: bin/headless-window shot manage.png
crop: 820,460,920,520
-->
{% include figure.html src="/assets/manual/chartbook-templates/manage-templates.png" alt="The Chartbook Templates dialog listing Desk and Pair, each with its chart and chartbook counts, a pencil and a bin" caption="The **Manage Templates…** dialog." %}

The pencil renames a template. The chartbooks on it stay on it. The bin
deletes it, after saying how many chartbooks use it. Those chartbooks keep
their arrangement and indicators and stop following. A new template with the
same name does not put them back on it.

## Putting a chartbook on a template

A chartbook put on a template keeps its symbols and link modes, chart by
chart (the first chart keeps the first chart's symbol, and so on). When the
template has more charts than the chartbook, each extra chart is a copy of
the chart beside it (or of the focused chart), with no drawings. Charts past the template's last one
are removed, with their drawings. A maximised chart is restored.

When no template holds the arrangement being replaced, a dialog asks
first, *This chartbook's layout and indicators will be replaced by "Desk".
Its charts keep their symbols.*, naming the charts that would go, with
**Cancel** and **Replace Layout**.

## Changing a template

Splitting or closing a chart, dragging a divider, or changing a chart's
resolution, indicators or chart template in one chartbook on the template
changes it in all of them. A chart added in one appears beside the same chart
in the others.

## See also

- [Chartbook templates from the CLI](/manual/cli/#chartbook-templates)
