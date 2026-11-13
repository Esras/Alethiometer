// Your own messages keep the indentation you typed when they're shown as Markdown: CommonMark drops a
// paragraph line's leading spaces (and reads four as a code block). Outside fences, a line's leading
// whitespace becomes no-break spaces, which Markdown leaves alone; list items keep theirs, since
// that's how lists nest. Display only: the agent gets the text as typed.
const NBSP = '\u00a0';
const LIST = /^\s*([-*+]|\d+[.)])\s/;
const FENCE = /^\s*(`{3,}|~{3,})/;

export function keepIndent(text: string): string {
  let fence: string | null = null;
  return text
    .split('\n')
    .map((line) => {
      const f = FENCE.exec(line);
      if (fence) {
        if (f && f[1][0] === fence[0] && f[1].length >= fence.length) fence = null;
        return line;
      }
      if (f) {
        fence = f[1];
        return line;
      }
      if (LIST.test(line)) return line;
      const lead = /^[ \t]+/.exec(line)?.[0];
      if (!lead || lead.length === line.length) return line;
      return lead.replace(/\t/g, '    ').replace(/ /g, NBSP) + line.slice(lead.length);
    })
    .join('\n');
}
