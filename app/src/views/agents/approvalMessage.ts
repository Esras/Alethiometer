// The text an agent (or a gate in it) sends with a permission request, split for
// the approval card.
//
// A gate's prompt can be a field list that restates the gated call:
//
//   tool         : bash
//   rule         : *
//   command      : python3
//   full command : cd ../x && python3 - <<'EOF' …
//
// Most of that is on the card already (the tool beside the title, the command in
// its block and in Parsed). This matches the `label : value` shape, not any one
// agent by name: pi's gate is the one seen so far. If every line has the shape,
// the fields that repeat the card are dropped and the rest (`rule : *`) is
// returned for the header; anything else is free text and comes back untouched.
// Nothing is rewritten: a field either shows as sent or isn't shown.

export type ApprovalMessage = {
  /** Fields of a `label : value` list that say something the card doesn't. */
  fields: { label: string; value: string }[];
  /** Free text, as sent. Empty when the message was a field list. */
  text: string;
};

/** A short label of letters and spaces, then a colon set off by a space (a gate pads its labels to align them; prose reads `Note: x`), then a value. */
const FIELD = /^([A-Za-z][A-Za-z ]{0,23}?)\s+:\s*(\S.*)$/;
/** A value this short is inside too many commands to say anything (`*` is in every glob). */
const MIN_INSIDE = 3;
const HERE = '\u0000';

const squash = (s: string) => s.replace(/\s+/g, ' ').trim();

/**
 * @param message the request's text
 * @param command the command the card shows
 * @param tool the tool's name as the card shows it (`bash`)
 */
export function splitMessage(message: string, command: string, tool: string): ApprovalMessage {
  const text = message.trim();
  if (!text) return { fields: [], text: '' };
  const cmd = command.trim();
  // A multi-line command restated in a field would read as lines that aren't fields.
  const flat = cmd.length >= MIN_INSIDE ? text.replaceAll(cmd, HERE) : text;
  const lines = flat
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean);
  const parsed = lines.map((l) => FIELD.exec(l));
  if (parsed.some((m) => !m)) return { fields: [], text: message };

  const flatCmd = squash(cmd);
  const repeats = (value: string) => {
    const v = squash(value);
    return (
      v.includes(HERE) ||
      v.toLowerCase() === tool.trim().toLowerCase() ||
      (!!flatCmd && (v === flatCmd || v.includes(flatCmd))) ||
      (v.length >= MIN_INSIDE && flatCmd.includes(v))
    );
  };
  const fields = parsed.map((m) => ({ label: squash(m![1]), value: squash(m![2]) })).filter((f) => !repeats(f.value));
  return { fields, text: '' };
}

/** `rule * · mode strict`: what the header shows for the fields left. */
export const fieldsLine = (fields: ApprovalMessage['fields']) => fields.map((f) => `${f.label} ${f.value}`).join(' · ');

/** Free text on one line: runs of whitespace collapse, line breaks read ` · `. */
export const oneLine = (text: string) => text.split('\n').map(squash).filter(Boolean).join(' · ');
