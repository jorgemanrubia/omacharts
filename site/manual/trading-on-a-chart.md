---
title: "Trading from a chart"
description: "A chart's account, quantity and Buy and Sell; limit and stop orders, linked exits, moving and cancelling orders, flattening, and executions on the chart."
section: trading
nav_order: 36
---

{: .warning }
Trading is experimental (**Beta**). An order is sent the moment you click, press or drag: nothing asks first, on any account.

A chart trades what it charts, on its account. ES, whichever feed draws it, trades its front month as Rithmic names it, such as ESZ6, and a chart of a dated contract, such as ESH7, trades that contract, through the account's login ([Trading services](/manual/brokers/)).

## The Account part

With an account to trade on, the chart's top-right corner leads with its trading parts, before the gear.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 1400x900 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click User, type LT50-1043, click Name, ctrl+a, type Lucid, Sign in
  omacharts trading login fake ; click User, type TDY100-2287, click Name, ctrl+a, type Tradeify, Sign in
  # the fakes build numbers each account after its user, and holds a different day for each login
  omacharts chart set --symbol ES --resolution 15m
  move 1130 880           # off the chart, so its gear is hidden
capture: bin/headless-window shot /tmp/account-part.png
crop: 1000x88+1110+4
-->
{% include figure.html src="/assets/manual/trading-on-a-chart/account-part.png" alt="A chart's top-right corner: the account, the day's P&L, Buy, Sell, Qty and the position held" caption="The Account part of a chart holding one contract." %}

| Part | Shows, and does |
|---|---|
| **Account** | The account the chart trades on, and its trouble if any, such as *signed out*. Click it to choose another, listed by login. Shown only when there is more than one account |
| **P&L** | The account's P&L for the day, in the profit or loss colour |
| **Buy**, **Sell** | Buy or sell the chart's quantity at the market. Greyed out with the reason in the tooltip when the chart cannot trade |
| **Qty** | The quantity orders are sent with. Click it for the others on offer and **Edit quantities…** |
| **Current position** | What the account holds in the contract, with its open P&L, such as *+1 (+$37.50)* |

A chart with no account of its own trades on the default one, and follows it when the default changes. Choosing the default account in the picker leaves it that way.

