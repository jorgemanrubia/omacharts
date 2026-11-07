# Rithmic in Omacharts: spec

Draft, 2026-11-04. Exploratory: it lives on the `rithmic` branch, nothing here
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

Omacharts has to keep appealing to people who never trade from it. Trading
is experimental, and strictly opt-in, by two rules.

**Trading is behind a feature flag, `trading`, off unless somebody switches
it on.** Off, the app is the one it was before any of this, even with a
broker login saved: no trading panel or toggle; no account, P&L, Buy Sell or
Quantity in a chart's header; no order or position lines or tags on a chart;
no orders in the chart's menu; no trading keys and no trading rows on the
shortcuts sheet; no Preferences → Trading in the sidebar, no Trading colours
and lines on the Theme page and no trading parts in Preferences → Charts →
Header (a header that holds them keeps them where they were, unseen, for
when the flag is on); no trading part typed to `config set chart_header…`;
and nothing signs in to a broker's order plant, at startup or ever. Rithmic
as a data feed is not trading and stays: with the flag off its login is
typed in its own dialog from the Data page, kept as the data login, and
signs in its data alone; the row signs it in, out and removes it like any
other feed's.

The flag is switched from a terminal, and an open window follows at once,
in place — the trading pieces come or go as they do when the first login
is added or the last removed, with the charts, the arrangement and the
focus as they were. Switched off, every session on an order plant signs
out; switched on, the saved logins sign in as at startup:

```
omacharts features                    # every flag and whether it is on
omacharts features enable trading
omacharts features disable trading
```

It is kept in the settings as `feature.trading`, a machine's setting that a
restore from elsewhere leaves alone. The command is accepted and listed
nowhere — not in `--help`, `surface --json`, the completions, the man page,
the agent skill, the manual or `doc/cli.md` — and `config list` leaves it
out, so that an experiment is not advertised. A flag nobody knows exits
with code 3. More flags are a row each in `FLAGS`, in `src/features.rs`: a
name, a default and what it switches. Checking one is a load of a bool,
read once at startup and written when it is switched; nothing asks the
database, and with trading off its pieces are never built.

The second flag is `sierra`: Sierra Chart as a data feed, read from its
files, and with `trading` on as well, as a broker through its DTC server.
See [sierra.md](sierra.md).

**With the flag on, trading's settings are always visible in Preferences;
everything else is unchanged until there is an account.** Every trading
setting is in Preferences whether or not there is a login, so it can be set
before the first one: Preferences → Trading (its accounts, Order entry and
Chart), the Trading colours and lines on the Theme page of a theme of one's
own, and the header's P&L, Buy Sell and Quantity in Preferences → Charts →
Header, listed and offered like any other part. A setting changed with no
account changes nothing anybody sees until there is one.

Everywhere else, until somebody adds a trading account the app is the one it
was before any of this: the same chart, header, corners, menus, panels and
shortcuts sheet, with no new button, menu item, panel, tab, shortcut, empty
state or message, and no cost in startup, memory or frame time. The header's
trading parts draw nothing and take no room, so a header that has them looks
as one without them; the range a chart holds to ("YTD") stays in the corner
under its prices; the chart's menu has no orders and no line for them; and
Ctrl+B, Ctrl+S, Ctrl+F and Ctrl and a digit do nothing. Adding an account
brings the trading pieces in; removing the last one takes them all away
again, without a restart. Data feeds are not trading: a richer feed is for
anyone who charts. Neither are the general changes made on this branch for
everyone, such as the header's sections and the symbol starting where the
legend's names do; the keys that moved to make room for trading's (the
sidebars to Ctrl+Shift+B and Ctrl+Shift+D, finding a symbol to Ctrl+K and /
alone, the chartbook's screenshot to Ctrl+Alt+O) moved for everyone too.

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

Both are done. A test fails when a feed is named above the boundary
(009a59d), and the fake broker, in the registry of a build with the
`fakes` feature, signs in from the Trading page, lists its account and
fills an order through the same code a Rithmic login goes through
(3910107).

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
- **Exploratory until the author says otherwise.** The manual has what
  anybody meets without trading: the Log, the header's two sections, and the
  keys that moved to make room for trading's. Trading itself has no manual
  pages and no commands yet: a chart's `account`, `trading_size` and `source`
  are exempted in `src/cli/spec.rs` as exploratory, and `AGENTS.md`'s rules
  for both apply from the moment it is meant to ship.

## Stages

Each stage is usable on its own and is a commit series on `rithmic`, not a
release.

### 1. Rithmic as a data feed

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

An ordered list of data feeds. With nothing configured it is one row,
Yahoo, and a button to add another; no brokerage, no "Experimental", no
sign-in rows.

```
Data feeds
Symbols use the first data feed that offers them.
  ⋮ Rithmic   CME futures, front month · live · through Lucid
  ⋮ Yahoo     Stocks, ETFs, indexes, futures, FX, crypto · delayed   [Remove]
  [ Add a data feed… ]
```

- **Only the last feed cannot be removed.** Yahoo is a feed like the others
  since 5b1cb64: it has a handle and Remove, and comes back through the
  button. The last feed left offers no Remove, because with none no chart
  could draw.
- **Add a data feed…** offers the feeds not on the list. Rithmic's login is
  a broker's, so with trading on adding it adds a login in Preferences →
  Trading, through the same dialog: name, user ID, password, system and
  gateway. With trading off the same dialog opens from the Data page, and
  the login signs in its data alone.
- **A feed's row** has a handle to drag it up or down, one action (Sign in…
  or Sign out) and Remove. Its subtitle says what it serves and how it
  stands: "live", "not signed in", "session expired", or the feed's own
  words for a refusal ("permission denied").
