import { describe, expect, it } from 'vitest';
import { blank, changed, chipLabels, collisions, imageFlag, merged, resolve } from './newAgent';

const H = [
  { key: 'pi', label: 'Pi', sandbox_from: 'localhost/aleth-pi:local', sandbox_command: 'pi-acp' },
  { key: 'claude', label: 'Claude Code', sandbox_from: 'localhost/aleth-claude:local', sandbox_command: '' },
];

describe('a saved agent in the form', () => {
  const raw = { ...blank('pi', 'Sandbox Pi'), runs: 'openshell' as const, model: 'opus', task: 'Wire it.' };

  it('fills an empty Directories with where New agent was opened from, and that is not a change', () => {
    const base = resolve(raw, ['/r/aleth'], H);
    expect(base.dirs).toEqual(['/r/aleth']);
    expect(changed(raw, base, base)).toEqual([]);
    expect(merged(raw, base, base).dirs).toEqual([]);
  });

  it('counts a changed field and saves only it', () => {
    const base = resolve(raw, ['/r/aleth'], H);
    const f = { ...base, model: 'sonnet', thinking: 'high', dirs: ['/r/aleth', '/r/pi-acp'] };
    expect(changed(raw, base, f)).toEqual(['model', 'thinking', 'dirs']);
    const out = merged(raw, base, f);
    expect(out).toMatchObject({ name: 'Sandbox Pi', model: 'sonnet', thinking: 'high', task: 'Wire it.' });
    expect(out.dirs).toEqual(['/r/aleth', '/r/pi-acp']);
  });

  it('never counts the task typed for a launch, nor saves it', () => {
    for (const r of [raw, { ...raw, task: '' }]) {
      const base = resolve(r, [], H);
      const f = { ...base, task: 'Do this instead' };
      expect(changed(r, base, f)).toEqual([]);
      expect(merged(r, base, f).task).toBe(r.task);
    }
  });

  it("doesn't count a field the form removed", () => {
    const base = resolve(raw, [], H);
    expect(changed(raw, base, { ...base, runs: 'local', image: 'other' })).toEqual(['runs']);
  });
});

describe('directories', () => {
  it('two folders with the same name read parent/name and collide in a new sandbox', () => {
    const d = ['/r/aleth', '/src/miao/app', '/src/glean-kit/app'];
    expect(chipLabels(d)).toEqual(['aleth', 'miao/app', 'glean-kit/app']);
    expect(collisions(d)).toEqual(['/src/miao/app', '/src/glean-kit/app']);
    expect(collisions(['/r/a', '/r/b'])).toEqual([]);
  });
});

describe('image', () => {
  it("is flagged when it's another harness's and not this one's", () => {
    const f = { ...blank('claude'), runs: 'openshell' as const, image: 'localhost/aleth-pi:local' };
    expect(imageFlag(f, H)).toBe(
      "localhost/aleth-pi:local is Pi's image; Claude Code's is localhost/aleth-claude:local",
    );
    expect(imageFlag({ ...f, harness: 'pi' }, H)).toBeNull();
    expect(imageFlag({ ...f, image: 'ghcr.io/x:1' }, H)).toBeNull();
    expect(imageFlag({ ...f, sandbox: 'box' }, H)).toBeNull();
  });
});
