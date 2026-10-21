import type { Message } from '../views/incoming/types';

/** The newest unread message or reply (else the newest), as messageUrl's `m` and `reply`. */
export function newest(messages: Message[]): [Message | undefined, Message | undefined] {
  const all = messages.flatMap((m) => [[m, undefined] as const, ...(m.replies ?? []).map((r) => [m, r] as const)]);
  const n = (x: (typeof all)[number]) => Number((x[1] ?? x[0]).key ?? 0);
  const unread = all.filter((x) => !(x[1] ?? x[0]).read);
  const best = (unread.length ? unread : all).reduce<(typeof all)[number] | undefined>(
    (a, x) => (!a || n(x) >= n(a) ? x : a),
    undefined,
  );
  return best ? [best[0], best[1]] : [undefined, undefined];
}

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
