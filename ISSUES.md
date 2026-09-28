# Running issues

Found while wiring up connectors on the real Mac. Not in HANDOFF.md's "in flight" list — track separately until triaged in there.

## Open

### No way to dismiss a signal in Incoming

`queries/incoming.rs` only exposes `confirm` (needs a `topic_id`), `wrong`, `move`, `split`. There's no "drop this, I don't care" action that clears an item from Incoming without linking it to a topic — confirmed absent while reviewing the key table (`c`/`x`/`t`/`o` in `app/README.md`) against the actual command list. User: fine deferring this, tracked here for later.

### Topics created by the AI will need an archive/cleanup story

Added (see "Resolved" below): the small model can now propose brand-new topics, not just link into existing ones. Nothing prunes topics that turn out to be wrong, near-duplicates, or one-offs that seemed broad enough at the time but weren't — the `topics` table's `archived_at` column exists but nothing sets it anywhere in the codebase. User: "eventually" needed, not now. Revisit once topic volume from AI proposals is visible.

### `suggest_topics` failures are silently swallowed after every sync

`ai/suggest.rs::suggest_topics` catches its own errors (missing credentials, throttling, etc.) and returns `Ok(Outcome { status: "failed", message })` rather than an `Err` — documented behavior, see `docs/ai.md`'s "Topic suggestions" section: "It never returns an error." `docs/ai.md`'s own suggested scheduling snippet accounts for this by checking `o.status == "failed"` and logging it:

```rust
let out = crate::ai::with_live(&app, |d| crate::ai::suggest::suggest_topics(&app, d, 10));
if let Ok(o) = out { if o.status == "failed" { eprintln!("suggest: {:?}", o.message) } }
```

But the actual wiring in `sync/mod.rs::suggest_after_sync` doesn't do this — it goes through `ai::call(app, "suggest_topics", ...)`, which always wraps `Outcome` (even a `"failed"` one) in `Ok(json!(out))`. So the `if let Err(e)` check in `suggest_after_sync` never fires for this path, and a credentials/throttling failure produces zero output anywhere: not `eprintln`, not `sources.last_error`, not any UI-visible state.

Repro: sync a source with items and no Bedrock credentials configured. Sync succeeds, `topic_links`/`topics` stay empty, nothing in logs, nothing in `ai.status` (which only checks config shape, not live creds).

