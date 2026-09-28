# Obsidian (work vault)

Tag: `work`. Reads Markdown files straight from disk; writes by appending a line or flipping one checkbox. No API, no plugin and no secrets.

## Configure

Add a source to `settings.json`:

```json
{
  "obsidian_vault": "/Users/me/Work vault",
  "sources": [
    {
      "kind": "obsidian",
      "tag": "work",
      "name": "Work vault",
      "poll_secs": 60,
      "config": {
        "ignore": ["Archive/", "*.excalidraw.md"]
      }
    }
  ]
}
```

| Config key | Default | Meaning |
|---|---|---|
| `vault` | `settings.obsidian_vault`, else `<data dir>/vault` | Absolute path to the vault folder. Only needed if this source should read a different folder than the one Topics and Reflect use. |
| `ignore` | none | Extra patterns to skip, added to the built-in list below. `Folder/` matches a folder, a pattern containing `/` is anchored at the vault root, anything else matches the file name. `*`, `**` and `?` work as in `.gitignore`. |

Always skipped, whatever `ignore` says: hidden files and folders (`.obsidian/`, `.trash/`, `.git/`…), `Reflections/` (Alethiometer's own reflection notes; the Life lens must never become work items), `Templates/` and `templates/`, symlinks, and anything that isn't `.md`.

Alethiometer doesn't add an Obsidian source by itself when `obsidian_vault` is set. Reading the whole vault into Alethiometer is your call, so add the source yourself.

The vault must only sync through BAA-covered storage (see the boundary rules). Alethiometer doesn't check that.

## Sync

- **Live:** a file watcher (FSEvents on the Mac, inotify on Linux) triggers a sync about a second after a Markdown file stops changing.
- **Poll:** the vault is also rescanned every `poll_secs`, capped at 60 s, in case the watcher misses something or can't start. A rescan only stats files; it re-reads the ones whose modification time changed.
- The cursor records every file's modification time, so renames, deletions and files synced in with old timestamps are all noticed.

## What is read

| Item | From | Fields |
|---|---|---|
| `note` (one per file) | every `.md` file | title = file name, body = content (first 64 KB, for search), summary = first line that isn't a heading, link `obsidian://open?path=<absolute path>`, time = modified time |
| `task` (one per checkbox) | `- [ ] …`, `* [ ]`, `1. [ ]` lines outside code blocks | title = task text without due/done markers, owner "you", due from `📅 2026-10-01` (Tasks plugin) or `due:: 2026-10-01` (Dataview), shown as a day (no time). `[x]`, `[X]`, `[-]` = done; anything else is open |

A task's id is its note's path plus a hash of its words, so adding a due date keeps the same item. Editing the words makes a new item and closes the old one.

### How it shows up

- **First sync:** everything is imported as already seen, so the existing vault doesn't flood Incoming.
- **Notes** never show up in Incoming. They're your own writing, and every save would resurface them. They're searchable, and new ones can be filed from search.
- **New tasks** show up in Incoming (Unsorted until they're filed), except…
- **A topic's own note** (`topics.obsidian_path`) and every task in it are linked to that topic as confirmed and marked seen. You put them there, so there's nothing to triage. A link you've marked Wrong stays wrong.
- **Removed tasks** (checked off, edited or deleted), and every item from a deleted or renamed file, are set to `done`. Nothing is ever deleted from Alethiometer. After a rename the note comes back as a new item, and links you confirmed on the old one stay on the old one.

## What is written (outbox ops)

Both ops re-read the file, check that the target is unambiguous, write to a hidden temp file next to it and rename it over the original. Just before the rename they check that the file still holds exactly what was read. If it doesn't, the op fails with "the note changed while writing; retry". They never guess.

Paths are relative to the vault (absolute paths inside the vault also work). `..`, paths outside the vault, symlinks that lead out and non-`.md` files are refused.

### `obsidian.append`

```json
{ "path": "Topics/Datadog renewal.md", "text": "- [ ] Ask about SSO", "under_heading": "Asks for Thu" }
```

- Appends one line (multi-line text is refused). With `under_heading`, the line goes at the end of that heading's section, before any blank lines. The section ends at the next heading of the same or a higher level. `"## Asks"` matches only a level-2 heading; `"Asks"` matches any level.
- The heading is missing: it's added at the end of the file (at the given level, else `##`), followed by the line.
- The heading appears more than once (outside code blocks): the op fails.
- The note doesn't exist: the op fails, unless `"create": true` is passed (the folder must already exist).
- Keeps the file's line endings (LF or CRLF).
- **Idempotent.** The exact line may already be where it would go: the last non-blank
  line of that heading's section, or of the file without `under_heading`. Then nothing
  is written and the receipt says `"changed": false`, with that line's number. A retried
  write (for example after a crash between writing and storing the receipt) never adds
  the line twice. The flip side: appending the same line twice in a row on purpose does
  nothing the second time. The same words elsewhere in the note don't count, and
  "exact" includes whitespace.

### `obsidian.toggle_task`

```json
{ "path": "Topics/Datadog renewal.md", "task_text": "True-down at 12 months", "done": true }
```

- Finds the checkbox line whose words match `task_text` (due and done markers are ignored on both sides) and changes only the character between the brackets: `x` for done, a space for open.
- No match, or more than one: the op fails.
- Already in that state: nothing is written (`"changed": false`). The op sets a state
  rather than flipping it, so it's idempotent too: a retry of `done: true` on a task
  that's already checked writes nothing.

Receipt for both:

```json
{ "path": "Topics/Datadog renewal.md", "changed": true, "line": 12, "mtime": "2026-09-28T10:52:03", "mtime_ms": 1790592723000 }
```

The next sync picks up the change right away through the watcher.

## Known limits

- `obsidian.append`'s retry check only sees its own line at the end of the section. If you add more lines to that section before a failed append is retried, the retry appends the line again.
- A small race remains between the final check and the rename. If Obsidian saves in that moment, its save wins over ours.
- Only files are read. Frontmatter, links and tags go into the note body for search but aren't parsed.
