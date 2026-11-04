//! Schema migrations, and the one number that says which have run.
//!
//! The database is one file on one machine, with no server to coordinate an
//! upgrade and nobody to run a command by hand: whatever shape it is in when
//! the app opens it, the app has to carry it forward on its own. So every
//! change to the schema is a numbered step here, applied in order, and
//! `PRAGMA user_version` records how far a given file has got.
//!
//! `user_version` rather than a row in a table, because it lives in the
//! database header: nothing that empties a table can lose it, it costs no
//! query to read, and it is written inside the same transaction as the change
//! it describes.
//!
//! Three rules hold this together, and all three matter more than they look:
//!
//! 1. **A version that has shipped is frozen.** Never renumber, never edit the
//!    SQL of a step somebody's database has already run — their file will not
//!    be visited again, so an edit changes what new installs get and nothing
//!    else, and the two drift apart silently.
//! 2. **One transaction per step, bumping the version inside it.** A machine
//!    that loses power mid-upgrade comes back on the old version with the old
//!    shape, which is a state the next launch knows how to fix. Half-applied
//!    is the one state nothing can fix.
//! 3. **Steps talk to the schema, not to the app.** A step that calls
//!    `Store::set_indicators` breaks on the day that method changes shape,
//!    years after the step was written and long after anyone remembers why it
//!    exists. The SQL of the day is frozen in amber here on purpose.
//!
//! There is one step, and it is the schema. Rule 1 starts applying the day
//! Omacharts is released; until then the list is free to be rewritten, and a
//! single authoritative `CREATE` is a far better thing to inherit than a chain
//! of upgrades describing shapes that only ever existed on one developer's
//! laptop.

use std::path::Path;

use rusqlite::{params, Connection};

/// How far the migrations go. A database claiming more than this was written
/// by a newer Omacharts than the one opening it.
pub const LATEST: i32 = 10;

/// How far the market data file's own migrations go.
pub const MARKET_LATEST: i32 = 2;

/// The schema the market data file is attached as.
pub const MARKET: &str = "market";

/// The settings version that moved the market data out.
pub const MARKET_MOVED_OUT: i32 = 4;

/// What went wrong before the database was usable.
#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    /// Opened by an older build than the one that last wrote it.
    ///
    /// Worth its own variant rather than a generic failure, because the right
    /// response is the opposite of the usual one: a database we cannot read is
    /// a reason to carry on with an empty one, and a database from the future
    /// is a reason to stop and say so. It is intact, it is the user's, and a
    /// build that quietly started them a fresh watchlist beside it would look
    /// exactly like having lost everything.
    FromTheFuture { found: i32, known: i32 },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sqlite(error) => write!(f, "{error}"),
            Error::FromTheFuture { found, known } => write!(
                f,
                "this database was written by a newer Omacharts \
                 (its schema is version {found}, this build knows {known}). \
                 Upgrade Omacharts, or move the file aside to start fresh."
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Error {
        Error::Sqlite(error)
    }
}

