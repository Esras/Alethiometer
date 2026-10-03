# Database and sync architecture: plan

Status: **Phases 0 and 1 built (2026-10-03); 2 to 4 agreed, not built.** Follows `REVIEW-2026-10-03.md`. The contained fixes from that review are in (commits `b2191b2`…`c3f2424`); this covers what needs a structural change.

Priorities: accuracy > performance, maintainability > speed of development.

## Decisions (2026-10-03)

- **Changed while you looked:** triage applies to what the UI showed. Anything newer stays in Incoming.
- **Change notification:** the core pushes changes to the UI.
- **Time:** UTC first, before anything else adds more local-time code.
- **Derived data:** the database computes it (views, triggers), never two edits by the application.
- **Layering:** SQL lives only in the storage layer; the rest of the app calls repositories inside a unit of work.

## Problems this solves

| From the review | Root cause |
|---|---|
| UI stalls when a sync, the embedder or Notes holds the lock (seconds at scale) | One `Mutex<Connection>` shared by every thread and command |
| Partial writes: Confirm/Move/Dismiss commit half their work, then error | Writes with no transaction; `app.db()` hands out a bare connection |
| Business rules and SQL interleaved: about 360 statements in 33 files | No storage layer: `queries/`, `sync/`, `ai/`, `glean/`, `agents/`, `settings/` all write SQL |
| Times misorder across DST and time-zone changes | Instants stored as naive local strings, mixed with SQLite's UTC `'now'` |
| Several syncs of one source at once; a catch-up erases live syncs' progress | No owner per source; the cursor is one JSON blob rewritten whole |
| One bad Slack channel stops all of Slack | Polls are all-or-nothing |
| The 50-message cap deletes stored messages; reused ids let a split move the wrong one | Messages deleted and re-inserted on every sync |
| Confirm buries messages that arrived after render; Gmail archives past unread mail | Triage acts on the DB's current state, not on what the user saw |
| Outbox rows stuck in `sent`; a retry replays an archive after an undo | No atomic claim, no lease, no per-item order |
| `topics.list` 1.7 s, the Notes index ~2 s, embedding backfill O(N²) | Derived data recomputed from all history on every call |
| Every action reloads every view (~10 calls per refresh) | One global `ui.rev`, polled |

## Phase 0: time (done)

Built as planned, with `jiff` in place of chrono (`src/time.rs`, `src/clock.rs`, migration 21 in `src/utc_migration.rs`). Instants are stored as `YYYY-MM-DDTHH:MM:SS.sssZ`. Day-only dues moved to `items.due_day`. The migration converted the 200k-item scratch database (1.2M values) in 8 seconds. The plan as written:

- **Two kinds of time, two types.**
  - An **instant** (a message was sent, a meeting starts) is stored as UTC RFC 3339 (`2026-10-03T08:52:00Z`) and is `DateTime<Utc>` in Rust.
  - A **calendar day** (a Reflect day, a Notes page, a day-only Jira due date) stays a local `YYYY-MM-DD` and is `NaiveDate`.
  - A "today" query converts the local day's start and end to UTC instants and queries the range.
- **Connectors already get offsets or epochs** (Slack `ts`, Gmail `internalDate`, Jira and GitHub ISO with offset, gcal `dateTime` with offset), and today they throw the offset away. They keep it instead.
- **Column defaults** become `strftime('%Y-%m-%dT%H:%M:%SZ','now')` everywhere, not a mix of `datetime('now')` and `'localtime'`. `fake_now` shifts the app clock only, which feeds every write.
- **A data migration** parses each time column and rewrites it as UTC using the zone rules (the `chrono-tz` database). This is exact except inside the repeated hour of a DST fall-back, where a naive local time is ambiguous. Those rows take the first occurrence and are counted in the migration log.
- **The UI** formats instants in local time at the edge, and Rust stops formatting local strings for storage.
- **Proven by:** fixtures across a DST fall-back and a zone change (ordering, "today", day strip, Reflect day), plus a migration test on a copy of the sample data.

