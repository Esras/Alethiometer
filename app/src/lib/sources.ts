// Per-source display: label and the muted dot color used on the chronology line.

import { ui } from './app.svelte';

export const SOURCES: Record<string, { label: string; dot: string }> = {
  gcal: { label: 'Google Cal', dot: '#3f9a9a' },
  slack: { label: 'Slack', dot: '#b0567a' },
  teams: { label: 'Teams', dot: '#6f72b8' },
  jira: { label: 'Jira', dot: '#4f7ec2' },
  github: { label: 'GitHub', dot: '#9a8f84' },
  obsidian: { label: 'Obsidian', dot: '#8a6fb8' },
  gmail: { label: 'Gmail', dot: '#c9744a' },
  sheets: { label: 'Sheets', dot: '#4f9a5f' },
  notion: { label: 'Notion', dot: '#8f897e' },
  marvin: { label: 'Marvin', dot: '#c0645a' },
  protoncal: { label: 'Proton Cal', dot: '#6d5bb8' },
  protonmail: { label: 'Proton Mail', dot: '#6d5bb8' },
  notes: { label: 'Notes', dot: '#6f9a7c' },
  agents: { label: 'Agents', dot: '#8ec07c' },
};

export function sourceLabel(kind: string): string {
  return SOURCES[kind]?.label ?? kind;
}

/** Chat sources (the core's `CHAT_KINDS`): `r` replies, `s` opens the split review on any signal with 2+ messages. */
export function isChat(kind: string): boolean {
  return ui.info?.chat_kinds.includes(kind) ?? false;
}
