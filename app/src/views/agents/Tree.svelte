<script lang="ts">
  import { ui } from '../../lib/app.svelte';
  import { agentColor } from './look';
  import { popover } from '../../lib/ui/popover';
  import { chord } from '../../lib/keys.svelte';
  import Dock from '../../lib/ui/Dock.svelte';
  import GroupPicker from './GroupPicker.svelte';
  import {
    act,
    ag,
    age,
    archivable,
    askArchive,
    groupById,
    homeKey,
    look,
    moveTo,
    nodes,
    openArchived,
    openThread,
    pendingFor,
    projectKey,
    searchArchived,
    sessionById,
    turnEnded,
    type Node,
  } from './state.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import StatusMark from '../../lib/ui/StatusMark.svelte';
  import type { Group, Request, Session, Status } from './types';

  let { onNew, filterEl = $bindable() }: { onNew: () => void; filterEl?: HTMLInputElement } = $props();

  const all = $derived(nodes());
  const ARCH = new Set(['archived-head', 'archived-label', 'archived']);
  const list = $derived(all.filter((n) => !ARCH.has(n.type)));
  const archived = $derived(all.filter((n) => ARCH.has(n.type)));
  const inArchive = $derived(!!ag.sel?.startsWith('a:'));
  $effect(() => searchArchived(ag.filter));
  const requests = $derived(ag.tree?.requests ?? []);
  const counts = $derived(ag.tree?.counts ?? { running: 0, waiting: 0, failed: 0 });

  // A folded group shows its loudest status and how many sessions have it.
  const LOUD: Status[] = ['failed', 'waiting', 'running', 'idle', 'done'];
  function folded(sessions: Session[]): { status: Status; n: number } | null {
    for (const st of LOUD) {
      const n = sessions.filter((s) => s.status === st).length;
      if (n) return { status: st, n };
    }
    return null;
  }

  const hostClass = (g: Group) =>
    g.source === 'acp' ? 'local' : g.host.toLowerCase().startsWith('scion') ? 'scion' : 'other';

  function needLine(r: Request): { verb: string; verbClass: string; text: string } {
    if (r.kind === 'ask') {
      const g = ag.tree?.groups.find((g) => g.room && r.thread === `r:${g.room.id}`);
      return { verb: g?.room ? `#${g.room.name}` : 'ask', verbClass: 'msg', text: r.question };
    }
    const tool = r.tool.split(' · ')[0];
    return { verb: tool, verbClass: 'tool', text: r.command };
  }

  function tagClass(tag: string) {
    return tag === 'low' ? 'green' : 'orange';
  }

  function openRequest(r: Request) {
    ag.later.delete(r.id);
    ag.sel = r.thread;
    openThread(r.thread);
    ag.zone = 'thread';
  }

  // The group a row belongs to, for the done frame (01) and the undo window (03).
  function gidOf(n: Node): string | null {
    if (n.type === 'group' || n.type === 'room') return n.group.id;
    if (n.type === 'session') return n.s.group_id;
    return null;
  }
  const nudges = $derived(ag.tree?.nudges ?? {});
  const framed = (n: Node) => {
    const g = gidOf(n);
    return !!g && !!nudges[g];
  };
  const lastInFrame = (i: number) =>
    (framed(list[i]) && gidOf(list[i + 1] ?? list[i]) !== gidOf(list[i])) || (framed(list[i]) && i === list.length - 1);
  const pendingOf = (n: Node) => pendingFor(n.key) ?? (gidOf(n) ? pendingFor(`g:${gidOf(n)}`) : undefined);
  const secs = (until: number) => Math.max(0, Math.ceil((until - now) / 1000));

  function menu(e: MouseEvent, n: Node) {
    e.preventDefault();
    ag.sel = n.key;
    ag.zone = 'tree';
    ag.menu = { key: n.key, x: e.clientX, y: e.clientY };
  }
  const menuNode = $derived(ag.menu ? list.find((n) => n.key === ag.menu!.key) : undefined);
  const menuGroup = $derived(menuNode ? groupById(gidOf(menuNode)) : undefined);
  function run(f: () => void) {
    f();
    ag.menu = null;
  }

  const fmtDay = (ts: string) =>
    ts ? new Date(ts).toLocaleDateString(undefined, { day: 'numeric', month: 'short' }) : '';

  function click(n: Node) {
    ag.sel = n.key;
    if (n.type === 'group' || n.type === 'unsorted') {
      const k = n.type === 'group' ? n.group.id : n.key;
      if (ag.folded.has(k)) ag.folded.delete(k);
      else ag.folded.add(k);
      ag.folded = new Set(ag.folded);
      return;
    }
    openThread(n.key);
    ag.zone = 'tree';
  }

  // Drag a top-level local session onto another group of its project (pointer events, as SourcesPanel's edge).
  let over = $state<string | null>(null);
  let ghost = $state<{ x: number; y: number; name: string } | null>(null);
  const root = (s: Session) => {
    let r = s;
    for (let p = sessionById(r.parent_id); p; p = sessionById(p.parent_id)) r = p;
    return r;
  };
  function dropKey(n: Node): string | null {
    const d = ag.drag;
    if (!d) return null;
    if (n.type === 'group') return n.group.source === 'acp' && projectKey(n.group) === d.project ? n.key : null;
    if (n.type === 'unsorted') return n.project === d.project ? n.key : null;
    if (n.type === 'session' && n.s.source === 'acp' && projectKey(root(n.s)) === d.project) return homeKey(root(n.s));
    return null;
  }
  function grab(e: PointerEvent, s: Session) {
    if (e.button !== 0 || s.source !== 'acp' || s.parent_id) return;
    const x0 = e.clientX;
    const y0 = e.clientY;
    const move = (m: PointerEvent) => {
      if (!ag.drag) {
        if (Math.hypot(m.clientX - x0, m.clientY - y0) < 5) return;
        ag.drag = { session: s.id, project: projectKey(s) };
        ag.menu = null;
      }
      ghost = { x: m.clientX, y: m.clientY, name: s.name };
      over = document.elementFromPoint(m.clientX, m.clientY)?.closest<HTMLElement>('[data-drop]')?.dataset.drop ?? null;
    };
    const end = (drop: boolean) => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('keydown', key, true);
      if (ag.drag && drop && over) moveTo(s.id, over.startsWith('g:') ? over.slice(2) : null);
      ag.drag = null;
      over = null;
      ghost = null;
    };
    const up = () => end(true);
    const key = (k: KeyboardEvent) => {
      if (k.key !== 'Escape' || !ag.drag) return;
      k.preventDefault();
      k.stopPropagation();
      end(false);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('keydown', key, true);
  }

  const host = $derived(ag.tree?.agentd);
  const localLive = $derived(
    (ag.tree?.sessions ?? []).filter((s) => s.source === 'acp' && !s.parent_id && s.live).length,
  );
  const anyLocal = $derived((ag.tree?.sessions ?? []).some((s) => s.source === 'acp'));
  let armed = $state<'' | 'stop' | 'restart'>('');
  function twice(what: 'stop' | 'restart') {
    if (armed !== what && localLive > 0) {
      armed = what;
      setTimeout(() => (armed = ''), 4000);
      return;
    }
    armed = '';
    act(what === 'stop' ? 'stop_all' : 'host_restart', {}).catch(() => {});
  }

  // Ages tick faster than the 15 s app clock while the tab is open.
  let now = $state(Date.now());
  $effect(() => {
    const t = setInterval(() => (now = Date.now() + (ui.info?.clock_offset_ms ?? 0)), 1000);
    return () => clearInterval(t);
  });