## Phase 1: domain models, repositories, unit of work

```
domain/         models (Person, Item, Topic, Link, OutboxOp…) and ports (traits): no SQL, no serde_json::Value
  ports.rs      trait People, Items, Links, Topics, Outbox, SyncState…; trait UnitOfWork { fn people(&self) -> &dyn People; … }
store/          the only module with SQL (a test fails on SQL anywhere else)
  db.rs         Db { writer, readers }: read(|r| …), write(|uow| …)
  people.rs     SqlPeople<'t> { tx }: impl People (get, update(&Person), …)
  views.rs      read models for screens: IncomingRow, TopicSummary, DayStripBlock…
triage/, sync/, ai/, …   application logic: models and ports only
api.rs          the middleware: opens a read or a unit of work per command, calls one service, commits or rolls back
```

- **Repositories take and return models.** `uow.people().update(&person)`, `uow.links().confirm(item, topic)`. SQL, row mapping and the storage format stay behind the port, so a backend can be swapped. People is a real case: a person is an Obsidian note mirrored into SQLite, and `People::update` writes both.
- **A unit of work per request, opened and closed by the edge, never by the application.**
  - `api.rs` is the middleware: each command runs its service inside `Db::write(|uow| …)` (`BEGIN IMMEDIATE`; commit if the service returns Ok, roll back on an error or a panic).
  - The other entry points are requests too, and open theirs the same way: each sync batch apply, each outbox send, each agent flush.
  - Services receive `&dyn UnitOfWork`, an object whose only methods return repositories. They never see `Db`, a connection or SQL.
  - Repositories never commit. A change within one aggregate is one repository method (a person and their accounts). A business action across aggregates (Confirm: link, item, outbox, held todo, todo) is one service, in one request, so it commits whole or not at all.
  - `Db` is the one instance per `App`. Repositories aren't process singletons: they're bound to the request's transaction, and the ~400 tests each run their own in-memory `App` in parallel.
- **`Db::read(|r| …)`** runs on a reader connection inside `BEGIN DEFERRED` (one consistent snapshot) and never waits on the writer (WAL).
- **Rules for writing models back:**
  - Get and update in the same unit of work; never hold a model across transactions and write it back whole.
  - Models edited in the UI carry their revision, and `update` refuses a stale one (the same compare-and-set as Phase 3).
- **Lists use read models, not aggregates.** Screens get dedicated queries returning view models; loading a full `Item` per Incoming row would be N+1. Writes always go through repositories.
- **No guard or connection leaves a closure**, so holding the database across a network call or file read isn't expressible by accident. In debug builds, a closure holding the writer for more than 100 ms is logged with its caller.
- **Traits are also kept for the outer ports:** HTTP, the OS (`Platform`), the clock, the AI model. Tests use in-memory SQLite behind the same ports, as now.
- **Migration path.** `app.db()` stays as a deprecated alias while modules move, and goes when nothing calls it. Order:
  1. hot reads (`incoming.list`, `topics.list`, day strip, search, `notes.page`) as read models;
  2. triage actions;
  3. sync apply and outbox;
  4. AI, Glean and agents.

  The UTC types from Phase 0 are what models carry.

### Phase 1 as built (2026-10-03)

- **`store::Db`** (`src/store/mod.rs`): one writer connection and an `r2d2` pool of read-only readers (none for an in-memory database, where reads use the writer).
  - `read(|r: &dyn Reads| …)` runs in `BEGIN DEFERRED` on a reader.
  - `write(|uow: &dyn UnitOfWork| …)` runs in `BEGIN IMMEDIATE`, commits on `Ok`, and rolls back on an error or a panic.
  - Debug builds log a write holding the writer over 100 ms, with its caller.
  - Taking the writer twice on one thread panics instead of deadlocking.
