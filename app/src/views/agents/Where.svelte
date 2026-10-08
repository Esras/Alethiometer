<script lang="ts">
  // Where the new agent runs: on this machine, in an OpenShell sandbox (when
  // the CLI is installed), or on Scion, and for Scion the Hub project's group
  // (pills under Joins).
  import { ag, openForm } from './state.svelte';

  const scion = $derived((ag.tree?.groups ?? []).filter((g) => g.source === 'scion'));
  const openshell = $derived(!!ag.tree?.openshell);
  const on = $derived(ag.form?.kind === 'scion' ? ag.form.group : null);
  const sandboxed = $derived(ag.form?.kind === 'local' && !!ag.form.openshell);
  const local = $derived(ag.form?.kind === 'local' ? ag.form.group : null);
</script>

{#if scion.length || openshell}
  <div class="fields">
    <span class="lab label">Where</span>
    <span class="chips" role="radiogroup" aria-label="Where the agent runs">
      <button class="chip" class:on={!on && !sandboxed} onclick={() => openForm(null, local)}>Local</button>
      {#if openshell}<button class="chip" class:on={sandboxed} onclick={() => openForm(null, local, true)}>OpenShell</button>{/if}
      {#if scion.length}<button class="chip" class:on={!!on} onclick={() => !on && openForm(scion[0])}>Scion</button>{/if}
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
