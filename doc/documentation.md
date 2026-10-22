# Writing documentation for Omacharts

This is how documentation is written here: the manual under `site/manual/`,
the README, `doc/*.md`, and the `--help` text and tooltips in the binary. Read
it before writing or changing any of them. It is two things — a description
of the voice and the shape of a page, and a record of the maintainer's
corrections, which is the part that keeps it honest. The last section says
how to add to it.

## Who it is for

Regular people who use Omacharts to look at markets: traders and investors
on Omarchy, not programmers or engineers. Write for somebody who knows what
a chart, a symbol and a moving average are, and does not know or care how
software works inside. When a sentence would only make sense to a developer,
it does not belong in the manual.

## What the manual is

The letter, then parts, read in order by somebody new and jumped into by
everybody else. The parts are `manual.sections` in `site/_config.yml`; a
chapter joins one with `section:` in its front matter.

1. **The letter.** *A philosophy of charting software*, written by the
   maintainer. Nobody else writes or edits it.
2. **Introduction.** *How Omacharts works*, *Quick start* and, last,
   *Installation*. The first two are a
   conversational walk through the model of the app — the window, charts,
   chartbooks, watchlists, templates, linking, the theme, the CLI driving
   the same window, the bar widget, the feeds — that explains how the pieces
   fit and why, then puts a chart on screen in a few minutes. It stays at
   the level of the model and sends the reader to the reference for the
   rest.
3. **The reference parts**, one chapter per topic, written for lookup:
   every key, command, option, setting and dialog that belongs to the
   topic, each with its behaviour, dense and scannable.
   - **Charting**: the window, symbols, charts, indicators, templates,
     drawings, layouts, chartbooks, watchlists (linking included),
     screenshots.
   - **Configuration**: themes, market data feeds, preferences, your data.
   - **Agents**: the CLI and the agent skill.
   - **Appendices**: the Omarchy plugin, then hotkeys.

There is no troubleshooting chapter. What a message on screen means
belongs in the chapter about the thing that shows it.

The README is the front door and stays short; `doc/cli.md` is the terminal's
own manual; `--help` is read by somebody halfway through typing a command.
All of them share the voice.

## The voice

Write like a person who knows the app explaining it to another person who
wants to use it. Plain, direct, helpful. If a sentence would sound odd said
out loud to a colleague, rewrite it.

- **Say what the thing is first.** Before how it behaves or why it exists,
  say plainly what it is: "A chart template is a saved set of indicators.
  Put several charts on the same template and they all show those
  indicators; change the template and they all change." Not "Two kinds of
  thing get repeated across charts, and each has a template so that you
  change it once."
- **Then what you do with it**, in the order somebody would do it.
- **A reason only where the reader would ask for one.** Most behaviour needs
  no justification. When a reason helps somebody predict a case the page
  does not list, give it in a plain sentence of its own, not tacked on after
  a dash.
- **No mannerisms.** No epigrams, aphorisms or punchlines; no sentences built
  to land a point ("Both exit 0." as a closer); no "A is B: C" headings; no
  parallel triplets for rhythm; no sections that end on a moral. Few dashes
  and colons — a full stop is usually better.
- **Addresses the reader as you.** *We* is the project, used sparingly. *I*
  belongs to the letter alone.
- **Shows real things.** Real symbols (NVDA, SPY, a watchlist called Semis),
  real commands with their real output and exit code, real numbers. An
  example is run before it is written down.
- **Says what happens at the edge**, plainly: what is refused, what changes
  nothing.
- **Uses plain words, and the app's own.** Chartbook, watchlist, sidebar,
  strip, chart template, link mode, Omarchy plugin: the words the UI uses, the same word
  every time. No synonyms for variety.
- **British spelling** in prose: colour, licence, maximise, centre. Code is
  spelled as the code spells it: `--color`, `chart set --colouring`.
- **Contractions are fine** where they make a sentence sound like speech.

Do not take the voice from the code comments or `AGENTS.md`: those argue a
design to other developers, and their habits (a reason after every dash, a
closing line that lands) read as mannered in a manual.

## Two registers

**The guide** talks. Paragraphs, a thread the reader follows, each idea
introduced by what it is and what you would use it for. It may spend three
sentences on one concept and show one screenshot per idea. It names the
chapter where the detail lives rather than giving the detail.

> A chartbook is a saved screen: the charts, how they are split, and the
> watchlist beside them. You can keep several, say one for the open, one
> for the week and one for metals, each with its own watchlist. Switch
> between them with Ctrl+Alt and an arrow, and each comes back as you left
> it.