- **Ports per area** are in `domain/<area>.rs`, with `Reads` and `UnitOfWork` in `domain/ports.rs`. `store/<area>.rs` implements them on `Sql`, the connection in the request's transaction.
- **`api.rs` is the middleware.** A module's `route(name)` returns `Route::Read` or `Route::Write`, and `api.rs` opens the read or unit of work around it. The outbox is woken after each write.
  - A command that makes a network call or reads files has no route. It runs in the module's `call` and opens short reads and writes around that IO, never across it.
  - The pattern is two-phase: names come from one snapshot, the files are read with nothing open, then the result is built. `ai::context::Notes`, the embedder's topic text and Glean's entities all work this way.
  - The other edges open their own: a sync batch (`sync_source`), an outbox send (claim, send, record), an agent flush, an Agents-in-Incoming sync, and the sample-data seed.
- **Proofs** (`store::tests`):
  - `no_sql_outside_store`. Test modules may write fixtures directly.
  - `a_long_read_runs_while_a_write_commits_and_keeps_its_snapshot`.
  - `an_error_or_a_panic_rolls_the_whole_unit_of_work_back`.
  - `concurrent_glean_calls_never_pass_the_daily_limit`.
  - `queries::todos::a_confirm_that_fails_partway_changes_nothing`.
- **`app.db()` is gone from the app.** It, `Db::legacy` and `store::over` (ports over a bare connection) are `cfg(test)` and used only for fixtures and checks. The connection-taking helpers (`sync::apply_in`, `register_sources`, `load_source`, `split::*`, `people::scan`) are test-only too.
- **Fixed along the way:**
  - Actions commit whole (Confirm, Move, Dismiss, merges, sync follow-ups, an Agents-in-Incoming sync).
  - The outbox claim is atomic, and so is the Glean quota across processes.
  - Notes, topic notes and People notes are no longer read while the database is held.
  - Prompts, Ask Glean's evidence and the sample join times use local days.
  - Categories write `settings.json` after commit and revert the table if that write fails.
- **Different from the plan above:**
  - The Agents tab's models stay in `agents::model`; `domain::agents` uses them.
  - Some repositories still carry source fields as a JSON string (`raw_json`, `HostState.turn`).
  - Models edited in the UI don't carry a revision yet (Phase 3's compare-and-set).
  - The sample-data seed writes the stand-in vault's notes inside its unit of work. That happens once, on an empty database.
- **Measured on the 200k-item scratch database** (release, the same output as before):
  - `incoming.list`: 0.48 s, was 0.64 s.
  - `topics.list`: 0.97 s, was 1.03 s.
  - `notes.page`: about 2.1 s, unchanged; it waits on Phase 4.

## Phase 2: one worker per source; message upserts

- **A thread per enabled source** owns its sync. "Sync now", socket nudges and settings changes are messages to it. A restart joins the old worker first, so two polls of one source can't overlap.
- **`sync_streams(source_id, stream, state, error, failures, synced_at)`** replaces the cursor blob.
  - A poll commits stream by stream.
  - One stream's error is recorded on its row and the rest go on.
  - After N `not_found` failures, a tracked thread is dropped.
- **Overlap windows plus idempotent upserts** replace high-water marks on inputs that aren't strictly ordered (Slack search, GitHub and Jira queries).
- **`items.remote_rev`** decides "changed" (Jira `updated`/changelog id, GitHub `updated_at` + last comment id, gcal etag), plus a periodic re-fetch of known open items that left the query.
- **Messages upsert on `UNIQUE(item_id, ext_key)`.** Rows without an `ext_key` (15 in the sample data; real data unchecked) get a synthetic one in the migration. Caps apply only when reading. The UI and the AI split address messages by `ext_key`.

## Phase 3: triage against what you saw; outbox

- **`items.rev`** is maintained by a trigger, bumped whenever an item gains a message or becomes unseen. `incoming.list` returns it with the rendered message keys, and Confirm, Move, Dismiss and Trash send both back.
- **What triage does with them:**
  - It applies to the messages you saw.
  - If the rev moved, the item stays unseen with only the newer messages marked new.
  - Gmail files and archives only up to the rendered messages; Dismiss gets the same guard.
  - Slack's mark-read uses the newest rendered `ts`.
