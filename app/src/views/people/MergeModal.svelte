<script lang="ts">
  // Merge two people (design canvas `MergeModal`): the merged-away note on the
  // left, the kept note after the merge on the right (`mergeDiff.ts` lines them
  // up). Accounts and aliases are unioned and fields only one note has are
  // copied, by the core (`people.merge_preview`); a field both set differently
  // is a line to decide: `n` goes to the next one, ← takes the left note's
  // value, → keeps the kept note's. `s` swaps who is kept. ⌘↵ stages the merge:
  // it's written when you leave People, and `u` undoes it until then.
  import { call } from '../../lib/api';
  import { chord, mod } from '../../lib/keys.svelte';
  import { personColor, type PeopleRow } from '../../lib/people';
  import Modal from '../../lib/ui/Modal.svelte';
  import { diffRows } from './mergeDiff';
  import type { StagedMerge } from './types';

  let {
    keep: keepRow,
    gone: goneRow,
    onstage,
    onclose,
  }: {
    keep: PeopleRow;
    gone: PeopleRow;
    onstage: (m: StagedMerge) => void;
    onclose: () => void;
  } = $props();

  type Side = { id: number; name: string; path: string; text: string };
  type Preview = {
    keep: Side;
    gone: Side;
    conflicts: { key: string; keep: string[]; gone: string[] }[];
    merged: string;
  };

  // svelte-ignore state_referenced_locally
  let pair = $state({ keep: keepRow, gone: goneRow });
  let take = $state<string[]>([]);
  let focus = $state(0);
  let data = $state<Preview | null>(null);
  let err = $state<string | null>(null);

  $effect(() => {
    const args = { keep: pair.keep.id, gone: pair.gone.id, take: [...take] };
    let stale = false;
    call<Preview>('people.merge_preview', args)
      .then((p) => !stale && ((data = p), (err = null)))
      .catch((e) => !stale && (err = String((e as Error).message ?? e)));
    return () => (stale = true);
  });

  const keys = $derived(data?.conflicts.map((c) => c.key) ?? []);
  const rows = $derived(data ? diffRows(data.keep.text, data.gone.text, data.merged, keys) : []);
  const current = $derived(keys[focus] ?? null);
  const goneColor = $derived(personColor(pair.gone.color));
  const keepColor = $derived(personColor(pair.keep.color));

  function pick(key: string, left: boolean) {
    focus = Math.max(0, keys.indexOf(key));
    take = left ? [...new Set([...take, key])] : take.filter((k) => k !== key);
  }

  function swap() {
    pair = { keep: pair.gone, gone: pair.keep };
    take = [];
    focus = 0;
    data = null;
  }

  function stage() {
    if (!data) return;
    onstage({
      keep: data.keep.id,
      gone: data.gone.id,
      keepName: data.keep.name,
      goneName: data.gone.name,
      take: [...take],
      keep_text: data.keep.text,
      gone_text: data.gone.text,
    });
  }

  function onkeys(e: KeyboardEvent): boolean {
    if (mod(e) && e.key === 'Enter') return (stage(), true);
    if (e.metaKey || e.ctrlKey || e.altKey) return false;
    if (e.key === 'n' && keys.length) return ((focus = (focus + 1) % keys.length), true);
    if (e.key === 'ArrowLeft' && current) return (pick(current, true), true);
    if (e.key === 'ArrowRight' && current) return (pick(current, false), true);
    if (e.key === 's') return (swap(), true);
    return false;
  }
</script>

