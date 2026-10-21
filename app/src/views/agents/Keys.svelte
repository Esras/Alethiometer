<script lang="ts">
  // The keys sheet (06), on ?.
  import KeySheet, { type KeySection } from '../../lib/ui/KeySheet.svelte';
  import { ag } from './state.svelte';

  const isMac = navigator.platform.toLowerCase().includes('mac');
  const M = isMac ? '⌘' : 'Ctrl+';
  const SECTIONS: KeySection[] = [
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
        [['M'], 'Move a local session to another group of its project (or drag it)'],
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
</script>

<KeySheet
  sections={SECTIONS}
  onclose={() => (ag.keys = false)}
  position="absolute"
  foot="Needs you keys work whenever a request is pinned and you aren’t typing. Single-letter keys are off while the composer has focus."
>
  {#snippet lede()}<kbd>⏎</kbd> goes in, <kbd>Esc</kbd> comes out. <kbd>Tab</kbd> goes from the tree to the thread, then toggles
    the thread and the composer.{/snippet}
</KeySheet>
