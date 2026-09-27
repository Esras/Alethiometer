// Keyboard-first: views register handlers while mounted; the newest wins.
// A handler returns true when it consumed the key.

export type KeyHandler = (e: KeyboardEvent) => boolean | void;

const stack: KeyHandler[] = [];

/** Call during component init. Unregisters on destroy. */
export function useKeys(handler: KeyHandler) {
  $effect(() => {
    stack.push(handler);
    return () => {
      const i = stack.lastIndexOf(handler);
      if (i >= 0) stack.splice(i, 1);
    };
  });
}

export function dispatchKey(e: KeyboardEvent): boolean {
  for (let i = stack.length - 1; i >= 0; i--) {
    if (stack[i](e)) {
      e.preventDefault();
      return true;
    }
  }
  return false;
}

/** True when focus is in a text field, so single-letter shortcuts should not fire. */
export function isTyping(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  return t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable;
}

/** ⌘ on macOS, Ctrl elsewhere. */
export function mod(e: KeyboardEvent): boolean {
  return e.metaKey || e.ctrlKey;
}

/** Plain key with no modifiers, not while typing. */
export function plain(e: KeyboardEvent, key: string): boolean {
  return e.key === key && !e.metaKey && !e.ctrlKey && !e.altKey && !isTyping(e);
}
