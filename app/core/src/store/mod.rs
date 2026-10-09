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

pub mod agents;
pub mod categories;
pub mod changes;
pub mod context;
pub mod daystrip;
pub mod embed;
pub mod glean;
pub mod incoming;
pub mod ingest;
pub mod items;
pub mod links;
pub mod message_keys;
pub mod notes;
pub mod outbox;
pub mod people;
pub mod priorities;
pub mod reflect;
pub mod respond;
pub mod sample;
pub mod schema;
pub mod search;
pub mod sources;
pub mod splits;
pub mod suggest;
pub mod timestamps;
pub mod todos;
pub mod topics;
pub mod usage;
pub mod utc_migration;

use crate::domain::ports::{Reads, UnitOfWork};
use crate::error::Result;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::hooks::{AuthContext, Authorization};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Reader connections: enough for the UI's calls and the sync threads' reads at once.
const READERS: u32 = 4;
/// Longer than this holding the writer gets logged in debug builds.
const SLOW_WRITE: Duration = Duration::from_millis(100);

pub struct Db {
    writer: Mutex<Connection>,
    /// None for an in-memory database (tests): reads go through the writer.
    readers: Option<r2d2::Pool<SqliteConnectionManager>>,
    changes: Arc<changes::Changes>,
}

/// Ports over a test's own connection (fixtures).
#[cfg(any(test, feature = "test-support"))]
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

type Seen = Arc<Mutex<BTreeSet<String>>>;

thread_local! {
    /// This thread is inside `Db::write` (or holds the legacy guard): taking
    /// the writer again would deadlock.
    static WRITING: Cell<bool> = const { Cell::new(false) };
    /// Set while `Db::traced` runs: every read on this thread notes the
    /// tables it read here.
    static TRACE: RefCell<Option<Seen>> = const { RefCell::new(None) };
}

/// Ends a `Db::traced` on this thread, even if its closure panics.
struct Tracing;

impl Drop for Tracing {
    fn drop(&mut self) {
        TRACE.with(|t| t.borrow_mut().take());
    }
}

struct Writing;

