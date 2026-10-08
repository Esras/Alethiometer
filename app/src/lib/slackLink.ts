import type { Message } from '../views/incoming/types';

/** A signal's Slack URL pointed at one message: the given top-level message, or (`reply`) one of its thread replies. Anything else keeps `url`. */
export function messageUrl(url: string, m: Message | undefined, reply?: Message): string {
  const key = (reply ?? m)?.key;
  const re = /^(https:\/\/[^/]+\/archives\/([A-Z0-9]+)\/p)\d{7,}$/;
  if (!key || !m?.key || !/^\d+\.\d+$/.test(key) || !re.test(url)) return url;
  return url.replace(
    re,
    (_, base, ch) => `${base}${key.replace('.', '')}${reply ? `?thread_ts=${m.key}&cid=${ch}` : ''}`,
  );
}
