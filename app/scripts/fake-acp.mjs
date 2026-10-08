#!/usr/bin/env node
// A scripted ACP v1 agent for UI development and screenshots: speaks the agent
// side over stdio like claude-agent-acp (shapes from its recorded scenarios in
// src-tauri/tests/fixtures/acp). Each prompt plays one turn: thinking, reads,
// a subagent, an edit and a shell command that ask for permission, output.
//
//   "agents": { "harnesses": [{ "key": "fake", "label": "Claude Code", "command": "node",
//               "args": ["<repo>/app/scripts/fake-acp.mjs"] }] }
//
// FAKE_ACP_FAST=1 drops the delays (tests). FAKE_ACP_MODE=stream plays a turn
// of numbered output chunks instead ("t1 t2 … tN ", FAKE_ACP_CHUNKS, every
// FAKE_ACP_DELAY_MS), with no permission asks: the restart tests check it
// arrives exactly once. FAKE_ACP_MODE=ask asks two questions through
// elicitation/create (a choice like Claude Code's AskUserQuestion, then text
// like pi's input) and says what it got. FAKE_ACP_MODE=gate gates a bash
// call the way pi-acp does for an extension's prompt: the request repeats
// only the call's id and title, with the gate's text as content. FAKE_ACP_MODE=pigate
// is that with pi's own prompt: a field list restating a long multi-line command.
// FAKE_ACP_NAME is its agentInfo name (e.g. claude-agent-acp's); FAKE_ACP_SAY_MODE=1
// adds the permission mode it's in to the gate's output. FAKE_ACP_MODE=piui is
// pi-acp with a permission extension: a bash call shown pending, then the
// extension's select (its own pi-ui id, every choice allow_once).
// FAKE_ACP_MODE=piquestion is an extension asking a yes/no question the same
// way with no call waiting. FAKE_ACP_MODE=write edits files in its directory
// (adds NOTES.md, rewrites README.md and KEEP.txt, deletes OLD.txt).

import readline from 'node:readline';

const fast = process.env.FAKE_ACP_FAST === '1';
const sleep = (ms) => new Promise((r) => setTimeout(r, fast ? 0 : ms));
const send = (m) => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...m }) + '\n');
let nextId = 1000;
const waiting = new Map();
let session = null;
let cancelled = false;
let turn = 0;
let used = 18000;
let mode = 'default';
let elicits = false;
const MODELS = { currentModelId: 'claude-sonnet-4-6', availableModels: [{ modelId: 'claude-sonnet-4-6', name: 'Sonnet' }, { modelId: 'claude-opus-4-6', name: 'Opus' }, { modelId: 'claude-haiku-4-5', name: 'Haiku' }] };
let effort = 'default';
const configOptions = () => [
  { id: 'model', currentValue: MODELS.currentModelId, options: [] },
  { id: 'effort', name: 'Effort', category: 'thought_level', type: 'select', currentValue: effort, options: ['default', 'low', 'medium', 'high'].map((v) => ({ value: v, name: v })) },
];
const MODES = { currentModeId: 'default', availableModes: [{ id: 'default', name: 'Default' }, { id: 'acceptEdits', name: 'Accept Edits' }, { id: 'plan', name: 'Plan' }, { id: 'auto', name: 'Auto' }] };

function request(method, params) {
  const id = nextId++;
  send({ id, method, params });
  return new Promise((resolve) => waiting.set(id, resolve));
}

const update = (u) => send({ method: 'session/update', params: { sessionId: session, update: u } });
const meta = (toolName, extra = {}) => ({ _meta: { claudeCode: { toolName, ...extra } } });

async function chunks(kind, text) {
  for (const part of text.match(/.{1,18}(\s|$)|.+/g) ?? []) {
    if (cancelled) return;
    update({ sessionUpdate: kind, content: { type: 'text', text: part } });
    await sleep(40);
  }
}

