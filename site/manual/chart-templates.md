---
title: "Chart templates"
description: "Named sets of indicators that charts share."
section: charting
nav_order: 16
---

A chart template is a saved set of indicators with a name. Put several charts on the same template and they all show those indicators. Change the template and they all change, in every chartbook. Out of the box there is one template, **Default**, which holds [Volume](/manual/indicators/#volume), and new charts open on it.

<!-- regenerate:
theme: tokyo-night
window: 2560x1040 @2, scratchpad kit (bin/headless-window, sandboxed HOME)
state:
  omacharts template create Momentum
  omacharts template indicator add Momentum rsi
  omacharts template indicator add Momentum macd
  omacharts chart set --template Momentum
  key ctrl+comma
  click 214 127        # Charts
  click 695 42         # Templates
  move 1270 510
capture: bin/headless-window shot /tmp/tpl.png
crop: 276,16,2008,1008   (the whole dialog, 12px layout margin)
-->
{% include figure.html src="/assets/manual/chart-templates/preferences-templates.png" alt="Preferences open on the Charts Templates tab, listing Default and Momentum" caption="Preferences › Charts › Templates, with two templates." %}

## Creating a template

1. Open Preferences (<kbd>Ctrl</kbd> + <kbd>,</kbd>) › Charts › **Templates**.
2. Click **+**, type a name and click **Create**.
3. In the template's editor, add indicators with **+**, as on a chart's [Indicators page](/manual/indicators/#the-indicators-page).
4. Put a chart on it from the chart menu › **Template** ([Choosing a chart's template](#choosing-a-charts-template)).

## How a chart on a template behaves

| Change | What happens |
|---|---|
| The template's indicators change | Every chart on it, in every chartbook, changes at once |
| You add, remove, hide, reorder or reconfigure an indicator on the chart | The chart comes off the template, keeping the indicators it showed, then makes the change |
| You drag a strip's edge on the chart | The template takes the new height, and so does every chart on it |
| You put the chart on another template | It shows that template's indicators in place of its own |
| You put the chart on **None** | It shows no indicators |
| The template is renamed | The chart stays on it |
| The template is deleted | The chart keeps the template's indicators as its own and stops following it |
| You split the chart | The new chart is on the same template ([Splitting and arranging charts](/manual/layouts/)) |

If a chart has indicators of its own, choosing a template asks before replacing them, and choosing **None** asks before removing them.

With the **Chart template** part of the header on, the header names the template the chart is on ([The header](/manual/charts/#the-header)).

## Choosing a chart's template

| Where | How |
|---|---|
| The chart menu › **Template** | **None**, then every template in the order they were made. The one the chart is on is marked |
| Chart settings › **Indicators** › **Chart template** | The same choice, above the list of what the chart shows |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>1</kbd> … <kbd>9</kbd> | The first to the ninth template, in that order, on the number row, whatever your layout puts on those keys with Shift. A number with no template does nothing |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>0</kbd> | **None**. On a chart with indicators of its own, it removes them, asking first |
| <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>PgUp</kbd> / <kbd>PgDn</kbd> | The previous or next of **None** and the templates, wrapping round |

<!-- regenerate:
theme: tokyo-night
window: 2560x1600 @2, scratchpad kit (fresh session)
state:
  move 1270 790
  key ctrl+b
  omacharts template create Momentum
  omacharts template indicator add Momentum rsi
  omacharts template indicator add Momentum macd
  omacharts chart set --template Momentum
  omacharts config set chart_header symbol,resolution,change,template
  run bin/virtual-pointer --button right click 600 150   # high enough that the menu has room below
  click 675 237        # Template
  move 732 440         # inside the submenu, on no row
capture: bin/headless-window shot /tmp/menu.png
crop: 0,0,1778,1090   (the submenu with 24px layout margin right and below)
-->
{% include figure.html src="/assets/manual/chart-templates/template-menu.png" alt="The chart menu's Template submenu open over a SPY daily chart, each template with its key" caption="The chart menu's **Template** submenu. The header's last part names the template." %}

## Editing a template

Preferences › Charts › **Templates** lists every template with its name, what it holds and how many charts use it. From a chart's own settings, **Edit Chart Template…** under **Chart template** opens the template the chart is on.

| Control | Does |
|---|---|
| **+** (**New chart template**) | Asks for a name (**New Chart Template**, **Create**) and opens the new, empty template's editor |
| Click a row, or its chevron | Opens the template's editor |
| Pencil (**Rename**) | Asks for a new name. Charts on it stay on it |
| Bin (**Delete**) | Asks first (**Delete Chart Template?**), saying how many charts use it. They keep its indicators as their own |

The editor is the [Indicators page](/manual/indicators/#the-indicators-page) of the template, headed **Indicators in the template**, with the same list, **+** and settings. Every chart on the template changes as you edit.

<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, scratchpad kit
state:
  key ctrl+b
  omacharts template create Momentum
  omacharts template indicator add Momentum rsi
  omacharts template indicator add Momentum macd
  omacharts chart set --template Momentum
  key ctrl+i
  move 1270 710
capture: bin/headless-window shot /tmp/locked.png
crop: 688,68,1184,1304   (the whole dialog, 16px layout margin)
-->
{% include figure.html src="/assets/manual/chart-templates/chart-on-a-template.png" alt="The Indicators page of a chart on the Momentum template" caption="The Indicators page of a chart on **Momentum**, its indicators listed without controls." %}

## The template new charts open with

Preferences › Charts › Defaults › Indicators › **Template** is the template a new chart opens on, or **None**. A new chartbook's first chart opens on it. A chart made by splitting copies the chart it came from instead.

**Apply to all charts now** under the defaults, and **Reset to defaults** in a chart's settings, put charts on this template the way choosing it does, asking first where a chart would lose its own indicators ([Chart settings](/manual/chart-settings/#defaults-for-new-charts)). If this template is deleted, new charts open on **None**. **Default** can be deleted like any other template, and is not recreated.

## See also

- [Chart templates from the CLI](/manual/cli/#chart-templates)
