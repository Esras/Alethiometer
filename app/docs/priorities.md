# Priorities

The design for the Priorities tab, replacing 8b's urgent × important grid. Agreed with the user 2026-10-06; **built 2026-10-07** except heat and tag suggestions (see "Built" and "Decided while building" below). Mock: `project/Priorities Three Orders.dc.html` (interactive; serve `project/` locally as for the other design files, e.g. `python3 -m http.server`), screenshot `project/design_handoff_helm/screenshots/priorities-three-orders.png`. The sample todos, scores and the Neglect weights in it are made up.

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

## Built (2026-10-07)

Code: `store/priorities.rs` (each open todo's facts, one query), `queries/priorities.rs` (the curves, the score, the three orders, the sample todos), `views/Priorities.svelte`. Nothing is stored: every read scores afresh.

- **Where the score is computed (user, 2026-10-07):** the database gives each todo's raw facts; Rust resolves people (the Resolver decides who is "you" and whose People note has which weight) and computes the curves, groups and score as pure functions. A plain view couldn't read the Clock (`fake_now`) or `settings.json`. Nothing is written, so AGENTS.md's "never written twice" holds.
- **Settings:** `settings.json` › `priorities` (file only; not on a Settings screen yet): `coefficients`, `neglect` (`age`, `quiet`), `quick_max_minutes` (30), `tags` (`name`, `coefficient`, `feeds`: value / pressure / effort).
- **Which todos:** open, owned by you, local / Jira / Obsidian tasks on work sources, as the Todos tab, plus the asks you delegated from Notes. Jira epics and `todo_exclude_projects` are left out.
- **New inputs:** `items.sched_day` (local todos: `todos.schedule`, TodoForm's Scheduled field, a held todo's `sched`); `items.source_created_at` (Jira `created`); `topics.priority` (`topics.set_priority`); `people.weight` (a People note's `weight`, clamped to 0–1); Jira `issuelinks` in the sync's fields; `item_tags` / `topic_tags` (storage only).
- **Every table has `created_at` and `updated_at`** (user, 2026-10-06), kept by the database (`store/timestamps.rs`, see `db-plan.md`).
- **Screens for the new inputs** (user, 2026-10-07): `p` cycles a topic's priority (H → M → L → none) on the Topics index and on a topic page; the Todos tab has a Sched column for local todos, edited with `d`; TodoForm has a Scheduled field for local todos.
- **Knock-on:** the Todos tab's Priority column is the todo's rank; a topic's Open panel lists your todos and other people's items mixed, by Priority score (user, 2026-10-07): other people's items are scored with the same factors and coefficients as yours (`topic_scores`); `/` on Priorities focuses its filter instead of opening search. Merging topics moves topic tags; the merged-away topic's priority is dropped (the target keeps its own).
- **Sample data:** the mock's 34 todos (made up). Five titles differ from the mock so other tabs' sample data behave as before: PLAT-1204 (not 1201, which the Notes sample uses), "retry limits ADR", "renew staging webhook secret", "decide retry caps" (Notes' word matching) and "Thank Lee for covering the pager" (the Glean ask test's local evidence).

## Decided while building (user, 2026-10-06/07)

- **Scope:** build steps 1, 2, 5 and 6, plus tag storage. Heat (step 3) and tag suggestions and their Incoming section (step 4) wait for the work Mac; the tag UI is to be discussed.
- **Scale:** the raw Taskwarrior sum (unbounded; a hard due date alone reaches 12). The Score filter is the top, middle and bottom third of the Priority column, over all open todos.
- **Coefficients:** Taskwarrior's defaults only: due 12, scheduled 5, blocks 8, topic priority H 6.0 / M 3.9 / L 1.8, tags 1.0 each. The factors Taskwarrior doesn't have are built with the curves below and start at 0 until tuned: topic meeting, waiting on you, who asked, topic breadth, estimate (effort).
- **Curves:** due is Taskwarrior's (0.2 at 14+ days out, linear to 1.0 at 7 days overdue; a day-only due counts whole days); scheduled is Taskwarrior's step (1 from its day on); blocks is 1 for any open outward Blocks link; topic meeting 1 within 15 minutes of the next meeting on the topic (or during it), linear to 0 at 8 hours out; waiting on you 0 → 1 over 3 days, only when someone else (not a bot) spoke last on the thread; who asked is the asker's People-note `weight`; breadth is 0.5·min(1, people/10) + 0.5·min(1, sources/5); estimate 1 at ≤5 min, 0 at ≥4 h, log scale.
- **Topic meeting:** a Pressure factor (the mock's "meeting in 8m").
- **Who asked:** a weight per person, `weight` in the People note (0 to 1).
- **Topic priority:** H, M, L or none.
- **Effort pills:** only "AI guess" / "your estimate" (coloured by size). The mock's "one click", "hand off", "needs a review meeting" were tags, not effort.
- **Reason pills:** coloured by their share of the score: red for a third or more, yellow from a sixth, green below. Only factors that added to the score get a pill.
- **Delegated asks** (Notes `^@name` lines) are in, scored exactly like your todos (user, 2026-10-07). They carry no estimate, and none is guessed: they aren't yours to do, so they can't be quick wins and sit at the bottom of Quick wins. They aren't in the Todos tab. A Notes line has no due date or thread yet, so for now they score mostly on their topic.
- **Neglect:** 0.4 × age days + 0.6 × quiet days, both weights settings. Age runs from the source's own created time where it gives one (Jira), else when Alethiometer stored the todo. Quiet time runs from the latest of: the newest non-bot message on its thread, the todo's `updated_at` (any change to the row), its topic link's `updated_at`, its creation.
- **No estimate:** Jira epics are left out of Priorities; other unestimated todos sit at the bottom of Quick wins, faded, and Effort counts 0.
- **Date filter:** Overdue, Today, Next 7 days, Later, No date (plain day counts, no week boundary). A row's date is its hard due date, else its soft one.

**Calls made without asking (say if any is wrong):**
- The Factor filter lists a factor when its fact exists (a date, an asker, an estimate, a meeting within 8 hours), even if it adds 0; breadth needs someone on the topic. A soft date that has passed is "Today" in the Date filter (it's never overdue).
- A hard due date's text is brighter only when it's today; the mock also brightened Wednesday.
- The columns fill the window's height (fixed by the window, not by their content) rather than the mock's 625px. The Assistant pane stays at the right, as on every tab.
- Topic priority shows as a small mono tag (grey, not a status colour).

**A consequence to know:** quiet time uses `items.updated_at`, so anything that changes a todo's row ends its quiet time, including a sync rewriting a Jira issue after a bot's comment (Jira's `updated` moves). The bot's comment itself isn't counted as a signal.

## Not built yet

- Heat (thread and topic) and its baseline; the early-days cap.
- Tag suggestions (the small model's pass, the Incoming section) and any tag UI; the vocabulary is empty by default.
- A Settings screen for the coefficients; a page for experimenting with sorts and coefficients.
- In the design system artifact: decision 14 (Priorities compact) and dropping the Priorities exceptions in decisions 3 and 12. The code follows them; the artifact isn't updated.

## Not yet designed (from the design pass)

- ~~**Who asked** has no weights~~ Settled: a `weight` per person in their People note (its coefficient starts at 0).
- ~~**Delegated asks** mean something different~~ Settled (user, 2026-10-07): the same factors as your todos; no estimate, so never a quick win.
- **The heat baseline:** per topic, per channel, over what window. Best worked out on the work Mac against real signals.
- **The 0-to-1 curves** for factors Taskwarrior doesn't have: first versions built ("Decided while building"), at coefficient 0 until tuned on real data.
- **Effort in the Priority score:** built, coefficient 0 until picked.
- ~~**Screens for the new inputs**~~ Built: `p` on topics, Sched (`d`) in the Todos tab, Scheduled in TodoForm.
- Dividers (decision 15) at compact density; the column header descriptions (the user will revisit); a page for experimenting with sorts and coefficients (later).