async function tool(id, name, kind, title, rawInput, result, extra = {}) {
  update({ sessionUpdate: 'tool_call', toolCallId: id, kind, title: name, status: 'pending', rawInput: {}, content: [], ...meta(name, extra.meta) });
  await sleep(120);
  update({ sessionUpdate: 'tool_call_update', toolCallId: id, kind, title, rawInput, ...meta(name, extra.meta) });
  if (extra.permission) {
    const r = await request('session/request_permission', {
      sessionId: session,
      toolCall: { toolCallId: id, kind, title, rawInput, status: 'pending', content: extra.content ?? [] },
      options: extra.permission,
      _meta: { permission: { title, version: 1 } },
    });
    const chosen = r?.outcome?.optionId;
    if (!chosen || chosen === 'reject') {
      update({ sessionUpdate: 'tool_call_update', toolCallId: id, status: 'failed', content: [{ type: 'content', content: { type: 'text', text: 'The user denied this.' } }] });
      return false;
    }
  }
  await sleep(extra.ms ?? 250);
  update({ sessionUpdate: 'tool_call_update', toolCallId: id, status: 'completed', content: extra.content ?? [{ type: 'content', content: { type: 'text', text: '```\n' + result + '\n```' } }], rawOutput: result, ...meta(name, extra.meta) });
  return true;
}

async function stream() {
  const n = Number(process.env.FAKE_ACP_CHUNKS ?? 60);
  const delay = Number(process.env.FAKE_ACP_DELAY_MS ?? 50);
  for (let i = 1; i <= n && !cancelled; i++) {
    update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `t${i} ` } });
    await new Promise((r) => setTimeout(r, delay));
  }
  return cancelled ? 'cancelled' : 'end_turn';
}

async function ask() {
  if (!elicits) {
    update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'This client can’t answer questions.' } });
    return 'end_turn';
  }
  if (process.env.FAKE_ACP_ASK === 'form') {
    // Two questions at once, like AskUserQuestion with several: shown as a form.
    const r = await request('elicitation/create', {
      sessionId: session,
      mode: 'form',
      message: 'Please answer the following questions.',
      requestedSchema: {
        type: 'object',
        properties: {
          question_0: { type: 'string', title: 'Database', description: 'Where should the cache live?', oneOf: [{ const: 'Postgres', title: 'Postgres' }, { const: 'SQLite', title: 'SQLite' }] },
          question_0_custom: { type: 'string', title: 'Other' },
          question_1: { type: 'array', title: 'Evict', description: 'When should entries go?', items: { anyOf: [{ const: 'TTL', title: 'After a TTL' }, { const: 'LRU', title: 'Least recently used' }, { const: 'Size', title: 'Over a size cap' }] } },
        },
      },
    });
    update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `Got ${r?.action === 'accept' ? JSON.stringify(r.content) : r?.action}` } });
    return 'end_turn';
  }
  const pick = await request('elicitation/create', {
    sessionId: session,
    mode: 'form',
    message: 'Which database should the cache use?',
    requestedSchema: {
      type: 'object',
      properties: {
        question_0: {
          type: 'string',
          title: 'Database',
          oneOf: [
            { const: 'Postgres', title: 'Postgres', description: 'The one we already run' },
            { const: 'SQLite', title: 'SQLite', description: 'A file next to the app' },
          ],
        },
        question_0_custom: { type: 'string', title: 'Other' },
      },
    },
  });
  const name = await request('elicitation/create', {
    sessionId: session,
    mode: 'form',
    message: 'What should the table be called?',
    requestedSchema: { type: 'object', properties: { value: { type: 'string', title: 'Answer' } }, required: ['value'] },
  });
  const got = (r) => (r?.action === 'accept' ? JSON.stringify(r.content) : r?.action ?? 'nothing');
  update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `Got ${got(pick)} then ${got(name)}` } });
  return cancelled ? 'cancelled' : 'end_turn';
}

