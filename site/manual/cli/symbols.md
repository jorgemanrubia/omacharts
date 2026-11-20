---
title: "Symbols"
description: "Searching the instruments, looking one up, and the exchange suffix other commands take."
section: agents
parent: cli
nav_order: 1
---

`symbol` searches the same instruments the window searches. Which
instruments there are, and how a listing abroad is named, is in [Symbols and
search](/manual/symbols/).

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

Nothing matching exits 3: `omacharts: nothing matches "ZZZQX"`.

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

An instrument the search does not know exits 3 and says what to try.

## `--suffix` on other commands

`--suffix` names a symbol's venue on `chart set`, `watchlist add`, `watchlist
remove` and `watchlist move`. On `add` and `remove` it applies to every
symbol given. `--symbol SAP --suffix DE` and `--symbol SAP.DE` are the same.
A symbol typed in lower case is charted in capitals. One the search does not
know is refused with exit 3:

```
$ omacharts chart set --symbol zzzqx
omacharts: no instrument called ZZZQX; try `omacharts symbol search ZZZQX`
  [exit 3]
```
