---
title: "Chartbooks"
description: "A saved screen of charts with the watchlist beside them, and the strip of chartbooks along the bottom of the window."
section: charting
nav_order: 19
---

A chartbook is a saved screen: the charts, how they are split, and the
watchlist beside them. Switch to another chartbook and back, and it is as you
left it. Every change is kept as you make it.

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
  omacharts watchlist create Metals
  omacharts watchlist add Metals GLD SLV PPLT
  omacharts chartbook rename "Chartbook 1" Overview
  omacharts chartbook create Semis --symbol NVDA --switch
  omacharts chartbook watchlist Semis Semis
  omacharts chart split horizontal
  omacharts chart set --chart pos:1 --resolution 1h
  omacharts chart split vertical --chart pos:1
  omacharts chart set --chart pos:2 --resolution 15m
  omacharts chart focus pos:0
  omacharts chartbook create Commodities --symbol GLD
  omacharts chartbook watchlist Commodities Metals
  omacharts chartbook create Macro --symbol SPY
  click 300 300; key Down ×8, 2.5 s apart; key Up ×8   # every row gets a price
  move 1279 719
capture: bin/headless-window shot semis.png, then magick -resize 2000x -quality 88
crop: none
-->
{% include figure.html src="/assets/manual/chartbooks/semis-chartbook.jpg" alt="The Semis chartbook: three NVDA charts, the Semis watchlist in the sidebar, and four chartbook tabs along the bottom" caption="The *Semis* chartbook, its watchlist beside it, and the strip of chartbooks along the bottom." wide="true" %}

## The strip

The strip is the row of chartbook tabs along the bottom of the window. It
appears with the second chartbook and goes away when only one is left.

| Gesture | Does |
|---|---|
| Click a tab | Opens that chartbook. |
| Double-click a tab | Renames it in place. <kbd>Enter</kbd> or a click elsewhere keeps the name; <kbd>Esc</kbd> puts the old one back. |
| Drag a tab onto another | Moves it there. |
| Right-click a tab | Opens that chartbook and its menu. |

A chartbook you haven't named is called *Chartbook 1*, *Chartbook 2* and so
on, by its place in the strip. Emptying the name in a rename gives the number
back.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture on this page)
  move 600 300
  bin/virtual-pointer --button right click 104 708   # the Semis tab
capture: bin/headless-window shot tab.png
crop: 0,1020,760,420
-->
{% include figure.html src="/assets/manual/chartbooks/tab-menu.png" alt="A chartbook tab's menu: Rename, Duplicate, Remove, Take screenshot and Template" caption="A tab's menu." %}

| Menu item | Does |
|---|---|
| **Rename** | The same rename a double-click starts. |
| **Duplicate** | A copy beside it, opened, with its name ready to type over. See [Duplicating a chartbook](#duplicating-a-chartbook). |
| **Remove** | Removes the chartbook and its charts, after asking. |
| **Take screenshot** | A picture of the whole chartbook. See [Screenshots](/manual/screenshots/). |
| **Template** | **None**, the chartbook templates, **Save as New Template…** and **Manage Templates…**. See [Chartbook templates](/manual/chartbook-templates/). |

## Keys

| Key | Does |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>N</kbd> | **New chartbook**, while the keyboard is on a chart. In the sidebar the same key adds a symbol. |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>R</kbd> | Renames the open chartbook, in its tab. With one chartbook there is no tab, and nothing happens. |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Alt</kbd> + <kbd>X</kbd> | Removes the open chartbook, after asking. |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> | The previous or next chartbook, round the ends. |

**New chartbook** in the main menu opens the same dialog as
<kbd>Ctrl</kbd> + <kbd>N</kbd> ([The window](/manual/window/)).

## A new chartbook

**New chartbook** asks for a name, a template and a watchlist, then opens
the chartbook with one chart on the symbol the focused chart was showing.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed)
state:
  (the state of the first picture on this page)
  omacharts chartbook template save Desk --book Semis
  click 300 300; move 1279 719
  key ctrl+n
  type Energy
capture: bin/headless-window shot new.png
crop: 890,355,780,730
-->
{% include figure.html src="/assets/manual/chartbooks/new-chartbook.png" alt="The New chartbook dialog, with Energy typed as the name" caption="The **New chartbook** dialog." %}

| Row | Does |
|---|---|
| **Name** | Filled in with the next number and selected, so typing replaces it. |
| **Template** | **None**, or a chartbook template to build the chartbook from and keep it on. Shown only once a template exists. |
| **Watchlist** | The list the sidebar shows. Lists other chartbooks are using are left out. The last choice, **Create a new watchlist**, makes an empty list named after the chartbook when **Create** is pressed. Cancelling leaves nothing behind. |

<kbd>Enter</kbd> in the name, or <kbd>Ctrl</kbd> + <kbd>Enter</kbd>
anywhere, creates it. <kbd>Esc</kbd> cancels.

## The chartbook's watchlist

Each chartbook shows one watchlist, and keeps it when you come back to it. A
list one chartbook is using can't be given to another, except the default
watchlist, which any number of chartbooks may show.

- Removing a chartbook frees its list for another.
- A chartbook whose list is deleted shows the default one.
- Picking another list in the sidebar gives it to the open chartbook. On the
  page of watchlists, a list another chartbook is using is dimmed and names
  that chartbook ([Watchlists](/manual/watchlists/#the-page-of-watchlists)).

## Duplicating a chartbook

**Duplicate** makes a copy of the chartbook beside the original, drawings
and chartbook template included. The copy shows the default watchlist and
opens as *… copy*, with the name ready to type over. The drawings that came with it are left as they were; its **Send drawings
to other charts** switch acts only on drawings made on the copy
([Drawings](/manual/drawings/#sharing-drawings-between-charts)).

## Removing a chartbook

Removing asks first, *Semis and its 3 charts will be removed. This cannot be
undone.*, with **Cancel** as the default. The chartbook beside it opens in
its place. The last chartbook can't be removed.

## See also

- [Managing chartbooks with the CLI](/manual/cli/#chartbooks)
