# Running issues

Found while wiring up connectors on the real Mac. Not in HANDOFF.md's "in flight" list — track separately until triaged in there.

## Open

### Priorities: scoring isn't built, and needs design first

The Priorities tab (8b) only ever shows sample data: nothing computes quadrants, scores or reason pills for real items, so with `demo_data: false` it's empty. The design never decided what makes something urgent or important. Inputs worth considering: due date, todo estimates (`todo_estimates`, built), who's asking, Jira status, mentions, an upcoming meeting on the topic. Also open: todos only, or every open signal (as 8b shows). Design pass first, then build.

### Todos: completing a Jira todo assumes a "Done" transition

`x` on a Jira todo queues `jira.transition` to "Done" (matched by transition name or target status). The DEVOPS project's done status is "Closed", reached only through a couple of intermediate steps, so completing a DEVOPS todo fails in the outbox. Needs a per-project done status and a way to walk the workflow (or open the issue in Jira instead). Creating issues is fine as is.

### Gmail: what's left

Built: Incoming mirrors the whole Inbox (read or not), filing labels by category (`config.category_labels`), marks read and archives (todo or not), Dismiss archives, `#` trashes, `u` restores, moving a filed thread to a topic in another category relabels it (docs/sources/gmail.md), a chip showing the thread's own current Gmail label matched to a category. Still open: Unsubscribe (19a), assistant help writing Gmail filters (needs `gmail.settings.basic`).

### Expanded signal layout in Incoming needs a design pass

Interim fix in `views/Incoming.svelte`: when a signal is expanded, the title gets its own full-width line (wrapping instead of truncating), and below it one muted info line joins `meta` (author · time) with the connector's `summary`, dropping ` · ` parts that repeat. Found on Jira issues, where `meta` names the reporter and `summary` is "Status · assignee", so the same name showed twice and the header was squished next to the action buttons. Still unlabeled when reporter and assignee differ ("Alice · 10:41 · Open · Bob"). The user wants to adjust the design; labeling roles would mean changing the Jira connector's `summary`, which the Topics timeline also shows.

### Slack messages as "updates" on filed items

Unseen items with a `confirmed` link now show under their topic in Incoming as updates (with Confirm, like any signal). A busy Slack thread filed under a topic would resurface on every new message; the user is designing how Slack should behave here. Jira only counts status/assignee changes by others and @mentions as updates, not plain comments.

### Changing `obsidian_vault` strands notes saved in the stand-in vault

With `obsidian_vault` empty, Reflect (and Topics) write to `<data dir>/vault`. Setting `obsidian_vault` later switches every read to the real vault and nothing moves the stand-in's files, so earlier reflections silently disappear from Reflect. Hit when connecting Obsidian on the work Mac: `Reflections/Daily/2026-09-28.md` was copied over by hand (the original is still in the stand-in). Fix direction: on startup, if `obsidian_vault` is set and `<data dir>/vault` has files, offer to copy them over (never overwrite), or at least warn.
