---
title: "Symbols"
description: "Searching the instruments, looking one up, and the exchange suffix other commands take."
section: agents
parent: cli
nav_order: 1
---

`symbol` searches the same instruments the window searches. Which
instruments there are, and how a listing abroad is named, is in [Symbols and
search](/manual/symbols/). `symbol refresh` fetches the newest US listings now
and says when they were built and how many there are (`--json`: `built`, `rows`, `updated`).
`symbol quote SYMBOL` is the last price of one instrument, from a chart of it
on screen when there is one, else as last fetched (`--json`: `source`,
`last`; no feed gives a bid or an ask, so they are null).

## `symbol search`

Finds instruments matching a query, best first.

```
$ omacharts symbol search semi --limit 5
SEMI       Columbia Select Technology ETF     ETF      NYSE Arca
SMH        VanEck Semiconductor ETF           ETF      NASDAQ
SOXX       iShares Semiconductor ETF          ETF      NASDAQ
TSM        Taiwan Semiconductor ADR           Stock    NYSE
2330.TW    Taiwan Semiconductor Manufacturin… Stock    TWSE
  [exit 0]

$ omacharts symbol search GC --limit 3
GC         Gold                               Futures
GCOW       Pacer Global Cash Cows Dividend E… ETF      Cboe BZX
GCAL       Goldman Sachs Dynamic California … ETF      NYSE Arca
  [exit 0]
```

| Argument or flag | Takes |
|---|---|
| `QUERY` | A ticker or part of a name. A whole [formula](/manual/symbols/#arithmetic-on-symbols), such as `GC/SI`, is the first row |
| `--limit N` | How many to show; 10 when not given |
| `--json` | The result as JSON |

A ticker no listing has yet, such as `ANT` the day it listed, is asked of the [feed](/manual/market-data/), and comes first when the feed knows it. Nothing matching, here or on the feed, exits 3: `omacharts: nothing matches "ZZZQX"`.

## `symbol show`

Everything known about one instrument: its ticker as charted, name, kind,
venue and currency.

```
$ omacharts symbol show SAP DE
SAP.DE
SAP
kind      Stock
exchange  XETRA
currency  EUR
  [exit 0]
```

| Argument or flag | Takes |
|---|---|
| `SYMBOL` | The ticker, or the ticker and its venue in one: `SAP.DE`. Or [arithmetic on symbols](/manual/symbols/#arithmetic-on-symbols), such as `GC/SI` |
| `SUFFIX` | The venue suffix, when not part of `SYMBOL` |
| `--json` | The result as JSON, with the symbol, suffix and local name as separate fields |

A ticker the search does not know is asked of the feed, the same as in `symbol search`. One the feed does not know either exits 3 and says what to try.

## `--suffix` on other commands

`--suffix` names a symbol's venue on `chart set`, `watchlist add`, `watchlist
remove` and `watchlist move`. On `add` and `remove` it applies to every
symbol given. `--symbol SAP --suffix DE` and `--symbol SAP.DE` are the same.
A symbol typed in lower case is charted in capitals. A ticker the search
does not know is asked of the feed on `chart set` and `watchlist add`, and
charted or added when the feed knows it; from then on the search knows it
too. One the feed does not know either is refused with exit 3:

```
$ omacharts chart set --symbol zzzqx
omacharts: no instrument called ZZZQX (Yahoo Finance does not know ZZZQX); try `omacharts symbol search ZZZQX`
  [exit 3]
```

Asking the feed takes as long as one request to it, a second or two.

