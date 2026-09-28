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
  - Don't reuse `urgencyColor()`: its 15/5/2-minute thresholds belong to the countdown. Add a separate `meetingPillWeight()`, and ask the user whether the strip should follow it.
- **Sections:** Active first, sorted by next meeting then last signal. Then **Quiet · no signal in 7 days**, with row content at 60% opacity. The 7 days is a `settings.json` value.
- The selected row uses `.sel` and sets `ui.topicId`, which lights its meetings on the strip.

**Keys**, following TopicPicker:
- The filter has focus on arrival. ↑/↓ or ⌃j/⌃k move, ↵ opens.
- Esc leaves the filter. Then j/k move, `a` archives, `n` starts a new topic, and `/` goes back to the filter. (Global `/` search stays as it is everywhere else.)
- Archive is `a`, not `e`: `e` already means "edit note" inside a topic.

**Archive:** hovering a row, active or quiet, swaps the Next meeting cell for an **Archive** button. There's no bulk archive (the user was explicit in chat1: never throw data away in bulk).

**Backend**
- `topics.list` gains `open_count`, `last_signal_at`, `last_signal_source`, `next_meeting {start, title}`, `obsidian_path` and an `archived` flag argument.
- New commands: `topics.archive {id}` and `topics.unarchive {id}`, which set or clear `archived_at`.
- Every other query already filters `archived_at IS NULL` (suggest, incoming, notes, search, github, obsidian), so archived topics drop out of AI suggestions and pickers with no further changes. This resolves the ISSUES.md "archive/cleanup story" item, though only manually for now.

**Out of scope:** a periodic review of quiet topics (possibly in Reflect), and "Dismiss a signal" in Incoming, which is tracked separately in ISSUES.md.

### Incoming · Dismiss (13a)
Resolves the ISSUES.md item "No way to dismiss a signal in Incoming".
- **Where:** a ghost **Dismiss** button (eye-slash icon, `--tx-muted-2`, no border) goes after Wrong topic in topic view, and after Move to topic in Unsorted. The key is `d`, listed in the hints row.
- **Undo:** a dismissed row stays in place, struck through (`--tx-label`, faint hatch), with Dismissed and an **Undo** `.btn` (`u`) in place of the actions. It disappears on the next `refresh()` or when the selection moves away.
- **What dismissing does:** it marks the item seen without a topic link and never writes to the source. The item comes back into Incoming only on new source activity (new messages, status change, edit), the same way `seen` resets today.
- **Backend:** add `incoming.dismiss {item_id}` and `incoming.undismiss {item_id}`. Add `items.dismissed_at` if needed to tell a dismissal apart from a plain seen.
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
- `10a-life-blocks.png`, `11a-notes-receipts.png`, `12c-topics-index.png`, `13a-incoming-dismiss.png`, `13b-ai-failure.png`

Caveats:
- **Capture glitches:** a few labels wrap or overlap that don't in a live browser ("Search everything", the Can wait chips, "Marvin · Inbox" in 11a). Where they differ, the HTML is the source of truth; no label in the real app should wrap.
- **Superseded in earlier rounds:** 7a–9d still show titled personal events on the strip (Physio, Pickup) and personal items in Priorities (e.g. Hetzner, Early pickup). The Work/Life toggle in 8b's header also predates the boundary rules. Follow the **boundary rules** above: on this machine, personal content appears only as untitled Life blocks, as in 10a/11a.
- **KDE window chrome:** 7a–10a show KDE-style title bars. Use the macOS title bar as in 11a.
- **Removed pieces:** 10a's "Pixel · connected" status and popover action "open on Pixel" are dropped (there's no link to the phone). Keep the popover's time, busy state and buffer.

## Files
- `design/Assistant Hub Explorations.dc.html`: all design rounds. Relevant IDs: **7a, 7b, 8a, 8b, 9a, 9b, 9c, 9d, 10a, 11a, 12c, 13a, 13b** (12a/12b superseded).
- `design/DayStripGruv.dc.html`: the day strip component (props: `highlight`, `minutes`, `note`, `life`).
- `design/support.js`, `design/_ds/…`: runtime needed to open the HTML references. Not for production.
