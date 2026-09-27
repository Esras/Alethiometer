# Slack

Reads your DMs, group DMs, the channels you follow and mentions of you. Each
burst of conversation becomes **one** signal in Incoming, e.g.
`#inc-2291 · 42 new · 09:05–10:40`, which expands to its messages. Writes back
(through the outbox) mark read, thread replies you type, and reactions.

Helm talks to Slack as **you**, through a small Slack app you create for
yourself. Nothing is hosted; the app only exists to issue tokens.

## 1. Create the app

1. Go to <https://api.slack.com/apps> → **Create New App** → **From a manifest**, pick your workspace.
2. Paste this manifest (YAML):

```yaml
_metadata:
  major_version: 1
display_information:
  name: Helm
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
`xapp-` token Helm just polls.

## 2. Store the tokens

Secrets never go in `settings.json`. On Linux they live in
`<data dir>/secrets.json` (mode 0600); on the Mac they will move to the Keychain.

| Secret name | Value |
|---|---|
| `slack.user_token` | `xoxp-…` User OAuth Token (required) |
| `slack.app_token` | `xapp-…` app-level token with `connections:write` (optional; enables live updates) |

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
    "gap_min": 120,
    "lookback_hours": 24,
    "thread_hours": 24
  }
}
```

| Config | Default | Meaning |
|---|---|---|
| `channels` | `[]` | Channels you follow: `#name` (a channel you're in) or channel id (also works for a public channel you haven't joined). An unknown name fails the sync with a clear error. |
| `dms` | `true` | Read all your DMs and group DMs. |
| `mentions` | `true` | Mentions of you in other channels (one signal per mention), via search. |
| `gap_min` | `120` | Minutes of quiet that end a window. The next message starts a new signal. |
| `lookback_hours` | `24` | How far back the first sync reads. Also the most a catch-up reads after Helm was off. |
| `thread_hours` | `24` | How long a thread keeps being checked for replies after its last activity. |
| `user_token_secret`, `app_token_secret` | `slack.user_token`, `slack.app_token` | Secret names. |

## What is read

- **Windows.** In each followed channel, DM and group DM, messages with no gap
  longer than `gap_min` form a window, stored as one item (`kind` `thread`, or
  `message` when it holds a single message): title `#channel`, `DM · Name` or
  `Group DM · alice, bob`; `occurred_at`/`ended_at` = first/last message;
  `msg_count`; summary = the last message; up to 50 messages kept for the
  expanded view. When new messages continue a window, the same item is updated
  and comes back as new in Incoming. Your own messages are kept for context but
  never make a window new, and a window of only your messages isn't stored.
- **Threads.** Replies form their own windows, titled
  `#channel · thread: <first words of the parent>`. Threads are found through
  parents in the last `thread_hours` and then watched until they go quiet for
  `thread_hours`. A reply to an older parent that Helm never saw is missed.
- **Mentions.** `search.messages` for `<@you>` since the last sync, skipping
  conversations already covered above; each one is an item `#channel · mention`.
- Join/leave/topic messages are ignored. Message text is converted from Slack
  markup to plain text with names resolved.
- The cursor stores only per-conversation timestamps, never message text.

Item external ids: `<channel>/<window start ts>`, `<channel>/<thread ts>/<window start ts>`,
`<channel>/mention/<ts>`. `raw_json` holds `channel`, `thread_ts` (threads),
`first_ts`, `last_ts` and, for channel/DM windows, `mark_ts`.

**Live updates.** With the `xapp-` token, Helm keeps a Socket Mode connection
open. Each event is acknowledged; a DM, a message in a followed channel or a
mention triggers a sync a few seconds later (bursts are coalesced). The sync is
the same poll as above, so polling on `poll_secs` stays as the fallback.

**Rate limits.** A 429 waits for `Retry-After` (up to 2 minutes, 3 tries), then
the sync fails and the next one retries. Slack's 2025 limits on
`conversations.history`/`replies` for non-Marketplace apps (1 request/minute)
do not apply to internal, customer-built apps like this one. Every DM costs one
`conversations.history` call per sync, so with hundreds of DMs keep `poll_secs`
at 300 or more.

## What is written (outbox)

| Op | Payload | Slack method |
|---|---|---|
| `slack.mark_read` | `{channel, ts}`, from the item's `raw_json.channel` / `raw_json.mark_ts` | `conversations.mark`. Marks the conversation read up to `ts`. Slack has no API for a thread's read state, so thread windows have no `mark_ts`. When confirmed, the item is marked seen locally. |
| `slack.reply` | `{channel, thread_ts, text}` | `chat.postMessage` as you. `text` is exactly what you typed. |
| `slack.react` | `{channel, ts, name}` (`eyes` or `:eyes:`) | `reactions.add`. An existing identical reaction counts as done. |

Slack answers most errors with HTTP 200 and `ok: false`. Helm treats those as
failures and shows Slack's error code (e.g. `channel_not_found`,
`missing_scope (needs scope im:write)`) on the outbox row, which you can retry
with ⌘R.
