<script lang="ts" module>
  export type Conversation = { value: string; name: string; kind: 'channel' | 'dm' | 'group' };
  export type Conversations = { muted: Conversation[]; choices: Conversation[] };
  export const KIND_ICON = { channel: 'ph-hash', dm: 'ph-user', group: 'ph-users' } as const;
</script>

<script lang="ts">
  import { call } from '../../lib/api';

  let {
    sourceId,
    value,
    onchange,
    onadd,
  }: { sourceId: number | undefined; value: string[]; onchange: (v: string[]) => void; onadd: () => void } = $props();

  let data = $state<Conversations | null>(null);

  $effect(() => {
    void value.join('\n');
    if (sourceId === undefined) return;
    call<Conversations>('settings.chat_conversations', { source: sourceId })
      .then((d) => (data = d))
      .catch(() => (data = null));
  });

  const rows = $derived(
    value.map((v) => data?.muted.find((m) => m.value === v) ?? { value: v, name: v, kind: 'channel' as const }),
  );
  const groups = $derived([
    { title: 'Channels', rows: rows.filter((r) => r.kind === 'channel') },
    { title: 'DMs', rows: rows.filter((r) => r.kind !== 'channel') },
  ]);
</script>

<div class="muted-lists">
  {#each groups as g (g.title)}
    <div class="grp">
      <span class="label">{g.title}</span>
      {#each g.rows as r (r.value)}
        <span class="item" title={r.value}>
          <i class="ph {KIND_ICON[r.kind]}"></i><span class="name ellipsis">{r.name.replace(/^#/, '')}</span>
          <button onclick={() => onchange(value.filter((v) => v !== r.value))} aria-label="Stop ignoring {r.name}"
            ><i class="ph ph-x"></i></button
          >
        </span>
      {:else}
        <span class="none">none</span>
      {/each}
    </div>
  {/each}
  <button class="btn" onclick={onadd}>Add… <kbd>↵</kbd></button>
</div>

<style>
  .muted-lists {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 8px;
    width: 360px;
  }
  .grp {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px 2px 8px;
    border-radius: var(--radius-sm);
    color: var(--tx-2);
  }
  .item:hover {
    background: var(--row-hover);
  }
  .item i {
    color: var(--tx-label);
  }
  .name {
    flex: 1;
    min-width: 0;
  }
  .item button {
    color: var(--tx-label);
    font-size: 0.8462rem;
    display: flex;
  }
  .item button:hover {
    color: var(--tx-title);
  }
  .none {
    padding: 2px 8px;
    color: var(--tx-label);
  }
</style>
