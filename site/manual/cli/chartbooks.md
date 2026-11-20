---
title: "Chartbooks"
description: "Creating, switching, renaming and deleting chartbooks."
section: agents
parent: cli
nav_order: 10
---

A chartbook is a saved screen: its charts, how they are split, and the
watchlist beside them. The strip and what a chartbook keeps are in
[Chartbooks](/manual/chartbooks/).

Names and `id:N` work as under [Naming things](/manual/cli/#naming-things); a chartbook's
`N` is its place in the strip, counting from 0. A place changes when a
chartbook before it is removed or a tab is dragged, so a script should use
names.

| Command | Does |
|---|---|
| `chartbook list` | Every chartbook, its number of charts and its watchlist; `*` marks the open one. `--json` adds `sidebar`, whether it shows the sidebar. |
| `chartbook show [BOOK]` | One chartbook's charts: id, symbol, resolution, bar style and link mode; `*` marks the focused chart. Default: the open one. |
| `chartbook create NAME` | A chartbook with one chart, showing the default watchlist. `--symbol SYMBOL` (default `SPY`), `--template TEMPLATE` ([Chartbook templates](/manual/cli/chartbook-templates/)), `--switch` to open it. Give it another list with `chartbook watchlist`. |
| `chartbook rename BOOK NAME` | Renames it. |
| `chartbook delete BOOK` | Removes it and its charts. The only chartbook is refused, exit `5`. |
| `chartbook switch BOOK` | Opens it. |
| `chartbook watchlist BOOK LIST` | Gives it a watchlist. A list another chartbook shows is refused, exit `5`, naming the book. |
| `chartbook duplicate [BOOK]` | Copies it beside itself. `--name NAME` (default: the name and *copy*), `--switch` to open the copy. Default: the open one. |

<!-- TODO(manual): `chartbook create … --watchlist LIST` is in the surface
but refused with exit 2 at v0.1.22, so the table above leaves it out: the launch option `--watchlist` is peeled off the
command line before the command sees it ("--watchlist says which list to
open a symbol in; it does not apply to commands"). The examples below use
`chartbook watchlist` instead. doc/cli.md's example has the same problem. -->

```
$ omacharts chartbook create Semis --symbol NVDA --switch
created chartbook "Semis" (id:1) showing NVDA
  [exit 0]

$ omacharts chartbook watchlist Semis Semis
chartbook "Semis" now shows watchlist "Semis"
  [exit 0]

$ omacharts chartbook list
  id:0 Overview                  1 charts  watchlist "Default"
* id:1 Semis                     1 charts  watchlist "Semis"
  [exit 0]

$ omacharts chartbook create Macro
created chartbook "Macro" (id:2) showing SPY
  [exit 0]

$ omacharts chartbook watchlist Macro Semis
omacharts: chartbook "Semis" already shows "Semis"; a watchlist belongs to one
  [exit 5]

$ omacharts chartbook duplicate Semis --name "Semis weekly"
duplicated chartbook "Semis" as "Semis weekly" (id:2)
  [exit 0]
```
