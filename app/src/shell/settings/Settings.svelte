<script lang="ts">
  // Settings (26a General, 27a Sources, 26c AI, 25d find): a modal over the
  // app, from the cog or ⌘,. Every change writes settings.json and applies
  // live; the header says how ("Saved · live", "Saved · Slack reconnected").
  import { call } from '../../lib/api';
  import { refresh, ui } from '../../lib/app.svelte';
  import { chord, isTyping, mod, pointerMoved } from '../../lib/keys.svelte';
  import AppIcon from '../../lib/ui/AppIcon.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import { listNav, moveSel } from '../../lib/ui/listNav';
  import Modal from '../../lib/ui/Modal.svelte';
  import CategoryManager from '../../views/topics/CategoryManager.svelte';
  import {
    find,
    get,
    GROUP_LABELS,
    GROUP_ORDER,
    embedLine,
    loadPage,
    patch,
    SECTIONS,
    shortDate,
    sourceState,
    sourceValue,
    type Edit,
    type Field,
    type Page,
    type Secret,
    type Section,
    type Seg,
    type SourceRow,
    type State,
  } from './model';
  import { DEFAULT_LOOK } from '../../views/agents/look';
  import type { AgentsLook } from '../../views/agents/types';
  import AgentsPreview from './AgentsPreview.svelte';
  import Row from './Row.svelte';
  import Segmented from '../../lib/ui/Segmented.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import SlackReview from './SlackReview.svelte';


  let page = $state<Page | null>(null);
  let section = $state<Section>('general');
  /** On Sources: a source's index, or 'add'. */
  let src = $state<number | 'add'>(0);
  let focus = $state(0);
  let editing = $state(false);
  let status = $state<State | null>(null);
  let err = $state<string | null>(null);
  let fonts = $state<{ name: string; mono: boolean }[]>([]);
  let findQ = $state('');
  let findSel = $state(0);
  let findInput = $state<HTMLInputElement | null>(null);
  /** The secret being replaced, and what's typed (never shown back). */
  let replacing = $state<string | null>(null);
  let secretValue = $state('');
  let removeArmed = $state<number | null>(null);
  let review = $state<SourceRow | null>(null);

  async function load() {
    page = await loadPage();
    status ??= page.state.message || page.state.error ? page.state : null;
    if (page.state.error) status = page.state;
  }
  load();

  // Hand edits to settings.json land while the modal is open: reload.
  $effect(() => {
    const t = setInterval(async () => {
      const st = await call<State>('settings.state').catch(() => null);
      if (!st || !page) return;
      if (st.rev !== page.state.rev || st.error !== page.state.error) {
        status = st;
        await load();
      }
    }, 2000);
    return () => clearInterval(t);
  });

  $effect(() => {
    if (section === 'general' && !fonts.length) call<{ name: string; mono: boolean }[]>('settings.fonts').then((f) => (fonts = f));
  });

  async function apply(edits: Edit[]) {
    try {
      status = await patch(edits);
      err = null;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
    await load();
    refresh();
  }

  function close() {
    ui.settingsOpen = false;
  }

  // ------------------------------------------------------------- rows

  type Item =
    | { t: 'field'; group: string; field: Field; path: Seg[]; value: unknown; pair?: unknown; extra?: string }
    | { t: 'enabled'; group: string; source: SourceRow }
    | { t: 'secret'; group: string; secret: Secret; source?: SourceRow }
    | { t: 'tier'; group: string; tier: 'small' | 'large' }
    | { t: 'price'; group: string; model: string }
    | { t: 'kind'; group: string; kind: string; label: string };

  const source = $derived(page && section === 'sources' && typeof src === 'number' ? page.sources[src] : undefined);

  function usageFor(model: string): string {
    const u = page?.usage.find((x) => x.model === model);
    if (!u) return '0 calls';
    const k = u.tokens >= 1000 ? `${Math.round(u.tokens / 1000)}k` : String(u.tokens);
    return `${u.calls} calls · ${k}`;
  }

  // j/k walks rows in the order they're drawn: grouped, groups in GROUP_ORDER.
  const rank = (n: string) => (GROUP_ORDER.includes(n) ? GROUP_ORDER.indexOf(n) : GROUP_ORDER.length);
  const items = $derived.by<Item[]>(() =>
    rawItems()
      .map((it, i) => ({ it, i }))
      .sort((a, b) => rank(a.it.group) - rank(b.it.group) || a.i - b.i)
      .map((x) => x.it),
  );

  function rawItems(): Item[] {
    if (!page) return [];
    const p = page;
    const out: Item[] = [];
    if (section === 'sources') {
      if (src === 'add' || !source) {
        for (const k of p.kinds) out.push({ t: 'kind', group: 'Add a source', kind: k.kind, label: k.label });
        return out;
      }
      const s = source;
      const kind = p.kinds.find((k) => k.kind === s.kind);
      out.push({ t: 'enabled', group: '', source: s });
      const fields = (kind?.fields ?? []).filter((f) => !f.opts?.secret && !(s.kind === 'glean' && f.key.endsWith('poll_secs')));
      const order = ['identity', 'secrets', 'reads', 'categories', 'limits'];
      for (const g of order) {
        if (g === 'secrets') {
          for (const k of s.secrets) out.push({ t: 'secret', group: 'secrets', secret: k, source: s });
        }
        for (const f of fields.filter((f) => f.group === g && !f.opts?.advanced)) {
          const value = sourceValue(p, s.index, s.kind, f.path);
          const extra = f.widget === 'glean_calls' ? gleanUsed(s) : '';
          // The tag's meaning comes from the code's rules, not a fixed line.
          const field = f.key === 'sources[].tag' ? { ...f, desc: p.tag_rules } : f;
          out.push({ t: 'field', group: g, field, path: ['sources', s.index, ...f.path], value, extra });
        }
      }
      for (const f of (kind?.fields ?? []).filter((f) => f.opts?.advanced || f.opts?.secret)) {
        out.push({ t: 'field', group: 'Advanced', field: f, path: ['sources', s.index, ...f.path], value: sourceValue(p, s.index, s.kind, f.path) });
      }
      return out;
    }
    const fields = p.fields.filter((f) => f.section === section && (!f.opts?.dev || p.dev));
    const inUse = new Set(['small', 'large'].map((t) => String(get(p.values, ['ai', t, 'provider']) ?? '')));
    const providerGroups = new Set(p.ai_secrets.map((k) => k.group));
    for (const f of fields) {
      if (providerGroups.has(f.group) && !p.ai_secrets.some((k) => k.group === f.group && inUse.has(k.provider))) continue;
      if (f.widget === 'tier') {
        out.push({ t: 'tier', group: 'Models', tier: f.path[1] as 'small' | 'large' });
        continue;
      }
      if (f.widget === 'prices') {
        const prices = (get(p.values, ['ai', 'prices']) ?? {}) as Record<string, unknown>;
        const models = new Set([...Object.keys(prices), String(get(p.values, ['ai', 'small', 'model']) ?? ''), String(get(p.values, ['ai', 'large', 'model']) ?? '')]);
        for (const m of [...models].filter(Boolean)) out.push({ t: 'price', group: 'Prices', model: m });
        continue;
      }
      if (f.widget === 'categories' || f.widget === 'sources') continue;
      const value = get(p.values, f.path) ?? defaultFor(f);
      const pair = f.opts?.pair ? get(p.values, [f.opts.pair]) : undefined;
      out.push({ t: 'field', group: f.group, field: f, path: f.path, value, pair });
    }
    if (section === 'ai') for (const k of p.ai_secrets.filter((k) => inUse.has(k.provider))) out.push({ t: 'secret', group: k.group, secret: k });
    return out;
  }

  function defaultFor(f: Field): unknown {
    if (f.widget === 'toggle') return false;
    if (f.widget === 'list') return [];
    return undefined;
  }

  function gleanUsed(s: SourceRow): string {
    return s.used_today != null ? `${s.used_today} used today` : '';
  }

  const groups = $derived.by(() => {
    const out: { name: string; items: { item: Item; i: number }[] }[] = [];
    items.forEach((item, i) => {
      let g = out.find((x) => x.name === item.group);
      if (!g) out.push((g = { name: item.group, items: [] }));
      g.items.push({ item, i });
    });
    return out;
  });

  $effect(() => {
    if (focus >= items.length) focus = Math.max(0, items.length - 1);
  });

  function goSection(s: Section) {
    section = s;
    focus = 0;
    editing = false;
    replacing = null;
    removeArmed = null;
    if (s === 'sources') src = page?.sources.length ? (typeof src === 'number' && src < page.sources.length ? src : 0) : 'add';
  }

  function goSource(i: number | 'add') {
    section = 'sources';
    src = i;
    focus = 0;
    editing = false;
    replacing = null;
    removeArmed = null;
  }

  // ------------------------------------------------------------- actions

  function setField(it: Extract<Item, { t: 'field' }>, value: unknown, path?: Seg[]) {
    const p = path ?? it.path;
    const isSource = p[0] === 'sources';
    // Day range's second half, and the like: a sibling key at the same level.
    const full = path && !isSource ? path : path && isSource ? [...it.path.slice(0, -1), ...path] : p;
    apply([{ path: full, value }]);
  }

  async function activate(it: Item | undefined) {
    if (!it || !page) return;
    if (it.t === 'enabled') return apply([{ path: ['sources', it.source.index, 'enabled'], value: !it.source.enabled }]);
    if (it.t === 'secret') return startReplace(it.secret);
    if (it.t === 'kind') return addSource(it.kind, it.label);
    if (it.t === 'tier') return apply([{ path: ['ai', it.tier, 'enabled'], value: get(page.values, ['ai', it.tier, 'enabled']) === false }]);
    if (it.t === 'field') {
      if (it.field.opts?.readonly) return;
      if (it.field.widget === 'toggle') return setField(it, !it.value);
      if (it.field.widget === 'slack_channels' && source) return (review = source);
      editing = true;
      return;
    }
    editing = true;
  }

  function startReplace(s: Secret) {
    if (s.oauth) {
      err = 'Sign-in tokens are written by Connect in the sync status popover.';
      return;
    }
    replacing = s.name;
    secretValue = '';
  }

  async function saveSecret() {
    if (!replacing || !secretValue.trim()) {
      replacing = null;
      return;
    }
    try {
      status = await call<State>('settings.secret_set', { name: replacing, value: secretValue });
      err = null;
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
    secretValue = '';
    replacing = null;
    await load();
  }

  async function addSource(kind: string, label: string) {
    if (!page) return;
    const taken = new Set(page.sources.filter((s) => s.kind === kind).map((s) => s.name));
    let name = label.replace(' · personal', '');
    for (let n = 2; taken.has(name); n++) name = `${label} ${n}`;
    const value = { kind, tag: kind === 'protoncal' ? 'personal' : 'work', name, enabled: true, poll_secs: kind === 'protoncal' ? 900 : 300, config: {} };
    const index = page.sources.length;
    await apply([{ op: 'push', path: ['sources'], value }]);
    if (!err) goSource(index);
  }

  async function removeSource(i: number) {
    if (removeArmed !== i) {
      removeArmed = i;
      return;
    }
    removeArmed = null;
    await apply([{ op: 'remove', path: ['sources', i] }]);
    goSource(page?.sources.length ? Math.max(0, i - 1) : 'add');
  }

  async function onlyThese(s: SourceRow) {
    try {
      const chans = await call<{ id: string; name: string }[]>('settings.slack_channels', { source: s.status?.id });
      await apply([{ path: ['sources', s.index, 'config', 'channels'], value: chans.map((c) => `#${c.name}`) }]);
    } catch (e) {
      err = String((e as Error).message ?? e);
    }
  }

  /** ←/→ on a swatch or segmented row: the next preset. */
  function pickNext(dir: number): boolean {
    const it = items[focus];
    if (it?.t !== 'field') return false;
    const list = it.field.widget === 'swatches' ? (it.field.opts?.presets ?? []) : it.field.widget === 'segmented' ? (it.field.opts?.enum ?? []) : null;
    if (!list?.length) return false;
    const i = list.findIndex((x) => x.toLowerCase() === String(it.value ?? '').toLowerCase());
    const next = list[(i + dir + list.length) % list.length];
    setField(it, next);
    return true;
  }

  /** ⌫ on an agents.* row: back to its default. */
  function resetAgents(): boolean {
    const it = items[focus];
    if (it?.t !== 'field' || it.path[0] !== 'agents') return false;
    const d = get(DEFAULT_LOOK, it.path.slice(1));
    if (d === undefined) return false;
    setField(it, d);
    return true;
  }

  function resetColors() {
    apply([{ path: ['agents', 'colors'], value: { ...DEFAULT_LOOK.colors } }]);
  }

  function step(dir: number) {
    const it = items[focus];
    if (it?.t !== 'field') return;
    const f = it.field;
    if (!['number', 'scale', 'hour', 'glean_calls', 'person_color'].includes(f.widget)) return;
    const st = f.opts?.step ?? (f.widget === 'scale' ? 0.05 : 1);
    const dec = String(st).split('.')[1]?.length ?? 0;
    let next = Number((Number(it.value ?? 0) + dir * st).toFixed(dec));
    if (f.opts?.min !== undefined) next = Math.max(f.opts.min, next);
    if (f.opts?.max !== undefined) next = Math.min(f.opts.max, next);
    if (f.widget === 'hour') next = Math.min(next, Number(it.pair ?? 24) - 1);
    if (next !== it.value) setField(it, next);
  }

  // ------------------------------------------------------------- find (25d)

  const results = $derived(page && findQ.trim() ? find(page, findQ) : []);

  function jump(r: (typeof results)[number]) {
    findQ = '';
    findInput?.blur();
    if (r.source !== null) goSource(r.source);
    else goSection(r.section);
    // Focus the row once the page has its items.
    queueMicrotask(() => {
      const i = items.findIndex((it) => it.t === 'field' && JSON.stringify(it.path) === JSON.stringify(r.path));
      if (i >= 0) focus = i;
    });
  }

  // ------------------------------------------------------------- keys

  const sectionIds = SECTIONS.map((s) => s.id);

  // Settings' own keys; Modal closes on the Escape this leaves, and traps Tab
  // when it's typed in a field.
  function onkeys(e: KeyboardEvent): boolean {
    if (e.code === 'Comma' && mod(e)) {
      if (e.shiftKey) call('settings.open_file').catch((x) => (err = String(x.message ?? x)));
      else close();
      return true;
    }
    if (isTyping(e)) {
      // Fields handle their own keys; esc leaves the field, not the modal.
      if (e.key === 'Escape') {
        (e.target as HTMLElement).blur();
        if (findQ) findQ = '';
        return true;
      }
      if (e.target === findInput) {
        const d = listNav(e);
        if (d) findSel = moveSel(findSel, d, results.length);
        else if (e.key === 'Enter' && results[findSel]) jump(results[findSel]);
        else return false;
        return true;
      }
      return false;
    }
    if (e.key === 'Escape') {
      if (replacing) replacing = null;
      else if (editing) editing = false;
      else return false;
      return true;
    }
    if (e.metaKey || e.ctrlKey) return false;
    if (e.altKey && (e.code === 'KeyJ' || e.code === 'KeyK') && page?.sources.length) {
      const n = page.sources.length;
      const cur = section === 'sources' && typeof src === 'number' ? src : -1;
      goSource(e.code === 'KeyJ' ? (cur + 1) % n : (cur - 1 + n) % n);
      return true;
    }
    const d = listNav(e, { bare: true });
    if (d) focus = moveSel(focus, d, items.length);
    else if (/^[1-7]$/.test(e.key)) goSection(sectionIds[Number(e.key) - 1]);
    else if ((e.key === 'ArrowLeft' || e.key === 'ArrowRight') && pickNext(e.key === 'ArrowRight' ? 1 : -1)) return true;
    else if (e.key === 'Backspace' && resetAgents()) return true;
    else if (e.key === 'Tab') {
      const i = sectionIds.indexOf(section);
      goSection(sectionIds[(i + (e.shiftKey ? sectionIds.length - 1 : 1)) % sectionIds.length]);
    } else if (e.key === 'Enter') activate(items[focus]);
    else if (e.key === ' ') {
      const it = items[focus];
      if (it?.t === 'enabled' || it?.t === 'tier' || (it?.t === 'field' && it.field.widget === 'toggle')) activate(it);
    } else if (e.key === '-' || e.key === '−') step(-1);
    else if (e.key === '+' || e.key === '=') step(1);
    else if (e.key === '/') {
      findInput?.focus();
    } else if (e.key === 'r' && (source || items[focus]?.t === 'secret')) {
      const it = items[focus];
      const s = it?.t === 'secret' ? it.secret : source?.secrets.find((k) => !k.oauth);
      if (s) startReplace(s);
    } else if (e.key === 'a') goSource('add');
    else if (e.key === 'X' && source) removeSource(source.index);
    else if (e.key === 's' && source?.status) call('sync.now', { source_id: source.status.id }).then(load, (x) => (err = String(x.message ?? x)));
    else return false;
    return true;
  }

  const counts = $derived<Record<string, number | undefined>>({
    categories: ui.info ? Object.keys(ui.info.categories).length : undefined,
    sources: page?.sources.length,
  });
  const attention = $derived(page?.sources.some((s) => s.attention) ?? false);
  const sectionInfo = $derived(SECTIONS.find((s) => s.id === section)!);
</script>

<Modal label="Settings" onclose={close} {onkeys}>
<div class="settings modal">
  <header>
    <i class="ph ph-gear-six cog"></i><span class="title">Settings</span>
    {#if err}
      <span class="msg warn"><i class="ph ph-warning"></i>{err}</span>
    {:else if status?.error}
      <span class="msg warn"><i class="ph ph-warning"></i>settings.json: {status.error}</span>
    {:else if status?.message}
      <span class="msg ok"><i class="ph ph-check"></i>{status.message}</span>
    {/if}
    <button class="esc" onclick={close}><kbd>Esc</kbd> <i class="ph ph-x"></i></button>
  </header>
  <div class="body">
    <nav>
      <FilterField class="find" bind:value={findQ} bind:el={findInput} placeholder="Find a setting" oninput={() => (findSel = 0)}>
        <kbd>/</kbd>
      </FilterField>
      {#each SECTIONS as s, i}
        <button class="sec" class:on={section === s.id && !findQ} onclick={() => goSection(s.id)}>
          <i class="ph ph-{s.icon}"></i><span>{s.label}</span>
          {#if s.id === 'sources' && attention && section !== 'sources'}<span class="dot"></span>{/if}
          <span class="navcount">{counts[s.id] ?? ''}</span><kbd>{i + 1}</kbd>
        </button>
        {#if s.id === 'sources' && section === 'sources' && page}
          {#each page.sources as so}
            <button class="sub" class:on={src === so.index && !findQ} onclick={() => goSource(so.index)}>
              <AppIcon source={so.kind} size={14} /><span class="ellipsis">{so.name}</span>
              {#if so.attention}<span class="dot"></span>{/if}
            </button>
          {/each}
          <button class="sub add" class:on={src === 'add' && !findQ} onclick={() => goSource('add')}><i class="ph ph-plus"></i><span>Add source</span><kbd>a</kbd></button>
        {/if}
      {/each}
      <span class="grow"></span>
      <button class="file" onclick={() => call('settings.open_file').catch((x) => (err = String(x.message ?? x)))}>
        <i class="ph ph-file-text"></i>Open settings.json<kbd>{chord('⌘⇧,', 'Ctrl+⇧,')}</kbd>
      </button>
    </nav>

    <section class="main scroll">
      {#if !page}
        <div class="loading">Loading…</div>
      {:else if findQ.trim()}
        <div class="results panel col">
          {#each results as r, i}
            <button class="result" class:sel={i === findSel} onclick={() => jump(r)} onmousemove={(e) => pointerMoved(e) && (findSel = i)}>
              <span class="rl">{r.field.label}</span>
              <span class="rk mono">{r.field.key}</span>
              <span class="rw">{r.where}</span>
            </button>
          {:else}
            <div class="empty-row">Nothing matches “{findQ}”. Find looks at labels, descriptions and keys.</div>
          {/each}
        </div>
      {:else}
        {#if section === 'sources' && source}
          {@const st = sourceState(source, ui.nowMs)}
          <div class="crumb">Sources</div>
          <div class="src-head">
            <AppIcon source={source.kind} size={22} />
            <h2>{source.name}</h2>
            <span class="state {st.tone}"><span class="d"></span>{st.label}</span>
            <span class="grow"></span>
            {#if removeArmed === source.index}<span class="armed"><kbd>⇧X</kbd> again removes {source.name} (its items stay)</span>{/if}
          </div>
        {:else if section === 'sources'}
          <h2>Add source</h2>
          <p class="blurb">Pick a kind. It starts enabled with defaults; fill in its page next.</p>
        {:else}
          <h2>{sectionInfo.label}</h2>
          {#if sectionInfo.blurb}<p class="blurb">{sectionInfo.blurb}</p>{/if}
        {/if}

        {#if section === 'categories'}
          <div class="card cats panel"><CategoryManager /></div>
        {:else if section === 'agents'}
          <AgentsPreview look={(get(page.values, ['agents']) as AgentsLook) ?? DEFAULT_LOOK} />
        {:else if section === 'developer' && !page.dev}
          <p class="blurb">Developer settings only show in dev builds.</p>
        {/if}

        {#each groups as g}
          {#if g.name}
            <div class="label glabel">
              {GROUP_LABELS[g.name] ?? g.name}
              {#if g.name === 'secrets'}<span class="gnote">secrets.json (0600), never shown</span>{/if}
              {#if g.name === 'Prices'}<span class="gnote">per million tokens, USD</span>{/if}
              {#if g.name === 'Message colors'}<span class="gnote">Box, icon and legend for each kind</span><button class="reset" onclick={resetColors}>Reset to default</button>{/if}
            </div>
          {/if}
          <div class="card panel" class:table={g.name === 'Models' || g.name === 'Prices'}>
            {#if g.name === 'Models'}
              <div class="label thead"><span>Tier</span><span>Provider</span><span>Model</span><span>Effort</span><span class="r">Today</span></div>
            {/if}
            {#each g.items as { item, i } (i)}
              {#if item.t === 'field'}
                <Row
                  field={item.field}
                  value={item.value}
                  pair={item.pair}
                  extra={item.field.widget === 'slack_channels' && item.value === 'all' ? 'every channel you’re in' : item.extra}
                  {fonts}
                  focused={i === focus}
                  editing={i === focus && editing}
                  onfocus={() => (focus = i)}
                  onedit={(on) => {
                    focus = i;
                    editing = on;
                  }}
                  onset={(v, path) => setField(item, v, path)}
                  onaction={(a) => {
                    if (a === 'review' && source) review = source;
                    if (a === 'only' && source) onlyThese(source);
                  }} />
              {:else if item.t === 'enabled'}
                <div class="row" class:focused={i === focus} onmousedown={() => (focus = i)} role="presentation">
                  <span class="lbl">Enabled</span>
                  <Switch on={item.source.enabled} label="Enabled" onclick={() => activate(item)} />
                </div>
              {:else if item.t === 'secret'}
                <div class="row" class:focused={i === focus} onmousedown={() => (focus = i)} role="presentation">
                  <div class="text">
                    <div class="lbl"><i class="ph ph-key"></i> <span class="mono">{item.secret.name}</span></div>
                    <div class="desc">
                      {#if item.secret.set}set{item.secret.set_at ? ` ${shortDate(item.secret.set_at)}` : ''}{:else if item.secret.oauth}not connected{:else}not set{item.secret.optional ? ' · optional' : ''}{/if}{item.secret.note ? ` · ${item.secret.note}` : ''}
                    </div>
                  </div>
                  {#if replacing === item.secret.name}
                    <!-- svelte-ignore a11y_autofocus -->
                    <input
                      class="input secret-input mono"
                      type="password"
                      autocomplete="off"
                      autofocus
                      bind:value={secretValue}
                      placeholder="paste the new value"
                      onkeydown={(e) => {
                        if (e.key === 'Enter') saveSecret();
                        if (e.key === 'Escape') replacing = null;
                        e.stopPropagation();
                      }}
                      onblur={() => (replacing = null)} />
                  {:else if item.secret.oauth}
                    <span class="muted">Connect in the sync status</span>
                  {:else}
                    <button class="btn" onclick={() => startReplace(item.secret)}>{item.secret.set ? 'Replace…' : 'Set…'} <kbd>r</kbd></button>
                  {/if}
                </div>
              {:else if item.t === 'tier'}
                {@const tier = item.tier}
                {@const model = String(get(page.values, ['ai', tier, 'model']) ?? '')}
                {@const effort = String(get(page.values, ['ai', tier, 'effort']) ?? '')}
                {@const on = get(page.values, ['ai', tier, 'enabled']) !== false}
                {@const provider = String(get(page.values, ['ai', tier, 'provider']) ?? '')}
                {@const allowed = (get(page.values, ['ai', 'allowed_providers']) ?? []) as string[]}
                <div class="trow" class:focused={i === focus} onmousedown={() => (focus = i)} role="presentation">
                  <span class="lbl tier">
                    <Switch {on} label="{tier === 'small' ? 'Small' : 'Large'} tier" onclick={() => activate(item)} />
                    {tier === 'small' ? 'Small' : 'Large'}
                  </span>
                  <select class="input text-input mono" value={provider} onchange={(e) => apply([{ path: ['ai', tier, 'provider'], value: e.currentTarget.value }])}>
                    {#each allowed.includes(provider) ? allowed : [provider, ...allowed] as p}<option value={p}>{p}</option>{/each}
                  </select>
                  <input
                    class="input text-input mono"
                    value={model}
                    list="ai-models"
                    onblur={(e) => e.currentTarget.value !== model && apply([{ path: ['ai', tier, 'model'], value: e.currentTarget.value.trim() }])}
                    onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()} />
                  <Segmented
                    options={[['', 'default'], ['low', 'low'], ['medium', 'med'], ['high', 'high']].map(([v, l]) => ({ value: v, label: l }))}
                    value={effort}
                    onpick={(v) => apply([{ path: ['ai', tier, 'effort'], value: v }])}
                  />
                  <span class="r muted tnum">{model ? usageFor(model) : '—'}</span>
                </div>
              {:else if item.t === 'price'}
                {@const pr = get(page.values, ['ai', 'prices', item.model]) as { input: number; output: number } | undefined}
                <div class="prow" class:focused={i === focus} onmousedown={() => (focus = i)} role="presentation">
                  <span class="mono model">{item.model}</span>
                  {#if !pr}<span class="muted">not set</span>{/if}
                  <label class="money">$<input
                      class="input sm tnum"
                      value={pr?.input ?? ''}
                      placeholder="—"
                      onblur={(e) => {
                        const v = e.currentTarget.value.trim();
                        if (v === String(pr?.input ?? '')) return;
                        if (!v && !pr) return;
                        apply([v ? { path: ['ai', 'prices', item.model], value: { input: Number(v), output: pr?.output ?? 0 } } : { op: 'remove', path: ['ai', 'prices', item.model] }]);
                      }}
                      onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()} /> in</label>
                  <label class="money">$<input
                      class="input sm tnum"
                      value={pr?.output ?? ''}
                      placeholder="—"
                      onblur={(e) => {
                        const v = e.currentTarget.value.trim();
                        if (v === String(pr?.output ?? '')) return;
                        if (!v && !pr) return;
                        apply([v ? { path: ['ai', 'prices', item.model], value: { input: pr?.input ?? 0, output: Number(v) } } : { op: 'remove', path: ['ai', 'prices', item.model] }]);
                      }}
                      onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()} /> out</label>
                </div>
              {:else if item.t === 'kind'}
                <button class="row kind" class:focused={i === focus} onclick={() => activate(item)} onmousemove={(e) => pointerMoved(e) && (focus = i)}>
                  <AppIcon source={item.kind} size={16} /><span class="lbl">{item.label}</span>
                  <span class="muted">{page.sources.filter((s) => s.kind === item.kind).length || ''}</span>
                </button>
              {/if}
            {/each}
          </div>
        {/each}
        {#if section === 'ai'}
          <div class="label glabel">Local embeddings<span class="gnote">run on this Mac; nothing leaves it</span></div>
          <div class="card panel">
            <div class="row">
              <div class="text">
                <div class="lbl mono">{page.embed.model}</div>
                <div class="desc" class:warnline={page.embed.status.state === 'failed'}>{embedLine(page.embed)}</div>
              </div>
              {#if page.embed.status.state === 'failed' || page.embed.status.state === 'missing'}
                <button class="btn" onclick={() => call('embed.download').then(load)}>{page.embed.status.state === 'failed' ? 'Retry' : 'Download'}</button>
              {/if}
            </div>
          </div>
        {/if}
        {#if section === 'developer' && page.dev}
          <div class="label glabel">Files</div>
          <div class="card panel"><div class="row"><span class="lbl">settings.json</span><span class="mono muted">{page.path}</span></div></div>
        {/if}
      {/if}
    </section>
  </div>
  <footer>
    <span><kbd>j</kbd> <kbd>k</kbd> setting</span>
    {#if section === 'sources'}
      <span><kbd>{chord('⌥j', 'Alt+J')}</kbd> <kbd>{chord('⌥k', 'Alt+K')}</kbd> previous / next source</span>
      <span><kbd>↵</kbd> edit</span><span><kbd>Space</kbd> toggle</span><span><kbd>r</kbd> replace secret</span><span><kbd>a</kbd> add</span><span><kbd>⇧X</kbd> remove</span><span><kbd>s</kbd> sync now</span>
    {:else if section === 'ai'}
      <span><kbd>1–7</kbd> section</span><span><kbd>↵</kbd> edit</span><span><kbd>/</kbd> find</span>
    {:else if section === 'agents'}
      <span><kbd>1–7</kbd> section</span><span><kbd>←</kbd> <kbd>→</kbd> pick a color</span><span><kbd>↵</kbd> edit</span><span><kbd>Space</kbd> toggle</span><span><kbd>⌫</kbd> reset</span>
    {:else}
      <span><kbd>1–7</kbd> or <kbd>⇥</kbd> section</span><span><kbd>↵</kbd> edit</span><span><kbd>−</kbd> <kbd>+</kbd> step</span><span><kbd>Space</kbd> toggle</span><span><kbd>/</kbd> find</span>
    {/if}
    <span><kbd>Esc</kbd> close</span>
  </footer>
  <datalist id="ai-models">
    {#each page?.usage ?? [] as u}<option value={u.model}></option>{/each}
  </datalist>
</div>
</Modal>
{#if review && page}
  <SlackReview
    source={review}
    channels={sourceValue(page, review.index, 'slack', ['config', 'channels'])}
    onclose={() => (review = null)}
    onchange={(v) => apply([{ path: ['sources', review!.index, 'config', 'channels'], value: v }])} />
{/if}

<style>
  .settings {
    position: fixed;
    top: 56px;
    left: 50%;
    transform: translateX(-50%);
    width: min(1290px, calc(100vw - 80px));
    height: calc(100vh - 110px);
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-md);
    /* Its header, nav and footer run edge to edge and carry their own padding. */
    padding: 0;
    overflow: hidden;
  }
  header {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-head);
  }
  .cog {
    color: var(--aqua);
    font-size: 1.2308rem;
  }
  .title {
    color: var(--tx-title);
    font-size: 1.0769rem;
    margin-right: 14px;
  }
  .msg {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .msg.ok {
    color: var(--aqua);
  }
  .msg.warn {
    color: var(--status-attention);
  }
  .esc {
    margin-left: auto;
    color: var(--tx-label);
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  nav {
    width: 268px;
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 12px 10px;
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  nav :global(.find) {
    margin-bottom: 10px;
  }
  .sec,
  .sub {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: var(--radius-md);
    color: var(--tx-2);
    text-align: left;
  }
  .sec i {
    color: var(--tx-label);
  }
  .sec:hover,
  .sub:hover {
    background: var(--row-hover);
  }
  .sec.on,
  .sub.on {
    background: var(--bg-head);
    color: var(--tx-title);
  }
  .sec.on i {
    color: var(--aqua);
  }
  .sub {
    padding: 6px 12px 6px 38px;
    gap: 8px;
  }
  .sub.add {
    color: var(--tx-label);
  }
  .navcount {
    margin-left: auto;
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--red);
    flex: none;
  }
  .sub .dot {
    margin-left: auto;
  }
  .sec kbd,
  .sub kbd,
  .file kbd {
    margin-block: -2px;
  }
  .sub.add kbd {
    margin-left: auto;
  }
  .grow {
    flex: 1;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 4px;
    border-top: 1px solid var(--border);
    color: var(--tx-muted-2);
  }
  .file kbd {
    margin-left: auto;
  }
  .main {
    flex: 1;
    min-width: 0;
    padding: 20px 32px 40px;
    background: var(--bg-app);
  }
  h2 {
    margin: 0;
    font-size: 1.3846rem;
    font-weight: 500;
    color: var(--tx-title);
  }
  .blurb {
    margin: 6px 0 0;
    color: var(--tx-muted-2);
  }
  .crumb {
    color: var(--tx-muted-2);
    margin-bottom: 6px;
  }
  .src-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 0.9615rem;
  }
  .state .d {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }
  .state.ok {
    color: var(--aqua);
  }
  .state.warn {
    color: var(--status-attention);
  }
  .state.off {
    color: var(--tx-label);
  }
  .armed {
    color: var(--status-attention);
  }
  .glabel {
    display: flex;
    align-items: baseline;
    margin: 24px 0 8px 4px;
  }
  .reset {
    margin-left: auto;
    letter-spacing: 0;
    text-transform: none;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .reset:hover {
    color: var(--tx-2);
  }
  .gnote {
    margin-left: 8px;
    letter-spacing: 0;
    text-transform: none;
    font-size: 0.9231rem;
    color: var(--tx-muted-2);
  }
  .card {
    overflow: visible;
  }
  .card:first-of-type {
    margin-top: 18px;
  }
  .glabel + .card {
    margin-top: 0;
  }
  .card.cats {
    overflow: hidden;
    margin-top: 18px;
  }
  .card.cats :global(.mgr) {
    background: none;
    border: 0;
    padding: 14px 18px;
  }
  .card.cats :global(.cat) {
    justify-content: start;
  }
  .note {
    margin: 6px 4px 0;
    color: var(--tx-label);
    font-size: 0.9231rem;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 16px;
    width: 100%;
    padding: var(--row-pad-comfortable) 18px;
    min-height: var(--row-min-comfortable);
    border-top: 1px solid var(--border);
    text-align: left;
  }
  .row:first-child,
  .thead + .trow {
    border-top: 0;
  }
  .row.focused,
  .trow.focused,
  .prow.focused {
    background: var(--aqua-tint);
  }
  .row .text {
    flex: 1;
    min-width: 0;
  }
  .lbl {
    flex: 1;
    color: var(--tx-title);
  }
  .lbl i {
    color: var(--tx-label);
  }
  .lbl.tier {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .desc {
    margin-top: 3px;
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  .warnline {
    color: var(--status-attention);
  }
  .row.kind {
    gap: 12px;
  }
  .muted {
    color: var(--tx-label);
  }
  .secret-input {
    width: 360px;
  }
  .text-input {
    width: auto;
  }
  .thead,
  .trow {
    display: grid;
    grid-template-columns: 110px 130px minmax(0, 1fr) auto 160px;
    gap: 16px;
    align-items: center;
    padding: 10px 18px;
  }
  .thead {
    border-bottom: 1px solid var(--border);
  }
  .trow {
    border-top: 1px solid var(--border);
  }
  .r {
    text-align: right;
  }
  .prow {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 10px 18px;
    border-top: 1px solid var(--border);
  }
  .prow:first-child {
    border-top: 0;
  }
  .prow .model {
    flex: 1;
    color: var(--tx-2);
  }
  .money {
    display: flex;
    align-items: center;
    gap: 3px;
    color: var(--tx-label);
  }
  .money input {
    width: 64px;
    padding: 2px 6px;
    text-align: right;
    border-radius: var(--radius-md);
  }
  .result {
    display: grid;
    grid-template-columns: 200px minmax(0, 1fr) auto;
    gap: 16px;
    padding: 10px 18px;
    text-align: left;
    border-top: 1px solid var(--border);
  }
  .result:first-child {
    border-top: 0;
  }
  .result.sel {
    background: var(--aqua-tint);
  }
  .rl {
    color: var(--tx-title);
  }
  .rk {
    color: var(--tx-label);
    font-size: 0.9231rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rw {
    color: var(--tx-muted-2);
  }
  .empty-row,
  .loading {
    padding: 14px 18px;
  }
  .empty-row {
    font: inherit;
  }
  .loading {
    color: var(--tx-label);
  }
  footer {
    flex: none;
    display: flex;
    gap: 18px;
    padding: 9px 20px;
    border-top: 1px solid var(--border);
    background: var(--bg-head);
    color: var(--tx-muted-2);
    font-size: 0.9231rem;
  }
  footer span:last-child {
    color: var(--tx-2);
  }
</style>