async function gate(cmd = 'rm -rf build && node scripts/clean.js', text = 'Dangerous command: deletes files. Allow?') {
  update({ sessionUpdate: 'tool_call', toolCallId: 'g1', kind: 'execute', title: cmd, status: 'pending', rawInput: { command: cmd } });
  const r = await request('session/request_permission', {
    sessionId: session,
    toolCall: { toolCallId: 'g1', title: cmd, status: 'in_progress', content: [{ type: 'terminal', terminalId: 'g1' }, { type: 'content', content: { type: 'text', text } }] },
    options: [
      { optionId: 'choice-0', name: 'Yes', kind: 'allow_once' },
      { optionId: 'choice-1', name: 'No', kind: 'reject_once' },
    ],
  });
  update({ sessionUpdate: 'tool_call_update', toolCallId: 'g1', status: 'completed', content: [] });
  const said = process.env.FAKE_ACP_SAY_MODE === '1' ? ` · ${mode}` : '';
  update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `gate: ${r?.outcome?.optionId ?? 'none'}${said}` } });
  return 'end_turn';
}

// FAKE_ACP_MODE=pigate: the same gate with pi's prompt, a field list that restates the
// call (tool, rule, command, then the whole command again) for a long heredoc.
const PI_COMMAND = [
  "cd ../Alethiometer-archive/app/src-tauri && python3 - <<'EOF'",
  'import json, pathlib',
  "rows = json.loads(pathlib.Path('tests/fixtures/acp/rows.json').read_text())",
  "kinds = sorted({r['kind'] for r in rows})",
  "print(len(rows), 'rows', kinds)",
  'for r in rows[:40]:',
  "    print(r['id'], r['kind'], (r.get('text') or '')[:80])",
  "print('done')",
  'EOF',
].join('\n');
const piGate = () => gate(PI_COMMAND, `tool         : bash\nrule         : *\ncommand      : python3\nfull command : ${PI_COMMAND}`);

async function piSelect(id, title) {
  const r = await request('session/request_permission', {
    sessionId: session,
    toolCall: { toolCallId: id, title, kind: 'other', status: 'pending', rawInput: { method: 'select', title, options: ['Yes', 'No'] } },
    options: [
      { optionId: 'choice-0', name: 'Yes', kind: 'allow_once' },
      { optionId: 'choice-1', name: 'No', kind: 'allow_once' },
    ],
  });
  return r?.outcome?.optionId ?? 'none';
}

async function piUi() {
  update({ sessionUpdate: 'tool_call', toolCallId: 'call_1', kind: 'execute', title: 'bash', status: 'pending', rawInput: { command: 'rm -rf build' } });
  const picked = await piSelect('pi-ui-1', '⚠️ Dangerous command:\n\n  rm -rf build\n\nAllow?');
  update({ sessionUpdate: 'tool_call_update', toolCallId: 'call_1', status: 'completed', content: [] });
  update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `piui: ${picked}` } });
  return 'end_turn';
}

async function piQuestion() {
  const picked = await piSelect('pi-ui-2', 'Also update the docs?');
  update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: `piquestion: ${picked}` } });
  return 'end_turn';
}

async function write() {
  const fs = await import('node:fs');
  fs.writeFileSync('NOTES.md', 'notes from the agent\n');
  fs.writeFileSync('README.md', '# demo, by the agent\n');
  fs.writeFileSync('KEEP.txt', 'the agent\'s version\n');
  fs.rmSync('OLD.txt', { force: true });
  update({ sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'wrote' } });
  return 'end_turn';
}