- **Rithmic's row says "through Lucid"**, the login its data comes through,
  and has no action and no Remove of its own: signing in and out, and
  removing it, are the Trading page's.
- **thinkorswim** is a feed added like the others, still marked
  Experimental on its row.
- The page manages data feeds and nothing else. Accounts and balances are
  on the Trading page.

#### Which feed serves a chart

- The single `provider` setting becomes `providers`: an ordered list of ids,
  top first. It is shared: an order restored on a machine with no Rithmic
  login is harmless, because a feed that is not signed in is passed over.
  `provider=tos` migrates to `providers=["tos"]` (which gains its users FX
  and crypto, which thinkorswim does not serve); `provider=yahoo`, or
  nothing, migrates to nothing stored. Yahoo is in the list where it is put
  (migration 8 wrote it at the bottom of every stored order, where it had
  been implied); nothing stored is Yahoo alone.
- A chart goes to the **first feed that translates its instrument and is
  usable**. Usable is read from the stored sign-in state when the list is
  built and when a sign-in or sign-out finishes. It is a read from disk,
  never a connection.
- **A feed is passed over only for a reason known before asking**: not
  signed in, or it does not serve the instrument. **A feed that fails
  while running keeps the chart.** Somebody watching an ES footprint on
  Rithmic must not be moved to delayed Yahoo bars with the ladders gone
  without noticing. The chart keeps the trouble dot it shows today, and the
  provider owns reconnecting, as the `Stream` contract already says.
- **Each feed has its own loader**, because pacing and cooldowns are per
  provider and Yahoo's rate limit must not pace Rithmic. With Yahoo alone
  there is one, as now.
- **The chart's header names its feed** only when more than one is
  configured. With Yahoo alone nothing is drawn and the window is what it is
  today. A chart passed over a feed says so: "Yahoo · Rithmic not signed
  in".

#### Symbols stay the app's own

`Instrument` in `crates/omacharts-engine/src/symbols.rs` is already the
canonical identity ("`ES`, not `ES=F`"), and `Provider::symbol_for`
returning an `Option` is already both the translation and the test of
whether a provider serves an instrument. Nothing above the provider changes:
charts, watchlists, search and stored chartbooks keep naming an instrument
as they do, so switching feeds, or losing one, never changes what a chart
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

- **A root is the contract trading now, not stitched.** Rithmic's `ES` is
  the front month, with the history that contract has (about 90 days
  intraday). That is what a prop trader watches at 1m or 5m, and it never
  crosses a roll.
- **The feed says which contract is the front month, and says when it
  changes.** Rithmic is asked once per root with `RequestFrontMonthContract`
  and `need_updates`, so a roll arrives as a `FrontMonthContractUpdate` the
  moment Rithmic makes it: every chart of the root moves to the new
  contract, as does every order placed from one. The hourly refresh of the
  answer stays as the fallback for a push that never came.
- **When the feed cannot say, the engine's rule does.** Rithmic Test
  answers "no data" for every root; it lists the contracts, with each one's
  expiry. A feed that cannot name the front month hands the listing to
  `omacharts_engine::contracts::choose`, which applies the root's own roll
  rule. The rule lives in the engine, not in any feed's crate, so the next
  feed gets it for nothing.
