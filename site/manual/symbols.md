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
| <kbd>Ctrl</kbd> + <kbd>K</kbd> or <kbd>/</kbd> | Empty |
| Click the symbol in a chart's header | Empty, for that chart, which the click focuses first |

A digit typed over the chart opens **Change resolution** instead ([Charts](/manual/charts/#choosing-a-resolution)). A letter typed while a drawing's text is being edited goes into the text.

## The search

The search opens near the top of the window, in the same place whichever chart is focused. Every chart fades, and the one a pick will change wears a wider focus ring. Adding a symbol to a watchlist changes no chart, so nothing fades and the end of the search row names the watchlist instead ([Watchlists](/manual/watchlists/#symbols)).

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window (HOME sandboxed, theme seeded)
state:
  omacharts chartbook create Manual --symbol NVDA --switch
  omacharts chartbook delete "Chartbook 1"
  omacharts chart set --resolution 1D
  click 400 400
  key ctrl+shift+b                          # sidebar hidden
  omacharts chart split horizontal    # the new chart, on the right, is focused
  move 1100 650
  key s
  type emi
capture: bin/headless-window shot /tmp/search.png
crop: none (resized to 2000 wide, JPEG)
-->
{% include figure.html src="/assets/manual/symbols/search-semi.jpg" alt="The symbol search with semi typed over two NVDA daily charts, listing SEMI, SMH, SOXX, TSM and other matches with their names, venues and kinds; the right-hand chart wears the wider ring" caption="The search with semi typed. It will change the chart with the wider ring, on the right." %}

The box reads **Symbol or name**. It matches a ticker or any part of a name, in the instrument's own language too. `台積` finds 2330.TW. Empty, it lists the majors.

| Key | Does |
|---|---|
| Typing | Narrows the list as you type |
| <kbd>↑</kbd> <kbd>↓</kbd> | Moves the highlight, with the keyboard still in the box |
| <kbd>Enter</kbd> or <kbd>Ctrl</kbd> + <kbd>Enter</kbd> | Charts the highlighted row. The best match is highlighted until you move the highlight |
| <kbd>Esc</kbd>, or a click outside | Closes it, changing nothing |

Each row shows the ticker, the name, the venue (NASDAQ, NYSE Arca, TWSE, BME and so on) and the kind, one of **Index**, **Futures**, **Stock**, **ETF**, **FX** or **Crypto**. Ten rows show at a time and the list scrolls to 24. Type more to narrow it. Once something is typed, a button in the search row clears it.

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

The venue tells two listings of one company apart. `BBVA.MC` is the Madrid listing and `BBVA` the New York one. Not every feed charts every venue ([Market data feeds](/manual/market-data/)). A chart asked for a symbol its feed does not have says **The data feed does not have this symbol.** or **No data feed in Preferences → Data charts this symbol.**

## Arithmetic on symbols

A symbol can be arithmetic on other symbols: `GC/SI` is gold over silver, `TNX-FVX` the ten-year yield less the five-year, `(SPY+QQQ)/2` a basket of the two, and `(2*RB+HO)*42/3-CL` a crack spread. It takes `+`, `-`, `*`, `/`, numbers and parentheses, with multiplying and dividing before adding and subtracting. Type one into the search and it is the first row. It goes anywhere a symbol goes, so a watchlist can hold it and a chart can follow it.

| Typed | Charts |
|---|---|
| `GC/SI` | Gold over silver |
| `TNX-FVX` | The ten-year yield less the five-year |
| `(SPY+QQQ)/2` | The average of SPY and QQQ |
| `SAN.MC/BBVA.MC` | Two listings abroad, each with its venue |
| `USDEUR/GC` | A pair turned over, as a leg like any other |

The chart is worked out from each leg's bars. The first symbol is the one whose bars the chart follows, and every other leg is read at its latest bar at or before each of them, so a stock over a future still lines up although the two trade at different hours. The open is worked out from the opens and the close from the closes. The high and the low are the larger and the smaller of the formula on the highs and on the lows, so a leg that is subtracted or divided by pulls them the other way. A day that would divide by zero is left out, and the chart has no volume.

A dash can be part of a ticker, so a run of letters with a dash in it is the ticker it names, and a minus only when it names nothing. A minus with spaces round it always subtracts.

In a watchlist, a formula that subtracts moves in points, the way a yield does, because a spread can cross zero. A ratio or a basket moves in percent. A spread that goes below zero is drawn on a linear scale, since a logarithmic one has nothing to show there.

A feed that streams, such as thinkorswim, charts no formulas. Each of its symbols is a stream of its own, and nothing joins two of them.

## Currency pairs

The search lists a dozen pairs, such as `EURUSD`. Any other pair of currencies charts when you type its six letters, or Yahoo's spelling of them: `SEKUSD` and `SEKUSD=X` are the same chart, kept as `SEKUSD`. A listed pair typed the other way round, `USDEUR`, is drawn as one over `EURUSD`, so the two read the same prices.

## See also

- [Searching symbols from the CLI](/manual/cli/#symbols)
- [Opening the app on a symbol](/manual/cli/#launching-the-app)
