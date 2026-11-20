---
title: "Fib retracement"
description: "Levels across a swing at the Fibonacci ratios"
section: agents
parent: cli/drawings
nav_order: 11
---

`fib`, or `fibonacci`, `retracement`, is a Fib retracement of the swing from `--from` to `--to`, with 0 at `--to`. It takes no `--text`. Its levels are set with the flags below, beside the ones [every drawing takes](/manual/cli/drawings/#flags). What it draws, and its properties in the window, are in [Fib retracement](/manual/drawings/fib-retracement/).

| Flag | Takes | Does |
|---|---|---|
| `--levels LIST` | Any of `0,0.236,0.382,0.5,0.618,0.786,1`; `all`; `none` | Which of a Fib retracement's levels are drawn; all by default. A percentage, `61.8` or `61.8%`, is read as its ratio |
| `--level RATIO` | One of the seven | The level the `--level-*` flags change. Without it they change all seven |
| `--level-color C` | A colour; `drawing` | The level's colour; `drawing` follows the drawing's |
| `--level-width F` | 0.5 to 12 | The level's thickness |
| `--level-style S` | `solid`, `dashed`, `dotted` | How the level is drawn |
| `--fib-labels B` | `on`, `off` | Whether a Fib retracement's levels are labelled with their ratio, as a percentage (61.8%); on by default |
| `--price-labels B` | `on`, `off` | Whether a Fib retracement's levels are labelled with their price; off by default |

```sh
omacharts chart drawing add fib --from 2026-09-08,178 --to 2026-09-19,192
```
