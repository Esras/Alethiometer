# Running issues

Found while wiring up connectors on the real Mac. Not in HANDOFF.md's "in flight" list — track separately until triaged in there.

## Open

### Keys: ⌃k opens the palette instead of moving up a list (found 2026-10-05)

`mod()` in `lib/keys.svelte.ts` says "⌘ on macOS, Ctrl elsewhere" but returns `metaKey || ctrlKey` everywhere, so App's ⌘K handler (`App.svelte:40`) also takes ⌃k. The pickers, the palette and the Topics index list ⌃j/⌃k for moving the cursor (`lib/ui/listNav.ts`); ⌃j works, ⌃k opens the palette, except inside Settings (a key floor). Fixing `mod()` per platform changes every ⌘ shortcut's Ctrl twin on the Mac, so check each before changing it.

### Agents: local runtimes (sandbox, OpenShell, ssh) (user, 2026-10-03: deferred)

Running a harness somewhere other than the host, e.g. the user's podman `claude-sandbox` (`~/.config/home-manager/claude-sandbox`), which Zed reaches over ACP today. Proposed shape, mirroring Scion's harness × broker: an `agents.runtimes` list in settings.json (`instances`, `start`, `exec` prefix, `cwd` template, `editor` URL, `mode`), harnesses gain an optional `runtime`, and everything that touches the session's files (the adapter, git for the branch fact, Explained's script reads) goes through one `exec` + `env -C {cwd}` helper. Today all of those assume the session directory is on the host; Explained reading the host copy of a sandboxed project would be wrong, not just missing. Sandboxed sessions would use Claude's `auto` mode.

### Agents: unverified against real agents and a live Scion Hub

The ACP client is tested against recorded claude-agent-acp traffic and a scripted stand-in (`app/scripts/fake-acp.mjs`); it has never driven a real Claude Code or Pi session. The Scion client is built from Scion's Go source and hand-written fixtures; it has never talked to a Hub. The list of things to confirm is in `app/docs/agents.md` ("To check against a live Hub"). Claude's cost is read as a running total from `usage_update` (unverified), and the cache state is an estimate.

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

### Priorities: scoring isn't built, and needs design first

The Priorities tab (8b) only ever shows sample data: nothing computes quadrants, scores or reason pills for real items, so with `demo_data: false` it's empty. The design never decided what makes something urgent or important. Inputs worth considering: due date, todo estimates (`todo_estimates`, built), who's asking, Jira status, mentions, an upcoming meeting on the topic. Also open: todos only, or every open signal (as 8b shows). Design pass first, then build.

### Todos: completing a Jira todo assumes a "Done" transition

`x` on a Jira todo queues `jira.transition` to "Done" (matched by transition name or target status). The DEVOPS project's done status is "Closed", reached only through a couple of intermediate steps, so completing a DEVOPS todo fails in the outbox. Needs a per-project done status and a way to walk the workflow (or open the issue in Jira instead). Creating issues is fine as is.

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

### Database Phases 2–4: unverified on real data

Migrations 23–32 (message keys, `sync_streams`, `remote_rev`, `items.rev`, outbox lease, derived topic values, corpus revisions, the embedding queue, Incoming indexes, the FTS update trigger) were tested on fixtures and scratch databases built from sample data, not on the user's database. Check the first start's time and that Incoming, Topics and Notes look the same afterwards. Message keys were checked on the sample data only: real rows without an id get a hashed key, and a GitHub or Jira comment re-read later moves to its id key, keeping its read state.

### Live queries re-run per table, not per row

Any write to `items` re-runs every subscription that read `items`, Incoming's included (0.13–0.15 s with 1,000 unseen signals, debug build; the list no longer carries message text). That's fine at today's write rate. If a busy sync makes it show, filter by row (the plan's next step) or page `incoming.list`.

A topic or Reflect page open on screen shows a note edited in Obsidian when the Obsidian source's watcher sees the file change. Paths that source ignores, and the sample data's stand-in vault (no watcher), show within a minute, at the periodic re-run.