**The reference** lists. Short paragraphs for behaviour, tables for anything
with more than two entries — keys, options, settings, parameters — one
precise line each. The behaviour is in the line; nothing restates what the
label already says. Explain only what is not obvious: the default, an
interaction with something else, an edge.

> | Key | Does |
> |---|---|
> | Ctrl+M | Gives the focused chart the whole window, or puts it back. Remembered with the chartbook. |
> | Ctrl+Alt+X | Closes the focused chart. The same key with Shift removes the chartbook. |

Both registers are the same voice, the reference with less room.

## Complete for what a user can do, silent on internals

The measure is Sierra Chart's documentation: every option documented, from
the user's side. Ours is held to that coverage and to a fraction of the
length — correct and succinct, one line per option where a line suffices,
a table where there is more than one option.

*Everything* means everything a user can see, do, configure or run: every
key, menu item, dialog control, command, flag, indicator parameter, drawing
option, exit code, Preferences row, and every file a user may touch or copy.
A reference chapter that leaves one out is not finished, in the same way a
feature without a command is not.

*Internals* means ids, migrations, storage shapes, code paths, how settings
keys are classified, how a widget is composed. None of it. A setting is
documented as the user meets it — a Preferences row, or a key
`omacharts config set` takes — never as a column.

The chapter outline carries a coverage checklist mapping every user-facing
item to the chapter that owns it. A writer finishes a chapter by walking that
list.

## The shape of a reference page

1. **One paragraph that says what the thing is and why it exists.** Two at
   most.
2. **A screenshot**, if the page is about something you look at.
3. **Sections by sub-topic.** Each: the behaviour in prose, then the keys.
   Tables where there are more than two of anything.
4. **No CLI section.** Every command and flag is in *The CLI*
   (`site/manual/cli.md`), one section per topic. The chapter ends with a
   `## See also` heading and a bullet list of links under it, even when
   there is only one — `- [Managing chartbooks with the
   CLI](/manual/cli/#chartbooks)` — and may name a single command in prose
   where it makes a point.
5. **Cross-links** to the owning chapter for anything mentioned but owned
   elsewhere, instead of repeating it. A key is owned by the chapter whose
   behaviour it triggers and listed again, once, in *Hotkeys*; indicator
   parameters live in *Indicators* and nowhere else.
6. **No closing summary.** The last section ends, then *See also*, and the
   page ends.

Headings are sentence case and name the topic plainly, the way a table of
contents would: "Chart templates", "Changing a template", "Choosing a link
mode". Not slogans or claims ("Templates: change one, and everything on it
follows").

## Commands, keys and names

- **Keys** as the desktop writes them: Ctrl+K, Ctrl+Shift+L, Alt+L,
  Ctrl+Alt+←. In the manual each key is its own `<kbd>`, joined by a plain
  `+` (<kbd>Ctrl</kbd>+<kbd>K</kbd>), because the site draws them as keycaps.
  Elsewhere — the README, `doc/*.md`, help text — plain text, with `<kbd>`
  only for the Super key on the installation steps.
- **Commands** in backticks inline: `omacharts chart set --resolution 1h`. A
  transcript shows the prompt, the output and the exit code, as a real run
  produced them:

  ```
  $ omacharts chart indicator add sma --period 200 --color Amber
  added SMA(200) on pos:0 AAPL 1D in "Macro"
    [exit 0]
  ```

- **Paths through the UI** with ›: Preferences › Charts › Defaults. Labels as
  the UI spells them, in the same case, in bold: **Apply to all charts now**.
- **Enumerated values** as the command takes them: `regular`, `extended`;
  `follow`, `focus`, `pinned`.
- **The app** is Omacharts; **the command** is `omacharts`. A chart is named
  `pos:N`, counting from 0.
- **Settings** by key in backticks, with the values they take:
  `chart_header` is a comma-separated list of `symbol`, `resolution`,
  `change` and `template`, or `none`.

## Screenshots

A screenshot earns its place when the page is about something you look at —
a dialog, the sidebar, a drawing, an indicator pane — and a reader would
otherwise have to imagine a layout. It does not earn one for a command's
output, which is text, or to decorate.

Every screenshot is a **fresh capture of the current UI**. Nothing under
`assets/examples/` goes in the manual: those are older, and the window has
moved on. Captures are taken with the repository's own harness, which gives
the window a screen nobody is looking at, with a profile and a bus of its own:

