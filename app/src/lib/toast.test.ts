import { afterEach, describe, expect, it, vi } from 'vitest';
import { closeToast, hold, toast, toasts, TOAST_MS } from './toast.svelte';

afterEach(() => {
  toasts.list.forEach((t) => closeToast(t.id));
  vi.useRealTimers();
});

describe('toast', () => {
  it('goes after TOAST_MS, not while hovered, and the same text again restarts it', () => {
    vi.useFakeTimers();
    toast(new Error('a person needs a name'));
    expect(toasts.list.map((t) => t.text)).toEqual(['a person needs a name']);
    vi.advanceTimersByTime(TOAST_MS - 1000);
    toast('a person needs a name');
    expect(toasts.list).toHaveLength(1);
    vi.advanceTimersByTime(TOAST_MS - 1000);
    expect(toasts.list).toHaveLength(1);
    const id = toasts.list[0].id;
    hold(id, true);
    vi.advanceTimersByTime(TOAST_MS * 2);
    expect(toasts.list).toHaveLength(1);
    hold(id, false);
    vi.advanceTimersByTime(TOAST_MS);
    expect(toasts.list).toHaveLength(0);
  });
});
