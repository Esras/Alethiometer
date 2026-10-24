# Alethiometer

Personal, local-first work assistant. Tauri 2 (Rust core, system WebKit) with a Svelte 5 UI. The design reference and spec live in `../project/design_handoff_helm/`.

## Status

All five views run against the real SQLite schema. With `demo_data: true` (the default) they show sample data from the design mocks. Slack, Gmail, Google Calendar, Jira, GitHub, Obsidian and the work AI run against live accounts on the work Mac; Microsoft Teams, Proton ICS and Glean are tested only against recorded API responses.

| Area | State |
|---|---|
| Schema, migrations, FTS5 search | done (`core/migrations`) |
| `settings.json` (categories, buffers, AI allow-list) | done |
| Day strip, countdown, Life blocks, leave-by, buffer override | done |
| Topics (7a/7b) | done |
| Incoming (8a), Priorities (8b), Notes (9a/10a/11a), Reflect (9b/9c/9d, nudge as in-app card) | done |
| Work AI (fail closed, Bedrock Converse + SigV4, ask, topic suggestions) | done, live on Bedrock; off until models are set. See `docs/ai.md` |
| Connectors: Google Calendar, Gmail, Slack (+ Socket Mode), Microsoft Teams (polling; chats, channels with admin approval), Jira, GitHub, Obsidian (watcher), Proton ICS (busy only) | done; Teams and Proton ICS untested live. Setup per source in `docs/sources/`; what chat sources share in `docs/chat.md` |
| People notes (`People/*.md`), person colours, new-person suggestions, link to person, Slack split review | done. See `docs/people.md` |
| People tab: the index (top 3 topics, 12-week strip, sort, topic columns), merge two people | done, 2026-10-24. See `docs/people.md` › "The People tab". The person page isn't built |
| Reclassify a filed signal (chronology 18c, Search view 18d), merge topics | done. See `docs/reclassify-and-merge.md` |
| Writeback outbox (pending → sent → confirmed/failed, ⌘R retry) | done. Wired: Slack mark-read on Confirm, Gmail mark-read on Confirm and Move, Jira create from Notes, calendar holds from Reflect. Connectors also support RSVP, reply, react, transition, comment, assign, label, Obsidian append/toggle; no UI for those yet |
| macOS Keychain, global hotkey, tray, notifications | behind `platform::Platform`; file-backed stub for now |
| Sync status (freshness per source, write states) | tab bar |
| Settings modal (⌘,), live reload of settings.json, typed source configs | done. See `docs/settings.md` |
| Local embeddings (bge-small, tract, sqlite-vec) | done, model downloads on first run; untested with the real model. See `docs/ai.md` |
| Glean (search-only): ⌘K and Search view routes, Assistant with Glean | done against recorded responses; untested live. See `docs/sources/glean.md` |
| Agents tab (Agents Final 01–08): local ACP sessions (Claude Code, Pi), Scion Hub rooms and agents, approvals with a shell parse, Settings › Agents | local sessions tested against a scripted ACP agent and recorded claude-agent-acp traffic; Scion built from its source, never run against a Hub. See `docs/agents.md` |

## Run it

### Browser mode (fastest; what the cloud sessions use)

No WebKitGTK needed. The dev server exposes the same `call(cmd, args)` API over HTTP.

```sh
npm ci
ALETHIOMETER_FAKE_NOW=10:52 npm run devserver   # builds the agent host (agentd), then the Rust core on 127.0.0.1:1421, data in ./.alethiometer-dev
npm run dev                              # UI on http://localhost:1420
```

The UI reaches the dev server through Vite's `/call` proxy. The dev server answers only loopback hosts and pages, sends no CORS headers, and needs `X-Alethiometer: 1` on every call (`curl -H 'X-Alethiometer: 1' -X POST 127.0.0.1:1421/call/app.info -d '{}'`), so other sites open in the browser can't read or change your data.

