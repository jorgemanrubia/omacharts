---
title: "Preferences"
description: "Every section of Preferences, row by row, and the settings that stay with this machine."
section: configuration
nav_order: 27
---

Preferences holds the settings that apply to the whole app. Open it with <kbd>Ctrl</kbd> + <kbd>,</kbd> or from the main menu ([The window](/manual/window/)). Every row is saved as it changes, with no Apply or Cancel. A chart's own settings, and the defaults new charts open with, are in [Chart settings](/manual/chart-settings/).

## The window

Preferences is a window of its own. Move it beside the charts to watch them change as you change a row. Opening it again while it is open brings it to the front. The sections down its side are **General**, **Market data** and **Templates**, then **Appearance**, **Overlays** and **Charts**. In a narrow window the list of sections becomes a page of its own and a section opens over it.

Reopened while the app is running, Preferences comes back on the section you left it on. Opening it from a chart's header goes to Overlays, where the header is edited.

<!-- regenerate:
theme: tokyo-night, as an Omarchy desktop's: a scratch HOME with each ~/.local/share/omarchy/themes/*/colors.toml copied in,
  tokyo-night's also as ~/.local/state/omarchy/current/theme/colors.toml and `tokyo-night` in theme.name beside it,
  ~/.config/omarchy/shell.json holding {} and an empty ~/.config/omarchy/plugins (so the Desktop group shows),
  ~/.config/gtk-4.0/settings.ini with gtk-icon-theme-name=Yaru-purple
window: 2560x2400 @2, bin/headless-window start --size 2560x2400 (tall enough for the window to fit its content), HOME as above
state:
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+comma
  run swaymsg '[title="^Preferences$"] resize set 820 770, move position 230 100'
  move 100 1150          # off the window, so no row is hovered
capture: bin/headless-window shot /tmp/prefs.png
crop: 436,176,1688,1588   (the whole window, 12px layout margin)
-->
{% include figure.html src="/assets/manual/preferences/general.png" alt="The Preferences window on General, with the Desktop, Time, Chart, Preference backups and Updates groups" caption="Preferences on General." %}

Appearance, Overlays and Charts sit under the **VISUAL** rule, which says which charts they change. **GLOBAL** changes every chart. **LOCAL** changes the focused chart alone; hover it to see the chart's symbol. In a split, that chart wears its focus ring twice as wide while **LOCAL** is picked, as it does under a picker. Once the focused chart has [a look of its own](/manual/charts/#a-charts-own-look) that differs from every chart's, a reset arrow appears after **LOCAL**, under either choice: click it, **Reset to global defaults**, to put the chart back on every chart's look. Preferences opens on GLOBAL.

### General

