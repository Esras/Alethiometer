<script lang="ts">
  // Reflect (9b day, 9c week; month and quarter follow the week pattern one
  // level up). Built for stopping: no day strip, search or assistant pane. The
  // note is the user's own writing; the AI never drafts it. The side column is
  // facts, the roll-up of the level below, and what to carry forward.
  //
  // Keys: d w m q scope · [ ] or ←/→ period · t this period ·
  // e/↵ write · Esc leave the note · o open in Obsidian · j/k carry row ·
  // > tomorrow · < schedule · x let go · u undo · T where time went ·
  // D decided · a ask about today.
  import { call } from '../lib/api';
  import { refresh, ui } from '../lib/app.svelte';
  import { isTyping, plain, useKeys } from '../lib/keys.svelte';
  import CarryList from './reflect/CarryList.svelte';
  import DayBrief from './reflect/DayBrief.svelte';
  import NoteEditor from './reflect/NoteEditor.svelte';
  import Rollup from './reflect/Rollup.svelte';
  import { rs } from './reflect/state.svelte';
  import type { CarryAction, CarryRow, ChildRow, Lens, ReflectData, Scope } from './reflect/types';

  const SCOPES: { id: Scope; label: string }[] = [
    { id: 'day', label: 'Day' },
    { id: 'week', label: 'Week' },
    { id: 'month', label: 'Month' },
    { id: 'quarter', label: 'Quarter' },
  ];
  const CHILD = {
    week: { label: 'Days', icon: 'calendar-dots', scope: 'day' },
    month: { label: 'Weeks', icon: 'calendar-dots', scope: 'week' },
    quarter: { label: 'Months', icon: 'calendar-dots', scope: 'month' },
  } as const;
  const OPEN_LABEL = { day: 'Carry forward', week: 'Still open from the week', month: 'Still open from the month', quarter: 'Still open from the quarter' };

  let d = $state<ReflectData | null>(null);
  let err = $state('');
  let selected = $state(0);
  let showWhere = $state(false);
  let showDecided = $state(false);
  let editor = $state<{ focus: () => void; openInObsidian: () => void } | null>(null);
  let askEl = $state<HTMLInputElement | null>(null);
  let question = $state('');
  let answer = $state<{ ok: boolean; text: string } | null>(null);
  let asking = $state(false);

  ui.topicId = null;

  $effect(() => {
    const args = { scope: rs.scope, period: rs.period ?? '', lens: rs.lens };
    void ui.rev;
    call<ReflectData>('reflect.get', args)
      .then((r) => {
        if (r.scope === rs.scope && r.lens === rs.lens) d = r;
        err = '';
      })
      .catch((e) => (err = String(e.message ?? e)));
  });

  // Keep the carry selection in range as rows come and go.
  $effect(() => {
    const n = d?.carry.length ?? 0;
    if (selected >= n) selected = Math.max(0, n - 1);
  });

  function setScope(s: Scope) {
    // Stay near the period being looked at.
    if (d && s !== rs.scope) rs.period = d.is_current ? null : d.period;
    rs.scope = s;
  }
  function step(dir: -1 | 1) {
    if (!d) return;
    const p = dir < 0 ? d.prev : d.next;
    rs.period = p;
  }
  function openChild(row: ChildRow) {
    if (!d || d.scope === 'day') return;
    rs.scope = CHILD[d.scope].scope;
    rs.period = row.period;
  }

  async function act(row: CarryRow, action: CarryAction) {
    try {
      await call('reflect.carry', { item_id: row.item_id, period: row.period, action });
      refresh();
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }
  function actSelected(action: CarryAction) {
    const row = d?.carry[selected];
    if (!row) return;
    if (action === 'undo' ? row.action === null : row.action !== null) return;
    act(row, action);
  }

  async function ask() {
    const q = question.trim();
    if (!q || asking) return;
    asking = true;
    answer = null;
    try {
      const r = await call<{ answer: string }>('ai.ask', { question: q, scope: 'today' });
      answer = { ok: true, text: r.answer };
    } catch (e) {
      answer = { ok: false, text: String((e as Error).message ?? e) };
    } finally {
      asking = false;
    }
  }

  useKeys((e) => {
    if (e.key === 'Escape' && isTyping(e)) {
      (e.target as HTMLElement).blur();
      return true;
    }
    if (plain(e, 'd')) return setScope('day'), true;
    if (plain(e, 'w')) return setScope('week'), true;
    if (plain(e, 'm')) return setScope('month'), true;
    if (plain(e, 'q')) return setScope('quarter'), true;
    if (plain(e, '[') || plain(e, 'ArrowLeft')) return step(-1), true;
    if (plain(e, ']') || plain(e, 'ArrowRight')) return step(1), true;
    if (plain(e, 't')) return (rs.period = null), true;
    if (plain(e, 'e') || (plain(e, 'Enter') && (e.target as HTMLElement).tagName !== 'BUTTON')) return editor?.focus(), true;
    if (plain(e, 'o')) return editor?.openInObsidian(), true;
    if (plain(e, 'a') && d?.scope === 'day' && rs.lens !== 'life') return askEl?.focus(), true;
    if (plain(e, 'T') && d?.brief) return (showWhere = !showWhere), true;
    if (plain(e, 'D') && d?.brief && rs.lens !== 'life') return (showDecided = !showDecided), true;
    const n = d?.carry.length ?? 0;
    if (plain(e, 'j') && n) return (selected = Math.min(n - 1, selected + 1)), true;
    if (plain(e, 'k') && n) return (selected = Math.max(0, selected - 1)), true;
    if (plain(e, '>')) return actSelected('tomorrow'), true;
    if (plain(e, '<')) return actSelected('schedule'), true;
    if (plain(e, 'x')) return actSelected('let_go'), true;
    if (plain(e, 'u')) return actSelected('undo'), true;
  });

  const placeholder = $derived(rs.lens === 'life' ? 'Write if you want to. Only the busy hours are counted here.' : 'What stood out today?');
</script>

<div class="view">
  <div class="controls">
    <div class="seg" role="tablist" aria-label="Scope">
      {#each SCOPES as s}
        <button class:on={rs.scope === s.id} onclick={() => setScope(s.id)} title="{s.label} ({s.id[0]})">{s.label}</button>
      {/each}
    </div>
    {#if d}
      <span class="period">
        <button class="car" onclick={() => step(-1)} title="Previous ([)" aria-label="Previous"><i class="ph ph-caret-left"></i></button>
        <span class="nowrap tnum">{d.title}</span>
        <button class="car" onclick={() => step(1)} title="Next (])" aria-label="Next"><i class="ph ph-caret-right"></i></button>
      </span>
      <span class="ctx nowrap">{d.context}</span>
      {#if !d.is_current}
        <button class="today nowrap" onclick={() => (rs.period = null)} title="t">back to {d.scope === 'day' ? 'today' : `this ${d.scope}`}</button>
      {/if}
    {/if}
  </div>

  {#if err}<div class="err"><i class="ph-bold ph-warning"></i>{err}</div>{/if}

  {#if d}
    <div class="grid" class:day={d.scope === 'day'}>
      {#key `${d.note.path}|${d.period}`}
        <NoteEditor bind:this={editor} note={d.note} scope={d.scope} period={d.period} lens={d.lens} {placeholder} />
      {/key}

      <div class="side scroll">
        {#if d.brief}
          <DayBrief brief={d.brief} bind:showWhere bind:showDecided life={d.lens === 'life'} />
        {/if}
        {#if d.children && d.scope !== 'day'}
          <Rollup rows={d.children} label={CHILD[d.scope].label} icon={CHILD[d.scope].icon} open={openChild} />
        {/if}
        {#if d.lens === 'life'}
          <div class="lifenote">
            <i class="ph ph-house"></i>
            <span>Life counts busy hours only: no titles, no personal items, no AI. What you write here is saved to the note above, in your work vault.</span>
          </div>
        {:else}
          <CarryList rows={d.carry} label={OPEN_LABEL[d.scope]} bind:selected textButtons={d.scope !== 'day'} {act} />
          {#if d.scope === 'day'}
            <div class="ask">
              <label class="askrow">
                <i class="ph ph-magnifying-glass"></i>
                <span class="field">
                  <input
                    bind:this={askEl}
                    bind:value={question}
                    onkeydown={(e) => e.key === 'Enter' && ask()}
                    aria-label="Ask about today"
                  />
                  {#if !question}<span class="hint ellipsis">Ask about today… <span class="eg">e.g. "what did Jordan say about the fix ETA?"</span></span>{/if}
                </span>
                {#if asking}<span class="asking nowrap">asking…</span>{/if}
              </label>
              {#if answer}
                <div class="answer" class:fail={!answer.ok}>
                  {#if !answer.ok}<i class="ph-bold ph-warning"></i>{/if}
                  <span>{answer.ok ? answer.text : `Couldn't ask: ${answer.text}`}</span>
                </div>
              {/if}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px 28px;
  }
  .controls {
    flex: none;
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }
  .period {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 1.1538rem;
    color: var(--tx-title);
  }
  .car {
    color: var(--tx-muted-2);
    display: flex;
    padding: 2px;
    border-radius: var(--r-block);
  }
  .car:hover {
    color: var(--tx-title);
  }
  .ctx {
    font-size: 0.9231rem;
    color: var(--tx-label);
  }
  .today {
    font-size: 0.9231rem;
    color: var(--aqua);
  }
  .today:hover {
    text-decoration: underline;
  }
  .err {
    display: flex;
    gap: 8px;
    align-items: center;
    color: var(--tx-2);
    font-size: 0.9231rem;
  }
  .err i {
    color: var(--orange);
  }
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr);
    gap: 20px;
  }
  .grid.day {
    grid-template-columns: minmax(0, 1fr) 400px;
  }
  .side {
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .ask {
    flex: none;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
  }
  .askrow {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 9px 12px;
    font-size: 1rem;
    color: var(--tx-label);
    position: relative;
  }
  .askrow i {
    color: var(--tx-muted-2);
  }
  .field {
    flex: 1;
    min-width: 0;
    position: relative;
    display: flex;
  }
  .field input {
    width: 100%;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--tx-body);
    padding: 0;
  }
  .hint {
    position: absolute;
    inset: 0;
    pointer-events: none;
    color: var(--tx-label);
  }
  .asking {
    font-size: 0.9231rem;
  }
  .askrow:focus-within {
    box-shadow: inset 0 0 0 1px var(--aqua);
    border-radius: var(--r-panel);
  }
  .eg {
    color: var(--tx-disabled);
  }
  .answer {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    padding: 8px 12px 10px;
    border-top: 1px solid var(--bg-head);
    font-size: calc(var(--fs-read) * 12.5 / 14);
    color: var(--tx-2);
    line-height: 1.5;
  }
  .answer.fail i {
    color: var(--orange);
    margin-top: 3px;
  }
  .lifenote {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 12px;
    border-radius: var(--r-panel);
    border: 1px dashed var(--border);
    color: var(--tx-label);
    font-size: 0.9231rem;
    line-height: 1.5;
  }
  .lifenote i {
    color: var(--blue);
    margin-top: 2px;
  }
</style>