- **The rules are data.** `contracts.tsv` carries, for each root, its month
  cycle and two dates in a small vocabulary: when a contract of a month
  stops trading, as the exchange's rulebook states it, and when the next
  contract becomes the front month. Equity index contracts roll on the
  Thursday eight days before the third-Friday expiry, where the volume goes
  over; contracts that are delivered (Treasuries, grains, metals, cattle,
  the softs) roll the business day before first notice day; energy, hogs,
  sugar and FX a few business days before the last trade. Business days
  are weekdays less the US exchange holidays. A root the table does not
  know is monthly, rolled eight days before its expiry. The rules are held
  to CME Group's and ICE Futures U.S.'s published calendars by
  `the_rules_give_the_exchanges_own_dates`.
- **At a roll the chart corrects itself.** The new contract's bars disagree
  with the cached tail of the old one, and the loader's `OVERLAP` check
  already treats that as history rewritten and fetches again.
- **Stitching, later.** When long daily charts from Rithmic matter, the
  provider fetches the earlier contracts back to the start and joins them on
  a fixed roll date, unadjusted, all inside its own `bars()`. Back-adjusting
  prices across rolls would be a chart option and never the default, because
  it moves prices a trader has drawn levels on.
- **Not a special case for daily.** Sending daily `ES` to Yahoo because
  Rithmic is not stitched would make the feed depend on the resolution and
  break "the first that serves it". Routing stays by instrument alone.

#### Dated contracts

A chart of `ESH7` itself: March 2027 of ES, and nothing else, until it
expires. Not a new kind: a dated contract is its root's instrument under
the contract's name, with the root's specification, hours and name, which
`SearchIndex::resolve` builds from the root whether or not anybody wrote
the contract down. `ESH7` is CME's own spelling, so it is neutral and what
every feed and broker starts from; each renders it its own way, Rithmic as
`ESH7`, Yahoo as `ESH27.CME` (with the exchange's Yahoo suffix: `CBT`,
`NYM`, `CMX`, `NYB`), thinkorswim as `/ESH7`. Typed as `ESH7`, `ESH27`,
`ESH2027` or `ES H7`, search and the commands read all four and store the
first. A one-digit year is read from last year to eight years on, so a
contract that expired lately is still what it was.

- **Search nests the contracts under their root, as TradingView does.** A
  futures root's row expands to the root's contracts, each with its month
  and expiry and the front month marked; picking one charts it. The listing
  is the feed's when the feed lists contracts (Rithmic's symbol search
  does, with expiries), and the rule's otherwise, which is what Yahoo alone
  shows; Yahoo serves the contracts themselves, so the expansion is there
  with no feed configured. Watchlists hold specific contracts as they hold
  anything.
- **The header of a dated contract shows its symbol and nothing more.**
  A chart of `ESH7` says `ESH7`, as any symbol says itself; search's fold
  is the way to another month.
- **An order from a chart of a dated contract goes to that contract**,
  whatever the root trades as now; `Session::contract_for` answers a
  dated contract with itself and a root with the front month.
- **A position outlives the roll.** Somebody holding `ESZ6` after the root
  rolls to `ESH7` sees the ESZ6 position on the ES chart still, named with
  its contract, priced from ESZ6's own trades, with a × that flattens
  ESZ6; the chart's own position line is for the contract trading now.
  Every month of the root a position is held in is shown that way, so a
  chart of `ESH7` shows a December position as held elsewhere too.

#### The Rithmic session

- **One connection per login.** A second sign-in on the same login forces
  the first one out, so each login has one `rithmic::Connection`, and it
  lends a data half and a trading half. The data login's data half is the
  one session every chart shares: CME bills a non-professional subscriber
  as a professional after two concurrent data sessions, and R | Trader Pro's
  plug-in mode, Rithmic's way around that, is Windows only. Any number of
  logins have a trading half, and only the data login a data half: a
  trading login hears the prices it marks and judges orders by on that
  one, borrowed, and opens none of its own for them. The bar
  widget asks the app rather than signing in itself.
- **Bars** come from Rithmic: minute bars from the same request as the
  minute ladders in stage 2, daily and weekly from its time bars.
