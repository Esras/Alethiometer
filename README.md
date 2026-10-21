# Alethiometer

A personal, local-first work assistant for one person's working day. It pulls Slack, Gmail, Google Calendar, Microsoft Teams, Jira, GitHub, Obsidian and Proton Calendar into one keyboard-driven window, files what arrives into topics, and keeps notes, todos and a daily reflection beside them. An Agents tab runs and supervises coding agents (Claude Code, Pi, Scion). It was called **Helm** during design.

Your data stays on your machine: one SQLite database, a `settings.json` and a secrets file. The work AI is off until you pick models on Amazon Bedrock, and it refuses to send anything to a provider you haven't allowed.

![Incoming: new Slack, Gmail and calendar signals waiting to be filed into topics, under the day strip](docs/screenshots/incoming.png)

![A topic: its next meeting, open todos, chronology and the Obsidian note side by side](docs/screenshots/topic.png)

## Status

Early and built for one user. All seven tabs (Incoming, Priorities, Topics, Todos, Notes, Reflect, Agents) work, and the app opens on sample data so you can look around. The connectors and the work AI are tested against recorded API responses but **haven't run against a live account yet**; expect rough edges when you connect your own. Developed on macOS and Linux; Windows type-checks but has never been run. Known problems are in `ISSUES.md`.

## Get started

**[SETUP.md](SETUP.md)** walks through installing the prerequisites, running the app on sample data, and connecting your own accounts.

The short version, to see it in a browser with sample data:

```sh
cd app && npm ci
ALETHIOMETER_FAKE_NOW=10:52 npm run devserver   # Rust core on :1421 (first build takes a few minutes)
npm run dev                                      # in a second terminal; open http://localhost:1420
```

## How it's built

A Tauri 2 app: a Rust core with SQLite in `app/src-tauri`, a Svelte 5 UI in `app/src`, and a small agent host daemon in `app/agentd` that keeps coding agents running when the window closes. There is no mobile build. `app/README.md` has the architecture, the `settings.json` fields and the full key list.

## Working on it

Read in this order:

1. **`AGENTS.md`**: the engineering rules (accuracy over performance, battle-tested libraries, how time and the database are handled, tests). `CLAUDE.md` points to it.
2. **`HANDOFF.md`**: what is built, what's in flight, every decision the user has made (don't re-ask those), and how to run the checks.
3. **`app/README.md`**: the app's status table and how to run it.
4. **`app/docs/`**: one document per area (`ai.md`, `agents.md`, `db-plan.md`, `people.md`, `settings.md`, `todos.md`, `reclassify-and-merge.md`, `ui-plan.md`), and `sources/` for connector setup.
5. **`ISSUES.md`**: bugs and gaps found on real accounts. Check it before connector work.

Checks:

- `cd app/src-tauri && cargo test --no-default-features`
- `cd app/agentd && cargo test`
- `cd app && npm run check`
- UI changes: `app/scripts/ui-diff.sh <ref>` and `node scripts/ui-diff.mjs` pixel-diff about 90 scenes against a git ref, and `node scripts/ui-keys.mjs` runs the keyboard checks (`app/docs/ui-plan.md`).

### The design

The app was built from a Claude Design handoff, which stays in the repo as the reference:

- **`project/design_handoff_helm/README.md`** is the spec. Its "Files" section says which mock IDs are current.
- **`project/*.dc.html`** are the HTML mocks: explorations by round (rounds 1–6 are superseded), Agents, Glean and the day strip.
- **`chats/`** holds the design conversations. Read them for intent: they show where the user landed, not only what was drawn.

The **Alethiometer design system** (https://claude.ai/artifact/WH4uwJJM3gk6Ux2HX7f565) holds the tokens, shared classes and numbered decisions for the front end. Where it differs from the "User" notes in `app/docs/ui-plan.md`, the notes win.

### Other files

- `REVIEW-2026-10-03.md`: an architecture and correctness review, with its findings.
- `patches/pi-acp/`: patches for the user's fork of `pi-acp`, the Pi adapter, kept until they land there. Alethiometer doesn't depend on them.

## License

Apache 2.0, see `LICENSE`.
