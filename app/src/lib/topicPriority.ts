// A topic's priority (Priorities, app/docs/priorities.md): H, M, L or none,
// set by you. `p` cycles it on the Topics index and on a topic page.
import { call } from './api';

export type TopicPriority = 'H' | 'M' | 'L' | null;

const ORDER: TopicPriority[] = ['H', 'M', 'L', null];
export const PRIORITY_NAME: Record<'H' | 'M' | 'L', string> = { H: 'high', M: 'medium', L: 'low' };

export const nextPriority = (p: TopicPriority): TopicPriority => ORDER[(ORDER.indexOf(p) + 1) % ORDER.length];

/** Sets the next one; the live queries bring it back. */
export function cyclePriority(id: number, current: TopicPriority): Promise<unknown> {
  return call('topics.set_priority', { id, priority: nextPriority(current) });
}