Fix direction: `suggest_after_sync` should inspect `Outcome.status` (per the doc's own snippet) and surface `"failed"` somewhere durable — at minimum `eprintln`, ideally something the UI/sync-status tab can show, since right now there's no way to distinguish "AI never ran," "AI ran and failed," and "AI ran and had nothing to suggest."

## Resolved

### The whole window froze on any slow command — the Tauri command handler was synchronous

The "UI freeze" reports (spinning cursor, no clicks registering anywhere in the window, not just on the button clicked) turned out to be a real main-thread block, not a missing loading indicator. `src-tauri/src/lib.rs`'s only Tauri command, `call`, was a plain synchronous `fn`. Per Tauri's own docs ("Asynchronous commands are preferred... to perform heavy work in a manner that doesn't result in UI freezes"), a non-`async` command runs on the thread handling IPC dispatch rather than a dedicated blocking pool — so any slow `api::call` (a GitHub sync, a Bedrock round-trip) blocked that thread and, with it, the window. The `SyncStatus.svelte` busy-indicator fix (previous entry) was real but treating a symptom; this is the actual cause. Fixed by making the command `async` and running the actual (synchronous, blocking) `api::call` via `tauri::async_runtime::spawn_blocking`, matching Tauri's documented pattern for wrapping non-async work.

### "Open in GitHub" (and "open in Obsidian") did nothing

`openExternal()` in `lib/api.ts` used `window.open(url, '_blank')`, which is a silent no-op in Tauri's WKWebView without the `opener` plugin and a matching capability — neither is configured (`capabilities/default.json` only grants `core:default` and window dragging). No error, no console warning, just nothing. The backend already had the right tool unused: `platform::Platform::open_url` (`open` on macOS, `xdg-open` on Linux), wired only to the Google OAuth consent flow. Added a generic `app.open_url` command that calls it, and pointed `openExternal()` at that instead of `window.open` — fixes all four call sites (Incoming, Priorities, Reflect's note editor, Topics' note pane) through the one shared helper.

### The small model could only link into existing topics, never propose a new one

`suggest_topics`'s prompt only ever offered a list of existing topics or null — there was no way for triage to surface "this looks like a new recurring subject." Added `new_topic` to the model's reply shape (2 to 5 words, validated server-side, dropped as "no suggestion" if the model ignores the constraint): when picked, `ai::suggest` resolves it via a shared `find_or_create` (case-insensitive reuse of an existing topic of the same name, else a new `topics` row, category `work`, no Obsidian note) and links the item as `suggested` exactly like any other pick — still not auto-confirmed. Scoped to the plain whole-item shape only; the multi-group split-by-topic shape still requires an existing topic or none, to avoid touching the trickier group-merge guardrails in `plan()` for comparatively little benefit right now.

### `prepare()` still skipped every item when zero topics existed, silently defeating the new-topic feature above

Found immediately after shipping the above: a fresh, real (non-demo) database has zero topics, and `prepare()` had `if candidates.is_empty() { continue; }` — a leftover optimization from when the model could only pick an existing topic, so "nothing to pick from" meant "don't bother asking." That's no longer true once the model can propose a brand-new topic, but the guard still fired first, so `suggest_topics` silently considered zero items (`considered: 0`, no error, nothing in any log) on every real sync. Removed the guard; the prompt now says "(none yet; propose a new_topic if...)" when the candidate list is empty. Added a regression test (`asks_the_model_even_with_zero_existing_topics`) that deletes all topics/topic_links/topic_repos/topic_people and asserts the model is still asked — this is exactly the gap the manual demo-seeded tests couldn't catch, since they always ran against pre-seeded topics.

### The manual "Sync" button had no pending/loading state

Same missing-feedback pattern as the assistant pane fix above: `SyncStatus.svelte`'s `syncNow()` awaited `sync.now` with no disabled/loading indicator on the button. With `sync.now` now unconditionally running `suggest_after_sync` too (see above), a manual sync click could block for several seconds (GitHub fetch + up to 10 sequential Bedrock calls) with zero visual feedback — reported as "freeze when clicking sync." Added a `syncing` state that disables the button and shows "Syncing…" while in flight.

### Bedrock only supported IAM access-key/secret (SigV4); the user's account uses a Bedrock API key (bearer token)

`ai/creds.rs` only resolved classic IAM `aws_access_key_id`/`aws_secret_access_key` and signed every call with SigV4. AWS also has Bedrock API keys (bearer tokens, `AWS_BEARER_TOKEN_BEDROCK` / `Authorization: Bearer <key>`, no signing) - that's what was actually provisioned. Added `AwsCreds::Bearer` alongside `AwsCreds::Keys`; `creds::resolve` checks `AWS_BEARER_TOKEN_BEDROCK` env and secret `bedrock_api_key` first, falling back to the existing IAM-key chain. `bedrock::converse` sends `Authorization: Bearer <token>` directly for bearer creds, skipping `sign_headers` entirely. See `docs/ai.md` section 2.

### `ai.*` failures from the UI (ask, etc.) weren't logged anywhere

`ai.ask`'s error went straight from `with_live(...)?` up through `api::call` to the UI as a one-shot string — nothing durable recorded it (no request metadata, no history). Found while debugging a live 503 from Bedrock: once the UI toast cleared there was nothing left to go on. Fixed by logging any `ai.*` failure at `api::call`'s single dispatch point (used by both the Tauri command and devserver), so this covers every current and future `ai.*` command, not just `ask`. Distinct from the `suggest_topics` issue above, which goes through `sync::suggest_after_sync` — a different call path that still swallows failures; not fixed by this change.

### No production code path created a `topics` row

Every `INSERT INTO topics` was inside `#[cfg(test)]` or `seed.rs` (demo data); `obsidian::after_apply` only matched existing topics by `obsidian_path`, never created one. Decided: a UI-driven create action rather than Obsidian auto-creation, since the latter needs Obsidian wired up first. Added `topics.create` (name + optional category, defaults to `work`; case-insensitive dedup against active topics; validates the category exists) and wired it into `TopicPicker.svelte`'s existing empty state ("No topic matches" → "Create ‘…’" when nothing matches, `t` in Incoming). Obsidian-driven auto-creation from new vault notes is still unaddressed — revisit once Obsidian is connected.

### GitHub's `review-requested:@me` search includes team-based review requests, not just individual ones

GitHub's search qualifier matches any PR where you (or a team you're in) was requested to review — there's no way to exclude team-only hits in the search query itself. The PR detail endpoint (`GET /repos/{repo}/pulls/{n}`) does distinguish: `requested_reviewers` (individuals with a pending request) vs `requested_teams`. Added `team_review_requests` (default `true`, matching prior behavior) to source config: when `false`, a PR found *only* via `review-requested:@me` (not also via `involves:@me` — i.e. no mention/assignment/comment/authorship of yours) and not individually in `requested_reviewers` is dropped entirely rather than surfaced as "review requested" noise. See `docs/sources/github.md`.

### `suggest_topics` only ran on syncs that brought brand-new items, never against an existing Incoming backlog

`sync::start`'s loop only called `suggest_after_sync` when `sync_source` returned `n > 0` new items that cycle. `suggest_topics`'s own query (`unsorted()`) already correctly scopes to everything with `seen = 0` and no confirmed/wrong topic link, regardless of age — so this was purely an outer gating bug. Concretely: GitHub's first sync (82 items) ran before Bedrock credentials existed, so every suggestion attempt failed; every sync since then found 0 *new* items, so `suggest_after_sync` was never called again and the entire backlog sat untriaged. Fixed by calling `suggest_after_sync` on every successful sync, not just ones with new items.

Trade-off worth watching: `docs/ai.md`'s own "Topic suggestions" section already flags that items with no confident pick get "asked again on later runs" with no de-dup ("If that repeats too often, track 'last asked' (open question below)"). Removing the `n > 0` gate means that now happens on every poll cycle (e.g. every 300s for GitHub) instead of only when new items arrive, so a backlog item that never gets a confident pick will re-cost a small-model call every cycle indefinitely. Not fixed here; revisit if API cost/traffic becomes noticeable.

**Follow-up**: the manual `sync.now` command (the "Sync" button in the tab bar's sources popover) has its own separate `if n > 0` gate that this fix missed entirely — found because the user kept clicking that button and still saw no suggestions after the first fix. Removed there too.
