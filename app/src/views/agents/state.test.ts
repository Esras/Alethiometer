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
  it('groups by project, Unsorted last in each, and a drag adds an empty Unsorted to its own project only', () => {
    const g = (id: string, dir: string) => ({ id, source: 'acp', project: dir.slice(3), project_dir: dir, name: id });
    const s = (id: string, group: string | null, dir: string) => ({
      id,
      source: 'acp',
      group_id: group,
      parent_id: null,
      name: id,
      cwd: dir,
      project_dir: dir,
      project: dir.slice(3),
    });
    ag.filter = '';
    ag.tree = {
      groups: [g('local:1', '/r/aleth'), g('local:2', '/r/k8s')],
      sessions: [s('a', 'local:1', '/r/aleth'), s('b', null, '/r/aleth'), s('c', 'local:2', '/r/k8s')],
      requests: [],
    } as never;
    const shape = () => nodes().map((n) => n.key);
    expect(shape()).toEqual([
      'p:acp:/r/aleth',
      'g:local:1',
      's:a',
      'u:acp:/r/aleth',
      's:b',
      'p:acp:/r/k8s',
      'g:local:2',
      's:c',
    ]);
    ag.drag = { session: 'c', project: 'acp:/r/k8s' };
    expect(shape().slice(-2)).toEqual(['s:c', 'u:acp:/r/k8s']);
    expect(shape().filter((k) => k.startsWith('u:'))).toHaveLength(2);
    ag.drag = null;
  });
});
