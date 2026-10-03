// Colours for the Agents tab, from settings.json `agents.*` (Settings › Agents).
// Every box is its kind's colour at the chosen strength; agent names take a
// palette colour unique within their group.

import type { AgentsLook, Session } from './types';

export const DEFAULT_LOOK: AgentsLook = {
  colors: { you: '#504945', tool: '#8a9a5b', message: '#9a9fd6', hub: '#fabd2f', needs_you: '#fe8019' },
  names: { color_by: 'agent', palette: ['#7fb3d5', '#b8bb26', '#b9a6e0', '#8ec07c', '#e5a8a0'], self_in_transcript: false },
  boxes: { strength: 'normal', fold_pairs: true },
};

const STRENGTH = { subtle: [0.07, 0.35], normal: [0.1, 0.45], strong: [0.14, 0.6] } as const;
const FAILED = '#fb4934';
/** "You" text; rooms are neutral. */
export const YOU = '#ebdbb2';
export const ROOM = '#d5c4a1';

function rgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '');
  const n = parseInt(h.length === 3 ? h.replace(/./g, (c) => c + c) : h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export const rgba = (hex: string, a: number) => `rgba(${rgb(hex).join(',')},${a})`;

/** The You box is a neutral: its colour is the border, the fill a shade of it over the ground. */
function mix(hex: string, a: number, over = '#1d2021'): string {
  const [r, g, b] = rgb(hex);
  const [R, G, B] = rgb(over);
  const m = (x: number, y: number) => Math.round(y + (x - y) * a);
  return `rgb(${m(r, R)},${m(g, G)},${m(b, B)})`;
}

/** CSS variables for one look: --tool, --tool-fill, --tool-border, … */
export function lookVars(look: AgentsLook): string {
  const [f, b] = STRENGTH[look.boxes.strength] ?? STRENGTH.normal;
  const c = look.colors;
  const v: Record<string, string> = {
    '--you-fill': mix(c.you, 0.4),
    '--you-border': c.you,
    '--tool': c.tool,
    '--tool-fill': rgba(c.tool, f),
    '--tool-border': rgba(c.tool, b),
    '--msg': c.message,
    '--msg-fill': rgba(c.message, f),
    '--msg-border': rgba(c.message, b),
    '--hub': c.hub,
    '--hub-fill': rgba(c.hub, f - 0.01),
    '--hub-border': rgba(c.hub, b),
    '--need': c.needs_you,
    '--need-fill': rgba(c.needs_you, f),
    '--need-border': rgba(c.needs_you, Math.min(1, b + 0.15)),
    '--need-soft': rgba(c.needs_you, f - 0.03),
    '--need-soft-border': rgba(c.needs_you, b),
    '--fail-fill': rgba(FAILED, f - 0.01),
    '--fail-border': rgba(FAILED, Math.min(1, b + 0.05)),
  };
  return Object.entries(v)
    .map(([k, x]) => `${k}:${x}`)
    .join(';');
}

function hash(s: string): number {
  let h = 0;
  for (const ch of s) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return h;
}

/** A session's name colour. */
export function agentColor(look: AgentsLook, s: Pick<Session, 'color' | 'template'> | undefined): string {
  if (!s) return YOU;
  const p = look.names.palette.length ? look.names.palette : DEFAULT_LOOK.names.palette;
  switch (look.names.color_by) {
    case 'off':
      return YOU;
    case 'template':
      return p[hash(s.template || '') % p.length];
    default:
      return p[s.color % p.length];
  }
}
