# Database and sync architecture: plan (for review)

Status: **proposal, not built.** Follows `REVIEW-2026-10-03.md`. The contained fixes from that review are in (commits `b2191b2`…`c3f2424`). This plan covers what's left that needs a structural change rather than a one-off fix.

Priorities: accuracy > performance, maintainability > speed of development.

## Problems this solves

| From the review | Root cause |
|---|---|
| UI stalls when a sync, the embedder or Notes holds the lock (seconds at scale) | One `Mutex<Connection>` shared by every thread and command |
| Partial writes: Confirm/Move/Dismiss commit half their work and then error | Multi-statement writes with no transaction; `app.db()` hands out a bare connection |
| Several syncs of one source at once; the cursor is last-writer-wins; a catch-up erases live syncs' progress | No owner per source; the cursor is one JSON blob rewritten whole |
| One bad Slack channel stops all of Slack | Polls are all-or-nothing |
| The 50-message cap deletes stored messages; message ids are reused so a split can move the wrong one | Messages are deleted and re-inserted on every sync |
| Confirm buries messages that arrived after render; Gmail archives past unread mail | Triage acts on the DB's current state, not on what the user saw |
| Outbox rows stuck in `sent` forever; a retry replays an archive after an undo | No atomic claim, no lease, no per-item order |
| `topics.list` 1.7 s, the Notes index ~2 s, embedding backfill O(N²) | Derived data recomputed from the whole corpus on each call |
| Every action reloads every view (~10 calls per refresh) | One global `ui.rev` |

## 1. `Db`: one writer, a pool of readers, closures only

```rust
pub struct Db { writer: Mutex<Connection>, readers: Pool /* 4 read-only WAL connections */ }

impl Db {
    /// A consistent snapshot (BEGIN DEFERRED … COMMIT); never blocks on the writer.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T>;
    /// BEGIN IMMEDIATE; commit on Ok, roll back on Err. Returns what changed (§6).
    pub fn write<T>(&self, f: impl FnOnce(&Tx) -> Result<T>) -> Result<T>;
}
```

- **No guard leaves a closure.** Holding the database across a network call or a file read becomes visibly wrong at the call site instead of easy. `#[track_caller]` plus a debug-build timer logs any closure that holds the writer for more than 100 ms, with its caller.
- **Every write is a transaction.** That alone fixes the "commits half, then errors" class: confirm, move, dismiss, trash, settle, and the outbox's confirm plus `on_confirmed`.
- **Reads don't wait for writes** (WAL). Incoming, Topics and search keep working while a sync applies a batch or the embedder runs.
- **The pool is hand-rolled** (about 60 lines, a `Mutex<Vec<Connection>>` with a condvar), not r2d2: no async, nothing else needed. sqlite-vec registers on every connection already (auto-extension).
- **Lock order** extends the agents rule: gate → writer → turn → store. A reader can be taken under anything, since it never waits on the writer.
- **Migration path.**
  - `app.db()` stays as a deprecated alias for the writer.
  - Modules move over one at a time, hot read paths first: `incoming.list`, `topics.list`, daystrip, search, `notes.page`, then the triage actions as `write`.
  - When nothing calls `app.db()` it's deleted.
  - Counting non-test call sites, 122 calls in 32 files.

## 2. One worker per source

- **A thread per enabled source** owns that source's sync. "Sync now", socket nudges and settings changes become messages on its channel (`Nudge`, `Now`, `Reconfigure(cfg)`, `Stop`). A restart joins the old worker before starting the new one.
  - Two polls of one source can no longer overlap.
  - The generation bookkeeping (`LIVE`, `GENERATIONS`, `RUNNING`) goes away.
- **Per-stream state rows** replace the single cursor blob:

  ```sql
  CREATE TABLE sync_streams (
    source_id INTEGER NOT NULL REFERENCES sources(id),
    stream    TEXT NOT NULL,      -- slack: channel id or channel/thread_ts; gmail: "history"; jira: "jql" …
    state     TEXT NOT NULL,      -- the stream's own cursor JSON
    error     TEXT, failures INTEGER NOT NULL DEFAULT 0, synced_at TEXT,
    PRIMARY KEY (source_id, stream)
  );
  ```

  - A poll applies and commits stream by stream.
  - One stream's error is recorded on that row and the rest go on.
  - After N `not_found` / `not_in_channel` failures, a tracked thread is dropped.
  - Connectors stay pure (a batch in, no database), as now.
