import { describe, expect, it } from 'vitest';
import { nextKey, putBack } from './nextKey';

describe('nextKey', () => {
  it('goes to the next topic left, even when one before it went too', () => {
    // A (dismissed, kept until you moved off) and B (confirmed) both leave.
    expect(nextKey([1, 2, 3, 4, 'unsorted'], 2, [3, 4, 'unsorted'])).toBe(3);
  });
  it('takes a new topic next, and the last list when nothing follows', () => {
    expect(nextKey([1, 2, 'unsorted'], 2, [1, 9, 'unsorted'])).toBe(9);
    expect(nextKey([1, 'unsorted', 'agents'], 'agents', [1, 'unsorted'])).toBe('unsorted');
  });
});

describe('putBack', () => {
  it('keeps a dismissed row in place when the one above it left too', () => {
    // 1 and 2 dismissed; the list without both comes back while 2 is still selected.
    const row = (item_id: number) => ({ item_id });
    const list = [3, 4, 5].map(row);
    putBack(list, [1, 2, 3, 4, 5].map(row), 2);
    expect(list.map((s) => s.item_id)).toEqual([2, 3, 4, 5]);
  });
});
