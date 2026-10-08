# Running issues

Found while wiring up connectors on the real Mac. Not in HANDOFF.md's "in flight" list — track separately until triaged in there.

## Open

### Agents: OpenShell sandboxes, what's left (2026-10-08)

Built (`app/docs/agents.md` › OpenShell sandboxes) from OpenShell's source and docs, tested only against `scripts/fake-openshell.mjs`. To check against a real gateway, local and remote:

- `sandbox create --detach --upload` with no main command: that the sandbox stays up (a login shell in a retained PTY) and the upload lands at `<working dir>/<directory name>`.
- `sandbox exec --no-tty` with stdin held open for hours: ACP lines both ways without buffering, the login shell's profile printing nothing to stdout (it would be a non-JSON line), and what a laptop sleep or a gateway restart does to it (the agent should show as exited and resume on the next message).
- Stop kills the `openshell` CLI process; whether the gateway then ends the adapter inside the sandbox is unchecked. If it doesn't, a second exec would start a second adapter on the same session.
- claude-agent-acp in auto mode under OpenShell's network policy (auto needs a model that supports it; the claude-code provider profile allows only api.anthropic.com and telemetry).
- pi permission extensions against `openshell::auto_allow`: that the gated call is shown pending when the extension asks (pi-acp 0.0.34 does, from the streamed tool call), and their choice names (allow by name: yes/allow/approve/ok/proceed/continue/run, not if it has no/deny/reject/block/cancel/don't).
- `rule get --status pending` has no JSON output: `openshell::parse_rules` reads the labelled text of openshell-cli's `sandbox_draft_get` at `67adcf1`. A relabel upstream breaks it quietly (no approvals show). Worth asking OpenShell for `-o json` there.
- `sandbox download` of a directory with a big `node_modules` the agent installed: it's all downloaded before the git-ignore filter drops it. Fine for a project, slow for a heavy one.

Not built: bringing changes back from a sandbox that was already there (no record of what went in, no local directory); the model list from inside the sandbox; the facts box (git runs here, where the sandbox path doesn't exist), and Explained's script reads, which find nothing for the same reason. Bring back writes files only; it never commits, and a binary file in conflict is listed like any other (no merge).

### Agents: local runtimes (podman sandbox, ssh) (user, 2026-10-03: deferred; OpenShell built 2026-10-08)

Running a harness somewhere other than the host, e.g. the user's podman `claude-sandbox` (`~/.config/home-manager/claude-sandbox`), which Zed reaches over ACP today. Proposed shape, mirroring Scion's harness × broker: an `agents.runtimes` list in settings.json (`instances`, `start`, `exec` prefix, `cwd` template, `editor` URL, `mode`), harnesses gain an optional `runtime`, and everything that touches the session's files (the adapter, git for the branch fact, Explained's script reads) goes through one `exec` + `env -C {cwd}` helper. Today all of those assume the session directory is on the host; Explained reading the host copy of a sandboxed project would be wrong, not just missing. Sandboxed sessions would use Claude's `auto` mode.

### Agents tree: ages ignore the clock offset for their first second

`views/agents/Tree.svelte` starts `now` at `Date.now()` without `ui.info.clock_offset_ms`; the 1 s interval adds it. So with a fake or offset clock, ages read one value (e.g. "5d") until the first tick and another ("6d") after. Found by the ui-diff sharding work (2026-10-06), where a shot racing that tick split the two sides; the scenes now wait past it. Not fixed: the fix is one line, but it needs a test.

### Agents: archiving, what's left (2026-10-05)

- **No "done" for local sessions** (user: allowed to archive half-done work, but there aren't enough signals yet). A local session goes idle after every turn and never reaches completed, so local groups get no done nudge and their confirm never shows Not done. Candidates: a merged PR or Done ticket on every member, a clean tree with nothing unpushed, the agent saying it finished.
- **Unpushed commits on Scion:** the Hub's agent record has no ahead/behind count, so the "N unpushed commits" line only appears when the session's branch fact has one (a git checkout on this machine). Scion agents usually have none, so a clone with unpushed work can be deleted with no warning beyond the other Not done lines.
- **The Scion group thread after its agents are deleted** is left as it is on the Hub (the design's open question).
- **An archive still in its 10 s undo window when the app quits doesn't happen.** The window is kept in the UI, and the archive is only written when it ends.
- **The tree behind the confirm** is dimmed by the modal's scrim, not drawn at 32% with the target still lit as in the mock.
- **Archived search** is a substring scan (ASCII case-insensitive) over every archived row, run 150 ms after each keystroke. It's fine at hundreds of transcripts; past that it wants an FTS table.

### Agents: unverified against real agents and a live Scion Hub

The ACP client is tested against recorded claude-agent-acp traffic and a scripted stand-in (`app/scripts/fake-acp.mjs`); it has never driven a real Claude Code or Pi session. The Scion client was checked against the sandbox Hub on 2026-10-06; what was confirmed and what's left is in `app/docs/agents.md` ("Checked against a live Hub"). The New agent form can't set a model or env, so an agent that talks to agentgateway needs a template that sets `ANTHROPIC_BASE_URL`. Claude's cost is read as a running total from `usage_update` (unverified), and the cache state is an estimate.

### AI prices: where they come from (user, 2026-10-02: open, come back to it)

The Settings design (26c) says prices are never typed in: read once a day from the AWS Price List API (`pricing:GetProducts`) and cached. That API needs classic IAM credentials (or an SSO profile) with `pricing:GetProducts`. The Bedrock API key (bearer token) the work Mac uses only works for Bedrock, so it can't call it. Options: AWS's public bulk price file for Bedrock (`https://pricing.us-east-1.amazonaws.com/offers/v1.0/aws/AmazonBedrock/current/region_index.json`, no credentials; checked reachable on 2026-10-02), GetProducts with separate IAM credentials, or keep hand-entered prices. Until decided, `ai.prices` stays editable in Settings → AI.

### ⌘K: when to ask Glean (user, 2026-10-02: open)

Built for now: ⌘K searches locally as you type; Glean only on ⌥↵ (also in the Search view). Candidates: debounced as you type with a cache, on a pause, or on demand as now. Every Glean call counts against `calls_per_day`.

### Glean: decisions made while building that need a look

- **Linking a Glean result to a topic (⌘T)** pins it to the topic, since Glean results aren't stored as items. A local item is filed as usual.
- **Glean isn't gated by `ai.allowed_providers`.** It isn't an LLM endpoint and it's the company's own search, but queries do carry work context (names, keys, signal titles). Say if it should need an allow-list entry.
- **Glean unavailable in the Assistant** answers from local evidence and says why. The hi-fi lists "Glean unavailable" as not yet designed.
- **Off-topic check without the embedding model**: until the model is downloaded, a Glean result needs only the shared entity, not the similarity.
- Not built: "+ topic" on the owner list (3d), the Glean directory into People notes (1f/1g, left out on purpose), `@` references in the Assistant input.

### Embedding model download is unverified

The model comes from `https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx` (and `tokenizer.json`). Hugging Face wasn't reachable from the build environment, so the URL, the file and its input/output names were checked only against a stand-in with the same interface. If the first run on the Mac fails, Settings → AI shows why and offers Retry. A work network that blocks huggingface.co needs the two files dropped into `<data dir>/models/bge-small-en-v1.5/` by hand.

### Priorities: built, not tuned (2026-10-07)

Built on real data (`app/docs/priorities.md`), but only Taskwarrior's factors count yet: due, scheduled, blocks, topic priority. Meeting, waiting on you, who asked, breadth and estimate are built at coefficient 0, heat isn't built, and tags have storage but no vocabulary or suggestions. To do on the work Mac: tune the coefficients (`settings.json` › `priorities`), work out the heat baseline, build the tag vocabulary from real data and design the tag UI (user, 2026-10-07: to discuss). Never run on real data yet: check the first load's time with the real todo count (the facts query is one pass, with correlated subqueries per todo).

### Todos: completing a Jira todo assumes a "Done" transition

`x` on a Jira todo queues `jira.transition` to "Done" (matched by transition name or target status). The DEVOPS project's done status is "Closed", reached only through a couple of intermediate steps, so completing a DEVOPS todo fails in the outbox. Needs a per-project done status and a way to walk the workflow (or open the issue in Jira instead). Creating issues is fine as is.

### Answering (RSVP, Slack reply): unverified against real accounts (2026-10-07)

Built and tested with fixtures only (`queries/respond.rs`). To check on the Mac: an RSVP on a recurring invite patches the series and every instance shows it after the next poll; whether Google keeps an earlier note (`comment`) when a later answer patches the entry without one; a Slack reply in a thread and in a DM; ⌥N on the Mac keyboard (a dead key on the US layout: the keydown arrives with `e.code` `KeyN`, but check nothing leaks into the next field). "Accepted · HH:MM" uses the outbox's `confirmed_at`, which is the system clock, so `fake_now` doesn't move it. An answer from here keeps showing after you change it in Google Calendar; the row says what was sent from here, not what Google says now.

### UI key checks: the font picker fails on `main` in the cloud container

`scripts/ui-keys.mjs picker` fails one or both font picker checks (cursor not on row 0; ↓ ↵ doesn't change the setting), on HEAD as well as with the 2026-10-07 changes, varying run to run. Probably the container's installed fonts; not checked on the Mac.

### Gmail: what's left

Built: Incoming mirrors the whole Inbox (read or not), filing labels by category (`config.category_labels`), marks read and archives (todo or not), Dismiss archives, `#` trashes, `u` restores, moving a filed thread to a topic in another category relabels it (docs/sources/gmail.md), a chip showing the thread's own current Gmail label matched to a category. Still open: Unsubscribe (19a), assistant help writing Gmail filters (needs `gmail.settings.basic`).

### Expanded signal layout in Incoming needs a design pass

Interim fix in `views/Incoming.svelte`: when a signal is expanded, the title gets its own full-width line (wrapping instead of truncating), and below it one muted info line joins `meta` (author · time) with the connector's `summary`, dropping ` · ` parts that repeat. Found on Jira issues, where `meta` names the reporter and `summary` is "Status · assignee", so the same name showed twice and the header was squished next to the action buttons. Still unlabeled when reporter and assignee differ ("Alice · 10:41 · Open · Bob"). The user wants to adjust the design; labeling roles would mean changing the Jira connector's `summary`, which the Topics timeline also shows.

### Jira: field changes other than status and assignee aren't updates (user, 2026-10-03: maybe never)

A change by someone else to priority, due date, summary, labels or any other field doesn't bring an issue back to Incoming; only status, assignee and comments do (`bump` in `sync/connectors/jira.rs`). The item still picks up the new values on the next poll. The user isn't sure these should count; revisit if a missed due-date change bites.

### Slack messages as "updates" on filed items

Unseen items with a `confirmed` link now show under their topic in Incoming as updates (with Confirm, like any signal). A busy Slack thread filed under a topic would resurface on every new message; the user is designing how Slack should behave here. Jira counts status/assignee changes and comments by others as updates.

### Sample data writes vault files inside its database transaction

`seed::seed_if_empty` writes the stand-in vault's People, Topics and Reflect notes while its unit of work is open. It runs once, on an empty database with no real vault, so nothing waits on it, but it breaks the rule of keeping file IO outside the database. Fix if the seed grows: write the files first, then the rows.

### Changing `obsidian_vault` strands notes saved in the stand-in vault

With `obsidian_vault` empty, Reflect (and Topics) write to `<data dir>/vault`. Setting `obsidian_vault` later switches every read to the real vault and nothing moves the stand-in's files, so earlier reflections silently disappear from Reflect. Hit when connecting Obsidian on the work Mac: `Reflections/Daily/2026-09-28.md` was copied over by hand (the original is still in the stand-in). Fix direction: on startup, if `obsidian_vault` is set and `<data dir>/vault` has files, offer to copy them over (never overwrite), or at least warn.

### Obsidian notes: live-preview editing with the vault's theme (user, 2026-10-05: deferred)

Notes are shown by our own `NoteText.svelte` and edited in a plain textarea, so a note goes from formatted to raw text the moment you edit. Idea: a CodeMirror 6 editor (as Obsidian uses) with decorations that hide markdown syntax except on the line being edited, styled by the theme named in `<vault>/.obsidian/appearance.json` (`cssTheme`, CSS in `.obsidian/themes/<name>/`, plus snippets). Needs: Obsidian's class names (`.cm-header-1`, `.HyperMD-list-line`, `.markdown-rendered`) and baseline CSS variables the theme doesn't define; our own decorations (Obsidian's live preview is closed; check for an open-source package first); tables, callouts and embeds last. Saving (conflicts, outbox) stays as it is. Start with the Topics note pane, then Reflect. Fidelity to the theme would be close, not exact; plugin-added styling won't carry over. Embedding Obsidian's own rendering isn't possible.

### Database Phases 2–4: unverified on real data

Migrations 23–32 (message keys, `sync_streams`, `remote_rev`, `items.rev`, outbox lease, derived topic values, corpus revisions, the embedding queue, Incoming indexes, the FTS update trigger) were tested on fixtures and scratch databases built from sample data, not on the user's database. Check the first start's time and that Incoming, Topics and Notes look the same afterwards. Message keys were checked on the sample data only: real rows without an id get a hashed key, and a GitHub or Jira comment re-read later moves to its id key, keeping its read state.

### Live queries re-run per table, not per row

Any write to `items` re-runs every subscription that read `items`, Incoming's included (0.13–0.15 s with 1,000 unseen signals, debug build; the list no longer carries message text). That's fine at today's write rate. If a busy sync makes it show, filter by row (the plan's next step) or page `incoming.list`.

A topic or Reflect page open on screen shows a note edited in Obsidian when the Obsidian source's watcher sees the file change. Paths that source ignores, and the sample data's stand-in vault (no watcher), show within a minute, at the periodic re-run.
