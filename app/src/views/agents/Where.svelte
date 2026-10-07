<script lang="ts">
  // Where the new agent runs: local or Scion, and for Scion the Hub project's
  // group (pills under Joins).
  import { ag, openForm } from './state.svelte';

  const scion = $derived((ag.tree?.groups ?? []).filter((g) => g.source === 'scion'));
  const on = $derived(ag.form?.kind === 'scion' ? ag.form.group : null);
</script>

{#if scion.length}
  <div class="fields">
    <span class="lab label">Where</span>
    <span class="chips" role="radiogroup" aria-label="Where the agent runs">
      <button class="chip" class:on={!on} onclick={() => openForm(null)}>Local</button>
      <button class="chip" class:on={!!on} onclick={() => !on && openForm(scion[0])}>Scion</button>
    </span>
    {#if on}
      <span class="lab label">Project</span>
      <span class="chips" role="radiogroup" aria-label="Scion project">
        {#each scion as g (g.id)}
          <button class="chip" class:on={on === g.id} onclick={() => openForm(g)}>{g.project}{g.name && g.name !== g.project ? ` · ${g.name}` : ''}</button>
        {/each}
      </span>
    {/if}
  </div>
{/if}
