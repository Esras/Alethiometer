//! Storage: the only module with SQL (`app/docs/db-plan.md`, Phase 1).
//!
//! - [`Db`] is the database: one writer connection and a pool of readers.
//!   Nothing outside the edge (`api.rs`, a sync batch, an outbox send, an
//!   agent flush) touches it.
//! - [`Db::read`] hands a closure the read ports ([`Reads`]) over one
//!   consistent snapshot on a reader; it never waits for the writer (WAL).
//! - [`Db::write`] hands a closure one unit of work ([`UnitOfWork`]) in a
//!   `BEGIN IMMEDIATE` transaction: committed if the closure returns `Ok`,
//!   rolled back on an error or a panic. Repositories never commit.
//! - No connection, guard or transaction leaves a closure, so holding the
//!   database across a network call or a file read can't be written by
//!   accident. Debug builds log a write that holds the writer over 100 ms.
//!
//! Each `store/<area>.rs` implements that area's ports (`crate::domain::ports`)
//! on [`Sql`], a connection in the request's transaction.

pub mod daystrip;
pub mod incoming;
pub mod items;
pub mod links;
pub mod notes;
pub mod outbox;
pub mod people;
pub mod schema;
pub mod search;
pub mod sources;
pub mod todos;
pub mod topics;
pub mod utc_migration;

use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::Result;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::cell::Cell;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Reader connections: enough for the UI's calls and the sync threads' reads at once.
const READERS: u32 = 4;
/// Longer than this holding the writer gets logged in debug builds.
const SLOW_WRITE: Duration = Duration::from_millis(100);

pub struct Db {
    writer: Mutex<Connection>,
    /// None for an in-memory database (tests): reads go through the writer.
    readers: Option<r2d2::Pool<SqliteConnectionManager>>,
}

/// Ports over a connection code not moved yet already holds (`app.db()`), so
/// it can call services that were. Goes with `Db::legacy`.
pub fn over(conn: &Connection) -> Sql<'_> {
    Sql(conn)
}

/// The connection in the request's transaction. Implements every port; what
/// a caller can reach is limited by the trait object it's handed.
pub struct Sql<'t>(&'t Connection);

impl<'t> Sql<'t> {
    pub(crate) fn conn(&self) -> &'t Connection {
        self.0
    }
}

thread_local! {
    /// This thread is inside `Db::write` (or holds the legacy guard): taking
    /// the writer again would deadlock.
    static WRITING: Cell<bool> = const { Cell::new(false) };
}

struct Writing;

impl Writing {
    fn enter() -> Writing {
        assert!(!WRITING.with(Cell::get), "the database writer is already held on this thread (a write inside a write, or app.db() inside one)");
        WRITING.with(|w| w.set(true));
        Writing
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        WRITING.with(|w| w.set(false));
    }
}

