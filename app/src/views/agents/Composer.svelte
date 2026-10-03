<script lang="ts">
  // "Message <agent or #room>" (⌘⏎). In a room, @ addresses one member. ↑ in
  // an empty composer brings back your last message. While an ask is being
  // answered (R), the text goes back as the answer.
  import { untrack } from 'svelte';
  import { act, ag, sessionById } from './state.svelte';
  import type { Group, Session } from './types';

  let {
    thread,
    session,
    group,
    el = $bindable(),
    onNewAgent,
    onOpen,
  }: {
    thread: string;
    session?: Session;
    group?: Group;
    el?: HTMLTextAreaElement;
    onNewAgent?: () => void;
    onOpen: (how: 'app' | 'terminal') => void;
  } = $props();

  const room = $derived(thread.startsWith('r:') ? group?.room : null);
  const members = $derived(room && group ? (ag.tree?.sessions ?? []).filter((s) => s.group_id === group.id) : []);
  const target = $derived(room ? `#${room.name}` : (session?.name ?? ''));
  const replyingTo = $derived(ag.replying ? ag.tree?.requests.find((r) => r.id === ag.replying) : undefined);
  // Keyed by thread in Pane, so the draft is read once.
  let text = $state(untrack(() => ag.drafts[thread] ?? ''));
  let images = $state<{ data: string; mime: string }[]>([]);
  let busy = $state(false);
  let pick = $state(0);
  $effect(() => {
    ag.drafts[thread] = text;
  });

  // @ completion: the word being typed at the caret.
  let caret = $state(0);
  const at = $derived.by(() => {
    if (!room) return null;
    const m = /(^|\s)@([\w.-]*)$/.exec(text.slice(0, caret));
    return m ? m[2].toLowerCase() : null;
  });
  const options = $derived(at === null ? [] : members.filter((s) => s.name.toLowerCase().startsWith(at)).slice(0, 6));

  function complete(s: Session) {
    const before = text.slice(0, caret).replace(/@([\w.-]*)$/, `@${s.name} `);
    text = before + text.slice(caret);
    queueMicrotask(() => {
      el?.setSelectionRange(before.length, before.length);
      caret = before.length;
    });
  }

  async function send() {
    const t = text.trim();
    if ((!t && !images.length) || busy) return;
    busy = true;
    try {
      if (replyingTo) {
        await act('answer', { request: replyingTo.id, text: t });
        ag.replying = null;
      } else {
        // One @member at the start addresses that member (a mention); otherwise the whole room.
        const m = /^@([\w.-]+)\s/.exec(t);
        const to = room && m && members.some((s) => s.name === m[1]) ? m[1] : '';
        await act('send', { thread, text: t, to, images });
      }
      text = '';
      images = [];
    } catch {
      /* shown in the error line */
    } finally {
      busy = false;
    }
  }

  // Local sessions only: ACP's image prompt capability isn't wired for Scion.
  function onPaste(e: ClipboardEvent) {
    if (!isLocal) return;
    const files = [...(e.clipboardData?.items ?? [])].filter((i) => i.kind === 'file' && i.type.startsWith('image/')).map((i) => i.getAsFile());
    if (!files.length) return;
    e.preventDefault();
    for (const f of files) {
      if (!f) continue;
      const reader = new FileReader();
      reader.onload = () => {
        const url = String(reader.result ?? '');
        const data = url.slice(url.indexOf(',') + 1);
        if (data) images = [...images, { data, mime: f.type }];
      };
      reader.readAsDataURL(f);
    }
  }

  function lastMine(): string {
    for (let i = ag.rows.length - 1; i >= 0; i--) if (ag.rows[i].kind === 'you') return ag.rows[i].text;
    return '';
  }

  function onKey(e: KeyboardEvent) {
    caret = el?.selectionStart ?? 0;
    if (options.length && (e.key === 'Tab' || e.key === 'Enter') && !e.metaKey && !e.ctrlKey) {
      complete(options[Math.min(pick, options.length - 1)]);
      e.preventDefault();
    } else if (options.length && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      pick = (pick + (e.key === 'ArrowDown' ? 1 : options.length - 1)) % options.length;
      e.preventDefault();
    } else if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      send();
      e.preventDefault();
    } else if (e.key === 'ArrowUp' && !text) {
      text = lastMine();
      e.preventDefault();
    } else if (e.key === 'Escape') {
      if (ag.replying) ag.replying = null;
      el?.blur();
      ag.zone = 'thread';
      e.preventDefault();
    } else if (e.key === 'Tab') {
      el?.blur();
      ag.zone = 'tree';
      e.preventDefault();
    } else if ((e.metaKey || e.ctrlKey) && e.key === '.') {
      return; // the view interrupts
    } else {
      e.stopPropagation();
      return;
    }
    e.stopPropagation();
  }

  // Grow with the text, up to about eight lines.
  $effect(() => {
    void text;
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  });

  const isLocal = $derived(session?.source === 'acp');
  const placeholder = $derived(
    replyingTo
      ? `Reply to ${sessionById(replyingTo.session_id)?.name ?? 'the agent'}`
      : `Message ${target}${isLocal && session && !session.live && !session.id.includes('~') ? ' · sending starts it again' : ''}`,
  );
</script>

