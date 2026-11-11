# Bundled Omarchy palettes

Each file here is the `colors.toml` of one of Omarchy's themes, copied
unchanged from `themes/<name>/colors.toml` in
[Omarchy](https://github.com/omacom/omarchy), which is MIT-licensed,
copyright David Heinemeier Hansson. `omarchy::bundled` builds a few of them
into the binary, for a machine without Omarchy, and
`tests/omarchy_themes.rs` holds every one to the contrast rules.

Most are Omarchy's take on a palette published elsewhere. Where the
original could be confirmed, it is:

| File | Palette | Licence |
|---|---|---|
| `catppuccin.toml`, `catppuccin-latte.toml` | [Catppuccin](https://github.com/catppuccin/catppuccin) Mocha and Latte | MIT |
| `everforest.toml` | [Everforest](https://github.com/sainnhe/everforest) | MIT |
| `flexoki-light.toml` | [Flexoki](https://github.com/kepano/flexoki) | MIT |
| `gruvbox.toml` | Gruvbox, as [gruvbox.nvim](https://github.com/ellisonleao/gruvbox.nvim) draws it | MIT |
| `kanagawa.toml` | [Kanagawa](https://github.com/rebelot/kanagawa.nvim) | MIT |
| `matte-black.toml` | [Matte Black](https://github.com/tahayvr/matteblack.nvim) | MIT |
| `nord.toml` | [Nord](https://github.com/nordtheme/nord) | MIT |
| `retro-82.toml` | [Retro '82](https://github.com/OldJobobo/retro-82.nvim) | CC0-1.0 |
| `rose-pine.toml` | [Rosé Pine](https://github.com/rose-pine/rose-pine-theme) | MIT |
| `tokyo-night.toml` | [Tokyo Night](https://github.com/tokyo-night/tokyo-night-vscode-theme) | MIT |

The rest (`ethereal`, `hackerman`, `last-horizon`, `lumon`, `lupine`,
`miasma`, `osaka-jade`, `ristretto`, `solitude`, `vantablack`, `white`) are
Omarchy's palettes, with no original we could confirm, and are taken under
Omarchy's licence. `osaka-jade` is from an earlier Omarchy release; the
current one no longer ships it.

A new Omarchy theme joins by copying its `colors.toml` in under the theme's
directory name, and a row above when its palette comes from elsewhere.