```sh
eval "$(bin/headless-window start)"        # 2560×1440 at scale 2
bin/headless-window app                     # the built binary, inside
bin/headless-window cli chart set --symbol NVDA --resolution 1D
bin/headless-window cli chart screenshot --output site/assets/manual/charts/nvda-daily.png
bin/headless-window shot /tmp/window.png    # the whole output, for a dialog or the sidebar
bin/headless-window stop
```

`chart screenshot` and `chartbook screenshot` give the chart without its
controls, which is what a page about charts wants; `shot` gives the whole
window, which is what a page about a dialog or the sidebar wants, cropped
afterwards. The harness sets `XDG_DATA_HOME` to a directory of its own; set
`HOME` to a scratch directory as well, so a dev build never syncs its bar
plugin into the real `~/.config`, and seed the theme deliberately rather
than inheriting whatever the desktop has, so two captures a month apart
match.

They live under `site/assets/manual/<chapter>/`, as PNG, named for what they
show: `drawings/properties-dialog.png`. The scaffolding decides the final
path; that is the default.

**Every screenshot carries the recipe to remake it**, as an HTML comment
directly above the image in the Markdown, so that after the UI changes an
agent can regenerate it without guessing:

```markdown
<!-- regenerate:
theme: tokyo-night
window: 2560x1440 @2, bin/headless-window
state:
  omacharts chartbook create Manual --symbol NVDA --switch
  omacharts chart set --resolution 1D --session regular
  omacharts chart indicator add sma --period 200 --color Amber
  omacharts chart drawing add hline --from 2026-09-01,178.4 --to 2026-10-20,178.4 --config 2
capture: bin/headless-window cli chart screenshot --output site/assets/manual/charts/sma-and-level.png
crop: none
-->
![NVDA daily with a 200-day average and a level drawn at 178.4](../assets/manual/charts/sma-and-level.png)
```

`theme` is what the capture was taken under; `window` is the output size and
scale; `state` is every command that built what is on screen, in order, from
a fresh profile; `capture` is the exact command; `crop` is `none` or the
rectangle in output pixels as `x,y,w,h`. A dialog or a key-driven state lists
the keys and clicks under `state` as the harness takes them
(`key ctrl+shift+s`, `click 753 24`). The alt text says what the picture
shows, in one sentence.

## What to avoid

| Not this | This |
|---|---|
| Simply press Ctrl+K to easily find a symbol. | Ctrl+K finds a symbol. |
| Omacharts allows you to split charts. | Split a chart with Ctrl+Alt+H. |
| This powerful feature lets you… | *(say what it does)* |
| You might want to consider adding an RSI. | Add an RSI when… *(or leave it out)* |
| Note that it should probably work over ssh. | It works over ssh. |
| In this section we will look at watchlists. | *(start with the watchlist)* |
| Ten steps, a minute or two each. Each one is the shortest way to… | *(the first step)* |
| To summarise, chartbooks are… | *(end the section)* |
| Great! Your chart is ready! | Your chart is on screen. |
| Please see the following table. | *(the table)* |
| i.e., e.g., etc. | that is; such as; and the rest |

No exclamation marks. No emoji, anywhere in documentation. No "simply",
"just", "easy", "powerful", "seamless", "intuitive", "robust". No hedging —
if the behaviour is uncertain, run it and find out; if it depends, say on
what. No marketing: the README's feature list is the one place an adjective
like *fast* appears, and it appears with its reason. No second-guessing the
reader ("you may be wondering"). No filler between a heading and the first
fact.

## This file is a record of the maintainer's steering

This file is living. Whenever the maintainer gives feedback on documentation
— on a page, a sentence, a screenshot, a choice of structure — update this
file in the same change as the fix, and record **the concrete preference as
the maintainer expressed it**: what they reacted to, what they asked for instead, with the
example if there was one. Do not generalise it into a principle about how to
maintain documentation; a principle loses the case that produced it, and the
next writer needs the case. One dated line each, newest last.

- 2026-10-21 — The manual has three tiers: the maintainer's letter first; then a
  conversational "how it works" that walks the model of the app, with a
  quick start (install → first launch → a symbol on screen → a split, an
  indicator, a watchlist, a chartbook, a CLI command, in a few minutes);
  then reference chapters that are "proper, more reference-like docs —
  precise, scannable, complete per topic (every key, command, option,
  setting with its behaviour), still in the same voice but denser and
  organized for lookup rather than narrative".
- 2026-10-21 — Dedicated reference chapters for Hotkeys (every shortcut in
  the app, grouped by context) and Indicators (one entry per indicator: what
  it shows, every parameter with defaults and allowed values, its pane,
  colours and styles, the CLI to add it). Other chapters cross-link to them
  rather than duplicating tables.
