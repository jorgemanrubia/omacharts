---
title: "Watchlists"
description: "Lists of symbols in the sidebar, and which charts a click in them moves."
section: charting
nav_order: 22
---

A watchlist is a list of symbols in the sidebar, on the right of the window.
Click a symbol, or step to it with an arrow key, and the charts linked to the
list show it. Each [chartbook](/manual/chartbooks/) shows one watchlist. Which
charts a click moves depends on each chart's
[link mode](#linking-charts-to-the-watchlist).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  omacharts watchlist create Semis
  omacharts watchlist add Semis NVDA AMD AVGO
  omacharts section create Semis Memory
  omacharts watchlist add Semis MU WDC STX --section Memory
  omacharts section create Semis Equipment
  omacharts watchlist add Semis AMAT LRCX KLAC --section Equipment
  omacharts chartbook create Semis --symbol NVDA --switch
  omacharts chartbook watchlist Semis Semis
  click 300 300; key Down ×8, 2.5 s apart; key Up ×8   # every row gets a price
  omacharts watchlist sort Semis change_pct --descending
  move 1279 719
capture: bin/headless-window shot sidebar.png
crop: 2000,0,560,800
-->
{% include figure.html src="/assets/manual/watchlists/sidebar-sorted.png" alt="The sidebar showing the Semis watchlist, sorted by Chg % with a down arrow on that column, with the Memory and Equipment sections below the first symbols" caption="The *Semis* watchlist, sorted by **Chg %** high to low within each section." %}

## The sidebar

<kbd>Ctrl</kbd> + <kbd>B</kbd> shows or hides the sidebar and puts the
keyboard in it ([The window](/manual/window/)). Drag its edge to make it
wider or narrower. Whether it is open, and how wide, is saved with each
chartbook.

Across its top are the button for the
[page of watchlists](#the-page-of-watchlists), the list's name, and **+**,
with **Add a symbol** and **Add a section**. An empty list offers
the same two, with their keys.

A row shows a dash until it has a price. When no price can be shown (the feed
refused, or there is no connection), a line above the first dash says why.

## Columns

| Column | Shows |
|---|---|
| **Symbol** | The ticker, always first. Hover a row for the instrument's full name. |
| **Last** | The last price. |
| **Chg %** | The move since the previous close, as a percentage. |

Drag a column's title along the row to move it. **Symbol** doesn't move.

## Sorting

Click a column's title to sort by it, low to high. Click again for high to
low, and a third time for the order the list was written in. An arrow beside the
title shows which, and its tooltip says what the next click does.

Sorting is within each section. The list re-sorts as prices move. A symbol with no price yet sorts last either
way. The sort is saved with the watchlist. While a list is sorted, dragging a
symbol within its section does nothing; dragging it into another section
still moves it.

## Symbols

| To | Do |
|---|---|
| Add one | <kbd>Ctrl</kbd> + <kbd>N</kbd> in the sidebar, or **+** › **Add a symbol**. The [symbol search](/manual/symbols/) opens, and the pick lands at the end of the highlighted row's section. |
| Remove one | <kbd>Delete</kbd> on the highlighted row, or right-click it › **Remove**. The highlight stays where it was. |
| Move one | Drag it to another place in its section, or into another section. Dropped on a section's name, it joins the end of that section. That is the way into a folded or empty one. |

A list may hold the same symbol in two sections. Adding a symbol a section
already has changes nothing.

## Sections

A section is a named group inside a list. Symbols in no section have no
header and sit at the top.

| To | Do |
|---|---|
| Add a section | <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>N</kbd> in the sidebar, or **+** › **Add a section**. Type the name, <kbd>Enter</kbd>. |
| Fold or unfold it | Click the arrow before its name, or <kbd>Enter</kbd> / <kbd>Space</kbd> with the keyboard on its name. The arrow keys skip a folded section's symbols. |
| Rename it | Double-click its name, type, <kbd>Enter</kbd>. Leaving the box without <kbd>Enter</kbd> keeps the old name. |
| Move it | Drag its name above or below another section. |
| Its menu | Right-click its name. |

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (as for the first picture on this page, without the sort)
  move 600 300
  bin/virtual-pointer --button right click 1080 176   # the Memory section's name
capture: bin/headless-window shot section.png
crop: 2000,300,560,400
-->
{% include figure.html src="/assets/manual/watchlists/section-menu.png" alt="A section's menu over the sidebar: Add symbol… Ctrl+N, Turn into a watchlist, and below a line Remove section…" caption="A section's menu." %}

| Menu item | Does |
|---|---|
| **Add symbol…** | Opens the symbol search, titled *Add to Memory*. The symbol lands at the end of the section, which unfolds if it was folded. |
| **Turn into a watchlist** | Makes a watchlist named after the section, moves its symbols into it, removes the section and shows the new list. It doesn't ask. |
| **Remove section…** | Removes the section and the symbols in it, after asking. |

## The page of watchlists

The button at the top-left of the sidebar opens the page of every watchlist
in place of the symbols. The same button, or <kbd>Esc</kbd>, goes back.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture in Chartbooks)
  click 1020 24                   # the page of watchlists
  move 1140 108                   # the pointer on Semis
capture: bin/headless-window shot lists.png
crop: 2000,0,560,340
-->
{% include figure.html src="/assets/manual/watchlists/switcher.png" alt="The page of watchlists: Default with 18 symbols, Semis with 9 under the pointer showing a pencil and a bin, and Metals dimmed with Commodities written beside it" caption="The page of watchlists, with *Metals* dimmed and *Commodities* written beside it." %}

| To | Do |
|---|---|
| Open a list | Click it, or <kbd>Enter</kbd> on it. The sidebar shows its symbols and the open chartbook takes it. No chart moves until you pick a symbol. |
| Make a list | **+** on this page. Type the name in the row that appears, <kbd>Enter</kbd>, and the new list opens. <kbd>Esc</kbd> or clicking away makes nothing. |
| Rename a list | The pencil that shows while the pointer is on the row, or right-click › **Rename**. Type, <kbd>Enter</kbd>. |
| Delete a list | The bin, or right-click › **Delete…**. It asks first, *Delete "Semis" and the 9 symbols in it?*, with **Cancel** the default. |

Each row counts the list's symbols. A list another chartbook is using is
dimmed, names that chartbook, and can't be opened here
([Chartbooks](/manual/chartbooks/#the-chartbooks-watchlist)).

The default watchlist is the one a fresh install calls *Default*. It can be
renamed and emptied but not deleted, and has no bin. Any number of chartbooks
may show it. A chartbook whose list is deleted falls back to it. The
[Omarchy plugin](/manual/bar-widget/) shows it.

## Keys

In the sidebar:

| Key | Does |
|---|---|
| <kbd>↑</kbd> <kbd>↓</kbd> | The previous or next symbol, as a click would pick it, stepping over the names of open sections. At either end it stays put. |
| <kbd>Ctrl</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | The first symbol of the previous or next section. |
| <kbd>Enter</kbd> / <kbd>Space</kbd> | On a section's name, folds or unfolds it. |
| <kbd>Ctrl</kbd> + <kbd>N</kbd> | Adds a symbol to the highlighted row's section. |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>N</kbd> | Adds a section. |
| <kbd>Delete</kbd> | Removes the highlighted symbol from the list. |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | The previous or next watchlist, round the ends. Only in the sidebar; over a chart the same keys resize it. |
| <kbd>Esc</kbd> | Back to the chart. On the page of watchlists, back to the list's symbols. |

From anywhere:

| Key | Does |
|---|---|
| <kbd>↑</kbd> <kbd>↓</kbd> / <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | Over a chart, the previous or next symbol in the list, without going to the sidebar. It puts the symbol on the focused chart, as typing it would ([Link modes](#link-modes)). A selected drawing takes <kbd>↑</kbd> <kbd>↓</kbd> first, to nudge it. |
| <kbd>Ctrl</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Shows watchlist N, counting down the page of watchlists. |

## Linking charts to the watchlist

A link mode is a chart's setting for what a click in the watchlist does to
it. Each chart has its own. Every chart starts as **Follow**.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture in Chartbooks, then)
  click 915 23                    # the 1h chart's chain
  click 857 140                   # Pinned
  omacharts chart set --chart pos:1 --symbol SPY
  click 1045 112                  # AMD in the sidebar
  move 760 520                    # the pointer on the 15m chart
capture: bin/headless-window shot linked.png, then magick -resize 2000x -quality 88
crop: none
-->
{% include figure.html src="/assets/manual/linking/follow-and-pinned.jpg" alt="The Semis chartbook after a click on AMD: the daily and 15-minute charts show AMD, the pinned hourly chart still shows SPY, and the pointer on the 15-minute chart draws a matching line on the daily chart" caption="After a click on AMD in the sidebar. The two Follow charts show AMD; the pinned chart still shows SPY." wide="true" %}

### Link modes

| Mode | A click in the watchlist | A symbol typed on the chart |
|---|---|---|
| **Follow** | Always moves it. The Follow charts all show one symbol. | Moves it and every other Follow chart. |
| **Focus** | Moves it while it is the focused chart. | Moves it alone. |
| **Pinned** | Never moves it, focused or not. | Moves it alone. |

**Follow** from the chain or the chart menu puts this chart's symbol on every
other Follow chart and highlights it in the watchlist. Switching to Follow with <kbd>Ctrl</kbd> +
<kbd>L</kbd> works the other way round. The chart takes the symbol the other
Follow charts show. Nothing moves when:

- Leaving Follow. The charts still following keep what they had.
- Setting the mode a chart is already in.
- Following from a chart with no symbol. The Follow charts keep what they
  were showing.

When the focused chart is pinned and no chart follows, a click in the sidebar
moves nothing. The pin on that chart lights for a moment and the sidebar's
highlight goes back to the symbol the chart shows.

A click in a watchlist moves charts in its own chartbook only.

### Choosing a link mode

The chain in a chart's top-right corner shows the mode. One link is Focus,
two links are Follow and a pin is Pinned. Follow is drawn dim, Focus and
Pinned in the theme's accent colour.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  move 1270 710
  omacharts chart set --symbol NVDA --resolution 1h
  omacharts chart split horizontal
  omacharts chart set --chart pos:1 --resolution 1D
  omacharts chart focus pos:0
  move 415 24
  click 415 23                    # the 1h chart's chain (left chart, so the menu opens over the chart beside it, not the sidebar)
capture: bin/headless-window shot chain.png
crop: 0,0,1150,380
-->
{% include figure.html src="/assets/manual/linking/chain-menu.png" alt="The chain's menu on the hourly chart: Follow, Always follows watchlist, ticked; Focus, Follows watchlist while focused; Pinned, Never follows watchlist" caption="The chain's menu, open on a Follow chart." %}

| Where | How |
|---|---|
| The chain | Click it for **Follow**, **Focus** and **Pinned**, the current one ticked. Clicking another chart's chain focuses that chart first. |
| The chart menu | Right-click the chart › **Watchlist** › **Follow**, **Focus**, **Pinned** ([Charts](/manual/charts/)). |
| <kbd>Ctrl</kbd> + <kbd>L</kbd> | Moves the focused chart to its next mode: Follow, Focus, Pinned and round to Follow. |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>L</kbd> | The same, the other way round. |

The chain appears once there is a second chart. With one chart, Focus and
Follow do the same thing. A lone chart shows the chain only while it is
pinned.

### The crosshair across Follow charts

The pointer on a Follow chart draws the same moment on the other Follow
charts, at their own resolutions. Focus and Pinned charts are left alone.
**Crosshair across Follow charts** in [Chart settings](/manual/chart-settings/)
(<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>S</kbd>) turns it off or on for the
whole window. It is on by default.

## See also

- [Managing watchlists with the CLI](/manual/cli/#watchlists-and-sections), including export and import
- [Linking charts from the CLI](/manual/cli/#linking)
