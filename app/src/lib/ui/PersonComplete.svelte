<script lang="ts">
  // `@` in a note's textarea: a menu of people (People notes) at the caret; ↑↓ pick, ⏎ or Tab writes the
  // person's `[[People/Name]]` link in place of the `@word`, Esc closes it until the word changes. The
  // textarea gets an `input` event, so its own binding and autosave see the change as typing.
  // Placement: textarea-caret measures the caret, which a textarea doesn't expose.
  import getCaretCoordinates from 'textarea-caret';
  import { peopleList } from '../mentions.svelte';
  import { atQuery, insertLink, matchPeople, personLink } from '../personLinks';
  import type { PersonHit } from '../people';
  import PersonPill from './PersonPill.svelte';

  let { el }: { el: HTMLTextAreaElement | undefined } = $props();

  let text = $state('');
  let caret = $state(0);
  let pick = $state(0);
  let closedAt = $state<number | null>(null);
  let pos = $state({ top: 0, left: 0 });

  const at = $derived(atQuery(text, caret));
  const options = $derived(at && closedAt !== at.start ? matchPeople(peopleList(), at.q) : []);

  function sync() {
    if (!el) return;
    text = el.value;
    caret = el.selectionStart ?? 0;
    pick = 0;
    const q = atQuery(text, caret);
    if (!q) return (closedAt = null);
    const c = getCaretCoordinates(el, q.start);
    const r = el.getBoundingClientRect();
    pos = { top: r.top + c.top - el.scrollTop + c.height + 2, left: r.left + c.left - el.scrollLeft };
  }

  function complete(p: PersonHit) {
    if (!el || !at) return;
    const r = insertLink(el.value, at.start, caret, personLink(p));
    el.value = r.text;
    el.setSelectionRange(r.caret, r.caret);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    sync();
  }

  function onKey(e: KeyboardEvent) {
    if (!options.length) return;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      pick = (pick + (e.key === 'ArrowDown' ? 1 : options.length - 1)) % options.length;
    } else if ((e.key === 'Enter' || e.key === 'Tab') && !e.metaKey && !e.ctrlKey && !e.shiftKey) {
      complete(options[Math.min(pick, options.length - 1)]);
    } else if (e.key === 'Escape') {
      closedAt = at?.start ?? null;
    } else return;
    // Before the textarea's own keys (Tab indents, Esc leaves the editor) and the view's.
    e.preventDefault();
    e.stopImmediatePropagation();
  }

  $effect(() => {
    const t = el;
    if (!t) return;
    const events = ['input', 'keyup', 'click', 'scroll'] as const;
    for (const ev of events) t.addEventListener(ev, sync);
    t.addEventListener('keydown', onKey, true);
    const blur = () => (closedAt = atQuery(t.value, t.selectionStart ?? 0)?.start ?? null);
    t.addEventListener('blur', blur);
    return () => {
      for (const ev of events) t.removeEventListener(ev, sync);
      t.removeEventListener('keydown', onKey, true);
      t.removeEventListener('blur', blur);
    };
  });
</script>

{#if options.length}
  <div class="person-menu popover flush" style:top="{pos.top}px" style:left="{pos.left}px" role="listbox">
    {#each options as p, i (p.id)}
      <button
        role="option"
        aria-selected={i === pick}
        class:on={i === pick}
        onmousedown={(e) => (e.preventDefault(), complete(p))}
        ><PersonPill who={{ color: p.color, known: true }} name={p.name} />{#if p.role}<span class="role">{p.role}</span
          >{/if}</button
      >
    {/each}
  </div>
{/if}

<style>
  .person-menu {
    position: fixed;
    z-index: 30;
    min-width: 220px;
    max-width: 360px;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
  }
  .person-menu button {
    display: flex;
    gap: 10px;
    align-items: baseline;
    justify-content: space-between;
    padding: 4px 10px;
    text-align: left;
    font-size: 0.923rem;
  }
  .person-menu button.on {
    background: var(--border);
  }
  .role {
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
