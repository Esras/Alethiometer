# Alethiometer: handoff for the next session

Read this first, then `app/README.md` and the docs it links. The original design handoff (mocks, chats, spec) is in `README.md`, `chats/` and `project/`; the spec is `project/design_handoff_helm/README.md`. The app was called Helm during design; it is now **Alethiometer**.

## Where things are

- Branch `claude/helm-ui-shell` holds all work. `main` holds only the design handoff.
- App: `app/` (Tauri 2, Rust core in `app/src-tauri`, Svelte 5 UI in `app/src`).
- **Devshell**: `flake.nix`/`devshell.toml`/`.envrc` at repo root (numtide/devshell, same pattern as `k8s-iac`). `direnv allow` once, then `cargo`/`node`/`pkg-config`/`openssl` resolve inside the repo — nothing installed system-wide. `language.rust` module handles Darwin `*-sys` crate linking (frameworks, `xcbuild` toolchain on PATH).
- Checks: `cd app/src-tauri && cargo test --no-default-features` (≈224 tests), `cargo build` (desktop; needs WebKitGTK dev libs on Linux, nothing extra on macOS), `cd app && npx svelte-check --tsconfig ./tsconfig.json`.
- Browser-mode dev: `ALETHIOMETER_FAKE_NOW=10:52 npm run devserver` + `npm run dev`; screenshots with `node scripts/shot.mjs out.png <tab> [keys…]`. Desktop dev: `npx tauri dev` (real window, real IPC).
- **`ISSUES.md`** (repo root): running list of bugs/gaps found wiring up real accounts on the Mac, separate from this file's "in flight" list until triaged in here. Check it before starting more connector work — several are load-bearing (the Tauri command was synchronous and froze the whole window on any slow call; `suggest_topics` had two separate gating bugs that made it silently do nothing).
- GitHub + Bedrock AI have now run against real accounts on the work Mac (see below). Every other connector is still tested with recorded-shape fixtures only.

## Built

- All five views (Incoming, Priorities, Topics, Notes, Reflect) matching the mocks, on SQLite with sample data (`demo_data: true`) or real data (`demo_data: false`).
- Sync framework (`sync/mod.rs`), outbox writeback (`outbox.rs`), connectors: Google Calendar, Slack (Socket Mode + read markers), Jira, GitHub, Obsidian, Proton ICS (busy only). Setup per source in `app/docs/sources/`.
- **GitHub: live and verified** on a real account (`gh` CLI token, read-only). `team_review_requests: false` (new) drops PRs where you were only pulled in via a team's review request, not a real mention/assignment/comment/authorship — GitHub's `review-requested:@me` search matches team membership, there's no way to exclude it in the query itself; distinguished via the PR detail endpoint's `requested_reviewers` vs `requested_teams`.
- **Work AI: live and verified** on Bedrock Converse (`ai/`) — real `ai.ask` round-trip confirmed against real GitHub data. Small `us.openai.gpt-6-luna` (effort low), large `us.anthropic.claude-opus-5-5`; US regions and `us.` ids only. Ask pane, topic suggestions after every sync (not just ones with new items), AI split of mixed signals.
- **Credentials: a Bedrock API key (bearer token) is the primary path**, not classic IAM access-key/secret — `AwsCreds::Bearer` alongside `AwsCreds::Keys` in `ai/creds.rs`; checks `AWS_BEARER_TOKEN_BEDROCK`/secret `bedrock_api_key` before the IAM-key chain. See `docs/ai.md` §2.
- **Topics can now be created two ways**: manually (`topics.create`, wired into `TopicPicker.svelte`'s "Create '…'" when nothing matches, `t` in Incoming), or **proposed by the small model itself** during triage (`new_topic` in the suggestion reply shape: 2 to 5 words, broad, validated server-side, dedup'd case-insensitively against existing topics) — still lands as `suggested`, never auto-confirmed. Splits: AI triage splits multi-topic signals (new-topic proposals not supported in that shape yet, see `ISSUES.md`); manual split in Unsorted (select messages, `t`).
- `app.open_url` command (backend `platform::Platform::open_url`, `open`/`xdg-open`) wired to `openExternal()` — "open in GitHub"/"open in Obsidian" links actually open something now; `window.open()` is a silent no-op in Tauri's WKWebView without a plugin this app doesn't have.

