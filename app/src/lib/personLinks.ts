// People in Obsidian notes: a `[[People/Name]]` wikilink shows as the person's pill, and `@` while
// typing in a note picks someone and writes that link (the vault's own form, so Obsidian links it too).
import type { PersonHit } from './people';

/** The wikilink Obsidian resolves to a person's note: its vault path without `.md`. */
export function personLink(p: Pick<PersonHit, 'note_path'>): string {
  return `[[${p.note_path.replace(/\.md$/i, '')}]]`;
}

/** `[[target|alias]]` (with or without the brackets) → its target and the text Obsidian shows. */
export function wikilink(raw: string): { target: string; shown: string } {
  const inner = raw.replace(/^\[\[|\]\]$/g, '');
  const [target, alias] = inner.split('|');
  const t = target.replace(/#.*$/, '').trim();
  return { target: t, shown: (alias ?? t.split('/').pop() ?? t).trim() };
}

/** The person a wikilink's target is: their note's path (`People/Kim Lee`), or their name alone (`Kim Lee`). */
export function personOf<P extends Pick<PersonHit, 'note_path' | 'name'>>(target: string, people: P[]): P | undefined {
  const t = target.toLowerCase().replace(/\.md$/, '');
  return (
    people.find((p) => p.note_path.toLowerCase().replace(/\.md$/, '') === t) ??
    people.find((p) => p.name.toLowerCase() === t)
  );
}

/** An `@word` being typed just before the caret: where its `@` is and the word so far. */
export function atQuery(text: string, caret: number): { start: number; q: string } | null {
  const m = /(^|[\s(])@([\p{L}\p{N}.'-]*)$/u.exec(text.slice(0, caret));
  return m ? { start: caret - m[2].length - 1, q: m[2] } : null;
}

/** People whose name has a word starting with `q`, first-word matches first; at most `n`. */
export function matchPeople<P extends Pick<PersonHit, 'name'>>(people: P[], q: string, n = 6): P[] {
  const l = q.toLowerCase();
  const words = (p: P) => p.name.toLowerCase().split(/\s+/);
  return people
    .map((p) => ({ p, at: words(p).findIndex((w) => w.startsWith(l)) }))
    .filter((x) => x.at >= 0)
    .sort((a, b) => a.at - b.at || a.p.name.localeCompare(b.p.name))
    .slice(0, n)
    .map((x) => x.p);
}

/** `text` with the `@word` from `start` to `caret` replaced by `link`; and where the caret goes. */
export function insertLink(text: string, start: number, caret: number, link: string): { text: string; caret: number } {
  const before = text.slice(0, start) + link;
  return { text: before + text.slice(caret), caret: before.length };
}
