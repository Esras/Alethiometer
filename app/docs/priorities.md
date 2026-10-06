# Priorities

The design for the Priorities tab, replacing 8b's urgent × important grid. Agreed with the user 2026-10-06; nothing here is built yet. Mock: the "Priorities · three orders" canvas (https://claude.ai/artifact/7idSzDcMgPFxTR1apMTM6r, private to the user until shared).

Priorities ranks open todos and shows them in three orders side by side: **Priority**, **Quick wins** and **Neglect**. Every column holds every todo, so nothing is hidden by a collapsed quadrant; a todo showing up near the top of more than one column is the point.

## Decided

- Todos only: local, Jira, Obsidian, and delegated asks (24). Suggesting todos from topics or recent signals comes later.
- The todo is what gets ranked. Its topic contributes; it doesn't decide.
- Jira priority isn't used: it's set inconsistently, and most issues keep the default.
- Coefficients start from Taskwarrior's urgency defaults (https://taskwarrior.org/docs/urgency/).
- Quick wins: 30 min or less, ordered by value per hour. Deadlines are ignored.
- Neglect: age combined with quiet time. Quiet time runs from the last signal on the todo, or the last change to it (edited, rescheduled, topic changed), else from creation. How the two combine is a setting. Surfacing old junk is the point.
- The tag vocabulary is built from real data while implementing (on the work Mac), and added to or trimmed in Settings.
- Tag suggestions show in Incoming, in their own section apart from topics with new signals and Unsorted.
- A topic's Open panel (22b's Tasks) is that topic's todos in Priority order. No groups.

## Scoring

Taskwarrior-style: each factor is scored 0 to 1 and multiplied by its coefficient (`settings.json`). The Priority score is the sum of every factor. **Value** (how much it matters), **Pressure** (time) and **Effort** (how long) group the factors, for the reason pills and for the other two columns.

| Factor | Feeds | From | Today |
|---|---|---|---|
| Due date (hard) | pressure | Jira `duedate` / `due_field`, local due | yes |
| Scheduled date (soft) | pressure | local todos; Marvin's do date later | no field |
| Estimate | effort | `todo_estimates`, AI or you | yes |
| Tags | per tag | suggested, then confirmed; inherited from the topic | no |
| Blocks | value | Jira `issuelinks` | not synced |
| Who asked | value | `from_item` author, Jira reporter | data yes, no weights |
| Waiting on you | pressure | time since someone else last spoke on the thread | no |
| Thread heat | pressure | the `from_item` thread, Jira `bump` | data yes |
| Topic priority | value | set by you | no (`topics.score` is sample-only) |
| Topic heat | pressure | the topic's signals against its own baseline | no |
| Topic breadth | value | `topic_people`, distinct sources | data yes |

Every todo in a topic inherits the three topic rows. A hard due date ramps steeply and goes red when overdue; a soft date starts adding pressure on its day, ramps gently and never goes red.

**Heat** is recent signal activity measured against how busy that place usually is (tf-idf in spirit), so a new reply in a quiet DM outweighs the 40th message in an incident channel. Bot accounts (`is_bot`) don't count.
- Topic heat: inherited by every todo in the topic, with a small coefficient.
- Thread heat: activity on the todo's own source (`from_item` for local todos, the issue itself for Jira). Obsidian `- [ ]` tasks and Notes lines have no thread and get topic heat only.
- A new topic's heat is capped until it has a few weeks of history.

**Tags:** a short list in Settings, each tag with a coefficient and the score it feeds. The small model suggests only from that list (never invents one, unlike topics), suggested then confirmed like topic links. A todo inherits its topic's tags.

## The page

| Column | Order | Faded |
|---|---|---|
| Priority | score: every factor times its coefficient | nothing |
| Quick wins | 30 min or less by value per hour, then the rest by length | past the cut-off, dashed line at the boundary |
| Neglect | age and quiet time combined, most neglected first | nothing |

