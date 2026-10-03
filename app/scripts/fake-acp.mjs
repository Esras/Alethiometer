#!/usr/bin/env node
// A scripted ACP v1 agent for UI development and screenshots: speaks the agent
// side over stdio like claude-agent-acp (shapes from its recorded scenarios in
// src-tauri/tests/fixtures/acp). Each prompt plays one turn: thinking, reads,
// a subagent, an edit and a shell command that ask for permission, output.
//
//   "agents": { "harnesses": [{ "key": "fake", "label": "Claude Code", "command": "node",
//               "args": ["<repo>/app/scripts/fake-acp.mjs"] }] }
//
// FAKE_ACP_FAST=1 drops the delays (tests).

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
const MODES = { currentModeId: 'default', availableModes: [{ id: 'default', name: 'Default' }, { id: 'acceptEdits', name: 'Accept Edits' }, { id: 'plan', name: 'Plan' }] };

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

async function play(text) {
  turn++;
  cancelled = false;
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
  update({ sessionUpdate: 'usage_update', used, size: 200000, cost: { amount: 0.41 * turn, currency: 'USD' }, _meta: { '_claude/model': 'claude-sonnet-4-6' } });
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
      return reply({ protocolVersion: 1, agentCapabilities: { loadSession: true, sessionCapabilities: { resume: {}, list: {} } }, authMethods: [] });
    case 'session/new':
      session = `fake-${Date.now().toString(36)}`;
      return reply({ sessionId: session, modes: MODES, configOptions: [{ id: 'model', currentValue: 'claude-sonnet-4-6', options: [] }] });
    case 'session/resume':
    case 'session/load':
      session = m.params.sessionId;
      return reply({ modes: MODES, configOptions: [{ id: 'model', currentValue: 'claude-sonnet-4-6', options: [] }] });
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
