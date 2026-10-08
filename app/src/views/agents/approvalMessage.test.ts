import { describe, expect, it } from 'vitest';
import { fieldsLine, oneLine, splitMessage } from './approvalMessage';

const cmd =
  "cd ../Alethiometer-archive/app/src-tauri && python3 - <<'EOF'\nimport json\nprint(json.dumps({'a': 1}))\nEOF";
const gate = `tool         : bash\nrule         : *\ncommand      : python3\nfull command : ${cmd}`;

describe('splitMessage', () => {
  it("keeps only what a gate's field list adds to the card (pi's prompt)", () => {
    const m = splitMessage(gate, cmd, 'bash');
    expect(m.text).toBe('');
    expect(m.fields).toEqual([{ label: 'rule', value: '*' }]);
    expect(fieldsLine(m.fields)).toBe('rule *');
  });

  it('shows nothing when every field repeats the card', () => {
    const m = splitMessage(`tool : bash\ncommand : python3\nfull command : ${cmd}`, cmd, 'bash');
    expect(m).toEqual({ fields: [], text: '' });
  });

  it('matches the shape, whatever the agent: other fields stay, joined on one line', () => {
    const m = splitMessage('tool : bash\nrule : git push *\nmode : strict', 'git push origin main', 'bash');
    expect(fieldsLine(m.fields)).toBe('rule git push * · mode strict');
  });

  it('does not drop a one-character value because the command happens to contain it', () => {
    expect(splitMessage('rule : *', 'ls *.md', 'bash').fields).toEqual([{ label: 'rule', value: '*' }]);
  });

  it('leaves free text alone, as sent', () => {
    const text = 'I need to delete the build folder before the clean.\nDangerous command: deletes files. Allow?';
    expect(splitMessage(text, 'rm -rf build', 'bash')).toEqual({ fields: [], text });
  });

  it('does not read a sentence with a colon as a field list (a gate sets its colons off with a space)', () => {
    const text = 'Dangerous command: deletes files. Allow?';
    expect(splitMessage(text, 'rm -rf build', 'bash')).toEqual({ fields: [], text });
  });

  it('treats a field list whose command was re-wrapped as free text (no fuzzy matching)', () => {
    const wrapped = 'tool : bash\nfull command : cd x &&\n  python3 y.py';
    expect(splitMessage(wrapped, 'cd x && python3 y.py', 'bash')).toEqual({ fields: [], text: wrapped });
  });

  it('is empty for no message', () => {
    expect(splitMessage('  \n', 'ls', 'bash')).toEqual({ fields: [], text: '' });
  });
});

describe('oneLine', () => {
  it('collapses whitespace and reads line breaks as a dot', () => {
    expect(oneLine('  Dangerous:   deletes files.\n\nAllow?  ')).toBe('Dangerous: deletes files. · Allow?');
  });
});
