<script lang="ts">
  // 9d: "Wrap up the day?" in the first real gap near the end of the day.
  // In-app twin of the macOS notification (the core also calls the platform's
  // notify_persistent / set_tray_dot). It stays until answered: any button or
  // × counts, it never auto-dismisses, and "Not today" is never held against you.
  // Keys while shown: Alt+R reflect now, Alt+M in 10 min, Alt+N not today, Alt+X close.
  import { call } from '../../lib/api';
  import { setTab, ui } from '../../lib/app.svelte';
  import { useKeys } from '../../lib/keys.svelte';
  import { openToday } from './state.svelte';
  import type { NudgeData } from './types';

  let data = $state<NudgeData | null>(null);
  let busy = false;

  async function poll() {
    if (busy) return;
    try {
      data = await call<NudgeData>('reflect.nudge');
    } catch {
      data = null;
    }
  }

  // Re-check on every clock tick (the shell ticks every 15 s). Not a live
  // query: asking can raise the notification and records that it did.
  $effect(() => {
    void ui.nowMs;
    poll();
  });

  async function answer(a: 'now' | 'snooze' | 'not_today' | 'dismissed') {
    busy = true;
    data = null;
    try {
      await call('reflect.nudge_answer', { answer: a });
    } finally {
      busy = false;
    }
    if (a === 'now') {
      openToday();
      setTab('reflect');
    }
  }

  useKeys((e) => {
    if (!data?.show || !e.altKey || e.metaKey || e.ctrlKey) return false;
    const map: Record<string, 'now' | 'snooze' | 'not_today' | 'dismissed'> = {
      KeyR: 'now',
      KeyM: 'snooze',
      KeyN: 'not_today',
      KeyX: 'dismissed',
    };
    const a = map[e.code];
    if (!a) return false;
    answer(a);
    return true;
  });
</script>

{#if data?.show}
  <div class="nudge popover" role="alertdialog" aria-labelledby="nudge-title">
    <div class="top">
      <i class="ph ph-compass app"></i>Alethiometer
      <span class="pin"><i class="ph-fill ph-push-pin"></i>stays until answered</span>
      <span class="at tnum">{data.at}</span>
      <button class="x" onclick={() => answer('dismissed')} title="Close (Alt+X)" aria-label="Close"
        ><i class="ph ph-x"></i></button
      >
    </div>
    <div>
      <div class="title" id="nudge-title">{data.title}</div>
      <div class="line">{data.line}</div>
    </div>
    <div class="btns">
      <button class="btn primary" onclick={() => answer('now')} title="Alt+R">Reflect now</button>
      <button class="btn" onclick={() => answer('snooze')} title="Alt+M">In 10 min</button>
      <button class="btn" onclick={() => answer('not_today')} title="Alt+N">Not today</button>
    </div>
  </div>
{/if}

<style>
  .nudge {
    width: 420px;
    border-radius: var(--radius-md);
    color: var(--tx-body);
    font-size: 1rem;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .top {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.8846rem;
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .app {
    font-size: 1.1538rem;
    color: var(--aqua);
  }
  .pin {
    display: flex;
    gap: 4px;
    align-items: center;
    margin-left: 8px;
    color: var(--yellow);
  }
  .at {
    margin-left: auto;
  }
  .x {
    margin-left: 8px;
    font-size: 1.0769rem;
    color: var(--tx-muted);
    display: flex;
  }
  .x:hover {
    color: var(--tx-title);
  }
  .title {
    font-size: 1.0769rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .line {
    color: var(--tx-muted);
    margin-top: 3px;
  }
  .btns {
    display: flex;
    gap: 6px;
  }
</style>
