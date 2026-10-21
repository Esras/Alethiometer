# Obsidian (work vault)

Tag: `work`. Reads Markdown files straight from disk. Writes go **through the running Obsidian app** by default, via the [Local REST API with MCP](https://github.com/coddingtonbear/obsidian-local-rest-api) community plugin, so open editors and your sync plugin see ordinary Obsidian edits. A direct-file mode remains for vaults without the plugin.

## Set up the plugin (write mode `obsidian`, the default)

Checked against plugin version 5.3.0 (its `docs/openapi.yaml` and `src/`).

1. In Obsidian: Settings → Community plugins → Browse → **Local REST API with MCP** → Install → Enable. The HTTPS server starts on `https://127.0.0.1:27124`.
2. Copy the API key from Settings → Local REST API with MCP and store it as the secret **`obsidian.api_key`** (Keychain on the Mac; `<data dir>/secrets.json`, mode 0600, on Linux). It never goes in `settings.json`.
3. Nothing to do for the certificate in most cases, see below.

Alethiometer uses two endpoints: `GET /vault/{path}` with `Accept: application/vnd.olrapi.note+json` (the note's `content` plus `stat.mtime`; 404 when missing) and `PUT /vault/{path}` with a `text/markdown` body (the plugin calls Obsidian's `vault.modify`, or `vault.create` for a new note; creates folders). Auth is `Authorization: Bearer <key>`.

### Certificate and port

The plugin generates its own certificate authority (name-constrained to `127.0.0.1` and `localhost`) and serves a certificate for `127.0.0.1` signed by it. Alethiometer **trusts exactly that CA and nothing else** for this connection (no system roots, no "disable verification"), and never sends it through a proxy.

- By default the CA is read from the plugin's settings in the vault: `<vault>/.obsidian/plugins/obsidian-local-rest-api/data.json`, field `crypto.caCert` (only that field is used; the file also holds the API key and private keys, which Alethiometer doesn't touch). Older installs that predate the CA have one self-signed certificate that doubles as the CA; rustls refuses that shape, so regenerate the certificate in the plugin's settings (it offers this).
- If your Obsidian config folder isn't `.obsidian`, or you want to pin a file yourself, set `config.cert_path` to a PEM file (download it from `https://127.0.0.1:27124/obsidian-local-rest-api.crt`) or to the plugin's `data.json`.
- Use `https://127.0.0.1:27124`, not `localhost`: the served certificate names the IP address.
- **Plain HTTP:** if you enable the plugin's insecure server (port 27123), set `"rest_url": "http://127.0.0.1:27123"`. This is only accepted for `127.0.0.1`/`localhost`/`[::1]`. The trade-off: any local process that can sniff loopback traffic sees the API key; HTTPS with the pinned CA doesn't have that problem.

If Obsidian isn't running (or the plugin is off), writes fail with "Obsidian isn't open (or the Local REST API plugin is off). Open Obsidian and retry (⌘R)." and stay in the outbox until you retry. A wrong key fails with a message naming the secret.

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
        "ignore": ["Archive/", "*.excalidraw.md"],
        "write_mode": "obsidian",
        "rest_url": "https://127.0.0.1:27124"
      }
    }
  ]
}
```

| Config key | Default | Meaning |
|---|---|---|
| `vault` | `settings.obsidian_vault`, else `<data dir>/vault` | Absolute path to the vault folder. Only needed if this source should read a different folder than the one Topics and Reflect use. |
| `ignore` | none | Extra patterns to skip, added to the built-in list below. `Folder/` matches a folder, a pattern containing `/` is anchored at the vault root, anything else matches the file name. `*`, `**` and `?` work as in `.gitignore`. |
| `write_mode` | `"obsidian"` | `"obsidian"`: writes go through the running app (Local REST API plugin). `"file"`: writes go straight to the files (temp file + rename), for vaults without the plugin. Reads always scan the files. |
| `rest_url` | `https://127.0.0.1:27124` | The plugin's address. `http://` is allowed on loopback only. |
| `cert_path` | the plugin's `data.json` in the vault | PEM file (or `data.json`) holding the CA to trust for `https`. |

Secret: `obsidian.api_key` (write mode `obsidian` only).

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

### People notes

