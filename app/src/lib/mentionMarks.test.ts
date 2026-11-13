import { describe, expect, it } from 'vitest';
import { mdTokens } from './markdown';
import { markMentions, mentionRuns, type Mention } from './mentionMarks';

const who = { color: 3, known: true, me: false };
const m = (text: string, name: string): Mention => ({ text, name, who });

describe('mentions through Markdown', () => {
  it('swaps each mention for its marker and back, longest first', () => {
    const ms = [m('@Jordan', 'Jordan Lee'), m('@Jordan Lee', 'Jordan Lee'), m('<@U1>', 'Pat Kim')];
    const t = markMentions('hi @Jordan Lee and <@U1>, @Jordan', ms);
    expect(mentionRuns(t)).toEqual(['hi ', 1, ' and ', 2, ', ', 0]);
  });
  it('keeps a Slack mention from being read as HTML', () => {
    const toks = mdTokens(markMentions('ping <@U1> now', [m('<@U1>', 'Pat Kim')]), false);
    const p = toks[0] as unknown as { tokens: { type: string; text: string }[] };
    expect(p.tokens.map((x) => x.type)).toEqual(['text']);
    expect(mentionRuns(p.tokens[0].text)).toEqual(['ping ', 0, ' now']);
  });
  it('leaves text without mentions alone', () => {
    expect(markMentions('plain <@U9>', [])).toBe('plain <@U9>');
    expect(mentionRuns('plain')).toEqual(['plain']);
  });
});
