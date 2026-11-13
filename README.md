# Omacharts

**[omacharts.org](https://omacharts.org)**

[![CI](https://github.com/jorgemanrubia/omacharts/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/jorgemanrubia/omacharts/actions/workflows/ci.yml?query=branch%3Amain)

Fast, beautiful charting software for [Omarchy](https://omarchy.org).

<p align="center">
  <a href="https://www.youtube.com/watch?v=B2wG-i3FWjc">
    <img src="assets/examples/video-poster.jpg" width="100%" alt="Watch Omacharts on YouTube">
  </a>
</p>

<p align="center">
  <img src="assets/examples/banner.jpg" width="100%" alt="Four chart layouts, each under a different Omarchy theme">
</p>

## Installing

Omacharts lives in Omarchy's own package repository, which is already enabled
on an Omarchy machine. Open the Omarchy menu with <kbd>Super</kbd> +
<kbd>Space</kbd>, choose **Install → Package**, and pick **omacharts**.

Or from a terminal:

```sh
omarchy pkg add omacharts
```

Either way, that puts the command on your path with the man page, shell completions and
the agent skill, and `pacman -Syu` carries Omacharts along with the rest of
the system.

### The latest release

Omarchy's repository can be a release or two behind. To install the newest
release straight from GitHub, on x86_64 or aarch64, run the same line every
time — it always fetches the latest:

```sh
curl -fLo /tmp/omacharts.pkg.tar.zst \
  https://github.com/jorgemanrubia/omacharts/releases/latest/download/omacharts-$(uname -m).pkg.tar.zst \
  && sudo pacman -U /tmp/omacharts.pkg.tar.zst
```

Either way, the app keeps itself current between packages: it downloads each
new release in the background, without `sudo`, and offers to restart into it.

### From source

On Arch, x86_64 or aarch64, you can build the same package from a clone:

```sh
git clone https://github.com/jorgemanrubia/omacharts
cd omacharts/packaging/aur
makepkg -si
```

To run it from a working tree instead — it rebuilds on every launch — use
`./bin/install`.

## Uninstalling

Open the Omarchy menu with <kbd>Super</kbd> + <kbd>Space</kbd>, choose
**Remove → Package**, and pick **omacharts**.

Or from a terminal:

```sh
omarchy pkg drop omacharts
```

Removing the package hides the bar widget. To uninstall the widget too, run
this before removing the package:

```sh
omacharts plugin uninstall
```

Your data stays in `~/.local/share/omacharts`; delete it for a clean slate. Everything you set up — settings, chartbooks, watchlists, drawings, themes — is in `omacharts.db` there, the file to copy to take it to another machine. Downloaded prices are in `market.db` beside it. Copies of `omacharts.db` are kept in `backups/` there — taken automatically, one every two hours for the last day and one a day for the week before, twenty at most — and any of them can be put back from Preferences › Data or with `omacharts backup restore`.

## Contributing

Pull requests are welcome.

## Licence

MIT. See [LICENSE](LICENSE).
