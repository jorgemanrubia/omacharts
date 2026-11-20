---
title: "Chartbook templates"
description: "Saving a chartbook's arrangement as a template, and putting chartbooks on one."
section: agents
parent: cli
nav_order: 11
---

A chartbook template is a chartbook's arrangement saved under a name: the
splits, and each chart's resolution and indicators, without the symbols.
Chartbooks on the same template change together. What following one means
is in [Chartbook templates](/manual/chartbook-templates/).

Names and `id:N` work as under [Naming things](/manual/cli/#naming-things); the ids are in
`chartbook template list` and do not change. `--book BOOK` picks the
chartbook for `save` and `apply`; the default is the open one.

| Command | Does |
|---|---|
| `chartbook template list` | Every template, how many charts it lays out and how many chartbooks use it; `*` marks the open chartbook's. |
| `chartbook template save NAME` | Saves the chartbook as a new template and puts it on it. A name that is blank, already a template's (capitals ignored), `None`, or starts with `id:` is refused, exit `2`. |
| `chartbook template apply TEMPLATE` | Puts the chartbook on the template. `none` takes it off and keeps everything. |
| `chartbook template rename TEMPLATE NAME` | Renames it. |
| `chartbook template delete TEMPLATE` | Deletes it; the chartbooks on it keep what they have. |
| `chartbook create NAME --template TEMPLATE` | A new chartbook on the template from the start, every chart on `--symbol`. |

A change made by a command counts the same as one made in the window. Where
the window asks before replacing an arrangement no template holds, a command
does not ask.

```
$ omacharts chartbook template save Desk
saved chartbook "Semis" as new chartbook template "Desk" (3 charts)
  [exit 0]

$ omacharts chartbook template save desk
omacharts: there is already a chartbook template called "Desk"
  [exit 2]

$ omacharts chartbook create Metals --symbol GLD --template Desk --switch
created chartbook "Metals" (id:2) showing GLD, on chartbook template "Desk": 3 charts
  [exit 0]

$ omacharts chartbook template list
* id:1  Desk                 3 charts  2 chartbooks
  [exit 0]

$ omacharts chartbook template apply none --book Metals
chartbook "Metals" is on no chartbook template; it keeps its layout and indicators
  [exit 0]
```

`chartbook duplicate` keeps the original's template, so the copy is on it
too; see [Chartbooks](/manual/chartbooks/#duplicating-a-chartbook).
