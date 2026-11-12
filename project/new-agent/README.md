# New agent form: design export

Exported 2026-11-12 from the Claude Design canvas "Alethiometer · New agent". This folder is the reference; the canvas itself isn't reachable from a coding session. The decisions are in `app/docs/decisions.md` › "New agent form and the Agents tree".

## Current

**F1–F3 are the design to build.** `renders/` has a PNG of each, for when you can't open the HTML.

| Board | What it shows |
| --- | --- |
| `F1-Saved-form.dc.html` | A saved agent picked, one field changed (dot on its label, "Save 1 change to …" in the footer). OpenShell, so Sandbox and Image show. |
| `F2-New-form.dc.html` | New: a one-off at the defaults, running locally (no Sandbox or Image rows). The strip under it is the "Save as a saved agent" box ticked. |
| `F3-Manage-form.dc.html` | ⌘, editing a saved agent: list with handles and row actions, the same form on the right, empty fields taking context. |

Two things in F1–F3 are text a mock can't express: the top line's repos are joined with `&` and follow the Directories field, and the Agents tree they refer to is grouped by repo set, not by group (no board draws the tree).

## History, kept for reference

Each row was a step; the captions on the boards say what each one tried.

- **3a–3e:** the modal as built (3a), the first shortening with directories as chips (3b–3d), and the Directories field's states (3e). 3e still holds for the Directories field.
- **W1–W7:** wireframes placing the read-only Joins information. The user found the mix of read-only and editable confusing; F1–F3 keep only the top line.
- **X1–X6:** broader directions. The user picked X3 (recent launches as recipes) and X4 (a draft row in the tree) to develop; X5 (launch as a timeline) might suit somewhere else; X6 (fork) doesn't fit this use.
- **Y1–Y4:** X3 and X4 combined.
- **Z1–Z3:** saved agents on the left, with an inline-value block on the right. Replaced by F1–F3's plain form.

`canvas.json` is the canvas's layout (board positions and titles) and its notes, including the long note under 3a on what folds into what.

## Opening the boards

Serve `project/` and open a board, e.g. `python3 -m http.server -d project` then `http://localhost:8000/new-agent/F1-Saved-form.dc.html`. The boards use the repo's `project/support.js`, which loads React from unpkg. Fonts come from Google Fonts and fall back to system fonts offline.

Files: `na.css` (shared styles for the hi-fi boards), `wf.css` (wireframes), `ex.css` (rows X–F), `ds/alethiometer/tokens.css` (the Alethiometer design system's tokens as installed in the canvas, a copy of its generated CSS; `project/_ds/alethiometer/bundle.css` is the repo's own copy of the system).