These are header parts, moved or removed in Preferences › Charts › **Header** ([The header](/manual/charts/#the-header)).

### Quantities

**Qty** offers the quantities of a template: **Futures** offers 1 to 9 and **Stocks** 100 to 900. <kbd>Ctrl</kbd> + <kbd>1</kbd> to <kbd>9</kbd> picks the first to the ninth. The quantity is kept with the chart.

**Edit quantities…**, on the Qty menu or in Preferences › Trading, opens **Quantities**. Each template has a **Name**, its **Quantities**, what it **Applies to** (Stocks, Futures) and **Only these symbols**, such as `MES, MNQ`. A chart takes the narrowest template that applies to its symbol.

### Trading another size

A chart can trade the other size of what it charts: MES on an ES chart, so the chart reads the full-size market and the orders go in micros. Set it in the chart's settings › **Trading** › **Trading symbol**; its clear button trades the chart's own symbol again. Only the other size of the chart's market, or one of its contract months, is accepted.

## Placing orders

| How | Sends |
|---|---|
| **Buy** or **Sell** in the header | The chart's quantity at the market |
| <kbd>Ctrl</kbd> + <kbd>B</kbd>, <kbd>Ctrl</kbd> + <kbd>S</kbd> | The same, from the keyboard |
| Right-click the chart at a price | The menu offers a buy and a sell at the market, then the limit and the stop orders that price allows, such as **Sell Limit 2 ESZ6 at 6,840.00** above the market and **Sell Stop 2 ESZ6 at 6,800.00** below it |

A market order on its way shows beside the last price, such as *Buying 2…*, and a limit or a stop faint at its own price. What happens next is a short notice over the window and a line in the [Log](/manual/window/#the-log): *Working · Sell Limit 2 ESZ6 at 6,840.00*, *Filled 1 ESZ6 at 6,824.25*, or a rejection, which is an error.

The menu leaves out an order that could not go at that price, such as a buy limit above the market or past the day's price limit. On a chart that cannot trade it has one greyed row that says why. One request per account is on its way at a time: another is refused with *Not sent · the last order is still on its way*.

## Exits

The label of a position has two tabs, **L** and **STP**. Drag **L** to a price for a limit that takes the profit, and **STP** for a stop that cuts the loss. Each is for the whole position.

Place the second while the first is working and the two are linked: whichever fills first cancels the other, and the notice says *Linked · whichever fills first cancels the other*. A solid line joins two exits the trading service holds linked, as Rithmic does. A dashed one means Omacharts holds the link, only while it runs, and says so.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 1400x900 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click User, type LT50-1043, click Name, ctrl+a, type Lucid, Sign in
  omacharts trading login fake ; click User, type TDY100-2287, click Name, ctrl+a, type Tradeify, Sign in
  # the fakes build numbers each account after its user, and holds a different day for each login
  omacharts chart set --symbol ES --resolution 15m
  omacharts trading cancel --all
  omacharts trading exit --target 7640 --stop 7540
  omacharts trading buy --limit 7550
  move 1130 880
  (wait for, or close, the notices of the day's orders)
capture: bin/headless-window shot /tmp/orders-on-chart.png
crop: 2236x1800+0+0
-->
{% include figure.html src="/assets/manual/trading-on-a-chart/orders.png" alt="An ES chart with a long position, a linked target and stop, and a buy limit below the market" caption="A position with a target and a stop linked by the trading service (the solid line), and a working buy limit." %}

## Order templates

Order templates are **Beta**. A template is a set of exits that go in with an order: any number of targets and of stops, or none of either. With one picked, every order the chart sends goes as a bracket, a market order too, and the exits are placed by the trading service as the entry fills. An order that closes some or all of the position goes without them, and the Log says so. A trading service that takes no brackets, such as Sierra Chart, refuses an order with a template.

**Edit order templates…**, on the header's template menu or in Preferences › Trading › **Order entry**, opens **Order templates**. **Add a template** makes one; click one to change its **Name** and its exits, drag the handle to put the templates in order, and the bin removes one. Each exit says how far it goes, in one of three ways:

| Mode | How far |
|---|---|
| **Ticks** | That many ticks from the fill |
| **Money** | Where the exit's own contracts make, or lose, that much |
| **Risk** | On a stop, where the whole order loses that much, which starts at **Value of 1R**. On a target, a multiple of R on the whole order, such as 2 for twice what 1R is worth |

Every distance rounds toward the entry, and is never under one tick.

With more than one exit in a group, each but the last takes a share of the order, such as 1/2, or a number of contracts; the last takes the rest. A share rounds down, and an exit left with nothing is not sent. A 3-lot on two targets of 1/2 sends 1 and 2. The editor says how its exits split an order of 1, 2 and 3, and which would not be sent. A template with no exits, or none left on the order's quantity, sends the order plain, and the Log says so.

### On a chart

The template part of the header, at its right, shows the chart's template, or only its icon with none. Its menu lists **No template** and the templates in order, and <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>1</kbd> to <kbd>9</kbd> picks the first to the ninth; <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>0</kbd> is no template. The template is kept with the chart. While there is a trading login these keys are the templates', not the watchlists'.

### On a working order

A limit or stop entry that has not begun to fill can be given a template three ways: the **Order template** submenu in the right-click menu on its line, the **Template** in the New order dialog, or by clicking the order to pick it out (it is drawn raised) and pressing <kbd>Ctrl</kbd> + <kbd>Shift</kbd> and the template's digit. Its exits are drawn faint where they will rest, and the template's name beside it. **No template**, or <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>0</kbd>, takes them off.

An entry with no target or no stop wears the missing tabs, as a position does: drag **L** or **STP** off it to a price, and that exit goes in with it for the whole order. Dragging an exit already drawn moves it.

Giving an entry exits, or changing them, cancels it and sends it again as one bracket, so it loses its place in the queue. If it fills before the cancel lands, nothing is sent again, and the Log says so. Omacharts remembers which template an entry was sent with only while it runs.

## The New order dialog

<kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>E</kbd> opens **New order**, for the focused chart's account and contract: **Side**, **Type** (Market, Limit or Stop, those the trading service takes), **Quantity**, which starts at the chart's, **Price**, which starts at the last trade, and **Template**, which starts at the chart's. Its button says the order, such as **Buy 2 ESZ6 at 6,810.00**, and sends it through the chart's own checks.

## Moving, cancelling and flattening

| To | Do |
|---|---|
| Move an order | Drag its line. While dragging, the label says where it goes and how far, such as *6,840.00 · +11 ticks*, an exit says the P&L it would close at, and either says why where it could not go, such as *Stop above market · not allowed* |
| Cancel an order | The **×** on its line, or its **Cancel** item, such as **Cancel Sell Limit 2 ESZ6 at 6,840.00**, in the right-click menu on the line |
| Flatten | The **×** on the position closes it at the market and cancels its exits. <kbd>Ctrl</kbd> + <kbd>F</kbd> closes it and cancels every working order in the contract |

Whether order labels sit at the chart's left or right is Preferences › Trading › **Order labels**. **Show P&L as** there writes the P&L on the chart in money or in R ([Orders and P&L on charts](/manual/brokers/#orders-and-pl-on-charts)).

## Executions on the chart

Turn on the chart's settings › **Trading** › **Show fills** to mark the account's executions on the chart, up to 30 days back. A triangle marks each fill, pointing right for one that opened or added and left for one that closed, in the theme's up colour for a buy and down colour for a sell. A dotted line joins an entry to its exit, coloured by whether it won or lost. The switch is kept with the chart.

## Keys

| Key | Does |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>B</kbd> | Buy the chart's quantity at the market |
| <kbd>Ctrl</kbd> + <kbd>S</kbd> | Sell the chart's quantity at the market |
| <kbd>Ctrl</kbd> + <kbd>F</kbd> | Flatten the chart's contract and cancel its working orders |
| <kbd>Ctrl</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Pick the quantity, first to ninth |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>A</kbd>, <kbd>P</kbd>, <kbd>O</kbd> | Show or hide the accounts, positions or orders ([The Trading panel](/manual/trading-panel/)) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>E</kbd> | New order ([The New order dialog](#the-new-order-dialog)) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | Pick the order template, first to ninth; with a working order picked out, give it that template ([Order templates](#order-templates)) |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>0</kbd> | No order template, on the chart or the picked order |

The first four act on the focused chart, and never while you are typing in a box. Until there is a trading login, none of these keys does anything.

## From the CLI

`omacharts trading buy`, `sell`, `exit`, `modify`, `cancel` and `flatten` act as the chart does, through the same checks. `chart set --account`, `--quantity`, `--trading-symbol`, `--order-template` and `--show-fills` set a chart's trading, `trading quantities` the quantities, `trading templates` the order templates, and `trading bracket` gives a working order one ([The CLI › Trading](/manual/cli/#trading)).

## See also

- [The Trading panel](/manual/trading-panel/)
- [Trading services](/manual/brokers/)
- [Hotkeys](/manual/hotkeys/#trading)
