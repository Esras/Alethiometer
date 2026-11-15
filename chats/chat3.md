# Due todos in Incoming

_Started 2026-10-27 UTC_

---

Condensed from the conversation: the user's messages and choices verbatim where they mattered, the assistant's turns summarized. Mock: `project/Incoming Todo Reminders.dc.html`. Spec: `project/design_handoff_helm/README.md` › Due todos in Incoming. Decision: `app/docs/decisions.md`, 2026-11-04.

## User

I am trying to figure out what a feature that would help surface todos in the Incoming pane look like. Utilize the Alethiometer design system for design guidelines, and work with me to ask questions for what might be important or useful for trying to design this feature.

## Assistant

Read the design system (README, decisions, Decision 13/14/19 cards) and asked questions in rounds.

- **What "surface todos" means:** the user chose **show existing todos** (not detecting asks in signals, not a signal → todo action).
- **Which todos:** **due soon / overdue**; maybe ones related to the open signal, but unsure.
- **Why:** **don't forget them**.
- **Actions from Incoming:** jump to it, mark done, link signal to todo.
- **Todos carry:** topic, person, due date, source signal link.
- **Window:** **overdue + today**. **Keyboard:** the same cursor as signals. **Overflow:** show all.

Flagged tensions: a strip showing everything can push signals below the fold; a side panel with one shared cursor is awkward; interleaving by time has no common sort key with signals.

## User

Do rough wireframes, I think.

This is mostly intended as reminder for myself kind of feature, rather than a "new todos," since I can already do that in the application. The strip / show all is probably because I am not quite understanding what you mean by that. My main thing was that stuff shouldn't be hidden, and I'm not looking to redesign the Incoming pane entirely, this should probably integrate with it.

The side panel + same cursor was a similar answer on my part, where I may not fully be understanding what you're proposing here, so my answers were off. The main thing is just that it shouldn't be an entirely separate navigation.

I'll worry about the color overlap later.

Undated todos maybe need a nudge somewhere for assigning dates to, but I think I need to put more time into how I do todo things.

## Assistant

Three wireframes on a canvas: **A** a due group at the top of the list, **B** todos attached under related signals (with a leftover group), **C** a Due panel beside the signals (h/l to move between columns).

## User

So either A or B. I think those are leaning more towards what I'm thinking. The nice thing about B is that it has context, but I think that the context is usually not a single signal, but could end up being multiple (need to deal with that later), so I'm thinking A. I guess they're naturally a distinct category, since other sources get their own.

## Assistant

A became a "Todos" group drawn like a source group.

## User

I think it's worth pulling the repo at this point or looking at the other screens to make this slightly more accurate. ToDos _don't_ link to any other signals right now, and ideally it would open the ToDo, but right now, hitting 'o' on a ToDo opens the Signal that is linked to the ToDo (Jira issue or the signal that an Alethiometer ToDo was created from). I don't know that this needs much more design, but it is probably worth making sure of that.

## Assistant

Read `Incoming.svelte`, `Todos.svelte` and the fixture. Incoming is per topic (topic list on the left, the selected topic's signals grouped by app, Assistant on the right). In Todos, `o` opens the source and `↵` expands the originating signal in place; both can be reused, so no new design there. The fixture has 20 Jira todos overdue or due today across ~17 topics, most with no new signals, 3 with no topic. Two versions: **A1** a Todos group inside each topic (todo-only topics join the list), **A2** one "Todos due" entry beside Unsorted.

## User

A1.

## Assistant

Asked four follow-ups. Answers: topics with only due todos are **mixed in**; no-topic todos go to **Unsorted**; done **leaves at once** (with undo); due todos **count toward the tab's number**.

## User

Mixed in: Looks fine.
No Topic: Just in plain unsorted. No extra section.
Done: Yup, the undo should still work as usual. Minor concern: I don't believe x is the "done" operation. That indicates wrong topic. In general, I have tried keeping the buttons unique / consistent across the application.
Counts: Yup.

## Assistant

Done moved to **Space**: it is done in the Todos tab too, and Incoming only uses it inside a signal's messages, which never applies on a todo row. `x` is done in Todos and Priorities but Wrong topic in Incoming, so `x` stays Wrong topic here.
