import { describe, expect, it, vi } from 'vitest';

const call = vi.fn();
vi.mock('../../lib/api', () => ({ call: (...a: unknown[]) => call(...a) }));
vi.mock('../../lib/app.svelte', () => ({ ui: {}, clockHM: () => '' }));

const { act, ag, nodes, poll } = await import('./state.svelte');
const { toasts } = await import('../../lib/toast.svelte');

describe('ag.error', () => {
  it("is the poll's own error; an action's error is a toast", async () => {
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
    expect(ag.error).toBeNull();
    expect(toasts.list.map((t) => t.text)).toContain('no terminal');
  });
});

describe('nodes', () => {
  it('lists local sessions once, under the exact set of repos, in any order; remote groups stay', () => {
    const s = (id: string, repos: string[], parent: string | null = null) => ({
      id,
      source: 'acp',
      group_id: null,
      parent_id: parent,
      name: id,
      cwd: repos[0],
      project_dir: repos[0],
      project: repos[0].slice(3),
      repos,
    });
    ag.filter = '';
    ag.tree = {
      groups: [
        { id: 'miao:c1', source: 'miao', project: 'checkout', project_dir: '', name: 'c1', goal: '', room: null },
      ],
      sessions: [
        s('a', ['/r/aleth']),
        s('b', ['/r/aleth', '/r/pi-acp']),
        s('c', ['/r/pi-acp', '/r/aleth']),
        s('d', ['/r/aleth']),
        s('b~1', ['/r/aleth', '/r/pi-acp'], 'b'),
        { ...s('m', ['/x']), source: 'miao', group_id: 'miao:c1' },
      ],
      requests: [],
    } as never;
    const shape = () => nodes().map((n) => n.key);
    expect(shape()).toEqual([
      'p:miao:checkout',
      'g:miao:c1',
      's:m',
      'p:acp:/r/aleth',
      's:a',
      's:d',
      'p:acp:/r/aleth\n/r/pi-acp',
      's:b',
      's:b~1',
      's:c',
    ]);
    const head = nodes().find((n) => n.key === 'p:acp:/r/aleth\n/r/pi-acp');
    expect(head?.type === 'project' && head.label).toBe('aleth & pi-acp');
  });
});
