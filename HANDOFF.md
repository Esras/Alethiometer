# Alethiometer: handoff for the next session

Read this first, then `app/README.md` and the docs it links. The original design handoff (mocks, chats, spec) is in `README.md`, `chats/` and `project/`; the spec is `project/design_handoff_helm/README.md`. The app was called Helm during design; it is now **Alethiometer**.

## Where things are

- Branch `claude/helm-ui-shell` holds all work. `main` holds only the design handoff.
- App: `app/` (Tauri 2, Rust core in `app/src-tauri`, Svelte 5 UI in `app/src`).
- Checks: `cd app/src-tauri && cargo test --no-default-features` (≈196 tests), `cargo build` (desktop; needs WebKitGTK dev libs on Linux), `cd app && npx svelte-check --tsconfig ./tsconfig.json`.
- Browser-mode dev: `ALETHIOMETER_FAKE_NOW=10:52 npm run devserver` + `npm run dev`; screenshots with `node scripts/shot.mjs out.png <tab> [keys…]`.
- Nothing has been run against real accounts. Every connector is tested with recorded-shape fixtures only.

## Built

- All five views (Incoming, Priorities, Topics, Notes, Reflect) matching the mocks, on SQLite with sample data (`demo_data: true`).
- Sync framework (`sync/mod.rs`), outbox writeback (`outbox.rs`), connectors: Google Calendar, Slack (Socket Mode + read markers), Jira, GitHub, Obsidian, Proton ICS (busy only). Setup per source in `app/docs/sources/`.
- Work AI on Bedrock Converse (`ai/`): small `us.openai.gpt-6-luna` (effort low), large `us.anthropic.claude-opus-5-5`; US regions and `us.` ids only. Ask pane, topic suggestions after sync, AI split of mixed signals.
- Splits: AI triage splits multi-topic signals; manual split in Unsorted (select messages, `t`).

## User decisions (don't re-ask)

- **BAA covers all of Bedrock.** Never question this again. US endpoints only.
- Bedrock credentials: long-lived IAM keys only, for now.
- Reflect is work-only (no Life/Both lens). Personal content never lands on the work Mac; Life = untitled busy blocks.
- Slack: signals follow Slack's read marker; websocket first, polling as catch-up; missing messages is the main concern, read-marker edge cases are not; marking read in the app marks read in Slack; mentions marked read up to the mention is fine; 3-hourly safety catch-up while connected.
- Splitting must not add steps to normal triage; manual split only in Unsorted; the AI triage should split mixed signals itself.
- Jira: no tagging issues (public record); dedupe by a JQL search; built-in due date is day-only; timed due fields show the time (option B); due items are never highlighted in the chronology.
- GitHub: one work org; token from `gh` (installed via Nix) is fine; mostly read-only (`read_only: true` available).
- Obsidian: edits can be anywhere in a note; go through the running Obsidian app (Local REST API plugin); prompting that Obsidian isn't open is fine. The vault syncs with a self-hosted sync plugin.
- Google sign-in: fine with re-auth every 7 days if needed; one Connect button saves the token.
- Name: Alethiometer.

## In flight / next

1. **Obsidian writes through the running app** (Local REST API plugin), in-place note editing in Topics, Reflect saves through the same path. A background agent was building this when the previous session ended; if it isn't merged on the branch, redo it from the notes in this file.
2. **Slack missed-message audit:** review every path where a message could fail to reach a signal (dropped Socket Mode events, the reconnect window, edits/deletes, old-thread replies, rate-limit backoff, paging limits, the 50-message cap per item, DMs beyond `max_dms` without a socket) and fix/test each.
3. UI for remaining writebacks: RSVP on invites in Incoming, Slack reply, Jira transitions/comments in Topics, ticking Obsidian tasks.
4. macOS pieces behind `platform.rs`: Keychain secrets, global capture hotkey, tray dot, persistent Reflect notification, launch at login.
5. First real-account run on the Mac: follow `app/docs/sources/*.md` and `app/docs/ai.md`.
