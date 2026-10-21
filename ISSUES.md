# Running issues

Found while wiring up connectors on the real Mac. Not in HANDOFF.md's "in flight" list — track separately until triaged in there.

## Open

### UI checks in the real web view (user, 2026-10-21: deferred)

`scripts/shot.mjs`, `ui-diff` and `ui-keys` drive Chromium against the browser-mode dev server, but the app renders in WKWebView (Mac), WebKitGTK (Linux) and WebView2 (Windows). Tauri's docs recommend WebdriverIO's `@wdio/tauri-service` with `tauri-plugin-wdio-webdriver`, a WebDriver server embedded in the app, which is the only route that works on macOS (`tauri-driver` has no WKWebView driver there). It would need the plugin behind a dev-only Cargo feature, the WebdriverIO dev dependencies, a test build and its own data dir. Firefox was ruled out: no Tauri platform uses Gecko.

### Agents: pi's cache-miss notices never reach Alethiometer (user, 2026-10-21: on hold)

pi (`showCacheMissNotices: true`) computes them only in its TUI (`interactive-mode.js`, `core/cache-stats.js` `detectCacheMiss`), never over RPC, so pi-acp has nothing to forward. pi-acp's `usage_update` carries only the running cost and context size, once per turn, so Alethiometer can't tell a miss from it reliably. The per-call usage (input, cacheRead, cacheWrite, cost split, model, timestamp) is in pi's `message_end`, which pi-acp drops. Options put to the user: port `detectMiss` into the pi-acp fork and send a notify chunk, or forward the usage in `_meta` and compute here. Also: pi `notify` chunks (`_meta.piAcp.notify`) are merged into the agent's output text today instead of a line of their own.

### Checked by tests only, not on screen (2026-10-21)

Playwright isn't in the Mac devshell, so these weren't looked at: the toasts (top right, under the nudge), Incoming's keys sheet and shorter hint bar, the `.` menu, the brighter ⋯, the note editors at line-height 1.45, Settings › Slack/Teams › Ignored and its Add… picker. New ui-keys checks (`scripts/ui-keys/incoming.mjs`, the Incoming keys and sticky New agent cases in `modal.mjs`) haven't been run.

### Ignore suggestions: what's left (2026-10-17)

Built (HANDOFF.md, "Ignore suggestions"); not yet run on real data, so the thresholds are untested in practice.
- The line shows only while the dismissed row is still on screen (until the selection moves and the list reloads). It comes back on the next dismiss of the same key until Not now.
- No list of current candidates or past Not nows; a Not now can't be undone before its 30 days.
- 1:1 mail is decided from headers (two addresses across From/To/Cc, no List-Id/List-Unsubscribe/Precedence bulk/Auto-Submitted, sender not a no-reply address). A person mailing from a service without those headers counts as 1:1.
- A domain is suggested once two addresses under it were dismissed; freemail domains (gmail.com) aren't excluded beyond that and the share threshold.
- GitHub and Jira count by repo and project only, not by notification reason or reporter.
- Gmail threads already in the Inbox carry no keys until they change; a refetch would add them.

### Microsoft Teams: built, never run against a tenant (2026-10-14)

Built from Graph v1.0's documentation and recorded shapes (`app/docs/sources/teams.md`, `tests/fixtures/teams/`). To check on a real tenant, chats first:

- Connect with the chat permissions only: does user consent work in this tenant, and does Entra accept the `http://localhost:<port>` redirect for a Public client/native registration (it should ignore the port for `localhost`; a browser that tries `::1` first must fall back to 127.0.0.1, where the listener is).
- Message ids being creation times in milliseconds: everything that orders messages (runs, mark read) assumes it, as observed in Graph's examples; it isn't a documented guarantee.
- `/me/chats?$orderby=lastMessagePreview/createdDateTime desc&$expand=lastMessagePreview` and chat messages filtered by `lastModifiedDateTime`: documented, but check that edits and new messages both show.
- Channels (after admin approval): the order of `GET …/channels/{id}/messages` (newest first is assumed only to stop paging early) and whether a new reply changes its post's `lastModifiedDateTime` (assumed; tracked threads are also checked on a timer, so a reply is caught within an hour either way).
- `markChatReadForUser` with your user and tenant ids; the "newest message" check before it.
- Replies as HTML with `<br>` line breaks.