<Modal label="Merge {pair.gone.name} into {pair.keep.name}" {onclose} {onkeys}>
  <div class="box modal" role="document">
    <div class="top">
      <span class="title"
        >Merge <span style:color={goneColor}>{pair.gone.name}</span> into
        <span style:color={keepColor}>{pair.keep.name}</span></span
      >
      {#if keys.length}<span class="decide">{keys.length} line{keys.length === 1 ? '' : 's'} to decide</span>{/if}
      <span class="grow"></span>
      <span class="label">Keep</span>
      <span class="who" role="group" aria-label="Who is kept">
        <button style:color={goneColor} onclick={swap}>{pair.gone.name}</button><button
          class="on"
          style:color={keepColor}
          aria-pressed="true">{pair.keep.name}</button
        >
      </span>
      <kbd>s</kbd>
    </div>

    {#if err}
      <div class="err">{err}</div>
    {:else if data}
      <div class="dr head">
        <span></span>
        <span class="gone-path"><span class="path">{data.gone.path}</span> · goes to Obsidian's trash</span>
        <span></span><span></span><span></span>
        <span class="keep-path"><span class="path">{data.keep.path}</span> · after the merge</span>
      </div>
      <div class="diff scroll">
        {#each rows as r, i (i)}
          {@const decide = r.right?.mark === '≠' && r.right.key !== null}
          {@const taken = decide && take.includes(r.right!.key!)}
          <div
            class="dr"
            class:add={r.right?.mark === '+' || r.right?.mark === '~'}
            class:cf={decide}
            class:cur={decide && r.right!.key === current}
          >
            <span class="n">{r.left?.n ?? ''}</span>
            <span class="l" class:struck={decide && !taken}>{r.left?.text ?? ''}</span>
            <span class="g">
              {#if decide}
                <button
                  class="side"
                  class:on={taken}
                  aria-label="Take the left note's value"
                  onclick={() => pick(r.right!.key!, true)}>←</button
                ><button
                  class="side"
                  class:on={!taken}
                  aria-label="Keep the kept note's value"
                  onclick={() => pick(r.right!.key!, false)}>→</button
                >
              {:else if r.left && r.right?.mark}→{/if}
            </span>
            <span class="n n2">{r.right?.n ?? ''}</span>
            <span class="m">{r.right?.mark ?? ''}</span>
            <span class="r">{r.right?.text ?? ''}</span>
          </div>
        {/each}
      </div>
    {:else}
      <div class="loading">Reading both notes…</div>
    {/if}

    <div class="foot">
      <span class="hints"
        ><kbd>n</kbd> next line to decide · <kbd>←</kbd> <kbd>→</kbd> pick its side · <kbd>s</kbd> swap who is kept ·
        written when you leave People, <kbd>u</kbd> undoes until then</span
      >
      <span class="grow"></span>
      <button class="btn md" onclick={onclose}>Cancel <kbd>Esc</kbd></button>
      <button class="btn md primary" disabled={!data} onclick={stage}
        >Merge into {pair.keep.name} <kbd>{chord('⌘↵', 'Ctrl+↵')}</kbd></button
      >
    </div>
  </div>
</Modal>

<style>
  .box {
    position: fixed;
    left: 50%;
    top: 56px;
    transform: translateX(-50%);
    width: min(1400px, calc(100vw - 80px));
    max-height: calc(100vh - 112px);
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    padding: 0;
    border-radius: var(--radius-md);
  }
  .top {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
  }
  .title {
    font-size: 1.4615rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .decide {
    font-size: 0.9231rem;
    color: var(--status-attention);
  }
  .grow {
    flex: 1;
  }
  .who {
    display: inline-flex;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-md);
    overflow: hidden;
    font-size: 0.9615rem;
  }
  .who button {
    padding: 4px 12px;
  }
  .who button.on {
    background: var(--aqua-fill);
    font-weight: 500;
  }
  .dr {
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) 56px 30px 16px minmax(0, 1fr);
    font-family: var(--font-mono);
    font-size: 0.9615rem;
    line-height: 22px;
  }
  .dr.head {
    font-family: var(--font-ui);
    font-size: 0.9231rem;
    line-height: 34px;
    border-bottom: 1px solid var(--border);
    color: var(--tx-label);
  }
  .gone-path {
    padding-left: 4px;
    background: var(--hatch);
  }
  .path {
    font-family: var(--font-mono);
    color: var(--tx-muted);
  }
  .keep-path .path {
    color: var(--tx-body);
  }
  .diff {
    flex: 1;
    min-height: 0;
    padding: 6px 0;
  }
  .n {
    color: var(--tx-disabled);
    text-align: right;
    padding-right: 8px;
  }
  .l {
    color: var(--tx-label);
    padding-right: 8px;
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .l.struck {
    text-decoration: line-through;
    text-decoration-color: var(--red);
  }
  .r {
    color: var(--tx-muted);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .g {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    color: var(--tx-disabled);
    font-family: var(--font-ui);
  }
  .m {
    color: var(--green);
  }
  .dr.add .r,
  .dr.add .m,
  .dr.add .n2 {
    background: var(--pill-green-fill);
  }
  .dr.add .r {
    color: var(--tx-body);
  }
  .dr.cf {
    background: color-mix(in srgb, var(--status-attention) 7%, transparent);
  }
  .dr.cf .m {
    color: var(--status-attention);
  }
  .dr.cf .r {
    color: var(--tx-body);
  }
  .dr.cur {
    outline: 1.5px solid var(--aqua);
    outline-offset: -1.5px;
  }
  .side {
    padding: 0 4px;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: 0.8462rem;
    line-height: 16px;
    color: var(--tx-label);
  }
  .side.on {
    border-color: var(--status-attention);
    color: var(--status-attention);
  }
  .err,
  .loading {
    padding: 16px 20px;
    color: var(--tx-label);
  }
  .err {
    color: var(--status-broken);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
  }
  .hints {
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
</style>
