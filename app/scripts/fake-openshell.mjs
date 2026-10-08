#!/usr/bin/env node
// A stand-in for the `openshell` CLI (NVIDIA/OpenShell), for tests and UI
// work: a "sandbox" is a directory under FAKE_OPENSHELL_ROOT, and `sandbox
// exec` runs the command on this machine in it. Point Alethiometer at it:
//
//   "agents": { "openshell": { "command": "<repo>/app/scripts/fake-openshell.mjs" } }
//
// Answers the subcommands Alethiometer runs, with the JSON shapes of
// openshell-cli's gateway_to_json and sandbox_to_json: `gateway list`,
// `sandbox list|create|start|exec`. `create --upload <dir>` copies the
// directory in, as the real one does (into the working directory, under its
// own name). FAKE_OPENSHELL_LOG appends each call's arguments as a JSON line;
// FAKE_OPENSHELL_CREATE_MS makes `create` take that long (an image pull).

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const root = process.env.FAKE_OPENSHELL_ROOT ?? path.join(os.tmpdir(), 'fake-openshell');
let args = process.argv.slice(2);
if (process.env.FAKE_OPENSHELL_LOG) fs.appendFileSync(process.env.FAKE_OPENSHELL_LOG, JSON.stringify(args) + '\n');

let gateway = 'local';
const g = args.indexOf('-g');
if (g >= 0) {
  gateway = args[g + 1];
  args.splice(g, 2);
}
const box = (name) => path.join(root, gateway, name);
const flag = (name) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : undefined;
};
const all = (name) => args.flatMap((a, i) => (a === name ? [args[i + 1]] : []));
const fail = (msg) => {
  process.stderr.write(`Error: ${msg}\n`);
  process.exit(1);
};
const json = (v) => process.stdout.write(JSON.stringify(v, null, 2) + '\n');

const [group, cmd] = args;
if (group === 'gateway' && cmd === 'list') {
  json([
    { name: 'local', endpoint: 'http://127.0.0.1:17670', type: 'local', source: 'system', auth: 'plaintext', active: true, is_remote: false, remote_host: null, resolved_host: null },
    { name: 'remote', endpoint: 'https://openshell.example.com', type: 'remote', source: 'user', auth: 'edge', active: false, is_remote: true, remote_host: 'gw', resolved_host: null },
  ]);
} else if (group === 'sandbox' && cmd === 'list') {
  const dir = path.join(root, gateway);
  const names = fs.existsSync(dir) ? fs.readdirSync(dir) : [];
  json(names.map((name) => ({ id: name, name, workspace: 'default', phase: 'Ready', labels: {}, annotations: {} })));
} else if (group === 'sandbox' && cmd === 'create') {
  const name = flag('--name') ?? fail('--name is required here');
  await new Promise((r) => setTimeout(r, Number(process.env.FAKE_OPENSHELL_CREATE_MS ?? 0)));
  fs.mkdirSync(box(name), { recursive: true });
  for (const up of all('--upload')) {
    const [local, dest] = up.split(':');
    const into = dest ? path.resolve(box(name), dest) : path.join(box(name), path.basename(local));
    fs.cpSync(local, into, { recursive: true });
  }
  json({ id: name, name, phase: 'Ready', from: flag('--from') ?? '', providers: all('--provider') });
} else if (group === 'sandbox' && cmd === 'start') {
  if (!fs.existsSync(box(args[2] ?? ''))) fail(`sandbox '${args[2]}' not found`);
} else if (group === 'sandbox' && cmd === 'exec') {
  const dash = args.indexOf('--');
  if (dash < 0) fail('a command is required after --');
  const name = flag('-n') ?? flag('--name');
  if (!name || !fs.existsSync(box(name))) fail(`sandbox '${name}' not found`);
  const [prog, ...rest] = args.slice(dash + 1);
  if (prog === 'pwd') {
    process.stdout.write(box(name) + '\n');
  } else {
    const workdir = flag('--workdir');
    const env = { ...process.env };
    for (const kv of all('--env')) {
      const i = kv.indexOf('=');
      env[kv.slice(0, i)] = kv.slice(i + 1);
    }
    const child = spawn(prog, rest, { cwd: workdir ? path.resolve(box(name), workdir) : box(name), env, stdio: 'inherit' });
    child.on('exit', (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
    for (const s of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(s, () => child.kill(s));
  }
} else {
  fail(`fake-openshell doesn't do: ${args.join(' ')}`);
}