/// What a step does, which is not always expressible in SQL.
pub enum Step {
    Sql(&'static str),
    /// Reads data out, decides something about it, writes it back. Takes the
    /// open transaction so its work commits or rolls back with the version.
    Rust(fn(&Connection) -> rusqlite::Result<()>),
}

pub struct Migration {
    pub version: i32,
    /// For the error message when this is the step that failed. A number alone
    /// sends whoever is reading the log back to the source to find out what
    /// was being attempted.
    pub name: &'static str,
    /// Whether to snapshot the file before running this.
    ///
    /// Adding a table or a column cannot lose anything, so the overwhelming
    /// majority of steps say no. Anything that rewrites or drops says yes: the
    /// watchlists in here are built by hand over months and exist nowhere
    /// else.
    pub risky: bool,
    pub step: Step,
}

/// Everything that has ever been true of this schema.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "the schema",
    risky: false,
    // Plain CREATE, not CREATE IF NOT EXISTS. There is no older shape in the
    // world to adopt, so a create that silently does nothing would only be
    // able to hide a bug: run twice, this fails loudly, and the version is
    // what guarantees it is not. Every table the app has is described here
    // once, in one place, rather than assembled from a history of ALTERs
    // nobody can read the result of.
    step: Step::Sql(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE bar_series (
             key        TEXT NOT NULL,
             interval   TEXT NOT NULL,
             first_ts   INTEGER NOT NULL,
             last_ts    INTEGER NOT NULL,
             count      INTEGER NOT NULL,
             fetched_at INTEGER NOT NULL,
             ts         BLOB NOT NULL,
             open       BLOB NOT NULL,
             high       BLOB NOT NULL,
             low        BLOB NOT NULL,
             close      BLOB NOT NULL,
             volume     BLOB NOT NULL,
             PRIMARY KEY (key, interval)
         );
         CREATE TABLE custom_themes (id TEXT PRIMARY KEY, json TEXT NOT NULL);
         CREATE TABLE custom_bar_schemes (id TEXT PRIMARY KEY, json TEXT NOT NULL);
         CREATE TABLE watchlists (
             id       INTEGER PRIMARY KEY AUTOINCREMENT,
             name     TEXT NOT NULL,
             position INTEGER NOT NULL
         );
         CREATE TABLE watchlist_sections (
             id        INTEGER PRIMARY KEY AUTOINCREMENT,
             name      TEXT NOT NULL,
             position  INTEGER NOT NULL,
             collapsed INTEGER NOT NULL DEFAULT 0,
             -- The default is DEFAULT_WATCHLIST, which SQLite wants spelled
             -- out. `the_default_watchlist_is_the_one_a_section_falls_into`
             -- is what keeps the literal and the constant honest.
             watchlist_id INTEGER NOT NULL DEFAULT 1
         );
         CREATE TABLE watchlist_entries (
             section_id INTEGER NOT NULL
                 REFERENCES watchlist_sections(id) ON DELETE CASCADE,
             symbol     TEXT NOT NULL,
             suffix     TEXT NOT NULL DEFAULT '',
             position   INTEGER NOT NULL,
             PRIMARY KEY (section_id, symbol, suffix)
         );
         -- The watchlist every install has and nobody can delete, and the
         -- section inside it holding whatever is in no section. Foreign keys
         -- are enforced here, so that section needs a real row for entries to
         -- hang off; it is filtered out of the named ones, and callers only
         -- ever see it as the nameless first.
         INSERT INTO watchlists (id, name, position) VALUES (1, 'Default', 0);
         INSERT INTO watchlist_sections (id, name, position, watchlist_id)
             VALUES (0, '', -1, 1);",
    ),
},
Migration {
    version: 2,
    name: "drawings",
    risky: false,
    // What a person draws on a chart belongs to the symbol, not to the pane
    // it was drawn in: a trend line on AAPL means nothing on NVDA and
    // everything on AAPL in another chartbook, at another resolution. So
    // drawings are keyed the way watchlist entries are, by symbol and
    // suffix, and the rest is JSON, like a theme — the shape of a drawing
    // will grow, and a column per field would mean a migration per idea.
    step: Step::Sql(
        "CREATE TABLE drawings (
             id     INTEGER PRIMARY KEY AUTOINCREMENT,
             symbol TEXT NOT NULL,
             suffix TEXT NOT NULL DEFAULT '',
             json   TEXT NOT NULL
         );
         CREATE INDEX drawings_by_symbol ON drawings (symbol, suffix);",
    ),
},
Migration {
    version: 3,
    name: "quotes",
    risky: false,
    // The latest two closes of a symbol, fetched many to a request for the bar
    // widget. Beside `bar_series` rather than in it: a close alone is not a
    // bar, and one written as a bar would be compared against the real
    // session later and read as history rewritten. Keyed like `bar_series`.
    // `last` is null for a symbol the provider left out of its answer, so
    // that `fetched_at` still records that it was asked. `previous_at` is how
    // a batch that skipped a session is told from one that did not: its
    // previous close is then older than a session the bars hold.
    step: Step::Sql(
        "CREATE TABLE quotes (
             key         TEXT PRIMARY KEY,
             last        REAL,
             previous    REAL,
             at          INTEGER,
             fetched_at  INTEGER NOT NULL,
             previous_at INTEGER
         );",
    ),
},
Migration {
    version: 4,
    name: "market data moves to its own file",
    // Copied and then dropped: what is cached may be history the provider
    // will not hand back — a week is all Yahoo keeps of minute bars — so the
    // file is snapshotted first like any step that drops something.
    risky: true,
    step: Step::Rust(move_market_data),
},
Migration {
    version: 5,
    name: "data feeds become an ordered list",
    // One feed for every chart becomes a list of them, top first, with Yahoo
    // implied at the bottom and never stored. So Yahoo, or nothing, is an
    // empty list and nothing is written; any other feed is a list of one.
    // Risky, because it drops the old row, though what it held is carried
    // into the new one in the same transaction.
    risky: true,
    step: Step::Rust(feeds_become_a_list),
},
Migration {
    version: 6,
    name: "the header gains its account",
    // Which source a chart is from was a note after the header's parts,
    // whatever they were; now it is a part, and a header somebody chose gets
    // it where the note sat. A header nobody chose is not stored, and the
    // standard one has it already. Nothing is lost, so nothing to snapshot.
    risky: false,
    step: Step::Sql(
        "UPDATE settings
             SET value = CASE
                 WHEN lower(trim(value)) IN ('none', '') THEN 'account'
                 ELSE value || ',account'
             END
         WHERE key = 'chart_header'
           AND ',' || lower(replace(value, ' ', '')) || ',' NOT LIKE '%,account,%';",
    ),
},
Migration {
    version: 7,
    name: "the Rithmic login becomes the first of a list",
    // There can be any number of broker logins now, each with an id its
    // password is kept under. The one login there was becomes the first,
    // with the id "1", named after its system, and the data login. Its
    // password stays where it is in the keyring: a login's password is
    // looked for under its id and then, as it was saved before, under its
    // user and system. A login that was signed out had no user ID left, and
    // there is nothing to carry. Risky, because it drops the old rows.
    risky: true,
    step: Step::Rust(login_becomes_a_list),
},
Migration {
    version: 8,
    name: "Yahoo becomes a feed in the order like the others",
    // Yahoo was implied at the bottom of every stored order; now it is in
    // the order where it is, and can be taken out. An order that does not
    // name it gets it at the bottom, where it was, so nothing is routed
    // differently. Nothing stored stays nothing, which is Yahoo alone.
    risky: false,
    step: Step::Sql(
        "UPDATE settings
             SET value = json_insert(value, '$[#]', 'yahoo')
         WHERE key = 'providers'
           AND json_valid(value)
           AND json_type(value) = 'array'
           AND NOT EXISTS (
               SELECT 1 FROM json_each(settings.value) AS feed
               WHERE lower(trim(feed.value)) = 'yahoo'
           );",
    ),
},
Migration {
    version: 9,
    name: "a login names its broker",
    // Logins were Rithmic's, kept under Rithmic's name with its system and
    // gateway as fields of their own. Now any broker's are kept in one
    // list: each names its broker, and what its dialog asked beyond the
    // user ID is a list of named values. Ids stay, so each password is
    // found in the keyring where it always was, and so do the accounts and
    // the default account, which name logins by id. Risky, because it drops
    // the old rows.
    risky: true,
    step: Step::Rust(logins_name_their_broker),
},
Migration {
    version: 10,
    name: "the account part splits into the account and the data feed",
    // The header's account part named the account and the feed both. Now
    // they are two parts, and a header that showed the one shows both,
    // the feed right after the account, so nothing a chart said goes
    // unsaid. A header with no account part gets neither: it chose to
    // say nothing, and the standard header already shows both.
    risky: false,
    step: Step::Sql(
        "UPDATE settings
             SET value = replace(value, 'account', 'account,feed')
         WHERE key IN ('chart_header', 'chart_header_center')
           AND ',' || replace(value, ' ', '') || ',' LIKE '%,account,%'
           AND ',' || replace(value, ' ', '') || ',' NOT LIKE '%,feed,%';",
    ),
}];

