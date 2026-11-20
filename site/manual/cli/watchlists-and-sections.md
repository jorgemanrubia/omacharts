---
title: "Watchlists and sections"
description: "Making, filling, ordering and sorting watchlists and their sections, and moving them between machines."
section: agents
parent: cli
nav_order: 15
---

Names and `id:N` work as under [Naming things](/manual/cli/#naming-things). A command
with no list given acts on the default watchlist.

| Command | Does |
|---|---|
| `watchlist list` | Every watchlist, with its id and how many symbols it holds. |
| `watchlist show [LIST]` | Its sections and symbols. |
| `watchlist create NAME` | An empty watchlist. |
| `watchlist rename LIST NAME` | Renames it. |
| `watchlist delete LIST` | Deletes it and everything in it. The default is refused, exit `5`. |
| `watchlist add LIST SYMBOL…` | Puts symbols in it, at the end. `--section NAME` puts them in a section rather than at the top; `--suffix S` applies a venue suffix to every symbol given ([Symbols](/manual/cli/symbols/)). A ticker that is not an instrument is skipped and named; when none is, exit `3`. |
| `watchlist remove LIST SYMBOL…` | Takes symbols out of the top of the list, or with `--section NAME` out of that section. `--suffix S` as for `add`. |
| `watchlist move LIST SYMBOL` | Moves one symbol. `--section SECTION` into another section, `--before SYMBOL` in front of another, or both; `--from SECTION` when it is in two; `--suffix S`. |
| `watchlist sort LIST [COLUMN]` | Sorts it within each section by `symbol`, `last` or `change_pct`, low to high, or high to low with `--descending`; `none` for its own order. With no column, prints the sort. |
| `watchlist export [LIST…]` | Writes watchlists as JSON to stdout. Default: every one. |
| `watchlist import FILE` | Brings in an export, adding what is missing. `-` reads stdin; `--replace` makes each list exactly as exported. |
| `section list [LIST]` | Its sections, with their ids and counts. The symbols in no section are `(no section)`. |
| `section create LIST NAME` | Adds a section, last. |
| `section rename LIST SECTION NAME` | Renames it. |
| `section delete LIST SECTION` | Removes it and the symbols in it. |
| `section promote LIST SECTION` | Turns it into a watchlist of its own, carrying its symbols. |
| `section order LIST SECTION…` | Puts the sections in this order, first to last; any left out keep their order behind them. |

A chartbook's watchlist is set with `chartbook watchlist`
([Chartbooks](/manual/cli/chartbooks/)).

## Making and filling a list

```
$ omacharts watchlist create Semis
created watchlist "Semis" (id 2)
  [exit 0]

$ omacharts watchlist add Semis NVDA AMD AVGO
added 3 to "Semis": NVDA AMD AVGO
  [exit 0]

$ omacharts section create Semis Memory
created section "Memory" in "Semis"
  [exit 0]

$ omacharts watchlist add Semis MU WDC --section Memory
added 2 to "Semis": MU WDC
  [exit 0]

$ omacharts watchlist show Semis
Semis

  (no section)
    NVDA
    AMD
    AVGO

  Memory
    MU
    WDC
  [exit 0]

$ omacharts watchlist add Semis XYZNOPE
omacharts: none of those are instruments this knows: XYZNOPE
  [exit 3]
```

A file of tickers goes in the same way; see [Scripting](/manual/cli/#scripting).

## Putting a list in order

The symbols in no section have no header and cannot be ordered; naming them
is refused.

```
$ omacharts section order Semis Equipment
sections of "Semis": Equipment, Memory
  [exit 0]

$ omacharts watchlist add Semis TSM --section Memory
added 1 to "Semis": TSM
  [exit 0]

$ omacharts watchlist move Semis TSM --section Equipment --before AMAT
moved TSM from Memory to Equipment in "Semis", in front of AMAT
  [exit 0]

$ omacharts watchlist move Semis NVDA
omacharts: nowhere to move it to: name a --section, a --before, or both
  [exit 2]

$ omacharts watchlist add Semis NVDA --section Memory
added 1 to "Semis": NVDA
  [exit 0]

$ omacharts watchlist move Semis NVDA --section Equipment
omacharts: NVDA is in 2 sections of "Semis"; say which with --from id:5 or --from id:6
  [exit 4]
```

Without `--before`, a moved symbol lands at the end of its section. Without
`--section`, it stays in the section it is in.

## Sorting

```
$ omacharts watchlist sort Semis change_pct --descending
"Semis" sorted by change_pct, high to low
  [exit 0]

$ omacharts watchlist sort Semis
change_pct:desc
  [exit 0]

$ omacharts watchlist sort Semis none
"Semis" is in its own order
  [exit 0]
```

## Turning a section into a watchlist

```
$ omacharts section promote Default Energy
turned section "Energy" into a watchlist with its 2 symbols
  [exit 0]
```

The default watchlist cannot be deleted:

```
$ omacharts watchlist delete Default
omacharts: "Default" is the watchlist the bar widget shows and cannot be deleted; rename it instead
  [exit 5]
```

## Moving watchlists between machines

`watchlist export` writes watchlists, with their sections and symbols, as one
JSON file. Copy it to the other machine and read it there with `watchlist
import`.

```
$ omacharts watchlist export > watchlists.json
$ omacharts watchlist export Semis Metals > some.json

$ omacharts watchlist import some.json
Semis: already up to date
Metals: added 1 symbol
  [exit 0]
```

Importing adds and never removes. Lists are matched by name (the default
list to the default list, whatever it is called); a matched list gains the
sections and symbols it lacks, after what it has, and a list with no match is
created. Importing the same file again changes nothing.

`--replace` makes each watchlist in the file exactly what it was where it
was exported, sections and order included:

```
$ omacharts watchlist import --replace some.json
Semis: replaced, 6 symbols in 1 section
Metals: replaced, 3 symbols in 0 sections
  [exit 0]
```

Neither form touches a watchlist the file does not name. A file that is not
an export (exit `2`), one made by a newer Omacharts, or one naming a list
that is ambiguous here changes nothing.

`-` reads the file from stdin; see [Scripting](/manual/cli/#scripting).

An export is plain JSON:

```json
{
  "omacharts": "watchlists",
  "version": 1,
  "watchlists": [
    {
      "name": "Semis",
      "sections": [
        { "name": "", "symbols": ["NVDA", "AMD", "AVGO"] },
        { "name": "Memory", "symbols": ["MU", "WDC"] }
      ]
    }
  ]
}
```

The section with an empty name holds the symbols in no section.
