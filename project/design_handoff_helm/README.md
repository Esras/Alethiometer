# Handoff: Alethiometer — personal work assistant (macOS)

## Overview
Alethiometer is a single-user, local-first desktop assistant for one person: a manager moving into director-track work with a heavy meeting load, who loses track of time and attention easily. It gathers work signals (calendar, Slack, Jira, GitHub, a work Obsidian vault) under **Topics**. It then helps with four jobs: triage (Incoming), deciding what's next (Priorities), capture (Notes) and reflection (Reflect). Life/personal commitments are visible only as untitled busy time.

This is a personal tool, not a product. Hard-coded choices, a `settings.json` and a short README for setup are all fine. Don't build settings screens, onboarding, multi-user support, telemetry or billing.

## About the design files
The files in `design/` are **design references built in HTML**. They're prototypes showing the intended look and behavior, not production code. Recreate them in the stack below. Open `design/Helm Explorations 17-20.dc.html` in a browser (serve the folder locally, e.g. `python3 -m http.server`). The rounds are split into four pan/zoom files (17–20, 13–16, 7–12, 1–6), each with **newest at the top** and a nav bar linking the other three. Earlier rounds are superseded; follow the option IDs listed per view below.

## Fidelity
**High-fidelity** for layout, color, type and interaction model. Copy in the mocks is illustrative sample data. Match the colors, density and keyboard model closely.

---

## Stack (decided)
- **Tauri 2**: Rust core, UI in the system WebKit web view (no bundled Chromium). Pick a lightweight UI framework (Svelte or Solid preferred over React; your call).
- **Development happens in the cloud (Linux).** Everything except Mac-specific integrations must build, run and test on Linux (Tauri uses WebKitGTK on Linux). Put Mac-only pieces behind a small platform trait with Linux stubs: Keychain, global hotkey, notifications, launch at login and the menu-bar/tray icon.
- **Packaging and signing happen on the Mac.** Sign every build with one stable **Apple Development** certificate (free Apple ID, personal team), set in `tauri.conf.json` → `bundle.macOS.signingIdentity`. With a stable signature, macOS keeps Keychain and Calendar access across rebuilds. No notarization is needed.
- **Storage:** one SQLite file in the app's support directory. Use **sqlx** (plain SQL, compile-time-checked queries, migrations) or rusqlite; no heavy ORM. Use **FTS5** for search and the **sqlite-vec** extension for embeddings (loading it may favor rusqlite; decide early).
- **Config:** `settings.json` (source list, calendar URLs, buffers, model choices). Secrets go in the macOS Keychain; on Linux dev, a `0600` file is fine.
- **No server and no MCP.** Use direct API clients per source.

## Boundary rules (non-negotiable)
1. **Every source is tagged `work` or `personal`** in config. Every query and view can filter on it.
2. **Work data may contain PHI.** It may only be sent to LLM endpoints that are allow-listed in config (providers with a BAA) or to local models. If the configured provider isn't on the allow-list, **fail closed**; never fall back.
3. **Personal content never lands on this machine.** The only personal input in Phases 0 and 1 is Proton Calendar **"Limited view" share links**: busy times only, one link per calendar. Store and show them as untitled "Life" blocks.
4. **Nothing personal goes to the work AI.** Life blocks may be used for time math only.
5. **No automatic deletions, ever.** Reclassifying only marks an item wrong and drops it to Unsorted.
6. **AI suggests and answers; it never writes the user's words.** It never drafts reflections, notes or replies.

## Phases
- **Phase 0:** read-only sources, Topics, Incoming, and Life busy-blocks on the day strip.
- **Phase 1:** Priorities, **writeback**, Notes, Reflect. The MVP is Phase 0 + Phase 1.
- **Phase 2 (after real use):** sending `~` life lines to Marvin (receipt-confirmed), deeper personal integration, and possibly a QR hand-off to iPhone.

## Sources
| Source | Tag | Read | Writeback (Phase 1) | Notes |
|---|---|---|---|---|
| Work calendar | work | events, attendees | RSVP | Google Calendar API or EventKit; confirm with user |
| Slack | work | DMs, mentions, channels you follow | mark read, reply, react | Socket Mode app: app-level token (`xapp-`) + user token (`xoxp-`), installed from the app config page. Workspace admin approval may be required |
| Jira | work | assigned, watched, mentioned | transition, comment, assign | REST API token |
| GitHub | work | PRs (review requested / authored), issues, mentions | review comment, comment, label | Also: a Topic can have **linked repos** |
| Obsidian (work vault) | work | all notes, file watch | append / patch in place | Plain files. Never rewrite a whole file. Vault syncs only via BAA-covered storage |
| Proton Calendar (Limited view links) | personal | busy blocks only | none | ICS polling |

Sync is incremental polling with a stored cursor per source, except Slack Socket Mode (live) and Obsidian (file watcher).

## Writeback: the outbox
Every write goes through an `outbox` table: `pending → sent → confirmed`, or `failed(reason)`. The UI shows the state inline next to the item (see the receipt treatment in **11a**). Mark local state done only on `confirmed`. Failures stay visible with retry (`⌘R`). Nothing is removed locally until the remote side confirms.

## Data model (starting sketch; refine with the user)
- `sources(id, kind, tag work|personal, config_json, cursor, last_sync_at)`
- `items(id, source_id, external_id, kind [message|issue|pr|event|note|…], title, body, url, author_person_id, occurred_at, due_at, status, raw_json)`, with an FTS5 index on title and body
- `topics(id, name, category [see colors], obsidian_path, archived_at)`
- `topic_links(item_id, topic_id, state suggested|confirmed|wrong, confidence, reason)`. "Wrong" drops the item to Unsorted.
- `topic_repos(topic_id, repo)`, `topic_people(topic_id, person_id)`
- `people(id, display_name)` plus `person_handles(person_id, source_kind, handle)`
- `events(id, item_id, start, end, is_life_busy)`, with meetings attached to topics via `topic_links`
- `priorities(item_id, quadrant now|make_room|quick|can_wait, reasons_json [{label, weight red|yellow|green}], updated_at)`
- `notes_lines(id, page_date, kind note|do|research|delegated|life, text, target_json, state)`
- `reflections(id, scope day|week|month|quarter, period_start, lens both|work|life, obsidian_path)` plus `carry(item_id, action tomorrow|schedule|let_go)`
- `outbox(id, source_id, op, payload_json, state, error, created_at, confirmed_at)`
- `embeddings(item_id, vector)`, via sqlite-vec

## AI (open; decide with the user in the build thread)
- Tiers: a small, fast model for topic suggestions and reason pills; a larger one for briefings and questions; embeddings run **locally** (Ollama or MLX) so indexing never leaves the machine.
- Open questions: which BAA provider or endpoint, which models per tier, local vs. remote for classification, and the rate/cost budget.

---

## Views
**Window size:** 1760×1175 (`tauri.conf.json`), with `ui_scale` **1.15** in `settings.json`. That gives about 1530×1022 of layout space at the mock values. Rounds 7–19 were drawn at 1320×880. Their px values stay as they are, and the zoom does the scaling. See 20a for how the extra width is used:
- The day strip labels every hour (`ticks="hourly"`). Event blocks are 44px tall (up from 26).
- Overlapping events split into lanes: 2 lanes of 21px or 3 of 13px, with labels at 10–10.5px. The bar above an overlap is 3px: `#fe8019` where two events overlap and `#fb4934` where three do, over a faint band of the same color. The dashed "tentative" border stays on lanes. For 4+ overlaps (not mocked), show 3 lanes plus a "+N" chip.
- Signal rows have one action cluster on the title line: that source's actions (Todo, Trash, Unsubscribe / Accept, Decline) come first, then a 1px divider, then Confirm, Wrong topic and Dismiss, right-aligned so Confirm lines up down the list. The second line holds only the snippet, label and status. The panel fills the available width.

Other views can use the spare width as they are, for now. The top of every view except Reflect is a tab bar (Incoming, Priorities, Topics, Notes, Reflect, with ⌘K search at the right) and the **day strip**. The chat/ask pane is collapsible.

### Day strip (`DayStripGruv.dc.html`, see 7a/7b/11a)
- A 156px countdown card on the left: "Next · {meeting}" plus a large countdown and a 3px bar. The countdown color is green `#b8bb26` above 15 min, yellow `#fabd2f` from 5 to 15, orange `#fe8019` from 2 to 5, and red `#fb4934` under 2.
- To-scale blocks run 08–19h. All blocks are at 50% opacity; meetings linked to the **current topic** come back to full strength with a 1.5px aqua `#8ec07c` ring.
- A 2px "now" line in the countdown color. Tentative events use a dashed border; gaps are hatched.
- **Life blocks** (`life="busy"`) are blue diagonal hatching labeled "Life" with a house icon, with no titles. A dashed blue **leave-by tick** sits one travel buffer before the block (default 15 min; `b` on a block overrides it).
- If the current topic has nothing today, a note on the right names the next linked meeting (7b).