<div class="row">
  <label class="box" class:focus={ag.zone === 'composer'} class:reply={!!replyingTo}>
    <i class="ph {replyingTo ? 'ph-arrow-bend-up-left' : 'ph-chat-circle'}"></i>
    <div class="input-col">
      {#if images.length}
        <div class="thumbs">
          {#each images as img, i (i)}
            <div class="thumb">
              <img src="data:{img.mime};base64,{img.data}" alt="pasted" />
              <button class="rm" title="Remove" onclick={() => (images = images.filter((_, j) => j !== i))}><i class="ph ph-x"></i></button>
            </div>
          {/each}
        </div>
      {/if}
      <textarea
        bind:this={el}
        bind:value={text}
        rows="1"
        {placeholder}
        onfocus={() => (ag.zone = 'composer')}
        onkeydown={onKey}
        onkeyup={() => (caret = el?.selectionStart ?? 0)}
        onclick={() => (caret = el?.selectionStart ?? 0)}
        onpaste={onPaste}
      ></textarea>
    </div>
    {#if room && !text}<span class="hint-at">@ to address one member</span>{/if}
    {#if session?.source === 'scion' && !text}<span class="hint-at">sent through Scion</span>{/if}
    <span class="kbd">⌘⏎</span>
    {#if options.length}
      <div class="pop">
        {#each options as s, i (s.id)}
          <button class:on={i === pick} onmousedown={(e) => (e.preventDefault(), complete(s))}>@{s.name}<span>{s.harness} · {s.model}</span></button>
        {/each}
      </div>
    {/if}
  </label>
  {#if room}
    <button class="btn" onclick={onNewAgent}><i class="ph ph-user-plus"></i>Agent<span class="k">N</span></button>
  {/if}
  {#if session?.source === 'acp' && (session.status === 'running' || session.status === 'waiting') && session.live}
    <button class="btn" onclick={() => act('interrupt', { session: session.id })}><i class="ph ph-hand-palm"></i>Interrupt<span class="k">⌘.</span></button>
  {/if}
  {#if session?.source === 'acp' && session.live && !session.id.includes('~')}
    <button class="btn" title="Stop the agent; sending starts it again" onclick={() => act('stop', { session: session.id })}><i class="ph ph-stop"></i>Stop</button>
  {/if}
  {#if room || session?.source === 'scion'}
    <button class="btn" onclick={() => onOpen('app')}><i class="ph ph-arrow-square-out"></i>Open on Hub</button>
  {:else if session}
    <button class="btn" onclick={() => onOpen('app')}><i class="ph ph-arrow-square-out"></i>Open in Zed</button>
  {/if}
  <button class="btn icon" title="Terminal attached to this session" onclick={() => onOpen('terminal')}><i class="ph ph-terminal-window"></i></button>
</div>

<style>
  .row {
    display: flex;
    gap: 8px;
    align-items: flex-end;
  }
  .box {
    position: relative;
    flex: 1;
    display: flex;
    gap: 8px;
    align-items: flex-end;
    min-height: 36px;
    padding: 6px 12px;
    border-radius: 7px;
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    color: var(--tx-label);
  }
  .box.focus {
    border-color: var(--aqua);
  }
  .box.reply {
    border-color: var(--need);
  }
  .box i {
    font-size: 1.077rem;
  }
  .input-col {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .thumbs {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    padding-top: 6px;
  }
  .thumb {
    position: relative;
  }
  .thumb img {
    display: block;
    height: 48px;
    width: 48px;
    object-fit: cover;
    border-radius: 5px;
    border: 1px solid var(--border-strong);
  }
  .thumb .rm {
    position: absolute;
    top: -5px;
    right: -5px;
    width: 16px;
    height: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: var(--bg-head);
    border: 1px solid var(--border-strong);
    color: var(--tx-2);
    font-size: 0.7rem;
  }
  .thumb .rm:hover {
    color: var(--tx-title);
  }
  textarea {
    flex: none;
    width: 100%;
    min-width: 0;
    resize: none;
    background: none;
    border: 0;
    outline: none;
    padding: 0;
    font-size: 1rem;
    line-height: 1.45;
    color: var(--tx-body);
    max-height: 180px;
  }
  textarea::placeholder {
    color: var(--tx-label);
  }
  .hint-at {
    color: var(--tx-disabled);
    font-size: 0.923rem;
    white-space: nowrap;
  }
  .kbd {
    margin-left: auto;
    font: 0.846rem var(--font-mono);
    color: var(--tx-disabled);
  }
  .btn {
    display: flex;
    gap: 7px;
    align-items: center;
    padding: 7px 12px;
    border-radius: 7px;
    border: 1px solid var(--border-strong);
    color: var(--tx-2);
    white-space: nowrap;
  }
  .btn:hover {
    border-color: var(--tx-disabled);
  }
  .btn.icon {
    padding: 7px 10px;
  }
  .btn i {
    font-size: 1.077rem;
  }
  .k {
    font: 0.808rem var(--font-mono);
    color: var(--tx-label);
  }
  .pop {
    position: absolute;
    left: 30px;
    bottom: calc(100% + 4px);
    min-width: 220px;
    background: var(--bg-panel);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-input);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4);
    padding: 4px;
    display: flex;
    flex-direction: column;
    z-index: 5;
  }
  .pop button {
    display: flex;
    gap: 10px;
    justify-content: space-between;
    padding: 4px 8px;
    border-radius: var(--r-block);
    font: 0.923rem var(--font-mono);
    color: var(--tx-body);
    text-align: left;
  }
  .pop button span {
    color: var(--tx-label);
  }
  .pop button.on {
    background: var(--border);
  }
</style>
