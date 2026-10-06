<script lang="ts">
  // A sender's name in their person colour (16c). Someone without a People
  // note gets a dotted underline in their hash colour (17a); clicking it opens
  // Link to person (17b) when the caller offers it. Always coloured: 'thread'
  // is the weight-500 name in a thread, 'tertiary' the regular-weight one in a
  // meta line. `dim` fades it, for a dismissed message. `block` is a name on
  // its own line outside any button (Topics' People panel): the unknown name is
  // then a real button, so it takes the keyboard.
  import { personColor, type Who } from '../people';

  let {
    who,
    name,
    variant = 'thread',
    dim = false,
    block = false,
    onlink,
  }: { who: Who; name: string; variant?: 'thread' | 'tertiary'; dim?: boolean; block?: boolean; onlink?: () => void } = $props();
  const linkable = $derived(!who.known && !!who.account && !!onlink);
</script>

{#if linkable}
  <!-- A span unless `block`: it can sit inside a row's own button (the meta line). -->
  <svelte:element
    this={block ? 'button' : 'span'}
    class="name unknown ellipsis"
    class:tertiary={variant === 'tertiary'}
    class:dim
    class:block
    role={block ? undefined : 'button'}
    tabindex={block ? undefined : -1}
    style:color={personColor(who.color)}
    title="{name} · no People note · link to a person"
    onclick={(e: MouseEvent) => (e.stopPropagation(), onlink?.())}
    onkeydown={block ? undefined : () => {}}>{name}</svelte:element
  >
{:else}
  <span class="name ellipsis" class:tertiary={variant === 'tertiary'} class:dim style:color={personColor(who.color)} title={who.name && who.name !== name ? `${name} · ${who.name}` : name}>{name}</span>
{/if}

<style>
  .name {
    font-weight: 500;
    text-align: left;
    min-width: 0;
  }
  .name.tertiary {
    font-weight: 400;
  }
  .name.dim {
    opacity: 0.5;
  }
  .block {
    display: block;
    max-width: 100%;
  }
  .unknown {
    text-decoration: underline dotted var(--tx-label);
    text-underline-offset: 3px;
    cursor: pointer;
  }
</style>
