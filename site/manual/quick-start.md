---
title: "Quick start"
description: "Install, open, put a symbol on screen, split the chart, add an indicator, edit the watchlist, make a second chartbook and run a command, in a few minutes."
section: introduction
nav_order: 11
---

## 1. Install it

Open the Omarchy menu, choose **Install › Package** and pick **omacharts**.

From a terminal, the same thing is:

```sh
omarchy pkg add omacharts
```

Other ways to install, and removing it, are in
[Installation](/manual/installing/).

## 2. Open it

Start Omacharts from the app launcher, or type `omacharts` in a terminal.
The first launch opens a daily chart of SPY, with the **Default** watchlist
in the sidebar on the right.

On an Omarchy desktop the first launch also adds the Omarchy plugin to the
bar, showing the same watchlist ([Omarchy plugin](/manual/bar-widget/)).

## 3. Find a symbol

With the pointer over the chart, type `nv`. The search opens with what you
typed and narrows as you type.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (fresh profile)
  click 400 400
  type nv
  move 960 650           # off the list, so no row is hovered
capture: bin/headless-window shot search.png
crop: 712,244,1136,954   (the search popover, 24px layout margin)
-->
{% include figure.html src="/assets/manual/quick-start/search.png" alt="The symbol search with nv typed and NVDA highlighted at the top" caption="The search with nv typed." %}

<kbd>Enter</kbd> puts NVDA on the chart. <kbd>↑</kbd> and <kbd>↓</kbd> move
the highlight to another row first, and <kbd>Esc</kbd> closes the search
without changing anything. <kbd>Ctrl</kbd> + <kbd>K</kbd> opens the search
empty.

## 4. Set the resolution

Type a number over the chart and the resolution box opens with it. A bare
number is minutes, so `15` <kbd>Enter</kbd> is fifteen minutes. A letter
after it names the unit, as in `4h`, `1D` and `1W`. The strip along the top
of the chart has the common resolutions as buttons, and
<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> and <kbd>→</kbd> step
through them. Type `1D` <kbd>Enter</kbd> to go back to the daily.

## 5. Split it

<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>V</kbd> splits the chart. The new
chart, below, is a copy and has the focus, shown by the ring around it. Type
`1h` <kbd>Enter</kbd> and it becomes the hourly, with the daily above it.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (fresh profile)
  click 400 400
  type nv
  key Return
  key ctrl+alt+v
  type 1h
  key Return
  move 1270 710
capture: bin/headless-window shot split.png
crop: 0,0,2000,1440, resized to 1600 wide
-->
{% include figure.html src="/assets/manual/quick-start/split.jpg" alt="Two NVDA charts stacked, the daily above the hourly, with the focus ring on the hourly" caption="NVDA daily above NVDA hourly, with the focus ring on the hourly." %}

Press <kbd>↓</kbd>. The hourly chart, the focused one, moves to the next
symbol in the watchlist, AAPL, and the daily stays on NVDA. <kbd>↑</kbd>
brings it back. New charts are Focus charts, which change with the watchlist
only while focused. Make both Follow, with <kbd>Ctrl</kbd> + <kbd>L</kbd> on
each, and they change together; a Pinned chart never changes. See
[Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist).

## 6. Add an indicator

<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>I</kbd> opens the list of
indicators. Type `rsi` and press <kbd>Enter</kbd>. The RSI's parameters
open, with the RSI already drawn on the chart behind them.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (as step 5)
  key ctrl+shift+i
  type rsi
  key Return
capture: bin/headless-window shot add-rsi.png
crop: 720,100,1120,1240
-->
{% include figure.html src="/assets/manual/quick-start/add-rsi.png" alt="The RSI dialog with its Parameters and Line sections and the Cancel and Add buttons" caption="The RSI dialog." %}

<kbd>Ctrl</kbd> + <kbd>Enter</kbd> adds it. <kbd>Ctrl</kbd> + <kbd>I</kbd>
lists the chart's indicators, where you can reorder, hide, reconfigure or
remove them. See [Indicators](/manual/indicators/).

## 7. Edit the watchlist

<kbd>Ctrl</kbd> + <kbd>B</kbd> puts the keyboard in the sidebar (press it
twice if the sidebar is already open, since the first press hides it).
<kbd>↑</kbd> and <kbd>↓</kbd> move through the symbols,
<kbd>Ctrl</kbd> + <kbd>↑</kbd> and <kbd>↓</kbd> jump a section, and
<kbd>Enter</kbd> on a section's name folds it.

<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>N</kbd> asks for a section name.
Type `Metals` and press <kbd>Enter</kbd>. Right-click *Metals*, choose
**Add symbol…** and type GLD. Do the same for SLV.
<kbd>Ctrl</kbd> + <kbd>N</kbd> adds a symbol to the section the highlight is
in. <kbd>Delete</kbd> removes the highlighted symbol.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (as step 6)
  key ctrl+b
  key ctrl+b
  key ctrl+shift+n
  type Metals
  key Return
  omacharts watchlist add Default GLD SLV --section Metals
  scroll 1100 500 10
  move 1270 710
capture: bin/headless-window shot sidebar.png
crop: 2000,0,560,1440
-->
{% include figure.html src="/assets/manual/quick-start/sidebar-section.png" alt="The sidebar scrolled to the end of the Default watchlist, with the new Metals section holding GLD and SLV" caption="The *Metals* section at the end of the Default watchlist." %}

<kbd>Esc</kbd> puts the keyboard back on the chart. Columns, sorting,
dragging and keeping several lists are in [Watchlists](/manual/watchlists/).

## 8. Make a second chartbook

With the keyboard on the chart, <kbd>Ctrl</kbd> + <kbd>N</kbd> opens
**New chartbook**. Type `Semis` as the name, set **Watchlist** to
**Create a new watchlist** and press <kbd>Ctrl</kbd> + <kbd>Enter</kbd>.

You are in a second chartbook, with one chart, an empty watchlist called
*Semis*, and a strip along the bottom with a tab for each chartbook. The
first chartbook is unchanged.

## 9. Run a command

Leave the window open and, in a terminal:

```
$ omacharts watchlist add Semis NVDA AMD AVGO
added 3 to "Semis": NVDA AMD AVGO
  [exit 0]
```

The three symbols appear in the sidebar at once.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (as step 7)
  key Escape
  key ctrl+n
  type Semis
  click 715 384     (Watchlist)
  click 742 489     (Create a new watchlist)
  key ctrl+Return
  omacharts watchlist add Semis NVDA AMD AVGO
  move 1270 710
capture: bin/headless-window shot second-chartbook.png
crop: none, resized to 1600 wide
-->
{% include figure.html src="/assets/manual/quick-start/second-chartbook.jpg" alt="The Semis chartbook with its watchlist holding NVDA, AMD and AVGO, and two chartbook tabs along the bottom" caption="The *Semis* chartbook, with both chartbooks in the strip." %}

<kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>←</kbd> goes back to the first
chartbook, as you left it. <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>→</kbd>
returns to *Semis*. Clicking a tab does the same.

Every command has `--help`. See [The CLI](/manual/cli/).

## 10. The Keyboard shortcuts sheet

Press <kbd>?</kbd>. The **Keyboard shortcuts** sheet lists every key. Its
search box takes a word or a key, so `split` and `ctrl alt v` both find the
split. The same keys are in [Hotkeys](/manual/hotkeys/).
