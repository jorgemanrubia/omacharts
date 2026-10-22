---
title: "Installation"
description: "Installing Omacharts from Omarchy's repository, from the latest release or from source, what it puts on the machine, and removing it."
section: introduction
nav_order: 12
---

## From the Omarchy menu

Open the Omarchy menu with <kbd>Super</kbd> + <kbd>Space</kbd>, choose **Install › Package** and pick **omacharts**. It updates with the rest of the system.

## From a terminal

```sh
omarchy pkg add omacharts
```

## The latest release

Omarchy's repository can be a release or two behind. To install the newest release from GitHub:

```sh
curl -fLo /tmp/omacharts.pkg.tar.zst \
  https://github.com/jorgemanrubia/omacharts/releases/latest/download/omacharts-$(uname -m).pkg.tar.zst \
  && sudo pacman -U /tmp/omacharts.pkg.tar.zst
```

A package installed this way is not updated with the rest of the system until Omarchy's repository catches up. Run the same line again for the next release.

## From source

Building from a copy of the source installs Rust if it is missing:

```sh
git clone https://github.com/jorgemanrubia/omacharts
cd omacharts/packaging/aur
makepkg -si
```

The first build takes minutes.

## What the package installs

| Path | What |
|---|---|
| `/usr/bin/omacharts` | The app and the `omacharts` command. |
| `/usr/share/applications/org.omacharts.Omacharts.desktop` | The launcher entry, **Omacharts**, under Office and Finance. |
| `/usr/share/icons/hicolor/*/apps/org.omacharts.Omacharts.*` | The icon. |
| `/usr/share/man/man1/omacharts.1` | `man omacharts`, with every command. |
| `/usr/share/bash-completion/completions/omacharts`, `/usr/share/zsh/site-functions/_omacharts`, `/usr/share/fish/vendor_completions.d/omacharts.fish` | Completions for bash, zsh and fish. |
| `/usr/share/omacharts/agents/` | The agent skill. `omacharts skill install` sets it up for an agent ([The agent skill](/manual/agent-skill/)). |
| `/usr/share/doc/omacharts/` | `README.md` and `cli.md`, the CLI's manual. |
| `/usr/share/licenses/omacharts/LICENSE` | The MIT licence. |

## The first launch

Pick **Omacharts** in the Omarchy launcher, or run `omacharts` in a terminal.

On an Omarchy desktop the first launch also adds the Omarchy plugin to the bar, showing the default watchlist. Remove it and it stays removed. See [Omarchy plugin](/manual/bar-widget/).

Prices come from the default feed, which needs no account ([Market data feeds](/manual/market-data/)). The window follows the desktop's theme ([Themes](/manual/themes/)).

## Uninstalling

Remove the Omarchy plugin first, by turning off Preferences › General › Desktop › **Show in the Omarchy bar** ([Omarchy plugin](/manual/bar-widget/#putting-it-in-the-bar)). From a terminal, `omacharts plugin uninstall` does the same.

Then open the Omarchy menu with <kbd>Super</kbd> + <kbd>Space</kbd>, choose **Remove › Package** and pick **omacharts**. From a terminal:

```sh
omarchy pkg drop omacharts
```

If the package goes first, the plugin hides itself and its folder stays behind.

### What stays behind

Everything you made stays, and installing again later picks up where you left off.

| Path | What |
|---|---|
| `~/.local/share/omacharts/` | Your settings, chartbooks, watchlists, drawings and themes; the backups; the downloaded prices. |
| `~/.config/omacharts/` | Your sign-in to a feed, if you signed in to one. |
| `~/.config/omarchy/plugins/omacharts.watchlist/` | The Omarchy plugin, unless you removed it first. |
| `<Pictures>/Omacharts/` | Screenshots you kept. |
| The agent skill | If you installed it. Run `omacharts skill uninstall` before removing the package ([The agent skill](/manual/agent-skill/)). |

Delete them to start over ([Starting over](/manual/your-data/#starting-over)).

## See also

- [Launching the app from the CLI](/manual/cli/#launching-the-app)
