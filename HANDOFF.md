# Alethiometer: handoff for the next session

Read this first, then `app/README.md` and the docs it links. The original design handoff (mocks, chats, spec) is in `README.md`, `chats/` and `project/`; the spec is `project/design_handoff_helm/README.md`. The app was called Helm during design; it is now **Alethiometer**.

## Where things are

- **`main` now holds all work** (app + design handoff; design updates land there too). `claude/helm-ui-shell` is stale at `291d96f`. Work on `main` — an earlier session silently ran on an out-of-date checkout, check `git status`/`git log` first.
- App: `app/` (Tauri 2, Rust core in `app/src-tauri`, Svelte 5 UI in `app/src`).
- **Devshell**: `flake.nix`/`devshell.toml`/`.envrc` at repo root (numtide/devshell, same pattern as `k8s-iac`). `direnv allow` once, then `cargo`/`node`/`pkg-config`/`openssl` resolve inside the repo — nothing installed system-wide. `language.rust` module handles Darwin `*-sys` crate linking (frameworks, `xcbuild` toolchain on PATH).
- Checks: `cd app/src-tauri && cargo test --no-default-features` (≈230 tests), `cargo build` (desktop; needs WebKitGTK dev libs on Linux, nothing extra on macOS), `cd app && npx svelte-check --tsconfig ./tsconfig.json`.
- Browser-mode dev: `ALETHIOMETER_FAKE_NOW=10:52 npm run devserver` + `npm run dev` (set `ALETHIOMETER_DATA_DIR` to a temp dir to keep off the real DB); screenshots with `node scripts/shot.mjs out.png <tab> [keys…]` — needs Playwright, which the devshell doesn't have yet. Desktop dev: `npx tauri dev` (real window, real IPC; its watcher rebuilds/relaunches on Rust changes, Vite hot-reloads Svelte).
- Real data dir on the Mac: `~/Library/Application Support/dev.alethiometer.app` (`settings.json`, `secrets.json` mode 0600, `alethiometer.sqlite`). Keychain isn't wired, secrets live in the file.
- **`ISSUES.md`** (repo root): running list of bugs/gaps found wiring up real accounts on the Mac, separate from this file's "in flight" list until triaged in here. Check it before starting more connector work — several are load-bearing (the Tauri command was synchronous and froze the whole window on any slow call; `suggest_topics` had two separate gating bugs that made it silently do nothing).
- **Live on the work Mac: GitHub, Bedrock AI, Google Calendar, Jira, Gmail, Obsidian.** Slack and Proton ICS are still fixture-tested only.

## Built

