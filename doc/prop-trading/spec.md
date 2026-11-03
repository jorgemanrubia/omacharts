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

### 1. A Rithmic account in Preferences

A **Trading** section in Preferences, after Data, with one group for
Rithmic.

- **What it asks for:** user ID, password, system and gateway. System and
  gateway start as Rithmic Test and its URL; somebody with a prop-firm login
  types theirs, as the firm's welcome email gives them.
- **Sign in** connects, and a row says what is happening while it does:
  connecting, asking for accounts, asking for history. A refusal shows
  Rithmic's own words ("permission denied").
- **Once signed in** it shows:
  - the accounts, with each one's balance and profit when the PnL plant
    gives one;
  - the front-month ES contract it is watching, with its last trade and the
    best bid and offer, updating as they arrive;
  - how much history there was: "1120 minute bars in the last day".
- **Sign out** ends the session.
- **The password goes in the desktop's keyring** (Secret Service, which
  gnome-keyring provides on Omarchy), never in the settings database: the
  database is backed up and can be copied to another machine. User ID,
  system and gateway are settings, and they are `Local` in
  `src/setting_keys.rs`, because a login belongs to the machine it was typed
  on.
- No command yet. This stage is exploratory, and the CLI rule in
  `AGENTS.md` applies from stage 2, when there is something to script.

Done when: the author signs in with `~/.rithmic`'s login from the dialog and
sees account 951, ES moving and the history count.

### 2. Rithmic as a chart feed

Rithmic joins the feeds on the Data page, next to Yahoo and thinkorswim.

- **History** comes from the history plant's time bars, which carry volume
  and bid and ask volume. Live bars come from the ticker plant's trades.
- **Symbols.** A chart on a continuous future (`ES`, `NQ`, `CL`) is served
  the front month, asked of Rithmic. A specific contract (`ESZ6`) is served
  as written.
- **One session for the whole app.** CME bills a non-professional
  subscriber as a professional after two concurrent data sessions, and
  R | Trader Pro's plug-in mode, Rithmic's way around that, is Windows only.
  So every chart, the settings page and later the order ticket share the
  session stage 1 opens, and the bar widget asks the app rather than signing
  in itself.
- Commands for signing in and out, and the session's state, follow the
  feeds' existing commands.

### 3. Better volume

Accurate volume profiles and footprints from Rithmic's own data instead of
bars:

- volume-profile minute bars (bid and ask volume at each price, each
  minute) for history;
- live trades, with their aggressor, from then on.

This needs a data type beside `Bar`, which the engine does not have. It gets
a spec of its own when stage 2 is done.

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

Ask Rithmic for the process and the terms once stage 2 works, because a
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
5. **History depth for profiles.** *Recommendation:* decide after stage 2
   shows how far back Rithmic goes on a prop system. Test is not a guide.

## Not in scope

- Automated strategies. They are non-display use under CME's licensing,
  which needs a licence of its own.
- Other brokers. Interactive Brokers comes later, through its own crate,
  behind the same account boundary trading introduces in stage 4.
- Windows or macOS.
