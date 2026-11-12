<script lang="ts">
  // New agent (project/new-agent F1–F3; decisions.md › "New agent form and the Agents tree"). Left: the saved
  // agents in the user's order (⌘1–9), and New apart at the bottom (⌘N), a one-off at each field's defaults.
  // Right: one plain form, every option an editable field, least to most specific; a field that doesn't apply
  // is removed. Picking a saved agent fills the form; edits apply to that launch only (a dot on the label, ⌘Z
  // resets the focused field, and "Save N changes" in the footer writes them back; the Task and the session's
  // name are the launch's and never saved). It always opens on New. ⌘, edits the saved list in the same modal:
  // ⌥↑↓ reorders, ⏎ renames, ⌘D duplicates, ⌫ deletes; Done (⌘⏎) writes the edits, Esc throws them away.
  // Saved agents live in settings.json (`agents.saved`).
  import { untrack } from 'svelte';
  import { call } from '../../lib/api';
  import { ui } from '../../lib/app.svelte';
  import { chord, mod } from '../../lib/keys.svelte';
  import { toast } from '../../lib/toast.svelte';
  import Modal from '../../lib/ui/Modal.svelte';
  import Dirs from './Dirs.svelte';
  import {
    LABEL,
    applies,
    base as folderName,
    blank,
    changed,
    chipLabels,
    collisions,
    imageFlag,
    merged,
    resolve,
    type Field,
  } from './newAgent';
  import { act, ag, openThread } from './state.svelte';
  import type { ConfigChoice, SavedAgent } from './types';

  let { from = [], onClose }: { from?: string[]; onClose: () => void } = $props();

  const harnesses = $derived(ag.tree?.harnesses ?? []);
  const cli = $derived(!!ag.tree?.openshell);

  let saved = $state<SavedAgent[]>([]);
  let mode = $state<'launch' | 'manage'>('launch');
  /** The saved agent picked (-1: New), and in ⌘, the one being edited. */
  let pick = $state(-1);
  let msel = $state(0);
  /** Launch: the saved agent as stored, as it filled the form, and the form. */
  let raw = $state<SavedAgent>(blank(''));
  let filled = $state<SavedAgent>(blank(''));
  let f = $state<SavedAgent>(blank(''));
  let saveChanges = $state(false);
  let saveAs = $state(false);
  let saveName = $state('');
  /** The session's name for this launch (the top line's "as"); empty is `<harness>-<4 hex>`. */
  let agentName = $state('');
  /** ⌘,: the list as it was, for Esc. */
  let before: SavedAgent[] = [];
  let focus = $state<Field | 'name' | 'list' | 'save' | ''>('task');
  let busy = $state(false);
  let tried = $state(false);
  let recent = $state<{ dir: string; at: string }[]>([]);
  /** Where New agent was opened from: what an empty Directories field means. */
  let ctx = $state<string[]>(untrack(() => [...from]));
  let taskEl = $state<HTMLTextAreaElement>();
  let listEl = $state<HTMLDivElement>();
  let renameEl = $state<HTMLInputElement>();

  /** What the right pane edits: the launch form, or the saved agent itself in ⌘,. */
  const cur = $derived(mode === 'manage' ? saved[msel] : f);
  const h = $derived(harnesses.find((x) => x.key === cur?.harness));

  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now() + (ui.info?.clock_offset_ms ?? 0)), 15_000);
    return () => clearInterval(t);
  });

  Promise.all([
    call<SavedAgent[]>('agents.saved').catch(() => [] as SavedAgent[]),
    call<{ recent: { dir: string; at: string }[] }>('agents.dirs').catch(() => ({ recent: [] })),
  ]).then(([s, d]) => {
    saved = s;
    recent = d.recent ?? [];
    if (!ctx.length && recent.length) ctx = [recent[0].dir];
    pickAgent(-1);
  });

  function pickAgent(i: number) {
    pick = i;
    raw = i >= 0 ? structuredClone($state.snapshot(saved[i])) : blank(harnesses[0]?.key ?? '');
    filled = resolve(raw, ctx, harnesses);
    f = structuredClone($state.snapshot(filled));
    saveChanges = false;
    saveAs = false;
    tried = false;
  }

  // The harness's models and thinking levels (the core caches them per harness).
  type Models = { models: { id: string; name: string }[]; current: string; thinking: ConfigChoice | null };
  let models = $state<Record<string, Models | 'loading' | 'none'>>({});
  $effect(() => {
    const k = cur?.harness;
    if (!k || untrack(() => models[k])) return;
    models[k] = 'loading';
    call<Models>('agents.models', { harness: k, cwd: untrack(() => cur?.dirs[0] ?? '') })
      .then((r) => (models[k] = r))
      .catch(() => (models[k] = 'none'));
  });
  const m = $derived.by(() => {
    const x = cur ? models[cur.harness] : undefined;
    return typeof x === 'object' ? x : null;
  });
  const modelName = (id: string) => m?.models.find((x) => x.id === id)?.name || id;
  const levelName = (v: string) =>
    (m?.thinking?.options.find((o) => o.value === v)?.name ?? v).replace(/^thinking:?\s*/i, '');

  // An existing sandbox's directories are paths inside it.
  type Box = { name: string; phase: string };
  let boxes = $state<Box[] | null>(null);
  $effect(() => {
    if (cur?.runs !== 'openshell' || !cli || untrack(() => boxes)) return;
    boxes = [];
    call<Box[]>('agents.openshell_sandboxes', { gateway: '' })
      .then((b) => (boxes = b))
      .catch(() => {});
  });
  const inBox = $derived(cur?.runs === 'openshell' && !!cur.sandbox);
  const upload = $derived(cur?.runs === 'openshell' && !cur.sandbox);

  // Whether each directory is there, and its repo (for the top line).
  type DirInfo = { dir: string; exists: boolean; name: string; project: string };
  let info = $state<Record<string, DirInfo>>({});
  $effect(() => {
    const ds = (cur?.dirs ?? []).filter((d) => !info[d]);
    if (inBox || !ds.length) return;
    const t = setTimeout(() => {
      call<DirInfo[]>('agents.dir_info', { dirs: ds })
        .then((r) => {
          for (const x of r) info[x.dir] = x;
        })
        .catch(() => {});
    }, 150);
    return () => clearTimeout(t);
  });

  const dirs = $derived(cur?.dirs ?? []);
  const missing = $derived(inBox ? [] : dirs.filter((d) => info[d] && !info[d].exists));
  const collide = $derived(upload ? collisions(dirs) : []);
  const outside = $derived(inBox ? dirs.filter((d) => !d.startsWith('/')) : []);
  const repos = $derived.by(() => {
    const out: string[] = [];
    for (const d of dirs) {
      const n = inBox ? folderName(d) : info[d]?.project || folderName(d);
      if (!out.includes(n)) out.push(n);
    }
    return out;
  });

  const diff = $derived(mode === 'launch' ? changed(raw, filled, f) : []);
  const dot = (k: Field) => diff.includes(k);

  function show(k: Field, v: SavedAgent): string {
    if (k === 'model') return modelName(v.model || m?.current || '') || 'its default';
    if (k === 'thinking') return levelName(v.thinking || m?.thinking?.current || '');
    if (k === 'runs') return v.runs === 'openshell' ? 'OpenShell' : 'Local';
    if (k === 'harness') return harnesses.find((x) => x.key === v.harness)?.label ?? v.harness;
    if (k === 'sandbox') return v.sandbox || 'New sandbox';
    if (k === 'image') return v.image || h?.sandbox_from || "the gateway's default";
    if (k === 'dirs') return chipLabels(v.dirs).join(', ') || 'none';
    return v.task ? 'its own task' : 'none';
  }

  /** The hint line: a problem first, then the focused field's note. */
  const hint = $derived.by((): { warn: boolean; text: string; reset?: boolean } | null => {
    if (!cur) return null;
    if (missing.length) return { warn: true, text: `${missing[0]} isn't there` };
    if (collide.length)
      return {
        warn: true,
        text: `${chipLabels(dirs)
          .filter((_, i) => collide.includes(dirs[i]))
          .join(' and ')} would both upload as ${folderName(collide[0])}; remove one`,
      };
    if (outside.length) return { warn: true, text: `Give ${outside[0]} inside the sandbox, from /` };
    if (cur.runs === 'openshell' && !cli) return { warn: true, text: "openshell isn't on your PATH" };
    const flag = imageFlag(cur, harnesses);
    if (flag && (focus === 'image' || focus === 'harness')) return { warn: true, text: `Image · ${flag}` };
    if (tried && !dirs.length) return { warn: true, text: 'Add a directory: it runs in the first one' };
    if (mode === 'launch' && focus && focus in LABEL && dot(focus as Field))
      return {
        warn: false,
        text: `${LABEL[focus as Field]} · ${raw.name} has ${show(focus as Field, filled)}`,
        reset: true,
      };
    return { warn: false, text: note(focus) };
  });

  function note(k: typeof focus): string {
    const H = h?.label ?? 'the harness';
    switch (k) {
      case 'name':
        return `Name · the session's, as the tree lists it; empty is ${cur?.harness || 'agent'}-<4 hex>`;
      case 'runs':
        return cur?.runs === 'openshell'
          ? 'Runs · in an OpenShell sandbox, on the default gateway'
          : 'Runs · on this machine';
      case 'sandbox':
        return cur?.sandbox
          ? 'Sandbox · one that is there; the harness must be in its image'
          : 'Sandbox · made for this agent; kept after it stops, deleted when you archive it';
      case 'harness':
        return 'Harness · ← → to pick; it signs in its own way';
      case 'model':
        return models[cur?.harness ?? ''] === 'loading'
          ? `Model · asking ${H}…`
          : m?.models.length
            ? `Model · ${m.models.length} from ${H}`
            : `Model · ${H} lists none; type one`;
      case 'thinking':
        return `Thinking · levels of ${H}'s default model`;
      case 'image':
        return `Image · with ${h?.sandbox_command?.split(' ')[0] || 'the adapter'} in it`;
      case 'dirs':
        if (inBox) return 'Directories · paths inside the sandbox; it runs in the first';
        if (upload) return 'Directories · uploaded into the sandbox; a git folder goes in as a clone of its branch';
        return mode === 'manage'
          ? 'Directories · empty: the repo New agent is opened from'
          : 'Directories · it runs in the first; ⏎ or Tab adds';
      case 'task':
        return mode === 'manage'
          ? 'Task · filled in on each launch; still editable there'
          : 'Task · sent as the first message on start; not saved';
      case 'list':
        return mode === 'manage' ? 'Saved · ⏎ renames' : 'Saved · ↑↓ to pick, or ⌘1–9';
      default:
        return '';
    }
  }

  // Changing the harness: its own model and levels.
  function setHarness(k: string) {
    if (!cur || cur.harness === k) return;
    cur.harness = k;
    cur.model = '';
    cur.thinking = '';
  }
  function chipKey(e: KeyboardEvent, opts: string[], value: string, set: (v: string) => void) {
    const i = opts.indexOf(value);
    if (e.key === 'ArrowRight' && i < opts.length - 1) set(opts[i + 1]);
    else if (e.key === 'ArrowLeft' && i > 0) set(opts[i - 1]);
    else return;
    e.preventDefault();
  }

  function manage() {
    before = structuredClone($state.snapshot(saved));
    mode = 'manage';
    msel = Math.max(0, Math.min(pick, saved.length - 1));
    focus = 'list';
    queueMicrotask(() => renameEl?.focus());
  }
  /** Done: write the list, unless a name is missing. */
  function done() {
    const list = $state.snapshot(saved);
    const unnamed = list.findIndex((a) => !a.name.trim());
    if (unnamed >= 0) {
      msel = unnamed;
      toast('A saved agent needs a name');
      queueMicrotask(() => renameEl?.focus());
      return;
    }
    if (JSON.stringify(list) !== JSON.stringify(before))
      call('agents.saved_set', { saved: list }).catch((e) => toast(e));
    back();
  }
  /** Esc: throw the edits away. */
  function discard() {
    saved = before;
    back();
  }
  function back() {
    mode = 'launch';
    pickAgent(Math.min(msel, saved.length - 1));
    queueMicrotask(() => taskEl?.focus());
  }
  function unique(name: string): string {
    let n = name;
    for (let i = 2; saved.some((a) => a.name === n); i++) n = `${name} ${i}`;
    return n;
  }
  function add() {
    saved = [...saved, blank(harnesses[0]?.key ?? '', unique('New agent'))];
    msel = saved.length - 1;
    queueMicrotask(() => (renameEl?.focus(), renameEl?.select()));
  }
  function duplicate() {
    if (!saved[msel]) return;
    const c = { ...structuredClone($state.snapshot(saved[msel])), name: unique(`${saved[msel].name} copy`) };
    saved = [...saved.slice(0, msel + 1), c, ...saved.slice(msel + 1)];
    msel++;
  }
  function remove() {
    if (!saved[msel]) return;
    saved = saved.filter((_, i) => i !== msel);
    msel = Math.max(0, Math.min(msel, saved.length - 1));
  }
  function reorder(d: 1 | -1) {
    const j = msel + d;
    if (j < 0 || j >= saved.length) return;
    const s = [...saved];
    [s[msel], s[j]] = [s[j], s[msel]];
    saved = s;
    msel = j;
  }

  async function start() {
    if (busy || mode !== 'launch') return;
    tried = true;
    const ds = f.dirs.map((d) => d.trim()).filter(Boolean);
    if (!ds.length || missing.length || collide.length || outside.length || (f.runs === 'openshell' && !cli)) return;
    if (pick < 0 && saveAs && !saveName.trim()) {
      toast('Name the saved agent, or untick Save');
      return;
    }
    busy = true;
    try {
      const sandbox =
        f.runs === 'openshell' ? { gateway: '', existing: f.sandbox, workdir: ds[0], from: f.image } : undefined;
      const r = (await act('start_local', {
        harness: f.harness,
        cwd: ds[0],
        dirs: ds.slice(1),
        name: agentName,
        task: f.task,
        model: f.model,
        thinking: f.thinking,
        sandbox,
      })) as { id: string };
      const list = $state.snapshot(saved);
      if (pick >= 0 && saveChanges && diff.length) list[pick] = merged(raw, filled, f);
      else if (pick < 0 && saveAs) list.push({ ...merged(raw, filled, f), name: saveName.trim() });
      if (list.length !== saved.length || (pick >= 0 && saveChanges && diff.length))
        call('agents.saved_set', { saved: list }).catch((e) => toast(e));
      ag.sel = `s:${r.id}`;
      openThread(`s:${r.id}`);
      onClose();
    } catch {
      // act toasts it.
    } finally {
      busy = false;
    }
  }

  const inText = () => {
    const a = document.activeElement;
    return a instanceof HTMLTextAreaElement || (a instanceof HTMLInputElement && a.type !== 'checkbox');
  };

  function onkeys(e: KeyboardEvent): boolean {
    if (mod(e) && e.key === 'Enter') return (mode === 'launch' ? start() : done(), true);
    if (mod(e) && e.key === ',') return (mode === 'launch' ? manage() : done(), true);
    if (mod(e) && e.key.toLowerCase() === 'n' && !e.shiftKey) {
      if (mode === 'launch') pickAgent(-1);
      else add();
      return true;
    }
    if (mode === 'launch' && mod(e) && /^[1-9]$/.test(e.key)) {
      const i = Number(e.key) - 1;
      if (i < saved.length) pickAgent(i);
      return true;
    }
    if (mode === 'launch' && mod(e) && e.key.toLowerCase() === 'z' && !e.shiftKey) {
      const k = focus as Field;
      if (!dot(k)) return false;
      (f as Record<Field, unknown>)[k] = structuredClone($state.snapshot(filled[k]));
      return true;
    }
    if (mode === 'manage') {
      if (e.key === 'Escape') return (discard(), true);
      if (
        e.altKey &&
        (e.key === 'ArrowUp' || e.key === 'ArrowDown') &&
        !(document.activeElement instanceof HTMLTextAreaElement)
      )
        return (reorder(e.key === 'ArrowUp' ? -1 : 1), true);
      if (mod(e) && e.key.toLowerCase() === 'd') return (duplicate(), true);
    }
    if (focus === 'list' && !e.altKey && !mod(e)) {
      const n = mode === 'manage' ? saved.length : saved.length + 1;
      const at = mode === 'manage' ? msel : pick < 0 ? saved.length : pick;
      const go = (i: number) => (mode === 'manage' ? (msel = i) : pickAgent(i === saved.length ? -1 : i));
      if (e.key === 'ArrowDown' && at < n - 1) return (go(at + 1), true);
      if (e.key === 'ArrowUp' && at > 0) return (go(at - 1), true);
      if (mode === 'manage' && (e.key === 'Backspace' || e.key === 'Delete') && !inText()) return (remove(), true);
      if (e.key === 'Enter' && !inText()) {
        if (mode === 'manage') renameEl?.focus();
        else taskEl?.focus();
        return true;
      }
    }
    return false;
  }

  $effect(() => {
    if (mode === 'launch') taskEl?.focus();
  });
  const sub = (a: SavedAgent) =>
    `${harnesses.find((x) => x.key === a.harness)?.label ?? a.harness} · ${a.runs === 'openshell' ? 'OpenShell' : 'local'}`;
  const runsOpts = ['local', 'openshell'];