| Group › row | Does | Chapter |
|---|---|---|
| Desktop › **Show in the Omarchy bar** | Puts the Omarchy plugin in the bar, or takes it out. The group appears only on an Omarchy desktop. | [Omarchy plugin](/manual/bar-widget/) |
| Time › **Time zone**, **Time format** | How every chart tells the time. | [Time](#time) |
| Chart › **Preset resolutions**, **Edit…** | Opens the Resolutions dialog. | [The preset resolutions](/manual/charts/#the-preset-resolutions) |
| Chart › **Zoom sensitivity** | How far the wheel and a two-finger swipe zoom every chart, from 10% to 200%. 100% by default, where a wheel notch zooms as it always has. Dragged near 100%, the slider settles on it; the arrow keys step through it. | [Moving around](/manual/charts/#moving-around) |
| Preference backups › **Last backup**, **Back Up Now**, **Restore…** | When settings were last backed up, and restoring one. | [Where your data lives](/manual/your-data/) |
| Updates › **Update automatically** | Downloads new releases in the background, to start at the next launch. On by default. | [Installation](/manual/installing/#updates) |

#### Time

The **Time** group sets the clock every chart reads. A change applies at once to every chart on screen. It governs the time axis, the crosshair, every time typed into an indicator's settings, and the times `omacharts chart drawing` reads and prints.

| Row | Choices | Default |
|---|---|---|
| **Time zone** | **Each chart's exchange**, or a city, each listed with its offset from UTC today, such as **London (UTC+1)** | New York |
| **Time format** | **24-hour (15:30)**, **12-hour (3:30 PM)** | The desktop's clock format |

**Each chart's exchange** puts each chart in its own exchange's clock: ES in Chicago time, a Madrid listing in Madrid time, crypto in UTC. The crosshair's time then ends with the zone, such as **21 Oct 2026 07:30 CT**, so two linked charts that disagree about the time say why. Any other time zone, by its IANA name such as `Asia/Kolkata`, can be set from the CLI ([Settings](/manual/cli/#settings)); the row then lists it too.

Daily and longer bars are dates and read the same in every time zone.

### Charts

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
{% include figure.html src="/assets/manual/preferences/charts.png" alt="Preferences on Charts, with the Scales, Session, Indicators and Drawings groups" caption="Preferences on Charts." %}

A chart's scale, session hours, template and drawings. Under **GLOBAL**, every chart's and what a new chart opens with; under **LOCAL**, the focused chart's ([Defaults for new charts](/manual/chart-settings/#defaults-for-new-charts)).

### Market data

<!-- regenerate:
theme, window: as General above (seeded cache, so Cached data has series in it)
state:
  as General above, then
  click 313 208          # Market data in the sidebar
  move 100 1150
capture: bin/headless-window shot /tmp/data.png
crop: 436,176,1688,1588   (the whole window, 12px layout margin)
-->
{% include figure.html src="/assets/manual/preferences/data.png" alt="Preferences on Market data, with Yahoo Finance under Data feeds, Add a data feed… below it, and the Storage group" caption="Preferences on Market data." %}

| Group › row | Does | Chapter |
|---|---|---|
| **Data feeds** | The feeds charts get their prices from, numbered in the order they are tried. Each chart comes from the first feed that has its symbol and is signed in. With more than one, drag a feed by the handle that replaces its number under the pointer, or press <kbd>Alt</kbd>+<kbd>↑</kbd> or <kbd>Alt</kbd>+<kbd>↓</kbd>, to change the order; **Remove** takes one off, except the last. | [Market data feeds](/manual/market-data/) |
| Data feeds › **Sign in…** / **Sign out** | On the row of a feed that needs an account. | [Market data feeds](/manual/market-data/) |
| Data feeds › **Add a data feed…** | Adds a feed that is not in the list yet. | [Market data feeds](/manual/market-data/) |
| Storage › **Cached data**, **Clear** | How much downloaded price data is kept, and a button to delete it all. | [Where your data lives](/manual/your-data/) |
| Storage › **Keep at most** | 256 MB, 512 MB, 1 GB, 2 GB, 5 GB. | [Where your data lives](/manual/your-data/) |

### Templates

The list of chart templates, the same for every chart on one. **+** makes one, a row opens its editor, the pencil renames, the bin deletes ([Chart templates](/manual/chart-templates/)).

### Appearance

Under **GLOBAL**, everything on this page is every chart's, in every chartbook, and changes them all as you change it. Under **LOCAL**, the theme, the bars and OHLC thickness are the focused chart's: the first change gives it a look of its own. [Chart settings](/manual/chart-settings/) shows the same page, under a rule of its own.

| Group › row | Does | Chapter |
|---|---|---|
| Theme › **Follow system**, **Catalogue** | The theme: the desktop's, or one picked from every Omarchy theme. | [Choosing a theme](/manual/themes/#choosing-a-theme) |
| Bars › **Candles**, **OHLC bars**, **Line** | The style, as tabs at the group's head. | [Style](/manual/charts/#style) |
| Bars › **Up**, **Down** | Each direction's colour, and its body for candles (filled, bordered or hollow) or its thickness for OHLC bars: 1, 1.5, 2 or 3 pixels. Not shown for a line. | [Bars](/manual/themes/#bars) |
| Bars › **High**, **Low** | A line's colour at its highest close on screen and at its lowest. Shown only for a line. | [Bars](/manual/themes/#bars) |

### Overlays

Like Appearance: every chart's under **GLOBAL**, the focused chart's under **LOCAL**. **Crosshair across Follow charts** is between charts by nature, so it is greyed under LOCAL. [Chart settings](/manual/chart-settings/) shows the same page, under a rule of its own.

| Group › row | Does | Chapter |
|---|---|---|
| Header | What every chart's header shows and where, drawn as a chart shows it, with the parts it does not show underneath. Drag a part to any of the two rows' left, centre or right, or down among the others to take it off, as right-clicking or double-clicking it does; click one underneath to put it back. **Restore defaults** puts it back as it ships. | [The header](/manual/charts/#the-header) |
| Lines › **Gridlines** | The grid behind the bars. The scales and their labels stay either way. Off until turned on. | [Gridlines](/manual/charts/#gridlines) |
| Lines › **Last price line** | The dashed line at the last price. The price stays on the axis either way. | [The last price line](/manual/charts/#the-last-price-line) |
| Lines › **Crosshair across Follow charts** | The pointer on one chart draws the same moment on every other chart of the same symbol. | [Sharing the crosshair](/manual/charts/#sharing-the-crosshair) |

Right-clicking a chart's header and choosing **Edit header…** opens this page. A header with every part taken off has nothing to right-click, so this page is the way back to it.

## Settings that stay with this machine

The **Keep at most** limit, **Update automatically**, the sidebar width and the screenshot folder (the `screenshot_folder` setting, [Where it goes](/manual/screenshots/#where-it-goes)) stay with this machine. Restoring another machine's backup leaves them as they were ([Restoring](/manual/your-data/#restoring)).

## See also

- [Settings from the CLI](/manual/cli/#preferences)
