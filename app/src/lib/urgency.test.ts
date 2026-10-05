import { describe, expect, it } from 'vitest';
import { urgencyColor } from './app.svelte';

// Time pressure has three steps (ui-plan decision 11): red from 5 minutes.
describe('urgencyColor', () => {
  it('is green above 15 minutes, yellow from 6 to 15, red at 5 or less', () => {
    expect(urgencyColor(16)).toBe('var(--time-far)');
    expect(urgencyColor(15)).toBe('var(--time-near)');
    expect(urgencyColor(6)).toBe('var(--time-near)');
    expect(urgencyColor(5)).toBe('var(--time-now)');
    expect(urgencyColor(4)).toBe('var(--time-now)');
    expect(urgencyColor(0)).toBe('var(--time-now)');
  });
});
