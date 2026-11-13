// The transcript's size (settings.json `agents.transcript_zoom`): ⌘+ / ⌘− step it, ⌘0 resets it.

export const ZOOM_MIN = 0.5;
export const ZOOM_MAX = 2.5;
const STEPS = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5];

/** What a key does to the zoom, by its physical key (⌘= and ⌘⇧= both grow, whatever the layout types). */
export function zoomKey(e: { code: string; key: string }): 'in' | 'out' | 'reset' | null {
  if (e.code === 'Equal' || e.code === 'NumpadAdd' || e.key === '+' || e.key === '=') return 'in';
  if (e.code === 'Minus' || e.code === 'NumpadSubtract' || e.key === '-') return 'out';
  if (e.code === 'Digit0' || e.code === 'Numpad0' || e.key === '0') return 'reset';
  return null;
}

/** The next zoom, from browser-like steps; a value typed in Settings goes to the nearest step its way. */
export function stepZoom(z: number, how: 'in' | 'out' | 'reset'): number {
  if (how === 'reset') return 1;
  const next = how === 'in' ? STEPS.find((s) => s > z + 0.001) : [...STEPS].reverse().find((s) => s < z - 0.001);
  return next ?? (how === 'in' ? ZOOM_MAX : ZOOM_MIN);
}
