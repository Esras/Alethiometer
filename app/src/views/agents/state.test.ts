import { describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../../lib/api', () => ({ call: (...a: unknown[]) => call(...a) }));
vi.mock('../../lib/app.svelte', () => ({ ui: {}, clockHM: () => '' }));

const { act, ag, poll } = await import('./state.svelte');

describe('ag.error', () => {
  it("an action's error outlives the next poll; the poll's own error doesn't", async () => {
    call.mockImplementation(async (cmd: string) => {
      if (cmd === 'agents.open') throw new Error('no terminal');
      if (cmd === 'agents.poll' && call.mock.calls.length === 1) throw new Error('poll down');
      return { rev: 1, tree: null, thread: null };
    });
    await poll();
    expect(ag.error).toBe('poll down');
    await poll();
    expect(ag.error).toBeNull();
    await expect(act('open', { session: 's', how: 'terminal' })).rejects.toThrow();
    await poll();
    expect(ag.error).toBe('no terminal');
  });
});
