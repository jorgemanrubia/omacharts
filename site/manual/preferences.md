---
title: "Preferences"
description: "Every page of the Preferences dialog, row by row, and the settings that stay with this machine."
section: configuration
nav_order: 27
---

Preferences holds the settings that apply to the whole app. Open it with <kbd>Ctrl</kbd> + <kbd>,</kbd> or from the main menu ([The window](/manual/window/)). Every row is saved as it changes, with no Apply or Cancel. A chart's own settings, and the defaults new charts open with, are in [Chart settings](/manual/chart-settings/).

## The dialog

The sections down its side are **General**, **Charts**, **Market data**, **Appearance** and **Overlays**. In a narrow window the list of sections becomes a page of its own and a section opens over it.

Reopened while the app is running, the dialog comes back on the section and tab you left it on. Opening it from a chart's header goes to Overlays, where the header is edited.

<!-- regenerate:
theme: tokyo-night
window: 2560x1040 @2, $SP/kit/hw (a short window, so the dialog is no taller than its content)
state:
  mkdir -p $HOME/.config/omarchy/plugins; echo '{}' > $HOME/.config/omarchy/shell.json   # before `hw app`: a sandboxed Omarchy bar, so the Desktop group shows
  move 1270 510
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+comma
  click 272 89           # General (clears the keyboard-focus ring)
  click 1003 157         # Show in the Omarchy bar, off
  move 1270 510
capture: bin/headless-window shot /tmp/prefs.png
crop: 276,16,2008,1008   (the whole dialog, 12px layout margin)
-->
{% include figure.html src="/assets/manual/preferences/general.png" alt="The Preferences dialog on General, with the Desktop, Time, Chart, Preference backups and Updates groups" caption="Preferences on General." %}

### General

| Group › row | Does | Chapter |
|---|---|---|
| Desktop › **Show in the Omarchy bar** | Puts the Omarchy plugin in the bar, or takes it out. The group appears only on an Omarchy desktop. | [Omarchy plugin](/manual/bar-widget/) |
| Time › **Time zone**, **Time format** | How every chart tells the time. | [Time](#time) |
| Chart › **Preset resolutions**, **Edit…** | Opens the Resolutions dialog. | [The preset resolutions](/manual/charts/#the-preset-resolutions) |
| Chart › **Zoom sensitivity** | How far the wheel and a two-finger swipe zoom every chart, from 10% to 200%. 100% by default, where a wheel notch zooms as it always has. | [Moving around](/manual/charts/#moving-around) |
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
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+comma
  click 214 147          # Charts in the sidebar
capture: bin/headless-window shot /tmp/charts.png
crop: 292,76,1976,1288
-->
{% include figure.html src="/assets/manual/preferences/charts.png" alt="Preferences on Charts, Defaults tab" caption="Preferences on Charts, Defaults tab." %}

**Defaults** is what a new chart opens with. Its rows, and **Apply to all charts now**, are in [Chart settings](/manual/chart-settings/#defaults-for-new-charts).

| Tab › group › row | Does | Chapter |
|---|---|---|
| Templates › **Chart templates** | The list. + makes one, a row opens its editor, the pencil renames, the bin deletes. | [Chart templates](/manual/chart-templates/) |

### Market data

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, $SP/kit/hw
state:
  move 1270 710
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+comma
  click 214 243          # Market data in the sidebar
  click 214 223          # Market data again (clears the keyboard-focus ring)
  move 1270 710
capture: bin/headless-window shot /tmp/data.png
crop: 276,56,2008,1328   (the whole dialog, 12px layout margin)
-->
{% include figure.html src="/assets/manual/preferences/data.png" alt="Preferences on Market data, with the Data feed and Storage groups" caption="Preferences on Market data." %}

| Group › row | Does | Chapter |
|---|---|---|
| Data feed › **Yahoo Finance**, **thinkorswim** | The feed every chart gets its prices from. Open charts switch at once. | [Market data feeds](/manual/market-data/) |
| thinkorswim › **Sign in…** / **Sign out** | Shown only while thinkorswim is chosen. | [Market data feeds](/manual/market-data/) |
| Storage › **Cached data**, **Clear** | How much downloaded price data is kept, and a button to delete it all. | [Where your data lives](/manual/your-data/) |
| Storage › **Keep at most** | 256 MB, 512 MB, 1 GB, 2 GB, 5 GB. | [Where your data lives](/manual/your-data/) |

### Appearance

Everything on this page is the same on every chart, in every chartbook, and changes them all as you change it. A chart's settings show the same page, under **Global**.

| Group › row | Does | Chapter |
|---|---|---|
| Theme › **Follow system**, **Catalogue** | The theme: the desktop's, or one picked from every Omarchy theme. | [Choosing a theme](/manual/themes/#choosing-a-theme) |
| Bars › **Drawn as** | **Candles** or **OHLC**. | [Style](/manual/charts/#style) |
| Bars › **Thickness** | How many pixels wide OHLC bars are drawn, from 1 to 4. Only there while the bars are OHLC. | [Style](/manual/charts/#style) |
| Bars › **Up**, **Down** | Each direction's colour, and its body: filled, bordered or hollow. | [Bars](/manual/themes/#bars) |

### Overlays

The same on every chart, in every chartbook. A chart's settings show the same page, under **Global**.

| Group › row | Does | Chapter |
|---|---|---|
| **Left**, **Center**, **Right** | The parts the header shows in each section, in order. A part is in one section at a time. **Restore defaults** puts every section back as it ships. | [The header](/manual/charts/#the-header) |
| Lines › **Gridlines** | The grid behind the bars. The scales and their labels stay either way. Off until turned on. | [Gridlines](/manual/charts/#gridlines) |
| Lines › **Last price line** | The dashed line at the last price. The price stays on the axis either way. | [The last price line](/manual/charts/#the-last-price-line) |
| Lines › **Crosshair across Follow charts** | The pointer on one chart draws the same moment on every other chart of the same symbol. | [Sharing the crosshair](/manual/charts/#sharing-the-crosshair) |

Right-clicking a chart's header and choosing **Edit header…** opens this page.

## Settings that stay with this machine

The **Keep at most** limit, **Update automatically**, the sidebar width and the screenshot folder (the `screenshot_folder` setting, [Where it goes](/manual/screenshots/#where-it-goes)) stay with this machine. Restoring another machine's backup leaves them as they were ([Restoring](/manual/your-data/#restoring)).

## See also

- [Settings from the CLI](/manual/cli/#preferences)