Not built: push through Azure Event Hubs (Graph only pushes to a public HTTPS endpoint; a later stage, user 2026-10-14), a channel picker (channels are pasted as links), mention search in channels you don't list, the reaction UI (as for Slack). In a Teams chat ⌥T (reply in a thread) fails with a message instead of being hidden.


### Windows: not yet run on a Windows machine (2026-10-14)

Windows type-checks (all targets, Tauri on) but nothing has been built or run on Windows (the cloud sessions can't fetch the MSVC SDK). The checklist for the first run is `app/docs/windows.md`. What to check, and what is known but not done:

- `cargo test` in every crate and `npx tauri build` on Windows. `concurrent_writers_never_leave_a_mix` (`platform/src/fsx/tests.rs`) is the one most likely to fail: eight threads renaming over one file can hit a sharing violation on Windows. If it does, `fsx::write_atomic` needs a bounded retry on Windows; none was added without seeing the failure.
- `fsx::write_new` hard-links into place: fine on NTFS, fails on FAT/exFAT and some network or cloud-synced folders (a vault there can't take stand-in copies).
- WebView2's browser keys (Ctrl+R / F5 reload, Ctrl+F, Ctrl+P) reach the browser when no handler takes them. Tauri 2.12 has no setting for wry's `with_browser_accelerator_keys`; check what reloading mid-session does.
- Data and models live in Roaming `%APPDATA%` (user, 2026-10-14: keep the default), so a roaming domain profile would copy the database and the embedding model.
- "this Mac" in Settings › LLM Providers and the Notes margin.
- **Local agents on Windows** (agent host on AF_UNIX + job objects, `docs/agents.md` › Agent host): run `cargo test` in `app/agentd` (`kill_ends_the_agents_job_and_nothing_else` is Windows-only) and `app/src-tauri` (`tests/agents_restart.rs` now runs there, with node), then a real Claude Code and Pi session. Things only a Windows run can show: whether `npx.cmd` starts the adapters with their arguments (Rust refuses batch-file arguments it can't quote safely); whether the host survives the app's job object (`spawn_detached` breaks away only when the job allows it; otherwise closing the terminal that ran `npx tauri dev` may end it); that `alethiometer-agentd.exe` lands next to the installed app; that swapping in a new build (moving the running `.exe` aside to `.old`) works. Claude Code on Windows wants Git Bash.
- The approval card's Parsed panel reads commands as bash with POSIX paths: a `C:\…` working directory is one path segment, the "outside the repo" check only knows `/` and `~`, and `node_modules/.bin` lists `.cmd`/`.ps1` twins. It only explains; it gates nothing.
- Open in a terminal gives a cmd window; OpenShell and Scion attach lines are POSIX-quoted (both out of scope on Windows, user 2026-10-14). Open in Zed needs `zed` on the PATH.

### Agents: OpenShell sandboxes, what's left (2026-10-08)

Built (`app/docs/agents.md` › OpenShell sandboxes) from OpenShell's source and docs, tested against `scripts/fake-openshell.mjs`. Checked by hand on a local podman gateway at OpenShell 0.1.4 (2026-10-16), not yet through the app: `sandbox create --detach --upload` (the upload lands at `/sandbox/<directory name>`, `.gitignore` applied), `exec --no-tty` streaming held-open stdin line by line, the `aleth-pi` image's login shell printing nothing, `rule get` text parsing as `parse_rules` expects, `rule reject`, `sandbox download` (the directory's contents into the destination), `sandbox delete` of an absent sandbox exiting 0, and Pi reaching Bedrock through the `bedrock` provider. 0.1.2 and older block `exec` until stdin closes (fixed upstream in #4006), so they can't run ACP. To check against a real gateway, local and remote:

- **Each `sandbox exec` takes at most 4 MiB of stdin in total** (`MAX_EXEC_STDIN_BYTES` in openshell-cli, kept because the supervisor's stdin queue is unbounded). Past it the CLI exits mid-message, so a few pasted screenshots end the agent. Not handled. The direction under consideration: ACP over an OpenShell `forward`/`service` port instead of exec's stdin, like the agent host's socket.
- Pi on Bedrock needs `AWS_BEDROCK_FORCE_HTTP1=1` in the harness's `openshell.env`: its AWS SDK client defaults to HTTP/2 and OpenShell's TLS proxy only negotiates http/1.1 (`tlsv1 alert no application protocol`).
- `sandbox exec --no-tty` with stdin held open for hours, and what a laptop sleep or a gateway restart does to it (the agent should show as exited and resume on the next message).
- Stop kills the `openshell` CLI process; whether the gateway then ends the adapter inside the sandbox is unchecked. If it doesn't, a second exec would start a second adapter on the same session.
- claude-agent-acp in auto mode under OpenShell's network policy (auto needs a model that supports it; the claude-code provider profile allows only api.anthropic.com and telemetry).
- pi permission extensions against `openshell::auto_allow`: that the gated call is shown pending when the extension asks (pi-acp 0.0.34 does, from the streamed tool call), and their choice names (allow by name: yes/allow/approve/ok/proceed/continue/run, not if it has no/deny/reject/block/cancel/don't).
- `rule get --status pending` has no JSON output: `openshell::parse_rules` reads the labelled text of openshell-cli's `sandbox_draft_get` (unchanged from `67adcf1` to v0.1.4). A relabel upstream breaks it quietly (no approvals show). Worth asking OpenShell for `-o json` there.
- `sandbox download` of a directory with a big `node_modules` the agent installed: it's all downloaded before the git-ignore filter drops it. Fine for a project, slow for a heavy one.

Not built: bringing changes back from a sandbox that was already there (no record of what went in, no local directory); the model list from inside the sandbox; the facts box (git runs here, where the sandbox path doesn't exist), and Explained's script reads, which find nothing for the same reason. Bring back writes files only; it never commits, and a binary file in conflict is listed like any other (no merge).

### Agents: local runtimes (podman sandbox, ssh) (user, 2026-10-03: deferred; OpenShell built 2026-10-08)

Running a harness somewhere other than the host, e.g. the user's podman `claude-sandbox` (`~/.config/home-manager/claude-sandbox`), which Zed reaches over ACP today. Proposed shape, mirroring Scion's harness × broker: an `agents.runtimes` list in settings.json (`instances`, `start`, `exec` prefix, `cwd` template, `editor` URL, `mode`), harnesses gain an optional `runtime`, and everything that touches the session's files (the adapter, git for the branch fact, Explained's script reads) goes through one `exec` + `env -C {cwd}` helper. Today all of those assume the session directory is on the host; Explained reading the host copy of a sandboxed project would be wrong, not just missing. Sandboxed sessions would use Claude's `auto` mode.

### UI checks: two flaky or failing scenes (found 2026-10-08)

- **`todos-done` (ui-diff) races focus.** After the first todo is confirmed (`c`), the next `n` opens the todo form on the next signal but its field sometimes isn't focused, so "Reply to Dana" is typed into nothing and no second todo is made. Which side of the race a run lands on depends on timing (it flipped when the front end was reformatted, and reproduces on the unformatted UI with 400 ms between keys), and the later "Done and dismissed" scenes then start from different data and differ too. Either the form should take focus reliably after a confirm (an app bug, if a person typing fast hits it too), or the scene should wait for the field.
- **ui-keys `picker · font picker: ↓ ↵ picks, and the setting follows`** fails on fresh servers (the setting doesn't change), with or without the Prettier reformat. Not looked into.

### Agents tree: ages ignore the clock offset for their first second

`views/agents/Tree.svelte` starts `now` at `Date.now()` without `ui.info.clock_offset_ms`; the 1 s interval adds it. So with a fake or offset clock, ages read one value (e.g. "5d") until the first tick and another ("6d") after. Found by the ui-diff sharding work (2026-10-06), where a shot racing that tick split the two sides; the scenes now wait past it. Not fixed: the fix is one line, but it needs a test.

### Agents: the attach terminal needs `x-terminal-emulator` on Linux (2026-10-08)

`Platform::open_terminal` (`app/src-tauri/src/platform.rs`) runs `x-terminal-emulator`, which only Debian-family distros have; on Fedora the Scion attach button fails with "No such file or directory". Fix when Scion is used from Linux: try `$TERMINAL`, `xdg-terminal-exec`, then known terminals. The button is now hidden for local sessions and rooms, which have nothing to attach to; the backend's `("acp", "terminal")` arm in `agents/mod.rs` is unreachable from the UI.

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

### Jira field history: what's left (2026-10-09)

- Not checked on screen (no Playwright in the Mac devshell). Order is "in order" only; the user wants to compare against a separate section once seen.
- An existing issue gets its history only when it next changes in Jira (unchanged `remote_rev` skips it).
- A change that doesn't bump (priority, sprint, by someone else) is stored unread, but a later bump on a triaged issue marks everything before it read (`mark_read_if_triaged`), so it shows faded though never seen. Same as messages today.
- Every field is listed, noise included (Rank). Long or multi-line values show as "Field changed".
- Jira's changelog via `expand=changelog` may be capped per issue (unverified how many and which end); `bump` has the same exposure.
- Only Incoming shows it; a topic's chronology doesn't.
- Jira families split before this stay split.

### Slack messages as "updates" on filed items

Unseen items with a `confirmed` link now show under their topic in Incoming as updates (with Confirm, like any signal). A busy Slack thread filed under a topic would resurface on every new message; the user is designing how Slack should behave here. Jira counts status/assignee changes and comments by others as updates.

### Sample data writes vault files inside its database transaction

`seed::seed_if_empty` writes the stand-in vault's People, Topics and Reflect notes while its unit of work is open. It runs once, on an empty database with no real vault, so nothing waits on it, but it breaks the rule of keeping file IO outside the database. Fix if the seed grows: write the files first, then the rows.

### Obsidian notes: live-preview editing with the vault's theme (user, 2026-10-05: deferred)

Notes are shown by our own `NoteText.svelte` and edited in a plain textarea, so a note goes from formatted to raw text the moment you edit. Idea: a CodeMirror 6 editor (as Obsidian uses) with decorations that hide markdown syntax except on the line being edited, styled by the theme named in `<vault>/.obsidian/appearance.json` (`cssTheme`, CSS in `.obsidian/themes/<name>/`, plus snippets). Needs: Obsidian's class names (`.cm-header-1`, `.HyperMD-list-line`, `.markdown-rendered`) and baseline CSS variables the theme doesn't define; our own decorations (Obsidian's live preview is closed; check for an open-source package first); tables, callouts and embeds last. Saving (conflicts, outbox) stays as it is. Start with the Topics note pane, then Reflect. Fidelity to the theme would be close, not exact; plugin-added styling won't carry over. Embedding Obsidian's own rendering isn't possible.

### Database Phases 2–4: unverified on real data

Migrations 23–32 (message keys, `sync_streams`, `remote_rev`, `items.rev`, outbox lease, derived topic values, corpus revisions, the embedding queue, Incoming indexes, the FTS update trigger) were tested on fixtures and scratch databases built from sample data, not on the user's database. Check the first start's time and that Incoming, Topics and Notes look the same afterwards. Message keys were checked on the sample data only: real rows without an id get a hashed key, and a GitHub or Jira comment re-read later moves to its id key, keeping its read state.

### Live queries re-run per table, not per row

Any write to `items` re-runs every subscription that read `items`, Incoming's included (0.13–0.15 s with 1,000 unseen signals, debug build; the list no longer carries message text). That's fine at today's write rate. If a busy sync makes it show, filter by row (the plan's next step) or page `incoming.list`.

A topic or Reflect page open on screen shows a note edited in Obsidian when the Obsidian source's watcher sees the file change. Paths that source ignores, and the sample data's stand-in vault (no watcher), show within a minute, at the periodic re-run.
