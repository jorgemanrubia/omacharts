# Rithmic in Omacharts: spec

Draft, 2026-11-03. Exploratory: it lives on the `rithmic` branch, nothing here
is released, and every part of it can change. The background, with sources,
is in [research.md](research.md).

## What it is for

Omacharts becomes a platform a prop-firm trader can use all day: the charts
they already have, fed by their Rithmic login, and in time the buy and sell
buttons beside them. Rithmic is the piece that matters, because most futures
prop firms (Lucid, Tradeify, Apex, Bulenox and others) give their traders a
Rithmic login and let them choose the platform.

Each person's copy signs in to Rithmic directly with their own login. No
server of ours sits in between, and no login or market data passes through
one.

## What we know works

Tried on 2026-11-03 with the developer kit login (`~/.rithmic` on the
author's machine), against Rithmic Test at `wss://rituz00100.rithmic.com:443`:

- Sign-in to the ticker, order, PnL and history plants, as app `omacharts`.
  Rithmic Test takes any app name.
- One account, `951`, in USD, under FCM `Rithmic-FCM` and IB `Prospects`.
- The ES front month by asking for it: `ESZ6`.
- 1120 one-minute bars of ESZ6 from the last 24 hours.
- Live best bid and offer, and trades, as they happen.
- The same login is refused on the production gateway, every system there
  included. It is a Test login and nothing else.

Not seen yet: a balance from the PnL plant (the snapshot came back without
one), and orders of any kind.

## Choices already made

- **R | Protocol API**, through the [`rithmic-rs`](https://github.com/pbeets/rithmic-rs)
  crate (MIT or Apache-2.0). It is native Rust and needs no closed library.
  R | API+ is a C++ library Omacharts would have to bridge to and ship.
- **A crate of its own**, `crates/rithmic`, as `tos-market` is for
  thinkorswim. It hides tokio and `rithmic-rs` behind a thread and a
  callback, so the app sees plain events. The crate is the only place that
  knows the protocol.
- **Rithmic's `.proto` files are not committed.** The developer kit's
  licence forbids redistributing them; `rithmic-rs` carries its own generated
  code, and that is its authors' call.
- **Rithmic Test for all development.** Nothing touches a live or funded
  account until the author says so for that account, in so many words.

## Stages

Each stage is usable on its own and is a commit series on `rithmic`, not a
release.

### 1. Rithmic as a data source

Yahoo stays the default and serves everything it serves today. Data
providers become an ordered list, with Yahoo always on it. Each chart is
served by the first provider in that order that serves its instrument. Yahoo
serves futures too, delayed, so the order decides: with Rithmic above Yahoo,
futures come from Rithmic, and everything Rithmic does not have (stocks,
indexes, crypto, forex) still comes from Yahoo. The choice of feed stops
being one setting for every chart and becomes a question asked per
instrument.

- **Bars** for futures come from Rithmic: minute bars from the same request
  as the minute ladders below, daily and weekly from its time bars.
- **Symbols stay the app's own.** Charts, watchlists, search and stored
  chartbooks name an instrument the way they do now (`ES`, `AAPL`), whatever
  feed serves it. Each provider translates to its own names, as
  `Provider::symbol_for` does today: Rithmic turns a continuous future
  (`ES`) into the front month it asks Rithmic for (`ESZ6`) and a specific
  contract into itself. Nothing above the provider knows a Rithmic symbol,
  so switching feeds, or losing one, never changes what a chart is of.
- **One session for the whole app.** CME bills a non-professional
  subscriber as a professional after two concurrent data sessions, and
  R | Trader Pro's plug-in mode, Rithmic's way around that, is Windows only.
  So every chart shares one session, and the bar widget asks the app rather
  than signing in itself.
- **The Data settings are reworked** to show Yahoo as the baseline and
  Rithmic as something added to it. See "The Data page" below.
- **The password goes in the desktop's keyring** (Secret Service, which
  gnome-keyring provides on Omarchy), never in the settings database: the
  database is backed up and can be copied to another machine. User ID,
  system and gateway are settings, and they are `Local` in
  `src/setting_keys.rs`, because a login belongs to the machine it was typed
  on.

### 2. Volume at price

Volume indicators that need to know what traded at each price, and on which
side: volume profile, bid and ask volume, delta and cumulative delta,
footprints, imbalances, a per-bar point of control, an exact VWAP. One kind
of data serves all of them.

**The minute ladder.** For each minute of a contract: open, high, low,
close and volume, and for each price that traded, the **bid volume**
(sellers hitting the bid) and the **ask volume** (buyers lifting the offer).

- It is what Rithmic serves as history, as it is: "volume profile minute
  bars". On Rithmic Test a week of ESZ6 came back as 4558 minutes and 27,936
  price cells in 0.8 seconds. Nothing is rebuilt from ticks.
- It is the finest grain a chart of a minute or longer needs. A bar on any
  timeframe is the sum of its minutes, so footprints and delta are exact at
  1m, 5m, 1h or daily.
- It is small. Real ES trades about 30 prices a minute; with prices stored
  as whole ticks and volumes as varints that is in the order of 100 KB per
  contract per trading day, some 30 MB a year, under the cache limit that
  exists today.

**The question every indicator asks** is how much traded at each price, bid
and ask, between two times. Each stored day also keeps its **day ladder**,
the sum of its minutes, so a range is the whole days inside it plus the
loose minutes at either end. A year is some 250 day ladders and at most two
partial days: microseconds, however long the range. A footprint bar is the
same sum over its own minutes.

**Stored** in a table of its own beside `bar_series`, one row per contract
per trading day, the ladders as compact blobs. A day is the unit the cache
evicts, as a series is now.

**Live.** Each trade arrives with its aggressor. It is added to the current
minute's ladder in memory, and the chart is told as it is told about a
forming bar now. When the minute closes its ladder is final and the trades
are gone; ticks are never stored. On the next load Rithmic's own minute
replaces the one built live.

**Limits.** No history finer than a minute: tick or seconds charts would be
live only, unless tick replay is added later. How far back a prop firm's
system goes is not known yet; Rithmic is reported to keep a weekly data
quota, so history is fetched once and cached, never asked for twice.

### 3. Providers say what they can do

Feeds differ, and the richer ones will keep coming (Interactive Brokers is
likely next). A provider says what it serves: bars, live trades, minute
ladders, how far back. An indicator that needs ladders gets them when the
chart's feed has them.

**Yahoo must stay exactly as fast.** Somebody who never configures anything
pays nothing for any of this: no ladder is allocated, no query is made, no
hot path gains a branch that costs. On a Yahoo chart the volume profile
keeps today's estimate from bar ranges, and an indicator that cannot be
estimated says it needs a feed with volume at price. `examples/perf.rs` is
run before and after each change in this stage to prove it.

### 4. Trading

Orders from the chart, through the order plant. The first version:

- an account selector, for somebody with several prop accounts;
- buy and sell at the market, and limit and stop orders from the chart:
  click to place, drag to move;
- brackets with a stop and a target, held on Rithmic's servers so they
  survive Omacharts closing;
- **flatten** and **cancel all**, always one action away;
- panels for positions and working orders;
- the account's balance and Rithmic's loss limits (RMS), shown but not
  enforced.

Every order on a live account asks for confirmation until the trader turns
that off for that account. The order ticket says which account and system it
is about to use, every time.

### 5. Conformance

Rithmic checks a third-party app before it may sign in to production
systems, and registers its app name. Roughly: connect to Rithmic Test with
the registered name, keep the session well behaved (heartbeats, no
reconnect storms, a clean logout), and answer their questions. After that,
each prop firm decides whether its traders may use Omacharts.

Ask Rithmic for the process and the terms once stage 1 works, because a
working feed is what conformance looks at.

## Open decisions

Each has a recommendation, so work can go on until the author says
otherwise.

1. **Personal tool or product for others?** Rithmic's terms, and possibly
   CME's vendor rules, depend on it. *Recommendation:* personal use until
   stage 4 is solid, then ask Rithmic about offering it to others.
2. **Which instruments?** *Recommendation:* any CME future the login is
   entitled to, tested on ES, MES, NQ and MNQ.
3. **Enforce prop rules?** *Recommendation:* show Rithmic's RMS limits and
   the distance to them; enforce nothing. A prop firm's own rules change
   too often to copy into the app.
4. **Target a firm first?** *Recommendation:* Lucid, because the author has
   accounts there. Tradeify keeps an allow-list of platforms and has to be
   asked.
5. **History depth for ladders.** *Recommendation:* decide after stage 1
   shows how far back Rithmic goes on a prop system. Test is not a guide.

## Not in scope

- Automated strategies. They are non-display use under CME's licensing,
  which needs a licence of its own.
- Other brokers. Interactive Brokers comes later, through its own crate,
  behind the same account boundary trading introduces in stage 4.
- Windows or macOS.