- All five views (Incoming, Priorities, Topics, Notes, Reflect) matching the mocks, on SQLite with sample data (`demo_data: true`) or real data (`demo_data: false`).
- Sync framework (`sync/mod.rs`), outbox writeback (`outbox.rs`), connectors: Google Calendar, Slack (Socket Mode + read markers), Jira, GitHub, Obsidian, Proton ICS (busy only). Setup per source in `app/docs/sources/`.
- **GitHub: live and verified** on a real account (`gh` CLI token, read-only). `team_review_requests: false` (new) drops PRs where you were only pulled in via a team's review request, not a real mention/assignment/comment/authorship — GitHub's `review-requested:@me` search matches team membership, there's no way to exclude it in the query itself; distinguished via the PR detail endpoint's `requested_reviewers` vs `requested_teams`.
- **Work AI: live and verified** on Bedrock Converse (`ai/`) — real `ai.ask` round-trip confirmed against real GitHub data. Small `us.openai.gpt-6-luna` (effort low), large `us.anthropic.claude-opus-5-5`; US regions and `us.` ids only. Ask pane, topic suggestions after every sync (not just ones with new items), AI split of mixed signals.
- **Credentials: a Bedrock API key (bearer token) is the primary path**, not classic IAM access-key/secret — `AwsCreds::Bearer` alongside `AwsCreds::Keys` in `ai/creds.rs`; checks `AWS_BEARER_TOKEN_BEDROCK`/secret `bedrock_api_key` before the IAM-key chain. See `docs/ai.md` §2.
- **Topics can now be created two ways**: manually (`topics.create`, wired into `TopicPicker.svelte`'s "Create '…'" when nothing matches, `t` in Incoming), or **proposed by the small model itself** during triage (`new_topic` in the suggestion reply shape: 2 to 5 words, broad, validated server-side, dedup'd case-insensitively against existing topics) — still lands as `suggested`, never auto-confirmed. Splits: AI triage splits multi-topic signals (new-topic proposals not supported in that shape yet, see `ISSUES.md`); manual split in Unsorted (select messages, `t`).
- **Google Calendar: live.** OAuth client lives in an existing Internal tooling GCP project (not a personal one). First sync arrives read except invites still awaiting your answer. Recurring series: Incoming shows one row per series (next upcoming unseen instance; `sync::collapse_series`), confirm/move files every instance (`incoming::confirm_series`), new instances inherit the series' links and stay quiet (`sync::inherit_series`); topic timeline shows past instances plus only the next future one. Instances past `days_ahead` are dropped (incremental `syncToken` syncs return every instance of an edited series unbounded — had filled the DB to 2051). SyncStatus Connect waits for consent, then syncs.
- **Jira: live** (`qventus.atlassian.net`, classic API token in `jira_api_token`). JQL = yours (assignee/reporter/watcher) OR created in DEVOPS/IT/ITP2 in the last 7 days; Notes → Jira creates in DEVOPS.
- **Incoming**: dismiss (`d`/`u`, 13a; flips `seen`, no column, nothing written remotely; optimistic). **Updates on filed items**: unseen items with a `confirmed` link show under their topic with **Seen** instead of Confirm. Views reload when any source's `last_sync_at` changes (SyncStatus's 30 s poll); `suggest_after_sync` re-stamps it so AI suggestions show once the run finishes. Topics that get their first new signal join at the end (above Unsorted). Titles wrap when expanded, tooltips when truncated.
- **Gmail: live.** Every unread Inbox thread is a signal (first sync included); read/archived in Gmail → seen. Confirm/Seen/Move queue `gmail.mark_read` (skips if newer unread mail arrived); Dismiss never writes. Shares the calendar's OAuth client (`gcal.client_*`), own `gmail.refresh_token`, scope `gmail.modify`. See `docs/sources/gmail.md`.
- **Topics index (12c)** built: filter (`/`; no auto-focus — user uses j/k), Active/Archived, archive (`a`), quiet section (`quiet_days`), next-meeting pills (own thresholds; the strip countdown keeps its 15/5/2 — user: different information), category chips + `c` picker (type-to-filter), New topic (`n`, inline name field), assistant pane scoped to all topics (`ai.ask` scope `topics`). Scroll/selection/filter survive opening a topic.
- **Categories** are a DB table (`categories`, migration 0004), seeded once from `settings.categories`; managed in Topics index → Categories. `work`/`life` built in; delete refused while in use. The classification model suggests a category for new topics.
- **Markdown**: `lib/ui/Markdown.svelte`, a small element-building renderer (no `{@html}`) for AI answers and Incoming message text. The ask prompt allows light Markdown.
- `ui_scale` in settings.json: native webview zoom (`set_zoom`), read at startup; user runs 1.15.
- `app.open_url` command (backend `platform::Platform::open_url`, `open`/`xdg-open`) wired to `openExternal()` — "open in GitHub"/"open in Obsidian" links actually open something now; `window.open()` is a silent no-op in Tauri's WKWebView without a plugin this app doesn't have.

## User decisions (don't re-ask)

