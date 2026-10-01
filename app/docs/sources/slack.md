# Slack

Reads your DMs, group DMs, the channels you follow, threads you're in and mentions of you. Every new message becomes part of a signal in Incoming, yours included: one per thread, and one per conversation for its top-level messages, e.g. `#inc-2291 · 42 new · 09:05–10:40`, which expands to those messages. Incoming is where messages get filed to topics, so reading them in Slack changes nothing here; only triage does (confirm, move, dismiss or Seen). Confirming a top-level signal marks it read in Slack. Writes back (through the outbox) mark read, thread replies you type, and reactions.

Alethiometer talks to Slack as **you**, through a small Slack app you create for
yourself. Nothing is hosted; the app only exists to issue tokens.

## 1. Create the app

1. Go to <https://api.slack.com/apps> → **Create New App** → **From a manifest**, pick your workspace.
2. Paste this manifest (YAML):

```yaml
_metadata:
  major_version: 1
display_information:
  name: Alethiometer
  description: Personal work assistant. Reads my DMs, mentions and followed channels.
oauth_config:
  scopes:
    user:
      - channels:history   # read followed public channels
      - groups:history     # read followed private channels
      - im:history         # read DMs
      - mpim:history       # read group DMs
      - channels:read      # list channels you're in, resolve "#name"
      - groups:read
      - im:read            # list your DMs
      - mpim:read
      - users:read         # show names instead of user ids
      - users:read.email   # profile emails: match senders to People notes, colour by email
      - search:read        # mentions in channels you don't follow
      - channels:write     # mark read (conversations.mark)
      - groups:write
      - im:write
      - mpim:write
      - chat:write         # reply in a thread, as you
      - reactions:write    # add a reaction
settings:
  event_subscriptions:
    user_events:
      - message.channels
      - message.groups
      - message.im
      - message.mpim
  interactivity:
    is_enabled: false
  org_deploy_enabled: false
  socket_mode_enabled: true
  token_rotation_enabled: false
```

3. **Basic Information → App-Level Tokens → Generate Token and Scopes**: add the
   scope `connections:write`, generate. This is the `xapp-…` token (Socket Mode).
4. **OAuth & Permissions → Install to Workspace**, approve. Copy the
   **User OAuth Token** (`xoxp-…`).

Your workspace may require an admin to approve the app before you can install it.
If **Install** turns into **Request to Install**, an admin has to approve it first.

**Already installed?** Adding `users:read.email` means adding it under **OAuth & Permissions → User Token Scopes** and choosing **Reinstall to Workspace**; the user token (`xoxp-…`) can change, so store it again. Without the scope Slack senders still work, matched and coloured by user id (see `docs/people.md`).

Skip anything you don't want: without `search:read` set `"mentions": false`;
without the write scopes the matching outbox ops fail visibly; without the
`xapp-` token Alethiometer falls back to polling (see below).

## 2. Store the tokens

Secrets never go in `settings.json`. On Linux they live in
`<data dir>/secrets.json` (mode 0600); on the Mac they will move to the Keychain.

| Secret name | Value |
|---|---|
| `slack.user_token` | `xoxp-…` User OAuth Token (required) |
| `slack.app_token` | `xapp-…` app-level token with `connections:write` (optional but recommended: the live feed) |

For a second workspace, add another source and point it at other secret names
with `config.user_token_secret` / `config.app_token_secret`.

## 3. Add the source

In `settings.json` → `sources`:

```json
{
  "kind": "slack",
  "tag": "work",
  "name": "Slack",
  "poll_secs": 300,
  "config": {
    "channels": ["#inc-2291", "#eng-leads", "C0123ABCD"],
    "dms": true,
    "mentions": true,
    "lookback_hours": 72,
    "max_dms": 50,
    "thread_days": 90,
    "max_threads": 300,
    "catchup_hours": 3
  }
}
```

