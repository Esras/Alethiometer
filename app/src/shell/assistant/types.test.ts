import { describe, expect, it } from 'vitest';
import { claims, sentences } from './types';

describe('sentences', () => {
  it('keeps citations with the sentence they close', () => {
    const s = sentences('SSO is still a paid add-on [2]. Kim needs your sign-off [3], because policy requires it [4]. Ask @{Kim Lee}.');
    expect(s.map((x) => x.cites)).toEqual([[2], [3, 4], []]);
    expect(s[2].pieces.some((p) => p.t === 'person' && p.name === 'Kim Lee')).toBe(true);
  });
  it('handles text without punctuation', () => {
    expect(sentences('no ending [1]')[0].cites).toEqual([1]);
  });
});

describe('claims', () => {
  it('splits a sentence at its citations', () => {
    const [s] = sentences('Ravi hasn\'t answered [1], and SSO is a paid add-on [2] [3].');
    expect(claims(s).map((c) => c.cites)).toEqual([[1], [2, 3], []]);
  });
});
