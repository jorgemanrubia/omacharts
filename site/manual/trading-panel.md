---
title: "The Trading panel"
description: "Every account's money, what each holds, and the day's orders, in one panel under the charts."
section: trading
nav_order: 35
---

The Trading panel sits under the charts once there is a login ([Connecting a broker](/manual/brokers/)). It shows every account of every login at once, whichever chart is focused.

{: .warning }
Trading is experimental (**Beta**). Nothing in the panel asks before it acts.

Open it with the **Trading** toggle in the window's bottom-right corner (at the right end of the chartbook strip when there are several chartbooks), or with the key of a tab. The same key again closes it while that tab is in front. Whether it is open is remembered.

| Key | Tab |
|---|---|
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>A</kbd> | Accounts |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>P</kbd> | Positions |
| <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>O</kbd> | Orders |

While there is a Rithmic login, Rithmic's logos are in the panel's tab bar.

## Accounts

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click the User field, type a user, Sign in   # twice: logins Fake and Fake 2
  omacharts chart set --symbol ES --resolution 15m
  click 1244 706          # the Trading toggle, opens on Accounts
  move 1270 300
capture: bin/headless-window shot /tmp/accounts.png
crop: 2560x296+0+1144
-->
{% include figure.html src="/assets/manual/trading-panel/accounts.png" alt="The Trading panel on Accounts, with balance, day P&L and limits per account" caption="Accounts." %}

| Column | Shows |
|---|---|
| **Account** | Which account |
| **Currency**, **Balance** | As the broker reports them |
| **Day P&L**, **Open P&L** | The day's profit or loss, and that of what is held now |
| **Loss limit**, **Liquidates at** | The limits a prop firm sets, when the broker reports them |

## Positions

Each contract held, per account. **Flatten** on a row closes it at the market, and leaves the contract's working orders alone.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click the User field, type a user, Sign in   # twice: logins Fake and Fake 2
  omacharts chart set --symbol ES --resolution 15m
  click 205 590           # Positions
  move 1270 300
capture: bin/headless-window shot /tmp/positions.png
crop: 2560x296+0+1144
-->
{% include figure.html src="/assets/manual/trading-panel/positions.png" alt="The Trading panel on Positions, one ESZ6 contract held on each of two accounts, each with a Flatten button" caption="Positions." %}

| Column | Shows |
|---|---|
| **Account**, **Contract** | Which account holds which contract |
| **Qty** | How many, negative when short |
| **Average** | The average price they were opened at |
| **Open P&L** | The profit or loss of what is held now |

## Orders

The day's orders. **Open** shows the orders still working, and **All** the whole day's, filled and cancelled too. **Cancel** on a row cancels that order. **Cancel all · *account***, at the right of every tab, cancels every working order on the default account, which it names, and is greyed out when none is working.

<!-- regenerate:
theme: tokyo-night (copied into the sandbox HOME's .local/share/omarchy/themes)
window: 2560x1440 @2, bin/headless-window; built with --features fakes, a sandboxed HOME and XDG_*
  (seed a fresh rithmic-systems.json in the data dir, so the Rithmic dialog does not ask Rithmic for its systems)
state:
  omacharts trading login fake ; click the User field, type a user, Sign in   # twice: logins Fake and Fake 2
  omacharts chart set --symbol ES --resolution 15m
  click 338 590           # Orders
  click 1088 590          # All
  move 1270 300
capture: bin/headless-window shot /tmp/orders.png
crop: 2560x296+0+1144
-->
{% include figure.html src="/assets/manual/trading-panel/orders.png" alt="The Trading panel on Orders, showing All: working, filled and cancelled orders" caption="Orders, showing **All**." %}

| Column | Shows |
|---|---|
| **Time**, **Account**, **Contract** | When it was sent, on which account, for which contract |
| **Side**, **Qty**, **Type**, **Price** | Buy or Sell, how many, Market, Limit or Stop, and at what price |
| **Status** | Pending, Working, Part filled, Filled, Cancelled or Rejected. A pending order, sent but not yet answered, is dimmed |
| **Filled at** | The price it filled at |

## From the CLI

`omacharts trading accounts`, `positions` and `orders --all` print the same tables, and `--json` gives them as fields. `trading flatten ESZ6` and `trading cancel --all` do what the buttons do ([The CLI › Trading](/manual/cli/#trading)).

## See also

- [Trading from a chart](/manual/trading-on-a-chart/)
- [Connecting a broker](/manual/brokers/)