### Topics (7a): the root object
- Two columns. The left holds collapsible panels in this order: **Upcoming** (only when something is soon), **Open**, **Chronology** (timeline with line, dots and app icons; solid dot = past, open dot = upcoming or undone), **People**. The right is the topic's **Obsidian note**.
- The columns scroll independently. `z` or the fold button folds the left column.
- Add: a linked-repos list (GitHub) in the topic header or People area.

### Topics index (12c): picking a topic
Reference: 12c (12b is the same layout before it was checked against the code; 12a is superseded). Checked against `claude/helm-ui-shell` on 2026-09-28.

**What exists today:** there's no index yet. `Topics.svelte` opens `topics.list()[0]` when `ui.openTopicId` is null, and `[`/`]` step between topics.

**Entry and exit**
- Clicking the Topics tab with no topic open, or clicking the "Topics" crumb in a topic header, shows the index.
- `openTopic(id)` from other tabs still jumps straight into the topic. Its crumb reads the origin tab, as it does now.
- Esc inside a topic, when nothing is being edited, returns to the index.
- Keep `[`/`]` inside a topic, stepping in the index's sort order.

**Layout:** one panel (max 1060px), inside the existing shell with its tab bar, sync chip and day strip.
- Header: a filter field, an **Active / Archived** `.seg` control, and a **New topic** `.btn.primary`.
- If the filter matches nothing, show TopicPicker's "Create '…'" row.

**Rows:** the grid is `4px minmax(0,1fr) 70px 120px 240px`.
- **Bar:** `catColor(category)`.
- **Name:** shown with `new_count` as an aqua badge when it's above 0 (same style as the Incoming tab badge). Below it goes `obsidian_path`, or "no note" in italic.
- **Open:** a count, or — when there's nothing open.
- **Last signal:** "HH:MM · Source" for today, otherwise a day or date. `when_label` already does this.
- **Next meeting:** a pill in `.pill` style, followed by the meeting title in `--tx-muted`. Leave it blank when there's no meeting.
  - `.pill.red` "in 8 min" when it's 30 min away or less
  - `.pill.yellow` "in 3h 38m" later today
  - `.pill.green` "Thu 11:00" on another day, within 7 days, matching the Upcoming panel
  - Don't reuse `urgencyColor()`. Settled: the strip countdown keeps its 15/5/2 thresholds, because it shows different information.
- **Sections:** Active first, sorted by next meeting then last signal. Then **Quiet · no signal in 7 days**, with row content at 60% opacity. The 7 days is a `settings.json` value.
- The selected row uses `.sel` and sets `ui.topicId`, which lights its meetings on the strip.

**Keys**, following TopicPicker:
- As built: the filter doesn't take focus on arrival. j/k move, `/` focuses the filter, `a` archives, `n` starts a new topic, and `c` opens the category picker. This supersedes the earlier focus-on-arrival note.
- Archive is `a`, not `e`: `e` already means "edit note" inside a topic.

**Archive:** hovering a row, active or quiet, swaps the Next meeting cell for an **Archive** button. There's no bulk archive (the user was explicit in chat1: never throw data away in bulk).

**Backend**
- `topics.list` gains `open_count`, `last_signal_at`, `last_signal_source`, `next_meeting {start, title}`, `obsidian_path` and an `archived` flag argument.
- New commands: `topics.archive {id}` and `topics.unarchive {id}`, which set or clear `archived_at`.
- Every other query already filters `archived_at IS NULL` (suggest, incoming, notes, search, github, obsidian), so archived topics drop out of AI suggestions and pickers with no further changes. This resolves the ISSUES.md "archive/cleanup story" item, though only manually for now.

**Out of scope:** a periodic review of quiet topics (possibly in Reflect), and "Dismiss a signal" in Incoming, which is tracked separately in ISSUES.md.

### Gmail in Incoming (19a rows, 19b todo + archive)
Reference: 19a/19b, with the row layout from 20a.

**Rows (19a)**
- Triage (Confirm, Wrong topic, Dismiss) works as for other sources. The Gmail-only actions sit to the left of it, after a divider (see 20a):
  - **Todo…** (`e`)
  - **Trash** (`#`): also dismisses the email.
  - **Unsubscribe** (`⇧U`), only when the email has a one-click unsubscribe header (RFC 8058). It sends the one-click request, then trashes the email.
- Trash and Unsubscribe act on one email at a time. Both leave the row struck through, with `u` to undo, as Dismiss does.
- **Labels:** the category chip is the Gmail label, synced both ways. The tag icon means it's linked.
- **No plain Archive.** Archiving happens only through a todo (19b), so nothing leaves the Gmail inbox without a follow-up.
- Newsletters stay in the normal list.

**Todo + archive (19b)**
- `e` opens a small form under the email:
  - task text, drafted by the AI from the email
  - an optional due date
  - the topic, pre-filled from the suggestion
- A line under the form lists what saving will do: add a task to the topic's Obsidian note (linked back to the email), apply the category's Gmail label, and archive the thread.
- `⌘↵` **holds** the todo on the signal. Nothing is sent until the signal is confirmed (see Todos, 22c). Confirm then queues the task, the label and the archive through the outbox.
- Afterwards the row reads "Todo in … · archived" and has an **Open task** button.
- The form has the Marvin | Jira switch from 22c.

**Outbox actions:** `gmail.trash`, `gmail.unsubscribe` (one-click POST, then trash), `gmail.label`, `gmail.archive` and `obsidian.append_task`. Each one can be undone until the outbox flushes.

### Todos (22a tab, 22b topic tasks, 22c creation, 21e weekly review)
Marvin holds your todos, and Jira holds work tracked with others. Alethiometer shows both and never merges them. Only Marvin tasks linked to a Alethiometer topic are read, so personal Marvin lists stay off this machine.

**Todos tab (22a)**
- A new tab between Priorities and Topics, with the open count in its badge.
- One flat list. There are no topic groups and no rail.
- Columns:
  - checkbox
  - source icon (Marvin or Jira)
  - title, plus where it came from (`DM · Kim`, `Notes`, `Gmail`), in muted text
  - Jira status pill
  - topic tag (category bar and name)
  - priority: the task's quadrant from Priorities
  - due date
- Clicking a column header sorts by it (default: due date, ascending, with undated tasks last). `/` filters by text.
- Filters: Source (All · Marvin · Jira, with counts), Topic, Priority, Due.
- The Jira rows are issues assigned to you, plus any created from Alethiometer.
- **Overdue is quiet:** red date text (`#fb4934`) only, with no badge, re-sort or banner.
- How it differs from Priorities: only committed tasks, all of them, and nothing folded away.

**Topic view Tasks (22b)**
- Replaces the Open section from 7a.
- Tasks are grouped by the Priorities quadrants (Now-ish · Make room · Quick touch · Can wait), with the same reason pills. A task changes group whenever it moves in Priorities. Empty quadrants are hidden.
- Includes the topic's Jira issues assigned to others, with their assignee. These don't appear in the Todos tab.

**Creating (22c)**
- `t` on Slack rows, `e` on Gmail rows (19b), lines in Notes, and `t` in Todos or the Topic view.
- The form has a **Marvin todo | Jira issue** switch (`⌘1`/`⌘2`). You choose each time; nothing is guessed.
  - Marvin: text (drafted from the source), optional due date.
  - Jira: summary only. Project, type and assignee fold into one picker ("PLAT · Task · you"), filled from the topic's usual project.
- The topic is **read-only** and comes from the signal. To change it, use Wrong topic first.
- **Held until confirmed.** On a signal, Save only holds the todo, and nothing goes to Marvin or Jira yet.
  - Confirm creates it with the confirmed topic.
  - Wrong topic moves the held todo to the new topic.
  - Dismiss discards it, with `u` to undo.
  - While a todo is held, the row reads "… held until you confirm" and its Confirm button shows "+ create issue" (or "+ create todo").
  - Held todos don't expire and aren't listed elsewhere, because triage should be quick.
- On creation, the topic's Obsidian note gets **one link line** only, never the task text.
- The line above Save lists every effect.

**Completing**
- `x` or the checkbox.
- Completing also closes the linked source: the Marvin task is completed, a Jira issue moves to Done, and a Slack Later item is marked complete.
- The row stays struck through, with "Done · …", until the next refresh. `u` undoes it through the outbox.