/// Everything that has ever been true of the market data file.
///
/// A file of its own so that everything somebody made — settings, watchlists,
/// drawings, themes — is one small file to copy, and everything fetched is
/// another that can be thrown away. The same two tables the settings file
/// held, in the same shape, so moving them is a copy.
pub const MARKET_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "bars and quotes",
    risky: false,
    step: Step::Sql(
        "CREATE TABLE market.bar_series (
             key        TEXT NOT NULL,
             interval   TEXT NOT NULL,
             first_ts   INTEGER NOT NULL,
             last_ts    INTEGER NOT NULL,
             count      INTEGER NOT NULL,
             fetched_at INTEGER NOT NULL,
             ts         BLOB NOT NULL,
             open       BLOB NOT NULL,
             high       BLOB NOT NULL,
             low        BLOB NOT NULL,
             close      BLOB NOT NULL,
             volume     BLOB NOT NULL,
             PRIMARY KEY (key, interval)
         );
         CREATE TABLE market.quotes (
             key         TEXT PRIMARY KEY,
             last        REAL,
             previous    REAL,
             at          INTEGER,
             fetched_at  INTEGER NOT NULL,
             previous_at INTEGER
         );",
    ),
},
Migration {
    version: 2,
    name: "minute ladders",
    risky: false,
    // Volume at price, one row per contract per trading day: the minutes as
    // one blob and their sum as another, so a range reads day totals and
    // never sums a day again. A minute sealed live goes into `ladder_tail`
    // on its own and is folded into its day later; `ladder_coverage` is
    // which minutes are known, so a gap is told from a quiet market. The
    // layouts, and why, are in `crate::store`'s ladder section.
    step: Step::Sql(
        "CREATE TABLE market.ladder_days (
             key        TEXT NOT NULL,
             day        INTEGER NOT NULL,
             tick_size  REAL NOT NULL,
             minutes    INTEGER NOT NULL,
             first_ts   INTEGER NOT NULL,
             last_ts    INTEGER NOT NULL,
             fetched_at INTEGER NOT NULL,
             data       BLOB NOT NULL,
             total      BLOB NOT NULL,
             PRIMARY KEY (key, day)
         );
         CREATE TABLE market.ladder_tail (
             key       TEXT NOT NULL,
             ts        INTEGER NOT NULL,
             day       INTEGER NOT NULL,
             tick_size REAL NOT NULL,
             data      BLOB NOT NULL,
             PRIMARY KEY (key, ts)
         );
         CREATE TABLE market.ladder_coverage (
             key     TEXT NOT NULL,
             from_ts INTEGER NOT NULL,
             to_ts   INTEGER NOT NULL,
             PRIMARY KEY (key, from_ts)
         );",
    ),
}];

/// Version 4: the bars and quotes over to the market data file, then out of
/// this one.
///
/// Two files commit separately. If the market file committed and the
/// settings file did not, the next launch copies again and keeps what is
/// already there. If it went the other way — the settings file moved on and
/// the market file never received the rows — the `.bak` is what still holds
/// them, which is why this step stays risky.
fn move_market_data(tx: &Connection) -> rusqlite::Result<()> {
    tx.execute_batch(
        "INSERT OR IGNORE INTO market.bar_series
             (key, interval, first_ts, last_ts, count, fetched_at,
              ts, open, high, low, close, volume)
         SELECT key, interval, first_ts, last_ts, count, fetched_at,
                ts, open, high, low, close, volume
         FROM main.bar_series;
         INSERT OR IGNORE INTO market.quotes
             (key, last, previous, at, fetched_at, previous_at)
         SELECT key, last, previous, at, fetched_at, previous_at
         FROM main.quotes;
         DROP TABLE main.bar_series;
         DROP TABLE main.quotes;",
    )
}

/// Version 5: `provider=X` becomes `providers=["X"]`, and `provider=yahoo`
/// becomes nothing at all.
///
/// The id is written as the JSON array the app reads, quoted by SQLite's own
/// `json_array` so that a hand-typed value with a quote in it cannot make
/// the row unreadable. Case and spacing are left as they were typed: the app
/// matches an id however it was written, as it always has.
fn feeds_become_a_list(tx: &Connection) -> rusqlite::Result<()> {
    tx.execute_batch(
        "INSERT OR REPLACE INTO settings (key, value)
             SELECT 'providers', json_array(trim(value)) FROM settings
             WHERE key = 'provider' AND lower(trim(value)) NOT IN ('yahoo', '');
         DELETE FROM settings WHERE key = 'provider';",
    )
}

