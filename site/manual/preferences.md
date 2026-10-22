---
title: "Preferences"
description: "Every page of the Preferences dialog, row by row, and the settings that stay with this machine."
section: configuration
nav_order: 27
---

Preferences holds the settings that apply to the whole app. Open it with <kbd>Ctrl</kbd> + <kbd>,</kbd> or from the main menu ([The window](/manual/window/)). Every row is saved as it changes, with no Apply or Cancel. A chart's own settings, and the defaults new charts open with, are in [Chart settings](/manual/chart-settings/).

## The dialog

In a narrow window the list of sections becomes a page of its own and a section opens over it.

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
{% include figure.html src="/assets/manual/preferences/general.png" alt="The Preferences dialog on General, with the Desktop and Screenshots groups" caption="Preferences on General." %}

### General

| Group › row | Does | Chapter |
|---|---|---|
| Desktop › **Show in the Omarchy bar** | Puts the Omarchy plugin in the bar, or takes it out. The group appears only on an Omarchy desktop. | [Omarchy plugin](/manual/bar-widget/) |
| Screenshots › **Also save a PNG** | Keeps a file as well as copying to the clipboard, which always happens. | [Screenshots](/manual/screenshots/) |
| Screenshots › **Save to**, **Change…** | The folder the PNG goes to. Greyed out while the switch above is off. | [Screenshots](/manual/screenshots/) |

### Charts

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, $SP/kit/hw
state:
  omacharts chart set --symbol NVDA --resolution 1D
  key ctrl+comma
  click 214 168          # Charts in the sidebar
capture: bin/headless-window shot /tmp/charts.png
crop: 301,122,1958,1356
-->
{% include figure.html src="/assets/manual/preferences/charts.png" alt="Preferences on Charts, Defaults tab" caption="Preferences on Charts, Defaults tab." %}

**Defaults** is what a new chart opens with. Its rows, and **Apply to all charts now**, are in [Chart settings](/manual/chart-settings/#defaults-for-new-charts).

| Tab › group › row | Does | Chapter |
|---|---|---|
| Templates › **Chart templates** | The list. + makes one, a row opens its editor, the pencil renames, the bin deletes. | [Chart templates](/manual/chart-templates/) |
| Header › **In the header** | The parts every chart's header shows, in order. | [The header](/manual/charts/#the-header) |
| Other › Chart › **Preset resolutions**, **Edit…** | Opens the Resolutions dialog. | [The preset resolutions](/manual/charts/#the-preset-resolutions) |

### Theme

**Theme** chooses the theme and **Customize** makes one of your own ([Themes and colours](/manual/themes/)).

### Data

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, $SP/kit/hw; after `hw start` and before `hw app`, XDG_DATA_HOME in the session's env file
  pointed at $HOME/.local/share so paths read from ~
state:
  move 1270 710
  omacharts chart set --symbol NVDA --resolution 1D
  omacharts backup create      # and a few changes between, so several are kept
  key ctrl+comma
  click 214 243          # Data in the sidebar
  click 214 223          # Data again (clears the keyboard-focus ring)
  move 1270 710
capture: bin/headless-window shot /tmp/data.png
crop: 276,56,2008,1328   (the whole dialog, 12px layout margin)
-->
{% include figure.html src="/assets/manual/preferences/data.png" alt="Preferences on Data, with the Data feed, Storage and Backups groups" caption="Preferences on Data." %}

| Group › row | Does | Chapter |
|---|---|---|
| Data feed › **Yahoo Finance**, **thinkorswim** | The feed every chart gets its prices from. Open charts switch at once. | [Market data feeds](/manual/market-data/) |
| thinkorswim › **Sign in…** / **Sign out** | Shown only while thinkorswim is chosen. | [Market data feeds](/manual/market-data/) |
| Storage › **Cached data**, **Clear** | How much downloaded price data is kept, and a button to delete it all. | [Where your data lives](/manual/your-data/) |
| Storage › **Keep at most** | 256 MB, 512 MB, 1 GB, 2 GB, 5 GB. | [Where your data lives](/manual/your-data/) |
| Backups › **Last backup**, **Back Up Now**, **Restore…** | When settings were last backed up, and restoring one. | [Where your data lives](/manual/your-data/) |

## Settings that stay with this machine

The screenshot folder, the **Keep at most** limit and the sidebar width stay with this machine. Restoring another machine's backup leaves them as they were ([Restoring](/manual/your-data/#restoring)).

## See also

- [Settings from the CLI](/manual/cli/#preferences)
