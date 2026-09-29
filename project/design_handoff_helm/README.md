# Handoff: Helm — personal work assistant (macOS)

## Overview
Helm is a single-user, local-first desktop assistant for one person: a manager moving into director-track work with a heavy meeting load, who loses track of time and attention easily. It gathers work signals (calendar, Slack, Jira, GitHub, a work Obsidian vault) under **Topics**. It then helps with four jobs: triage (Incoming), deciding what's next (Priorities), capture (Notes) and reflection (Reflect). Life/personal commitments are visible only as untitled busy time.

This is a personal tool, not a product. Hard-coded choices, a `settings.json` and a short README for setup are all fine. Don't build settings screens, onboarding, multi-user support, telemetry or billing.

## About the design files
The files in `design/` are **design references built in HTML**. They're prototypes showing the intended look and behavior, not production code. Recreate them in the stack below. Open `design/Assistant Hub Explorations.dc.html` in a browser (serve the folder locally, e.g. `python3 -m http.server`). It's a pan/zoom canvas of numbered design rounds, **newest at the top**. Earlier rounds are superseded; follow the option IDs listed per view below.

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
The window is about 1320×880 in the mocks. The top of every view except Reflect is a tab bar (Incoming, Priorities, Topics, Notes, Reflect, with ⌘K search at the right) and the **day strip**. The chat/ask pane is collapsible.

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
- A second line holds a `no reply` pill, **Accept** `y`, **Decline** `n` and **Decline with note…** `⇧N`, followed by the overlap line.
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

## Assets
`design/icons/`: desaturated app icons supplied by the user (gcal, gmail, jira, marvin, notion, obsidian, protoncal, protonmail, sheets, slack, thunderbird). **Add a GitHub icon in the same style.** Other icons come from Phosphor.

## Screenshots
Static renders of each reference view, in `screenshots/`, for side-by-side comparison:
- `7a-topics.png`, `7b-daystrip-empty.png`
- `8a-incoming.png`, `8b-priorities.png`
- `9a-notes.png`, `9b-reflect-day.png`, `9c-reflect-week.png`, `9d-reflect-nudge.png` (2x)
- `10a-life-blocks.png`, `11a-notes-receipts.png`, `12c-topics-index.png`, `13a-incoming-dismiss.png`, `13b-ai-failure.png`, `14a-rsvp-incoming.png`, `14b-rsvp-topic.png`, `15g-split-by-topic.png`, `15h-split-thread.png`, `16c-person-palette.png`, `17a-new-person.png`, `17b-link-person.png`, `18c-chronology-expand.png`, `18d-search-results.png`

Caveats:
- **Capture glitches:** a few labels wrap or overlap that don't in a live browser ("Search everything", the Can wait chips, "Marvin · Inbox" in 11a). Where they differ, the HTML is the source of truth; no label in the real app should wrap.
- **Superseded in earlier rounds:** 7a–9d still show titled personal events on the strip (Physio, Pickup) and personal items in Priorities (e.g. Hetzner, Early pickup). The Work/Life toggle in 8b's header also predates the boundary rules. Follow the **boundary rules** above: on this machine, personal content appears only as untitled Life blocks, as in 10a/11a.
- **KDE window chrome:** 7a–10a show KDE-style title bars. Use the macOS title bar as in 11a.
- **Removed pieces:** 10a's "Pixel · connected" status and popover action "open on Pixel" are dropped (there's no link to the phone). Keep the popover's time, busy state and buffer.

## Files
- `design/Assistant Hub Explorations.dc.html`: all design rounds. Relevant IDs: **7a, 7b, 8a, 8b, 9a, 9b, 9c, 9d, 10a, 11a, 12c, 13a, 13b, 14a, 14b, 15f, 15g, 15h, 16c, 17a, 17b, 18c, 18d** (12a/12b, 15a–15e, 16a/16b, 18a/18b superseded).
- `design/DayStripGruv.dc.html`: the day strip component (props: `highlight`, `minutes`, `note`, `life`).
- `design/support.js`, `design/_ds/…`: runtime needed to open the HTML references. Not for production.
