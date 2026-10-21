export const TOAST_MS = 8000;

export type Toast = { id: number; text: string; at: number; ok: boolean };

export const toasts = $state<{ list: Toast[] }>({ list: [] });

let next = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

export function toast(e: unknown, ok = false) {
  const text = typeof e === 'string' ? e : String((e as Error)?.message ?? e);
  const same = toasts.list.find((t) => t.text === text);
  const id = same?.id ?? next++;
  if (!same) toasts.list.push({ id, text, at: Date.now(), ok });
  hold(id, false);
}

export function closeToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  toasts.list = toasts.list.filter((t) => t.id !== id);
}

export function hold(id: number, on: boolean) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  if (!on)
    timers.set(
      id,
      setTimeout(() => closeToast(id), TOAST_MS),
    );
}
