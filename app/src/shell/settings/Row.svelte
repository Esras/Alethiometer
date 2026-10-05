<script lang="ts">
  // One setting: label, one-line description, the control on the right. The
  // key path shows only while the row is focused. Changes save immediately.
  import { ui } from '../../lib/app.svelte';
  import { personColor } from '../../lib/people';
  import MapEdit from './MapEdit.svelte';
  import FontPick from './FontPick.svelte';
  import ListEdit from './ListEdit.svelte';
  import type { Field, Seg } from './model';
  import Stepper from './Stepper.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Switch from '../../lib/ui/Switch.svelte';

  let {
    field,
    value,
    focused = false,
    editing = false,
    pair = undefined,
    fonts = [],
    extra = '',
    onset,
    onedit,
    onfocus,
    onaction = () => {},
  }: {
    field: Field;
    value: unknown;
    focused?: boolean;
    editing?: boolean;
    /** The second value of a pair row (day range: day_end_hour). */
    pair?: unknown;
    fonts?: { name: string; mono: boolean }[];
    /** Extra text after the control (usage, counts). */
    extra?: string;
    onset: (value: unknown, path?: Seg[]) => void;
    onedit: (on: boolean) => void;
    onfocus: () => void;
    /** Buttons that open something (Slack channels: Review…). */
    onaction?: (what: string) => void;
  } = $props();

  const o = $derived(field.opts ?? {});
  const readonly = $derived(!!o.readonly);
  let row: HTMLElement;
  let input = $state<HTMLInputElement | HTMLTextAreaElement | null>(null);

  $effect(() => {
    if (focused) row?.scrollIntoView({ block: 'nearest' });
  });
  $effect(() => {
    if (editing && input) {
      input.focus();
      if (input instanceof HTMLInputElement) input.select();
    }
  });

  function commitText(v: string) {
    onedit(false);
    if (v !== (value ?? '')) onset(v);
  }

  const hour = (h: unknown) => `${String(Number(h ?? 0)).padStart(2, '0')}:00`;
  const presets = [1.0, 1.15, 1.3];
  const keyLabel = $derived(o.pair ? `${field.key} · ${o.pair}` : field.key);
</script>

