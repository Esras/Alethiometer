<script lang="ts">
  // Notes (9a, then 10a/11a): a writing page, one per day. Type freely; a prefix
  // at the start of a line becomes its glyph. The "Goes to" margin quietly
  // suggests a topic, person or app; Tab overrides it. ⌃↵ / ⌘↵ sends work lines
  // with a destination into Incoming. `~` life lines stay here (Marvin is Phase 2).
  import { tick } from 'svelte';
  import { call } from '../lib/api';
  import { refresh, ui } from '../lib/app.svelte';
  import { mod, plain, useKeys } from '../lib/keys.svelte';
  import Margin from './notes/Margin.svelte';
  import Picker from './notes/Picker.svelte';
  import { GLYPHS, PREFIXES, type EditLine, type Line, type Page, type Target } from './notes/types';

  ui.topicId = null;

  let date = $state<string | null>(null);
  let page = $state<Page | null>(null);
  let lines = $state<EditLine[]>([]);
  let focus = $state<number | null>(null);
  let picker = $state<number | null>(null);
  let sending = $state(false);
  const els: HTMLTextAreaElement[] = $state([]);
  let measurer: HTMLDivElement;

  let seq = 0;
  const newKey = () => `k${Date.now().toString(36)}${seq++}`;

  function toEdit(l: Line): EditLine {
    return { ...l, key: l.key ?? (l.id !== null ? `id${l.id}` : newKey()), stale: false };
  }

  function blank(kind: EditLine['kind'] = 'note', bare = kind === 'note'): EditLine {
    return { id: null, key: newKey(), kind, bare, text: '', target: null, state: 'draft', receipt_at: null, error: null, locked: false, stale: true };
  }

  function setPage(p: Page) {
    page = p;
    lines = p.lines.map(toEdit);
    if (!lines.length || lines[lines.length - 1].locked) lines.push(blank());
  }

  async function load(d: string | null) {
    const p = await call<Page>('notes.page', d ? { date: d } : {});
    date = p.date;
    picker = null;
    setPage(p);
  }
  load(null);

  // ---- autosave (debounced, one request at a time) ----
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inflight: Promise<void> | null = null;
  let again = false;

  function dirty() {
    clearTimeout(timer);
    timer = setTimeout(save, 450);
  }

  function payload() {
    return lines
      .filter((l, i) => l.id !== null || l.text.trim() || i < lines.length - 1)
      .map((l) => ({ key: l.key, id: l.id, kind: l.kind, bare: l.bare, text: l.text, target: l.target && !l.target.auto ? l.target : null }));
  }

  function save(): Promise<void> {
    clearTimeout(timer);
    if (inflight) {
      again = true;
      return inflight;
    }
    const d = date;
    const sent = payload();
    let p: Promise<void>;
    const texts = new Map(sent.map((l) => [l.key, l.text]));
    p = call<Page>('notes.save', { date: d, lines: sent })
      .then((p) => {
        if (d !== date) return;
        for (const r of p.lines) {
          const l = lines.find((x) => x.key === r.key);
          if (!l) continue;
          l.id = r.id;
          l.target = r.target;
          l.state = r.state;
          l.locked = r.locked;
          if (texts.get(l.key) === l.text) l.stale = false;
        }
      })
      .catch((e) => {
        ui.error = String(e.message ?? e);
      })
      .finally(() => {
        inflight = null;
        if (again) {
          again = false;
          save();
        }
      });
    inflight = p;
    return p;
  }

  async function flush() {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
      await save();
    }
    while (inflight) await inflight;
  }

  $effect(() => () => {
    // Leaving the view: don't lose the last keystrokes.
    if (timer !== undefined) save();
  });

  // ---- editing ----
  async function place(i: number, pos: number) {
    await tick();
    const el = els[i];
    if (!el) return;
    el.focus();
    const p = Math.max(0, Math.min(pos, el.value.length));
    el.setSelectionRange(p, p);
  }

  function edited(i: number) {
    lines[i].stale = true;
    dirty();
  }

  function onInput(i: number, e: Event) {
    const el = e.currentTarget as HTMLTextAreaElement;
    const l = lines[i];
    let text = el.value.replace(/\n/g, ' ');
    let pos = el.selectionStart;
    for (const p of PREFIXES) {
      const m = text.match(p.re);
      if (m && !l.text.match(p.re) && pos >= m[0].length) {
        l.kind = p.kind;
        l.bare = false;
        text = text.slice(m[0].length);
        pos -= m[0].length;
        if (p.kind === 'life') l.target = null;
        break;
      }
    }
    l.text = text;
    if (el.value !== text) {
      el.value = text;
      el.setSelectionRange(pos, pos);
    }
    edited(i);
  }

  /** Visual row of `pos` inside a wrapped line, and how many rows it has. */
  function rows(el: HTMLTextAreaElement, pos: number): { row: number; count: number } {
    measurer.style.width = `${el.clientWidth}px`;
    measurer.textContent = el.value.slice(0, pos);
    const mark = document.createElement('span');
    mark.textContent = '​';
    measurer.appendChild(mark);
    const lh = 24;
    const row = Math.round(mark.offsetTop / lh);
    measurer.textContent = el.value + '​';
    return { row, count: Math.max(1, Math.round(measurer.offsetHeight / lh)) };
  }

  function onKeydown(i: number, e: KeyboardEvent) {
    if (e.isComposing || e.metaKey || e.ctrlKey || e.altKey) return;
    const el = e.currentTarget as HTMLTextAreaElement;
    const l = lines[i];
    const pos = el.selectionStart;
    const collapsed = el.selectionStart === el.selectionEnd;
    const stop = () => {
      e.preventDefault();
      e.stopPropagation();
    };

    if (e.key === 'Enter') {
      stop();
      if (l.locked) {
        lines.splice(i + 1, 0, blank());
        place(i + 1, 0);
      } else if (!l.text && !l.bare) {
        // Enter on an empty glyph line drops the glyph, like a list.
        l.kind = 'note';
        l.bare = true;
        edited(i);
      } else {
        const rest = l.text.slice(el.selectionEnd);
        l.text = l.text.slice(0, pos);
        const cont = !l.bare && l.kind !== 'delegated' && l.text.trim() !== '';
        const next = cont ? blank(l.kind, false) : blank();
        next.text = rest;
        lines.splice(i + 1, 0, next);
        edited(i);
        edited(i + 1);
        place(i + 1, 0);
      }
      return;
    }

    if (e.key === 'Backspace' && collapsed && pos === 0) {
      if (l.locked) return stop();
      stop();
      if (!l.bare) {
        l.kind = 'note';
        l.bare = true;
        l.target = null;
        edited(i);
        return;
      }
      if (i === 0) return;
      const prev = lines[i - 1];
      if (prev.locked) {
        if (!l.text) {
          lines.splice(i, 1);
          dirty();
        }
        place(i - 1, prev.text.length);
        return;
      }
      const at = prev.text.length;
      prev.text += l.text;
      lines.splice(i, 1);
      edited(i - 1);
      place(i - 1, at);
      return;
    }

    if (e.key === 'Delete' && collapsed && pos === l.text.length && i < lines.length - 1) {
      const next = lines[i + 1];
      if (l.locked || next.locked) return;
      stop();
      l.text += next.text;
      lines.splice(i + 1, 1);
      edited(i);
      place(i, pos);
      return;
    }

    if ((e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.shiftKey) {
      const up = e.key === 'ArrowUp';
      const { row, count } = rows(el, pos);
      const j = up ? i - 1 : i + 1;
      if ((up ? row > 0 : row < count - 1) || j < 0 || j >= lines.length) return;
      stop();
      place(j, up && count > 1 ? lines[j].text.length : Math.min(pos, lines[j].text.length));
      return;
    }

    if (e.key === 'Tab') {
      stop();
      if (!e.shiftKey && !l.locked && l.kind !== 'life' && l.text.trim()) {
        save();
        picker = i;
      }
      return;
    }

    if (e.key === 'Escape') {
      stop();
      el.blur();
    }
  }

  function pick(i: number, t: Target | null) {
    const l = lines[i];
    picker = null;
    if (!l) return;
    l.target = t ? { ...t, auto: false } : null;
    save();
    place(i, els[i]?.selectionStart ?? l.text.length);
  }

  function closePicker(i: number) {
    if (picker !== i) return;
    picker = null;
    place(i, els[i]?.selectionStart ?? lines[i].text.length);
  }

  /** Click below the last line: continue writing at the end. */
  function writeAtEnd() {
    const last = lines.length - 1;
    if (last < 0 || lines[last].locked) {
      lines.push(blank());
      place(lines.length - 1, 0);
    } else {
      place(last, lines[last].text.length);
    }
  }

  // ---- page actions ----
  async function go(d: string | null | undefined) {
    if (!d || d === date) return;
    await flush();
    await load(d);
  }

  async function goToday() {
    if (page?.today) return;
    await flush();
    await load(null);
  }

  async function send() {
    if (sending || !date) return;
    sending = true;
    try {
      await flush();
      const p = await call<Page>('notes.send', { date });
      const at = focus;
      setPage(p);
      refresh();
      if (at !== null) place(Math.min(at, lines.length - 1), 0);
    } finally {
      sending = false;
    }
  }

  async function retry() {
    if (!date || !lines.some((l) => l.state === 'failed')) return;
    await flush();
    setPage(await call<Page>('notes.retry', { date }));
  }

  useKeys((e) => {
    if (mod(e) && e.key === 'Enter') return send(), true;
    if (mod(e) && e.key.toLowerCase() === 'r') return retry(), true;
    if (mod(e) && e.key === '[') return go(page?.prev), true;
    if (mod(e) && e.key === ']') return go(page?.next), true;
    if (plain(e, '[')) return go(page?.prev), true;
    if (plain(e, ']')) return go(page?.next), true;
    if (plain(e, 't')) return goToday(), true;
    if (plain(e, 'Enter') || plain(e, 'i')) return writeAtEnd(), true;
  });

  // ---- derived ----
  const workLines = $derived(lines.filter((l) => l.kind !== 'life' && l.text.trim()));
  const editable = (l: EditLine) => l.state === 'draft' || l.state === 'held';
  const counts = $derived({
    ready: workLines.filter((l) => editable(l) && l.target && l.target.type !== 'keep').length,
    needs: workLines.filter((l) => editable(l) && !l.target).length,
    incoming: workLines.filter((l) => l.state === 'incoming').length,
    confirmed: lines.filter((l) => l.kind === 'life' && l.state === 'confirmed').length,
    waiting: lines.filter((l) => l.kind === 'life' && (l.state === 'pending' || l.state === 'failed')).length,
    life: lines.filter((l) => l.kind === 'life' && l.state === 'draft' && l.text.trim()).length,
  });
  const went = $derived(counts.incoming > 0 && counts.ready === 0);
  const held = $derived(workLines.some((l) => l.state === 'held'));

  /** Split text for the highlight layer: @names and #tags. */
  function segments(l: EditLine): { t: string; c?: string }[] {
    const out: { t: string; c?: string }[] = [];
    const re = /(@[A-Za-z][\w.-]*|(?:^|(?<=\s))#[\w][\w\-./]*)/g;
    let last = 0;
    for (const m of l.text.matchAll(re)) {
      if (m.index! > last) out.push({ t: l.text.slice(last, m.index) });
      out.push({ t: m[0], c: m[0][0] === '@' ? (l.kind === 'delegated' ? 'mention' : 'mention-soft') : 'tag' });
      last = m.index! + m[0].length;
    }
    if (last < l.text.length) out.push({ t: l.text.slice(last) });
    return out;
  }
</script>

<div class="view">
  <div class="sheet">
    <div class="grid head">
      <span></span>
      <span class="day">
        <button class="car" onclick={() => go(page?.prev)} title="Previous day ([)"><i class="ph ph-caret-left"></i></button>
        <span class="title nowrap">{page?.label ?? ''}</span>
        <button class="car" disabled={!page?.next} onclick={() => go(page?.next)} title="Next day (])"><i class="ph ph-caret-right"></i></button>
        {#if page && !page.today}<button class="back" onclick={goToday} title="Back to today (t)">today</button>{/if}
      </span>
      <span class="label goes">{went ? 'Went to' : 'Goes to'}</span>
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="body scroll" onclick={(e) => e.target === e.currentTarget && writeAtEnd()}>
      {#each lines as l, i (l.key)}
        {@const g = GLYPHS[l.kind]}
        <div
          class="grid row"
          class:cur={focus === i}
          class:sent={l.state === 'incoming'}
          class:life={l.kind === 'life' && l.state !== 'failed'}
          class:failed={l.state === 'failed'}
          class:done={l.state === 'confirmed'}
          class:tail={i === lines.length - 1 && l.id === null && !l.text}
        >
          <span class="glyph">{#if !l.bare}<i class="ph-bold ph-{g.icon}" style:color={g.color}></i>{/if}</span>
          <div class="text">
            <div class="mirror" aria-hidden="true">{#each segments(l) as s}<span class={s.c}>{s.t}</span>{/each}{#if !l.text && lines.length === 1}<span class="ph-text">Write anything. Start a line with - [] () ^@ or ~</span>{/if}&#8203;</div>
            <textarea
              bind:this={els[i]}
              value={l.text}
              rows="1"
              spellcheck="false"
              readonly={l.locked}
              oninput={(e) => onInput(i, e)}
              onkeydown={(e) => onKeydown(i, e)}
              onfocus={() => (focus = i)}
              onblur={() => focus === i && (focus = null)}
            ></textarea>
          </div>
          <div class="margin">
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="mcell" onclick={() => !l.locked && l.kind !== 'life' && l.text.trim() && (picker = i)}>
              <Margin line={l} typing={focus === i} />
            </div>
            {#if picker === i}
              <Picker line={l} onpick={(t) => pick(i, t)} onclose={() => closePicker(i)} />
            {/if}
          </div>
        </div>
      {/each}
      <div class="measure" bind:this={measurer} aria-hidden="true"></div>
    </div>

    <div class="foot">
      <span class="legend nowrap">
        <span><b>- [] () ^@</b> kinds</span>
        <span><b>#</b> topic</span>
        <span><b>~</b> life</span>
        <span><b>Tab</b> change where it goes</span>
      </span>
      <span class="counts nowrap tnum">
        {#if counts.ready}<span>{counts.ready} ready</span>{/if}
        {#if counts.incoming}<span>{counts.incoming} in Incoming</span>{/if}
        {#if counts.needs}<span>{counts.needs} {held && !counts.ready ? 'held' : 'needs a topic'}</span>{/if}
        {#if counts.confirmed}<span style:color="var(--green)">{counts.confirmed} confirmed</span>{/if}
        {#if counts.waiting}<span style:color="var(--orange)">{counts.waiting} waiting</span>{/if}
        {#if counts.life}<span class="lifec">{counts.life} life · Phase 2</span>{/if}
      </span>
      <button class="btn primary" class:off={!counts.ready} onclick={send} title="Send to Incoming (⌃↵ / ⌘↵)">Send to Incoming ⌃↵</button>
    </div>
  </div>
</div>

<style>
  .view {
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: center;
    padding: 18px 28px 16px;
  }
  .sheet {
    width: 100%;
    max-width: 1060px;
    display: flex;
    flex-direction: column;
    border-radius: var(--r-panel);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    overflow: hidden;
  }
  .grid {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) 270px;
    gap: 12px;
  }
  .head {
    flex: none;
    align-items: center;
    padding: 12px 26px 10px;
    border-bottom: 1px solid var(--bg-head);
  }
  .day {
    display: flex;
    gap: 10px;
    align-items: center;
    font-size: 1rem;
    color: var(--tx-muted);
  }
  .day .title {
    color: var(--tx-title);
    font-weight: 500;
  }
  .car {
    display: flex;
    color: var(--tx-label);
    border-radius: var(--r-block);
    padding: 2px;
  }
  .car:hover:not(:disabled) {
    color: var(--tx-2);
    background: var(--hover-tint);
  }
  .car:disabled {
    color: var(--tx-disabled);
    cursor: default;
  }
  .back {
    font-size: 0.8846rem;
    color: var(--tx-label);
    padding: 0 6px;
    border-radius: var(--r-block);
  }
  .back:hover {
    color: var(--tx-2);
  }
  .goes {
    padding-left: 15px;
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 12px 26px;
    position: relative;
    cursor: text;
  }
  .row {
    align-items: start;
    padding: 5px 0;
    border-radius: var(--r-block);
    transition: opacity 120ms;
  }
  .row.cur {
    background: rgba(142, 192, 124, 0.05);
  }
  .row.sent {
    opacity: 0.55;
  }
  .row.life {
    background: repeating-linear-gradient(135deg, rgba(131, 165, 152, 0.05) 0 3px, transparent 3px 6px);
  }
  .row.failed {
    background: rgba(254, 128, 25, 0.06);
  }
  .glyph {
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1.0769rem;
  }
  .text {
    position: relative;
    min-width: 0;
  }
  .mirror,
  textarea,
  .measure {
    font-family: inherit;
    font-size: calc(var(--fs-read) * 15 / 14);
    line-height: 1.6;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    word-break: normal;
    letter-spacing: normal;
    padding: 0;
    margin: 0;
    border: 0;
  }
  .mirror {
    color: var(--tx-body);
    min-height: 24px;
  }
  textarea {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    resize: none;
    overflow: hidden;
    background: transparent;
    color: transparent;
    caret-color: var(--aqua);
    outline: none;
    display: block;
  }
  textarea::selection {
    background: rgba(142, 192, 124, 0.25);
    color: transparent;
  }
  textarea[readonly] {
    cursor: default;
  }
  .row.done .mirror {
    color: var(--tx-label);
    text-decoration: line-through;
    text-decoration-color: var(--tx-disabled);
  }
  .mention {
    color: var(--red);
  }
  .mention-soft {
    color: var(--tx-2);
  }
  .tag {
    color: var(--tx-muted-2);
  }
  .ph-text {
    color: var(--tx-disabled);
  }
  .measure {
    position: absolute;
    visibility: hidden;
    pointer-events: none;
    top: 0;
    left: 0;
  }
  .margin {
    position: relative;
    min-width: 0;
    font-size: 0.9615rem;
    border-left: 1px solid var(--border);
    padding-left: 14px;
    align-self: stretch;
  }
  .row.tail .margin {
    border-left-color: transparent;
  }
  .mcell {
    min-height: 24px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 2px;
    cursor: default;
  }
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 10px 26px;
    border-top: 1px solid var(--bg-head);
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .legend {
    display: flex;
    gap: 14px;
    min-width: 0;
    overflow: hidden;
  }
  .legend b {
    font-weight: 400;
    color: var(--tx-2);
    white-space: pre;
  }
  .counts {
    margin-left: auto;
    display: flex;
    gap: 14px;
    color: var(--tx-muted);
  }
  .lifec {
    color: var(--tx-label);
  }
  .btn.primary {
    padding: 3px 11px;
  }
  .btn.off {
    opacity: 0.45;
  }
</style>
