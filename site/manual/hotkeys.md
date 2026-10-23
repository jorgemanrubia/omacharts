---
title: "Hotkeys"
description: "Every key and mouse gesture the window answers to, grouped by where you are when you press it."
section: appendices
nav_order: 33
---

A key goes to whatever has the keyboard when you press it, whether the
chart, the sidebar, a dialog or a drawing you are typing in.

## The Keyboard shortcuts sheet

The Keyboard shortcuts sheet (<kbd>?</kbd>) lists the keys inside the
window, with a search box
([The window](/manual/window/#the-keyboard-shortcuts-sheet)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  (any chartbook, sidebar hidden)
  key question
  move 1270 710
capture: bin/headless-window shot sheet.png
crop: 768,68,1024,1304   (the whole dialog, 16px layout margin; the list scrolls inside it)
-->
{% include figure.html src="/assets/manual/hotkeys/shortcuts-sheet.png" alt="The Keyboard shortcuts sheet, open on the Finding things group" caption="The sheet, open on its first group." %}

Some menus and tooltips write the modifiers in another order (Shift+Ctrl+N
rather than Ctrl+Shift+N).

The sheet leaves out <kbd>Ctrl</kbd> + <kbd>F</kbd> and <kbd>/</kbd>, the
keys inside dialogs and while typing on the chart, and the mouse. The
Omarchy plugin's panel has keys of its own, in
[Omarchy plugin](/manual/bar-widget/#using-it).

## Finding things

| Key | Does | See |
|---|---|---|
| Type a letter | Opens the symbol search with that letter in it | [Symbols](/manual/symbols/) |
| <kbd>Ctrl</kbd> + <kbd>K</kbd> / <kbd>Ctrl</kbd> + <kbd>F</kbd> / <kbd>/</kbd> | Opens the symbol search empty | [Symbols](/manual/symbols/) |
| Type a number | Opens the resolution box with that digit in it. A bare number is minutes | [Charts](/manual/charts/) |
| <kbd>Ctrl</kbd> + <kbd>I</kbd> | The focused chart's indicators | [Indicators](/manual/indicators/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>I</kbd> | The list of indicators to add | [Indicators](/manual/indicators/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>S</kbd> | The focused chart's settings | [Chart settings](/manual/chart-settings/) |
| <kbd>Ctrl</kbd> + <kbd>,</kbd> | Preferences | [Preferences](/manual/preferences/) |
| <kbd>F10</kbd> | The main menu | [The window](/manual/window/) |
| <kbd>?</kbd> / <kbd>Ctrl</kbd> + <kbd>?</kbd> | The Keyboard shortcuts sheet | [The window](/manual/window/) |

## Chartbooks

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>N</kbd> | **New chartbook**, while the keyboard is not in the sidebar | [Chartbooks](/manual/chartbooks/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>R</kbd> | Renames the open chartbook, in its tab | [Chartbooks](/manual/chartbooks/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Alt</kbd> + <kbd>X</kbd> | Removes the open chartbook, after asking. The last one cannot be removed | [Chartbooks](/manual/chartbooks/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> | The previous or next chartbook | [Chartbooks](/manual/chartbooks/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>O</kbd> | A screenshot of the open chartbook, to the clipboard | [Screenshots](/manual/screenshots/) |

## Watchlist

With the keyboard in the sidebar:

| Key | Does | See |
|---|---|---|
| <kbd>↑</kbd> <kbd>↓</kbd> | The previous or next symbol, stepping over the names of open sections | [Watchlists](/manual/watchlists/) |
| <kbd>Ctrl</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | The previous or next section | [Watchlists](/manual/watchlists/) |
| <kbd>Enter</kbd> / <kbd>Space</kbd> | On a section's name, folds or unfolds it | [Watchlists](/manual/watchlists/) |
| <kbd>Ctrl</kbd> + <kbd>N</kbd> | Adds a symbol to the section of the highlighted row | [Watchlists](/manual/watchlists/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>N</kbd> | Adds a section | [Watchlists](/manual/watchlists/) |
| <kbd>Delete</kbd> | Removes the highlighted symbol from the list | [Watchlists](/manual/watchlists/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | The previous or next watchlist. Over a chart the same keys resize it | [Watchlists](/manual/watchlists/) |
| <kbd>Esc</kbd> | Back to the chart. On the page of watchlists, back to the list's symbols | [Watchlists](/manual/watchlists/) |

From anywhere:

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>B</kbd> | Shows the sidebar with the keyboard in it, or hides it and gives the keyboard back to the chart | [The window](/manual/window/) |
| <kbd>↑</kbd> <kbd>↓</kbd> / <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | Over a chart, the previous or next symbol in the watchlist, without going to it | [Watchlists](/manual/watchlists/) |
| <kbd>Ctrl</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Shows watchlist N, counting down the page of watchlists | [Watchlists](/manual/watchlists/) |

## Arranging charts

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>H</kbd> | Splits the focused chart horizontally. The new chart is a copy | [Layouts](/manual/layouts/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>V</kbd> | Splits it vertically | [Layouts](/manual/layouts/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>X</kbd> | Closes the focused chart | [Layouts](/manual/layouts/) |
| <kbd>Ctrl</kbd> + <kbd>M</kbd> | Gives the focused chart the whole window, or puts it back | [Layouts](/manual/layouts/) |
| <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> | Focuses the chart in that direction | [Layouts](/manual/layouts/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> | Grows the focused chart in that direction, by moving the divider it sits against | [Layouts](/manual/layouts/) |
| <kbd>Ctrl</kbd> + <kbd>L</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>L</kbd> | The focused chart's next or previous link mode: Focus, Follow, Pinned | [Watchlists](/manual/watchlists/#choosing-a-link-mode) |
| <kbd>Ctrl</kbd> + <kbd>O</kbd> | A screenshot of the focused chart, to the clipboard | [Screenshots](/manual/screenshots/) |

## The focused chart

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> | The previous or next resolution on the strip | [Charts](/manual/charts/) |
| <kbd>←</kbd> <kbd>→</kbd> | Pans five bars. With a drawing selected, nudges it instead | [Charts](/manual/charts/) |
| <kbd>+</kbd> <kbd>−</kbd> | Zooms in or out | [Charts](/manual/charts/) |
| <kbd>End</kbd> | Jumps to the latest bar | [Charts](/manual/charts/) |
| <kbd>Ctrl</kbd> + <kbd>Esc</kbd> | Resets zoom, pan and price scale to how the chart opens | [Charts](/manual/charts/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>G</kbd> | Shows or hides the gridlines | [Charts](/manual/charts/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>B</kbd> | Hides the bars, or shows them again in the style they had | [Hiding the bars](/manual/charts/#hiding-the-bars) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Chart template N, in the order they are listed | [Chart templates](/manual/chart-templates/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>0</kbd> | No chart template | [Chart templates](/manual/chart-templates/) |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>PgUp</kbd> <kbd>PgDn</kbd> | The previous or next chart template | [Chart templates](/manual/chart-templates/) |
| <kbd>Esc</kbd> | Back to the chart from wherever the keyboard is. On the chart, deselects the tool and clears the selection | [The window](/manual/window/) |

The template keys are the number row, whatever your layout prints on it
with Shift held.

## Drawing

Tools and configurations:

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>D</kbd> | Shows or hides the drawing tools | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>L</kbd> | A line: click where it starts, then where it ends | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>H</kbd> | A horizontal line | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>A</kbd> | An arrow | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>Z</kbd> | A zig-zag: click each corner, double-click or <kbd>Enter</kbd> to end it | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>R</kbd> | A rectangle | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>C</kbd> | A circle, dragged to any shape | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>T</kbd> | Text: click, then type | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Configuration N, for the selected drawing or the one about to be drawn | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd>, held | A point being placed or dragged snaps to the open, high, low or close of the candle under it | [Drawings](/manual/drawings/) |

With drawings selected:

| Key | Does | See |
|---|---|---|
| <kbd>Enter</kbd> / <kbd>F2</kbd> | Puts the caret in the drawing's text | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>Enter</kbd> | The drawing's properties | [Drawings](/manual/drawings/) |
| <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> | Nudges it a pixel. Ten with <kbd>Shift</kbd> | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>↑</kbd> <kbd>↓</kbd> | Brings it to the front, or sends it to the back | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + <kbd>+</kbd> / <kbd>Ctrl</kbd> + <kbd>−</kbd> | Grows or shrinks its text. On a figure, its label | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>+</kbd> / <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>−</kbd> | Thickens or thins its line or edge | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> <kbd>↑</kbd> <kbd>↓</kbd> | Puts its label against that edge | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>.</kbd> | Puts its label back in the middle | [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>A</kbd> | Steps its arrowhead: none, filled, open, barb | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + <kbd>X</kbd> / <kbd>Ctrl</kbd> + <kbd>C</kbd> / <kbd>Ctrl</kbd> + <kbd>V</kbd> | Cuts or copies the selected drawings, or the selected indicator, and pastes them | [Drawings](/manual/drawings/) |
| <kbd>Delete</kbd> / <kbd>Backspace</kbd> | Deletes them | [Drawings](/manual/drawings/) |
| <kbd>Esc</kbd> | Clears the selection | [Drawings](/manual/drawings/) |

With an indicator selected:

| Key | Does | See |
|---|---|---|
| <kbd>Alt</kbd> + <kbd>Enter</kbd> | Its settings | [Indicators](/manual/indicators/#selecting-an-indicator) |
| <kbd>Ctrl</kbd> + <kbd>X</kbd> / <kbd>Ctrl</kbd> + <kbd>C</kbd> / <kbd>Ctrl</kbd> + <kbd>V</kbd> | Cuts or copies it, and pastes it on the focused chart | [Indicators](/manual/indicators/#selecting-an-indicator) |
| <kbd>Delete</kbd> / <kbd>Backspace</kbd> | Removes it | [Indicators](/manual/indicators/#selecting-an-indicator) |
| <kbd>Esc</kbd> | Clears the selection | [Indicators](/manual/indicators/#selecting-an-indicator) |

On the chart, whatever is selected:

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>A</kbd> | Selects every drawing on the plot | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + <kbd>Z</kbd> | Undoes the last change to this chart's drawings | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + <kbd>Y</kbd> / <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> | Redoes it | [Drawings](/manual/drawings/) |
| <kbd>Enter</kbd> | While a zig-zag is being drawn, ends it at the last corner | [Drawings](/manual/drawings/) |

While a box you are typing in has the keyboard, <kbd>Ctrl</kbd> + <kbd>C</kbd>,
<kbd>Ctrl</kbd> + <kbd>V</kbd> and the arrows belong to it.

## Typing on the chart

While the caret is in a drawing:

| Key | Does | See |
|---|---|---|
| <kbd>Enter</kbd> | Keeps what you typed | [Drawings](/manual/drawings/) |
| <kbd>Shift</kbd> + <kbd>Enter</kbd> | A new line | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + <kbd>B</kbd> / <kbd>Ctrl</kbd> + <kbd>I</kbd> | Bold or italic, over the selection or the word the caret is in | [Drawings](/manual/drawings/) |
| <kbd>Esc</kbd> | Leaves the text as it was | [Drawings](/manual/drawings/) |

A click elsewhere keeps what was typed.

## Dialogs and boxes

| Key | Does | See |
|---|---|---|
| <kbd>Ctrl</kbd> + <kbd>Enter</kbd> | Finishes the dialog with the button in its corner: **Create** in New chartbook, **Add** for an indicator, **Done** in a drawing's properties and in a tool's configurations. In the symbol search and the list of indicators, takes the highlighted row | [The window](/manual/window/) |
| <kbd>Enter</kbd> | In a search box, takes the highlighted row | [The window](/manual/window/) |
| <kbd>↑</kbd> <kbd>↓</kbd> | In a search box, moves the highlight while the keyboard stays in the box | [Symbols](/manual/symbols/) |
| <kbd>Esc</kbd> | Closes the dialog. In Preferences, goes back a page first | [The window](/manual/window/) |
| <kbd>Alt</kbd> + <kbd>←</kbd> <kbd>→</kbd> | Switches pages: **Chart** and **Indicators** in Chart settings, **General** and **Text** in a drawing's properties | [Chart settings](/manual/chart-settings/), [Drawings](/manual/drawings/) |
| <kbd>Alt</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | In a drawing's properties, applies configuration N | [Drawings](/manual/drawings/) |
| <kbd>Enter</kbd> / <kbd>Esc</kbd> | In the resolution box, sets the resolution, or closes the box | [Charts](/manual/charts/) |
| <kbd>Enter</kbd> / <kbd>Esc</kbd> | Renaming a tab or a section, keeps the name, or puts the old one back | [Chartbooks](/manual/chartbooks/), [Watchlists](/manual/watchlists/) |

A click outside the symbol search, the list of indicators or the Keyboard
shortcuts sheet closes it. A form (New chartbook, a drawing's properties)
closes only on <kbd>Esc</kbd> or its own buttons.

## Mouse

On a chart:

| Gesture | Does | See |
|---|---|---|
| Click | Focuses the chart | [Layouts](/manual/layouts/) |
| Drag the plot | Pans | [Charts](/manual/charts/) |
| Wheel | Zooms time, about the pointer | [Charts](/manual/charts/) |
| <kbd>Shift</kbd> + wheel | Pans | [Charts](/manual/charts/) |
| <kbd>Ctrl</kbd> + wheel | Scales price | [Charts](/manual/charts/) |
| Trackpad, sideways | Pans, whatever is held | [Charts](/manual/charts/) |
| Wheel or drag over the price axis | Scales price, taking it off automatic | [Charts](/manual/charts/) |
| Drag the time axis | Scales time | [Charts](/manual/charts/) |
| Double-click the price axis | Puts the price scale back on automatic | [Charts](/manual/charts/) |
| Double-click the time axis | Resets the zoom, ending on the latest bar | [Charts](/manual/charts/) |
| Right-click the plot | The chart menu | [Charts](/manual/charts/) |
| Right-click the price axis | **Auto scale price**, **Logarithmic scale**, **Reset chart** | [Charts](/manual/charts/) |
| Click the symbol in the header | The symbol search | [Symbols](/manual/symbols/) |
| Click the resolution in the header | The list of resolutions | [Charts](/manual/charts/) |
| Right-click the header | **Edit header…** | [Charts](/manual/charts/#the-header) |
| Right-click the resolution strip | **Edit resolutions…** | [Charts](/manual/charts/) |
| Double-click an indicator's name in the legend | Its settings. The eye, gear and bin beside it hide it, open its settings, or remove it | [Indicators](/manual/indicators/) |
| The gear, top-right | Chart settings | [Chart settings](/manual/chart-settings/) |
| The corner arrows, top-right | Gives the chart the whole window, or puts it back | [Layouts](/manual/layouts/) |
| The chain, top-right | The link mode: Focus, Follow, Pinned | [Watchlists](/manual/watchlists/#choosing-a-link-mode) |
| An indicator strip's ×, and its chevrons | Removes the strip, or moves it up or down the stack | [Indicators](/manual/indicators/) |
| Drag the edge of an indicator strip | Makes it taller or shorter | [Indicators](/manual/indicators/) |
| Drag the divider between two charts | Resizes them | [Layouts](/manual/layouts/) |

Drawing with the mouse:

| Gesture | Does | See |
|---|---|---|
| Click a tool | Selects it for one drawing | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + click a tool | Keeps the tool selected for the next drawings. <kbd>Esc</kbd> deselects it | [Drawings](/manual/drawings/) |
| Right-click a tool | Its configurations | [Drawings](/manual/drawings/) |
| Press, then press again, or press and drag | Places a drawing between two anchors | [Drawings](/manual/drawings/) |
| Right-click while placing | Abandons the unfinished drawing and keeps the tool | [Drawings](/manual/drawings/) |
| Click a drawing | Selects it. Drag an end, or the whole thing | [Drawings](/manual/drawings/) |
| <kbd>Shift</kbd> + click / <kbd>Ctrl</kbd> + click a drawing | Adds it to the selection, or takes it out | [Drawings](/manual/drawings/) |
| <kbd>Shift</kbd> + drag the empty chart | Selects every drawing the box touches | [Drawings](/manual/drawings/) |
| <kbd>Ctrl</kbd> + drag a corner | Snaps it to the candle under it | [Drawings](/manual/drawings/) |
| Double-click a drawing | Puts the caret in it, to type | [Drawings](/manual/drawings/) |
| Click an indicator, or its name in the legend | Selects it | [Indicators](/manual/indicators/#selecting-an-indicator) |
| Double-click an indicator | Opens its settings | [Indicators](/manual/indicators/#selecting-an-indicator) |
| Right-click an indicator | Selects it and opens its menu: **Cut**, **Copy**, **Paste**, **Remove**, **Properties…** | [Indicators](/manual/indicators/#selecting-an-indicator) |
| Right-click a drawing | Selects it and opens its menu: **Edit text**, **Cut**, **Copy**, **Paste**, **Remove**, **Configuration**, **Thickness**, **Arrowhead**, **Bring to front**, **Send to back**, **Properties…** | [Drawings](/manual/drawings/) |

Around the window:

| Gesture | Does | See |
|---|---|---|
| Click a tab | Opens that chartbook | [Chartbooks](/manual/chartbooks/) |
| Double-click a tab | Renames it | [Chartbooks](/manual/chartbooks/) |
| Drag a tab | Moves it along the strip | [Chartbooks](/manual/chartbooks/) |
| Right-click a tab | **Rename**, **Duplicate**, **Remove**, **Take screenshot**, **Template** | [Chartbooks](/manual/chartbooks/) |
| Click a symbol in the sidebar | Shows it on the charts that take the watchlist | [Watchlists](/manual/watchlists/#linking-charts-to-the-watchlist) |
| Drag a symbol or a section's name | Moves it, within a section or into another | [Watchlists](/manual/watchlists/) |
| Click a column's title | Sorts by it, low to high, then high to low, then the list's own order | [Watchlists](/manual/watchlists/) |
| Drag a column's title | Moves the column | [Watchlists](/manual/watchlists/) |
| Right-click the column titles | **Edit columns…** | [Watchlists](/manual/watchlists/#columns) |
| Click the arrow before a section's name | Folds or unfolds it | [Watchlists](/manual/watchlists/) |
| Double-click a section's name | Renames it | [Watchlists](/manual/watchlists/) |
| Right-click a section's name | **Add symbol…**, **Turn into a watchlist**, **Remove section…** | [Watchlists](/manual/watchlists/) |
| Right-click a symbol in the sidebar | **Remove** | [Watchlists](/manual/watchlists/) |
| Drag the sidebar's edge | Makes it wider or narrower | [Watchlists](/manual/watchlists/) |
| Drag the blank space in the window's corners | Moves the window. Double-click maximises it, right-click opens the window menu | [The window](/manual/window/) |

## See also

- [The Keyboard shortcuts sheet](/manual/window/#the-keyboard-shortcuts-sheet)
