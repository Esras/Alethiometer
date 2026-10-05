<script lang="ts">
  // A pinned ask: a Scion input-needed, or an ACP elicitation (Claude Code's
  // AskUserQuestion, pi's questions). The question, its choices as option
  // cards (number keys), Write a reply… (R) and Later (Esc). An elicitation
  // that's more than one choice gets its fields as a small form.
  import { agentColor } from './look';
  import { act, ag, age, look, openThread, sessionById } from './state.svelte';
  import type { FormField, Request } from './types';
  import NeedCard from './NeedCard.svelte';

  let { req, compact = false, onReply }: { req: Request; compact?: boolean; onReply: () => void } = $props();

  const s = $derived(sessionById(req.session_id));

  export function choose(key: string): boolean {
    const c = req.choices.find((c) => c.key === key);
    if (!c) return false;
    act('answer', req.form ? { request: req.id, choice: c.key } : { request: req.id, text: c.title }).catch(() => {});
    return true;
  }

  type Opt = { value: string; title: string; description: string };
  type Field = { name: string; label: string; hint: string; kind: 'one' | 'many' | 'bool' | 'number' | 'text'; options: Opt[]; required: boolean };
  const optsOf = (f: FormField | undefined): Opt[] => {
    const list: { const: string; title?: string; description?: string }[] = f?.oneOf ?? f?.anyOf ?? f?.enum?.map((e) => ({ const: e })) ?? [];
    return list.map((o) => ({ value: String(o.const), title: o.title || String(o.const), description: o.description ?? '' }));
  };
  const fields = $derived<Field[]>(
    Object.entries(req.form?.properties ?? {}).map(([name, f]) => {
      const many = f.type === 'array';
      const options = optsOf(many ? f.items : f);
      const kind = many ? 'many' : options.length ? 'one' : f.type === 'boolean' ? 'bool' : f.type === 'number' || f.type === 'integer' ? 'number' : 'text';
      return { name, label: f.title ?? '', hint: f.description ?? '', kind, options, required: !!req.form?.required?.includes(name) };
    }),
  );
  // One choice (plus optional text, which Write a reply fills) or a lone text field: the plain card does.
  const formNeeded = $derived(!!req.form && !req.choices.length && !(fields.length === 1 && fields[0].kind === 'text'));
  let values = $state<Record<string, unknown>>({});
  const filled = (v: unknown) => v !== undefined && v !== '' && !(Array.isArray(v) && !v.length);
  const ready = $derived(fields.every((f) => !f.required || filled(values[f.name])));
  function toggle(name: string, v: string) {
    const cur = Array.isArray(values[name]) ? (values[name] as string[]) : [];
    values[name] = cur.includes(v) ? cur.filter((x) => x !== v) : [...cur, v];
  }
  function submit() {
    const content = Object.fromEntries(Object.entries(values).filter(([, v]) => filled(v)));
    act('answer', { request: req.id, content }).catch(() => {});
  }
</script>

