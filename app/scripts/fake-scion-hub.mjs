#!/usr/bin/env node
// A stand-in Scion Hub for UI development: serves the hand-written fixtures in
// src-tauri/tests/fixtures/scion (shaped from Scion's Go types, not recorded
// from a live Hub), and keeps posts in memory so rooms and DMs answer.
//
//   node scripts/fake-scion-hub.mjs [port]      (default 1431)
//   source: { "kind": "scion", "name": "hub", "tag": "work",
//             "config": { "hub_url": "http://127.0.0.1:1431" } }  + any value for the scion.token secret

import http from 'node:http';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const dir = fileURLToPath(new URL('../src-tauri/tests/fixtures/scion/', import.meta.url));
const load = (f) => JSON.parse(readFileSync(dir + f, 'utf8'));
const port = Number(process.argv[2] ?? 1431);

const conversations = load('conversations.json');
const room = load('room_messages.json');
const dm = load('dm_messages.json');
const agents = load('agents.json');
const me = load('me.json');
let n = 0;

function post(conv, msg, recipient, recipientId) {
  const list = conv.kind === 'group' ? room : dm;
  const now = new Date().toISOString();
  list.items.unshift({
    ...list.items.find((m) => String(m.sender).startsWith('user:')),
    id: `f9${String(++n).padStart(6, '0')}-0000-4000-8000-000000000000`,
    msg,
    type: 'instruction',
    recipient,
    recipientId,
    dispatchState: 'dispatched',
    dispatchedAt: now,
    createdAt: now,
  });
  conv.lastActivityAt = now;
}

http
  .createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    const p = url.pathname;
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      const send = (code, v) => {
        res.writeHead(code, { 'content-type': 'application/json' });
        res.end(JSON.stringify(v));
      };
      const get = req.method === 'GET';
      let m;
      if (get && p === '/api/v1/auth/me') return send(200, me);
      if (get && p === '/api/v1/projects') return send(200, load('projects.json'));
      if (get && (m = p.match(/^\/api\/v1\/projects\/([^/]+)\/agents$/)))
        return send(200, { ...agents, agents: agents.agents.filter((a) => !a.projectId || a.projectId === m[1]) });
      if (get && (m = p.match(/^\/api\/v1\/projects\/([^/]+)\/providers$/))) return send(200, load('providers.json'));
      if (get && p === '/api/v1/conversations') {
        const pid = url.searchParams.get('include_project_groups');
        return send(200, { conversations: conversations.conversations.filter((c) => !c.projectId || c.projectId === pid) });
      }
      if (get && (m = p.match(/^\/api\/v1\/conversations\/([^/]+)\/messages$/))) {
        const c = conversations.conversations.find((c) => c.id === m[1]);
        return send(200, c?.kind === 'group' ? room : dm);
      }
      if (get && (m = p.match(/^\/api\/v1\/conversations\/([^/]+)$/))) return send(200, load('conversation.json'));
      if (get && p.endsWith('/interagent')) return send(200, load('interagent.json'));
      if (get && p === '/api/v1/templates') return send(200, load('templates.json'));
      if (get && p.startsWith('/api/v1/templates/') && p.includes('/files/')) return send(200, load('template_file.json'));
      if (get && p === '/api/v1/harness-configs') return send(200, load('harness_configs.json'));
      if (get && (m = p.match(/^\/api\/v1\/agents\/([^/]+)$/))) return send(200, agents.agents.find((a) => a.id === m[1]) ?? {});
      if (req.method === 'DELETE' && (m = p.match(/^\/api\/v1\/agents\/([^/]+)$/))) {
        const before = agents.agents.length;
        agents.agents = agents.agents.filter((a) => a.id !== m[1]);
        return before === agents.agents.length ? send(404, { error: { code: 'not_found', message: 'no such agent' } }) : (res.writeHead(204), res.end());
      }
      const args = body ? JSON.parse(body) : {};
      if (!get && (m = p.match(/^\/api\/v1\/chat\/conversations\/([^/]+)\/messages$/))) {
        const c = conversations.conversations.find((c) => c.kind === 'group' && c.externalRef?.endsWith(m[1]));
        if (c) post(c, args.content ?? '', 'agent:planner', c.defaultAgentId);
        return send(200, load('chat_send.json'));
      }
      if (!get && (m = p.match(/^\/api\/v1\/agents\/([^/]+)\/message$/))) {
        const c = conversations.conversations.find((c) => c.kind === 'direct' && c.externalRef?.includes(m[1]));
        const a = agents.agents.find((a) => a.id === m[1]);
        if (c) post(c, args.structured_message?.msg ?? args.message ?? '', `agent:${a?.slug ?? 'agent'}`, m[1]);
        return send(200, load('agent_message.json'));
      }
      if (!get && p === '/api/v1/agents') return send(200, load('create_agent.json'));
      if (!get && p.endsWith('/participants')) return send(200, load('participant.json'));
      if (!get && /\/agents\/[^/]+\/(start|stop|restart|suspend|keys)$/.test(p)) return send(200, {});
      send(404, { error: { code: 'not_found', message: `fake hub has no ${req.method} ${p}` } });
    });
  })
  .listen(port, '127.0.0.1', () => console.error(`fake scion hub on http://127.0.0.1:${port}`));
