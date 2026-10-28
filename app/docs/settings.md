# Settings

The Settings modal (design 26a General, 27a Sources, 26c AI, 25d find). Open it with the cog next to the sync status or **⌘,**. **⌘⇧,** opens `settings.json` itself. Esc closes it.

## Live reload

Every change writes `settings.json` right away (temp file + rename). There's no Save button. The app re-reads the file and applies only what changed. The header says how:

- **Saved · live**: most settings (day strip, buffers, Topics, People, AI, type, `ui_scale` through the webview zoom).
- **Saved · Slack reconnected**: a source's settings changed, so only that connector restarted. Each source's loops carry a generation number and stop when it moves on. A search-only source (Glean) says "updated".
- **Saved · restarting**: the vault path (`obsidian_vault`) and `fake_now` can't apply live, so the app restarts itself. The dev server can't, and says "restart Alethiometer to apply".

Hand edits are picked up the same way: a file watcher on the data dir reloads `settings.json`. A file that doesn't parse or validate is refused. The app keeps the last good settings and Settings shows the error in its header. Opening the app with an invalid file still works: it runs on the last settings that were good (`settings.last-good.json`, kept on every successful load and change), or on defaults without sample data if there are none, shows the error, and never writes over the broken file. Categories are part of validation, so a bad colour or repeated key is refused before it applies.

`ai.allowed_providers` can't be changed from Settings. It's the safety gate for work data, so it stays a hand edit.

## Schema

Rows come from the JSON schema of `Settings` (`settings/mod.rs`) and of each connector's typed config (`settings/sources.rs`), generated with `schemars`. A field's doc comment is its description: the first line shows. Most fields have none: a description is only there when the setting does something non-standard or surprising. `x-section`, `x-group`, `x-label`, `x-unit`, `x-step`, `x-min`/`x-max`, `x-enum`, `x-widget` place and shape the row (`settings/schema.rs` flattens them).

Connectors still read their own config from JSON. `settings::sources::tests::defaults_match_connectors` keeps the typed defaults and the connectors' in step. Unknown keys are allowed, wrong types are refused.

## Sections

1. **General**: Window (`ui_scale`), Type (`ui_font`, `mono_font`, `ui_font_size`, `reading_font_size`), Day strip, Topics, Notes (vault), Data (sample data), Ignore suggestions (`ignore_suggestions`: after how many actions, what share dismissed, within how many days).
2. **Categories**: the Topics index category editor (writes `categories` back).
3. **People**: your colour, your accounts, when to suggest a People note.
4. **Sources**: one sub-item per source (a Scion Hub is one: `scion`, see `agents.md`), plus Add source. Pages: Identity, Secrets, What it reads (Categories on Google Calendar), Limits, Advanced (secret names, paths). A red dot means a required secret is missing or the last sync failed. ⇧X twice removes a source; its items stay. A rename keeps the source's data.
5. **LLM Providers** (section id `ai`): allowed providers (read-only), Bedrock region and profile, models (each tier has an on/off toggle, `ai.small.enabled` / `ai.large.enabled`) with today's usage, prices (editable for now; see ISSUES.md), local embeddings state.
6. **Agents**: a live preview, then Approvals (`agents.accept_edits`), Message colors (`←`/`→` picks a preset, Reset to default), Agent names, Boxes, and "<harness> in OpenShell" per harness: Policy file and Environment (`agents.harnesses[i].openshell.policy` and `.env`). The rest of the harness list is settings.json only; with no list in the file, the first edit writes the defaults in. `⌫` resets a row. See `agents.md`.
7. **Developer**: `fake_now` (dev builds only) and the settings path.

`life_calendars` moves into the Proton source's `urls` the first time the app opens the file.

## Secrets

Secrets are write-only: a page shows a secret's name, whether it's set and when (`secrets.meta.json`, names and times only), and Replace… / Set…. The value goes to `secrets.json` (0600) and is never read back to the UI. Replacing one restarts the sources that use it. Sign-in tokens (Google) are written by Connect, not pasted.

## Type

Interface sizes are rem off the root font size (`ui_font_size`, default 13). Reading text (messages, notes, drafts, answers) uses `--fs-read` (`reading_font_size`, default 14). `ui_scale` zooms both. Fonts are picked from installed families (`fontdb`), each shown in its own face.

## Check consistency

On Sources, the **Check consistency** entry under the source list (`c` opens it; `c` again checks) lists what older versions of the connectors left behind, per source with a count. Nothing changes until you press a finding's **Repair**; the checks run again after it, and a toast says how it went (the call keeps running if you close Settings). Code: `queries/consistency.rs` (`consistency.check`, `consistency.merge_senders`), `sync/repair.rs` (`sync.repair_authors`), `core/src/store/consistency.rs`.

- **Messages missing an author** (Gmail, Jira, GitHub; Slack's and agents' bots are meant to have none): own messages (not context) with no `author_id`. Repair reads those items again from the source (`Connector::refetch`), 25 per unit of work under the source's poll lock, after forgetting their stored revision so the same revision applies. It never resurfaces or marks anything seen, never creates an item, and leaves the cursor alone. Gmail reads threads whether or not they're in the Inbox. It stops at the first error (a rate limit) and says how many it did. GitHub reads all of an item's comments (up to 100), so older comments it never stored come in too.
- **Same sender on two people** (Gmail): a bare-address handle and exactly one `address name` twin on another person (ISSUES.md). Repair moves handles, accounts, item authors and topic links onto the row with more items (the lower id on a tie) and deletes the other; a pair with a People note is left alone.
- **Calendar leftovers** (Google Calendar): items done with no block that were neither cancelled (`summary = 'cancelled'`, `change = 'cancelled'`) nor declined (your attendee entry). Listed only.

## Keys

`j`/`k` setting, `1`–`7` or `⇥` section, `←`/`→` pick a colour (Agents), `⌫` reset (Agents), `⌥j`/`⌥k` previous/next source, `↵` edit, `−`/`+` step, `space` toggle, `r` replace secret, `a` add source, `⇧X` remove source, `s` sync now, `c` check consistency (Sources), `/` find, `esc` close.