`People/*.md` are also read as People (their frontmatter lists each person's accounts and colour), whenever a sync touches `People/` and at startup. Creating a person or linking an account from Incoming writes the note with `obsidian.replace`. See `docs/people.md`.

## What is written (outbox ops)

Every op reads the note first and computes the new text itself; none of them merges or guesses.

- **Write mode `obsidian`:** read with `GET` (as Obsidian has it), then `PUT` the whole new text. The plugin has no conditional write for `PUT` (its `If-Match` only applies to `PATCH`), so the check happens right before the `PUT`; the window is one local round trip. `PATCH` isn't used: its heading operations own the whitespace (an appended list item becomes a separate list), target headings by full path, and a `PATCH` of the whole document keeps the old front matter and trims blank lines, so it can't carry "exactly this text".
- **Write mode `file`:** read the file, write a hidden temp file next to it and rename it over the original. Just before the rename it checks the file still holds exactly what was read; if not, "the note changed while writing; retry".

Paths are relative to the vault (absolute paths inside the vault also work). `..`, paths outside the vault, symlinks that lead out and non-`.md` files are refused, in both modes.

### `obsidian.replace`

```json
{ "path": "Topics/Datadog renewal.md", "base": "## Where we are\n…", "text": "## Links\n…\n## Where we are\n…" }
```

The whole note, as edited in Alethiometer: anything anywhere, including reordering. `base` is the text the edit started from (`null`: the note didn't exist, and it's created with its folders).

- The note holds `base`: `text` is written.
- The note already holds `text` (a retry after the write landed): nothing is written, `"changed": false`.
- Anything else: the op fails with `conflict: the note changed in Obsidian since you started editing; nothing was written`. **Nothing is merged.** The editor then offers *reload* (take Obsidian's version, drop yours) or *keep mine* (re-send your text with the note as it is now as the new base, still checked).
- `base` is required, so a replace can't be sent unguarded.
- Once a replace of a note goes through, earlier failed or queued replaces of that note are deleted (`Outbox::drop_superseded`; migration 0037 cleared the ones already stuck): they're whole-note writes the later one already overtook, and retrying one would only conflict again (user, 2026-10-21).

Used by the Topics note editor and Reflect (see below).

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

Receipts:

```json
{ "path": "Topics/Datadog renewal.md", "changed": true, "line": 12, "via": "obsidian" }
{ "path": "Topics/Datadog renewal.md", "changed": true, "line": 12, "via": "file", "mtime": "2026-09-28T10:52:03", "mtime_ms": 1790592723000 }
```

(`line` only for append/toggle.) The next sync picks up the change right away through the watcher.

## Editing notes in the app

- **Topics:** the note pane shows the preview; `e` turns it into a plain-text editor (same type and colours), `⌘S` saves, `esc` or clicking away saves and returns to the preview, `o` opens the note in Obsidian. A topic with no note linked gets `Topics/<topic name>.md` (characters not allowed in file names become `-`); the file is created on the first save and the path is then stored on the topic. The header shows `saving…`, `saved`, `conflict: changed in Obsidian · reload (r) / keep mine (k)` or `Obsidian isn't open · ⌘R retry`.
- **Reflect:** the reflection note autosaves as before (debounced), through the same `obsidian.replace`.
- A save is one outbox row. Typing more while it's still queued (or while Obsidian is closed) updates that row's text instead of queuing another, so ⌘R later sends your latest text once.
- **No obsidian source configured** (the sample data: its "Obsidian" source is a placeholder with no vault): there's no Obsidian to talk to, so saves write the file under the vault folder directly, with the same base check and conflict UI. That keeps the sample app editable.

## Known limits

- `obsidian.append`'s retry check only sees its own line at the end of the section. If you add more lines to that section before a failed append is retried, the retry appends the line again.
- A small race remains between the check and the write (the rename in `file` mode, the `PUT` in `obsidian` mode). If Obsidian saves in that same moment, one of the two writes is lost.
- The editor's base is the file on disk when the topic loaded. If Obsidian hasn't saved an edit to disk yet (it does so within about two seconds), the save reports a conflict rather than overwriting it.
- A custom `authorizationHeaderName` in the plugin's settings isn't supported (the default `Authorization` header is used).
- Only files are read. Frontmatter, links and tags go into the note body for search but aren't parsed.
