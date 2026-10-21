import { describe, expect, it } from 'vitest';
import { messageUrl, newest } from './slackLink';
import type { Message } from '../views/incoming/types';

const msg = (key: string, read: boolean, replies: Message[] = []): Message =>
  ({ id: Number(key), key, time: '', author: '', text: '', who: {}, read, replies }) as unknown as Message;

describe('newest', () => {
  it("opens the thread with the new reply, not the last message's thread", () => {
    const updated = msg('100.000100', true, [msg('100.000200', true), msg('300.000100', false)]);
    const later = msg('200.000100', true, [msg('200.000200', true)]);
    const [m, r] = newest([updated, later]);
    expect([m?.key, r?.key]).toEqual(['100.000100', '300.000100']);
    expect(messageUrl('https://x.slack.com/archives/C1/p100000100', m, r)).toBe(
      'https://x.slack.com/archives/C1/p300000100?thread_ts=100.000100&cid=C1',
    );
  });

  it('falls back to the newest when everything was read', () => {
    const [m, r] = newest([msg('100.000100', true), msg('200.000100', true)]);
    expect([m?.key, r]).toEqual(['200.000100', undefined]);
  });
});
