//! What the command line can do, written down once.
//!
//! The parser, `--help`, the JSON surface, the shell completions and the man
//! page are all built from the table below rather than maintained beside each
//! other. A description of a parser that lives apart from the parser is wrong
//! by the second release, and the whole point of this CLI is that something
//! which cannot ask questions can rely on what it is told.
//!
//! Adding a command means adding a [`Verb`] here and an arm in
//! [`crate::cli::exec`]. Nothing else has to be touched, and nothing else is
//! allowed to describe the surface.

/// A positional argument.
pub struct Arg {
    pub name: &'static str,
    pub help: &'static str,
    pub required: bool,
    /// Takes the rest of the line, like a list of symbols.
    pub many: bool,
    /// The only values accepted, when there is a fixed set. Empty otherwise.
    pub values: &'static [&'static str],
}

impl Arg {
    const fn req(name: &'static str, help: &'static str) -> Arg {
        Arg { name, help, required: true, many: false, values: &[] }
    }
    const fn opt(name: &'static str, help: &'static str) -> Arg {
        Arg { name, help, required: false, many: false, values: &[] }
    }
    const fn many(name: &'static str, help: &'static str) -> Arg {
        Arg { name, help, required: true, many: true, values: &[] }
    }
    const fn of(mut self, values: &'static [&'static str]) -> Arg {
        self.values = values;
        self
    }
}

/// A named option. A `value` of `None` is a switch.
pub struct Flag {
    pub long: &'static str,
    pub value: Option<&'static str>,
    pub help: &'static str,
    pub values: &'static [&'static str],
}

impl Flag {
    const fn switch(long: &'static str, help: &'static str) -> Flag {
        Flag { long, value: None, help, values: &[] }
    }
    const fn valued(long: &'static str, value: &'static str, help: &'static str) -> Flag {
        Flag { long, value: Some(value), help, values: &[] }
    }
    const fn of(mut self, values: &'static [&'static str]) -> Flag {
        self.values = values;
        self
    }
}

pub struct Verb {
    pub name: &'static str,
    pub about: &'static str,
    pub args: &'static [Arg],
    pub flags: &'static [Flag],
    /// One real invocation. Examples are what an agent copies, so every verb
    /// has one and it has to be a command that works as written.
    pub example: &'static str,
    /// Offers `--json`.
    pub json: bool,
    /// Changes something. A window that is open has to be told, which is what
    /// [`crate::cli::Live`] is for — and it is why this flag exists here
    /// rather than being inferred from the verb's name.
    pub writes: bool,
    /// Reads or writes the stored arrangement of charts rather than the
    /// database tables, so a running window has to flush it first.
    pub workspace: bool,
}

pub struct Noun {
    pub name: &'static str,
    pub about: &'static str,
    pub verbs: &'static [Verb],
}

/// How a watchlist, section or chartbook is named on the command line.
pub const SELECTOR: &str =
    "a name, case-insensitive, or `id:N` when two of them share one";

/// How a chart is named on the command line.
///
/// Position, not identity, is the stable way to point at one. Rebuilding an
/// arrangement hands out fresh pane ids in layout order, so an id read before
/// a rebuild names a different chart afterwards — `pos:0` is the first chart
/// in the arrangement whatever the ids happen to be this time round.
pub const CHART_SELECTOR: &str =
    "`pos:N`, counting from 0 in layout order, or a raw id from `chart list` \
     — prefer the position, which survives the window rebuilding";

