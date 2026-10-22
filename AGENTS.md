# Working on Omacharts

## Every feature ships with its command

Anything the app can do, `omacharts` can do from a terminal, in the same
change that adds it. A feature with no command is not finished.

This is not a nice-to-have that fell out of building a CLI. It is the point:
a capability you can only reach by clicking cannot be scripted, cannot be
tested from the outside, and cannot be used by an agent at all. The moment
one feature is exempt, the CLI stops being something anybody can rely on,
and the next person has no reason to keep it true either.

### What needs a command, and what does not

In scope — anything that changes **what you are looking at**, or **what is
stored as content or structure**:

- watchlists, their sections, and the symbols in them
- chartbooks: creating, renaming, deleting, switching
- chart layouts: splitting, closing, which chart holds what
- a chart's symbol, resolution, indicators, bar style, session, link mode
- which watchlist a chartbook shows
- settings that are real preferences
- cache management

Out of scope — **presentational geometry**, which only means anything while a
window is on screen:

- sidebar width, split ratios, divider positions, indicator pane heights
- which pane is maximized — remembered with the chartbook so that a restart
  finds it, but filled with a key or the corner, not a command
- scroll and zoom position

Those exist to be dragged with a mouse. A command to set one to 289 pixels is
noise in the help output that makes the useful commands harder to find.

The test to apply: **if a person would reasonably want to script it, or an
agent would need it to set up a working arrangement, it is in.** If it only
exists because something had to have a number, it is out. Note that splitting
a chart is firmly in and the *ratio* of the split is out — building a two-by-
two of particular symbols is worth scripting; nudging a divider to 47% is not.

## Commands act on what you are looking at

Every chart and chartbook verb defaults to the focused chart in the open
chartbook. That is what makes "add an RSI to this" work in one command rather
than a `status` call, a parse, and an identifier threaded into a second one —
and an agent that has to guess an identifier will eventually guess wrong.

Two rules keep that honest, and a new verb has to follow both:

- **Say what it acted on.** With an implicit target, naming the chart in the
  output is the only way anyone catches it reaching the wrong one. "added
  SMA(200) on pos:0 AAPL 1D in Macro", not "ok".
- **Refuse rather than fall back.** No window open means no focused chart.
  That is `EXIT_NO_WINDOW`, with its own message, never an answer taken from
  what was stored when the window last closed — an agent cannot tell stale
  from live and will act on it.

A chart is named back as `pos:N`, its position in the arrangement, not its id.
`materialise_book` hands out fresh pane ids every time it rebuilds, so an id is
only good until the next rebuild. Positions survive. Anything that reports a
chart, or remembers one across a rebuild, has to use the position.

## Where the surface is defined

`src/cli/spec.rs`, and nowhere else. One table describes every command, and
the parser, `--help`, the man page, the shell completions and the JSON surface
are all built from it. A description of a parser kept beside the parser is
wrong by the second release.

The authoritative machine-readable description is:

```
omacharts surface --json
```

That is what anything driving this from a script should read — every command,
every argument and flag with its type, the values enumerated ones accept, the
exit codes, and a worked example per command. `--help` is the path for people
and is held to the same standard, but prose cannot say that a flag takes an
integer without ambiguity, and the JSON can.

## The skill is part of the surface

`agents/skills/omacharts/SKILL.md` is the agent-facing skill, installed — only
ever on request — by `omacharts skill install`, which links it into whichever
agents the machine has. **One file for all of them:** Claude and Codex both read
a directory with a `SKILL.md` out of their own config, so a copy each would be a
second thing to drift. `agents/.claude-plugin/` wraps the same directory as a
Claude Code plugin, which only Claude has a use for; `src/cli/skill.rs` has the
reasoning, including why it is a symlink.

It exists for the two things this file and the surface cannot do: it is read
outside this repo, which is how an agent anywhere on the machine knows to reach
for omacharts at all, and it carries **workflows** rather than vocabulary,
because "a 2×2 of the majors at 15m with RSI on each,
linked" is several commands in an order plus a handful of facts no single
command's help contains.

It deliberately **does not** describe the command surface. That is generated,
it regenerates itself, and a prose copy of it is wrong by the second release —
confidently wrong, which is worse for an agent than nothing at all. One line
points at `omacharts surface --json` and that is the whole of it.

