# People, person colours and the split review

Design: 15g/15h (split review), 16c (person palette), 17a (new person), 17b (link to person), the People tab (design canvas "People and the graph", decisions.md › "People tab design"). Code: `src-tauri/src/queries/people.rs`, `queries/incoming.rs` (review), `queries/split.rs` (moving messages), `src/lib/people.ts` (palette), `src/views/incoming/SplitReview.svelte`, `PersonPicker.svelte`.

## People are notes

A person is a note in the work vault, `People/<Name>.md`:

```
---
aliases: [Jordan, jlee]
slack: U04JRDN
email: [jordan.lee@work.com]
jira: 5f2c…
github: jlee-work
color: 212          # optional; index into the 256-colour palette
role: Staff engineer
team: "[[Platform]]"
---
Free notes…
```

- `slack`, `teams` (the Entra object id), `email`, `jira`, `github` are the person's accounts; each can be one value or a list. Emails and GitHub logins match case-insensitively.
- `color` overrides the hash colour. `role` shows under the name in a topic's People panel. Other fields are free-form; nothing reads them yet.
- The notes are the source of truth. `people::scan` mirrors them into `people` (`note_path` set) and `person_accounts`, at startup, after an Obsidian sync that touches `People/`, and after the app writes a People note. A deleted note's person keeps its row but stops matching anything.
- People are read from `App.vault` (the `obsidian_vault` setting), with or without an Obsidian source configured.

## Matching authors

Every message author (`item_messages.author_id`) and item author (`person_handles`) maps to an account: Slack user id (and, with the `users:read.email` scope, the email on that user's Slack profile, kept in `account_profiles` with their real name), Teams user id (with the email on their Microsoft profile, the same way), Gmail/Calendar sender address (a Gmail relay like a GitHub notification maps to its GitHub login; other relays to nothing), Jira accountId, GitHub login. The account is looked up in `person_accounts` at read time (a Slack account also through its profile email, so a note that only lists an email still claims that person's Slack messages), so editing a note in Obsidian recolours past and future signals alike. Authors with no id at all (bots, the sample data) fall back to a note's name or aliases.

Gmail, Jira and GitHub messages now store `author_id` too (it was Slack only). Rows stored before this change have none and are matched by name only.

