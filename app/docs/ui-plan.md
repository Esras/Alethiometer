# Front-end components: the design-system work list

The target is the **Alethiometer** design system artifact (https://claude.ai/artifact/WH4uwJJM3gk6Ux2HX7f565): its README (the rules), `tokens.json`, `components/bundle.css` and Consolidation (`merges.md`). That list was written against `main@6408967`. Everything below was re-checked against `main@7b1ab84` on 2026-10-04: line numbers are current, sites it missed are added, and items are grouped by the shared piece they become rather than by decision number.

Paths are under `app/src/`. "→" is the target class or component.

## The rule for what becomes what

- **A global class in `app.css`** when the thing is only a look: `.btn`, `.kbd`, `.count`, `.chip`, `.label`, `.panel-head`, `.input`, `.inset`, `.scrim`, `.row`, `.empty-pane`/`.empty-row`, `.hints`, `.tag`, `.err`, `.avatar`, `.page-title`/`.pane-title`. Most of these sit inline inside buttons and rows, where a wrapper component adds prop plumbing and makes `.btn.primary .kbd` harder to style.
- **A `lib/ui` component** when it carries behaviour: Modal, Popover, Picker, Cycler, Switch, FilterField, NeedCard, Avatar, PersonName variants, StatusMark, CountdownPill. Each one today is written several times with different Escape, outside-click, focus and key handling.
- **Size of the problem:** 7,918 lines inside `<style>` blocks (44% of all `.svelte` lines). Top files: Settings 486, Incoming 413, agents/Tree 402, agents/Pane 296, Priorities 287, Topics 282, agents/RowView 271, TopicsIndex 258, DayStrip 238, Notes 218. Today `lib/ui/Panel` has one user and `lib/ui/Pill` two.

## Phase 0 · Foundation (no visible change)

1. **Tokens into `app.css :root`.** Missing today: `aqua-fill`, `aqua-badge`, `aqua-line`, `hover-tint`, `row-hover`, `scrim`, `status-broken/attention/headsup/done`, `time-far/near/now`, `link-out`, `pill-red/yellow/green-fill`, `route-glean`, `thinking`, `result-failed`, `query-1`…`query-6`, `query-local`, `agent-*`, `space-2`…`space-16`, the shadows `popover`, `modal`, `tab-underline` and `panel-accent`, `row-comfortable/cozy/compact`, `avatar-lg/sm`, `btn-pad-sm/md` and `input-pad-sm/md`. The Agents box colours (`look.ts:40–57`) and `--cat-*` stay injected at runtime.
2. **Radius tokens.** Add `--radius-sm/md/full` alongside `--r-block/--r-input/--r-panel` at the old values first. The value change is decision 12 (Phase 3).
3. **Clear name collisions before adding the global classes.** Svelte's scoped rules would mix with a new global rule of the same name:
   - **Plain-number `.count`:** `Priorities:367`, `reflect/Rollup:62`, `reflect/CarryList:96`, `topics/TopicsIndex:329`, `Search:260`, `shell/AssistantPane:301` and `lib/ui/Panel:95`. Rename them (e.g. `.n`, as bundle.css does).
   - **Local `.modal` / `.popover`:** `settings/Settings:700`, `agents/NewModal:55` and `shell/DayStrip:461`.
   - **The `agents/form.css` leak.** It's imported as plain CSS by `NewLocal`/`NewScion`, so it's global. Its `.form`, `.form .foot`, `.form .k` and `.form .primary .k` already style `todos/TodoForm.svelte:68` (`class="form"`) by accident. Its `.input`, `.chip`, `.check` and `.b` clash with the targets. Scope it under `.agent-form` or fold it into the shared classes in Phase 1.
   - **Global `kbd`** (`app.css:187`, unboxed). Changing it boxes all 32 `<kbd>` uses at once, many inside `.btn.primary`. Treat it as decision 9 (Phase 3), not as a rename.
4. **A screenshot check.** `scripts/shot.mjs` already drives Playwright.
   - Run `ALETHIOMETER_FAKE_NOW=10:52 ALETHIOMETER_DATA_DIR=<tmp> npm run devserver` with `npm run dev`.
   - Shoot each tab plus the key sequences that open pickers, Settings and the agent forms, on `main` and on the branch.
   - Diff with ImageMagick `compare -metric AE`. Phase 1 should give a zero diff; mask StatusMark's running dot.
   - There are no component tests and no lint/stylelint today. `npm run check` (svelte-check) runs after each extraction.

## Phase 1 · Merges that shouldn't change the look

1. **Panel.** Reflect rebuilds it three times: `reflect/CarryList:75/82` (markup :39), `reflect/DayBrief:100/107` (:25) and `reflect/Rollup:41/48` (:18) → `<Panel collapsible={false}>`, which needs no new prop. Panel's own scoped surface becomes the global `.panel` so others can share it.
2. **Column panel surface** → `.panel` plus a `.panel.col` modifier (flex column, `flex: 1`, `min-height: 0`). Sites: `Todos:343`, `topics/NotePane:233`, `Incoming:935`, `Search:226`, `topics/TopicsIndex:290`, `Notes:451 .sheet`, `reflect/NoteEditor:258` (also a Panel-style header at :268), `settings/Settings:1128 .results` and `:939 .card`, and `Reflect:284 .ask`.
3. **NeedCard** → `lib/ui/NeedCard.svelte`, with `{icon, title, via?, link?, compact?}` and snippets `body`, `actions`, `right`; class `.c-need` / `.c-need-one`.
   - **AskCard sites:** `agents/AskCard` `.card` 119, `.one` 276, `.btns` 199, `.b.primary` 217 and `.link` 145.
   - **ApprovalCard sites:** `agents/ApprovalCard` `.card` 99, `.copy` 129, `.btns` 175, `.b.primary` 200 and `.one` 230.
   - **Not quite identical:** `.top` padding is 8/12/4 in Ask and 8/12/6 in Approval, and Approval adds `max-height: 55vh`. Pick one.
   - **`.said`** is also copied at `incoming/AgentPart:227`.
4. **Command block** → `.inset.command`. Sites: `agents/ApprovalCard:140` and `incoming/AgentPart:215`.
5. **Mono tag** → a new global `.tag`, which isn't in bundle.css yet. Sites: `agents/Panels:130` and `agents/Tree:354`.
6. **Footer hint bar** → a new global `.hints`, also missing from bundle.css.
   - Same rule: `Incoming:1257`, `Search:355` and `Todos:482`.
   - Same idea under other names: `Notes:626 .foot`, `shell/assistant/SourcesPanel:278`, `incoming/PersonPicker:229`, `notes/Picker:155`, `shared/SignalDetail:105` and `DayStrip:508 .pop-foot`.
7. **StatusMark** moves to `lib/ui/StatusMark.svelte`. It's already imported by Pane, Joins, Tree and Incoming. It needs an `offline`/`problem` status to absorb the copies:
   - `shell/SyncStatus:231` (plus `.dot.bad` :237);
   - `agents/StatusLine:148` (plus `.dot.off` :154);
   - `settings/Settings:837 .dot` and `:895 .state .d`.
8. **Colours written out by hand.**
   - `reflect/Nudge:88–89`: `#282828` → `--bg-panel`, `#504945` → `--border-strong`.
   - `rgba(142,192,124,.14)` ×7 → `--aqua-fill`: `agents/ApprovalCard:201`, `AskCard:218`, `Composer:325`, `RowView:512`, `incoming/AgentPart:269` and `agents/form.css:113, 222`.
   - Glean's `#9a9df5` → `--route-glean`: `shell/AssistantPane:220` and `lib/hydrated.ts:47` `ROUTE_COLOR`, which FacetBar, Search and SearchPalette use.
   - Exact token matches:
     - `DayStrip:366` `#fe8019` → `--orange` and `:372` `#fb4934` → `--red`;
     - `agents/RowView:146` `#7c6f64` → `--thinking` and `:476` `#d97a6c` → `--result-failed`;
     - `agents/Tree:349` `#8a9a5b` → `--tool`;
     - `agents/StatusLine:66/69` → `--tx-muted-2`, `--border`, `--aqua`.
   - Redundant `var(--msg, #9a9fd6)` fallbacks: `agents/Tree:352, 492, 521` and `agents/Body:53`.
   - **Not a pure swap:** `agents/Tree:448 .host.other` `#9a9fd6` → `--msg` would make that host colour follow the user's Agents setting.
   - No token yet: `incoming/SplitReview:35` BARS and `topics/CategoryManager:11`.
9. **Label copies** → global `.label`. They're byte-identical, so just delete them:
   - `assistant/GleanAnswer:140` and `assistant/SourcesPanel:164`;
   - `incoming/PersonPicker:143`;
   - `AssistantPane:334 .slabel` and `settings/AgentsPreview:46 .cap`;
   - `settings/Settings:913 .glabel` and `:1065 .thead`.
10. **Key-cap copies** → `.kbd`. These are full copies: `agents/AskCard:182 .key`, `agents/Keys:159`, `agents/Pane:259` (base :254) and `agents/StatusLine:163`. `shell/SearchPalette:321` isn't one (no border, 4px), so it's a visible change.
11. **Settings duplicates itself.** The `.seg` (`settings/Row:266` vs `Settings:1078–1092`) and the on/off `.switch` (`Row:242–265` vs `Settings:1018–1040`) are each written twice. Keep one of each; see Cycler and Switch below.
12. **`:global` reach-ins.** Six of the 12 `:global(` uses style a child's internals, which shows a missing prop:
    - `Settings:955/960` → CategoryManager;
    - `Incoming:1144` and `SplitReview:515` → PersonName `.name`;
    - `TopicsIndex:371` and `Topics:348` → CategoryPicker positioning;
    - `NewScion:159`.

    Replace each with a prop as the component it reaches into is touched.

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
4. **`lib/ui/Cycler.svelte`**, with `{options: {value, label}[], value = $bindable, key?, label, disabled?}` (decision 10).
   - **Renders:** `.cycler` (value, `ph-arrows-clockwise`, `.kbd`).
   - **Replaces:** global `.seg` (`app.css:164`, used at `Reflect:145`), `topics/TopicsIndex:214`, `incoming/SplitReview:294` (CSS :435), `settings/Settings:613` and `Row:81/93/128/161`, `todos/TodoForm:69` (`.switch`, keys ⌘1/⌘2), and `assistant/SourcesPanel:202 .tabs` (Kept/Dropped, missed by the list).
   - **`key` must be optional:** Settings' controls sit under the modal key floor, and Reflect gives each option its own key today.
5. **`lib/ui/Switch.svelte`**: the on/off toggle written twice in Settings. It isn't a cycler.
6. **`lib/ui/FilterField.svelte`**, with `{value = $bindable, placeholder, icon, el = $bindable}`: an icon plus a bare input in an `.input` box.
   - **Takes nine copies:** `Todos:298`, `agents/Tree:222`, `topics/TopicsIndex:309`, `settings/Settings:771 .find`, `shell/TabBar:105`, `PersonPicker:154`, `SearchPalette:297`, `SlackReview:147` and `TopicPicker:134`.
   - **Needs `.input:focus-within`**, which bundle.css lacks; nearly every field is a wrapper.
7. **People** (decision 2): `lib/ui/Avatar.svelte` with `{name, color, size: 'lg' | 'sm'}`, a `PersonName` `variant: 'plain' | 'tertiary'`, and later a `PersonRow`.
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

**2 · People.** `.person` is `tx-body`, with the colour on the avatar. **This recolours every name** PersonName renders (Incoming, SplitReview, SignalDetail); only names in meta lines keep the colour (`.person-tertiary`).

**3 · Selection** → `.row.cursor` (aqua-tint, no edge), `.check` and `.open` (bg-head). Every edge goes. There is no `border-left` anywhere; all of them are `box-shadow: inset`.
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

**5 · Popovers, modals, scrims, insets.** These are the Phase 2 Modal and Popover. Inset blocks → `.inset` (6px radius, `--border`):
- `agents/Pane:315 .facts` (7px) and `agents/Joins:38` (7px);
- `SplitReview:558 .legend`;
- `reflect/DayBrief:132 .tile` (bg-head border);
- `settings/AgentsPreview:39` (8px).

**6, 17, 19 · Buttons** → `.btn` (3px 10px), `.btn.md` (5px 12px), `.btn.primary` (now **filled** `aqua-fill`) and `.quiet`.
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

**11 · Colour roles.**
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

## Open: needs the user

- **14 · Row density.** The three densities are settled, but which list gets which isn't. The README and decisions say so; `tokens.json` quietly proposes an assignment. No row sets `line-height` today, so heights follow content. A suggested assignment from the current padding:
  - **Comfortable:** Incoming signals and topics, Settings, Todos, the Topics index.
  - **Cozy:** Priorities, the Topics panels, Reflect panels, Chronology, Search, the Glean panels, SlackReview, Notes.
  - **Compact:** the Agents tree, the five pickers, the ⌘K palette, AskCard options.
  - **No density fits:** message rows (Incoming `.msg` :1071, SplitReview :476, SignalDetail :68, at 2–4px).
- **13 · Done, dismissed and moved.** There's no target. It's catalogued in the design system.
  - **Hatching:** `Incoming:1057 .sig.gone` and `TopicsIndex:468 .row.merged`.
  - **The same hatching means Life time:** `DayStrip:386/393` and `Notes:528`.
  - **Strikethrough:** `Todos:408`, `Notes:583`, `Topics:395`, `SplitReview:518`.
  - **Opacity 0.35–0.6** on read, done, sent and dimmed items.
- **15 · Dividers.** Today it's a mix:
  - `--border` lines in Settings, the Glean panels, and the Incoming, SplitReview and Topics group rules;
  - `--bg-head` lines in Priorities, Rollup, CarryList and StatusLine;
  - nothing elsewhere.

  It needs trying in the app.
- **Move to topic** (`Incoming:671`) by the same rule as Confirm (decision 19)?
- **`.cats-btn.on` / `.srcbtn.on`:** a toggled-on button has no style in the system.

## Problems in the design system itself

- **Stale files.** `tokens.css` and the generated `api/tokens.md` are still version 1 (`sel-edge`, `r-block` 4px, `r-panel` 8px, `r-pill`, `r-modal`). Use `tokens.json` and `bundle.css`.
- **Retired components in the manifest.** `manifest.json` still lists Segmented and SelectedRow, and Button as "outlined, never filled".
- **Missing classes.** `bundle.css` has no `.hints`, `.tag` or `.err`, though Consolidation targets the first two. Its `.input` has no `:focus-within`.

## What this doesn't do

It changes no code. Phases 0 and 1 can land without decisions from the user. Phase 2 changes behaviour (Escape, focus, outside click) and needs the screenshot check plus manual key testing. Phase 3 changes the look and should go decision by decision.
