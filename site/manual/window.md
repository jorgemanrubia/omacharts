---
title: "The window"
description: "The chart, its header and legend, the corners, the sidebar, the drawing tools, the chartbook strip, the menus and the dialogs."
section: charting
nav_order: 12
---

The chart fills the window. The sidebar on the right, the drawing tools on the left and the chartbook strip along the bottom can each be hidden, leaving the chart, its header and its scales.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook rename "Chartbook 1" Desk
  omacharts chartbook create Macro --symbol SPY
  omacharts chartbook create Metals --symbol GC
  omacharts chart set --symbol NVDA --resolution 1D
  omacharts chart indicator add sma --period 50 --color Blue
  omacharts config set chart_header symbol,resolution,change,template
  key ctrl+d
  move 1279 400
capture: bin/headless-window shot /tmp/anatomy.png
crop: none (resized to 2000 wide)
-->
{% include figure.html src="/assets/manual/window/anatomy.jpg" alt="The window with the drawing tools on the left, an NVDA daily chart, the sidebar on the right and three chartbook tabs along the bottom" caption="One chart, with the drawing tools and the sidebar out and three chartbooks in the strip." %}

## Parts of the window

| Part | Where | What it is |
|---|---|---|
| The chart | Everything not taken by the sidebar, the tools or the strip | One or more charts, split as you arranged them. See [Charts](/manual/charts/) and [Splitting and arranging charts](/manual/layouts/). |
| The header | Top-left of each chart | The chart's symbol and resolution, and optionally the day's change and its chart template. Click the symbol to open the [symbol search](/manual/symbols/); click the resolution for the list of [resolutions](/manual/charts/#resolutions). What it shows is [one setting for every chart](/manual/charts/#the-header). |
| The indicator legend | Under the header, on the focused chart only | One row per indicator. See [Indicators](/manual/indicators/). |
| The resolution strip | Centred across the top of the focused chart | The preset resolutions as buttons. Hidden when the chart is too narrow for it. The resolution in the header offers the same list. |
| The chart's corner | Top-right of each chart, beside the price axis | The gear (**Chart settings**), the **Maximize** button and the link button. See [The corners](#the-corners). |
| The window's corner | Top-right of the window | The watchlist toggle and the main menu. |
| The sidebar | Right edge | The open chartbook's watchlist. See [Watchlists](/manual/watchlists/). |
| The drawing tools | Left edge | One column of tools. See [Drawings](/manual/drawings/). |
| The chartbook strip | Bottom edge, under the sidebar too | One tab per chartbook. Shown when there are two chartbooks or more. See [Chartbooks](/manual/chartbooks/). |

The sidebar and the tools push the charts aside rather than covering them. Drag the sidebar's edge to make it between 160 and 900 pixels wide. The charts keep at least 480. Whether the sidebar is shown and how wide it is are kept with each chartbook. Whether the tools are out is kept for the window.

## The corners

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the anatomy state above, then)
  omacharts chart split horizontal
  omacharts chart set --link focus
  move 950 250
capture: bin/headless-window shot /tmp/corner.png
crop: 1060,0,1500,170
-->
{% include figure.html src="/assets/manual/window/corner.png" alt="A chart's top-right corner with its gear, Maximize and link buttons, and beyond the price axis the sidebar's header with the watchlist toggle and the main menu" caption="A chart's corner and, over the sidebar, the window's corner." %}