| Config | Default | Meaning |
|---|---|---|
| `channels` | `[]` | Channels you follow: `#name` (a channel you're in) or channel id (also works for a public channel you haven't joined). An unknown name fails the sync with a clear error. `"all"` follows every channel you're a member of (the list is refreshed on each catch-up). Each followed channel costs a `conversations.info` and a `conversations.history` per catch-up, so with ~100 channels a catch-up takes a few minutes at Slack's Tier 3 limit; use the `xapp-` token so that only reconnects and the `catchup_hours` pass do that. |
| `muted` | `[]` | Channels (`#name` or id) still read and stored like any other, but never new in Incoming: their items arrive already seen, so no AI triage and no mark read in Slack. Applies to everything in the channel, threads and mentions of you included. Find them with search. |
| `dms` | `true` | Read your DMs and group DMs. |
| `mentions` | `true` | Use `search.messages` (needs `search:read`) on catch-up: mentions of you in channels you don't follow, and threads you replied in that Alethiometer doesn't know yet. |
| `lookback_hours` | `72` | The furthest back anything is read: bounds the first sync, a catch-up after Alethiometer was off, and a channel you haven't read for weeks. |
| `max_dms` | `50` | DMs and group DMs checked per catch-up, most recently active first. Without the `xapp-` token that is every poll, and **DMs beyond it are not read** (except when a mention search finds them). |
| `thread_days` | `90` | A thread you're in is tracked this long after its last reply. |
| `max_threads` | `300` | At most this many tracked threads; the least recently active are dropped first. |
| `catchup_hours` | `3` | While the socket stays connected, run the full catch-up (as on reconnect) at least this often, as a safety net for events Slack didn't deliver. |
| `user_token_secret`, `app_token_secret` | `slack.user_token`, `slack.app_token` | Secret names. |

## What is read

**Every message is captured; triage here decides when a signal is done.** Slack's read marker plays no part. A signal (a *run*) holds a stream's messages since its last triaged run, yours included, bounded by `lookback_hours` on the first read. Until you triage it, every new message in that stream joins it and brings it back as new. Once you've triaged it, the next message starts a new run.

- **Top level.** In a DM, group DM or followed channel, the run's top-level messages form one item (`DM · Priya`, `#inc-2291`, `Group DM · alice, bob`; `kind` `message` for one message, else `thread`). A DM or channel mixes topics, so a run isn't assumed to be one topic: the AI assigns topics per message (a split into parts, each with its own topic), seeing the messages before the run and the topics you filed them under as context. The split review is where you correct that.
- **Threads.** Each thread is one item for good (`#inc-2291 · thread: <first words of the parent>`), and one topic: the parent and every reply belong together. New replies (yours too) start a new run on the same item, which comes back as new holding only those replies; since it keeps its topic, it shows under that topic as an update (**Seen**).
- **Mentions** of you in channels you don't follow: one item each (`#random · mention`). A mention inside a thread goes to that thread's item. Confirming one marks that channel read in Slack up to the mention (see "What is written").
- **Context messages.** Each item also stores, faded and before its own messages: for a thread, its first message and up to 3 replies before the run; at top level, up to 3 messages before the run (one extra `conversations.history` call when the item is written). They're never counted in `msg_count`, never split off, and the AI reads them as context only. Mentions have none.
- Join/leave/topic messages are ignored. Text is converted from Slack markup to plain text with names resolved. Messages with no `text` (Datadog and other integrations) use their attachments (pretext, title, text, `Title: value` fields, else the fallback) and Block Kit blocks. A bot without a name on the message is named with `bots.info`. Every message keeps the sender's Slack user id (`item_messages.author_id`). Up to 50 messages are kept per item; `msg_count` counts them all.

**Threads you're in are never too old.** Alethiometer tracks a thread (in its cursor,
for `thread_days` after its last reply) when it's in a DM or followed channel,
or when you started it, replied, were mentioned or follow it in Slack. Replies
are caught however old the parent is:

- live, the socket delivers every reply; one in a tracked thread, in a thread
  you started (`parent_user_id`), by you, or mentioning you is fetched at once;
- on catch-up, every tracked thread is checked with `conversations.replies`
  (without the socket: threads active in the last day every poll, quieter ones
  hourly), threads with new replies under recent parents are found in channel
  history, and two searches discover threads Alethiometer didn't know: mentions of you
  and your own replies since the last catch-up.

Limits: with the socket down *and* no search, a first reply to a thread
Alethiometer has never seen, in a channel you don't follow, under a parent older than a
day, and neither by nor mentioning you, is missed. Search matches only arrive
once Slack has indexed them (usually seconds).

**How it's fetched: socket first, polling as catch-up.**

- **With the `xapp-` token** Alethiometer keeps a Socket Mode connection open. Each
  event is acknowledged; a DM, a message in a followed channel, a relevant
  thread reply or a mention flags that conversation/thread, and a sync a few
  seconds later (bursts coalesced) fetches only what was flagged. Followed
  channels you haven't joined send no events and are read every poll. Every 2 minutes the streams of open items are re-read as well, in case an event was lost.