## User decisions (don't re-ask)

- **BAA covers all of Bedrock.** Never question this again. US endpoints only.
- **Bedrock credentials: a Bedrock API key (bearer token) is what's actually provisioned**, not a classic IAM access-key/secret pair — supersedes the old "long-lived IAM keys only" note. Both paths are supported; bearer is checked first. Long-lived (not short-term/session) either way.
- Reflect is work-only (no Life/Both lens). Personal content never lands on the work Mac; Life = untitled busy blocks.
- Slack: **reading something in Slack never removes it from Incoming.** Only triage in Alethiometer (confirm, move or dismiss) closes a signal. Slack's read marker may decide which messages count as new, but it never closes or shrinks an item. The other direction stays as it is: triaging here marks the item read in Slack, and marking mentions read up to the mention is fine. Your own messages belong in the signal as context. Websocket first, polling as catch-up. Missing messages is the main concern. A safety catch-up runs every 3 hours while connected.
- Splitting must not add steps to normal triage; manual split only in Unsorted; the AI triage should split mixed signals itself.
- Jira: no tagging issues (public record); dedupe by a JQL search; built-in due date is day-only; timed due fields show the time (option B); due items are never highlighted in the chronology.
- GitHub: one work org; token from `gh` (installed via Nix) is fine; mostly read-only (`read_only: true` available); **team-only review requests are noise, filter them out** (`team_review_requests: false`).
- Obsidian: edits can be anywhere in a note; go through the running Obsidian app (Local REST API plugin); prompting that Obsidian isn't open is fine. The vault syncs with a self-hosted sync plugin.
- Google sign-in: fine with re-auth every 7 days if needed; one Connect button saves the token.
- **AI-proposed topics should be 2 to 5 words and fairly broad** (a recurring subject, not a one-off event's title).
- **Dismissing a signal in Incoming without linking it to a topic is wanted, but deferred** — tracked in `ISSUES.md`, not built yet.
- Name: Alethiometer.

## In flight / next

1. **Obsidian (done, untested live):** writes go through the running Obsidian app via the Local REST API plugin (pinned to its own CA; `write_mode: "file"` fallback). `e` edits a topic note in place; Reflect saves the same way. Open questions: dismissing a conflicted save after "reload", spurious conflicts if Obsidian hasn't flushed to disk yet, custom auth header names.
2. **Slack missed-message audit:** review every path where a message could fail to reach a signal (dropped Socket Mode events, the reconnect window, edits/deletes, old-thread replies, rate-limit backoff, paging limits, the 50-message cap per item, DMs beyond `max_dms` without a socket) and fix/test each.
3. UI for remaining writebacks: RSVP on invites in Incoming, Slack reply, Jira transitions/comments in Topics, ticking Obsidian tasks.
4. macOS pieces behind `platform.rs`: Keychain secrets, global capture hotkey, tray dot, persistent Reflect notification, launch at login.
5. **First real-account run on the Mac: done for GitHub + Bedrock AI**, per `app/docs/sources/*.md` and `app/docs/ai.md`. Still to do: Google Calendar, Slack, Jira, Obsidian, Proton ICS.
6. **Dismiss a signal** (Incoming) and **archive topics** (cleanup for AI-proposed topics that turn out wrong/duplicate/one-off) — both wanted, both deferred; see `ISSUES.md`.
7. Check `ISSUES.md`'s "Open" section before starting more connector work; several found bugs there (the swallowed `suggest_topics` failures, the still-synchronous split-shape new-topic gap) are directly relevant to whatever's touched next.
