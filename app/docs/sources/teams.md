# Microsoft Teams

Reads your Teams chats (1:1, group and meeting chats) and, when an admin has approved it, the channels you list. It works like Slack (`docs/chat.md`): every new message becomes part of a signal in Incoming, yours included; reading in Teams changes nothing here; only triage does. Writes back (through the outbox): mark a chat read when you confirm it, replies you type (`r`), and reactions (connector only, no UI yet).

Alethiometer talks to Teams as **you**, through Microsoft Graph (the only supported API for reading your own messages), with an app registration in your organisation's Microsoft Entra ID. Nothing is hosted.

**It polls.** Graph only pushes changes to a public HTTPS endpoint (or an Azure Event Hub), which a desktop app doesn't have; new messages arrive with the next poll (`poll_secs`, 120 is reasonable). Push through Event Hubs is a possible later stage (ISSUES.md).

**It comes in two stages**, so chats work even if an admin never approves the rest:

| Stage | Reads | Permissions (delegated) | Who can approve |
|---|---|---|---|
| Chats | your chats; names and emails of people in them | `User.Read`, `User.ReadBasic.All`, `Chat.ReadWrite`, `ChatMessage.Send` | usually you, at Connect (unless your organisation turned user consent off) |
| Channels | the channels in `channels` | adds `Team.ReadBasic.All`, `Channel.ReadBasic.All`, `ChannelMessage.Read.All`, `ChannelMessage.Send` | `ChannelMessage.Read.All` always needs an admin |

Connect asks only for what the source's settings need: with no `channels`, only the chat permissions. Adding a channel later means pressing Connect again; until an admin approves, the sign-in fails (Microsoft says "Need admin approval") and the chats keep working on the old sign-in. Take the channels out again to stop asking.

## 1. Register the app

In the Entra admin center (entra.microsoft.com) → **Applications → App registrations → New registration**:

1. Name: Alethiometer. Supported account types: **Accounts in this organizational directory only**.
2. Redirect URI: platform **Public client/native (mobile & desktop)**, URI `http://localhost`. Entra ignores the port for `localhost`, so Alethiometer can use any free one. No client secret: it's a public client signing in with PKCE.
3. **API permissions → Add a permission → Microsoft Graph → Delegated**: the chat stage's four, and the channel stage's four if you want channels. An admin can **Grant admin consent** here; otherwise you consent to the chat ones at Connect.
4. Copy the **Application (client) ID** and the **Directory (tenant) ID** from the Overview page.

Your organisation may not let you register apps; then an admin has to, with the same settings.

## 2. Add the source

Settings → Sources → Add source → Microsoft Teams, or in `settings.json` → `sources`:

```json
{
  "kind": "teams",
  "tag": "work",
  "name": "Teams",
  "poll_secs": 120,
  "config": {
    "client_id": "00000000-0000-0000-0000-000000000000",
    "tenant": "contoso.onmicrosoft.com",
    "channels": []
  }
}
```

Then **Connect** in the sync status popover: your browser opens Microsoft's sign-in, and Alethiometer stores the refresh token itself (secret `teams.refresh_token`). Microsoft hands out a new refresh token on every use and Alethiometer stores each one; an unused one expires after 90 days, and your organisation's sign-in policies can end it sooner. Then press Connect again.

| Config | Default | Meaning |
|---|---|---|
| `client_id` | | The app registration's Application (client) ID. |
| `tenant` | `organizations` | Directory (tenant) ID or domain. `organizations` lets any work account sign in. |
| `chats` | `true` | Read your chats. |
| `channels` | `[]` | Channels to read: paste each channel's link (… next to the channel → **Get link to channel**), or write `<team id>|<channel id>`. Needs the channel stage. |
| `muted` | `[]` | Chat or channel ids: still read and stored, never new in Incoming. Listed by name in Settings › Teams › Ignored (chats and channels apart, Add… picks from what's been seen); `.` then `i` on a Teams signal in Incoming adds its conversation. |
| `lookback_hours` | `72` | The furthest back anything is read: the first sync, and after Alethiometer was off. |
| `max_chats` | `50` | Chats checked per poll, most recently active first (Graph lists 50 per call). A chat beyond it is read again once it's among the most recent. |
| `thread_days` | `90` | A channel thread is tracked this long after its last reply. |
| `max_threads` | `300` | At most this many tracked threads; the least recently active are dropped first. |
| `secret_prefix` | `teams` | Secret name prefix, for a second tenant. |

## What is read

- **Chats.** Each poll lists your most recently active chats with their last message (one call), and reads the messages of those with something new since the last poll (by last-modified time, so edits to an open signal show too). A chat's messages since its last triaged run are one item (`<chat id>/<first message id>`; `kind` `message` for one message, else `thread`), titled `Chat · Priya Shah`, `Group chat · Launch prep` (its topic, else its members) or `Meeting chat · …`. Three earlier messages come along as context. Teams chats have no threads.
- **Channels.** A listed channel's new posts are one item per run (`<team id>|<channel id>/<first post id>`, titled `#incidents · Platform`), like a Slack channel. A post with replies is a thread: its replies attach to the post when an item holds it (they show under the post, and new ones bring that item back), else they get a thread item `<team id>|<channel id>/thread/<post id>` with the post as context. Threads of posts that changed are read on the poll; tracked threads also every poll while active in the last day, hourly after that.
- **Text.** Teams messages are HTML; they're shown as Markdown (htmd): links, bold, lists and code stay, a mention is the person's name, an emoji its character, a file `[file: name]`. System events (someone joined, a call started) and deleted messages are left out.
- **People.** Authors are accounts of kind `teams` (their Entra object id); their email comes from their profile (`User.ReadBasic.All`), so People notes match them by email. A People note can also list `teams: <object id>`. Guests and people from other organisations get their name only. Your own id comes from the first sync.

Item fields follow `docs/chat.md`: `raw_json` has `channel` (the chat id, or `<team id>|<channel id>`), `conv` (`dm`, `group` or `channel`), `msgs`, `thread_ts` (a thread's post id) and `mark_ts` (chats only). Message ids are Teams' creation times in milliseconds.

The cursor stores ids, times and titles, never message text.

## What is written (outbox)

| Op | Payload | Graph call |
|---|---|---|
| `teams.mark_read` | `{channel, ts}`, from a chat item's `raw_json.mark_ts` | `markChatReadForUser`. Graph marks the whole chat read, so it's sent only when `ts` is still the chat's newest message; otherwise it confirms without marking (receipt `not_marked`). It also confirms without marking when Teams is already read that far (`already`). Channels have no read marker in Graph. |
| `teams.reply` | `{channel, thread_ts?, text}` | A chat message, a channel post, or a reply to the post `thread_ts`. `text` is sent as HTML, escaped, with its line breaks. Chats have no threads, so ⌥T there fails with a message. Not resent on an unknown outcome. |
| `teams.react` | `{channel, ts, name, thread_ts?}` | `setReaction` with `name` as the emoji itself ("👍"). |

Graph's 429 and 503 wait for `Retry-After` (up to 2 minutes, 3 tries), then the sync fails and the next one retries. A 403 on one chat or channel (no permission, no admin approval) is recorded for that stream and the rest of the poll goes on.

## Not built

- Push (Event Hubs), and searching for mentions of you in channels you don't list.
- A channel picker like Slack's Review…: channels are pasted as links.
- Not run against a real tenant yet: built from Graph's v1.0 documentation and recorded shapes (`tests/fixtures/teams/`).
