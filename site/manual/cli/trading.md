---
title: "Trading"
description: "Trading logins, accounts, positions and orders. Experimental."
section: agents
parent: cli
nav_order: 18
---

Trading is experimental ([Trading](/manual/trading/)). The `trading` group
does what the Trading panel and a chart's Account part do. The settings a
chart trades with are [`chart set`](/manual/cli/charts/#chart-set) flags: `--account`,
`--quantity`, `--trading-symbol`, `--order-template` and `--show-fills`.

| Command | Does |
|---|---|
| `trading logins` | Each login, its service, and whether it is live and its data feed is among the data feeds |
| `trading login SERVICE` | Opens the service's sign-in dialog in the window, to add a login |
| `trading sign-in LOGIN` | Signs a login in with its saved password, or opens its dialog when it has none |
| `trading sign-out LOGIN` | Signs it out and forgets its password; the login stays |
| `trading remove LOGIN` | Removes the login, its password, its accounts and its data feed |
| `trading data LOGIN` | Puts this login's data feed at the top of the data feeds |
| `trading sync` | Asks every signed-in login for its accounts again, and what they hold, as the panel's sync does: an account opened since sign-in is listed once its login answers. Nothing signs out |
| `trading accounts` | Every account with its balance, the day's and the open P&L, its loss limit and liquidation level; `*` marks the default |
| `trading default ACCOUNT` | The account charts trade on until given one of their own |
| `trading positions` | What every account holds, at what average, with its open P&L |
| `trading orders [--all]` | The day's working orders; with `--all`, the filled and cancelled ones too |
| `trading buy`, `trading sell` | An order for what the chart trades, on its account: at the market, or with `--limit PRICE` or `--stop PRICE`. `--quantity N`, any whole number, instead of the chart's Qty. `--template T` brackets it with an order template, or `none`, instead of the chart's |
| `trading exit --target PRICE --stop PRICE` | A target, a stop, or both linked, for the whole of the position the chart trades |
| `trading modify ORDER --price PRICE` | Moves a working order |
| `trading cancel ORDER`, `trading cancel --all [--account ACCOUNT]` | Cancels one order, or every open order on an account (the default one when not given) |
| `trading flatten`, `trading flatten CONTRACT [--account ACCOUNT]` | Closes the chart's position at the market and cancels its orders, as Ctrl+F does; or, with a contract, closes what an account holds in it and leaves its orders, as the Positions tab does |
| `trading quantities [list]`, `set NAME QUANTITIES`, `remove NAME` | The quantities a chart's Qty offers; `set` takes `--applies-to stocks` or `futures` and `--symbols` |
| `trading templates [list]`, `set NAME`, `remove NAME`, `move NAME POSITION` | The order templates (Beta). `set` takes `--target EXIT` and `--stop EXIT`, once per exit or `none`, and `--rename NAME`; `move` puts one at a position, which is its <kbd>Ctrl</kbd> + <kbd>Shift</kbd> digit |
| `trading bracket ORDER --template T` | Gives a working entry an order template's exits, or `none` takes them off: it is cancelled and placed again as one bracket (Beta) |

An exit is how far, then optionally how much after an `@`. How far is `8t`
(ticks), `$250` (what the whole order makes or loses there), or a
multiple of R on the whole order, such as `1r`, `0.5r` or `2r`, for a
target or a stop alike: R is Preferences › Trading › **Value of 1R**, 500
until it is changed, so a stop at `1r` risks it and one at `0.5r` half of
it, and `$500` rests where `1r` does. How much is a number of contracts
such as `@2`, a share of the position such as `@1/2`, `@33%` or `@0.33`, or
`@rest` (`@all`); an exit that says none closes 1 contract, the last of
its group the rest. Sizes need not add up: `--target 1.5r@1/2` alone
leaves the other half without a target.

```
$ omacharts trading templates set Half --target 1.5r@1/2 --stop 1r
Half: +1.5R 1/2 | −1R
  [exit 0]

$ omacharts trading templates set Runner --target 1r --target 2r --target 3r --stop 1r
Runner: +1R ×1 · +2R ×1 · +3R rest | −1R
  [exit 0]

$ omacharts chart set --order-template Runner
```

`buy`, `sell`, `exit` and `flatten` act on the chart `--book` and `--chart`
name, the focused one by default ([Which chart a command acts
on](/manual/cli/#which-chart-a-command-acts-on)), in the open chartbook: an order goes
from a chart on screen. An account is named by its name or its number, or,
when two logins share a number, by its login's name and its number, such as
`Demo/1234`, or by its `account` in `trading accounts --json`.

```
$ omacharts trading logins
Demo                 Rithmic                live · data
  [exit 0]

$ omacharts trading positions
Demo               ESZ6        +1 at     6,824.25  +$37.50
  [exit 0]

$ omacharts trading buy --limit 6810 --quantity 2
Demo: Buy Limit 2 ESZ6 at 6,810.00
  [exit 0]

$ omacharts trading exit --target 6840 --stop 6815
Demo: Sell Stop 1 ESZ6 at 6,815.00
Demo: Sell Limit 1 ESZ6 at 6,840.00
  [exit 0]

$ omacharts trading orders
B4           14:31:09 Demo           ESZ6     Sell    1 Limit      6,840.00  Working
B3           14:31:09 Demo           ESZ6     Sell    1 Stop       6,815.00  Working
B2           14:31:07 Demo           ESZ6     Buy     2 Limit      6,810.00  Working
  [exit 0]

$ omacharts trading modify B2 --price 6805
Demo: Move Buy Limit 2 ESZ6 to 6,805.00
  [exit 0]

$ omacharts trading flatten
Demo: Flatten ESZ6 and cancel its orders
  [exit 0]

$ omacharts trading buy --limit 6900
omacharts: Demo: Not placed · 6,900.00 is above the market now
  [exit 5]
```

Orders, cancels and flattens need the window, where the sessions are signed
in, and so do `positions`, `orders`, `login` and `sign-in`; with none open
they exit `6`. Each goes through the chart's own checks,
and what the chart would refuse is refused with exit `5` in its words. There
are no confirmations, as there are none on the chart. The reply says what was
sent; the trading service's answer reaches the Log and `trading orders`, so a script
waiting for a fill reads `trading orders --json` until its `status` says so.
One order per account is on its way at a time, as on the chart, and the next
is refused until the last is answered.

An order is named by its id from `trading orders`. Should two accounts have an order of the same id, `modify` and `cancel` refuse it with exit `4` until `--account` says which.

`trading flatten` with the chart's position already flat says `Already flat
in ESZ6` and exits `0`. `trading modify` moves the price only, and needs a chart open
that trades the order's account and contract, since the order is moved from
there.

A password is never typed in a terminal. `trading login` and a `trading
sign-in` without a saved password open the dialog in the window and return;
`trading logins` says when the login is live.

With `--json`, `logins` gives `logins` and the `services` on offer,
`accounts` gives `accounts` with `balance`, `day_pnl`, `open_pnl`,
`loss_limit`, `liquidates_at`, `max_order_qty` and `default`, `positions`
gives `positions`, and `orders` gives `orders` with `id`, `side`, `qty`,
`filled`, `type`, `price`, `status`, `working` and `filled_at`. An order,
cancel or flatten gives `said`, one `{account, text}` per line the window
said.
