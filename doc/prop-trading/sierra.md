# Sierra Chart in Omacharts

Sierra Chart as a provider: a data feed read from the files Sierra Chart
writes, and a broker reached through its local DTC server. Experimental,
and off unless the `sierra` feature is on:

```sh
omacharts features enable sierra     # the data feed
omacharts features enable trading    # and, with both on, the broker
```

With `sierra` off nothing of it exists: no feed on the Data page or in
`provider list`, no `provider setup sierra`, no folder looked for, no file
watched and no connection made. The user manual says nothing about it until
it is no longer experimental; this file is where it is described.

## The data feed

Sierra Chart signs in to its own data service and keeps what it hears in
its `Data` folder: an intraday file of every trade, `ESZ26_FUT_CME.scid`,
and a daily one, `ESZ26_FUT_CME.dly`. The feed reads that folder and
nothing else, so a chart of a contract Sierra Chart has is on screen as fast
as the disk gives up the part of the file it covers, and works whether or
not Sierra Chart is running.

- **Setup.** Preferences → Data → Add a data feed → Sierra Chart opens a
  dialog asking for Sierra Chart's folder. It is filled in with the first
  one found on this machine (`~/Downloads/SierraChart`, `~/SierraChart`,
  any Wine prefix's `drive_c/SierraChart`, Bottles, `/opt/SierraChart`), a
  button browses for another, and a menu offers the others found. The line
  under it says how many contracts with trades it holds, or what is wrong,
  as it is typed; Set up is pressable only for a folder that charts. From a
  terminal: `omacharts provider setup sierra` finds it, and
  `omacharts provider setup sierra folder=~/SierraChart port=11099` says
  where. The folder and port are kept in the `feed_setups` setting, local
  to the machine.
- **What it charts.** Futures roots Sierra Chart has files of, and their
  dated contracts. A root is its front month: the contract the rule in
  `contracts.tsv` says trades now, when Sierra Chart has its trades;
  otherwise the soonest unexpired contract it has trades of. Not stitched,
  as with Rithmic.
- **Bars.** Every intraday resolution from 1 minute to 4 hours is folded
  from the trades, in parallel over a memory-mapped file, with a binary
  search to start where the chart's cache ends; nothing of the file before
  that is read. Daily bars are Sierra Chart's `.dly`. Prices are stored
  scaled — ES at a hundred times its price — and the scale is read from
  Sierra Chart's own symbol settings (`SymbolSettings/*.xml`,
  `real-time-multiplier`).
- **Volume at price.** Exact: each trade record says what traded at the bid
  and at the ask, so the minute ladders, the volume profile, delta and
  footprints are built from the prints.
- **Live.** The intraday file growing. A chart follows it with inotify and
  reads only the records appended, so a chart is as current as Sierra Chart
  writes: its *Intraday File Flush Time in Milliseconds* (Global Settings →
  Data/Trade Service Settings). The setup dialog says to lower it for live
  charts.
- **Market depth.** Sierra Chart records depth in
  `Data/MarketDepthData/*.depth`. The app has no use for depth (its ladders
  are traded volume, not resting size), so those files are not read.

### The DTC server, for what the files cannot say

The files do not say which contracts a root lists or when each expires. The
feed asks Sierra Chart's DTC server for that, when it is listening, and the
charts never wait for it. The setup dialog says whether it is listening and,
while it is not, how to turn it on: in Sierra Chart, Global Settings →
Sierra Chart Server Settings → the *DTC Protocol Server* tab:

| Setting | Value |
| --- | --- |
| Enable DTC Protocol Server | Yes |
| Listening Port | 11099 (the port in the dialog) |
| Encoding | Automatic |
| Allowed Incoming IPs | Local Computer Only |
| Allow Trading | Yes, only once the `trading` feature is on |

The read-only rows *DTC Protocol Server is Listening* and *DTC Protocol
Clients* there are the first things to look at when the dialog says it is
not listening.

Sierra Chart's DTC server refuses market data for CME group exchanges
("Market data request not allowed"), which is why the charts come from the
files and never from the server. Its historical data server (the
*Historical Data Port*, 11098) refuses them too ("Request is not
authorized") and downloads nothing, so it cannot fill a gap in a file
either: a gap is filled by Sierra Chart itself, when a chart of the
contract is open there.

`provider setup` is accepted and listed nowhere — not in `--help`, `surface
--json` or the completions — because Sierra Chart is the only feed that has
a use for it; with the flag off it knows no such feed.

## The broker

With both `sierra` and `trading` on, Preferences → Trading → Add a trading
account offers Sierra Chart. Its dialog asks a name, the DTC server's host
and port (this machine, 11099), and says as it is typed whether the server
answers and allows trading; Sign in waits for one that does, and the steps
that turn *Allow Trading* on are shown while it does not. There is no
password: Sierra Chart is signed in to its broker itself, and its server,
set to *Local Computer Only*, answers this machine alone. *Require
Authentication* is not supported yet.

The login is a `Broker` and its `Session` in `src/trading/adapters/sierra.rs`,
over the `dtc` crate's `Trading`, and the trading UI knows no more of it
than of Rithmic. What it maps:

- **Accounts.** Whatever Sierra Chart trades: its simulator's `Sim1`, `Sim2`,
  and the broker's own accounts. Only a broker account has a balance (cash,
  open and daily P&L, its daily loss limit); the simulated ones have none.
- **Contracts.** The exchange's spelling above the adapter, `ESZ6`, and
  Sierra Chart's on the wire, `ESZ26_FUT_CME`. A root trades as the contract
  its Sierra Chart charts are drawn from.
- **Orders.** Market, limit and stop, placed, cancelled and moved, each
  waiting for the server's answer; a rejection comes back in its words.
  Cancel all and flatten.
- **Prices.** The server sends no CME market data, so positions are marked
  to the charts of their contracts, as for any broker that cannot price
  them.

### What DTC through Sierra Chart cannot do

- **No market data for CME group exchanges**, streaming or historical: the
  charts are the files' (see above).
- **No linking orders after they are placed.** DTC makes two orders
  one-cancels-other only as they are submitted together, so `link` is off
  for this broker.
- **A stop-limit is moved by its stop.** Cancel-replace changes Price1, and
  Sierra Chart moves the limit with it.
- **Prices on the wire are Sierra Chart's**, scaled by the contract's
  `DisplayPriceMultiplier` (ES at a hundred times): fills read that way, and
  the adapter divides an order's price the same way going out. That second
  half is not yet confirmed by an order: it needs one, on a simulated
  account, with the author's say-so.
- **Simulated or not is per account**, not per login: Sierra Chart lists its
  simulator's accounts beside the broker's.

### What was verified, and how

Every order path is tested against `dtc::fake`, a DTC server in memory.
Against the author's Sierra Chart (build 2933, `sc_futures_direct`), only
reads: the encoding handshake, logon, accounts, balances, positions, open
orders, fills, and the contracts of ES with their expiries
(`cargo run -p dtc --example look -- ES`, which sends no order message).
Sierra Chart's JSON parser drops every field written before `"Type"`, so the
client writes it first.
