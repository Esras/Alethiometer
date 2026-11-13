import { describe, expect, it } from 'vitest';
import { keepIndent } from './keepIndent';

const N = '\u00a0';

describe('keepIndent', () => {
  it('keeps a paragraph line’s leading spaces as no-break spaces', () => {
    expect(keepIndent('First\n\n  indented\n    four')).toBe(`First\n\n${N}${N}indented\n${N.repeat(4)}four`);
  });
  it('turns a tab into four', () => expect(keepIndent('a\n\n\tb')).toBe(`a\n\n${N.repeat(4)}b`));
  it('leaves fenced code and list items alone', () => {
    const t = '```\n  code\n```\n- top\n  - nested';
    expect(keepIndent(t)).toBe(t);
  });
  it('leaves blank lines alone', () => expect(keepIndent('a\n   \nb')).toBe('a\n   \nb'));
});
