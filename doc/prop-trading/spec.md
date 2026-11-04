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

## Charting first

Omacharts has to keep appealing to people who never trade from it. Trading is
strictly opt-in: until somebody adds a trading account in Preferences →
Trading, the app is the one it was before any of this, with no new button,
menu item, panel, tab, theme group, shortcut, empty state or message, and no
cost in startup, memory or frame time. Preferences → Trading itself is the
one exception, because it is where an account is added. Adding one brings
the trading pieces in; removing the last one takes them all away again,
without a restart. Data feeds are not trading: a richer feed is for anyone
who charts.

## A generic foundation

Rithmic is the first rich data feed and the first broker, not the shape of
the app. This branch is done only when what it builds is a foundation other
feeds and brokers plug into (Interactive Brokers is the likely next one;
thinkorswim already is a feed):

- **Data feeds**: one boundary in the engine. What a feed serves, how it
  delivers (polled or streamed), what richer data it has (minute ladders,
  trades, later depth), how far back, and what it needs to sign in, are all
  declared by the feed and read by routing, the loader, indicators and the
  Data page without code naming any one feed.
- **Data elements**: bars, minute ladders, day totals, trades and ticks are
  the engine's own types, with no feed's quirks in them; a feed maps into
  them at its edge.
- **Brokers, accounts and orders**: one boundary in the app. Accounts,
  positions, P&L, orders, order statuses and refusals are neutral types; a
  broker says which order kinds it supports and whether an account is
  simulated; the panel, the chart menu and the chart's lines know no broker.
- **Connections**: a login shared by a data feed and a broker is one
  connection owned once and used by both, whatever the broker.
- **Setup**: each feed and broker keeps its own sign-in handling (Rithmic's
  dialog, thinkorswim's browser flow), reached through one seam: the feed
  says which kind of sign-in it needs, and one registry in the UI maps it to
  its dialog. Adding a provider is its dialog and one registry entry, not a
  generic form.

**Done when:** nothing above those boundaries names Rithmic or thinkorswim
or encodes their quirks (checked by an architecture review of the whole
branch, with every finding fixed or written down with its reason); and a
second fake feed and a second fake broker, in tests, provide streaming,
ladders, sign-in, accounts and orders without any change above the
boundaries.

## What we know works

Tried on 2026-11-03 with a developer kit login, against Rithmic Test at
`wss://rituz00100.rithmic.com:443`:

- Sign-in to the ticker, order, PnL and history plants, as app `omacharts`.
  Rithmic Test takes any app name.
- One account in USD, under FCM `Rithmic-FCM` and IB `Prospects`, which is
  how every developer kit login is set up.
- No front month by asking: Test answers "no data" for every root. Symbol
  search lists the contracts, and the nearest one more than 8 days from
  expiry is ESZ6.
- Shallow history: about 2,200 minute bars (a day and a half) whatever
  window is asked for, and 75 sparse daily bars back to 2016. Minute bars
  are stamped with the time they close.
- Volume-profile minute bars, with bid and ask volume at each price: a week
  asked for came back as 4558 minutes.
- Live best bid and offer, and trades with their aggressor, in bursts with
  quiet spells of several minutes.
- A second session on the same login forces the first one out. The window
  and anything else signed in at once kick each other out, which is one more
  reason for one session per app.
- The same login is refused on the production gateway, every system there
  included. It is a Test login and nothing else.

Since then, balances, positions and orders of every kind: see Orders and
Signing in, below.

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
- **Nobody's login leaves their machine.** A user ID or password is never
  committed, never written to a log, never printed by a test or a command,
  and never sent anywhere but Rithmic's gateway. The app reads the password
  from the keyring; development reads a login from the developer's own
  `~/.rithmic`, which is outside the repo. A test that needs a login is
  skipped without one, never fed one from the repo or CI. `Credentials`'
  `Debug` leaves the password out, and a test holds it to that. Account
  numbers stay out of the repo too: it is public.
- **Exploratory until the author says otherwise.** No commands and no manual
  pages for any of this yet; `AGENTS.md`'s rules for both apply from the
  moment it is meant to ship.

## Stages

Each stage is usable on its own and is a commit series on `rithmic`, not a
release.

### 1. Rithmic as a data source

**Done when:** with nothing configured the app behaves and measures as it
does on `main`; adding Rithmic with a Test login puts ES and NQ charts on
Rithmic, live, while AAPL in the same chartbook stays on Yahoo; signing out
moves the futures back to Yahoo; removing Rithmic leaves no row and no
header label.

Yahoo stays the default and serves everything it serves today. Data
providers become an ordered list, with Yahoo always on it. Each chart is
served by the first provider in that order that serves its instrument. Yahoo
serves futures too, delayed, so the order decides: with Rithmic above Yahoo,
futures come from Rithmic, and everything Rithmic does not have (stocks,
indexes, crypto, forex) still comes from Yahoo. The choice of feed stops
being one setting for every chart and becomes a question asked per
instrument.

Done in three steps, in this order:

1. **The window holds a list of feeds** instead of one, with Yahoo alone on
   it and `bin/perf-compare` showing nothing changed. No Rithmic code yet.
2. **The Rithmic provider**, in `crates/omacharts-engine/src/providers/rithmic/`
   on top of `crates/rithmic`.
3. **The Data page**, as below.

#### The Data page

An ordered list of sources. With nothing configured it is one row, Yahoo,
and a button to add another; no brokerage, no "Experimental", no sign-in
rows.

```
Sources                                  first match wins, top down
  ⋮ Rithmic   CME futures, front month · live · 951        [Sign out]
    Yahoo     Stocks, ETFs, indexes, futures, FX, crypto · delayed
  [ Add a source… ]
```

- **Yahoo is the floor.** It cannot be removed or moved: it serves
  everything, so a source under it would never be reached. The page refuses
  the move rather than only hiding the handle.
- **Add a source…** offers Rithmic and thinkorswim. Rithmic opens a dialog
  for user ID, password, system and gateway, prefilled from what is stored;
  "Sign in…" on its row opens the same dialog.
- **A source's row** has a handle to drag it up or down, one action (sign
  in, sign out, sign in again) and Remove. Its subtitle says what it serves
  and how it stands: "live · 951", "not signed in", "session expired", or
  Rithmic's own words for a refusal ("permission denied").
