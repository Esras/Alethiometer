// A message's resolved mentions (`people::Mention`) through Markdown: each one's text is swapped for a
// private-use marker before parsing (so `<@U123>` isn't read as HTML, nor `@Kim Lee` split up), and the
// marker comes back out of the text tokens as the person.
import type { Who } from './people';

export interface Mention {
  /** Exactly what the message holds: `<@U123>`, `@kimlee`, `@Kim Lee`. */
  text: string;
  name: string;
  who: Who;
}

const OPEN = '\ue000';
const CLOSE = '\ue001';
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** The text with each mention replaced by its marker, longest first. */
export function markMentions(text: string, mentions: Mention[] | undefined): string {
  if (!mentions?.length) return text;
  const order = mentions.map((m, i) => ({ m, i })).sort((a, b) => b.m.text.length - a.m.text.length);
  const re = new RegExp(order.map(({ m }) => escape(m.text)).join('|'), 'g');
  return text.replace(re, (hit) => `${OPEN}${mentions.findIndex((m) => m.text === hit)}${CLOSE}`);
}

/** A text token's runs: plain text, or the index of a mention. */
export function mentionRuns(text: string): (string | number)[] {
  if (!text.includes(OPEN)) return [text];
  return text
    .split(new RegExp(`${OPEN}(\\d+)${CLOSE}`))
    .map((s, i) => (i % 2 ? Number(s) : s))
    .filter((s) => s !== '');
}
