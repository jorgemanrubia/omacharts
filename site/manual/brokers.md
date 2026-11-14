---
title: "Connecting a broker"
description: "Signing in to Rithmic for a futures account or a prop firm, the accounts that come with it, and the default account."
section: trading
nav_order: 34
---

{: .warning }
Trading is experimental, and marked **Beta** in Preferences and here. It places real orders on real accounts, and nothing asks before an order is sent. Try it on a demo or evaluation account first.

Omacharts trades futures through Rithmic, which most futures prop firms, such as LucidTrading and Tradeify, use for their accounts. A login gives you its accounts, their positions and orders in the [Trading panel](/manual/trading-panel/), and buying and selling from a chart ([Trading from a chart](/manual/trading-on-a-chart/)).

## Adding a login

In Preferences › **Trading**, press **Add a trading service…** and choose **Rithmic**. The same list is under **Add a data feed…** on the Data page.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  move 1270 710
  key ctrl+comma
  click 214 283          # Trading
  click 760 236          # Add a trading service…
  click 760 306          # Rithmic
capture: bin/headless-window shot /tmp/rithmic.png
crop: 792x994 around the dialog
-->
{% include figure.html src="/assets/manual/brokers/rithmic-sign-in.png" alt="The Sign in to Rithmic dialog, with User ID, Password, System and Gateway" caption="Signing in to Rithmic." %}

| Field | What goes in it |
|---|---|
| **User ID** | The user your broker or prop firm gave you. **Sign in** stays greyed out until it is filled in |
| **Password** | Kept in the desktop keyring and sent only to Rithmic. Left empty when signing in again, the kept one is used |
| **System** | **Rithmic Test**, **Rithmic 01**, **Rithmic Paper Trading**, then the prop firms Rithmic lists, alphabetically. Type to search. Until Omacharts has asked Rithmic once, only Rithmic 01 and Paper Trading are offered. The list is asked for again once a week old |
| **Gateway** | The server nearest you: Chicago, New York, Colo75, Frankfurt, Ireland, Sydney, São Paulo, Hong Kong, Mumbai, Seoul, Tokyo, Singapore or Cape Town. Rithmic Test has only Chicago |

The login is named after its system, such as **Tradeify**, and **Tradeify 2** for a second one. It signs in as soon as you press **Sign in**, and again each time Omacharts starts while its password is kept. The [Log](/manual/window/#the-log) says *Signed in · 3 accounts*, or why not.

A Rithmic login also carries market data. The first one becomes a data feed, at the top of Preferences › Data, so charts of what it trades come from it ([Market data feeds](/manual/market-data/)).

## Logins and accounts

Each login is a row in Preferences › Trading › **Trading services**, with its system and how it stands: **signing in**, **live**, **not signed in**, or Rithmic's own words when it refused. **data** is added on the login charts get their prices from.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click the User field, type a user, Sign in   # twice: logins Fake and Fake 2
  omacharts chart set --symbol ES --resolution 15m
  key ctrl+comma
  click 214 283          # Trading
  move 1270 710
capture: bin/headless-window shot /tmp/trading-prefs.png
crop: 2008x1328+276+56
-->
{% include figure.html src="/assets/manual/brokers/preferences-trading.png" alt="Preferences on Trading, with the Beta tag in the sidebar, a login and its accounts" caption="Preferences › Trading, with a login and its accounts." %}

| Button | Does |
|---|---|
| **Sign out** | Ends the session and forgets the password. The login stays, as not signed in |
| **Sign in** | Signs in again with the kept password |
| **Sign in…** | Opens the dialog again, filled in but for the password |
| **Use for data** | Charts take their prices from this login instead. Offered when there are several |
| **Remove** | Removes the login, its password and its accounts, after asking. When it was the data feed, it asks whether the feed goes too |

Under each login are its accounts, each with its currency and balance. One account is the **Default**: a chart trades on it until you give the chart one of its own. **Make default** on another moves it.

## The rest of the page

| Row | Does |
|---|---|
| Order entry › **Edit quantities…** | The quantities a chart's **Qty** offers ([Quantities](/manual/trading-on-a-chart/#quantities)) |
| Chart › **Order labels** | Whether the labels of orders and positions sit at the **Left** or the **Right** of the chart |
| Chart › **Show P&L as** | **Money**, or **Risk (R)**: multiples of what one unit of risk is worth. Only what the charts show follows it; the Trading panel always shows money |
| Chart › **Value of 1R** | What 1R is worth, in the account's currency; 500 until changed. Shown with Risk (R) |

The colours of orders, positions and P&L are in Preferences › Theme › **Trading**.

## From the CLI

`omacharts trading logins`, `trading login rithmic`, `trading sign-in`, `trading sign-out`, `trading remove`, `trading data`, `trading accounts` and `trading default` do the same from a terminal. A password is only ever typed in the window's dialog ([The CLI › Trading](/manual/cli/#trading)).

## See also

- [The Trading panel](/manual/trading-panel/)
- [Trading from a chart](/manual/trading-on-a-chart/)
- [Market data feeds](/manual/market-data/)

Trading Platform by Rithmic™ is a trademark of Rithmic, LLC. Powered by OMNE™ is a trademark of Omnesys, LLC and Omnesys Technologies, Inc.
