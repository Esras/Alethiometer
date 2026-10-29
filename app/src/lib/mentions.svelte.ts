import { call } from './api';
import type { PersonHit } from './people';

/** The item an `[item N]` citation opened; ItemModal (mounted once in App) shows it. */
export const viewing = $state<{ id: number | null }>({ id: null });

let people = $state<PersonHit[]>([]);
let loading = false;

function load() {
  if (loading) return;
  loading = true;
  call<PersonHit[]>('people.search', { q: '' })
    .then((r) => (people = r))
    .catch(() => (loading = false));
}

export type Part = { text: string } | { mention: string; color: number } | { items: number[] };

const ITEMS = /\[items?\s*#?\d+(?:\s*[,;&]\s*(?:items?\s*)?#?\d+)*\]/gi;
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** Splits plain text into runs, `@Person Name` (a People note's name, or its unique first name) and `[item 12]` citations. */
export function parts(text: string, cite = true): Part[] {
  if (!text.includes('@') && !(cite && /\[item/i.test(text))) return [{ text }];
  load();
  const byName = new Map<string, PersonHit>();
  const firsts = new Map<string, PersonHit[]>();
  for (const p of people) {
    byName.set(p.name.toLowerCase(), p);
    const f = p.name.split(/\s+/)[0].toLowerCase();
    firsts.set(f, [...(firsts.get(f) ?? []), p]);
  }
  const names = [...byName.keys()].sort((a, b) => b.length - a.length).map(escape);
  const at = new RegExp(`(?<![\\w@])@(?:(${names.join('|') || '(?!)'})(?![\\w])|([\\w.-]*\\w))`, 'gi');
  const out: Part[] = [];
  const plain = (s: string) => {
    let i = 0;
    for (const m of s.matchAll(at)) {
      const hit = m[1]
        ? byName.get(m[1].toLowerCase())
        : firsts.get(m[2].toLowerCase())?.length === 1
          ? firsts.get(m[2].toLowerCase())![0]
          : undefined;
      if (m.index! > i) out.push({ text: s.slice(i, m.index) });
      out.push({ mention: m[0], color: hit ? hit.color : -1 });
      i = m.index! + m[0].length;
    }
    if (i < s.length) out.push({ text: s.slice(i) });
  };
  let i = 0;
  for (const m of cite ? text.matchAll(ITEMS) : []) {
    plain(text.slice(i, m.index));
    out.push({ items: [...m[0].matchAll(/\d+/g)].map((n) => Number(n[0])) });
    i = m.index! + m[0].length;
  }
  plain(text.slice(i));
  return out;
}
