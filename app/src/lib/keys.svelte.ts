// Keyboard-first: views register handlers while mounted; the newest wins.
// A handler returns true when it consumed the key.
//
// A modal handler (Settings, the ⌘K palette) is a floor: keys it doesn't
// consume stop there instead of reaching the view underneath, which would
// otherwise act on a selection the user can't see. Pickers opened from a
// modal register after it, so they still come first.

export type KeyHandler = (e: KeyboardEvent) => boolean | void;

type Entry = { handler: KeyHandler; modal: boolean };

const stack: Entry[] = [];

/** Call during component init. Unregisters on destroy. */
export function useKeys(handler: KeyHandler, opts: { modal?: boolean } = {}) {
  $effect(() => {
    const entry: Entry = { handler, modal: !!opts.modal };
    stack.push(entry);
    return () => {
      const i = stack.lastIndexOf(entry);
      if (i >= 0) stack.splice(i, 1);
    };
  });
}

/**
 * The same push and pop without a component's init, for a Svelte action
 * (lib/ui/popover.ts). Returns the function that pops it.
 */
export function pushKeys(handler: KeyHandler, opts: { modal?: boolean } = {}): () => void {
  const entry: Entry = { handler, modal: !!opts.modal };
  stack.push(entry);
  return () => {
    const i = stack.lastIndexOf(entry);
    if (i >= 0) stack.splice(i, 1);
  };
}

export function dispatchKey(e: KeyboardEvent): boolean {
  for (let i = stack.length - 1; i >= 0; i--) {
    if (stack[i].handler(e)) {
      e.preventDefault();
      return true;
    }
    if (stack[i].modal) return false;
  }
  return false;
}

/** True when focus is in a text field, so single-letter shortcuts should not fire. */
export function isTyping(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  return t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable;
}

/** ⌘ on macOS, Ctrl elsewhere. */
export function mod(e: KeyboardEvent): boolean {
  return e.metaKey || e.ctrlKey;
}

/** Plain key with no modifiers, not while typing. */
export function plain(e: KeyboardEvent, key: string): boolean {
  return e.key === key && !e.metaKey && !e.ctrlKey && !e.altKey && !isTyping(e);
}

let pointer = { x: NaN, y: NaN };

export function pointerMoved(e: MouseEvent): boolean {
  if (e.screenX === pointer.x && e.screenY === pointer.y) return false;
  pointer = { x: e.screenX, y: e.screenY };
  return true;
}