- **Outbox:**
  - Claim with `UPDATE … SET state = 'sent', lease_until = … WHERE id = (the oldest pending row with no earlier unfinished row for the same item) RETURNING …`. That's atomic and ordered per item.
  - At startup, an expired `sent` row goes back to `pending` if its op is idempotent, else to `failed` with "unknown whether it went through".
  - An `idem_key` on ops that create things.
  - Notes autosave merges only into rows still `pending`, in the same unit of work.

## Phase 4: derived data in the database; live queries

**Derived data, computed by SQLite.** SQLite has views, but not materialized ones: a view is its query, run on every read. So:

| Value | How | Measured (200k items, 140k links) |
|---|---|---|
| New signals per topic | View over unseen items (partial index) | 5 ms |
| Open owned items per topic | View over open items (partial index) | < 1 ms |
| Last signal per topic (all history) | `topics.last_signal_at`, kept by triggers on `topic_links` and `items` (an unlink recomputes that topic's max from an index) | 0.5 ms to read |
| Next meeting per topic | Query over the 7-day `events_start` range | ~190 ms; to be narrowed |
| Full-history pass for comparison | One `GROUP BY` over all links | 5.5 s; the correlated version is 1.7 s |

The triggers run inside the writing transaction, so the application makes one edit and the database keeps the derived value consistent with it. A change that misses a trigger is caught by a test comparing every maintained column against its from-scratch query on fixtures.

- **The Notes index** is cached in memory and keyed by the data version of the tables it reads (see below). File reads happen outside any lock.
- **The embedding queue:** triggers on item insert and on title/body updates add rows to `embed_dirty(kind, id)`, so the backfill is O(N) with no rescans.

**Live queries (subscriptions):** the Qt/Vue model at the database boundary, with dependencies discovered rather than declared.

- **The UI subscribes** to a named query with its arguments (`incoming.list {topic}`, `topics.list`, `daystrip.get {day}`…). The core runs it on a reader with SQLite's **authorizer** callback on, which reports every table the statement reads: that set is the subscription's dependency list (like Vue's dependency tracking on a render).
- **The writer's update hook** reports every table a transaction changed; the commit hook says it landed, and the rollback hook discards it. After each commit the core re-runs only the subscriptions that read a changed table. It pushes the new result to the UI (a Tauri event on desktop, a long-poll on the devserver) only if the result differs.
- **Granularity starts at the table.** That over-notifies, but each re-run is a fast query now, and row-level filters can come later where a profile shows it's worth it. Virtual tables (FTS, vectors) don't fire the update hook; only search reads them, and search isn't a live query.
- **In the frontend,** a subscription feeds a Svelte `$state` that components read. Svelte 5's runes are the same fine-grained model as Vue 3, so components re-render on their own. `ui.rev` and the 2–3 s polls go away.
- `incoming.list` gets paging, and messages and context load only for expanded rows.

rusqlite 0.40 exposes the authorizer and the update, commit and rollback hooks (feature `hooks`); the bundled SQLite is 3.53.

## Order and checks

| Phase | Proves it with |
|---|---|
| 0 Time | DST and zone-change fixtures; a migration test on the sample data |
| 1 Models, repositories | No SQL outside `store/` (test); a 2 s read running while a write commits; failure injected mid-action rolls everything back |
| 2 Sync workers, messages | Concurrent sync-now, loop and socket nudge on fixtures; one failing stream; a 60-message run keeps all 60 |
| 3 Triage, outbox | A message arriving mid-render; a crash between claim and send; a retry after an undo |
| 4 Derived data, live queries | Maintained columns equal their from-scratch queries; the scratch-DB benchmark (topics.list, notes.page < 50 ms); one action re-runs only the subscriptions that read what it changed |

Each phase lands as its own commits with its own tests; the app works between phases.
