<script lang="ts">
  // One row of the shared grammar (rooms and transcripts): a 20px icon column
  // whose colour says the kind, a mono header (who → target · type, then
  // delivery and time on the right), and the body below. Boxes grow into the
  // margin so text lines up whether or not a row is boxed.
  import Body from './Body.svelte';
  import RowView from './RowView.svelte';
  import { agentColor, ROOM, YOU } from './look';
  import { hm, look, sessionById } from './state.svelte';
  import type { Row, Who } from './types';

  let {
    row,
    own = null,
    sel = false,
    open = false,
    earlier = 0,
    nested = false,
    waiting = false,
    onDelivery,
    onToggle,
    nameColor,
  }: {
    row: Row;
    own?: string | null;
    sel?: boolean;
    open?: boolean;
    earlier?: number;
    nested?: boolean;
    waiting?: boolean;
    onDelivery?: (action: string) => void;
    onToggle?: () => void;
    /** Settings preview: colours from the page being edited. */
    nameColor?: (w: Who | null | undefined) => string;
  } = $props();

  const lk = $derived(look());
  const color = (w: Who | null | undefined) =>
    nameColor ? nameColor(w) : !w ? agentColor(lk, sessionById(own)) : w.kind === 'you' ? YOU : w.kind === 'room' ? ROOM : w.kind === 'hub' ? 'var(--hub)' : agentColor(lk, sessionById(w.session));
  // A session's own rows drop the name unless the setting says otherwise.
  const ownName = $derived(lk.names.self_in_transcript && !row.who && own ? sessionById(own)?.name : null);

  const d = $derived(row.delivery);
  const loud = $derived(d?.state === 'unreachable' ? 'unreach' : d?.state === 'failed' ? 'fail' : null);
  const box = $derived.by(() => {
    if (nested) return '';
    if (loud === 'unreach') return 'b-need';
    if (loud === 'fail') return 'b-fail';
    switch (row.kind) {
      case 'you':
        return 'b-you';
      case 'tool':
        return 'b-tool';
      case 'message':
        return 'b-msg';
      case 'hub':
        return 'b-hub';
      case 'ask':
        // Answered: no longer waiting on you.
        return row.request ? 'b-need' : '';
      case 'approval':
        return 'b-need';
      case 'failed':
        return 'b-fail';
      default:
        return '';
    }
  });
  const ICON: Record<string, string> = {
    you: 'ph-user',
    output: 'ph-text-align-left',
    thinking: 'ph-brain',
    tool: 'ph-wrench',
    message: 'ph-arrows-left-right',
    state: 'ph-swap',
    hub: 'ph-info',
    ask: 'ph-question',
    approval: 'ph-shield-warning',
    failed: 'ph-x-circle',
  };
  const lines = (s: string) => s.split('\n');
  const long = (s: string) => s.split('\n').length > 3 || s.length > 400;
  const clip = (s: string) => lines(s).slice(0, 3).join('\n');
  const secs = (ms: number | null) => (ms == null ? '' : ms < 1000 ? `${(ms / 1000).toFixed(1)}s` : ms < 60000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms / 60000)}m`);
</script>

{#snippet header(main: import('svelte').Snippet, right?: import('svelte').Snippet)}
  <div class="head">
    {@render main()}
    <span class="right">
      {#if right}{@render right()}{/if}
      {#if earlier}<span class="earlier">{earlier} earlier <span class="k">E</span></span>{/if}
      {#if row.at}<span class="time">{hm(row.at)}</span>{/if}
    </span>
  </div>
{/snippet}

{#snippet whoTo()}
  {#if row.who}
    <span class="who" style:color={color(row.who)}>{row.who.name}</span>
    {#if row.to}<span class="arrow">→</span><span class="target" style:color={color(row.to)}>{row.to.kind === 'you' ? 'you' : row.to.name}</span>{/if}
    {#if row.label}<span class="type">· {row.label}</span>{/if}
  {:else if row.to}
    {#if ownName}<span class="who" style:color={color(null)}>{ownName}</span>{/if}
    <span class="dir">{row.label === 'result' ? 'from' : 'to'}</span><span class="who" style:color={color(row.to)}>{row.to.kind === 'you' ? 'you' : row.to.name}</span>
    {#if row.label}<span class="type">· {row.label}</span>{/if}
  {:else}
    {#if ownName}<span class="who" style:color={color(null)}>{ownName}</span>{/if}
    <span class="own">{row.label || row.kind}</span>
  {/if}
{/snippet}

{#snippet deliveryMark()}
  {#if d}
    {#if d.state === 'delivered'}<span class="dl"><i class="ph ph-check"></i>{d.count || ''}</span>
    {:else if d.state === 'deferred'}<span class="dl yellow"><i class="ph ph-clock"></i>deferred{d.delivered_at ? `, delivered ${hm(d.delivered_at)}` : ''}</span>
    {:else if d.state === 'pending'}<span class="dl">sending…</span>
    {:else if d.state === 'unreachable'}<span class="dl orange"><i class="ph ph-plugs"></i>not delivered</span>
    {:else if d.state === 'failed'}<span class="dl red"><i class="ph ph-x-circle"></i>failed</span>{/if}
  {/if}
{/snippet}

<div class="r {box}" class:sel class:nested>
  {#if row.kind === 'state'}
    <div class="g">
      <i class="ph {ICON.state} ic" style:color="var(--tx-label)"></i>
      <div class="mid">
        <div class="head line">
          <span class="line-text">{#if row.who}<span style:color={color(row.who)}>{row.who.name}</span> {/if}{row.text}</span>
          <span class="right">{@render deliveryMark()}<span class="time">{hm(row.at)}</span></span>
        </div>
      </div>
    </div>
  {:else if row.kind === 'hub'}
    <div class="g">
      <i class="ph {ICON.hub} ic" style:color="var(--hub)"></i>
      <div class="mid">
        <div class="head line hubline">
          <span class="line-text"><span style:color="var(--hub)">Hub</span> · {row.text}</span>
          <span class="right"><span class="time">{hm(row.at)}</span></span>
        </div>
      </div>
    </div>
  {:else if row.kind === 'thinking'}
    <div class="g">
      <i class="ph {ICON.thinking} ic" style:color="var(--thinking)"></i>
      <div class="mid">
        {#snippet m()}<span class="own">thinking{row.tokens ? ` · ${row.tokens} tok` : ''}</span>{/snippet}
        {#snippet r()}<span class="k">E</span>{/snippet}
        {@render header(m, r)}
        <div class="think" class:open role="button" tabindex="0" onclick={() => onToggle?.()} onkeydown={(e) => e.key === 'Enter' && onToggle?.()}>{row.text}</div>
      </div>
    </div>
  {:else if row.kind === 'tool' || row.kind === 'approval'}
    {@const t = row.tool}
    <div class="g">
      <i class="ph {ICON[row.kind]} ic" style:color={row.kind === 'approval' ? 'var(--need)' : 'var(--tool)'}></i>
      <div class="mid">
        {#if row.kind === 'approval'}
          {#snippet m()}<span class="tname need">{t?.name ?? 'tool'}</span><span class="type">· needs approval</span>{/snippet}
          {#snippet r()}{#if waiting}<span class="waiting">waiting for you ↓</span>{/if}{/snippet}
          {@render header(m, r)}
          <div class="cmd">{row.text || t?.detail}</div>
        {:else}
          {#snippet m()}
            <span class="tname" class:failed={t?.status === 'failed'}>{t?.name}</span>
            <span class="detail">{t?.detail}{#if t?.added != null}&nbsp; <span class="add">+{t.added}</span> <span class="del">−{t.removed}</span>{/if}</span>
          {/snippet}
          {#snippet r()}{#if t?.status === 'in_progress' || t?.status === 'pending'}<span class="run">running</span>{:else if t?.ms != null}<span class="time">{secs(t.ms)}</span>{/if}{/snippet}
          {@render header(m, r)}
        {/if}
      </div>
    </div>
    {#if row.kind === 'tool' && (t?.result || row.note)}
      <div class="g">
        <i class="ph ph-arrow-elbow-down-right ic" style:color="var(--tx-label)"></i>
        <div class="mid">
          <div class="result" class:open class:failed={t?.status === 'failed'} role="button" tabindex="0" onclick={() => onToggle?.()} onkeydown={(e) => e.key === 'Enter' && onToggle?.()}>
            {#if t?.result.startsWith('✓')}<span class="ok">✓</span>{(open ? t.result : clip(t.result)).slice(1)}{:else}{open ? t?.result : clip(t?.result ?? '')}{/if}{#if row.note}{t?.result ? ' · ' : ''}{row.note}{/if}{#if !open && t && long(t.result)}<span class="more"> … <span class="k">E</span></span>{/if}
          </div>
        </div>
      </div>
    {/if}
  {:else}
    <div class="g">
      <i class="ph {ICON[row.kind] ?? 'ph-dot'} ic" style:color={row.kind === 'you' ? YOU : row.kind === 'output' ? 'var(--tx-2)' : row.kind === 'message' ? 'var(--msg)' : row.kind === 'ask' ? 'var(--need)' : 'var(--red)'}></i>
      <div class="mid">
        {#snippet r()}
          {#if row.note}<span class="note">{row.note}</span>{/if}
          {#if row.kind === 'ask' && waiting}<span class="waiting">waiting for you ↓</span>{/if}
          {@render deliveryMark()}
        {/snippet}
        {@render header(whoTo, r)}
        {#if row.text}<Body text={row.text} tone={row.kind === 'you' || row.kind === 'ask' ? 'you' : row.kind === 'message' && !nested ? 'boxed' : row.kind === 'message' ? 'boxed' : 'agent'} />{/if}
        {#if row.images?.length}
          <div class="imgs">
            {#each row.images as img, i (i)}<img src="data:{img.mime};base64,{img.data}" alt="pasted" />{/each}
          </div>
        {/if}
        {#if d && loud}
          <div class="grid">
            <span class="gk">why</span><span>{d.reason || (loud === 'unreach' ? 'the agent isn’t running' : 'the Hub refused it')}</span>
            {#if loud === 'unreach'}<span class="gk">options</span><span>starting it again runs a fresh session with this as its task</span>
            {:else if d.retry_at}<span class="gk">next</span><span>retrying at {hm(d.retry_at)}</span>{/if}
          </div>
          <div class="acts">
            {#if loud === 'unreach'}
              <button class="primary" onclick={() => onDelivery?.('start_and_send')}><span class="k">⏎</span>Start it again and send</button>
              <button onclick={() => onDelivery?.('forward')}><span class="k">F</span>Send to another agent</button>
              <button onclick={() => onDelivery?.('discard')}><span class="k">Esc</span>Discard</button>
            {:else}
              <button class="primary" onclick={() => onDelivery?.('retry')}><span class="k">⏎</span>Retry now</button>
              <button onclick={() => onDelivery?.('edit')}><span class="k">E</span>Edit</button>
              <button onclick={() => onDelivery?.('cancel')}><span class="k">Esc</span>Cancel</button>
            {/if}
          </div>
        {:else if sel && d && (d.state === 'deferred' || d.seen_by.length)}
          <div class="grid">
            <span class="gk">delivery</span>
            <span
              >{#if d.state === 'deferred'}<span class="yellow">deferred {hm(d.at)}</span> {d.reason}{#if d.delivered_at}
                  <span class="arrow">→</span> <span class="aqua">delivered {hm(d.delivered_at)}</span> at the start of its next turn{/if}{:else}<span class="aqua">delivered {hm(d.delivered_at || d.at)}</span>{d.count > 1 ? ` to ${d.count}` : ''}{/if}</span
            >
            {#if d.seen_by.length}<span class="gk">seen by</span><span>{d.seen_by.join(' · ')}</span>{/if}
          </div>
        {/if}
      </div>
    </div>
    {#each row.children as c (c.ext || c.id)}
      {#if c.kind === 'tool' && c.tool?.summary}
        <div class="g">
          <i class="ph ph-wrench ic" style:color="var(--tool)"></i>
          <div class="mid">
            <div class="head line">
              <span class="line-text"><span style:color={color(c.tool.name === row.to?.name ? row.to : null)}>{c.tool.name}</span> · {c.tool.summary}</span>
              <span class="right"><span class="k">E</span></span>
            </div>
          </div>
        </div>
      {:else}
        <RowView row={c} {own} nested={true} />
      {/if}
    {/each}
  {/if}
</div>

<style>
  .r {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .r.nested {
    gap: 6px;
  }
  .b-you,
  .b-tool,
  .b-msg,
  .b-hub,
  .b-need,
  .b-fail {
    margin: 0 -10px;
    padding: 6px 10px;
    border-radius: 7px;
    border: 1px solid transparent;
  }
  .b-you {
    background: var(--you-fill);
    border-color: var(--you-border);
  }
  .b-tool {
    background: var(--tool-fill);
    border-color: var(--tool-border);
  }
  .b-msg {
    background: var(--msg-fill);
    border-color: var(--msg-border);
  }
  .b-hub {
    background: var(--hub-fill);
    border-color: var(--hub-border);
  }
  .b-need {
    background: var(--need-fill);
    border-color: var(--need-border);
  }
  .b-fail {
    background: var(--fail-fill);
    border-color: var(--fail-border);
  }
  .sel {
    background: var(--aqua-tint);
  }
  .sel:not(.b-you, .b-tool, .b-msg, .b-hub, .b-need, .b-fail) {
    border: 1px solid transparent;
    margin: 0 -10px;
    padding: 6px 10px;
    border-radius: 7px;
  }
  .g {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr);
    gap: 8px;
    align-items: start;
  }
  .ic {
    font-size: 1rem;
    line-height: 20px;
  }
  .mid {
    min-width: 0;
  }
  .head {
    display: flex;
    gap: 6px;
    align-items: baseline;
    font: 0.923rem/20px var(--font-mono);
    min-width: 0;
  }
  .head.line {
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  .head.hubline {
    color: var(--tx-muted);
  }
  .line-text {
    min-width: 0;
  }
  .right {
    margin-left: auto;
    display: flex;
    gap: 10px;
    align-items: baseline;
    white-space: nowrap;
  }
  .time {
    font-size: 0.846rem;
    color: var(--tx-disabled);
  }
  .k {
    font-size: 0.808rem;
    color: var(--tx-label);
  }
  .earlier {
    font-size: 0.846rem;
    color: var(--tx-label);
  }
  .earlier .k {
    color: var(--tx-muted-2);
  }
  .who {
    font-weight: 500;
  }
  .arrow {
    color: var(--tx-disabled);
  }
  .dir {
    color: var(--tx-label);
  }
  .type {
    color: var(--tx-disabled);
    font-size: 0.808rem;
  }
  .own {
    color: var(--tx-label);
    font-size: 0.846rem;
  }
  .note {
    font-size: 0.846rem;
    color: var(--tx-label);
  }
  .waiting {
    font-size: 0.846rem;
    color: var(--need);
  }
  .dl {
    display: flex;
    gap: 3px;
    align-items: center;
    font-size: 0.846rem;
    color: var(--tx-disabled);
  }
  .dl i {
    font-size: 0.846rem;
  }
  .yellow {
    color: var(--yellow);
  }
  .orange {
    color: var(--orange);
  }
  .red {
    color: var(--red);
  }
  .aqua {
    color: var(--aqua);
  }
  .think {
    font: italic 1rem/1.5 var(--font-ui);
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .think.open {
    white-space: pre-wrap;
  }
  .tname {
    font-weight: 600;
    color: var(--tool);
    font-size: 0.9615rem;
  }
  .tname.need {
    color: var(--need);
  }
  .tname.failed {
    color: var(--red);
  }
  .detail {
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    font-size: 0.9615rem;
  }
  .add {
    color: var(--green);
  }
  .del {
    color: var(--red);
  }
  .run {
    font-size: 0.846rem;
    color: var(--aqua);
  }
  .cmd {
    font: 0.923rem/1.6 var(--font-mono);
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .imgs {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 5px;
  }
  .imgs img {
    height: 96px;
    width: auto;
    max-width: 160px;
    object-fit: cover;
    border-radius: 6px;
    border: 1px solid var(--border-strong);
  }
  .result {
    font: 0.885rem/20px var(--font-mono);
    color: var(--tx-label);
    white-space: pre;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .result.open {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 480px;
    overflow-y: auto;
  }
  .result.failed {
    color: var(--result-failed);
  }
  .ok {
    color: var(--green);
  }
  .grid {
    margin-top: 6px;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 1px 12px;
    font: 0.846rem/1.6 var(--font-mono);
    color: var(--tx-muted-2);
  }
  .gk {
    color: var(--tx-disabled);
  }
  .acts {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 8px;
  }
  .acts button {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 4px 11px;
    border-radius: var(--r-input);
    font-size: 0.9615rem;
    border: 1px solid var(--border-strong);
    color: var(--tx-2);
  }
  .acts button .k {
    font: 0.808rem var(--font-mono);
  }
  .acts .primary {
    background: var(--aqua-fill);
    border-color: var(--aqua);
    color: var(--aqua);
  }
  .acts .primary .k {
    color: var(--aqua);
  }
</style>
