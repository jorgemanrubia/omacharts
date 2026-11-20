---
title: "Your data"
description: "Backups of your settings, and the cache of downloaded prices."
section: agents
parent: cli
nav_order: 20
---

## `backup`

`backup` keeps and restores copies of your settings ([Where your data
lives](/manual/your-data/)).

```
$ omacharts backup list
settings-20261021-200603.db  2026-10-21 22:06:03     64 kB  schema 4
settings-20261021-180012.db  2026-10-21 20:00:12     64 kB  schema 4
settings-20261020-191533.db  2026-10-20 21:15:33     64 kB  schema 4
  [exit 0]

$ omacharts backup create
backed up the settings to settings-20261021-200135.db (64 kB)
  [exit 0]

$ omacharts backup create
nothing changed since settings-20261021-200135.db, so nothing new was kept
  [exit 0]
```

`backup list` shows each copy's name, when it was taken (local time) and its
size. With `--json` it also names the folder.

`backup restore` takes a name from `backup list`, with or without `.db`, or
the path to a file:

```
$ omacharts backup restore settings-20261021-180012
restored the settings from settings-20261021-180012.db (2026-10-21 20:00:12); what they replaced is kept as settings-20261021-201029.db
  [exit 0]

$ omacharts backup restore ~/Downloads/notes.db
omacharts: notes.db is not an Omacharts settings file; nothing was changed
  [exit 1]
```

A restore does the same as from Preferences; see
[Restoring](/manual/your-data/#restoring). A name that is not a backup fails
with exit 1 and points at `backup list`.

## `cache`

`cache` shows, limits and clears the downloaded prices.

```
$ omacharts cache status
21 series · 4 MB of 1.0 GB · 0%
  [exit 0]

$ omacharts cache limit 2GB
the cache will be pruned back under 2.0 GB
  [exit 0]

$ omacharts cache limit
1.0 GB
  [exit 0]

$ omacharts cache clear
cleared 21 cached series
  [exit 0]
```

`cache limit` without a size reads it. A size is a number with `MB` or `GB`
(`512MB`, `2GB`, `1.5GB`). Any size is accepted, not only the sizes
Preferences offers; something that is not a size is refused with exit 2.
`config set cache_limit` takes the same limit, in bytes. `cache clear`
removes downloaded prices only. Settings, watchlists and chartbooks stay.
