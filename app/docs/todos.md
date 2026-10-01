# Todos

What you've committed to, whichever system holds it (design 22a, 22c; code in `src-tauri/src/queries/todos.rs`).

A todo is an `items` row you own (`owner = 'you'`, `status = 'open'`) from a source that holds todos. The shared columns (title, due, topic link, priority) are the internal model; the per-source fields stay in `items.raw_json`, and `TodoExt` is their typed view. A row whose `raw_json` doesn't fit its source's shape isn't listed.

| Variant | Source | Complete (`x`) | Reopen (`u`) |
|---|---|---|---|
| `Local` | built-in `notes` source: made in Alethiometer, or a Notes "do" line | local write | local write |
| `Jira` | issues assigned to you, except projects in the Jira source's `config.todo_exclude_projects` (e.g. `["DPR"]`) | outbox `jira.transition` to Done | transition back to the status it had (To Do if that was Done) |
| `Obsidian` | `- [ ]` lines in the vault | outbox `obsidian.toggle_task` | same, unticked |

Jira and Obsidian rows change status only when the outbox confirms (`outbox::on_confirmed`), as for every other write. Marvin will be a fourth variant; it isn't built.

## Making one

- **From a signal (Incoming, `n`):** the todo is *held* (`held_todos`) and nothing is sent. Confirming or moving the signal creates it with the topic it was filed under (`todos::settle`), and a Gmail thread is then archived. Wrong topic leaves it held for whichever topic comes next. Dismiss discards it; undoing the dismiss brings it back.
- **From the Todos tab (`n`):** created at once, with an optional topic.
- **Local:** a new item on the `notes` source, remembering the signal it came from (`from`, `from_item`, its url).
- **Jira:** `jira.create_issue` with `assign_me` in the project from `config.create_project` (type `config.create_issue_type`, default Task), plus the due date. When Jira confirms, a stub item with the issue key stands in until the next poll updates it in place; it's filed under the topic.

Making a todo never writes to the topic's Obsidian note. A todo shows on its topic through its topic link; `- [ ]` tasks written in a topic's note are linked to that topic by the Obsidian sync.

## Estimates

Each todo can carry an estimate in minutes (`todo_estimates`), shown in the Todos tab's Est. column. The small AI tier guesses one for todos that have none (`ai/estimate.rs`: after each sync, and when the Todos tab opens), snapped to 5m, 10m, 15m, 30m, 45m, 1h, 1h30m, 2h, 3h, 4h or 8h, and shown in muted italics with a `~`. `e` (or a click) lets you type your own (`45m`, `1h30m`, `2h`, `1d` = 8h, or plain minutes; ↵ saves; over 2h shows a muted "split?"); yours is never overwritten, and saving it empty hands it back to the AI. Jira epics are never estimated (shown as `-`): you track them, the work is in their issues. Estimates stay in Alethiometer and are never sent to Jira; a future Marvin variant would send them as Marvin's time estimate.