Which makes drift the only real way this gets worse, so it is tested:
**`every_command_in_the_agent_skill_is_a_command_that_runs`** pulls every
`omacharts` line out of the skill's fenced blocks and runs each block, in
order, against a seeded store — and requires exit 0, not merely that it parses.
A workflow is a claim about an order, and "set the chart at `pos:1`" can parse
perfectly after a split that never made one. If you change a command, that test
is what tells you the skill needs changing too.

## Adding a command

1. Add a `Verb` to the right `Noun` in `src/cli/spec.rs`. Give it a real
   `example` — an invocation that works exactly as written, because examples
   are what an agent copies.
2. Set `writes` if it changes anything, and `workspace` if what it changes is
   the stored arrangement of charts rather than a database table. Those two
   flags are what decide whether a window that is open gets flushed before the
   command and refreshed after it.
3. Add the arm in `src/cli/exec.rs`.
4. Return text, never print. The same code answers a terminal in this process
   and a terminal in somebody else's.

Nothing else needs touching. `cargo test` fails if the table and the parser
disagree, if a verb has no example, if an example names the wrong command, or
if the enumerated values drift from what the engine actually accepts.

## Every setting says whether it travels

Settings are backed up, and can be restored from another machine's file. So
every key the app writes is classified in `src/setting_keys.rs`:

- **shared**: somebody's choice. It is backed up, and it comes along when
  settings arrive from another machine. Most keys are this.
- **local**: this machine's own (where screenshots go, how big the cache may
  grow, how wide the sidebar is). It is backed up, but kept as it is when
  another machine's settings are restored.
- **bookkeeping**: the app's note to itself, rewritten as it runs (a sweep
  time, a cooldown, the last symbol). It never makes a backup look necessary.

`every_key_written_is_classified` reads the source for every `set_setting` and
fails on a key nobody classified. When it fails, decide which of the three the
new key is and add it there. Do not silence the test.

## A command must not be able to kill the window

A command runs inside the running instance, and that instance called it from
GLib — a C frame. Unwinding through one aborts the process instead of
unwinding it, so a panic in a command does not fail the command: it takes the
window down and the arrangement on screen with it. Somebody loses their work
because of a typo.

Two rules, and both are in `src/cli`:

- **No `expect`, `unwrap`, indexing or slicing on anything that came from a
  command or from the stored arrangement.** `required` is how a required
  argument is read, `Workspace::book_mut` and `charts::panes_mut` return a
  `Result`, and the arrangement is read out of a settings row that
  `config set workspace` can write anything into. Every one of these used to
  be an `expect`.
- **`cli::run` catches what is left.** A bug nobody foresaw comes back as
  `EXIT_BUG` with a message naming the command, and the window carries on.
  That is a net, not a licence: a command reaching it is still a bug to fix.

## How a command reaches a window that is already open

GTK hands a second invocation's arguments to the instance already running, and
`GApplicationCommandLine` carries that instance's output and exit status back
to the terminal that typed it. So a command runs **inside the app** when there
is one, which is what makes a watchlist created in a terminal appear in the
rail immediately.

With nothing running, the same command runs in the invoking process against
the database, with no GTK and no display. That is what makes it work over ssh.

`src/main.rs` decides between the two by asking the session bus whether the
application id is owned. Nothing polls and nothing watches a file: a command
is a push, and costs exactly nothing until one arrives.

Two rules follow, and breaking either is how this gets slow:

- **Never add a timer or a file watch** to notice external changes. The app is
  told; it does not look.
- **Refresh what changed, not everything.** Rebuilding the window because a
  watchlist gained a symbol is fine with five symbols and stutters with five
  hundred.

### Which is why a test run needs its own bus, not just its own profile

The same dispatch is what makes an experiment land on somebody's real charts.
A command is handed to whatever owns the application id on the session bus,
and a disposable `XDG_DATA_HOME` does nothing about that — the window that
catches it reads its own store, and writes the change through. Setting `HOME`
is no help either: the store follows `XDG_DATA_HOME` first.

So both, every time:

```sh
XDG_DATA_HOME=$(mktemp -d) DBUS_SESSION_BUS_ADDRESS= omacharts chart list
dbus-run-session -- env XDG_DATA_HOME=$(mktemp -d) ./target/debug/omacharts
```

The first keeps a command in its own process; the second is for when the run
needs a window of its own. Two separate sessions have changed a live chart by
getting this half right.

### And its own compositor, when the run needs to be driven

A window that has to be typed into, clicked on and photographed needs a
third thing nobody has: a screen. This desktop has one monitor and somebody
is using it, so a test window either takes it or sits unfocused, and an
unfocused GTK4 window is one synthetic input never reaches. An agent gave
up on verifying a drawing fix for exactly that reason, and another worked
around it by patching the app to call a button's own handler from a timer —
which proves the handler runs, not that clicking the button does.

`bin/headless-window` is the answer: a second compositor with no monitor
behind it, on its own bus and its own profile, which an agent owns
completely.

```sh
eval "$(bin/headless-window start)"
bin/headless-window app
bin/headless-window key ctrl+b
bin/headless-window click 753 24
bin/headless-window drag 900 400 400 400
bin/headless-window shot /tmp/after.png
bin/headless-window stop
```

It is sway on its headless backend, because the headless backend is the
part Hyprland does not have: aquamarine builds its allocator from a
backend's DRM descriptor, the headless one has none, and a Hyprland with
nothing else available refuses to start at all. Nested Hyprland works only
inside another Wayland compositor, which means a window on the screen this
exists to stay off.

Native Wayland throughout, which is the other half of the point: the output
is 2560×1440 at scale 2, so the window is drawn at HiDPI and a screenshot
is what a user would actually see. `GDK_BACKEND=x11` gets no scaling under
XWayland, which is how a test window came to look broken enough that
somebody asked whether the app was.

What it needs from the machine is `sway`; `wtype`, `grim`, `dbus-daemon`
and `python3` are already here. No root, no `uinput`, no seat of its own,
and nothing written outside its own directory under `$XDG_RUNTIME_DIR`.
The limits — coordinates are layout coordinates, the first chord a fresh
window is sent is swallowed and has to be sent twice, and `swaymsg`'s own
cursor command cannot drag — are in the review skill's section on driving
the window, which is where an agent is when it needs them.

## A fresh target directory is a cold build, and cold here means minutes

The dependency graph includes the GTK bindings, so building this workspace
from nothing compiles over a thousand crates. A warm `target/` in the repo
makes a local change feel instant — one crate recompiled and a relink — and
that is misleading: point `CARGO_TARGET_DIR` somewhere new and the same
change takes minutes instead.

Which matters because concurrent agents are each given their own target
directory on purpose. Two builds of different branches both write
`target/debug/omacharts`, so sharing one directory means overwriting each
other's binary mid-run and reporting results from a mix of the two. The
directories have to stay separate; what should be shared is the compilation
cache underneath them:

```sh
sudo pacman -S --needed sccache
```

```toml
# ~/.cargo/config.toml
[build]
rustc-wrapper = "sccache"
```

Then the first build to need a crate pays for it and every other target
directory gets it from the cache, so a fresh directory stops meaning a cold
build. `sccache --show-stats` reports the hit rate. Do not add this to the
repo's own `.cargo/config.toml` — it is a property of the machine doing the
building, not of the project, and CI has no cache to hit.

Before blaming a PR for a slow review, check which of the two kinds of build
it was.

## A regression is measured, not noticed

Three things are what this app is for: being quick to have a chart on screen,
being quick to draw one, and answering a hand or a keyboard without a pause.
All three are measured, and the pull request that breaks one says so rather
than somebody noticing a month later that the app feels slow.

```sh
cargo run --release --example perf          # the table, for reading
bin/perf-compare                            # the table twice, against main
```

`examples/perf.rs` is the scenarios: a startup budget, frame costs at four
screenfuls across one and four charts, and the latency of a pointer motion, a
character typed into the search box, a tick arriving and a command running.
Offline and deterministic, every row — a synthetic series, a scratch `HOME`,
no socket. `frame_bench` and `stream_bench` stay what they were: the same
scenes, drawn through the same `Scene`, printed as a mean for somebody reading
them.