- **The password goes in the desktop's keyring** (Secret Service, which
  gnome-keyring provides on Omarchy), under the login's id, never in the
  settings database: the database is backed up and can be copied to
  another machine. The logins themselves, each with its name, broker, user
  ID, system and gateway, are the `logins` setting, `Local` in
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
gets ladders exactly when its chart's feed has them; the volume profile
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
  yet; footprint and delta later) shows one line on a chart whose feed has
  none, "Needs a data feed with volume at price", rather than an invented
  estimate.

**Yahoo must stay exactly as fast.** Somebody who never configures anything
pays nothing for any of this. A chart holds ladders only when its feed has
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
- **A data feed** is a row on the Data page. Only one Rithmic login
  supplies market data, the first added unless the trader picks another;
  the others connect for orders, positions and P&L only. Several logins each
  streaming quotes could count as several data sessions under CME's
  two-session rule, and whether sessions with different firms count apart is
  not known.

**Preferences → Trading** has two sections. **Trading accounts** lists the
logins, each a row with its accounts under it, edited the way the Data page
edits feeds: "Add a trading account…" (Rithmic for now) opens the same login
dialog. Adding the first login also adds Rithmic as the first data feed,
through that login. Removing a login removes its accounts; if it was the
data login, Rithmic data moves to another login, or the Rithmic feed goes if
none is left. Nothing else on the Data page is touched.

```
Trading accounts
  Lucid                                            [Sign out] [Remove]
  LucidTrading · live · data
      1234   USD · $50,210                                  Default
      1235   USD · $48,900                             [Make default]
  Rithmic Test                      [Use for data] [Sign out] [Remove]
  Rithmic Test · live
      951    USD · $1,000,000                          [Make default]
  [ Add a trading account… ]

Order entry
  [ Edit quantities… ]

Chart
  Order labels                                              Left ▾
```

Each login is a row under Trading accounts, titled with its name, its
standing under it ("live", "not signed in", "signing in", or the broker's
words), with "data" on the data login; another login can be made it with
"Use for data". With no login, Trading accounts is its heading and "Add a
trading account…"; Order entry and Chart are there whether or not there
is a login, so they can be set before the first one. Chart's one row,
"Order labels", Left or Right, says which side of every chart the marks'
labels sit at, Left until changed; it is kept with the other settings,
and every chart redraws with it the moment it changes. The chart itself
is as it was with no login: no marks without an account.
Removing the data login asks about the data feed: another login takes it
over, or the trader chooses whether the Rithmic data feed goes too, or it
stays, not signed in, when it is the last one.

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

On the chart, every order and the position has a line at its price and
one label on it, in one piece: a tag filled in the line's colour, a body,
and a × as its end segment. The lines run from the chart's left edge to
the last bar and stop there, so the room after the last bar holds the
last price's line alone; scrolled back so the last bar is past the axis,
they run across the whole chart. The last price's line is drawn over
every mark's line, so it still reads with the price sitting on the
entry, and under every label. Where the labels sit is the trader's
choice (Preferences → Trading → Chart → Order labels):

- **Left**, the default: at the chart's left edge, the tag at the edge
  and the × towards the bars, the position's L and STP tabs after its ×.
  They never move with the bars; the oldest bars on screen are under
  them, and the axis chips say where each order is.
- **Right**: by the price axis, the position's tabs before its label.
  The labels float in the room after the last bar at their line's
  height, so with the chart scrolled until the last bar reaches the axis
  they sit at the lines' ends.

What the pointer finds follows the labels: a ×, a tab, and an order's
line, grabbed along its run or on its label, and the drag's preview is
drawn on the same side. The tag names the order's type, L for a limit
and STP for a stop, with the size: `▲ L 1` and `▼ STP 1` for a limit and
a stop that open or add to a position, the side in the shape, `L 1` and
`STP 1` for the ones that close some of it (`STL` is kept for a
stop-limit), whose side goes without saying, and
`▲ 5` and `▼ 2` for a long and a short position, the side in the same
shape (`ESH7 ▲ 1` for one held in another month). The body is money, in the ink: an exit's P&L
if it fills, the position's open P&L on a fill; an entry has none. An
order's price is on the axis, in its own chip; the position has none,
the last price's chip beside it saying enough.

Only the position's line is solid. Every order's is dashed — a limit and
a stop that open, a target (L) and a stop loss (STP) — and a stop's,
entry or exit, is heavier than a limit's, so the two read apart with the
same pattern: 1.5 points for a limit, 2 for a stop and for the
position, each scaled with the display. The position's STP tab is
outlined heavier than its L tab for the same reason. A theme of one's
own can still set each kind's pattern (Preferences → Theme → Trading).