- **thinkorswim** becomes a source added like Rithmic, still marked
  Experimental on its row.
- The page manages sources and nothing else. Accounts and balances come
  back with trading (stage 4).

#### Which source serves a chart

- The single `provider` setting becomes `providers`: an ordered list of ids,
  top first, with Yahoo implied last. It is shared: an order restored on a
  machine with no Rithmic login is harmless, because a source that is not
  signed in is passed over. `provider=tos` migrates to `providers=["tos"]`
  (which gains its users FX and crypto, which thinkorswim does not serve);
  `provider=yahoo`, or nothing, migrates to nothing stored.
- A chart goes to the **first source that translates its instrument and is
  usable**. Usable is read from the stored sign-in state when the list is
  built and when a sign-in or sign-out finishes. It is a read from disk,
  never a connection.
- **A source is passed over only for a reason known before asking**: not
  signed in, or it does not serve the instrument. **A source that fails
  while running keeps the chart.** Somebody watching an ES footprint on
  Rithmic must not be moved to delayed Yahoo bars with the ladders gone
  without noticing. The chart keeps the trouble dot it shows today, and the
  provider owns reconnecting, as the `Stream` contract already says.
- **Each source has its own loader**, because pacing and cooldowns are per
  provider and Yahoo's rate limit must not pace Rithmic. With Yahoo alone
  there is one, as now.
- **The chart's header names its source** only when more than one is
  configured. With Yahoo alone nothing is drawn and the window is what it is
  today. A chart passed over a source says so: "Yahoo · Rithmic not signed
  in".

#### Symbols stay the app's own

`Instrument` in `crates/omacharts-engine/src/symbols.rs` is already the
canonical identity ("`ES`, not `ES=F`"), and `Provider::symbol_for`
returning an `Option` is already both the translation and the test of
whether a provider serves an instrument. Nothing above the provider changes:
charts, watchlists, search and stored chartbooks keep naming an instrument
as they do, so switching sources, or losing one, never changes what a chart
is of.

- **Rithmic serves futures roots** that `contracts.tsv` puts on a CME
  exchange, and declines everything else, which is what leaves stocks to
  Yahoo.
