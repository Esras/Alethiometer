<script lang="ts">
  // Where the new agent runs: on this machine, or in an OpenShell sandbox (when the CLI is installed).
  import { ag, openForm } from './state.svelte';

  const openshell = $derived(!!ag.tree?.openshell);
  const sandboxed = $derived(ag.form?.kind === 'local' && !!ag.form.openshell);
  const local = $derived(ag.form?.kind === 'local' ? ag.form.group : null);
  const cwd = $derived(ag.form?.kind === 'local' ? ag.form.cwd : undefined);
</script>

{#if openshell}
  <div class="fields">
    <span class="lab label">Where</span>
    <span class="chips" role="radiogroup" aria-label="Where the agent runs">
      <button class="chip" class:on={!sandboxed} onclick={() => openForm(local, false, cwd)}>Local</button>
      <button class="chip" class:on={sandboxed} onclick={() => openForm(local, true, cwd)}>OpenShell</button>
    </span>
  </div>
{/if}
