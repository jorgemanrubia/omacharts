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
- a chart's symbol, resolution, indicators, bar style, session, link group
- which watchlist a chartbook shows, and which link group a watchlist drives
- settings that are real preferences
- cache management

Out of scope — **presentational geometry**, which only means anything while a
window is on screen:

- sidebar width, split ratios, divider positions, indicator pane heights
- which pane is maximized
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
  line styles, link groups — compare what the table offers against what the
  engine defines. One caught the table offering four bar styles when there
  were two.
- **The key-pinning tests** — `the_group_a_command_sets_is_the_one_the_rail_reads`
  and `the_scheme_this_remembers_is_the_one_the_dialog_remembers` — hold a
  command and the window to the same settings key where the window's own
  constant is private to it.

Say plainly what they cannot reach, because a test that looks like it proves
parity and does not is worse than none:

- **A capability that changes nothing stored.** Scrolling, zooming and
  maximizing a chart are out of scope by design, and the field test cannot
  tell them apart from something that should have been in.
- **Settings reached only through `config set`.** `timeframes` and
  `watchlist_columns` are real preferences with no command of their own, and
  nothing fails because of it.
- **A resolution.** `--resolution` takes anything parseable rather than a
  fixed set, so there are no variants to compare; the test checks only that
  every resolution the header strip offers is one a command accepts.
- **Whether a window that is open actually catches up.** Nothing tests that,
  and `config set theme` is the case that does not: it writes the row and the
  running window keeps the theme it started with.

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

That is the whole thing. The script writes the version into the four places a
tag cannot float — `Cargo.toml`, `Cargo.lock`, the README's download line, and
the PKGBUILD's `pkgver` — then builds and tests with `--locked` to prove the
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

Four files have to agree, and `Cargo.lock` is the one everybody forgets: a lock
file left behind its own `Cargo.toml` makes every `--locked` build refuse, which
turns CI red on `main` and on everything branched from it. `bin/release` checks
all four and will not tag if any disagrees.

### If something goes wrong

The release exists as soon as **build** finishes, so a failure in **sync** or
**package** leaves a release carrying the generic tarball but no Arch package.
Fix the cause and re-run the failed job from the Actions tab.

Re-running uses the workflow **as it was at the tag**, so a fix to the workflow
itself does not apply to a tag already cut. Either build and attach the package
by hand that once, or cut the next version.

Never move a tag that has been released. The PKGBUILD digest would stop matching
the tarball, and `makepkg` would refuse it for everybody who tried to build it.
