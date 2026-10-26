# Working on Alethiometer

For agents and people.

Priorities: **accuracy over performance, maintainability over speed of development.**

## Read in this order

1. **`HANDOFF.md`**: what's in flight and what's next.
2. **`app/docs/decisions.md`**: every decision the user has made (don't re-ask those). **`app/docs/dev.md`**: how to build, run and check.
3. **`app/README.md`**: the app's status table and how to run it.
4. **`app/docs/`**: one document per area: `ai.md`, `agents.md`, `db-plan.md`, `people.md`, `settings.md`, `todos.md`, `reclassify-and-merge.md`, `ui-plan.md` (front-end work), and `sources/` for connector setup.
5. **`ISSUES.md`**: bugs and gaps found on real accounts. Check it before connector work.

## Engineering rules

- **Use battle-tested libraries instead of hand-rolling them.** Time zones, parsing (shell, HTML, Markdown, ICS), crypto and signing, HTTP, retries, connection pools, protocols. If a well-maintained crate or package does it, use it. Write your own only when nothing fits, and say why in a comment. Examples already decided: `jiff` for time, `tree-sitter-bash` for shell commands.
- **Time** (`app/docs/db-plan.md`, Phase 0):
  - An instant is stored and handled in UTC. A calendar day is a local date.
  - Only the app's `Clock` turns one into the other. Nothing else reads the system time zone or local time.
- **Database** (`app/docs/db-plan.md`):
  - Every write that changes more than one row runs in one transaction, opened and committed by the edge: a command, a sync batch or an outbox send. It is never opened by business logic, and never committed by a repository.
  - Derived values are computed by the database (views, triggers), never written twice by the application.
  - The target is SQL only in `store/`, with repositories that take and return models. New code goes there, not into new SQL elsewhere.
- **Never hold the database across a network call or a file read.**
- **Keep the build green after every edit when you're running inside Alethiometer.** The user runs `npx tauri dev`, which rebuilds on each save; a broken build takes the window away, and with it the only way to answer your permission prompts. Add a function or field before anything uses it, and change a signature and all its callers in one edit.
- **Tests:** a fix comes with a test that fails without it. Races get a test that runs the competing paths at once. Fixtures replace live accounts.
- **Formatting is the tools', not by hand**, 120 columns: Rust with rustfmt (`rustfmt.toml`; `cargo fmt` in `app/src-tauri` and `app/agentd`, `cargo fmt --check` must pass), the front end with Prettier (`app/.prettierrc.json`; `npm run format` in `app`, `npm run format:check` must pass). Run them before committing. Don't match an older wide style.
- **Keep each change minimal and say what it doesn't do.** Record decisions in `app/docs/decisions.md`, open problems in `ISSUES.md`, and current state in `HANDOFF.md`.

## The design

The app was built from a Claude Design handoff, which stays in the repo as the reference:

- **`project/design_handoff_helm/README.md`** is the spec. Its "Files" section says which mock IDs are current.
- **`project/*.dc.html`** are the HTML mocks: explorations by round (rounds 1–6 are superseded), Agents, Glean and the day strip.
- **`chats/`** holds the design conversations. Read them for intent: they show where the user landed, not only what was drawn.
- **`project/fixtures/busy-week.json`** (also `busy-week.js`, which sets `window.BUSY_WEEK`) is invented data at the density of a real account over four weeks (2026-09-28 to 2026-10-25, today is the last day): ~250–300 signals per weekday (Slack ~123, agents ~68, Gmail ~30, Jira ~13, calendar ~10), 600 people in 42 groups (teams, projects, cross-team, social), 184 topics (83 active, 101 quiet), and meetings from recurring series with fixed guest lists. Signals draw their people and topics from a group, so the same people recur together and people have home topics. Its `shape` field gives the measured density. Populate mocks from it instead of a handful of hand-made rows, and show the density rather than trimming it to fit. It is large (7 MB): query it with `jq` or grep. Regenerate with `node app/scripts/gen-busy-week.mjs`.

The **Alethiometer design system** (https://claude.ai/artifact/WH4uwJJM3gk6Ux2HX7f565) holds the tokens, shared classes and numbered decisions for the front end. Where it differs from the "User" notes in `app/docs/ui-plan.md`, the notes win.

## Other files

- `REVIEW-2026-10-03.md`: an architecture and correctness review, with its findings.
- `patches/pi-acp/`: patches for the user's fork of `pi-acp`, the Pi adapter, kept until they land there. Alethiometer doesn't depend on them.
