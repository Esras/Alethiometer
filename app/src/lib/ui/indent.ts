// Tab / Shift+Tab in a note textarea indent and outdent lines (tab characters, as Obsidian does)
// instead of moving focus. execCommand keeps the browser's undo stack and fires `input`.
export function indentKeys(e: KeyboardEvent) {
  if (e.key !== 'Tab' || e.metaKey || e.ctrlKey || e.altKey) return;
  e.preventDefault();
  const t = e.currentTarget as HTMLTextAreaElement;
  const { value, selectionStart: a, selectionEnd: b } = t;
  if (!e.shiftKey && a === b) {
    document.execCommand('insertText', false, '\t');
    return;
  }
  const start = value.lastIndexOf('\n', a - 1) + 1;
  const end = b > a && value[b - 1] === '\n' ? b - 1 : b;
  const next = value
    .slice(start, end)
    .split('\n')
    .map((l) => (e.shiftKey ? l.replace(/^(\t| {1,4})/, '') : `\t${l}`))
    .join('\n');
  t.setSelectionRange(start, end);
  document.execCommand('insertText', false, next);
  t.setSelectionRange(start, start + next.length);
}