A mark wears one colour, on its line, its outline, its tag, its × and its
chip, and the colour says one thing. Green and red are money's: a mark
with money to say, an exit or the position, wears the sign of it, so a
target is green, a stop red, and the position green or red as it stands,
an exit's chip filled. A mark with no money, a limit or a stop that opens or
adds, wears its side instead, in two colours the candles never use: the
theme's Blue for a buy and a long, its Amber for a sell and a short, the
chip outlined. The position wears its side too until the broker's P&L
has come. The header's Buy and Sell, and the panel's sides and sizes,
are in the same Blue and Amber, and the position's tabs in the colour
of the exit they would make. Every theme has the four, and each is set
by hand on a theme of one's own (Preferences → Trading). A working order's line can be dragged to
another price, and the position's label has an L and an STP tab beside
it, dragged out to place a target or a stop for the whole position.
While it is dragged it is a preview: its line in the order's look, its
label without the ×, the body saying what an exit would realise there,
the mark in the colour of that, so a target dragged under the entry
turns red, or where any other order would be and how many ticks it has
come; where it may not go with the market where it is, it says why and
letting go does nothing. The × cancels an order or flattens the position, on a press and
release without moving; a press on it that moves on drags the line as
the line does. A new exit is linked to the
other one working on the position, so whichever fills first cancels the
other, with whatever the broker has (`src/trading/linking.rs`): Rithmic
links working orders on its servers; Sierra Chart over DTC links two only
as they are submitted together, as one OCO pair, so a stop added to a
working target cancels the target and submits both as one pair; where no
safe order of steps exists (a target added to a working stop on Sierra
Chart) or the broker cannot link at all, the app holds the link while it
runs. The line joining the two exits says which: solid for the broker's,
dashed for the app's, faint until confirmed. Flattening cancels the exits
left behind. Tried on Rithmic Test on 2026-11-03. See *Linking exits* in
`sierra.md` for the reasoning.

Whatever is asked shows at once, faint, and at full strength only once the
broker's book confirms it: a new order once the exchange has it working
(not when Rithmic has merely received it), a market order as "Buying 1…"
beside the last price until its fill, a move once the order is reported
at its new price, a cancel once it is reported cancelled, a flatten once
nothing is held. A refusal takes it away with the broker's reason; a
request that is never answered stays pending rather than being taken as
done. Part of an order filled reads "L 1/2", and the position holds
only what filled. Toasts say only what the broker has confirmed.

Every order from a chart is for the chart's **quantity**, shown in its
header ("Qty 2") and picked there, or with Ctrl and a digit, from the
quantity template that applies to its instrument: a named list of
quantities for stocks or futures, optionally narrowed to some roots, the
narrowest winning (Preferences → Trading → Edit quantities…, or a right
click on the header's quantity). Out of the box stocks go 100 to 900 and
futures 1 to 9. Beside it, the header's **Buy** and **Sell** buy or sell
the quantity at market, in the theme's Buy and Sell colours (its Blue and
Amber, the chart's entries' own), and say why
not, greyed, when the chart cannot trade. Ahead of them, first on the
header's right, the header's **P&L**: the account's P&L for the day,
closed and open, marked to every trade ("P&L +$1,250.00" in the profit
colour, "P&L -$340.50" in the loss colour), and nothing until the broker
has said. The P&L, the two and the quantity lead the header's right, and
are there only while the chart has an account to trade on. With a trading account, Ctrl+B buys the quantity at market,
Ctrl+S sells it, and Ctrl+F cancels every
order working in the chart's contract and flattens it; the sidebars moved
to Ctrl+Shift+B and Ctrl+Shift+D, and watchlist N to Ctrl+Shift and a
digit, to make room. Tried on Rithmic Test on 2026-11-04: Ctrl+2 then
Ctrl+B on ES filled 2, and Ctrl+F left the account flat.

**No confirmations, for now.** Nothing asks first, on Rithmic Test or on a
live account: an order from the menu, a key or a drag, a move, a cancel and
a flatten go to the broker at once, and the pending look is the feedback.
Whether a live account should ask, and how, is an open decision, below.

#### The Log

The Log is what the data feeds, the brokers and the app have said since
the window started, and the one place a refusal or a failure is said. It
keeps the last 1000 lines in memory and nothing after the app quits, and
opens beside the charts with Ctrl+Shift+M (again to hide it) or the main menu's Log.

