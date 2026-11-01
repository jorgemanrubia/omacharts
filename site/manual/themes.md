---
title: "Themes and colours"
description: "The desktop theme, the shipped themes, themes of your own, and the palette that indicators and drawings use."
section: configuration
nav_order: 25
---

A theme is the set of colours the window is drawn in. It includes the palette, the named colours that indicators and drawings use. On an Omarchy desktop the window follows the desktop theme and changes with it.

<!-- regenerate:
theme: tokyo-night, everforest, catppuccin-latte, gruvbox (one capture each, the desktop theme switched between them)
window: 2560x1600 @2, $SP/kit/hw
state:
  omacharts chart set --symbol NVDA --resolution 1D
  omacharts chart indicator add sma --period 50 --color Amber
  omacharts chart indicator add ema --period 21 --color Blue
  omacharts chart indicator add rsi
  move 1270 790
  for each theme: $SP/kit/theme THEME; sleep 4
capture: bin/headless-window cli chart screenshot --output /tmp/theme-THEME.png   (four, joined 2x2 with magick +append/-append, resized to 1600 wide)
crop: none
-->
{% include figure.html src="/assets/manual/themes/four-themes.jpg" alt="The same NVDA daily chart under four desktop themes" caption="The same chart under Tokyo Night, Everforest, Catppuccin Latte and Gruvbox." %}

## Choosing a theme

Preferences › Theme › **Theme** lists the themes in groups, separated by a line.

| Entry | What it is |
|---|---|
| **System** | The desktop's Omarchy theme, followed as it changes. The row says which one, *Following Tokyo Night*. Only on Omarchy, where it is the default. |
| Your own themes | Made with **Customize**, under the names you gave them. |
| **Midnight**, **Carbon**, **Paper**, **Daylight** | Shipped with Omacharts. Midnight is the default without Omarchy. |
| Installed Omarchy themes | Every theme installed on the desktop, under its own name. These do not follow the desktop. A theme installed while the window is open appears next time it opens. |

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window start --size 2560x1600, with the desktop's themes' colors.toml copied into $HOME/.local/share/omarchy/themes
state:
  move 1270 790
  omacharts config set theme omarchy
  key ctrl+comma
  click 214 205          # Theme in the sidebar
  click 985 157          # the Theme row's drop-down
capture: bin/headless-window shot /tmp/theme-list.png
crop: 934,228,1294,962   (the Theme group and the drop-down, 16px layout margin)
-->
{% include figure.html src="/assets/manual/themes/theme-list.png" alt="The Theme drop-down open, with System first and the shipped and installed themes below" caption="The Theme drop-down." %}

| Shipped theme | Look |
|---|---|
| Midnight | Dark. Deep slate blue. |
| Carbon | Dark. Neutral greys with a blue accent. |
| Paper | Light. Warm off-white with ink text. |
| Daylight | Light. White with cool neutrals. |

A light theme makes the whole window light, dialogs and menus included.

## Your own themes

A theme of your own is a copy of another theme. System, the shipped themes and installed Omarchy themes cannot be edited themselves. Preferences › Theme › Make it yours › **Customize** asks for a name (*Copy of Tokyo Night* is offered) and **Create** makes the copy, selects it and shows a row for every colour. A copy of System keeps the colours it had when copied and stops following the desktop. The copy stays in the list when you switch back to the original.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, $SP/kit/hw
state:
  omacharts config set theme omarchy
  key ctrl+comma
  click 214 205          # Theme
  click 971 212          # Customize
  key Return             # Create, with the name offered
capture: bin/headless-window shot /tmp/custom-theme.png
crop: 301,122,1958,1356
-->
{% include figure.html src="/assets/manual/themes/custom-theme.png" alt="Preferences on Theme with a theme of your own chosen, showing its colour rows" caption="A theme of your own." %}

A swatch opens the colour chooser. The colour applies as you pick it. **Rename** changes the name only. **Delete** asks first (*Charts go back to System*) and puts the window on System, or on Midnight without Omarchy.

| Group | Row | Colours |
|---|---|---|
| Bars | **Up outline**, **Up body** | A rising candle's edge and body. A body can be made transparent. A fully transparent body gives a hollow candle. |
| Bars | **Down outline**, **Down body** | The same for a falling candle. |
| Bars | **No change** | A bar that closed where it opened. |
| Bars | **Monochrome** | Every bar, when a chart's **Colouring** is Monochrome. Until set, it follows the theme's text colour. |
| Backgrounds | **Chart background** | The chart, and lists beside it. |
| Backgrounds | **Panels** | The window, the sidebar, dialogs. |
| Backgrounds | **Toolbars** | Headers, popovers, cards. |
| Text | **Text** | All text. |
| Text | **Secondary text** | Subtitles, hints and other quieter labels. |
| Palette | **Blue**, **Amber**, **Violet**, **Teal**, **Rose**, **Green**, **Orange**, **Cyan** | The colours indicators and drawings pick from ([The palette](#the-palette)). |
| Other | **Borders** | Separators and outlines. |
| Other | **Grid lines** | The chart's gridlines. |
| Other | **Axes** | The axis rules. |
| Other | **Crosshair** | The crosshair. |
| Other | **Accent** | A selected row, a switch that is on, the button a dialog suggests. |

Volume bars take the candle colours, dimmed toward the background. Filled, hollow, red-up or monochrome bars are the chart's **Colouring**, not the theme's ([Colouring](/manual/charts/#colouring)).

## The palette

The palette is the theme's named colours for indicators and drawings. An indicator or drawing on a palette colour changes with the theme.

An indicator added without a colour gets the next of Blue, Amber, Violet, Teal, Orange and Cyan not already on the chart. After six they repeat. Green and Rose are never given automatically, but you can pick them.

### The palette picker

A colour button in the indicator and drawing dialogs opens the theme's palette first.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, bin/headless-window start --size 2560x1600 (fresh session, HOME sandboxed)
state:
  move 1270 790
  omacharts chart set --symbol NVDA --resolution 1D
  omacharts chart indicator add sma --period 50 --color Amber
  key ctrl+i
  click 480 456          # the SMA 50 row
  click 849 392          # its Colour swatch
  move 1270 790
capture: bin/headless-window shot /tmp/picker.png
crop: 750,656,1206,520   (the Line group and the palette, 16px layout margin round both)
-->
{% include figure.html src="/assets/manual/themes/palette-picker.png" alt="The palette picker open under an indicator's Colour row, with the colour in use ringed and Custom… below" caption="The palette picker." %}

| Choice | When the theme changes |
|---|---|
| A swatch | The colour changes with the theme. |
| **Custom…** | Opens the colour chooser. The colour picked there does not change with the theme. |

## The Omarchy plugin's colours

The Omarchy plugin takes its colours from the desktop theme, even when the window is on another theme ([Omarchy plugin](/manual/bar-widget/#what-it-shows)).

## See also

- [The `theme` setting in the CLI](/manual/cli/#settings)
