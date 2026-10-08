<script lang="ts">
  // "Message <agent or #room>" (⌘⏎). In a room, @ addresses one member. ↑ in
  // an empty composer brings back your last message. While an ask is being
  // answered (R), the text goes back as the answer.
  import { untrack } from 'svelte';
  import { chord, mod } from '../../lib/keys.svelte';
  import { popover } from '../../lib/ui/popover';
  import { act, ag, pinned, sessionById } from './state.svelte';
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
  // A reply to the ask pinned above is typed in its card; this box takes one to another ask.
  const replyingTo = $derived(ag.replying && ag.replying !== pinned(thread)?.id ? ag.tree?.requests.find((r) => r.id === ag.replying) : undefined);
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
  // Escape or a click elsewhere closes the menu until the word being typed changes.
  let dismissed = $state(false);
  $effect(() => {
    void at;
    dismissed = false;
  });
  const options = $derived(at === null || dismissed ? [] : members.filter((s) => s.name.toLowerCase().startsWith(at)).slice(0, 6));

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
    if (!t && !images.length) {
      // Cmd+Enter again on an empty box, with something already held back: same as Send now.
      if (hasQueued && isLocal) act('send_queued', { session: session!.id }).catch(() => {});
      return;
    }
    if (busy) return;
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
    } else if (mod(e) && e.key === 'Enter') {
      send();
      e.preventDefault();
    } else if (e.key === 'ArrowUp' && !text) {
      text = lastMine();
      e.preventDefault();
    } else if (e.key === 'Escape' && options.length) {
      return; // the menu's own Escape, through the key stack
    } else if (e.key === 'Escape') {
      if (ag.replying) ag.replying = null;
      el?.blur();
      ag.zone = 'tree';
      e.preventDefault();
    } else if (e.key === 'Tab') {
      el?.blur();
      ag.zone = 'thread';
      e.preventDefault();
    } else if (mod(e) && e.key === '.') {
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
  const hasQueued = $derived((session?.queued?.length ?? 0) > 0);
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
    <kbd class="send-key">{chord('⌘⏎', 'Ctrl+⏎')}</kbd>
    {#if options.length}
      <div class="pop popover flush" use:popover={{ onclose: () => (dismissed = true), trigger: el }}>
        {#each options as s, i (s.id)}
          <button class:on={i === pick} onmousedown={(e) => (e.preventDefault(), complete(s))}>@{s.name}<span>{s.harness} · {s.model}</span></button>
        {/each}
      </div>
    {/if}
  </label>
  {#if room}
    <button class="btn" onclick={onNewAgent}><i class="ph ph-user-plus"></i>Agent<kbd>N</kbd></button>
  {/if}
  {#if session?.source === 'acp' && session.live && hasQueued}
    <button class="btn primary" onclick={() => act('send_queued', { session: session.id })}><i class="ph ph-fast-forward"></i>Send now<kbd>{chord('⌘↵', 'Ctrl+↵')}</kbd></button>
  {/if}
  {#if session?.source === 'acp' && (session.status === 'running' || session.status === 'waiting') && session.live}
    <button class="btn" onclick={() => act('interrupt', { session: session.id })}><i class="ph ph-hand-palm"></i>Interrupt<kbd>{chord('⌘.', 'Ctrl+.')}</kbd></button>
  {/if}
  {#if session?.source === 'acp' && session.live && !session.id.includes('~')}
    <button class="btn" title="Stop the agent; sending starts it again" onclick={() => act('stop', { session: session.id })}><i class="ph ph-stop"></i>Stop</button>
  {/if}
  {#if room || session?.source === 'scion'}
    <button class="btn" onclick={() => onOpen('app')}><i class="ph ph-arrow-square-out"></i>Open on Hub</button>
  {:else if session && !session.sandbox}
    <button class="btn" onclick={() => onOpen('app')}><i class="ph ph-arrow-square-out"></i>Open in Zed</button>
  {:else if session?.sandbox?.upload && !session.parent_id}
    <button class="btn" title="Copy what the agent changed in its sandbox into {session.sandbox.upload}" onclick={() => (ag.bringBack = session.id)}><i class="ph ph-download-simple"></i>Bring back</button>
  {/if}
  {#if !room && (session?.source === 'scion' || session?.sandbox)}
    <button class="btn icon" title={session?.sandbox ? 'A shell in its sandbox' : 'Terminal attached to this session'} onclick={() => onOpen('terminal')}><i class="ph ph-terminal-window"></i></button>
  {/if}
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
    border-radius: var(--radius-md);
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
    margin-bottom: 4px;
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
    border-radius: var(--radius-md);
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
  .send-key {
    margin-left: auto;
    margin-bottom: 4px;
  }
  .btn {
    border-radius: var(--radius-md);
  }
  .btn i {
    font-size: 1.077rem;
  }
  .pop {
    position: absolute;
    left: 30px;
    bottom: calc(100% + 4px);
    min-width: 220px;
    border-radius: var(--radius-md);
    display: flex;
    flex-direction: column;
    z-index: 5;
  }
  .pop button {
    display: flex;
    gap: 10px;
    justify-content: space-between;
    padding: 4px 12px;
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
