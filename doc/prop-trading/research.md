# Rithmic for Omacharts: research notes for a spec

Researched 2026-11-03. Nothing was logged in to, no account was contacted, `~/.rithmic` was not read.

Source quality varies, so each claim is tagged:
- **[primary]**: Rithmic, CME, a prop firm's own help centre, or protocol definitions / library source code.
- **[secondary]**: an established vendor or forum (Ironbeam, Optimus, NinjaTrader forum, Linn Software).
- **[tertiary]**: SEO and affiliate sites (pickmytrade, propfirm review blogs). Many read as machine-written. Use them as leads, not as facts.

---

## 0. How this lands in Omacharts today

`crates/omacharts-engine/src/provider.rs` defines `Provider` (polled `bars()`, plus `capabilities`, `pacing`, cooldowns, `retire()`) and an optional `Stream` (`subscribe(symbol, timeframe, since, sink)` delivering `Update::{Snapshot, Bar, Lost}`). Implementations live in `crates/omacharts-engine/src/providers/{yahoo,tos,synthetic.rs}`. The thinkorswim transport sits in its own crate, `crates/tos-market`. `ProviderError::NeedsSetup` and `FetchFailure::NeedsSignIn` already cover "feed needs a login".

What is missing for Rithmic:
- `Bar` is `{ts, open, high, low, close, volume}`. It has no bid/ask (aggressor) volume and no per-price volume, and nothing in the engine models trades or ticks. Accurate profiles and footprints need a new data type, either a trade stream or a per-bar price ladder, beside `Bar`.
- `Stream` only carries bars. A ladder/DOM needs quotes and depth, which would be a second stream kind.
- Trading (orders, positions, PnL, account risk) does not belong behind `Provider`, which is a market-data boundary. It needs a new broker/account boundary.
- A `crates/rithmic` crate would mirror `crates/tos-market`, either wrapping `rithmic-rs` or owning its own transport.

---

## 1. Rithmic's APIs for a third-party app

### 1.1 The options

| API | What it is | Fit for Omacharts |
|---|---|---|
| **R\|Protocol API** | Wire spec: Google Protocol Buffers over secure WebSockets. Any language, any OS. Rithmic says it was aimed at mobile and web. | **The only one that fits a Rust/Linux app.** |
| **R\|API+** | C++ and .NET libraries. Adds server-side trailing stops, brackets, OCOs, and custom tick/volume/range bars. | Would mean FFI to a C++ library. Linux builds exist for Diamond, but R\|API+ Linux availability was not confirmed. |
| **R\|Diamond API** | R\|API+ plus direct connections to exchange-facing gateways and market-data handlers. Linux C/C++, co-location, extra gating. Built for HFT. | Overkill, and gated. |

No newer public API turned up. R\|Protocol 0.89.0.0 (template 5.42) is the version `rithmic-rs` targets.