- Each line is its time, who said it (a feed or a login, by name, never a
  user ID) and what happened; a line with more to it has Details, the
  feed's or the system's own words. The same line said again by the same
  source is one line, counted.
- **All**, **Warnings** and **Errors** filter it; **Copy** puts the lines
  shown on the clipboard with their details; **Clear** empties it.
- An error while the Log is closed waits in the window's corner, the newest
  with "· N more" for the rest, until the Log is opened or the notice
  dismissed; after a dismissed one, a dot on the main menu's button.
- Toasts are only for confirmations, something asked for and done: an order
  working, filled or cancelled, exits linked. A refusal is in the Log, and
  so in the corner, and never a toast; something that needed nothing doing
  ("Already flat in ESZ6", "Not cancelled · already done") is a note in the
  Log and nothing else.
- Nothing in it is a credential: every login's user ID is concealed from the
  log as soon as it is known, whatever a broker repeats back, and no
  password or token is ever written.

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
on the next frame. Signing in from the Trading page goes through the same
`Desk::sign_in` and refreshes the same way, and signs in the data login's
data half too; so does a session coming back after a drop. Either way the
desk says how it went in the Log once, as it takes the answer. After that
everything follows Rithmic's pushes: no polling, no timers.

After a refresh, an order sent from here before a drop that the refreshed
book has never heard of, by its tag, did not reach the broker: it stops
being drawn as pending, and the Log says "Not placed · Buy Limit 1 ESZ6
never reached the broker". Moves, cancels and flattens are answered by the book as
always, and nothing is held as on its way to that login's accounts.

The Accounts tab shows the balance, the day's P&L, the open P&L, the loss
limit and the balance the account is liquidated at, as Rithmic says them;
shown, not enforced. The Orders tab shows the day's orders, not only this
session's.

**Open P&L is marked to the market, not taken from the PnL plant**, which
on Rithmic Test reports a held position's open P&L as zero however the
market moves. A position is worth (last − average) × size × what a point
is worth, and its last is, in order:

1. the last trade in that contract on its exchange, `MNQZ6` on CME, heard
   on the one Rithmic data session the app already has: the data login's
   data half, which the charts share, borrowed while a chart has it open.
   A trading login never opens a data session of its own for this, so no
   login has a second one beside the charts' (with one Rithmic login, it
   is that login's one session). The opening trade a watch hears can be
   the trader's own fill, so it marks zero, but only until the market
   trades again; every trade replaces it.
2. while that has heard nothing, or there is no such session open, the
   last price of a chart on screen showing that very contract, from any
   feed: an `MNQZ6` chart, never a continuous `MNQ`, whose price may be
   another month's or back-adjusted. A chart moving marks the position
   again, at most five times a second.
3. with neither, the broker's own figure.

The Positions tab, the chart's pill, the header's Day P&L and the
Accounts tab's Open P&L are all the one mark. An account's open and day
P&L are re-marked only when every position it holds has a price from 1
or 2; otherwise they are the broker's.

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
otherwise. Signing in at startup was one, and is decided: a saved login
signs in by itself after the first frame, as Signing in says.

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
5. **Does conformance open every firm's system?** Probably not. Rithmic's
   own answer was that each funding evaluator or broker decides which
   applications its users may use, and Tradeify publishes an allow-list.
   *Recommendation:* ask Rithmic whether a conformed app is blocked on a
   firm's system until the firm enables it, ask Lucid whether it would, and
   use nothing but Rithmic Test until a firm says yes.
6. **History depth for ladders.** *Recommendation:* decide after stage 1
   shows how far back Rithmic goes on a prop system. Test is not a guide.
7. **Confirmations before trading ships.** Nothing asks first now, on any
   account. *Recommendation:* decide before trading ships whether a live
   account asks before an order, a move, a cancel or a flatten, and how,
   so that a mis-click on a funded account cannot cost money unseen.
8. **Does a feed's setup text come back?** The sign-in pages once showed
   each feed's own paragraph on what signing in involves, what it needs and
   what it may do; nothing showed it, and Rithmic's said it could not place
   an order, so it was removed. *Recommendation:* leave it out unless a
   page turns out to need it, and then write it beside the feed again.

## Not in scope

- Automated strategies. They are non-display use under CME's licensing,
  which needs a licence of its own.
- Other brokers. Interactive Brokers comes later, through its own crate,
  behind the same account boundary trading introduces in stage 4.
- Windows or macOS.