- **`symbol_for` stays offline.** It is called on the main thread and when
  building cache keys, so Rithmic's answer for `ES` is `ES`. Finding that the
  front month is `ESZ6` takes a request, and happens when bars are fetched,
  on the provider's thread. The cache key is `rithmic:ES`, which keeps a roll
  out of the store.
- **A spelling no rule produces** goes in `Instrument::overrides`, as it
  does for thinkorswim.

#### Front month, and rolls

- **For now, front month only, not stitched.** Rithmic's `ES` is the
  contract trading now, with the history that contract has (about 90 days
  intraday). That is what a prop trader watches at 1m or 5m, and it never
  crosses a roll.
- **At a roll the chart corrects itself.** The new contract's bars disagree
  with the cached tail of the old one, and the loader's `OVERLAP` check
  already treats that as history rewritten and fetches again.
- **Stitching, later.** When long daily charts from Rithmic matter, the
  provider fetches the earlier contracts back to the start and joins them on
  a fixed roll date, unadjusted, all inside its own `bars()`. Back-adjusting
  prices across rolls would be a chart option and never the default, because
  it moves prices a trader has drawn levels on.
- **Not a special case for daily.** Sending daily `ES` to Yahoo because
  Rithmic is not stitched would make the source depend on the resolution and
  break "the first that serves it". Routing stays by instrument alone.

#### Dated contracts, later

A chart of `ESZ6` itself is a separate change after routing. It is not a
new kind: a futures root gets an optional contract month
(`contract: Option<Contract>`, stored as `"Z6"` and left out when unset, so
every stored chartbook stays valid). `ESZ6` is CME's own spelling, so it is
neutral; each provider renders it, Rithmic as `ESZ6`, Yahoo as `ESZ26.CME`,
thinkorswim as `/ESZ6`. Search works out the next four quarterly contracts
of a root when what is typed reads as a root, a month code and a digit,
rather than listing them, so the index and startup stay the size they are.

#### The Rithmic session

- **One session for the whole app.** CME bills a non-professional
  subscriber as a professional after two concurrent data sessions, and
  R | Trader Pro's plug-in mode, Rithmic's way around that, is Windows only.
  So every chart shares one session, owned by the provider and closed by its
  `retire()`, and the bar widget asks the app rather than signing in itself.
- **Bars** come from Rithmic: minute bars from the same request as the
  minute ladders in stage 2, daily and weekly from its time bars.
- **The password goes in the desktop's keyring** (Secret Service, which
  gnome-keyring provides on Omarchy), never in the settings database: the
  database is backed up and can be copied to another machine. User ID,
  system and gateway are settings, and they are `Local` in
  `src/setting_keys.rs`, because a login belongs to the machine it was typed
  on.
- **A refusal is shown in Rithmic's words**, such as "permission denied",
  not as a generic failure.

### 2. Volume at price

**Done when:** an ES chart on Rithmic draws its volume profile from ladders,
not bar ranges, over a week of history loaded once and read back from the
cache on the next start; the forming minute's ladder grows with live trades;
and a range query over the whole cached history takes under a millisecond.

Volume indicators that need to know what traded at each price, and on which
side: volume profile, bid and ask volume, delta and cumulative delta,
footprints, imbalances, a per-bar point of control, an exact VWAP. One kind
of data serves all of them.

**The minute ladder.** For each minute of a contract: open, high, low,
close and volume, and for each price that traded, the **bid volume**
(sellers hitting the bid) and the **ask volume** (buyers lifting the offer),
and the volume that traded there with no known aggressor, which is in the
price's volume and on neither side, so a feed that never says still draws
a profile and has no delta. Each minute also keeps its totals by side (bid, ask, and volume with no
known aggressor) and trade counts by side, so bid and ask volume per bar,
delta and average trade size never add up the price arrays.

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

**Ticks, in memory.** Each trade arrives with its price, size, time to the
nanosecond and aggressor. It goes into a bounded buffer of the contract's
trades for the session (about 5 million, the oldest whole minutes dropped
first) and into the forming minute's ladder, without allocating. The buffer
is what tick, volume and range bars are built from, each with a ladder of
its own, and later a time and sales list. Ticks are never stored.

**Live feeds history.** When a minute closes it is saved, cheaply and
incrementally, without rewriting its day. The store knows which minutes it
holds, so the next start asks the provider only for what is missing, which
also spares Rithmic's data quota. A disconnect is recorded as a gap, not as
quiet minutes, so it is filled from history. When the provider's own minute
arrives for a span that was built live, it replaces the live one: it has the
trades a disconnect missed.