</script>

<div
  class="tree"
  class:focus={ag.zone === 'tree'}
  class:dragging={!!ag.drag}
  role="presentation"
  onmousedown={(e) => {
    ag.zone = 'tree';
    if (!(e.target as HTMLElement).closest('input, textarea, select'))
      (document.activeElement as HTMLElement | null)?.blur();
  }}
>
  <div class="filter-row">
    <FilterField
      class="tree-filter"
      sm
      bind:value={ag.filter}
      bind:el={filterEl}
      placeholder="Filter sessions"
      onkeydown={(e) => {
        if (e.key === 'Escape' || e.key === 'Enter') {
          (e.currentTarget as HTMLInputElement).blur();
          if (e.key === 'Escape') ag.filter = '';
          e.preventDefault();
          e.stopPropagation();
        }
      }}
    >
      <kbd>/</kbd>
    </FilterField>
    <span class="counts">
      <span title="running"><StatusMark status="running" />{counts.running}</span>
      <span title="waiting"><StatusMark status="waiting" />{counts.waiting}</span>
      <span class="red" title="failed"><i class="ph ph-x"></i>{counts.failed}</span>
    </span>
    <button class="plus" onclick={onNew} title="New session (n)" aria-label="New session"
      ><i class="ph ph-plus"></i></button
    >
  </div>

  {#each (ag.tree?.hubs ?? []).filter((h) => h.state === 'error') as h (h.source)}
    <div class="hub-err" title={h.error}><i class="ph ph-plugs"></i>Scion · {h.name}: {h.error}</div>
  {/each}

  {#if requests.length}
    <div class="needs">
      <div class="needs-head label">
        <i class="ph ph-bell-simple"></i>Needs you<span class="n">{requests.length}</span>
      </div>
      {#each requests as r (r.id)}
        {@const s = sessionById(r.session_id)}
        {@const l = needLine(r)}
        <button class="need" class:on={ag.open === r.thread} onclick={() => openRequest(r)}>
          {#if r.kind === 'ask'}<i class="ph ph-question q"></i>{:else}<StatusMark status="waiting" />{/if}
          <span class="who" style:color={agentColor(look(), s)}>{s?.name ?? '?'}</span>
          <span class="what"><span class={l.verbClass}>{l.verb}</span> {l.text}</span>
          <span class="mono-tag {r.kind === 'ask' ? 'orange' : tagClass(r.tag)}"
            >{r.kind === 'ask' ? 'question' : r.tag}</span
          >
        </button>
      {/each}
    </div>
  {/if}

  <div class="list">
    {#each list as n, idx (n.key)}
      {@const fr = framed(n)}
      {@const pend = pendingOf(n)}
      {@const dk = dropKey(n)}
      <div
        class="slot"
        class:framed={fr}
        class:ftop={fr && n.type === 'group'}
        class:pending={!!pend}
        class:drop={!!dk && dk === over}
        data-drop={dk}
      >
        {#if n.type === 'project'}
          <div class="project label">{n.label}</div>
        {:else if n.type === 'unsorted'}
          {@const f = folded(n.sessions)}
          <button class="group" class:sel={ag.sel === n.key} onclick={() => click(n)}>
            <i class="ph {n.open ? 'ph-caret-down' : 'ph-caret-right'} caret"></i>
            <span class="gname"><span class="name unsorted">Unsorted</span></span>
            <span class="gright">
              {#if !n.open && f}
                <span class="fold" class:red={f.status === 'failed'}
                  ><StatusMark status={f.status} size={7} />{f.n}</span
                >
              {/if}
            </span>
          </button>
        {:else if n.type === 'group'}
          {@const f = folded(n.sessions)}
          <button class="group" class:sel={ag.sel === n.key} onclick={() => click(n)} oncontextmenu={(e) => menu(e, n)}>
            <i class="ph {n.open ? 'ph-caret-down' : 'ph-caret-right'} caret"></i>
            <span class="gname"
              ><span class="name">{n.group.name}</span>{#if n.group.goal}<span class="goal">{n.group.goal}</span
                >{/if}</span
            >
            <span class="gright">
              {#if pend && pend.target === n.key}
                <span class="undo-note">{pend.label} in {secs(pend.until)}s · <kbd>u</kbd> undo</span>
              {:else}
                {#if !n.open && f}
                  <span class="fold" class:red={f.status === 'failed'}
                    ><StatusMark status={f.status} size={7} />{f.n}</span
                  >
                {/if}
                <span class="host {hostClass(n.group)}">{n.group.host}</span>
              {/if}
            </span>
          </button>
        {:else if n.type === 'room'}
          {@const room = n.group.room!}
          <button class="row" class:sel={ag.sel === n.key} onclick={() => click(n)} oncontextmenu={(e) => menu(e, n)}>
            <span class="mark"><i class="ph ph-hash hash"></i></span>
            <span class="name room">{room.name}</span>
            <span class="mono c2">room · {room.members}</span>
            <span class="mono msgc">{room.unread ? `${room.unread} new` : ''}</span>
            <span class="age" class:orange={requests.some((r) => r.thread === n.key)}>{age(room.active_at, now)}</span>
          </button>
        {:else if n.type === 'session'}
          {@const s = n.s}
          <button
            class="row"
            class:sel={ag.sel === n.key}
            class:sub={n.depth > 0}
            class:loose={n.unsorted}
            class:lifted={ag.drag?.session === s.id}
            onpointerdown={(e) => grab(e, s)}
            onclick={() => click(n)}
            oncontextmenu={(e) => menu(e, n)}
            style:--indent="{14 * n.depth}px"
          >
            <span class="mark">
              {#if n.depth > 0}<span class="elbow"></span>{/if}
              <StatusMark status={turnEnded(s) ? 'problem' : s.status} />
            </span>
            <span
              class="name"
              title={turnEnded(s) ? 'Ended its turn since you looked' : undefined}
              style:color={agentColor(look(), s)}
              >{s.name}{#if n.unsorted && s.cwd}<span class="cwd">{s.cwd.replace(/^\/(Users|home)\/[^/]+/, '~')}</span
                >{/if}</span
            >
            {#if pend && pend.target === n.key}
              <span class="undo-note wide">{pend.label} in {secs(pend.until)}s · <kbd>u</kbd> undo</span>
            {:else}
              <span class="mono" class:dim={s.status === 'done'}>{s.harness}</span>
              <span class="mono" class:dim={s.status === 'done'}>{s.model}</span>
              <span class="age" class:orange={s.status === 'waiting' || turnEnded(s)}>{age(s.active_at, now)}</span>
            {/if}
          </button>
          {#if n.unsorted && s.guess?.group_id}
            <div class="guess">
              Looks like <span class="where">{s.guess.label}</span>
              <button class="move" onclick={() => moveTo(s.id, s.guess!.group_id)}>Move</button>
              <button onclick={() => act('guess_dismiss', { session: s.id })}>Not this</button>
            </div>
          {/if}
        {/if}
      </div>
      {#if lastInFrame(idx)}
        {@const gid = gidOf(n)!}
        <div class="nudge" class:pending={!!pendingFor(`g:${gid}`)}>
          <i class="ph ph-check-circle"></i>
          <span class="ntext">Done · {nudges[gid]}</span>
          <button class="btn" onclick={() => askArchive(`g:${gid}`)}>Archive group</button>
          <button class="btn" onclick={() => act('nudge_snooze', { group: gid }).catch(() => {})}>Not now</button>
        </div>
      {/if}
    {:else}
      <div class="empty-pane">
        {#if ag.filter}{archived.length ? 'Nothing live matches.' : `Nothing matches "${ag.filter}".`}{:else}No sessions
          yet. <button class="link-act" onclick={onNew}>Start one</button> (n), or add a Scion Hub in Settings › Sources.{/if}
      </div>
    {/each}
    {#if archived.length}
      <div class="archive" class:lit={inArchive}>
        {#each archived as n (n.key)}
          {#if n.type === 'archived-head'}
            <div class="ahead label">
              <i class="ph ph-archive"></i>Archived<span class="acount"
                >{n.groups ? `${n.groups} group${n.groups === 1 ? '' : 's'} · ` : ''}{n.sessions} session{n.sessions ===
                1
                  ? ''
                  : 's'} · of {n.total}</span
              >
            </div>
          {:else if n.type === 'archived-label'}
            <div class="alabel" class:label={n.project} class:gl={!n.project}>{n.label}</div>
          {:else if n.type === 'archived'}
            {@const h = n.item}
            <button
              class="arow"
              class:grp={h.kind === 'group'}
              class:sel={ag.sel === n.key}
              onclick={() => ((ag.sel = n.key), openArchived(h.id))}
            >
              <span class="aname"
                >{#if h.kind === 'group'}<span class="name">{h.name}</span>{#if h.goal}<span class="goal">{h.goal}</span
                    >{/if}{:else}<span class="name">{h.name}</span>{/if}</span
              >
              <span class="mono"
                >{h.kind === 'group'
                  ? h.room
                    ? `#${h.room}`
                    : 'group'
                  : [h.harness, h.model].filter(Boolean).join(' · ')}</span
              >
              <span class="age">{fmtDay(h.at)}</span>
              {#if h.line}<span class="aline"><span class="who">{h.who}</span> {h.line}</span>{/if}
            </button>
          {/if}
        {/each}
        <div class="afoot">Newest first in each project · <kbd>⏎</kbd> opens read-only</div>
      </div>
    {/if}
  </div>

  {#if ag.moving}
    <Dock under=".tree .row.sel">
      <GroupPicker session={ag.moving} onclose={() => (ag.moving = null)} />
    </Dock>
  {/if}

  {#if ghost}
    <div class="ghost" style:left="{ghost.x + 12}px" style:top="{ghost.y + 8}px">
      <i class="ph ph-arrow-bend-down-right"></i>{ghost.name}
    </div>
  {/if}

  {#if ag.menu && menuNode}
    {@const k = ag.menu?.key ?? ''}
    {@const s = menuNode?.type === 'session' ? menuNode.s : undefined}
    {@const isGroup = menuNode?.type !== 'session'}
    {@const room = menuGroup?.room}
    {@const can = archivable(k)}
    <div
      class="menu popover flush"
      role="menu"
      style:left="{ag.menu?.x ?? 0}px"
      style:top="{ag.menu?.y ?? 0}px"
      use:popover={{ onclose: () => (ag.menu = null) }}
    >
      {#if !isGroup || room}
        <button
          role="menuitem"
          onclick={() =>
            run(() => {
              const key = isGroup && room ? `r:${room.id}` : k;
              ag.sel = key;
              openThread(key);
              ag.zone = 'thread';
            })}>{isGroup ? `Open #${room?.name}` : 'Open'}<kbd>⏎</kbd></button
        >
      {/if}
      {#if s}
        <button
          role="menuitem"
          disabled={s.status !== 'running' && !s.live}
          onclick={() => run(() => act('stop', { session: s.id }).catch(() => {}))}
          >Stop<kbd>{chord('⌘.', 'Ctrl+.')}</kbd></button
        >
      {/if}
      {#if s && s.source === 'acp' && !s.parent_id}
        <button role="menuitem" onclick={() => run(() => (ag.moving = s.id))}>Move to group…<kbd>m</kbd></button>
      {/if}
      {#if menuGroup}
        <button role="menuitem" onclick={() => run(onNew)}>New agent in {menuGroup.name}<kbd>n</kbd></button>
      {/if}
      <div class="divider"></div>
      <button role="menuitem" disabled={!can} onclick={() => run(() => askArchive(k))}
        >Archive {isGroup ? 'group' : 'session'}{can ? '…' : ''}{#if !can}<span class="off">stop it first</span
          >{/if}<kbd>a</kbd></button
      >
    </div>
  {/if}

  {#if (ag.tree?.hub_failed ?? 0) > 0}
    <div class="hostline bad">
      <i class="ph ph-warning-circle"></i><span class="err broken"
        >{ag.tree?.hub_failed} Hub or sandbox delete{ag.tree?.hub_failed === 1 ? '' : 's'} failed · retry with {chord(
          '⌘R',
          'Ctrl+R',
        )} in the sync status</span
      >
    </div>
  {/if}

  {#if host && (anyLocal || (host.state !== 'idle' && host.state !== ''))}
    <div
      class="hostline"
      class:bad={host.state === 'missing' || host.state === 'error' || host.state === 'unsupported'}
      title={host.error || host.binary}
    >
      <i class="ph {host.state === 'running' ? 'ph-cpu' : host.state === 'starting' ? 'ph-circle-notch' : 'ph-plugs'}"
      ></i>
      {#if host.state === 'running'}
        <span>Agent host · v{host.version} · {localLive} running</span>
        {#if host.stale}<span class="note">a newer build is ready</span>{/if}
      {:else if host.state === 'idle' || host.state === ''}
        <span>Agent host · starts with the next local session</span>
      {:else if host.state === 'starting'}
        <span>Agent host · starting…</span>
      {:else}
        <span class="err broken">{host.error || 'Agent host unavailable'}</span>
      {/if}
      <span class="hacts">
        {#if host.state === 'running' && localLive > 0}
          <button onclick={() => twice('stop')}
            >{armed === 'stop' ? `Stops ${localLive} · click again` : 'Stop all'}</button
          >
        {/if}
        {#if host.state === 'running' ? host.stale : host.state === 'error' || host.state === 'missing'}
          <button onclick={() => twice('restart')}
            >{armed === 'restart'
              ? `Stops ${localLive} · click again`
              : host.state === 'running'
                ? 'Restart host'
                : 'Retry'}</button
          >
        {/if}
      </span>
    </div>
  {/if}
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
  }
  .filter-row {
    flex: none;
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 12px 14px 10px;
  }
  .filter-row :global(.tree-filter) {
    flex: 1;
    width: auto;
    /* 25px tall, as before: it lines up with the 26px + button beside it. */
    padding-block: 4px;
  }
  .filter-row kbd {
    margin-left: auto;
  }
  .counts {
    display: flex;
    gap: 10px;
    align-items: center;
    font: 0.885rem var(--font-mono);
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .counts span {
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .counts .red {
    color: var(--status-broken);
  }
  .counts .red i {
    font-size: 0.846rem;
    width: 8px;
    display: inline-flex;
    justify-content: center;
  }
  .plus {
    width: 26px;
    height: 26px;
    border-radius: var(--radius-md);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--tx-muted-2);
    border: 1px solid var(--border);
    font-size: 1.077rem;
  }
  .plus:hover {
    color: var(--tx-2);
    border-color: var(--border-strong);
  }

  .needs {
    flex: none;
    margin: 0 10px 6px;
    border-radius: var(--radius-md);
    background: var(--need-soft, rgba(254, 128, 25, 0.08));
    border: 1px solid var(--need-soft-border, rgba(254, 128, 25, 0.35));
    padding: 7px 0 6px;
  }
  .needs-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px 4px;
    color: var(--need, var(--orange));
  }
  .needs-head i {
    font-size: 1rem;
  }
  .needs-head .n {
    margin-left: auto;
    font: 0.846rem var(--font-mono);
    color: inherit;
  }
  .need {
    width: 100%;
    text-align: left;
    display: grid;
    grid-template-columns: 12px 86px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    padding: 4px 12px;
  }
  .need.on,
  .need:hover {
    background: var(--need-fill, rgba(254, 128, 25, 0.12));
  }
  .need .q {
    font-size: 0.923rem;
    color: var(--need, var(--orange));
  }
  .need .who {
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .need .what {
    font: 0.923rem var(--font-mono);
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .need .what .tool {
    color: var(--tool, #8a9a5b);
  }
  .need .what .msg {
    color: var(--msg, #9a9fd6);
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 0 10px;
    display: flex;
    flex-direction: column;
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
  }
  .project {
    padding: 12px 14px 3px;
  }
  .slot:first-child .project {
    padding-top: 10px;
  }
  .slot.drop {
    background: var(--aqua-tint);
  }
  .tree.dragging {
    cursor: grabbing;
    user-select: none;
  }
  .tree.dragging .row.lifted {
    opacity: 0.5;
  }
  .ghost {
    position: fixed;
    z-index: 60;
    pointer-events: none;
    display: flex;
    gap: 6px;
    align-items: center;
    padding: 3px 8px;
    border: 1px solid var(--aqua-line);
    border-radius: var(--radius-sm);
    background: var(--bg-head);
    color: var(--tx-title);
  }
  .ghost i {
    color: var(--aqua);
  }
  .name.unsorted {
    color: var(--tx-muted);
  }
  .group {
    width: 100%;
    text-align: left;
    display: grid;
    grid-template-columns: 14px minmax(0, 1fr) auto;
    gap: 6px;
    align-items: center;
    padding: var(--row-pad-compact) 14px;
    min-height: var(--row-min-compact);
  }
  .caret {
    font-size: 0.846rem;
    color: var(--tx-label);
  }
  .gname {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .gname .name {
    color: var(--tx-body);
    font-weight: 500;
  }
  .goal {
    color: var(--tx-label);
    font-size: 0.923rem;
    margin-left: 8px;
  }
  .gright {
    display: flex;
    gap: 8px;
    align-items: center;
    font: 0.846rem var(--font-mono);
    color: var(--tx-muted-2);
  }
  .fold {
    display: flex;
    gap: 4px;
    align-items: center;
  }
  .fold.red {
    color: var(--status-broken);
  }
  .host {
    background: var(--bg-head);
    border-radius: var(--radius-sm);
    padding: 1px 6px;
    font: 0.846rem var(--font-mono);
    color: var(--tx-muted-2);
    white-space: nowrap;
  }
  .host.scion {
    color: var(--blue);
  }
  .host.other {
    color: #9a9fd6;
  }
  .row {
    width: 100%;
    text-align: left;
    display: grid;
    grid-template-columns: 30px minmax(0, 1fr) 90px 52px 30px;
    gap: 6px;
    align-items: center;
    padding: var(--row-pad-compact) 14px;
    min-height: var(--row-min-compact);
  }
  .row.sub {
    grid-template-columns: calc(30px + var(--indent)) minmax(0, 1fr) 90px 52px 30px;
  }
  .row.loose {
    padding: var(--row-pad-compact) 14px;
    min-height: var(--row-min-compact);
  }
  .group:hover,
  .row:hover {
    background: var(--row-hover);
  }
  .group.sel,
  .row.sel {
    background: var(--aqua-tint);
  }
  .tree:not(.focus) .sel {
    background: var(--bg-head);
  }
  .mark {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
  }
  .elbow {
    width: 7px;
    height: 9px;
    border-left: 1px solid var(--border-strong);
    border-bottom: 1px solid var(--border-strong);
    margin-top: -6px;
  }
  .hash {
    font-size: 1rem;
    color: var(--msg, #9a9fd6);
  }
  .row .name {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row .name.room {
    color: var(--tx-body);
  }
  .cwd {
    font: 0.885rem var(--font-mono);
    color: var(--tx-disabled);
    margin-left: 6px;
  }
  .mono {
    font: 0.885rem var(--font-mono);
    color: var(--tx-muted-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mono.dim {
    color: var(--tx-disabled);
  }
  .mono.c2 {
    color: var(--tx-2);
  }
  .mono.msgc {
    color: var(--msg, #9a9fd6);
  }
  .age {
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
    text-align: right;
  }
  .age.orange {
    color: var(--status-attention);
  }
  .guess {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 2px 14px 0 50px;
    font-size: 0.923rem;
    color: var(--tx-label);
  }
  .guess .where {
    color: var(--tx-2);
  }
  .guess .move {
    color: var(--aqua);
  }
  .guess button:hover {
    text-decoration: underline;
  }
  .empty-pane {
    display: block;
    padding: 18px 14px;
    font-size: 0.923rem;
    line-height: 1.5;
    text-align: left;
  }
  .hub-err {
    flex: none;
    margin: 0 10px 6px;
    padding: 5px 10px;
    border-radius: var(--radius-md);
    border: 1px solid color-mix(in srgb, var(--status-broken) 45%, transparent);
    background: color-mix(in srgb, var(--status-broken) 7%, transparent);
    color: var(--tx-2);
    font-size: 0.923rem;
    display: flex;
    gap: 8px;
    align-items: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hub-err i {
    color: var(--status-broken);
  }
  .slot {
    display: flex;
    flex-direction: column;
  }
  .slot.framed {
    margin: 0 8px;
    border-inline: 1px solid color-mix(in srgb, var(--status-done) 32%, transparent);
    background: color-mix(in srgb, var(--status-done) 5%, transparent);
  }
  .slot.ftop {
    margin-top: 2px;
    border-top: 1px solid color-mix(in srgb, var(--status-done) 32%, transparent);
    border-radius: var(--radius-md) var(--radius-md) 0 0;
  }
  .nudge {
    margin: 0 8px 4px;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 5px 10px;
    border: 1px solid color-mix(in srgb, var(--status-done) 32%, transparent);
    border-top-color: color-mix(in srgb, var(--status-done) 22%, transparent);
    border-radius: 0 0 var(--radius-md) var(--radius-md);
    background: color-mix(in srgb, var(--status-done) 5%, transparent);
    font-size: 0.923rem;
  }
  .nudge i {
    color: var(--status-done);
  }
  .ntext {
    flex: 1;
    min-width: 0;
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .slot.pending,
  .nudge.pending {
    background: var(--hatch);
  }
  .slot.pending .name,
  .slot.pending .goal,
  .slot.pending .mono {
    text-decoration: line-through;
    color: var(--tx-disabled);
  }
  .undo-note {
    font: 0.846rem var(--font-mono);
    color: var(--status-headsup);
    white-space: nowrap;
  }
  .undo-note.wide {
    grid-column: span 3;
    text-align: right;
  }
  .archive {
    margin-top: 10px;
    opacity: 0.4;
    transition: opacity 120ms;
  }
  .archive:hover,
  .archive.lit {
    opacity: 1;
  }
  .ahead {
    display: flex;
    gap: 6px;
    align-items: center;
    margin: 0 14px;
    padding: 10px 0 3px;
    border-top: 1px solid var(--bg-head);
  }
  .acount {
    margin-left: auto;
    text-transform: none;
    letter-spacing: 0;
    font: 0.846rem var(--font-mono);
  }
  .alabel {
    padding: 8px 14px 2px;
  }
  .alabel.gl {
    padding: 4px 14px 0 28px;
    color: var(--tx-label);
    font-size: 0.923rem;
  }
  .arow {
    width: 100%;
    text-align: left;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 48px;
    gap: 6px;
    align-items: center;
    padding: var(--row-pad-compact) 14px var(--row-pad-compact) 42px;
    min-height: var(--row-min-compact);
  }
  .arow.grp {
    padding-left: 28px;
  }
  .arow:hover {
    background: var(--row-hover);
  }
  .arow.sel {
    background: var(--aqua-tint);
  }
  .aname {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arow .name {
    color: var(--tx-body);
  }
  .arow.grp .name {
    font-weight: 500;
  }
  .aline {
    grid-column: 1 / -1;
    font: 0.885rem var(--font-mono);
    color: var(--tx-muted-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .aline .who {
    color: var(--tx-2);
  }
  .afoot {
    padding: 8px 14px;
    font-size: 0.846rem;
    color: var(--tx-label);
  }
  .menu {
    position: fixed;
    z-index: 50;
    width: 230px;
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
  }
  .menu button {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 5px 12px;
    text-align: left;
    color: var(--tx-2);
  }
  .menu button:hover:not(:disabled) {
    background: var(--hover-tint);
  }
  .menu button:disabled {
    color: var(--tx-disabled);
    cursor: default;
  }
  .menu button kbd {
    margin-left: auto;
  }
  .menu .off {
    font-size: 0.846rem;
    color: var(--tx-label);
  }
  .menu .divider {
    height: 1px;
    background: var(--border);
    margin: 4px 0;
  }
  .hostline {
    flex: none;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 14px 8px;
    border-top: 1px solid var(--bg-head);
    font: 0.846rem var(--font-mono);
    color: var(--tx-label);
    min-width: 0;
  }
  .hostline > span:not(.hacts) {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .hostline.bad i,
  .hostline .note {
    color: var(--status-headsup);
  }
  .hacts {
    margin-left: auto;
    display: flex;
    gap: 10px;
    flex: none;
  }
  .hacts button {
    color: var(--tx-2);
  }
  .hacts button:hover {
    color: var(--aqua);
  }
</style>
