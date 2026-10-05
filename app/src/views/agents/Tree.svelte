<script lang="ts">
  import { ui } from '../../lib/app.svelte';
  import { agentColor } from './look';
  import { act, ag, age, look, nodes, openThread, sessionById, type Node } from './state.svelte';
  import FilterField from '../../lib/ui/FilterField.svelte';
  import StatusMark from '../../lib/ui/StatusMark.svelte';
  import type { Group, Request, Session, Status } from './types';

  let { onNew, filterEl = $bindable() }: { onNew: () => void; filterEl?: HTMLInputElement } = $props();

  const list = $derived(nodes());
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

  const hostClass = (g: Group) => (g.source === 'acp' ? 'local' : g.host.toLowerCase().startsWith('scion') ? 'scion' : 'other');

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

  function click(n: Node) {
    ag.sel = n.key;
    if (n.type === 'group') {
      if (ag.folded.has(n.group.id)) ag.folded.delete(n.group.id);
      else ag.folded.add(n.group.id);
      ag.folded = new Set(ag.folded);
      return;
    }
    openThread(n.key);
    ag.zone = 'tree';
  }

  const host = $derived(ag.tree?.agentd);
  const localLive = $derived((ag.tree?.sessions ?? []).filter((s) => s.source === 'acp' && !s.parent_id && s.live).length);
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