const STYLES: &[&str] = &["candles", "ohlc"];
const SESSIONS: &[&str] = &["regular", "extended"];
const SPLITS: &[&str] = &["horizontal", "vertical"];
const KINDS: &[&str] = &["volume", "sma", "ema", "vwap", "volume_profile", "rsi", "atr"];
const ANCHORS: &[&str] = &["session", "week", "month", "quarter", "year"];
const LINE_STYLES: &[&str] = &["solid", "dashed", "dotted"];
const SWITCHES: &[&str] = &["on", "off"];
const LINKS: &[&str] =
    &["none", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
const COLOURING: &[&str] = &["coloured", "monochrome"];

/// How a colour is written on the command line.
///
/// Both forms, because the app itself keeps them apart: a named swatch is
/// re-resolved every time the theme changes, and a hex is the one the user
/// picked and is never touched again. Offering only hex would quietly opt
/// every scripted indicator out of following the desktop's theme.
/// Where `skill install` writes, and how to send it somewhere else.
///
/// A flag rather than only an environment variable because a project's own
/// `.claude/skills` is a real place to want it, and because a test has to be
/// able to install the skill without writing into the configuration of
/// whoever is running the tests.
pub const SKILLS_DIR: &str =
    "one skills directory to use instead of any agent's own; \
     each agent is found through its own variable otherwise";

/// The three `skill` verbs take the same flags, so they are written once.
///
/// One per agent, plus the explicit directory. Every agent in
/// `cli::skill::KNOWN` has to appear here or nobody can ask for it by name, and
/// `every_agent_has_a_flag_that_selects_it` is what fails when one does not.
const AGENT_FLAGS: &[Flag] = &[
    Flag::switch("claude", "just Claude, whether or not it looks installed"),
    Flag::switch("codex", "just Codex, whether or not it looks installed"),
    Flag::valued("to", "DIR", SKILLS_DIR),
];

pub const COLOUR: &str =
    "a palette name — Blue, Amber, Violet, Teal, Rose, Green, Orange, Cyan — \
     which follows the theme, or #rrggbb, which does not";

pub const SURFACE: &[Noun] = &[
    Noun {
        name: "status",
        about: "What the app has open right now",
        verbs: &[Verb {
            name: "show",
            about: "The open chartbook, the focused chart, and what else is arranged around it",
            args: &[],
            flags: &[],
            example: "omacharts status show --json",
            json: true,
            writes: false,
            workspace: true,
        }],
    },
    Noun {
        name: "symbol",
        about: "Search the instrument inventory",
        verbs: &[
            Verb {
                name: "search",
                about: "Find instruments matching a query, best first",
                args: &[Arg::req("QUERY", "what to look for: a ticker or part of a name")],
                flags: &[Flag::valued("limit", "N", "how many to show (default 10)")],
                example: "omacharts symbol search semiconductor --limit 5",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "show",
                about: "Everything known about one instrument",
                args: &[
                    Arg::req("SYMBOL", "the canonical ticker, without a venue suffix"),
                    Arg::opt("SUFFIX", "the venue suffix for a listing abroad, such as DE"),
                ],
                flags: &[],
                example: "omacharts symbol show SAP DE",
                json: true,
                writes: false,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "watchlist",
        about: "Watchlists and the symbols in them",
        verbs: &[
            Verb {
                name: "list",
                about: "Every watchlist, in the order the app shows them",
                args: &[],
                flags: &[],
                example: "omacharts watchlist list --json",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "show",
                about: "One watchlist's sections and symbols",
                args: &[Arg::opt("LIST", SELECTOR)],
                flags: &[],
                example: "omacharts watchlist show Semis",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "create",
                about: "Make an empty watchlist",
                args: &[Arg::req("NAME", "what to call it")],
                flags: &[],
                example: "omacharts watchlist create Semis",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "rename",
                about: "Give a watchlist a different name",
                args: &[Arg::req("LIST", SELECTOR), Arg::req("NAME", "the new name")],
                flags: &[],
                example: "omacharts watchlist rename Semis Semiconductors",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "delete",
                about: "Remove a watchlist and everything in it",
                args: &[Arg::req("LIST", SELECTOR)],
                flags: &[],
                example: "omacharts watchlist delete Semis",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "add",
                about: "Put symbols in a watchlist",
                args: &[Arg::req("LIST", SELECTOR), Arg::many("SYMBOL", "one or more tickers")],
                flags: &[
                    Flag::valued("section", "NAME", "put them in this section rather than at the top"),
                    Flag::valued("suffix", "S", "venue suffix, applied to every symbol given"),
                ],
                example: "omacharts watchlist add Semis NVDA AMD AVGO TSM MU",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "remove",
                about: "Take symbols out of a watchlist",
                args: &[Arg::req("LIST", SELECTOR), Arg::many("SYMBOL", "one or more tickers")],
                flags: &[
                    Flag::valued("section", "NAME", "only from this section"),
                    Flag::valued("suffix", "S", "venue suffix, applied to every symbol given"),
                ],
                example: "omacharts watchlist remove Semis MU",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "move",
                about: "Move a symbol into another section, or to another place in its own",
                args: &[
                    Arg::req("LIST", SELECTOR),
                    Arg::req("SYMBOL", "the ticker to move"),
                ],
                flags: &[
                    Flag::valued(
                        "section",
                        "SECTION",
                        "the section to move it into (default: leave it in the one it is in)",
                    ),
                    Flag::valued(
                        "before",
                        "SYMBOL",
                        "land in front of this symbol rather than at the end",
                    ),
                    Flag::valued(
                        "from",
                        "SECTION",
                        "which section to take it out of, when it is in more than one",
                    ),
                    Flag::valued("suffix", "S", "venue suffix of the symbol being moved"),
                ],
                example: "omacharts watchlist move Semis MU --section Energy",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "link",
                about: "Which link group a watchlist drives, if any — the list then follows it",
                args: &[
                    Arg::req("LIST", SELECTOR),
                    Arg::opt("GROUP", "the group it drives, or `none`; omit to read it").of(LINKS),
                ],
                flags: &[],
                example: "omacharts watchlist link Semis 3",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "feed",
                about: "The default watchlist with quotes, as the bar widget reads it",
                args: &[],
                flags: &[Flag::switch("refresh", "fetch quotes that have gone stale first")],
                example: "omacharts watchlist feed --refresh",
                json: true,
                writes: false,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "section",
        about: "The named groups inside a watchlist",
        verbs: &[
            Verb {
                name: "list",
                about: "The sections of one watchlist",
                args: &[Arg::opt("LIST", SELECTOR)],
                flags: &[],
                example: "omacharts section list Default",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "create",
                about: "Add a section to a watchlist",
                args: &[Arg::req("LIST", SELECTOR), Arg::req("NAME", "what to call it")],
                flags: &[],
                example: "omacharts section create Default Energy",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "rename",
                about: "Give a section a different name",
                args: &[
                    Arg::req("LIST", SELECTOR),
                    Arg::req("SECTION", SELECTOR),
                    Arg::req("NAME", "the new name"),
                ],
                example: "omacharts section rename Default Energy Oil",
                flags: &[],
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "delete",
                about: "Remove a section and the symbols in it",
                args: &[Arg::req("LIST", SELECTOR), Arg::req("SECTION", SELECTOR)],
                flags: &[],
                example: "omacharts section delete Default Energy",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "promote",
                about: "Turn a section into a watchlist of its own",
                args: &[Arg::req("LIST", SELECTOR), Arg::req("SECTION", SELECTOR)],
                flags: &[],
                example: "omacharts section promote Default Energy",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "order",
                about: "Put a watchlist's sections in this order",
                args: &[
                    Arg::req("LIST", SELECTOR),
                    Arg::many(
                        "SECTION",
                        "the sections, first to last; any left out keep their order behind them",
                    ),
                ],
                flags: &[],
                example: "omacharts section order Default Metals Energy",
                json: true,
                writes: true,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "chartbook",
        about: "Saved arrangements of charts",
        verbs: &[
            Verb {
                name: "screenshot",
                about: "Save a PNG of the open chartbook: every chart, as laid out",
                args: &[],
                flags: &[
                    Flag::valued(
                        "output",
                        "PATH",
                        "the file to write, or a folder to name it in \
                         (default: the configured folder)",
                    ),
                    Flag::switch("clipboard", "also copy the image, as the window's own key does"),
                ],
                example: "omacharts chartbook screenshot --output /tmp/book.png",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "list",
                about: "Every chartbook, and which one is open",
                args: &[],
                flags: &[],
                example: "omacharts chartbook list --json",
                json: true,
                writes: false,
                workspace: true,
            },
            Verb {
                name: "show",
                about: "One chartbook: its charts, their symbols and its watchlist",
                args: &[Arg::opt("BOOK", SELECTOR)],
                flags: &[],
                example: "omacharts chartbook show Macro",
                json: true,
                writes: false,
                workspace: true,
            },
            Verb {
                name: "create",
                about: "Make a chartbook holding one chart",
                args: &[Arg::req("NAME", "what to call it")],
                flags: &[
                    Flag::valued("watchlist", "LIST", "the watchlist it shows (default: the current one)"),
                    Flag::valued("symbol", "SYMBOL", "what its first chart opens on"),
                    Flag::switch("switch", "make it the open chartbook"),
                ],
                example: "omacharts chartbook create Semis --watchlist Semis --symbol NVDA --switch",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "rename",
                about: "Give a chartbook a different name",
                args: &[Arg::req("BOOK", SELECTOR), Arg::req("NAME", "the new name")],
                flags: &[],
                example: "omacharts chartbook rename Macro Rates",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "delete",
                about: "Remove a chartbook and its charts",
                args: &[Arg::req("BOOK", SELECTOR)],
                flags: &[],
                example: "omacharts chartbook delete Semis",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "switch",
                about: "Open a chartbook",
                args: &[Arg::req("BOOK", SELECTOR)],
                flags: &[],
                example: "omacharts chartbook switch Macro",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "watchlist",
                about: "Choose which watchlist a chartbook shows",
                args: &[Arg::req("BOOK", SELECTOR), Arg::req("LIST", SELECTOR)],
                flags: &[],
                example: "omacharts chartbook watchlist Semis Semis",
                json: true,
                writes: true,
                workspace: true,
            },
        ],
    },
    Noun {
        name: "chart",
        about: "The charts inside a chartbook",
        verbs: &[
            Verb {
                name: "list",
                about: "The charts in a chartbook, with the ids the other verbs take",
                args: &[],
                flags: &[Flag::valued("book", "BOOK", "which chartbook (default: the open one)")],
                example: "omacharts chart list --json",
                json: true,
                writes: false,
                workspace: true,
            },
            Verb {
                name: "split",
                about: "Divide a chart in two, copying what it shows",
                args: &[Arg::req("DIRECTION", "which way to divide it").of(SPLITS)],
                flags: &[
                    Flag::valued("book", "BOOK", "which chartbook (default: the open one)"),
                    Flag::valued("chart", "CHART", CHART_SELECTOR),
                ],
                example: "omacharts chart split horizontal",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "close",
                about: "Remove a chart, giving its space to its neighbour",
                args: &[],
                flags: &[
                    Flag::valued("book", "BOOK", "which chartbook (default: the open one)"),
                    Flag::valued("chart", "CHART", CHART_SELECTOR),
                ],
                example: "omacharts chart close --chart 2",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "focus",
                about: "Make a chart the one the keyboard and the menus act on",
                args: &[Arg::req("CHART", CHART_SELECTOR)],
                flags: &[Flag::valued("book", "BOOK", "which chartbook (default: the open one)")],
                example: "omacharts chart focus 2",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "set",
                about: "Change what a chart shows",
                args: &[],
                flags: &[
                    Flag::valued("book", "BOOK", "which chartbook (default: the open one)"),
                    Flag::valued("chart", "CHART", CHART_SELECTOR),
                    Flag::valued("symbol", "SYMBOL", "the instrument to chart"),
                    Flag::valued("suffix", "S", "its venue suffix, for a listing abroad"),
                    Flag::valued("resolution", "TF", "such as 5m, 1h, 1D, 1W"),
                    Flag::valued("style", "STYLE", "how bars are drawn").of(STYLES),
                    Flag::valued("session", "SESSION", "which hours to include").of(SESSIONS),
                    Flag::valued("link", "GROUP", "the link group it joins and then leads, or `none` to leave one")
                        .of(LINKS),
                    Flag::valued("grid", "BOOL", "draw the grid").of(&["on", "off"]),
                ],
                example: "omacharts chart set --symbol NVDA --resolution 1h --style candles",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "indicator",
                about: "Add, remove, list or reconfigure a chart's indicators",
                args: &[
                    Arg::req("ACTION", "what to do").of(&["list", "add", "remove", "set"]),
                    Arg::opt("KIND", "the indicator, for add, remove and set").of(KINDS),
                ],
                flags: &[
                    Flag::valued("book", "BOOK", "which chartbook (default: the open one)"),
                    Flag::valued("chart", "CHART", CHART_SELECTOR),
                    Flag::valued("id", "N", "which one, when a chart has two of a kind"),
                    Flag::valued("period", "N", "bars averaged: SMA, EMA, RSI, ATR"),
                    Flag::valued("anchor", "WHEN", "what VWAP and the volume profile reset on")
                        .of(ANCHORS),
                    Flag::valued("rows", "N", "volume profile rows, or `auto` to follow the instrument's own increment"),
                    Flag::valued("value-area", "F", "the share of volume the value area covers, 0-1"),
                    Flag::valued("poc-color", "COLOUR", "the volume profile's point of control"),
                    Flag::valued("color", "COLOUR", "the line, or the volume profile's background"),
                    Flag::valued("width", "F", "line thickness; 0 draws no line at all"),
                    Flag::valued("style", "STYLE", "how the line is drawn").of(LINE_STYLES),
                    Flag::valued("height", "F", "share of the chart a pane takes, 0.05-0.95: volume, RSI, ATR"),
                    Flag::valued("overbought", "F", "the RSI level drawn across the top"),
                    Flag::valued("oversold", "F", "the RSI level drawn across the bottom"),
                    Flag::valued("bands", "LIST", "which VWAP bands are drawn: 1,2,3 or none"),
                    Flag::valued("band-alpha", "F", "how solid the VWAP shading is, 0.02-0.6"),
                    Flag::valued("visible", "BOOL", "draw it at all").of(SWITCHES),
                ],
                example: "omacharts chart indicator add sma --period 200 --color Amber --style dashed",
                json: true,
                writes: true,
                workspace: true,
            },
            Verb {
                name: "screenshot",
                about: "Save a PNG of the focused chart, as it looks on screen",
                args: &[],
                flags: &[
                    Flag::valued(
                        "output",
                        "PATH",
                        "the file to write, or a folder to name it in \
                         (default: the configured folder)",
                    ),
                    Flag::switch("clipboard", "also copy the image, as the window's own key does"),
                ],
                example: "omacharts chart screenshot --output /tmp/chart.png",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "crosshair",
                about: "Whether a pointer on one chart draws a line on the charts linked to it",
                args: &[Arg::opt("STATE", "omit to read it").of(SWITCHES)],
                flags: &[],
                example: "omacharts chart crosshair off",
                json: true,
                writes: true,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "config",
        about: "Stored preferences",
        verbs: &[
            Verb {
                name: "list",
                about: "Every setting that has been written, and its value",
                args: &[],
                flags: &[],
                example: "omacharts config list --json",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "get",
                about: "One setting's value",
                args: &[Arg::req("KEY", "the setting's name, from `config list`")],
                flags: &[],
                example: "omacharts config get theme",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "set",
                about: "Write a setting",
                args: &[Arg::req("KEY", "the setting's name"), Arg::req("VALUE", "what to set it to")],
                flags: &[],
                example: "omacharts config set theme omarchy",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "bars",
                about: "Whether bars carry their direction in colour, or none at all",
                args: &[Arg::opt("STATE", "omit to read it").of(COLOURING)],
                flags: &[],
                example: "omacharts config bars monochrome",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "refresh",
                about: "Whether charts left open fetch new bars for themselves",
                args: &[Arg::opt("STATE", "omit to read it").of(SWITCHES)],
                flags: &[],
                example: "omacharts config refresh off",
                json: true,
                writes: true,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "skill",
        about: "The agent skill that teaches an agent to drive this app",
        verbs: &[
            Verb {
                name: "status",
                about: "Whether the skill is installed, per agent, and the paths either end of it",
                args: &[],
                flags: AGENT_FLAGS,
                example: "omacharts skill status",
                json: true,
                // Nothing in the database and nothing on screen: these three
                // reach into an agent's configuration, which no window has to
                // hear about. `writes` would only make one rebuild for nothing.
                writes: false,
                workspace: false,
            },
            Verb {
                name: "install",
                about: "Link the skill in for whichever agents you have. Only ever when asked",
                args: &[],
                flags: AGENT_FLAGS,
                example: "omacharts skill install",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "uninstall",
                about: "Take the skill back out again, leaving nothing behind",
                args: &[],
                flags: AGENT_FLAGS,
                example: "omacharts skill uninstall",
                json: true,
                writes: false,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "plugin",
        about: "The Omacharts widget in the Omarchy bar",
        verbs: &[
            Verb {
                name: "status",
                about: "Whether the widget is in the bar, and the folder it lives in",
                args: &[],
                flags: &[],
                example: "omacharts plugin status",
                json: true,
                // Like the skill verbs: this reaches into ~/.config/omarchy,
                // which is neither the database nor the arrangement, so a
                // window that is open has nothing to catch up on.
                writes: false,
                workspace: false,
            },
            Verb {
                name: "install",
                about: "Write the widget and add it to the bar, or bring an installed one up to date",
                args: &[],
                flags: &[],
                example: "omacharts plugin install",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "uninstall",
                about: "Take the widget out of the bar and remove its folder",
                args: &[],
                flags: &[],
                example: "omacharts plugin uninstall",
                json: true,
                writes: false,
                workspace: false,
            },
        ],
    },
    Noun {
        name: "cache",
        about: "The cached market data",
        verbs: &[
            Verb {
                name: "status",
                about: "How much the cache holds, against its limit",
                args: &[],
                flags: &[],
                example: "omacharts cache status --json",
                json: true,
                writes: false,
                workspace: false,
            },
            Verb {
                name: "clear",
                about: "Throw away every cached series. Settings and watchlists stay",
                args: &[],
                flags: &[],
                example: "omacharts cache clear",
                json: true,
                writes: true,
                workspace: false,
            },
            Verb {
                name: "limit",
                about: "Read or set the size the cache is pruned back under",
                args: &[Arg::opt("SIZE", "such as 512MB or 2GB; omit to read it")],
                flags: &[],
                example: "omacharts cache limit 2GB",
                json: true,
                writes: true,
                workspace: false,
            },
        ],
    },
];

/// The values this table offers have to be the values the engine accepts.
///
/// Written out rather than built from the engine because a `const` cannot
/// call anything — so this is the test that keeps the two honest. Offering a
/// style that does not exist would tell an agent to try something that can
/// only ever fail.
/// Find a verb by the pair of names the parser matched.
pub fn verb(noun: &str, verb: &str) -> Option<&'static Verb> {
    SURFACE
        .iter()
        .find(|n| n.name == noun)?
        .verbs
        .iter()
        .find(|v| v.name == verb)
}

/// What a chart and a chartbook are stored with, and what reaches each one.
///
/// A field, the command that sets it, and the flag that carries it — or, where
/// the second is empty, why the field needs no command. The window's own
/// structs are private to it, so this is the one place the two lists are put
/// beside each other; see
/// `every_field_a_chart_is_stored_with_is_reachable_from_a_command`.
#[cfg(test)]
const STORED_FIELDS: &[(&str, &str, &str)] = &[
    ("id", "", "identity rather than state: a chart is named back as pos:N"),
    ("symbol", "chart set", "--symbol"),
    ("suffix", "chart set", "--suffix"),
    ("timeframe", "chart set", "--resolution"),
    ("indicators", "chart indicator", ""),
    ("bar_style", "chart set", "--style"),
    ("session", "chart set", "--session"),
    ("show_grid", "chart set", "--grid"),
    ("linked", "chart set", "--link"),
    ("name", "chartbook rename", ""),
    ("layout", "chart split", ""),
    ("focused", "chart focus", ""),
    ("panes", "chart list", ""),
    ("watchlist", "chartbook watchlist", ""),
    ("sidebar_shown", "", "presentational: the rail is shown with a mouse or a key"),
    ("sidebar_width", "", "presentational: a width is dragged, never scripted"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use omacharts_engine::indicators::{LineStyle, MAX_PANE_SHARE, MIN_PANE_SHARE};
    use omacharts_engine::{link, BarStyle, IndicatorKind, Reset, Session, Timeframe};

    #[test]
    fn the_bar_styles_on_offer_are_the_ones_that_exist() {
        let engine: Vec<&str> = BarStyle::ALL.iter().map(|s| s.key()).collect();
        assert_eq!(STYLES, engine.as_slice());
    }

    #[test]
    fn the_sessions_on_offer_are_the_ones_that_exist() {
        let mut engine: Vec<&str> = Session::ALL.iter().map(|s| s.key()).collect();
        let mut offered = SESSIONS.to_vec();
        engine.sort_unstable();
        offered.sort_unstable();
        assert_eq!(offered, engine);
    }

    #[test]
    fn the_indicators_on_offer_are_the_ones_that_exist() {
        let engine: Vec<&str> = IndicatorKind::ALL.iter().map(|k| k.key()).collect();
        let mut offered = KINDS.to_vec();
        let mut engine = engine;
        offered.sort_unstable();
        engine.sort_unstable();
        assert_eq!(offered, engine);
    }

    #[test]
    fn the_anchors_on_offer_are_the_ones_that_exist() {
        let mut engine: Vec<&str> = Reset::ALL.iter().map(|r| r.key()).collect();
        let mut offered = ANCHORS.to_vec();
        offered.sort_unstable();
        engine.sort_unstable();
        assert_eq!(offered, engine);
    }

    /// `LineStyle` is spelled by serde rather than by a `key` method, so this
    /// compares against what it actually serialises to.
    #[test]
    fn the_line_styles_on_offer_are_the_ones_that_exist() {
        let engine: Vec<String> = LineStyle::ALL
            .iter()
            .map(|s| serde_json::to_string(s).unwrap().trim_matches('"').to_string())
            .collect();
        assert_eq!(LINE_STYLES.to_vec(), engine);
    }

    /// The help names the palette, and a name it offers that the theme does
    /// not carry resolves to the accent — which looks like the command was
    /// ignored rather than wrong.
    #[test]
    fn the_colours_the_help_names_are_the_ones_every_theme_carries() {
        for name in omacharts_engine::theme::SWATCH_NAMES {
            assert!(COLOUR.contains(name), "the help does not mention {name}");
        }
    }

    #[test]
    fn the_link_groups_on_offer_are_the_ones_that_exist() {
        let engine: Vec<String> = link::ALL
            .iter()
            .map(|group| match group.number() {
                None => "none".to_string(),
                Some(n) => n.to_string(),
            })
            .collect();
        assert_eq!(LINKS.to_vec(), engine);
    }

    /// A resolution is typed rather than chosen from a list — "3m" is a
    /// reasonable thing to want — so there is no set of variants to compare
    /// against here, and this checks the narrower thing that is actually true:
    /// every resolution the header strip offers is one a command accepts.
    /// Saying that plainly matters more than the test looking as strong as
    /// the ones above it.
    #[test]
    fn every_resolution_the_header_strip_offers_is_one_a_command_accepts() {
        for preset in Timeframe::PRESETS {
            assert_eq!(Timeframe::parse(&preset.key()), Some(preset), "{}", preset.key());
        }
    }

    /// Everything a chart is stored with has a command that sets it.
    ///
    /// This is the test that makes "every feature ships with its command" a
    /// thing that can fail rather than a thing people mean to do. A new piece
    /// of chart or chartbook state is a new field on one of the window's two
    /// stored structs, and a field nobody can set from a terminal fails here
    /// until either a command or a written reason exists for it.
    ///
    /// Read out of the source because both structs are private to the window.
    /// That is the limit of what this can prove: it catches a field arriving
    /// with no command, and it cannot catch a capability that changes nothing
    /// stored — an action on screen that only moves what is already there.
    #[test]
    fn every_field_a_chart_is_stored_with_is_reachable_from_a_command() {
        const WINDOW: &str = include_str!("../ui/window.rs");
        for shape in ["StoredPane", "Chartbook"] {
            let fields = fields_of(WINDOW, shape);
            assert!(!fields.is_empty(), "no {shape} to read in window.rs");
            for field in fields {
                let found = STORED_FIELDS.iter().find(|(name, ..)| *name == field);
                let Some((_, command, note)) = found else {
                    panic!(
                        "{shape}.{field} is stored with nothing in STORED_FIELDS for it: \
                         name the command that sets it, or why it needs none"
                    );
                };
                if command.is_empty() {
                    assert!(!note.is_empty(), "{shape}.{field} has no command and no reason");
                    continue;
                }
                let (noun, name) = command.split_once(' ').expect("a noun and a verb");
                let found = verb(noun, name)
                    .unwrap_or_else(|| panic!("{shape}.{field} names {command}, which is not a command"));
                if let Some(flag) = note.strip_prefix("--") {
                    assert!(
                        found.flags.iter().any(|f| f.long == flag),
                        "{shape}.{field} names `{command} {note}`, and that verb has no {note}"
                    );
                }
            }
        }
    }

    /// The field names of one struct, read out of Rust source.
    ///
    /// Enough of a parser for two plain structs of named fields and no more:
    /// anything cleverer would be a second thing to maintain, and these two
    /// are the only ones read this way.
    #[cfg(test)]
    fn fields_of(source: &str, shape: &str) -> Vec<String> {
        let start = source
            .find(&format!("struct {shape} {{"))
            .unwrap_or_else(|| panic!("no struct {shape}"));
        let body = &source[start..];
        let end = body.find("\n}").unwrap_or(body.len());
        body[..end]
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| !line.starts_with('#') && !line.starts_with("//"))
            .filter_map(|line| line.split_once(':'))
            .map(|(name, _)| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// The help names a range, and a number outside it is refused rather than
    /// quietly brought back — so the range named has to be the one the engine
    /// clamps to. It moved from 0.6 to 0.95 the day a pane became draggable,
    /// which is exactly the drift this notices.
    #[test]
    fn the_pane_height_the_help_names_is_the_range_the_engine_clamps_to() {
        let named = verb("chart", "indicator")
            .expect("a chart indicator verb")
            .flags
            .iter()
            .find(|flag| flag.long == "height")
            .expect("a height flag");
        let range = format!("{MIN_PANE_SHARE}-{MAX_PANE_SHARE}");
        assert!(named.help.contains(&range), "the help says {:?}, not {range}", named.help);
    }

    #[test]
    fn no_two_commands_share_a_name() {
        for noun in SURFACE {
            let mut seen: Vec<&str> = noun.verbs.iter().map(|v| v.name).collect();
            let before = seen.len();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), before, "{} has a duplicate verb", noun.name);
        }
    }
}
