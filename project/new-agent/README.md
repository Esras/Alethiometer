# New agent form: design

The final boards of the New agent form redesign (Claude Design canvas "Alethiometer · New agent", 2026-11-12). The decisions are in `app/docs/decisions.md` › "New agent form and the Agents tree"; read that first. The canvas's explorations weren't exported: the decision entry says what was tried and set aside.

## Boards

`renders/` has a PNG of each F board, for when you can't open the HTML.

| Board | What it shows |
| --- | --- |
| `F1-Saved-form.dc.html` | A saved agent picked, one field changed (dot on its label, "Save 1 change to …" in the footer). OpenShell, so Sandbox and Image show. |
| `F2-New-form.dc.html` | New: a one-off at the defaults, running locally (no Sandbox or Image rows). The strip under it is the "Save as a saved agent" box ticked. |
| `F3-Manage-form.dc.html` | ⌘, editing a saved agent: list with handles and row actions, the same form on the right, empty fields taking context. |
| `Dir-chips.dc.html` | The Directories field's states (empty, typing with recent matches, a missing path, two folders with the same name, many). Drawn for the earlier form; the field itself is unchanged in F1–F3. |

Not drawn anywhere: the Agents tree grouped by repo set, which replaces groups. The top line's `alethiometer & pi-acp` is how a set reads.

## Opening them

Serve `project/` and open a board, e.g. `python3 -m http.server -d project` then `http://localhost:8000/new-agent/F1-Saved-form.dc.html`. The boards use the repo's `project/support.js`, which loads React from unpkg. Fonts come from Google Fonts and fall back to system fonts offline.

Files: `na.css` (shared styles: chips, inputs, the Directories field, buttons), `ex.css` (the two-pane layout and the form), `ds/alethiometer/tokens.css` (the Alethiometer design system's tokens as they were in the canvas; `project/_ds/alethiometer/bundle.css` is the repo's own copy of the system). Both stylesheets hold only the rules these four boards use.
