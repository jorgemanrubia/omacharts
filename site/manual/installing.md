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

Either way, Omacharts keeps itself up to date from there ([Updates](#updates)).

## Updates

Omacharts downloads each new release from GitHub in the background and checks it against the release's published checksum. When one is ready, a notice in the window's corner says **Omacharts 0.2.15 is ready** with **Restart to update**. Restarting, or just quitting and opening it again, starts the new version where you left off.

Nothing needs <kbd>sudo</kbd>: the package's files are never touched. Downloads go to `~/.local/share/omacharts/versions/`, which holds only the newest. Once the package catches up with a download, through `omarchy update` or `pacman -Syu`, the download is deleted and the package's own version runs.

Turn it off with Preferences › General › Updates › **Update automatically**. What is already downloaded stays in use until the package catches up.

| Command | Does |
|---|---|
| `omacharts update check` | Asks GitHub for the newest release and says whether it is newer. |
| `omacharts update download` | Downloads it now, whatever the switch says. The next launch uses it. |
| `omacharts update reset` | Removes the downloads, so the next launch runs the package's version. |

A build from a working tree never updates itself, and nothing is downloaded or run from a download as root.

The menu entry, the icon, the manual page and the agent skill are the package's, and stay at its version until the package catches up.

### When an update misbehaves

A downloaded version that will not start is set aside, and the package's version runs instead. Three launches in a row that end before its window has been up for a few seconds, or whose own update of your settings fails, set it aside too. A full disk, a settings file another program holds, or a launch with no display to open, from ssh or a script, doesn't count. A version that has run properly once is never set aside, because it may be the only one that can open your settings. A download that doesn't match its checksum is tried again at the next check, and set aside if it doesn't match twice.

Before a downloaded version first opens your settings, a copy of them is kept as `~/.local/share/omacharts/versions/.saved-<version>`, apart from the regular backups so it isn't pruned. If it can't be taken, on a full disk for example, that version leaves your settings alone: its window runs with nothing saved, and its commands stop with an error, until there is room. If a version that was set aside had already updated your settings to a form the running version can't read, and nothing newer can open them, they are put back from the newest such copy the running version can read. The window says so once, even when a command did it. What changed since is kept beside them in `~/.local/share/omacharts/`, as `omacharts.db.from-<version>-<time>`.

To run the package's version once, without removing anything:

```sh
OMACHARTS_NO_UPDATE=1 omacharts
```

To go back to the package's version for good, until the next release:

```sh
omacharts update reset
```

That keeps a newer version that has already run, because it may have updated your settings to a form the package's version can't read. `omacharts update reset --all` removes that one too, and forgets which backup came before which version, so nothing is put back for you after it.

If the package's version then says the settings were written by a newer Omacharts, either:

- run `omacharts update download` to fetch the newest release again, which can read them; or
- put back the copy taken just before the download first opened them, `~/.local/share/omacharts/versions/.saved-<version>`, which a reset keeps until the package catches up with that version, or an older backup from `~/.local/share/omacharts/backups/` ([Backups](/manual/your-data/#backups)). With Omacharts closed, copy it over `~/.local/share/omacharts/omacharts.db` and delete `omacharts.db-wal` and `omacharts.db-shm` beside it if they are there.

`update reset` also forgets a version that was set aside, so it can be downloaded again.

The downloads are for one machine. A home folder shared between machines over the network, with different architectures or Omacharts running on more than one at once, is not supported.

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
| `/usr/bin/omacharts` | The `omacharts` command. It starts the app, or a newer downloaded version. |
| `/usr/lib/omacharts/omacharts` | The app. |
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
| `~/.local/share/omacharts/` | Your settings, chartbooks, watchlists, drawings and themes; the backups; the downloaded prices; a downloaded update. |
| `~/.config/omacharts/` | Your sign-in to a feed, if you signed in to one. |
| `~/.config/omarchy/plugins/omacharts.watchlist/` | The Omarchy plugin, unless you removed it first. |
| `<Pictures>/Omacharts/` | Screenshots you kept. |
| The agent skill | If you installed it. Run `omacharts skill uninstall` before removing the package ([The agent skill](/manual/agent-skill/)). |

Delete them to start over ([Starting over](/manual/your-data/#starting-over)).

## See also

- [Launching the app from the CLI](/manual/cli/#launching-the-app)
