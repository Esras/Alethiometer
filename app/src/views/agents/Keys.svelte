<script lang="ts">
  // The keys sheet (06), on ?.
  import Modal from '../../lib/ui/Modal.svelte';
  import { ag } from './state.svelte';

  const isMac = navigator.platform.toLowerCase().includes('mac');
  const M = isMac ? '⌘' : 'Ctrl+';
  const SECTIONS: { title: string; keys: [string[], string][] }[] = [
    {
      title: 'Everywhere',
      keys: [
        [[`${M}K`], 'Search everything'],
        [['/'], 'Filter sessions'],
        [['G', 'I'], 'Go to Incoming'],
        [['G', 'A'], 'Go to Agents'],
        [[`${M}.`], 'Interrupt the selected agent'],
        [['?'], 'This sheet'],
      ],
    },
    {
      title: 'Tree',
      keys: [
        [['J', 'K'], 'Move'],
        [['⇧J', '⇧K'], 'Next or previous that needs you (a request, failed, or ended a turn since you looked)'],
        [['H', 'L'], 'Fold or open a group'],
        [['⏎'], 'Open a session or room'],
        [['N'], 'New agent in this group'],
        [['A'], 'Archive the selected row (right-click too)'],
        [['U'], 'Undo an archive, for 10 seconds'],
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
        [['1', '2', '3', '…'], 'Allow once / session / always / deny…, or pick an answer'],
        [['R'], 'Write a reply'],
        [['Esc'], 'Later, and back to the tree'],
      ],
    },
    {
      title: 'Composer',
      keys: [
        [[`${M}⏎`], 'Send'],
        [['@'], 'Address one member (rooms)'],
        [['↑'], 'Bring back your last message'],
        [['Esc'], 'Back to the tree'],
        [['Tab'], 'Switch between the composer and the thread'],
      ],
    },
  ];

  const close = () => (ag.keys = false);
  // Modal does Esc; ? closes too, as it opens.
  function onkeys(e: KeyboardEvent): boolean {
    if (e.key !== '?') return false;
    close();
    return true;
  }
</script>

<Modal label="Keys" onclose={close} {onkeys} position="absolute">
  <div class="sheet modal">
    <div class="head">
      <span class="pane-title">Keys</span>
      <span class="lede"><kbd>⏎</kbd> goes in, <kbd>Esc</kbd> comes out. <kbd>Tab</kbd> goes from the tree to the thread, then toggles the thread and the composer.</span>
      <span class="close"><kbd>?</kbd> or <kbd>Esc</kbd> to close</span>
    </div>
    <div class="grid">
      {#each SECTIONS as s (s.title)}
        <div class="sec">
          <div class="st label">{s.title}</div>
          <div class="keys">
            {#each s.keys as [ks, what] (what)}
              <span class="ks">{#each ks as k (k)}<kbd>{k}</kbd>{/each}</span>
              <span class="what">{what}</span>
            {/each}
          </div>
        </div>
      {/each}
    </div>
    <div class="foot">Needs you keys work whenever a request is pinned and you aren’t typing. Single-letter keys are off while the composer has focus.</div>
  </div>
</Modal>

<style>
  .sheet {
    width: min(1080px, calc(100% - 48px));
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .head {
    display: flex;
    gap: 14px;
    align-items: baseline;
  }
  .lede {
    font-size: 1rem;
    color: var(--tx-muted);
  }
  .close {
    margin-left: auto;
    font-size: 0.885rem;
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
  .foot {
    font-size: 0.923rem;
    color: var(--tx-label);
  }
</style>
