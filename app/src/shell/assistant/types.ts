// `ai.ask_glean`'s reply (glean/ask.rs).

export interface Chip {
  id: string;
  label: string;
  on: boolean;
}

export interface QueryRun {
  mark: string;
  text: string;
  returned: number;
  kept: number;
  error: string | null;
}

export interface Source {
  n: number | null;
  key: string;
  title: string;
  url: string | null;
  icon: string;
  system: string;
  container: string;
  queries: number[];
  reason: string;
  kept: boolean;
  item_id: number | null;
  topic: { id: number; name: string; category: string } | null;
}

export interface Person {
  name: string;
  role: string;
  why: string;
  evidence: { n: number; icon: string; text: string }[];
  color: number;
  known: boolean;
}

export interface Reply {
  answer: string;
  queries: QueryRun[];
  sources: Source[];
  people: Person[];
  chips: Chip[];
  glean: { state: string; message?: string };
  route: string;
  results: number;
}

/** Query marks use the Gruvbox blue, orange and green; the local route is aqua. */
export const MARK_COLORS = ['#83a598', '#fe8019', '#b8bb26', '#d3869b', '#fabd2f', '#fb4934'];
export function markColor(i: number, q: QueryRun): string {
  return q.mark === '◇' ? 'var(--aqua)' : MARK_COLORS[i % MARK_COLORS.length];
}

/** An answer cut into sentences, each with its text runs, citation numbers and @{Person} chips. */
export type Piece = { t: 'text'; s: string } | { t: 'cite'; n: number } | { t: 'person'; name: string };
export interface Sentence {
  pieces: Piece[];
  cites: number[];
}

/** A sentence's claims: the text up to each run of citations, with those citations. */
export function claims(s: Sentence): { pieces: Piece[]; cites: number[] }[] {
  const out: { pieces: Piece[]; cites: number[] }[] = [];
  let cur: { pieces: Piece[]; cites: number[] } = { pieces: [], cites: [] };
  s.pieces.forEach((p, i) => {
    cur.pieces.push(p);
    if (p.t === 'cite') {
      cur.cites.push(p.n);
      const next = s.pieces[i + 1];
      // A run of citations ([3] [4]) closes one claim.
      if (!(next && (next.t === 'cite' || (next.t === 'text' && !next.s.trim() && s.pieces[i + 2]?.t === 'cite')))) {
        out.push(cur);
        cur = { pieces: [], cites: [] };
      }
    }
  });
  if (cur.pieces.length) out.push(cur);
  return out;
}

export function sentences(answer: string): Sentence[] {
  const out: Sentence[] = [];
  // Split after . ! ? (and any citations that follow them).
  const parts = answer.match(/[^.!?]+(?:[.!?]+(?:\s*\[\d+\])*|$)/g) ?? [answer];
  for (const part of parts) {
    if (!part.trim()) continue;
    const pieces: Piece[] = [];
    const cites: number[] = [];
    const re = /\[(\d+)\]|@\{([^}]+)\}/g;
    let last = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(part))) {
      if (m.index > last) pieces.push({ t: 'text', s: part.slice(last, m.index) });
      if (m[1]) {
        const n = Number(m[1]);
        pieces.push({ t: 'cite', n });
        cites.push(n);
      } else pieces.push({ t: 'person', name: m[2] });
      last = re.lastIndex;
    }
    if (last < part.length) pieces.push({ t: 'text', s: part.slice(last) });
    out.push({ pieces, cites });
  }
  return out;
}
