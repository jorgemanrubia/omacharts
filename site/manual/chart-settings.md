---
title: "Chart settings"
description: "A chart's own settings, row by row, and the defaults every new chart opens with."
section: configuration
nav_order: 28
---

Each chart has settings of its own. New charts open with the defaults set in Preferences.

## The Chart settings dialog

Chart settings is the dialog for the focused chart. Open it with <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>S</kbd>, the gear in the chart's top-right corner, or **Chart settings…** in the chart menu. Every row takes effect at once. It has two pages, **Chart** and **Indicators**, and <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> moves between them.

### The Chart page

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (one NVDA 1D chart, sidebar hidden)
  key ctrl+shift+s
capture: bin/headless-window shot /tmp/chartsettings.png; then scroll 640 500 10 and shot /tmp/chartsettings2.png
crop: 700,84,1160,1276 (both)
-->
{% include figure.html src="/assets/manual/charts/chart-settings.png" alt="The Chart page of the Chart settings dialog, showing the Title row, the Bars group with its Colouring, Hollow up bars and Swap up and down colours rows, and the Scales group" caption="The Chart page, top." %}

<!-- regenerate: as the figure above; this is its second capture, /tmp/chartsettings2.png, after scroll 640 500 10 -->
{% include figure.html src="/assets/manual/charts/chart-settings-lower.png" alt="The Chart page scrolled down, showing the Session, Drawings and Reset to defaults groups" caption="The Chart page, bottom." %}

<!-- regenerate:
as above, with omacharts chart set --colouring theme-mono run before key ctrl+shift+s
crop: 700,84,1160,1276
-->
{% include figure.html src="/assets/manual/charts/chart-settings-monochrome.png" alt="The Chart page with Monochrome chosen, the Swap up and down colours row greyed out" caption="Monochrome paints a rise and a fall alike, so the swap is greyed out; Hollow up bars is how it shows direction." %}

| Group › row | Does |
|---|---|
| **Title** | The chart title, shown by the header's **Chart title** part. Blank for none ([Chart titles](/manual/charts/#chart-titles)) |
| **Bars** › **Style** | **Candles**, **OHLC bars**, or **None** to hide the bars ([Bars](/manual/charts/#bars)). The three rows under it are greyed out while they are hidden |
| **Bars** › **Thickness** | How many pixels wide OHLC bars are drawn, from 1 to 4. Only there on OHLC bars |
| **Bars** › **Colouring** | **Vivid**, **Soft**, **Quiet** or **Monochrome** ([Colouring](/manual/charts/#colouring)) |
| **Bars** › **Hollow up bars** | Draws a rising candle as an outline. Greyed out on OHLC bars, which have no body |
| **Bars** › **Swap up and down colours** | Paints the colouring the other way round, red for a rise. Greyed out on Monochrome |
| **Scales** › **Auto scale price** | Fits the visible bars vertically. Off, the chart can be dragged up and down ([Scales](/manual/charts/#scales)) |
| **Scales** › **Logarithmic scale** | Equal heights are equal percentage moves ([Logarithmic scale](/manual/charts/#logarithmic-scale)) |
| **Scales** › **Gridlines** | Draws the grid. The scales and their labels stay either way |
| **Scales** › **Reset chart** › **Reset** | Both scales back to how the chart opened, at the latest bars. The same as <kbd>Ctrl</kbd> + <kbd>Esc</kbd> |
| **Session** › **Hours** | **Extended hours** or **Regular hours only** ([Sessions](/manual/charts/#sessions)) |
| **Drawings** › **Show drawings from other charts** | Off, the chart shows only its own drawings |
| **Drawings** › **Send drawings to other charts** | Off, what is drawn on this chart stays on this chart ([Sharing drawings between charts](/manual/drawings/#sharing-drawings-between-charts)) |
| **Reset to defaults** › **Reset** | Puts this chart back on the defaults. See [Putting one chart back on the defaults](#putting-one-chart-back-on-the-defaults) |

### The Indicators page

The Indicators page lists the chart's indicators under **On the chart**, or the template's under **On the Default template** on a chart on one, with the **Chart template** row above them. <kbd>Ctrl</kbd> + <kbd>I</kbd> and the chart menu's **Indicators…** open the dialog on this page. Its rows are in [Indicators](/manual/indicators/#the-indicators-page), and the template row in [Chart templates](/manual/chart-templates/).

## Defaults for new charts

Preferences › Charts › **Defaults** is what every new chart opens with. A chart made by splitting copies the chart it divides instead ([Layouts](/manual/layouts/)). Changing a default changes no chart already open, and changing a chart never changes the defaults.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  key ctrl+comma
  click 214 147           # Charts
capture: bin/headless-window shot /tmp/prefs-charts.png
crop: 292,76,1976,1288   (the whole dialog; the Defaults page scrolls inside it)
-->
{% include figure.html src="/assets/manual/charts/preferences-defaults.png" alt="Preferences open on the Charts Defaults tab" caption="Preferences › Charts › Defaults." %}

| Group › row | Choices | Default |
|---|---|---|
| **Bars** › **Style** | **Candles**, **OHLC bars** | **Candles** |
| **Bars** › **Thickness** | 1 to 4 pixels. Only there on OHLC bars | 2 |
| **Bars** › **Colouring** | **Vivid**, **Soft**, **Quiet**, **Monochrome** | **Vivid** |
| **Bars** › **Hollow up bars** | On or off. Greyed out on OHLC bars | Off |
| **Bars** › **Swap up and down colours** | On or off. Greyed out on Monochrome | Off |
| **Scales** › **Logarithmic scale** | On or off | Off |
| **Scales** › **Gridlines** | On or off | Off |
| **Session** › **Hours** | **Extended hours**, **Regular hours only** | **Extended hours** |
| **Indicators** › **Template** | A chart template, or **None** ([Chart templates](/manual/chart-templates/)) | **Default** |
| **Drawings** › **Show drawings from other charts**, **Send drawings to other charts** | On or off ([Drawings](/manual/drawings/)) | On |

A new chart's price scale starts by fitting the bars.

### Applying the defaults to every chart

Once a row has changed, *New charts will use these options.* and **Apply to all charts now** appear under the last group. The button puts every chart in every chartbook on the defaults, template included. When that would remove some charts' own indicators, it asks first with **Apply to All Charts?**, saying how many charts have indicators of their own and whether they are replaced by the default template's or removed. **Cancel** changes nothing.

### Putting one chart back on the defaults

**Reset to defaults** › **Reset**, at the bottom of the Chart page, puts the focused chart's bars, session, scales, drawing switches and template back on the defaults. It asks first when the chart has indicators of its own, as choosing a template does ([Chart templates](/manual/chart-templates/)).

## See also

- [Changing charts from the CLI](/manual/cli/#charts)
- [The chart defaults from the CLI](/manual/cli/#settings)
