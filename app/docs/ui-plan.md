# Front-end components: the design-system work list

The target is the **Alethiometer** design system artifact (https://claude.ai/artifact/WH4uwJJM3gk6Ux2HX7f565): its README (the rules), `tokens.json`, `components/bundle.css` and Consolidation (`merges.md`). That list was written against `main@6408967`. Everything below was re-checked against `main@7b1ab84` on 2026-10-04: line numbers are current, sites it missed are added, and items are grouped by the shared piece they become rather than by decision number.

Paths are under `app/src/`. "→" is the target class or component.

## Status (2026-10-05, on `main`)

- **Done:** Phases 0–2, and Phase 3 decisions 3, 4, 5, 6/17/19, 7, 8, 9, 11, 12, 13, 14 and 18, and NeedCard's 6px head. Each has a "Done (2026-10-05)" note below, and a "User, 2026-10-05" note where the user changed or settled something on review; those notes win over the design system artifact.
- **Next, in this order:** the icon colour list (below); 20 Glean citations (the `local` field is in; next the marks and colours). Then 15 (lines vs outlined rows) goes back to the user.
- **User, 2026-10-05:** pickers that act on a row float under that row (`Dock under`), replacing the 2026-10-01 fixed dock; lines between rows stay, at least in a topic's Open panel (15 leans to lines; Priorities isn't live, so it can't be compared).
- **Deferred:** the Cycler (10; built in ac11400 and reverted: one button showing only the current value hides the options), the Life-block hatching (DayStrip, Notes: the feature needs rethinking), Notes line states.
- **Icon colours (user, 2026-10-05):** yellow is the icons' accent (`--icon`) for an icon that names a thing or an action, carets included; a field's icon (search, filter, a picker's arrow) and a bare close × are grey; add/create (+) is aqua, the "Confirm green"; an icon in a red failure count takes the red; a disabled button's icon greys with its text. All in `app.css` as `:where()` rules, so anything that colours an icon on purpose wins. Per-icon exceptions come from the list at https://claude.ai/artifact/LjJifUHdovU3Wa3UAygkVn (the user decides as they go).
- **The design system artifact** was brought up to date on 2026-10-05 (as of `6d4abfd`: decisions 12 and 18 done, 20's `local` flag, the open items below). Its stale generated files (`tokens.css`, `api/`, `manifest.json`) are the page's to regenerate; see "Problems in the design system itself".
- **How each decision was run:** a Sonnet subagent in a worktree with its own `UI_DIFF_SLOT`, from the current head; it commits one decision with a Done note and reports before/after crop regions. The lead cherry-picks, re-runs `scripts/ui-diff.sh <previous head>` + `node scripts/ui-diff.mjs` on the scenes it touched, `node scripts/ui-keys.mjs` and `npm run check`, then sends the user the crops (before above after). Two agents at a time; resolve `ui-diff.mjs` conflicts by keeping both scene blocks above `// Done and dismissed`.

## The rule for what becomes what

- **A global class in `app.css`** when the thing is only a look: `.btn`, `.kbd`, `.count`, `.chip`, `.label`, `.panel-head`, `.input`, `.inset`, `.scrim`, `.row`, `.empty-pane`/`.empty-row`, `.hints`, `.tag`, `.err`, `.avatar`, `.page-title`/`.pane-title`. Most of these sit inline inside buttons and rows, where a wrapper component adds prop plumbing and makes `.btn.primary .kbd` harder to style.
- **A `lib/ui` component** when it carries behaviour: Modal, Popover, Picker, Cycler, Switch, FilterField, NeedCard, Avatar, PersonName variants, StatusMark, CountdownPill. Each one today is written several times with different Escape, outside-click, focus and key handling.
- **Size of the problem:** 7,918 lines inside `<style>` blocks (44% of all `.svelte` lines). Top files: Settings 486, Incoming 413, agents/Tree 402, agents/Pane 296, Priorities 287, Topics 282, agents/RowView 271, TopicsIndex 258, DayStrip 238, Notes 218. Today `lib/ui/Panel` has one user and `lib/ui/Pill` two.

## Checking that a change doesn't change the look

`scripts/ui-diff.sh [ref]` starts the UI at `ref` on :1430 and this checkout's on :1420, each on its own dev server.
- **What both sides get:** fresh sample data, the clock at 10:52, and two scripted agents (one waiting on an approval, one on a question).
- **The comparison:** `node scripts/ui-diff.mjs [scene…]` shoots about 85 scenes on both and pixel-diffs them. The scenes cover every tab, the pickers, Settings' sections, the palette, Search, the Assistant, the agent cards and the New agent modal.
- **Diffs are exact,** with Chromium's deterministic raster and transitions off. A 0.01 alpha change to one token shows up as thousands of pixels.
- **Not covered:**
  - Settings › Developer, which shows each server's data directory;
  - anything the sample data can't produce: Scion rooms, a Glean answer, Slack review, todos.

## Phase 0 and Phase 1 · done (2026-10-05)

Pixel-identical to `eb633f4` on every scene. Commits `6501866`…`5920b59`.
- **Tokens:** `app.css` has the design system's tokens that aren't injected at runtime, at the system's values.
- **Agent form CSS:** `agents/form.css` is scoped under `.agent-form`. TodoForm keeps what it used to pick up from it.
- **Colours written out by hand** that equal a token now name it (43).
- **Reflect's three panels** use `<Panel collapsible={false}>`.
- **New global classes:**
  - `.panel` / `.panel.col`, the surface of Panel and of ten main columns and cards;
  - `.hint-bar`, for Incoming, Search and the Topics index;
  - `.mono-tag`;
  - `.command-block`;
  - `.kbd`.
- **Section-label copies** use `.label`.
- **Components:**
  - `lib/ui/StatusMark` (moved, plus `problem` and `offline` dots), now drawing the sync chip's and the room line's dots;
  - `views/agents/NeedCard`, the frame of AskCard and ApprovalCard;
  - `lib/ui/Switch` and `lib/ui/Segmented`, Settings' controls, written once.

**Left from Phase 0/1 on purpose:**
- **Radius tokens** (`--radius-sm/md/full`) come with decision 12. The old `--r-input` 6px and `--r-panel` 8px both become 6px, so there's no single old value to alias.
- **Todos' footer** differs from `.hint-bar` (7px padding, wraps, shrinks). It joins in the visible pass.
- **NeedCard props, to settle in the visible pass:**
  - `headBottom` (asks 4px, approvals 6px). **Done (2026-10-05):** the prop is gone; the head's bottom padding is 6px for both (the user's call; 6px is the app's usual spacing). Only the ask card's head changes (+2px);
  - `maxHeight` (approvals 55vh).
- **Settings' 6px attention dot** is a notification mark, not a StatusMark.
- **Agents tree colours.** `#9a9fd6` and `#8a9a5b` in the tree stay as literals: `--msg` and `--tool` follow the user's settings.
- **`:global` reach-ins** into child components (`Settings:955/960` → CategoryManager, `Incoming:1144` and `SplitReview:515` → PersonName, `TopicsIndex:371` and `Topics:348` → CategoryPicker, `NewScion:159`) get props when those components are next touched.
- **Collisions to clear before decisions 4 and 5 add `.count`, `.modal` and `.popover` globally:**
  - plain-number `.count` in `Priorities`, `reflect/Rollup`, `reflect/CarryList`, `topics/TopicsIndex`, `Search`, `shell/AssistantPane` and `lib/ui/Panel`;
  - local `.modal` (`settings/Settings`, `agents/NewModal`);
  - local `.popover` (`shell/DayStrip`).

## Phase 2 · progress (2026-10-05)

Done, pixel-identical on every scene (wave 1 to `7e6be64`, the pickers to `5e2d80e`), with `scripts/ui-keys.mjs` checks for the behaviour:
- **`lib/ui/Modal.svelte`** owns the behaviour of Settings, Slack review, New agent, the Keys sheet and the ⌘K palette.
  - **What it does:** one modal key-floor entry (the site's own keys first), Escape closes, mousedown on the scrim closes, Tab stays inside, focus returns, and `role=dialog` / `aria-modal`.
  - **The look stays with each site.** Scrim colour, z, position, inset and `isolate` are props until decision 5.
- **`lib/ui/popover.ts`**, an action on the popover's own element: Escape through the key stack (new `pushKeys`), and a pointerdown outside it and its trigger closes it. It's on the day strip's Life popover, the sync chip, the Priorities peek and the composer's @ menu.
- **Moved into `lib/ui`:**
  - `CountdownPill`'s look (the data stays in `shell/`);
  - `Stepper`;
  - `Avatar` (Topics people, the person picker, Glean's people; today's sizes are props).
- **`PersonName`** takes `variant` (`thread` | `tertiary`, both in the person's colour) and `dim`, instead of `:global` reach-ins from Incoming and the split review.

**Behaviour changes to try on the Mac:**
- **Settings, the palette, the Keys sheet:** they close on mousedown on the scrim, not on a click, so a drag that ends there no longer closes them. Tab stays inside, and focus goes back where it was.
- **Slack review:** it's a key floor now, so ⌘, no longer closes Settings underneath it. Press Esc first.
- **The Keys sheet:** Esc and `?` close it, and other keys stop at it instead of being swallowed.
- **New agent:** its keys go through the key stack. ⌘K, ⌘1–7 and `g i` still don't reach the window under it.
- **Stacking:** each box now sits inside its scrim's z-index. The Nudge toast could draw over the palette where they overlap.
- **Life popover in the day strip:** a click outside now closes it.
- **The Priorities peek:** a click outside closes it.
- **The composer's @ menu:**
  - Escape closes the menu and keeps focus; a second Escape blurs as before.
  - A click outside closes it, and typing another @ word brings it back.
  - While it's closed, Tab, ↵ and the arrows aren't taken for completion.

- **The five pickers** (topic, person, category, the Notes target picker, the font picker) share `lib/ui/picker.svelte.ts`. Four hand-written lists share `lib/ui/listNav.ts`: the palette, Settings, Slack review and the Topics index.
  - **All pickers:**
    - While a picker is open, keys it doesn't use stop at it instead of reaching the view.
    - ⌘↵ doesn't pick.
    - Clicking inside it (padding, icon, scrollbar) no longer closes it.
    - Rows pick on mousedown, and the cursor scrolls into view.
    - The cursor stops at both ends; it no longer wraps.
  - **Person picker:** it searches 80ms after the last key, not on every keystroke.
  - **Category picker:** it closes when its field loses focus, and it picks on mousedown.
  - **Notes target picker:** Tab no longer wraps around. ⌘↵ no longer sends the note while it's open.
  - **Keys (7a7a160, every list):** in a field, ↑↓ move the cursor; ⌃j/⌃k are gone (they collided with ⌘K's Ctrl twin). Outside a field, j/k and ↑↓; `[` and `]` move between lists where a view has several. ⌘ shortcuts take ⌘ on the Mac and Ctrl elsewhere, so on the Mac the Ctrl twins (⌃K, ⌃2…) no longer fire.
  - **Font picker:** it closes at once on blur, and its button now toggles it.
  - **Settings' list:** ↑↓ and j/k with Shift or Alt held no longer move the cursor.

**Not checked on screen:** Slack review (it needs a Slack source), Glean's people avatars (they need a Glean answer). PersonPicker's ↵, which writes a People note, was tried once by hand and isn't in the checks.

`scripts/ui-keys.mjs` leaves marks on the sample data (a notes line's target, Settings' "Saved"). Restart `scripts/ui-diff.sh` before running `ui-diff.mjs` after it.

**Moved to Phase 3:** the Cycler and FilterField. Both change the look (decisions 10 and 7), so they go with the visible pass. (FilterField is done: see decision 7.) **The Cycler is deferred (user, 2026-10-05):** it was built (ac11400) and reverted. One button showing only the current value hides the other options, which the segmented control shows at a glance. `Segmented` and `.seg` stay as they are until this is revisited.

## Phase 2 · New shared components (behaviour plus look)

1. **`lib/ui/Modal.svelte`**, with `{open, onclose, label, width?, inset?, children, foot?}`.
   - **What it provides:** the scrim and its click, `useKeys(..., {modal: true})` with Escape, a focus trap, focus restore and `aria-modal`.
   - **What's wrong today:** five scrims with five alphas, and only NewModal traps focus. SlackReview and Keys have no modal key floor, and nothing restores focus.
   - **Takes:** `settings/Settings:694/700`, `settings/SlackReview:102/108`, `agents/NewModal:46/55`, `agents/Keys:87/96` (absolute, 12px radius) and `shell/SearchPalette:270/276`.
   - **Look** → `.scrim` and `.modal` (shadow `0 24px 64px .55`). Today there are five different shadows.
2. **`lib/ui/Popover.svelte`**, with `{anchor | 'dock', placement?, flush?, onclose, closeOn?: 'outside' | 'blur', children}`.
   - **What it provides:** Escape through `useKeys`, one outside-click method (there are five today), and `.popover` / `.popover.flush`.
   - **Takes:** `DayStrip:461`, `SyncStatus:244`, the `Priorities:525` peek (measured in JS), `settings/FontPick:104`, `agents/Composer:339`, `notes/Picker:82` and `topics/CategoryPicker:49` (bg-app today, should be bg-panel).
   - **Absorbs `lib/ui/Dock`:** `anchor='dock'`. That fixes PersonPicker's double shadow (Dock's `drop-shadow` plus the card's own).
   - **Shares only the class:** `reflect/Nudge:81`, a fixed toast with no Escape (Alt+X only), and `assistant/SourcesPanel:137`, a side sheet.
3. **`lib/ui/Picker.svelte`**, plus a `listNav` helper. The five pickers are near-duplicates: TopicPicker (8 importers), PersonPicker, CategoryPicker, notes/Picker and FontPick. Each has a filter input, ↑↓ / ⌃j⌃k, ↵ to pick, Esc, and the pointer moving the cursor. They differ by accident:
   - **Keys:** notes/Picker has no ⌃j/k and wraps around.
   - **Close:** blur in four of them, but window mousedown in CategoryPicker and a 120ms timeout in FontPick.
   - **Pick:** mousedown in some, click in CategoryPicker.
   - **Frames:** three different ones.
   - **Cursors:** a right edge, a left edge, or none.
   - **Data loading:** PersonPicker sends a query per keystroke with no debounce; notes/Picker debounces at 80ms.

   Props: `{source: sync list | async (q) => list, row: snippet, create?, footer?}`. About 700 lines go. ⌃j/⌃k list navigation is also hand-written in SearchPalette, Settings, SlackReview and TopicsIndex; `listNav` covers those too.
4. **Deferred, see above.** **`lib/ui/Cycler.svelte`**, with `{options: {value, label}[], value = $bindable, key?, label, disabled?}` (decision 10).
   - **Renders:** `.cycler` (value, `ph-arrows-clockwise`, `.kbd`).
   - **Replaces:** global `.seg` (`app.css:164`, used at `Reflect:145`), `topics/TopicsIndex:214`, `incoming/SplitReview:294` (CSS :435), `settings/Settings:613` and `Row:81/93/128/161`, `todos/TodoForm:69` (`.switch`, keys ⌘1/⌘2), and `assistant/SourcesPanel:202 .tabs` (Kept/Dropped, missed by the list).
   - **`key` must be optional:** Settings' controls sit under the modal key floor, and Reflect gives each option its own key today.
5. **`lib/ui/Switch.svelte`**: the on/off toggle written twice in Settings. It isn't a cycler.
6. **`lib/ui/FilterField.svelte`**, with `{value = $bindable, placeholder, icon, el = $bindable}`: an icon plus a bare input in an `.input` box. **Done (2026-10-05):** props are `value`, `el`, `placeholder`, `icon` (a Phosphor name), `sm`, `field` (an action, the pickers' `p.field`), `class` and a trailing `children` snippet, and any `<input>` attribute (`onkeydown`, `oninput`, `onblur`…) passes through.
   - **Takes nine copies:** `Todos:298`, `agents/Tree:222`, `topics/TopicsIndex:309`, `settings/Settings:771 .find`, `shell/TabBar:105`, `PersonPicker:154`, `SearchPalette:297`, `SlackReview:147` and `TopicPicker:134`.
   - **Needs `.input:focus-within`**, which bundle.css lacks; nearly every field is a wrapper.
7. **People** (decision 2): `lib/ui/Avatar.svelte` with `{name, color, size: 'lg' | 'sm'}`, for person rows only (never in threads), a `PersonName` `variant: 'thread' | 'tertiary'` (both coloured), and later a `PersonRow`.
   - **Avatars:** `Topics:530` (26px, markup :233), `PersonPicker:191` (22px → 18) and `assistant/GleanAnswer:119` (15px → 18).
   - **Names:** `Topics:236/543` rebuilds the unknown style, `Incoming:771/1211 .np-name` colours by hand, `Incoming:1144` makes the tertiary marker through `:global`, and `GleanAnswer:39–62` is a person chip.
8. **CountdownPill split.** The look (`shell/CountdownPill:27`, 14px radius) goes to `lib/ui`; the `live('daystrip.get')` wrapper stays in `shell/`.
9. **Settings' Stepper** moves to `lib/ui` and sits on `.input` plus icon buttons.
10. **Later, if the class layer isn't enough:** a thin `lib/ui/Row.svelte` with `{cursor, open, checked}` that also does scroll-into-view and pointer sync.
    - **Keyboard handling stays in each view.** `lib/keys.svelte.ts` only gives a handler stack. Each view keeps its own cursor and repeats `scrollIntoView` through `querySelector` (Todos, Incoming, Search, TopicsIndex, Chronology, SearchPalette, SplitReview, Priorities). `settings/Row:46` already scrolls itself.
    - **About 28 row implementations in 24 files.** Migrate the pickers first, then SlackReview, SourcesPanel, CarryList and Rollup.
    - **Hard ones:** Incoming (three nested cursors), the Agents Tree, the Todos grid, Settings, the TopicsIndex table and RowView.

## Phase 3 · Visible changes, by decision

**1 · Headings** → `.page-title` (24px) and `.pane-title` (19px). Only three sizes change:
- `Priorities:311` 20px → 24;
- `settings/Settings:870` h2 18px → 19;
- `agents/Keys:112` 17px → 19.

These already match:
- `agents/Pane:301` (19px);
- `Topics:363` `.name` and `:380` `.name-input` (24px). `.name-input` must keep the page-title size.

**2 · People** (corrected by the user, 2026-10-04). A name is always in the person's colour. The design system first said thread names go plain with an added avatar; the user rejected that, and the design system now matches.
- **Thread names don't change:** they keep their colour, at weight 500, with no avatar.
  - Incoming's expanded messages and replies: `Incoming:720/743/757`;
  - the split review: `SplitReview:328/345`;
  - SignalDetail, which Todos, Search and the Chronology use: `SignalDetail:31/38/46`.
- **Meta-line names** stay coloured, at regular weight (`.person-tertiary`). Today that's built through `:global` at `Incoming:583/1144`; it becomes a PersonName `variant="tertiary"`.
- **Avatars only lead a row about a person:** the Topics People panel, the person picker and Glean's people.

**3 · Selection** → `.row.cursor` (aqua-tint, no edge), `.check` and `.open` (bg-head). Every edge goes. There is no `border-left` anywhere; all of them are `box-shadow: inset`.
- **Done (2026-10-05):** every site below is a fill now (`aqua-tint` cursor, `bg-head` open row, a check for picked rows) and no cursor has an edge. Left alone: Priorities' `.selected` bar, the Chronology and Search `.review` boxes, Panel's `.accent`, the tab underline, chip rings, and Glean's `--rc`/orange marks (`Search .row.lit`, `SearchPalette .row.sel`, `SourcesPanel .row.sel`). Decision 13's `.msg.moved` lost its edge here and kept its group tint. A signal whose messages have the keyboard (split review, or ↵ into its messages) is the open row (`bg-head`), so the message cursor shows on it.
- **Right edge:**
  - global `.sel` (`app.css:182`), which also gives a right edge to `Priorities:178/194`, `reflect/CarryList:46` and `TopicsIndex:178`;
  - `Incoming:884 .topic.on` and `TopicPicker:158 .opt.on`;
  - `Todos:386 .todo.sel`. Keep its topic edge at :381; it isn't a selection.
- **Left edge:**
  - Settings: `settings/Row:214`, `Settings:982` and `:1147`;
  - Incoming: `Incoming:1125 .msg.focus` (edge only) and `:976 .sig.sel`, `PersonPicker:187`, `SplitReview:490`;
  - Notes and Topics: `notes/Picker:131`, `topics/Chronology:206`;
  - Search: `Search:281`;
  - Agents: `agents/Tree:469` (grey fill, wrong), `agents/RowView:291` (→ `.c-row.cursor`) and `agents/Pane:495`.
- **`.open`:** `agents/Tree:474 .tree:not(.focus) .sel` is exactly this state.
- **Wrong fill strength:**
  - `Notes:522` (5%);
  - `Topics:460 .up-row.today` (7% → `aqua-tint-soft`; not a cursor);
  - `PersonPicker:188` (12%).
- **Not row cursors:**
  - `Priorities:481 .selected` is the summary bar;
  - `Chronology:270 .review` and `Search:346 .review` are inline review boxes (tint-soft plus a 3px edge). The list says nothing about them.
- **Already right:** `SearchPalette:369`, `FontPick:134`, `SlackReview:185` and `CategoryPicker:84`.
- **Make it `.check`:** `Incoming:1128 .msg.chosen` (set at :736/755).
- **Decision 13, not 3:** `SplitReview:494 .msg.moved`.

**4 · Counts and chips** → `.count` / `.count.on` and `.chip` (fully round).
- **Counts:**
  - `shell/TabBar:88 .badge` (8px) and `shell/SyncStatus:305 .state`;
  - `Incoming:910` (active `.on .count` at :917);
  - `TopicsIndex:447`, which is always aqua and says "N new" → neutral.
- **Aqua "new" pills going grey:** `Incoming:1163 .pill.split` and `SplitReview:416`.
- **Chips:**
  - already close: `lib/ui/FacetBar:84`;
  - copies FacetBar's chip: `SearchPalette:305`;
  - 5px radius: `agents/Pane:377` (plus `.c-*` 389–413), `agents/form.css:97/112` and `Priorities:455`;
  - other radii: `TopicsIndex:416/424` (10px with an inset ring), `Topics:503/511` (9px) and `Todos:321/327` (`--r-input`);
  - `Todos:425 .status`.
- **Done (2026-10-05):** `--radius-full: 999px`, `.pill` (now round), `.count`, `.count.on` and `.chip`, `.chip.on` are in `app.css`; every site above uses them. Counts are grey and aqua only on the open row or tab (TabBar's active tab, Incoming's open topic); TopicsIndex's "N new" and both `split` pills are plain grey `.count`s. Chips keep their marks (category squares, Pane's `c-*` fills, FacetBar's `--rc` and `.hot`/`.on`/`.off`, the palette anchor's yellow on-state), and Priorities' selected chip is `.on`. Todos' Jira `.status` is a `.chip` that keeps `display: inline-block` so it can truncate. Left alone: Panel's, Priorities' header, Settings' nav and Search's counts, and the sync chip, Assistant source chips and Settings' list/value chips are not in the list (their local class names were renamed so the globals don't reach them).
- **User, 2026-10-05:** Priorities' compact chips keep the `.chip.on` ring as their cursor for now: Priorities as a whole hasn't been designed yet (ISSUES.md), so its chips aren't worth settling.

**5 · Popovers, modals, scrims, insets.** These are the Phase 2 Modal and Popover. Inset blocks → `.inset` (6px radius, `--border`):
- `agents/Pane:315 .facts` (7px) and `agents/Joins:38` (7px);
- `SplitReview:558 .legend`;
- `reflect/DayBrief:132 .tile` (bg-head border);
- `settings/AgentsPreview:39` (8px).
- **Done (2026-10-05):** `.popover`, `.popover.flush`, `.scrim`, `.modal` and `.inset` are in `app.css` (each keeps its own radius, `.inset` is 6px); every popover, picker, menu and modal box takes one, and `Modal` lost its `scrim` and `z` props (one `--scrim`, one z-index of 60), keeping `position`, `inset` (what the scrim covers: Keys the Agents pane, New agent below the title bar) and `isolate` (behaviour). Insets are the five above plus `.command-block` (approval cards, Incoming) and New agent's Scion instructions block; left alone: the Agents thread `.panel`, TodoForm's `.form`, and the Assistant Sources side sheet (a docked panel, not a popover). The Nudge toast now has the class's 10px 12px padding (14px 16px before).

**6, 17, 19 · Buttons** → `.btn` (3px 10px), `.btn.md` (5px 12px), `.btn.primary` (now **filled** `aqua-fill`) and `.quiet`.
- **Done (2026-10-05):** `.btn`, `.btn.md`, filled `.btn.primary`, `.btn.on` and the disabled look are in `app.css`; the sites below use them. Confirm and Move to topic are plain `.btn` (neither had a key hint). Opacity-disabled is gone from `form.css`, AskCard, Notes and StatusLine. Left alone: AskCard `.opt` and `.choice` (choice cards), Joins `.new` (a span, not a button), FontPick `.current` (a field), and `.quiet`, not added because four views already use that name locally. Assistant Sources takes `.btn.on` but no scene reaches it (it needs a Glean answer).
- **User, 2026-10-05:** text-only actions are buttons too, so there is no `.quiet`: Incoming's Dismiss, Edit and Discard (held todo), TodoForm's Cancel and Clear, and ApprovalCard's Copy are `.btn` (the design system's Button README is wrong on this). Buttons that create are filled: Todos' New and the empty Agents pane's New agent. SyncStatus's Connect repeats per source, so it's plain. The carry list's taller rows are fine for now.
- **Local `.btn` that differs** (riskiest): `agents/Composer:311` and `agents/Pane:245` (7px 12px, 7px radius).
- **Other button rules:**
  - `agents/RowView:498 .acts button` and `incoming/AgentPart:255 .ans`;
  - `reflect/CarryList:131 .b`;
  - the NeedCard `.b` (AskCard 205/217, ApprovalCard 185/200, form.css 211/221);
  - `agents/AskCard:250 .opt`, `agents/Joins:112 .new` (dashed) and `FontPick:90 .current` (→ `.input`).
- **Padding tweaks:** `Notes:656` and `reflect/Nudge:143`. **Gap tweaks:** `Incoming:1042` and `SplitReview:594`.
- **Disabled is `tx-disabled` with no fill, never opacity.** Opacity today:
  - `agents/form.css:227` (.6) and `AskCard:223` (.5);
  - `Notes:659 .btn.off` (.45);
  - `StatusLine:88` (.6).
  - `settings/Row:326` is an input, not a button.
- **Repeated on every row, so plain `.btn` with no key:** Confirm (`Incoming:676`). The same rule also hits **Move to topic** (`Incoming:671`), which the list missed.
- **Toggled-on buttons with no target in the system:** `TopicsIndex:381 .cats-btn.on` and `AssistantPane:320 .srcbtn.on` (orange).

**7 · Inputs** → `.input` / `.input.sm`: bg-app, border-strong at rest, aqua only on focus, placeholder in `tx-disabled`.
- **Already right:** `settings/Row:283`.
- **Wrong border or background:**
  - `settings/MapEdit:97` and `Settings:1115` (4px radius);
  - `agents/form.css:54/186`;
  - `AskCard:263` (bg-panel and `--border`);
  - `todos/TodoForm:144` (the list cites :110, which is the form);
  - `topics/CategoryManager:79`;
  - `AssistantPane:368` (8px radius).
- **Aqua at rest:**
  - `settings/ListEdit:102` and `Settings:1041` (only `.secret-input` after :1050);
  - `ApprovalCard:220` and `AgentPart:302`;
  - `PersonPicker:154`, `TopicsIndex:385` and `Todos:465`;
  - `DayStrip:501 .buf` (also 4px).
- **Missed by the list:**
  - `Reflect:302/325` (aqua `:focus-within` ring);
  - `notes/Picker:95/103`, `CategoryPicker:59` and `FontPick:116`;
  - `settings/Stepper:102/107`;
  - `StatusLine:79` (a select).
- **Probably exempt:** the borderless editors in `Composer:283`, `Notes:563`, `reflect/NoteEditor:316` and `topics/NotePane:308`.
- **Done (2026-10-05):** `.input` / `.input.sm` are in `app.css` (`:focus-within` for wrappers) and `lib/ui/FilterField` replaces the filter boxes (Todos, Topics index and its new-topic name, Settings' find, the agents tree, ⌘K, Slack review, the topic and person pickers); the other fields take the class or lose their own copy of it. No field is aqua at rest. Left alone: the borderless editors (Composer, Notes, NoteEditor, NotePane), Topics' heading rename, `FontPick .current` (a button that already looks like a field), the tab bar's Search (a button that opens ⌘K: the look, not the class, or it would be aqua when the palette hands back the focus), and Reflect's ask box (`.input` on the whole box, its two-part placeholder isn't a FilterField's). Heights changed (px, before → after): Topics' filter 34 → 28 (it lines up with `.btn.md` now) and its new-name field 34 → 28, Settings' find 32 → 28, the agent forms' fields 25–27 → 27–29, `TodoForm` 29 → 24 and its date 31 → 26, `CategoryManager`'s names 26 → 21, the assistant's ask box 40 → 32, the ⌘K field 35 → 33, Todos' filter 31 → 32. The tree's filter keeps its 25px (it sits by the 26px `+`).
- **User, 2026-10-05:** the ⌘K palette keeps its bare line, not a FilterField box: a field that is always focused would always be aqua, and the user prefers the old look. It's the one exception.

**8 · Labels.**
- **Odd tracking** → `.label`:
  - 0.08em: `agents/Joins:47`, `agents/Keys:136`, `agents/Tree:379`, `agents/form.css:46` and `settings/SlackReview:162`;
  - 0.06em: `agents/Tree:300` and `agents/Panels:80/204`;
  - 0.02em: `reflect/NoteEditor:346`.
- **Labels in aqua that aren't a state:** `Priorities:498` and `Chronology:276`.
- **Panel headers outside Panel** → `.panel-head` in `tx-2`:
  - `Priorities:353` (`tx-muted`);
  - `Todos:365` (`tx-label`);
  - `Incoming:951 .group` (`tx-muted-2`).
- **Done (2026-10-05):** `.label` (0.1em, `tx-label`), `.panel-head` (0.08em, `tx-2`) and `.label .n` / `.panel-head .n` (a bare count after the words) are in `app.css`, and every local copy is gone: the odd-tracking labels (agent form `.lab`, Joins, Keys, Tree's project and Needs you, Panels' Parsed/Explained heads and "If planted", Slack review's column header), the aqua Priorities "Selected" and Chronology "Now", and the headers Panel (its toggle), Priorities' quadrants, Todos' column head and Incoming's source groups now use `.panel-head`. Orange stays on the two state labels (Tree's "Needs you", Panels' "If planted"). Left alone: `NoteEditor .h` (an orange mixed-case heading in note text, not a label), Settings' `.glabel`/`.thead` (already `.label`) and Panel's own count (it sits beside the header, outside `.panel-head`).

**9 · Key hints**, boxed everywhere → `.kbd`.
- **The one-line change:** the global `kbd` at `app.css:187` boxes 32 `<kbd>` uses: Priorities ×8, Incoming ×7, NotePane ×5, SplitReview ×4, and others.
- **Unboxed hint spans:**
  - Settings: `settings/Settings:790` and `settings/Row:230/353`;
  - Todos: `Todos:334` and `TodoForm:196`;
  - Incoming: `SplitReview:452`;
  - Topics: `CategoryManager:87` and `TopicsIndex:404`;
  - Notes: `notes/Picker:149`;
  - Agents: `ApprovalCard:206` (plus `.primary .k` :211 at .75 opacity), `AskCard:226`, `Composer:305` (named `.kbd` but unboxed) and `:335`, `Pane:254`, `RowView:342/350/508/516`, `AgentPart:288` and `form.css:230/234`.
- **Keys written as plain text** (these need markup changes):
  - `Todos:272/482`, `Incoming:823/1257`, `TopicsIndex:273/519` and `Search:207/355/366`;
  - `SignalDetail:55/105`;
  - `TabBar:38/126`, `Tree:97/250`, `TopicPicker:89/143`, `SourcesPanel:104/225` and `AssistantPane:162/323`.
- **Done (2026-10-05):** every key hint is one `<kbd>` styled by the global `.kbd, kbd` in `app.css` (mono, `bg-head`, `border-strong`, 3px radius because the old global `kbd` had none; aqua-line in `.btn.primary`, grey in disabled), all local hint rules are gone, modifiers read through `chord('⌘K', 'Ctrl+K')` (`lib/keys.svelte.ts`), and the plain-text keys in legends and sentences are boxed one key each (`j/k` is two boxes, `1–7` one); inside `.btn`, `.chip` and `.input` the box takes back its extra 2px of height so those don't grow, and left alone are the Notes legend's typed prefixes (`#`, `~`, `- [] ()`), `Settings/Row`'s `.key` (a settings.json name) and tooltips (`title=`), which can't hold markup. Wording that changed: the Tab bar's hint is `Ctrl+K` off the Mac (it said `/`, which doesn't work on every tab), Notes' send button says `⌘↵` on the Mac (it said `⌃↵`; `mod()` is ⌘ there), and the other `⌘…` hints read `Ctrl+…` off the Mac.

**11 · Colour roles.**
- **Done (2026-10-05):** `urgencyColor()` has three steps on the `--time-*` tokens (green above 15 min, yellow 6–15, red at 5 or less; unit test in `lib/urgency.test.ts`), `meetingPill()` keeps its 30-minute ladder, and `.err` is one global class in `app.css` (orange `status-attention`; `.err.broken` red `status-broken` for a source or tool that can't be reached: the agent host line, a failing source in the sync popover and the assistant's failed ask, the rest stay orange); local copies lost their colour and `.hub-err` uses `color-mix` on `status-broken`. Status uses of `--orange|red|yellow|green` in the files touched became `status-*` / `time-now` (same values). Left alone: categories, Agents' own colours (`c-*`, `--need`, `--msg`, `--tool`, `mono-tag` verdicts), the done checkbox's topic colour, Priorities' reason pills and legend, Notes' kinds, Glean's orange marks, diff +/- greens, the day strip's overbooked bands, and `.fatal`. Not visible at 10:52 (next block is 7 minutes away): the countdown's red and the sources/host/hub reds.
- **User, 2026-10-05:** the assistant's failed ask (e.g. Bedrock rejecting the credentials) is red, and its icon is `ph-warning-circle` (a ❗), not `ph-warning` (⚠): ⚠ goes with orange "needs you", ❗ with red "broken".
- **Time pressure, `urgencyColor()` (`lib/app.svelte.ts:158`):** today it's four steps, with orange at 2–5 and red under 2. The system says three steps, with red from 5 minutes. **That turns the countdown red three minutes earlier.** Used at `CountdownPill:20/22` and `DayStrip:29`.
- **`meetingPill()` (`:167`):** red within 30 minutes, which is a different ladder. The design system's author wasn't sure it belongs; leave it until decided.
- **`.err` → one global class.** Orange (`status-attention`) when it needs you; red (`status-broken`) when a source or tool can't be reached. All of these are orange today:
  - the list's five: `Incoming:1246`, `Search:369`, `TopicsIndex:534`, `Topics:407` and `CategoryManager:91`;
  - twelve more it missed: `Todos:338`, `Reflect:258/265`, `form.css:200`, `Pane:512`, `AgentPart:316`, `PersonPicker:241`, `SplitReview:599`, `Chronology:296`, `SyncStatus:286`, `AssistantPane:296` and `Tree:592`.
- **Red "broken" with raw alphas:** `agents/Tree:557 .hub-err`.
- **Use the role tokens, not raw colours.** The status uses of `var(--orange|red|yellow|green)` (orange ×58, red ×19, yellow ×20, green ×19) move to `status-*` / `time-*` as each file is touched. Categories stay as they are.

**12 · Radii and tints.**
- **4px → 2px is visible.** `--r-block` has 17 uses, plus 17 literal `4px`. These change: key caps, app icons, Markdown code, the FacetBar source tiles, and the picker and Settings inputs.
- **Radii going to `radius-md` (6px):** `--r-panel` 8px (36 uses), and the literals 5px ×11, 7px ×10 (all Agents), 8px, 10px ×7, 12px and 14px.
- **Pills and chips → `radius-full`:** 9px ×8 plus the chips.
- **Leave alone:** 50% ×19 (dots) and the 2px/3px marks.
- **Aqua alphas with a named tint:**
  - .14 ×7 → `aqua-fill` (Phase 1);
  - .2 at `Incoming:918`, `TopicsIndex:452` and `TabBar:96` → `aqua-badge`;
  - .45 at `Priorities:489` → `aqua-line`;
  - .1 at `SplitReview:491` → `aqua-tint`.
- **Aqua alphas with no tint**, so pick the nearest or add one:
  - .5 ×5: `form.css:5`, `Tree:475`, `TopicsIndex:427`, `Chronology:294` and `Topics:515`;
  - .18 ×4: `SplitReview:424`, `Incoming:1172`, `TopicsIndex:425` and `Topics:513`;
  - .25 ×2 at `Notes:577` and `NotePane:320`, both text selection;
  - .55 at `NotePane:244`, .12 at `PersonPicker:188`, .08 at `Priorities:488` and .05 at `Notes:522`.
- **Warm-white washes:** .03 ×9 → `row-hover`; .05 ×5 → `hover-tint` (including `app.css:133`).
- **Done, radii (2026-10-05):** `app.css` has `--radius-sm` (2px), `--radius-md` (6px) and `--radius-full`; `--r-block`, `--r-input` and `--r-panel` are gone and no `border-radius` in `app/src` is a px literal except the marks below.
  - **`radius-md`:** every `--r-input` and `--r-panel` (panels, popovers, modals, the palette, cards, buttons, inputs), every literal 5/6/7/8/10/12px (Agents' boxes, buttons, tabs and thread rows, the Settings and New agent modals, the Keys sheet, the assistant's question bubble), and the four-pixel inputs and buttons (Settings' price fields, MapEdit, the category colour input, the `.think` and `.buf` fields, Segmented's inner thumb). The `--r-block` that sat on a button or a box (the carets in Notes and Reflect, Notes' Back, Priorities' eye, Markdown's `pre` and NoteText's `.code` block) is `md` too. Partial radii keep their shape (`0 md md 0` on the Incoming topic rows and Search rows).
  - **`radius-sm`:** the global `kbd` (was 3px), `.mono-tag`, the Agents `.skill` and host tags, AppIcon's tile and image, FacetBar's source tiles, Markdown's `code`, NoteText's inline code, Glean's citation tag, the day strip's `.block` and `+N` badge, the agent form's checkbox, and the row highlights (Notes' `.row`, the Notes picker's options, the Incoming and split-review message rows, the `0 2px 2px 0` selection bars at Incoming, the topic picker and the Topics index).
  - **`radius-full`:** `CountdownPill` (14px), `Switch` (10px) and Glean's `.person` (999px).
  - **Left as literals (small marks):** the 2px bars, swatches and ticks (Priorities, Rollup, DayBrief's blocks and meter, TodoForm and Todos' category bars, the split review's bar and swatch, the Notes picker, Margin, Topics index and category picker swatches, FacetBar's `.sq`, Search's `mark` and bar, the palette's swatch, the day strip's `.bar`), the 3px marks (Priorities' selected swatch, the assistant's swatch, the status line's bar), the 1px lines, and every `50%` dot.
  - **Visible:** panels, popovers, modals and the agent boxes go from 8/10/12px to 6px; key caps 3 → 2px; app icons, FacetBar tiles, Markdown code and the message rows 4 → 2px; the pickers' options 4 → 2px; Segmented's thumb and the 4px inputs 4 → 6px; the day strip's event blocks 4 → 2px, stacked ones (`laneStyle`, which set 3px) too; the Switch and the countdown pill are round (they were 10px and 14px on a 20px and 28px box, so they were round already).
  - **User, 2026-10-05:** Segmented's thumb (6px inside a 6px frame) stays as is: the segmented control is due to be redone, and its corners go with that.
- **Done, tints (2026-10-05):** `Chronology`'s fade gradient (.5) and `NotePane`'s editing border (.55) are `aqua-line` (.45); the warm-white .04 hovers are `row-hover` (.03) at Incoming's `.topic:hover`, Settings' nav `.sec`/`.sub` hover and Glean's `.prow.hot`, all list rows (no button or control used it, so `hover-tint` took none). Left alone: the text-selection .25 (Notes, NotePane), Priorities' `.selected` .08 (Priorities hasn't been designed, ISSUES.md) and Markdown's code .07 (it follows Markdown, not a wash). Visible: the Chronology fade and the NotePane edit border are a shade fainter, and the three row hovers are a shade fainter.

**16 · Text links.** The look stays. Converge on `.link-act` (aqua), `.link-toggle`, `.link-nav` (grey →) and `.link-out` (blue ↗). Sites:
- **Act here:** `reflect/DayBrief:191`, `agents/Tree:554`.
- **Go elsewhere in the app:** `AskCard:145`.
- **Ghost buttons:** `Incoming:1047 .ghost`, `TodoForm:193 .ghost`.
- **Show/hide:** `Incoming:1102 .more`.
- **Hover-underline links:** `Todos:418`, `Pane:340`.
- **Leave the app:** `lib/ui/Markdown:149 a`.
- **Unsure, not settled:** `lib/ui/NoteText:142 .link`, which is green.

**18 · Empty states.** No visual change. `.empty-pane` (icon, sentence, button) is for a whole pane or list; `.empty-row` is for an empty panel.
- **`.empty` rules:**
  - views: `Priorities:441`, `Todos:478` (the odd one), `Incoming:1253`, `Search:351` and `Topics:492`;
  - topics: `TopicsIndex:515` and `NotePane:322`;
  - other: `CarryList:158`, `agents/Tree:548`, `Settings:1163`, `SlackReview:203`, `SourcesPanel:274` and `SearchPalette:400`.
- **`.none` rules:** `TopicPicker:185`, `CategoryPicker:70` and `FontPick:137`.
- **Empty states written as `.muted`:** `DayBrief:79/92` and `SyncStatus:176`.
- **The only centred hint box today:** `agents/Pane:230 .hello`. The list cites `Pane:221/227`, which is a layout wrapper. `App:111 .fatal` would also take it.
- **Wrongly using `.empty` for loading text:** `Settings:485` and `SlackReview:83`.
- **Done (2026-10-05):** `.empty-pane` and `.empty-row` are in `app.css` as the design system has them, and every site below takes one; every scene is pixel-identical. Each site keeps what differs as a local override, so the user can take them off one at a time. Every site was left-aligned with the colour `tx-label`, so what is shared is the colour; `font: inherit` puts back the size and line height the site inherited (13px, `normal`; the design system's are 12px and 20px).
  - **`.empty-pane`** (local: `display: block`, `text-align: left`, `font: inherit`, own padding): Todos (`24px 16px`, and `tx-muted-2`, the odd one), Incoming (`16px`), Search and Topics index (`16px 18px`), the Agents tree (`18px 14px`, `0.923rem` and a line height of 1.5, which it keeps), the Assistant's Sources list (`12px 14px`). Agents' `.hello` is the one hint that was centred: it takes the class and keeps `margin: auto`, `gap: 10px` (8px), no padding (16px) and `font: inherit` (13px, not 12px).
  - **`.empty-row`** (local: own padding, `font: inherit`): Priorities and CarryList (`10px 12px`), Topics' Open panel (`8px 12px`), Settings' find results (`14px 18px`), the palette (`10px 14px` and a top border, shared with its `.note`), the three pickers (topic `6px 10px`, which already had the 12px and only sets the line height; category `5px 12px`; font `8px 10px`), DayBrief's two folds, the sync popover's "No live sources yet" and the topic note's "Empty note." (no padding; the first two inherit 12px and 12.5px, the note 13px with its 1.75 line height).
  - **Loading text** is not an empty state: Settings' "Loading…" and Slack review's "Asking Slack…" and error line are `.loading` (and `.warn`) with the same look, local to each.
  - **Left alone:** `App .fatal` (an error, and the pane would need every property overridden), `Pane .empty` (a layout wrapper, not a hint), the `.none` value placeholders ("No topic", "no note", "no entry", `Pane .fv.none`), `DayStrip .note.empty`, `SyncStatus .muted.connect` (a message, not an empty state), and the hints written with other names: Priorities' `.sel-meta`, agents `Pane .quiet`, `Panels .benign`, `AgentPart`'s `.why` and `.note`.
  - **Differences kept, for the user to decide:** padding runs from 0 to `24px 16px` (the design system: `2px 12px` for a row, `16px` for a pane); text is 13px at most sites, 12px in the topic picker, DayBrief and Agents tree, 12.5px in the sync popover; line height is `normal` except Todos (1.5), the Agents tree (1.5) and the topic note (1.75); only Todos is `tx-muted-2`; no pane is centred or has an icon but Agents' `.hello`; `.hello`'s text now centres where it wraps (it was left-aligned there); the `+` in its New agent button still takes `.hello i`'s 26px and grey (`tx-disabled`), which looks accidental.
  - **Scenes:** new `empty-*` scenes in `ui-diff.mjs` reach Search, the palette, the Agents tree, Settings' find, the category and font pickers, Topics index (Archived), a topic page (Nothing open and Empty note) and a day with no calendar, decisions or carry rows; `agents`, `todos` and `sync` already reach the rest. Not covered: Incoming and Priorities (the sample data fills them), the topic picker (typing a name offers Create), Slack review, the Sources list, Settings' Loading… and `.fatal`. They share their rules' shape with covered sites (same overrides on the same class), so the cascade is the same; the topic picker's font size is the design system's, equal to what it had.

**20 · Glean citations** → `.cite`, `.claim`, `.query`, `.source`, with `--qc` set to the hovered query's `markColor()`.
- **Tag colour:** `GleanAnswer:87 .cite` is grey and should be blue at rest. `:102 .cite.lit` is fixed orange.
- **Highlight:** `GleanAnswer:82 .sentence.under` is an orange underline and becomes a highlighter in the query's colour. `lit` (`GleanAnswer:20`) is a `Set<number>` and must carry the colour. The hover state is at `AssistantPane:40–41`.
- **Sources panel:**
  - `SourcesPanel:250 .num` has no tag;
  - `:243 .row.sel` is fixed orange;
  - `:247` only dims the rest;
  - `:183 .query.on` is orange.
- **Query colours:** `MARK_COLORS` (`shell/assistant/types.ts:53`) → `query-1…6`.
- **Do this before changing the local query's mark to a grey ◆.** The front end finds the local query by `mark === '◇'` (`SourcesPanel:36/37/95`, `types.ts:55`). `◆` is already Glean's mark 4 (`src-tauri/src/glean/ask.rs:139`), and `ask.rs:229` pushes `◇`, so switching the mark alone would mistake Glean query 4 for the local one. Add an explicit `local` field on `QueryRun` first.
- **Done (step one, 2026-10-05):** `QueryRun` has `local: bool` (Rust, serialised as `local`; set only where the local query is pushed; the type is serialise-only, so nothing stored lacks it) and `local: boolean` in `types.ts`. `markColor`, `SourcesPanel`'s Glean/local counts and its `L`/`Q` label, and `AssistantPane`'s "N queries" count, test `q.local`, not the mark or the run's position. No mark or colour changed; the mark change (grey `◆`) and the rest of 20 are still to do.

## Decided by the user (2026-10-04)

**13 · Done and dismissed, partly.**
- **Done (2026-10-05):** done todos (Todos only; no other view shows one) show `ph-fill ph-check-square` and fade to `opacity: 0.45` (CarryList's done fade) with no strikethrough; one `--hatch` token in `app.css` now serves `Incoming .sig.gone`, `TopicsIndex .row.merged` and the new `SplitReview .msg.gone` hatching; the day strip's gap stripes (`gaps`, `.gap`) are gone. Left alone: `Topics .name.merged` (a heading, no hatching today), `Notes .row.life`/`.row.done` and the DayStrip `.block.life` hatch (different meanings), CarryList's `.done` (a carry action, not a done todo).
- **User, 2026-10-05:** the done checkbox stays in the topic's colour ("a nice touch"). The hatch was too faint to notice at 0.025; it's 0.06 now. Life blocks (DayStrip, Notes) keep their own hatch: that feature needs rethinking as a whole.
- **Done todos fade out** and show a filled checkbox. Today `Todos:221–223, 396, 408` show an outline `ph-check-square` in aqua and strike the title in `tx-label`. Assumed: the fade replaces the strikethrough.
- **Dismissed, archived and trashed signals look the same.** For Gmail, Dismiss archives the thread, so it reads "Archived"; Trash (`#`) is Gmail-only and moves the thread to Gmail's Trash. Their row style (`Incoming:1057 .sig.gone`) stays one style.
- **Dismissed things are hatched, faded and struck through, everywhere.** That covers dismissed signals and dismissed messages in the split review. `SplitReview:515–520 .msg.gone` strikes and fades today but has no hatching.
- **A merged topic keeps its strikethrough and hatching** (`TopicsIndex:468`, `Topics:395`).
- **Hatching means one thing:** "this is being moved, or is about to be irrelevant here".
- **The day strip's hatched gaps go** (`DayStrip:33–44, 163–165, 386–392`). They render as faint diagonal stripes between events and add no information (user, 2026-10-04).
- **The design system's split-review card is wrong.** There's no crossed-out "moved to another topic" message.
  - **What the app actually has:**
    - `.msg.gone` is a *dismissed* message;
    - `.msg.moved` (`:494`, "moved from X · u undo") is a message moved *into* this group, tinted in the group's colour with an aqua edge.
  - **`.msg.moved` belongs with decision 3.** It's a selection edge, so it loses the edge.
- **Trash discards a held todo, like Dismiss.** Fixed in `queries/incoming.rs` `trash_seen`, with a test.

**15 · Dividers, for now.** The `--border` lines between rows (Settings, the Glean panels, the group rules) are row markers and stay. Revisit lines and outlines after the first passes.

**19 · Move to topic is plain.** In Unsorted, the main button on every signal row is "Move to topic" (`Incoming:671`, `btn primary` today); `t` does the same on any signal. Like Confirm, it becomes a plain `.btn`.

**A toggled-on button takes the selected look:** the `aqua-tint` fill with an `aqua` border, the same as `.chip.on`. That means a new `.btn.on` class.
- **Topics index "Categories"** (`TopicsIndex:218, 381`) is aqua text and border today.
- **Assistant "Sources"** (`AssistantPane:161, 320`) is an orange border today, and orange means "needs you".

**14 · Row density, assigned.** Today's vertical padding is in brackets. Rows that wrap or hold two lines (a title over a meta or description line) keep the same padding and grow with their content. Line wrapping is fine everywhere.
- **Comfortable, 9px (38px one-line):**
  - Incoming signals `Incoming:980` (9) and its topic list `:871` (8);
  - Settings `Settings:968` / `Row:202` (9, min-height 48/52 today);
  - Todos `Todos:357` (7);
  - the Topics index `TopicsIndex:353` (9);
  - Priorities quadrant rows `Priorities:386` (9, two lines);
  - a topic page's Upcoming `Topics:453` (9) and People `:521` (9);
  - Search results `Search:272` (10, two lines). This is a suggestion, as the closest match to today, like Priorities; the user said Comfortable or Cozy.
- **Cozy, 5px (30px):**
  - a topic page's Chronology `Chronology:195` (7);
  - Reflect's Days/Weeks/Months rollup `Rollup:67` (10);
  - Glean sources `SourcesPanel:233` (8);
  - Notes lines `Notes:516` (5).
- **Compact, 2px (24px):**
  - a topic page's Open `Topics:480` (8);
  - Reflect's carry list, which is "Carry forward" on a day and "Still open from the week" on a week (`CarryList:101`, 8). It wraps;
  - Glean queries `SourcesPanel:174` (4);
  - the Agents tree `Tree:394/450` (4);
  - the pickers: TopicPicker `:147` (6), PersonPicker `:178` (6), CategoryPicker `:74` (5), notes/Picker `:121` (5), FontPick `:127` (5);
  - ⌘K results `SearchPalette:354` (8);
  - AskCard options `:250` (4);
  - message rows, which keep their reading line height: Incoming `:1071` (4), the split review `:476` (3) and SignalDetail `:68` (2);
  - Settings › Slack › Review…, the channel table (`SlackReview:154`, 6). Assumed compact as a picker; the user had taken it for Incoming's message list.
- **Done (2026-10-05):** `app.css` has `--row-pad-comfortable` (9px), `--row-pad-cozy` (5px) and `--row-pad-compact` (2px), and every list above takes its density's vertical padding from its own row rule (horizontal padding, dividers and every other look unchanged); Settings' `min-height` 48/52 is gone so its rows follow the padding. Not covered by a scene: Glean sources and queries, Slack review, AskCard's form options and SignalDetail's messages.
- **User, 2026-10-05:** Compact at 20px looked squished; rows take the design's heights. Each row rule also sets `min-height: var(--row-min-…)` (38 / 30 / 24px), so a one-line row is the design's height even though the app's text line is 16px, not 20px. Chosen over `line-height: 20px` because that would also spread every line of rows that wrap or hold two lines; with `min-height` those keep growing with their content.

**Deferred**
- **Life blocks in the day strip** (`:211, 393`, hatched in the Life colour) are to be redone. They come only from Proton ICS busy times (`store/ingest.rs:217`) or the sample data, and Proton ICS has never run against a real account.
- **Notes line states** (confirmed, sent, failed, Life, `Notes:528, 583`) need a follow-up. Life lines also use a hatching.

## Open: needs the user

- **15 · Lines vs outlined rows,** after the first passes. Where lines between rows are today:
  - **`--border` lines:**
    - Settings rows `Settings:975`, `Row:208`, plus the table rows `:1073`, `:1100` and `:1142`;
    - Glean sources `SourcesPanel:240` and the Assistant's people rows `GleanAnswer:152`;
    - group rules in Incoming `:962`, the split review `:471` and Topics `:451`.
  - **`--bg-head` lines (fainter):** Priorities rows `:392`, Reflect Rollup `:74` and the carry list `:107`, and the Agents status lines `StatusLine:97`.
  - **Already outlined:** AskCard choices `:173/257`, the Agents transcript boxes (RowView).
- **18 · Empty states, make them one look?** They share `.empty-pane`/`.empty-row` but keep each site's padding, size, line height and colour (listed in 18's "Differences kept"). Settling them is a visible change.
- **The New agent button's `+`** in the empty Agents pane is 26px and grey because `.hello i` reaches it. Looks accidental; the fix is one selector and visible.
- **The segmented control** is to be redone (user); its thumb's 6px-in-6px corners go with that.

## Problems in the design system itself

- **Stale files.** `tokens.css` and the generated `api/tokens.md` are still version 1 (`sel-edge`, `r-block` 4px, `r-panel` 8px, `r-pill`, `r-modal`). Use `tokens.json` and `bundle.css`.
- **Retired components in the manifest.** `manifest.json` still lists Segmented and SelectedRow, and Button as "outlined, never filled".
- **Missing classes.** `bundle.css` has no `.hints`, `.tag` or `.err`, though Consolidation targets the first two. Its `.input` has no `:focus-within`.

## What this doesn't do

Phases 0 and 1 landed without decisions from the user and without visible change. Phase 2 changes behaviour (Escape, focus, outside click) and needs `scripts/ui-diff` plus manual key testing. Phase 3 changes the look and should go decision by decision.