- 2026-10-21 — "EVERYTHING must be documented — no feature, command, flag,
  key, setting, preference, dialog, menu item, indicator parameter, drawing
  option, exit code or data/file location left out", from the user's
  perspective: everything a user can see, do, configure or run, and no
  internals (ids, migrations, code paths, the shared/local/bookkeeping
  classification of settings).
- 2026-10-21 — The completeness the maintainer likes is Sierra Chart's documentation
  ("every option documented, from the user's perspective"); ours should be
  as complete and more concise — "one precise line per option where a line
  suffices, tables over paragraphs for keys/settings/parameters, no
  restating what the UI label already says unless the behaviour isn't
  obvious, explain only the non-obvious (defaults, interactions, edge
  cases)".
- 2026-10-21 — The manual should "freely include ad-hoc screenshots to show
  things", and they must be fresh captures of the current UI; the ones in
  `assets/examples/` are outdated. Default home `site/assets/manual/`.
- 2026-10-21 — Every screenshot in the docs carries an HTML comment beside
  it (`<!-- regenerate: … -->` right above the image) with the theme, window
  size and scale, the `omacharts` commands that build the state (symbols,
  resolution, indicators, drawings, layout), and the capture command and
  crop, so a future agent can regenerate it after the UI changes.
- 2026-10-22 — "I don't like the expanded 'from the command line' sections
  on every manual page." Every command reference lives on one page, The CLI
  (`site/manual/cli.md`), one section per topic; a topic chapter keeps no CLI
  section of its own and ends with a see-also link to its section there, such
  as "Managing chartbooks with the CLI". The content stays; only its place
  changed. A single command inside prose, to make a point, is fine.
- 2026-10-22 — "Use CLI in general instead of command line, it is more
  common": headings, prose and link text say CLI.
- 2026-10-22 — The top-level sections are called parts: Introduction (was
  Guide), then Charting, Configuration ("include installation stuff here"),
  Agents ("the cli and agents here") and Appendices, replacing Reference.
  "Remove troubleshooting completely." A part's page is "just a simple
  vertical list" of its chapters.
- 2026-10-22 — "See also should be a proper heading, bullet list inside with
  the link itself, even if only one."
- 2026-10-22 — The installing chapter is named just "Installation" and sits
  in the Introduction part, at its end.
- 2026-10-22 — Renamed: the bar widget chapter is "Omarchy plugin", in
  Appendices, with a real screenshot of the widget; "Market data feeds".
  Linking and watchlists are one chapter, "Watchlists".
- 2026-10-22 — "I prefer to call it sidebar": the list of watchlists beside
  the charts is the sidebar, as the window's own shortcut sheet says, never
  "the rail" (the code's internal name).
- 2026-10-22 — "The writing style is still too verbose": after the first
  draft of the manual, a pass to make every chapter more succinct. Shorter
  sentences, fewer words per fact, no clause that restates the one before
  it, no reason given where the reader would not ask for one — and nothing
  a user can do left out.
- 2026-10-22 — On "Templates: change one, and everything on it follows /
  Two kinds of thing get repeated across charts, and each has a template so
  that you change it once": "it is not telling you what a template is. The
  redaction and style feels agentic. It does not feel like a human helping
  other human what templates are, without mannerisms." Say what a thing is
  first, in plain words; headings name topics; no epigram style.
- 2026-10-22 — On the quick start's opening ("Ten steps, a minute or two
  each, with your hands on the keyboard. Each one is the shortest way to
  something you will do every day; the table at the end names the chapter
  that has the rest of it."): "wasted text… we don't need those
  introductions to what is coming, I don't like them." No paragraph that
  previews a page or a section, says how it is organised or what comes at
  its end. Start with the content.
- 2026-10-22 — On the quick start's install step, which offered the menu and
  the terminal in one breath: "keep the average user in mind: installing
  things in Omarchy normally is done through the menu, not the terminal.
  Fine to include the terminal but the mainstream path should be the
  recommended one." Lead with what most Omarchy users do (the Omarchy menu,
  the window, a key); give the terminal as the alternative after it.
- 2026-10-22 — "Try to minimise dashes in general, they scream an agent is
  writing too." Em dashes (—) in prose are the exception, not a habit:
  rewrite as two sentences, a comma, or brackets. Tables and code are
  unaffected.
- 2026-10-22 — On "Ten ship with Omacharts. They are computed the way
  TradingView computes them…": no references to TradingView (or other
  products) in the manual, and never state how many indicators (or other
  growing sets) there are: "it is a set that will grow fast". Say "the
  indicators", not "the ten".