**Limits.** No history finer than a minute: tick, volume, range and seconds
bars are live only, unless tick replay is added later.

**Built for more than the profile.** Footprints, bid and ask volume bars,
delta and cumulative delta, imbalances and a per-bar point of control are
not part of this work, but the data is shaped for them: each is an indicator
over minute ladders, ticks and side totals, and adding one needs no new
storage, no new feed and no migration. How far back a prop firm's
system goes is not known yet; Rithmic is reported to keep a weekly data
quota, so history is fetched once and cached, never asked for twice.

### 3. Providers say what they can do

**Done when:** an indicator can declare that it needs volume at price and
gets ladders exactly when its chart's source has them; the volume profile
uses them and keeps its estimate otherwise; and `bin/perf-compare` against
`main` shows the same allocation counts for every scenario.

Feeds differ, and the richer ones will keep coming (Interactive Brokers is
likely next). A provider says what it serves, per timeframe as `Capability`
does now, and richer data is added the way live streaming was: an optional
part of a provider that answers "none" unless the provider has it, so no
provider is asked about something it lacks.

- **Ladders and trades** are a trait of their own in `provider.rs`, reached
  through a method on `Provider` that returns `None` by default. The types,
  a minute ladder and a trade with its side, live in a new
  `crates/omacharts-engine/src/ladders.rs`. `Bar` does not change.
- **Indicators say what they need**: bars, or volume at price. The volume
  profile needs bars and keeps today's estimate from bar ranges, and is exact
  when the chart has ladders. An indicator that needs volume at price (none
  yet; footprint and delta later) shows one line on a chart whose source has
  none, "Needs a source with volume at price", rather than an invented
  estimate.

**Yahoo must stay exactly as fast.** Somebody who never configures anything
pays nothing for any of this. A chart holds ladders only when its source has
them and one of its indicators needs them; otherwise there is no ladder
allocated, no query made and no second request. The draw path asks whether
there are ladders once per frame, outside the loop over bars. The perf
harness runs on the synthetic provider, and `bin/perf-compare` is run before
and after each change in stages 1 to 3 to show the allocation counts have
not moved.

### 4. Trading

#### Logins, accounts and data feeds

Three things, kept apart, so the broker an order goes to and the feed a chart
is drawn from can be combined freely:

- **A login** is a broker connection: a name the trader chooses ("Lucid",
  "Tradeify", "Rithmic Test"), the Rithmic user ID, system and gateway, and
  the password in the keyring under that login. There can be any number of
  them. Each login has its own session; one login is one session, because a
  second sign-in on the same login forces the first one out.
- **An account** is one of the account IDs inside a login, listed by the
  order plant after sign-in. A prop trader usually has several under one
  login. The list is cached on the machine (bookkeeping, never in a backup)
  so the page and the header draw without connecting.
- **A data feed** is a source on the Data page. Only one Rithmic login
  supplies market data, the first added unless the trader picks another;
  the others connect for orders, positions and P&L only. Several logins each
  streaming quotes could count as several data sessions under CME's
  two-session rule, and whether sessions with different firms count apart is
  not known.

**Preferences → Trading** lists the logins, each with its accounts under it,
edited the way the Data page edits sources: "Add a broker…" (Rithmic for
now) opens the same login dialog. Adding the first login also adds Rithmic
as the first data source, through that login. Removing a login removes its
accounts; if it was the data login, Rithmic data moves to another login, or
the Rithmic source goes if none is left. Nothing else on the Data page is
touched.

```
Accounts
  Lucid — LucidTrading · live                    [Sign out] [Remove]
      1234   USD · $50,210 · Default
      1235   USD · $48,900                        [Make default]
  Rithmic Test — Rithmic Test · live · data      [Sign out] [Remove]
      951    USD · $1,000,000
  [ Add a broker… ]
```

**The chart's account and feed.** Every chart has an account, whose money
its orders spend: the default, or one chosen for it. A chart's default
feed is its account's own, Rithmic through the data login, when that
serves the instrument, and otherwise the Data page's order; nothing is
stored for it. A feed chosen for the chart overrides it until the feed
stops serving the instrument or the chart changes account. One function
decides a chart's account and one its feed, for the header and for the
order ticket alike, so they can never disagree. A chart on the default
follows it when the default changes, and a chart whose account's login was
removed goes back to the default account and its default feed.

