import { describe, expect, it } from 'vitest';
import { mdTokens, safeHref } from './markdown';

describe('mdTokens', () => {
  it('reads a GFM table', () => {
    const [t] = mdTokens('| a | b |\n|:-|-:|\n| x | `y` |');
    expect(t.type).toBe('table');
    expect(t.type === 'table' && t.rows[0][1].tokens[0].type).toBe('codespan');
  });

  it('keeps a Slack fence whole, its first word as content', () => {
    const code = mdTokens('see ```select 1\nfrom t``` ok', false).filter((t) => t.type === 'code');
    expect(code.map((t) => t.type === 'code' && t.text.trim())).toEqual(['select 1\nfrom t']);
  });

  it('leaves [item N] as text for the citation pills', () => {
    const [p] = mdTokens('see [item 12] here');
    const inl = p.type === 'paragraph' ? (p.tokens ?? []) : [];
    expect(inl.map((t) => ('text' in t ? t.text : '')).join('')).toBe('see [item 12] here');
    expect(inl.map((t) => t.type)).toEqual(['text']);
  });
});

describe('safeHref', () => {
  it('only lets web links out', () => {
    expect(safeHref('https://x.com')).toBe('https://x.com');
    expect(safeHref('javascript:alert(1)')).toBeNull();
  });
});
