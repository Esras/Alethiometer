<script lang="ts">
  // The "Goes to" cell for one line: a quiet destination before send, the
  // result after (11a), and the life-line receipt states.
  import { catColor } from '../../lib/app.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import type { EditLine } from './types';

  let { line, typing = false }: { line: EditLine; typing?: boolean } = $props();

  const t = $derived(line.target);
  const empty = $derived(!line.text.trim());
  /** "Incoming · …" uses the short form: topic name, People/Name, or the Jira key. */
  const short = $derived(t ? (t.key ?? t.label) : '');
  const receipt = $derived(line.receipt_at ? line.receipt_at.slice(11, 16) : '');
</script>

{#if line.kind === 'life'}
  {#if line.state === 'confirmed'}
    <span class="main"><i class="ph-bold ph-check" style:color="var(--green)"></i><AppIcon source="marvin" size={14} /><span class="ellipsis">Marvin · Inbox</span></span>
    <span class="sub tnum ellipsis">receipt {receipt} · clearing from this Mac</span>
  {:else if line.state === 'failed'}
    <span class="main"><i class="ph-bold ph-warning" style:color="var(--orange)"></i><span class="ellipsis">Not delivered · kept here</span></span>
    <span class="sub ellipsis">{line.error ?? 'Marvin unreachable'} · retrying · <kbd>⌘R</kbd> now</span>
  {:else if line.state === 'pending'}
    <span class="main"><i class="ph ph-clock" style:color="var(--tx-muted-2)"></i><AppIcon source="marvin" size={14} /><span class="ellipsis">Marvin · Inbox</span></span>
    <span class="sub ellipsis">sent · waiting for receipt</span>
  {:else if !empty}
    <span class="main muted"><span class="dim"><AppIcon source="marvin" size={14} /></span><span class="ellipsis">Marvin · not sent yet</span></span>
    <span class="sub ellipsis">Phase 2 · stays on this Mac</span>
  {/if}
{:else if line.state === 'incoming' && t}
  <span class="main"><i class="ph ph-tray-arrow-down" style:color="var(--aqua)"></i><span class="ellipsis">Incoming · {short}</span></span>
{:else if typing && line.stale}
  <span class="main"><span style:color="var(--tx-disabled)">…</span></span>
{:else if empty}
  <span></span>
{:else if !t}
  <span class="main muted"><i class="ph ph-question"></i><span class="ellipsis">{line.state === 'held' ? 'held · needs a topic' : 'needs a topic'}</span></span>
{:else}
  <span class="main">
    {#if t.type === 'topic'}
      <span class="sq" style:background={catColor(t.category ?? 'work')}></span>
    {:else if t.type === 'person'}
      <i class="ph ph-user" style:color="var(--tx-muted-2)"></i>
    {:else if t.type === 'keep'}
      <i class="ph ph-note-blank" style:color="var(--tx-muted-2)"></i>
    {:else if t.app}
      <AppIcon source={t.app} size={14} />
    {/if}
    <span class="ellipsis" class:muted={t.type === 'keep'}>{t.type === 'keep' ? 'stays on this page' : t.label}</span>
  </span>
  {#if t.create}
    <span class="sub ellipsis">creates a Jira task on send</span>
  {:else if t.offer}
    <span class="sub ellipsis">offer: {t.offer}</span>
  {/if}
{/if}

<style>
  .main {
    display: flex;
    gap: 7px;
    align-items: center;
    color: var(--tx-2);
    min-width: 0;
    white-space: nowrap;
  }
  .main i {
    font-size: 13px;
    flex: none;
  }
  .muted {
    color: var(--tx-label);
  }
  .dim {
    opacity: 0.6;
    display: flex;
  }
  .sq {
    width: 7px;
    height: 7px;
    border-radius: 2px;
    flex: none;
  }
  .sub {
    font-size: 11.5px;
    color: var(--tx-label);
  }
</style>
