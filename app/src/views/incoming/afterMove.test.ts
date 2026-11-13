import { describe, expect, it } from 'vitest';
import { afterMove } from './afterMove';

describe('afterMove', () => {
  const none = () => false;
  it('goes to the next message in time order, not with the chunk that moved', () => {
    expect(afterMove([1, 2, 3, 4, 5], new Set([2, 3]), none, 2)).toBe(4);
  });
  it('skips messages that moved earlier in the chunk or were dismissed', () => {
    // 2 and 4 moved (a scattered selection); 5 was dismissed.
    expect(afterMove([1, 2, 3, 4, 5, 6], new Set([2, 4]), (id) => id === 5, 4)).toBe(6);
  });
  it('falls back to the nearest one before when the chunk was at the end', () => {
    expect(afterMove([1, 2, 3, 4], new Set([3, 4]), (id) => id === 2, 4)).toBe(1);
  });
  it('stays when nothing is left to go to', () => {
    expect(afterMove([1, 2], new Set([1, 2]), none, 1)).toBe(1);
  });
});