| Button | Shows | Does |
|---|---|---|
| Gear | While the pointer is on the chart | Opens **Chart settings** for that chart. See [Chart settings](/manual/chart-settings/). |
| **Maximize** (two diagonal arrows) | While the pointer is on the chart and the chartbook has two charts or more | Gives the chart the whole window, or puts it back. The arrows point inward while it is maximised. See [Maximising a chart](/manual/layouts/#maximising-a-chart). |
| Link button | When the chartbook has two charts or more, or always on a Pinned chart | Opens the chart's link modes. One link on a Focus chart, only under the pointer; a chain on a Follow chart and a pin on a Pinned chart, always shown and in the accent colour. See [Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist). |
| Watchlist toggle | Always, in the window's corner | Shows or hides the sidebar, like <kbd>Ctrl</kbd> + <kbd>B</kbd>. The key also puts the keyboard in the sidebar, and gives it back to the chart when hiding it. Lit while the sidebar is out. |
| Main menu | Always, in the window's corner | Opens the [main menu](#the-main-menu). <kbd>F10</kbd> opens it too. |
| Drawing tools' toggle | Always, in the window's top-left corner | Shows or hides the tools, like <kbd>Ctrl</kbd> + <kbd>D</kbd>. |

Clicking another chart's gear or corner focuses that chart first. A chart's buttons hide when the keyboard moves the focus to another chart, and come back when the pointer moves. With the sidebar closed, the window's corner sits over the top-right chart, whose buttons move left.

The blank space in both window corners is the window's handle. Drag it to move the window, double-click it to maximise the window, and right-click it for the desktop's window menu, where the desktop has one.

## The focused chart

One chart at a time is the focused one. It has a thin ring in the accent colour when the chartbook has more than one chart. Keys and the menus act on it. Clicking anywhere on a chart, with any button, focuses it and gives it the keyboard. <kbd>Alt</kbd> + an arrow moves the focus to the chart in that direction ([Splitting and arranging charts](/manual/layouts/#focusing-a-chart)). Only the focused chart shows its resolution strip and its indicator legend.

<kbd>Esc</kbd> puts down a drawing tool in hand, clears a selection and gives the keyboard back to the focused chart from wherever it is. In a dialog it closes the dialog ([Dialogs](#dialogs)).

## The main menu

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (the corner state above, sidebar open)
  key F10
capture: bin/headless-window shot /tmp/menu.png
crop: 1052,0,1508,340   (from the focused chart's border; the menu sits against the window's right edge, so it ends there)
-->
![The main menu open under the window's corner, with New chartbook, Preferences and Keyboard Shortcuts](/assets/manual/window/main-menu.png)

| Item | Key | Does |
|---|---|---|
| **New chartbook** | <kbd>Ctrl</kbd> + <kbd>N</kbd> | Opens the **New chartbook** dialog. See [Chartbooks](/manual/chartbooks/). |
| **Preferences** | <kbd>Ctrl</kbd> + <kbd>,</kbd> | Opens Preferences. See [Preferences](/manual/preferences/). |
| **Keyboard Shortcuts** | <kbd>?</kbd> | Opens the Keyboard shortcuts sheet. |

## The Keyboard shortcuts sheet

<kbd>?</kbd> or <kbd>Ctrl</kbd> + <kbd>?</kbd> opens a sheet of the window's keys, grouped as Finding things, Chartbooks, Watchlist, Charts, Chart and Drawing. The search box at the top has the keyboard when the sheet opens.

Type a word or a key, in any order. `split` and `ctrl alt v` both find the split, `watchlist` finds the sidebar's keys, and arrows are `left`, `right`, `up` and `down`. When nothing matches, the sheet says **No shortcut matches that**.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (any chart, sidebar hidden, so no sidebar letters show beside the dialog)
  key question
  type "ctrl alt"
capture: bin/headless-window shot /tmp/sheet.png
crop: 490,68,1580,1304   (the whole dialog, 16px layout margin above and below, chart either side)
-->
{% include figure.html src="/assets/manual/window/shortcuts-sheet.png" alt="The Keyboard shortcuts sheet with ctrl alt typed, listing the chartbook, watchlist and split keys that use it" caption="The sheet filtered to the rows that use Ctrl+Alt." %}

[Hotkeys](/manual/hotkeys/) lists the same keys and the mouse gestures. A button's tooltip names its key in brackets, as in **Watchlist (Ctrl+B)**.

## Dialogs

| Key or gesture | In a dialog |
|---|---|
| <kbd>Esc</kbd> | Closes it. In a dialog with several pages, such as Preferences, goes back a page first. In a search box, closes the dialog rather than clearing the box. |
| <kbd>Ctrl</kbd> + <kbd>Enter</kbd> | Finishes a dialog that makes something (**New chartbook**, adding an indicator, a drawing's **Properties**) from wherever the keyboard is in it. The button it stands for says so in its tooltip. Plain <kbd>Enter</kbd> takes the highlighted row of a list instead. |
| A click outside | Closes the pickers: the symbol search, **Add an indicator** and the Keyboard shortcuts sheet. A dialog you fill in stays open. |
| Typing | Goes to the search box. Every picker opens with the keyboard in its box. |
| <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> | Moves between the tabs of a dialog that has them (**Chart settings** and a drawing's **Properties**), wrapping at either end. |

## Quitting

Close the window as you close any other in Omarchy, with <kbd>Super</kbd> + <kbd>W</kbd>. There is nothing to save, and the window opens again as it was left. The [Omarchy plugin](/manual/bar-widget/) stays in the bar.

## See also

- [Which chart a command acts on](/manual/cli/#which-chart-a-command-acts-on)
