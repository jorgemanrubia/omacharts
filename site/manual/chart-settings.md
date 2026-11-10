---
title: "Chart settings"
description: "A chart's own settings, row by row, and the defaults every new chart opens with."
section: configuration
nav_order: 28
---

Each chart has settings of its own. New charts open with the defaults set in Preferences.

## The Chart settings window

Chart settings is a window of its own for the focused chart, so the chart stays in sight while you change it. Open it with <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>S</kbd>, the gear in the chart's top-right corner, or **Chart settings…** in the chart menu. Every row takes effect at once. The chart's name is under the window's title, such as **pos:0 NVDA 1D**. Focus another chart while it is open and it shows that one.

The sections down its side are the chart's own, **Chart** and **Indicators**, then **Appearance** and **Overlays** under the **VISUAL** rule, the same pages and the same rule as in [Preferences](/manual/preferences/). **GLOBAL** changes every chart. **LOCAL** changes this chart alone: the first change gives it [a look of its own](/manual/charts/#a-charts-own-look). In a split Chart settings opens on **LOCAL**, since the chart in sight is one of several; on a chart alone it opens on **GLOBAL**. Focusing another chart starts it the same way again. The choice is this window's own, so picking one here leaves Preferences on its own choice. The reset arrow after **LOCAL** works as it does in Preferences. <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> moves from one section to the next.

### The Chart page

<!-- regenerate:
theme, window: as Preferences › General ([Preferences](/manual/preferences/)): tokyo-night as an Omarchy desktop's,
  Yaru-purple icons, 2560x2400 @2 from bin/headless-window start --size 2560x2400, HOME sandboxed
state:
  omacharts chart set --symbol NVDA --resolution 1D
  click 640 900          # the chart, so it has the keyboard
  key ctrl+shift+s
  run swaymsg '[title="^Chart settings$"] move position 260 100'   # its own size, 760x640
  move 100 1150          # off the window, so no row is hovered
capture: bin/headless-window shot /tmp/chartsettings.png; then scroll 760 500 10, move 100 1150, wait for the scrollbar to fade and shot /tmp/chartsettings2.png
crop: 496,176,1568,1328 (both; the whole window, 12px layout margin)
-->
{% include figure.html src="/assets/manual/charts/chart-settings.png" alt="The Chart settings window on Chart, showing the Title row, the Bars group and the Scales group, with Chart, Indicators, Appearance and Overlays down its side" caption="The Chart page, top." %}

<!-- regenerate: as the figure above; this is its second capture, /tmp/chartsettings2.png, after scroll 760 500 10 -->
{% include figure.html src="/assets/manual/charts/chart-settings-lower.png" alt="The Chart page scrolled down, showing the Session, Drawings and Reset to defaults groups" caption="The Chart page, bottom." %}

| Group › row | Does |
|---|---|
| **Title** | The chart title, shown by the header's **Chart title** part. Blank for none ([Chart titles](/manual/charts/#chart-titles)) |
| **Bars** › **Show bars** | Off, this chart's bars are hidden ([Hiding the bars](/manual/charts/#hiding-the-bars)). How bars are drawn, OHLC thickness included, is part of the chart's look, on the **Appearance** page |
| **Scales** › **Auto scale price** | Fits the visible bars vertically. Off, the chart can be dragged up and down ([Scales](/manual/charts/#scales)) |
| **Scales** › **Logarithmic scale** | Equal heights are equal percentage moves ([Logarithmic scale](/manual/charts/#logarithmic-scale)) |
| **Scales** › **Gridlines** | Draws the grid, on every chart, or on this chart alone when it has [a look of its own](/manual/charts/#a-charts-own-look). The scales and their labels stay either way ([Gridlines](/manual/charts/#gridlines)) |
| **Scales** › **Last price line** | The dashed line across at the last price, on every chart, or on this chart alone when it has a look of its own. The price stays on the axis either way ([The last price line](/manual/charts/#the-last-price-line)) |
| **Scales** › **Reset chart** › **Reset** | Both scales back to how the chart opened, at the latest bars. The same as <kbd>Ctrl</kbd> + <kbd>Esc</kbd> |
| **Session** › **Hours** | **Extended** or **Regular** ([Sessions](/manual/charts/#sessions)) |
| **Drawings** › **Show drawings from other charts** | Off, the chart shows only its own drawings |
| **Drawings** › **Send drawings to other charts** | Off, what is drawn on this chart stays on this chart ([Sharing drawings between charts](/manual/drawings/#sharing-drawings-between-charts)) |
| **Reset to defaults** › **Reset** | Puts this chart back on the defaults. See [Putting one chart back on the defaults](#putting-one-chart-back-on-the-defaults) |

### The Indicators page

The Indicators page lists the chart's indicators under **On the chart**, or the template's under **On the Default template** on a chart on one, with the **Chart template** and **Show legend for indicators** rows above them; off, the second takes the [legend](/manual/indicators/#the-legend) off the chart. <kbd>Ctrl</kbd> + <kbd>I</kbd> and the chart menu's **Indicators…** open Chart settings on this page. Its rows are in [Indicators](/manual/indicators/#the-indicators-page), and the template row in [Chart templates](/manual/chart-templates/).

## Defaults for new charts

Preferences › **Charts** is what every new chart opens with. A chart made by splitting copies the chart it divides instead ([Layouts](/manual/layouts/)). Changing a chart in its own settings never changes the defaults.

<!-- regenerate:
theme: the fresh profile's own (Yaru-purple icons from a scratch XDG_CONFIG_HOME's gtk-4.0/settings.ini)
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+comma
  click 294 355          # Charts in the sidebar, the last row
  move 1270 700
capture: bin/headless-window shot /tmp/charts.png
crop: 449,69,1662,1302   (the whole window, 12px margin)
-->
{% include figure.html src="/assets/manual/charts/preferences-defaults.png" alt="Preferences open on Charts" caption="Preferences › Charts." %}

| Group › row | Choices | Default |
|---|---|---|
| **Scales** › **Logarithmic scale** | On or off | Off |
| **Session** › **Hours** | **Extended**, **Regular** | **Extended** |
| **Indicators** › **Template** | A chart template, or **None** ([Chart templates](/manual/chart-templates/)) | **Default** |
| **Indicators** › **Show legend for indicators** | On or off ([The legend](/manual/indicators/#the-legend)) | On |
| **Drawings** › **Show drawings from other charts**, **Send drawings to other charts** | On or off ([Drawings](/manual/drawings/)) | On |

A new chart's price scale starts by fitting the bars.

### Every chart, or the focused one

**GLOBAL / LOCAL**, on the rule above Appearance in the sidebar, says which charts the rows change.

- **GLOBAL** changes the default and every chart in every chartbook at once, that row only: turning on **Logarithmic scale** leaves each chart's hours as they were. Each chart keeps its template and its indicators as they are, with their settings; the template only says what a new chart opens on.
- **LOCAL** changes the focused chart, as its own settings do, and the rows show that chart's settings. Its symbol is in LOCAL's tooltip. The defaults stay as they are.

### Putting one chart back on the defaults

**Reset to defaults** › **Reset**, at the bottom of the Chart page, shows the focused chart's bars and puts its session, scales, drawing switches, legend and template back on the defaults. Gridlines, the last-price line and how bars are drawn are part of the chart's look, and stay as they are. It asks first when the chart has indicators of its own, as choosing a template does ([Chart templates](/manual/chart-templates/)).

## See also

- [Changing charts from the CLI](/manual/cli/#charts)
- [The chart defaults from the CLI](/manual/cli/#settings)