**Weekly review (21e)**
- A "Stale todos" section in Reflect's week view only.
- Stale means 7 or more days overdue, or 21 or more days open with no date.
- Each one needs a choice: Keep (`k`), New date (`d`), Done (`x`), or Drop (`⌫`). For a Jira issue, Drop is replaced by Unassign me.
- The AI doesn't suggest a choice.
- The day strip shows nothing for todos.

**Outbox actions:** `marvin.create`, `marvin.complete`, `marvin.drop`, `jira.create`, `jira.transition_done`, `jira.unassign`, `slack.later_complete`, `obsidian.append_link`.

### Settings (26a General, 26b Sources, 26c AI)
Reference: round 26 (25 is superseded; 25d's find becomes the field at the top of the sidebar).

- **Modal, not a tab.** A cog to the right of the sync status, or `⌘,`, opens it over the app with the app dimmed behind. `esc` closes it. `⌘⇧,` opens `settings.json` itself.
- **Live reload.** Saving writes `settings.json` and Alethiometer re-reads it and applies the change live: `ui_scale` through webview zoom, and a source change restarts only that connector. Edits you make to the file by hand are picked up the same way. If a setting truly can't apply live, the app restarts itself on save and says so in the header ("Saved · live", "Saved · Slack reconnected").
- **Layout.** A sidebar with find (`/`) and sections numbered 1–6 (General, Categories, People, Sources, AI, Developer); a red dot marks a section that needs attention. Every group is its own card with space between cards. Descriptions are one short line, or none where the label says enough. The `settings.json` key path shows only on the focused row. Categories, People and Developer use the same cards but aren't drawn yet.
- **Keys:** `j/k` setting, `1–6` or `⇥` section, `↵` edit, `−/+` step, `space` toggle, `/` find.

**General (26a):** Window (ui_scale), **Type** (`ui_font`, `mono_font` from installed fonts, each shown in its own face; `ui_font_size` 13 for lists, labels and controls, `reading_font_size` 14 for messages, notes and drafts; ui_scale multiplies both), Day strip (day range, Life travel buffer), Topics (quiet after), Notes (vault), Data (sample data).

**Sources (26b):** a list of sources with sync state on the left, the selected source's cards on the right: Identity, Secrets, What it reads, Limits.
- **Secrets** are in the Keychain only, never shown, and can only be replaced (`r`).
- **Tag** shows what `personal` means today (not sent to the AI, kept out of Incoming, no write-back except Marvin). That's a current decision, not a fixed rule; take the description from the code's actual rules rather than hard-coding it.
- **Slack channels** is a mode: **All I'm in** (`"all"`, shows the count) or **Only these**. **Review… ↵** opens a searchable, sortable list of channels you're in, with a checkbox, last activity and messages per day for each. Unchecking one in All switches to Only these with the rest still checked. The row warns that past about 100 channels a catch-up takes minutes without the live-feed (`xapp-`) token.

**AI (26c):** allowed providers (bedrock; US regions only, changed in settings.json only), Bedrock region and AWS profile, and a models table (Small / Large tier: model, effort, today's usage; an empty model turns a tier off).
- **Prices are never entered by hand.** They're read once a day from the AWS Price List API (`pricing:GetProducts`, Bedrock, the configured region) and cached; `p` refreshes them. A model that isn't listed shows "not listed", and its cost estimate shows tokens only.

### Settings (26a General, 27a Sources, 26c AI; 25d find)
A modal over the app, opened from the cog to the right of the sync status, or with `⌘,`. `Esc` closes it. It isn't a tab: ⌘1–⌘6 stay the tabs. Reference: rounds 26 and 27 (25 is superseded except 25d's find).

**Schema.** One row per field of `Settings` in `settings.rs`. Descriptions come from its doc comments, cut to one short line or dropped when the label says enough. `sources[].config` is untyped today (`serde_json::Value`); give each connector a typed config struct and generate one JSON schema (`schemars`), so the page can render source fields and hand edits are validated.

**Layout**
- Sidebar: Find (`/`, 25d) at the top, then the sections numbered 1–6: General, Categories, People, Sources, AI, Developer. **Sources expands in the sidebar** with one sub-item per source plus "Add source" (27a). A red dot marks a source that needs attention. "Open settings.json" (`⌘⇧,`) sits at the bottom.
- Main pane: a title, then each group as its own card (label above the card, 24px between groups). Each row has the label, an optional one-line description and the control on the right. The key path (e.g. `quiet_days`) shows only on the focused row.
- A source's page header shows the icon, name, live status and an Enabled toggle, followed by the groups Identity, Secrets, What it reads and Limits.
- Secrets are write-only: the page shows the secret's name and when it was set, plus Replace… (`r`), and writes the value to the Keychain. A value is never displayed.
- `ai.allowed_providers` is read-only here ("settings.json only"): it's the safety gate, so changing it stays a deliberate hand edit.

**Saving and reload**
- Every change writes settings.json immediately; there's no Save button. The modal header confirms it ("Saved · live", "Saved · Slack reconnected").
- Helm re-reads the file and applies the change live: ui_scale through the webview zoom, a source change by restarting only that connector. Hand edits to the file are picked up the same way (file watcher).
- If a setting truly can't apply live, Helm restarts itself and says so in the Settings header. There's no "restart later" path.

**Keys:** `j`/`k` setting, `1`–`6` or `⇥` section, `⌥j`/`⌥k` previous / next source, `↵` edit, `−`/`+` step, `space` toggle, `r` replace secret, `a` add source, `⇧X` remove source, `/` find, `esc` close.

**Cleanup:** fold `life_calendars` into the `protoncal` source; show `fake_now` (Developer) only in dev builds.

### Delegated asks (24a capture, 24b list, 24c by person, 24d topic + Reflect)
Small things you've asked someone else to do: not engineering work (that stays in Jira) and not your own todos. Reference: round 24 (round 23 is superseded).

**Model:** a new `asks` table (`person_id, text, due, from_item, topic_id, state open|done|dropped, created_at`). Asks stay in Alethiometer and are never sent to Marvin or Jira. Jira issues assigned to other people stay in Jira. There's no status beyond open or done, and no nudge count.

**Glyph:** the red triangle (`ph-triangle`, `#fb4934`) from Notes `^@name`, wherever an ask is listed.

**Capturing (24a)**
- **Suggestions are conservative and opt-in.** The AI suggests an ask only when your own message is a direct request to a named person; hedged or conditional wording gets nothing. The suggestion is a dashed strip under the messages.
- **Confirming the signal never tracks a suggestion.** Only `a` tracks it. `⇧A` marks it not an ask. An untracked suggestion goes away when the signal leaves Incoming.
- **Form:** the todo form gains a third target, **Ask someone**, with a Who field (the People picker), the ask text, a due date and the topic from the signal. Like a held todo, it's created when the signal is confirmed. The form never sends a message; sending belongs to the reply flow.
- **Form keys:** `⌥1 / ⌥2 / ⌥3` switch Todo / Jira issue / Ask. ⌘1–⌘6 are the global tabs, so TodoForm's current ⌘1/⌘2 must move too.
- **Replies:** when the person you asked writes in the same DM or thread after the ask, the signal shows a line naming the ask. That's a lookup, not an AI judgement. **The AI never closes an ask**; only `⇧C` (or `x` in Todos) does.
- Email asks come only from threads already in Incoming. Sent mail isn't read.

**Todos list (24b)**
- One list. Asks are a **Delegated** filter chip next to the source chips, not a separate mode.
- Row: checkbox, then the triangle (in place of the source icon), then **Who** as a fixed-width column (76px, person colour), then the title. Who is empty on your own todos, so titles line up. The topic colour is the 3px left edge, as in Todos.svelte.
- Status, Priority and Est. are blank or `-` for asks.
- Ages are numbers only (`40m`, `2h`, `8d`); never mix them with words like "today" in one place.
- Detail pane: who and role, "due {date} · asked {age}", the message you asked in (`o` opens it), and an AI-drafted nudge for the same DM or thread. The nudge is never sent without `⌘↵`. Then: Add to 1:1 agenda, Take back as your todo, Drop.
- **Keys** (new; no clash with j/k x u n / e s o): `r` nudge (edit the draft), `a` add to 1:1 agenda, `b` take back, `⇧X` drop.

**By person (24c)**
- `g` groups the Delegated list by person, as a grid of cards that all share one layout: name, role and next 1:1 (from Calendar) in the header, then **You asked**, then **Waiting on you** (your todos whose source signal came from them; "nothing" when empty). The cards have no buttons.
- Navigation: `j`/`k` moves row by row in reading order and carries on into the next card. `⇥`/`⇧⇥` jumps between people. The focused card has the aqua outline. The list keys act on the focused row; `⇧A` adds all of that person's asks to their 1:1 agenda.
- The 1:1 agenda is an Obsidian write through the outbox.

**Topic and Reflect (24d)**
- A topic's Tasks gets a **Delegated** group under the priority quadrants.
- Weekly Reflect: **Delegated, gone quiet** next to Stale todos, for asks 3d+ past due or 10d+ old with no due date. Each one offers Nudge (`r`), Take back (`b`) or Drop (`⇧X`).

**Open:** what a Notes `^@name` line does today is unclear. notes.rs has a path that offers a Jira task for the person named, but it isn't known to work in practice. Revisit it with asks: turning the line into an ask is the obvious candidate.

**Outbox actions:** `slack.reply` / `gmail.reply` (nudge, only on ⌘↵), `obsidian.append` (1:1 agenda).

### Reclassifying a confirmed signal (18c chronology, 18d search)
Resolves HANDOFF's "Wanted later: topic merge/reclassification" for single signals. Merging topics is still out of scope.
- **One topic per signal.** Reclassifying moves a signal. It never adds a second topic, and it never writes to the source; writebacks don't depend on topics.
- **Two actions**, the same keys everywhere:
  - `t` moves the whole signal, using the usual topic picker (create allowed).
  - `s` opens the split review (15g) for this one signal, to move some of its messages. Only offered when the signal has more than one message.
  - Afterwards, the old spot shows one quiet line ("moved to X", `u` undo) until you leave the view.
- **No prompts on select or hover.** Selecting an entry adds nothing. The actions only appear as small hint text once an entry is expanded.

**18c Topic chronology**
- ↵ or a click on an entry expands it in place and shows its messages: sender colours (16c), your own replies, one faded context message before the signal, and Open in Slack. Esc collapses it.
- While it's expanded, `t` and `s` act on it. This is also the general "see more" for any chronology entry, not just for reclassifying.

**18d Search results**
- ⌘K stays a quick-jump field: ↵ opens the top hit.
- ⌘↵ in ⌘K opens a full **Search** results view and moves focus out of the field. There, single keys work: j/k move, ↵ opens, space expands (as in 18c), `t` moves, `s` splits, `/` goes back to the query, Esc goes back.
- Each result shows its source icon, a title, a snippet with the match highlighted, its topic ("no topic" if it has none, with a category bar) and a date.
- Source chips at the top filter the results.

**No AI feedback:** corrections aren't fed back as examples. The classifier already gets recent classified context (see the Slack split section).

### People (17a new person, 17b link to person)
**A person is a note in the work vault** (`People/<Name>.md`). The vault is work-only and doesn't sync to any external system.
```
---
aliases: [Jordan, jlee]
slack: U04JRDN
email: [jordan.lee@work.com]
jira: 5f2c…
github: jlee-work
color: 212          # optional; index into the 256 person palette (16c)
team: "[[Platform]]"
manager: "[[Dana Ruiz]]"
---
Free notes…
```
**Reading the notes**
- The Obsidian connector reads `People/*.md` frontmatter into a local `people` table and a lookup from each account to its person (`person_accounts(source, account_id) → person_id`).
- Every message and item author is matched through that lookup. `aliases` is Obsidian's own field, so `[[Jordan]]` still resolves.
- Colour: the note's `color` if set, otherwise the hash colour (16c).

**Agent context**
- One link away only: which notes link to this person (backlinks), and the person note's own text.
- Plus activity from the app's database: which topics this person's signals were confirmed to, and how recently.
- Don't follow links more than one step.

**17a New person**
- **When it appears:** an unknown sender in 3 or more signals within 7 days (a setting).
- **Where:** a dashed strip under the signal's messages showing the name, account, email and how many signals they've been in.
- **Actions:** **Create note** (`p`) writes the stub note with `aliases` and the account fields. **Link to…** (`l`) opens 17b. **Not now** hides the suggestion for that person for 7 days.
- **Separate from triage:** Confirm is unaffected.
- **Unknown names:** a dotted underline, in their hash colour.

**17b Link to person**
- **Opening it:** click an unknown name or address, or press `l`.
- **The picker** is built like the topic picker. It matches names and aliases, and each result shows its note path and the accounts already linked.
- **↵** adds this account to the chosen note's frontmatter. That write goes through the outbox, like any other Obsidian write.
- **Last row:** "Create person '…'".
- **Never automatic:** don't auto-create, auto-link or auto-merge people.

**Open:** extra frontmatter fields (`team`, `manager`, `role`) are free-form for now, so nothing should depend on them yet.

### Slack split review (15g, 15h) and person colours (16c)
Reference: 15g is a DM grouped by topic, and 15h is a channel with a thread. 15f is the In order view of the same signal. 15a–15e are superseded.

**When it shows:** the AI split one Slack signal across topics. ↵ on the signal's `split` pill or on a ⋯ gap opens the review, and Esc goes back.

**Modes:** **By topic** or **In order**, and `g` flips between them.
- The default depends on the type: a 1:1 DM opens In order; a channel, group DM or thread opens By topic.
- `g` flips only the signal you're on and isn't remembered.

**By topic (15g):**
- Each topic's messages sit together in time order. A ⋯ marks where messages from another topic came in between.
- Groups are ordered by their first message and separated by a hairline and 8px of space.
- Each group has a 4px bar on the left. Bar colours cycle through the Gruvbox brights for this signal only; they're not topic colours.
- `t` moves the selected messages to a group straight away, at their place in time. The moved message is tinted briefly with `u` to undo it.

**Threads (15h):**
- A thread's replies are indented under its first message, behind a 1px rule. Only real threads get indented.
- A thread from the same channel and span shows inside that channel's signal while you review it. It stays a separate item underneath.
- The AI treats a thread as one unit and only splits it when there's clear evidence.

**Rows:**
- Each row is time, sender and text. The sender is always shown, you included, in their person colour. The message text is the same colour for everyone.

**Actions:**
- `d` dismisses a message in place: struck through, with `u` to undo it.
- **Confirm** (↵) confirms every group that has a topic. The Unsorted group stays in Incoming.
- `t` opens the usual topic picker, where you can also create a topic.
- j/k skip the ⋯ gaps. The footer shows each group's colour, topic and count, and wraps onto a second line when there are many groups.

**New activity:** before triage, new messages are added to the same signal. After triage, they start a new signal in Incoming. When the AI classifies them later, it only classifies the new messages, but it gets the last few messages from that conversation with their topics as context.

**Data changes needed** (checked against `slack/poll.rs` on `claude/helm-ui-shell`):
1. **Keep your own messages, and let triage close a signal.**
   - Today, your reply in Slack moves the read marker, which closes the signal, and a signal made only of your messages is dropped.
   - Change: your messages go into the signal like anyone else's. Closing a signal is based on triage, not on Slack's read marker or a timer:
     - While a signal is untriaged, new messages in that stream (yours included) are added to it. The AI classifies only the new messages and uses the earlier ones as context.
     - Once it's triaged (confirmed, moved or dismissed), the next message starts a new signal.
   - The `MAX_MSGS` cap of 50 still applies.
1b. **Reading in Slack never clears Incoming** (a standing user rule).
   - Today, reading a stream in Slack moves Slack's read marker, which marks its item seen and takes it out of Incoming (`Batch::seen` from `check_conv`, `check_thread` and `check_mention_reads`).
   - Change: only triage in Alethiometer removes an item from Incoming. The Slack marker can still decide which messages are new, but it never closes an item.
   - The other direction stays as it is: triaging here still sends `slack.mark_read` for top-level runs.
   - Also: the "read part of it in Slack, so its messages shrink to what's still unread" rule in `emit` goes. An item keeps every message until it's triaged.
   - Check the other connectors (Gmail read state, Jira) for the same leak and apply the same rule.
2. **Store a thread's first message.** `check_thread` drops it today (`m.ts != thread_ts`). Keep it as a context message so a thread can show inside its channel's signal (15h).
3. **Context messages.** Store a thread's first message, and the few messages before the signal, flagged `context: true`. They're shown but never classified or counted in `msg_count`. The AI uses them as context.
4. **Store the sender's ID.** `Message` stores only the display name today. Add `author_id` (the Slack user ID) so person colours and People links have a key.

**Person colours (16c):**
- 256 colours interpolated in OKLCH through the Gruvbox brights (red, orange, yellow, green, aqua, blue, purple, back to red). Hue and saturation come from Gruvbox; lightness is fixed at **L 0.65**.
- Default colour: `palette[fnv1a(key) % 256]`, where the key is the person's email when a source provides it, otherwise the source ID.
- **People** (probably Obsidian notes) link one person to their Slack ID, email and Jira account. One person then has one colour everywhere, on names, avatars and initials. A People note can set the colour; otherwise it defaults to the hash colour.
- The user is set by hand to index 0 (Gruvbox red). Setting their own colour is a user setting.
- Some people will hash into the low-saturation part between green and aqua and look nearly grey. **That's intended.** Don't add a minimum saturation or spread people apart; the user wants the colours stable.
- Person colours and group bars can be similar hues; that's fine. They mean different things (person vs group) and aren't read as linked.

**Open:** how a People note is formatted and stored in Obsidian (People isn't defined yet), and reclassifying a signal after it's been confirmed (deferred).

### RSVP (14a Incoming, 14b Topic)
**Answering an invite and triaging it are separate actions.** Accepting or declining never confirms, moves or dismisses the signal, and triaging never sends a reply.

**Where:** RSVP shows in three places. It does not appear in Priorities or the day strip popover.
- Incoming invite rows
- A topic's **Upcoming** panel (the next 7 days)
- A topic's **Chronology**, which covers invites further out

**Row (Incoming):**
- The title line keeps Confirm, Wrong topic and Dismiss.
- A second line holds a `no reply` pill, **Accept** `⌥Y`, **Decline** `⌥N` and **Decline with note…** `⌥⇧N`, followed by the overlap line. (User, 2026-10-07: `n` already holds a todo in Incoming, so the keys moved to ⌥; the same keys in a topic.)
- The Google Cal group count includes invites.

**Overlap line:** yellow `--yellow` with a warning-circle icon, reading "Overlaps {title} {HH:MM–HH:MM}". Life blocks read "Life" plus the time, never a title (boundary rule). Leave the line out when nothing overlaps.

**Topic:**
- **Upcoming:** the selected invite row expands to show the same buttons, in a smaller size (11.5px). Its panel count reads "2 · 1 needs a reply".
- **Chronology:** invites show a `no reply` pill in the right column. The selected invite gets the buttons.

**Decline with note:** opens an inline field under the row. ⌘↵ sends the decline with the note, and Esc cancels.

**After replying:**
- The line becomes "Accepted · HH:MM" in green or "Declined · HH:MM" in muted text.
- A "Change in Google Calendar" link calls `openExternal`. The reply can't be changed in-app.
- The row stays in Incoming until it's triaged.

**Backend:**
- Replies go through the outbox (`gcal.rsvp {item_id, response: accepted|declined, comment?}`), and the row shows the pending state while sending.
- The item stores the reply as `rsvp` (`needs_action|accepted|declined`) from the event's `attendees[self].responseStatus`.
- Overlaps come from `events` on the same day, Life blocks included.
- Only accept and decline are offered; tentative and propose-new-time are left out on purpose.
- (User, 2026-10-07) A recurring invite answers the whole series. A tentative invite (answered Maybe elsewhere) gets no buttons. The overlap line lists every overlap. In a topic, Tab moves j/k between Upcoming and Chronology.

### Incoming · Dismiss (13a)
Resolves the ISSUES.md item "No way to dismiss a signal in Incoming".
- **Where:** a ghost **Dismiss** button (eye-slash icon, `--tx-muted-2`, no border) goes after Wrong topic in topic view, and after Move to topic in Unsorted. The key is `d`, listed in the hints row.
- **Undo:** a dismissed row stays in place, struck through (`--tx-label`, faint hatch), with Dismissed and an **Undo** `.btn` (`u`) in place of the actions. It disappears on the next `refresh()` or when the selection moves away.
- **What dismissing does:** it marks the item seen without a topic link and never writes to the source. The item comes back into Incoming only on new source activity (new messages, status change, edit), the same way `seen` resets today.
- **Backend (as built):** `incoming.dismiss`/`incoming.undismiss` flip `seen`. There's no `dismissed_at` column, and nothing is written to the source.
- Dismissed items still count for Reflect and search. Dismissing only takes them out of triage.

### AI failures in the sync chip (13b)
Resolves the ISSUES.md item "`suggest_topics` failures are silently swallowed". It gives that failure a place to show.
- **Chip:** when topic suggestions last failed, the dot turns `--orange` and the chip adds "· suggestions failed", the same pattern as failed writes.
- **Popover:** a new **AI · Bedrock** section below Sources has one row, **Topic suggestions**, with a state and a Retry button (`.btn.primary`). The states:
  - "not run yet"
  - "ok · nothing new" or "ok · 3 suggested · 2m ago"
  - "failed 3m ago" in orange, with an error line underneath and a line saying how many signals are waiting in Unsorted and when the last success was
- **Backend:** `suggest_after_sync` persists the last `Outcome` (status, message, counts, at), and `sync.status` (or a new `ai.status_live`) returns it.
- **Notification:** a macOS notification fires once when suggestions go from ok to failed, and again only after a recovery. Never on every poll.
- Ask errors stay inline in the Ask pane and aren't listed here.

### Incoming (8a)
- The left column lists topics with counts; **Unsorted** is last. The right pane shows the selected topic's new signals, grouped by app.
- Per item: **Confirm** or **Wrong topic** (goes to Unsorted). The selected row gets a tinted fill plus an aqua right edge.

### Priorities (8b)
- A fixed 2×2 grid: **Now-ish · Make room / Quick touch · Can wait**. The eye icon toggles a quadrant between full cards and compact topic chips with counts; Can wait starts compact.
- Card: title, faded topic name, and **reason pills** colored by weight (red, yellow, green).
- `j`/`k` select, `↵` opens the topic, `o` opens the item in its app. The Selected bar sits at the bottom. Quadrant headers are neutral.

### Notes (9a, then 10a/11a)
- A free-writing page. Prefixes turn into glyphs:
  - `-` note: dash, `#a89984`
  - `[]` do: square, blue `#83a598`
  - `()` research: circle, green `#b8bb26`
  - `^@name` delegated: triangle, red `#fb4934`
  - `~` life: house, blue, with hatched row (Phase 2 send)
- A "Goes to" margin (250–270px) suggests a topic, person or app. `Tab` opens a picker to override it; `#` sets a topic.
- `⌃↵`/`⌘↵` sends everything to Incoming. After send, show per-line status as in 11a: faded "Incoming · {topic}", "held · needs a topic", receipt states.
- The global capture hotkey opens Notes from anywhere.

### Reflect (9b day, 9c week, 9d nudge)
- The reflection note takes most of the space. A compact side panel holds facts, plan-vs-did bars, and folded "Where time went" and "Decided" sections.
- Carry-forward actions: `>` tomorrow, `<` schedule, `×` let go. Scope toggle: Both · Work · Life (on this machine Life means busy-hour totals only).
- Day, Week, Month and Quarter each write their own Obsidian note, and each level rolls up the one below.
- No day strip, search or assistant pane; only a countdown pill when a meeting is near.
- **Nudge (9d):** a macOS notification in the first real gap near the end of the day. It stays until answered (any button or × counts). Until then, a dot shows on the tray icon.

### Agents (Agents Final 01–08)
Reference: `design/Helm Agents Final.dc.html`, screens **01–08**. The exploration that led here isn't included. **High-fidelity** for layout, the row grammar and the keyboard model. **Colors are a first pass**: build them as settings (screen 07), not constants.

The Agents tab shows coding-agent sessions from two kinds of source, in one tree:
- **Local ACP sessions.** Helm is the ACP client and launches the agent itself: Pi through `pi-acp`, Claude Code through its ACP adapter. Helm can only see sessions it started. A session that Zed launched belongs to Zed and can't be watched; screen 02's "Zed → Pi" label is sample data. Local sessions can't take part in rooms, because there's no channel between them, and Helm must not build one.
- **Scion agents** (Hub mode). Helm is a Hub client. Rooms, DMs, delivery status and agent-to-agent messages all come from Scion's conversation model. Helm has **no messaging layer of its own**.

**Layout** (in the 1530×1022 layout space): the title bar, the tab bar (with an **Agents** tab and its count) and the day strip, then a body grid of `450px | 1fr` on `#282828`.
- **Tree** (left, 1px `#3c3836` right border):
  - **Filter row:** "Filter sessions" (`/`), then counts for running, waiting and failed, then a + button.
  - **Needs you box:** orange fill `rgba(254,128,25,.08)` with a `.35` border. Each row is `12px 86px 1fr auto`: icon, agent name, the request in mono, and a tag (`low`, `outside repo`, `question`).
  - **Hierarchy:** project label (10.5px uppercase `#928374`) › group row (caret, name 500 `#ebdbb2`, goal 12px `#928374`, host tag) › room row (`ph-hash`, "room · N", unread count) and session rows.
  - **Session rows** are `30px 1fr 90px 52px 30px`: status, name in the agent color, harness, model, age. Subagents are indented to `44px` with an elbow.
  - **Selected row:** `#3c3836` fill with a 2px aqua `#8ec07c` inset on the left.
  - **Status marks:**
    - running: a 7px dot `#8ec07c`
    - waiting: an 8px ring, half-filled `#fe8019`
    - idle: an 8px ring `#928374`
    - done: `ph-check` `#b8bb26`
    - failed: `ph-x` `#fb4934`
- **Pane header** (padding `14px 20px 0`, grid `1fr 560px`, centered vertically):
  - Left: a mono 12px line (status mark, name in the agent color, context in `#928374`), the title at 19px/500 `#fbf1c7`, and a subtitle at 13px `#a89984`.
  - Right: a **2×2 facts box** with ticket, PR, branch and todo. Mono 11.5px, padding `6px 11px`, radius 7, `#1d2021` fill, `#3c3836` border.
- **Thread panel:** margin `12px 16px 14px`, radius 8, `#1d2021` fill, `#3c3836` border.
  - Legend: a row of chips that doubles as the filter. Each chip is filled like the box for its kind, or outlined in `#3c3836` if that kind is unboxed. "Following latest" sits at the right.
  - Stream: newest at the bottom, padding `0 22px`, 10px between rows.
  - Bottom: the pinned request card (if any), the composer row, then the status line.

**Row grammar** (the same in rooms and transcripts):
- **Icon column:** every row has a 20px column, then an 8px gap. The icon is 13px with a 20px line height, and its color says the row's **kind**.
- **Header line:** mono 12px/20px. Subject (the name at 500 in the agent color, `→` in `#665c54`, then the target), then `· type` at 10.5px `#665c54`. Right-aligned: delivery state, then time (11px `#665c54`).
- **Body:** 13.5px/1.55 body font. Your text is `#fbf1c7`, an agent's text `#ebdbb2`, and text inside a message box `#d5c4a1`.
- **Boxes:** `margin: 0 -10px; padding: 6px 10px; radius 7px`. They grow into the margin, so text lines up whether or not a row is boxed.
- **In a session's own transcript, its own rows drop the name.** The header line shows only the type ("output", "thinking · 640 tok"). Anyone else is named: "You · prompt", "to settings-scan · task", "from planner · instruction".

| Kind | Icon (Phosphor) | Icon | Box (fill / border) |
|---|---|---|---|
| You | `ph-user` | `#ebdbb2` | `#32302f` / `#504945` |
| Output (the session's own text) | `ph-text-align-left` | `#d5c4a1` | none |
| Thinking | `ph-brain` | `#7c6f64` | none. Folds to one italic `#928374` line |
| Tool call | `ph-wrench` | olive `#8a9a5b` | `rgba(138,154,91,.10)` / `.45`. The tool name is mono 12.5/600 olive. The result is a second row: `ph-arrow-elbow-down-right` `#928374`, mono 11.5px |
| Message: agent ↔ agent, agent → room, agent → you, subagent task and result | `ph-arrows-left-right` | periwinkle `#9a9fd6` | `rgba(154,159,214,.10)` / `.45` |
| State: started, running → completed, joined | `ph-swap` | `#928374` | none. One mono 11.5px line |
| Hub notice (loop cap, rate limit) | `ph-info` | `#fabd2f` | `rgba(250,189,47,.09)` / `.45` |
| Needs you: ask / approval | `ph-question` / `ph-shield-warning` | `#fe8019` | `rgba(254,128,25,.10)` / `.6` |
| Failed delivery | `ph-x-circle` | `#fb4934` | `rgba(251,73,52,.09)` / `.5` |
| Selected row | | | `#32302f`, a 1px `#8ec07c` border and a 2px `#8ec07c` left inset |

**Folding:** a run of messages between the same two agents folds to its last message, with "N earlier · E" at the right. The latest run stays open. Thinking and long tool output also fold (`E`).

**Agent colors** (by agent). Each agent keeps one color everywhere: the tree, the thread, Needs you and the pane header. Colors are unique within a group and reused across groups, assigned in order of creation:
1. sky `#7fb3d5`
2. green `#b8bb26`
3. lavender `#b9a6e0`
4. aqua `#8ec07c`
5. rose `#e5a8a0`

"You" is `#ebdbb2`, and rooms are neutral `#d5c4a1`. A setting switches names to by template or off.

**Pinned request cards:**
- **Approval** (02, 03): a header (icon, plain-language title, `tool · cwd`, Copy) and the command in a mono block. Below it, two panels side by side:
  - **Parsed** is a deterministic shell parse: numbered steps with tags (`reads files`, `shell per file`, `local bin`), then checks such as "No writes" or "Paths inside repo".
  - **Explained** comes from a small model, with the provider named: what the command does, plus a suggestion to trust only its read-only parts.

  Buttons: **Allow once** (`O`, primary), **This session** (`S`), **Always: `pattern`** (`A`, editable), **Deny…** (`D`), and "Esc later" at the right. The request also shows in Incoming (see Agents in Incoming).
- **Ask** (Scion `input-needed`): a header ("handler is waiting for you · input-needed · 4m", with a link to the session or room), then the question at 14px `#fbf1c7`. The choices read from the message become option cards, keyed `1` and `2`. Then **Write a reply…** (`R`) and **Later** (`Esc`).

**Composer row:** "Message <agent or #room>" with a hint and `⌘⏎`. Then **Agent** (`N`, rooms only), **Open on Hub** or **Open in Zed**, and a terminal button that attaches to the session.

**Status line:** mono 11px `#928374`.
- **Sessions:** harness, model and host · cwd on the left. On the right: a context % bar, cache state, turns, elapsed time and cost.
- **Rooms:** Hub, thread id and mode on the left. On the right: loop cap, message count, live, and `? keys`.

**Screens:**
- **01 Room.** The room is a group's Scion thread, and its members are the group's sessions. Agent posts to the room and messages between agents are both message boxes. A mention you send while the agent is mid-turn shows "deferred, delivered HH:MM". Selecting a row with `J`/`K` adds a detail grid ("delivery", "seen by").
- **02 Local session.** A local session's transcript. The `settings-scan` subagent is one message box: task, its tool calls folded to one line, then result. There are no other agents.
- **03 Scion session.** Messages from planner, reviewer and the room are part of the agent's input, so they appear in its transcript. Its outgoing replies are Scion message calls; show them as "to planner" message rows, not raw tool calls.
- **04 Direct messages.** A tab on the agent, not a separate place in the tree: Transcript (`T`) or Messages (`M`). Messages filters to you ↔ the agent, your mentions of it in rooms, and its asks. The composer sends a Scion DM.
- **05 New agent.** `N` (or the Agent button) opens a modal over the whole tab, below the title bar. Scrim `rgba(15,16,18,.68)`. Modal 740px wide, `#282828`, 1px `#504945` border, radius 10px, shadow `0 24px 64px rgba(0,0,0,.55)`. Header: user-plus icon `#8ec07c`, "New agent", then "Scion Hub · grove <grove>" in mono `#928374`, and `Esc` at the right. The modal takes the keyboard: `Tab` moves through its fields, letters type, `Esc` closes it.
  - **Joins block** at the top (`#1d2021`, 1px `#3c3836`, radius 7px): a "Joins <project>" label, then the group's row as the tree draws it (caret, name, goal, host chip), then the room and members with their status marks. The new agent comes last, dashed in `#8ec07c`, and its label follows the Name field. Turning off "Add to #room" takes the room out of the block.

  The fields follow Scion:
  - Name (unique in the grove)
  - Template: chips with versions from the Hub catalog. A template is a role: harness-agnostic `agent_instructions` plus skills.
  - Harness: the harness config, meaning harness, model and auth together.
  - Broker
  - Branch (its own clone)
  - "Then: add to #room": a step Helm takes after start
  - Instructions: read-only, edited on the Hub
  - Skills
  - Task: the first instruction

  Buttons: **Start agent** (`⌘⏎`) and **Cancel** (`Esc`). The footer shows the matching `scion start <name> -t <template> -b <branch> "<task>"`.
- **06 Keys.** An overlay on `?`; the full table is under Keyboard below.
- **07 Settings › Agents.** A new section 6 in the Settings modal (Developer becomes 7). A live preview sits at the top. Each row is one `settings.json` key:
  - `agents.colors.you`, `.tool`, `.message`, `.hub` and `.needs_you`: each a short list of preset swatches, picked with ←/→.
  - `agents.names.color_by` (`agent` | `template` | `off`) and `agents.names.palette`
  - `agents.names.self_in_transcript` (default `false`)
  - `agents.boxes.strength`: subtle fill .07 / border .35, normal .10 / .45, strong .14 / .6
  - `agents.boxes.fold_pairs` (default `true`)
- **08 Delivery states**, quiet to loud:
  - **delivered:** `✓` and a count, `#665c54`
  - **deferred:** `ph-clock` `#fabd2f`
  - **unreachable** (the agent isn't running): `ph-plugs` `#fe8019` in an orange box. Actions: `⏎` Start it again and send, `F` Send to another agent, `Esc` Discard.
  - **failed:** `ph-x-circle` `#fb4934` in a red box, with the Hub's reason and a retry countdown. Actions: `⏎` Retry now, `E` Edit, `Esc` Cancel.

  Unreachable and failed also appear in Incoming (see Agents in Incoming).

**Keyboard:** `⏎` goes in, `Esc` comes out, and `Tab` cycles between the tree, the thread and the composer.
- **Everywhere:** `⌘K` search, `/` filter sessions, `g i` Incoming, `g a` Agents, `⌘.` interrupt the selected agent, `?` keys.
- **Tree:** `j`/`k`, `h`/`l` fold or open a group, `⏎` open, `n` new agent in the group.
- **Thread:** `j`/`k` move between rows, `e` fold or open, `⏎` reply to a message or open a tool call, `c` copy, `g` jump to latest, `t`/`m` switch tabs.
- **Needs you:** `o` `s` `a` `d`, `1`–`3`, `r` and `Esc`. These work whenever a request is pinned and focus isn't in the composer. **Proposed:** `1`–`4` also pick an approval's options in the agent's order, as they do in Incoming.
- **Composer:** `⌘⏎` send, `@` address one member, `↑` edit your last message.
- Single-letter keys are off while you're typing.

**Mapping sources to rows:**
- **ACP** (local): `session/update` notifications map to rows: agent message chunks become Output, thought chunks Thinking, and `tool_call` / `tool_call_update` Tool rows with results. `session/request_permission` becomes the approval card. The agent defines the permission options, so "Always: pattern" is **Helm's own allowlist**: Helm answers matching requests itself before they reach you, and one policy covers every agent.
- **Scion** (Hub):
  - Conversations (DMs and group threads) stream over SSE.
  - Message types map to rows: `instruction` and `reply` become messages, `state-change` becomes State, `input-needed` becomes an ask, `mention` a message, and `system` a Hub notice.
  - Each message's delivery status drives screen 08.
  - The agent state (e.g. `RUNNING`, `WAITING_FOR_INPUT`, `COMPLETED`) drives the status mark.
  - Templates and harness configs are listed from the Hub, and new agents are started through it.

**State:** selected tree node; focus zone (tree, thread or composer); selected row; folded runs; per-session transcript/messages tab; the queue of pinned requests per session (which also feeds the global Needs you list); the new-agent form; agent color assignments per group (persisted); and the `agents.*` settings.

**Open:**
- How the Hub adds a newly started agent to an existing group thread.
- Whether a harness's Scion message calls can be recognized reliably in its transcript.
- Whether a Zed-launched session can be observed at all (assume not).
- Final colors and box strength.

### Agents in Incoming (Agents Incoming 2a–2c)
Reference: `design/Helm Agents Incoming.dc.html`: **2a** a topic, **2b** Agents waiting, **2c** open states. It fills the "Also in Incoming" gap listed under Not built in `app/docs/agents.md`.

**What comes in:**
- **Waiting on you:** a pinned approval or ask, an undelivered message (unreachable or failed), a session that failed.
- **Not waiting:** a Scion message addressed to you (a DM, or a room reply to you), the final output of each local turn (one signal per turn), a Scion agent that completed.
- Subagents never show up by themselves.

**Where:**
- Agent signals are filed under topics like any signal, in an **Agents** app group after GitHub, oldest first (2a). The topic comes from the session's ticket, found the way the facts box finds it. Otherwise the AI guesses it from the group, its goal and the first task; a session with no group uses its directory and first task instead.
- **Agents waiting (2b)** is a second view: an entry under the rule next to Unsorted, styled like it. It lists every waiting agent across topics, grouped by project. They're the same signals as in their topics, so acting in one view acts in both.
- No special colour: a waiting agent is one signal among many.

**Row (collapsed):** one line, like any signal.
- The agent's status mark, the same as in the tree, sits in the 38px gutter.
- Then the agent's name in its colour, and the reason in title weight: *needs approval*, *has a question*, *didn't get your message*, *didn't take your message*, *stopped*, *wrote to you*, *ended its turn*, *finished*.
- Then `project › group · time` in the meta text, then the usual action cluster.
- Source actions go before the divider: **Reply** `r` on messages and turn ends, the PR on a finished agent.

**Actions only touch the signal, never the agent:** `c` confirms the topic, `t` moves it to another, `x` sends it back to Unsorted, `d` takes it out of Incoming. Confirming a waiting agent leaves its request open in Agents.

**Open (↵):** the conversation it's about, in Incoming's message layout (time, who, text). Then a line on why it's here, then the agent's answers.
- **Approval:** the full command, then Parsed and Explained (the same panels as the Agents card), then the agent's options.
- **Ask:** the question, the choices with their details, and Reply… `r`.
- **Unreachable:** Start it again and send, Send to another agent, Discard.
- **Failed delivery:** Retry, Edit, Cancel. The Hub gives no retry time.
- **Failed agent:** Start it again, Open on Hub.
- **Wrote to you / ended its turn:** the messages, with your last message faded before them. `r` opens a reply under them. A reply to a local session is its next prompt, and it resumes the session if needed.
- **Finished:** the agent's summary and a facts line (task, time, turns, PR, ticket).

**Keys:** `1`–`4` pick the agent's answers in its order, **only while the row is open**. `r` replies, and `o` opens the session in Agents. Everything else is as for any signal. No letter means something different here than it does elsewhere in Incoming.

**Leaving Incoming:**
- **Answered here:** a green check and the answer ("✓ Allowed once · 10:51") replace the actions, and the status mark changes to the agent's new state. The row is gone when you move off it. There's no undo.
- **Acted on in Agents** (answering, replying, sending to that agent): it's gone from Incoming at the next refresh. Just looking at it doesn't count.
- An ask also closes when the agent posts again or stops.

**Badges:** the Incoming badge counts agent signals. The Agents badge stays the waiting count.

**Open:** whether Agents belongs after GitHub in the fixed app order.

### Archiving agents (Agents Archive 01–05)
Reference: `design/Helm Agents Archive.dc.html`, sections **01–05**. Earlier archive explorations aren't included.

**Rules:**
- `a`, or right-click › Archive, archives the selected row and only that row: a session, a room, a group, or an Unsorted session. Archiving a session leaves its group and room.
- Nothing running or waiting for you can be archived, and neither can a room or group with a member still running. Stop them first.
- Archiving deletes the Hub agents and their clones, through the outbox. Messages and transcripts stay in the local store and stay searchable. Unsorted local sessions are only hidden.
- `⌘⏎` archives, `Esc` cancels, and `⌘⌫` force-archives something that has stopped but isn't done. `⏎` never archives. `u` undoes for 10 s.
- Archived items leave the tree. They come back only through the tree filter (`/`), and they open read-only. No restart, no reuse, no unarchive.
- ⌘K doesn't archive and doesn't list archived items.

**Screens:**
- **01 Done nudge.** It appears when every session in a group has completed and the PR is merged (or the ticket is Done).
  - A frame goes around the whole group: 1px `rgba(184,187,38,.32)` border, `rgba(184,187,38,.05)` fill, radius 7px, margin `2px 8px 4px`.
  - The frame's footer has a `rgba(184,187,38,.22)` top border. It shows a check-circle in `#b8bb26`, "Done · completed, #208 merged" (ellipsizes), then **Archive group** in `#8ec07c` and **Not now** in `#928374`.
  - The footer has no key hint, because `a` acts on the selection.
  - A folded group keeps the frame and footer. Not now hides it for 7 days. Alethiometer never archives on its own.
- **02 Confirm.** Same modal style as New agent.
  - **Header:** archive icon `#8ec07c`, "Archive", a kind tag (SESSION / GROUP / ROOM: mono 10.5px uppercase, 1px `#665c54`), the name, then the location in mono `#928374`.
  - **Body:** a small tree drawn like the Agents tree. Rows that will be archived carry the selection look (`#3c3836` with a 2px `#8ec07c` inset), and a group is one selection block tagged "will archive". Other rows are at 50% and tagged "unchanged".
  - **Below the tree:** one line with a trash icon in `#a89984`: "N agents and their clones will be deleted on the Hub."
  - **Behind the modal:** the tree dims to 32% except the target, which keeps the selection look.
  - **Footer:** primary **Archive session** / **Archive group and N agents** (`⌘⏎`, green), **Cancel** (`Esc`), and "u undoes for 10s".
  - **02c Not done** (stopped, not done): an amber block (`rgba(250,189,47,.08)` fill, 1px `rgba(250,189,47,.45)` border) titled "Not done". It lists unpushed commits first (lost with the clone), then open PRs, tickets not Done and failed agents. Footer: **Archive anyway** (`⌘⌫`) and Cancel. That is the only red button (`#fb4934`).
  - **02d Still running:** a prohibit icon in `#fe8019` and "Can't archive" in the title. One sentence: "<group> still has N agents going. Stop them, then archive it." The tree tags rows "running" (`#8ec07c`) or "waiting for you" (`#fe8019`); other rows are dim and tagged "ready". Footer: **Close** (`Esc`) only.
- **03 Undo.** The row stays in the tree for 10 s: struck through in `#665c54` on a faint hatch, with "deleting N agents in 8s · u undo" in `#fabd2f`. Then it leaves the tree, and the deletes go out through the outbox.
- **04 Tree filter.**
  - **Results:** `/` shows live matches first. Then a divider ("Archived · N goals · M sessions · of T"), and archived matches in tree layout (project › group › session) with the matched line under each.
  - **Fading:** while the selection is in the live results, the whole archived section is at 40%. When the pointer is over it or the selection moves into it, all of it comes up to full strength.
  - **Opening one:** `⏎` opens an archived item read-only. The header gets an "archived 2 Oct" chip, matches are highlighted (`⌘G` for the next one), and the bottom bar reads "Archived 2 Oct · read-only" with Back (`Esc`).
- **05 Right-click.**
  - Right-clicking selects the row, so the menu and `a` always act on the same thing.
  - Items: Open `⏎`, Stop `⌘.`, New agent in <group> `n`, a divider, then **Archive session…** / **Archive group…** `a`.
  - On a running agent, Archive is off with "stop it first".
  - Menu: 230px wide, `#32302f`, 1px `#504945` border, radius 8px.

**Outbox actions:** `scion.delete_agent`, one per agent, sent after the undo window.

**Open:**
- What happens to the group thread on Scion once its agents are deleted. Deferred; it's Scion-specific.
- A Hub delete that fails after the undo window isn't designed yet.

## Interaction model
Keyboard-first throughout: `j`/`k` move, `↵` opens, `o` opens in the source app, `/` or `⌘K` search, `z` fold, `Tab` retarget, `⌘R` retry, `Esc` close. Every action needs a key. Use hover tints from the palette and an aqua focus ring. No animations beyond short (≤150ms) fades or collapses.

## Design tokens (Gruvbox dark hard)
- Grounds: `#1d2021` app, `#282828` panels, `#32302f` panel headers and dividers, `#3c3836` borders, `#504945` strong borders
- Text: `#fbf1c7` titles, `#ebdbb2` body, `#d5c4a1` secondary, `#bdae93` / `#a89984` muted, `#928374` labels, `#665c54` disabled
- Interface state: aqua `#8ec07c` (selection edge, active tab underline, primary outline buttons, focus)
- Category colors (the user's kitty palette only): yellow `#fabd2f` work topics, blue `#83a598` Personal/Life, green `#b8bb26` Jobs, purple `#d3869b` self-holds on the work calendar (lunch, walk), red `#fb4934` alerts/delegated. Orange `#fe8019` is only for countdown and warning states.
- Category tints on blocks: the category color at 16–18% alpha plus a 2px inset top edge in the full color.
- Type: Inter. 13px base, 15px/1.6 for Notes lines, 10.5px uppercase with .1em tracking for section labels, 17–18px/500 for headings. Tabular numbers for times.
- Radius 4px on blocks, 6px on inputs and pills, 8px on panels.
- Buttons are outlined, never filled.
- **Scale:** all sizes above are CSS px at `ui_scale` 1.0, and the target is 1.15 (so 13px renders at about 15px). Suggested code cleanup: move the ~190 hard-coded `font-size`/`width` values in `app.css` and the Svelte components to rem, with tokens like `--fs-label: .8077rem` (10.5/13) and `--fs-body: 1rem`, and set `html { font-size: 13px }`. Then one root value controls type, and `ui_scale` controls everything else. Until then, `ui_scale` alone is enough.

## Assets
`design/icons/`: desaturated app icons supplied by the user (gcal, gmail, jira, marvin, notion, obsidian, protoncal, protonmail, sheets, slack, thunderbird). **Add a GitHub icon in the same style.** Other icons come from Phosphor.

## Screenshots
Static renders of each reference view, in `screenshots/`, for side-by-side comparison:
- `7a-topics.png`, `7b-daystrip-empty.png`
- `8a-incoming.png`, `8b-priorities.png`
- `9a-notes.png`, `9b-reflect-day.png`, `9c-reflect-week.png`, `9d-reflect-nudge.png` (2x)
- `10a-life-blocks.png`, `11a-notes-receipts.png`, `12c-topics-index.png`, `13a-incoming-dismiss.png`, `13b-ai-failure.png`, `14a-rsvp-incoming.png`, `14b-rsvp-topic.png`, `15g-split-by-topic.png`, `15h-split-thread.png`, `16c-person-palette.png`, `17a-new-person.png`, `17b-link-person.png`, `18c-chronology-expand.png`, `18d-search-results.png`, `19a-gmail-rows.png`, `19b-gmail-todo.png`
- At 1760×1175 (`ui_scale` 1.15): `20a-incoming.png`, `20b-topic.png`, `20c-priorities.png`, `20d-notes.png`, `20e-reflect-day.png`, `20f-topics-index.png`. Where these differ from rounds 7–19 on sizing, the day strip or the Incoming row layout, follow round 20.

Caveats:
- **Capture glitches:** a few labels wrap or overlap that don't in a live browser ("Search everything", the Can wait chips, "Marvin · Inbox" in 11a). The 19/20 captures are worse: row titles, "Search everything" and the keyboard footer all wrap onto two lines. In the browser each row title fits on one line (checked in 20a). Where they differ, the HTML is the source of truth; no label in the real app should wrap.
- **Superseded in earlier rounds:** 7a–9d still show titled personal events on the strip (Physio, Pickup) and personal items in Priorities (e.g. Hetzner, Early pickup). The Work/Life toggle in 8b's header also predates the boundary rules. Follow the **boundary rules** above: on this machine, personal content appears only as untitled Life blocks, as in 10a/11a.
- **KDE window chrome:** 7a–10a show KDE-style title bars. Use the macOS title bar as in 11a.
- **Removed pieces:** 10a's "Pixel · connected" status and popover action "open on Pixel" are dropped (there's no link to the phone). Keep the popover's time, busy state and buffer.

## Files
- `design/Helm Agents Archive.dc.html`: archiving agents, **01–05** (see Archiving agents). Opens with `support.js`, `_ds/` and `DayStripGruv.dc.html` from this folder.
- `design/Helm Agents Incoming.dc.html`: agents in Incoming, **2a–2c** (see Agents in Incoming). Opens with `support.js`, `_ds/`, `icons/` and `DayStripGruv.dc.html` from this folder.
- `design/Helm Agents Final.dc.html`: the Agents tab, screens **01–08** (see the Agents section). Opens with `support.js`, `_ds/` and `DayStripGruv.dc.html` from this folder.
- `design/Helm Explorations 21-24.dc.html`: rounds 21–24. Relevant IDs: **24a–24d, 22a, 22b, 22c, 21e** (round 23 and 21a–21d superseded).
- `design/Helm Explorations 25-28.dc.html`: rounds 25 and later. Relevant IDs: **27a, 26a, 26c, 25d** (26b's source list, 27b, 27c and the rest of 25 superseded).
- `design/Helm Explorations 17-20.dc.html`: rounds 17–20. Relevant IDs: **17a, 17b, 18c, 18d, 19a, 19b, 20a–20f** (18a/18b superseded). Round 20 re-renders the reference views at 1760×1175.
- `design/Helm Explorations 13-16.dc.html`: rounds 13–16. Relevant IDs: **13a, 13b, 14a, 14b, 15f, 15g, 15h, 16c** (15a–15e, 16a/16b superseded).
- `design/Helm Explorations 7-12.dc.html`: rounds 7–12. Relevant IDs: **7a, 7b, 8a, 8b, 9a, 9b, 9c, 9d, 10a, 11a, 12c** (12a/12b superseded).
- `design/Helm Explorations 1-6.dc.html`: rounds 1–6, early directions, all superseded.
- The old single `Assistant Hub Explorations.dc.html` has been removed (it ran out of memory in Firefox).
- `design/DayStripGruv.dc.html`: the day strip component (props: `highlight`, `minutes`, `note`, `life`).
- `design/support.js`, `design/_ds/…`: runtime needed to open the HTML references. Not for production.
