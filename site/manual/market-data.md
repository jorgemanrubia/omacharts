---
title: "Market data feeds"
description: "Where prices come from, choosing a feed in Preferences, and signing in to one that needs an account."
section: configuration
nav_order: 26
---

A feed is where charts, the sidebar and the Omarchy plugin get their prices. One feed serves the whole app. Choose it in Preferences › Data › **Data feed**, and every open chart switches to it.

## Yahoo Finance

Yahoo Finance is the default and needs no account. It covers every listing the [symbol search](/manual/symbols/) finds. Indexes are delayed 15 minutes and futures 10.

Prices update about once a bar, and not while the market is shut or while you have panned back into history.

A chart that says *Rate limited by the data provider. Prices fill in shortly.* carries on by itself after a while.

## thinkorswim

{: .note }
Experimental. A change on Schwab's side can stop the feed working until Omacharts catches up.

thinkorswim shows live prices from your Schwab account. It needs Chromium, Chrome, Brave or Edge on this machine.

It carries US stocks and ETFs, some indexes, and futures (`ES` is charted as `/ES`), but not currencies, crypto or listings abroad. A chart of one of those says *This data provider cannot chart this symbol. Another one may be able to.* Intraday resolutions reach about twenty sessions back, daily bars as far as the account has.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, $SP/kit/hw
state:
  omacharts config set provider tos      # not signed in
  omacharts chart set --symbol EURUSD --resolution 1D
capture: bin/headless-window shot /tmp/unserved.png
crop: 450,721,1100,140
-->
![A chart's message: This data provider cannot chart this symbol. Another one may be able to.](/assets/manual/market-data/cannot-chart.png)

### Signing in

In Preferences › Data, tick **thinkorswim** and press **Sign in…** in the group below. A browser window opens at thinkorswim, where you type your password and one-time code. Omacharts reads prices only and cannot place an order.

The row reports what the browser is doing, then becomes **Sign out**. Without one of those browsers the button is greyed out and says what to install.

<!-- regenerate:
theme: tokyo-night
window: 2560x1040 @2, $SP/kit/hw (no thinkorswim session)
state:
  key ctrl+comma
  click 214 203          # Data
  click 510 228          # the thinkorswim row
  move 1270 510
capture: bin/headless-window shot /tmp/tos.png
crop: 276,16,2008,1008   (the whole dialog, 12px layout margin)
-->
{% include figure.html src="/assets/manual/market-data/data-feed-thinkorswim.png" alt="Preferences on Data with thinkorswim chosen and a Sign in… button below" caption="Preferences › Data with thinkorswim chosen and nobody signed in." %}

Until someone signs in, a chart on thinkorswim says *This data provider is not signed in.* and points to Preferences.

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, $SP/kit/hw (no thinkorswim session)
state:
  omacharts chart set --symbol NVDA --resolution 1W
  omacharts config set provider tos
  move 1270 790
capture: bin/headless-window shot /tmp/not-signed-in.png
crop: 440,721,1180,140
-->
![A chart's message: This data provider is not signed in. Open Preferences → Market data → Provider.](/assets/manual/market-data/not-signed-in.png)

### Expired sessions

Schwab ends a session after a while. Charts stop updating and say so, and Preferences offers **Sign in again**.

The session stays on this machine and is not in backups ([Where your data lives](/manual/your-data/#where-everything-is)).

## See also

- [Feeds and signing in from the CLI](/manual/cli/#market-data)
