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
- **Live on the work Mac: GitHub, Bedrock AI, Google Calendar, Jira, Gmail.** Slack, Obsidian, Proton ICS are still fixture-tested only.

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
- Slack: signals follow Slack's read marker; websocket first, polling as catch-up; missing messages is the main concern, read-marker edge cases are not; marking read in the app marks read in Slack; mentions marked read up to the mention is fine; 3-hourly safety catch-up while connected.
- Splitting must not add steps to normal triage; manual split only in Unsorted; the AI triage should split mixed signals itself.
- Jira: no tagging issues (public record); dedupe by a JQL search; built-in due date is day-only; timed due fields show the time (option B); due items are never highlighted in the chronology.
- GitHub: one work org; token from `gh` (installed via Nix) is fine; mostly read-only (`read_only: true` available); **team-only review requests are noise, filter them out** (`team_review_requests: false`).
- Obsidian: edits can be anywhere in a note; go through the running Obsidian app (Local REST API plugin); prompting that Obsidian isn't open is fine. The vault syncs with a self-hosted sync plugin.
- Google sign-in: fine with re-auth every 7 days if needed; one Connect button saves the token.
- **AI-proposed topics should be 2 to 5 words and fairly broad** (a recurring subject, not a one-off event's title).
- **Dismissing a signal in Incoming without linking it to a topic**: built (design 13a, `d`/`u`).
- Calendar: quiet first sync (only pending invites surface); one Incoming row per recurring series, but the topic timeline shows individual instances (past + next upcoming only). Horizon trickle of new instances is fine.
- GCP: no personal projects (user blocks project creation org-wide); reuse existing Internal tooling project.
- Jira: assigned/reported/watched issues are usually already filed — their updates should still be *seen* in Incoming, not re-triaged. New issues in DEVOPS/IT/ITP2 (projects the user owns, small volume) should surface. Missing creations while the app is closed >7 days is fine.
- Updates to filed things (GitHub comments, Slack messages, Jira changes) must show in Incoming. **Slack's behavior here is being designed by the user in a separate Design thread — don't build it ahead of that.**
- Background AI suggestions should appear promptly, and new topics shouldn't jump to the top of the list.
- Name: Alethiometer.

## In flight / next

0. **Next up:** 13b (AI failure panel) exists as a design and relates to the open `suggest_topics` swallowed-failure issue. Wanted later: topic merge/reclassification (the Topics assistant can only advise), assistant help writing Gmail filters (needs `gmail.settings.basic` + assistant actions).
0a. Open questions for the user: should Topics' "Upcoming" (next 7 days) also show only the next instance of a series? Should plain Jira comments by others count as updates?
0b. Watch: UI lag/CPU. A dismiss-triggered `refresh()` loop (effect depending on `ui.rev`) was the dismiss lag and a ~150% CPU spike; fixed with `untrack`. If lag reappears, suspect contention on the single `Mutex<Connection>` shared by sync threads and UI calls, or the full `incoming.list` reload after each action. Any `$effect` that calls `refresh()` must `untrack` it.
1. **Obsidian (done, untested live):** writes go through the running Obsidian app via the Local REST API plugin (pinned to its own CA; `write_mode: "file"` fallback). `e` edits a topic note in place; Reflect saves the same way. Open questions: dismissing a conflicted save after "reload", spurious conflicts if Obsidian hasn't flushed to disk yet, custom auth header names.
2. **Slack missed-message audit:** review every path where a message could fail to reach a signal (dropped Socket Mode events, the reconnect window, edits/deletes, old-thread replies, rate-limit backoff, paging limits, the 50-message cap per item, DMs beyond `max_dms` without a socket) and fix/test each.
3. UI for remaining writebacks: RSVP on invites in Incoming, Slack reply, Jira transitions/comments in Topics, ticking Obsidian tasks.
4. macOS pieces behind `platform.rs`: Keychain secrets, global capture hotkey, tray dot, persistent Reflect notification, launch at login.
5. **First real-account run on the Mac: done for GitHub, Bedrock AI, Google Calendar, Jira.** Still to do: Slack (waiting on the user's design), Obsidian, Proton ICS.
6. **Archive topics** (cleanup for AI-proposed topics that turn out wrong/duplicate/one-off) — wanted, deferred; see `ISSUES.md`. Dismiss is built.
7. Check `ISSUES.md`'s "Open" section before starting more connector work; several found bugs there (the swallowed `suggest_topics` failures, the still-synchronous split-shape new-topic gap) are directly relevant to whatever's touched next.