impl Db {
    /// A migrated connection (`schema::open`); readers are opened beside it
    /// when it's a file.
    pub fn new(writer: Connection) -> anyhow::Result<Db> {
        let readers = match writer.path().filter(|p| !p.is_empty()) {
            Some(path) => Some(
                r2d2::Pool::builder()
                    .max_size(READERS)
                    .min_idle(Some(0))
                    .build(
                        SqliteConnectionManager::file(Path::new(path))
                            .with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)
                            .with_init(|c| {
                                c.busy_timeout(Duration::from_secs(5))?;
                                c.pragma_update(None, "query_only", true)
                            }),
                    )?,
            ),
            None => None,
        };
        Ok(Db { writer: Mutex::new(writer), readers })
    }

    /// One consistent snapshot, on a reader connection.
    pub fn read<T>(&self, f: impl FnOnce(&dyn Reads) -> Result<T>) -> Result<T> {
        match &self.readers {
            Some(pool) => {
                let mut c = pool.get()?;
                let tx = c.transaction_with_behavior(TransactionBehavior::Deferred)?;
                f(&Sql(&tx))
            }
            None => {
                let _w = Writing::enter();
                let mut c = self.lock();
                let tx = c.transaction_with_behavior(TransactionBehavior::Deferred)?;
                f(&Sql(&tx))
            }
        }
    }

    /// One unit of work: committed if `f` returns `Ok`, rolled back if it
    /// returns an error or panics.
    #[track_caller]
    pub fn write<T>(&self, f: impl FnOnce(&dyn UnitOfWork) -> Result<T>) -> Result<T> {
        let caller = std::panic::Location::caller();
        let _w = Writing::enter();
        let mut c = self.lock();
        let started = Instant::now();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&Sql(&tx));
        let out = match out {
            Ok(v) => tx.commit().map(|()| v).map_err(Into::into),
            Err(e) => Err(e), // dropping `tx` rolls back
        };
        if cfg!(debug_assertions) && started.elapsed() > SLOW_WRITE {
            eprintln!("database: a write from {caller} held the writer for {:?}", started.elapsed());
        }
        out
    }

    /// The writer connection itself: the old `app.db()`, for code not moved
    /// to repositories yet (no transaction; each statement commits alone).
    /// Goes when nothing calls it.
    pub fn legacy(&self) -> LegacyGuard<'_> {
        LegacyGuard { _w: Writing::enter(), guard: self.lock() }
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic inside `write` rolled its transaction back; the connection is fine.
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `app.db()`: the writer, held until dropped.
pub struct LegacyGuard<'a> {
    // Field order: the mutex guard drops first, then the thread is marked free.
    guard: MutexGuard<'a, Connection>,
    _w: Writing,
}

impl std::ops::Deref for LegacyGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.guard
    }
}

impl std::ops::DerefMut for LegacyGuard<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        &mut self.guard
    }
}

/// Each port is implemented on `Sql` in its own file; these hand it out.
macro_rules! ports {
    ($tr:ident { $($name:ident: $port:path),* $(,)? }) => {
        impl $tr for Sql<'_> {
            $(fn $name(&self) -> &dyn $port { self })*
        }
    };
}

ports!(Reads {
    day_strip: crate::domain::daystrip::DayStripViews,
    incoming: crate::domain::incoming::IncomingViews,
    notes: crate::domain::notes::NotesViews,
    people: crate::domain::people::PeopleViews,
    search: crate::domain::search::SearchViews,
    sources: crate::domain::sources::SourceViews,
    todos: crate::domain::todos::TodoViews,
    topics: crate::domain::topics::TopicViews,
});