`.github/workflows/perf.yml` runs the comparison on every pull request, in a job
of its own so the build and the tests are not behind it, and puts the table in
the job summary rather than in a comment — which is why it needs no write token
and no secret. What it
fails on is the allocation counts, which are exact and the same on every
machine. The clock is reported and only fails at a doubling, because a GitHub
runner is a slice of a shared machine and below that a regression and a busy
neighbour are the same number. `bin/perf-compare`'s header has the whole of the
reasoning; read it before changing a threshold.

Two things follow for anyone adding to the harness. It must stay inside
`examples/perf.rs` and `src/ui/chart/bench.rs`, because the comparison copies
those two files onto the older revision — a harness that reached into a third
file could not be grafted, and the job would stop comparing and start reporting.
And a scenario has to be idempotent: its allocation counts are taken from a
single run after a warm-up, and the two passes of a side have to agree on them
to the byte. One that leaves something behind measures a different thing the
second time, and the comparison says so rather than picking a number.

## Verifying parity

Before calling a feature done:

```
omacharts surface --json | jq -r '.commands[].command'
```

Read that list against what the UI can do. If the feature you just added is
not in it, and it is not presentational geometry, it is not finished.

### What the tests enforce on their own

A contract nobody can check rots at the first hurried feature, so four tests
fail rather than leaving it to somebody remembering to read the list.

- **`every_field_a_chart_is_stored_with_is_reachable_from_a_command`** reads
  `StoredPane` and `Chartbook` out of `window.rs` and requires every field to
  name the command that sets it, or to say why it needs none. A new piece of
  chart state fails here until one of the two is true. It checks the command
  and the flag against `spec`, so a name that no longer exists fails too.
- **`every_example_in_the_table_is_a_command_that_runs`** runs every verb's
  example against a seeded store with a window standing in. An example that
  cannot be parsed, or an argument the table describes and the arm never
  reads, fails here — which is exactly how `chart focus` shipped broken.
- **The enumerated-value tests** — bar styles, sessions, indicators, anchors,
  line styles, link modes — compare what the table offers against what the
  engine defines. One caught the table offering four bar styles when there
  were two.
- **The key-pinning test** — `the_scheme_this_remembers_is_the_one_the_dialog_remembers`
  — holds a command and the window to the same settings key where the
  window's own constant is private to it.

Say plainly what they cannot reach, because a test that looks like it proves
parity and does not is worse than none:

- **A capability that changes nothing stored.** Scrolling and zooming a chart
  are out of scope by design, and the field test cannot tell them apart from
  something that should have been in.
- **Settings reached only through `config set`.** `timeframes` and
  `watchlist_columns` are real preferences with no command of their own, and
  nothing fails because of it.
- **A resolution.** `--resolution` takes anything parseable rather than a
  fixed set, so there are no variants to compare; the test checks only that
  every resolution the header strip offers is one a command accepts.
- **Whether a window that is open actually catches up.** Nothing tests that,
  and `config set theme` is the case that does not: it writes the row and the
  running window keeps the theme it started with.

## Every change ships with its manual

A pull request that adds a feature, or changes what a user sees or does,
updates the manual under `site/manual/` in the same pull request: the
chapter that owns the topic, the CLI page when a command or flag changes,
Hotkeys when a key changes, and any screenshot the change makes out of date
(each one carries the recipe to take it again). Write it following
`doc/documentation.md`, the manual's style guide. A change is not finished
until the manual describes it. A manual that lags one release is wrong for
everybody who reads it, and the next person has no way to know which parts
to trust.

What needs no manual change: internals a user never meets, and fixes that
make the app do what the manual already says.

## Documentation is written in one voice, and the guide to it is a record

Before writing or changing anything a person reads — the manual under
`site/manual/`, the README, `doc/*.md`, the `--help` text — read
`doc/documentation.md`. It says how documentation here is written, down to
how a key and a command are spelled and how a screenshot is taken and made
reproducible, and it is kept in the voice it describes.

It is also where the maintainer's steering on documentation is written down
as it was given, so the next page starts from what the last one was corrected
on. Feedback on a page goes into that file in the same change as the fix,
as the concrete preference rather than a principle distilled from it.

