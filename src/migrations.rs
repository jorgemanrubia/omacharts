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
pub const LATEST: i32 = 2;

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
}];

/// Bring a database up to [`LATEST`], or say why it cannot be.
///
/// `snapshot_to` is where to leave a copy before anything destructive runs;
/// `None` for a database with no file behind it, which is every test and the
/// in-memory fallback.
pub fn run(conn: &Connection, snapshot_to: Option<&Path>) -> Result<(), Error> {
    apply(conn, MIGRATIONS, LATEST, snapshot_to)
}

/// What [`run`] does, over any list — so the awkward cases can be tested
/// without inventing a schema change to do it with.
fn apply(
    conn: &Connection,
    migrations: &[Migration],
    latest: i32,
    snapshot_to: Option<&Path>,
) -> Result<(), Error> {
    let from = version(conn)?;
    if from > latest {
        return Err(Error::FromTheFuture { found: from, known: latest });
    }

    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > from).collect();
    if pending.is_empty() {
        return Ok(());
    }
    if let Some(path) = snapshot_to.filter(|_| pending.iter().any(|m| m.risky)) {
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
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;
    }
    Ok(())
}

pub fn version(conn: &Connection) -> rusqlite::Result<i32> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
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
        assert_eq!(
            schema(&conn),
            vec![
                "bar_series:key,interval,first_ts,last_ts,count,fetched_at,ts,open,high,low,close,volume",
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

    fn schema(conn: &Connection) -> Vec<String> {
        let mut statement = conn
            .prepare(
                "SELECT m.name || ':' || group_concat(c.name)
                 FROM sqlite_master m
                 JOIN pragma_table_info(m.name) c
                 WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'
                 GROUP BY m.name ORDER BY m.name",
            )
            .unwrap();
        statement.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
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
        apply(&conn, &chain(), 3, None).unwrap();

        assert_eq!(version(&conn).unwrap(), 3);
        assert_eq!(trail(&conn), vec!["first", "second", "third"]);
    }

    /// A database part way along takes the rest of the chain and no more.
    /// Re-running a step that has already run is how an upgrade turns into a
    /// bug report.
    #[test]
    fn only_the_steps_a_database_has_not_had_are_applied() {
        let conn = Connection::open_in_memory().unwrap();
        apply(&conn, &chain()[..1], 1, None).unwrap();
        assert_eq!(trail(&conn), vec!["first"]);

        apply(&conn, &chain(), 3, None).unwrap();
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
        assert!(apply(&conn, &broken, 3, None).is_err());

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
            version: 1,
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
        conn.execute_batch("CREATE TABLE before (id INTEGER PRIMARY KEY);").unwrap();
        apply(&conn, RISKY, 1, Some(&backup)).unwrap();

        let restored = Connection::open(&backup).unwrap();
        assert!(table_exists(&restored, "before"), "the snapshot missed what was there");
        assert!(!table_exists(&restored, "after"), "the snapshot was taken too late to help");

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
        apply(&conn, SAFE, 1, Some(&backup)).unwrap();

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
    }
}