async function play(text) {
  turn++;
  cancelled = false;
  if (process.env.FAKE_ACP_MODE === 'piui') return piUi();
  if (process.env.FAKE_ACP_MODE === 'piquestion') return piQuestion();
  if (process.env.FAKE_ACP_MODE === 'write') return write();
  if (process.env.FAKE_ACP_MODE === 'stream') return stream();
  if (process.env.FAKE_ACP_MODE === 'ask') return ask();
  if (process.env.FAKE_ACP_MODE === 'gate') return gate();
  if (process.env.FAKE_ACP_MODE === 'pigate') return piGate();
  const t = `t${turn}`;
  await chunks('agent_thought_chunk', 'Probably a flex row without min-width. Check Header.tsx before touching the modal, then look for overrides.');
  await tool(`${t}_read`, 'Read', 'read', 'Read src/settings/Header.tsx', { file_path: `${process.cwd()}/src/settings/Header.tsx` }, Array.from({ length: 64 }, (_, i) => `${i + 1}\tline ${i + 1}`).join('\n'));
  await tool(`${t}_grep`, 'Grep', 'search', 'grep -n "settings-head" src/', { pattern: 'settings-head', path: 'src/' }, 'src/settings/Header.tsx:18        <header class="settings-head">\nsrc/settings/SettingsModal.tsx:41  import { SettingsHead } from "./Header"\nsrc/settings/settings.css:7        .settings-head { display: flex; gap: 12px }');
  // A subagent: its inner calls carry the Task's id as parentToolUseId.
  const task = `${t}_task`;
  update({ sessionUpdate: 'tool_call', toolCallId: task, kind: 'think', title: 'Task', status: 'pending', rawInput: {}, ...meta('Task', { subagent: true }) });
  update({ sessionUpdate: 'tool_call_update', toolCallId: task, kind: 'think', title: 'settings-scan', rawInput: { description: 'settings scan', subagent_type: 'settings-scan', prompt: 'Find every component that renders .settings-head, and anything that overrides its width.' }, ...meta('Task', { subagent: true }) });
  for (const [i, n] of ['Read', 'Read', 'Grep', 'Read', 'Grep'].entries()) {
    await tool(`${t}_sub${i}`, n, n === 'Read' ? 'read' : 'search', n === 'Read' ? `Read src/settings/File${i}.tsx` : 'grep -rn width src/settings', n === 'Read' ? { file_path: `src/settings/File${i}.tsx` } : { pattern: 'width' }, 'ok', { meta: { parentToolUseId: task }, ms: 80 });
  }
  await sleep(200);
  update({ sessionUpdate: 'tool_call_update', toolCallId: task, status: 'completed', content: [{ type: 'content', content: { type: 'text', text: 'Only Header.tsx renders it. SettingsModal.tsx imports it but sets no width. No other overrides.' } }], ...meta('Task', { subagent: true, toolResponse: { totalDurationMs: 4800 } }) });
  await chunks('agent_message_chunk', 'Title and close button share one flex row with no min-width, so below 720px the title wraps under the button.');
  const old = 'export function Header() {\n  return <header class="settings-head">\n    <h1>Settings</h1>\n    <Close />\n  </header>\n}';
  const nw = 'export function Header() {\n  return <header class="settings-head">\n    <h1 class="title">Settings</h1>\n    <span class="hint">⌘,</span>\n    <Close />\n  </header>\n}';
  await tool(`${t}_edit`, 'Edit', 'edit', 'Edit src/settings/Header.tsx', { file_path: `${process.cwd()}/src/settings/Header.tsx`, old_string: old, new_string: nw }, 'The file was updated successfully.', {
    content: [{ type: 'diff', path: `${process.cwd()}/src/settings/Header.tsx`, oldText: old, newText: nw }],
    // Like Claude Code: acceptEdits applies in-project edits without asking.
    permission: mode === 'acceptEdits' ? null : [
      { optionId: 'allow-once', name: 'Yes', kind: 'allow_once' },
      { optionId: 'allow-with-updates', name: 'Yes, allow all edits during this session', kind: 'allow_always' },
      { optionId: 'reject', name: 'No', kind: 'reject_once' },
    ],
  });
  await tool(`${t}_run`, 'Bash', 'execute', 'npm run check', { command: 'npm run check', description: 'Type check' }, '✓ 0 errors, 0 warnings', { ms: 900 });
  await chunks('agent_thought_chunk', 'Before handing back, confirm nothing else in settings/ is misformatted.');
  await chunks('agent_message_chunk', 'Before I hand back, I’ll check formatting on every settings file changed since the last merge.');
  const cmd = "find src -name '*.tsx' -newer .git/ORIG_HEAD -print0 | xargs -0 grep -l 'settings-head' | xargs -I{} sh -c 'npx prettier --check \"{}\" || git diff --stat -- \"{}\"'";
  await tool(`${t}_bash`, 'Bash', 'execute', cmd, { command: cmd, description: 'Check formatting' }, 'Checking formatting...\nAll matched files use Prettier code style!', {
    permission: [
      { optionId: 'allow-once', name: 'Yes', kind: 'allow_once' },
      { optionId: 'allow-with-updates', name: "Yes, and don't ask again for prettier --check commands", kind: 'allow_always' },
      { optionId: 'reject', name: 'No', kind: 'reject_once' },
    ],
  });
  used += 21000;
  update({ sessionUpdate: 'usage_update', used, size: 200000, cost: { amount: 0.41 * turn, currency: 'USD' }, _meta: { '_claude/model': MODELS.currentModelId } });
  if (!cancelled) await chunks('agent_message_chunk', `Done: ${text.slice(0, 60)}`);
  return cancelled ? 'cancelled' : 'end_turn';
}

