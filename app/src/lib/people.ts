// Person colours (16c) and the shapes `people.*` and Incoming return.
//
// 256 colours interpolated in OKLCH through the Gruvbox brights (red, orange,
// yellow, green, aqua, blue, purple, back to red). Hue and chroma come from
// Gruvbox; lightness is fixed at 0.65 so no one reads louder. The backend
// hands out indices (`queries/people.rs`): a People note's `color`, else
// fnv1a(email or account id) % 256; you are `people.my_color` (0, red).
// Colours are never spread apart: some people land in the grey stretch
// between green and aqua, and that's intended.

export interface Account {
  kind: 'slack' | 'email' | 'jira' | 'github';
  id: string;
}

/** Who wrote a message (`people::Who`). */
export interface Who {
  color: number;
  /** Matched to a People note. */
  known: boolean;
  me: boolean;
  person_id?: number;
  name?: string;
  /** For Link to… when not known. */
  account?: Account;
}

/** A new-person suggestion (17a). */
export interface NewPerson {
  kind: Account['kind'];
  account: string;
  name: string;
  source: string;
  signals: number;
  color: number;
}

/** A People note, as the link picker lists them (17b). */
export interface PersonHit {
  id: number;
  name: string;
  note_path: string;
  role: string | null;
  color: number;
  kinds: string[];
}

const BRIGHTS = ['#fb4934', '#fe8019', '#fabd2f', '#b8bb26', '#8ec07c', '#83a598', '#d3869b'];
const L = 0.65;

function oklch(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  const lin = (c: number) => {
    c /= 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  const [r, g, b] = [lin((n >> 16) & 255), lin((n >> 8) & 255), lin(n & 255)];
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const h = (Math.atan2(bb, a) * 180) / Math.PI;
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, Math.hypot(a, bb), h < 0 ? h + 360 : h];
}

function build(): string[] {
  const stops = [...BRIGHTS, BRIGHTS[0]].map(oklch);
  const out: string[] = [];
  for (let i = 0; i < 256; i++) {
    const t = (i * BRIGHTS.length) / 256;
    const k = Math.floor(t);
    const f = t - k;
    const [, c0, h0] = stops[k];
    const [, c1, h1] = stops[k + 1];
    const dh = ((h1 - h0 + 540) % 360) - 180;
    const h = (h0 + f * dh + 360) % 360;
    out.push(`oklch(${L.toFixed(3)} ${(c0 + f * (c1 - c0)).toFixed(3)} ${h.toFixed(1)})`);
  }
  return out;
}

export const PALETTE: readonly string[] = build();

export function personColor(index: number): string {
  return PALETTE[((index % 256) + 256) % 256];
}

/** "Slack", "Gmail"… for an account kind; email shows the address itself. */
export function accountLabel(kind: string): string {
  return { slack: 'Slack', email: 'Email', jira: 'Jira', github: 'GitHub' }[kind] ?? kind;
}

/** First letter for an avatar dot (16c/17b). */
export function initial(name: string): string {
  return (name.trim()[0] ?? '?').toUpperCase();
}
