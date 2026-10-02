# Reclassifying signals and merging topics

Design: 18c (chronology), 18d (search). Merging topics had no design; the decisions below are the user's (2026-10-01).

## Pickers

Move to topic and Link to person always open in one fixed spot: the view's dock, bottom-left of its panel just above the key hints, floating over the list (`lib/ui/Dock.svelte`, `DockHost.svelte`). Nothing shifts when a picker opens. Incoming, the split review, a topic's chronology and People panel, the Topics index and Search all use it. (The todo form's topic field is a form field and stays where it is.)

## Reclassifying a filed signal

One topic per signal: a move never adds a second topic, and it writes nothing to the source except a Gmail thread's category label when the category changes (`gmail.relabel`). A recurring event moves as a whole series.

- **Chronology (18c):** j/k select an entry (selecting adds nothing), ↵ or a click expands it in place (messages with sender colours, your replies, one faded context message, Open in the app), Esc collapses. While it's expanded: `t` moves the whole signal (topic picker, create allowed), `s` opens the split review for this one signal (only with more than one message). A moved signal leaves "moved to X · u undo" where it was until you leave the topic.
- **Search (18d):** ⌘K (Ctrl+K) stays a quick jump: ↵ opens the top hit. ⌘↵, or ↵ when nothing matches quickly, opens the Search tab: every work item matching, with its source, a snippet with the match marked, its topic ("no topic" when it has none) and its date; source chips filter. j/k move, ↵ opens (the topic, else the app), space expands, `t` moves, `s` splits, `u` undoes, `/` goes back to the query, Esc goes back to where you were.
- A move is filed straight away: the old link becomes `wrong`, the new one `confirmed` (`incoming.reclassify`). `u` restores every link exactly as it was (`incoming.unreclassify`).
- The split review on a filed signal shows only that signal (plus parts the review makes); a moved part is confirmed to its new topic straight away, so c is Done. Moving a message to Unsorted puts it back in Incoming.

## Merging topics

- `m` merges the selected topic (Topics index) or the open topic into another, picked in the dock. Topics already staged to merge away can't be picked.
- The merge is staged: the topic shows "merged into B · u undo" and nothing is written until you leave the screen (the index for a topic, one topic for another, or another tab). `u` cancels it. A merge still staged when the app closes is dropped.
- Then (`topics.merge`): every signal link moves to B (where an item has links to both, confirmed beats suggested beats wrong); people and repos linked by hand, and Notes lines pointing at A, move too; Gmail threads filed under A get B's category label if the categories differ; A's note text (frontmatter left out) is appended to B's note under `## Merged from A` (an ordinary note save: the outbox, or the file); A's note file stays where it is; A is deleted.
