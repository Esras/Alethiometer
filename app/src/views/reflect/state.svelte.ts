// Reflect's own view state. Lives outside the component so it survives tab
// switches, and so the nudge's "Reflect now" can open today's day note.
import type { Lens, Scope } from './types';

export const rs = $state({
  scope: 'day' as Scope,
  /** Any date inside the period (YYYY-MM-DD); null = the current one. */
  period: null as string | null,
  lens: 'both' as Lens,
});

export function openToday() {
  rs.scope = 'day';
  rs.period = null;
}
