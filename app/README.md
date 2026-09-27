# Helm

Personal, local-first work assistant. Tauri 2 (Rust core, system WebKit) with a Svelte 5 UI. The design reference and spec live in `../project/design_handoff_helm/`.

## Status

All five views run against the real SQLite schema. With `demo_data: true` (the default) they show sample data from the design mocks. Connectors, writeback and the work AI are built and tested against recorded API responses, but none has run against a live account yet.

| Area | State |
|---|---|
| Schema, migrations, FTS5 search | done (`src-tauri/migrations`) |
| `settings.json` (categories, buffers, AI allow-list) | done |
| Day strip, countdown, Life blocks, leave-by, buffer override | done |
| Topics (7a/7b) | done |
| Incoming (8a), Priorities (8b), Notes (9a/10a/11a), Reflect (9b/9c/9d, nudge as in-app card) | done |
| Work AI (fail closed, Bedrock Converse + SigV4, ask, topic suggestions) | done, untested against a live account; off until models are set. See `docs/ai.md` |
| Connectors: Google Calendar, Slack (+ Socket Mode), Jira, GitHub, Obsidian (watcher), Proton ICS (busy only) | done, untested live. Setup per source in `docs/sources/` |
| Writeback outbox (pending → sent → confirmed/failed, ⌘R retry) | done. Wired: Slack mark-read on Confirm, Jira create from Notes, calendar holds from Reflect. Connectors also support RSVP, reply, react, transition, comment, assign, label, Obsidian append/toggle; no UI for those yet |
| macOS Keychain, global hotkey, tray, notifications | behind `platform::Platform`; file-backed stub for now |
| Sync status (freshness per source, write states) | tab bar |
| sqlite-vec embeddings | not started |

## Run it

### Browser mode (fastest; what the cloud sessions use)

No WebKitGTK needed. The dev server exposes the same `call(cmd, args)` API over HTTP.

```sh
npm ci
HELM_FAKE_NOW=10:52 npm run devserver   # Rust core on 127.0.0.1:1421, data in ./.helm-dev
npm run dev                              # UI on http://localhost:1420
```

`HELM_FAKE_NOW` pins the clock to the mock's 10:52 so the countdown and "now" line match the designs. Delete `.helm-dev/` to re-seed.

Screenshots: `node scripts/shot.mjs out.png topics ']'` (Playwright; the extra args are key presses).

### Desktop

Linux needs `libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libayatana-appindicator3-dev`.

```sh
npx tauri dev
```

### Mac build and signing

Build the `.app` on the Mac with `npx tauri build`. Set `bundle.macOS.signingIdentity` in `src-tauri/tauri.conf.json` to your Apple Development certificate (free Apple ID, personal team). Signing every build with the same certificate keeps Keychain and Calendar approvals across rebuilds. No notarization is needed.

## Data and config

Everything lives in the app data directory (`~/Library/Application Support/dev.helm.assistant` on macOS, `~/.local/share/dev.helm.assistant` on Linux; override with `HELM_DATA_DIR`):

- `helm.sqlite`: the database.
- `settings.json`: written with defaults on first run. Edit by hand.
- `secrets.json`: API tokens on Linux (mode 0600). On the Mac these move to the Keychain.
- `vault/`: stand-in work Obsidian vault until `obsidian_vault` is set.

`settings.json` fields:

| Field | Meaning |
|---|---|
| `categories` | Category key → `{label, color}`. Topics and calendar blocks reference the keys (`work`, `life`, `jobs`, `self`, `alert`). Change a color here and restart. |
| `day_start_hour`, `day_end_hour` | Day strip range (default 8–19). |
| `life_buffer_min` | Default travel buffer before a Life block (press `b` on a block to override one). |
| `obsidian_vault` | Absolute path to the work vault. |
| `life_calendars` | Proton Calendar "Limited view" links. Busy times only. |
| `ai.allowed_providers` | Providers covered by a BAA. Anything else fails closed. |
| `ai.region`, `ai.small`, `ai.large` | Bedrock region and model per tier. Empty model = AI off. See `docs/ai.md`. |
| `ai.aws_profile` | AWS profile for Bedrock credentials (empty = `AWS_PROFILE`, then `default`). |
| `fake_now` | Dev only: pin the clock to `HH:MM`. |
| `demo_data` | Fill an empty database with the sample data. Turn off before connecting real sources. |

## Architecture

- `src-tauri/src/api.rs`: one entry point, `call("<area>.<name>", args)`, used by the Tauri command and by `bin/devserver.rs`.
- `src-tauri/src/queries/<area>.rs`: read model, actions and sample data for each view.
- `src-tauri/src/ai/`: the only path to an LLM. Checks the allow-list before anything leaves the machine; builds prompts from work sources only.
- `src-tauri/src/platform.rs`: Mac-only integrations behind a trait.
- `src/shell/`: title bar, tabs, day strip, search, assistant pane.
- `src/views/`: one component per tab.

Times are stored as local naive timestamps (`YYYY-MM-DDTHH:MM:SS`). That's fine for one person on one machine; revisit if you travel across time zones with it.

## Keys

| Key | Where | Action |
|---|---|---|
| `⌘1`–`⌘5` | anywhere | switch tab |
| `⌘K` or `/` | anywhere | search |
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
