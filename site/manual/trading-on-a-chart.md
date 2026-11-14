---
title: "Trading from a chart"
description: "A chart's account, quantity and Buy and Sell; limit and stop orders, linked exits, moving and cancelling orders, flattening, and executions on the chart."
section: trading
nav_order: 36
---

{: .warning }
Trading is experimental (**Beta**). An order is sent the moment you click, press or drag: nothing asks first, on any account.

A chart trades what it charts, on its account. ES charted from any feed trades the front ES contract, such as ESZ6, through the account's login ([Connecting a broker](/manual/brokers/)).

## The Account part

With an account to trade on, the chart's top-right corner leads with its trading parts, before the gear.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click the User field, type a user, Sign in   # twice: logins Fake and Fake 2
  omacharts chart set --symbol ES --resolution 15m
  move 1270 710
capture: bin/headless-window shot /tmp/account-part.png
crop: 1020x88+850+4
-->
{% include figure.html src="/assets/manual/trading-on-a-chart/account-part.png" alt="A chart's top-right corner: the account, the day's P&L, Buy, Sell, Qty and the position held" caption="The Account part of a chart holding one contract." %}

| Part | Shows, and does |
|---|---|
| **Account** | The account the chart trades on, and its trouble if any, such as *signed out*. Click it to choose another, listed by login. Shown only when there is more than one account |
| **P&L** | The account's P&L for the day, in the profit or loss colour |
| **Buy**, **Sell** | Buy or sell the chart's quantity at the market. Greyed out with the reason in the tooltip when the chart cannot trade |
| **Qty** | The quantity orders are sent with. Click it for the others on offer and **Edit quantities…** |
| **Current position** | What the account holds in the contract, with its open P&L, such as *+1 (+$37.50)* |

A chart with no account of its own trades on the default one, and follows it when the default changes. Choosing the default account in the picker leaves it that way. These parts are header parts, moved or removed in Preferences › Charts › **Header** ([The header](/manual/charts/#the-header)).

### Quantities

**Qty** offers the quantities of a template: **Futures** offers 1 to 9 and **Stocks** 100 to 900. <kbd>Ctrl</kbd> + <kbd>1</kbd> to <kbd>9</kbd> picks the first to the ninth. The quantity is kept with the chart.

**Edit quantities…**, on the Qty menu or in Preferences › Trading, opens **Quantities**. Each template has a **Name**, its **Quantities**, what it **Applies to** (Stocks, Futures) and **Only these symbols**, such as `MES, MNQ`. A chart takes the narrowest template that applies to its symbol.

### Trading another size

A chart can trade the other size of what it charts: MES on an ES chart, so the chart reads the full-size market and the orders go in micros. Set it in the chart's settings › **Trading** › **Trading symbol**; its clear button trades the chart's own symbol again. Only a symbol that moves with the chart's is accepted.

## Placing orders

| How | Sends |
|---|---|
| **Buy** or **Sell** in the header, <kbd>Ctrl</kbd> + <kbd>B</kbd>, <kbd>Ctrl</kbd> + <kbd>S</kbd> | The chart's quantity at the market |
| Right-click the chart at a price | The menu offers a buy and a sell at the market, then the limit and the stop orders that price allows, such as **Sell Limit 2 ESZ6 at 6,840.00** above the market and **Sell Stop 2 ESZ6 at 6,800.00** below it |

An order on its way shows beside the last price, such as *Buying 2…*. What happens next is a short notice on the chart and a line in the [Log](/manual/window/#the-log): *Working*, *Filled 1 ESZ6 at 6,824.25*, or a rejection, which is an error and stays in the corner.

The menu leaves out an order that could not go at that price, such as a buy limit above the market or past the day's price limit. One request per account is on its way at a time: another is refused with *Not sent · the last order is still on its way*.

## Exits

The label of a position has two tabs, **L** and **STP**. Drag **L** to a price for a limit that takes the profit, and **STP** for a stop that cuts the loss. Each is for the whole position.

Place the second while the first is working and the two are linked: whichever fills first cancels the other, and the notice says *Linked · whichever fills first cancels the other*. A solid line joins two exits Rithmic holds linked. A dashed one means Omacharts holds the link while it runs, and says so.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click the User field, type a user, Sign in   # twice: logins Fake and Fake 2
  omacharts chart set --symbol ES --resolution 15m
  omacharts trading cancel --all
  omacharts trading exit --target 7640 --stop 7540
  omacharts trading buy --limit 7550
  move 1270 710
capture: bin/headless-window shot /tmp/orders-on-chart.png
crop: 1990x1440+0+0
-->
{% include figure.html src="/assets/manual/trading-on-a-chart/orders.png" alt="An ES chart with a long position, a linked target and stop, and a buy limit below the market" caption="A position with a linked target and stop, and a working buy limit." %}

## Moving, cancelling and flattening

| To | Do |
|---|---|
| Move an order | Drag its line. While dragging, the label says where it goes and how far, such as *6,840.00 · +11 ticks*, or *not allowed* where it could not go |
| Cancel an order | The **×** on its line, or **Cancel** on its line in the right-click menu |
| Flatten | The **×** on the position, or <kbd>Ctrl</kbd> + <kbd>F</kbd>, which also cancels the contract's working orders |

Whether order labels sit at the chart's left or right is Preferences › Trading › **Order labels**. **Show P&L as** there writes the P&L on the chart in money or in R ([Connecting a broker](/manual/brokers/#the-rest-of-the-page)).

## Executions on the chart

Turn on the chart's settings › **Trading** › **Show fills** to mark the account's executions on the chart, up to 30 days back. A triangle marks each fill, pointing right for one that opened or added and left for one that closed, in the theme's up colour for a buy and down colour for a sell. A dotted line joins an entry to its exit, coloured by whether it won or lost. The switch is kept with the chart.

## Keys

| Key | Does |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>B</kbd> | Buy the chart's quantity at the market |
| <kbd>Ctrl</kbd> + <kbd>S</kbd> | Sell the chart's quantity at the market |
| <kbd>Ctrl</kbd> + <kbd>F</kbd> | Flatten the chart's contract and cancel its orders |
| <kbd>Ctrl</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Pick the quantity, first to ninth |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>A</kbd>, <kbd>P</kbd>, <kbd>O</kbd> | Show or hide the accounts, positions or orders ([The Trading panel](/manual/trading-panel/)) |

They work only on a chart with an account, and never while you are typing in a box.

## From the CLI

`omacharts trading buy`, `sell`, `exit`, `modify`, `cancel` and `flatten` act on a chart as these do, through the same checks. `chart set --account`, `--quantity`, `--trading-symbol` and `--show-fills` set a chart's trading, and `trading quantities` the templates ([The CLI › Trading](/manual/cli/#trading)).

## See also

- [The Trading panel](/manual/trading-panel/)
- [Connecting a broker](/manual/brokers/)
- [Hotkeys](/manual/hotkeys/#trading)