</script>

<Modal
  label={mode === 'manage' ? 'Saved agents' : 'New agent'}
  onclose={onClose}
  {onkeys}
  inset="32px 0 0 0"
  isolate
  sticky
>
  <div class="new modal">
    <div class="top">
      {#if mode === 'manage'}
        <i class="ph ph-pencil-simple"></i><span class="t">Saved agents</span><span class="sub">editing</span>
      {:else}
        <i class="ph ph-user-plus"></i><span class="t">New agent</span>
      {/if}
      <kbd>Esc</kbd>
    </div>
    <div class="cols">
      <div
        class="saved"
        class:wide={mode === 'manage'}
        role="listbox"
        aria-label="Saved agents"
        tabindex="0"
        bind:this={listEl}
        onfocusin={() => (focus = 'list')}
      >
        <span class="lab label gh"
          >Saved<span class="r">{mode === 'manage' ? saved.length : `${chord('⌘,', 'Ctrl+,')} edit`}</span></span
        >
        {#each saved as a, i (i)}
          {#if mode === 'manage'}
            <div class="rc" class:cur={i === msel} role="option" aria-selected={i === msel}>
              <span class="hdl"><i class="ph ph-dots-six-vertical"></i></span>
              {#if i === msel}
                <span class="sm-in f"><input bind:this={renameEl} bind:value={saved[i].name} aria-label="Name" /></span>
                <span class="act">
                  <button class="ibs" title="Duplicate ({chord('⌘D', 'Ctrl+D')})" onclick={duplicate}
                    ><i class="ph ph-copy"></i></button
                  ><button class="ibs" title="Delete (⌫)" onclick={remove}><i class="ph ph-trash"></i></button>
                </span>
              {:else}
                <button class="a nm" tabindex="-1" onclick={() => (msel = i)}>{a.name}</button>
                <span class="b">{sub(a)}</span>
              {/if}
            </div>
          {:else}
            <button
              class="rc"
              class:cur={i === pick}
              role="option"
              aria-selected={i === pick}
              tabindex="-1"
              onclick={() => pickAgent(i)}
              ><span class="n">{i < 9 ? i + 1 : ''}</span><span class="a nm">{a.name}</span><span class="b"
                >{sub(a)}</span
              ></button
            >
          {/if}
        {/each}
        {#if mode === 'manage'}
          <button class="rc nw" tabindex="-1" onclick={add}
            ><span class="n"><i class="ph ph-plus"></i></span><span class="a">Add</span><span class="b"
              >or tick Save on a New launch</span
            ></button
          >
        {:else}
          <button
            class="rc nw"
            class:cur={pick < 0}
            role="option"
            aria-selected={pick < 0}
            tabindex="-1"
            onclick={() => pickAgent(-1)}
            ><span class="n">N</span><span class="a">New</span><span class="b">one-off; not saved</span></button
          >
        {/if}
      </div>

      {#if cur}
        <div class="frm">
          <div class="jn" onfocusin={() => (focus = 'name')}>
            <span class="t2">Joins</span>
            {#if repos.length}
              <span class="repos" title={dirs.join('\n')}>{repos.join(' & ')}</span>
            {:else}
              <span class="t2">the repos of its directories</span>
            {/if}
            {#if mode === 'launch'}
              <span class="t2">as</span>
              <span class="sm-in"
                ><input bind:value={agentName} aria-label="Name" placeholder="{cur.harness || 'agent'}-…" /></span
              >
            {/if}
          </div>

          <div class="fields">
            {#if cli || cur.runs === 'openshell'}
              <span class="lab label"
                >Runs{#if dot('runs')}<span class="md"></span>{/if}</span
              >
              <span
                class="chips"
                role="radiogroup"
                aria-label="Where it runs"
                tabindex="0"
                onfocus={() => (focus = 'runs')}
                onkeydown={(e) => chipKey(e, runsOpts, cur.runs || 'local', (v) => (cur.runs = v as 'local'))}
              >
                <button
                  class="chip"
                  class:on={cur.runs !== 'openshell'}
                  tabindex="-1"
                  onclick={() => (cur.runs = 'local')}>Local</button
                ><button
                  class="chip"
                  class:on={cur.runs === 'openshell'}
                  tabindex="-1"
                  onclick={() => (cur.runs = 'openshell')}>OpenShell</button
                >
              </span>
            {/if}

            {#if applies(cur, 'sandbox')}
              <span class="lab label"
                >Sandbox{#if dot('sandbox')}<span class="md"></span>{/if}</span
              >
              <span class="input" onfocusin={() => (focus = 'sandbox')}>
                <select bind:value={cur.sandbox} aria-label="Sandbox">
                  <option value="">New sandbox</option>
                  {#if cur.sandbox && !boxes?.some((b) => b.name === cur.sandbox)}<option value={cur.sandbox}
                      >{cur.sandbox}</option
                    >{/if}
                  {#each boxes ?? [] as b (b.name)}<option value={b.name}>{b.name} · {b.phase.toLowerCase()}</option
                    >{/each}
                </select>
              </span>
            {/if}

            <span class="lab label"
              >Harness{#if dot('harness')}<span class="md"></span>{/if}</span
            >
            <span
              class="chips"
              role="radiogroup"
              aria-label="Harness"
              tabindex="0"
              onfocus={() => (focus = 'harness')}
              onkeydown={(e) =>
                chipKey(
                  e,
                  harnesses.map((x) => x.key),
                  cur.harness,
                  setHarness,
                )}
            >
              {#each harnesses as x (x.key)}
                <button class="chip" class:on={x.key === cur.harness} tabindex="-1" onclick={() => setHarness(x.key)}
                  >{x.label}</button
                >
              {/each}
            </span>

            <span class="lab label"
              >Model{#if dot('model') || dot('thinking')}<span class="md"></span>{/if}</span
            >
            <span class="two">
              <span class="input" onfocusin={() => (focus = 'model')}>
                {#if m?.models.length}
                  <select
                    aria-label="Model"
                    value={cur.model || m.current}
                    onchange={(e) => {
                      const v = (e.currentTarget as HTMLSelectElement).value;
                      cur.model = v === m?.current ? '' : v;
                    }}
                  >
                    {#if cur.model && !m.models.some((x) => x.id === cur.model)}<option value={cur.model}
                        >{cur.model}</option
                      >{/if}
                    {#each m.models as x (x.id)}<option value={x.id}>{x.name || x.id}</option>{/each}
                  </select>
                {:else}
                  <input
                    bind:value={cur.model}
                    aria-label="Model"
                    placeholder={models[cur.harness] === 'loading'
                      ? `asking ${h?.label ?? 'the harness'}…`
                      : 'a model id'}
                  />
                {/if}
              </span>
              {#if m?.thinking}
                {@const t = m.thinking}
                <span class="input" onfocusin={() => (focus = 'thinking')}>
                  <select
                    aria-label="Thinking"
                    value={cur.thinking || t.current}
                    onchange={(e) => {
                      const v = (e.currentTarget as HTMLSelectElement).value;
                      cur.thinking = v === t.current ? '' : v;
                    }}
                  >
                    {#each t.options as o (o.value)}<option value={o.value}>{levelName(o.value)}</option>{/each}
                  </select>
                </span>
              {/if}
            </span>

            {#if applies(cur, 'image')}
              {@const flag = imageFlag(cur, harnesses)}
              <span class="lab label" class:warn={!!flag}
                >Image{#if flag}<i class="ph ph-warning" title={flag}></i>{:else if dot('image')}<span class="md"
                  ></span>{/if}</span
              >
              <span class="input" class:bad={!!flag} onfocusin={() => (focus = 'image')}
                ><input
                  aria-label="Image"
                  value={cur.image || h?.sandbox_from || ''}
                  placeholder="the gateway's default"
                  oninput={(e) => {
                    const v = (e.currentTarget as HTMLInputElement).value.trim();
                    cur.image = v === (h?.sandbox_from ?? '') ? '' : v;
                  }}
                /></span
              >
            {/if}

            <span class="lab label"
              >Directories{#if dot('dirs')}<span class="md"></span>{/if}</span
            >
            <Dirs
              bind:dirs={cur.dirs}
              recent={inBox ? [] : recent}
              bad={[...missing, ...collide]}
              placeholder={inBox ? '/sandbox/project' : mode === 'manage' ? 'empty: the repo you’re in' : 'add a path…'}
              {now}
              onfocus={() => (focus = 'dirs')}
            />

            <span class="lab label top-lab"
              >Task{#if dot('task')}<span class="md"></span>{/if}</span
            >
            {#if mode === 'launch'}
              <textarea
                bind:this={taskEl}
                bind:value={cur.task}
                rows="4"
                aria-label="Task"
                placeholder="What should it do?"
                onfocus={() => (focus = 'task')}
              ></textarea>
            {:else}
              <textarea
                bind:value={cur.task}
                rows="4"
                aria-label="Task"
                placeholder="empty: typed on each launch"
                onfocus={() => (focus = 'task')}
              ></textarea>
            {/if}
          </div>
          <div class="hint" class:warn={hint?.warn}>
            {#if hint?.text}
              {@const [head, ...rest] = hint.text.split(' · ')}
              {#if rest.length}<span>{head}</span><span class="sep">·</span><span>{rest.join(' · ')}</span>{:else}<span
                  >{head}</span
                >{/if}
              {#if hint.reset}<kbd>{chord('⌘Z', 'Ctrl+Z')}</kbd><span>reset</span>{/if}
            {/if}
          </div>
        </div>
      {:else}
        <div class="frm empty">
          <p>
            No saved agents yet. <button class="link-act" onclick={add}>Add one</button>, or tick Save on a New launch.
          </p>
        </div>
      {/if}
    </div>

    <div class="foot">
      {#if mode === 'manage'}
        <button class="btn md primary" onclick={done}>Done<kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button>
        <span class="keys">
          <kbd>⌥↑</kbd><kbd>⌥↓</kbd><span>reorder; sets {chord('⌘1–9', 'Ctrl+1–9')}</span><span class="sep">·</span><kbd
            >⏎</kbd
          ><span>rename</span><span class="sep">·</span><kbd>{chord('⌘D', 'Ctrl+D')}</kbd><span>duplicate</span><span
            class="sep">·</span
          ><kbd>⌫</kbd><span>delete</span><span class="sep">·</span><kbd>Esc</kbd><span>discard</span>
        </span>
      {:else}
        <button class="btn md primary" disabled={busy} onclick={start}>Start<kbd>{chord('⌘⏎', 'Ctrl+⏎')}</kbd></button>
        <button class="btn md" onclick={onClose}>Cancel<kbd>Esc</kbd></button>
        {#if pick >= 0 && diff.length}
          <label class="chk" onfocusin={() => (focus = 'save')}
            ><input type="checkbox" bind:checked={saveChanges} /><span
              >Save {diff.length} change{diff.length === 1 ? '' : 's'} to {raw.name}</span
            ></label
          >
        {:else if pick < 0}
          <label class="chk" onfocusin={() => (focus = 'save')}>
            <input type="checkbox" bind:checked={saveAs} />
            {#if saveAs}
              <span class="sm-in f"
                ><input
                  bind:value={saveName}
                  aria-label="Saved agent's name"
                  placeholder="its name"
                  onkeydown={(e) => e.key === 'Enter' && !mod(e) && e.preventDefault()}
                /></span
              >
              {#if saved.length < 9}<span class="becomes"
                  >becomes {chord(`⌘${saved.length + 1}`, `Ctrl+${saved.length + 1}`)}</span
                >{/if}
            {:else}
              <span>Save as a saved agent</span>
            {/if}
          </label>
        {/if}
      {/if}
    </div>
  </div>
</Modal>

<style>
  .new {
    width: min(740px, calc(100vw - 48px));
    max-height: calc(100vh - 80px);
    border-radius: var(--radius-md);
    padding: 0;
    display: flex;
    flex-direction: column;
    font-size: 1rem;
    color: var(--tx-body);
  }
  .top {
    flex: none;
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  .top > i {
    font-size: 1.231rem;
    color: var(--aqua);
  }
  .top kbd {
    margin-left: auto;
  }
  .t {
    color: var(--tx-title);
    font-weight: 500;
  }
  .sub {
    font: 0.885rem var(--font-mono);
    color: var(--tx-label);
  }
  .cols {
    display: flex;
    min-height: 0;
    flex: 1 1 auto;
    overflow: hidden;
  }

  /* Saved agents */
  .saved {
    width: 214px;
    flex: none;
    border-right: 1px solid var(--border);
    padding: 6px 0;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    outline: none;
  }
  .saved.wide {
    width: 250px;
  }
  .gh {
    padding: 8px 14px 4px;
    display: flex;
    align-items: center;
  }
  .gh .r {
    margin-left: auto;
    letter-spacing: 0;
    text-transform: none;
    font: 0.846rem var(--font-mono);
    color: var(--tx-label);
  }
  .rc {
    display: grid;
    grid-template-columns: 18px minmax(0, 1fr);
    gap: 2px 10px;
    padding: 7px 14px;
    text-align: left;
    width: 100%;
    box-sizing: border-box;
  }
  .rc:hover {
    background: var(--row-hover);
  }
  .rc .n {
    grid-row: span 2;
    font: 0.846rem/18px var(--font-mono);
    color: var(--tx-label);
    text-align: center;
    border: 1px solid var(--border-strong);
    border-radius: 2px;
    height: 18px;
    box-sizing: border-box;
  }
  .rc .a {
    color: var(--tx-title);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: left;
  }
  .rc .b {
    font: 0.846rem var(--font-mono);
    color: var(--tx-label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .rc.cur {
    background: var(--aqua-tint);
    box-shadow: inset -2px 0 0 var(--aqua);
  }
  .rc.cur .n {
    border-color: var(--aqua);
    color: var(--aqua);
  }
  .rc.nw {
    margin-top: auto;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }
  .rc.nw .n {
    border-style: dashed;
  }
  .rc.nw .a {
    color: var(--tx-body);
  }
  .rc .hdl {
    grid-row: span 2;
    color: var(--tx-disabled);
    align-self: center;
  }
  .rc .act {
    display: flex;
    gap: 4px;
  }
  .ibs {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: var(--radius-md);
    color: var(--tx-muted-2);
    font-size: 0.923rem;
  }
  .ibs:hover {
    background: var(--hover-tint);
  }
  .sm-in {
    display: inline-flex;
    align-items: center;
    height: 24px;
    box-sizing: border-box;
    padding: 0 8px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border-strong);
    background: var(--bg-app);
    min-width: 0;
  }
  .sm-in:focus-within,
  .sm-in.f {
    border-color: var(--aqua);
  }
  .sm-in input {
    background: none;
    border: 0;
    outline: 0;
    padding: 0;
    font: 0.923rem var(--font-mono);
    color: var(--tx-body);
    width: 150px;
    min-width: 0;
  }
  .sm-in input::placeholder {
    color: var(--tx-disabled);
  }

  /* The form */
  .frm {
    flex: 1;
    min-width: 0;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    overflow-y: auto;
  }
  .frm.empty {
    color: var(--tx-muted-2);
  }
  .jn {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.9615rem;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }
  .jn .t2 {
    color: var(--tx-label);
    white-space: nowrap;
  }
  .jn .repos {
    color: var(--tx-body);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .jn .sm-in {
    flex: none;
  }
  .jn .sm-in input {
    width: 120px;
  }
  .fields {
    display: grid;
    grid-template-columns: 74px minmax(0, 1fr);
    gap: 8px 10px;
    align-items: center;
  }
  .fields .lab {
    white-space: nowrap;
  }
  .fields .lab.warn {
    color: var(--orange);
  }
  .fields .lab i {
    margin-left: 5px;
  }
  .top-lab {
    align-self: start;
    padding-top: 6px;
  }
  .md {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--aqua);
    margin-left: 6px;
    vertical-align: 1px;
  }
  .chips {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    outline: none;
    border-radius: var(--radius-md);
  }
  .chips:focus-visible {
    box-shadow: 0 0 0 1px var(--aqua);
  }
  .chips .chip {
    font: 0.923rem var(--font-mono);
  }
  .input {
    display: flex;
    gap: 8px;
    align-items: center;
    min-width: 0;
  }
  .input.bad {
    border-color: var(--orange);
  }
  .input input,
  .input select {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    padding: 0;
    font: 0.9615rem var(--font-mono);
    color: var(--tx-body);
  }
  .input select option {
    background: var(--bg-panel);
  }
  .two {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 96px;
    gap: 8px;
    min-width: 0;
  }
  .two > :only-child {
    grid-column: 1 / -1;
  }
  textarea {
    display: block;
    width: 100%;
    box-sizing: border-box;
    padding: 6px 10px;
    border-radius: var(--radius-md);
    background: var(--bg-app);
    border: 1px solid var(--border-strong);
    font-size: 1rem;
    line-height: 1.5;
    color: var(--tx-body);
    resize: vertical;
    outline: none;
  }
  textarea:focus {
    border-color: var(--aqua);
  }
  textarea::placeholder {
    color: var(--tx-disabled);
  }
  .hint {
    display: flex;
    gap: 6px 10px;
    align-items: center;
    flex-wrap: wrap;
    margin: -6px 0 0 84px;
    font: 0.846rem/1.6 var(--font-mono);
    color: var(--tx-muted-2);
    min-height: 18px;
  }
  .hint .sep {
    color: var(--tx-disabled);
  }
  .hint.warn {
    color: var(--orange);
  }

  /* The header and footer keep their height when the window is short; the columns scroll instead. */
  .foot {
    flex: none;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 12px 16px;
    border-top: 1px solid var(--border);
    min-width: 0;
  }
  .chk {
    margin-left: auto;
    display: inline-flex;
    gap: 8px;
    align-items: center;
    font-size: 0.923rem;
    color: var(--tx-2);
    min-width: 0;
  }
  .chk input[type='checkbox'] {
    accent-color: var(--aqua);
    width: 14px;
    height: 14px;
    margin: 0;
  }
  .becomes {
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
    white-space: nowrap;
  }
  .keys {
    margin-left: auto;
    display: flex;
    gap: 6px;
    align-items: center;
    font: 0.846rem var(--font-mono);
    color: var(--tx-muted-2);
    white-space: nowrap;
    overflow: hidden;
  }
  .keys .sep {
    color: var(--tx-disabled);
  }
</style>