- 2026-10-22 — On the caption "Add an indicator, before anything is typed.
  Volume leads because almost every chart wants it.": "this does not make
  sense. Please aim to be dry or drier here. Don't try to make things
  appealing." Captions and prose describe what is there; no justifying or
  selling the app's choices, no colour added to make a sentence engaging.
- 2026-10-22 — On Market data feeds' opening ("A feed is the source of
  prices… There are two. Yahoo Finance is the default… thinkorswim streams…
  Switching is instant, because each feed's bars are cached separately…"):
  "wasteful text, just say what a feed is. There will be many more feeds
  coming at some point. The two feeds now are not the story here." An
  opening says what the thing is and stops; the current members of a set
  that will grow (feeds, indicators, drawing tools) are listed where they
  are documented, not summarised up front, and how it works inside is left
  out.
- 2026-10-22 — On a Drawings table with a `chart set` column
  (`--show-drawings on\|off`): "what is that --show… params here, super
  confusing". A topic chapter describes the window; flags and commands go
  on the CLI page, and the chapter links there in See also. (The `\|` also
  rendered with its backslash: never put a pipe inside code in a table.)
- 2026-10-22 — Again, on "Omacharts is one window with a chart in it. This
  chapter walks through the pieces of the app in the order you meet them,
  and names the chapter that has the detail of each.": "please seriously
  remove all of these." No sentence anywhere that talks about the page
  itself — what it covers, how it is ordered, where the detail is. A page
  starts with its first real fact.
- 2026-10-22 — On "Every instrument Omacharts knows is loaded in memory, so
  finding one is a matter of typing. Each keystroke searches again, with no
  network and nothing to wait for.": "meant for a software engineer. The
  users don't care about that… terrible AI mannerism. What do you want to
  communicate to the user? Just type and find any symbol quickly. Come on!"
  Write what the user does and sees ("Start typing over a chart to find a
  symbol."), never how the app does it (memory, caches, threads, the
  network, databases), and never dress a plain fact up.

- 2026-10-22 — "We are addressing regular human users, not programmers or
  engineers." Recorded as "Who it is for" at the top of this file.
- 2026-10-22 — On "A screenshot is a picture of a chart, or of the whole
  chartbook, without the controls. It has the bars, the scales, the header,
  the indicator legend and the drawings, and none of the buttons.": "Too
  comprehensive, a human would not describe a screenshot with so many words
  to another human." Saying what a thing is takes one short sentence, not an
  inventory of its parts. (Something like "A screenshot is a picture of a
  chart or chartbook, without the buttons.")
- 2026-10-22 — On "A typical use is one arrangement for the open, shared by
  chartbooks for semis, metals and rates.": "remove typical assumptions you
  make about trading, the advice that is producing is quite poor." The
  manual explains the app; it does not suggest how to trade, what a
  "typical" setup is, or what somebody might keep a chartbook for. Examples
  use real symbols only to show what a control does.
- 2026-10-22 — "Dividers are saved as a share of their split, not in
  pixels, so a template fits a window of any size.": "implementation detail
  the user doesn't care about, remove." How something is stored is never
  the user's business, even when it explains a behaviour.
- 2026-10-22 — Twice removed from the Omarchy plugin chapter: "The widget
  lives in ~/.config/omarchy/plugins/…, and its place in the bar in
  ~/.config/omarchy/shell.json. Removing the package hides the widget…":
  "users do not need that level of detail." Where the app keeps its files
  belongs only in Where your data lives, not in the chapter about a feature.
- 2026-10-22 — "Omarchy talks about plugins. Maybe better to use plugin or
  Omarchy plugin here?" Yes: Omarchy's own docs call these plugins. The
  thing in the bar is the **Omarchy plugin** (then "the plugin"), never
  "the bar widget" or "the widget". Setting labels the UI shows keep their
  wording.
- 2026-10-22 — "The cropping of the native menus is wrong sometimes" (the
  chart menu's Template submenu, cut at its bottom edge). A screenshot of a
  menu, popover or dialog shows all of it, edges, corners and last row
  included, with a small even margin around it. Look at every capture
  before using it.
- 2026-10-22 — "Too low-level detail information": on "A name is refused,
  with Save disabled and the box marked, when it is blank, when another
  template already has it, whatever its capitals, or when it is None.
  Saving always makes a new template and never replaces one." Validation
  rules and the edge cases a dialog already handles on screen do not go in
  the manual. Document what the user does, not every case the code checks.