<div class="row" class:focused class:readonly bind:this={row} onmousedown={onfocus} role="presentation">
  <div class="text">
    <div class="lbl">{field.label}</div>
    {#if focused}
      <div class="key mono">{keyLabel}</div>
    {:else if field.desc}
      <div class="desc">{field.desc}</div>
    {/if}
  </div>
  <div class="control">
    {#if field.widget === 'toggle'}
      <Switch on={!!value} label={field.label} disabled={readonly} onclick={() => onset(!value)} />
    {:else if field.widget === 'number'}
      <Stepper value={Number(value ?? 0)} step={o.step ?? 1} min={o.min} max={o.max} unit={o.unit} {editing} {onedit} onchange={(v) => onset(v)} />
    {:else if field.widget === 'scale'}
      <Segmented options={presets.map((p) => ({ value: p, label: p.toFixed(p === 1 ? 1 : 2) }))} value={Number(value)} onpick={(p) => onset(p)} />
      <Stepper value={Number(value ?? 1)} step={o.step ?? 0.05} min={o.min} max={o.max} digits={2} {editing} {onedit} onchange={(v) => onset(v)} />
    {:else if field.widget === 'hour'}
      <Stepper value={Number(value ?? 0)} step={1} min={0} max={Number(pair ?? 24) - 1} format={hour} {editing} {onedit} onchange={(v) => onset(v)} />
      <span class="muted">to</span>
      <Stepper value={Number(pair ?? 24)} step={1} min={Number(value ?? 0) + 1} max={24} format={hour} onchange={(v) => onset(v, [o.pair ?? 'day_end_hour'])} />
    {:else if field.widget === 'enum'}
      {#if (o.enum ?? []).length <= 4}
        <Segmented options={(o.enum ?? []).map((opt) => ({ value: opt, label: opt || 'default' }))} value={value ?? ''} disabled={readonly} onpick={(opt) => onset(opt)} />
      {:else}
        <span class="dropdown">
          <select value={value ?? ''} onchange={(e) => onset(e.currentTarget.value)}>
            {#each o.enum ?? [] as opt}<option value={opt}>{opt || 'default'}</option>{/each}
          </select><i class="ph ph-caret-down"></i>
        </span>
      {/if}
    {:else if field.widget === 'font'}
      <FontPick value={String(value ?? '')} {fonts} mono={!!o.mono} open={editing} onclose={() => onedit(false)} onchange={(v) => onset(v)} />
    {:else if readonly && Array.isArray(value)}
      <span class="chips">{#each value as v}<span class="chip">{v}</span>{/each}</span>
      <span class="lock"><i class="ph ph-lock-simple"></i>settings.json only</span>
    {:else if field.widget === 'list' || field.widget === 'masked_list'}
      <ListEdit items={(value as string[]) ?? []} masked={field.widget === 'masked_list'} {editing} {onedit} onchange={(v) => onset(v)} />
    {:else if field.widget === 'map' || field.widget === 'category_map'}
      <MapEdit map={(value as Record<string, string>) ?? {}} categories={field.widget === 'category_map'} {editing} {onedit} onchange={(v) => onset(v)} />
    {:else if field.widget === 'category'}
      <span class="dropdown">
        <select value={value ?? ''} onchange={(e) => onset(e.currentTarget.value)}>
          {#each Object.entries(ui.info?.categories ?? {}) as [k, c]}<option value={k}>{c.label}</option>{/each}
        </select><i class="ph ph-caret-down"></i>
      </span>
    {:else if field.widget === 'hour_range'}
      {@const r = (value as number[]) ?? [9, 17]}
      <Stepper value={r[0] ?? 9} min={0} max={(r[1] ?? 17) - 1} format={hour} onchange={(v) => onset([v, r[1] ?? 17])} />
      <span class="muted">to</span>
      <Stepper value={r[1] ?? 17} min={(r[0] ?? 9) + 1} max={24} format={hour} onchange={(v) => onset([r[0] ?? 9, v])} />
    {:else if field.widget === 'slack_channels'}
      {@const all = value === 'all'}
      {@const list = Array.isArray(value) ? (value as string[]) : []}
      <Segmented
        options={[{ value: 'all', label: "All I'm in" }, { value: 'only', label: 'Only these' }]}
        value={all ? 'all' : 'only'}
        onpick={(v) => (v === 'all' ? !all && onset('all') : all && onaction('only'))}
      />
      {#if all}
        <span class="extra">{extra || 'every channel'}</span>
      {:else}
        <span class="extra">{list.length} {list.length === 1 ? 'channel' : 'channels'}</span>
      {/if}
      <button class="btn" onclick={() => onaction('review')}>Review… <span class="k">↵</span></button>
    {:else if field.widget === 'glean_calls'}
      <Stepper value={Number(value ?? 0)} step={o.step ?? 50} min={o.min} {editing} {onedit} onchange={(v) => onset(v)} />
    {:else if field.widget === 'code'}
      <textarea
        class="mono code"
        bind:this={input}
        value={String(value ?? '')}
        rows="3"
        onblur={(e) => commitText(e.currentTarget.value)}
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            e.currentTarget.value = String(value ?? '');
            e.currentTarget.blur();
          }
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) e.currentTarget.blur();
        }}></textarea>
    {:else if field.widget === 'swatches'}
      <span class="swatches">
        {#each o.presets ?? [] as c (c)}
          <button class="sw" class:on={String(value).toLowerCase() === c.toLowerCase()} style:background={c} title={c} aria-label={c} onclick={() => onset(c)}></button>
        {/each}
      </span>
    {:else if field.widget === 'segmented'}
      <Segmented options={(o.enum ?? []).map((opt) => ({ value: opt, label: opt.charAt(0).toUpperCase() + opt.slice(1) }))} value={value ?? ''} onpick={(opt) => onset(opt)} />
    {:else if field.widget === 'palette'}
      {#if editing}
        <ListEdit items={(value as string[]) ?? []} {editing} {onedit} onchange={(v) => onset(v)} />
      {:else}
        <button class="swatches" onclick={() => onedit(true)} title="↵ edit">
          {#each (value as string[]) ?? [] as c, i (i)}<span class="sw static" style:background={c}></span>{/each}
        </button>
      {/if}
    {:else if field.widget === 'person_color'}
      <span class="swatch" style:background={personColor(Number(value ?? 0))}></span>
      <Stepper value={Number(value ?? 0)} step={1} min={0} max={255} {editing} {onedit} onchange={(v) => onset(v)} />
    {:else}
      <input
        class="text-input"
        class:mono={field.widget === 'folder' || field.key.includes('profile') || field.key.endsWith('url') || field.key.includes('model')}
        bind:this={input}
        value={String(value ?? '')}
        disabled={readonly}
        placeholder={field.key === 'obsidian_vault' ? 'built-in stand-in vault' : ''}
        onfocus={() => onedit(true)}
        onblur={(e) => commitText(e.currentTarget.value)}
        onkeydown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur();
          if (e.key === 'Escape') {
            e.currentTarget.value = String(value ?? '');
            e.currentTarget.blur();
          }
        }} />
    {/if}
    {#if o.unit && field.widget !== 'number'}<span class="unit">{o.unit}</span>{/if}
    {#if extra && field.widget !== 'slack_channels'}<span class="extra">{extra}</span>{/if}
    {#if o.restart && focused}<span class="extra">restarts the app</span>{/if}
  </div>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 52px;
    padding: 9px 18px;
    border-top: 1px solid var(--border);
    box-shadow: inset 2px 0 0 transparent;
  }
  .row:first-child {
    border-top: 0;
  }
  .row.focused {
    background: var(--aqua-tint-soft);
    box-shadow: inset 2px 0 0 var(--aqua);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .lbl {
    color: var(--tx-title);
  }
  .desc {
    margin-top: 3px;
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  .key {
    margin-top: 3px;
    color: var(--tx-label);
    font-size: 0.8462rem;
  }
  .control {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: none;
    max-width: 60%;
  }
  .text-input,
  select,
  .code {
    width: 360px;
    max-width: 100%;
    padding: 5px 10px;
    border-radius: var(--r-input);
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    outline: none;
  }
  .code {
    width: 420px;
    resize: vertical;
    font-size: 0.9231rem;
  }
  .text-input:focus,
  .code:focus,
  select:focus {
    border-color: var(--aqua);
  }
  .dropdown {
    position: relative;
    display: inline-flex;
    width: 360px;
    max-width: 100%;
  }
  .dropdown select {
    width: 100%;
    appearance: none;
    -webkit-appearance: none;
    color: var(--tx-title);
    font: inherit;
    padding-right: 30px;
  }
  .dropdown i {
    position: absolute;
    right: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--tx-label);
    pointer-events: none;
  }
  .text-input:disabled {
    color: var(--tx-label);
  }
  .unit,
  .muted,
  .extra {
    color: var(--tx-label);
    font-size: 0.9231rem;
    white-space: nowrap;
  }
  .chips {
    display: flex;
    gap: 6px;
  }
  .chip {
    padding: 2px 8px;
    border-radius: var(--r-input);
    background: var(--bg-head);
    color: var(--tx-2);
  }
  .lock {
    display: flex;
    gap: 4px;
    align-items: center;
    color: var(--tx-label);
    font-size: 0.9231rem;
  }
  .k {
    color: var(--tx-label);
    margin-left: 2px;
  }
  .swatch {
    width: 14px;
    height: 14px;
    border-radius: 50%;
  }
  .readonly .lbl {
    color: var(--tx-2);
  }
  .swatches {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 4px;
  }
  .sw {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    flex: none;
    opacity: 0.9;
  }
  .sw.on {
    opacity: 1;
    box-shadow:
      0 0 0 2px var(--bg-app),
      0 0 0 4px var(--tx-title);
  }
  .sw.static {
    width: 16px;
    height: 16px;
  }
</style>
