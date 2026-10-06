import { describe, expect, it } from 'vitest';
import { nextKey } from './nextKey';

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