ports!(UnitOfWork {
    reads: Reads,
    events: crate::domain::daystrip::Events,
    items: crate::domain::items::Items,
    links: crate::domain::links::Links,
    note_lines: crate::domain::notes::NoteLines,
    outbox: crate::domain::outbox::Outbox,
    sources: crate::domain::sources::Sources,
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::msg;

    fn file_db() -> (Db, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::new(schema::open(&dir.path().join("t.sqlite")).unwrap()).unwrap();
        (db, dir)
    }

    /// A life block (id 1) with a 10-minute buffer.
    fn life_block(db: &Db) {
        db.legacy()
            .execute_batch(
                "INSERT INTO sources (id, kind, tag, name) VALUES (1, 'protoncal', 'personal', 'Life');
                 INSERT INTO events (id, source_id, start, end, category, is_life_busy, buffer_min)
                 VALUES (1, 1, '2026-10-03T17:00:00.000Z', '2026-10-03T18:00:00.000Z', 'life', 1, 10);",
            )
            .unwrap();
    }

    fn buffer(r: &dyn Reads) -> Option<i64> {
        let day = (crate::time::t("2026-10-03T00:00:00"), crate::time::t("2026-10-04T00:00:00"));
        r.day_strip().blocks(day.0, day.1).unwrap()[0].buffer_min
    }

    #[test]
    fn an_error_or_a_panic_rolls_the_whole_unit_of_work_back() {
        let (db, _dir) = file_db();
        life_block(&db);
        let r: Result<()> = db.write(|uow| {
            uow.events().set_buffer(1, 20)?;
            uow.events().set_buffer(1, 30)?;
            Err(msg("failed after two writes"))
        });
        assert!(r.is_err());
        assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(10));
        let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = db.write(|uow| -> Result<()> {
                uow.events().set_buffer(1, 20)?;
                panic!("mid-action");
            });
        }));
        assert!(p.is_err());
        assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(10), "the panic rolled back");
        db.write(|uow| uow.events().set_buffer(1, 25)).unwrap();
        assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(25), "and the writer still works");
    }

    #[test]
    fn a_long_read_runs_while_a_write_commits_and_keeps_its_snapshot() {
        let (db, _dir) = file_db();
        life_block(&db);
        let db = std::sync::Arc::new(db);
        let (started_tx, started) = std::sync::mpsc::channel();
        let (go_tx, go) = std::sync::mpsc::channel::<()>();
        let reader = {
            let db = db.clone();
            std::thread::spawn(move || {
                db.read(|r| {
                    let before = buffer(r);
                    started_tx.send(()).unwrap();
                    go.recv().unwrap(); // the write commits meanwhile
                    Ok((before, buffer(r)))
                })
                .unwrap()
            })
        };
        started.recv().unwrap();
        let t = Instant::now();
        db.write(|uow| uow.events().set_buffer(1, 45)).unwrap();
        assert!(t.elapsed() < Duration::from_secs(1), "the write didn't wait for the read");
        go_tx.send(()).unwrap();
        assert_eq!(reader.join().unwrap(), (Some(10), Some(10)), "one snapshot for the whole read");
        assert_eq!(db.read(|r| Ok(buffer(r))).unwrap(), Some(45));
    }

    #[test]
    #[should_panic(expected = "already held on this thread")]
    fn the_writer_taken_twice_on_one_thread_is_a_bug_not_a_hang() {
        let (db, _dir) = file_db();
        let _ = db.write(|_| {
            let _inner = db.legacy();
            Ok(())
        });
    }

    /// SQL lives in `store/`. Files that still have it outside are listed
    /// with their count of statements; the list only shrinks.
    #[test]
    fn no_new_sql_outside_store() {
        let re = regex::Regex::new(r#"(?i)"\s*(SELECT|INSERT\s+(OR\s+\w+\s+)?INTO|UPDATE\s+\w+\s+SET|DELETE\s+FROM|CREATE\s+(TABLE|INDEX|TRIGGER|VIEW)|ALTER\s+TABLE|DROP\s+TABLE|WITH\s+\w+\s+AS)\b"#).unwrap();
        let root = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut found = std::collections::BTreeMap::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p != root.join("store") {
                        dirs.push(p);
                    }
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    // Test fixtures may write rows directly.
                    let code = text.split("#[cfg(test)]\nmod tests").next().unwrap();
                    let n = re.find_iter(code).count();
                    if n > 0 {
                        found.insert(p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), n);
                    }
                }
            }
        }
        let baseline: std::collections::BTreeMap<String, usize> =
            SQL_OUTSIDE_STORE.lines().filter_map(|l| l.split_once(' ')).map(|(n, f)| (f.to_owned(), n.parse().unwrap())).collect();
        let mut problems = vec![];
        for (f, n) in &found {
            match baseline.get(f) {
                None => problems.push(format!("{f}: {n} new SQL statements outside store/ (move them into a repository)")),
                Some(b) if n > b => problems.push(format!("{f}: {n} SQL statements, up from {b} (move the new ones into a repository)")),
                Some(b) if n < b => problems.push(format!("{f}: {n} SQL statements, down from {b}: lower its line in SQL_OUTSIDE_STORE")),
                _ => {}
            }
        }
        for f in baseline.keys().filter(|f| !found.contains_key(*f)) {
            problems.push(format!("{f}: no SQL left: remove its line from SQL_OUTSIDE_STORE"));
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    /// `<count> <file>`: SQL statements not yet moved into `store/`.
    const SQL_OUTSIDE_STORE: &str = include_str!("sql_outside_store.txt");
}