**The header's Account part** names the chart's account and feed, and
clicking it opens the picker. It is offered when there is more than one
account or more than one feed; otherwise it is blank, takes no space and
cannot be clicked. An account is named by its login, `Lucid`, with its
number only when the login holds several or another login has the same
name, `Lucid 1234`. The text names the account whenever there is one, and
the feed only when it is not the account's default; trouble goes after the
thing in trouble:

| Case | Text |
|---|---|
| No account, several feeds | `Yahoo`, or `Yahoo · Rithmic not signed in` when one was passed over |
| An account on its default feed | `Rithmic`, `Lucid 1234` |
| A feed chosen, or fallen back to | `Rithmic · Yahoo`, `Lucid 1234 · Yahoo` |
| Several accounts and one feed | `Lucid 1234` |
| The login in trouble | `Rithmic signed out`, `Lucid 1234 signed out · Yahoo` |
| The chosen feed refused | `Rithmic · Yahoo not signed in` |
| No feed serves the instrument | `Rithmic · no data feed` |

The popover lists the accounts, and under the chart's own, indented, the
feeds that serve its instrument, once there is more than one feed. The
other accounts stay collapsed, and a single account is a heading.
Choosing another account closes it and puts the chart on that account's
default feed. The account's default feed is marked "(default)", and
choosing it stores nothing. With no account it is a flat list of the
feeds, the order's pick marked "(default)". An account's row has a line
under it only for trouble; a feed's row is its name alone. The popover
lists accounts and feeds and nothing else.

#### Orders

Tried on Rithmic Test on 2026-11-03: the Test account is a simulated
$1,000,000 account (loss limit $999,999, 20 contracts a side, 40 an order).
A one-lot market buy of ESZ6 filled and a flatten closed it, with about
$1.97 commission a side. An order reports each step (received, open
pending, at the gateway, sent to the exchange, open, fill, complete);
flatten is one request and Rithmic places the closing order; positions and
P&L arrive as pushed updates. (The PnL snapshot came back empty that
day; on 2026-11-04 it answered with the balance and the position.)

The first version of order entry:

- buy and sell at the market, and limit and stop orders from the chart:
  click to place, drag to move;
- brackets with a stop and a target, held on Rithmic's servers so they
  survive Omacharts closing;
- **flatten** and **cancel all**, always one action away;
- panels for positions and working orders;
- the account's balance and Rithmic's loss limits (RMS), shown but not
  enforced.

On the chart, a working order's line can be dragged to another price,
and the position's pill has a TP and an SL tab beside it, dragged out to
place a target or a stop for the whole position. While it is dragged it
is a preview: its line in the order's look, what it would be, and how
many ticks it has come; where it may not go with the market where it is,
it says why and letting go does nothing. Every pill ends in a ×, which
cancels an order or flattens the position. A new exit is linked to the
other one working on the position when the broker links orders (Rithmic
does, on its servers), so whichever fills first cancels the other; a
broker that does not is said to, and flattening cancels the exits left
behind. Tried on Rithmic Test on 2026-11-03.

Whatever is asked shows at once, faint, and at full strength only once the
broker's book confirms it: a new order once the exchange has it working
(not when Rithmic has merely received it), a market order as "Buying 1…"
beside the last price until its fill, a move once the order is reported
at its new price, a cancel once it is reported cancelled, a flatten once
nothing is held. A refusal takes it away with the broker's reason; a
request that is never answered stays pending rather than being taken as
done. Part of an order filled reads "Sell Limit 1/2", and the position
holds only what filled. Toasts say only what the broker has confirmed.

Every order from a chart is for the chart's **trading size**, shown in
its header ("2 contracts") and picked there, or with Ctrl and a digit,
from the size template that applies to its instrument: a named list of
sizes for stocks or futures, optionally narrowed to some roots, the
narrowest winning (Preferences → Trading → Edit trading sizes…). Out of
the box stocks go 100 to 900 and futures 1 to 9. With a trading account,
Ctrl+B buys the size at market, Ctrl+S sells it, and Ctrl+F cancels every
order working in the chart's contract and flattens it; the sidebars moved
to Ctrl+Shift+B and Ctrl+Shift+D, and watchlist N to Ctrl+Shift and a
digit, to make room. Tried on Rithmic Test on 2026-11-04: Ctrl+2 then
Ctrl+B on ES filled 2, and Ctrl+F left the account flat.