## An agent writing on GitHub says so

Anything an agent posts to GitHub in the maintainer's name — a pull request
comment, an issue reply, a review, a close — starts with 🤖, as the first
character of the message, before anything else:

```
🤖 Thanks. I moved in_regular_hours into the test module before merging …
```

The account is a person's, so without the marker a contributor has no way to
tell whether the thing arguing with their design decision read their code or
generated a paragraph about it. One character tells them, and it tells them
before they have read the argument rather than after.

It goes on what the project says, not on what it does: commit messages, PR
descriptions and code comments carry no marker. Co-authorship in a commit
trailer already records that, and a comment in the source is the project's
voice regardless of who typed it.

## Releasing

```sh
bin/release 0.1.3
```

That is the whole thing. A release bumps the last number — 0.2.0 is followed
by 0.2.1, new features or not — unless the maintainer explicitly says otherwise. The script writes the version into the three places a
tag cannot float — `Cargo.toml`, `Cargo.lock` and the PKGBUILD's `pkgver` —
then builds and tests with `--locked` to prove the
tree agrees with itself, commits, tags and pushes. It refuses to run on a dirty
tree, off `main`, behind `origin`, or onto a tag that already exists.

Everything is written **before** the tag, on a machine with a toolchain. The
tag then points at a commit that is already consistent. The one exception is
the digest of the tarball GitHub builds for the tag, which cannot exist until
the tag does; `.github/workflows/release.yml` writes that single line afterwards
and nothing else.

The workflow then runs three jobs in order:

1. **build** — compiles the binary, tars it with the licence, README, assets and
   packaging, and creates the release with notes generated the way the
   "Generate release notes" button generates them. `.github/release.yml` decides
   how they are grouped.
2. **sync** — writes the tarball's `sha256sums` into the PKGBUILD and commits it
   to `main`. No toolchain, no cargo: it refuses outright if the tag does not
   already name itself in the PKGBUILD, because that means something was tagged
   by hand.
3. **package** — builds the Arch package in an `archlinux:base-devel` container
   from that PKGBUILD and attaches it to the release. Because it builds from the
   PKGBUILD the digest was just written into, a release that gets this far has
   proved the recipe works against the tarball GitHub actually serves.

Afterwards, `git pull` to pick up the digest commit.

### Label the pull request before it is merged

The notes are generated, not written, and `.github/release.yml` groups them by
the labels on the pull requests that landed: `enhancement` or `feature` becomes
**Added**, `bug` or `fix` becomes **Fixed**, and anything unlabelled falls
through to **Changed**. So a release where nobody labelled anything comes out as
one flat list under a heading that says nothing — which is the thing that
config exists to prevent.

Label it when you merge it, while you still remember which it was. Fixing it
afterwards means regenerating and replacing the notes by hand:

```sh
gh pr edit <N> --add-label bug
gh api -X POST repos/jorgemanrubia/omacharts/releases/generate-notes \
  -f tag_name=v0.1.5 -f previous_tag_name=v0.1.4 --jq .body > notes.md
gh release edit v0.1.5 --notes-file notes.md
```

### The README is updated by the change, not by the release

`bin/release` touches one line of it — the version in the download command —
and nothing else. A capability that needs explaining to somebody reading the
README is explained in the pull request that adds the capability, in the same
change, the same way the command and its `doc/cli.md` entry are. Leaving it for
release day means writing from memory about work that is already merged.

### Do not bump the version by hand

Three files have to agree, and `Cargo.lock` is the one everybody forgets: a lock
file left behind its own `Cargo.toml` makes every `--locked` build refuse, which
turns CI red on `main` and on everything branched from it. `bin/release` checks
all three and will not tag if any disagrees.

### If something goes wrong

The release exists as soon as **build** finishes, so a failure in **sync** or
**package** leaves a release carrying the generic tarball but no Arch package.
Fix the cause and re-run the failed job from the Actions tab.

Re-running uses the workflow **as it was at the tag**, so a fix to the workflow
itself does not apply to a tag already cut. Either build and attach the package
by hand that once, or cut the next version.

Never move a tag that has been released. The PKGBUILD digest would stop matching
the tarball, and `makepkg` would refuse it for everybody who tried to build it.
