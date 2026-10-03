// One entry point to the Rust core: `call(cmd, args)`.
// In the Tauri window it goes through `invoke`; in a plain browser (UI dev
// without WebKitGTK) it goes to the dev server (`npm run devserver`), through
// Vite's `/call` proxy. The dev server refuses calls without X-Alethiometer.

import { invoke } from '@tauri-apps/api/core';

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const DEV_SERVER = import.meta.env.VITE_ALETHIOMETER_DEVSERVER ?? '';

export async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri) return invoke<T>('call', { cmd, args });
  const res = await fetch(`${DEV_SERVER}/call/${cmd}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-alethiometer': '1' },
    body: JSON.stringify(args),
  });
  const body = await res.json();
  if (!res.ok) throw new Error(body.error ?? res.statusText);
  return body as T;
}

/** Open an item in its source app / browser. */
export async function openExternal(url: string) {
  await call('app.open_url', { url });
}
