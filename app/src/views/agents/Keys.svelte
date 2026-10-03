<script lang="ts">
  // The keys sheet (06), on ?.
  import { ag } from './state.svelte';

  const isMac = navigator.platform.toLowerCase().includes('mac');
  const M = isMac ? '⌘' : 'Ctrl';
  const SECTIONS: { title: string; keys: [string[], string][] }[] = [
    {
      title: 'Everywhere',
      keys: [
        [[M, 'K'], 'Search everything'],
        [['/'], 'Filter sessions'],
        [['G', 'I'], 'Go to Incoming'],
        [['G', 'A'], 'Go to Agents'],
        [[M, '.'], 'Interrupt the selected agent'],
        [['?'], 'This sheet'],
      ],
    },
    {
      title: 'Tree',
      keys: [
        [['J', 'K'], 'Move'],
        [['H', 'L'], 'Fold or open a group'],
        [['⏎'], 'Open a session or room'],
        [['N'], 'New agent in this group'],
        [['Esc'], 'Back to the tree, from anywhere'],
      ],
    },
    {
      title: 'Thread',
      keys: [
        [['J', 'K'], 'Move between rows'],
        [['E'], 'Fold or open: thinking, pairs, tool output'],
        [['⏎'], 'Reply to a message, or open a tool call'],
        [['C'], 'Copy the row'],
        [['G'], 'Jump to latest'],
        [['T', 'M'], 'Transcript or Messages'],
      ],
    },
    {
      title: 'Needs you',
      keys: [
        [['O'], 'Allow once'],
        [['S'], 'Allow for this session'],
        [['A'], 'Always (the agent’s own rule)'],
        [['D'], 'Deny…'],
        [['1', '2', '3'], 'Pick an answer'],
        [['R'], 'Write a reply'],
        [['Esc'], 'Later'],
      ],
    },
    {
      title: 'Composer',
      keys: [
        [[M, '⏎'], 'Send'],
        [['@'], 'Address one member (rooms)'],
        [['↑'], 'Bring back your last message'],
        [['Esc'], 'Back to the thread'],
      ],
    },
  ];
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={() => (ag.keys = false)}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="sheet" onclick={(e) => e.stopPropagation()}>
    <div class="head">
      <span class="title">Keys</span>
      <span class="lede"><span class="kbd">⏎</span> goes in, <span class="kbd">Esc</span> comes out. <span class="kbd">Tab</span> moves between the tree, the thread and the composer.</span>
      <span class="close">? or Esc to close</span>
    </div>
    <div class="grid">
      {#each SECTIONS as s (s.title)}
        <div class="sec">
          <div class="st">{s.title}</div>
          <div class="keys">
            {#each s.keys as [ks, what] (what)}
              <span class="ks">{#each ks as k (k)}<span class="kbd">{k}</span>{/each}</span>
              <span class="what">{what}</span>
            {/each}
          </div>
        </div>
      {/each}
    </div>
    <div class="foot">Needs you keys work whenever a request is pinned and you aren’t typing. Single-letter keys are off while the composer has focus.</div>
  </div>
</div>

<style>
  .scrim {
    position: absolute;
    inset: 0;
    background: rgba(12, 13, 14, 0.66);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 20;
  }
  .sheet {
    width: min(1080px, calc(100% - 48px));
    border-radius: 12px;
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.5);
    padding: 18px 24px 20px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }
  .title {
    font-size: 1.308rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .lede {
    font-size: 1rem;
    color: var(--tx-muted);
  }
  .close {
    margin-left: auto;
    font: 0.885rem var(--font-mono);
    color: var(--tx-label);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 18px 28px;
  }
  .sec {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .st {
    font-size: 0.808rem;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--tx-label);
    padding-bottom: 2px;
    border-bottom: 1px solid var(--border);
  }
  .keys {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 6px 12px;
    align-items: center;
  }
  .ks {
    display: flex;
    gap: 3px;
    justify-content: flex-end;
  }
  .what {
    font-size: 1rem;
    color: var(--tx-2);
  }
  .kbd {
    font: 0.846rem var(--font-mono);
    color: var(--tx-body);
    background: var(--bg-head);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-block);
    padding: 0 5px;
    white-space: nowrap;
  }
  .foot {
    font-size: 0.923rem;
    color: var(--tx-label);
  }
</style>
