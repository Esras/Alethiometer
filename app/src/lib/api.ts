// One entry point to the Rust core: `call(cmd, args)`.
// In the Tauri window it goes through `invoke`; in a plain browser (UI dev
// without WebKitGTK) it goes to the dev server (`npm run devserver`).

import { invoke } from '@tauri-apps/api/core';

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const DEV_SERVER = import.meta.env.VITE_ALETHIOMETER_DEVSERVER ?? 'http://127.0.0.1:1421';

export async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri) return invoke<T>('call', { cmd, args });
  const res = await fetch(`${DEV_SERVER}/call/${cmd}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args),
  });
  const body = await res.json();
  if (!res.ok) throw new Error(body.error ?? res.statusText);
  return body as T;
}

/** Open an item in its source app / browser. */
export function openExternal(url: string) {
  window.open(url, '_blank');
}
