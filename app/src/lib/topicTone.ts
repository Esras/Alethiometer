// Topic colours drawn as a fill (the People index's topic boxes; design canvas
// `PeopleSort`, decisions "People tab design"). Every topic is drawn at one
// OKLCH lightness with its chroma capped, so a box's colour reads across its
// whole length and no hue outshines another: the category colours are the
// user's and their lightness is uneven, so they aren't used as they are.
// culori does the colour maths and pulls an out-of-gamut colour back into sRGB
// by lowering its chroma.
import { clampChroma, converter, formatHex } from 'culori';

const oklch = converter('oklch');

/** `color` at OKLCH lightness `l`, chroma at most `cap`, in sRGB. */
export function tone(color: string, l: number, cap: number): string {
  const c = oklch(color);
  if (!c) return color;
  return formatHex(clampChroma({ mode: 'oklch', l, c: Math.min(c.c ?? 0, cap), h: c.h }, 'oklch'));
}

/** A topic box's ground, and the fill that grows across it with the count. */
export function topicBox(color: string): { bg: string; fill: string } {
  return { bg: tone(color, 0.31, 0.03), fill: tone(color, 0.47, 0.12) };
}

/** How far a box is filled, in %: `n` against the busiest box in the list, at least 5 so it shows. */
export function fillWidth(n: number, max: number): number {
  return max > 0 ? Math.max(5, Math.round(Math.min(1, n / max) * 100)) : 5;
}

/** A strip cell's shade, 0 (none) to 4, against the busiest week in the list. */
export function shade(n: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (n <= 0 || max <= 0) return 0;
  return Math.max(1, Math.min(4, Math.ceil((n / max) * 4))) as 1 | 2 | 3 | 4;
}
