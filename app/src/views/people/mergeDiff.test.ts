import { describe, expect, it } from 'vitest';
import { diffRows } from './mergeDiff';

// As `people.merge_preview` gives them (see `person_merge` tests in the core).
const KIM = '---\nslack: U0KIM\nemail: [kim@work.com]\nrole: Buyer\n---\n## How Kim works\n- Short docs\n';
const KIMBERLY =
  '---\nemail: [kimberly.p@work.com]\nrole: Procurement analyst\nmanager: Dana\n---\n- Out until Sep 1\n';
const MERGED =
  '---\nslack: U0KIM\nemail: [kim@work.com, kimberly.p@work.com]\nrole: Buyer\nmanager: Dana\n---\n## How Kim works\n- Short docs\n\n## Merged from Kimberly P.\n- Out until Sep 1\n';

const show = (rows: ReturnType<typeof diffRows>) =>
  rows.map((r) => `${r.left?.text ?? ''} | ${r.right?.mark ?? ''}${r.right?.text ?? ''}`);

describe('merge rows', () => {
  it('lines each left line up with where it ends up, and marks what changed', () => {
    expect(show(diffRows(KIM, KIMBERLY, MERGED, ['role']))).toEqual([
      '--- | ---',
      ' | slack: U0KIM',
      'email: [kimberly.p@work.com] | ~email: [kim@work.com, kimberly.p@work.com]',
      'role: Procurement analyst | ≠role: Buyer',
      'manager: Dana | +manager: Dana',
      '--- | ---',
      ' | ## How Kim works',
      ' | - Short docs',
      ' | +',
      ' | +## Merged from Kimberly P.',
      '- Out until Sep 1 | +- Out until Sep 1',
    ]);
  });

  it('puts a block list beside its field', () => {
    const gone = '---\naliases:\n  - Kimberly\n  - KP\n---\n';
    const merged =
      '---\nslack: U0KIM\nemail: [kim@work.com]\nrole: Buyer\naliases: [Kimberly, KP]\n---\n## How Kim works\n- Short docs\n';
    const rows = diffRows(KIM, gone, merged, []);
    expect(show(rows).slice(4, 7)).toEqual(['aliases: | +aliases: [Kimberly, KP]', '  - Kimberly | ', '  - KP | ']);
  });
});