/// Version 7: `rithmic_user`, `rithmic_system` and `rithmic_gateway`
/// become one login in `rithmic_logins`, which is the data login.
fn login_becomes_a_list(tx: &Connection) -> rusqlite::Result<()> {
    tx.execute_batch(
        "INSERT OR REPLACE INTO settings (key, value)
             SELECT 'rithmic_logins', json_array(json_object(
                 'id', '1',
                 'name', coalesce(nullif(trim(s.value), ''), 'Rithmic Test'),
                 'user', trim(u.value),
                 'system', coalesce(nullif(trim(s.value), ''), 'Rithmic Test'),
                 'gateway', coalesce(nullif(trim(g.value), ''), 'wss://rituz00100.rithmic.com:443')))
             FROM settings u
             LEFT JOIN settings s ON s.key = 'rithmic_system'
             LEFT JOIN settings g ON g.key = 'rithmic_gateway'
             WHERE u.key = 'rithmic_user' AND trim(u.value) != '';
         INSERT OR REPLACE INTO settings (key, value)
             SELECT 'rithmic_data_login', '1' FROM settings WHERE key = 'rithmic_logins';
         DELETE FROM settings WHERE key IN ('rithmic_user', 'rithmic_system', 'rithmic_gateway');",
    )
}

/// Version 9: `rithmic_logins` becomes `logins`, each naming the broker
/// `rithmic` and keeping its system and gateway as values; and
/// `rithmic_data_login` becomes `data_login`. A list that cannot be read is
/// carried as it is, so that nothing is lost that a person could still
/// recover by hand.
fn logins_name_their_broker(tx: &Connection) -> rusqlite::Result<()> {
    let read = |key: &str| -> rusqlite::Result<Option<String>> {
        match tx.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)) {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    };
    let write = |key: &str, value: &str| {
        tx.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![key, value])
    };
    if let Some(stored) = read("rithmic_logins")? {
        let carried = match serde_json::from_str::<Vec<serde_json::Value>>(&stored) {
            Ok(logins) => serde_json::Value::Array(logins.into_iter().map(names_its_broker).collect()).to_string(),
            Err(_) => stored,
        };
        write("logins", &carried)?;
    }
    if let Some(data) = read("rithmic_data_login")? {
        write("data_login", &data)?;
    }
    tx.execute("DELETE FROM settings WHERE key IN ('rithmic_logins', 'rithmic_data_login')", [])?;
    Ok(())
}

/// One Rithmic login as version 9 keeps it.
fn names_its_broker(login: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut login) = login else { return login };
    let mut values = Vec::new();
    for key in ["system", "gateway"] {
        if let Some(serde_json::Value::String(value)) = login.remove(key) {
            values.push(serde_json::json!([key, value]));
        }
    }
    login.insert("broker".into(), "rithmic".into());
    if !values.is_empty() {
        login.insert("values".into(), serde_json::Value::Array(values));
    }
    serde_json::Value::Object(login)
}

/// Bring a database up to [`LATEST`], or say why it cannot be.
///
/// `snapshot_to` is where to leave a copy before anything destructive runs;
/// `None` for a database with no file behind it, which is every test and the
/// in-memory fallback.
///
/// The market data file goes first, attached as [`MARKET`]: the settings
/// file's last step moves what it held into it, so it has to exist.
///
/// Returns the version the settings file was at before, so a caller can tell
/// which steps just ran.
pub fn run(conn: &Connection, snapshot_to: Option<&Path>) -> Result<i32, Error> {
    apply(conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None)?;
    apply(conn, "main", MIGRATIONS, LATEST, snapshot_to)
}

/// What [`run`] does, over any list — so the awkward cases can be tested
/// without inventing a schema change to do it with.
fn apply(
    conn: &Connection,
    schema: &str,
    migrations: &[Migration],
    latest: i32,
    snapshot_to: Option<&Path>,
) -> Result<i32, Error> {
    let from = version_of(conn, schema)?;
    if from > latest {
        return Err(Error::FromTheFuture { found: from, known: latest });
    }

    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > from).collect();
    if pending.is_empty() {
        return Ok(from);
    }
    // A file at version 0 is a new one, with nothing in it to lose.
    if let Some(path) = snapshot_to.filter(|_| from > 0 && pending.iter().any(|m| m.risky)) {
        snapshot(conn, path)?;
    }

    for migration in pending {
        // One transaction, holding both the change and the record of it. The
        // version moving is what makes the change final; if the step fails,
        // neither happened.
        let tx = conn.unchecked_transaction()?;
        match &migration.step {
            Step::Sql(sql) => tx.execute_batch(sql)?,
            Step::Rust(run) => run(&tx)?,
        }
        tx.pragma_update(Some(schema), "user_version", migration.version)?;
        tx.commit()?;
    }
    Ok(from)
}

pub fn version(conn: &Connection) -> rusqlite::Result<i32> {
    version_of(conn, "main")
}

fn version_of(conn: &Connection, schema: &str) -> rusqlite::Result<i32> {
    conn.pragma_query_value(Some(schema), "user_version", |row| row.get(0))
}