impl Writing {
    fn enter() -> Writing {
        assert!(
            !WRITING.with(Cell::get),
            "the database writer is already held on this thread (a write inside a write)"
        );
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
                r2d2::Pool::builder().max_size(READERS).min_idle(Some(0)).build(
                    SqliteConnectionManager::file(Path::new(path))
                        .with_flags(
                            OpenFlags::SQLITE_OPEN_READ_ONLY
                                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                                | OpenFlags::SQLITE_OPEN_URI,
                        )
                        .with_init(|c| {
                            c.busy_timeout(Duration::from_secs(5))?;
                            c.pragma_update(None, "query_only", true)
                        }),
                )?,
            ),
            None => None,
        };
        let changes = Arc::new(changes::Changes::default());
        changes.install(&writer)?;
        Ok(Db {
            writer: Mutex::new(writer),
            readers,
            changes,
        })
    }

    /// Which tables commits changed (`changes::Changes`).
    pub fn changes(&self) -> &Arc<changes::Changes> {
        &self.changes
    }

    /// `read`, also returning every table the reads touched (SQLite's
    /// authorizer reports them as statements are prepared, so the
    /// connection's statement cache is emptied first).
    pub fn read_traced<T>(&self, f: impl FnOnce(&dyn Reads) -> Result<T>) -> Result<(T, BTreeSet<String>)> {
        let seen = Seen::default();
        let out = self.read_noting(&seen, f)?;
        let tables = std::mem::take(&mut *seen.lock().unwrap_or_else(|e| e.into_inner()));
        Ok((out, tables))
    }

    /// Run `f`, which may take several reads (and read files between them,
    /// with nothing open), returning every table those reads touched. A
    /// write inside it is refused: what's traced is only ever re-run.
    pub fn traced<T>(&self, f: impl FnOnce() -> Result<T>) -> Result<(T, BTreeSet<String>)> {
        let seen = Seen::default();
        TRACE.with(|t| {
            let mut t = t.borrow_mut();
            assert!(t.is_none(), "Db::traced inside Db::traced");
            *t = Some(seen.clone());
        });
        let out = {
            let _t = Tracing;
            f()
        }?;
        let tables = std::mem::take(&mut *seen.lock().unwrap_or_else(|e| e.into_inner()));
        Ok((out, tables))
    }

    fn read_noting<T>(&self, seen: &Seen, f: impl FnOnce(&dyn Reads) -> Result<T>) -> Result<T> {
        let trace = |c: &Connection| -> rusqlite::Result<()> {
            c.flush_prepared_statement_cache();
            let seen = seen.clone();
            c.authorizer(Some(move |ctx: AuthContext<'_>| changes::authorize(ctx, Some(&seen))))
        };
        // A reader goes back to no authorizer; the writer (in-memory
        // databases read through it) to its own.
        let untrace = |c: &Connection| c.authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
        let untrace_writer = |c: &Connection| c.authorizer(Some(|ctx: AuthContext<'_>| changes::authorize(ctx, None)));
        match &self.readers {
            Some(pool) => {
                let mut c = pool.get()?;
                trace(&c)?;
                let out = c
                    .transaction_with_behavior(TransactionBehavior::Deferred)
                    .map_err(Into::into)
                    .and_then(|tx| f(&Sql(&tx)));
                untrace(&c)?;
                out
            }
            None => {
                let _w = Writing::enter();
                let mut c = self.lock();
                trace(&c)?;
                let out = c
                    .transaction_with_behavior(TransactionBehavior::Deferred)
                    .map_err(Into::into)
                    .and_then(|tx| f(&Sql(&tx)));
                untrace_writer(&c)?;
                out
            }
        }
    }

    /// One consistent snapshot, on a reader connection.
    pub fn read<T>(&self, f: impl FnOnce(&dyn Reads) -> Result<T>) -> Result<T> {
        if let Some(seen) = TRACE.with(|t| t.borrow().clone()) {
            return self.read_noting(&seen, f);
        }
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
        if TRACE.with(|t| t.borrow().is_some()) {
            return Err(crate::error::msg(format!("a write from {caller} inside a traced read")));
        }
        let _w = Writing::enter();
        let mut c = self.lock();
        let started = Instant::now();
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&Sql(&tx));
        let out = match out {
            Ok(v) => tx.commit().map(|()| v).map_err(Into::into),
            Err(e) => Err(e), // dropping `tx` rolls back
        };
        match out {
            Ok(_) => self.changes.publish(),
            Err(_) => self.changes.discard(),
        }
        if cfg!(debug_assertions) && started.elapsed() > SLOW_WRITE {
            eprintln!(
                "database: a write from {caller} held the writer for {:?}",
                started.elapsed()
            );
        }
        out
    }

    /// The writer connection itself, for tests' fixtures and checks (no
    /// transaction; each statement commits alone). Not in the app.
    #[cfg(any(test, feature = "test-support"))]
    pub fn legacy(&self) -> LegacyGuard<'_> {
        LegacyGuard {
            _w: Writing::enter(),
            guard: self.lock(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic inside `write` rolled its transaction back; the connection is fine.
        self.writer.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `Db::legacy`: the writer, held until dropped.
#[cfg(any(test, feature = "test-support"))]
pub struct LegacyGuard<'a> {
    // Field order: the mutex guard drops first, then the thread is marked free.
    guard: MutexGuard<'a, Connection>,
    _w: Writing,
}

#[cfg(any(test, feature = "test-support"))]
impl std::ops::Deref for LegacyGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        &self.guard
    }
}

#[cfg(any(test, feature = "test-support"))]
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
    agents: crate::domain::agents::AgentViews,
    categories: crate::domain::categories::CategoryViews,
    context: crate::domain::context::ContextViews,
    day_strip: crate::domain::daystrip::DayStripViews,
    glean: crate::domain::glean::GleanViews,
    incoming: crate::domain::incoming::IncomingViews,
    items: crate::domain::items::ItemViews,
    notes: crate::domain::notes::NotesViews,
    outbox: crate::domain::outbox::OutboxViews,
    people: crate::domain::people::PeopleViews,
    priorities: crate::domain::priorities::PriorityViews,
    reflect: crate::domain::reflect::ReflectViews,
    respond: crate::domain::respond::RespondViews,
    search: crate::domain::search::SearchViews,
    sources: crate::domain::sources::SourceViews,
    suggest: crate::domain::suggest::SuggestViews,
    todos: crate::domain::todos::TodoViews,
    topics: crate::domain::topics::TopicViews,
    usage: crate::domain::usage::UsageViews,
    vectors: crate::domain::embed::VectorViews,
});

ports!(UnitOfWork {
    reads: Reads,
    agents: crate::domain::agents::AgentState,
    categories: crate::domain::categories::Categories,
    events: crate::domain::daystrip::Events,
    ingest: crate::domain::ingest::Ingest,
    items: crate::domain::items::Items,
    search_feedback: crate::domain::glean::SearchFeedback,
    links: crate::domain::links::Links,
    note_lines: crate::domain::notes::NoteLines,
    outbox: crate::domain::outbox::Outbox,
    people: crate::domain::people::People,
    reflections: crate::domain::reflect::Reflections,
    sample: crate::domain::sample::SampleData,
    sources: crate::domain::sources::Sources,
    splits: crate::domain::splits::Splits,
    suggest_runs: crate::domain::suggest::SuggestRuns,
    todos: crate::domain::todos::Todos,
    topics: crate::domain::topics::Topics,
    usage: crate::domain::usage::Usage,
    vectors: crate::domain::embed::Vectors,
});

#[cfg(test)]
mod tests;