- **Catch-up** runs when the connection opens (at startup and after any
  disconnect): followed channels, up to `max_dms` DMs, open items, all tracked
  threads and the two searches. A DM costs one `conversations.info`; its
  history is read only when that reports something new. Slack has no
  documented call that returns unread state for all conversations at once
  (the Slack client uses undocumented ones, which Alethiometer doesn't), so it's one
  small call per DM.
- **Periodic catch-up** while the socket stays connected: every
  `catchup_hours` (default 3) the next sync is the same catch-up as on
  reconnect. Socket Mode delivery is best-effort: Slack retries an event that
  wasn't acknowledged a few times and then drops it, and events can be lost in
  a hiccup on Alethiometer's side without the connection dropping. The pass
  stays as cheap as a reconnect: DMs are read only when `conversations.info`
  reports something new, followed channels from the last day, plus
  one `conversations.replies` per tracked thread and the two searches. It
  runs on the regular `poll_secs` schedule once due; when it last completed
  is kept in the cursor (`caught_up`).
- **Without the `xapp-` token** every poll (`poll_secs`) is a catch-up, capped
  to the `max_dms` most recently active DMs (activity Alethiometer has seen, then the
  conversation's creation time; Slack lists no last-activity time for DMs).

**Item fields.** External ids: `<channel>/<first unread ts>` (top level),
`<channel>/thread/<thread ts>`, `<channel>/mention/<ts>`. `raw_json` holds
`channel`, `first_ts`, `last_ts`, `thread_ts` (threads), `mark_ts` (top-level items and mentions in conversations you're in: what "mark read" sends; the newest message of the run, or the mention itself), and `msgs`: each stored message's ts (context messages first), in the same order as the item's messages, so a split by
topic (AI triage, or selecting messages in Unsorted) can move some of them to
a child item `<external id>#<n>`; later syncs don't bring moved messages back.
A child keeps `mark_ts` only if it holds the newest message, and a split
item's conversation is marked read only once every part has left Incoming.

The cursor stores only timestamps, ids and thread titles, never message text:
per-conversation newest ts, tracked threads, open items, the last message of each stream's newest triaged run (`closed`) and the search positions. Before each sync, open items you've triaged since are moved from `open` to `closed`. Upgrading from the first version of this connector resets it; old
"window" items stay as they are.

**Rate limits.** A 429 waits for `Retry-After` (up to 2 minutes, 3 tries per
call), then the sync fails and the next one retries. `conversations.info` and
`conversations.replies` are Tier 3 (about 50/minute); a catch-up with many DMs
or tracked threads is therefore slow but completes. Slack's 2025 limits on
`conversations.history`/`replies` for non-Marketplace apps (1 request/minute)
do not apply to internal, customer-built apps like this one.

## What is written (outbox)

| Op | Payload | Slack method |
|---|---|---|
| `slack.mark_read` | `{channel, ts}`, from the item's `raw_json.channel` / `raw_json.mark_ts` | `conversations.info`, then `conversations.mark`: the conversation is read up to `ts` in Slack too. Queued when you confirm a top-level item or a mention. Never moves the read marker backwards (below). When confirmed, the item is marked seen locally. |
| `slack.reply` | `{channel, thread_ts, text}` | `chat.postMessage` as you. `text` is exactly what you typed. |
| `slack.react` | `{channel, ts, name}` (`eyes` or `:eyes:`) | `reactions.add`. An existing identical reaction counts as done. |

**Threads can't be marked read from Alethiometer.** Slack's public Web API has no
method to set a thread's read state (`conversations.mark` covers only the
top level; the Slack client uses an undocumented internal method, which Alethiometer
deliberately doesn't call). Thread items therefore carry no `mark_ts` and
confirming one only marks it seen in Alethiometer; the thread stays unread in Slack
until you open it there. A `slack.mark_read` payload with a `thread_ts` fails
with "Slack has no API to mark a thread read".

**Mentions in channels you don't follow** are marked read: confirming one
marks that channel read up to the mention, which also marks everything before
it read. That's intended: in a channel you don't follow, the mention is the
unread part that matters. Only channels you're a member of get a `mark_ts`
(without membership Slack keeps no read marker for you).

**The read marker only moves forward.** `conversations.mark` sets the marker
to exactly the given `ts`, even if that is earlier than where you already are
in Slack. So `slack.mark_read` first reads the current marker
(`conversations.info`); if Slack is already read up to `ts` or later (you
read on in Slack since the item was synced, or the op waited in the outbox),
it marks nothing and confirms (receipt `already: <last_read>`). The check runs
at send time, not when the op is queued, because only Slack knows the marker
now: the cursor's copy is as old as the last poll. A read in Slack in the
second between the two calls could still be pulled back to `ts`.

Slack answers most errors with HTTP 200 and `ok: false`. Alethiometer treats those as
failures and shows Slack's error code (e.g. `channel_not_found`,
`missing_scope (needs scope im:write)`) on the outbox row, which you can retry
with ⌘R.