<div class="tree" class:focus={ag.zone === 'tree'}>
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
    <button class="plus" onclick={onNew} title="New session (n)" aria-label="New session"><i class="ph ph-plus"></i></button>
  </div>

  {#each (ag.tree?.hubs ?? []).filter((h) => h.state === 'error') as h (h.source)}
    <div class="hub-err" title={h.error}><i class="ph ph-plugs"></i>Scion · {h.name}: {h.error}</div>
  {/each}

  {#if requests.length}
    <div class="needs">
      <div class="needs-head label"><i class="ph ph-bell-simple"></i>Needs you<span class="n">{requests.length}</span></div>
      {#each requests as r (r.id)}
        {@const s = sessionById(r.session_id)}
        {@const l = needLine(r)}
        <button class="need" class:on={ag.open === r.thread} onclick={() => openRequest(r)}>
          {#if r.kind === 'ask'}<i class="ph ph-question q"></i>{:else}<StatusMark status="waiting" />{/if}
          <span class="who" style:color={agentColor(look(), s)}>{s?.name ?? '?'}</span>
          <span class="what"><span class={l.verbClass}>{l.verb}</span> {l.text}</span>
          <span class="mono-tag {r.kind === 'ask' ? 'orange' : tagClass(r.tag)}">{r.kind === 'ask' ? 'question' : r.tag}</span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="list">
    {#each list as n (n.key)}
      {#if n.type === 'project'}
        <div class="project label">{n.label}</div>
      {:else if n.type === 'unsorted'}
        <div class="project unsorted label">Unsorted</div>
      {:else if n.type === 'group'}
        {@const f = folded(n.sessions)}
        <button class="group" class:sel={ag.sel === n.key} onclick={() => click(n)}>
          <i class="ph {n.open ? 'ph-caret-down' : 'ph-caret-right'} caret"></i>
          <span class="gname"><span class="name">{n.group.name}</span>{#if n.group.goal}<span class="goal">{n.group.goal}</span>{/if}</span>
          <span class="gright">
            {#if !n.open && f}
              <span class="fold" class:red={f.status === 'failed'}><StatusMark status={f.status} size={7} />{f.n}</span>
            {/if}
            <span class="host {hostClass(n.group)}">{n.group.host}</span>
          </span>
        </button>
      {:else if n.type === 'room'}
        {@const room = n.group.room!}
        <button class="row" class:sel={ag.sel === n.key} onclick={() => click(n)}>
          <span class="mark"><i class="ph ph-hash hash"></i></span>
          <span class="name room">{room.name}</span>
          <span class="mono c2">room · {room.members}</span>
          <span class="mono msgc">{room.unread ? `${room.unread} new` : ''}</span>
          <span class="age" class:orange={requests.some((r) => r.thread === n.key)}>{age(room.active_at, now)}</span>
        </button>
      {:else}
        {@const s = n.s}
        <button class="row" class:sel={ag.sel === n.key} class:sub={n.depth > 0} class:loose={n.unsorted} onclick={() => click(n)} style:--indent="{14 * n.depth}px">
          <span class="mark">
            {#if n.depth > 0}<span class="elbow"></span>{/if}
            <StatusMark status={s.status} />
          </span>
          <span class="name" style:color={agentColor(look(), s)}
            >{s.name}{#if n.unsorted && s.cwd}<span class="cwd">{s.cwd.replace(/^\/(Users|home)\/[^/]+/, '~')}</span>{/if}</span
          >
          <span class="mono" class:dim={s.status === 'done'}>{s.harness}</span>
          <span class="mono" class:dim={s.status === 'done'}>{s.model}</span>
          <span class="age" class:orange={s.status === 'waiting'}>{age(s.active_at, now)}</span>
        </button>
        {#if n.unsorted && s.guess?.group_id}
          <div class="guess">
            Looks like <span class="where">{s.guess.label}</span>
            <button class="move" onclick={() => act('move', { session: s.id, group: s.guess!.group_id })}>Move</button>
            <button onclick={() => act('guess_dismiss', { session: s.id })}>Not this</button>
          </div>
        {/if}
      {/if}
    {:else}
      <div class="empty">
        {#if ag.filter}Nothing matches "{ag.filter}".{:else}No sessions yet. <button class="link" onclick={onNew}>Start one</button> (n), or add a Scion Hub in Settings › Sources.{/if}
      </div>
    {/each}
  </div>

  {#if host && (anyLocal || (host.state !== 'idle' && host.state !== ''))}
    <div class="hostline" class:bad={host.state === 'missing' || host.state === 'error' || host.state === 'unsupported'} title={host.error || host.binary}>
      <i class="ph {host.state === 'running' ? 'ph-cpu' : host.state === 'starting' ? 'ph-circle-notch' : 'ph-plugs'}"></i>
      {#if host.state === 'running'}
        <span>Agent host · v{host.version} · {localLive} running</span>
        {#if host.stale}<span class="note">a newer build is ready</span>{/if}
      {:else if host.state === 'idle' || host.state === ''}
        <span>Agent host · starts with the next local session</span>
      {:else if host.state === 'starting'}
        <span>Agent host · starting…</span>
      {:else}
        <span class="err">{host.error || 'Agent host unavailable'}</span>
      {/if}
      <span class="hacts">
        {#if host.state === 'running' && localLive > 0}
          <button onclick={() => twice('stop')}>{armed === 'stop' ? `Stops ${localLive} · click again` : 'Stop all'}</button>
        {/if}
        {#if host.state === 'running' ? host.stale : host.state === 'error' || host.state === 'missing'}
          <button onclick={() => twice('restart')}>{armed === 'restart' ? `Stops ${localLive} · click again` : host.state === 'running' ? 'Restart host' : 'Retry'}</button>
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
    color: var(--red);
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
    border-radius: var(--r-input);
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
    border-radius: 7px;
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
  .project:first-child {
    padding-top: 10px;
  }
  .project.unsorted {
    margin: 14px 14px 0;
    padding: 10px 0 3px;
    border-top: 1px solid var(--bg-head);
  }
  .group {
    width: 100%;
    text-align: left;
    display: grid;
    grid-template-columns: 14px minmax(0, 1fr) auto;
    gap: 6px;
    align-items: center;
    padding: var(--row-pad-compact) 14px;
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
    color: var(--red);
  }
  .host {
    background: var(--bg-head);
    border-radius: var(--r-block);
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
  }
  .row.sub {
    grid-template-columns: calc(30px + var(--indent)) minmax(0, 1fr) 90px 52px 30px;
  }
  .row.loose {
    padding: var(--row-pad-compact) 14px;
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
    color: var(--orange);
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
  .empty {
    padding: 18px 14px;
    color: var(--tx-label);
    font-size: 0.923rem;
    line-height: 1.5;
  }
  .link {
    color: var(--aqua);
  }
  .hub-err {
    flex: none;
    margin: 0 10px 6px;
    padding: 5px 10px;
    border-radius: 7px;
    border: 1px solid rgba(251, 73, 52, 0.45);
    background: rgba(251, 73, 52, 0.07);
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
    color: var(--red);
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
  .hostline .err {
    color: var(--orange);
  }
  .hostline .note {
    color: var(--yellow);
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
