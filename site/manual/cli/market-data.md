---
title: "Market data"
description: "Choosing the data feed, and signing in to one."
section: agents
parent: cli
nav_order: 17
---

`config set provider NAME` stores the feed ([Market data
feeds](/manual/market-data/)); the names are in `provider list`.
A name that is not a feed is refused with exit 2. `omacharts --provider NAME`
uses a feed for one launch without changing the stored setting ([Launching
the app](/manual/cli/#launching-the-app)). The feed in use is `--provider` when it was
given, otherwise the stored setting, otherwise Yahoo.

thinkorswim signs in through Chromium, Chrome, Brave or Edge. If the app does
not find your browser on its own, set `TOS_BROWSER` to its path.

## `provider login` and `provider logout`

`provider login` signs in to a feed, as **Sign in…** in Preferences does. It
waits in the terminal while you sign in and prints the browser's progress on
stderr. `provider logout` signs out. A session lasts until thinkorswim ends
it; `provider status` says when that happened. Signing in again asks for no
new code while thinkorswim still trusts the browser you signed in with.

```
$ omacharts provider login
a browser is opening at thinkorswim; sign in there
…
signed in to thinkorswim
  [exit 0]

$ omacharts provider logout tos
no saved thinkorswim session to forget
  [exit 0]
```

Both take a feed's name and default to the feed in use. `provider logout
yahoo` is refused with exit 5; Yahoo has no sign-in.

## `provider list` and `provider status`

`provider list` shows every feed with what is stored and what is running.
The two differ only when the launch was given `--provider`.

```
$ omacharts provider list
yahoo    Yahoo Finance · Every listing the symbol search covers · delayed 15 min for indexes, 10 for futures   (in use)
tos      thinkorswim · US listings, from your thinkorswim account · live, as each print arrives · experimental   (not signed in)
  [exit 0]
```

`provider status` describes the feed in use: whether you are signed in (and
when a session expired) and, for a streamed feed with a window open, what is
streaming. While the market is shut, the line says a series has nothing new.

```
$ omacharts provider status
Yahoo Finance · Every listing the symbol search covers · delayed 15 min for indexes, 10 for futures
nothing to sign in to; it works as it is
delivery: polled — charts are refetched on a timer
  [exit 0]

$ omacharts provider status
thinkorswim · US listings, from your thinkorswim account · live, as each print arrives
Signed in · account D-1 · signed in 6 Oct 2026
session: /home/you/.config/omacharts/tos.env
browser profile: /home/you/.config/omacharts/tos-browser
delivery: streamed — bars arrive as they print, and nothing fetches on a timer
streaming 2 series:
  /ES 1m · 2 charts · 2602 bars · last bar 14:32 · updated 3 s ago
  AAPL 1D · 1 chart · waiting for the first snapshot
  [exit 0]
```

With `--json` the same facts are fields: `delivery`, `subscriptions`, and
for `provider list` `stored`, `inUse`, `needsSignIn` and `session` per feed.
