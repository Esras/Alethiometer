import { describe, expect, it } from 'vitest';
import { isMac, mod } from './keys.svelte';

const key = (init: Partial<KeyboardEvent>) =>
  ({ ctrlKey: false, metaKey: false, altKey: false, ...init }) as KeyboardEvent;

describe('mod', () => {
  it.skipIf(isMac)('is Ctrl, but not AltGr (which Windows reports as Ctrl+Alt)', () => {
    expect(mod(key({ ctrlKey: true, key: '[' }))).toBe(true);
    expect(mod(key({ ctrlKey: true, altKey: true, key: '[' }))).toBe(false);
    expect(mod(key({ metaKey: true, key: '[' }))).toBe(false);
  });

  it.runIf(isMac)('is ⌘ on the Mac, with or without ⌥', () => {
    expect(mod(key({ metaKey: true, key: '[' }))).toBe(true);
    expect(mod(key({ metaKey: true, altKey: true, key: '[' }))).toBe(true);
    expect(mod(key({ ctrlKey: true, key: '[' }))).toBe(false);
  });
});