**No confirmations, for now.** Nothing asks first, on Rithmic Test or on a
live account: an order from the menu, a key or a drag, a move, a cancel and
a flatten go to the broker at once, and the pending look is the feedback.
Whether a live account should ask, and how, is to be decided again before
trading ships.

#### Signing in, and what comes with it

A saved trading login signs in by itself when the app starts, after the
window's first frame is painted, from an idle at low priority behind
whatever the charts asked for, on a thread of its own per login. Nothing
on the main loop waits on the network or the keyring. Each sign-in is
followed, on the same thread, by a **refresh**: the money and the risk
limits, the positions, the working orders, and today's orders, filled,
cancelled or rejected, with their fills. The refresh is over once the
plants have been quiet for 300 ms after its last answer; then the book is
marked refreshed and the panel, the headers and the chart lines show it,
on the next frame. Signing in from the Trading page refreshes the same
way, and so does a session coming back after a drop. After that
everything follows Rithmic's pushes: no polling, no timers.

After a refresh, an order sent from here before a drop that the refreshed
book has never heard of, by its tag, did not reach the broker: it stops
being drawn as pending, and a toast says "Buy Limit 1 ESZ6 never reached
the broker". Moves, cancels and flattens are answered by the book as
always, and nothing is held as on its way to that login's accounts.

The Accounts tab shows the balance, the day's P&L, the open P&L, the loss
limit and the balance the account is liquidated at, as Rithmic says them;
shown, not enforced. The Orders tab shows the day's orders, not only this
session's.

What Rithmic Test returned for each request, tried on 2026-11-04 with a
position of one ESZ6 and a sell limit far above the market, from a fresh
start of the window, all within about 0.9 s:

| Request | What Test returns |
| --- | --- |
| `show_orders` | The day's orders as **exchange notifications only**, flagged as a snapshot: a `fill` for each filled order (size, price, fill id), a `cancel` or a `reject` for each that ended so, and a `status` for each still working, with the user tag. No Rithmic order notification precedes them, so each is taken whole from the exchange's. Rithmic's day had not rolled at 01:00 UTC: the orders of the 3rd were still listed. |
| `show_order_history_dates` | Dates back to 2013, the latest the 3rd: the day in progress is not listed. |
| `show_order_history_summary` | For the 3rd, the same exchange notifications for every order that day. Asked only for dates that are today in UTC, so on Test, nothing. |
| `show_fill_history` | One empty answer, by time and by trade date alike. Fills come from the exchange's notifications instead, and `history()` returns nothing on Test. |
| `get_account_rms_info` | Loss limit $999,999, 40 an order, 20 a side, status active; no auto-liquidation threshold, and no RMS update pushed. |
| `get_pnl_position_snapshot` | The balance ($997,462.57), the day's P&L and an ESZ6 position of +1 at 7,546.50, flagged as a snapshot. When it says nothing of positions, as it did on the 3rd, they are worked out from today's fills, and the balance stays "—" until the first push. |

Then, with nothing done in the window, the position, the working order,
the day's orders and the balance were there; cancelling the limit and a
market buy both showed on the next frame, and Ctrl+F left the account
flat with nothing working, which a separate sign-in confirmed.

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
6. **Does conformance open every firm's system?** Probably not. Rithmic's
   own answer was that each funding evaluator or broker decides which
   applications its users may use, and Tradeify publishes an allow-list.
   *Recommendation:* ask Rithmic whether a conformed app is blocked on a
   firm's system until the firm enables it, ask Lucid whether it would, and
   use nothing but Rithmic Test until a firm says yes.
5. **History depth for ladders.** *Recommendation:* decide after stage 1
   shows how far back Rithmic goes on a prop system. Test is not a guide.

## Not in scope

- Automated strategies. They are non-display use under CME's licensing,
  which needs a licence of its own.
- Other brokers. Interactive Brokers comes later, through its own crate,
  behind the same account boundary trading introduces in stage 4.
- Windows or macOS.
