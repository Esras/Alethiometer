<script lang="ts">
  // Settings › Agents (07): a live preview of the row grammar in the colours
  // being edited.
  import { agentColor, lookVars, YOU } from '../../views/agents/look';
  import RowView from '../../views/agents/RowView.svelte';
  import type { AgentsLook, Row, Who } from '../../views/agents/types';

  let { look }: { look: AgentsLook } = $props();

  const base = { rev: 0, note: '', tool: null, delivery: null, children: [], request: null, tokens: null, open: false, ext: '' };
  const who = (name: string, session: string): Who => ({ name, session, kind: 'agent' });
  const rows: Row[] = [
    { ...base, id: 1, at: '2026-10-03T10:52:00', kind: 'message', who: who('planner', 'p'), to: null, label: 'instruction', text: 'The secret is per project. Read it from the webhook config, not env.' },
    {
      ...base,
      id: 2,
      at: '2026-10-03T10:57:00',
      kind: 'tool',
      who: null,
      to: null,
      label: '',
      text: '',
      tool: { name: 'run', detail: 'npm test -- hooks', status: 'completed', ms: 2600, result: '✓ 41 passed', added: null, removed: null, summary: '' },
    },
    { ...base, id: 3, at: '2026-10-03T11:03:00', kind: 'you', who: { name: 'You', session: null, kind: 'you' }, to: who('handler', 'h'), label: 'mention', text: 'Use the staging Jira project for tests, not prod.' },
  ];
  const colors: Record<string, number> = { p: 0, h: 1 };
  const nameColor = (w: Who | null | undefined) => (!w ? agentColor(look, { color: 1, template: 'builder' }) : w.kind === 'you' ? YOU : agentColor(look, { color: colors[w.session ?? ''] ?? 2, template: w.name }));
</script>

<div class="preview" style={lookVars(look)}>
  <div class="label cap">Live preview</div>
  <div class="rows">
    {#each rows as r (r.id)}<RowView row={r} own="h" {nameColor} />{/each}
  </div>
</div>

<style>
  .preview {
    margin-top: 6px;
    border-radius: var(--r-panel);
    background: var(--bg-app);
    border: 1px solid var(--border);
    padding: 8px 22px 12px;
  }
  .cap {
    margin: 0 -10px 8px;
  }
  .rows {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
</style>