`ALETHIOMETER_FAKE_NOW` pins the clock to the mock's 10:52 so the countdown and "now" line match the designs. Delete `.alethiometer-dev/` to re-seed.

Screenshots: `node scripts/shot.mjs out.png topics ']'` (Playwright; the extra args are key presses; `ALETHIOMETER_SHOT_SIZE=1760x1175` for the app's window size).
To check a UI refactor changes nothing: `scripts/ui-diff.sh <ref>`, then `node scripts/ui-diff.mjs` (pixel diff of ~40 scenes against `<ref>`).

### Desktop

Linux needs `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev`.

Windows needs Rust with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`), Visual Studio Build Tools with "Desktop development with C++" (the C compiler and Windows SDK for the bundled SQLite, sqlite-vec, tree-sitter and tract), Node 22+, and WebView2 (already on Windows 10 and 11). Run everything from a normal PowerShell or cmd; the Nix devshell and `scripts/ui-diff.sh` are Unix-only. `npx tauri build` makes an MSI and an NSIS installer, unsigned (SmartScreen will warn). Not run on Windows yet: `docs/windows.md` is the checklist.

```sh
npx tauri dev
```

### Mac build and signing

Build the `.app` on the Mac with `npx tauri build`. Set `bundle.macOS.signingIdentity` in `src-tauri/tauri.conf.json` to your Apple Development certificate (free Apple ID, personal team). Signing every build with the same certificate keeps Keychain and Calendar approvals across rebuilds. No notarization is needed.

## Data and config

Everything lives in the app data directory (`~/Library/Application Support/dev.alethiometer.app` on macOS, `~/.local/share/dev.alethiometer.app` on Linux, `%APPDATA%\dev.alethiometer.app` on Windows; override with `ALETHIOMETER_DATA_DIR`):

- `alethiometer.sqlite`: the database.
- `settings.json`: written with defaults on first run. Edit it in Settings (⌘,) or by hand; either way changes apply live (`docs/settings.md`).
- `secrets.json`: API tokens (mode 0600; Windows has no file modes, so there only the profile folder's permissions protect it), for now on the Mac too. `secrets.meta.json` keeps when each was set (names and times only).
- `models/`: the local embedding model, downloaded on first run.
- `vault/`: stand-in work Obsidian vault until `obsidian_vault` is set.

`settings.json` fields:

| Field | Meaning |
|---|---|
| `categories` | Array of `{key, label, color}`, in UI order. The source of truth: read at startup, and every create, rename, recolor or delete in the Topics index (Categories) writes it back (the rest of the file keeps its content and order). Keys never change. `work` (default) and `life` (Life blocks) are built in; removing one of those, or a key topics or calendar blocks still use, keeps it and writes it back. The old key → `{label, color}` map is accepted once: the database's categories win and the file is rewritten as the array. Edits while the app runs take effect on restart, and the app's next category change overwrites them. |
| `day_start_hour`, `day_end_hour` | Day strip range (default 8–19). |
| `life_buffer_min` | Default travel buffer before a Life block (press `b` on a block to override one). |
| `obsidian_vault` | Absolute path to the work vault. |
| `life_calendars` | Old place for Proton Calendar links: moved into the protoncal source's `urls` on first open. |
| `ai.allowed_providers` | Providers covered by a BAA. Anything else fails closed. |
| `ai.region`, `ai.small`, `ai.large` | Bedrock region and model per tier. Empty model = AI off. See `docs/ai.md`. |
| `ai.aws_profile` | AWS profile for Bedrock credentials (empty = `AWS_PROFILE`, then `default`). |
| `quiet_days` | Topics with no signal for this many days go under "Quiet" in the Topics index (default 7). |
| `ui_scale` | Desktop window zoom (default `1.0`, e.g. `1.15`). Applies live. |
| `ui_font`, `mono_font` | Interface and monospace font families (empty = system). |
| `ui_font_size`, `reading_font_size` | Base sizes in px at scale 1.0: lists and controls (13), messages, notes and drafts (14). |
| `fake_now` | Dev only: pin the clock to `HH:MM`. |
| `demo_data` | Fill an empty database with the sample data. Turn off before connecting real sources. |
| `people.my_color` | Your person colour: index into the 256-colour palette (default `0`, Gruvbox red). See `docs/people.md`. |
| `people.me` | Your own accounts besides your Slack user id, as `kind:account` (`email:me@work.com`, `jira:…`, `github:…`). |
| `people.suggest_after`, `people.suggest_days` | Suggest a People note for an unknown sender seen in this many signals (default 3) within this many days (default 7). |

## Architecture

- `src-tauri/src/api.rs`: one entry point, `call("<area>.<name>", args)`, used by the Tauri command and by `bin/devserver.rs`.
- `src-tauri/src/queries/<area>.rs`: read model, actions and sample data for each view.
- `src-tauri/src/ai/`: the only path to an LLM. Checks the allow-list before anything leaves the machine; builds prompts from work sources only.
- `src-tauri/src/platform.rs`: Mac-only integrations behind a trait.
- `agentd/`: the agent host, a separate small program local agents run in so they outlive the app (`docs/agents.md`, "Agent host").
- `src/shell/`: title bar, tabs, day strip, search, assistant pane.
- `src/views/`: one component per tab.

Instants are stored in UTC (`YYYY-MM-DDTHH:MM:SS.sssZ`, fixed width so text order is time order); calendar days (a Reflect day, a day-only due date) as `YYYY-MM-DD`. The app's `Clock` (`src-tauri/src/clock.rs`) is the only place that turns either into wall-clock time, using the system zone; the UI uses the same zone (`app.info.tz`). Times use `jiff`; see `src-tauri/src/time.rs`. Databases from before this were converted by migration 21 (`src-tauri/src/utc_migration.rs`).

## Keys

| Key | Where | Action |
|---|---|---|
| `⌘1`–`⌘8` | anywhere | switch tab (`⌘8` Agents); `g i` / `g a` Incoming / Agents |
| `⌘K` or `/` | anywhere | search (`⌥↵` Glean, `⌥A` anchor, `⇧↵` off-topic, `⌘T` link, `⌘⌫` reject, `⌘↵` Search view) |
| `⌘,` / `⌘⇧,` | anywhere | Settings / open settings.json |
| `e` | Assistant (Glean) | Sources panel (`⇥` Kept/Dropped, `p` pin, `x` reject) |
| `⌘I` | anywhere | toggle assistant pane |
| `Esc` | overlays | close |
| `b` | Life block popover | change travel buffer |
| `z` | Topics | fold the left column |
| `[` `]` | Topics | previous / next topic |
| `f` | Topics | cycle chronology filter |
| `e` | Topics | open the note in Obsidian |
| `j`/`k`, `[` `]`, `↵`, `c`, `x`, `t`, `o` | Incoming | move, switch topic, expand, confirm, wrong topic, move to topic, open in app |
| `j`/`k`/`h`/`l`, `↵`, `o`, `p`, `v` | Priorities | move, open topic, open in app, peek, compact/expand quadrant |
| `Tab`, `⌃↵`/`⌘↵`, `⌘R`, `[` `]`, `t` | Notes | change destination, send to Incoming, retry, previous/next day, today |
| `d` `w` `m` `q`, `l`, `[` `]`, `j`/`k`, `>` `<` `x`, `u`, `a` | Reflect | scope, lens, period, carry row, carry actions, undo, ask |
| `Tab`, `j`/`k`, `h`/`l`, `↵`, `n`, `e`, `c`, `g`, `t`/`m`, `o` `s` `a` `d`, `1`–`3`, `r`, `⌘.`, `?` | Agents | zone, move, fold, open/reply, new agent, fold row, copy, latest, tabs, approve, answer, reply, interrupt, keys (`docs/agents.md`) |
