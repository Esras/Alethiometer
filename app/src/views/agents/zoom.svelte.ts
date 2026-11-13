// The transcript's zoom as shown: the stored setting, or what ⌘+ / ⌘− just set while it's being saved.
import { call } from '../../lib/api';
import { ui } from '../../lib/app.svelte';
import { toast } from '../../lib/toast.svelte';
import { stepZoom } from './zoom';

const z = $state({ pending: null as number | null });
let timer: ReturnType<typeof setTimeout> | undefined;

export const transcriptZoom = () => z.pending ?? ui.info?.agents?.transcript_zoom ?? 1;

/** Step the zoom now; it's written to settings.json once the keys stop (each write re-runs every live query). */
export function zoom(how: 'in' | 'out' | 'reset') {
  z.pending = stepZoom(transcriptZoom(), how);
  clearTimeout(timer);
  timer = setTimeout(async () => {
    const value = z.pending;
    if (value === null) return;
    try {
      await call('settings.patch', { edits: [{ path: ['agents', 'transcript_zoom'], value }] });
      if (ui.info?.agents) ui.info.agents.transcript_zoom = value;
      if (z.pending === value) z.pending = null;
    } catch (e) {
      toast(e);
    }
  }, 400);
}
