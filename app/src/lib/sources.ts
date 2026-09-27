// Per-source display: label and the muted dot color used on the chronology line.

export const SOURCES: Record<string, { label: string; dot: string }> = {
  gcal: { label: 'Google Cal', dot: '#3f9a9a' },
  slack: { label: 'Slack', dot: '#b0567a' },
  jira: { label: 'Jira', dot: '#4f7ec2' },
  github: { label: 'GitHub', dot: '#9a8f84' },
  obsidian: { label: 'Obsidian', dot: '#8a6fb8' },
  gmail: { label: 'Gmail', dot: '#c9744a' },
  sheets: { label: 'Sheets', dot: '#4f9a5f' },
  notion: { label: 'Notion', dot: '#8f897e' },
  marvin: { label: 'Marvin', dot: '#c0645a' },
  protoncal: { label: 'Proton Cal', dot: '#6d5bb8' },
  protonmail: { label: 'Proton Mail', dot: '#6d5bb8' },
  helm: { label: 'Notes', dot: '#6f9a7c' },
};

export function sourceLabel(kind: string): string {
  return SOURCES[kind]?.label ?? kind;
}
