---
title: "Symbols and search"
description: "Finding an instrument by typing, what the search covers and how it ranks, and venue suffixes for listings abroad."
section: charting
nav_order: 13
---

Start typing over a chart to find a symbol. Results show up as you type, and <kbd>Enter</kbd> charts the first one.

## Opening the search

| Key or gesture | Opens the search |
|---|---|
| Type a letter over the chart | With that letter already in the box |
| <kbd>Ctrl</kbd> + <kbd>K</kbd>, <kbd>Ctrl</kbd> + <kbd>F</kbd> or <kbd>/</kbd> | Empty |
| Click the symbol in a chart's header | Empty, for that chart, which the click focuses first |

A digit typed over the chart opens the resolution box instead ([Charts](/manual/charts/#resolutions)). A letter typed while a drawing's text is being edited goes into the text.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  (two NVDA charts side by side, sidebar open; any chart will do)
  key Escape
  key s
  type emi
capture: bin/headless-window shot /tmp/search.png
crop: 712,244,1136,954   (the search popover, 24px layout margin)
-->
{% include figure.html src="/assets/manual/symbols/search-semi.png" alt="The symbol search with semi typed, listing SEMI, SMH, SOXX, TSM and other matches with their names, venues and kinds" caption="The search with semi typed." %}

## The search dialog

The box reads **Symbol or name**. It matches a ticker or any part of a name, in the instrument's own language too. `台積` finds 2330.TW. Empty, it lists the majors.

| Key | Does |
|---|---|
| Typing | Narrows the list as you type |
| <kbd>↑</kbd> <kbd>↓</kbd> | Moves the highlight, with the keyboard still in the box |
| <kbd>Enter</kbd> or <kbd>Ctrl</kbd> + <kbd>Enter</kbd> | Charts the highlighted row. The best match is highlighted until you move the highlight |
| <kbd>Esc</kbd>, or a click outside | Closes it, changing nothing |

Each row shows the ticker, the name, the venue (NASDAQ, NYSE Arca, TWSE, BME and so on) and the kind, one of **Index**, **Futures**, **Stock**, **ETF**, **FX** or **Crypto**. The list stops at 24 rows. Type more to narrow it.

Something that looks like a ticker, such as `XYZ` or `2330.TW`, is offered even when it matches nothing, as **Not in the list — look it up anyway**. Pick it and the chart asks the [feed](/manual/market-data/); if the feed does not have the symbol, the chart says so. A name that matches nothing is not offered.

Picking a symbol charts it on the focused chart, and on every Follow chart too when the focused chart is one ([Linking charts to the watchlist](/manual/watchlists/#linking-charts-to-the-watchlist)).

## What the search covers

| Kind | What is there |
|---|---|
| Stocks and ETFs | Every US-listed equity and ETF, and every listing on Taiwan's two exchanges, the TWSE and the TPEx |
| Indexes | The majors: the S&P 500, the Nasdaq 100 and Composite, the Dow, the Russell 2000, the VIX, the PHLX Semiconductor, the dollar index, Treasury yields, and the main indexes abroad (DAX, FTSE 100, Nikkei 225, TAIEX, IBEX 35 and others) |
| Futures | By root (`ES`, `NQ`, `CL`, `GC`, `ZN` and the rest): index and micro index futures, energy, metals, Treasuries, currencies, grains, softs and livestock |
| FX | The major and cross pairs, as `EURUSD` |
| Crypto | The main coins, as `BTC` |
| Listings abroad | The large companies of Europe and Japan by their home listing (`SAP.DE`, `BBVA.MC`, `7203.T`), and anything else you type with a [venue suffix](#listings-abroad) |

The best match comes first. `GC` is gold futures, and `semi` lists the semiconductor ETFs before companies with *semi* in the name.

## Listings abroad

A listing outside the US is its ticker plus its venue's suffix. `SAP DE` and `SAP.DE` are the same instrument. A dot followed by anything that is not one of these suffixes stays part of the ticker, so `BRK.B` is one US ticker.

| Suffix | Venue |
|---|---|
| `MC` | BME, Madrid |
| `L` | LSE, London |
| `DE` | XETRA, Frankfurt |
| `PA` | Euronext Paris |
| `AS` | Euronext Amsterdam |
| `SW` | SIX, Zurich |
| `MI` | Borsa Italiana, Milan |
| `T` | TSE, Tokyo |
| `TO` | TSX, Toronto |
| `HK` | HKEX, Hong Kong |
| `TW` | TWSE, Taiwan |
| `TWO` | TPEx, Taiwan |

The venue tells two listings of one company apart. `BBVA.MC` is the Madrid listing and `BBVA` the New York one. Not every feed charts every venue ([Market data feeds](/manual/market-data/)). A chart asked for a symbol its feed does not have says **The data provider does not have this symbol.** or **This data provider cannot chart this symbol. Another one may be able to.**

## See also

- [Searching symbols from the CLI](/manual/cli/#symbols)
- [Opening the app on a symbol](/manual/cli/#launching-the-app)
