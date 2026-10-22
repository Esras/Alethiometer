# Alethiometer: handoff for the next session

State of the work. Read `AGENTS.md` first (rules, reading order), then this. Standing material moved out on 2026-10-21: how to build, check and run is `app/docs/dev.md`; every user decision (don't re-ask) is `app/docs/decisions.md`; the feature-by-feature record is `app/docs/built.md`; bugs and gaps are `ISSUES.md`.

The original design handoff (mocks, chats, spec) is in `README.md`, `chats/` and `project/`; the spec is `project/design_handoff_helm/README.md`. The app was called Helm during design.

## Where things are

- **`main` holds all work** (app + design handoff; design updates land there too), including the front-end component work (2026-10-05, merged through `ec1d2a5`). Work on `main` — an earlier session silently ran on an out-of-date checkout, check `git status`/`git log` first.
- **`ISSUES.md`** (repo root): running list of bugs/gaps found wiring up real accounts on the Mac, separate from this file's "in flight" list until triaged in here. Check it before starting more connector work — several are load-bearing (the Tauri command was synchronous and froze the whole window on any slow call; `suggest_topics` had two separate gating bugs that made it silently do nothing).
- **Live on the work Mac: GitHub, Bedrock AI, Google Calendar, Jira, Gmail, Obsidian, Slack.** Proton ICS and Microsoft Teams are fixture-tested only.
- **Windows and Teams (2026-10-14):** the app type-checks for `x86_64-pc-windows-msvc` (Tauri on, all targets) but has never been built or run on Windows; what to check is in ISSUES.md. To type-check from Linux: `rustup target add x86_64-pc-windows-msvc`, then `cargo check --target x86_64-pc-windows-msvc --all-targets` with `CC_x86_64_pc_windows_msvc`/`AR_…` pointed at a stub that only creates its output file (the C dependencies need the MSVC SDK, which the cloud sessions can't download). Chat sources share `docs/chat.md`; Teams is `docs/sources/teams.md`.

## In flight / next

**Priorities (user, 2026-09-28), in order:**

1. **Slack**: live on the work Mac (`lookback_hours: 12`). Next: the missed-message audit (below).
2. **Gmail**: done and resynced; filing archives (todo or not). Still open: Unsubscribe (19a).
3. **Priorities**: built 2026-10-07 on `claude/priorities-design`: todos only, three orders (Priority, Quick wins, Neglect), Taskwarrior's coefficients, scored on read. Delegated asks are included, scored the same. Heat and tag suggestions wait for the work Mac. Every table now has `created_at`/`updated_at`. See `app/docs/priorities.md` ("Built", "Decided while building").
4. 13b (AI failure panel): built in the sync chip; the macOS notification on ok → failed waits for notifications in `platform.rs`.
5. **Todos (22a, 22c)**: built, see `app/docs/todos.md`. One typed model (`queries/todos.rs`, `TodoExt::Local | Jira | Obsidian`) over `items`; local todos live on the built-in `notes` source and never sync. Todos tab (sort, `/` filter, source chips, `x`/`u`, `n` new). In Incoming, `n` holds a todo on the signal (`held_todos`) until Confirm/Move creates it with that topic (Gmail threads are archived by the filing itself). Jira todos: `jira.create_issue` with `assign_me` + due, a stub item until the next poll. Creating a todo never writes to the topic's Obsidian note (the user's call; 22c's "link line" is dropped); todos show on the topic through their topic link, and `- [ ]` tasks in a topic's note are linked to that topic by the Obsidian sync. Not built: Marvin (needs the personal-boundary decision), 22b topic Tasks, 21e stale review, Topic/Priority/Due filter chips, AI-drafted todo text, Slack Later.

Slack and Gmail can only be tested with fixtures outside the user's Mac.

**Also open:**

- **Agents:** first real runs on the Mac (Claude Code via claude-agent-acp, Pi via pi-acp; Node 22+; `npm run agentd` builds the agent host, `npm run build:mac` bundles it), whether the work Mac's endpoint policy tolerates the long-lived agent host, and the Scion "To check against a live Hub" list in `app/docs/agents.md`. Not built: group rename/delete, native ACP subagent sessions. Projects, per-project Unsorted, moving local sessions (`m`, drag) built 2026-10-09 (`app/docs/agents.md` › Tree); not yet tried in the real window.

- **Settings/Glean follow-ups (ISSUES.md):** where AI prices come from; when ⌘K asks Glean; Glean decisions to confirm (⌘T pins Glean results; Glean not gated by `ai.allowed_providers`; local-only answers when Glean is down); first real run of the embedding model and of a Glean instance.

- **Expanded signal layout** (ISSUES.md): the user wants to redesign it, probably with structured connector fields (status, owner, reporter, due) instead of one joined `summary`, and a body excerpt (Incoming never shows an item's description today). Mocks were offered (HTML in `project/` or a published page); the user passed for now and may use the design tool.
- **Not built from the design spec** (checked against the code 2026-10-01): Gmail Unsubscribe (19a), topic Tasks (22b), stale todos review (21e), Todos filter chips and AI-drafted todo text (22a/22c), Also: a thread whose parent is older than the Slack lookback doesn't show inside its channel's split review (15h).
- **Front-end components** (`app/docs/ui-plan.md` is the work list and the record; its "Status" block at the top says what's done and next). The design system artifact ("Alethiometer", Design System type, https://claude.ai/artifact/WH4uwJJM3gk6Ux2HX7f565) is the reference; the ui-plan's per-decision "User, 2026-10-05" notes win over it where they differ.
  - **Lines (15, user 2026-10-05):** keep lines between rows, at least in a topic's Open panel (Priorities can't be compared: it isn't live).
  - **Next:** 20 Glean citations (parked by the user; the `local` field is in, next the marks and colours), then 15 lines vs outlined rows (open, the user looks after the passes).
  - **For the user** (ui-plan "Open: needs the user"): 15 lines vs outlined rows; whether empty states become one look; the New agent button's oversized grey `+`; the segmented control's redo; per-icon colour exceptions.
  - **Deferred:** the Cycler (decision 10; built and reverted, it hid the other options), the Life-block hatching (the feature needs rethinking), Notes line states.
  - **Icons (user, 2026-10-05):** yellow stays the icon accent (`--icon`; it began as an accident, NoteText's styles leaking to every `<i>`), with roles on top in `app.css`: field icons and a bare close × grey, + aqua, icons in a red count red, disabled with their button. Exceptions per icon come from the user's list (link in the ui-plan). The design system artifact is up to date as of `76232d7` (2026-10-05).
- **Design pass for the bigger window:** the app now opens at 1760×1175 (`tauri.conf.json`, about 80% of the user's screen); the mocks and the design README ("The window is about 1320×880") assume the old size. The user wants to review layouts for the larger space with the design thread.
- Questions for the user: should Topics' "Upcoming" (next 7 days) also show only the next instance of a series?
- Watch UI lag/CPU. A dismiss-triggered `refresh()` loop (an effect depending on `ui.rev`) was the dismiss lag and a ~150% CPU spike; fixed with `untrack`. Any `$effect` that calls `refresh()` must `untrack` it. Reads no longer wait for the writer (reader pool, Phase 1). If lag reappears, suspect the full `incoming.list` re-run after each write (a live query, 0.13–0.15 s with 1,000 unseen signals on a debug build; paging isn't built), or a long write (debug builds log writes over 100 ms).
- **Database plan, left over** (`app/docs/db-plan.md`): the first run on the user's real data (migrations 23–32) and the merge of `claude/funny-faraday-mgdokr` into `main`. Paging `incoming.list` and row-level live-query filters wait on a profile from real data. Revisions on UI-edited models were decided against (user, 2026-10-03: single-field edits, one session).
- **Slack missed-message audit:** review every path where a message could fail to reach a signal (dropped Socket Mode events, the reconnect window, edits/deletes, old-thread replies, rate-limit backoff, paging limits, DMs beyond `max_dms` without a socket) and fix/test each.
- **Obsidian (live):** connected to the work vault; a Topics note save through the plugin is verified. Writes go through the running Obsidian app via the Local REST API plugin (pinned to its own CA; `write_mode: "file"` fallback). Open questions: spurious conflicts if Obsidian hasn't flushed to disk yet, custom auth header names. (A conflicted save is deleted once a later save of the note goes through, 2026-10-21; one dismissed with *reload* and never followed by a save still stays failed.)
- UI for remaining writebacks: Slack reactions (backend `slack.react`; custom emoji need `emoji:read` and a picker), Jira transitions/comments in Topics, ticking Obsidian tasks.
- macOS pieces behind `platform.rs`: Keychain secrets, global capture hotkey, tray dot, persistent Reflect notification, launch at login.
- Real-account runs still to do: Proton ICS.
- People, not yet: two notes describing one person stay separate until you merge them by hand (never automatic, by design).
- Wanted later: assistant help writing Gmail filters (needs `gmail.settings.basic` + assistant actions).
- Check `ISSUES.md`'s "Open" section before starting connector work.

