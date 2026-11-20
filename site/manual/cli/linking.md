---
title: "Linking"
description: "A chart's link mode: whether it follows the symbol picked in the watchlist."
section: agents
parent: cli
nav_order: 12
---

A chart's link mode says whether it follows the symbol picked in the
watchlist. What each mode does is in [Linking charts to the
watchlist](/manual/watchlists/#linking-charts-to-the-watchlist).

`chart set --link MODE` takes `focus`, `follow` or `pinned`. New charts are `focus`. `chart list`
prints each chart's mode in its last column.

```
$ omacharts chart set --chart pos:1 --link pinned --symbol SPY
chart 68: symbol SPY, link pinned
  [exit 0]

$ omacharts chart set --chart pos:2 --link focus
chart 71: link focus
  [exit 0]

$ omacharts chart set --chart pos:2 --link follow
chart 79: link follow; 2 other Follow charts now show AMD
  [exit 0]
```

`chart set --symbol` on a Follow chart changes that one chart only. To put a
symbol on every Follow chart, click it in the sidebar, type it in the search,
or run `omacharts SYMBOL`.

`omacharts SYMBOL` exits `5` when the focused chart is pinned and no chart
follows ([Launching the app](/manual/cli/#launching-the-app)):

```
$ omacharts NVDA
omacharts: NVDA not shown; the focused chart is pinned and no chart follows the watchlist
  [exit 5]
```