const rl = readline.createInterface({ input: process.stdin });
rl.on('line', async (line) => {
  let m;
  try {
    m = JSON.parse(line);
  } catch {
    return;
  }
  if (m.id !== undefined && !m.method) {
    waiting.get(m.id)?.(m.result);
    waiting.delete(m.id);
    return;
  }
  const reply = (result) => send({ id: m.id, result });
  switch (m.method) {
    case 'initialize':
      elicits = !!m.params?.clientCapabilities?.elicitation?.form;
      return reply({
        protocolVersion: 1,
        agentCapabilities: { loadSession: true, sessionCapabilities: { resume: {}, list: {} } },
        authMethods: [],
        ...(process.env.FAKE_ACP_NAME ? { agentInfo: { name: process.env.FAKE_ACP_NAME, version: '0.0.0' } } : {}),
      });
    case 'session/new':
      session = `fake-${Date.now().toString(36)}`;
      return reply({ sessionId: session, modes: MODES, models: MODELS, configOptions: configOptions() });
    case 'session/resume':
    case 'session/load':
      session = m.params.sessionId;
      return reply({ modes: MODES, models: MODELS, configOptions: configOptions() });
    case 'session/set_model':
      MODELS.currentModelId = m.params.modelId;
      return reply({});
    case 'session/set_config_option':
      if (m.params.configId !== 'effort') return send({ id: m.id, error: { code: -32602, message: `unknown config ${m.params.configId}` } });
      effort = m.params.value;
      update({ sessionUpdate: 'config_option_update', configOptions: configOptions() });
      return reply({ configOptions: configOptions() });
    case 'session/set_mode':
      mode = m.params.modeId;
      update({ sessionUpdate: 'current_mode_update', currentModeId: mode });
      return reply({});
    case 'session/prompt': {
      const text = (m.params.prompt ?? []).map((b) => b.text ?? '').join('');
      const stopReason = await play(text);
      return reply({ stopReason });
    }
    case 'session/cancel':
      cancelled = true;
      for (const [id, resolve] of waiting) {
        resolve({ outcome: { outcome: 'cancelled' } });
        waiting.delete(id);
      }
      return;
    default:
      if (m.id !== undefined) send({ id: m.id, error: { code: -32601, message: `${m.method} not supported` } });
  }
});
rl.on('close', () => process.exit(0));
