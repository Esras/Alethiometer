# Database and sync architecture: what's left

Phases 0–4 are built (2026-10-03). This file now lists only what's left and what was decided against. The full plan, how each phase was built, the measurements and the tests that prove each phase are in `git show 03a3899:app/docs/db-plan.md`.

## Left

- **Run it on the real database.** Migrations 23–32 have run on fixtures and scratch databases built from the sample data, never on the user's. On the first start, check how long the migrations take and that Incoming, Topics and Notes look the same afterwards. Message keys were checked on the sample data only. Real rows without an id get a hashed key, and a GitHub or Jira comment re-read later moves to its id key and keeps its read state.
- **Merge to `main`.** The work is on `claude/funny-faraday-mgdokr`, with `main` merged in.

## Waiting on a profile from real data

- **Paging `incoming.list`.** The list no longer carries message text. On a scratch database with 1,000 unseen signals it is 0.39 MB and re-runs in 0.13–0.15 s (debug build). Paging changes how j/k, scroll and counts behave, so build it only if the re-run shows up as lag.
- **Row-level live-query filters.** Any write to a table re-runs every subscription that read that table. That's fine at today's write rate. Filter by row only if a busy sync makes the re-runs show.

## Decided against

- **Revisions on models edited in the UI** (compare-and-set on save). Topic edits each write one field (`rename`, `set_category`, `set_archived`); nothing reads a whole model and writes it back. People rows are a mirror of the `People/` notes, and a note save already refuses a note that changed underneath it. Nothing in the background edits existing topics. A collision needs a second app on the same data directory, and even then the later write wins on one field.
- **`idem_key` on outbox ops.** The ops that are safe to resend already find what an earlier send made (a hold by its event id, a Jira issue by its summary). The rest (replies, comments, appends) have no API hook for one without changing what you wrote.
- **A gcal etag as the item revision.** Skipping unchanged events would also skip re-applying category colours.
- **A cursor per stream.** The cursor stays one document per source. The per-source worker makes each source single-writer, which is what the shared blob lacked, and `sync_streams` holds each stream's errors and failure counts.
- **Polls left as they are:** settings and the sync-status chip. They read in-memory state, not the database.

## Smaller open items

- **The sample-data seed writes vault files inside its database transaction.** It runs once, on an empty database, so nothing waits on it. If the seed grows, write the files first and then the rows (also in `ISSUES.md`).
- **Some repositories still carry source fields as a JSON string** (`raw_json`, `HostState.turn`) instead of typed fields.

## Where each phase lives (code comments refer to these)

- **Phase 0, time:** instants are UTC, days are local, and only `Clock` converts between them (`time.rs`, `clock.rs`, `utc_migration.rs`).
- **Phase 1, storage:** SQL only in `store/`, behind ports in `domain/`. `api.rs` opens one read snapshot or one unit of work per command. Commands that read files are `Route::ReadFiles` and take short snapshots around the file reads.
- **Phase 2, sync:** one worker per source (`sync/worker.rs`), `sync_streams`, `items.remote_rev`, messages upserted by key (`store/message_keys.rs`).
- **Phase 3, triage and outbox:** triage sends back the `rev` and message keys it showed, and applies only to those. Outbox sends are leased and ordered per item.
- **Phase 4, derived data and live queries:** views and triggers keep derived values (`maintained_values_match_their_from_scratch_queries`). Live queries are in `live.rs`, `store/changes.rs` and `lib/live.ts`. Incoming's expanded rows load their messages through `incoming.messages`.