{#snippet title()}
  {#if compact}<span style:color={agentColor(look(), s)}>{s?.name}</span> is waiting for you{:else}{s?.name ?? 'An agent'} is waiting for you{/if}
{/snippet}
{#snippet meta()}
  <span class="via">{req.via || 'input-needed'} · {age(req.at, Date.now())}</span>
  {#if req.thread !== `s:${req.session_id}`}
    <button class="link" onclick={() => openThread(`s:${req.session_id}`)}><i class="ph ph-arrow-square-out"></i>Open session</button>
  {/if}
{/snippet}
{#snippet summary()}
  <span class="c">{req.question}</span>
{/snippet}

<NeedCard icon="question" {compact} {title} {meta} {summary} headBottom={4}>
    <div class="q">{req.question}</div>
    {#if formNeeded}
      <div class="fields">
        {#each fields as f (f.name)}
          <div class="field">
            {#if f.label || f.hint}<div class="fl">{f.label}{#if f.label && f.hint}<span class="fh">{f.hint}</span>{:else if f.hint}{f.hint}{/if}</div>{/if}
            {#if f.kind === 'one' || f.kind === 'many'}
              <div class="opts">
                {#each f.options as o (o.value)}
                  {@const on = f.kind === 'one' ? values[f.name] === o.value : Array.isArray(values[f.name]) && (values[f.name] as string[]).includes(o.value)}
                  <button class="opt" class:on title={o.description} onclick={() => (f.kind === 'one' ? (values[f.name] = on ? undefined : o.value) : toggle(f.name, o.value))}>
                    <i class="ph {f.kind === 'one' ? (on ? 'ph-radio-button' : 'ph-circle') : on ? 'ph-check-square' : 'ph-square'}"></i>{o.title}
                  </button>
                {/each}
              </div>
            {:else if f.kind === 'bool'}
              <button class="opt" class:on={values[f.name] === true} onclick={() => (values[f.name] = values[f.name] !== true)}><i class="ph {values[f.name] === true ? 'ph-check-square' : 'ph-square'}"></i>Yes</button>
            {:else}
              <input
                class="input sm"
                type={f.kind === 'number' ? 'number' : 'text'}
                placeholder={f.required ? '' : 'Optional'}
                oninput={(e) => (values[f.name] = f.kind === 'number' ? (e.currentTarget.value === '' ? undefined : Number(e.currentTarget.value)) : e.currentTarget.value)}
                onkeydown={(e) => e.stopPropagation()}
              />
            {/if}
          </div>
        {/each}
      </div>
    {:else if req.choices.length}
      <div class="choices" style:grid-template-columns="repeat({Math.min(req.choices.length, 3)}, minmax(0, 1fr))">
        {#each req.choices as c (c.key)}
          <button class="choice" onclick={() => choose(c.key)}>
            <kbd>{c.key}</kbd>
            <span class="ct">{c.title}</span>
            <span></span>
            {#if c.detail}<span class="cd">{c.detail}</span>{/if}
          </button>
        {/each}
      </div>
    {/if}
    <div class="btns">
      {#if formNeeded}<button class="btn md primary" disabled={!ready} onclick={submit}>Send answer</button>{/if}
      <button class="btn md" class:on={ag.replying === req.id} onclick={onReply}><kbd>R</kbd>Write a reply…</button>
      <button class="btn md" onclick={() => ag.later.add(req.id)}><kbd>Esc</kbd>Later</button>
      {#if req.form}<span class="right">Asked by {s?.name ?? 'the agent'}</span>
      {:else if req.choices.length}<span class="right">Choices read from {s?.name ?? 'the agent'}’s message</span>{/if}
    </div>
</NeedCard>

<style>
  .via {
    font: 0.885rem var(--font-mono);
    color: var(--tx-label);
  }
  .link {
    margin-left: auto;
    font-size: 0.923rem;
    color: var(--tx-label);
    display: flex;
    gap: 5px;
    align-items: center;
  }
  .link:hover {
    color: var(--tx-2);
  }
  .q {
    padding: 0 12px 0 38px;
    font-size: 1.077rem;
    line-height: 1.5;
    color: var(--tx-title);
    text-wrap: pretty;
  }
  .choices {
    margin: 8px 12px 0;
    display: grid;
    gap: 10px;
  }
  .choice {
    text-align: left;
    padding: 7px 10px;
    border-radius: var(--r-input);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 2px 9px;
    align-items: baseline;
  }
  .choice:hover {
    border-color: var(--border-strong);
  }
  .ct {
    color: var(--tx-body);
  }
  .cd {
    font-size: 0.923rem;
    line-height: 1.45;
    color: var(--tx-muted-2);
    text-wrap: pretty;
  }
  .btns {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 9px 12px;
  }
  .fields {
    margin: 8px 12px 0 38px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .fl {
    font-size: 0.923rem;
    color: var(--tx-2);
    margin-bottom: 4px;
  }
  .fh {
    margin-left: 8px;
    color: var(--tx-label);
  }
  .opts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .opt {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    padding: var(--row-pad-compact) 9px;
    border-radius: var(--r-input);
    background: var(--bg-panel);
    border: 1px solid var(--border);
    color: var(--tx-body);
  }
  .opt.on {
    border-color: var(--need);
  }
  .right {
    margin-left: auto;
    font-size: 0.885rem;
    color: var(--tx-label);
  }
  .c {
    flex: 1;
    min-width: 0;
    font-size: 1rem;
    color: var(--tx-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
