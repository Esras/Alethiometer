# Database and sync architecture: plan

Status: **agreed direction, not built.** Follows `REVIEW-2026-10-03.md`. The contained fixes from that review are in (commits `b2191b2`…`c3f2424`); this covers what needs a structural change.

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

## Phase 0: time

- **Two kinds of time, two types.**
  - An **instant** (a message was sent, a meeting starts) is stored as UTC RFC 3339 (`2026-10-03T08:52:00Z`) and is `DateTime<Utc>` in Rust.
  - A **calendar day** (a Reflect day, a Notes page, a day-only Jira due date) stays a local `YYYY-MM-DD` and is `NaiveDate`.
  - A "today" query converts the local day's start and end to UTC instants and queries the range.
- **Connectors already get offsets or epochs** (Slack `ts`, Gmail `internalDate`, Jira and GitHub ISO with offset, gcal `dateTime` with offset), and today they throw the offset away. They keep it instead.
- **Column defaults** become `strftime('%Y-%m-%dT%H:%M:%SZ','now')` everywhere, not a mix of `datetime('now')` and `'localtime'`. `fake_now` shifts the app clock only, which feeds every write.
- **A data migration** parses each time column and rewrites it as UTC using the zone rules (the `chrono-tz` database). This is exact except inside the repeated hour of a DST fall-back, where a naive local time is ambiguous. Those rows take the first occurrence and are counted in the migration log.
- **The UI** formats instants in local time at the edge, and Rust stops formatting local strings for storage.
- **Proven by:** fixtures across a DST fall-back and a zone change (ordering, "today", day strip, Reflect day), plus a migration test on a copy of the sample data.

## Phase 1: storage layer, unit of work, repositories

```
store/          the only module with SQL (a test fails on SQL elsewhere)
  db.rs         Db { writer, readers }: read(|r| …), write(|uow| …)
  items.rs      ItemsRepo: get, upsert_from_sync, mark_seen_upto(rev), …
  links.rs      LinksRepo: suggest, confirm, move_to, wrong, …
  topics.rs, outbox.rs, sync_state.rs, people.rs, agents.rs, …
triage/, sync/, ai/, …   application logic: repositories only, no SQL
api.rs          each command opens a read or a unit of work and calls one service
```

- **`Db::write(|uow| …)`** is `BEGIN IMMEDIATE`, with commit on Ok and rollback on Err. A unit of work hands out the repositories bound to that transaction, so every multi-step action is atomic by construction. `Db::read(|r| …)` runs on a reader connection inside `BEGIN DEFERRED` (one consistent snapshot) and never waits on the writer (WAL).
- **No connection or guard leaves a closure**, so holding the database across a network call or file read isn't expressible by accident. In debug builds, a closure holding the writer for more than 100 ms is logged with its caller.
- **Repositories are concrete SQLite code**, with no trait per repository. Tests use in-memory SQLite, as now. Traits stay where there's a real second implementation: HTTP, the OS (`Platform`), the clock, the AI model. Those ports exist already; the connectors are pure and return a batch.
- **Migration path.** `app.db()` stays as a deprecated alias while modules move, and goes when nothing calls it. Order:
  1. hot reads (`incoming.list`, `topics.list`, day strip, search, `notes.page`);
  2. triage actions;
  3. sync apply and outbox;
  4. AI, Glean and agents.

  The UTC types from Phase 0 are what the repositories take and return.

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
| 1 Storage layer | No SQL outside `store/` (test); a 2 s read running while a write commits; failure injected mid-action rolls everything back |
| 2 Sync workers, messages | Concurrent sync-now, loop and socket nudge on fixtures; one failing stream; a 60-message run keeps all 60 |
| 3 Triage, outbox | A message arriving mid-render; a crash between claim and send; a retry after an undo |
| 4 Derived data, live queries | Maintained columns equal their from-scratch queries; the scratch-DB benchmark (topics.list, notes.page < 50 ms); one action re-runs only the subscriptions that read what it changed |

Each phase lands as its own commits with its own tests; the app works between phases.
