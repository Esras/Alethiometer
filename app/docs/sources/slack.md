# Slack

Reads your DMs, group DMs, the channels you follow, threads you're in and
mentions of you. What you haven't read **in Slack** becomes a signal in
Incoming: one per thread, and one per conversation for its unread top-level
messages, e.g. `#inc-2291 · 42 new · 09:05–10:40`, which expands to those
messages. When you read them in Slack the signal stops being new; when you
confirm it in Alethiometer, Alethiometer marks it read in Slack. Writes back (through the
outbox) mark read, thread replies you type, and reactions.

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
    "max_threads": 300
  }
}
```

| Config | Default | Meaning |
|---|---|---|
| `channels` | `[]` | Channels you follow: `#name` (a channel you're in) or channel id (also works for a public channel you haven't joined). An unknown name fails the sync with a clear error. |
| `dms` | `true` | Read your DMs and group DMs. |
| `mentions` | `true` | Use `search.messages` (needs `search:read`) on catch-up: mentions of you in channels you don't follow, and threads you replied in that Alethiometer doesn't know yet. |
| `lookback_hours` | `72` | The furthest back anything is read: bounds the first sync, a catch-up after Alethiometer was off, and a channel you haven't read for weeks. |
| `max_dms` | `50` | DMs and group DMs checked per catch-up, most recently active first. Without the `xapp-` token that is every poll, and **DMs beyond it are not read** (except when a mention search finds them). |
| `thread_days` | `90` | A thread you're in is tracked this long after its last reply. |
| `max_threads` | `300` | At most this many tracked threads; the least recently active are dropped first. |
| `user_token_secret`, `app_token_secret` | `slack.user_token`, `slack.app_token` | Secret names. |

## What is read

**Signals follow Slack's read marker.** Every conversation and every thread has
its own "read up to here" marker in Slack (`last_read`). A signal holds the
messages after it:

- **Top level.** In a DM, group DM or followed channel, the unread top-level
  messages form one item (`DM · Priya`, `#inc-2291`, `Group DM · alice, bob`;
  `kind` `message` for one message, else `thread`). New messages from others
  update the same item and bring it back as new. If you read part of it in
  Slack it shrinks to what's still unread. Once you've read past all of it the
  item is marked seen, and the next message starts a new item. A run of
  messages is not assumed to be one topic: it's just "since you last looked".
- **Threads.** Each thread is one item for good (`#inc-2291 · thread: <first
  words of the parent>`), holding its unread replies (per the thread's own
  read marker, from the parent message in `conversations.replies`). New replies
  after you've read it bring the same item back as new.
- **Mentions** of you in channels you don't follow: one item each
  (`#random · mention`). A mention inside a thread goes to that thread's item.
- Your own messages are kept for context but never make an item new, and a run
  of only your messages isn't stored. Join/leave/topic messages are ignored.
  Text is converted from Slack markup to plain text with names resolved.
  Up to 50 messages are kept per item; `msg_count` counts them all.

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
  channels you haven't joined send no events and are read every poll. Every
  2 minutes the read markers of open items are checked (Slack sends no
  "you read this" event to apps), so reading in Slack clears signals here.
- **Catch-up** runs when the connection opens (at startup and after any
  disconnect): followed channels, up to `max_dms` DMs, open items, all tracked
  threads and the two searches. A DM costs one `conversations.info`; its
  history is read only when that reports something unread or new. Slack has no
  documented call that returns unread state for all conversations at once
  (the Slack client uses undocumented ones, which Alethiometer doesn't), so it's one
  small call per DM.
- **Without the `xapp-` token** every poll (`poll_secs`) is a catch-up, capped
  to the `max_dms` most recently active DMs (activity Alethiometer has seen, then the
  conversation's creation time; Slack lists no last-activity time for DMs).

**Item fields.** External ids: `<channel>/<first unread ts>` (top level),
`<channel>/thread/<thread ts>`, `<channel>/mention/<ts>`. `raw_json` holds
`channel`, `first_ts`, `last_ts`, `thread_ts` (threads), `mark_ts` (top-level
items in conversations you're in: what "mark read" sends), and `msgs`: each
stored message's ts, in the same order as the item's messages, so a later
"split by topic" can move some of them to another item.

The cursor stores only timestamps, ids and thread titles, never message text:
per-conversation newest ts, tracked threads, open items and the search
positions. Upgrading from the first version of this connector resets it; old
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
| `slack.mark_read` | `{channel, ts}`, from the item's `raw_json.channel` / `raw_json.mark_ts` | `conversations.mark`: the conversation is read up to `ts` in Slack too. Queued when you confirm a top-level item. When confirmed, the item is marked seen locally. |
| `slack.reply` | `{channel, thread_ts, text}` | `chat.postMessage` as you. `text` is exactly what you typed. |
| `slack.react` | `{channel, ts, name}` (`eyes` or `:eyes:`) | `reactions.add`. An existing identical reaction counts as done. |

**Threads can't be marked read from Alethiometer.** Slack's public Web API has no
method to set a thread's read state (`conversations.mark` covers only the
top level; the Slack client uses an undocumented internal method, which Alethiometer
deliberately doesn't call). Thread items therefore carry no `mark_ts` and
confirming one only marks it seen in Alethiometer; the thread stays unread in Slack
until you open it there. A `slack.mark_read` payload with a `thread_ts` fails
with "Slack has no API to mark a thread read". Mentions in channels you don't
follow aren't marked either: marking the channel up to the mention would also
mark everything before it.

Slack answers most errors with HTTP 200 and `ok: false`. Alethiometer treats those as
failures and shows Slack's error code (e.g. `channel_not_found`,
`missing_scope (needs scope im:write)`) on the outbox row, which you can retry
with ⌘R.