- **Overlap windows instead of high-water marks** on inputs that aren't strictly ordered (Slack search, GitHub search, Jira JQL). Every apply is an idempotent upsert, so re-reading a window costs nothing but requests.
- **A remote version per item** (`items.remote_rev`: Jira `updated` or changelog id, GitHub `updated_at` + last comment id, gcal etag). `apply` decides "bump" by comparing against it, not the connector against a global cursor.
- **Reconcile known open items:** every Nth poll, re-fetch Jira and GitHub items still open locally that left the query.

## 3. Messages: upsert, never delete

- **Upsert on `UNIQUE(item_id, ext_key)`.** Rows without an `ext_key` (15 in the sample data; real data unchecked) get a synthetic one in the migration.
- **Caps apply at read time** (the UI's expanded view, AI context), never at storage. The Slack 50 cap stops deleting history.
- **The UI and the AI split address messages by `ext_key`**, so a sync can't make an id point at a different message.

## 4. Triage against what the user saw

- `items.rev` is bumped by a trigger whenever the item gains a message or becomes unseen. `incoming.list` returns it, and Confirm, Move, Dismiss and Trash send it back.
- **Same rev:** as today, in one `write`.
- **Changed rev (proposed):** apply to what was seen and keep the item in Incoming with the new messages marked new. For Gmail, file and archive only up to the messages that were rendered; Dismiss gets the same guard. Alternative: refuse with "new messages, look again". **This is your call (below).**
- The Slack mark-read and Gmail file payloads carry the rendered message keys, not the database's.

## 5. Outbox: claim, lease, order

- **Claim:** `UPDATE outbox SET state = 'sent', lease_until = now + 5 min WHERE id = (SELECT id … WHERE state = 'pending' AND NOT EXISTS (an earlier unfinished row for the same item) ORDER BY id LIMIT 1) RETURNING …`. That's atomic, and per-item ordered, so a retried archive can't land after a later restore.
- **At startup,** a `sent` row past its lease goes back to `pending` if its op is idempotent (mark-read, label, archive), else to `failed` with "unknown whether it went through".
- **An `idem_key`** for ops that create things (Jira create, Slack reply), so a retry checks first.
- **The Notes autosave** (which merges into a pending row) only merges into rows that are still `pending`, in the same `write`.

## 6. Derived state and change notification

- **`topic_stats`** (new count, open count, last signal, next meeting) is maintained in the same `write` that changes links or items, or `topics.list` is rewritten as one grouped pass if that measures well enough. I'll measure both on the scratch DB and keep the simpler one.
- **The Notes index** is cached in memory and invalidated by a `content_rev` that `write` bumps when links, topics or notes change. File reads happen outside any lock.
- **The embedding queue:** a trigger on items insert and on title/body updates adds rows to `embed_dirty(kind, id)`. The embedder drains it in batches with no full scans, so the backfill is O(N).
- **Change domains:** `write` closures declare what they touched (`incoming`, `topics`, `todos`, `daystrip`, `agents`…). After the commit the core emits them: a Tauri event on desktop, a long-poll endpoint on the devserver. The frontend replaces `ui.rev` with per-domain revisions and stops the 2–3 s polls. `incoming.list` gets paging and returns deltas after an action.

## 7. Later, separately: time in UTC

Timestamps are naive local strings, mixed with SQLite's UTC `'now'` in some columns. They misorder across DST fall-back and time-zone travel. The fix is a data migration (parse each column, store UTC RFC 3339, convert at the edges) that touches every query with a time comparison. It's worth doing before the corpus is years deep, but as its own change with its own tests.

## Order and checks

| Phase | Contents | Proves it with |
|---|---|---|
| 1 | `Db` read/write; hot reads on the pool; triage actions as `write`; lock-hold logging | A test where a 2 s read runs while a write commits; atomicity tests that inject a failure mid-action |
| 2 | Sync worker per source, `sync_streams`, message upsert | Concurrent sync-now plus loop plus socket nudge on fixtures; one failing stream; a 60-message run keeps all 60 |
| 3 | Triage `rev` compare-and-set; outbox claim, lease and order | Confirm with a message arriving mid-render; crash between claim and send; retry after undo |
| 4 | Derived state, embed queue, change domains, frontend scoped revisions and paging | The scratch-DB benchmark (targets: topics.list < 50 ms, notes.page < 50 ms, no full scan per 30 s); UI refresh call counts |
| 5 | UTC migration | DST and zone-change fixtures |

Phases 1–3 are what accuracy needs; 4 is what scale needs; 5 is what history needs. Each phase lands as its own commits with its own tests, and the app keeps working between them.

## Decisions for you

1. **Changed-rev triage (§4):** apply to what you saw and keep the new messages in Incoming (recommended), or refuse and make you look again?
2. **Change notification (§6):** push events from the core (recommended), or keep polling with per-domain revisions?
3. **UTC migration (§7):** after phases 1–4 (recommended), or sooner?
