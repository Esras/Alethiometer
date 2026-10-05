<script lang="ts">
  // The Joins block at the top of New agent (05): where the agent lands, drawn
  // the way the tree draws the group. The new agent comes last, dashed, named
  // as the Name field says.
  import { agentColor } from './look';
  import { look } from './state.svelte';
  import StatusMark from '../../lib/ui/StatusMark.svelte';
  import type { Session } from './types';

  let {
    project,
    name,
    goal = '',
    host = '',
    room = null,
    members = [],
    newName,
  }: { project: string; name: string; goal?: string; host?: string; room?: string | null; members?: Session[]; newName: string } = $props();
</script>

<div class="joins inset">
  <div class="lab label">Joins{#if project}<span class="proj">{project}</span>{/if}</div>
  <div class="group">
    <i class="ph ph-caret-down"></i>
    <span class="gn"><span class="name">{name}</span>{#if goal}<span class="goal">{goal}</span>{/if}</span>
    {#if host}<span class="host">{host}</span>{/if}
  </div>
  <div class="members">
    {#if room}<span class="m"><i class="ph ph-hash room"></i><span class="rn">{room}</span></span>{/if}
    {#each members as s (s.id)}
      <span class="m"><StatusMark status={s.status} /><span style:color={agentColor(look(), s)}>{s.name}</span></span>
    {/each}
    <span class="m new"><span class="ring"></span>{newName || 'new agent'}</span>
  </div>
</div>

<style>
  .joins {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .lab {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }
  .proj {
    color: var(--tx-disabled);
  }
  .group {
    display: grid;
    grid-template-columns: 14px minmax(0, 1fr) auto;
    gap: 6px;
    align-items: center;
  }
  .group i {
    font-size: 0.846rem;
    color: var(--tx-label);
  }
  .gn {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    color: var(--tx-body);
    font-weight: 500;
  }
  .goal {
    color: var(--tx-label);
    font-size: 0.923rem;
    margin-left: 8px;
  }
  .host {
    font: 0.846rem var(--font-mono);
    color: var(--blue);
    background: var(--bg-head);
    border-radius: 4px;
    padding: 1px 6px;
    white-space: nowrap;
  }
  .members {
    display: flex;
    gap: 14px;
    align-items: center;
    flex-wrap: wrap;
    padding-left: 20px;
    font-size: 0.9615rem;
  }
  .m {
    display: flex;
    gap: 6px;
    align-items: center;
    white-space: nowrap;
  }
  .room {
    font-size: 0.923rem;
    color: var(--msg);
  }
  .rn {
    color: var(--tx-2);
  }
  .new {
    padding: 1px 8px;
    border-radius: 5px;
    border: 1px dashed var(--aqua);
    color: var(--aqua);
  }
  .ring {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    box-sizing: border-box;
    border: 1.5px dashed var(--aqua);
    flex: none;
  }
</style>