/// Put a consistent copy of the database beside it, to go back to.
///
/// `VACUUM INTO` rather than copying the file: the database is in WAL mode, so
/// the file on its own is not the whole story, and a copy taken while a
/// checkpoint is outstanding is a copy missing the most recent thing the user
/// did. This takes the snapshot through SQLite, which knows about the WAL.
fn snapshot(conn: &Connection, path: &Path) -> rusqlite::Result<()> {
    // VACUUM INTO refuses to overwrite, and a backup from a previous upgrade
    // has already done its job — the file it was protecting is long since
    // migrated.
    let _ = std::fs::remove_file(path);
    conn.execute("VACUUM INTO ?1", params![path.to_string_lossy()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("ATTACH ':memory:' AS market", []).unwrap();
        run(&conn, None).unwrap();
        conn
    }

    /// Every table the app reads, with the columns it reads from them. The
    /// schema is one `CREATE` now rather than a history to replay, so this is
    /// the thing standing between a typo in it and an app that cannot start.
    #[test]
    fn a_fresh_database_has_everything_the_app_expects() {
        let conn = fresh();
        assert_eq!(version(&conn).unwrap(), LATEST);
        assert_eq!(version_of(&conn, MARKET).unwrap(), MARKET_LATEST);
        assert_eq!(
            tables(&conn, MARKET),
            vec![
                "bar_series:key,interval,first_ts,last_ts,count,fetched_at,ts,open,high,low,close,volume",
                "ladder_coverage:key,from_ts,to_ts",
                "ladder_days:key,day,tick_size,minutes,first_ts,last_ts,fetched_at,data,total",
                "ladder_tail:key,ts,day,tick_size,data",
                "quotes:key,last,previous,at,fetched_at,previous_at",
            ]
        );
        assert_eq!(
            tables(&conn, "main"),
            vec![
                "custom_bar_schemes:id,json",
                "custom_themes:id,json",
                "drawings:id,symbol,suffix,json",
                "meta:key,value",
                "settings:key,value",
                "watchlist_entries:section_id,symbol,suffix,position",
                "watchlist_sections:id,name,position,collapsed,watchlist_id",
                "watchlists:id,name,position",
            ]
        );
    }

    /// The watchlist that is always there, and the section inside it that
    /// holds whatever nobody filed. Entries hang off that section by foreign
    /// key, so an install without it cannot store a loose symbol at all.
    #[test]
    fn a_fresh_database_already_has_the_watchlist_nobody_can_delete() {
        let conn = fresh();
        let (id, name): (i64, String) = conn
            .query_row("SELECT id, name FROM watchlists", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!((id, name.as_str()), (crate::store::DEFAULT_WATCHLIST, "Default"));

        let root: (i64, i64) = conn
            .query_row("SELECT id, watchlist_id FROM watchlist_sections", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(root, (crate::store::ROOT_SECTION, crate::store::DEFAULT_WATCHLIST));
    }

    /// A section written without naming a watchlist belongs to the permanent
    /// one. The column default is the only thing that decides that, and it is
    /// a literal in SQL that nothing but this ties to the constant the rest of
    /// the app resolves the permanent watchlist by.
    #[test]
    fn the_default_watchlist_is_the_one_a_section_falls_into() {
        let conn = fresh();
        conn.execute("INSERT INTO watchlist_sections (name, position) VALUES ('Mine', 0)", [])
            .unwrap();

        let owner: i64 = conn
            .query_row("SELECT watchlist_id FROM watchlist_sections WHERE name = 'Mine'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(owner, crate::store::DEFAULT_WATCHLIST);
    }

    /// Both files' tables, one list after the other.
    fn schema(conn: &Connection) -> Vec<String> {
        let mut all = tables(conn, "main");
        all.extend(tables(conn, MARKET));
        all
    }

    fn tables(conn: &Connection, schema: &str) -> Vec<String> {
        let mut statement = conn
            .prepare(&format!(
                "SELECT m.name || ':' || group_concat(c.name)
                 FROM {schema}.sqlite_master m
                 JOIN pragma_table_info(m.name, '{schema}') c
                 WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'
                 GROUP BY m.name ORDER BY m.name"
            ))
            .unwrap();
        statement.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }

    /// A settings file from before the split, with bars and a quote in it,
    /// comes out with them in the market file and nothing else disturbed.
    #[test]
    fn bars_written_before_the_split_move_to_the_market_file() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute("ATTACH ':memory:' AS market", []).unwrap();
        apply(&conn, "main", &MIGRATIONS[..3], 3, None).unwrap();
        conn.execute_batch(
            "INSERT INTO settings (key, value) VALUES ('theme', 'midnight');
             INSERT INTO bar_series VALUES ('AAPL', '1D', 1, 2, 2, 3, x'01', x'02', x'03', x'04', x'05', x'06');
             INSERT INTO quotes (key, last, previous, at, fetched_at, previous_at)
                 VALUES ('AAPL', 2.0, 1.0, 2, 3, 1);",
        )
        .unwrap();

        run(&conn, None).unwrap();

        let moved: (String, Vec<u8>) = conn
            .query_row("SELECT key, volume FROM market.bar_series", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(moved, ("AAPL".to_string(), vec![6]));
        let quote: f64 =
            conn.query_row("SELECT last FROM market.quotes", [], |r| r.get(0)).unwrap();
        assert_eq!(quote, 2.0);
        assert!(!tables(&conn, "main").iter().any(|t| t.starts_with("bar_series")));
        let theme: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'theme'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(theme, "midnight");
    }

    /// The feed somebody chose comes along as a list of one; the default,
    /// or nothing chosen, leaves nothing stored at all.
    #[test]
    fn the_chosen_feed_becomes_the_top_of_a_list() {
        let migrated = |stored: Option<&str>| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            apply(&conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None).unwrap();
            apply(&conn, "main", &MIGRATIONS[..4], 4, None).unwrap();
            if let Some(value) = stored {
                conn.execute("INSERT INTO settings (key, value) VALUES ('provider', ?1)", [value])
                    .unwrap();
            }
            run(&conn, None).unwrap();
            let read = |key: &str| -> Option<String> {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).ok()
            };
            assert_eq!(read("provider"), None, "the old row goes");
            read("providers")
        };
        assert_eq!(migrated(None), None);
        assert_eq!(migrated(Some("yahoo")), None);
        assert_eq!(migrated(Some(" Yahoo ")), None);
        // And from version 8, Yahoo after it, where it was implied.
        assert_eq!(migrated(Some("tos")).as_deref(), Some(r#"["tos","yahoo"]"#));
        assert_eq!(migrated(Some("a\"b")).as_deref(), Some(r#"["a\"b","yahoo"]"#));
    }

    /// A header somebody chose gets the account where the note on it sat,
    /// after everything else, and the data feed after that once the two
    /// are apart; one nobody chose is left unstored.
    #[test]
    fn a_chosen_header_gains_the_account_at_its_end() {
        let migrated = |stored: Option<&str>| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            apply(&conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None).unwrap();
            apply(&conn, "main", &MIGRATIONS[..5], 5, None).unwrap();
            if let Some(value) = stored {
                conn.execute("INSERT INTO settings (key, value) VALUES ('chart_header', ?1)", [value])
                    .unwrap();
            }
            run(&conn, None).unwrap();
            conn.query_row("SELECT value FROM settings WHERE key = 'chart_header'", [], |r| r.get::<_, String>(0))
                .ok()
        };
        assert_eq!(migrated(None), None);
        assert_eq!(migrated(Some("change,symbol")).as_deref(), Some("change,symbol,account,feed"));
        assert_eq!(migrated(Some("none")).as_deref(), Some("account,feed"));
        assert_eq!(migrated(Some("symbol, Account")).as_deref(), Some("symbol, Account"));
    }

    /// The one Rithmic login there was becomes the first of the list, and
    /// the data login; a signed-out one, which had no user ID, leaves
    /// nothing. The old rows go either way.
    #[test]
    fn the_rithmic_login_becomes_the_first_of_a_list() {
        let migrated = |rows: &[(&str, &str)]| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            apply(&conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None).unwrap();
            apply(&conn, "main", &MIGRATIONS[..6], 6, None).unwrap();
            for (key, value) in rows {
                conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2)", [key, value]).unwrap();
            }
            apply(&conn, "main", &MIGRATIONS[..7], 7, None).unwrap();
            let read = |key: &str| -> Option<String> {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).ok()
            };
            for old in ["rithmic_user", "rithmic_system", "rithmic_gateway"] {
                assert_eq!(read(old), None, "{old} is still there");
            }
            (read("rithmic_logins"), read("rithmic_data_login"))
        };
        assert_eq!(migrated(&[]), (None, None));
        assert_eq!(migrated(&[("rithmic_system", "Lucid"), ("rithmic_gateway", "wss://x")]), (None, None));
        let (logins, data) = migrated(&[("rithmic_user", " trader ")]);
        let logins: serde_json::Value = serde_json::from_str(&logins.unwrap()).unwrap();
        assert_eq!(
            logins,
            serde_json::json!([{
                "id": "1",
                "name": "Rithmic Test",
                "user": "trader",
                "system": "Rithmic Test",
                "gateway": "wss://rituz00100.rithmic.com:443"
            }])
        );
        assert_eq!(data.as_deref(), Some("1"));
        let (logins, _) =
            migrated(&[("rithmic_user", "trader"), ("rithmic_system", "LucidTrading"), ("rithmic_gateway", "wss://x")]);
        let logins: serde_json::Value = serde_json::from_str(&logins.unwrap()).unwrap();
        assert_eq!(logins[0]["name"], "LucidTrading");
        assert_eq!(logins[0]["gateway"], "wss://x");
    }

    /// An order that does not name Yahoo gets it at the bottom, where it
    /// was implied; one that names it, or nothing stored, is left alone.
    #[test]
    fn yahoo_is_appended_to_an_order_that_does_not_name_it() {
        let migrated = |stored: Option<&str>| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            apply(&conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None).unwrap();
            apply(&conn, "main", &MIGRATIONS[..7], 7, None).unwrap();
            if let Some(value) = stored {
                conn.execute("INSERT INTO settings (key, value) VALUES ('providers', ?1)", [value]).unwrap();
            }
            run(&conn, None).unwrap();
            conn.query_row("SELECT value FROM settings WHERE key = 'providers'", [], |r| r.get::<_, String>(0)).ok()
        };
        assert_eq!(migrated(None), None);
        assert_eq!(migrated(Some(r#"["rithmic","tos"]"#)).as_deref(), Some(r#"["rithmic","tos","yahoo"]"#));
        assert_eq!(migrated(Some(r#"["tos"]"#)).as_deref(), Some(r#"["tos","yahoo"]"#));
        assert_eq!(migrated(Some(r#"["Yahoo","tos"]"#)).as_deref(), Some(r#"["Yahoo","tos"]"#));
        assert_eq!(migrated(Some("not a list")).as_deref(), Some("not a list"));
    }

    /// The Rithmic logins become logins that name their broker, with the
    /// same ids, the system and gateway as values, and the data login under
    /// its new key; the app reads them back as it reads any login. A
    /// profile from before there were lists comes the whole way.
    #[test]
    fn the_logins_name_their_broker() {
        let migrated = |from: usize, rows: &[(&str, &str)]| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            apply(&conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None).unwrap();
            apply(&conn, "main", &MIGRATIONS[..from], from as i32, None).unwrap();
            for (key, value) in rows {
                conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2)", [key, value]).unwrap();
            }
            run(&conn, None).unwrap();
            let read = |key: &str| -> Option<String> {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).ok()
            };
            for old in ["rithmic_logins", "rithmic_data_login"] {
                assert_eq!(read(old), None, "{old} is still there");
            }
            (read("logins"), read("data_login"))
        };
        assert_eq!(migrated(8, &[]), (None, None));

        let two = r#"[{"id":"1","name":"Lucid","user":"trader","system":"LucidTrading","gateway":"wss://x"},
                      {"id":"18f","name":"Rithmic Test","user":"dev","system":"Rithmic Test",
                       "gateway":"wss://rituz00100.rithmic.com:443","signed_out":true}]"#;
        let (logins, data) = migrated(8, &[("rithmic_logins", two), ("rithmic_data_login", "18f")]);
        assert_eq!(data.as_deref(), Some("18f"));
        let logins: Vec<crate::trading::Login> = serde_json::from_str(&logins.unwrap()).unwrap();
        assert_eq!(logins.len(), 2);
        assert_eq!((logins[0].id.as_str(), logins[0].name.as_str(), logins[0].broker.as_str()), ("1", "Lucid", "rithmic"));
        assert_eq!((logins[0].user.as_str(), logins[0].value("system"), logins[0].value("gateway")), ("trader", Some("LucidTrading"), Some("wss://x")));
        assert!(!logins[0].signed_out);
        assert_eq!((logins[1].id.as_str(), logins[1].signed_out), ("18f", true));

        let (logins, data) = migrated(6, &[("rithmic_user", "trader")]);
        assert_eq!(data.as_deref(), Some("1"));
        let logins: serde_json::Value = serde_json::from_str(&logins.unwrap()).unwrap();
        assert_eq!(
            logins,
            serde_json::json!([{
                "id": "1",
                "name": "Rithmic Test",
                "broker": "rithmic",
                "user": "trader",
                "values": [["system", "Rithmic Test"], ["gateway", "wss://rituz00100.rithmic.com:443"]]
            }])
        );

        let unreadable = migrated(8, &[("rithmic_logins", "not a list")]);
        assert_eq!(unreadable.0.as_deref(), Some("not a list"), "carried, not lost");
    }

    /// A header that showed the account shows the account and the data
    /// feed, the feed right after it; one that did not is as it was.
    #[test]
    fn the_account_part_splits_into_the_account_and_the_feed() {
        let migrated = |rows: &[(&str, &str)]| {
            let conn = Connection::open_in_memory().unwrap();
            conn.execute("ATTACH ':memory:' AS market", []).unwrap();
            apply(&conn, MARKET, MARKET_MIGRATIONS, MARKET_LATEST, None).unwrap();
            apply(&conn, "main", &MIGRATIONS[..9], 9, None).unwrap();
            for (key, value) in rows {
                conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2)", [key, value]).unwrap();
            }
            run(&conn, None).unwrap();
            let read = |key: &str| -> Option<String> {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).ok()
            };
            (read("chart_header"), read("chart_header_center"))
        };
        assert_eq!(migrated(&[]), (None, None));
        assert_eq!(
            migrated(&[("chart_header", "symbol,account,size"), ("chart_header_center", "stripe")]),
            (Some("symbol,account,feed,size".into()), Some("stripe".into()))
        );
        assert_eq!(migrated(&[("chart_header", "symbol, account")]).0.as_deref(), Some("symbol, account,feed"));
        assert_eq!(migrated(&[("chart_header_center", "account")]).1.as_deref(), Some("account,feed"));
        // Without the account, or with the feed already, nothing moves.
        assert_eq!(migrated(&[("chart_header", "symbol,resolution")]).0.as_deref(), Some("symbol,resolution"));
        assert_eq!(migrated(&[("chart_header", "feed,account")]).0.as_deref(), Some("feed,account"));
    }

    #[test]
    fn migrating_twice_changes_nothing_the_second_time() {
        // Every launch runs this. The second one has to be a read of one
        // number and nothing else — not least because the schema step would
        // now fail outright if it were ever handed a database it had built.
        let conn = fresh();
        let after_once = (version(&conn).unwrap(), schema(&conn));
        run(&conn, None).unwrap();

        assert_eq!((version(&conn).unwrap(), schema(&conn)), after_once);
    }

    #[test]
    fn a_database_from_the_future_is_refused_rather_than_opened() {
        // It is intact and it is theirs. Starting them a blank one beside it
        // looks exactly like having lost the lot.
        let conn = fresh();
        conn.pragma_update(None, "user_version", LATEST + 1).unwrap();

        match run(&conn, None) {
            Err(Error::FromTheFuture { found, known }) => {
                assert_eq!((found, known), (LATEST + 1, LATEST));
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// Three steps, of both kinds, over a database that starts at nothing.
    ///
    /// There is one real migration today and there will not be a second until
    /// after release — so without this, the first upgrade Omacharts ever
    /// performs on somebody's machine would be the first time a chain of more
    /// than one step had run anywhere.
    #[test]
    fn a_chain_of_steps_runs_in_order_and_stops_at_the_end() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn, "main", &chain(), 3, None).unwrap();

        assert_eq!(version(&conn).unwrap(), 3);
        assert_eq!(trail(&conn), vec!["first", "second", "third"]);
    }

    /// A database part way along takes the rest of the chain and no more.
    /// Re-running a step that has already run is how an upgrade turns into a
    /// bug report.
    #[test]
    fn only_the_steps_a_database_has_not_had_are_applied() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn, "main", &chain()[..1], 1, None).unwrap();
        assert_eq!(trail(&conn), vec!["first"]);

        apply(&conn, "main", &chain(), 3, None).unwrap();
        assert_eq!(trail(&conn), vec!["first", "second", "third"], "a step ran twice");
    }

    /// Both kinds of step, writing their name as they go.
    fn chain() -> Vec<Migration> {
        vec![
            Migration {
                version: 1,
                name: "first",
                risky: false,
                step: Step::Sql(
                    "CREATE TABLE trail (at INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);
                     INSERT INTO trail (name) VALUES ('first');",
                ),
            },
            Migration {
                version: 2,
                name: "second",
                risky: false,
                step: Step::Rust(|conn| {
                    conn.execute("INSERT INTO trail (name) VALUES ('second')", [])?;
                    Ok(())
                }),
            },
            Migration {
                version: 3,
                name: "third",
                risky: false,
                step: Step::Sql("INSERT INTO trail (name) VALUES ('third');"),
            },
        ]
    }

    fn trail(conn: &Connection) -> Vec<String> {
        let mut statement = conn.prepare("SELECT name FROM trail ORDER BY at").unwrap();
        statement.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn a_step_that_fails_leaves_the_version_where_it_was() {
        // The state nothing can recover from is half-applied, so the version
        // and the change it describes share a transaction.
        let mut broken = chain();
        broken[1] = Migration {
            version: 2,
            name: "throws half way",
            risky: false,
            step: Step::Sql(
                "CREATE TABLE gone (id INTEGER PRIMARY KEY);
                 INSERT INTO nonexistent (id) VALUES (1);",
            ),
        };

        let conn = Connection::open_in_memory().unwrap();
        assert!(apply(&conn, "main", &broken, 3, None).is_err());

        assert_eq!(version(&conn).unwrap(), 1, "the failed step moved the version");
        assert_eq!(trail(&conn), vec!["first"], "the step before it was rolled back too");
        assert!(!table_exists(&conn, "gone"), "half of the failed step survived");
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
            > 0
    }

    #[test]
    fn a_risky_step_leaves_a_copy_of_the_file_behind() {
        const RISKY: &[Migration] = &[Migration {
            version: 2,
            name: "rewrites something",
            risky: true,
            step: Step::Sql("CREATE TABLE after (id INTEGER PRIMARY KEY);"),
        }];

        let dir = std::env::temp_dir().join(format!("omacharts-migrate-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (db, backup) = (dir.join("omacharts.db"), dir.join("omacharts.db.bak"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&backup);

        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("CREATE TABLE before (id INTEGER PRIMARY KEY); PRAGMA user_version = 1;")
            .unwrap();
        apply(&conn, "main", RISKY, 2, Some(&backup)).unwrap();

        let restored = Connection::open(&backup).unwrap();
        assert!(table_exists(&restored, "before"), "the snapshot missed what was there");
        assert!(!table_exists(&restored, "after"), "the snapshot was taken too late to help");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_file_is_not_snapshotted_however_risky_its_steps() {
        const RISKY: &[Migration] = &[Migration {
            version: 1,
            name: "rewrites something",
            risky: true,
            step: Step::Sql("CREATE TABLE after (id INTEGER PRIMARY KEY);"),
        }];
        let dir = std::env::temp_dir().join(format!("omacharts-new-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (db, backup) = (dir.join("omacharts.db"), dir.join("omacharts.db.bak"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&backup);

        let conn = Connection::open(&db).unwrap();
        apply(&conn, "main", RISKY, 1, Some(&backup)).unwrap();
        assert!(!backup.exists(), "a new install has nothing to lose");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_is_snapshotted_for_a_step_that_cannot_lose_anything() {
        // Adding a table copies nothing, and the database is tens of
        // megabytes of cached bars: a snapshot on every launch that happens to
        // add a column is a cost paid forever for no risk avoided.
        const SAFE: &[Migration] = &[Migration {
            version: 1,
            name: "adds a table",
            risky: false,
            step: Step::Sql("CREATE TABLE added (id INTEGER PRIMARY KEY);"),
        }];

        let dir = std::env::temp_dir().join(format!("omacharts-nosnap-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let (db, backup) = (dir.join("omacharts.db"), dir.join("omacharts.db.bak"));
        let _ = std::fs::remove_file(&db);
        let _ = std::fs::remove_file(&backup);

        let conn = Connection::open(&db).unwrap();
        apply(&conn, "main", SAFE, 1, Some(&backup)).unwrap();

        assert!(!backup.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_versions_are_consecutive_and_end_at_latest() {
        // A gap or a repeat means a database can stop somewhere no step will
        // ever pick it up from again.
        for (i, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version, i as i32 + 1, "{} is misnumbered", migration.name);
        }
        assert_eq!(MIGRATIONS.last().map(|m| m.version), Some(LATEST));
        for (i, migration) in MARKET_MIGRATIONS.iter().enumerate() {
            assert_eq!(migration.version, i as i32 + 1, "{} is misnumbered", migration.name);
        }
        assert_eq!(MARKET_MIGRATIONS.last().map(|m| m.version), Some(MARKET_LATEST));
        assert!(MIGRATIONS.iter().any(|m| m.version == MARKET_MOVED_OUT && m.risky));
    }
}