- **BAA covers all of Bedrock.** Never question this again. US endpoints only.
- **Bedrock credentials: a Bedrock API key (bearer token) is what's actually provisioned**, not a classic IAM access-key/secret pair — supersedes the old "long-lived IAM keys only" note. Both paths are supported; bearer is checked first. Long-lived (not short-term/session) either way.
- Reflect is work-only (no Life/Both lens). Personal content never lands on the work Mac; Life = untitled busy blocks.
- Slack: **reading something in Slack never removes it from Incoming.** Only triage in Alethiometer (confirm, move or dismiss) closes a signal. Slack's read marker may decide which messages count as new, but it never closes or shrinks an item. The other direction stays as it is: triaging here marks the item read in Slack, and marking mentions read up to the mention is fine. Your own messages belong in the signal as context. Websocket first, polling as catch-up. Missing messages is the main concern. A safety catch-up runs every 3 hours while connected.
- Splitting must not add steps to normal triage, and the AI splits mixed signals itself. Manual splitting of *unsplit* signals happens in Unsorted. Two exceptions, both from the design: (a) the split review (15g/15h) lets you correct an AI split in Incoming, and (b) `s` on a confirmed signal in the chronology or search (18c/18d) opens the same review.
- Jira: no tagging issues (public record); dedupe by a JQL search; built-in due date is day-only; timed due fields show the time (option B); due items are never highlighted in the chronology.
- GitHub: one work org; token from `gh` (installed via Nix) is fine; mostly read-only (`read_only: true` available); **team-only review requests are noise, filter them out** (`team_review_requests: false`).
- Obsidian: edits can be anywhere in a note; go through the running Obsidian app (Local REST API plugin); prompting that Obsidian isn't open is fine. The work vault is **work-only and doesn't sync to any external system** (the self-hosted sync plugin is the user's separate personal vault, not part of this app). Person notes in `People/` hold coworker details and stay in this vault.
- Google sign-in: fine with re-auth every 7 days if needed; one Connect button saves the token.
- **AI-proposed topics should be 2 to 5 words and fairly broad** (a recurring subject, not a one-off event's title).
- **Dismissing a signal in Incoming without linking it to a topic**: built (design 13a, `d`/`u`).
- Calendar: quiet first sync (only pending invites surface); one Incoming row per recurring series, but the topic timeline shows individual instances (past + next upcoming only). Horizon trickle of new instances is fine.
- GCP: no personal projects (user blocks project creation org-wide); reuse existing Internal tooling project.
- Jira: assigned/reported/watched issues are usually already filed — their updates should still be *seen* in Incoming, not re-triaged. New issues in DEVOPS/IT/ITP2 (projects the user owns, small volume) should surface. Missing creations while the app is closed >7 days is fine.
- Updates to filed things must show in Incoming. Slack's design has landed (design README, "Slack split review", data changes 1 and 1b). Before triage, new messages join the open signal. After triage, the next message starts a new item, which the updates mechanism shows under its topic with **Seen**.
- Background AI suggestions should appear promptly, and new topics shouldn't jump to the top of the list.
- Name: Alethiometer.
- **Reading anything in its own app never removes it from Incoming. This applies to every source, not just Slack.** Gmail: reading or archiving a thread in Gmail must not mark it seen here. Alethiometer is meant to be the main way the user works through Gmail, so archive, trash and labels will be actions here (see ISSUES.md "Gmail writebacks"). The other direction stays: triage here writes back to Gmail.
- **Incoming is where everything gets viewed.** No message is marked seen automatically. An update on a filed item stays in Incoming until the user acts on it here (Seen, Move or Dismiss).

## In flight / next

**Priorities (user, 2026-09-28), in order:**

1. **Slack**: design data changes 1, 1b, 2, 3, 4 are built (triage closes a run via `slack::close_triaged` before each sync; context messages in `item_messages.context`; sender id in `item_messages.author_id`; Gmail no longer marks items seen when read in Gmail). Next: go live on the work Mac, then the split review UI (15g/15h) and person colours (16c), then the missed-message audit (below). Interpretation to confirm with the user: after triage, a thread's next replies come back on the same item (so it shows under its topic as an update); a top-level run starts a new item that the AI classifies.
2. **Gmail writebacks**: archive, trash, labels↔categories. See `ISSUES.md` Open.
3. 13b (AI failure panel) exists as a design and relates to the open `suggest_topics` swallowed-failure issue.

Slack and Gmail can only be tested with fixtures outside the user's Mac.

**Also open:**

- Questions for the user: should Topics' "Upcoming" (next 7 days) also show only the next instance of a series? Should plain Jira comments by others count as updates?
- Watch UI lag/CPU. A dismiss-triggered `refresh()` loop (an effect depending on `ui.rev`) was the dismiss lag and a ~150% CPU spike; fixed with `untrack`. Any `$effect` that calls `refresh()` must `untrack` it. If lag reappears, suspect contention on the single `Mutex<Connection>` shared by sync threads and UI calls, or the full `incoming.list` reload after each action.
- **Slack missed-message audit:** review every path where a message could fail to reach a signal (dropped Socket Mode events, the reconnect window, edits/deletes, old-thread replies, rate-limit backoff, paging limits, the 50-message cap per item, DMs beyond `max_dms` without a socket) and fix/test each.
- **Obsidian (live):** connected to the work vault; a Topics note save through the plugin is verified. Writes go through the running Obsidian app via the Local REST API plugin (pinned to its own CA; `write_mode: "file"` fallback). Open questions: dismissing a conflicted save after "reload", spurious conflicts if Obsidian hasn't flushed to disk yet, custom auth header names.
- UI for remaining writebacks: RSVP on invites in Incoming (designed, 14a/14b, design README "RSVP"), Slack reply, Jira transitions/comments in Topics, ticking Obsidian tasks.
- macOS pieces behind `platform.rs`: Keychain secrets, global capture hotkey, tray dot, persistent Reflect notification, launch at login.
- Real-account runs still to do: Slack, Proton ICS.
- Reclassifying a single signal is designed (18c/18d). 
- Wanted later: topic merge/reclassification (the Topics assistant can only advise); assistant help writing Gmail filters (needs `gmail.settings.basic` + assistant actions).
- Check `ISSUES.md`'s "Open" section before starting connector work.

