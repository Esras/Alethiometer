#!/usr/bin/env node
// After `tauri build --bundles app` (npm run build:mac): put the agent host
// (app/agentd, release build) into Alethiometer.app/Contents/MacOS next to
// the app, and re-sign the bundle with tauri.conf.json's signingIdentity
// ("-" is ad-hoc). The app copies it into its data directory and runs that
// copy, so a running agent host is never the file inside the bundle.

import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, readFileSync, chmodSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const conf = JSON.parse(readFileSync(`${root}src-tauri/tauri.conf.json`, 'utf8'));
const identity = conf.bundle?.macOS?.signingIdentity ?? '-';
const app = `${root}src-tauri/target/release/bundle/macos/${conf.productName}.app`;
const bin = `${root}agentd/target/release/alethiometer-agentd`;

if (process.platform !== 'darwin') {
  console.log('bundle-agentd: not macOS; the app finds the agent host in app/agentd/target (see docs/agents.md)');
  process.exit(0);
}
if (!existsSync(app)) throw new Error(`no app bundle at ${app}: run tauri build first`);
if (!existsSync(bin)) execFileSync('cargo', ['build', '--release', '--manifest-path', `${root}agentd/Cargo.toml`], { stdio: 'inherit' });

const dest = `${app}/Contents/MacOS/alethiometer-agentd`;
copyFileSync(bin, dest);
chmodSync(dest, 0o755);
execFileSync('codesign', ['--force', '--sign', identity, dest], { stdio: 'inherit' });
// Adding a file breaks the bundle's seal; sign it again.
execFileSync('codesign', ['--force', '--sign', identity, app], { stdio: 'inherit' });
console.log(`bundle-agentd: ${dest} (signed ${identity === '-' ? 'ad-hoc' : identity})`);