Sources: [Rithmic APIs page](https://rithmic.com/apis) (Wix site, the page did not render for fetching; content from search snippets) [primary]; [Ironbeam: Rithmic APIs](https://www.ironbeam.com/rithmic-api-futures-trading/) [secondary]; [pickmytrade API overview](https://pickmytrade.io/help/rithmic/rithmic-api-overview/) [tertiary].

### 1.2 Existing open-source clients

| Project | Lang | Licence | State (GitHub, Nov 2026) | Notes |
|---|---|---|---|---|
| [pbeets/rithmic-rs](https://github.com/pbeets/rithmic-rs) ([crates.io](https://crates.io/crates/rithmic-rs), [docs](https://docs.rs/rithmic-rs)) | Rust | MIT OR Apache-2.0 (repo reports Apache-2.0) | v3.3.0, pushed 2026-10-19, 61 stars | tokio actor per plant: Ticker, Order, History, PnL, Repository. Bracket, OCO, modify, cancel, exit position, trade routes; history `load_ticks_all` / `load_time_bars_all` / `load_volume_profile_minute_bars`; agreement acceptance. Uses `native-tls` only (no rustls). MSRV 1.85. Does not reconnect on its own (example loop provided). "Used to trade live capital." Ships prost-generated `src/rti.rs`, so the protos are effectively redistributed. |
| [BurnOutTrader/ff_rithmic_api](https://github.com/BurnOutTrader/ff_rithmic_api) | Rust | MIT | Pushed 2025-09, 17 stars | Lower-level, prost + tokio. The author points to "tick-trade" as a better implementation. |
| [rundef/async_rithmic](https://github.com/rundef/async_rithmic) ([docs](https://async-rithmic.readthedocs.io)) | Python | MIT | Pushed 2026-10-09, 128 stars | Ships the `.proto` sources in the repo (`async_rithmic/protocol_buffers/source/*.proto`). Includes a `conformance.py` helper. |
| [jacksonwoody/pyrithmic](https://github.com/jacksonwoody/pyrithmic) | Python | MIT | Pushed 2024-08 | Older. |
| [rithmic-rapi on PyPI](https://pypi.org/project/rithmic-rapi/) ("pyrithmic", a different project) | Python | Apache-2.0 | 2026 | States the `.proto` files are **proprietary to Rithmic, LLC and not redistributed**, and that you must get them through your own Rithmic developer relationship. |

**Licence caveat:** the client code is permissively licensed, but whether Rithmic allows the `.proto` definitions (or code generated from them) to be redistributed is disputed between projects. For an open-source app this needs a written answer from Rithmic.

### 1.3 Plants (one WebSocket and one login per plant)

`RequestLogin.infra_type` enumerates `TICKER_PLANT`, `ORDER_PLANT`, `HISTORY_PLANT`, `PNL_PLANT` and `REPOSITORY_PLANT`. Login carries `user`, `password`, `app_name`, `app_version`, `system_name`, plus `mac_addr`, `os_version` and `os_platform`. ([request_login.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/request_login.proto)) [primary: protocol definition]

- **Ticker plant (live market data).**
  - `RequestMarketDataUpdate` update bits: LAST_TRADE, BBO, ORDER_BOOK, OPEN, HIGH_LOW, CLOSE, SETTLEMENT, OPEN_INTEREST, price limits, projected settlement, and more.
  - `LastTrade` has `trade_price`, `trade_size`, an **`aggressor` (BUY/SELL)**, exchange order IDs, `volume`, `vwap`, and timestamps to the microsecond plus the exchange source time to the nanosecond.
  - `OrderBook` is market-by-price: price, size and **order count** per level, plus implied sizes.
  - `DepthByOrder` is **market-by-order**: NEW/CHANGE/DELETE per order, with order priority and exchange order ID. Quantower exposes this as "MBO mode".
  - Also symbol search, front-month lookup, reference data and product codes.

  Sources: [last_trade.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/last_trade.proto), [depth_by_order.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/depth_by_order.proto), [Quantower Rithmic connection](https://help.quantower.com/connections/connection-to-rithmic).

- **History plant.**
  - Tick-bar replay (TICK / RANGE / VOLUME bars; a 1-tick bar is a single trade).
  - Time-bar replay (SECOND / MINUTE / DAILY / WEEKLY).
  - Volume-profile minute bars.
  - Live bar updates (tick bars and time bars).
  - Both bar responses carry **`bid_volume` and `ask_volume`** (see section 5).
  - Limits seen by `rithmic-rs`:
    - Replies are capped at 10,000 records unless `resume_bars` is set.
    - The server cuts a reply after about 4 seconds of streaming and expects the client to ask it to continue.
    - Time-bar replays sometimes stop early without warning.
    - An empty volume-profile reply is not always real.
    - Times are i32 Unix seconds (overflow in 2038).
    - A full 23-hour ES session runs to hundreds of thousands of tick records.
  - **No historical depth / order-book replay** in the protocol.

  Sources: [rithmic-rs history_plant.rs](https://github.com/pbeets/rithmic-rs/blob/main/src/plants/history_plant.rs), [response_tick_bar_replay.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/response_tick_bar_replay.proto).

- **Order plant.**
  - `RequestNewOrder`: LIMIT, MARKET, STOP_LIMIT, STOP_MARKET, MIT, LIT; DAY / GTC / IOC / FOK; native trailing stop (`trail_by_ticks`); timed release and cancel; if-touched conditions; `manual_or_auto` flag.
  - **`RequestBracketOrder`** (server-side) supports:
    - Target and/or stop by ticks, either following the order (bracket) or static.
    - Break-even ticks and break-even trigger.
    - Trailing-stop trigger.
    - OCO-style `order_operation_type`.
  - Modify / update the stop or target bracket level, cancel, cancel-all, **exit position** (flatten).
  - Show orders, brackets, fill history, order history, replay executions.
  - Account list, **account RMS info**, product RMS info, trade routes.
  - `rithmic-rs` also exposes OCO and order linking.

  Sources: [request_bracket_order.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/request_bracket_order.proto), [rithmic-rs src/api/commands](https://github.com/pbeets/rithmic-rs/tree/main/src/api/commands).

- **PnL plant.** `AccountPnLPositionUpdate`: `open_position_pnl`, `closed_position_pnl`, `day_pnl`, `day_open_pnl`, `day_closed_pnl`, `net_quantity`, `account_balance`, `margin_balance`, `min_account_balance`, `percent_maximum_allowable_loss`, buying power. Per-instrument position updates as well. ([account_pnl_position_update.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/account_pnl_position_update.proto))

- **Risk (via the order plant).** `ResponseAccountRmsInfo`: `loss_limit`, `max_order_quantity`, `buy_limit` / `sell_limit`, `min_account_balance`, auto-liquidate flag / threshold / criteria, `disable_on_auto_liquidate`, account status. These are the server-side limits a prop firm sets, so the app can display them without hard-coding firm rules.

- **Repository plant.** List, show and accept Rithmic agreements; market-data self-certification (non-professional or professional). Without this, a new user's API login fails until they have signed in R\|Trader Pro.

### 1.4 Systems, gateways, URLs

- A **system** is a named Rithmic environment: "Rithmic Test", "Rithmic Paper Trading", "Rithmic 01", or a firm's own system such as "Apex", "TopstepTrader", "LucidTrading", "Earn2Trade" or "Tradesea". A **gateway** is a regional server ("Chicago Area", "Aurora Area", "Europe", "Frankfurt", "Singapore", "Tokyo" and so on).
  - R\|Trader Pro v17.106 showed 31 systems and 18 gateways for Rithmic 01. [pickmytrade server list](https://pickmytrade.io/help/rithmic/rithmic-server-list/) [tertiary, but a dated observation]
- **The protocol can list them itself:**
  - `RequestRithmicSystemInfo` returns system names, sent before login.
  - `RequestRithmicSystemGatewayInfo` returns a system's `gateway_name[]` and `gateway_uri[]`.
  - So Omacharts can fill a "System" dropdown from the server instead of shipping a list. (fields in [rithmic-rs src/rti.rs](https://github.com/pbeets/rithmic-rs/blob/main/src/rti.rs))
- **How a user picks:** the prop firm's welcome email or help page names the system and gateway, and the user selects them in the platform.
  - Apex: "Apex" system, Chicago Area.
  - Bulenox: "Rithmic Paper Trading", Chicago.
  - BluSky, Elite Trader Funding and Take Profit Trader use the generic "Rithmic Paper Trading" system.
  - Firms with dedicated systems since 2024 include 4PropTrader, DayTraders.com, 10XFutures, Lucid, Thrive, Legends, Earn2Trade, **Tradesea (Oct 2025)** and Halcyon (Apr 2026).

  Sources: [Linn Software: Rithmic prop firm support](https://www.linnsoft.com/support/rithmic-prop-firm-support) [secondary].
- **URLs:**
  - Test: `wss://rituz00100.rithmic.com:443`, system "Rithmic Test" ([async_rithmic connection docs](https://async-rithmic.readthedocs.io/en/latest/connection.html)).
  - A user reported `wss://rprotocol.rithmic.com:443` for Paper Trading ([async_rithmic #24](https://github.com/rundef/async_rithmic/issues/24)).
  - Production URLs are given out **only after conformance**. `rithmic-rs` asks for a primary and an alternate ("beta") URL.

---

## 2. Getting access

### 2.1 Developer process (as reported; Rithmic does not publish it)

1. **Contact Rithmic** at `apihelp@rithmic.com` or through the [API inquiry form](https://www.rithmic.com/support/contact/form?category=api). Rithmic asks whether the app is **for personal use or a product offered to customers**, what the `app_name` is, and the language and deployment. ([async_rithmic #42](https://github.com/rundef/async_rithmic/issues/42), [pickmytrade API overview](https://pickmytrade.io/help/rithmic/rithmic-api-overview/))
2. **Dev kit and Test credentials.** You receive Rithmic Test username, password and URL, plus a **4-character prefix** for `app_name` (e.g. `abcd:Omacharts`). Reported turnaround: 1–2 business days. No conformance is needed to use Test, and Test is described as "very limited". ([#42](https://github.com/rundef/async_rithmic/issues/42), [Elite Trader thread](https://www.elitetrader.com/et/threads/rithmic-api-for-python.334570/page-5))
3. **Conformance.** What the open-source authors report:
   - You connect to the **order plant on Rithmic Test** with the prefixed app name and leave it running; Rithmic checks it from their end.
   - async_rithmic's author believes they mainly verify regular heartbeats and a well-behaved session.
   - Some report a questionnaire ("submit their conformance questions").
   - Reported time: about 3–5 business days, "pretty quick".

   No public checklist exists. Rithmic's own description (via search snippets) is "basic functionality and stability should be complete before requesting conformance". ([#24](https://github.com/rundef/async_rithmic/issues/24), [#42](https://github.com/rundef/async_rithmic/issues/42), [async_rithmic docs](https://async-rithmic.readthedocs.io/en/latest/connection.html#conformance))
4. **Production.** After conformance, Rithmic whitelists the `app_name` and gives production URLs. Live credentials still come from the FCM, broker or prop firm. Ironbeam describes the same order: dev kit, then Test, then conformance, then production credentials. ([Ironbeam](https://www.ironbeam.com/rithmic-api-futures-trading/))

### 2.2 Costs

- **API / dev kit:** no public price list. A 2019–2020 forum post reports **$100/month for API access to Paper Trading** ([Elite Trader](https://www.elitetrader.com/et/threads/rithmic-api-for-python.334570/page-5)). Unverified and possibly stale.
- **Broker examples** (Ironbeam): $25/month Rithmic connection fee, $0.10 per contract routing fee, non-professional L1 from $3/exchange/month, full CME L2 bundle $41/month. Ironbeam also says "all APIs include historical tick data back to December 2011". ([Ironbeam](https://www.ironbeam.com/rithmic-api-futures-trading/)) [secondary]
- **Prop firms** usually bundle data. Tradeify: "does not charge data fees", but the user must sign the non-professional data agreement on first login, or they get delayed quotes. ([Tradeify platforms post](https://tradeify.co/post/futures-prop-trading-platforms-tradeify)) [primary]

### 2.3 CME licensing

- **Display vs non-display.** Showing charts, a DOM or time and sales to a human is *display* use, licensed through the distributor (here Rithmic and the FCM or prop firm), with the subscriber self-certifying as non-professional or professional. **Non-display** use, such as automated or semi-automated trading systems consuming the data, needs a direct CME Information License Agreement (ILA). ([CME ILA guide](https://www.cmegroup.com/market-data/files/information-license-agreement-ila-guide.pdf), [Tickblaze CME FAQ](https://www.tickblaze.com/cme-market-data-faq/))
  - Implication: a charting and manual-trading app that relies on the user's Rithmic entitlement does not appear to need its own ILA. Automation features (auto-brackets are fine, since they run on Rithmic's servers; strategies are not) could pull in non-display licensing.
- **Non-professional limits:**
  - Fees are charged per device (session), per exchange, per month.
  - CME allows a non-professional at most **two concurrent market-data sessions** per distributor. A third pushes the user to professional rates (about $105/exchange or $420 for the bundle, multiplied per session, under the 2020 rule).
  - Rithmic's mitigation is **R\|Trader Pro plug-in mode**: other apps on the same machine connect through R\|Trader Pro and count as one session.
  - Sources: [Optimus: CME fees, users and logins](https://community.optimusfutures.com/t/cme-market-data-fees-update-users-logins-and-data/3450), [Optimus plug-in article](https://optimusfutures.com/blog/r-trader-pro-plugin-how-to-avoid-professional-fees-with-concurrent-logins/), [Topstep help](https://intercom.help/topstep-llc/en/articles/8284189) [secondary]
- **CME list prices, Jan 2026** (as reported; the CME PDF did not download here): non-pro top of book $1.55/exchange or $4.65 for the 4-exchange bundle; non-pro depth $12.10/exchange or $36.50 for the bundle. ([pickmytrade fee page](https://pickmytrade.io/help/rithmic/rithmic-exchange-data-fees/) citing the [CME Jan 2026 fee list](https://www.cmegroup.com/market-data/files/january-2026-market-data-fee-list.pdf)) Brokers mark these up.
- **Linux problem:** R\|Trader Pro desktop is **Windows-only** (a web version exists at `rtraderpro.rithmic.com/rtraderpro-web`). Plug-in mode is therefore not available on Omarchy. An Omacharts login is a direct session that counts toward the two-session limit. A user also running R\|Trader web or a phone app at the same time could become billable as professional. ([pickmytrade system requirements](https://pickmytrade.io/help/rithmic/rithmic-system-requirements/)) [tertiary on the web URL]

### 2.4 What prop-firm users need

- **Sign Rithmic agreements and the market-data self-certification first.** This is usually done in R\|Trader Pro, but the repository plant can do it over the API. Without it, an API login fails even with valid credentials. ([async_rithmic #24](https://github.com/rundef/async_rithmic/issues/24), [DayTraders: signing agreements](https://help.daytraders.com/en/articles/10159643-signing-your-rithmic-agreements), [rithmic-rs Repository plant](https://github.com/pbeets/rithmic-rs#repository-plant))
- **Use the right system and gateway**, and the firm-issued Rithmic user ID, which is not always the firm's portal login.
- **Third-party API entitlement.** Reports say a user ID can log in to R\|Trader Pro yet get "permission denied" on R\|Protocol, and that free Rithmic trial IDs cannot use R\|Protocol at all. ([async_rithmic #24](https://github.com/rundef/async_rithmic/issues/24), [Sierra Chart support board](https://www.sierrachart.com/SupportBoard.php?ThreadID=107178), [pickmytrade access-denied article](https://blog.pickmytrade.io/rithmic-access-denied-api-access-level-error-fix/) [tertiary])
- **The firm must allow the platform** (section 4).

---

## 3. Tradesea and comparable platforms

### 3.1 Tradesea

- **What it is.** A web and mobile (iOS; Android in beta) futures platform on Rithmic. TradingView-powered charts with chart trading (drag SL/TP), a DOM ladder with one-click entry and draggable orders, and three entry modes (Scalp / DOM / Order). Brackets, OCO and trailing stops. Multi-account management with an account selector. A lead/follower copier. Personal daily loss and profit limits, position risk settings, and **personal lockouts**. The **Compass** journal auto-logs and tags every fill. **Polaris** is an AI assistant wired to the user's own trades.
- **Distribution.** Only through partner prop firms and brokers, with pricing set by the firm. It has its **own Rithmic system** (since Oct 2025). Partners reported in 2026: Lucid, Tradeify, BluSky, FundedSeat (add-on fee), Halcyon, EdgeProp, Onyx, Tradentry, YRM Prop.
- **Onboarding.** Account Centre, then Connect New, then pick the firm, enter the Rithmic credentials, pick Simulation or Live, and verify status.
- **What stands out:** zero install; a TradingView-like workspace instead of a legacy DOM tool; one place for execution, journal and AI; multi-prop-account handling; risk lockouts.
- **Reported weaknesses:** mobile lag, bracket slippage, a thinner order-flow toolset (no footprint or volume-profile emphasis), Rithmic only, and it does not enforce firm payout or consistency rules.

Sources: [Rithmic platforms page](https://www.rithmic.com/platforms), [tradingfunder Tradesea](https://tradingfunder.com/platforms/tradesea/), [damnpropfirms review](https://damnpropfirms.com/best-prop-firm-trading-platforms/tradesea-review/), [propfirmapp review](https://propfirmapp.com/trading-tools/tradesea), [Linn Software](https://www.linnsoft.com/support/rithmic-prop-firm-support). [mostly tertiary; feature list consistent across sources]

### 3.2 Others

| Platform | Rithmic onboarding | Notable for prop traders |
|---|---|---|
| **Quantower** | Connection screen: user, password, server ("Rithmic Paper Chicago" / "Rithmic Aurora Chicago" or a firm's system); optional "Use RTrader" plug-in mode; "Enable MBO" toggle. ([help](https://help.quantower.com/connections/connection-to-rithmic)) | DOM Trader, chart trading, footprint/cluster and volume analysis (paid tier), multi-account. Free for many prop firms (Tradeify unlocks it with Tradesea). |
| **Volumetrica / DeepCharts** | Select Rithmic, use the firm credentials. Bundled free by some firms (FundedSeat, Phidias, YRM). ([tradingfunder](https://tradingfunder.com/platforms/volumetrica/)) | Order-flow first: volume profile, footprint, DeepMap heatmap, T&S, web version, copier. Volumetrica also sells prop-firm risk infrastructure. |
| **ATAS** | Rithmic connection with firm credentials; plug-in mode supported. | Footprint/cluster charts, smart DOM, volume profile, Smart Tape. |
| **Bookmap** | Rithmic, plug-in mode. Loads only **about 1 hour** of Rithmic backfill; deeper history needs dxFeed ($14/month) or locally recorded data. ([Bookmap forum](https://bookmap.com/forum/viewtopic.php?p=6193)) | Depth heatmap (needs live MBO/MBP), recorded locally over time. |
| **MotiveWave** | Rithmic: historical ticks yes, full depth yes, server-side orders yes. ([docs](https://docs.motivewave.com/knowledge-base/connection/connection-details)) | DOM, chart trading, Linux/macOS desktop builds (Java). |
| **NinjaTrader 8** | Rithmic connection (often in plug-in mode). History comes from **NinjaTrader's own servers** (about 1 year of ticks, minutes since about 2006) rather than Rithmic. ([NT forum](https://forum.ninjatrader.com/forum/ninjatrader-8/platform-technical-support-aa/1234283-the-definitive-guide-to-historical-data)) | **ATM strategies** (bracket templates with auto-breakeven and trail), SuperDOM, chart trader, Flatten Everything, hotkeys. The de facto standard prop traders compare against. |
| **TradingView** | **No native Rithmic broker.** Prop traders use Tradovate-based accounts, or webhook bridges (PickMyTrade and others) for Rithmic. ([pickmytrade blog](https://blog.pickmytrade.io/rithmic-automation-connect-tradingview-to-rithmic-in-2026/)) | Charting standard; chart trading through Tradovate. |
| **R\|Trader Pro** (Rithmic's own) | Windows desktop plus web version. | Baseline DOM, PnL, risk view; plug-in hub. |

### 3.3 Core trading features prop traders expect

Synthesised from the platforms above.

- **Account and connection:** account selector, multi-account view, clear SIM / LIVE / evaluation labelling, connection status, and agreement / self-certification handling.
- **DOM / price ladder:** bid/ask sizes, last trade, volume-at-price column, working orders on the ladder, click-to-place, drag-to-modify, a cancel column.
- **Chart trading:** place, drag and cancel orders and brackets on the chart; position line with live open PnL.
- **Brackets / ATM:**
  - Saved bracket templates: target and stop in ticks, multiple targets, auto-breakeven, trailing stop.
  - Rithmic's server-side brackets cover most of this, so protection survives a disconnect.
  - OCO.
- **Flatten / exit:** a flatten button per instrument and account (Rithmic `exit_position`), cancel-all, and "flatten everything".
- **Positions panel:** quantity, average price, open / closed / day PnL.
- **Orders and fills panels.**
- **Risk display:**
  - Day PnL against the daily loss limit.
  - Distance to the trailing-drawdown threshold.
  - Max contracts.
  - Personal lockouts: stop trading after a loss or profit target (Tradesea, NinjaTrader, Volumetrica).
- **Hotkeys** for buy/sell market, join bid/ask, flatten, reverse, and cancel all.
- **Order confirmation** toggles (on by default for live accounts, often off for scalping), plus quantity presets.
- **Journal / trade review:** a differentiator (Tradesea Compass), not a baseline.

---

## 4. Prop-firm constraints

### 4.1 Allowed platforms (and changes)

- **Topstep:** since Aug 1, 2025, new Combines and resets are **TopstepX only**. Rithmic is legacy. Not a target for a new Rithmic platform. ([Topstep help](https://help.topstep.com/en/articles/11667498-important-changes-to-topstep-supported-platforms)) [primary]
- **Apex:** supports Rithmic, Tradovate and WealthCharts in 2026. The platform is fixed at purchase. On Rithmic, the user picks any Rithmic-compatible front end (NinjaTrader, Sierra, Bookmap, ATAS, Quantower and so on). ([traderspost](https://blog.traderspost.io/article/best-apex-trader-funding-supported-platforms-for-prop-traders)) [tertiary; check Apex's help centre, which returned 403]
- **Tradeify:** lists Tradovate, WealthCharts, **Tradesea**, Sierra Chart, Quantower, TradingView (via Tradovate) and NinjaTrader (Elite Live only). Choosing Tradesea at checkout "also unlocks Sierra Chart and Quantower" because they connect through Rithmic. **An explicit allow-list:** an unlisted Rithmic front end may not be permitted. ([Tradeify](https://tradeify.co/post/futures-prop-trading-platforms-tradeify)) [primary]
- ProjectX white-label platforms at Bulenox, Tradeify, Lucid, Alpha and Phidias **shut down in Feb 2026**, and their traders moved to Tradovate, Rithmic or NinjaTrader. (search summary of [Topstep wiki](https://en.wikipedia.org/wiki/Topstep) and others) [tertiary]

### 4.2 Typical rules (examples)

**Tradeify (help centre):** [primary] ([rules overview](https://intercom.help/tradeify/en/articles/12268167-essential-trading-rules-overview))
- **Drawdown:** end-of-day trailing drawdown, but a breach is checked in real time.
- **Daily loss limit:** a soft breach that pauses trading until 6 PM ET. Growth $50k: $1,250. Select Eval: none.
- **Consistency:**
  - Select Eval: 40%.
  - Growth Funded: 35%.
  - Lightning: 20/25/30% by payout number.
- **Max contracts:** $50k: 4 minis or 40 micros; $150k: 12 minis or 120 micros.
- **Flat by 4:45 PM ET:** no overnight positions.
- **News:** allowed.
- **Microscalping:** on funded accounts, more than 50% of trades and profit must come from positions held over 10 seconds.
- **No hedging:** including across accounts.
- **Automation:** allowed only if the trader owns the strategy; no HFT.

**Apex (4.0, March 2026):** [tertiary; verify]
- **Drawdown:** EOD trailing or intraday trailing (intraday trails unrealised peak).
- **Daily loss limit:** EOD accounts only; $50k: $1,000; pauses trading, does not fail the account.
- **Consistency:** funded (PA) consistency rule raised from 30% to 50%.
- **Rules removed:** MAE, 5:1 R:R, and others.
- **Scaling:** tiered contract scaling in PA; half size until the threshold is cleared (pre-4.0 rule, may have changed).
- **News:** normal trading during news allowed, "news gambling" (orders on both sides) is not.

Sources: [tanto Apex rules](https://tanto-website.pages.dev/learn/apex-trader-funding-rules-what-you-need-to-know), [dev.to consistency change](https://dev.to/acesley_chan_5b46dfd6bd24/apex-just-doubled-its-consistency-rule-from-30-to-50-heres-who-gets-richer-and-who-gets-culled-4n5i).

### 4.3 What a platform shows or enforces

- **Server-side, by the firm or Rithmic RMS:** loss limit / auto-liquidation, max order quantity, buy/sell limits, minimum balance, account disable. The platform **reads these** (`ResponseAccountRmsInfo`, PnL `percent_maximum_allowable_loss`, `min_account_balance`) and shows them. Breach handling stays with the firm.
- **Commonly shown client-side:**
  - Day PnL against the daily loss limit.
  - Balance against the drawdown threshold, which needs the firm's trailing formula, so it is often approximate.
  - Max contracts.
  - Time to the mandatory flat time.
- **Commonly enforced client-side (optional, personal):** lockouts after a loss or profit target, max position size, blocking orders that would exceed contracts, confirmations. Tradesea and NinjaTrader do this. Third-party sources say Tradesea does **not** enforce consistency or payout rules.
- **Rarely or never done by platforms:** consistency %, microscalping, hedging across accounts. These are payout-review rules computed by the firm.

---

## 5. Data for accurate volume profiles and footprints

From the protocol definitions:
- **Every trade with its aggressor.** History tick replay (1-tick bars) gives `volume`, `bid_volume` and `ask_volume`; the live `LastTrade` gives `aggressor` BUY/SELL. Per-trade bid/ask attribution is enough for exact volume-at-price, delta, and footprint (bid × ask per price).
- **Volume-profile minute bars (server-built).** Each minute carries `profile_price[]`, `profile_bid_volume[]`, `profile_ask_volume[]`, `profile_no_aggressor_volume[]`, and trade counts per side. This is a ready-made footprint per minute with far less data than raw ticks. It is the cheapest way to build accurate session or composite profiles and 1-minute-or-coarser footprints.
- **Time bars** (second / minute / daily / weekly) also carry `bid_volume` and `ask_volume` per bar, giving delta per bar.
- **Live:** keep the profile current from `LastTrade` (aggressor) and/or live tick-bar updates.
- **No depth history.** Heatmaps and historical DOM need locally recorded depth (Bookmap's approach) or a vendor such as dxFeed.

Sources: [response_tick_bar_replay.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/response_tick_bar_replay.proto), [response_time_bar_replay.proto](https://github.com/rundef/async_rithmic/blob/main/async_rithmic/protocol_buffers/source/response_time_bar_replay.proto), [rithmic-rs rti.rs ResponseVolumeProfileMinuteBars](https://github.com/pbeets/rithmic-rs/blob/main/src/rti.rs).

**How far back, and quotas:**
- **How far back:**
  - Ironbeam says tick history goes back to Dec 2011.
  - A MultiCharts test found minute data from Dec 2011 and ticks from Feb 2013.
  - A Bookmap forum post says Rithmic holds about a month of tick volume.
  - Old (2015) MultiCharts reports say one day of ticks and about 20 days of minutes.

  These conflict; see the uncertain section.
- **Weekly quota:** Rithmic enforces a **weekly allotment of historical data**. One user hit a 40 GB cap in 3 days by re-pulling a month of ticks on every launch, and history was then suspended for the rest of the week. ([Optimus forum, Aug 2020](https://community.optimusfutures.com/t/rithmic-historical-data-download/3727))
- **Design consequence:** Omacharts must **cache ticks / profile bars locally and fetch only the gap**. The existing price cache and `Pacing` design fit this, but the cache has to store a richer record than `Bar`.

---

## What's uncertain

1. **Conformance contents and cost.** No public checklist or price list exists. The process comes from open-source authors' recollections, one of them an account that reads as possibly machine-written ([#42](https://github.com/rundef/async_rithmic/issues/42)). The $100/month figure dates from about 2020.
2. **"Personal use" vs "product offered to customers."** Rithmic asks which one applies. A distributed app used by many people probably falls under different (vendor/ISV) terms, fees or contracts than one person's tool. Nothing public describes those terms.
3. **Licence on the `.proto` files.** async_rithmic and rithmic-rs redistribute them (as source or generated code); rithmic-rapi says they are proprietary. This matters for an open-source repo.
4. **Whether a conformed `app_name` works on every prop-firm system** or each firm must enable it, and whether firms with allow-lists (Tradeify) would refuse Omacharts logins or treat them as a rule breach. Tradesea's model (firm partnerships plus its own system) suggests that firm cooperation matters.
5. **R\|Protocol and plug-in mode.** It is unclear whether an R\|Protocol client can join R\|Trader Pro plug-in mode; it is documented for desktop platforms. There is no R\|Trader Pro on Linux in any case, so the two-session CME limit is a live risk.
6. **History depth per plant / per entitlement.** Reports range from 1 day to back to 2011, and they may depend on the FCM, the system (prop systems vs Rithmic 01) or the era. The weekly byte quota figure (40 GB) is from 2020.
7. **Rithmic Test data.** Described as "very limited", and reportedly no R\|Protocol on paper demos with live data. Development may need a real prop or broker account.
8. **Apex rules and platform list.** Taken from third-party summaries; Apex's help centre returned 403. Prop rules change often (Apex 4.0 in March 2026, Tradeify consistency changes in Sept 2025).
9. **CME fee figures.** The CME PDF could not be downloaded; figures are as quoted by third parties.
10. **Exact R\|Protocol rate limits** (requests per second, concurrent replays) are undocumented publicly. Rithmic can force a logout (`ForcedLogout` message).
11. **Rithmic's own site** (rithmic.com, a Wix site) did not render for fetching, so Rithmic's own wording was only seen in search snippets.

## Decisions for the author

1. **Personal tool or distributed product?** This drives Rithmic's terms and fees and possibly CME vendor status. Decide before emailing Rithmic.
2. **Wrap `rithmic-rs`** (Apache/MIT, active, has everything listed above, but native-tls only and no reconnect) **or write an own transport** in a `crates/rithmic` crate like `tos-market`. Get Rithmic's answer on proto redistribution either way.
3. **Charting-only first, or trading from day one?** Trading needs a new broker/account boundary (orders, positions, PnL, RMS), a DOM, brackets, flatten and confirmations, plus much more conformance and liability exposure. Market data alone fits the existing `Provider`/`Stream` with a new trade/profile data type.
4. **Data model for profiles:** store raw ticks (exact, heavy, hits the weekly quota), store server volume-profile minute bars (light, footprints at 1 minute or coarser), or both: profile bars for history and live ticks forward.
5. **Which firms to target first.** Apex (open to any Rithmic front end), Tradeify (allow-list), Lucid and others. Skip Topstep. Possibly approach a firm about allow-listing, as Tradesea did.
6. **Which risk rules to show or enforce client-side.** Recommended: display RMS and PnL limits from Rithmic, and offer optional personal lockouts. Do not try to compute consistency or payout rules.
7. **How to handle the CME two-session limit on Linux.** Plug-in mode is not available. Warn users, or share a single session across Omacharts windows and processes (the widget bar included).