- **Layout:** header, Selected panel, filter row, the three columns, key hints.
- **Selected:** a fixed 64px, always present, so picking a todo never moves the page. Line 1: source icon, title, topic, date, and its rank in each column as buttons that jump there. Line 2: Value, Pressure and Effort, each with its number and reason pills. Effort's pills: where the estimate came from ("AI guess" / "your estimate", coloured by size), plus what makes it cheaper or costlier ("one click", "hand off", "needs a review meeting").
- **Filters:** text (`/`), Topic (with a search box), Score (7 and up, 4 to 7, under 4), Date (overdue, today, rest of the week, later, none), Effort, Source, Factor (each factor, plus the tags). Options combine with "or" inside a dropdown and "and" across. A filter removes todos from all three columns. Chip labels stay fixed (a count badge shows how many are picked), and the "21 of 34" count and Clear keep their space, so nothing shifts.
- **Rows:** compact (24px, decision 14 changes for Priorities), one line: rank, topic-colour bar, source icon, title, faded topic, date, the column's number (score, estimate, age). "due Wed" is hard, "sched Thu" soft; overdue is red. In Neglect the date slot shows quiet time ("quiet 12d").
- **Columns scroll** on their own, at a fixed height.
- **Splines** between columns for the cursor todo (aqua) and the hovered one (grey dashed), following each column's scroll. When the todo is scrolled out of a column, its spline pins to that edge with a hollow end, and a "↑ #n" / "↓ #n" button scrolls back to it.
- **Cursor:** `aqua-tint` in the focused column; the same todo in the others is `bg-head` (decision 3's open row of an unfocused list). The focused column's name brightens and its count turns aqua.
- **Keys:** `j`/`k` move down and up the focused column, scrolling it; `[`/`]` move to the previous or next column on the same todo and scroll to it; `/` focuses the text filter; `Esc` closes a dropdown or leaves the field; `↵` opens the topic, `o` opens the todo in its app, `x` completes it.
- **Day strip:** lights the selected todo's topic meetings, as today.
- **Phone width:** columns stack, no splines.

## Build steps

1. **Missing inputs.** A scheduled date on local todos. Jira `issuelinks` in the sync's field list (`FIELDS` in `sync/connectors/jira.rs`). A topic priority you can set. A last-changed time on todos, for Neglect.
2. **Scores.** Factors, Value/Pressure/Effort and the Priority score per todo, computed by the database (a view), per AGENTS.md's rule on derived values. Heat aggregates signals, not todos, over an items table measured at 200k rows (REVIEW-2026-10-03); with indexes a view should do, cache only if it's slow. Reasons come from the factors that contributed. The sample seed in `queries/priorities.rs` goes.
3. **Heat.** Topic heat and thread heat with their baselines, and the early-days cap.
4. **Tags.** The vocabulary in `settings.json`; suggestions from the small model's pass after each sync, shown in Incoming in their own section.
5. **The page.** Rebuild `views/Priorities.svelte` as three columns: rows, cut-off, splines, filters, keys, the Selected panel. The quadrants, eye toggle and topic chips go. Update the `ui-diff` scenes and `ui-keys` checks.
6. **Knock-on changes.**
    - The Todos tab's priority column (22a) shows a rank instead of a quadrant.
    - A topic's Open panel lists its todos in Priority order.
    - In the design system: decision 14 moves Priorities to compact, and the Priorities exceptions in decisions 3 and 12 (chip ring cursor, raw-alpha selected bar) go away.

## Not yet designed

- **Who asked** has no weights: nothing records that a manager or a customer outranks a peer. Needs a weight per person, or a tag on the person's note.
- **Delegated asks** are other people's work, so Value, Pressure and Effort mean something different. A guess: value from the topic, pressure from how far past due, effort tiny (a nudge).
- **The heat baseline:** per topic, per channel, over what window. Best worked out on the work Mac against real signals.
- **The 0-to-1 curves** for factors Taskwarrior doesn't have: who asked, waiting on you, breadth, neglect.
- **Effort in the Priority score:** Taskwarrior has no such factor, so its coefficient is ours to pick.
- **Screens for the new inputs:** setting a topic's priority, and a scheduled date on local todos.
- Dividers (decision 15) at compact density; the column header descriptions (the user will revisit); a page for experimenting with sorts and coefficients (later).