**You** are your Slack and Teams sources' own user ids, each Jira source's `email` (the API token's owner; each poll also stores your Jira accountId with that email in `account_profiles`), plus `people.me` in settings (`["email:me@work.com", "jira:…"]`), shown in `people.my_color` (default 0, Gruvbox red).

## Colours (16c)

256 colours interpolated in OKLCH through the Gruvbox brights (red, orange, yellow, green, aqua, blue, purple, back to red) at lightness 0.65. A person's colour is their note's `color`, else `fnv1a(key) % 256` (32-bit FNV-1a), where the key is their first email when the note lists one, otherwise their first account, otherwise their name. An unknown sender is hashed by their email when the source gives one (mail, or a Slack profile), otherwise by their account id (or name). Colours are not spread apart; some people land in the grey stretch between green and aqua, by design.

The backend hands out indices; `lib/people.ts` owns the palette (checked against all 256 values in the 16c mock).

## New person (17a) and Link to person (17b)

- An unknown sender seen in `people.suggest_after` (3) or more signals within `people.suggest_days` (7) days gets a dashed **New person?** strip under an expanded signal's messages. A split family counts once. You, known people, Slack bot users (Jira, Opsgenie…, per `users.info`) and mail from lists or systems (`news@`, `noreply…`, `billing@`…) never do.
- The strip shows the Slack profile's real name and email when there are some. **Create note** (`p`) writes `People/<real name>.md` (the display name when there's no real name) with the display name as an alias, the account, and the profile email. **Link to…** (`l`) opens the picker. **Not now** hides the suggestion for that account for 7 days (`person_snoozes`).
- Unknown names have a dotted underline in their hash colour; clicking one (in Incoming or a topic's People panel) opens the picker. It matches note names and aliases; ↵ adds the account to that note's frontmatter, keeping the rest of the note as written (a missing key is added, a value becomes a list, a list grows). The last row creates a new person.
- Writes go through `obsidian::save` like any note edit: the outbox (`obsidian.replace`, refused if the note changed meanwhile) or the file when no Obsidian source reads the vault. While a write is queued, the account is recorded locally so colours follow right away; the next scan has the final word.
- Nothing is automatic: no auto-create, auto-link or auto-merge.

## The People tab

Tab 4 (`⌘4`), after Topics. Code: `queries/people_index.rs` (`people.index`, a live query), `queries/person_merge.rs` (`people.merge_preview`, `people.merge`), `src/views/People.svelte`, `src/views/people/`, `src/lib/topicTone.ts`.

- **Rows:** every People note, then senders with no note. A person's signals are the **confirmed** work signals they wrote in (a message, or the item's author), matched as everywhere else; you're left out. A signal counts once per week and once in the total however many messages they wrote in it.
- **Columns:** the person (avatar, name in their colour, `role`); their top three topics in the last 30 days, each box filled by length against the busiest box in the list (`topicTone.ts`: every category colour at one OKLCH lightness, chroma capped, through `culori`); the 30-day total and 12 Monday weeks (the last is this week so far), shaded in four steps against the busiest week in the list; last signal (its source's icon); next meeting.
- **Groups:** Recent (a signal in 30 days), Quiet (every other People note, with or without signals), No People note (a sender with an account, a confirmed signal in 30 days, not a bot and not mail from a list or system; named by the address for mail, else the profile's real name or the latest name seen).
- **Next meeting:** the soonest timed event within 7 days with them as a guest. Guests are read from the event as Google sent it (the item's `raw_json`, so no new table and no connector change; Proton ICS feeds are busy blocks only and never read guests). Events you declined have no `events` row; events they declined are skipped. Matched by email to the person's `email` accounts.
- **Keys:** `j`/`k`; `s` sort picker (Last signal, the default; Signals; Next meeting; Name), `⇧S` reverses, rows sort within their group; `t` adds a topic filter (AND, up to 8): each picked topic becomes an 82px column of 30-day counts tinted in the topic's colour, matching anyone with a confirmed signal there ever (so a cell can be 0), and `⌫` removes the last; `/` filters by name, alias, role or account; `n` asks for a name and writes a stub `People/<Name>.md`; `l` on a No People note row opens Link to person; `m` merges; `u` undoes a staged merge. `↵` does nothing until the person page is built.
- **Merge (`m`):** a picker under the row (people by name, alias or account; then those sharing an email domain; then by signals), then the two notes side by side. The kept note stays as written; the other's `aliases` and account fields are unioned in, a field only it has is copied, a field both set differently is a line to decide (`n` next, `←` takes the left note's value, `→` keeps the kept one), and its body is appended under `## Merged from <name>`. `s` swaps who is kept, `⌘↵` stages it. Staged merges are written when you leave People (`u` undoes until then), from the texts the dialog showed: if either note changed since, nothing is written. With an Obsidian source it's one outbox op, `obsidian.merge_notes` (both notes checked, then the kept one written and the other deleted the way Obsidian's own Delete does, into the trash its "Deleted files" setting names); without one, the files are written directly and the other note moves to the vault's `.trash/`. When it lands, topics linked to the merged-away person by hand (`topic_people`) link to the kept one. The merged-away name isn't added as an alias (the design doesn't show one), so a signal matched only by that name (no account id) stops matching.

## A topic's People panel

Whoever wrote in the topic's confirmed signals (by how many signals, then how recently), plus anyone in `topic_people`, up to 9, you excluded. The second line is the note's `role`, else "N signals · when". The assistant's topic context gets each known person's note gist, the vault notes linking to them (from the Obsidian source's items; one link away only), and the topics their signals were filed under.

## Split review (15g, 15h)

A signal is part of a split family when the AI (or a manual split in Unsorted) moved some of its messages to child items (`split.rs`). While another part still holds messages, the signal shows a `split` pill and `⋯` where the other parts' messages came in between.

`s` (or a click on the pill or a `⋯`) opens the review in place. `s` also works on any Slack signal with more than one message that hasn't been split, under a topic or in Unsorted (user decision 2026-10-01): it opens with one group, and moving messages to another topic makes the split.

- **By topic**: each part's messages in time order, `⋯` where another group's came in between; groups ordered by first message. **In order**: the conversation as it happened, in runs per group. `g` flips; a 1:1 DM opens In order, everything else By topic.
- Bars cycle yellow, blue, purple, orange, green, aqua, red for this signal only (not topic colours). Names are in their person colour.
- `j`/`k` move, space selects, shift+`j`/`k` selects a range like text (back toward where it started shrinks it; rows picked with space stay), `t` moves the selection (or the current message) to a topic: into that topic's group if there is one, else a new part suggested to it (`incoming.review_move`). The picker's first row, **Unsorted**, moves it to the Unsorted group (made if there isn't one). Thread replies move with their first message.
- `d` dismisses in place: the messages go to the family's one dismissed part (seen, no topic; `incoming.review_dismiss`) and stay in the review struck through.
- `u` undoes the current message's move or dismiss (else the latest): it goes back to its old group's topic, or Unsorted, recreated if it was pruned.
- c confirms every part that has a topic (`incoming.review_confirm`); Unsorted stays in Incoming. Esc goes back.
- A part left with no messages is deleted (`split::prune`) unless it's the root, has a held todo, or something else (an outbox row) still points at it. Messages and their keys stay in the family, so the next Slack sync doesn't re-add moved messages.

Not built: showing an orphan thread item of the same channel inside its channel's review (threads now attach to their parent message, so this only matters for threads whose parent is older than the lookback).
